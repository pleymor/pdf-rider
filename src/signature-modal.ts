import { SignatureStore } from "./signature-store";

type SignatureReadyHandler = (imageData: string) => void;

// ── Smoothing algorithms (ported from full-rust branch) ──────────────────────

/** Douglas-Peucker polyline simplification. */
function douglasPeucker(points: [number, number][], epsilon: number): [number, number][] {
  const n = points.length;
  if (n <= 2) return points.slice();

  const [ax, ay] = points[0];
  const [bx, by] = points[n - 1];
  const lineLen = Math.max(Math.hypot(bx - ax, by - ay), 0.001);

  let maxDist = 0;
  let maxIdx = 0;
  for (let i = 1; i < n - 1; i++) {
    const [px, py] = points[i];
    const dist = Math.abs((by - ay) * px - (bx - ax) * py + bx * ay - by * ax) / lineLen;
    if (dist > maxDist) { maxDist = dist; maxIdx = i; }
  }

  if (maxDist > epsilon) {
    const left = douglasPeucker(points.slice(0, maxIdx + 1), epsilon);
    const right = douglasPeucker(points.slice(maxIdx), epsilon);
    left.pop();
    return left.concat(right);
  }
  return [points[0], points[n - 1]];
}

/** Smooth point positions with moving average, keeping endpoints fixed. */
function smoothPoints(points: [number, number][], window: number): [number, number][] {
  const n = points.length;
  if (n <= 2) return points.slice();
  const half = Math.floor(window / 2);
  const result: [number, number][] = [points[0]];
  for (let i = 1; i < n - 1; i++) {
    const start = Math.max(0, i - half);
    const end = Math.min(n, i + half + 1);
    let sx = 0, sy = 0;
    for (let j = start; j < end; j++) { sx += points[j][0]; sy += points[j][1]; }
    const count = end - start;
    result.push([sx / count, sy / count]);
  }
  result.push(points[n - 1]);
  return result;
}

/** Smooth a float array with moving average, keeping endpoints fixed. */
function smoothValues(values: number[], window: number): number[] {
  const n = values.length;
  if (n <= 2) return values.slice();
  const half = Math.floor(window / 2);
  const result = [values[0]];
  for (let i = 1; i < n - 1; i++) {
    const start = Math.max(0, i - half);
    const end = Math.min(n, i + half + 1);
    let sum = 0;
    for (let j = start; j < end; j++) sum += values[j];
    result.push(sum / (end - start));
  }
  result.push(values[n - 1]);
  return result;
}

/** Render strokes with smooth Catmull-Rom splines on an arbitrary canvas context. */
function renderSmoothStrokes(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  strokes: [number, number][][],
): void {
  ctx.clearRect(0, 0, width, height);

  for (const rawPoints of strokes) {
    if (rawPoints.length < 2) continue;

    let pts = douglasPeucker(rawPoints, 1.0);
    if (pts.length < 2) continue;

    for (let p = 0; p < 2; p++) pts = smoothPoints(pts, 3);
    if (pts.length < 2) continue;

    const n = pts.length;

    const rawWidths: number[] = [];
    for (let i = 0; i < n; i++) {
      const [dx, dy] = i === 0
        ? [pts[1][0] - pts[0][0], pts[1][1] - pts[0][1]]
        : i === n - 1
          ? [pts[n - 1][0] - pts[n - 2][0], pts[n - 1][1] - pts[n - 2][1]]
          : [pts[i + 1][0] - pts[i - 1][0], pts[i + 1][1] - pts[i - 1][1]];
      const len = Math.max(Math.hypot(dx, dy), 0.001);
      const verticality = Math.abs(dy / len);
      const minW = 0.8;
      const maxW = 4.0;
      rawWidths.push(minW + verticality * (maxW - minW));
    }

    let widths = rawWidths;
    for (let p = 0; p < 4; p++) widths = smoothValues(widths, 7);

    ctx.strokeStyle = "#0f0f23";
    ctx.lineCap = "round";
    ctx.lineJoin = "round";

    for (let i = 0; i < n - 1; i++) {
      const p0 = i > 0 ? pts[i - 1] : pts[i];
      const p1 = pts[i];
      const p2 = pts[i + 1];
      const p3 = i + 2 < n ? pts[i + 2] : pts[i + 1];

      const cp1x = p1[0] + (p2[0] - p0[0]) / 6;
      const cp1y = p1[1] + (p2[1] - p0[1]) / 6;
      const cp2x = p2[0] - (p3[0] - p1[0]) / 6;
      const cp2y = p2[1] - (p3[1] - p1[1]) / 6;

      ctx.lineWidth = (widths[i] + widths[i + 1]) * 0.5;
      ctx.beginPath();
      ctx.moveTo(p1[0], p1[1]);
      ctx.bezierCurveTo(cp1x, cp1y, cp2x, cp2y, p2[0], p2[1]);
      ctx.stroke();
    }
  }
}

// ── SignatureModal ────────────────────────────────────────────────────────────

export class SignatureModal {
  private backdrop: HTMLElement;
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;

  private isDrawing = false;
  private hasContent = false;

  /** All strokes collected so far — each stroke is a list of [x,y] pairs. */
  private strokes: [number, number][][] = [];

  private store: SignatureStore;
  private savedSectionEl: HTMLElement;
  private savedListEl: HTMLElement;
  private selectedSavedId: string | null = null;

  private handlers: SignatureReadyHandler[] = [];

  constructor(store: SignatureStore) {
    this.store = store;
    this.backdrop = document.getElementById("signature-modal")!;
    this.canvas = document.getElementById("signature-canvas") as HTMLCanvasElement;
    this.ctx = this.canvas.getContext("2d")!;
    this.savedSectionEl = document.getElementById("sig-saved-section")!;
    this.savedListEl = document.getElementById("sig-saved-list")!;

    this.migrateOldStrokes();
    this.initDrawing();
    this.bindButtons();
  }

  /** Migrate old localStorage strokes key into the new store. */
  private migrateOldStrokes(): void {
    const oldStrokes = this.store.consumeOldStrokes();
    if (!oldStrokes || oldStrokes.length === 0) return;

    // Render strokes on an offscreen canvas to produce a base64 image
    const offscreen = document.createElement("canvas");
    offscreen.width = this.canvas.width;
    offscreen.height = this.canvas.height;
    const offCtx = offscreen.getContext("2d")!;
    renderSmoothStrokes(offCtx, offscreen.width, offscreen.height, oldStrokes);
    const dataUrl = offscreen.toDataURL("image/png");
    this.store.add(dataUrl);
  }

  onSignatureReady(handler: SignatureReadyHandler): void {
    this.handlers.push(handler);
  }

  private emit(imageData: string): void {
    this.handlers.forEach((h) => h(imageData));
  }

  open(): void {
    this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    this.strokes = [];
    this.hasContent = false;
    this.selectedSavedId = null;
    this.renderSavedList();
    this.backdrop.classList.remove("hidden");
    this.canvas.focus();
  }

  close(): void {
    this.backdrop.classList.add("hidden");
  }

  private clearCanvas(): void {
    this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    this.strokes = [];
    this.hasContent = false;
    this.selectedSavedId = null;
    this.updateSelectedCard();
  }

  // ── Saved signatures list ───────────────────────────────────────────────

  private renderSavedList(): void {
    const sigs = this.store.getAll();
    this.savedListEl.innerHTML = "";

    if (sigs.length === 0) {
      this.savedSectionEl.classList.add("hidden");
      return;
    }

    this.savedSectionEl.classList.remove("hidden");

    for (const sig of sigs) {
      const card = document.createElement("div");
      card.className = "sig-saved-card";
      card.dataset.sigId = sig.id;

      const thumb = document.createElement("img");
      thumb.className = "sig-saved-thumb";
      thumb.src = sig.imageDataUrl;
      thumb.draggable = false;
      card.appendChild(thumb);

      const delBtn = document.createElement("button");
      delBtn.className = "sig-saved-delete";
      delBtn.title = "Delete";
      delBtn.dataset.i18nTitle = "sigDelete";
      delBtn.textContent = "\u2715";
      card.appendChild(delBtn);

      card.addEventListener("click", () => this.onSavedCardClick(sig.id, sig.imageDataUrl));
      delBtn.addEventListener("click", (e) => this.onDeleteSavedClick(sig.id, e));

      this.savedListEl.appendChild(card);
    }

    this.updateSelectedCard();
  }

  private onSavedCardClick(id: string, imageDataUrl: string): void {
    this.selectedSavedId = id;
    this.strokes = [];
    this.updateSelectedCard();

    // Draw the saved image on the canvas
    const img = new Image();
    img.onload = () => {
      this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
      const scaleX = this.canvas.width / img.width;
      const scaleY = this.canvas.height / img.height;
      const s = Math.min(scaleX, scaleY, 1);
      const dx = (this.canvas.width - img.width * s) / 2;
      const dy = (this.canvas.height - img.height * s) / 2;
      this.ctx.drawImage(img, dx, dy, img.width * s, img.height * s);
      this.hasContent = true;
    };
    img.src = imageDataUrl;
  }

  private onDeleteSavedClick(id: string, e: MouseEvent): void {
    e.stopPropagation();
    this.store.delete(id);
    if (this.selectedSavedId === id) {
      this.clearCanvas();
    }
    this.renderSavedList();
  }

  private updateSelectedCard(): void {
    for (const card of this.savedListEl.children) {
      const el = card as HTMLElement;
      el.classList.toggle("selected", el.dataset.sigId === this.selectedSavedId);
    }
  }

  // ── Drawing ──────────────────────────────────────────────────────────────

  private initDrawing(): void {
    this.canvas.style.touchAction = "none";

    this.canvas.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      // If a saved signature was selected, starting to draw means a new signature
      if (this.selectedSavedId) {
        this.selectedSavedId = null;
        this.updateSelectedCard();
      }
      this.isDrawing = true;
      this.canvas.setPointerCapture(e.pointerId);
      this.strokes.push([[e.offsetX, e.offsetY]]);

      // Start rough preview path
      this.ctx.beginPath();
      this.ctx.moveTo(e.offsetX, e.offsetY);
      this.ctx.strokeStyle = "#000";
      this.ctx.lineWidth = 2;
      this.ctx.lineCap = "round";
      this.ctx.lineJoin = "round";
    });

    this.canvas.addEventListener("pointermove", (e) => {
      if (!this.isDrawing) return;
      e.preventDefault();
      const current = this.strokes[this.strokes.length - 1];
      current.push([e.offsetX, e.offsetY]);

      // Rough preview
      this.ctx.lineTo(e.offsetX, e.offsetY);
      this.ctx.stroke();
      this.ctx.beginPath();
      this.ctx.moveTo(e.offsetX, e.offsetY);
      this.hasContent = true;
    });

    const stopDrawing = (): void => {
      if (!this.isDrawing) return;
      this.isDrawing = false;
      // Re-render all strokes with smooth curves
      renderSmoothStrokes(this.ctx, this.canvas.width, this.canvas.height, this.strokes);
    };
    this.canvas.addEventListener("pointerup", stopDrawing);
    this.canvas.addEventListener("pointercancel", stopDrawing);
  }

  // ── Buttons ──────────────────────────────────────────────────────────────

  private bindButtons(): void {
    document
      .getElementById("sig-close-btn")!
      .addEventListener("click", () => this.close());

    document.getElementById("sig-clear-btn")!.addEventListener("click", () => {
      this.clearCanvas();
    });

    document.getElementById("sig-place-btn")!.addEventListener("click", () => {
      if (!this.hasContent) return;

      const dataUrl = this.canvas.toDataURL("image/png");
      const b64 = dataUrl.split(",")[1]; // strip data:image/png;base64,

      if (this.selectedSavedId) {
        // Re-using an existing saved signature — update its last-used time
        this.store.touch(this.selectedSavedId);
      } else {
        // New signature (drawn or image) — save it
        this.store.add(dataUrl);
      }

      this.close();
      this.emit(b64);
    });

    document
      .getElementById("sig-image-btn")!
      .addEventListener("click", () => this.loadImageFile());
  }

  private loadImageFile(): void {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/png,image/jpeg";
    input.style.display = "none";

    input.addEventListener("change", () => {
      const file = input.files?.[0];
      if (!file) return;

      const reader = new FileReader();
      reader.onload = (e) => {
        const dataUrl = e.target?.result as string;
        if (!dataUrl) return;

        const img = new Image();
        img.onload = () => {
          this.clearCanvas();
          // Scale to fit the canvas while preserving aspect ratio
          const scaleX = this.canvas.width / img.width;
          const scaleY = this.canvas.height / img.height;
          const s = Math.min(scaleX, scaleY, 1);
          const dx = (this.canvas.width - img.width * s) / 2;
          const dy = (this.canvas.height - img.height * s) / 2;
          this.ctx.drawImage(img, dx, dy, img.width * s, img.height * s);
          this.hasContent = true;
        };
        img.src = dataUrl;
      };
      reader.readAsDataURL(file);
      input.remove();
    });

    document.body.appendChild(input);
    input.click();
  }
}
