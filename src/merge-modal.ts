import type { Translations } from "./i18n";
import { MergeList, fileNameOf, type MergeItem } from "./merge-list";
import { checkMergeInput, openPdfsDialog } from "./tauri-bridge";
import { errorText } from "./print-errors";

/**
 * Runs the merge for the listed files, in order. Resolves true when done
 * (the dialog then closes), false to keep the dialog open (e.g. the Save As
 * dialog was cancelled or the merge failed).
 */
type ConfirmHandler = (paths: string[]) => Promise<boolean>;

// Arrow characters fall back to a colour emoji font on Windows: draw them.
const ICON_UP = '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M8 13V3M3.5 7.5L8 3l4.5 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>';
const ICON_DOWN = '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M8 3v10M3.5 8.5L8 13l4.5-4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>';
const ICON_REMOVE = '<svg width="12" height="12" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M3.5 3.5l9 9M12.5 3.5l-9 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>';

/** File › Merge PDFs…: the list of files to combine, in order. */
export class MergeModal {
  private backdrop: HTMLElement;
  private listEl: HTMLOListElement;
  private emptyEl: HTMLElement;
  private noteEl: HTMLElement;
  private summaryEl: HTMLElement;
  private addBtn: HTMLButtonElement;
  private applyBtn: HTMLButtonElement;
  private cancelBtn: HTMLButtonElement;
  private closeBtn: HTMLButtonElement;
  private list = new MergeList();
  private confirm: ConfirmHandler | null = null;
  private busy = false;
  private _t: Translations | null = null;
  /** Row being dragged by its handle. */
  private dragId: number | null = null;

  constructor() {
    this.backdrop = document.getElementById("merge-modal")!;
    this.listEl = document.getElementById("merge-list") as HTMLOListElement;
    this.emptyEl = document.getElementById("merge-empty")!;
    this.noteEl = document.getElementById("merge-note")!;
    this.summaryEl = document.getElementById("merge-summary")!;
    this.addBtn = document.getElementById("merge-add-btn") as HTMLButtonElement;
    this.applyBtn = document.getElementById("merge-apply-btn") as HTMLButtonElement;
    this.cancelBtn = document.getElementById("merge-cancel-btn") as HTMLButtonElement;
    this.closeBtn = document.getElementById("merge-close-btn") as HTMLButtonElement;
    this.bind();
  }

  setConfirmHandler(cb: ConfirmHandler): void {
    this.confirm = cb;
  }

  applyTranslations(t: Translations): void {
    this._t = t;
    this.render();
  }

  isOpen(): boolean {
    return !this.backdrop.classList.contains("hidden");
  }

  /**
   * Opens the dialog with `paths` as the starting list. `note`, when given,
   * is shown above the list (e.g. that unsaved changes are not included).
   */
  open(paths: string[], note: string | null = null): void {
    this.list.clear();
    this.busy = false;
    this.noteEl.textContent = note ?? "";
    this.noteEl.classList.toggle("hidden", !note);
    this.backdrop.classList.remove("hidden");
    this.addFiles(paths);
    this.addBtn.focus();
  }

  close(): void {
    if (this.busy) return;
    this.backdrop.classList.add("hidden");
  }

  /** Appends files to the list and checks each one in the background. */
  addFiles(paths: string[]): void {
    const added = this.list.add(paths);
    this.render();
    for (const item of added) void this.check(item);
  }

  private async check(item: MergeItem): Promise<void> {
    try {
      const pages = await checkMergeInput(item.path);
      this.list.setStatus(item.id, { state: "ok", pages });
    } catch (err) {
      this.list.setStatus(item.id, { state: "error", message: errorText(err) });
    }
    this.render();
  }

  private tr<K extends keyof Translations>(key: K, fallback: string): string {
    return (this._t?.[key] as string | undefined) ?? fallback;
  }

  private bind(): void {
    this.closeBtn.addEventListener("click", () => this.close());
    this.cancelBtn.addEventListener("click", () => this.close());
    this.backdrop.addEventListener("click", (e) => {
      if (e.target === this.backdrop) this.close();
    });
    this.backdrop.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        this.close();
      }
    });
    this.addBtn.addEventListener("click", async () => {
      const paths = await openPdfsDialog();
      if (paths.length > 0) this.addFiles(paths);
    });
    this.applyBtn.addEventListener("click", () => void this.handleApply());
  }

  private async handleApply(): Promise<void> {
    if (this.busy || this.list.readiness() !== "ready" || !this.confirm) return;
    this.busy = true;
    this.render();
    let done = false;
    try {
      done = await this.confirm(this.list.paths());
    } finally {
      this.busy = false;
      if (done) this.close();
      else this.render();
    }
  }

  private render(): void {
    // Rebuilding the rows mid-drag would drop the dragged row and its
    // pointer capture: refresh only the summary; the drop re-renders.
    if (this.dragId !== null) {
      this.renderSummary();
      return;
    }
    const items = this.list.items;
    this.listEl.replaceChildren(...items.map((item, i) => this.renderRow(item, i, items.length)));
    this.emptyEl.classList.toggle("hidden", items.length > 0);
    this.listEl.classList.toggle("hidden", items.length === 0);
    this.renderSummary();
  }

  private renderSummary(): void {
    const readiness = this.list.readiness();
    let text: string;
    if (this.busy) {
      text = this.tr("mergeBusy", "Merging…");
    } else if (readiness === "too-few") {
      text = this.tr("mergeTooFew", "Add at least two PDF files.");
    } else if (readiness === "checking") {
      text = this.tr("mergeCheckingAll", "Checking files…");
    } else if (readiness === "has-errors") {
      text = this.tr("mergeHasErrors", "Remove the files that cannot be merged.");
    } else {
      text = this.tr("mergeSummary", "{files} files, {pages} pages in total.")
        .replace("{files}", String(this.list.items.length))
        .replace("{pages}", String(this.list.totalPages()));
    }
    this.summaryEl.textContent = text;
    this.summaryEl.classList.toggle("error", !this.busy && readiness === "has-errors");
    this.applyBtn.disabled = this.busy || readiness !== "ready";
    this.addBtn.disabled = this.busy;
    this.cancelBtn.disabled = this.busy;
  }

  private renderRow(item: MergeItem, index: number, count: number): HTMLLIElement {
    const li = document.createElement("li");
    li.className = "merge-row";
    li.dataset["id"] = String(item.id);
    if (item.status.state === "error") li.classList.add("has-error");

    const handle = document.createElement("span");
    handle.className = "merge-handle";
    handle.textContent = "⠇⠇";
    handle.title = this.tr("mergeDragHandle", "Drag to reorder");
    handle.setAttribute("aria-hidden", "true");
    this.bindDrag(handle, li, item.id);

    const num = document.createElement("span");
    num.className = "merge-index";
    num.textContent = `${index + 1}.`;

    const file = document.createElement("div");
    file.className = "merge-file";
    const name = document.createElement("span");
    name.className = "merge-name";
    name.textContent = fileNameOf(item.path);
    name.title = item.path;
    const meta = document.createElement("span");
    meta.className = "merge-meta";
    if (item.status.state === "ok") {
      meta.textContent = this.tr("mergePages", "{n} page(s)").replace("{n}", String(item.status.pages));
    } else if (item.status.state === "checking") {
      meta.textContent = this.tr("mergeChecking", "Checking…");
    } else {
      meta.textContent = item.status.message;
      meta.classList.add("error");
    }
    file.append(name, meta);

    const up = this.rowButton(ICON_UP, this.tr("mergeMoveUp", "Move up"), "up", index === 0, () => {
      this.list.move(item.id, -1);
      this.renderKeepingFocus(item.id, "up");
    });
    const down = this.rowButton(ICON_DOWN, this.tr("mergeMoveDown", "Move down"), "down", index === count - 1, () => {
      this.list.move(item.id, 1);
      this.renderKeepingFocus(item.id, "down");
    });
    const remove = this.rowButton(ICON_REMOVE, this.tr("mergeRemove", "Remove from the list"), "remove", false, () => {
      this.list.remove(item.id);
      this.render();
      this.addBtn.focus();
    });
    for (const b of [up, down, remove]) b.disabled ||= this.busy;

    li.append(handle, num, file, up, down, remove);
    return li;
  }

  private rowButton(icon: string, title: string, role: string, disabled: boolean, action: () => void): HTMLButtonElement {
    const b = document.createElement("button");
    b.className = "icon-btn merge-row-btn";
    b.innerHTML = icon; // constant SVG markup
    b.title = title;
    b.setAttribute("aria-label", title);
    b.dataset["role"] = role;
    b.disabled = disabled;
    b.addEventListener("click", action);
    return b;
  }

  /** Re-renders, then focuses the same button of the moved row so repeated presses keep moving it. */
  private renderKeepingFocus(id: number, role: string): void {
    this.render();
    const row = this.listEl.querySelector<HTMLElement>(`li[data-id="${id}"]`);
    const btn = row?.querySelector<HTMLButtonElement>(`button[data-role="${role}"]`);
    if (btn && !btn.disabled) btn.focus();
    else row?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }

  /**
   * Pointer-driven reordering. HTML5 drag and drop is not used: Tauri's
   * file-drop handling swallows it inside the window on Windows.
   */
  private bindDrag(handle: HTMLElement, li: HTMLLIElement, id: number): void {
    handle.addEventListener("pointerdown", (e) => {
      if (this.busy || e.button !== 0) return;
      e.preventDefault();
      this.dragId = id;
      handle.setPointerCapture(e.pointerId);
      li.classList.add("dragging");
    });
    handle.addEventListener("pointermove", (e) => {
      if (this.dragId !== id || (e.buttons & 1) === 0) return;
      // Target index = number of other rows whose middle is above the pointer.
      const others = Array.from(this.listEl.children).filter(r => r !== li) as HTMLElement[];
      const index = others.filter(r => {
        const box = r.getBoundingClientRect();
        return box.top + box.height / 2 < e.clientY;
      }).length;
      const current = Array.from(this.listEl.children).indexOf(li);
      if (index === current) return;
      // Move the row node itself (not a re-render) so pointer capture survives.
      this.listEl.insertBefore(li, others[index] ?? null);
      this.list.moveTo(id, index);
      this.listEl.querySelectorAll<HTMLElement>(".merge-index").forEach((el, i) => {
        el.textContent = `${i + 1}.`;
      });
    });
    const end = () => {
      if (this.dragId !== id) return;
      this.dragId = null;
      this.render();
    };
    handle.addEventListener("pointerup", end);
    handle.addEventListener("pointercancel", end);
  }
}
