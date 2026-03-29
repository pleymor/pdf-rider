# Feature Specification: Continuous Flex Page Layout

**Feature Branch**: `005-continuous-flex-layout`
**Created**: 2026-03-25
**Status**: Draft
**Input**: User description: "currently, the pdf is displayed page by page. I'd prefer to render the whole document, like in any other PDF reader. Additionally, I'd like to render it in a flex way: if we have enough width for multiple consecutive pages, align them horizontally"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Continuous Scrolling View (Priority: P1)

A user opens a PDF document and can scroll through all pages continuously without any page-by-page navigation. All pages are visible in a single scrollable view, just like mainstream PDF readers (Adobe Acrobat, browser PDF viewers, Preview).

**Why this priority**: This is the core experience change requested. The current page-by-page model is unusual and disorienting compared to every other PDF reader. Fixing this delivers the most user value.

**Independent Test**: Open any multi-page PDF and verify all pages appear stacked vertically and are accessible by scrolling — no pagination controls needed.

**Acceptance Scenarios**:

1. **Given** a multi-page PDF is open, **When** the user opens the document, **Then** all pages are rendered and visible in a single scrollable container
2. **Given** a continuously rendered PDF, **When** the user scrolls down, **Then** subsequent pages appear without any user interaction beyond scrolling
3. **Given** a large PDF (50+ pages), **When** the user scrolls to a page, **Then** that page is visible and legible without requiring "next page" navigation

---

### User Story 2 - Horizontal Page Grouping When Wide Enough (Priority: P2)

When the viewer window is wide enough to display two or more pages side-by-side, consecutive pages are grouped horizontally in rows. As the window narrows, pages revert to a single-column layout. This mirrors how PDF readers like Adobe Acrobat handle "Two Page View" but done responsively based on available space.

**Why this priority**: This is the flex layout behavior explicitly requested. It improves reading comfort for wide screens and documents like books/magazines that benefit from two-page spreads.

**Independent Test**: Resize the viewer window to be wide enough for two pages, verify two pages appear side-by-side per row; then narrow the window and verify pages return to a single-column layout.

**Acceptance Scenarios**:

1. **Given** a PDF is open and the viewer is wide enough for two pages, **When** the page layout is displayed, **Then** pages appear in pairs side-by-side in each row
2. **Given** two pages are displayed side-by-side, **When** the user resizes the window to be narrower, **Then** pages reflow to a single-column layout without requiring a page reload
3. **Given** a document with an odd number of pages displayed two-per-row, **When** the last row has only one page, **Then** it is displayed alone without breaking the layout

---

### User Story 3 - Smooth Resize Reflow (Priority: P3)

When the user resizes the viewer window, the page layout reflows smoothly so that the number of columns adjusts without losing the user's reading position.

**Why this priority**: A responsive layout that jumps or resets scroll position on resize would frustrate users. Maintaining position on reflow completes the experience.

**Independent Test**: Scroll to page 10 of a document, resize the window to trigger a layout change, verify page 10 is still visible and the user's reading position is approximately preserved.

**Acceptance Scenarios**:

1. **Given** the user is reading a specific page, **When** the window is resized and column count changes, **Then** the currently visible page remains visible after reflow
2. **Given** a window resize that changes from 2-column to 1-column layout, **When** reflow occurs, **Then** there is no full page reload and rendering artifacts do not appear

---

### Edge Cases

- What happens when the PDF has only one page? (Single page should display centered; no flex grouping applies)
- How does the layout handle pages of very different sizes within the same document? (Each row accommodates the tallest page in that row; pages are centered vertically within their row)
- What happens when the window is resized while a PDF is loading? (Layout applies correctly once pages are rendered)
- How do existing annotation tools interact with the new layout? (Annotations remain positioned correctly on their respective pages regardless of layout)
- What happens with landscape pages that exceed the viewer width? (Pages scale down to fit within the available column width, maintaining aspect ratio)

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The viewer MUST render all pages of a PDF document in a single scrollable view, not page by page
- **FR-002**: Pages MUST be arranged in rows, with each row containing as many pages as fit horizontally within the current viewer width, up to a maximum of 3 columns
- **FR-003**: When only one page fits in the available width, pages MUST be arranged in a single vertical column
- **FR-004**: When two or more pages fit in the available width, consecutive pages MUST be grouped horizontally in rows (maximum 3 pages per row)
- **FR-005**: The layout MUST reflow automatically when the viewer is resized or the zoom level changes, adjusting the number of columns without requiring the user to reload or navigate
- **FR-006**: The currently visible page or reading position MUST be approximately preserved when a resize- or zoom-triggered reflow changes the column count
- **FR-007**: Each page MUST scale proportionally to fit within its column, preserving aspect ratio
- **FR-008**: Pages that are smaller than the column width MUST be centered within the column
- **FR-009**: An odd-numbered last page in a multi-column row MUST display without layout breakage
- **FR-010**: Vertical spacing between rows MUST be consistent and visually clear so rows are distinguishable while maintaining a continuous reading flow

### Key Entities

- **Page**: A single rendered page of the PDF document, with a natural width and height. Scales to fit its assigned column.
- **Row**: A horizontal grouping of one or more consecutive pages. Width is determined by the viewer container; height is determined by the tallest page in the row.
- **Column**: A slot within a row. Number of columns per row is determined by how many page-widths fit in the available viewer width.
- **Viewer Container**: The scrollable area that holds all rows of pages. Determines available width for layout calculation.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All pages of a PDF are visible by scrolling — no pagination controls are required to advance through the document
- **SC-002**: On a display wide enough for two standard-sized pages, at least two pages appear side-by-side per row
- **SC-003**: When the viewer window is resized or zoom level changes, the layout reflows within 500ms and no visible rendering artifacts remain
- **SC-004**: The user's reading position (currently visible page) is preserved within one page offset after a resize- or zoom-triggered reflow
- **SC-005**: A document with 100 pages can be opened and scrolled through without perceptible lag; only pages near the viewport are rendered at any time, keeping memory usage bounded regardless of document length

## Clarifications

### Session 2026-03-25

- Q: Should pages be rendered eagerly (all upfront), lazily (only visible + buffer), or progressively? → A: Lazy/virtualized — only render pages near the viewport; discard off-screen pages to keep memory bounded.
- Q: Should there be a cap on the maximum number of columns? → A: Max 3 columns — on very wide displays up to 3 pages may appear side-by-side; never more.
- Q: When the user zooms in or out, should the column count recalculate? → A: Yes — zoom changes effective page size and triggers the same column reflow as a window resize.

## Assumptions

- The column count ranges from 1 to 3. The number of columns is determined by how many pages at their natural size (or a minimum readable size) fit side-by-side in the viewer — no manual column toggle is needed. Three columns only appear on very wide displays.
- Pages within a row are sized uniformly (same width per column) even if they have different natural dimensions, to maintain a clean grid appearance.
- The feature does not introduce a manual "single page" or "two page" toggle — the layout is always responsive and automatic.
- Existing annotation and zoom features continue to work correctly with the new layout; this spec does not change their behavior.
- Scroll position is tracked per-page (i.e., which page is in view) so it can be restored after reflow.
- Pages are rendered lazily: only pages within or near the visible viewport are rendered; off-screen pages are unloaded to keep memory usage bounded. A render buffer of approximately 1–2 pages above and below the viewport is maintained to avoid blank flicker during normal scrolling.
