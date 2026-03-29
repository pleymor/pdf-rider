# Data Model: Continuous Flex Page Layout

**Feature**: 005-continuous-flex-layout
**Date**: 2026-03-25

---

## Entities

### PdfPageView

Represents a single page's DOM subtree and render state. Lives inside `pdf-viewer.ts`.

```
PdfPageView {
  pageNum: number              // 1-indexed page number
  wrapper: HTMLDivElement      // .page-wrapper[data-page="N"] — root element for the page
  canvas: HTMLCanvasElement    // .page-canvas — PDF rendered content
  textLayer: HTMLDivElement    // .page-text-layer — selectable text spans
  linkLayer: HTMLDivElement    // .page-link-layer — clickable URL anchors
  formLayer: HTMLDivElement    // .page-form-layer — interactive form inputs
  annotationCanvas: HTMLCanvasElement  // .page-annotation-canvas — user drawings
  overlay: CanvasOverlay       // annotation draw/select handler for this page
  rendered: boolean            // true if canvas has current rendered content
  rendering: boolean           // true if a render task is in-flight (prevents double-render)
}
```

**Lifecycle**:
- Created for every page when a PDF is loaded (one per page in document)
- `rendered = false` initially; wrapper sized to placeholder dimensions
- Transitions to `rendered = true` when IntersectionObserver triggers render
- Cleared (canvas wiped, layers emptied, `rendered = false`) when page scrolls out of buffer zone

---

### PageDimensions

Pre-fetched natural dimensions for each page. Used for placeholder sizing and column calculation.

```
PageDimensions {
  pageNum: number      // 1-indexed
  width: number        // pixels at scale=1.0 (PDF points × devicePixelRatio)
  height: number       // pixels at scale=1.0
}
```

**Note**: Actual rendered size = `width × scale` and `height × scale`.

---

### LayoutState

Computed layout configuration. Recalculated on container resize and zoom change.

```
LayoutState {
  columnCount: number    // 1, 2, or 3 — current number of columns
  refPageWidth: number   // widest page width (px at scale=1.0); reference for column calc
  gap: number            // pixels between columns (12px) and rows (20px)
  scale: number          // current zoom scale (e.g., 1.5)
}
```

**Column count formula**: `min(3, max(1, floor((containerWidth + gap) / (refPageWidth × scale + gap))))`

---

### AnnotationStore (unchanged)

Existing store, already keyed by page number. No changes required.

```
AnnotationStore {
  getForPage(pageNum: number): Annotation[]
  addToPage(pageNum: number, ann: Annotation): void
  removeFromPage(pageNum: number, annId: string): void
  getAllPages(): Map<number, Annotation[]>
}
```

---

## DOM Structure (new)

```html
<div id="viewer-scroll">            <!-- flex column, overflow:auto -->
  <div class="page-row">           <!-- flex row, gap:12px, justify:center -->
    <div class="page-wrapper" data-page="1">  <!-- position:relative, shadow -->
      <canvas class="page-canvas"></canvas>
      <div class="page-text-layer"></div>
      <div class="page-link-layer"></div>
      <div class="page-form-layer"></div>
      <canvas class="page-annotation-canvas"></canvas>
    </div>
    <div class="page-wrapper" data-page="2"> ... </div>  <!-- if columnCount >= 2 -->
  </div>
  <div class="page-row">
    <div class="page-wrapper" data-page="3"> ... </div>
    <div class="page-wrapper" data-page="4"> ... </div>
  </div>
  ...
</div>
```

**Placeholder state** (page not yet rendered): wrapper div has explicit `width`/`height` set from `PageDimensions × scale`; canvas is cleared; layers are empty.

---

## State Transitions

```
Page State Machine:

  [not created] ──load()──> [placeholder]
                                  │
                     enters IntersectionObserver zone
                                  │
                                  ▼
                            [rendering]
                                  │
                          render completes
                                  │
                                  ▼
                             [rendered]
                                  │
                    exits IntersectionObserver zone
                    (and is > buffer distance away)
                                  │
                                  ▼
                            [placeholder]   ◄── cleared to free memory
```

---

## Coordinate System (unchanged)

Three coordinate spaces are preserved from the existing architecture:

| Space | Origin | Y-axis | Units | Used by |
|-------|--------|--------|-------|---------|
| PDF points | bottom-left | up | pt | Annotation storage |
| Viewport pixels | top-left | down | px | pdfjs rendering |
| Canvas pixels | top-left | down | px | CanvasOverlay draw |

`models.ts` `canvasToPdf()` and `pdfToCanvas()` conversion functions remain unchanged. Each `PdfPageView` uses its own `viewport` object (obtained from `pdfDoc.getPage(n).getViewport({scale, rotation})`).
