import {
  defaultToolState,
  hexToRgb,
  rgbToHex,
  type ActiveToolState,
  type TextAlignmentValue,
  type ToolKind,
} from "./models";
import {
  ICON_OPEN_FILE,
  ICON_PAGE_UP,
  ICON_PAGE_DOWN,
  ICON_ZOOM_OUT,
  ICON_ZOOM_IN,
  ICON_EDITOR_FREE_TEXT,
  ICON_EDITOR_INK,
  ICON_EDITOR_SIGNATURE,
  ICON_PRINT,
  ICON_RECT,
  ICON_CIRCLE,
  ICON_ROTATE_CW,
  ICON_ALIGN_LEFT,
  ICON_ALIGN_CENTER,
  ICON_ALIGN_RIGHT,
  ICON_FIT_WIDTH,
  ICON_FIT_HEIGHT,
} from "./icons";

type ToolbarEvent =
  | { type: "open" }
  | { type: "save" }
  | { type: "save-as" }
  | { type: "rotate" }
  | { type: "zoom-in" }
  | { type: "zoom-out" }
  | { type: "fit-width" }
  | { type: "fit-height" }
  | { type: "page-prev" }
  | { type: "page-next" }
  | { type: "page-goto"; page: number }
  | { type: "tool-change"; tool: ToolKind }
  | { type: "style-change"; style: ActiveToolState }
  | { type: "layer-change"; dir: "front" | "back" }
  | { type: "signature" };

type EventHandler = (e: ToolbarEvent) => void;

export class Toolbar {
  private state: ActiveToolState = defaultToolState();
  private handlers: EventHandler[] = [];

  private el: HTMLElement;
  private documentSection!: HTMLElement;
  private annotationSection!: HTMLElement;
  private modeBtn!: HTMLButtonElement;
  private isAnnotationMode = false;
  private pageInput!: HTMLInputElement;
  private pageTotal!: HTMLSpanElement;
  private pageNavSection!: HTMLElement;
  private toolBtns: Partial<Record<ToolKind, HTMLButtonElement>> = {};

  // Text style section
  private textStyleSection!: HTMLElement;
  private colorPicker!: HTMLInputElement;
  private sizeInput!: HTMLInputElement;
  private boldBtn!: HTMLButtonElement;
  private italicBtn!: HTMLButtonElement;
  private underlineBtn!: HTMLButtonElement;
  private alignBtns: HTMLButtonElement[] = [];

  // Shape style section
  private shapeStyleSection!: HTMLElement;
  private shapeColorPicker!: HTMLInputElement;
  private shapeStrokeInput!: HTMLInputElement;

  constructor() {
    this.el = document.getElementById("toolbar-container")!;
    this.build();
  }

  on(handler: EventHandler): void {
    this.handlers.push(handler);
  }

  private emit(e: ToolbarEvent): void {
    this.handlers.forEach((h) => h(e));
  }

  updatePageInfo(current: number, total: number): void {
    if (this.pageInput) this.pageInput.value = String(current);
    if (this.pageTotal) this.pageTotal.textContent = `/ ${total}`;
    if (this.pageNavSection) {
      this.pageNavSection.style.display = total >= 2 ? "flex" : "none";
    }
  }

  setLoaded(loaded: boolean): void {
    this.documentSection.style.display = loaded ? "contents" : "none";
  }

  getStyle(): ActiveToolState {
    return { ...this.state };
  }

  showTextStyles(state: Pick<ActiveToolState, "color" | "fontSize" | "bold" | "italic" | "underline" | "alignment">): void {
    this.state.color     = { ...state.color };
    this.state.fontSize  = state.fontSize;
    this.state.bold      = state.bold;
    this.state.italic    = state.italic;
    this.state.underline = state.underline;
    this.state.alignment = state.alignment;

    this.colorPicker.value = rgbToHex(state.color);
    this.sizeInput.value   = String(state.fontSize);
    this.boldBtn.classList.toggle("active",      state.bold);
    this.italicBtn.classList.toggle("active",    state.italic);
    this.underlineBtn.classList.toggle("active", state.underline);
    this.alignBtns.forEach(b => b.classList.toggle("active", b.dataset["align"] === state.alignment));

    this.textStyleSection.style.display = "flex";
  }

  hideTextStyles(): void {
    this.textStyleSection.style.display = "none";
  }

  showShapeStyles(state: Pick<ActiveToolState, "color" | "strokeWidth">): void {
    this.state.color       = { ...state.color };
    this.state.strokeWidth = state.strokeWidth;

    this.shapeColorPicker.value = rgbToHex(state.color);
    this.shapeStrokeInput.value = String(state.strokeWidth);

    this.shapeStyleSection.style.display = "flex";
  }

  hideShapeStyles(): void {
    this.shapeStyleSection.style.display = "none";
  }

  private sep(): HTMLElement {
    const d = document.createElement("div");
    d.className = "toolbar-sep";
    return d;
  }

  private btn(label: string, title: string, cls = "btn"): HTMLButtonElement {
    const b = document.createElement("button");
    b.className = cls;
    b.innerHTML = label;
    b.title = title;
    return b;
  }

  private build(): void {
    // Open (always visible)
    const openBtn = this.btn(`${ICON_OPEN_FILE}<span>Open</span>`, "Open PDF");
    openBtn.addEventListener("click", () => this.emit({ type: "open" }));
    this.el.append(openBtn);

    // Everything below requires an open document — hidden until setLoaded(true)
    this.documentSection = document.createElement("div");
    this.documentSection.style.display = "none";
    this.el.append(this.documentSection);
    const d = this.documentSection;

    d.append(this.sep());

    // Page navigation
    const prevBtn = this.btn(ICON_PAGE_UP, "Previous page", "icon-btn");
    prevBtn.addEventListener("click", () => this.emit({ type: "page-prev" }));

    this.pageInput = document.createElement("input");
    this.pageInput.type = "number";
    this.pageInput.className = "toolbar-input";
    this.pageInput.value = "1";
    this.pageInput.min = "1";
    this.pageInput.addEventListener("change", () => {
      const n = parseInt(this.pageInput.value, 10);
      if (!isNaN(n)) this.emit({ type: "page-goto", page: n });
    });

    this.pageTotal = document.createElement("span");
    this.pageTotal.textContent = "/ –";

    const nextBtn = this.btn(ICON_PAGE_DOWN, "Next page", "icon-btn");
    nextBtn.addEventListener("click", () => this.emit({ type: "page-next" }));

    const navWrapper = document.createElement("div");
    navWrapper.className = "page-nav";
    navWrapper.append(prevBtn, this.pageInput, this.pageTotal, nextBtn);

    this.pageNavSection = document.createElement("div");
    this.pageNavSection.style.cssText = "display:none;align-items:center;gap:4px;";
    this.pageNavSection.append(navWrapper, this.sep());
    d.append(this.pageNavSection);

    // Zoom
    const zoomOut = this.btn(ICON_ZOOM_OUT, "Zoom out", "icon-btn");
    zoomOut.addEventListener("click", () => this.emit({ type: "zoom-out" }));
    const zoomIn = this.btn(ICON_ZOOM_IN, "Zoom in", "icon-btn");
    zoomIn.addEventListener("click", () => this.emit({ type: "zoom-in" }));

    const fitWidthBtn = this.btn(ICON_FIT_WIDTH, "Fit to width", "icon-btn");
    fitWidthBtn.addEventListener("click", () => this.emit({ type: "fit-width" }));
    const fitHeightBtn = this.btn(ICON_FIT_HEIGHT, "Fit to height", "icon-btn");
    fitHeightBtn.addEventListener("click", () => this.emit({ type: "fit-height" }));

    // Rotate
    const rotateBtn = this.btn(ICON_ROTATE_CW, "Rotate 90° clockwise", "icon-btn");
    rotateBtn.addEventListener("click", () => this.emit({ type: "rotate" }));

    d.append(zoomOut, zoomIn, fitWidthBtn, fitHeightBtn, rotateBtn, this.sep());

    // Annotation mode toggle
    this.modeBtn = this.btn("Annoter", "Mode annotation");
    this.modeBtn.addEventListener("click", () => this.toggleMode());
    d.append(this.modeBtn, this.sep());

    // Annotation-only section (hidden in read mode)
    this.annotationSection = document.createElement("div");
    this.annotationSection.style.cssText = "display:none;align-items:center;gap:4px;";
    d.append(this.annotationSection);
    const ann = this.annotationSection;

    // Drawing tools
    const tools: [ToolKind, string, string][] = [
      ["rect",      ICON_RECT,               "Rectangle"],
      ["circle",    ICON_CIRCLE,             "Circle"],
      ["text",      ICON_EDITOR_FREE_TEXT,   "Text"],
      ["signature", ICON_EDITOR_SIGNATURE,   "Signature"],
    ];

    for (const [kind, icon, title] of tools) {
      const b = this.btn(icon, title, "icon-btn");
      b.addEventListener("click", () => {
        if (kind === "signature") {
          this.emit({ type: "signature" });
          return;
        }
        this.setActiveTool(kind);
      });
      this.toolBtns[kind] = b;
      ann.append(b);
    }

    // Contextual style sections (hidden by default)
    this.buildTextStyleSection(ann);
    this.buildShapeStyleSection(ann);

    ann.append(this.sep());

    // Save / Save As / Print
    const saveBtn = this.btn("Save", "Save PDF");
    saveBtn.addEventListener("click", () => this.emit({ type: "save" }));
    const saveAsBtn = this.btn("Save As…", "Save PDF as…");
    saveAsBtn.addEventListener("click", () => this.emit({ type: "save-as" }));
    const printBtn = this.btn(ICON_PRINT, "Print", "icon-btn");
    printBtn.addEventListener("click", () => window.print());
    d.append(saveBtn, saveAsBtn, this.sep(), printBtn);
  }

  private buildTextStyleSection(container: HTMLElement): void {
    this.textStyleSection = document.createElement("div");
    this.textStyleSection.style.cssText = "display:none;align-items:center;gap:4px;";

    this.textStyleSection.append(this.sep());

    // Colour
    this.colorPicker = document.createElement("input");
    this.colorPicker.type = "color";
    this.colorPicker.className = "color-picker";
    this.colorPicker.title = "Text colour";
    this.colorPicker.value = rgbToHex(this.state.color);
    this.colorPicker.addEventListener("input", () => {
      this.state.color = hexToRgb(this.colorPicker.value);
      this.emitStyleChange();
    });
    this.textStyleSection.append(this.colorPicker);

    // Font size
    this.sizeInput = document.createElement("input");
    this.sizeInput.type = "number";
    this.sizeInput.className = "toolbar-input";
    this.sizeInput.title = "Font size";
    this.sizeInput.value = String(this.state.fontSize);
    this.sizeInput.min = "6";
    this.sizeInput.max = "72";
    this.sizeInput.addEventListener("change", () => {
      const n = parseInt(this.sizeInput.value, 10);
      if (!isNaN(n) && n > 0) { this.state.fontSize = n; this.emitStyleChange(); }
    });
    this.textStyleSection.append(this.sizeInput);

    // Bold / Italic / Underline
    this.boldBtn      = this.btn("B", "Bold",      "icon-btn");
    this.italicBtn    = this.btn("I", "Italic",    "icon-btn");
    this.underlineBtn = this.btn("U", "Underline", "icon-btn");
    this.boldBtn.style.fontWeight          = "700";
    this.italicBtn.style.fontStyle         = "italic";
    this.underlineBtn.style.textDecoration = "underline";

    ([
      [this.boldBtn,      "bold"]      as const,
      [this.italicBtn,    "italic"]    as const,
      [this.underlineBtn, "underline"] as const,
    ] as const).forEach(([b, key]) => {
      b.addEventListener("click", () => {
        (this.state[key] as boolean) = !this.state[key];
        b.classList.toggle("active", this.state[key] as boolean);
        this.emitStyleChange();
      });
    });
    this.textStyleSection.append(this.boldBtn, this.italicBtn, this.underlineBtn);

    // Alignment
    const alignDefs: [TextAlignmentValue, string, string][] = [
      ["left", ICON_ALIGN_LEFT, "Align left"],
      ["center", ICON_ALIGN_CENTER, "Align center"],
      ["right", ICON_ALIGN_RIGHT, "Align right"],
    ];
    this.alignBtns = alignDefs.map(([val, icon, title]) => {
      const b = this.btn(icon, title, "icon-btn");
      b.dataset["align"] = val;
      if (val === "left") b.classList.add("active");
      b.addEventListener("click", () => {
        this.state.alignment = val;
        this.alignBtns.forEach(ab => ab.classList.remove("active"));
        b.classList.add("active");
        this.emitStyleChange();
      });
      return b;
    });
    this.textStyleSection.append(...this.alignBtns);

    // Layer order
    const frontBtn = this.btn("↑", "Bring to front", "icon-btn");
    const backBtn  = this.btn("↓", "Send to back",   "icon-btn");
    frontBtn.addEventListener("click", () => this.emit({ type: "layer-change", dir: "front" }));
    backBtn.addEventListener ("click", () => this.emit({ type: "layer-change", dir: "back"  }));
    this.textStyleSection.append(frontBtn, backBtn);

    container.append(this.textStyleSection);
  }

  private buildShapeStyleSection(container: HTMLElement): void {
    this.shapeStyleSection = document.createElement("div");
    this.shapeStyleSection.style.cssText = "display:none;align-items:center;gap:4px;";

    this.shapeStyleSection.append(this.sep());

    // Stroke colour
    this.shapeColorPicker = document.createElement("input");
    this.shapeColorPicker.type = "color";
    this.shapeColorPicker.className = "color-picker";
    this.shapeColorPicker.title = "Stroke colour";
    this.shapeColorPicker.value = rgbToHex(this.state.color);
    this.shapeColorPicker.addEventListener("input", () => {
      this.state.color = hexToRgb(this.shapeColorPicker.value);
      this.emitStyleChange();
    });
    this.shapeStyleSection.append(this.shapeColorPicker);

    // Stroke width
    const strokeLabel = document.createElement("span");
    strokeLabel.textContent = "w:";
    strokeLabel.style.cssText = "color:#aaa;font-size:11px;";

    this.shapeStrokeInput = document.createElement("input");
    this.shapeStrokeInput.type = "number";
    this.shapeStrokeInput.className = "toolbar-input";
    this.shapeStrokeInput.title = "Stroke width";
    this.shapeStrokeInput.value = String(this.state.strokeWidth);
    this.shapeStrokeInput.min = "0.5";
    this.shapeStrokeInput.max = "20";
    this.shapeStrokeInput.step = "0.5";
    this.shapeStrokeInput.addEventListener("change", () => {
      const sw = parseFloat(this.shapeStrokeInput.value);
      if (!isNaN(sw) && sw > 0) { this.state.strokeWidth = sw; this.emitStyleChange(); }
    });
    this.shapeStyleSection.append(strokeLabel, this.shapeStrokeInput);

    // Layer order
    const frontBtn = this.btn("↑", "Bring to front", "icon-btn");
    const backBtn  = this.btn("↓", "Send to back",   "icon-btn");
    frontBtn.addEventListener("click", () => this.emit({ type: "layer-change", dir: "front" }));
    backBtn.addEventListener ("click", () => this.emit({ type: "layer-change", dir: "back"  }));
    this.shapeStyleSection.append(frontBtn, backBtn);

    container.append(this.shapeStyleSection);
  }

  private emitStyleChange(): void {
    this.emit({ type: "style-change", style: this.getStyle() });
  }

  private setActiveTool(tool: ToolKind): void {
    this.state.tool = tool;
    for (const [kind, btn] of Object.entries(this.toolBtns) as [
      ToolKind,
      HTMLButtonElement,
    ][]) {
      btn.classList.toggle("active", kind === tool);
    }
    this.emit({ type: "tool-change", tool });
  }

  private toggleMode(): void {
    this.isAnnotationMode = !this.isAnnotationMode;
    this.modeBtn.classList.toggle("active", this.isAnnotationMode);
    this.annotationSection.style.display = this.isAnnotationMode ? "flex" : "none";
    if (!this.isAnnotationMode) {
      this.clearActiveTool();
      this.hideTextStyles();
      this.hideShapeStyles();
    }
  }

  clearActiveTool(): void {
    this.state.tool = "select";
    for (const btn of Object.values(this.toolBtns)) {
      btn?.classList.remove("active");
    }
    this.emit({ type: "tool-change", tool: "select" });
  }
}
