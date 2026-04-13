// ── Types ─────────────────────────────────────────────────────────────────────

export interface SavedSignature {
  id: string;
  /** Full data:image/png;base64,… string for thumbnail & reuse. */
  imageDataUrl: string;
  /** Epoch ms — used for sorting DESC (most-recently-used first). */
  lastUsedAt: number;
}

// ── SignatureStore ────────────────────────────────────────────────────────────

export class SignatureStore {
  private static STORAGE_KEY = "pdf-rider-saved-signatures";
  static OLD_STROKES_KEY = "pdf-rider-last-signature";

  private signatures: SavedSignature[] = [];

  constructor() {
    this.load();
  }

  // ── Public API ────────────────────────────────────────────────────────────

  /** All saved signatures, sorted by lastUsedAt DESC. */
  getAll(): SavedSignature[] {
    return this.signatures.slice().sort((a, b) => b.lastUsedAt - a.lastUsedAt);
  }

  /** Save a new signature and return the created entry. */
  add(imageDataUrl: string): SavedSignature | null {
    const sig: SavedSignature = {
      id: crypto.randomUUID(),
      imageDataUrl,
      lastUsedAt: Date.now(),
    };
    this.signatures.push(sig);
    if (!this.save()) {
      // Quota exceeded — roll back
      this.signatures.pop();
      return null;
    }
    return sig;
  }

  /** Remove a saved signature by id. */
  delete(id: string): void {
    this.signatures = this.signatures.filter((s) => s.id !== id);
    this.save();
  }

  /** Update lastUsedAt to now (moves signature to the top of the list). */
  touch(id: string): void {
    const sig = this.signatures.find((s) => s.id === id);
    if (sig) {
      sig.lastUsedAt = Date.now();
      this.save();
    }
  }

  /** Check whether the old strokes key needs migration. */
  hasOldStrokesKey(): boolean {
    return localStorage.getItem(SignatureStore.OLD_STROKES_KEY) !== null;
  }

  /** Read old strokes data and remove the key. Returns null if nothing to migrate. */
  consumeOldStrokes(): [number, number][][] | null {
    try {
      const raw = localStorage.getItem(SignatureStore.OLD_STROKES_KEY);
      if (!raw) return null;
      const strokes = JSON.parse(raw) as [number, number][][];
      localStorage.removeItem(SignatureStore.OLD_STROKES_KEY);
      if (Array.isArray(strokes) && strokes.length > 0) return strokes;
    } catch { /* ignore corrupt data */ }
    return null;
  }

  // ── Persistence ───────────────────────────────────────────────────────────

  private load(): void {
    try {
      const raw = localStorage.getItem(SignatureStore.STORAGE_KEY);
      if (raw) this.signatures = JSON.parse(raw) as SavedSignature[];
    } catch { /* ignore corrupt data */ }
  }

  /** Returns false on QuotaExceededError. */
  private save(): boolean {
    try {
      localStorage.setItem(
        SignatureStore.STORAGE_KEY,
        JSON.stringify(this.signatures),
      );
      return true;
    } catch {
      return false;
    }
  }
}
