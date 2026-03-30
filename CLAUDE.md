# pdf-rider Development Guidelines

Auto-generated from all feature plans. Last updated: 2026-03-25
Don't push without explicit demand

## Active Technologies
- TypeScript 5.x (frontend), Rust stable ≥1.77 (backend — unchanged) + pdfjs-dist (PDF rendering), Tauri v2 (desktop shell), CanvasOverlay (internal annotation system) (005-continuous-flex-layout)
- N/A (no new persistence) (005-continuous-flex-layout)

- Rust stable (≥ 1.77) + TypeScript 5.x + Tauri v2, pdf.js (pdfjs-dist), lopdf 0.39, image crate (001-pdf-read-annotate-sign)

## Project Structure

```text
src/
tests/
```

## Commands

cargo test; cargo clippy

## Code Style

Rust stable (≥ 1.77) + TypeScript 5.x: Follow standard conventions

## Recent Changes
- 005-continuous-flex-layout: Added TypeScript 5.x (frontend), Rust stable ≥1.77 (backend — unchanged) + pdfjs-dist (PDF rendering), Tauri v2 (desktop shell), CanvasOverlay (internal annotation system)

- 001-pdf-read-annotate-sign: Added Rust stable (≥ 1.77) + TypeScript 5.x + Tauri v2, pdf.js (pdfjs-dist), lopdf 0.39, image crate

<!-- MANUAL ADDITIONS START -->
<!-- MANUAL ADDITIONS END -->
