import type { Annotation } from "./models";

/** In-memory store mapping 1-indexed page numbers to annotation arrays. */
export class AnnotationStore {
  private store = new Map<number, Annotation[]>();

  add(annotation: Annotation): void {
    const page = annotation.page;
    if (!this.store.has(page)) {
      this.store.set(page, []);
    }
    this.store.get(page)!.push(annotation);
  }

  remove(page: number, index: number): void {
    const list = this.store.get(page);
    if (list) {
      list.splice(index, 1);
      if (list.length === 0) this.store.delete(page);
    }
  }

  removeRef(ann: Annotation): boolean {
    const list = this.store.get(ann.page);
    if (!list) return false;
    const idx = list.indexOf(ann);
    if (idx === -1) return false;
    list.splice(idx, 1);
    if (list.length === 0) this.store.delete(ann.page);
    return true;
  }

  getForPage(page: number): Annotation[] {
    return this.store.get(page) ?? [];
  }

  getAllGrouped(): Map<number, Annotation[]> {
    return new Map(this.store);
  }

  /** Flatten all annotations across all pages into a single array. */
  getAll(): Annotation[] {
    const result: Annotation[] = [];
    for (const list of this.store.values()) {
      result.push(...list);
    }
    return result;
  }

  bringToFront(ann: Annotation): void {
    const list = this.store.get(ann.page);
    if (!list) return;
    const idx = list.indexOf(ann);
    if (idx !== -1 && idx < list.length - 1) {
      list.splice(idx, 1);
      list.push(ann);
    }
  }

  sendToBack(ann: Annotation): void {
    const list = this.store.get(ann.page);
    if (!list) return;
    const idx = list.indexOf(ann);
    if (idx > 0) {
      list.splice(idx, 1);
      list.unshift(ann);
    }
  }

  replaceAll(anns: Annotation[]): void {
    this.store.clear();
    for (const ann of anns) this.add(ann);
  }

  clear(): void {
    this.store.clear();
  }

  isEmpty(): boolean {
    return this.store.size === 0;
  }
}
