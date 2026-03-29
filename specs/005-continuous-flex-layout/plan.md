# Implementation Plan: Continuous Flex Page Layout

**Branch**: `005-continuous-flex-layout` | **Date**: 2026-03-25 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/005-continuous-flex-layout/spec.md`

## Summary

Replace the current single-page renderer with a continuous scrolling layout that renders all pages in a single scrollable container. Pages are arranged into rows using a responsive flex layout: 1–3 columns depending on available viewer width (capped at 3). Only pages near the visible viewport are rendered (lazy/virtualized via `IntersectionObserver`). Column count recalculates automatically on window resize and zoom change. The existing annotation, text selection, link, and form systems are preserved per-page.

## Technical Context

**Language/Version**: TypeScript 5.x (frontend), Rust stable ≥1.77 (backend — unchanged)
**Primary Dependencies**: pdfjs-dist (PDF rendering), Tauri v2 (desktop shell), CanvasOverlay (internal annotation system)
**Storage**: N/A (no new persistence)
**Testing**: `cargo test` (Rust — no backend changes); manual visual testing + unit tests for layout calculation functions
**Target Platform**: Windows x86_64 desktop (Tauri v2 + WebView2)
**Project Type**: Desktop app — frontend-only change
**Performance Goals**: Layout reflow < 500ms on resize/zoom; individual page render < 200ms; memory bounded at ~(visible pages + 2 buffer) × page size
**Constraints**: Max 3 columns; lazy rendering; scroll position preserved on reflow; no new npm/crate dependencies

## Constitution Check

### Principle I — Tauri Desktop-First ✅
No changes to backend. All modifications are TypeScript frontend. No new runtime targets.

### Principle II — Single Executable Distribution ✅
No new npm packages or crates. No side-car DLLs. Build output unchanged.

### Principle III — Rust Backend / Web Frontend ✅
All layout and rendering logic resides in TypeScript frontend. No file I/O or OS calls from frontend. No new Tauri commands needed.

### Principle IV — Simplicity & YAGNI ✅ (justified)
This refactoring is substantial but directly user-requested. No abstractions for hypothetical future requirements are introduced. The `PdfPageView` class is the minimum structure needed to manage per-page DOM. The layout functions are simple arithmetic. No new files beyond what's necessary.

### Principle V — Test-Driven Development ✅
Unit tests for layout calculation functions (`calculateColumnCount`, `buildRows`) must be written first and fail before implementation. Per-page rendering logic follows TDD for the `PdfPageView` lifecycle.

*No constitution violations. Complexity Tracking table not required.*

## Project Structure

### Documentation (this feature)

```text
specs/005-continuous-flex-layout/
├── plan.md              ← this file
├── research.md          ← Phase 0 output
├── data-model.md        ← Phase 1 output
└── tasks.md             ← /speckit.tasks output (not yet created)
```

### Source Code (modified files)

```text
index.html                         ← remove static layer divs from #viewer-scroll
src/
├── main.ts                        ← update: remove syncAllLayers, update nav/annotation
├── pdf-viewer.ts                  ← major refactor: PdfPageView + layout manager
├── canvas-overlay.ts              ← minor refactor: parameterize canvas in constructor
└── styles/
    └── app.css                    ← update: replace #viewer-container styles with per-page
```

**No new files.** All changes are in existing files.

**Structure Decision**: Single-project layout unchanged. Frontend-only modifications. Rust backend untouched.

## Implementation Phases

### Phase A: Layout Calculation (pure functions — test first)

Extract two pure functions (can live at top of `pdf-viewer.ts` or a private module):

```typescript
function calculateColumnCount(containerWidth: number, refPageWidthPx: number, gap: number): number
// Returns 1–3: min(3, max(1, floor((containerWidth + gap) / (refPageWidthPx + gap))))

function buildRows(pageCount: number, columnCount: number): number[][]
// Returns array of rows; each row is array of 1-indexed page numbers
// e.g. buildRows(5, 2) → [[1,2],[3,4],[5]]
```

**Tests to write first** (cargo test equivalent: Vitest unit tests):
- `calculateColumnCount(800, 600, 12)` → 1 (only 1 page fits)
- `calculateColumnCount(1300, 600, 12)` → 2 (two pages fit)
- `calculateColumnCount(1900, 600, 12)` → 3 (three pages fit)
- `calculateColumnCount(3000, 600, 12)` → 3 (capped at 3)
- `buildRows(5, 2)` → `[[1,2],[3,4],[5]]`
- `buildRows(6, 3)` → `[[1,2,3],[4,5,6]]`
- `buildRows(1, 2)` → `[[1]]`

### Phase B: CanvasOverlay Parameterization

**Change**: `CanvasOverlay` constructor currently uses `getElementById('annotation-canvas')`. Change it to accept `HTMLCanvasElement` as a parameter.

```typescript
// Before
constructor(store: AnnotationStore) {
  this.canvas = document.getElementById('annotation-canvas') as HTMLCanvasElement;
  ...
}

// After
constructor(canvas: HTMLCanvasElement, store: AnnotationStore) {
  this.canvas = canvas;
  ...
}
```

Update the single call-site in `main.ts` to pass the element explicitly. This is a non-breaking refactor when only one page exists (current behavior preserved until full refactor).

### Phase C: PdfPageView Class

Add a `PdfPageView` class to `pdf-viewer.ts`. Responsibilities:
- Create and own the DOM subtree for one page (wrapper + 5 child elements)
- Own a `CanvasOverlay` instance for that page's annotation canvas
- Track `rendered` / `rendering` boolean state
- Expose `setPlaceholderSize(w, h)` to set wrapper dimensions before rendering
- Expose `clear()` to wipe canvas + layers (placeholder state, keeps wrapper size)
- Expose `render(pdfDoc, scale, rotation, store)` — render this page to its canvas + layers

### Phase D: HTML + CSS Changes

**`index.html`**: Remove the static children of `#viewer-scroll`:
```html
<!-- REMOVE these lines: -->
<div id="viewer-container">
  <canvas id="pdf-canvas"></canvas>
  <div id="text-layer"></div>
  <div id="link-layer"></div>
  <div id="form-layer"></div>
  <canvas id="annotation-canvas"></canvas>
</div>
```
`#viewer-scroll` becomes an empty div; rows are injected dynamically by `PdfViewer`.

**`app.css`**:
- Remove: `#viewer-container`, `#pdf-canvas`, `#text-layer`, `#link-layer`, `#form-layer`, `#annotation-canvas` rules
- Change `#viewer-scroll` from `justify-content: center; align-items: flex-start` to `flex-direction: column; align-items: center; gap: 20px`
- Add: `.page-row { display: flex; flex-direction: row; gap: 12px; justify-content: center; align-items: flex-start; }`
- Add: `.page-wrapper { position: relative; line-height: 0; box-shadow: 0 4px 20px rgba(0,0,0,0.6); }`
- Add: `.page-canvas { display: block; background: #fff; }` (and equivalent rules for `.page-text-layer`, `.page-link-layer`, `.page-form-layer`, `.page-annotation-canvas` — mirror existing `#text-layer`, etc.)

### Phase E: PdfViewer Refactor

**`PdfViewer` class changes**:

1. Remove `_currentPage`, `currentPageObj`, single-canvas rendering fields
2. Add:
   - `pageViews: PdfPageView[]`
   - `pageDimensions: { width: number; height: number }[]`
   - `_focusedPage: number` (currently-visible page, for toolbar)
   - `intersectionObserver: IntersectionObserver`
   - `resizeObserver: ResizeObserver`
3. `load(filePath)`:
   - Load PDF doc
   - Pre-fetch all page dimensions (`getPage(n).getViewport({scale: 1.0})`)
   - Create all `PdfPageView` instances
   - Build rows + inject into `#viewer-scroll`
   - Start `IntersectionObserver` and `ResizeObserver`
4. `buildLayout()`:
   - Calculate `columnCount` from current container width + ref page width
   - Call `buildRows()` to group pages
   - Clear `#viewer-scroll` and re-inject `.page-row` divs with `.page-wrapper` children
   - Set placeholder sizes on all `PdfPageView` instances
   - Re-observe all wrappers
5. `IntersectionObserver` callback:
   - Entries entering zone: schedule `pageView.render(...)` if not already rendered/rendering
   - Entries leaving zone (and far from viewport): call `pageView.clear()`
   - Update `_focusedPage` from highest-ratio visible entry
6. `ResizeObserver` callback:
   - Debounce 100ms
   - Recalculate column count; if changed, call `buildLayout()` and restore scroll to `_focusedPage`
7. `setScale(scale)`:
   - Store new scale
   - Record `_focusedPage`
   - Mark all pages as `rendered = false` (dimensions changed)
   - Rebuild placeholder sizes
   - Re-render visible pages
   - Restore scroll to `_focusedPage`
8. `goToPage(n)`:
   - `pageViews[n-1].wrapper.scrollIntoView({ behavior: 'smooth', block: 'start' })`

### Phase F: main.ts Updates

1. Remove global `syncAllLayers()` function
2. Remove the single `CanvasOverlay` instance (`overlay`) created with `getElementById`
3. Per-page annotation access: `viewer.getPageView(n).overlay` — each page manages its own overlay
4. Annotation tool mode changes: iterate `viewer.pageViews` and update each overlay's mode
5. Page navigation `prev/next/goto`: delegate to `viewer.goToPage(n)` (scroll-based)
6. Toolbar page count update: subscribe to `viewer`'s `focused-page-changed` custom event
7. Remove/update `buildFormLayer` call site — now called per-page inside `PdfPageView.render()`

### Phase G: Print Compatibility

The existing `renderAllPagesForPrint(dpi)` in `pdf-viewer.ts` renders all pages as JPEG for the Tauri print command. This is separate from the viewer canvas and does not use `PdfPageView` — it iterates pages directly. Verify it continues to work after refactor (it should, as it uses `pdfDoc` directly).

## Key Risk: Annotation Coordinate System

The annotation coordinate conversion (`canvasToPdf` / `pdfToCanvas` in `models.ts`) uses the `viewport` object of the current page. In the new architecture, each `PdfPageView` holds its own `viewport`. The event handlers in `main.ts` for annotation mouse events must use the correct page's viewport (determined by which page the mouse is over).

**Mitigation**: Mouse events on `#annotation-canvas` in the current code fire on the single global canvas. In the new code, each `PdfPageView`'s `annotationCanvas` fires its own events, scoped to that page automatically. No coordinate disambiguation needed.
