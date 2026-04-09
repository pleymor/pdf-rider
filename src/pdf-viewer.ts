import * as pdfjs from "pdfjs-dist";
import type { PDFDocumentProxy, PDFPageProxy, PageViewport } from "pdfjs-dist";
import { convertFileSrc } from "@tauri-apps/api/core";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { CanvasOverlay } from "./canvas-overlay";
import { AnnotationStore } from "./annotation-store";
import { defaultToolState, type ActiveToolState, CSS_UNITS } from "./models";
import { openUrl } from "./tauri-bridge";

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

// ── Pure layout functions (exported for unit tests) ───────────────────────────

const MAX_COLUMNS = 3;
const PAGE_GAP = 12; // px between columns

/**
 * Calculate how many pages fit side-by-side in the viewer, capped at MAX_COLUMNS.
 */
export function calculateColumnCount(
  containerWidth: number,
  refPageWidthPx: number,
  gap: number
): number {
  return Math.min(MAX_COLUMNS, Math.max(1, Math.floor((containerWidth + gap) / (refPageWidthPx + gap))));
}

/**
 * Group 1-indexed page numbers into rows of `columnCount`.
 */
export function buildRows(pageCount: number, columnCount: number): number[][] {
  const rows: number[][] = [];
  for (let i = 0; i < pageCount; i += columnCount) {
    const row: number[] = [];
    for (let j = i; j < Math.min(i + columnCount, pageCount); j++) {
      row.push(j + 1);
    }
    rows.push(row);
  }
  return rows;
}

// ── PdfPageView ───────────────────────────────────────────────────────────────

export class PdfPageView {
  readonly pageNum: number;
  readonly wrapper: HTMLElement;
  readonly overlay: CanvasOverlay;
  rendered = false;
  rendering = false;

  private canvas: HTMLCanvasElement;
  private textLayer: HTMLElement;
  private linkLayer: HTMLElement;
  private formLayer: HTMLElement;
  private annotationCanvas: HTMLCanvasElement;
  private _viewport: PageViewport | null = null;

  constructor(pageNum: number, toolState: ActiveToolState) {
    this.pageNum = pageNum;

    this.wrapper = document.createElement("div");
    this.wrapper.className = "page-wrapper";
    this.wrapper.dataset.page = String(pageNum);

    this.canvas = document.createElement("canvas");
    this.canvas.className = "page-canvas";

    this.textLayer = document.createElement("div");
    this.textLayer.className = "page-text-layer";

    this.linkLayer = document.createElement("div");
    this.linkLayer.className = "page-link-layer";

    this.formLayer = document.createElement("div");
    this.formLayer.className = "page-form-layer";

    this.annotationCanvas = document.createElement("canvas");
    this.annotationCanvas.className = "page-annotation-canvas";

    this.wrapper.append(
      this.canvas,
      this.textLayer,
      this.linkLayer,
      this.formLayer,
      this.annotationCanvas
    );
    this.overlay = new CanvasOverlay(this.annotationCanvas, this.wrapper, toolState);
  }

  get viewport(): PageViewport | null { return this._viewport; }

  setPlaceholderSize(width: number, height: number): void {
    this.wrapper.style.width  = `${Math.round(width)}px`;
    this.wrapper.style.height = `${Math.round(height)}px`;
  }

  async render(
    pdfDoc: PDFDocumentProxy,
    scale: number,
    rotation: number,
    store: AnnotationStore,
    formValues: Map<string, string>,
    onFormChange: (name: string, val: string) => void
  ): Promise<void> {
    if (this.rendering) return;
    this.rendering = true;
    try {
      const page = await pdfDoc.getPage(this.pageNum);
      const viewport = page.getViewport({ scale, rotation: (page.rotate + rotation) % 360 });
      this._viewport = viewport;

      // Render PDF content into an off-screen buffer first so the visible
      // canvas is never blank — old content stays until the new frame is ready.
      const buf = document.createElement("canvas");
      buf.width  = viewport.width;
      buf.height = viewport.height;
      const bufCtx = buf.getContext("2d")!;
      await page.render({ canvasContext: bufCtx, viewport }).promise;

      // Atomic swap: resize + blit in one synchronous block.
      this.canvas.width  = viewport.width;
      this.canvas.height = viewport.height;
      this.canvas.style.width  = `${viewport.width}px`;
      this.canvas.style.height = `${viewport.height}px`;
      this.canvas.getContext("2d")!.drawImage(buf, 0, 0);

      // Size annotation canvas (no content to preserve here).
      this.annotationCanvas.width  = viewport.width;
      this.annotationCanvas.height = viewport.height;
      this.annotationCanvas.style.width  = `${viewport.width}px`;
      this.annotationCanvas.style.height = `${viewport.height}px`;

      // Snap wrapper to actual rendered size
      this.setPlaceholderSize(viewport.width, viewport.height);

      // Build overlay layers (text first — link layer scans text spans)
      await this.buildTextLayer(page, viewport);
      await this.buildLinkLayer(page, viewport);
      await this.buildFormLayer(page, viewport, formValues, onFormChange);

      // Sync annotation overlay
      this.overlay.syncToPage(
        this.pageNum,
        scale,
        viewport.height / scale,
        store.getForPage(this.pageNum),
        viewport
      );

      this.rendered = true;
    } finally {
      this.rendering = false;
    }
  }

  /** Remove rendered content while preserving placeholder dimensions. */
  clear(): void {
    const ctx = this.canvas.getContext("2d");
    if (ctx) ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    this.textLayer.innerHTML  = "";
    this.linkLayer.innerHTML  = "";
    this.formLayer.innerHTML  = "";
    this.overlay.syncToPage(this.pageNum, 1, 841, []);
    this.rendered = false;
  }

  // ── Text layer ──────────────────────────────────────────────────────────────

  private async buildTextLayer(page: PDFPageProxy, viewport: PageViewport): Promise<void> {
    this.textLayer.innerHTML = "";
    this.textLayer.style.width  = `${viewport.width}px`;
    this.textLayer.style.height = `${viewport.height}px`;

    const textContent = await page.getTextContent();
    const fragment = document.createDocumentFragment();
    const measureCtx = document.createElement("canvas").getContext("2d")!;

    for (const rawItem of textContent.items) {
      if (!("str" in rawItem)) continue;
      const item = rawItem as { str: string; transform: number[]; width: number; height: number };
      if (!item.str) continue;

      const [a, b, , , tx, ty] = item.transform;
      const [vx, vy]  = viewport.convertToViewportPoint(tx, ty);
      const [vpx, vpy] = viewport.convertToViewportPoint(tx + a, ty + b);
      const fontSizePx = Math.hypot(vpx - vx, vpy - vy);
      if (fontSizePx < 1) continue;

      const angleDeg = Math.atan2(vpy - vy, vpx - vx) * (180 / Math.PI);
      const widthPx  = Math.max(item.width * viewport.scale, 1);

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
      span.style.setProperty("user-select",        "text");
      span.style.setProperty("-webkit-user-select", "text");

      measureCtx.font = `${fontSizePx}px sans-serif`;
      const measured = measureCtx.measureText(item.str).width;
      const scaleX   = measured > 0.5 ? widthPx / measured : 1;

      const transforms: string[] = [];
      if (Math.abs(angleDeg) > 0.5) transforms.push(`rotate(${angleDeg}deg)`);
      if (Math.abs(scaleX - 1) > 0.02) transforms.push(`scaleX(${scaleX})`);
      if (transforms.length) span.style.transform = transforms.join(" ");

      fragment.appendChild(span);
    }
    this.textLayer.appendChild(fragment);
  }

  // ── Link layer ──────────────────────────────────────────────────────────────

  private async buildLinkLayer(page: PDFPageProxy, viewport: PageViewport): Promise<void> {
    this.linkLayer.innerHTML = "";

    // Collect text span positions for URL detection
    const mc = document.createElement("canvas").getContext("2d")!;
    const measure = (text: string, span: HTMLSpanElement): number => {
      mc.font = `${span.style.fontSize} ${span.style.fontFamily || "sans-serif"}`;
      return mc.measureText(text).width;
    };

    interface SpanEntry {
      span: HTMLSpanElement;
      start: number;
      end: number;
      text: string;
      pivotX: number;   // canvas x of text origin (= CSS left)
      pivotY: number;   // canvas y of text origin (= CSS top + fontSize)
      top: number;      // CSS top (used for line-break detection only)
      fontSize: number;
      angle: number;    // CSS rotation in degrees (from transform:rotate())
    }
    type Rect = { left: number; top: number; right: number; bottom: number };

    let combined = "";
    const entries: SpanEntry[] = [];
    const lineBreakPos = new Set<number>();
    let prevTop = NaN;

    for (const span of this.textLayer.querySelectorAll("span") as NodeListOf<HTMLSpanElement>) {
      const text = span.textContent ?? "";
      if (!text) continue;
      const spanTop  = parseFloat(span.style.top)      || 0;
      const fontSize = parseFloat(span.style.fontSize) || 12;
      if (combined.length > 0 && Math.abs(spanTop - prevTop) > 2) {
        lineBreakPos.add(combined.length);
        combined += " ";
      }
      const start = combined.length;
      combined += text;
      const rotMatch = /rotate\(([+-]?\d+(?:\.\d+)?)deg\)/.exec(span.style.transform || "");
      entries.push({
        span,
        start,
        end: combined.length,
        text,
        pivotX:   parseFloat(span.style.left) || 0,
        pivotY:   spanTop + fontSize,   // transform-origin is 0% 100% = bottom-left
        top:      spanTop,
        fontSize,
        angle:    rotMatch ? parseFloat(rotMatch[1]) : 0,
      });
      prevTop = spanTop;
    }

    const domRectsForRange = (urlStart: number, urlEnd: number): Rect[] => {
      const rects: Rect[] = [];
      for (const e of entries) {
        if (e.end <= urlStart || e.start >= urlEnd) continue;
        const urlL = Math.max(e.start, urlStart) - e.start;
        const urlR = Math.min(e.end,   urlEnd)   - e.start;
        const dxL = measure(e.text.slice(0, urlL), e.span);
        const dxR = measure(e.text.slice(0, urlR), e.span);
        if (dxR <= dxL) continue;
        if (Math.abs(e.angle) < 0.5) {
          // No rotation — simple horizontal rect.
          rects.push({ left: e.pivotX + dxL, top: e.pivotY - e.fontSize,
                       right: e.pivotX + dxR, bottom: e.pivotY });
        } else {
          // Rotate the 4 corners of the URL sub-rect around the pivot and
          // return the axis-aligned bounding box.
          const θ = e.angle * Math.PI / 180;
          const cos = Math.cos(θ), sin = Math.sin(θ);
          // corners relative to pivot (pivotX, pivotY):
          //   x axis = text advance direction, y axis = up (−fontSize)
          const corners: [number, number][] = [
            [dxL, -e.fontSize], [dxR, -e.fontSize],
            [dxR, 0],           [dxL, 0],
          ];
          const xs = corners.map(([x, y]) => e.pivotX + cos * x - sin * y);
          const ys = corners.map(([x, y]) => e.pivotY + sin * x + cos * y);
          rects.push({ left: Math.min(...xs), right: Math.max(...xs),
                       top:  Math.min(...ys), bottom: Math.max(...ys) });
        }
      }
      return rects;
    };

    interface DomEntry { startPos: number; rects: Rect[] }
    const urlRe = /https?:\/\/[^\s)\]>"]+/g;
    let m: RegExpExecArray | null;
    const domLinks = new Map<string, DomEntry>();

    while ((m = urlRe.exec(combined)) !== null) {
      const urlEnd = m.index + m[0].length;
      const rects  = domRectsForRange(m.index, urlEnd);
      if (rects.length > 0 && !domLinks.has(m[0])) {
        domLinks.set(m[0], { startPos: m.index, rects });
      }
      if (!lineBreakPos.has(urlEnd)) continue;
      for (const e of entries) {
        if (e.start < urlEnd + 1) continue;
        if (e.start > urlEnd + 2) break;
        const fragMatch = /^[^\s)\]>"]+/.exec(e.text);
        if (!fragMatch) break;
        const frag = fragMatch[0];
        if (frag.length >= 2 && frag.length <= 40 && /[./\-_?#=&%]/.test(frag)) {
          const fullUrl = m[0] + frag;
          if (!domLinks.has(fullUrl)) {
            const contRects = domRectsForRange(e.start, e.start + frag.length);
            domLinks.set(fullUrl, { startPos: m.index, rects: [...rects, ...contRects] });
          }
        }
        break;
      }
    }

    // Formal PDF link annotations
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const annotations: any[] = await (page as any).getAnnotations();
    const addedUrls = new Set<string>();

    const addLink = (url: string, rects: Rect[]) => {
      addedUrls.add(url);
      for (const r of rects) {
        const a = document.createElement("a");
        a.style.cssText = `position:absolute;left:${r.left}px;top:${r.top}px;width:${r.right - r.left}px;height:${r.bottom - r.top}px;`;
        a.addEventListener("click", (e) => { e.preventDefault(); void openUrl(url); });
        this.linkLayer.appendChild(a);
      }
    };

    for (const ann of annotations) {
      if (ann.subtype !== "Link") continue;
      const url: string | undefined = ann.url ?? ann.unsafeUrl;
      if (!url || (!url.startsWith("http://") && !url.startsWith("https://"))) continue;
      if (addedUrls.has(url)) continue;

      let bestDomUrl = "";
      let bestEntry: DomEntry | undefined;
      for (const [du, entry] of domLinks) {
        if (du === url) { bestDomUrl = du; bestEntry = entry; break; }
        const n = Math.min(du.length, url.length);
        if (n >= 10 && du.slice(0, n) === url.slice(0, n) && du.length > bestDomUrl.length) {
          bestDomUrl = du; bestEntry = entry;
        }
      }

      if (bestEntry) {
        addLink(url, [...bestEntry.rects]);
      } else {
        const [x1, y1, x2, y2] = viewport.convertToViewportRectangle(
          ann.rect as [number, number, number, number]
        );
        addLink(url, [{ left: Math.min(x1,x2), top: Math.min(y1,y2), right: Math.max(x1,x2), bottom: Math.max(y1,y2) }]);
      }
    }

    // DOM-discovered URLs not covered by a formal annotation
    for (const [url, { rects }] of domLinks) {
      if (!addedUrls.has(url)) addLink(url, rects);
    }
  }

  // ── Form layer ──────────────────────────────────────────────────────────────

  private async buildFormLayer(
    page: PDFPageProxy,
    viewport: PageViewport,
    storedValues: Map<string, string>,
    onChange: (name: string, val: string) => void
  ): Promise<void> {
    this.formLayer.innerHTML = "";
    this.formLayer.style.width  = `${viewport.width}px`;
    this.formLayer.style.height = `${viewport.height}px`;

    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const annotations: any[] = await (page as any).getAnnotations({ intent: "display" });
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
      const fontSize  = Math.max(height * 0.65, 8);
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
          inp.checked = !!initialVal && initialVal !== "Off" && initialVal !== "false";
          inp.addEventListener("change", () => onChange(fieldName, inp.checked ? "true" : "false"));
          inp.style.cssText = `position:absolute;left:${left}px;top:${top}px;width:${width}px;height:${height}px;`;
          this.formLayer.appendChild(inp);
          fieldEls.push({ el: inp, top });
          continue;
        } else if (a.radioButton) {
          const inp = document.createElement("input");
          inp.type  = "radio";
          inp.name  = fieldName;
          inp.value = a.exportValue ?? "true";
          inp.checked = storedVal === inp.value || (!storedVal && a.fieldValue === a.exportValue);
          inp.addEventListener("change", () => { if (inp.checked) onChange(fieldName, inp.value); });
          inp.style.cssText = `position:absolute;left:${left}px;top:${top}px;width:${width}px;height:${height}px;`;
          this.formLayer.appendChild(inp);
          fieldEls.push({ el: inp, top });
          continue;
        } else {
          continue;
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

      el.style.position = "absolute";
      el.style.left      = `${left}px`;
      el.style.top       = `${top}px`;
      el.style.width     = `${width}px`;
      el.style.height    = `${height}px`;
      el.style.fontSize  = `${fontSize}px`;
      this.formLayer.appendChild(el);
      fieldEls.push({ el, top });
    }

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
}

// ── PdfViewer ─────────────────────────────────────────────────────────────────

export class PdfViewer {
  private pdfDoc: PDFDocumentProxy | null = null;
  private _currentPage = 1;
  private _focusedPage = 1;
  private _pageCount  = 0;
  private _scale      = CSS_UNITS; // 100% actual size
  private _rotation   = 0;
  private _toolState: ActiveToolState = defaultToolState();
  private _store: AnnotationStore = new AnnotationStore();
  private _formValues: Map<string, string> = new Map();
  private _onFormChange: (name: string, val: string) => void = () => {};

  pageDimensions: Array<{ width: number; height: number }> = [];
  pageViews: PdfPageView[] = [];
  columnCount = 1;

  private onLayoutChangedCb?: (views: PdfPageView[]) => void;
  private observer: IntersectionObserver | null = null;
  private resizeObserver: ResizeObserver | null = null;
  private resizeTimer: ReturnType<typeof setTimeout> | null = null;
  private scrollEl: HTMLElement;

  constructor() {
    this.scrollEl = document.getElementById("viewer-scroll") as HTMLElement;
  }

  // ── Accessors ───────────────────────────────────────────────────────────────

  get currentPage(): number   { return this._currentPage; }
  get pageCount():   number   { return this._pageCount; }
  get scale():       number   { return this._scale; }
  get rotation():    number   { return this._rotation; }

  get pageWidthPt():  number {
    const dim = this.pageDimensions[0];
    return dim ? dim.width : 595;
  }
  get pageHeightPt(): number {
    const dim = this.pageDimensions[0];
    return dim ? dim.height : 841;
  }

  get currentViewport(): PageViewport | null {
    return this.pageViews[this._currentPage - 1]?.viewport ?? null;
  }

  // ── Setup ────────────────────────────────────────────────────────────────────

  /** Register a callback invoked after every buildLayout() with the fresh pageViews array. */
  onLayoutChanged(cb: (views: PdfPageView[]) => void): void {
    this.onLayoutChangedCb = cb;
  }

  /** Call from main.ts before the first load() to provide shared context. */
  setup(
    toolState: ActiveToolState,
    store: AnnotationStore,
    formValues: Map<string, string>,
    onFormChange: (name: string, val: string) => void
  ): void {
    this._toolState    = toolState;
    this._store        = store;
    this._formValues   = formValues;
    this._onFormChange = onFormChange;
  }

  // ── Load ────────────────────────────────────────────────────────────────────

  async load(filePath: string): Promise<void> {
    await this._openDoc(convertFileSrc(filePath) + `?t=${Date.now()}`);
  }

  async loadWithPassword(filePath: string, password: string): Promise<void> {
    await this._openDoc(convertFileSrc(filePath) + `?t=${Date.now()}`, password);
  }

  private async _openDoc(url: string, password?: string): Promise<void> {
    if (this.pdfDoc) {
      await this.pdfDoc.destroy();
      this.pdfDoc = null;
    }
    this.observer?.disconnect();
    if (this._scrollHandler) { this.scrollEl.removeEventListener("scroll", this._scrollHandler); this._scrollHandler = null; }

    const loadingTask = pdfjs.getDocument(password ? { url, password } : { url });
    this.pdfDoc = await loadingTask.promise;
    this._pageCount   = this.pdfDoc.numPages;
    this._currentPage = 1;
    this._focusedPage = 1;
    this._rotation    = 0;

    // Pre-fetch page dimensions (T010)
    this.pageDimensions = [];
    for (let i = 1; i <= this._pageCount; i++) {
      const p  = await this.pdfDoc.getPage(i);
      const vp = p.getViewport({ scale: 1.0 });
      this.pageDimensions.push({ width: vp.width, height: vp.height });
    }

    this.buildLayout();
    this.setupResizeObserver();
  }

  // ── Layout (T011 + T018-T019) ────────────────────────────────────────────────

  buildLayout(): void {
    this.observer?.disconnect();
    if (this._scrollHandler) { this.scrollEl.removeEventListener("scroll", this._scrollHandler); this._scrollHandler = null; }
    this.scrollEl.innerHTML = "";
    this.pageViews = [];

    if (!this.pdfDoc || this.pageDimensions.length === 0) return;

    // Column count (US2: T018)
    // Swap width/height when user rotation is 90° or 270° so placeholder sizes
    // match the rendered viewport and don't trigger a ResizeObserver loop.
    const swapped = this._rotation % 180 !== 0;
    const containerWidth  = Math.max(1, this.scrollEl.clientWidth - 40);
    const refPageWidthPx  = Math.max(...this.pageDimensions.map(d => swapped ? d.height : d.width)) * this._scale;
    this.columnCount = calculateColumnCount(containerWidth, refPageWidthPx, PAGE_GAP);

    // Build rows (US2: T019)
    const rows = buildRows(this._pageCount, this.columnCount);
    for (const row of rows) {
      const rowEl = document.createElement("div");
      rowEl.className = "page-row";
      for (const pageNum of row) {
        const view = new PdfPageView(pageNum, this._toolState);
        const dim  = this.pageDimensions[pageNum - 1];
        if (dim) view.setPlaceholderSize(
          (swapped ? dim.height : dim.width) * this._scale,
          (swapped ? dim.width : dim.height) * this._scale
        );
        rowEl.appendChild(view.wrapper);
        this.pageViews.push(view);
      }
      this.scrollEl.appendChild(rowEl);
    }

    this.setupObserver();
    this.onLayoutChangedCb?.(this.pageViews);
  }

  // ── IntersectionObserver (T012) ──────────────────────────────────────────────

  private setupObserver(): void {
    // IntersectionObserver for lazy rendering / clearing
    this.observer = new IntersectionObserver(
      (entries) => {
        const visiblePages = entries
          .filter(e => e.isIntersecting)
          .map(e => parseInt((e.target as HTMLElement).dataset.page ?? "0"))
          .filter(Boolean);

        for (const entry of entries) {
          const pageNum  = parseInt((entry.target as HTMLElement).dataset.page ?? "0");
          const pageView = this.pageViews[pageNum - 1];
          if (!pageView) continue;

          if (entry.isIntersecting) {
            if (!pageView.rendered && !pageView.rendering) {
              void this._renderPage(pageView);
            }
          } else {
            const minV = Math.min(...(visiblePages.length > 0 ? visiblePages : [pageNum]));
            const maxV = Math.max(...(visiblePages.length > 0 ? visiblePages : [pageNum]));
            if (pageNum < minV - 2 || pageNum > maxV + 2) {
              pageView.clear();
            }
          }
        }
      },
      { root: this.scrollEl, rootMargin: "200px 0px" }
    );

    for (const pv of this.pageViews) this.observer.observe(pv.wrapper);

    // Scroll listener for accurate current-page tracking
    this._scrollHandler = () => this._updateFocusedPage();
    this.scrollEl.addEventListener("scroll", this._scrollHandler, { passive: true });
    // Run once to set initial focused page
    this._updateFocusedPage();
  }

  private _scrollHandler: (() => void) | null = null;

  private _updateFocusedPage(): void {
    const scrollTop = this.scrollEl.scrollTop;
    const viewportMid = scrollTop + this.scrollEl.clientHeight / 3;
    let best = 1;
    let bestDist = Infinity;
    for (const pv of this.pageViews) {
      const top = pv.wrapper.offsetTop;
      const bottom = top + pv.wrapper.offsetHeight;
      const mid = (top + bottom) / 2;
      const dist = Math.abs(mid - viewportMid);
      if (dist < bestDist) {
        bestDist = dist;
        best = pv.pageNum;
      }
    }
    if (best !== this._focusedPage) {
      this._focusedPage = best;
      this._currentPage = best;
      this.scrollEl.dispatchEvent(
        new CustomEvent("focused-page-changed", {
          detail: { page: best },
          bubbles: true,
        })
      );
    }
  }

  private async _renderPage(pageView: PdfPageView): Promise<void> {
    if (!this.pdfDoc) return;
    const expectedScale    = this._scale;
    const expectedRotation = this._rotation;
    await pageView.render(
      this.pdfDoc,
      expectedScale,
      expectedRotation,
      this._store,
      this._formValues,
      this._onFormChange
    );
    // If scale/rotation changed while this render was running (e.g. the user
    // zoomed again before the previous render finished), the render that just
    // completed used a stale scale and its setPlaceholderSize() call left the
    // wrapper at the wrong size.  Re-render immediately at the current values.
    if (this._scale !== expectedScale || this._rotation !== expectedRotation) {
      void this._renderPage(pageView);
    }
  }

  // ── Navigation (T015) ────────────────────────────────────────────────────────

  goToPage(n: number, behavior: ScrollBehavior = "smooth"): void {
    const clamped = Math.max(1, Math.min(n, this._pageCount));
    this._currentPage = clamped;
    this.pageViews[clamped - 1]?.wrapper.scrollIntoView({ behavior, block: "start" });
  }

  async nextPage(): Promise<void> { this.goToPage(this._currentPage + this.columnCount); }
  async prevPage(): Promise<void> { this.goToPage(this._currentPage - this.columnCount); }

  // ── Zoom / Rotate / Reflow (T022-T026) ──────────────────────────────────────

  async setScale(scale: number): Promise<void> {
    this._scale = Math.max(0.25 * CSS_UNITS, Math.min(5.0 * CSS_UNITS, scale));
    this.reflow();
  }

  async rotate(): Promise<void> {
    this._rotation = (this._rotation + 90) % 360;
    this.reflow();
  }

  reflow(): void {
    const savedPage = this._focusedPage;

    // When the column count won't change, avoid tearing down the DOM entirely
    // (which causes a full black-screen flash).  Instead update placeholder
    // sizes and re-render visible pages in place — old canvas content stays
    // visible until the new render overwrites it.
    if (this.pdfDoc && this.pageViews.length === this._pageCount) {
      const swapped        = this._rotation % 180 !== 0;
      const containerWidth = Math.max(1, this.scrollEl.clientWidth - 40);
      const refW           = Math.max(...this.pageDimensions.map(d => swapped ? d.height : d.width)) * this._scale;
      const newCols        = calculateColumnCount(containerWidth, refW, PAGE_GAP);

      if (newCols === this.columnCount) {
        for (const view of this.pageViews) {
          const dim = this.pageDimensions[view.pageNum - 1];
          if (dim) view.setPlaceholderSize(
            (swapped ? dim.height : dim.width) * this._scale,
            (swapped ? dim.width  : dim.height) * this._scale
          );
          view.rendered = false;
        }
        this.onLayoutChangedCb?.(this.pageViews);
        // Scroll first so getBoundingClientRect() below reflects the final
        // scroll position — pages visible *after* the scroll get renders too.
        this.goToPage(savedPage, "instant");
        const scrollRect = this.scrollEl.getBoundingClientRect();
        for (const view of this.pageViews) {
          const r = view.wrapper.getBoundingClientRect();
          if (r.bottom > scrollRect.top - 200 && r.top < scrollRect.bottom + 200) {
            void this._renderPage(view);
          }
        }
        return;
      }
    }

    this.buildLayout();
    this.goToPage(savedPage, "instant");
  }

  restoreRotation(degrees: number): void {
    this._rotation = degrees;
  }

  // ── ResizeObserver (T021) ────────────────────────────────────────────────────

  private setupResizeObserver(): void {
    this.resizeObserver?.disconnect();
    this.resizeObserver = new ResizeObserver(() => {
      if (this.resizeTimer) clearTimeout(this.resizeTimer);
      this.resizeTimer = setTimeout(() => this.reflow(), 100);
    });
    this.resizeObserver.observe(this.scrollEl);
  }

  // ── Print (T027 — unchanged logic) ──────────────────────────────────────────

  async renderAllPagesForPrint(dpi = 200): Promise<string[]> {
    if (!this.pdfDoc) return [];
    const scale   = dpi / 72;
    const results: string[] = [];
    for (let i = 1; i <= this._pageCount; i++) {
      const page     = await this.pdfDoc.getPage(i);
      const viewport = page.getViewport({ scale, rotation: (page.rotate + this._rotation) % 360 });
      const canvas   = document.createElement("canvas");
      canvas.width   = viewport.width;
      canvas.height  = viewport.height;
      const ctx = canvas.getContext("2d")!;
      await page.render({ canvasContext: ctx, viewport }).promise;
      results.push(canvas.toDataURL("image/jpeg", 0.92).split(",")[1]);
    }
    return results;
  }

  // ── Lifecycle ────────────────────────────────────────────────────────────────

  isLoaded(): boolean { return this.pdfDoc !== null; }

  async close(): Promise<void> {
    this.observer?.disconnect();
    if (this._scrollHandler) { this.scrollEl.removeEventListener("scroll", this._scrollHandler); this._scrollHandler = null; }
    this.resizeObserver?.disconnect();
    this.scrollEl.innerHTML = "";
    this.pageViews = [];
    if (this.pdfDoc) {
      await this.pdfDoc.destroy();
      this.pdfDoc = null;
    }
    this._pageCount   = 0;
    this._currentPage = 1;
    this.pageDimensions = [];
  }

  // ── Backward-compat shims (removed in T016) ──────────────────────────────────

  /** @deprecated Use buildLayout() + IntersectionObserver instead. */
  async render(): Promise<void> {
    if (!this.pdfDoc) return;
    this.buildLayout();
  }

  /** @deprecated Kept for transition; per-page form layers are built in PdfPageView.render(). */
  async buildFormLayer(
    _container: HTMLElement,
    _storedValues: Map<string, string>,
    _onChange: (name: string, val: string) => void
  ): Promise<void> { /* no-op during transition */ }

  /** @deprecated Kept for transition; link annotations are now per-page. */
  async getPageLinkAnnotations(): Promise<Array<{ url: string; rect: [number, number, number, number] }>> {
    return [];
  }
}
