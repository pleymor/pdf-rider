import { PdfViewer, PdfPageView } from "./pdf-viewer";
import { Toolbar } from "./toolbar";
import { CompressModal } from "./compress-modal";
import { SignatureModal } from "./signature-modal";
import { SettingsModal } from "./settings";
import { getTranslations, applyTranslationsToDOM } from "./i18n";
import { AnnotationStore } from "./annotation-store";
import { AnnotationHistory } from "./annotation-history";
import {
  defaultToolState,
  type Annotation,
  type CircleAnnotation,
  type FormFieldValue,
  type RectAnnotation,
  type TextAnnotation,
} from "./models";
import {
  openPdfDialog,
  savePdfDialog,
  saveAnnotatedPdf,
  stripAnnotationStreams,
  compressPdf,
  readAnnotations,
  getStartupArgs,
  printPages,
} from "./tauri-bridge";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { tempDir, basename } from "@tauri-apps/api/path";
import { ask } from "@tauri-apps/plugin-dialog";

// ── Zoom levels ───────────────────────────────────────────────────────────────

const ZOOM_LEVELS = [0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0];

function snapZoom(current: number, dir: 1 | -1): number {
  if (dir === 1) {
    return ZOOM_LEVELS.find(z => z > current + 0.005) ?? ZOOM_LEVELS[ZOOM_LEVELS.length - 1];
  } else {
    return [...ZOOM_LEVELS].reverse().find(z => z < current - 0.005) ?? ZOOM_LEVELS[0];
  }
}

// ── State ─────────────────────────────────────────────────────────────────────

const appWindow = getCurrentWindow();
let unlistenClose: (() => void) | null = null;

const formValues = new Map<string, string>();

let filePath: string | null = null;
let outputPath: string | null = null;
let displayFilePath: string | null = null;
let isDirty = false;
let guardRegistering = false;
let pendingSignature: string | null = null;
let editingTextAnn:  TextAnnotation | null = null;
let editingShapeAnn: RectAnnotation | CircleAnnotation | null = null;

// ── Instances ─────────────────────────────────────────────────────────────────

const viewer = new PdfViewer();
const store  = new AnnotationStore();
const toolState = defaultToolState();
const toolbar   = new Toolbar();
const history   = new AnnotationHistory();
const compressModal  = new CompressModal();
const sigModal       = new SignatureModal();
const settingsModal  = new SettingsModal();

function onFormChange(name: string, val: string): void {
  formValues.set(name, val);
  setDirty(true);
}

viewer.setup(toolState, store, formValues, onFormChange);

// ── Per-page overlay event wiring ─────────────────────────────────────────────

function wirePageOverlay(pv: PdfPageView): void {
  const ov = pv.overlay;

  ov.onAnnotationCreated((ann: Annotation) => {
    history.push(store.getAll());
    store.add(ann);
    setDirty(true);
    toolbar.clearActiveTool();
  });

  ov.onBeforeModify(() => history.push(store.getAll()));

  ov.onAnnotationMoved(() => setDirty(true));

  ov.onAnnotationRemoved((ann: Annotation) => {
    store.removeRef(ann);
    setDirty(true);
  });

  ov.onAnnotationReordered((ann: Annotation, dir) => {
    if (dir === "front") store.bringToFront(ann);
    else store.sendToBack(ann);
    setDirty(true);
  });

  ov.onTextAnnotationSelected((ann: TextAnnotation | null) => {
    editingTextAnn = ann;
    if (ann) {
      editingShapeAnn = null;
      toolbar.showTextStyles(ann);
      toolbar.hideShapeStyles();
    } else if (ov.currentTool !== "text") {
      toolbar.hideTextStyles();
    }
  });

  ov.onShapeAnnotationSelected((ann: RectAnnotation | CircleAnnotation | null) => {
    editingShapeAnn = ann;
    if (ann) {
      editingTextAnn = null;
      toolbar.showShapeStyles(ann);
      toolbar.hideTextStyles();
    } else if (ov.currentTool !== "rect" && ov.currentTool !== "circle") {
      toolbar.hideShapeStyles();
    }
  });
}

viewer.onLayoutChanged((pageViews) => {
  for (const pv of pageViews) {
    wirePageOverlay(pv);
    pv.overlay.setTool(toolState.tool);
    pv.overlay.setStyle(toolState);
  }
});

// ── Focused-page tracking → toolbar page indicator ────────────────────────────

document.getElementById("viewer-scroll")!.addEventListener("focused-page-changed", (e: Event) => {
  const { page } = (e as CustomEvent<{ page: number }>).detail;
  toolbar.updatePageInfo(page, viewer.pageCount);
});

// ── Translations ──────────────────────────────────────────────────────────────

{
  const t = getTranslations(settingsModal.getSettings().language);
  toolbar.applyTranslations(t);
  compressModal.applyTranslations(t);
  applyTranslationsToDOM(t);
}
settingsModal.onChange(s => {
  const t = getTranslations(s.language);
  toolbar.applyTranslations(t);
  compressModal.applyTranslations(t);
  applyTranslationsToDOM(t);
});

// ── Startup handling ──────────────────────────────────────────────────────────

(async () => {
  try {
    const startup = await getStartupArgs();
    if (startup.filePath) {
      if (startup.shouldPrint) {
        document.getElementById("toolbar-container")!.style.display = "none";
        await loadPdf(startup.filePath);
        const pages = await viewer.renderAllPagesForPrint(200);
        await printPages(pages);
        await appWindow.close();
      } else {
        await loadPdf(startup.filePath);
      }
    }
  } catch {
    // Not running in Tauri (browser preview) — skip
  }
})();

// ── Helpers ───────────────────────────────────────────────────────────────────

function showToast(msg: string, isError = false): void {
  const toast = document.getElementById("toast")!;
  toast.textContent = msg;
  toast.className = `toast${isError ? " error" : ""}`;
  setTimeout(() => toast.classList.add("hidden"), 3000);
}

function setDirty(val: boolean): void {
  isDirty = val;
  if (val) { void registerCloseGuard(); }
  else      { unregisterCloseGuard(); }
}

async function registerCloseGuard(): Promise<void> {
  // guardRegistering is a synchronous flag that prevents a second async call
  // from registering a second listener before the first await resolves.
  if (unlistenClose || guardRegistering) return;
  guardRegistering = true;
  try {
    unlistenClose = await appWindow.onCloseRequested(async (event) => {
      // Re-check isDirty: may have been cleared (e.g. save) before this listener
      // finished registering, or by a concurrent "yes" click.
      if (!isDirty) { return; /* no preventDefault → window closes normally */ }
      event.preventDefault();
      const ok = await ask("Close without saving?", { title: "Unsaved changes", kind: "warning" });
      if (ok) {
        // Set isDirty=false BEFORE close() so that if close() re-triggers this
        // handler, the !isDirty guard above returns early instead of looping.
        setDirty(false);
        await appWindow.close();
      }
    });
  } finally {
    guardRegistering = false;
  }
}

function unregisterCloseGuard(): void {
  unlistenClose?.();
  unlistenClose = null;
}

/** Re-sync annotation overlays for all currently-rendered pages. */
function syncAllPageOverlays(): void {
  for (const pv of viewer.pageViews) {
    if (!pv.rendered || !pv.viewport) continue;
    pv.overlay.syncToPage(
      pv.pageNum,
      viewer.scale,
      pv.viewport.height / viewer.scale,
      store.getForPage(pv.pageNum),
      pv.viewport
    );
  }
}

async function confirmUnsaved(): Promise<boolean> {
  if (!isDirty) return true;
  return ask("Discard unsaved changes and continue?", { title: "Unsaved changes", kind: "warning" });
}

// ── Load / Save ───────────────────────────────────────────────────────────────

async function promptPassword(fp: string): Promise<void> {
  const pw = window.prompt("This PDF is password-protected. Enter password:");
  if (pw === null) return;
  try {
    store.clear();
    formValues.clear();
    const saved = await readAnnotations(fp);
    for (const ann of saved) store.add(ann);

    await viewer.loadWithPassword(fp, pw);
    filePath = fp;
    outputPath = null;
    displayFilePath = null;
    history.clear();
    setDirty(false);
    toolbar.setLoaded(true);
    toolbar.updatePageInfo(1, viewer.pageCount);
    toolbar.updateZoom(viewer.scale);
  } catch {
    showToast("Wrong password or could not open file.", true);
  }
}

async function loadPdf(path: string): Promise<void> {
  try {
    store.clear();
    formValues.clear();

    // Read annotations BEFORE viewer.load() so IntersectionObserver finds them populated
    const saved = await readAnnotations(path);
    for (const ann of saved) store.add(ann);

    let loadPath = path;
    if (saved.length > 0) {
      const tmp  = await tempDir();
      const base = await basename(path);
      const dp   = `${tmp}pdf-rider-display-${base}`;
      await stripAnnotationStreams(path, dp);
      displayFilePath = dp;
      loadPath = dp;
    } else {
      displayFilePath = null;
    }

    filePath = path;
    outputPath = null;

    await viewer.load(loadPath);

    history.clear();
    setDirty(false);
    toolbar.setLoaded(true);
    toolbar.updatePageInfo(1, viewer.pageCount);
    toolbar.updateZoom(viewer.scale);
  } catch (err: unknown) {
    const msg = String(err);
    if (msg.includes("PasswordException") || msg.includes("password")) {
      await promptPassword(path);
    } else {
      showToast(`Could not open file: ${msg}`, true);
    }
  }
}

async function openFile(): Promise<void> {
  const path = await openPdfDialog();
  if (!path) return;
  await loadPdf(path);
}

async function saveFile(forceDialog: boolean): Promise<void> {
  if (!filePath) return;
  let target: string;
  if (forceDialog) {
    const picked = await savePdfDialog(filePath);
    if (!picked) return;
    outputPath = picked;
    target = picked;
  } else {
    target = outputPath ?? filePath;
  }
  try {
    const fv: FormFieldValue[] = [...formValues.entries()].map(([name, value]) => ({ name, value }));
    await saveAnnotatedPdf(displayFilePath ?? filePath, target, store.getAll(), viewer.rotation, fv);
    filePath = target;
    setDirty(false);
    showToast("Saved.");
  } catch (err: unknown) {
    showToast(`Save failed: ${err}`, true);
  }
}

// ── Compress handler ──────────────────────────────────────────────────────────

compressModal.onConfirm(async (level) => {
  if (!filePath) return;
  const out = await savePdfDialog(filePath);
  if (!out) return;
  try {
    const result = await compressPdf(filePath, out, level);
    const fromMB = (result.originalBytes   / 1_048_576).toFixed(1);
    const toMB   = (result.compressedBytes / 1_048_576).toFixed(1);
    const pct    = result.originalBytes > 0
      ? Math.round((1 - result.compressedBytes / result.originalBytes) * 100)
      : 0;
    showToast(`Compressed: ${fromMB} MB \u2192 ${toMB} MB (${pct}% smaller)`);
  } catch (err) {
    showToast(`Compression failed: ${err}`, true);
  }
});

// ── Toolbar events ────────────────────────────────────────────────────────────

toolbar.on(async (e) => {
  switch (e.type) {
    case "open":
      if (await confirmUnsaved()) await openFile();
      break;

    case "save":
      await saveFile(false);
      break;

    case "save-as":
      await saveFile(true);
      break;

    case "compress":
      if (filePath) compressModal.open();
      break;

    case "rotate":
      if (viewer.isLoaded()) {
        await viewer.rotate();
        setDirty(true);
      }
      break;

    case "zoom-in":
    case "zoom-out":
      if (viewer.isLoaded()) {
        await viewer.setScale(snapZoom(viewer.scale, e.type === "zoom-in" ? 1 : -1));
        toolbar.updateZoom(viewer.scale);
      }
      break;

    case "zoom-set":
      if (viewer.isLoaded()) {
        await viewer.setScale(e.scale);
        toolbar.updateZoom(viewer.scale);
      }
      break;

    case "fit-width":
      if (viewer.isLoaded()) {
        const scroll = document.getElementById("viewer-scroll")!;
        await viewer.setScale((scroll.clientWidth - 40) / viewer.pageWidthPt);
        toolbar.updateZoom(viewer.scale);
      }
      break;

    case "fit-height":
      if (viewer.isLoaded()) {
        const scroll = document.getElementById("viewer-scroll")!;
        await viewer.setScale((scroll.clientHeight - 40) / viewer.pageHeightPt);
        toolbar.updateZoom(viewer.scale);
      }
      break;

    case "page-prev":
      if (viewer.isLoaded()) viewer.goToPage(viewer.currentPage - viewer.columnCount);
      break;

    case "page-next":
      if (viewer.isLoaded()) viewer.goToPage(viewer.currentPage + viewer.columnCount);
      break;

    case "page-goto":
      if (viewer.isLoaded()) viewer.goToPage(e.page);
      break;

    case "tool-change":
      toolState.tool = e.tool;
      for (const pv of viewer.pageViews) pv.overlay.setTool(e.tool);
      editingTextAnn  = null;
      editingShapeAnn = null;
      if (e.tool === "text") {
        toolbar.showTextStyles(toolState);
        toolbar.hideShapeStyles();
      } else if (e.tool === "rect" || e.tool === "circle") {
        toolbar.showShapeStyles(toolState);
        toolbar.hideTextStyles();
      } else {
        toolbar.hideTextStyles();
        toolbar.hideShapeStyles();
      }
      break;

    case "style-change":
      Object.assign(toolState, e.style);
      for (const pv of viewer.pageViews) pv.overlay.setStyle(toolState);
      if (editingTextAnn) {
        const pv = viewer.pageViews.find(p => p.pageNum === editingTextAnn!.page);
        pv?.overlay.applyTextAnnotationStyle(editingTextAnn, e.style);
        setDirty(true);
      } else if (editingShapeAnn) {
        const pv = viewer.pageViews.find(p => p.pageNum === editingShapeAnn!.page);
        pv?.overlay.applyShapeAnnotationStyle(editingShapeAnn, e.style.color, e.style.strokeWidth);
        setDirty(true);
      }
      break;

    case "layer-change": {
      const ann = editingTextAnn ?? editingShapeAnn;
      if (ann) {
        if (e.dir === "front") store.bringToFront(ann);
        else store.sendToBack(ann);
        const pv = viewer.pageViews.find(p => p.pageNum === ann.page);
        pv?.overlay.reorderTextAnnotation(ann, e.dir);
        setDirty(true);
      }
      break;
    }

    case "signature":
      sigModal.open();
      break;

    case "settings":
      settingsModal.open();
      break;
  }
});

// ── Keyboard shortcuts ────────────────────────────────────────────────────────

document.addEventListener("keydown", async (e) => {
  const tag = (document.activeElement as HTMLElement)?.tagName ?? "";
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" ||
      (document.activeElement as HTMLElement)?.isContentEditable) return;

  const ctrl  = e.ctrlKey || e.metaKey;
  const shift = e.shiftKey;

  if (ctrl && !shift && e.key === "z") {
    e.preventDefault();
    const prev = history.undo(store.getAll());
    if (prev) { store.replaceAll(prev); syncAllPageOverlays(); setDirty(true); }
    return;
  }
  if (ctrl && (e.key === "y" || (shift && e.key === "Z"))) {
    e.preventDefault();
    const next = history.redo(store.getAll());
    if (next) { store.replaceAll(next); syncAllPageOverlays(); setDirty(true); }
    return;
  }
  if (ctrl && !shift && e.key === "o") {
    e.preventDefault();
    if (await confirmUnsaved()) await openFile();
    return;
  }
  if (ctrl && shift && e.key === "S") {
    e.preventDefault();
    await saveFile(true);
    return;
  }
  if (ctrl && !shift && e.key === "s") {
    e.preventDefault();
    await saveFile(false);
    return;
  }
  if (ctrl && !shift && e.key === "p") {
    e.preventDefault();
    window.print();
    return;
  }
  if (ctrl && shift && e.key === "E") {
    e.preventDefault();
    if (filePath) compressModal.open();
    return;
  }
  if (ctrl && (e.key === "+" || e.key === "=")) {
    e.preventDefault();
    if (viewer.isLoaded()) { await viewer.setScale(snapZoom(viewer.scale, 1));  toolbar.updateZoom(viewer.scale); }
    return;
  }
  if (ctrl && e.key === "-") {
    e.preventDefault();
    if (viewer.isLoaded()) { await viewer.setScale(snapZoom(viewer.scale, -1)); toolbar.updateZoom(viewer.scale); }
    return;
  }
  if (ctrl && e.key === "0") {
    e.preventDefault();
    if (viewer.isLoaded()) { await viewer.setScale(1.0); toolbar.updateZoom(viewer.scale); }
    return;
  }
  if (!ctrl && (e.key === "ArrowRight" || e.key === "PageDown")) {
    if (viewer.isLoaded()) { e.preventDefault(); viewer.goToPage(viewer.currentPage + viewer.columnCount); }
    return;
  }
  if (!ctrl && (e.key === "ArrowLeft" || e.key === "PageUp")) {
    if (viewer.isLoaded()) { e.preventDefault(); viewer.goToPage(viewer.currentPage - viewer.columnCount); }
    return;
  }
});

// ── Signature events ──────────────────────────────────────────────────────────

sigModal.onSignatureReady((imageData: string) => {
  pendingSignature = imageData;
  document.getElementById("viewer-scroll")!.style.cursor = "crosshair";
  showToast("Click on the page to place your signature. Press Esc to cancel.");
});

// Click on a page wrapper to place signature
document.getElementById("viewer-scroll")!.addEventListener("click", (e: MouseEvent) => {
  if (!pendingSignature) return;
  const wrapper = (e.target as HTMLElement).closest(".page-wrapper") as HTMLElement | null;
  if (!wrapper) return;

  const pageNum = parseInt(wrapper.dataset.page ?? "0");
  const pv = viewer.pageViews.find(p => p.pageNum === pageNum);
  if (!pv) return;

  const rect = wrapper.getBoundingClientRect();
  const x = e.clientX - rect.left;
  const y = e.clientY - rect.top;

  history.push(store.getAll());
  const ann = pv.overlay.placeSignature(x, y, pendingSignature, 150, 60);
  store.add(ann);
  setDirty(true);

  pendingSignature = null;
  document.getElementById("viewer-scroll")!.style.cursor = "";
  toolbar.clearActiveTool();
});

// ── Context menu ──────────────────────────────────────────────────────────────

const ctxMenu    = document.getElementById("ctx-menu")!;
const ctxCopyBtn = document.getElementById("ctx-copy") as HTMLButtonElement;

document.addEventListener("contextmenu", (e: MouseEvent) => {
  e.preventDefault();
  const sel = window.getSelection()?.toString() ?? "";
  ctxCopyBtn.disabled = sel.length === 0;
  ctxMenu.style.left = `${e.clientX}px`;
  ctxMenu.style.top  = `${e.clientY}px`;
  ctxMenu.classList.remove("hidden");
});

ctxCopyBtn.addEventListener("click", async () => {
  const text = window.getSelection()?.toString() ?? "";
  if (text) await navigator.clipboard.writeText(text);
  ctxMenu.classList.add("hidden");
});

document.addEventListener("mousedown", (e: MouseEvent) => {
  if (!ctxMenu.contains(e.target as Node)) ctxMenu.classList.add("hidden");
});

// ── Escape / zoom keyboard shortcuts (window-level) ──────────────────────────

window.addEventListener("keydown", async (e: KeyboardEvent) => {
  if (e.ctrlKey && (e.key === "+" || e.key === "=" || e.key === "-")) {
    if (!viewer.isLoaded()) return;
    e.preventDefault();
    await viewer.setScale(snapZoom(viewer.scale, e.key === "-" ? -1 : 1));
    toolbar.updateZoom(viewer.scale);
    return;
  }
  if (e.key !== "Escape") return;
  if (pendingSignature) {
    pendingSignature = null;
    document.getElementById("viewer-scroll")!.style.cursor = "";
    showToast("Signature placement cancelled.");
  } else if (toolState.tool !== "select") {
    toolState.tool = "select";
    for (const pv of viewer.pageViews) pv.overlay.setTool("select");
    toolbar.clearActiveTool();
    editingTextAnn  = null;
    editingShapeAnn = null;
    toolbar.hideTextStyles();
    toolbar.hideShapeStyles();
  }
});

// ── Ctrl+wheel zoom ───────────────────────────────────────────────────────────

document.getElementById("viewer-scroll")!.addEventListener("wheel", async (e: WheelEvent) => {
  if (!e.ctrlKey || !viewer.isLoaded()) return;
  e.preventDefault();
  await viewer.setScale(snapZoom(viewer.scale, e.deltaY < 0 ? 1 : -1));
  toolbar.updateZoom(viewer.scale);
}, { passive: false });

// ── Drag-drop ─────────────────────────────────────────────────────────────────

(async () => {
  try {
    await appWindow.onDragDropEvent(async (event) => {
      if (event.payload.type === "over") {
        document.body.classList.add("drag-over");
      } else if (event.payload.type === "leave") {
        document.body.classList.remove("drag-over");
      } else if (event.payload.type === "drop") {
        document.body.classList.remove("drag-over");
        const pdfPath = event.payload.paths.find((p: string) => p.toLowerCase().endsWith(".pdf"));
        if (pdfPath) {
          if (await confirmUnsaved()) void loadPdf(pdfPath);
        } else if (event.payload.paths.length > 0) {
          showToast("Only PDF files can be opened.", true);
        }
      }
    });
  } catch {
    // Fallback for dev server / browser preview
  }
})();
