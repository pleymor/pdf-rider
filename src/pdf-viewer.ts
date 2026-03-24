import * as pdfjs from "pdfjs-dist";
import type { PDFDocumentProxy, PDFPageProxy, PageViewport } from "pdfjs-dist";
import { convertFileSrc } from "@tauri-apps/api/core";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

export class PdfViewer {
  private pdfDoc: PDFDocumentProxy | null = null;
  private currentPageObj: PDFPageProxy | null = null;
  private _currentPage = 1;
  private _pageCount = 0;
  private _scale = 1.5;
  private _rotation = 0;
  private _viewport: PageViewport | null = null;

  private canvas: HTMLCanvasElement;
  private textLayerDiv: HTMLElement;
  private onPageChangedCb?: (page: number, total: number) => void;
  private onLoadedCb?: (pageCount: number) => void;

  constructor() {
    this.canvas = document.getElementById("pdf-canvas") as HTMLCanvasElement;
    this.textLayerDiv = document.getElementById("text-layer") as HTMLElement;
  }

  get currentPage(): number { return this._currentPage; }
  get pageCount(): number { return this._pageCount; }
  get scale(): number { return this._scale; }
  get rotation(): number { return this._rotation; }

  /** The pdfjs PageViewport for the current render — includes rotation and scale. */
  get currentViewport(): PageViewport | null { return this._viewport; }

  /** Height of the current page in PDF points (accounts for rotation). */
  get pageHeightPt(): number {
    if (this._viewport) return this._viewport.height / this._scale;
    if (!this.currentPageObj) return 841; // A4 fallback
    return this.currentPageObj.getViewport({ scale: 1, rotation: this._rotation }).viewBox[3];
  }

  /** Width of the rendered canvas in pixels. */
  get canvasWidth(): number { return this.canvas.width; }
  /** Height of the rendered canvas in pixels. */
  get canvasHeight(): number { return this.canvas.height; }

  /** Width of the current page in PDF points (accounts for rotation). */
  get pageWidthPt(): number {
    if (this._viewport) return this._viewport.width / this._scale;
    if (!this.currentPageObj) return 595; // A4 fallback
    return this.currentPageObj.getViewport({ scale: 1, rotation: this._rotation }).viewBox[2];
  }

  onPageChanged(cb: (page: number, total: number) => void): void {
    this.onPageChangedCb = cb;
  }

  onLoaded(cb: (pageCount: number) => void): void {
    this.onLoadedCb = cb;
  }

  /** Load a PDF from a local file path. */
  async load(filePath: string): Promise<void> {
    if (this.pdfDoc) {
      await this.pdfDoc.destroy();
      this.pdfDoc = null;
    }

    const url = convertFileSrc(filePath);
    const loadingTask = pdfjs.getDocument({ url });
    this.pdfDoc = await loadingTask.promise;
    this._pageCount = this.pdfDoc.numPages;
    this._currentPage = 1;
    this._rotation = 0;
    this.onLoadedCb?.(this._pageCount);
    await this.render();
  }

  /** Load a password-protected PDF. */
  async loadWithPassword(filePath: string, password: string): Promise<void> {
    if (this.pdfDoc) {
      await this.pdfDoc.destroy();
      this.pdfDoc = null;
    }

    const url = convertFileSrc(filePath);
    const loadingTask = pdfjs.getDocument({ url, password });
    this.pdfDoc = await loadingTask.promise;
    this._pageCount = this.pdfDoc.numPages;
    this._currentPage = 1;
    this._rotation = 0;
    this.onLoadedCb?.(this._pageCount);
    await this.render();
  }

  /** Rotate the document view 90° clockwise and re-render. Marks the document dirty. */
  async rotate(): Promise<void> {
    this._rotation = (this._rotation + 90) % 360;
    await this.render();
  }

  /** Render the current page at the current scale. */
  async render(): Promise<void> {
    if (!this.pdfDoc) return;

    const page = await this.pdfDoc.getPage(this._currentPage);
    this.currentPageObj = page;
    const viewport = page.getViewport({ scale: this._scale, rotation: (page.rotate + this._rotation) % 360 });
    this._viewport = viewport;

    this.canvas.width = viewport.width;
    this.canvas.height = viewport.height;
    this.canvas.style.width = `${viewport.width}px`;
    this.canvas.style.height = `${viewport.height}px`;

    const ctx = this.canvas.getContext("2d")!;
    ctx.clearRect(0, 0, viewport.width, viewport.height);

    await page.render({ canvasContext: ctx, viewport }).promise;

    // Render selectable text layer — must complete before link layer can scan the DOM
    await this.buildTextLayer(page, viewport);

    this.onPageChangedCb?.(this._currentPage, this._pageCount);
    this.canvas.dispatchEvent(
      new CustomEvent("page-rendered", {
        bubbles: true,
        detail: { page: this._currentPage, width: viewport.width, height: viewport.height },
      })
    );
  }

  async goToPage(n: number): Promise<void> {
    if (!this.pdfDoc) return;
    this._currentPage = Math.max(1, Math.min(n, this._pageCount));
    await this.render();
  }

  async nextPage(): Promise<void> {
    if (this._currentPage < this._pageCount) {
      await this.goToPage(this._currentPage + 1);
    }
  }

  async prevPage(): Promise<void> {
    if (this._currentPage > 1) {
      await this.goToPage(this._currentPage - 1);
    }
  }

  async adjustZoom(delta: number): Promise<void> {
    this._scale = Math.max(0.5, Math.min(3.0, this._scale + delta));
    await this.render();
    return;
  }

  async setScale(scale: number): Promise<void> {
    this._scale = Math.max(0.25, Math.min(5.0, scale));
    await this.render();
  }

  /** Build a transparent, selectable text layer from raw text content.
   *  Each text item becomes an absolutely-positioned span with inline styles only,
   *  so no external CSS file is required. */
  private async buildTextLayer(page: PDFPageProxy, viewport: PageViewport): Promise<void> {
    this.textLayerDiv.innerHTML = "";
    this.textLayerDiv.style.width  = `${viewport.width}px`;
    this.textLayerDiv.style.height = `${viewport.height}px`;

    const textContent = await page.getTextContent();
    const fragment = document.createDocumentFragment();

    // Off-screen canvas for measuring rendered text widths
    const measureCtx = document.createElement("canvas").getContext("2d")!;

    for (const rawItem of textContent.items) {
      if (!("str" in rawItem)) continue;
      const item = rawItem as { str: string; transform: number[]; width: number; height: number };
      if (!item.str) continue;

      const [a, b, , , tx, ty] = item.transform;

      // Baseline origin in viewport (CSS) pixels
      const [vx, vy] = viewport.convertToViewportPoint(tx, ty);

      // Convert a second point to get direction and font-size in viewport space
      const [vpx, vpy] = viewport.convertToViewportPoint(tx + a, ty + b);
      const fontSizePx = Math.hypot(vpx - vx, vpy - vy);
      if (fontSizePx < 1) continue;

      // Angle of text direction in CSS space
      const angleDeg = Math.atan2(vpy - vy, vpx - vx) * (180 / Math.PI);

      // Target width of the text run in viewport pixels
      const widthPx = Math.max(item.width * viewport.scale, 1);

      const span = document.createElement("span");
      span.textContent = item.str;
      span.style.position        = "absolute";
      span.style.left            = `${vx}px`;
      span.style.top             = `${vy - fontSizePx}px`;
      span.style.fontSize        = `${fontSizePx}px`;
      span.style.fontFamily      = "sans-serif";
      span.style.lineHeight      = "1";
      span.style.color           = "transparent";
      span.style.whiteSpace      = "pre";
      span.style.cursor          = "text";
      span.style.pointerEvents   = "auto";
      span.style.transformOrigin = "0% 100%";
      span.style.setProperty("user-select",         "text");
      span.style.setProperty("-webkit-user-select",  "text");

      // Scale the span horizontally so it matches the PDF text width.
      // This ensures the selection highlight covers the full visible text.
      measureCtx.font = `${fontSizePx}px sans-serif`;
      const measured = measureCtx.measureText(item.str).width;
      const scaleX = measured > 0.5 ? widthPx / measured : 1;

      const transforms: string[] = [];
      if (Math.abs(angleDeg) > 0.5) transforms.push(`rotate(${angleDeg}deg)`);
      if (Math.abs(scaleX - 1) > 0.02) transforms.push(`scaleX(${scaleX})`);
      if (transforms.length) span.style.transform = transforms.join(" ");

      fragment.appendChild(span);
    }

    this.textLayerDiv.appendChild(fragment);
  }

  /** Build an interactive form layer over AcroForm Widget annotations.
   *  @param container   The #form-layer div to populate.
   *  @param storedValues Previously entered values, keyed by full field name.
   *  @param onChange    Called whenever a field value changes. */
  async buildFormLayer(
    container: HTMLElement,
    storedValues: Map<string, string>,
    onChange: (name: string, value: string) => void
  ): Promise<void> {
    container.innerHTML = "";
    if (!this.currentPageObj || !this._viewport) return;

    const viewport = this._viewport;
    container.style.width  = `${viewport.width}px`;
    container.style.height = `${viewport.height}px`;

    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const annotations: any[] = await (this.currentPageObj as any).getAnnotations({ intent: "display" });

    const fieldEls: { el: HTMLElement; top: number }[] = [];

    for (const a of annotations) {
      if (a.subtype !== "Widget") continue;
      if (a.hidden || a.readOnly) continue;

      const fieldName: string = a.fieldName ?? "";
      if (!fieldName) continue;

      const [x1, y1, x2, y2] = viewport.convertToViewportRectangle(a.rect as [number, number, number, number]);
      const left   = Math.min(x1, x2);
      const top    = Math.min(y1, y2);
      const width  = Math.abs(x2 - x1);
      const height = Math.abs(y2 - y1);
      const fontSize = Math.max(height * 0.65, 8);

      const storedVal = storedValues.get(fieldName);
      const initialVal: string = storedVal ?? (a.fieldValue ?? "");

      let el: HTMLElement;

      if (a.fieldType === "Tx") {
        if (a.multiLine) {
          const ta = document.createElement("textarea");
          ta.value = initialVal;
          ta.style.resize = "none";
          ta.style.overflow = "hidden";
          ta.addEventListener("input", () => onChange(fieldName, ta.value));
          el = ta;
        } else {
          const inp = document.createElement("input");
          inp.type = "text";
          inp.value = initialVal;
          inp.addEventListener("input", () => onChange(fieldName, inp.value));
          el = inp;
        }
      } else if (a.fieldType === "Btn") {
        if (a.checkBox) {
          const inp = document.createElement("input");
          inp.type = "checkbox";
          inp.checked = initialVal === "true" || initialVal === "Yes" || initialVal === "On";
          inp.addEventListener("change", () => onChange(fieldName, inp.checked ? "true" : "false"));
          inp.style.position = "absolute";
          inp.style.left   = `${left}px`;
          inp.style.top    = `${top}px`;
          inp.style.width  = `${width}px`;
          inp.style.height = `${height}px`;
          container.appendChild(inp);
          fieldEls.push({ el: inp, top });
          continue; // skip generic positioning below
        } else if (a.radioButton) {
          const inp = document.createElement("input");
          inp.type  = "radio";
          inp.name  = fieldName;
          inp.value = a.exportValue ?? "true";
          inp.checked = storedVal === inp.value || (!storedVal && a.fieldValue === a.exportValue);
          inp.addEventListener("change", () => {
            if (inp.checked) onChange(fieldName, inp.value);
          });
          inp.style.position = "absolute";
          inp.style.left   = `${left}px`;
          inp.style.top    = `${top}px`;
          inp.style.width  = `${width}px`;
          inp.style.height = `${height}px`;
          container.appendChild(inp);
          fieldEls.push({ el: inp, top });
          continue;
        } else {
          continue; // push button — skip
        }
      } else if (a.fieldType === "Ch") {
        const sel = document.createElement("select");
        const opts: Array<{ exportValue: string; displayValue: string }> = a.options ?? [];
        for (const opt of opts) {
          const option = document.createElement("option");
          option.value = opt.exportValue;
          option.textContent = opt.displayValue;
          if (opt.exportValue === initialVal) option.selected = true;
          sel.appendChild(option);
        }
        sel.addEventListener("change", () => onChange(fieldName, sel.value));
        el = sel;
      } else {
        continue;
      }

      el.style.position  = "absolute";
      el.style.left      = `${left}px`;
      el.style.top       = `${top}px`;
      el.style.width     = `${width}px`;
      el.style.height    = `${height}px`;
      el.style.fontSize  = `${fontSize}px`;
      container.appendChild(el);
      fieldEls.push({ el, top });
    }

    // Sort by vertical position and wire up arrow-key navigation
    fieldEls.sort((a, b) => a.top - b.top);
    const els = fieldEls.map(f => f.el);
    for (let i = 0; i < els.length; i++) {
      els[i].addEventListener("keydown", (e: Event) => {
        const ke = e as KeyboardEvent;
        if (ke.key !== "ArrowDown" && ke.key !== "ArrowUp") return;
        ke.preventDefault();
        const next = ke.key === "ArrowDown" ? els[i + 1] : els[i - 1];
        if (next) next.focus();
      });
    }
  }

  /** Returns formal Link annotations for the current page with their viewport rects.
   *  QuadPoints are used when present (multi-line links); otherwise the annotation rect.
   *  Bare text URLs and accurate link hit-rects are computed in the UI layer via DOM
   *  scanning after the text layer has been rendered. */
  async getPageLinkAnnotations(): Promise<Array<{ url: string; rect: [number, number, number, number] }>> {
    if (!this.pdfDoc || !this._viewport) return [];
    const page = await this.pdfDoc.getPage(this._currentPage);
    const annotations = await page.getAnnotations();
    const viewport = this._viewport;
    const result: Array<{ url: string; rect: [number, number, number, number] }> = [];

    for (const ann of annotations) {
      if (ann.subtype !== "Link") continue;
      const url: string | undefined = ann.url ?? ann.unsafeUrl;
      if (!url || (!url.startsWith("http://") && !url.startsWith("https://"))) continue;

      const quads = ann.quadPoints as number[] | undefined;
      if (quads && quads.length >= 8) {
        for (let i = 0; i + 7 < quads.length; i += 8) {
          const xs = [quads[i], quads[i + 2], quads[i + 4], quads[i + 6]];
          const ys = [quads[i + 1], quads[i + 3], quads[i + 5], quads[i + 7]];
          const pdfRect: [number, number, number, number] = [
            Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys),
          ];
          const [x1, y1, x2, y2] = viewport.convertToViewportRectangle(pdfRect);
          result.push({ url, rect: [x1, y1, x2, y2] });
        }
      } else {
        const [x1, y1, x2, y2] = viewport.convertToViewportRectangle(
          ann.rect as [number, number, number, number]
        );
        result.push({ url, rect: [x1, y1, x2, y2] });
      }
    }

    return result;
  }

  /** Restore a known rotation without re-rendering (call render/renderCurrentPage after). */
  restoreRotation(degrees: number): void {
    this._rotation = degrees;
  }

  /** Renders all pages as JPEG data URLs (base64 only, no prefix) for silent printing. */
  async renderAllPagesForPrint(dpi = 200): Promise<string[]> {
    if (!this.pdfDoc) return [];
    const scale = dpi / 72;
    const results: string[] = [];
    for (let i = 1; i <= this._pageCount; i++) {
      const page = await this.pdfDoc.getPage(i);
      const viewport = page.getViewport({
        scale,
        rotation: (page.rotate + this._rotation) % 360,
      });
      const canvas = document.createElement("canvas");
      canvas.width = viewport.width;
      canvas.height = viewport.height;
      const ctx = canvas.getContext("2d")!;
      await page.render({ canvasContext: ctx, viewport }).promise;
      results.push(canvas.toDataURL("image/jpeg", 0.92).split(",")[1]);
    }
    return results;
  }

  isLoaded(): boolean {
    return this.pdfDoc !== null;
  }

  async close(): Promise<void> {
    if (this.pdfDoc) {
      await this.pdfDoc.destroy();
      this.pdfDoc = null;
      this._pageCount = 0;
      this._currentPage = 1;
    }
    const ctx = this.canvas.getContext("2d")!;
    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    this.canvas.width = 0;
    this.canvas.height = 0;
    this.textLayerDiv.innerHTML = "";
  }
}
