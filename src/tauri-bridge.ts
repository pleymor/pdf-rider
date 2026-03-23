import { invoke } from "@tauri-apps/api/core";
import type { Annotation } from "./models";

/** Opens a file-picker dialog filtered to PDF files. Returns the path or null. */
export async function openPdfDialog(): Promise<string | null> {
  return invoke<string | null>("open_pdf_dialog");
}

/** Opens a Save As dialog. Returns the chosen path or null. */
export async function savePdfDialog(
  currentPath: string
): Promise<string | null> {
  return invoke<string | null>("save_pdf_dialog", {
    currentPath,
  });
}

/** Returns the number of pages in the PDF at `filePath`. */
export async function getPageCount(filePath: string): Promise<number> {
  return invoke<number>("get_page_count", { filePath });
}

/**
 * Stores `annotations` as JSON metadata in the PDF at `inputPath` and saves
 * the result to `outputPath`. Annotations are NOT burned into the content
 * stream, so they remain fully editable when the file is reopened.
 */
export async function saveAnnotatedPdf(
  inputPath: string,
  outputPath: string,
  annotations: Annotation[],
  rotationDelta = 0,
): Promise<void> {
  return invoke<void>("save_annotated_pdf", {
    inputPath,
    outputPath,
    annotations,
    rotationDelta,
  });
}

/**
 * Reads editable annotations stored in the PDF's CCAnnot catalog entry.
 * Returns an empty array if the file has no stored annotations.
 */
export async function readAnnotations(
  filePath: string
): Promise<Annotation[]> {
  return invoke<Annotation[]>("read_annotations", { filePath });
}

export interface StartupArgs {
  filePath: string | null;
  shouldPrint: boolean;
}

/** Returns the CLI arguments parsed at startup (file path and print flag). */
export async function getStartupArgs(): Promise<StartupArgs> {
  return invoke<StartupArgs>("get_startup_args");
}

/** Returns true if our ProgID is already registered in HKCU. */
export async function checkPdfAssociation(): Promise<boolean> {
  return invoke<boolean>("check_pdf_association");
}

/** Writes ProgID and .pdf association into HKCU (no admin required). */
export async function registerPdfHandler(): Promise<void> {
  return invoke<void>("register_pdf_handler");
}

/** Silently registers the print verb under both ProgIDs (called at every startup). */
export async function registerPrintVerb(): Promise<void> {
  return invoke<void>("register_print_verb");
}

/** Opens Windows Settings → Default Apps. */
export async function openDefaultAppsSettings(): Promise<void> {
  return invoke<void>("open_default_apps_settings");
}
