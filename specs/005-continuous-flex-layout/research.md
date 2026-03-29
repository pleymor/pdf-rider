# Research: Continuous Flex Page Layout

**Feature**: 005-continuous-flex-layout
**Date**: 2026-03-25

---

## Decision 1: Virtual Scrolling Strategy

**Decision**: IntersectionObserver with placeholder divs (option B from spec)

**Rationale**: IntersectionObserver is a browser-native API with no dependencies. It fires asynchronously off the main thread, making it ideal for triggering page renders as the user approaches them. Placeholder divs (sized from pre-fetched page dimensions) maintain correct scroll height so the scrollbar doesn't jump, while off-screen pages are cleared from memory.

**Alternatives considered**:
- Scroll event + getBoundingClientRect: synchronous, causes layout thrash on every scroll
- Eager rendering all pages: too slow for 50+ page PDFs; large memory usage
- Progressive rendering (top-to-bottom queue): no way to skip ahead; user must wait for earlier pages

**Implementation**: Each `.page-wrapper` div is observed. `rootMargin: "200px 0px"` extends the observable zone ~200px above/below the viewport, giving a one-page look-ahead buffer. When a page enters the zone it's scheduled for rendering; when it leaves (and is > 2 pages from the viewport) it's cleared to free memory.

---

## Decision 2: Column Count Calculation

**Decision**: Compute columns dynamically from container width and reference page width; cap at 3

**Rationale**: `columnCount = Math.min(3, Math.max(1, Math.floor((containerWidth + gap) / (refPageWidth + gap))))` where `refPageWidth` is the widest page in the document. This ensures we never place more pages than can comfortably fit, and never exceed the user-defined cap of 3.

**Reference page width**: Use the widest page (in PDF points) as the reference. Since most PDFs have uniform page sizes, this is effectively the page width. For mixed-size documents (e.g., landscape inserts) this ensures columns never overflow.

**Gap**: 12px between columns and 20px between rows (matches existing padding of `#viewer-scroll`).

**Alternatives considered**:
- Average page width: misleading for documents with outlier pages
- First page width: wrong for documents where page 1 is a cover of a different size
- Hardcoded breakpoints: inflexible, doesn't respond to font/zoom changes

---

## Decision 3: Page Dimension Pre-Fetching

**Decision**: Load all page dimensions at document open time before any rendering

**Rationale**: `pdfjs` supports `pdfDoc.getPage(n).getViewport({scale: 1.0})` without rendering anything to canvas. This gives natural width/height for all pages cheaply. Pre-fetching upfront allows sizing all placeholder divs immediately, so the scrollbar height and layout are correct from the first frame.

**Cost**: For a 100-page PDF, fetching all viewports adds ~10–50ms (CPU only, no GPU). Acceptable at load time.

**Alternatives considered**:
- Fetch on demand: placeholder sizes unknown until pages are nearby; scrollbar jumps
- Assume uniform size based on page 1: breaks for mixed-orientation documents

---

## Decision 4: CanvasOverlay Parameterization

**Decision**: Change `CanvasOverlay` constructor to accept `HTMLCanvasElement` directly instead of using `document.getElementById`

**Rationale**: The current `CanvasOverlay` uses `getElementById('annotation-canvas')` internally. With per-page canvases, each `PdfPageView` creates its own annotation canvas element and passes it to a new `CanvasOverlay` instance. No other logic changes in `CanvasOverlay`.

**Alternatives considered**:
- Single shared CanvasOverlay moved between pages: annotations would only be visible on the current "active" page; spec requires annotations visible on all pages simultaneously
- Keep getElementById and add a setter: unnecessary complexity

---

## Decision 5: "Current Page" Tracking (for Toolbar)

**Decision**: Use IntersectionObserver intersection ratios to determine the topmost ≥50% visible page

**Rationale**: Maintains the toolbar "Page N of M" display. The page with the highest intersection ratio in the top half of the viewport is reported as the current page. This is updated whenever intersection changes.

**Alternatives considered**:
- Scroll event + manual calculation: layout-dependent, brittle with multi-column
- No current-page tracking: breaks page indicator in toolbar

---

## Decision 6: Page Navigation ("Go to Page N")

**Decision**: Keep existing next/prev/goto toolbar controls; have them `scrollIntoView()` the target page wrapper

**Rationale**: Spec says "no pagination controls required to advance" (scrolling is sufficient), but doesn't prohibit navigation controls. Keeping them with scroll-based behavior preserves usability and avoids regressions. The implementation changes from "re-render the page" to "smooth scroll to the page's wrapper div."

---

## Existing Code Impact Summary

| File | Change Type | Reason |
|------|-------------|--------|
| `src/pdf-viewer.ts` | Major refactor | Add PdfPageView, layout manager, lazy rendering |
| `src/canvas-overlay.ts` | Minor refactor | Parameterize canvas element in constructor |
| `src/main.ts` | Moderate refactor | Remove syncAllLayers, update annotation+nav integration |
| `src/styles/app.css` | Moderate update | Replace single-container styles with per-page styles |
| `index.html` | Minor update | Remove static layer divs from viewer-scroll |
| `src/models.ts` | Minimal | No changes required (coordinate system unchanged) |
| `src-tauri/` | No change | No backend involvement |
