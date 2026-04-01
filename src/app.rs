use crate::annotation::{interaction::InteractionState, overlay, store::AnnotationStore};
use crate::pdf::writer;
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
    annotations: AnnotationStore,
    interaction: InteractionState,
    dirty: bool,
}

impl Default for ViewerState {
    fn default() -> Self {
        Self {
            file_path: None,
            page_dims: Vec::new(),
            page_count: 0,
            scale: 1.5,
            rotation: 0,
            annotations: AnnotationStore::default(),
            interaction: InteractionState::default(),
            dirty: false,
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
    annotations: &[crate::pdf::models::Annotation],
) -> Option<Image> {
    let doc = pdfium.load_pdf_from_file(path, None).ok()?;
    let page = doc.pages().get(page_index).ok()?;

    let pdfium_rotation = match rotation {
        90 => Some(PdfPageRenderRotation::Degrees90),
        180 => Some(PdfPageRenderRotation::Degrees180),
        270 => Some(PdfPageRenderRotation::Degrees270),
        _ => None,
    };

    let page_height_pt = page.height().value as f64;

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
    let mut rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();

    // Composite annotation overlay if any
    if !annotations.is_empty() {
        if let Some(overlay_pixmap) = overlay::render_overlay(
            annotations,
            w,
            h,
            page_height_pt,
            scale as f64,
        ) {
            let overlay_data = overlay_pixmap.data();
            let base = rgba.as_mut();
            // Alpha-blend overlay onto base
            for i in (0..base.len()).step_by(4) {
                let sa = overlay_data[i + 3] as u32;
                if sa == 0 { continue; }
                let da = 255 - sa;
                base[i]     = ((overlay_data[i] as u32 * sa + base[i] as u32 * da) / 255) as u8;
                base[i + 1] = ((overlay_data[i + 1] as u32 * sa + base[i + 1] as u32 * da) / 255) as u8;
                base[i + 2] = ((overlay_data[i + 2] as u32 * sa + base[i + 2] as u32 * da) / 255) as u8;
                base[i + 3] = (sa + base[i + 3] as u32 * da / 255) as u8;
            }
        }
    }

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
            let page_num = (i + 1) as u32;
            let page_anns = state.annotations.get_for_page(page_num);
            let image = if i < 20 {
                state.file_path.as_ref().and_then(|path| {
                    render_page_to_image(pdfium, path, i as u16, state.scale, state.rotation, page_anns)
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
                Ok(loaded) => {
                    let page_count = loaded.dims.len() as u16;
                    let ann_count = loaded.annotations.len();
                    {
                        let mut s = state.lock().unwrap();
                        s.file_path = Some(path.clone());
                        s.page_dims = loaded.dims;
                        s.page_count = page_count;
                        s.rotation = 0;
                        s.annotations.load(loaded.annotations);
                    }

                    let s = state.lock().unwrap();
                    update_ui(&pdfium, &s, &ui);
                    ui.set_page_count(page_count as i32);
                    ui.set_current_page(1);
                    ui.set_page_text("1".into());
                    ui.set_has_document(true);
                    let status = if ann_count > 0 {
                        format!(
                            "{} — {} pages, {} annotations",
                            path.file_name().unwrap_or_default().to_string_lossy(),
                            page_count,
                            ann_count
                        )
                    } else {
                        format!(
                            "{} — {} pages",
                            path.file_name().unwrap_or_default().to_string_lossy(),
                            page_count
                        )
                    };
                    ui.set_status_text(SharedString::from(status));
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

    // ── Tool switching ─────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_set_tool(move |tool| {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            s.interaction.tool = crate::annotation::interaction::Tool::from_str(&tool);
            ui.set_active_tool(tool);
        });
    }

    // ── Pointer events on pages ─────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_page_pointer_down(move |page, x, y| {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            s.interaction.pointer_down(page as u32, x, y);
            ui.set_drawing(true);
            ui.set_drawing_page(page);
            ui.set_draw_x(x.min(x));
            ui.set_draw_y(y.min(y));
            ui.set_draw_w(0.0);
            ui.set_draw_h(0.0);
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_page_pointer_move(move |_page, x, y| {
            let ui = ui_weak.unwrap();
            let s = state.lock().unwrap();
            if s.interaction.drawing {
                let sx = s.interaction.start_x;
                let sy = s.interaction.start_y;
                ui.set_draw_x(sx.min(x));
                ui.set_draw_y(sy.min(y));
                ui.set_draw_w((x - sx).abs());
                ui.set_draw_h((y - sy).abs());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_page_pointer_up(move |page, x, y| {
            let ui = ui_weak.unwrap();
            ui.set_drawing(false);
            let mut s = state.lock().unwrap();
            let page_idx = (page - 1) as usize;
            let page_height_pt = s.page_dims.get(page_idx)
                .map(|d| if s.rotation == 90 || s.rotation == 270 { d.width_pt } else { d.height_pt })
                .unwrap_or(841.0);
            let scale = s.scale;

            if let Some(ann) = s.interaction.pointer_up(
                page as u32, x, y, scale, page_height_pt,
            ) {
                s.annotations.add(ann);
                s.dirty = true;
                update_ui(&pdfium, &s, &ui);
            }
        });
    }

    // ── Click in select mode (hit test) ─────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_page_click(move |page, x, y| {
            let ui = ui_weak.unwrap();
            let s = state.lock().unwrap();
            let page_idx = (page - 1) as usize;
            let page_height_pt = s.page_dims.get(page_idx)
                .map(|d| if s.rotation == 90 || s.rotation == 270 { d.width_pt } else { d.height_pt })
                .unwrap_or(841.0);
            let scale = s.scale;

            // Hit test: find annotation under click point
            let anns = s.annotations.get_for_page(page as u32);
            if let Some((idx, bounds)) = hit_test(anns, x, y, scale as f64, page_height_pt as f64) {
                ui.set_has_selection(true);
                ui.set_selection_page(page);
                ui.set_sel_x(bounds.0);
                ui.set_sel_y(bounds.1);
                ui.set_sel_w(bounds.2);
                ui.set_sel_h(bounds.3);
                let _ = idx; // will be used for move/resize/delete later
            } else {
                ui.set_has_selection(false);
            }
        });
    }

    // ── Save ─────────────────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_save_file(move || {
            let ui = ui_weak.unwrap();
            let s = state.lock().unwrap();
            let Some(path) = &s.file_path else { return };
            let all_annotations = s.annotations.all();
            if all_annotations.is_empty() { return; }

            match save_annotated_pdf(path, &all_annotations) {
                Ok(()) => {
                    ui.set_status_text(SharedString::from("Saved"));
                }
                Err(e) => {
                    ui.set_status_text(SharedString::from(format!("Save error: {}", e)));
                }
            }
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

struct LoadedDoc {
    dims: Vec<PageDim>,
    annotations: Vec<crate::pdf::models::Annotation>,
}

fn load_document(pdfium: &Pdfium, path: &std::path::Path) -> Result<LoadedDoc, String> {
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

    // Load annotations from PDF metadata (CCAnnot)
    let lopdf_doc = lopdf::Document::load(path).unwrap_or_default();
    let meta = writer::load_meta(&lopdf_doc);

    Ok(LoadedDoc {
        dims,
        annotations: meta.annotations,
    })
}

/// Hit test: find annotation at canvas pixel (x, y).
/// Returns (index, (canvas_left, canvas_top, canvas_width, canvas_height)) or None.
fn hit_test(
    annotations: &[crate::pdf::models::Annotation],
    canvas_x: f32,
    canvas_y: f32,
    scale: f64,
    page_height_pt: f64,
) -> Option<(usize, (f32, f32, f32, f32))> {
    use crate::pdf::models::Annotation;

    // Test in reverse order (topmost first)
    for (i, ann) in annotations.iter().enumerate().rev() {
        let bounds = ann_canvas_bounds(ann, scale, page_height_pt);
        let (left, top, w, h) = bounds;
        let tolerance = 5.0;
        if canvas_x >= left - tolerance
            && canvas_x <= left + w + tolerance
            && canvas_y >= top - tolerance
            && canvas_y <= top + h + tolerance
        {
            return Some((i, bounds));
        }
    }
    None
}

/// Get canvas-pixel bounding box for an annotation.
fn ann_canvas_bounds(
    ann: &crate::pdf::models::Annotation,
    scale: f64,
    page_height_pt: f64,
) -> (f32, f32, f32, f32) {
    use crate::pdf::models::Annotation;

    match ann {
        Annotation::Rect(r) => {
            let left = (r.x * scale) as f32;
            let top = ((page_height_pt - r.y - r.height) * scale) as f32;
            let w = (r.width * scale) as f32;
            let h = (r.height * scale) as f32;
            (left, top, w, h)
        }
        Annotation::Circle(c) => {
            let left = (c.x * scale) as f32;
            let top = ((page_height_pt - c.y - c.height) * scale) as f32;
            let w = (c.width * scale) as f32;
            let h = (c.height * scale) as f32;
            (left, top, w, h)
        }
        Annotation::Text(t) => {
            let left = (t.x * scale) as f32;
            let top = ((page_height_pt - t.y) * scale) as f32;
            let w = (t.width * scale) as f32;
            let h = (t.font_size * 1.2 * t.content.split('\n').count() as f64 * scale) as f32;
            (left, top, w, h)
        }
        Annotation::Signature(s) => {
            let left = (s.x * scale) as f32;
            let top = ((page_height_pt - s.y - s.height) * scale) as f32;
            let w = (s.width * scale) as f32;
            let h = (s.height * scale) as f32;
            (left, top, w, h)
        }
    }
}

fn save_annotated_pdf(
    path: &std::path::Path,
    annotations: &[crate::pdf::models::Annotation],
) -> Result<(), String> {
    use std::collections::HashMap;

    let mut doc = lopdf::Document::load(path).map_err(|e| format!("{}", e))?;

    // Load existing meta to preserve stream IDs
    let mut meta = writer::load_meta(&doc);
    meta.annotations = annotations.to_vec();

    // Group annotations by page
    let mut by_page: HashMap<u32, Vec<&crate::pdf::models::Annotation>> = HashMap::new();
    for ann in annotations {
        by_page.entry(ann.page()).or_default().push(ann);
    }

    // Get page object IDs
    let page_ids: Vec<(u32, lopdf::ObjectId)> = doc
        .get_pages()
        .into_iter()
        .map(|(num, id)| (num, id))
        .collect();

    // Write annotations per page
    for (page_num, page_id) in &page_ids {
        let page_anns: Vec<crate::pdf::models::Annotation> = by_page
            .get(page_num)
            .map(|v| v.iter().map(|a| (*a).clone()).collect())
            .unwrap_or_default();

        let existing_sid = meta.stream_ids.get(page_num).map(|arr| (arr[0], arr[1] as u16));

        match writer::write_annotations_for_page(&mut doc, *page_id, &page_anns, existing_sid)? {
            Some(sid) => {
                meta.stream_ids.insert(*page_num, [sid.0, sid.1 as u32]);
            }
            None => {
                meta.stream_ids.remove(page_num);
            }
        }
    }

    // Save metadata
    writer::save_meta(&mut doc, &meta)?;

    // Write to file
    doc.save(path).map_err(|e| format!("{}", e))?;
    Ok(())
}
