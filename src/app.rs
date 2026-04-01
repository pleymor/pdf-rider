use crate::annotation::{interaction::InteractionState, overlay, store::AnnotationStore};
use crate::pdf::writer;
use crate::{App, PageData};
use pdfium_render::prelude::*;
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel};
use std::path::PathBuf;
use std::rc::Rc;
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

/// Build placeholders for all pages (no rendering yet).
fn build_page_placeholders(state: &ViewerState) -> (ModelRc<PageData>, Rc<VecModel<PageData>>) {
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
            PageData {
                image: Image::default(),
                width: w * state.scale,
                height: h * state.scale,
                page_num: (i + 1) as i32,
                rendered: false,
            }
        })
        .collect();

    let vm = Rc::new(VecModel::from(items));
    (ModelRc::from(vm.clone()), vm)
}

/// Render a single page and update the model in-place.
fn render_single_page(
    pdfium: &Pdfium,
    state: &ViewerState,
    vm: &VecModel<PageData>,
    page_idx: usize,
) {
    if page_idx >= state.page_dims.len() { return; }
    let page_num = (page_idx + 1) as u32;
    let page_anns = state.annotations.get_for_page(page_num);
    if let Some(path) = &state.file_path {
        if let Some(img) = render_page_to_image(pdfium, path, page_idx as u16, state.scale, state.rotation, page_anns) {
            let mut data = vm.row_data(page_idx).unwrap();
            data.image = img;
            data.rendered = true;
            vm.set_row_data(page_idx, data);
        }
    }
}

/// Render visible pages (based on scroll position).
fn render_visible_pages(
    pdfium: &Pdfium,
    state: &ViewerState,
    vm: &VecModel<PageData>,
    scroll_y: f32,
    viewport_h: f32,
) {
    let gap = 8.0_f32;
    let mut y = 0.0_f32;
    for (i, dim) in state.page_dims.iter().enumerate() {
        let h = if state.rotation == 90 || state.rotation == 270 {
            dim.width_pt
        } else {
            dim.height_pt
        };
        let page_h = h * state.scale;
        let page_bottom = y + page_h;

        // Page is visible if it overlaps [scroll_y, scroll_y + viewport_h]
        if page_bottom >= scroll_y - 200.0 && y <= scroll_y + viewport_h + 200.0 {
            // Only render if not already rendered
            if let Some(data) = vm.row_data(i) {
                if !data.rendered {
                    render_single_page(pdfium, state, vm, i);
                }
            }
        }
        y += page_h + gap;
    }
}

/// Rebuild all pages (for zoom/rotation changes).
fn rebuild_all_pages(
    pdfium: &Pdfium,
    state: &ViewerState,
    ui: &App,
) -> Rc<VecModel<PageData>> {
    let (model_rc, vm) = build_page_placeholders(state);
    ui.set_pages(model_rc);
    ui.set_zoom_text(format!("{}%", (state.scale * 100.0).round() as i32).into());

    // Render visible pages
    let scroll_y = ui.get_current_scroll_y();
    let viewport_h = ui.get_viewer_height();
    render_visible_pages(pdfium, state, &vm, scroll_y, viewport_h);
    vm
}

/// Re-render only one page (for annotation changes).
fn update_single_page(
    pdfium: &Pdfium,
    state: &ViewerState,
    vm: &VecModel<PageData>,
    page_num: u32,
) {
    let page_idx = (page_num - 1) as usize;
    render_single_page(pdfium, state, vm, page_idx);
}

// Legacy wrapper — rebuilds everything. Used for zoom/rotation.
fn update_ui(pdfium: &Pdfium, state: &ViewerState, ui: &App) {
    let (model_rc, vm) = build_page_placeholders(state);
    ui.set_pages(model_rc);
    ui.set_zoom_text(format!("{}%", (state.scale * 100.0).round() as i32).into());
    let scroll_y = ui.get_current_scroll_y();
    let viewport_h = ui.get_viewer_height();
    render_visible_pages(pdfium, state, &vm, scroll_y, viewport_h);
}

// ── Setup ────────────────────────────────────────────────────────────────────

pub fn setup(ui: &App) {
    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
        .or_else(|_| Pdfium::bind_to_system_library())
        .expect("Failed to load PDFium library. Place pdfium.dll next to the executable.");

    let pdfium = Arc::new(Pdfium::new(bindings));
    let state = Arc::new(Mutex::new(ViewerState::default()));
    // Shared page model (Rc because Slint is single-threaded)
    let pages_vm: std::rc::Rc<std::cell::RefCell<Option<Rc<VecModel<PageData>>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));

    // ── Open file ────────────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        let pages_vm = pages_vm.clone();
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
                    let vm = rebuild_all_pages(&pdfium, &s, &ui);
                    *pages_vm.borrow_mut() = Some(vm);
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
            let is_text_tool = s.interaction.tool == crate::annotation::interaction::Tool::Text;

            if let Some(ann) = s.interaction.pointer_up(
                page as u32, x, y, scale, page_height_pt,
            ) {
                if is_text_tool {
                    // Store pending text annotation, show dialog
                    s.interaction.pending_text_ann = Some(ann);
                    ui.set_text_input_value("".into());
                    ui.set_show_text_input(true);
                } else {
                    s.annotations.add(ann);
                    s.dirty = true;
                    update_ui(&pdfium, &s, &ui);
                }
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

            // Check if clicking a resize handle first
            if s.interaction.selected_idx.is_some()
                && s.interaction.selected_page == page as u32
                && ui.get_has_selection()
            {
                let sx = ui.get_sel_x();
                let sy = ui.get_sel_y();
                let sw = ui.get_sel_w();
                let sh = ui.get_sel_h();
                let handle_r = 10.0_f32;

                // NW
                if (x - sx).abs() < handle_r && (y - sy).abs() < handle_r {
                    drop(s);
                    ui.invoke_resize_start(0, page, sx + sw, sy + sh);
                    return;
                }
                // NE
                if (x - (sx + sw)).abs() < handle_r && (y - sy).abs() < handle_r {
                    drop(s);
                    ui.invoke_resize_start(1, page, sx, sy + sh);
                    return;
                }
                // SW
                if (x - sx).abs() < handle_r && (y - (sy + sh)).abs() < handle_r {
                    drop(s);
                    ui.invoke_resize_start(2, page, sx + sw, sy);
                    return;
                }
                // SE
                if (x - (sx + sw)).abs() < handle_r && (y - (sy + sh)).abs() < handle_r {
                    drop(s);
                    ui.invoke_resize_start(3, page, sx, sy);
                    return;
                }
            }

            // Hit test: find annotation under click point
            let anns = s.annotations.get_for_page(page as u32);
            if let Some((idx, bounds)) = hit_test(anns, x, y, scale as f64, page_height_pt as f64) {
                let ann = &anns[idx];
                let type_str = ann_type_str(ann);
                // Sync UI style from selected annotation
                let (cr, cg, cb) = match ann {
                    crate::pdf::models::Annotation::Rect(r) => (r.color.r, r.color.g, r.color.b),
                    crate::pdf::models::Annotation::Circle(c) => (c.color.r, c.color.g, c.color.b),
                    crate::pdf::models::Annotation::Text(t) => (t.color.r, t.color.g, t.color.b),
                    crate::pdf::models::Annotation::Signature(_) => (0, 0, 0),
                };
                let sw = match ann {
                    crate::pdf::models::Annotation::Rect(r) => r.stroke_width,
                    crate::pdf::models::Annotation::Circle(c) => c.stroke_width,
                    _ => 2.0,
                };
                let fs = match ann {
                    crate::pdf::models::Annotation::Text(t) => t.font_size,
                    _ => 14.0,
                };

                drop(s);
                let mut s = state.lock().unwrap();
                s.interaction.selected_idx = Some(idx);
                s.interaction.selected_page = page as u32;
                s.interaction.dragging = false;
                ui.set_has_selection(true);
                ui.set_selection_page(page);
                ui.set_sel_x(bounds.0);
                ui.set_sel_y(bounds.1);
                ui.set_sel_w(bounds.2);
                ui.set_sel_h(bounds.3);
                ui.set_selection_type(type_str.into());
                ui.set_cur_r(cr as i32);
                ui.set_cur_g(cg as i32);
                ui.set_cur_b(cb as i32);
                ui.set_cur_stroke_width(sw as i32);
                ui.set_cur_font_size(fs as i32);
            } else {
                drop(s);
                let mut s = state.lock().unwrap();
                s.interaction.clear_selection();
                ui.set_has_selection(false);
                ui.set_selection_type("".into());
            }
        });
    }

    // ── Drag move (select mode) ─────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_page_drag_move(move |page, x, y| {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            let Some(idx) = s.interaction.selected_idx else { return };
            let sel_page = s.interaction.selected_page;
            if sel_page != page as u32 { return; }
            let scale = s.scale;

            if !s.interaction.dragging {
                let ann_clone = s.annotations.get_for_page(sel_page).get(idx).cloned();
                if let Some(ann) = ann_clone {
                    s.interaction.start_drag(x, y, &ann);
                }
                return;
            }

            // Compute new selection box position from drag delta (visual only, no re-render)
            let dx_px = x - s.interaction.drag_start_x;
            let dy_px = y - s.interaction.drag_start_y;
            let page_height_pt = s.page_dims.get((page - 1) as usize)
                .map(|d| if s.rotation == 90 || s.rotation == 270 { d.width_pt } else { d.height_pt })
                .unwrap_or(841.0);

            // Get original bounds to compute visual offset
            let orig_x = s.interaction.drag_orig_pdf_x;
            let orig_y = s.interaction.drag_orig_pdf_y;
            let dx_pdf = dx_px as f64 / scale as f64;
            let dy_pdf = -(dy_px as f64) / scale as f64;

            // Temporarily compute where the annotation would be
            let ann_clone = s.annotations.get_for_page(sel_page).get(idx).cloned();
            if let Some(mut tmp_ann) = ann_clone {
                crate::annotation::interaction::set_ann_origin(&mut tmp_ann, orig_x + dx_pdf, orig_y + dy_pdf);
                let bounds = ann_canvas_bounds(&tmp_ann, scale as f64, page_height_pt as f64);
                ui.set_sel_x(bounds.0);
                ui.set_sel_y(bounds.1);
                ui.set_sel_w(bounds.2);
                ui.set_sel_h(bounds.3);
            }
        });
    }

    // ── Drag end (select mode) ──────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_page_drag_end(move |page, x, y| {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            if !s.interaction.dragging {
                s.interaction.end_drag();
                return;
            }
            let Some(idx) = s.interaction.selected_idx else {
                s.interaction.end_drag();
                return;
            };
            let sel_page = s.interaction.selected_page;
            let scale = s.scale;

            // Apply final position
            let dx_pdf = (x - s.interaction.drag_start_x) as f64 / scale as f64;
            let dy_pdf = -(y - s.interaction.drag_start_y) as f64 / scale as f64;
            let orig_x = s.interaction.drag_orig_pdf_x;
            let orig_y = s.interaction.drag_orig_pdf_y;

            if let Some(anns) = s.annotations.get_mut_for_page(sel_page) {
                if let Some(ann) = anns.get_mut(idx) {
                    crate::annotation::interaction::set_ann_origin(ann, orig_x + dx_pdf, orig_y + dy_pdf);
                }
            }
            s.interaction.end_drag();
            s.dirty = true;
            update_ui(&pdfium, &s, &ui);
        });
    }

    // ── Delete selected ─────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_delete_selected(move || {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            let Some(idx) = s.interaction.selected_idx else { return };
            let page = s.interaction.selected_page;
            s.annotations.remove(page, idx);
            s.interaction.clear_selection();
            s.dirty = true;
            ui.set_has_selection(false);
            update_ui(&pdfium, &s, &ui);
        });
    }

    // ── Set color ────────────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_set_color(move |r, g, b| {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            let color = crate::pdf::models::RgbColor { r: r as u8, g: g as u8, b: b as u8 };
            s.interaction.color = color.clone();
            ui.set_cur_r(r);
            ui.set_cur_g(g);
            ui.set_cur_b(b);

            // Apply to selected annotation if any
            if let Some(idx) = s.interaction.selected_idx {
                let page = s.interaction.selected_page;
                if let Some(anns) = s.annotations.get_mut_for_page(page) {
                    if let Some(ann) = anns.get_mut(idx) {
                        set_ann_color(ann, &color);
                        s.dirty = true;
                    }
                }
                update_ui(&pdfium, &s, &ui);
            }
        });
    }

    // ── Set stroke width ─────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_set_stroke_width(move |w| {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            s.interaction.stroke_width = w as f64;
            ui.set_cur_stroke_width(w as i32);

            if let Some(idx) = s.interaction.selected_idx {
                let page = s.interaction.selected_page;
                if let Some(anns) = s.annotations.get_mut_for_page(page) {
                    if let Some(ann) = anns.get_mut(idx) {
                        set_ann_stroke_width(ann, w as f64);
                        s.dirty = true;
                    }
                }
                update_ui(&pdfium, &s, &ui);
            }
        });
    }

    // ── Set font size ────────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_set_font_size(move |fs| {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            s.interaction.font_size = fs as f64;
            ui.set_cur_font_size(fs as i32);

            if let Some(idx) = s.interaction.selected_idx {
                let page = s.interaction.selected_page;
                if let Some(anns) = s.annotations.get_mut_for_page(page) {
                    if let Some(ann) = anns.get_mut(idx) {
                        if let crate::pdf::models::Annotation::Text(t) = ann {
                            t.font_size = fs as f64;
                            s.dirty = true;
                        }
                    }
                }
                update_ui(&pdfium, &s, &ui);
            }
        });
    }

    // ── Resize ───────────────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_resize_start(move |_handle, page, anchor_x, anchor_y| {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            if s.interaction.selected_idx.is_none() { return; }
            s.interaction.dragging = false;
            // Store anchor (the fixed corner) in canvas px
            s.interaction.drag_start_x = anchor_x;
            s.interaction.drag_start_y = anchor_y;
            s.interaction.start_page = page as u32;
            ui.set_resizing(true);
        });
    }
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_resize_move(move |mouse_x, mouse_y| {
            let ui = ui_weak.unwrap();
            let s = state.lock().unwrap();
            if s.interaction.selected_idx.is_none() { return; }

            // Anchor is the fixed corner, mouse is the moving corner
            let ax = s.interaction.drag_start_x;
            let ay = s.interaction.drag_start_y;

            // Update selection box visually (no re-render)
            let left_px = ax.min(mouse_x);
            let top_px = ay.min(mouse_y);
            let w_px = (mouse_x - ax).abs().max(10.0);
            let h_px = (mouse_y - ay).abs().max(10.0);

            ui.set_sel_x(left_px);
            ui.set_sel_y(top_px);
            ui.set_sel_w(w_px);
            ui.set_sel_h(h_px);
        });
    }
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_resize_end(move || {
            let ui = ui_weak.unwrap();
            let mut s = state.lock().unwrap();
            let Some(idx) = s.interaction.selected_idx else {
                ui.set_resizing(false);
                return;
            };
            let page = s.interaction.selected_page;
            let scale = s.scale;
            let page_idx = (page - 1) as usize;
            let page_height_pt = s.page_dims.get(page_idx)
                .map(|d| if s.rotation == 90 || s.rotation == 270 { d.width_pt } else { d.height_pt })
                .unwrap_or(841.0) as f64;

            // Read final selection box from UI
            let left_px = ui.get_sel_x();
            let top_px = ui.get_sel_y();
            let w_px = ui.get_sel_w();
            let h_px = ui.get_sel_h();

            let pdf_left = left_px as f64 / scale as f64;
            let pdf_bottom = page_height_pt - (top_px as f64 + h_px as f64) / scale as f64;
            let pdf_w = w_px as f64 / scale as f64;
            let pdf_h = h_px as f64 / scale as f64;

            if pdf_w > 5.0 && pdf_h > 5.0 {
                if let Some(anns) = s.annotations.get_mut_for_page(page) {
                    if let Some(ann) = anns.get_mut(idx) {
                        match ann {
                            crate::pdf::models::Annotation::Rect(r) => {
                                r.x = pdf_left; r.y = pdf_bottom; r.width = pdf_w; r.height = pdf_h;
                            }
                            crate::pdf::models::Annotation::Circle(c) => {
                                c.x = pdf_left; c.y = pdf_bottom; c.width = pdf_w; c.height = pdf_h;
                            }
                            crate::pdf::models::Annotation::Text(t) => {
                                t.x = pdf_left; t.y = pdf_bottom + pdf_h; t.width = pdf_w;
                            }
                            crate::pdf::models::Annotation::Signature(sig) => {
                                sig.x = pdf_left; sig.y = pdf_bottom; sig.width = pdf_w; sig.height = pdf_h;
                            }
                        }
                    }
                }
            }

            s.dirty = true;
            ui.set_resizing(false);
            update_ui(&pdfium, &s, &ui);
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

    // ── Text input dialog ──────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        ui.on_text_input_submit(move |text| {
            let ui = ui_weak.unwrap();
            ui.set_show_text_input(false);
            let mut s = state.lock().unwrap();
            if let Some(mut ann) = s.interaction.pending_text_ann.take() {
                if !text.is_empty() {
                    if let crate::pdf::models::Annotation::Text(ref mut t) = ann {
                        t.content = text.to_string();
                    }
                    s.annotations.add(ann);
                    s.dirty = true;
                    update_ui(&pdfium, &s, &ui);
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_text_input_cancel(move || {
            let ui = ui_weak.unwrap();
            ui.set_show_text_input(false);
            let mut s = state.lock().unwrap();
            s.interaction.pending_text_ann = None;
        });
    }

    // ── Signature ─────────────────────────────────────────────────────────────
    {
        // High-res canvas (2x the display size for crisp rendering)
        const SIG_W: u32 = 960;
        const SIG_H: u32 = 360;
        // Scale factor: Slint modal draws at ~480x180, canvas is 2x
        const SIG_SCALE: f32 = 2.0;

        let sig_pixmap: std::rc::Rc<std::cell::RefCell<tiny_skia::Pixmap>> =
            std::rc::Rc::new(std::cell::RefCell::new(tiny_skia::Pixmap::new(SIG_W, SIG_H).unwrap()));
        let sig_b64: std::rc::Rc<std::cell::RefCell<Option<String>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let sig_draw_count: std::rc::Rc<std::cell::Cell<u32>> =
            std::rc::Rc::new(std::cell::Cell::new(0));
        // Store all stroke points for smooth re-rendering
        // Each stroke is a Vec of (x, y) in high-res canvas coords
        let sig_strokes: std::rc::Rc<std::cell::RefCell<Vec<Vec<(f32, f32)>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

        {
            let ui_weak = ui.as_weak();
            let sig_pixmap = sig_pixmap.clone();
            let sig_strokes = sig_strokes.clone();
            ui.on_open_signature_modal(move || {
                let ui = ui_weak.unwrap();
                let mut pm = sig_pixmap.borrow_mut();
                *pm = tiny_skia::Pixmap::new(SIG_W, SIG_H).unwrap();
                sig_strokes.borrow_mut().clear();
                update_sig_image(&ui, &pm);
                ui.set_show_signature_modal(true);
            });
        }
        {
            let ui_weak = ui.as_weak();
            let sig_pixmap = sig_pixmap.clone();
            let sig_draw_count = sig_draw_count.clone();
            let sig_strokes = sig_strokes.clone();
            ui.on_sig_draw(move |x1, y1, x2, y2| {
                let mut pm = sig_pixmap.borrow_mut();

                let sx2 = x2 * SIG_SCALE;
                let sy2 = y2 * SIG_SCALE;
                let sx1 = x1 * SIG_SCALE;
                let sy1 = y1 * SIG_SCALE;

                // Collect point for smooth re-rendering later
                {
                    let mut strokes = sig_strokes.borrow_mut();
                    if strokes.is_empty() || strokes.last().map_or(true, |s| s.is_empty()) {
                        strokes.push(vec![(sx1, sy1)]);
                    }
                    if let Some(current) = strokes.last_mut() {
                        current.push((sx2, sy2));
                    }
                }

                // Draw rough preview line
                let dx = sx2 - sx1;
                let dy = sy2 - sy1;
                let angle = dy.atan2(dx);
                let nib_angle = std::f32::consts::FRAC_PI_4;
                let cross = (angle - nib_angle).sin().abs();
                let width = 1.5 + cross * 5.0;

                let mut paint = tiny_skia::Paint::default();
                paint.set_color(tiny_skia::Color::from_rgba8(15, 15, 35, 255));
                paint.anti_alias = true;
                let stroke = tiny_skia::Stroke {
                    width,
                    line_cap: tiny_skia::LineCap::Round,
                    line_join: tiny_skia::LineJoin::Round,
                    ..tiny_skia::Stroke::default()
                };
                let mut pb = tiny_skia::PathBuilder::new();
                pb.move_to(sx1, sy1);
                pb.line_to(sx2, sy2);
                if let Some(path) = pb.finish() {
                    pm.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
                }

                let count = sig_draw_count.get() + 1;
                sig_draw_count.set(count);
                if count % 3 == 0 {
                    update_sig_image(&ui_weak.unwrap(), &pm);
                }
            });
        }
        {
            let ui_weak = ui.as_weak();
            let sig_pixmap = sig_pixmap.clone();
            let sig_strokes = sig_strokes.clone();
            ui.on_sig_draw_end(move || {
                let ui = ui_weak.unwrap();
                // Mark end of current stroke
                sig_strokes.borrow_mut().push(Vec::new());
                // Re-render all strokes with smooth Bezier curves
                let mut pm = sig_pixmap.borrow_mut();
                *pm = tiny_skia::Pixmap::new(SIG_W, SIG_H).unwrap();
                render_smooth_strokes(&mut pm, &sig_strokes.borrow());
                update_sig_image(&ui, &pm);
            });
        }
        {
            let ui_weak = ui.as_weak();
            let sig_pixmap = sig_pixmap.clone();
            let sig_strokes = sig_strokes.clone();
            ui.on_sig_clear(move || {
                let ui = ui_weak.unwrap();
                let mut pm = sig_pixmap.borrow_mut();
                *pm = tiny_skia::Pixmap::new(SIG_W, SIG_H).unwrap();
                sig_strokes.borrow_mut().clear();
                update_sig_image(&ui, &pm);
            });
        }
        {
            let ui_weak = ui.as_weak();
            let sig_pixmap = sig_pixmap.clone();
            ui.on_sig_upload(move || {
                let ui = ui_weak.unwrap();
                let Some(path) = rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg"])
                    .pick_file()
                else { return };
                if let Ok(img) = image::open(&path) {
                    let resized = img.resize(SIG_W, SIG_H, image::imageops::FilterType::Lanczos3);
                    let rgba = resized.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    let mut pm = sig_pixmap.borrow_mut();
                    *pm = tiny_skia::Pixmap::new(SIG_W, SIG_H).unwrap();
                    if let Some(src) = tiny_skia::Pixmap::from_vec(
                        rgba.into_raw(), tiny_skia::IntSize::from_wh(w, h).unwrap(),
                    ) {
                        let dx = ((SIG_W - w) / 2) as i32;
                        let dy = ((SIG_H - h) / 2) as i32;
                        pm.draw_pixmap(dx, dy, src.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
                    }
                    update_sig_image(&ui, &pm);
                }
            });
        }
        {
            let ui_weak = ui.as_weak();
            let sig_pixmap = sig_pixmap.clone();
            let sig_b64 = sig_b64.clone();
            ui.on_sig_place(move || {
                let ui = ui_weak.unwrap();
                let pm = sig_pixmap.borrow();
                // Flush final image
                update_sig_image(&ui, &pm);
                let has_content = pm.data().chunks(4).any(|px| px[3] > 0);
                if !has_content { return; }
                let img = image::RgbaImage::from_raw(SIG_W, SIG_H, pm.data().to_vec()).unwrap();
                let mut png_buf: Vec<u8> = Vec::new();
                image::DynamicImage::ImageRgba8(img)
                    .write_to(&mut std::io::Cursor::new(&mut png_buf), image::ImageFormat::Png)
                    .unwrap();
                let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_buf);
                *sig_b64.borrow_mut() = Some(b64);
                ui.set_show_signature_modal(false);
                ui.set_sig_placing(true);
            });
        }
        {
            let ui_weak = ui.as_weak();
            ui.on_sig_cancel(move || {
                ui_weak.unwrap().set_show_signature_modal(false);
            });
        }
        {
            let ui_weak = ui.as_weak();
            let pdfium = pdfium.clone();
            let state = state.clone();
            let sig_b64 = sig_b64.clone();
            ui.on_sig_click_on_page(move |page, x, y| {
                let ui = ui_weak.unwrap();
                ui.set_sig_placing(false);
                let Some(b64) = sig_b64.borrow_mut().take() else { return };
                let mut s = state.lock().unwrap();
                let page_idx = (page - 1) as usize;
                let page_height_pt = s.page_dims.get(page_idx)
                    .map(|d| if s.rotation == 90 || s.rotation == 270 { d.width_pt } else { d.height_pt })
                    .unwrap_or(841.0);
                let scale = s.scale;
                let pdf_x = x as f64 / scale as f64;
                let pdf_y = page_height_pt as f64 - y as f64 / scale as f64;
                let ann = crate::pdf::models::Annotation::Signature(
                    crate::pdf::models::SignatureAnnotation {
                        page: page as u32,
                        x: pdf_x - 75.0,
                        y: pdf_y - 30.0,
                        width: 150.0,
                        height: 60.0,
                        image_data: b64,
                    },
                );
                s.annotations.add(ann);
                s.dirty = true;
                update_ui(&pdfium, &s, &ui);
            });
        }
    }

    // ── Save As ──────────────────────────────────────────────────────────────
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_save_file_as(move || {
            let ui = ui_weak.unwrap();
            let s = state.lock().unwrap();
            let Some(src_path) = &s.file_path else { return };
            let all_annotations = s.annotations.all();

            let Some(dest) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .set_title("Save As")
                .set_file_name(
                    src_path.file_name().unwrap_or_default().to_string_lossy().to_string()
                )
                .save_file()
            else { return };

            // Copy original file to destination first, then save annotations
            if dest != *src_path {
                if let Err(e) = std::fs::copy(src_path, &dest) {
                    ui.set_status_text(SharedString::from(format!("Copy error: {}", e)));
                    return;
                }
            }

            match save_annotated_pdf(&dest, &all_annotations) {
                Ok(()) => {
                    ui.set_status_text(SharedString::from(format!(
                        "Saved as {}",
                        dest.file_name().unwrap_or_default().to_string_lossy()
                    )));
                }
                Err(e) => {
                    ui.set_status_text(SharedString::from(format!("Save error: {}", e)));
                }
            }
        });
    }

    // ── Scroll position tracking + lazy rendering (poll every 150ms) ────────
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let state = state.clone();
        let pages_vm = pages_vm.clone();
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

            // Lazy render pages that became visible
            if let Some(vm) = pages_vm.borrow().as_ref() {
                let viewport_h = ui.get_viewer_height();
                render_visible_pages(&pdfium, &s, vm, scroll_y, viewport_h);
            }
        });
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

fn set_ann_color(ann: &mut crate::pdf::models::Annotation, color: &crate::pdf::models::RgbColor) {
    match ann {
        crate::pdf::models::Annotation::Rect(r) => r.color = color.clone(),
        crate::pdf::models::Annotation::Circle(c) => c.color = color.clone(),
        crate::pdf::models::Annotation::Text(t) => t.color = color.clone(),
        crate::pdf::models::Annotation::Signature(_) => {}
    }
}

fn set_ann_stroke_width(ann: &mut crate::pdf::models::Annotation, w: f64) {
    match ann {
        crate::pdf::models::Annotation::Rect(r) => r.stroke_width = w,
        crate::pdf::models::Annotation::Circle(c) => c.stroke_width = w,
        _ => {}
    }
}

fn ann_type_str(ann: &crate::pdf::models::Annotation) -> &'static str {
    match ann {
        crate::pdf::models::Annotation::Rect(_) => "rect",
        crate::pdf::models::Annotation::Circle(_) => "circle",
        crate::pdf::models::Annotation::Text(_) => "text",
        crate::pdf::models::Annotation::Signature(_) => "signature",
    }
}

/// Re-render all strokes using Catmull-Rom spline interpolation for smooth curves.
fn render_smooth_strokes(pixmap: &mut tiny_skia::Pixmap, strokes: &[Vec<(f32, f32)>]) {
    let mut paint = tiny_skia::Paint::default();
    paint.set_color(tiny_skia::Color::from_rgba8(15, 15, 35, 255));
    paint.anti_alias = true;

    for points in strokes {
        if points.len() < 2 { continue; }

        // For each consecutive pair of points, compute Catmull-Rom control points
        // and draw a cubic Bezier curve with calligraphic width
        let n = points.len();
        for i in 0..n - 1 {
            let p0 = if i > 0 { points[i - 1] } else { points[i] };
            let p1 = points[i];
            let p2 = points[i + 1];
            let p3 = if i + 2 < n { points[i + 2] } else { points[i + 1] };

            // Catmull-Rom to cubic Bezier control points
            let cp1x = p1.0 + (p2.0 - p0.0) / 6.0;
            let cp1y = p1.1 + (p2.1 - p0.1) / 6.0;
            let cp2x = p2.0 - (p3.0 - p1.0) / 6.0;
            let cp2y = p2.1 - (p3.1 - p1.1) / 6.0;

            // Calligraphic width based on angle
            let dx = p2.0 - p1.0;
            let dy = p2.1 - p1.1;
            let angle = dy.atan2(dx);
            let nib_angle = std::f32::consts::FRAC_PI_4;
            let cross = (angle - nib_angle).sin().abs();
            let width = 1.5 + cross * 5.0;

            let stroke = tiny_skia::Stroke {
                width,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                ..tiny_skia::Stroke::default()
            };

            let mut pb = tiny_skia::PathBuilder::new();
            pb.move_to(p1.0, p1.1);
            pb.cubic_to(cp1x, cp1y, cp2x, cp2y, p2.0, p2.1);
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
            }
        }
    }
}

fn update_sig_image(ui: &App, pixmap: &tiny_skia::Pixmap) {
    let w = pixmap.width();
    let h = pixmap.height();
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    buf.make_mut_bytes().copy_from_slice(pixmap.data());
    ui.set_sig_canvas_image(Image::from_rgba8(buf));
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
