import {
  defaultToolState,
  hexToRgb,
  rgbToHex,
  type ActiveToolState,
  type TextAlignmentValue,
  type ToolKind,
  CSS_UNITS,
} from "./models";
import type { Translations } from "./i18n";
import {
  ICON_PAGE_UP,
  ICON_PAGE_DOWN,
  ICON_ZOOM_OUT,
  ICON_ZOOM_IN,
  ICON_EDITOR_FREE_TEXT,
  ICON_EDITOR_SIGNATURE,
  ICON_RECT,
  ICON_CIRCLE,
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
  | { type: "compress" }
  | { type: "rotate" }
  | { type: "zoom-in" }
  | { type: "zoom-out" }
  | { type: "zoom-set"; scale: number }
  | { type: "fit-width" }
  | { type: "fit-height" }
  | { type: "page-prev" }
  | { type: "page-next" }
  | { type: "page-goto"; page: number }
  | { type: "tool-change"; tool: ToolKind }
  | { type: "style-change"; style: ActiveToolState }
  | { type: "layer-change"; dir: "front" | "back" }
  | { type: "signature" }
  | { type: "pages" }
  | { type: "print" }
  | { type: "settings" }
  | { type: "undo" }
  | { type: "redo" };

type EventHandler = (e: ToolbarEvent) => void;

interface MenuItemDef {
  label: keyof Translations;
  shortcut?: string;
  action: () => void;
  requiresDoc?: boolean;
}

interface MenuDef {
  id: string;
  label: keyof Translations;
  items: (MenuItemDef | null)[]; // null = separator
}

export class Toolbar {
  private state: ActiveToolState = defaultToolState();
  private handlers: EventHandler[] = [];

  private el: HTMLElement;
  private menuBarEl: HTMLElement;
  private contextEl: HTMLElement;

  // Menu bar state
  private activeMenuId: string | null = null;
  private dropdowns = new Map<string, HTMLElement>();
  private menuButtons = new Map<string, HTMLButtonElement>();
  private docMenuEntries: HTMLButtonElement[] = [];

  // Context toolbar elements
  private annotationSection!: HTMLElement;
  private modeBtn!: HTMLButtonElement;
  private isAnnotationMode = false;
  private pageInput!: HTMLInputElement;
  private pageTotal!: HTMLSpanElement;
  private pageNavSection!: HTMLElement;
  private zoomInput!: HTMLInputElement;
  private fitWidthBtn!: HTMLButtonElement;
  private fitHeightBtn!: HTMLButtonElement;
  private _lastScale = CSS_UNITS;
  private toolBtns: Partial<Record<ToolKind, HTMLButtonElement>> = {};

  private _i18nText  = new Map<HTMLElement, keyof Translations>();
  private _i18nTitle = new Map<HTMLElement, keyof Translations>();

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
    this.menuBarEl = document.getElementById("menubar-row")!;
    this.contextEl = document.getElementById("context-toolbar")!;
    this.buildMenuBar();
    this.buildContextToolbar();
    this.setupMenuInteraction();
  }

  on(handler: EventHandler): void {
    this.handlers.push(handler);
  }

  applyTranslations(t: Translations): void {
    this._i18nText.forEach((key, el)  => { el.textContent = t[key] ?? null; });
    this._i18nTitle.forEach((key, el) => { el.title = t[key] ?? ""; });
  }

  private reg(el: HTMLElement, text?: keyof Translations, title?: keyof Translations): void {
    if (text)  this._i18nText.set(el, text);
    if (title) this._i18nTitle.set(el, title);
  }

  private emit(e: ToolbarEvent): void {
    this.handlers.forEach((h) => h(e));
  }

  updateZoom(scale: number): void {
    this._lastScale = scale;
    if (this.zoomInput && document.activeElement !== this.zoomInput) {
      this.zoomInput.value = `${Math.round(scale / CSS_UNITS * 100)}%`;
    }
  }

  updateFitMode(mode: "none" | "fit-width" | "fit-height"): void {
    this.fitWidthBtn.classList.toggle("active", mode === "fit-width");
    this.fitHeightBtn.classList.toggle("active", mode === "fit-height");
  }

  updatePageInfo(current: number, total: number): void {
    if (this.pageInput) this.pageInput.value = String(current);
    if (this.pageTotal) this.pageTotal.textContent = `/ ${total}`;
    if (this.pageNavSection) {
      this.pageNavSection.style.display = total >= 2 ? "flex" : "none";
    }
  }

  setLoaded(loaded: boolean): void {
    this.contextEl.style.display = loaded ? "flex" : "none";
    for (const btn of this.docMenuEntries) {
      btn.disabled = !loaded;
    }
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

  // ── Menu Bar ────────────────────────────────────────────────────────────────

  private getMenuDefs(): MenuDef[] {
    return [
      {
        id: "file",
        label: "menuFile",
        items: [
          { label: "btnOpen", shortcut: "Ctrl+O", action: () => this.emit({ type: "open" }) },
          null,
          { label: "btnSave", shortcut: "Ctrl+S", action: () => this.emit({ type: "save" }), requiresDoc: true },
          { label: "btnSaveAs", shortcut: "Ctrl+Shift+S", action: () => this.emit({ type: "save-as" }), requiresDoc: true },
          { label: "btnCompress", shortcut: "Ctrl+Shift+E", action: () => this.emit({ type: "compress" }), requiresDoc: true },
          null,
          { label: "menuPrint", shortcut: "Ctrl+P", action: () => this.emit({ type: "print" }), requiresDoc: true },
          null,
          { label: "menuSettings", action: () => this.emit({ type: "settings" }) },
        ],
      },
      {
        id: "edit",
        label: "menuEdit",
        items: [
          { label: "menuUndo", shortcut: "Ctrl+Z", action: () => this.emit({ type: "undo" }), requiresDoc: true },
          { label: "menuRedo", shortcut: "Ctrl+Y", action: () => this.emit({ type: "redo" }), requiresDoc: true },
        ],
      },
      {
        id: "view",
        label: "menuView",
        items: [
          { label: "menuZoomIn", shortcut: "Ctrl++", action: () => this.emit({ type: "zoom-in" }), requiresDoc: true },
          { label: "menuZoomOut", shortcut: "Ctrl+\u2212", action: () => this.emit({ type: "zoom-out" }), requiresDoc: true },
          { label: "menuResetZoom", shortcut: "Ctrl+0", action: () => this.emit({ type: "zoom-set", scale: CSS_UNITS }), requiresDoc: true },
          null,
          { label: "menuFitWidth", action: () => this.emit({ type: "fit-width" }), requiresDoc: true },
          { label: "menuFitHeight", action: () => this.emit({ type: "fit-height" }), requiresDoc: true },
          null,
          { label: "menuRotate", action: () => this.emit({ type: "rotate" }), requiresDoc: true },
        ],
      },
      {
        id: "pages",
        label: "menuPages",
        items: [
          { label: "menuManagePages", action: () => this.emit({ type: "pages" }), requiresDoc: true },
        ],
      },
    ];
  }

  private buildMenuBar(): void {
    const menus = this.getMenuDefs();
    for (const menu of menus) {
      const wrapper = document.createElement("div");
      wrapper.style.position = "relative";
      wrapper.style.display = "inline-block";

      // Top-level menu button
      const menuBtn = document.createElement("button");
      menuBtn.className = "menubar-item";
      menuBtn.dataset["menuId"] = menu.id;
      this.reg(menuBtn, menu.label);
      this.menuButtons.set(menu.id, menuBtn);

      // Dropdown panel
      const dropdown = document.createElement("div");
      dropdown.className = "menu-dropdown";

      for (const item of menu.items) {
        if (item === null) {
          const sep = document.createElement("div");
          sep.className = "menu-sep";
          dropdown.append(sep);
          continue;
        }

        const entry = document.createElement("button");
        entry.className = "menu-entry";
        entry.tabIndex = -1;

        const labelSpan = document.createElement("span");
        this.reg(labelSpan, item.label);
        entry.append(labelSpan);

        if (item.shortcut) {
          const shortcutSpan = document.createElement("span");
          shortcutSpan.className = "shortcut";
          shortcutSpan.textContent = item.shortcut;
          entry.append(shortcutSpan);
        }

        const action = item.action;
        entry.addEventListener("click", () => {
          this.closeAllMenus();
          action();
        });

        if (item.requiresDoc) {
          entry.disabled = true; // disabled until setLoaded(true)
          this.docMenuEntries.push(entry);
        }

        dropdown.append(entry);
      }

      this.dropdowns.set(menu.id, dropdown);
      wrapper.append(menuBtn, dropdown);
      this.menuBarEl.append(wrapper);
    }
  }

  private setupMenuInteraction(): void {
    // Click on menu button: toggle
    this.menuBarEl.addEventListener("click", (e) => {
      const btn = (e.target as HTMLElement).closest(".menubar-item") as HTMLButtonElement | null;
      if (!btn) return;
      const id = btn.dataset["menuId"]!;
      if (this.activeMenuId === id) {
        this.closeAllMenus();
      } else {
        this.openMenu(id);
      }
    });

    // Hover: switch when another menu is already open
    this.menuBarEl.addEventListener("mouseover", (e) => {
      if (!this.activeMenuId) return;
      const btn = (e.target as HTMLElement).closest(".menubar-item") as HTMLButtonElement | null;
      if (!btn) return;
      const id = btn.dataset["menuId"]!;
      if (id !== this.activeMenuId) {
        this.openMenu(id);
      }
    });

    // Click outside: close
    document.addEventListener("mousedown", (e) => {
      if (!this.activeMenuId) return;
      if (this.menuBarEl.contains(e.target as Node)) return;
      this.closeAllMenus();
    });

    // Keyboard navigation
    document.addEventListener("keydown", (e) => {
      if (!this.activeMenuId) return;

      const dropdown = this.dropdowns.get(this.activeMenuId)!;
      const entries = Array.from(dropdown.querySelectorAll<HTMLButtonElement>(".menu-entry:not(:disabled)"));
      const currentIdx = entries.indexOf(document.activeElement as HTMLButtonElement);

      if (e.key === "Escape") {
        e.preventDefault();
        e.stopImmediatePropagation();
        this.closeAllMenus();
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        e.stopImmediatePropagation();
        const next = currentIdx < entries.length - 1 ? currentIdx + 1 : 0;
        entries[next]?.focus();
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        e.stopImmediatePropagation();
        const prev = currentIdx > 0 ? currentIdx - 1 : entries.length - 1;
        entries[prev]?.focus();
      } else if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
        e.preventDefault();
        e.stopImmediatePropagation();
        const menuIds = Array.from(this.menuButtons.keys());
        const curIdx = menuIds.indexOf(this.activeMenuId!);
        const nextIdx = e.key === "ArrowRight"
          ? (curIdx + 1) % menuIds.length
          : (curIdx - 1 + menuIds.length) % menuIds.length;
        this.openMenu(menuIds[nextIdx]);
      }
    });
  }

  private openMenu(id: string): void {
    // Close previous
    if (this.activeMenuId && this.activeMenuId !== id) {
      this.dropdowns.get(this.activeMenuId)?.classList.remove("open");
      this.menuButtons.get(this.activeMenuId)?.classList.remove("open");
    }

    this.activeMenuId = id;
    const dropdown = this.dropdowns.get(id)!;
    const btn = this.menuButtons.get(id)!;
    dropdown.classList.add("open");
    btn.classList.add("open");

    // Check if dropdown overflows right edge
    requestAnimationFrame(() => {
      const rect = dropdown.getBoundingClientRect();
      if (rect.right > window.innerWidth) {
        dropdown.style.left = "auto";
        dropdown.style.right = "0";
      } else {
        dropdown.style.left = "0";
        dropdown.style.right = "auto";
      }
    });

    // Focus first enabled entry
    const first = dropdown.querySelector<HTMLButtonElement>(".menu-entry:not(:disabled)");
    first?.focus();
  }

  private closeAllMenus(): void {
    if (!this.activeMenuId) return;
    this.dropdowns.get(this.activeMenuId)?.classList.remove("open");
    this.menuButtons.get(this.activeMenuId)?.classList.remove("open");
    this.activeMenuId = null;
  }

  // ── Context Toolbar ─────────────────────────────────────────────────────────

  private buildContextToolbar(): void {
    const ctx = this.contextEl;

    // Page navigation
    const prevBtn = this.btn(ICON_PAGE_UP, "Previous page", "icon-btn");
    this.reg(prevBtn, undefined, "ttPagePrev");
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
    this.pageTotal.textContent = "/ \u2013";

    const nextBtn = this.btn(ICON_PAGE_DOWN, "Next page", "icon-btn");
    this.reg(nextBtn, undefined, "ttPageNext");
    nextBtn.addEventListener("click", () => this.emit({ type: "page-next" }));

    const navWrapper = document.createElement("div");
    navWrapper.className = "page-nav";
    navWrapper.append(prevBtn, this.pageInput, this.pageTotal, nextBtn);

    this.pageNavSection = document.createElement("div");
    this.pageNavSection.style.cssText = "display:none;align-items:center;gap:4px;";
    this.pageNavSection.append(navWrapper, this.sep());
    ctx.append(this.pageNavSection);

    // Zoom
    const zoomOut = this.btn(ICON_ZOOM_OUT, "Zoom out (Ctrl+\u2212)", "icon-btn");
    this.reg(zoomOut, undefined, "ttZoomOut");
    zoomOut.addEventListener("click", () => this.emit({ type: "zoom-out" }));

    this.zoomInput = document.createElement("input");
    this.zoomInput.type = "text";
    this.zoomInput.className = "zoom-input";
    this.zoomInput.value = "100%";
    this.zoomInput.title = "Zoom (free input)";
    this.reg(this.zoomInput, undefined, "ttZoomInput");
    this.zoomInput.addEventListener("focus", () => this.zoomInput.select());
    this.zoomInput.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        this.commitZoomInput();
        this.zoomInput.blur();
      } else if (e.key === "Escape") {
        this.zoomInput.value = `${Math.round(this._lastScale / CSS_UNITS * 100)}%`;
        this.zoomInput.blur();
      }
    });
    this.zoomInput.addEventListener("blur", () => {
      this.zoomInput.value = `${Math.round(this._lastScale / CSS_UNITS * 100)}%`;
    });

    const zoomIn = this.btn(ICON_ZOOM_IN, "Zoom in (Ctrl++)", "icon-btn");
    this.reg(zoomIn, undefined, "ttZoomIn");
    zoomIn.addEventListener("click", () => this.emit({ type: "zoom-in" }));

    this.fitWidthBtn = this.btn(ICON_FIT_WIDTH, "Fit to width", "icon-btn");
    this.reg(this.fitWidthBtn, undefined, "ttFitWidth");
    this.fitWidthBtn.addEventListener("click", () => this.emit({ type: "fit-width" }));
    this.fitHeightBtn = this.btn(ICON_FIT_HEIGHT, "Fit to height", "icon-btn");
    this.reg(this.fitHeightBtn, undefined, "ttFitHeight");
    this.fitHeightBtn.addEventListener("click", () => this.emit({ type: "fit-height" }));

    ctx.append(zoomOut, this.zoomInput, zoomIn, this.fitWidthBtn, this.fitHeightBtn, this.sep());

    // Annotation mode toggle
    this.modeBtn = this.btn("Annotate", "Annotation mode");
    this.reg(this.modeBtn, "btnAnnotate", "ttAnnotate");
    this.modeBtn.addEventListener("click", () => this.toggleMode());
    ctx.append(this.modeBtn);

    // Annotation-only section (hidden in read mode)
    this.annotationSection = document.createElement("div");
    this.annotationSection.style.cssText = "display:none;align-items:center;gap:4px;";
    ctx.append(this.annotationSection);
    const ann = this.annotationSection;

    ann.append(this.sep());

    // Drawing tools
    const tools: [ToolKind, string, string, keyof Translations][] = [
      ["rect",      ICON_RECT,             "Rectangle", "ttRect"],
      ["circle",    ICON_CIRCLE,           "Circle",    "ttCircle"],
      ["text",      ICON_EDITOR_FREE_TEXT, "Text",      "ttText"],
      ["signature", ICON_EDITOR_SIGNATURE, "Signature", "ttSignature"],
    ];

    for (const [kind, icon, title, ttKey] of tools) {
      const b = this.btn(icon, title, "icon-btn");
      this.reg(b, undefined, ttKey);
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
    this.reg(this.colorPicker, undefined, "ttTextColor");
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
    this.reg(this.boldBtn,      undefined, "ttBold");
    this.reg(this.italicBtn,    undefined, "ttItalic");
    this.reg(this.underlineBtn, undefined, "ttUnderline");
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
    const alignDefs: [TextAlignmentValue, string, string, keyof Translations][] = [
      ["left",   ICON_ALIGN_LEFT,   "Align left",   "ttAlignLeft"],
      ["center", ICON_ALIGN_CENTER, "Align center", "ttAlignCenter"],
      ["right",  ICON_ALIGN_RIGHT,  "Align right",  "ttAlignRight"],
    ];
    this.alignBtns = alignDefs.map(([val, icon, title, ttKey]) => {
      const b = this.btn(icon, title, "icon-btn");
      this.reg(b, undefined, ttKey);
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
    const frontBtn = this.btn("\u2191", "Bring to front", "icon-btn");
    const backBtn  = this.btn("\u2193", "Send to back",   "icon-btn");
    this.reg(frontBtn, undefined, "ttBringFront");
    this.reg(backBtn,  undefined, "ttSendBack");
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
    this.reg(this.shapeColorPicker, undefined, "ttStrokeColor");
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
    const frontBtn = this.btn("\u2191", "Bring to front", "icon-btn");
    const backBtn  = this.btn("\u2193", "Send to back",   "icon-btn");
    this.reg(frontBtn, undefined, "ttBringFront");
    this.reg(backBtn,  undefined, "ttSendBack");
    frontBtn.addEventListener("click", () => this.emit({ type: "layer-change", dir: "front" }));
    backBtn.addEventListener ("click", () => this.emit({ type: "layer-change", dir: "back"  }));
    this.shapeStyleSection.append(frontBtn, backBtn);

    container.append(this.shapeStyleSection);
  }

  // ── Shared logic ────────────────────────────────────────────────────────────

  private commitZoomInput(): void {
    const raw = this.zoomInput.value.replace("%", "").trim();
    const pct = parseFloat(raw);
    if (!isNaN(pct) && pct >= 10 && pct <= 500) {
      const scale = Math.round(pct) / 100 * CSS_UNITS;
      this._lastScale = scale;
      this.zoomInput.value = `${Math.round(scale / CSS_UNITS * 100)}%`;
      this.emit({ type: "zoom-set", scale });
    }
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
