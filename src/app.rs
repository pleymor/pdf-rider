use crate::App;
use pdfium_render::prelude::*;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};
use std::sync::{Arc, Mutex};

/// Render a page from the given document at the given scale.
fn render_page(doc: &PdfDocument, page_index: u16, scale: f32) -> Option<Image> {
    let page = doc.pages().get(page_index).ok()?;
    let width = (page.width().value * scale) as i32;
    let height = (page.height().value * scale) as i32;

    let bitmap = page
        .render_with_config(
            &PdfRenderConfig::new()
                .set_target_width(width)
                .set_target_height(height),
        )
        .ok()?;

    let img = bitmap.as_image();
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();

    let mut pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    pixel_buffer.make_mut_bytes().copy_from_slice(&rgba);

    Some(Image::from_rgba8(pixel_buffer))
}

struct ViewerState {
    current_page: u16,
    page_count: u16,
    scale: f32,
}

pub fn setup(ui: &App) {
    // Initialize PDFium
    let bindings = Pdfium::bind_to_library(
        Pdfium::pdfium_platform_library_name_at_path("./"),
    )
    .or_else(|_| Pdfium::bind_to_system_library())
    .expect("Failed to load PDFium library. Place pdfium.dll next to the executable.");

    let pdfium = Arc::new(Pdfium::new(bindings));

    let viewer = Arc::new(Mutex::new(ViewerState {
        current_page: 0,
        page_count: 0,
        scale: 1.5,
    }));

    // Open file callback
    {
        let ui_weak = ui.as_weak();
        let pdfium = pdfium.clone();
        let viewer = viewer.clone();
        ui.on_open_file(move || {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("PDF files", &["pdf"])
                .pick_file()
            else {
                return;
            };

            let ui = ui_weak.unwrap();
            match pdfium.load_pdf_from_file(&path, None) {
                Ok(doc) => {
                    let page_count = doc.pages().len();
                    {
                        let mut v = viewer.lock().unwrap();
                        v.current_page = 0;
                        v.page_count = page_count;
                    }

                    let scale = viewer.lock().unwrap().scale;
                    if let Some(img) = render_page(&doc, 0, scale) {
                        ui.set_current_page_image(img);
                    }

                    ui.set_status_text(
                        format!("{} — {} pages", path.display(), page_count).into(),
                    );
                    ui.set_has_document(true);
                }
                Err(e) => {
                    ui.set_status_text(format!("Error: {}", e).into());
                }
            }
        });
    }

    // Zoom in callback
    {
        let ui_weak = ui.as_weak();
        let viewer = viewer.clone();
        ui.on_zoom_in(move || {
            let mut v = viewer.lock().unwrap();
            v.scale = (v.scale + 0.25).min(5.0);
            let _ = ui_weak.unwrap();
            // Re-render will be handled when we have persistent doc reference
        });
    }

    // Zoom out callback
    {
        let ui_weak = ui.as_weak();
        let viewer = viewer.clone();
        ui.on_zoom_out(move || {
            let mut v = viewer.lock().unwrap();
            v.scale = (v.scale - 0.25).max(0.25);
            let _ = ui_weak.unwrap();
            // Re-render will be handled when we have persistent doc reference
        });
    }
}
