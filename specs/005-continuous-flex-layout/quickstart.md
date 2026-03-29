# Quickstart: Testing Continuous Flex Layout

**Feature**: 005-continuous-flex-layout

## Running the app

```bash
npm run tauri dev
```

## Test scenarios

### 1. Continuous scroll (P1)
1. Open any multi-page PDF (≥ 5 pages)
2. Verify all pages visible by scrolling — no "Next Page" button required
3. Scroll to last page — confirm it appears without navigation

### 2. Flex columns (P2)
1. Open a PDF with portrait pages
2. Widen the window until two pages appear side-by-side
3. Widen further — confirm a third column appears (if pages are small enough)
4. Narrow the window — confirm reflow back to 2 then 1 column

### 3. Reflow preserves position (P3)
1. Scroll to page 8 of a 20-page PDF
2. Resize window to trigger column change
3. Verify page 8 is still visible (within one page offset)

### 4. Zoom triggers reflow
1. Open a PDF in 2-column layout
2. Zoom in (Ctrl++) until pages are too wide for 2 columns
3. Verify reflow to 1 column
4. Zoom out — verify reflow back to 2 columns

### 5. Annotations per-page
1. Open a PDF, draw an annotation on page 1
2. Scroll to page 3, draw an annotation
3. Scroll back to page 1 — confirm page 1 annotation still visible

### 6. Large document performance
1. Open a 100-page PDF
2. Verify the first page is visible within normal load time
3. Scroll rapidly to page 80 — verify pages render without excessive lag

## Build

```bash
npm run tauri build
```

Output: `src-tauri/target/release/pdf-reader.exe`
