import {
  canvasToPdf,
  hexToRgb,
  pdfToCanvas,
  rgbToCss,
  rgbToHex,
  type ActiveToolState,
  type Annotation,
  type CircleAnnotation,
  type RectAnnotation,
  type RgbColor,
  type SignatureAnnotation,
  type TextAlignmentValue,
  type TextAnnotation,
  type ToolKind,
} from "./models";

/** Minimal duck-type for pdfjs PageViewport — avoids importing pdfjs-dist here. */
interface PdfViewport {
  convertToViewportPoint(pdfX: number, pdfY: number): number[];
  convertToPdfPoint(viewX: number, viewY: number): number[];
}

type AnnotationCreatedHandler      = (ann: Annotation) => void;
type AnnotationMovedHandler        = (ann: Annotation) => void;
type AnnotationRemovedHandler      = (ann: Annotation) => void;
type AnnotationReorderHandler      = (ann: Annotation, dir: "front" | "back") => void;
type TextAnnotationSelectedHandler  = (ann: TextAnnotation | null) => void;
type ShapeAnnotationSelectedHandler = (ann: RectAnnotation | CircleAnnotation | null) => void;
type ResizeHandle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

const HANDLE_R = 4; // half-size of handle squares in px
const HANDLE_HIT = 7; // hit radius in px
const HANDLE_CURSORS: Record<ResizeHandle, string> = {
  nw: "nw-resize", n: "n-resize",  ne: "ne-resize",
  e:  "e-resize",  se: "se-resize", s:  "s-resize",
  sw: "sw-resize", w:  "w-resize",
};
const MIN_SIZE_PT = 10; // minimum annotation dimension in PDF pts

export class CanvasOverlay {
  private canvas: HTMLCanvasElement;
  private container: HTMLElement;
  private ctx: CanvasRenderingContext2D;

  private style: ActiveToolState;
  private activeTool: ToolKind = "select";

  // ── Draw state ──────────────────────────────────────────────────────────────
  private isDrawing = false;
  private startX = 0;
  private startY = 0;

  // ── Select / move / resize state ────────────────────────────────────────────
  private selected: Annotation | null = null;
  private dragging = false;
  private resizing = false;
  private resizeHandle: ResizeHandle | null = null;
  /** PDF-coord anchor for the FIXED edge during resize */
  private resizeAnchorX = 0;
  private resizeAnchorY = 0;
  private dragTarget: Annotation | null = null;
  private dragOrigX = 0;
  private dragOrigY = 0;
  private dragOrigW = 0;
  private dragOrigH = 0;
  private dragOffsetX = 0;
  private dragOffsetY = 0;
  /** PDF coords of the mouse at drag-start — used for delta-based dragging. */
  private dragStartPdfX = 0;
  private dragStartPdfY = 0;

  // ── Page state ──────────────────────────────────────────────────────────────
  private currentPage = 1;
  private pageHeightPt = 841;
  private scale = 1.5;
  private viewport: PdfViewport | null = null;

  // ── Callbacks ───────────────────────────────────────────────────────────────
  private createdHandlers:      AnnotationCreatedHandler[]      = [];
  private movedHandlers:        AnnotationMovedHandler[]        = [];
  private removedHandlers:      AnnotationRemovedHandler[]      = [];
  private reorderHandlers:      AnnotationReorderHandler[]      = [];
  private textSelectedHandlers:  TextAnnotationSelectedHandler[]  = [];
  private shapeSelectedHandlers: ShapeAnnotationSelectedHandler[] = [];

  /** Called on hover when no annotation is hit; return the desired cursor string (e.g. "pointer") or "" for default. */
  onHoverCursor: ((offsetX: number, offsetY: number) => string) | null = null;

  private committed: Annotation[] = [];

  // ── Before-modify callbacks ──────────────────────────────────────────────────
  private beforeModifyHandlers: (() => void)[] = [];

  constructor(canvas: HTMLCanvasElement, container: HTMLElement, initialStyle: ActiveToolState) {
    this.style = { ...initialStyle };
    this.canvas = canvas;
    this.container = container;
    this.ctx = this.canvas.getContext("2d")!;

    this.canvas.addEventListener("mousedown",    this.onMouseDown);
    this.canvas.addEventListener("mousemove",    this.onMouseMove);
    this.canvas.addEventListener("mouseup",      this.onMouseUp);
    this.canvas.addEventListener("mouseleave",   this.onMouseLeave);
    this.canvas.addEventListener("dblclick",     this.onDblClick);
    this.canvas.addEventListener("contextmenu",  this.onContextMenu);
    window.addEventListener("keydown", this.onKeyDown);

    // In select mode the canvas has pointer-events:none by default so text
    // selection works.  We listen on the container (always receives
    // events) to detect when the cursor is over an annotation and temporarily
    // flip pointer-events:auto so the canvas can handle clicks/drags.
    container.addEventListener("mousemove", this.onContainerMouseMove);
    container.addEventListener("mousedown", this.onContainerMouseDown);

    this.applyPointerEvents(this.activeTool);
  }

  onAnnotationCreated    (h: AnnotationCreatedHandler):      void { this.createdHandlers.push(h); }
  onAnnotationMoved      (h: AnnotationMovedHandler):        void { this.movedHandlers.push(h); }
  onAnnotationRemoved    (h: AnnotationRemovedHandler):      void { this.removedHandlers.push(h); }
  onAnnotationReordered  (h: AnnotationReorderHandler):      void { this.reorderHandlers.push(h); }
  onTextAnnotationSelected (h: TextAnnotationSelectedHandler):  void { this.textSelectedHandlers.push(h); }
  onShapeAnnotationSelected(h: ShapeAnnotationSelectedHandler): void { this.shapeSelectedHandlers.push(h); }

  /** Register a callback invoked just before any annotation is mutated (move,
   *  resize, delete, style change, text edit). Use to snapshot history. */
  onBeforeModify(h: () => void): void { this.beforeModifyHandlers.push(h); }
  private emitBeforeModify(): void { this.beforeModifyHandlers.forEach(h => h()); }

  private emit             (a: Annotation): void { this.createdHandlers.forEach(h => h(a)); }
  private emitMoved        (a: Annotation): void { this.movedHandlers.forEach(h => h(a)); }
  private emitRemoved      (a: Annotation): void { this.removedHandlers.forEach(h => h(a)); }
  private emitReordered    (a: Annotation, dir: "front" | "back"): void {
    this.reorderHandlers.forEach(h => h(a, dir));
  }
  private emitTextSelected  (a: TextAnnotation | null):                void { this.textSelectedHandlers.forEach(h => h(a)); }
  private emitShapeSelected (a: RectAnnotation | CircleAnnotation | null): void { this.shapeSelectedHandlers.forEach(h => h(a)); }

  get currentTool(): ToolKind { return this.activeTool; }

  /** Apply shape style fields to an existing rect/circle and redraw. */
  applyShapeAnnotationStyle(ann: RectAnnotation | CircleAnnotation, color: RgbColor, strokeWidth: number): void {
    this.emitBeforeModify();
    ann.color = { ...color };
    ann.strokeWidth = strokeWidth;
    this.redrawCommitted();
    this.emitMoved(ann);
  }

  /** Apply text style fields to an existing annotation and redraw. */
  applyTextAnnotationStyle(
    ann: TextAnnotation,
    style: Pick<ActiveToolState, "color" | "fontSize" | "bold" | "italic" | "underline" | "alignment">
  ): void {
    this.emitBeforeModify();
    ann.color     = { ...style.color };
    ann.fontSize  = style.fontSize;
    ann.bold      = style.bold;
    ann.italic    = style.italic;
    ann.underline = style.underline;
    ann.alignment = style.alignment;
    this.redrawCommitted();
    this.emitMoved(ann);
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
      this.resizing = false;
      this.dragTarget = null;
      this.redrawCommitted();
    }
    this.activeTool = tool;
    this.applyPointerEvents(tool);
    if (tool === "text")         this.canvas.style.cursor = "text";
    else if (tool === "select")  this.canvas.style.cursor = "default";
    else if (tool !== "signature") this.canvas.style.cursor = "crosshair";
    else                         this.canvas.style.cursor = "";
  }

  private applyPointerEvents(tool: ToolKind): void {
    // Annotation canvas only captures events when a draw tool is active.
    // "select" and "signature" keep pointer-events:none so the text layer
    // (below in z-index) receives events and text can be selected.
    const isDrawTool = tool === "rect" || tool === "circle" || tool === "text";
    this.canvas.style.pointerEvents = isDrawTool ? "auto" : "none";
  }

  /** Convert PDF user-space point → canvas pixel coords (handles rotation via viewport). */
  private toCanvas(pdfX: number, pdfY: number): [number, number] {
    if (this.viewport) {
      const r = this.viewport.convertToViewportPoint(pdfX, pdfY);
      return [r[0], r[1]];
    }
    const c = pdfToCanvas(pdfX, pdfY, this.scale, this.pageHeightPt);
    return [c.x, c.y];
  }

  /** Convert canvas pixel coords → PDF user-space point (handles rotation via viewport). */
  private toPdf(canvasX: number, canvasY: number): [number, number] {
    if (this.viewport) {
      const r = this.viewport.convertToPdfPoint(canvasX, canvasY);
      return [r[0], r[1]];
    }
    const p = canvasToPdf(canvasX, canvasY, this.scale, this.pageHeightPt);
    return [p.x, p.y];
  }

  syncToPage(page: number, scale: number, pageHeightPt: number, annotations: Annotation[], viewport?: PdfViewport): void {
    this.currentPage  = page;
    this.scale        = scale;
    this.pageHeightPt = pageHeightPt;
    this.viewport     = viewport ?? null;
    this.selected = null;
    this.dragging = false;
    this.resizing = false;
    this.dragTarget = null;

    this.committed = [...annotations];
    this.redrawCommitted();
    // Always reset pointer-events to the correct state for the current tool.
    // Without this, if the canvas was left with pointer-events:auto (from a draw
    // mode or annotation hover) before loading a new PDF, form inputs below the
    // canvas would be unclickable.
    this.applyPointerEvents(this.activeTool);
  }

  // ── Bounding box ─────────────────────────────────────────────────────────────

  private getAnnBounds(ann: Annotation): { left: number; top: number; right: number; bottom: number } {
    const { scale } = this;
    if (ann.kind === "text") {
      // ann.y is the baseline. Compute the canvas position of the baseline.
      const [bx, by] = this.toCanvas(ann.x, ann.y);
      const [bxRight] = this.toCanvas(ann.x + ann.width, ann.y);
      const left   = Math.min(bx, bxRight);
      const right  = Math.max(bx, bxRight);
      const lineH  = ann.fontSize * scale * 1.2;
      const lineCount = this.textLineCount(ann);
      const top    = by - 2 - ann.fontSize * scale;
      const bottom = top + lineCount * lineH + 4;
      return { left, top, right, bottom };
    } else {
      // Convert the two opposite PDF corners and take the axis-aligned bounding box.
      const [x1, y1] = this.toCanvas(ann.x, ann.y + ann.height);            // PDF top-left
      const [x2, y2] = this.toCanvas(ann.x + ann.width, ann.y);             // PDF bottom-right
      return {
        left:   Math.min(x1, x2),
        top:    Math.min(y1, y2),
        right:  Math.max(x1, x2),
        bottom: Math.max(y1, y2),
      };
    }
  }

  private hitTest(cx: number, cy: number): Annotation | null {
    const P = 5;
    for (let i = this.committed.length - 1; i >= 0; i--) {
      const b = this.getAnnBounds(this.committed[i]);
      if (cx >= b.left - P && cx <= b.right  + P &&
          cy >= b.top  - P && cy <= b.bottom + P) return this.committed[i];
    }
    return null;
  }

  // ── Resize handles ───────────────────────────────────────────────────────────

  /** Returns canvas-px positions of all handles for a selection box. */
  private handlePositions(b: { left: number; top: number; right: number; bottom: number }): Record<ResizeHandle, { x: number; y: number }> {
    const mx = (b.left + b.right)  / 2;
    const my = (b.top  + b.bottom) / 2;
    return {
      nw: { x: b.left,  y: b.top    }, n: { x: mx,     y: b.top    }, ne: { x: b.right, y: b.top    },
      e:  { x: b.right, y: my       },                                  w: { x: b.left,  y: my       },
      sw: { x: b.left,  y: b.bottom }, s: { x: mx,     y: b.bottom }, se: { x: b.right, y: b.bottom },
    };
  }

  private handlesFor(ann: Annotation): ResizeHandle[] {
    return ann.kind === "text"
      ? ["w", "e"]
      : ["nw", "n", "ne", "e", "se", "s", "sw", "w"];
  }

  private hitHandle(cx: number, cy: number, ann: Annotation): ResizeHandle | null {
    const SEL_PAD = 5;
    const b = this.getAnnBounds(ann);
    const box = { left: b.left - SEL_PAD, top: b.top - SEL_PAD, right: b.right + SEL_PAD, bottom: b.bottom + SEL_PAD };
    const positions = this.handlePositions(box);
    for (const h of this.handlesFor(ann)) {
      const p = positions[h];
      if (Math.abs(cx - p.x) <= HANDLE_HIT && Math.abs(cy - p.y) <= HANDLE_HIT) return h;
    }
    return null;
  }

  /** Compute fixed-edge anchors in PDF pts for the given handle. */
  private startResize(ann: Annotation, handle: ResizeHandle): void {
    this.resizing      = true;
    this.resizeHandle  = handle;
    this.dragTarget    = ann;
    this.dragOrigX     = ann.x;
    this.dragOrigY     = ann.y;
    this.dragOrigW     = ann.width;
    this.dragOrigH     = ann.kind !== "text" ? ann.height : 0;

    // anchorX = the FIXED x edge (left or right) in PDF pts
    const left   = ann.x;
    const right  = ann.x + ann.width;
    const bottom = ann.y;
    const top    = ann.kind !== "text" ? ann.y + ann.height : ann.y;

    if (["e", "ne", "se"].includes(handle)) this.resizeAnchorX = left;
    else if (["w", "nw", "sw"].includes(handle)) this.resizeAnchorX = right;

    // anchorY = the FIXED y edge (top or bottom) in PDF pts
    if (["n", "ne", "nw"].includes(handle)) this.resizeAnchorY = bottom;
    else if (["s", "se", "sw"].includes(handle)) this.resizeAnchorY = top;
  }

  private applyResize(mouseX: number, mouseY: number): void {
    const ann = this.dragTarget!;
    const h   = this.resizeHandle!;
    const [mx, my] = this.toPdf(mouseX, mouseY);
    const M   = MIN_SIZE_PT;

    if (h === "e" || h === "ne" || h === "se") {
      ann.width = Math.max(M, mx - this.resizeAnchorX);
    }
    if (h === "w" || h === "nw" || h === "sw") {
      const newLeft = Math.min(this.resizeAnchorX - M, mx);
      ann.width = this.resizeAnchorX - newLeft;
      ann.x = newLeft;
    }
    if (ann.kind !== "text") {
      if (h === "n" || h === "ne" || h === "nw") {
        ann.height = Math.max(M, my - this.resizeAnchorY);
      }
      if (h === "s" || h === "se" || h === "sw") {
        const newBottom = Math.min(this.resizeAnchorY - M, my);
        ann.y      = newBottom;
        ann.height = this.resizeAnchorY - newBottom;
      }
    }
  }

  // ── Redraw ───────────────────────────────────────────────────────────────────

  redrawCommitted(): void {
    this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    for (const ann of this.committed) {
      this.drawAnnotation(ann);
    }
    if (this.selected && this.committed.includes(this.selected)) {
      this.drawSelectionBox(this.selected);
    }
  }

  private drawSelectionBox(ann: Annotation): void {
    const SEL_PAD = 5;
    const b = this.getAnnBounds(ann);
    const box = { left: b.left - SEL_PAD, top: b.top - SEL_PAD, right: b.right + SEL_PAD, bottom: b.bottom + SEL_PAD };

    this.ctx.strokeStyle = "#4d9eff";
    this.ctx.lineWidth   = 1.5;
    this.ctx.setLineDash([5, 3]);
    this.ctx.strokeRect(box.left, box.top, box.right - box.left, box.bottom - box.top);
    this.ctx.setLineDash([]);

    // Draw resize handles
    const positions = this.handlePositions(box);
    this.ctx.fillStyle   = "#fff";
    this.ctx.strokeStyle = "#4d9eff";
    this.ctx.lineWidth   = 1;
    for (const h of this.handlesFor(ann)) {
      const { x, y } = positions[h];
      this.ctx.fillRect  (x - HANDLE_R, y - HANDLE_R, HANDLE_R * 2, HANDLE_R * 2);
      this.ctx.strokeRect(x - HANDLE_R, y - HANDLE_R, HANDLE_R * 2, HANDLE_R * 2);
    }
  }

  // ── Annotation drawing ───────────────────────────────────────────────────────

  /** Compute the number of rendered lines for a text annotation (accounts for word-wrap). */
  private textLineCount(ann: TextAnnotation): number {
    const { scale } = this;
    const saved = this.ctx.font;
    this.ctx.font = `${ann.italic ? "italic" : "normal"} ${ann.bold ? "bold" : "normal"} ${ann.fontSize * scale}px Helvetica, Arial, sans-serif`;
    const count = this.wrapLines(this.ctx, ann.content, ann.width * scale).length;
    this.ctx.font = saved;
    return count;
  }

  /** Split text on explicit newlines then word-wrap each paragraph to maxWidth. */
  private wrapLines(ctx: CanvasRenderingContext2D, text: string, maxWidth: number): string[] {
    const result: string[] = [];
    for (const para of text.split("\n")) {
      if (!para) { result.push(""); continue; }
      const words = para.split(" ");
      let line = "";
      for (const word of words) {
        const candidate = line ? `${line} ${word}` : word;
        if (line && ctx.measureText(candidate).width > maxWidth) {
          result.push(line);
          line = word;
        } else {
          line = candidate;
        }
      }
      result.push(line);
    }
    return result;
  }

  private drawAnnotation(ann: Annotation): void {
    const ctx = this.ctx;
    const { scale } = this;

    if (ann.kind === "rect") {
      const [x1, y1] = this.toCanvas(ann.x, ann.y + ann.height); // PDF top-left
      const [x2, y2] = this.toCanvas(ann.x + ann.width, ann.y);  // PDF bottom-right
      ctx.strokeStyle = rgbToCss(ann.color);
      ctx.lineWidth   = ann.strokeWidth;
      ctx.strokeRect(Math.min(x1, x2), Math.min(y1, y2), Math.abs(x2 - x1), Math.abs(y2 - y1));

    } else if (ann.kind === "circle") {
      const [x1, y1] = this.toCanvas(ann.x, ann.y + ann.height);
      const [x2, y2] = this.toCanvas(ann.x + ann.width, ann.y);
      const cx = (x1 + x2) / 2;
      const cy = (y1 + y2) / 2;
      ctx.strokeStyle = rgbToCss(ann.color);
      ctx.lineWidth   = ann.strokeWidth;
      ctx.beginPath();
      ctx.ellipse(cx, cy, Math.abs(x2 - x1) / 2, Math.abs(y2 - y1) / 2, 0, 0, Math.PI * 2);
      ctx.stroke();

    } else if (ann.kind === "text") {
      const [x, y] = this.toCanvas(ann.x, ann.y);
      ctx.fillStyle = rgbToCss(ann.color);
      ctx.font      = `${ann.italic ? "italic" : "normal"} ${ann.bold ? "bold" : "normal"} ${ann.fontSize * scale}px Helvetica, Arial, sans-serif`;
      ctx.textAlign = ann.alignment as CanvasTextAlign;
      const lineH   = ann.fontSize * scale * 1.2;
      const maxW    = ann.width * scale;
      const tx      = ann.alignment === "left" ? x : ann.alignment === "right" ? x + maxW : x + maxW / 2;
      const lines   = this.wrapLines(ctx, ann.content, maxW);
      lines.forEach((line, i) => {
        const ly = y + i * lineH;
        ctx.fillText(line, tx, ly);
        if (ann.underline) {
          const w      = ctx.measureText(line).width;
          const ulY    = ly + ann.fontSize * scale * 0.12;
          const startX = ann.alignment === "left" ? x : ann.alignment === "right" ? tx - w : tx - w / 2;
          ctx.strokeStyle = rgbToCss(ann.color);
          ctx.lineWidth   = 1;
          ctx.beginPath();
          ctx.moveTo(startX, ulY);
          ctx.lineTo(startX + w, ulY);
          ctx.stroke();
        }
      });

    } else if (ann.kind === "signature") {
      const [x1, y1] = this.toCanvas(ann.x,             ann.y + ann.height); // canvas top-left
      const [x2, y2] = this.toCanvas(ann.x + ann.width, ann.y);              // canvas bottom-right
      const img = new Image();
      img.onload = () => this.ctx.drawImage(img,
        Math.min(x1, x2), Math.min(y1, y2), Math.abs(x2 - x1), Math.abs(y2 - y1));
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
    ctx.lineWidth   = this.style.strokeWidth;
    ctx.setLineDash([4, 3]);
    if (this.activeTool === "rect") {
      ctx.strokeRect(x0, y0, w, h);
    } else if (this.activeTool === "circle") {
      ctx.beginPath();
      ctx.ellipse(x0 + w / 2, y0 + h / 2, w / 2, h / 2, 0, 0, Math.PI * 2);
      ctx.stroke();
    } else if (this.activeTool === "text") {
      ctx.strokeStyle = "#aaaaaa";
      ctx.lineWidth   = 1;
      const lineH = this.style.fontSize * this.scale * 1.4;
      ctx.strokeRect(x0, y0, w, lineH);
    }
    ctx.setLineDash([]);
  }

  // ── Mouse handlers ────────────────────────────────────────────────────────────

  /** Runs on the parent viewer-container (always receives events regardless of
   *  canvas pointer-events).  In select mode, enables pointer-events on the
   *  canvas only when the cursor is actually over an annotation or resize handle,
   *  keeping text selection available for the rest of the page. */
  private onContainerMouseMove = (e: MouseEvent): void => {
    if (this.activeTool !== "select") return;
    if (this.dragging || this.resizing) return; // keep state stable mid-drag

    const rect = this.canvas.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;

    const hitAnn = this.hitTest(cx, cy);
    const hitH   = this.selected ? this.hitHandle(cx, cy, this.selected) : null;
    const over   = hitAnn !== null || hitH !== null;

    this.canvas.style.pointerEvents = over ? "auto" : "none";
    this.canvas.style.cursor = over
      ? (hitH ? HANDLE_CURSORS[hitH] : "grab")
      : "default";
  };

  private onContainerMouseDown = (e: MouseEvent): void => {
    if (this.activeTool !== "select") return;
    if (this.dragging || this.resizing) return;
    if (!this.selected) return;

    const rect = this.canvas.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;

    const hitAnn = this.hitTest(cx, cy);
    const hitH   = this.hitHandle(cx, cy, this.selected);
    if (!hitAnn && !hitH) {
      this.selected = null;
      this.redrawCommitted();
      this.emitTextSelected(null);
      this.emitShapeSelected(null);
    }
  };

  private onMouseDown = (e: MouseEvent): void => {
    if (this.activeTool === "signature") return;

    if (this.activeTool === "select") {
      // 1. Check resize handle on selected annotation first
      if (this.selected) {
        const h = this.hitHandle(e.offsetX, e.offsetY, this.selected);
        if (h) {
          this.emitBeforeModify();
          this.startResize(this.selected, h);
          this.canvas.style.cursor = HANDLE_CURSORS[h];
          return;
        }
      }
      // 2. Check for annotation hit (drag/select)
      const hit = this.hitTest(e.offsetX, e.offsetY);
      this.selected = hit;
      if (hit) {
        this.emitBeforeModify();
        const bounds = this.getAnnBounds(hit);
        this.dragging    = true;
        this.dragTarget  = hit;
        this.dragOrigX   = hit.x;
        this.dragOrigY   = hit.y;
        this.dragOrigW   = hit.width;
        this.dragOrigH   = hit.kind !== "text" ? hit.height : 0;
        this.dragOffsetX = e.offsetX - bounds.left;
        this.dragOffsetY = e.offsetY - bounds.top;
        [this.dragStartPdfX, this.dragStartPdfY] = this.toPdf(e.offsetX, e.offsetY);
        this.canvas.style.cursor = "grabbing";
      }
      this.redrawCommitted();
      this.emitTextSelected(hit?.kind === "text" ? hit : null);
      this.emitShapeSelected((hit?.kind === "rect" || hit?.kind === "circle") ? hit : null);
      return;
    }

    if (this.activeTool === "text") {
      e.preventDefault();
      this.isDrawing = true;
      this.startX = e.offsetX;
      this.startY = e.offsetY;
      return;
    }

    this.isDrawing = true;
    this.startX = e.offsetX;
    this.startY = e.offsetY;
  };

  private onMouseMove = (e: MouseEvent): void => {
    if (this.activeTool === "select") {
      if (this.resizing && this.dragTarget) {
        this.applyResize(e.offsetX, e.offsetY);
        this.redrawCommitted();
        return;
      }
      if (this.dragging && this.dragTarget) {
        const [curPdfX, curPdfY] = this.toPdf(e.offsetX, e.offsetY);
        this.dragTarget.x = this.dragOrigX + (curPdfX - this.dragStartPdfX);
        this.dragTarget.y = this.dragOrigY + (curPdfY - this.dragStartPdfY);
        this.redrawCommitted();
        return;
      }
      // Hover: check handle cursor, then grab cursor
      if (this.selected) {
        const h = this.hitHandle(e.offsetX, e.offsetY, this.selected);
        if (h) { this.canvas.style.cursor = HANDLE_CURSORS[h]; return; }
      }
      if (this.hitTest(e.offsetX, e.offsetY)) {
        this.canvas.style.cursor = "grab";
      } else {
        const override = this.onHoverCursor?.(e.offsetX, e.offsetY);
        this.canvas.style.cursor = override || "default";
      }
      return;
    }

    if (!this.isDrawing) return;
    this.drawLivePreview(e.offsetX, e.offsetY);
  };

  private onMouseUp = (e: MouseEvent): void => {
    if (this.activeTool === "select") {
      if (this.resizing && this.dragTarget) {
        this.emitMoved(this.dragTarget);
        this.resizing     = false;
        this.resizeHandle = null;
        this.dragTarget   = null;
        this.canvas.style.cursor = "default";
        return;
      }
      if (this.dragging && this.dragTarget) {
        const moved = this.dragTarget.x !== this.dragOrigX || this.dragTarget.y !== this.dragOrigY;
        if (moved) this.emitMoved(this.dragTarget);
        this.dragging   = false;
        this.dragTarget = null;
        this.canvas.style.cursor = "grab";
      }
      return;
    }

    if (!this.isDrawing) return;
    this.isDrawing = false;

    const x0 = Math.min(this.startX, e.offsetX);
    const y0 = Math.min(this.startY, e.offsetY);
    const w  = Math.abs(e.offsetX - this.startX);
    const h  = Math.abs(e.offsetY - this.startY);
    if (this.activeTool === "text") {
      this.redrawCommitted();
      const isDrag = w >= 8;
      if (isDrag) {
        this.openTextInput(x0, y0, w);
      } else {
        this.openTextInput(
          this.startX,
          this.startY - this.style.fontSize * this.scale,
          0
        );
      }
      return;
    }

    if (w < 4 || h < 4) { this.redrawCommitted(); return; }

    const [p1x, p1y] = this.toPdf(x0,     y0    );
    const [p2x, p2y] = this.toPdf(x0 + w, y0 + h);
    const px = Math.min(p1x, p2x), py = Math.min(p1y, p2y);
    const pw = Math.abs(p2x - p1x), ph = Math.abs(p2y - p1y);

    if (this.activeTool === "rect") {
      const ann: RectAnnotation = {
        kind: "rect", page: this.currentPage,
        x: px, y: py, width: pw, height: ph,
        color: { ...this.style.color }, strokeWidth: this.style.strokeWidth,
      };
      this.committed.push(ann);
      this.redrawCommitted();
      this.emit(ann);
    } else if (this.activeTool === "circle") {
      const ann: CircleAnnotation = {
        kind: "circle", page: this.currentPage,
        x: px, y: py, width: pw, height: ph,
        color: { ...this.style.color }, strokeWidth: this.style.strokeWidth,
      };
      this.committed.push(ann);
      this.redrawCommitted();
      this.emit(ann);
    }
  };

  private onMouseLeave = (): void => {
    if ((this.resizing || this.dragging) && this.dragTarget) {
      // Restore original geometry
      this.dragTarget.x     = this.dragOrigX;
      this.dragTarget.y     = this.dragOrigY;
      this.dragTarget.width = this.dragOrigW;
      if (this.dragTarget.kind !== "text") this.dragTarget.height = this.dragOrigH;
      this.resizing     = false;
      this.resizeHandle = null;
      this.dragging     = false;
      this.dragTarget   = null;
      this.redrawCommitted();
      return;
    }
    if (this.isDrawing) {
      this.isDrawing = false;
      this.redrawCommitted();
    }
  };

  private onKeyDown = (e: KeyboardEvent): void => {
    // Don't intercept keys while the user is typing in a form field
    const tag = (document.activeElement as HTMLElement)?.tagName ?? "";
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" ||
        (document.activeElement as HTMLElement)?.isContentEditable) return;
    if ((e.key === "Delete" || e.key === "Backspace") &&
        this.selected && this.activeTool === "select") {
      this.emitBeforeModify();
      const idx = this.committed.indexOf(this.selected);
      if (idx !== -1) this.committed.splice(idx, 1);
      const removed = this.selected;
      this.selected = null;
      this.redrawCommitted();
      this.emitRemoved(removed);
    }
  };

  // ── Text placement ───────────────────────────────────────────────────────────

  /**
   * @param left    canvas-px left edge of the text box
   * @param top     canvas-px top edge of the text box
   * @param widthPx drawn width in canvas-px; 0 = auto (default size)
   */
  private openTextInput(left: number, top: number, widthPx: number): void {
    const container = this.container;
    const fontSize  = this.style.fontSize * this.scale;
    const hasWidth  = widthPx > 0;

    const input = document.createElement("div");
    input.contentEditable = "true";
    input.style.cssText = `
      position: absolute;
      left: ${left}px; top: ${top}px;
      ${hasWidth ? `width: ${widthPx}px;` : "min-width: 120px;"}
      font-size: ${fontSize}px; line-height: 1.2; font-family: Helvetica, Arial, sans-serif;
      font-weight: ${this.style.bold ? "bold" : "normal"};
      font-style: ${this.style.italic ? "italic" : "normal"};
      text-decoration: ${this.style.underline ? "underline" : "none"};
      text-align: ${this.style.alignment};
      color: ${rgbToCss(this.style.color)};
      outline: 1px dashed #aaa; background: rgba(0,0,0,0.08);
      padding: 2px 4px; white-space: pre-wrap; z-index: 20;
    `;

    const commit = (): void => {
      if (!input.isConnected) return;
      const content = (input.innerText ?? input.textContent ?? "").trim();
      input.remove();
      document.removeEventListener("mousedown", outsideClick, true);
      if (content) {
        // baseline = top of box + top padding (2px) + font size
        const baselineCanvasY = top + 2 + fontSize;
        const [pdfX, pdfY] = this.toPdf(left, baselineCanvasY);
        const annWidth = hasWidth
          ? widthPx / this.scale
          : content.length * this.style.fontSize * 0.55 + 10;
        const ann: TextAnnotation = {
          kind: "text", page: this.currentPage,
          x: pdfX, y: pdfY,
          width: annWidth,
          content,
          color: { ...this.style.color },
          fontSize: this.style.fontSize,
          bold: this.style.bold, italic: this.style.italic, underline: this.style.underline,
          alignment: this.style.alignment,
        };
        this.committed.push(ann);
        this.redrawCommitted();
        this.emit(ann);
      }
    };

    const outsideClick = (ev: MouseEvent): void => {
      if (!input.contains(ev.target as Node)) commit();
    };

    input.addEventListener("keydown", (ev: KeyboardEvent) => {
      if (ev.key === "Enter" && !ev.shiftKey) { ev.preventDefault(); commit(); }
      else if (ev.key === "Escape") {
        input.remove();
        document.removeEventListener("mousedown", outsideClick, true);
      }
    });

    container.appendChild(input);
    input.focus();
    setTimeout(() => document.addEventListener("mousedown", outsideClick, true), 0);
  }

  // ── Double-click edit ─────────────────────────────────────────────────────────

  private onDblClick = (e: MouseEvent): void => {
    if (this.activeTool !== "select") return;
    const hit = this.hitTest(e.offsetX, e.offsetY);
    if (!hit) return;
    e.preventDefault();
    if (hit.kind === "text") this.handleTextEdit(hit);

  };

  private onContextMenu = (e: MouseEvent): void => {
    e.preventDefault(); // suppress browser context menu
  };

  reorderTextAnnotation(ann: Annotation, dir: "front" | "back"): void {
    this.reorderAnnotation(ann, dir);
  }

  private reorderAnnotation(ann: Annotation, dir: "front" | "back"): void {
    const idx = this.committed.indexOf(ann);
    if (idx === -1) return;
    this.committed.splice(idx, 1);
    if (dir === "front") this.committed.push(ann);
    else this.committed.unshift(ann);
    this.redrawCommitted();
    this.emitReordered(ann, dir);
  }

  private handleTextEdit(ann: TextAnnotation): void {
    const container = this.container;
    const { scale } = this;
    const [canvasX, canvasY] = this.toCanvas(ann.x, ann.y); // baseline
    // boxTop = baseline - 2px padding - fontSize (mirrors openTextInput commit logic)
    const boxTop = canvasY - 2 - ann.fontSize * scale;
    const boxW   = ann.width * scale;

    // Hide the canvas rendering while the edit div is visible
    const idx = this.committed.indexOf(ann);
    if (idx !== -1) this.committed.splice(idx, 1);
    this.selected = null;
    this.redrawCommitted();

    const restore = (): void => {
      if (idx !== -1) this.committed.splice(idx, 0, ann);
    };

    const input = document.createElement("div");
    input.contentEditable = "true";
    input.textContent = ann.content;
    input.style.cssText = `
      position: absolute;
      left: ${canvasX}px; top: ${boxTop}px;
      width: ${boxW}px;
      font-size: ${ann.fontSize * scale}px; line-height: 1.2; font-family: Helvetica, Arial, sans-serif;
      font-weight: ${ann.bold ? "bold" : "normal"};
      font-style: ${ann.italic ? "italic" : "normal"};
      text-decoration: ${ann.underline ? "underline" : "none"};
      text-align: ${ann.alignment};
      color: ${rgbToCss(ann.color)};
      outline: 1px dashed #aaa; background: transparent;
      padding: 2px 4px; white-space: pre-wrap; z-index: 10;
    `;
    const commit = (): void => {
      if (!input.isConnected) return;
      const content = (input.innerText ?? input.textContent ?? "").trim();
      input.remove();
      document.removeEventListener("mousedown", outsideClick, true);
      restore();
      if (content && content !== ann.content) {
        this.emitBeforeModify();
        ann.content = content;
        this.emitMoved(ann);
      }
      this.redrawCommitted();
    };
    const outsideClick = (ev: MouseEvent): void => {
      if (!input.contains(ev.target as Node)) commit();
    };
    input.addEventListener("keydown", (ev: KeyboardEvent) => {
      if (ev.key === "Enter" && !ev.shiftKey) { ev.preventDefault(); commit(); }
      else if (ev.key === "Escape") {
        input.remove();
        document.removeEventListener("mousedown", outsideClick, true);
        restore();
        this.redrawCommitted();
      }
    });
    container.appendChild(input);
    input.focus();
    const range = document.createRange();
    range.selectNodeContents(input);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(range);
    setTimeout(() => document.addEventListener("mousedown", outsideClick, true), 0);
  }

  private handleTextStyleEdit(ann: TextAnnotation): void {
    this.selected = ann;
    this.redrawCommitted();
    this.emitTextSelected(ann);
  }

  /** Place a signature image on the current page at canvas coordinates. */
  placeSignature(canvasX: number, canvasY: number, imageData: string, widthPt: number, heightPt: number): SignatureAnnotation {
    const [pdfX, pdfY] = this.toPdf(canvasX, canvasY);
    const ann: SignatureAnnotation = {
      kind: "signature", page: this.currentPage,
      x: pdfX, y: pdfY - heightPt,
      width: widthPt, height: heightPt, imageData,
    };
    this.committed.push(ann);
    this.redrawCommitted();
    return ann;
  }
}
