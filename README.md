<div align="center">

# PDF Rider

**A fast, lightweight, open-source PDF reader for Windows.**
Read, annotate, sign, fill forms, and compress — all in a single 5.5 MB portable executable. No installer. No subscription. No telemetry.

[![Latest Release](https://img.shields.io/github/v/release/pleymor/pdf-rider?style=flat-square&label=Download)](https://github.com/pleymor/pdf-rider/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange?style=flat-square)](https://www.rust-lang.org/)

![PDF Rider main view](docs/screenshots/main-page.png)

</div>

---

## Why PDF Rider?

Most PDF tools are either bloated desktop applications that take minutes to install and gigabytes of disk space, or cloud-based services that upload your documents to a third-party server. PDF Rider is neither.

| | PDF Rider | Adobe Acrobat | Online tools |
|---|---|---|---|
| **Binary size** | 5.5 MB | ~4 GB | — |
| **Installer required** | No | Yes | No |
| **Works offline** | Yes | Yes | No |
| **Your files stay local** | Always | Yes | No |
| **Price** | Free & open source | ~€180/year | Freemium |
| **Startup** | Instant | Slow | Browser-dependent |

---

## Features

### Read
- **Crisp rendering** powered by PDF.js — text, vectors, and images at any zoom level
- **Page navigation** with keyboard shortcuts, page number input, and prev/next buttons
- **Zoom controls** — fit to width, fit to height, snap levels, or type any percentage
- **Rotation** — rotate pages 90° clockwise
- **Drag & drop** to open files

### Annotate
- **Text annotations** with full typography controls: bold, italic, underline, size, color, alignment
- **Shape annotations** — rectangles and circles with stroke color
- **Freehand signature** — draw directly or insert a signature image
- **Layer management** — bring to front / send to back
- Annotations are saved as PDF metadata and **fully re-editable** on reopen

![Annotation mode with rich toolbar](docs/screenshots/annotations.png)

### Fill forms
- **Interactive form support** — text fields, checkboxes, radio buttons
- **Keyboard navigation** between fields (Tab / arrow keys)
- Filled values are saved back into the PDF

### Compress
- **Three quality presets**: Screen (smallest), Ebook (balanced), Print (high quality)
- JPEG re-encoding, FlateDecode stream compression, metadata stripping, dead object pruning
- Typical result: **35% size reduction** with no visible quality loss

![Compression result toast: 1.7 MB → 1.1 MB](docs/screenshots/compression.png)

### Internationalized
- **20 languages** — switch instantly in Settings, no restart required
- UI fully translated: English, Français, 中文, हिन्दी, Español, العربية, বাংলা, Русский, Português, اردو, Bahasa Indonesia, Deutsch, 日本語, Kiswahili, मराठी, తెలుగు, Türkçe, தமிழ், Tiếng Việt, 한국어

![Language picker with 20 languages](docs/screenshots/i18n.png)

### Windows integration
- **Set as default PDF handler** directly from the app — no admin rights required
- **Print** silently from the command line (`pdf-rider-portable.exe --print file.pdf`)
- Registers both open and print verbs in HKCU

---

## Download

**[→ Download the latest release](https://github.com/pleymor/pdf-rider/releases/latest)**

Single portable `.exe`, 5.5 MB. Drop it anywhere and run it — no installer, no dependencies.

---

## Contributing

PDF Rider is built with **Rust + TypeScript + Tauri v2**. The codebase is intentionally small and straightforward. Contributions are welcome.

### Prerequisites

| Tool | Minimum version | Install |
|------|----------------|---------|
| Rust (stable) | 1.77 | [rustup.rs](https://rustup.rs) |
| Node.js | 18 | [nodejs.org](https://nodejs.org) |
| Tauri CLI | 2.x | `cargo install tauri-cli` |
| Visual Studio Build Tools | 2019+ | [visualstudio.microsoft.com](https://visualstudio.microsoft.com/visual-cpp-build-tools/) |

> **Platform**: Windows only (the portable `.exe` uses Windows-specific APIs for printing and file association).

### Build

```bash
# Clone
git clone https://github.com/pleymor/pdf-rider.git
cd pdf-rider

# Install JS dependencies
npm install

# Run in development (hot-reload frontend, auto-rebuild Rust on change)
npm run tauri dev

# Build a release portable executable
npm run tauri build
# Output: src-tauri/target/release/pdf-rider-portable.exe
```

### Architecture

```
pdf-rider/
├── src/                        # TypeScript frontend (Vite + PDF.js)
│   ├── main.ts                 # App entrypoint, event wiring
│   ├── tauri-bridge.ts         # All invoke() wrappers with types
│   ├── toolbar.ts              # Toolbar component & events
│   ├── compress-modal.ts       # Compression modal
│   ├── i18n.ts                 # 20-language translation table
│   └── styles/app.css          # All styles
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs              # App state, Tauri setup, command registration
│   │   ├── commands/           # Tauri command handlers (one file per feature)
│   │   │   ├── compress.rs     # compress_pdf command
│   │   │   ├── pdf.rs          # get_page_count, save_annotated_pdf, read_annotations
│   │   │   ├── dialog.rs       # open/save dialog commands
│   │   │   └── shell.rs        # print, URL open, file association
│   │   └── pdf/
│   │       └── compress.rs     # PDF compression logic (lopdf)
│   └── Cargo.toml
├── specs/                      # Feature specifications (one folder per feature)
└── index.html                  # Single-page app shell
```

**How a feature works end-to-end:**
1. User clicks a toolbar button → `toolbar.ts` emits a typed event
2. `main.ts` handles the event, calls a wrapper in `tauri-bridge.ts`
3. `tauri-bridge.ts` calls `invoke("command_name", { ...args })`
4. Tauri routes to the matching `#[tauri::command]` fn in `commands/`
5. The Rust handler processes the PDF (via `lopdf` or the filesystem) and returns a result
6. The result is displayed as a toast or used to update the UI

### Code quality checks

Run these before opening a PR — both must pass:

```bash
# Rust: lints and type checks
cd src-tauri && cargo clippy -- -D warnings

# TypeScript: type checks (no emit)
npx tsc --noEmit
```

### Branching & commit conventions

| What | Convention |
|------|-----------|
| Branch name | `NNN-short-description` (e.g. `004-dark-mode`) |
| Commit message | Imperative, present tense: `Add dark mode toggle` |
| One commit per PR | Squash before merging |
| PR title | Same as the squashed commit message |

Feature branches are numbered sequentially. Pick the next available number from the existing `specs/` directories.

### Opening a PR

1. Fork the repo and create a branch from `main`
2. Make your changes — keep PRs focused on a single feature or fix
3. Run `cargo clippy` and `tsc --noEmit`, fix any warnings
4. Open a PR with a clear description of **what** and **why**
5. Link the relevant spec from `specs/` if one exists

### Good first issues

Check the [open issues](https://github.com/pleymor/pdf-rider/issues) — issues labeled [`good first issue`](https://github.com/pleymor/pdf-rider/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22) are a great place to start.

---

## License

MIT — see [LICENSE](LICENSE) for details.
