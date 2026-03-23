import {
  defaultToolState,
  hexToRgb,
  rgbToHex,
  type ActiveToolState,
  type TextAlignmentValue,
  type ToolKind,
} from "./models";

type ToolbarEvent =
  | { type: "open" }
  | { type: "save" }
  | { type: "save-as" }
  | { type: "zoom-in" }
  | { type: "zoom-out" }
  | { type: "page-prev" }
  | { type: "page-next" }
  | { type: "page-goto"; page: number }
  | { type: "tool-change"; tool: ToolKind }
  | { type: "style-change"; style: ActiveToolState }
  | { type: "text-layer"; dir: "front" | "back" }
  | { type: "signature" };

type EventHandler = (e: ToolbarEvent) => void;

export class Toolbar {
  private state: ActiveToolState = defaultToolState();
  private handlers: EventHandler[] = [];

  private el: HTMLElement;
  private pageInput!: HTMLInputElement;
  private pageTotal!: HTMLSpanElement;
  private toolBtns: Partial<Record<ToolKind, HTMLButtonElement>> = {};

  // Text style section
  private textStyleSection!: HTMLElement;
  private colorPicker!: HTMLInputElement;
  private sizeInput!: HTMLInputElement;
  private boldBtn!: HTMLButtonElement;
  private italicBtn!: HTMLButtonElement;
  private underlineBtn!: HTMLButtonElement;
  private alignBtns: HTMLButtonElement[] = [];

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

  private sep(): HTMLElement {
    const d = document.createElement("div");
    d.className = "toolbar-sep";
    return d;
  }

  private btn(label: string, title: string, cls = "btn"): HTMLButtonElement {
    const b = document.createElement("button");
    b.className = cls;
    b.textContent = label;
    b.title = title;
    return b;
  }

  private build(): void {
    // Open
    const openBtn = this.btn("Open", "Open PDF");
    openBtn.addEventListener("click", () => this.emit({ type: "open" }));
    this.el.append(openBtn, this.sep());

    // Page navigation
    const prevBtn = this.btn("◀", "Previous page", "icon-btn");
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

    const nextBtn = this.btn("▶", "Next page", "icon-btn");
    nextBtn.addEventListener("click", () => this.emit({ type: "page-next" }));

    const navWrapper = document.createElement("div");
    navWrapper.className = "page-nav";
    navWrapper.append(prevBtn, this.pageInput, this.pageTotal, nextBtn);
    this.el.append(navWrapper, this.sep());

    // Zoom
    const zoomOut = this.btn("−", "Zoom out", "icon-btn");
    zoomOut.addEventListener("click", () => this.emit({ type: "zoom-out" }));
    const zoomIn = this.btn("+", "Zoom in", "icon-btn");
    zoomIn.addEventListener("click", () => this.emit({ type: "zoom-in" }));
    this.el.append(zoomOut, zoomIn, this.sep());

    // Drawing tools
    const tools: [ToolKind, string, string][] = [
      ["rect", "▭", "Rectangle"],
      ["circle", "○", "Circle"],
      ["text", "T", "Text"],
      ["signature", "✍", "Signature"],
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
      this.el.append(b);
    }

    // Text style section (hidden until text tool or text annotation selected)
    this.buildTextStyleSection();

    this.el.append(this.sep());

    // Save / Save As / Print
    const saveBtn = this.btn("Save", "Save PDF");
    saveBtn.addEventListener("click", () => this.emit({ type: "save" }));
    const saveAsBtn = this.btn("Save As…", "Save PDF as…");
    saveAsBtn.addEventListener("click", () => this.emit({ type: "save-as" }));
    const printBtn = this.btn("Print", "Print PDF", "icon-btn");
    printBtn.textContent = "🖨";
    printBtn.title = "Print";
    printBtn.addEventListener("click", () => window.print());
    this.el.append(saveBtn, saveAsBtn, this.sep(), printBtn);
  }

  private buildTextStyleSection(): void {
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
    this.boldBtn.style.fontWeight       = "700";
    this.italicBtn.style.fontStyle      = "italic";
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
      ["left", "⇐", "Align left"],
      ["center", "⇔", "Align center"],
      ["right", "⇒", "Align right"],
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
    frontBtn.addEventListener("click", () => this.emit({ type: "text-layer", dir: "front" }));
    backBtn.addEventListener ("click", () => this.emit({ type: "text-layer", dir: "back"  }));
    this.textStyleSection.append(frontBtn, backBtn);

    this.el.append(this.textStyleSection);
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

  clearActiveTool(): void {
    this.state.tool = "select";
    for (const btn of Object.values(this.toolBtns)) {
      btn?.classList.remove("active");
    }
    this.emit({ type: "tool-change", tool: "select" });
  }
}
