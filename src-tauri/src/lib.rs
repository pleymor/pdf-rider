mod commands;
mod pdf;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::dialog::open_pdf_dialog,
            commands::dialog::save_pdf_dialog,
            commands::pdf::get_page_count,
            commands::pdf::save_annotated_pdf,
            commands::pdf::read_annotations,
            commands::pdf::export_annotated_pdf,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}
