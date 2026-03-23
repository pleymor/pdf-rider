import {
  defaultToolState,
  hexToRgb,
  rgbToHex,
  type ActiveToolState,
  type ToolKind,
  type TextAlignmentValue,
} from "./models";

type ToolbarEvent =
  | { type: "open" }
  | { type: "save" }
  | { type: "save-as" }
  | { type: "export" }
  | { type: "zoom-in" }
  | { type: "zoom-out" }
  | { type: "page-prev" }
  | { type: "page-next" }
  | { type: "page-goto"; page: number }
  | { type: "tool-change"; tool: ToolKind }
  | { type: "style-change"; style: ActiveToolState }
  | { type: "signature" };

type EventHandler = (e: ToolbarEvent) => void;

export class Toolbar {
  private state: ActiveToolState = defaultToolState();
  private handlers: EventHandler[] = [];

  private el: HTMLElement;
  private pageInput!: HTMLInputElement;
  private pageTotal!: HTMLSpanElement;
  private toolBtns: Partial<Record<ToolKind, HTMLButtonElement>> = {};

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
    this.el.append(this.sep());

    // Colour picker
    const colorPicker = document.createElement("input");
    colorPicker.type = "color";
    colorPicker.className = "color-picker";
    colorPicker.title = "Annotation colour";
    colorPicker.value = rgbToHex(this.state.color);
    colorPicker.addEventListener("input", () => {
      this.state.color = hexToRgb(colorPicker.value);
      this.emit({ type: "style-change", style: this.getStyle() });
    });
    this.el.append(colorPicker);

    // Font size
    const sizeInput = document.createElement("input");
    sizeInput.type = "number";
    sizeInput.className = "toolbar-input";
    sizeInput.title = "Font size";
    sizeInput.value = String(this.state.fontSize);
    sizeInput.min = "6";
    sizeInput.max = "72";
    sizeInput.addEventListener("change", () => {
      const n = parseInt(sizeInput.value, 10);
      if (!isNaN(n) && n > 0) {
        this.state.fontSize = n;
        this.emit({ type: "style-change", style: this.getStyle() });
      }
    });
    this.el.append(sizeInput, this.sep());

    // Bold / Italic / Underline
    const styleToggles: [keyof ActiveToolState, string, string][] = [
      ["bold", "B", "Bold"],
      ["italic", "I", "Italic"],
      ["underline", "U", "Underline"],
    ];
    for (const [key, label, title] of styleToggles) {
      const b = this.btn(label, title, "icon-btn");
      b.style.fontWeight = key === "bold" ? "700" : "400";
      b.style.fontStyle = key === "italic" ? "italic" : "normal";
      b.style.textDecoration = key === "underline" ? "underline" : "none";
      b.addEventListener("click", () => {
        (this.state as Record<string, unknown>)[key as string] =
          !(this.state as Record<string, unknown>)[key as string];
        b.classList.toggle("active");
        this.emit({ type: "style-change", style: this.getStyle() });
      });
      this.el.append(b);
    }
    this.el.append(this.sep());

    // Alignment
    const alignments: [TextAlignmentValue, string][] = [
      ["left", "⇐"],
      ["center", "⇔"],
      ["right", "⇒"],
    ];
    const alignBtns: HTMLButtonElement[] = [];
    for (const [align, icon] of alignments) {
      const b = this.btn(icon, `Align ${align}`, "icon-btn");
      if (align === "left") b.classList.add("active");
      b.addEventListener("click", () => {
        this.state.alignment = align;
        alignBtns.forEach((ab) => ab.classList.remove("active"));
        b.classList.add("active");
        this.emit({ type: "style-change", style: this.getStyle() });
      });
      alignBtns.push(b);
      this.el.append(b);
    }
    this.el.append(this.sep());

    // Save / Save As / Export
    const saveBtn = this.btn("Save", "Save (keeps annotations editable)");
    saveBtn.addEventListener("click", () => this.emit({ type: "save" }));
    const saveAsBtn = this.btn("Save As…", "Save as… (keeps annotations editable)");
    saveAsBtn.addEventListener("click", () => this.emit({ type: "save-as" }));
    const exportBtn = this.btn("Export…", "Export flat PDF (annotations visible in all viewers)");
    exportBtn.addEventListener("click", () => this.emit({ type: "export" }));
    this.el.append(saveBtn, saveAsBtn, exportBtn);
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
    this.emit({ type: "style-change", style: this.getStyle() });
  }

  clearActiveTool(): void {
    this.state.tool = "select";
    for (const btn of Object.values(this.toolBtns)) {
      btn?.classList.remove("active");
    }
    this.emit({ type: "tool-change", tool: "select" });
  }
}
