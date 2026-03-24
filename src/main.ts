import { PdfViewer } from "./pdf-viewer";
import { Toolbar } from "./toolbar";
import { CanvasOverlay } from "./canvas-overlay";
import { SignatureModal } from "./signature-modal";
import { AnnotationStore } from "./annotation-store";
import {
  defaultToolState,
  type Annotation,
  type CircleAnnotation,
  type RectAnnotation,
  type TextAnnotation,
} from "./models";
import {
  openPdfDialog,
  savePdfDialog,
  saveAnnotatedPdf,
  readAnnotations,
  getStartupArgs,
  checkPdfAssociation,
  registerPdfHandler,
  registerPrintVerb,
  printPages,
  openUrl,
} from "./tauri-bridge";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask } from "@tauri-apps/plugin-dialog";

// ── State ─────────────────────────────────────────────────────────────────────

const appWindow = getCurrentWindow();
let unlistenClose: (() => void) | null = null;

let filePath: string | null = null;
let outputPath: string | null = null;
let isDirty = false;
let pendingSignature: string | null = null; // base64 PNG while in placing mode
let editingTextAnn:  TextAnnotation | null = null;
let editingShapeAnn: RectAnnotation | CircleAnnotation | null = null;

// ── Instances ─────────────────────────────────────────────────────────────────

const viewer = new PdfViewer();
const store = new AnnotationStore();
const toolState = defaultToolState();
const toolbar = new Toolbar();
const overlay = new CanvasOverlay(toolState);
const sigModal = new SignatureModal();

// Hide viewer until a PDF is loaded
document.getElementById("viewer-container")!.style.display = "none";

// ── Startup handling ──────────────────────────────────────────────────────────

(async () => {
  try {
    void registerPrintVerb(); // always register print verb silently

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
    } else {
      const associated = await checkPdfAssociation();
      if (!associated) {
        const ok = await ask(
          "PDF Reader is not registered as a PDF handler.\n\n" +
            "Register now? This adds 'Open' and 'Print' verbs to Explorer's context menu " +
            "and sets PDF Reader as your default PDF viewer.",
          { title: "Register as PDF viewer", kind: "info" }
        );
        if (ok) {
          try {
            await registerPdfHandler();
            showToast(
              "Registered. To confirm as default, check Windows Settings → Default Apps."
            );
          } catch (e) {
            showToast(`Registration failed: ${e}`, true);
          }
        }
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
  if (val) {
    void registerCloseGuard();
  } else {
    unregisterCloseGuard();
  }
}

async function registerCloseGuard(): Promise<void> {
  if (unlistenClose) return;
  unlistenClose = await appWindow.onCloseRequested(async (event) => {
    event.preventDefault();
    const ok = await ask("Close without saving?", {
      title: "Unsaved changes",
      kind: "warning",
    });
    if (ok) {
      unlistenClose?.();
      unlistenClose = null;
      await appWindow.destroy();
    }
  });
}

function unregisterCloseGuard(): void {
  unlistenClose?.();
  unlistenClose = null;
}

async function renderCurrentPage(): Promise<void> {
  await viewer.render();
  overlay.syncToPage(
    viewer.currentPage,
    viewer.scale,
    viewer.pageHeightPt,
    store.getForPage(viewer.currentPage),
    viewer.currentViewport ?? undefined
  );
  toolbar.updatePageInfo(viewer.currentPage, viewer.pageCount);
  void updateLinkLayer();
}

// Stored link rects (canvas-px coords) for the current page — used for cursor detection
let pageLinks: { url: string; left: number; top: number; right: number; bottom: number }[] = [];

const linkLayerEl = document.getElementById("link-layer")!;

async function updateLinkLayer(): Promise<void> {
  const canvas = document.getElementById("pdf-canvas") as HTMLCanvasElement;
  linkLayerEl.innerHTML = "";
  linkLayerEl.style.width  = canvas.style.width;
  linkLayerEl.style.height = canvas.style.height;
  pageLinks = [];

  // Use the inline CSS styles set by buildTextLayer (left, top, fontSize, transform)
  // to compute URL hit rects.  This avoids getBoundingClientRect() coordinate
  // ambiguity: span.style.left is already in CSS-px relative to #viewer-container,
  // which is the same coordinate space as our #link-layer children.
  const textLayerEl = document.getElementById("text-layer") as HTMLElement;
  const mc = document.createElement("canvas").getContext("2d")!;

  const measure = (text: string, span: HTMLSpanElement): number => {
    mc.font = `${span.style.fontSize} ${span.style.fontFamily || "sans-serif"}`;
    return mc.measureText(text).width;
  };

  interface SpanEntry {
    span: HTMLSpanElement;
    start: number;
    end: number;
    text: string;
    left: number;     // parseFloat(span.style.left)  — CSS px from viewer-container left
    top: number;      // parseFloat(span.style.top)   — CSS px from viewer-container top
    fontSize: number; // parseFloat(span.style.fontSize)
  }

  let combined = "";
  const entries: SpanEntry[] = [];
  for (const span of textLayerEl.querySelectorAll("span") as NodeListOf<HTMLSpanElement>) {
    const text = span.textContent ?? "";
    if (!text) continue;
    const start = combined.length;
    combined += text;
    entries.push({
      span,
      start,
      end: combined.length,
      text,
      left:     parseFloat(span.style.left)     || 0,
      top:      parseFloat(span.style.top)      || 0,
      fontSize: parseFloat(span.style.fontSize) || 12,
    });
  }

  // Return one tight rect per span that intersects the URL range.
  // Multi-line URLs get one rect per line (one per span), NOT a merged union — a union
  // would span from the wrapped line's left margin to the first line's right edge,
  // creating an enormous hit area that covers non-URL text.
  // scaleX is intentionally NOT applied: PDFs sometimes set item.width to the full
  // paragraph width (to define the link annotation area), which would make scaleX >> 1
  // and inflate hit rects far beyond the visible URL text.
  type Rect = { left: number; top: number; right: number; bottom: number };
  const domRectsForRange = (urlStart: number, urlEnd: number): Rect[] => {
    const rects: Rect[] = [];
    for (const e of entries) {
      if (e.end <= urlStart || e.start >= urlEnd) continue;
      const urlL = Math.max(e.start, urlStart) - e.start;
      const urlR = Math.min(e.end,   urlEnd)   - e.start;
      const clipL = e.left + measure(e.text.slice(0, urlL), e.span);
      const clipR = e.left + measure(e.text.slice(0, urlR), e.span);
      if (clipR > clipL) {
        rects.push({ left: clipL, top: e.top, right: clipR, bottom: e.top + e.fontSize });
      }
    }
    return rects;
  };

  // Scan visible text for URLs; group rects per URL
  const urlRe = /https?:\/\/[^\s)\]>"]+/g;
  let m: RegExpExecArray | null;
  const domLinks = new Map<string, Rect[]>();
  while ((m = urlRe.exec(combined)) !== null) {
    const rects = domRectsForRange(m.index, m.index + m[0].length);
    if (rects.length > 0) {
      if (!domLinks.has(m[0])) domLinks.set(m[0], []);
      domLinks.get(m[0])!.push(...rects);
    }
  }

  // Merge with formal PDF link annotations.
  // DOM-found rects take priority (one <a> per line segment).
  // Annotation rect is fallback for image/button links with no visible URL text.
  const annotations = await viewer.getPageLinkAnnotations();
  const addedUrls = new Set<string>();

  const addLink = (url: string, rects: Rect[]) => {
    addedUrls.add(url);
    for (const r of rects) pageLinks.push({ url, ...r });
  };

  for (const { url, rect } of annotations) {
    if (addedUrls.has(url)) continue;
    let domRects: Rect[] | undefined;
    for (const [domUrl, rects] of domLinks) {
      if (domUrl === url) { domRects = rects; break; }
      const n = Math.min(domUrl.length, url.length);
      if (n >= 10 && domUrl.slice(0, n) === url.slice(0, n)) { domRects = rects; }
    }
    if (domRects) {
      addLink(url, domRects);
    } else {
      const [x1, y1, x2, y2] = rect;
      addLink(url, [{ left: Math.min(x1, x2), top: Math.min(y1, y2), right: Math.max(x1, x2), bottom: Math.max(y1, y2) }]);
    }
  }

  // Add DOM-discovered URLs not covered by a formal annotation
  for (const [url, rects] of domLinks) {
    if (!addedUrls.has(url)) addLink(url, rects);
  }

  // Render one <a> per line segment
  for (const link of pageLinks) {
    const a = document.createElement("a");
    a.style.cssText = [
      `left:${link.left}px`,
      `top:${link.top}px`,
      `width:${link.right - link.left}px`,
      `height:${link.bottom - link.top}px`,
    ].join(";");
    a.addEventListener("click", (e) => {
      e.preventDefault();
      void openUrl(link.url);
    });
    linkLayerEl.appendChild(a);
  }
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
    toolbar.setLoaded(true);
    document.getElementById("viewer-container")!.style.display = "";
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
    toolbar.setLoaded(true);
    document.getElementById("viewer-container")!.style.display = "";
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
    await saveAnnotatedPdf(filePath, target, store.getAll(), viewer.rotation);
    filePath = target; // subsequent saves use the saved file (stream IDs live there)
    setDirty(false);

    // Reload the PDF canvas from the saved file so the freshly burned annotations
    // are shown by pdf.js. The rotation was baked into the PDF's Rotate entry, so
    // after load (which resets _rotation to 0) the page renders correctly via page.rotate.
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

    case "rotate":
      if (viewer.isLoaded()) {
        await viewer.rotate();
        setDirty(true);
        overlay.syncToPage(
          viewer.currentPage,
          viewer.scale,
          viewer.pageHeightPt,
          store.getForPage(viewer.currentPage),
          viewer.currentViewport ?? undefined
        );
      }
      break;

    case "zoom-in":
      if (viewer.isLoaded()) {
        await viewer.adjustZoom(0.25);
        overlay.syncToPage(
          viewer.currentPage,
          viewer.scale,
          viewer.pageHeightPt,
          store.getForPage(viewer.currentPage),
          viewer.currentViewport ?? undefined
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
          store.getForPage(viewer.currentPage),
          viewer.currentViewport ?? undefined
        );
      }
      break;

    case "fit-width":
      if (viewer.isLoaded()) {
        const scroll = document.getElementById("viewer-scroll")!;
        const avail = scroll.clientWidth - 40; // minus 2×20px padding
        const newScale = avail / viewer.pageWidthPt;
        await viewer.setScale(newScale);
        overlay.syncToPage(
          viewer.currentPage,
          viewer.scale,
          viewer.pageHeightPt,
          store.getForPage(viewer.currentPage),
          viewer.currentViewport ?? undefined
        );
        void updateLinkLayer();
      }
      break;

    case "fit-height":
      if (viewer.isLoaded()) {
        const scroll = document.getElementById("viewer-scroll")!;
        const avail = scroll.clientHeight - 40; // minus 2×20px padding
        const newScale = avail / viewer.pageHeightPt;
        await viewer.setScale(newScale);
        overlay.syncToPage(
          viewer.currentPage,
          viewer.scale,
          viewer.pageHeightPt,
          store.getForPage(viewer.currentPage),
          viewer.currentViewport ?? undefined
        );
        void updateLinkLayer();
      }
      break;

    case "page-prev":
      if (viewer.isLoaded()) {
        await viewer.prevPage();
        overlay.syncToPage(
          viewer.currentPage,
          viewer.scale,
          viewer.pageHeightPt,
          store.getForPage(viewer.currentPage),
          viewer.currentViewport ?? undefined
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
          store.getForPage(viewer.currentPage),
          viewer.currentViewport ?? undefined
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
          store.getForPage(viewer.currentPage),
          viewer.currentViewport ?? undefined
        );
        toolbar.updatePageInfo(viewer.currentPage, viewer.pageCount);
      }
      break;

    case "tool-change":
      overlay.setTool(e.tool);
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
      Object.assign(toolState, e.style); // always persist for next annotation
      overlay.setStyle(toolState);       // always sync overlay draw style
      if (editingTextAnn) {
        overlay.applyTextAnnotationStyle(editingTextAnn, e.style);
        setDirty(true);
      } else if (editingShapeAnn) {
        overlay.applyShapeAnnotationStyle(editingShapeAnn, e.style.color, e.style.strokeWidth);
        setDirty(true);
      }
      break;

    case "layer-change": {
      const ann = editingTextAnn ?? editingShapeAnn;
      if (ann) {
        if (e.dir === "front") store.bringToFront(ann);
        else store.sendToBack(ann);
        overlay.reorderTextAnnotation(ann, e.dir);
        setDirty(true);
      }
      break;
    }

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

overlay.onTextAnnotationSelected((ann: TextAnnotation | null) => {
  editingTextAnn = ann;
  if (ann) {
    editingShapeAnn = null; // mutual exclusion: deselect shape
    toolbar.showTextStyles(ann);
    toolbar.hideShapeStyles();
  } else if (overlay.currentTool !== "text") {
    toolbar.hideTextStyles();
  }
});

overlay.onShapeAnnotationSelected((ann: RectAnnotation | CircleAnnotation | null) => {
  editingShapeAnn = ann;
  if (ann) {
    editingTextAnn = null; // mutual exclusion: deselect text
    toolbar.showShapeStyles(ann);
    toolbar.hideTextStyles();
  } else if (overlay.currentTool !== "rect" && overlay.currentTool !== "circle") {
    toolbar.hideShapeStyles();
  }
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

// ── Context menu ──────────────────────────────────────────────────────────────

const ctxMenu = document.getElementById("ctx-menu")!;
const ctxCopyBtn = document.getElementById("ctx-copy") as HTMLButtonElement;

document.addEventListener("contextmenu", (e: MouseEvent) => {
  e.preventDefault();
  const sel = window.getSelection()?.toString() ?? "";
  ctxCopyBtn.disabled = sel.length === 0;
  ctxMenu.style.left = `${e.clientX}px`;
  ctxMenu.style.top = `${e.clientY}px`;
  ctxMenu.classList.remove("hidden");
});

ctxCopyBtn.addEventListener("click", async () => {
  const text = window.getSelection()?.toString() ?? "";
  if (text) await navigator.clipboard.writeText(text);
  ctxMenu.classList.add("hidden");
});

// Close context menu on any click outside it
document.addEventListener("mousedown", (e: MouseEvent) => {
  if (!ctxMenu.contains(e.target as Node)) {
    ctxMenu.classList.add("hidden");
  }
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
    editingTextAnn  = null;
    editingShapeAnn = null;
    toolbar.hideTextStyles();
    toolbar.hideShapeStyles();
  }
});

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
  }
})();
