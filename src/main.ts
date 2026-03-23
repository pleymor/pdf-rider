import { PdfViewer } from "./pdf-viewer";
import { Toolbar } from "./toolbar";
import { CanvasOverlay } from "./canvas-overlay";
import { SignatureModal } from "./signature-modal";
import { AnnotationStore } from "./annotation-store";
import {
  defaultToolState,
  type Annotation,
} from "./models";
import {
  openPdfDialog,
  savePdfDialog,
  saveAnnotatedPdf,
  readAnnotations,
} from "./tauri-bridge";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask } from "@tauri-apps/plugin-dialog";

// ── State ─────────────────────────────────────────────────────────────────────

let filePath: string | null = null;
let outputPath: string | null = null;
let isDirty = false;
let pendingSignature: string | null = null; // base64 PNG while in placing mode

// ── Instances ─────────────────────────────────────────────────────────────────

const viewer = new PdfViewer();
const store = new AnnotationStore();
const toolState = defaultToolState();
const toolbar = new Toolbar();
const overlay = new CanvasOverlay(toolState);
const sigModal = new SignatureModal();

// ── Helpers ───────────────────────────────────────────────────────────────────

function showToast(msg: string, isError = false): void {
  const toast = document.getElementById("toast")!;
  toast.textContent = msg;
  toast.className = `toast${isError ? " error" : ""}`;
  setTimeout(() => toast.classList.add("hidden"), 3000);
}

function setDirty(val: boolean): void {
  isDirty = val;
}

async function renderCurrentPage(): Promise<void> {
  await viewer.render();
  overlay.syncToPage(
    viewer.currentPage,
    viewer.scale,
    viewer.pageHeightPt,
    store.getForPage(viewer.currentPage)
  );
  toolbar.updatePageInfo(viewer.currentPage, viewer.pageCount);
}

async function promptPassword(fp: string): Promise<void> {
  const pw = window.prompt("This PDF is password-protected. Enter password:");
  if (pw === null) return; // cancelled
  try {
    await viewer.loadWithPassword(fp, pw);
    store.clear();
    filePath = fp;
    outputPath = null;
    setDirty(false);
    await renderCurrentPage();
  } catch {
    showToast("Wrong password or could not open file.", true);
  }
}

async function loadPdf(path: string): Promise<void> {
  try {
    await viewer.load(path);
    store.clear();
    filePath = path;
    outputPath = null;

    // Restore any previously saved annotations from the PDF catalog
    const saved = await readAnnotations(path);
    for (const ann of saved) {
      store.add(ann);
    }
    // Mark as burned so the overlay doesn't double-render them
    // (they are already visible via the burned PDF content stream)
    overlay.markBurned(saved);

    setDirty(false);
    await renderCurrentPage();
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
    // Save: write directly to the last save path, or overwrite the source file
    target = outputPath ?? filePath;
  }

  try {
    await saveAnnotatedPdf(filePath, target, store.getAll());
    filePath = target; // subsequent saves use the saved file (stream IDs live there)
    setDirty(false);

    // Reload the PDF canvas from the saved file so the freshly burned annotations
    // are shown by pdf.js. Then mark everything burned so the overlay doesn't double.
    const savedPage = viewer.currentPage;
    await viewer.load(target);
    if (savedPage !== 1) await viewer.goToPage(savedPage);
    await renderCurrentPage();
    overlay.markBurned(store.getAll());

    showToast("Saved.");
  } catch (err: unknown) {
    showToast(`Save failed: ${err}`, true);
  }
}

async function confirmUnsaved(): Promise<boolean> {
  if (!isDirty) return true;
  return ask("Discard unsaved changes and continue?", {
    title: "Unsaved changes",
    kind: "warning",
  });
}

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

    case "zoom-in":
      if (viewer.isLoaded()) {
        await viewer.adjustZoom(0.25);
        overlay.syncToPage(
          viewer.currentPage,
          viewer.scale,
          viewer.pageHeightPt,
          store.getForPage(viewer.currentPage)
        );
      }
      break;

    case "zoom-out":
      if (viewer.isLoaded()) {
        await viewer.adjustZoom(-0.25);
        overlay.syncToPage(
          viewer.currentPage,
          viewer.scale,
          viewer.pageHeightPt,
          store.getForPage(viewer.currentPage)
        );
      }
      break;

    case "page-prev":
      if (viewer.isLoaded()) {
        await viewer.prevPage();
        overlay.syncToPage(
          viewer.currentPage,
          viewer.scale,
          viewer.pageHeightPt,
          store.getForPage(viewer.currentPage)
        );
        toolbar.updatePageInfo(viewer.currentPage, viewer.pageCount);
      }
      break;

    case "page-next":
      if (viewer.isLoaded()) {
        await viewer.nextPage();
        overlay.syncToPage(
          viewer.currentPage,
          viewer.scale,
          viewer.pageHeightPt,
          store.getForPage(viewer.currentPage)
        );
        toolbar.updatePageInfo(viewer.currentPage, viewer.pageCount);
      }
      break;

    case "page-goto":
      if (viewer.isLoaded()) {
        await viewer.goToPage(e.page);
        overlay.syncToPage(
          viewer.currentPage,
          viewer.scale,
          viewer.pageHeightPt,
          store.getForPage(viewer.currentPage)
        );
        toolbar.updatePageInfo(viewer.currentPage, viewer.pageCount);
      }
      break;

    case "tool-change":
      overlay.setTool(e.tool);
      break;

    case "style-change":
      overlay.setStyle(e.style);
      break;

    case "signature":
      sigModal.open();
      break;
  }
});

// ── Annotation events ─────────────────────────────────────────────────────────

overlay.onAnnotationCreated((ann: Annotation) => {
  store.add(ann);
  setDirty(true);
  toolbar.clearActiveTool();
});

overlay.onAnnotationMoved(() => {
  // Position mutated in place on the shared reference — just mark dirty
  setDirty(true);
});

overlay.onAnnotationRemoved((ann: Annotation) => {
  store.removeRef(ann);
  setDirty(true);
});

overlay.onAnnotationReordered((ann: Annotation, dir) => {
  if (dir === "front") store.bringToFront(ann);
  else store.sendToBack(ann);
  setDirty(true);
});

// ── Signature events ──────────────────────────────────────────────────────────

sigModal.onSignatureReady((imageData: string) => {
  pendingSignature = imageData;
  const container = document.getElementById("viewer-container")!;
  container.style.cursor = "crosshair";
  showToast("Click on the page to place your signature. Press Esc to cancel.");
});

// Click on viewer to place signature
document.getElementById("viewer-container")!.addEventListener("click", (e: MouseEvent) => {
  if (!pendingSignature) return;

  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
  const x = e.clientX - rect.left;
  const y = e.clientY - rect.top;

  // Default signature size: 150 × 60 PDF points
  const ann = overlay.placeSignature(x, y, pendingSignature, 150, 60);
  store.add(ann);
  setDirty(true);

  pendingSignature = null;
  (e.currentTarget as HTMLElement).style.cursor = "";
  toolbar.clearActiveTool();
});

// Escape cancels pending signature or deselects active drawing tool
window.addEventListener("keydown", (e: KeyboardEvent) => {
  if (e.key !== "Escape") return;
  if (pendingSignature) {
    pendingSignature = null;
    document.getElementById("viewer-container")!.style.cursor = "";
    showToast("Signature placement cancelled.");
  } else if (overlay.currentTool !== "select") {
    overlay.setTool("select");
    toolbar.clearActiveTool();
  }
});

// ── Window close guard + drag-drop ────────────────────────────────────────────

(async () => {
  try {
    const appWindow = getCurrentWindow();

    await appWindow.onCloseRequested(async (event) => {
      if (isDirty) {
        event.preventDefault();
        const ok = await ask("Close without saving?", {
          title: "Unsaved changes",
          kind: "warning",
        });
        if (ok) await appWindow.destroy();
      }
    });

    await appWindow.onDragDropEvent(async (event) => {
      if (event.payload.type === "over") {
        document.body.classList.add("drag-over");
      } else if (event.payload.type === "leave" || event.payload.type === "cancel") {
        document.body.classList.remove("drag-over");
      } else if (event.payload.type === "drop") {
        document.body.classList.remove("drag-over");
        const pdfPath = event.payload.paths.find((p: string) =>
          p.toLowerCase().endsWith(".pdf")
        );
        if (pdfPath) {
          if (await confirmUnsaved()) loadPdf(pdfPath);
        } else if (event.payload.paths.length > 0) {
          showToast("Only PDF files can be opened.", true);
        }
      }
    });
  } catch {
    // Fallback for dev server / browser preview
    window.addEventListener("beforeunload", (e) => {
      if (isDirty) {
        e.preventDefault();
        e.returnValue = "";
      }
    });
  }
})();
