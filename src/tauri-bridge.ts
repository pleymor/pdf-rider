import { invoke } from "@tauri-apps/api/core";
import type { Annotation, FormFieldValue } from "./models";

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
  formFields: FormFieldValue[] = [],
): Promise<void> {
  return invoke<void>("save_annotated_pdf", {
    inputPath,
    outputPath,
    annotations,
    rotationDelta,
    formFields,
  });
}

/** Creates a copy of the PDF with annotation content streams emptied (display use).
 *  CCAnnot metadata is preserved. Returns true if streams were found and cleared. */
export async function stripAnnotationStreams(
  inputPath: string,
  outputPath: string,
): Promise<boolean> {
  return invoke<boolean>("strip_annotation_streams", { inputPath, outputPath });
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

/** Removes all HKCU registry entries created by registerPdfHandler. */
export async function unregisterPdfHandler(): Promise<void> {
  return invoke<void>("unregister_pdf_handler");
}

/** Registers the print verb under both ProgIDs. */
export async function registerPrintVerb(): Promise<void> {
  return invoke<void>("register_print_verb");
}

/** Extracts selected pages from a PDF into a new file, preserving vector text. */
export async function extractPdfPages(
  inputPath: string,
  outputPath: string,
  pages: number[],
): Promise<void> {
  return invoke<void>("extract_pdf_pages", { inputPath, outputPath, pages });
}

export interface PrinterList {
  printers: string[];
  defaultPrinter: string;
}

/** Returns the list of available printers and the default printer name. */
export async function listPrinters(): Promise<PrinterList> {
  return invoke<PrinterList>("list_printers");
}

/** Prints a PDF file natively to a named printer via the system handler. */
export async function printPdfFile(
  filePath: string,
  printerName: string,
  copies?: number,
): Promise<void> {
  return invoke<void>("print_pdf_file", {
    filePath,
    printerName,
    copies: copies ?? null,
  });
}

/** Sends pre-rendered page images (base64 JPEG) to a printer. */
export async function printPages(
  pagesB64: string[],
  printerName?: string,
  copies?: number,
  orientation?: string,
  fitMode?: string,
): Promise<void> {
  return invoke<void>("print_pages", {
    pagesB64,
    printerName: printerName ?? null,
    copies: copies ?? null,
    orientation: orientation ?? null,
    fitMode: fitMode ?? null,
  });
}

/** Opens Windows Settings → Default Apps. */
export async function openDefaultAppsSettings(): Promise<void> {
  return invoke<void>("open_default_apps_settings");
}

/** Opens an http/https URL in the system default browser. */
export async function openUrl(url: string): Promise<void> {
  return invoke<void>("open_url", { url });
}

export interface PageOperation {
  page: number;
  rotation: number;
  delete: boolean;
}

/** Applies per-page rotation and deletion to a PDF, then saves. */
export async function modifyPages(
  inputPath: string,
  outputPath: string,
  operations: PageOperation[],
): Promise<void> {
  return invoke<void>("modify_pages", { inputPath, outputPath, operations });
}

export interface CompressResult {
  originalBytes: number;
  compressedBytes: number;
}

/**
 * Compresses JPEG image streams in `inputPath` at the given quality level
 * and writes the result to `outputPath`. Returns both file sizes in bytes.
 */
export async function compressPdf(
  inputPath: string,
  outputPath: string,
  level: "screen" | "ebook" | "print",
): Promise<CompressResult> {
  return invoke<CompressResult>("compress_pdf", { inputPath, outputPath, level });
}
