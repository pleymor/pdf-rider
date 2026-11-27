/** Helpers for errors raised while printing (kept free of DOM/Tauri imports). */

/** Error returned by the Rust print commands (`PrintError`). */
export interface PrintError {
  kind: string;
  message: string;
}

export function isPrintCancelled(err: unknown): boolean {
  return !!err && typeof err === "object" && (err as PrintError).kind === "cancelled";
}

/** Readable text of anything thrown by `invoke()`, pdf.js or our own code. */
export function errorText(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === "string") return err;
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return String(err);
}

