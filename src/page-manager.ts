import type { PDFDocumentProxy } from "pdfjs-dist";
import type { Translations } from "./i18n";
import type { PageOperation } from "./tauri-bridge";
import { ICON_DELETE, ICON_ROTATE_CW } from "./icons";

interface PageState {
  page: number;
  rotation: number;
  deleted: boolean;
}

type ConfirmHandler = (operations: PageOperation[]) => void;

const RENDER_WIDTH_CSS = 800;
const THUMB_WIDTH_MIN = 120;
const THUMB_WIDTH_MAX = 800;
const THUMB_WIDTH_DEFAULT = 220;
const THUMB_WIDTH_STORAGE_KEY = "pdf-rider:page-manager-thumb-width";

export class PageManagerModal {
  private backdrop: HTMLElement;
  private container: HTMLElement;
  private grid: HTMLElement;
  private titleEl: HTMLElement;
  private cancelBtn: HTMLButtonElement;
  private applyBtn: HTMLButtonElement;
  private zoomSlider: HTMLInputElement;
  private zoomLabel: HTMLElement;
  private deletedCounter: HTMLElement;
  private pageStates: PageState[] = [];
  private cards: HTMLElement[] = [];
  private selectedIndices = new Set<number>();
  private anchorIndex: number | null = null;
  private focusedIndex: number | null = null;
  private confirmHandlers: ConfirmHandler[] = [];
  private _i18nText = new Map<HTMLElement, keyof Translations>();
  private _i18nTitle = new Map<HTMLElement, keyof Translations>();
  private _translations: Translations | null = null;
  private _keydownHandler = (e: KeyboardEvent) => this.handleKeydown(e);

  constructor() {
    const built = this.buildDOM();
    this.backdrop = built.backdrop;
    this.container = built.container;
    this.grid = built.grid;
    this.titleEl = built.titleEl;
    this.cancelBtn = built.cancelBtn;
    this.applyBtn = built.applyBtn;
    this.zoomSlider = built.zoomSlider;
    this.zoomLabel = built.zoomLabel;
    this.deletedCounter = built.deletedCounter;
    document.body.appendChild(this.backdrop);

    const initialWidth = this.readStoredWidth();
    this.applyThumbnailWidth(initialWidth);
    this.zoomSlider.value = String(initialWidth);
  }

  onConfirm(cb: ConfirmHandler): void {
    this.confirmHandlers.push(cb);
  }

  applyTranslations(t: Translations): void {
    this._translations = t;
    this._i18nText.forEach((key, el) => {
      const v = t[key];
      if (v !== undefined) el.textContent = v;
    });
    this._i18nTitle.forEach((key, el) => {
      const v = t[key];
      if (v !== undefined) el.title = v;
    });
    this.updateDeletedCounter();
  }

  async open(pdfDoc: PDFDocumentProxy, pageCount: number): Promise<void> {
    this.pageStates = [];
    this.cards = [];
    this.selectedIndices.clear();
    this.anchorIndex = null;
    this.focusedIndex = null;
    this.grid.innerHTML = "";

    for (let i = 1; i <= pageCount; i++) {
      this.pageStates.push({ page: i, rotation: 0, deleted: false });
    }

    this.backdrop.classList.remove("hidden");
    document.addEventListener("keydown", this._keydownHandler);

    for (let i = 0; i < pageCount; i++) {
      const state = this.pageStates[i];
      const card = this.createCard(i, state, pdfDoc);
      this.cards.push(card);
      this.grid.appendChild(card);
    }

    this.updateDeleteButtons();
    this.updateDeletedCounter();
  }

  close(): void {
    this.backdrop.classList.add("hidden");
    document.removeEventListener("keydown", this._keydownHandler);
    this.grid.innerHTML = "";
    this.cards = [];
    this.pageStates = [];
    this.selectedIndices.clear();
    this.anchorIndex = null;
    this.focusedIndex = null;
  }

  private buildDOM() {
    const backdrop = document.createElement("div");
    backdrop.className = "modal-backdrop hidden";

    const container = document.createElement("div");
    container.className = "page-manager-container";

    // Header
    const header = document.createElement("div");
    header.className = "page-manager-header";

    const titleEl = document.createElement("span");
    titleEl.className = "page-manager-title";
    titleEl.textContent = "Manage Pages";
    this._i18nText.set(titleEl, "pmTitle");

    // Zoom control
    const zoomGroup = document.createElement("div");
    zoomGroup.className = "page-manager-zoom";

    const zoomLabel = document.createElement("span");
    zoomLabel.className = "page-manager-zoom-label";
    zoomLabel.textContent = "Size";
    this._i18nText.set(zoomLabel, "pmZoom");

    const zoomSlider = document.createElement("input");
    zoomSlider.type = "range";
    zoomSlider.min = String(THUMB_WIDTH_MIN);
    zoomSlider.max = String(THUMB_WIDTH_MAX);
    zoomSlider.step = "10";
    zoomSlider.value = String(THUMB_WIDTH_DEFAULT);
    zoomSlider.className = "page-manager-zoom-slider";
    zoomSlider.addEventListener("input", () => {
      const w = Number(zoomSlider.value);
      this.applyThumbnailWidth(w);
    });
    zoomSlider.addEventListener("change", () => {
      const w = Number(zoomSlider.value);
      this.writeStoredWidth(w);
    });

    zoomGroup.append(zoomLabel, zoomSlider);

    const closeBtn = document.createElement("button");
    closeBtn.className = "icon-btn";
    closeBtn.innerHTML = "&times;";
    closeBtn.title = "Close";
    closeBtn.addEventListener("click", () => this.close());

    header.append(titleEl, zoomGroup, closeBtn);

    // Grid
    const grid = document.createElement("div");
    grid.className = "page-manager-grid";
    grid.tabIndex = -1;
    grid.addEventListener("mousedown", (e) => {
      if (e.target === grid) {
        this.clearSelection();
      }
    });

    // Footer
    const footer = document.createElement("div");
    footer.className = "page-manager-footer";

    const deletedCounter = document.createElement("span");
    deletedCounter.className = "page-manager-deleted-counter";
    deletedCounter.textContent = "";

    const footerButtons = document.createElement("div");
    footerButtons.className = "page-manager-footer-buttons";

    const cancelBtn = document.createElement("button");
    cancelBtn.className = "btn";
    cancelBtn.textContent = "Cancel";
    this._i18nText.set(cancelBtn, "pmCancel");
    cancelBtn.addEventListener("click", () => this.close());

    const applyBtn = document.createElement("button");
    applyBtn.className = "btn btn-primary";
    applyBtn.textContent = "Apply";
    this._i18nText.set(applyBtn, "pmApply");
    applyBtn.addEventListener("click", () => this.handleApply());

    footerButtons.append(cancelBtn, applyBtn);
    footer.append(deletedCounter, footerButtons);
    container.append(header, grid, footer);

    backdrop.addEventListener("click", (e) => {
      if (e.target === backdrop) this.close();
    });

    container.addEventListener(
      "wheel",
      (e) => {
        if (!e.ctrlKey && !e.metaKey) return;
        e.preventDefault();
        const step = 20;
        this.adjustZoom(e.deltaY < 0 ? step : -step);
      },
      { passive: false },
    );

    backdrop.appendChild(container);

    return {
      backdrop,
      container,
      grid,
      titleEl,
      cancelBtn,
      applyBtn,
      zoomSlider,
      zoomLabel,
      deletedCounter,
    };
  }

  private createCard(
    index: number,
    state: PageState,
    pdfDoc: PDFDocumentProxy,
  ): HTMLElement {
    const card = document.createElement("div");
    card.className = "page-thumbnail-card";
    card.tabIndex = 0;

    const canvasWrapper = document.createElement("div");
    canvasWrapper.className = "page-thumbnail-canvas-wrapper";

    const canvas = document.createElement("canvas");
    canvasWrapper.appendChild(canvas);

    // Render thumbnail async at high resolution; CSS controls displayed size
    void this.renderThumbnail(state.page, canvas, pdfDoc);

    // Selection: clicking on the canvas area selects the card
    canvasWrapper.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.handleCardClick(index, e);
    });
    card.addEventListener("focus", () => {
      this.focusedIndex = index;
    });

    // Controls
    const controls = document.createElement("div");
    controls.className = "page-thumbnail-controls";

    const pageNum = document.createElement("span");
    pageNum.className = "page-number";
    pageNum.textContent = String(state.page);

    const rotateBtn = document.createElement("button");
    rotateBtn.className = "icon-btn";
    rotateBtn.innerHTML = ICON_ROTATE_CW;
    rotateBtn.title = "Rotate page";
    this._i18nTitle.set(rotateBtn, "pmRotatePage");
    rotateBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      this.rotateTargets(index);
    });

    const deleteBtn = document.createElement("button");
    deleteBtn.className = "icon-btn delete-btn";
    deleteBtn.innerHTML = ICON_DELETE;
    deleteBtn.title = "Delete page";
    this._i18nTitle.set(deleteBtn, "pmDeletePage");
    deleteBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      this.toggleDeleteTargets(index);
    });

    controls.append(pageNum, rotateBtn, deleteBtn);
    card.append(canvasWrapper, controls);

    return card;
  }

  private async renderThumbnail(
    pageNum: number,
    canvas: HTMLCanvasElement,
    pdfDoc: PDFDocumentProxy,
  ): Promise<void> {
    const page = await pdfDoc.getPage(pageNum);
    const dpr = window.devicePixelRatio || 1;
    const baseViewport = page.getViewport({ scale: 1.0 });
    const scale = (RENDER_WIDTH_CSS * dpr) / baseViewport.width;
    const viewport = page.getViewport({ scale });

    canvas.width = viewport.width;
    canvas.height = viewport.height;
    // Aspect ratio for CSS-driven sizing
    canvas.style.aspectRatio = `${viewport.width} / ${viewport.height}`;

    const ctx = canvas.getContext("2d")!;
    await page.render({ canvasContext: ctx, viewport }).promise;
  }

  // ── Sizing ──────────────────────────────────────────────────────────────

  private applyThumbnailWidth(width: number): void {
    const w = Math.max(THUMB_WIDTH_MIN, Math.min(THUMB_WIDTH_MAX, width));
    this.container.style.setProperty("--pm-thumb-width", `${w}px`);
  }

  private adjustZoom(delta: number): void {
    const current = Number(this.zoomSlider.value);
    const next = Math.max(
      THUMB_WIDTH_MIN,
      Math.min(THUMB_WIDTH_MAX, current + delta),
    );
    if (next === current) return;
    this.zoomSlider.value = String(next);
    this.applyThumbnailWidth(next);
    this.writeStoredWidth(next);
  }

  private readStoredWidth(): number {
    try {
      const raw = localStorage.getItem(THUMB_WIDTH_STORAGE_KEY);
      if (!raw) return THUMB_WIDTH_DEFAULT;
      const n = Number(raw);
      if (!Number.isFinite(n)) return THUMB_WIDTH_DEFAULT;
      return Math.max(THUMB_WIDTH_MIN, Math.min(THUMB_WIDTH_MAX, n));
    } catch {
      return THUMB_WIDTH_DEFAULT;
    }
  }

  private writeStoredWidth(width: number): void {
    try {
      localStorage.setItem(THUMB_WIDTH_STORAGE_KEY, String(width));
    } catch {
      /* storage unavailable */
    }
  }

  // ── Selection ───────────────────────────────────────────────────────────

  private handleCardClick(index: number, e: MouseEvent): void {
    const card = this.cards[index];
    card.focus();
    if (e.shiftKey && this.anchorIndex !== null) {
      this.selectRange(this.anchorIndex, index);
    } else if (e.ctrlKey || e.metaKey) {
      this.toggleSelection(index);
      this.anchorIndex = index;
    } else {
      this.setSelection([index]);
      this.anchorIndex = index;
    }
  }

  private setSelection(indices: number[]): void {
    this.selectedIndices.clear();
    for (const i of indices) this.selectedIndices.add(i);
    this.refreshSelectionClasses();
  }

  private toggleSelection(index: number): void {
    if (this.selectedIndices.has(index)) {
      this.selectedIndices.delete(index);
    } else {
      this.selectedIndices.add(index);
    }
    this.refreshSelectionClasses();
  }

  private selectRange(a: number, b: number): void {
    const lo = Math.min(a, b);
    const hi = Math.max(a, b);
    this.selectedIndices.clear();
    for (let i = lo; i <= hi; i++) this.selectedIndices.add(i);
    this.refreshSelectionClasses();
  }

  private selectAll(): void {
    this.selectedIndices.clear();
    for (let i = 0; i < this.cards.length; i++) this.selectedIndices.add(i);
    this.refreshSelectionClasses();
  }

  private clearSelection(): void {
    this.selectedIndices.clear();
    this.anchorIndex = null;
    this.refreshSelectionClasses();
  }

  private refreshSelectionClasses(): void {
    for (let i = 0; i < this.cards.length; i++) {
      this.cards[i].classList.toggle("selected", this.selectedIndices.has(i));
    }
  }

  /** Returns indices the action should apply to: full selection if `index` is part of it,
   *  otherwise just the single index. */
  private resolveTargets(index: number): number[] {
    if (this.selectedIndices.has(index) && this.selectedIndices.size > 1) {
      return [...this.selectedIndices].sort((a, b) => a - b);
    }
    return [index];
  }

  // ── Actions ─────────────────────────────────────────────────────────────

  private rotateTargets(index: number): void {
    const targets = this.resolveTargets(index);
    for (const i of targets) this.rotateOne(i);
  }

  private rotateOne(index: number): void {
    const state = this.pageStates[index];
    state.rotation = (state.rotation + 90) % 360;
    const wrapper = this.cards[index].querySelector(
      ".page-thumbnail-canvas-wrapper",
    ) as HTMLElement | null;
    if (wrapper) {
      wrapper.style.transform = state.rotation
        ? `rotate(${state.rotation}deg)`
        : "";
    }
  }

  private toggleDeleteTargets(index: number): void {
    const targets = this.resolveTargets(index);
    // Decide direction by the clicked card's current state so the action feels predictable
    const goingToDelete = !this.pageStates[index].deleted;
    const aliveCount = this.pageStates.filter((s) => !s.deleted).length;

    if (goingToDelete) {
      // Block deleting all pages: leave at least one alive
      const wouldDelete = targets.filter((i) => !this.pageStates[i].deleted).length;
      if (aliveCount - wouldDelete < 1) return;
    }

    for (const i of targets) {
      this.pageStates[i].deleted = goingToDelete;
      this.cards[i].classList.toggle("deleted", goingToDelete);
    }
    this.updateDeleteButtons();
    this.updateDeletedCounter();
  }

  // ── Keyboard ────────────────────────────────────────────────────────────

  private handleKeydown(e: KeyboardEvent): void {
    if (this.backdrop.classList.contains("hidden")) return;

    // Don't intercept while editing form fields (we don't have any here, but be safe)
    const tag = (e.target as HTMLElement | null)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA") {
      if (e.key === "Escape") {
        e.preventDefault();
        this.close();
      }
      return;
    }

    switch (e.key) {
      case "Escape":
        e.preventDefault();
        this.close();
        return;
      case "Enter":
        e.preventDefault();
        this.handleApply();
        return;
      case "Delete":
      case "Backspace":
        e.preventDefault();
        this.deleteFromKeyboard();
        return;
      case "r":
      case "R":
        if (e.ctrlKey || e.metaKey || e.altKey) return;
        e.preventDefault();
        this.rotateFromKeyboard();
        return;
      case "a":
      case "A":
        if (e.ctrlKey || e.metaKey) {
          e.preventDefault();
          this.selectAll();
        }
        return;
      case "+":
      case "=":
        if (e.ctrlKey || e.metaKey) {
          e.preventDefault();
          this.adjustZoom(20);
        }
        return;
      case "-":
      case "_":
        if (e.ctrlKey || e.metaKey) {
          e.preventDefault();
          this.adjustZoom(-20);
        }
        return;
      case "ArrowLeft":
        e.preventDefault();
        this.moveFocus(-1, e.shiftKey);
        return;
      case "ArrowRight":
        e.preventDefault();
        this.moveFocus(1, e.shiftKey);
        return;
      case "ArrowUp":
        e.preventDefault();
        this.moveFocus(-this.computeRowSize(), e.shiftKey);
        return;
      case "ArrowDown":
        e.preventDefault();
        this.moveFocus(this.computeRowSize(), e.shiftKey);
        return;
      case "Home":
        e.preventDefault();
        this.focusIndex(0, e.shiftKey);
        return;
      case "End":
        e.preventDefault();
        this.focusIndex(this.cards.length - 1, e.shiftKey);
        return;
    }
  }

  private deleteFromKeyboard(): void {
    const targets = this.targetsFromKeyboard();
    if (targets.length === 0) return;
    // Reuse toggle logic: pick first target as the "clicked" reference
    this.toggleDeleteTargets(targets[0]);
  }

  private rotateFromKeyboard(): void {
    const targets = this.targetsFromKeyboard();
    if (targets.length === 0) return;
    for (const i of targets) this.rotateOne(i);
  }

  private targetsFromKeyboard(): number[] {
    if (this.selectedIndices.size > 0) {
      return [...this.selectedIndices].sort((a, b) => a - b);
    }
    if (this.focusedIndex !== null) return [this.focusedIndex];
    return [];
  }

  private moveFocus(delta: number, extend: boolean): void {
    const base =
      this.focusedIndex ?? (this.cards.length > 0 ? 0 : null);
    if (base === null) return;
    let next = base + delta;
    next = Math.max(0, Math.min(this.cards.length - 1, next));
    this.focusIndex(next, extend);
  }

  private focusIndex(index: number, extend: boolean): void {
    if (index < 0 || index >= this.cards.length) return;
    const card = this.cards[index];
    card.focus();
    this.focusedIndex = index;
    if (extend && this.anchorIndex !== null) {
      this.selectRange(this.anchorIndex, index);
    } else {
      this.setSelection([index]);
      this.anchorIndex = index;
    }
    card.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  private computeRowSize(): number {
    if (this.cards.length < 2) return 1;
    const firstTop = this.cards[0].getBoundingClientRect().top;
    for (let i = 1; i < this.cards.length; i++) {
      if (this.cards[i].getBoundingClientRect().top !== firstTop) {
        return i;
      }
    }
    return this.cards.length;
  }

  // ── Footer counter ──────────────────────────────────────────────────────

  private updateDeletedCounter(): void {
    const n = this.pageStates.filter((s) => s.deleted).length;
    if (n === 0) {
      this.deletedCounter.textContent = "";
      return;
    }
    const template = this._translations?.pmWillDelete ?? "Will delete: {n}";
    this.deletedCounter.textContent = template.replace("{n}", String(n));
  }

  private updateDeleteButtons(): void {
    const aliveCount = this.pageStates.filter((s) => !s.deleted).length;
    for (let i = 0; i < this.cards.length; i++) {
      const state = this.pageStates[i];
      const deleteBtn = this.cards[i].querySelector(
        ".page-thumbnail-controls .delete-btn",
      ) as HTMLButtonElement | null;
      if (deleteBtn) {
        deleteBtn.disabled = !state.deleted && aliveCount <= 1;
      }
    }
  }

  private handleApply(): void {
    const operations: PageOperation[] = this.pageStates.map((s) => ({
      page: s.page,
      rotation: s.rotation,
      delete: s.deleted,
    }));
    for (const cb of this.confirmHandlers) cb(operations);
    this.close();
  }
}
