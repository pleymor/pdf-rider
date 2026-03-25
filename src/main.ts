import { PdfViewer } from "./pdf-viewer";
import { Toolbar } from "./toolbar";
import { CanvasOverlay } from "./canvas-overlay";
import { CompressModal } from "./compress-modal";
import { SignatureModal } from "./signature-modal";
import { SettingsModal } from "./settings";
import { getTranslations, applyTranslationsToDOM } from "./i18n";
import { AnnotationStore } from "./annotation-store";
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
  compressPdf,
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

/** Stores user-entered form field values, keyed by full PDF field name. */
const formValues = new Map<string, string>();

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
const compressModal = new CompressModal();
const sigModal = new SignatureModal();
const settingsModal = new SettingsModal();

// Apply initial translations, then re-apply on language change
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

function onFormChange(name: string, val: string): void {
  formValues.set(name, val);
  setDirty(true);
}

/** Rebuild all overlay layers after any render (zoom, page change, rotate…). */
async function syncAllLayers(): Promise<void> {
  overlay.syncToPage(
    viewer.currentPage,
    viewer.scale,
    viewer.pageHeightPt,
    store.getForPage(viewer.currentPage),
    viewer.currentViewport ?? undefined
  );
  toolbar.updatePageInfo(viewer.currentPage, viewer.pageCount);
  toolbar.updateZoom(viewer.scale);
  void updateLinkLayer();
  await viewer.buildFormLayer(formLayerDiv, formValues, onFormChange);
}

async function renderCurrentPage(): Promise<void> {
  await viewer.render();
  await syncAllLayers();
}

// Stored link rects (canvas-px coords) for the current page — used for cursor detection
let pageLinks: { url: string; left: number; top: number; right: number; bottom: number }[] = [];

const linkLayerEl  = document.getElementById("link-layer")!;
const formLayerDiv = document.getElementById("form-layer") as HTMLElement;

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
  // Positions in `combined` where WE inserted a line-break space (as opposed to
  // natural spaces that are part of the PDF text).  Knowing these lets us detect
  // when a URL match ends at our separator — meaning the URL may continue on the
  // next line — while still preventing the regex from crossing line boundaries.
  const lineBreakPos = new Set<number>();
  let prevTop = NaN;
  for (const span of textLayerEl.querySelectorAll("span") as NodeListOf<HTMLSpanElement>) {
    const text = span.textContent ?? "";
    if (!text) continue;
    const spanTop = parseFloat(span.style.top) || 0;
    // Insert a space between spans on different lines so the URL regex cannot
    // accidentally match across a line boundary into the next line's first word.
    if (combined.length > 0 && Math.abs(spanTop - prevTop) > 2) {
      lineBreakPos.add(combined.length);
      combined += " ";
    }
    const start = combined.length;
    combined += text;
    entries.push({
      span,
      start,
      end: combined.length,
      text,
      left:     parseFloat(span.style.left)     || 0,
      top:      spanTop,
      fontSize: parseFloat(span.style.fontSize) || 12,
    });
    prevTop = spanTop;
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

  // Scan visible text for URLs; group rects per URL.
  // Store startPos so we can look for continuation spans after a line-break space.
  const urlRe = /https?:\/\/[^\s)\]>"]+/g;
  let m: RegExpExecArray | null;
  type DomEntry = { startPos: number; rects: Rect[] };
  const domLinks = new Map<string, DomEntry>();
  while ((m = urlRe.exec(combined)) !== null) {
    const urlEnd = m.index + m[0].length;
    const rects = domRectsForRange(m.index, urlEnd);
    if (rects.length > 0) {
      if (!domLinks.has(m[0])) domLinks.set(m[0], { startPos: m.index, rects });
      else domLinks.get(m[0])!.rects.push(...rects);
    }

    // If the URL ends exactly at one of our injected line-break spaces, the URL
    // may wrap onto the next line.  Check the immediately-following span: if its
    // text looks like a URL fragment (no whitespace, contains a URL-specific char
    // like . / - _ ? # = &, and is reasonably short), treat it as a continuation.
    if (!lineBreakPos.has(urlEnd)) continue;
    for (const e of entries) {
      if (e.start < urlEnd + 1) continue;
      if (e.start > urlEnd + 2) break;
      // Extract leading URL characters from the span text
      const fragMatch = /^[^\s)\]>"]+/.exec(e.text);
      if (!fragMatch) break;
      const frag = fragMatch[0];
      // Must be short (URL remainders) and contain at least one URL-special char
      // to rule out plain words that happen to follow the URL on the next line.
      if (frag.length >= 2 && frag.length <= 40 && /[./\-_?#=&%]/.test(frag)) {
        const fullUrl = m[0] + frag;
        if (!domLinks.has(fullUrl)) {
          const contRects = domRectsForRange(e.start, e.start + frag.length);
          domLinks.set(fullUrl, {
            startPos: m.index,
            rects: [...(rects.length > 0 ? rects : []), ...contRects],
          });
        }
      }
      break;
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

    // Find best match in domLinks (exact first, then longest common prefix)
    let bestDomUrl = "";
    let bestEntry: DomEntry | undefined;
    for (const [domUrl, entry] of domLinks) {
      if (domUrl === url) { bestDomUrl = domUrl; bestEntry = entry; break; }
      const n = Math.min(domUrl.length, url.length);
      if (n >= 10 && domUrl.slice(0, n) === url.slice(0, n) && domUrl.length > bestDomUrl.length) {
        bestDomUrl = domUrl;
        bestEntry = entry;
      }
    }

    if (bestEntry) {
      const allRects = [...bestEntry.rects];

      // Partial match: a line-break space cut the URL before it was fully matched.
      // Check whether the span immediately after the space is a clean URL continuation
      // (i.e. the span is almost entirely the remaining URL fragment, not a long line
      // that merely starts with the same chars).  This recovers the second-line hit rect
      // for multi-line URLs while avoiding false extensions into unrelated next-line text.
      if (bestDomUrl.length < url.length) {
        const remainder = url.slice(bestDomUrl.length);
        const matchEnd = bestEntry.startPos + bestDomUrl.length;
        for (const e of entries) {
          if (e.start < matchEnd + 1) continue; // before the space
          if (e.start > matchEnd + 2) break;    // too far ahead
          // Entry starts right after the injected space separator
          const remLen = Math.min(remainder.length, e.text.length);
          if (
            remLen >= 4 &&
            e.text.slice(0, remLen) === remainder.slice(0, remLen) &&
            e.text.length <= remLen + 2  // span is almost entirely the URL remainder
          ) {
            const clipR = e.left + measure(e.text.slice(0, remLen), e.span);
            if (clipR > e.left) {
              allRects.push({ left: e.left, top: e.top, right: clipR, bottom: e.top + e.fontSize });
            }
          }
          break;
        }
      }

      addLink(url, allRects);
    } else {
      const [x1, y1, x2, y2] = rect;
      addLink(url, [{ left: Math.min(x1, x2), top: Math.min(y1, y2), right: Math.max(x1, x2), bottom: Math.max(y1, y2) }]);
    }
  }

  // Add DOM-discovered URLs not covered by a formal annotation
  for (const [url, { rects }] of domLinks) {
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
    formValues.clear();
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
    const fv: FormFieldValue[] = [...formValues.entries()].map(([name, value]) => ({ name, value }));
    await saveAnnotatedPdf(filePath, target, store.getAll(), viewer.rotation, fv);
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

// ── Compress handler ──────────────────────────────────────────────────────────

compressModal.onConfirm(async (level) => {
  if (!filePath) return;
  const outputPath = await savePdfDialog(filePath);
  if (!outputPath) return;

  try {
    const result = await compressPdf(filePath, outputPath, level);
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
        await syncAllLayers();
      }
      break;

    case "zoom-in":
    case "zoom-out":
      if (viewer.isLoaded()) {
        await viewer.setScale(snapZoom(viewer.scale, e.type === "zoom-in" ? 1 : -1));
        await syncAllLayers();
      }
      break;

    case "zoom-set":
      if (viewer.isLoaded()) {
        await viewer.setScale(e.scale);
        await syncAllLayers();
      }
      break;

    case "fit-width":
      if (viewer.isLoaded()) {
        const scroll = document.getElementById("viewer-scroll")!;
        await viewer.setScale((scroll.clientWidth - 40) / viewer.pageWidthPt);
        await syncAllLayers();
      }
      break;

    case "fit-height":
      if (viewer.isLoaded()) {
        const scroll = document.getElementById("viewer-scroll")!;
        await viewer.setScale((scroll.clientHeight - 40) / viewer.pageHeightPt);
        await syncAllLayers();
      }
      break;

    case "page-prev":
      if (viewer.isLoaded()) { await viewer.prevPage();  await syncAllLayers(); }
      break;

    case "page-next":
      if (viewer.isLoaded()) { await viewer.nextPage();  await syncAllLayers(); }
      break;

    case "page-goto":
      if (viewer.isLoaded()) { await viewer.goToPage(e.page); await syncAllLayers(); }
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

    case "settings":
      settingsModal.open();
      break;
  }
});

// ── Keyboard shortcuts ────────────────────────────────────────────────────────

document.addEventListener("keydown", async (e) => {
  // Don't fire while typing in any input or contenteditable
  const tag = (document.activeElement as HTMLElement)?.tagName ?? "";
  if (tag === "INPUT" || tag === "TEXTAREA" ||
      (document.activeElement as HTMLElement)?.isContentEditable) return;

  const ctrl  = e.ctrlKey || e.metaKey;
  const shift = e.shiftKey;

  // Ctrl+O — Open
  if (ctrl && !shift && e.key === "o") {
    e.preventDefault();
    if (await confirmUnsaved()) await openFile();
    return;
  }
  // Ctrl+Shift+S — Save As  (must be checked before plain Ctrl+S)
  if (ctrl && shift && e.key === "S") {
    e.preventDefault();
    await saveFile(true);
    return;
  }
  // Ctrl+S — Save
  if (ctrl && !shift && e.key === "s") {
    e.preventDefault();
    await saveFile(false);
    return;
  }
  // Ctrl+P — Print
  if (ctrl && !shift && e.key === "p") {
    e.preventDefault();
    window.print();
    return;
  }
  // Ctrl+Shift+E — Compress
  if (ctrl && shift && e.key === "E") {
    e.preventDefault();
    if (filePath) compressModal.open();
    return;
  }
  // Ctrl++ / Ctrl+= — Zoom in
  if (ctrl && (e.key === "+" || e.key === "=")) {
    e.preventDefault();
    if (viewer.isLoaded()) { await viewer.setScale(snapZoom(viewer.scale, 1));  await syncAllLayers(); }
    return;
  }
  // Ctrl+- — Zoom out
  if (ctrl && e.key === "-") {
    e.preventDefault();
    if (viewer.isLoaded()) { await viewer.setScale(snapZoom(viewer.scale, -1)); await syncAllLayers(); }
    return;
  }
  // Ctrl+0 — Reset zoom to 100%
  if (ctrl && e.key === "0") {
    e.preventDefault();
    if (viewer.isLoaded()) { await viewer.setScale(1.0); await syncAllLayers(); }
    return;
  }
  // ArrowRight / PageDown — Next page
  if (!ctrl && (e.key === "ArrowRight" || e.key === "PageDown")) {
    if (viewer.isLoaded()) { e.preventDefault(); await viewer.nextPage();  await syncAllLayers(); }
    return;
  }
  // ArrowLeft / PageUp — Previous page
  if (!ctrl && (e.key === "ArrowLeft" || e.key === "PageUp")) {
    if (viewer.isLoaded()) { e.preventDefault(); await viewer.prevPage();  await syncAllLayers(); }
    return;
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
// Ctrl++/- zooms in/out
window.addEventListener("keydown", async (e: KeyboardEvent) => {
  if (e.ctrlKey && (e.key === "+" || e.key === "=" || e.key === "-")) {
    if (!viewer.isLoaded()) return;
    e.preventDefault();
    await viewer.setScale(snapZoom(viewer.scale, e.key === "-" ? -1 : 1));
    await syncAllLayers();
    return;
  }
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

// Ctrl+wheel zooms the document
document.getElementById("viewer-scroll")!.addEventListener("wheel", async (e: WheelEvent) => {
  if (!e.ctrlKey || !viewer.isLoaded()) return;
  e.preventDefault();
  await viewer.setScale(snapZoom(viewer.scale, e.deltaY < 0 ? 1 : -1));
  await syncAllLayers();
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
