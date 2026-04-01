use crate::{App, PageData};
use pdfium_render::prelude::*;
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// ── Zoom levels (same as TypeScript version) ─────────────────────────────────

const ZOOM_LEVELS: &[f32] = &[
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

fn snap_zoom(current: f32, dir: i32) -> f32 {
    if dir > 0 {
        ZOOM_LEVELS
            .iter()
            .find(|&&z| z > current + 0.005)
            .copied()
            .unwrap_or(*ZOOM_LEVELS.last().unwrap())
    } else {
        ZOOM_LEVELS
            .iter()
            .rev()
            .find(|&&z| z < current - 0.005)
            .copied()
            .unwrap_or(ZOOM_LEVELS[0])
    }
}

// ── Page dimensions ──────────────────────────────────────────────────────────

#[derive(Clone)]
struct PageDim {
    width_pt: f32,
    height_pt: f32,
}

// ── App state ────────────────────────────────────────────────────────────────

struct ViewerState {
    file_path: Option<PathBuf>,
    page_dims: Vec<PageDim>,
    page_count: u16,
    scale: f32,
    rotation: i32, // 0, 90, 180, 270
}

impl Default for ViewerState {
    fn default() -> Self {
        Self {
            file_path: None,
            page_dims: Vec::new(),
            page_count: 0,
            scale: 1.5,
            rotation: 0,
        }
    }
}

// ── Rendering ────────────────────────────────────────────────────────────────

fn render_page_to_image(
    pdfium: &Pdfium,
    path: &std::path::Path,
    page_index: u16,
    scale: f32,
    rotation: i32,
) -> Option<Image> {
    let doc = pdfium.load_pdf_from_file(path, None).ok()?;
    let page = doc.pages().get(page_index).ok()?;

    let pdfium_rotation = match rotation {
        90 => Some(PdfPageRenderRotation::Degrees90),
        180 => Some(PdfPageRenderRotation::Degrees180),
        270 => Some(PdfPageRenderRotation::Degrees270),
        _ => None,
    };

    let (page_w, page_h) = if rotation == 90 || rotation == 270 {
        (page.height().value, page.width().value)
    } else {
        (page.width().value, page.height().value)
    };

    let width = (page_w * scale) as i32;
    let height = (page_h * scale) as i32;

    let mut config = PdfRenderConfig::new()
        .set_target_width(width)
        .set_target_height(height);

    if let Some(rot) = pdfium_rotation {
        config = config.rotate(rot, true);
    }

    let bitmap = page.render_with_config(&config).ok()?;
    let img = bitmap.as_image();
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();

    let mut pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    pixel_buffer.make_mut_bytes().copy_from_slice(&rgba);

    Some(Image::from_rgba8(pixel_buffer))
}

/// Build the full pages model with rendered images.
fn build_rendered_pages(pdfium: &Pdfium, state: &ViewerState) -> ModelRc<PageData> {
    let items: Vec<PageData> = state
        .page_dims
        .iter()
        .enumerate()
        .map(|(i, dim)| {
            let (w, h) = if state.rotation == 90 || state.rotation == 270 {
                (dim.height_pt, dim.width_pt)
            } else {
                (dim.width_pt, dim.height_pt)
            };

            // Render up to 20 pages for responsiveness
            let image = if i < 20 {
                state.file_path.as_ref().and_then(|path| {
                    render_page_to_image(pdfium, path, i as u16, state.scale, state.rotation)
                })
            } else {
                None
            };

            let rendered = image.is_some();
            PageData {
                image: image.unwrap_or_default(),
                width: w * state.scale,
                height: h * state.scale,
                page_num: (i + 1) as i32,
                rendered,
            }
        })
        .collect();

    ModelRc::new(VecModel::from(items))
}

fn update_ui(pdfium: &Pdfium, state: &ViewerState, ui: &App) {
    let model = build_rendered_pages(pdfium, state);
    ui.set_pages(model);
    ui.set_zoom_text(format!("{}%", (state.scale * 100.0).round() as i32).into());
}

// ── Setup ────────────────────────────────────────────────────────────────────

pub fn setup(ui: &App) {
    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
        .or_else(|_| Pdfium::bind_to_system_library())
        .expect("Failed to load PDFium library. Place pdfium.dll next to the executable.");

    let pdfium = Arc::new(Pdfium::new(bindings));
    let state = Arc::new(Mutex::new(ViewerState::default()));

    // ── Open file ────────────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_open_file(move || {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf", "PDF"])
                .add_filter("All", &["*"])
                .set_title("Open PDF")
                .pick_file()
            else {
                return;
            };

            let ui = ui_weak.unwrap();
            match load_document(&pdfium, &path) {
                Ok(dims) => {
                    let page_count = dims.len() as u16;
                    {
                        let mut s = state.lock().unwrap();
                        s.file_path = Some(path.clone());
                        s.page_dims = dims;
                        s.page_count = page_count;
                        s.rotation = 0;
                    }

                    let s = state.lock().unwrap();
                    update_ui(&pdfium, &s, &ui);
                    ui.set_page_count(page_count as i32);
                    ui.set_current_page(1);
                    ui.set_page_text("1".into());
                    ui.set_has_document(true);
                    ui.set_status_text(SharedString::from(format!(
                        "{} — {} pages",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        page_count
                    )));
                }
                Err(e) => {
                    ui.set_status_text(format!("Error: {}", e).into());
                }
            }
        });
    }

    // ── Zoom in ──────────────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_zoom_in(move || {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            if s.page_count == 0 { return; }
            s.scale = snap_zoom(s.scale, 1);
            update_ui(&pdfium, &s, &ui);
        });
    }

    // ── Zoom out ─────────────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_zoom_out(move || {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            if s.page_count == 0 { return; }
            s.scale = snap_zoom(s.scale, -1);
            update_ui(&pdfium, &s, &ui);
        });
    }

    // ── Fit width ────────────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_fit_width(move || {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            if s.page_count == 0 { return; }
            let container_width = ui.get_viewer_width() - 60.0;
            let ref_width = if s.rotation == 90 || s.rotation == 270 {
                s.page_dims[0].height_pt
            } else {
                s.page_dims[0].width_pt
            };
            s.scale = (container_width / ref_width).clamp(0.25, 5.0);
            update_ui(&pdfium, &s, &ui);
        });
    }

    // ── Fit height ───────────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_fit_height(move || {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            if s.page_count == 0 { return; }
            let container_height = ui.get_viewer_height() - 80.0;
            let ref_height = if s.rotation == 90 || s.rotation == 270 {
                s.page_dims[0].width_pt
            } else {
                s.page_dims[0].height_pt
            };
            s.scale = (container_height / ref_height).clamp(0.25, 5.0);
            update_ui(&pdfium, &s, &ui);
        });
    }

    // ── Rotate ───────────────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_rotate(move || {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            if s.page_count == 0 { return; }
            s.rotation = (s.rotation + 90) % 360;
            update_ui(&pdfium, &s, &ui);
        });
    }

    // ── Page navigation ──────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_page_prev(move || {
            let ui = ui_weak.unwrap();
            let s = state.lock().unwrap();
            let cur = ui.get_current_page();
            if cur > 1 {
                let new_page = cur - 1;
                ui.set_current_page(new_page);
                ui.set_page_text(new_page.to_string().into());
                scroll_to_page(&ui, &s, new_page);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_page_next(move || {
            let ui = ui_weak.unwrap();
            let s = state.lock().unwrap();
            let cur = ui.get_current_page();
            if cur < s.page_count as i32 {
                let new_page = cur + 1;
                ui.set_current_page(new_page);
                ui.set_page_text(new_page.to_string().into());
                scroll_to_page(&ui, &s, new_page);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_page_goto(move |page| {
            let ui = ui_weak.unwrap();
            let s = state.lock().unwrap();
            let clamped = page.max(1).min(s.page_count as i32);
            ui.set_current_page(clamped);
            ui.set_page_text(clamped.to_string().into());
            scroll_to_page(&ui, &s, clamped);
        });
    }

    // ── Scroll position tracking (poll every 150ms) ─────────────────────────
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, std::time::Duration::from_millis(150), move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let s = state.lock().unwrap();
            if s.page_count == 0 { return; }
            let scroll_y = ui.get_current_scroll_y();
            let page = page_at_scroll_y(scroll_y, &s);
            if page != ui.get_current_page() {
                ui.set_current_page(page);
                ui.set_page_text(page.to_string().into());
            }
        });
        // Leak the timer so it lives for the app's lifetime
        std::mem::forget(timer);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn page_at_scroll_y(scroll_y: f32, state: &ViewerState) -> i32 {
    let gap = 8.0_f32;
    let mut y = 0.0_f32;
    for (i, dim) in state.page_dims.iter().enumerate() {
        let h = if state.rotation == 90 || state.rotation == 270 {
            dim.width_pt
        } else {
            dim.height_pt
        };
        let page_h = h * state.scale;
        if scroll_y < y + page_h * 0.5 {
            return (i + 1) as i32;
        }
        y += page_h + gap;
    }
    state.page_count as i32
}

fn scroll_to_page(ui: &App, state: &ViewerState, page: i32) {
    let page_idx = (page - 1).max(0) as usize;
    let gap = 8.0_f32; // matches spacing in .slint
    let mut y = 0.0_f32;
    for i in 0..page_idx.min(state.page_dims.len()) {
        let dim = &state.page_dims[i];
        let h = if state.rotation == 90 || state.rotation == 270 {
            dim.width_pt
        } else {
            dim.height_pt
        };
        y += h * state.scale + gap;
    }
    // viewport-y is negative (scrolling down = negative offset)
    ui.invoke_scroll_to(-y);
}

fn load_document(pdfium: &Pdfium, path: &std::path::Path) -> Result<Vec<PageDim>, String> {
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| format!("{}", e))?;

    let pages = doc.pages();
    let count = pages.len();
    let mut dims = Vec::with_capacity(count as usize);

    for i in 0..count {
        let page = pages.get(i).map_err(|e| format!("{}", e))?;
        dims.push(PageDim {
            width_pt: page.width().value,
            height_pt: page.height().value,
        });
    }

    Ok(dims)
}
