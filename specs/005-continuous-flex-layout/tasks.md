# Tasks: Continuous Flex Page Layout

**Input**: Design documents from `/specs/005-continuous-flex-layout/`
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅, quickstart.md ✅

**Organization**: Tasks grouped by user story for independent implementation and testing.
**Tests**: TDD required by constitution (Principle V) for pure functions (`calculateColumnCount`, `buildRows`); test tasks included for those functions only.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files or independent sections)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Configure frontend unit testing toolchain required by Constitution Principle V.

- [X] T001 Configure Vitest for TypeScript unit tests — add `vitest` dev dependency and `test` script to `package.json`; add `test: { include: ["src/__tests__/**"] }` to `vite.config.ts`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Pure layout functions, CanvasOverlay refactor, and HTML/CSS structural changes that all user story phases depend on.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [X] T002 [P] Write failing unit tests for `calculateColumnCount(containerWidth, refPageWidthPx, gap)` in `src/__tests__/pdf-layout.test.ts` — test cases: `(800, 600, 12)→1`, `(1300, 600, 12)→2`, `(1900, 600, 12)→3`, `(3000, 600, 12)→3` (capped at 3), `(600, 600, 12)→1` (minimum 1)
- [X] T003 [P] Write failing unit tests for `buildRows(pageCount, columnCount)` in `src/__tests__/pdf-layout.test.ts` — test cases: `(5,2)→[[1,2],[3,4],[5]]`, `(6,3)→[[1,2,3],[4,5,6]]`, `(1,2)→[[1]]`, `(4,1)→[[1],[2],[3],[4]]`
- [X] T004 Implement `calculateColumnCount(containerWidth: number, refPageWidthPx: number, gap: number): number` as exported function in `src/pdf-viewer.ts` — formula: `Math.min(3, Math.max(1, Math.floor((containerWidth + gap) / (refPageWidthPx + gap))))`; run `npx vitest run` and confirm T002 tests pass
- [X] T005 Implement `buildRows(pageCount: number, columnCount: number): number[][]` as exported function in `src/pdf-viewer.ts` — groups 1-indexed page numbers into rows of `columnCount`; run `npx vitest run` and confirm T003 tests pass
- [X] T006 [P] Refactor `CanvasOverlay` constructor in `src/canvas-overlay.ts` — replace `document.getElementById('annotation-canvas')` with an `HTMLCanvasElement` parameter: `constructor(canvas: HTMLCanvasElement, store: AnnotationStore)`; update the single call-site in `src/main.ts` to pass the element explicitly
- [X] T007 [P] Remove static viewer layer divs from `index.html` — delete `<div id="viewer-container">` and its five children (`#pdf-canvas`, `#text-layer`, `#link-layer`, `#form-layer`, `#annotation-canvas`); leave `#viewer-scroll` as an empty div
- [X] T008 [P] Replace `#viewer-container`, `#pdf-canvas`, `#text-layer`, `#link-layer`, `#form-layer`, `#annotation-canvas` CSS rules in `src/styles/app.css` with per-page classes: `.page-row { display:flex; flex-direction:row; gap:12px; justify-content:center; align-items:flex-start; }`, `.page-wrapper { position:relative; line-height:0; box-shadow:0 4px 20px rgba(0,0,0,0.6); }`, `.page-canvas { display:block; background:#fff; }`, `.page-text-layer`, `.page-link-layer`, `.page-form-layer`, `.page-annotation-canvas` (mirror `z-index` stack and `pointer-events` from the removed rules); change `#viewer-scroll` from `justify-content:center; align-items:flex-start` to `flex-direction:column; align-items:center; gap:20px; padding:20px`

**Checkpoint**: Pure functions tested and passing; CanvasOverlay parameterized; HTML/CSS structure ready for per-page injection.

---

## Phase 3: User Story 1 — Continuous Scrolling View (Priority: P1) 🎯 MVP

**Goal**: All pages of an open PDF are visible in a single scrollable container. No "Next Page" navigation required.

**Independent Test**: Open any multi-page PDF → verify all pages appear by scrolling top to bottom with no pagination controls.

- [X] T009 [US1] Add `PdfPageView` class to `src/pdf-viewer.ts` — constructor creates `.page-wrapper[data-page="N"]` div with child `.page-canvas` canvas, `.page-text-layer`, `.page-link-layer`, `.page-form-layer` divs, and `.page-annotation-canvas` canvas; creates a `CanvasOverlay` instance passing the annotation canvas; exposes `rendered: boolean`, `rendering: boolean` flags; exposes `setPlaceholderSize(width: number, height: number)` that sets wrapper width/height style
- [X] T010 [US1] Implement page dimension pre-fetching in `PdfViewer.load()` in `src/pdf-viewer.ts` — after `pdfDoc` is obtained, iterate all pages (`for i in 1..pageCount`) calling `pdfDoc.getPage(i).getViewport({scale: 1.0})` and store `{ width, height }` in `this.pageDimensions: Array<{width:number,height:number}>`
- [X] T011 [US1] Implement `PdfViewer.buildLayout()` in `src/pdf-viewer.ts` — (1-column for this story): clear `#viewer-scroll` innerHTML; create one `PdfPageView` per page; call `pageView.setPlaceholderSize(dims.width * scale, dims.height * scale)` on each; wrap each page in a `.page-row` div (single page per row); append all rows to `#viewer-scroll`; store views in `this.pageViews`
- [X] T012 [US1] Add `IntersectionObserver` in `PdfViewer` in `src/pdf-viewer.ts` — observe each `.page-wrapper` with `rootMargin: "200px 0px"`; on entry: if `!pageView.rendered && !pageView.rendering` schedule `pageView.render(pdfDoc, scale, rotation, store)`; on exit (and page index distance > 2 from any visible page): call `pageView.clear()`
- [X] T013 [US1] Implement `PdfPageView.render(pdfDoc, scale, rotation, store)` in `src/pdf-viewer.ts` — set `rendering=true`; get pdfjs page object; compute viewport; size canvas to viewport; render PDF to canvas context; build text layer (reuse existing `buildTextLayer` logic scoped to this page's `textLayer` div); build link layer (reuse existing `updateLinkLayer` logic scoped to `linkLayer`); build form layer (reuse existing `buildFormLayer` logic scoped to `formLayer`); load annotations from `store.getForPage(pageNum)` into `overlay`; set `rendered=true`, `rendering=false`
- [X] T014 [US1] Implement `PdfPageView.clear()` in `src/pdf-viewer.ts` — clear canvas context (`clearRect`); empty `textLayer`, `linkLayer`, `formLayer` innerHTML; clear overlay annotations display; set `rendered=false`; preserve wrapper `width`/`height` styles so placeholder size is maintained
- [X] T015 [US1] Implement `PdfViewer.goToPage(n: number)` in `src/pdf-viewer.ts` — call `this.pageViews[n-1].wrapper.scrollIntoView({ behavior: 'smooth', block: 'start' })`; update `this._currentPage = n`
- [X] T016 [US1] Remove global `syncAllLayers()` function from `src/main.ts`; replace annotation tool mode change handlers (pen, eraser, etc.) to iterate `viewer.pageViews` and call `pageView.overlay.setMode(mode)` on each; replace per-page annotation event wiring to attach mouse event handlers to each page's `annotationCanvas` element using `pageView.pageNum` to identify which page's store to update
- [X] T017 [US1] Update page navigation handlers in `src/main.ts` — `page-prev`: call `viewer.goToPage(viewer.currentPage - 1)`; `page-next`: call `viewer.goToPage(viewer.currentPage + 1)`; `page-goto`: call `viewer.goToPage(e.page)`;  remove all calls to the old `render()` method

**Checkpoint**: Open a multi-page PDF → all pages visible by scrolling; annotations still draw on pages; navigation controls scroll to target page.

---

## Phase 4: User Story 2 — Horizontal Page Grouping (Priority: P2)

**Goal**: When viewer is wide enough, consecutive pages appear side-by-side in rows (up to 3 columns). Layout updates automatically on resize.

**Independent Test**: Widen the viewer window until two pages appear side-by-side; narrow it back to verify single-column reflow.

- [X] T018 [US2] Wire `calculateColumnCount()` into `PdfViewer.buildLayout()` in `src/pdf-viewer.ts` — compute `refPageWidth = Math.max(...this.pageDimensions.map(d => d.width)) * scale`; compute `containerWidth = viewerScrollEl.clientWidth - 40` (subtract padding); call `calculateColumnCount(containerWidth, refPageWidth, 12)` and store result as `this.columnCount`
- [X] T019 [US2] Update `PdfViewer.buildLayout()` in `src/pdf-viewer.ts` — replace single-page-per-row logic with `buildRows(pageCount, columnCount)` call; create one `.page-row` div per returned row array; append each row's `PdfPageView` wrappers as children of that row div
- [X] T020 [US2] Verify odd-numbered last page renders without layout breakage in `src/pdf-viewer.ts` — ensure last row with fewer pages than `columnCount` still renders (no CSS change needed; flex row with fewer children naturally leaves empty space); add visual validation note in quickstart.md scenario 2
- [X] T021 [US2] Add `ResizeObserver` on `#viewer-scroll` in `src/pdf-viewer.ts` — observe the scroll container; on size change, call `buildLayout()` after a 100ms debounce (use `clearTimeout`/`setTimeout` pattern); disconnect and reconnect observer after layout rebuild
- [X] T022 [US2] Update `PdfViewer.setScale(scale)` in `src/pdf-viewer.ts` — store new scale; mark all `pageViews` as `rendered=false`; call `buildLayout()` to rebuild placeholder sizes and re-observe pages; re-render currently visible pages via IntersectionObserver trigger

**Checkpoint**: Widen window → 2 or 3 pages appear side-by-side; narrow window → single column; zoom in/out → column count adjusts.

---

## Phase 5: User Story 3 — Smooth Resize Reflow (Priority: P3)

**Goal**: When window is resized or zoom changes, the layout reflows within 500ms and the currently-visible page remains visible (within 1 page offset).

**Independent Test**: Scroll to page 10, resize window to trigger column change, verify page 10 is still visible after reflow.

- [X] T023 [US3] Implement focused-page tracking in `IntersectionObserver` callback in `src/pdf-viewer.ts` — track intersection ratios for all visible entries; after each callback, determine the page with the highest intersection ratio in the top half of the viewport; store as `this._focusedPage: number`; emit `new CustomEvent('focused-page-changed', { detail: { page: n } })` on `#viewer-scroll`
- [X] T024 [US3] Implement `PdfViewer.reflow()` in `src/pdf-viewer.ts` — save `const savedPage = this._focusedPage`; call `buildLayout()`; after layout rebuild, call `this.goToPage(savedPage)` with `behavior: 'instant'` (not smooth, to avoid visual jump during reflow)
- [X] T025 [US3] Update `ResizeObserver` callback in `src/pdf-viewer.ts` (from T021) to call `reflow()` instead of bare `buildLayout()` — preserves scroll position across resize-triggered column changes
- [X] T026 [US3] Update `PdfViewer.setScale()` in `src/pdf-viewer.ts` (from T022) to call `reflow()` instead of bare `buildLayout()` — preserves scroll position across zoom-triggered column changes

**Checkpoint**: Scroll to page 8, resize window, verify page 8 still visible; zoom in/out, verify current page stays in view.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T027 Verify `renderAllPagesForPrint()` in `src/pdf-viewer.ts` still works after refactor — it uses `pdfDoc.getPage(n)` directly (not PdfPageView); run the print flow manually and confirm all pages output correctly
- [X] T028 [P] Update `@media print` CSS in `src/styles/app.css` to target `.page-row` and `.page-wrapper` instead of `#viewer-container` — ensure box-shadow is removed and background is white for all page wrappers in print mode
- [X] T029 [P] Subscribe toolbar page indicator in `src/main.ts` to the `focused-page-changed` event from `#viewer-scroll` — replace any remaining `toolbar.updatePageInfo()` call-sites driven by explicit navigation with the event subscription
- [X] T030 Run all 6 quickstart.md test scenarios manually and confirm no regressions — continuous scroll, flex columns, reflow preserves position, zoom triggers reflow, annotations per-page, large document performance

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 — **BLOCKS all user story phases**
- **US1 (Phase 3)**: Depends on Phase 2 completion — no dependency on US2 or US3
- **US2 (Phase 4)**: Depends on Phase 2 completion + **Phase 3 (US1) completion** — column layout builds on top of continuous scroll infrastructure
- **US3 (Phase 5)**: Depends on Phase 4 (US2) completion — reflow requires column count changes to reflow from
- **Polish (Phase 6)**: Depends on all user story phases

### Within Each Phase

- T002 and T003 can run in parallel (both write to the same test file but independent test cases — write sequentially if single agent)
- T004 must follow T002 (write test, then implementation)
- T005 must follow T003
- T006, T007, T008 are fully parallel (different files)
- T009–T017 are sequential within US1 (T009 defines the class all others use)
- T018–T022 are mostly sequential within US2 (each builds on previous)
- T023–T026 are sequential within US3
- T027–T030 are mostly parallel in polish

### Parallel Opportunities

```
Phase 2 parallel group:
  T006 (canvas-overlay.ts)
  T007 (index.html)
  T008 (app.css)

Phase 2 test group (write tests before impl):
  T002 → T004  (calculateColumnCount)
  T003 → T005  (buildRows)

Phase 6 parallel group:
  T027 (print verification)
  T028 (print CSS)
  T029 (toolbar event subscription)
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Configure Vitest (T001)
2. Complete Phase 2: Foundational (T002–T008)
3. Complete Phase 3: User Story 1 — continuous single-column scroll (T009–T017)
4. **STOP and VALIDATE**: Open a PDF, scroll through all pages, draw annotations on multiple pages
5. Deliver as functional improvement — already better than current page-by-page UX

### Incremental Delivery

1. Phase 1 + 2 → infrastructure ready
2. Phase 3 (US1) → continuous single-column scroll ✓ (MVP)
3. Phase 4 (US2) → multi-column flex layout ✓
4. Phase 5 (US3) → smooth reflow with position preservation ✓
5. Phase 6 → polish and regression check ✓

---

## Notes

- [P] tasks = different files or independent; safe to run concurrently
- TDD tasks (T002–T005): run `npx vitest run` after each; tests MUST fail before implementation
- Commit after each phase checkpoint
- Constitution Principle V: tests for `calculateColumnCount` and `buildRows` MUST be committed with failing state before T004/T005 implementation commits
- `CanvasOverlay` refactor (T006) is backward-compatible — the existing `main.ts` call-site just needs to pass the element explicitly
- The `renderAllPagesForPrint` function is out of scope for layout changes (T027 is verification only)
