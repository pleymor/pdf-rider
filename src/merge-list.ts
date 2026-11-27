/** State of the merge dialog's file list, kept free of the DOM so it can be tested. */

export type MergeItemStatus =
  | { state: "checking" }
  | { state: "ok"; pages: number }
  | { state: "error"; message: string };

export interface MergeItem {
  /** Unique within the list; a file added twice gets two ids. */
  id: number;
  path: string;
  status: MergeItemStatus;
}

export type MergeReadiness = "too-few" | "checking" | "has-errors" | "ready";

export class MergeList {
  private _items: MergeItem[] = [];
  private nextId = 1;

  get items(): readonly MergeItem[] {
    return this._items;
  }

  /** Appends `paths` in order and returns the new items. */
  add(paths: string[]): MergeItem[] {
    const added = paths.map(path => ({ id: this.nextId++, path, status: { state: "checking" } as MergeItemStatus }));
    this._items.push(...added);
    return added;
  }

  remove(id: number): void {
    this._items = this._items.filter(i => i.id !== id);
  }

  clear(): void {
    this._items = [];
  }

  /** Moves an item one place up (-1) or down (+1); no-op at either end. */
  move(id: number, delta: -1 | 1): void {
    const from = this.indexOf(id);
    const to = from + delta;
    if (from < 0 || to < 0 || to >= this._items.length) return;
    this.moveTo(id, to);
  }

  /** Moves an item to position `index` (clamped to the list). */
  moveTo(id: number, index: number): void {
    const from = this.indexOf(id);
    if (from < 0) return;
    const [item] = this._items.splice(from, 1);
    const to = Math.max(0, Math.min(index, this._items.length));
    this._items.splice(to, 0, item);
  }

  setStatus(id: number, status: MergeItemStatus): void {
    const item = this._items.find(i => i.id === id);
    if (item) item.status = status;
  }

  paths(): string[] {
    return this._items.map(i => i.path);
  }

  totalPages(): number {
    return this._items.reduce((n, i) => n + (i.status.state === "ok" ? i.status.pages : 0), 0);
  }

  readiness(): MergeReadiness {
    if (this._items.length < 2) return "too-few";
    if (this._items.some(i => i.status.state === "error")) return "has-errors";
    if (this._items.some(i => i.status.state === "checking")) return "checking";
    return "ready";
  }

  private indexOf(id: number): number {
    return this._items.findIndex(i => i.id === id);
  }
}

/** Last segment of a Windows or POSIX path. */
export function fileNameOf(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** Suggested file name for the merged PDF: "<first file>-merged.pdf". */
export function defaultMergedName(paths: string[]): string {
  if (paths.length === 0) return "merged.pdf";
  const stem = fileNameOf(paths[0]).replace(/\.pdf$/i, "");
  return `${stem}-merged.pdf`;
}
