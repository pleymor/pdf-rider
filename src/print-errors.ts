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

/** Why a PDF could not be opened for printing, in the user's terms (pdf.js errors). */
export function describeLoadError(err: unknown): string {
  const name = err && typeof err === "object" && "name" in err ? String((err as { name: unknown }).name) : "";
  const msg = errorText(err);
  if (name === "PasswordException" || /password/i.test(msg)) {
    return "The PDF is password-protected. Open it in PDF Rider and print it from there.";
  }
  if (name === "MissingPDFException" || /^Missing PDF/i.test(msg)) {
    return "The file was not found. It may have been moved, renamed or deleted.";
  }
  if (name === "InvalidPDFException" || /Invalid PDF/i.test(msg)) {
    return "The file is not a valid PDF or is damaged.";
  }
  return `The file could not be opened (${msg}).`;
}
