import {
  canvasToPdf,
  pdfToCanvas,
  rgbToCss,
  type ActiveToolState,
  type Annotation,
  type CircleAnnotation,
  type RectAnnotation,
  type SignatureAnnotation,
  type TextAnnotation,
  type ToolKind,
} from "./models";

type AnnotationCreatedHandler = (ann: Annotation) => void;
type AnnotationMovedHandler   = (ann: Annotation) => void;
type AnnotationRemovedHandler = (ann: Annotation) => void;

export class CanvasOverlay {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;

  private style: ActiveToolState;
  private activeTool: ToolKind = "select";

  // ── Draw state ──────────────────────────────────────────────────────────────
  private isDrawing = false;
  private startX = 0;
  private startY = 0;

  // ── Select / move state ─────────────────────────────────────────────────────
  private selected: Annotation | null = null;
  private dragging = false;
  private dragTarget: Annotation | null = null;
  private dragOrigX = 0;
  private dragOrigY = 0;
  private dragOffsetX = 0; // canvas px from click to annotation top-left
  private dragOffsetY = 0;

  // ── Page state ──────────────────────────────────────────────────────────────
  private currentPage = 1;
  private pageHeightPt = 841;
  private scale = 1.5;

  // ── Callbacks ───────────────────────────────────────────────────────────────
  private createdHandlers: AnnotationCreatedHandler[] = [];
  private movedHandlers:   AnnotationMovedHandler[]   = [];
  private removedHandlers: AnnotationRemovedHandler[] = [];

  /** Annotations already placed on the current page (for redraw). */
  private committed: Annotation[] = [];

  constructor(initialStyle: ActiveToolState) {
    this.style = { ...initialStyle };
    this.canvas = document.getElementById(
      "annotation-canvas"
    ) as HTMLCanvasElement;
    this.ctx = this.canvas.getContext("2d")!;

    this.canvas.addEventListener("mousedown",  this.onMouseDown);
    this.canvas.addEventListener("mousemove",  this.onMouseMove);
    this.canvas.addEventListener("mouseup",    this.onMouseUp);
    this.canvas.addEventListener("mouseleave", this.onMouseLeave);
    window.addEventListener("keydown", this.onKeyDown);
  }

  onAnnotationCreated(handler: AnnotationCreatedHandler): void {
    this.createdHandlers.push(handler);
  }

  onAnnotationMoved(handler: AnnotationMovedHandler): void {
    this.movedHandlers.push(handler);
  }

  onAnnotationRemoved(handler: AnnotationRemovedHandler): void {
    this.removedHandlers.push(handler);
  }

  private emit(ann: Annotation): void {
    this.createdHandlers.forEach(h => h(ann));
  }

  private emitMoved(ann: Annotation): void {
    this.movedHandlers.forEach(h => h(ann));
  }

  private emitRemoved(ann: Annotation): void {
    this.removedHandlers.forEach(h => h(ann));
  }

  setStyle(style: ActiveToolState): void {
    this.style = { ...style };
    this.activeTool = style.tool;
    this.applyPointerEvents(style.tool);
  }

  setTool(tool: ToolKind): void {
    if (tool !== "select") {
      this.selected = null;
      this.dragging = false;
      this.dragTarget = null;
      this.redrawCommitted();
    }
    this.activeTool = tool;
    this.applyPointerEvents(tool);

    if (tool === "text")    this.canvas.style.cursor = "text";
    else if (tool === "select")    this.canvas.style.cursor = "default";
    else if (tool !== "signature") this.canvas.style.cursor = "crosshair";
    else                           this.canvas.style.cursor = "";
  }

  private applyPointerEvents(tool: ToolKind): void {
    // Signature uses viewer-container click handler (canvas must be transparent)
    this.canvas.style.pointerEvents = tool !== "signature" ? "auto" : "none";
  }

  /** Call after each page render to sync canvas size and stored annotations. */
  syncToPage(
    page: number,
    scale: number,
    pageHeightPt: number,
    annotations: Annotation[]
  ): void {
    this.currentPage = page;
    this.scale = scale;
    this.pageHeightPt = pageHeightPt;

    // Deselect when changing pages
    this.selected = null;
    this.dragging = false;
    this.dragTarget = null;

    const pdfCanvas = document.getElementById("pdf-canvas") as HTMLCanvasElement;
    this.canvas.width  = pdfCanvas.width;
    this.canvas.height = pdfCanvas.height;
    this.canvas.style.width  = pdfCanvas.style.width;
    this.canvas.style.height = pdfCanvas.style.height;

    this.committed = [...annotations];
    this.redrawCommitted();
  }

  // ── Bounding box helper ──────────────────────────────────────────────────────

  private getAnnBounds(ann: Annotation): { left: number; top: number; right: number; bottom: number } {
    const { scale, pageHeightPt } = this;
    const left   = ann.x * scale;
    const right  = (ann.x + ann.width) * scale;
    const bottom = (pageHeightPt - ann.y) * scale;

    if (ann.kind === "text") {
      const top = bottom - ann.fontSize * scale * 1.5;
      return { left, top, right, bottom };
    }

    const top = (pageHeightPt - (ann.y + ann.height)) * scale;
    return { left, top, right, bottom };
  }

  private hitTest(cx: number, cy: number): Annotation | null {
    const PAD = 5; // extra px to make small annotations easier to grab
    for (let i = this.committed.length - 1; i >= 0; i--) {
      const b = this.getAnnBounds(this.committed[i]);
      if (cx >= b.left - PAD && cx <= b.right  + PAD &&
          cy >= b.top  - PAD && cy <= b.bottom + PAD) {
        return this.committed[i];
      }
    }
    return null;
  }

  // ── Redraw ───────────────────────────────────────────────────────────────────

  redrawCommitted(): void {
    const { width, height } = this.canvas;
    this.ctx.clearRect(0, 0, width, height);
    for (const ann of this.committed) {
      this.drawAnnotation(ann);
    }
    if (this.selected && this.committed.includes(this.selected)) {
      this.drawSelectionBox(this.selected);
    }
  }

  private drawSelectionBox(ann: Annotation): void {
    const b = this.getAnnBounds(ann);
    const pad = 5;
    this.ctx.strokeStyle = "#4d9eff";
    this.ctx.lineWidth = 1.5;
    this.ctx.setLineDash([5, 3]);
    this.ctx.strokeRect(
      b.left  - pad, b.top  - pad,
      b.right - b.left + pad * 2,
      b.bottom - b.top + pad * 2
    );
    this.ctx.setLineDash([]);
  }

  // ── Annotation preview drawing ───────────────────────────────────────────────

  private drawAnnotation(ann: Annotation): void {
    const ctx = this.ctx;
    const { scale, pageHeightPt } = this;

    if (ann.kind === "rect") {
      const { x, y } = pdfToCanvas(ann.x, ann.y, scale, pageHeightPt);
      ctx.strokeStyle = rgbToCss(ann.color);
      ctx.lineWidth = ann.strokeWidth;
      ctx.strokeRect(x, y - ann.height * scale, ann.width * scale, ann.height * scale);

    } else if (ann.kind === "circle") {
      const cx = pdfToCanvas(ann.x + ann.width / 2, ann.y + ann.height / 2, scale, pageHeightPt);
      ctx.strokeStyle = rgbToCss(ann.color);
      ctx.lineWidth = ann.strokeWidth;
      ctx.beginPath();
      ctx.ellipse(cx.x, cx.y, (ann.width / 2) * scale, (ann.height / 2) * scale, 0, 0, Math.PI * 2);
      ctx.stroke();

    } else if (ann.kind === "text") {
      const { x, y } = pdfToCanvas(ann.x, ann.y, scale, pageHeightPt);
      ctx.fillStyle = rgbToCss(ann.color);
      const weight = ann.bold   ? "bold"   : "normal";
      const style  = ann.italic ? "italic" : "normal";
      ctx.font = `${style} ${weight} ${ann.fontSize * scale}px Helvetica, Arial, sans-serif`;
      ctx.textAlign = ann.alignment as CanvasTextAlign;
      const tx =
        ann.alignment === "left"  ? x :
        ann.alignment === "right" ? x + ann.width * scale :
                                    x + (ann.width * scale) / 2;
      ctx.fillText(ann.content, tx, y);
      if (ann.underline) {
        const w = ctx.measureText(ann.content).width;
        const ulY = y + ann.fontSize * scale * 0.12;
        ctx.strokeStyle = rgbToCss(ann.color);
        ctx.lineWidth = 1;
        const startX =
          ann.alignment === "left"  ? x :
          ann.alignment === "right" ? tx - w :
                                      tx - w / 2;
        ctx.beginPath();
        ctx.moveTo(startX, ulY);
        ctx.lineTo(startX + w, ulY);
        ctx.stroke();
      }

    } else if (ann.kind === "signature") {
      const { x, y } = pdfToCanvas(ann.x, ann.y, scale, pageHeightPt);
      const img = new Image();
      img.onload = () => {
        this.ctx.drawImage(img, x, y - ann.height * scale, ann.width * scale, ann.height * scale);
      };
      img.src = `data:image/png;base64,${ann.imageData}`;
    }
  }

  private drawLivePreview(curX: number, curY: number): void {
    this.redrawCommitted();
    const ctx = this.ctx;
    const x0 = Math.min(this.startX, curX);
    const y0 = Math.min(this.startY, curY);
    const w  = Math.abs(curX - this.startX);
    const h  = Math.abs(curY - this.startY);

    ctx.strokeStyle = rgbToCss(this.style.color);
    ctx.lineWidth = this.style.strokeWidth;

    if (this.activeTool === "rect") {
      ctx.setLineDash([4, 3]);
      ctx.strokeRect(x0, y0, w, h);
      ctx.setLineDash([]);
    } else if (this.activeTool === "circle") {
      ctx.setLineDash([4, 3]);
      ctx.beginPath();
      ctx.ellipse(x0 + w / 2, y0 + h / 2, w / 2, h / 2, 0, 0, Math.PI * 2);
      ctx.stroke();
      ctx.setLineDash([]);
    }
  }

  // ── Mouse handlers ────────────────────────────────────────────────────────────

  private onMouseDown = (e: MouseEvent): void => {
    if (this.activeTool === "signature") return;

    if (this.activeTool === "select") {
      const hit = this.hitTest(e.offsetX, e.offsetY);
      this.selected = hit;
      if (hit) {
        const bounds = this.getAnnBounds(hit);
        this.dragging     = true;
        this.dragTarget   = hit;
        this.dragOrigX    = hit.x;
        this.dragOrigY    = hit.y;
        this.dragOffsetX  = e.offsetX - bounds.left;
        this.dragOffsetY  = e.offsetY - bounds.top;
        this.canvas.style.cursor = "grabbing";
      }
      this.redrawCommitted();
      return;
    }

    if (this.activeTool === "text") {
      this.handleTextClick(e.offsetX, e.offsetY);
      return;
    }

    this.isDrawing = true;
    this.startX = e.offsetX;
    this.startY = e.offsetY;
  };

  private onMouseMove = (e: MouseEvent): void => {
    if (this.activeTool === "select") {
      if (this.dragging && this.dragTarget) {
        const newLeft = e.offsetX - this.dragOffsetX;
        const newTop  = e.offsetY - this.dragOffsetY;
        this.dragTarget.x = newLeft / this.scale;
        if (this.dragTarget.kind === "text") {
          this.dragTarget.y = this.pageHeightPt
            - (newTop / this.scale)
            - this.dragTarget.fontSize * 1.5;
        } else {
          this.dragTarget.y = this.pageHeightPt
            - newTop / this.scale
            - this.dragTarget.height;
        }
        this.redrawCommitted();
      } else {
        const hit = this.hitTest(e.offsetX, e.offsetY);
        this.canvas.style.cursor = hit ? "grab" : "default";
      }
      return;
    }

    if (!this.isDrawing) return;
    this.drawLivePreview(e.offsetX, e.offsetY);
  };

  private onMouseUp = (e: MouseEvent): void => {
    if (this.activeTool === "select") {
      if (this.dragging && this.dragTarget) {
        const moved = this.dragTarget.x !== this.dragOrigX ||
                      this.dragTarget.y !== this.dragOrigY;
        if (moved) this.emitMoved(this.dragTarget);
        this.canvas.style.cursor = "grab";
      }
      this.dragging   = false;
      this.dragTarget = null;
      return;
    }

    if (!this.isDrawing) return;
    this.isDrawing = false;

    const x0 = Math.min(this.startX, e.offsetX);
    const y0 = Math.min(this.startY, e.offsetY);
    const w  = Math.abs(e.offsetX - this.startX);
    const h  = Math.abs(e.offsetY - this.startY);

    if (w < 4 || h < 4) {
      this.redrawCommitted();
      return;
    }

    const pdfTL = canvasToPdf(x0,     y0,     this.scale, this.pageHeightPt);
    const pdfBR = canvasToPdf(x0 + w, y0 + h, this.scale, this.pageHeightPt);
    const pdfX  = pdfTL.x;
    const pdfY  = pdfBR.y;
    const pdfW  = pdfBR.x - pdfTL.x;
    const pdfH  = pdfTL.y - pdfBR.y;

    if (this.activeTool === "rect") {
      const ann: RectAnnotation = {
        kind: "rect",
        page: this.currentPage,
        x: pdfX, y: pdfY,
        width: pdfW, height: pdfH,
        color: { ...this.style.color },
        strokeWidth: this.style.strokeWidth,
      };
      this.committed.push(ann);
      this.redrawCommitted();
      this.emit(ann);

    } else if (this.activeTool === "circle") {
      const ann: CircleAnnotation = {
        kind: "circle",
        page: this.currentPage,
        x: pdfX, y: pdfY,
        width: pdfW, height: pdfH,
        color: { ...this.style.color },
        strokeWidth: this.style.strokeWidth,
      };
      this.committed.push(ann);
      this.redrawCommitted();
      this.emit(ann);
    }
  };

  private onMouseLeave = (): void => {
    if (this.dragging && this.dragTarget) {
      // Restore original position on cancel
      this.dragTarget.x = this.dragOrigX;
      this.dragTarget.y = this.dragOrigY;
      this.dragging   = false;
      this.dragTarget = null;
      this.redrawCommitted();
      return;
    }
    if (this.isDrawing) {
      this.isDrawing = false;
      this.redrawCommitted();
    }
  };

  private onKeyDown = (e: KeyboardEvent): void => {
    if ((e.key === "Delete" || e.key === "Backspace") &&
        this.selected && this.activeTool === "select") {
      const idx = this.committed.indexOf(this.selected);
      if (idx !== -1) this.committed.splice(idx, 1);
      const removed = this.selected;
      this.selected = null;
      this.redrawCommitted();
      this.emitRemoved(removed);
    }
  };

  // ── Text tool ─────────────────────────────────────────────────────────────────

  private handleTextClick(canvasX: number, canvasY: number): void {
    const container = document.getElementById("viewer-container")!;

    const input = document.createElement("div");
    input.contentEditable = "true";
    input.style.cssText = `
      position: absolute;
      left: ${canvasX}px;
      top: ${canvasY - this.style.fontSize * this.scale}px;
      min-width: 80px;
      max-width: ${this.canvas.width - canvasX}px;
      font-size: ${this.style.fontSize * this.scale}px;
      font-family: Helvetica, Arial, sans-serif;
      font-weight: ${this.style.bold ? "bold" : "normal"};
      font-style: ${this.style.italic ? "italic" : "normal"};
      text-decoration: ${this.style.underline ? "underline" : "none"};
      color: ${rgbToCss(this.style.color)};
      outline: 1px dashed #aaa;
      background: transparent;
      padding: 1px 2px;
      white-space: pre;
      z-index: 10;
    `;

    const commit = (): void => {
      const content = input.textContent?.trim() ?? "";
      if (content) {
        const approxWidth = (content.length * this.style.fontSize * 0.55) / this.scale;
        const pdfCoords = canvasToPdf(canvasX, canvasY, this.scale, this.pageHeightPt);
        const ann: TextAnnotation = {
          kind: "text",
          page: this.currentPage,
          x: pdfCoords.x,
          y: pdfCoords.y,
          width: approxWidth + 10,
          content,
          color: { ...this.style.color },
          fontSize: this.style.fontSize,
          bold: this.style.bold,
          italic: this.style.italic,
          underline: this.style.underline,
          alignment: this.style.alignment,
        };
        this.committed.push(ann);
        this.redrawCommitted();
        this.emit(ann);
      }
      input.remove();
    };

    input.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        commit();
      } else if (e.key === "Escape") {
        input.remove();
      }
    });
    input.addEventListener("blur", commit, { once: true });

    container.appendChild(input);
    input.focus();
  }

  /** Place a signature image on the current page at canvas coordinates. */
  placeSignature(
    canvasX: number,
    canvasY: number,
    imageData: string,
    widthPt: number,
    heightPt: number
  ): SignatureAnnotation {
    const pdfCoords = canvasToPdf(canvasX, canvasY, this.scale, this.pageHeightPt);
    const ann: SignatureAnnotation = {
      kind: "signature",
      page: this.currentPage,
      x: pdfCoords.x,
      y: pdfCoords.y - heightPt,
      width: widthPt,
      height: heightPt,
      imageData,
    };
    this.committed.push(ann);
    this.redrawCommitted();
    return ann;
  }
}
