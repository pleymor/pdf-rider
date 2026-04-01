import type { Annotation } from "./models";

export class AnnotationHistory {
  private static readonly MAX = 50;
  private undoStack: string[] = [];
  private redoStack: string[] = [];

  push(anns: Annotation[]): void {
    this.undoStack.push(JSON.stringify(anns));
    if (this.undoStack.length > AnnotationHistory.MAX)
      this.undoStack.shift();
    this.redoStack = [];
  }

  undo(current: Annotation[]): Annotation[] | null {
    if (!this.undoStack.length) return null;
    this.redoStack.push(JSON.stringify(current));
    return JSON.parse(this.undoStack.pop()!) as Annotation[];
  }

  redo(current: Annotation[]): Annotation[] | null {
    if (!this.redoStack.length) return null;
    this.undoStack.push(JSON.stringify(current));
    return JSON.parse(this.redoStack.pop()!) as Annotation[];
  }

  clear(): void {
    this.undoStack = [];
    this.redoStack = [];
  }
}
