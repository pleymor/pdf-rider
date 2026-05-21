use std::path::Path;
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, FilePath};

fn filepath_to_string(p: FilePath) -> String {
    match p {
        FilePath::Path(pb) => pb.to_string_lossy().into_owned(),
        FilePath::Url(u) => u.to_string(),
    }
}

/// Opens a native file-picker dialog filtered to PDF files.
/// Returns the selected absolute path, or null if cancelled.
#[tauri::command]
pub fn open_pdf_dialog(app: AppHandle) -> Result<Option<String>, String> {
    let result = app
        .dialog()
        .file()
        .add_filter("PDF Files", &["pdf"])
        .blocking_pick_file();

    Ok(result.map(filepath_to_string))
}

/// Opens a native Save As dialog.
/// Pre-fills the filename from `current_path` basename.
/// Returns the chosen absolute path, or null if cancelled.
#[tauri::command]
pub fn save_pdf_dialog(
    app: AppHandle,
    current_path: String,
) -> Result<Option<String>, String> {
    let default_name = Path::new(&current_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output.pdf")
        .to_string();

    let result = app
        .dialog()
        .file()
        .add_filter("PDF Files", &["pdf"])
        .set_file_name(&default_name)
        .blocking_save_file();

    Ok(result.map(filepath_to_string))
}

/// Opens a native folder-picker dialog. Returns the chosen path or null.
#[tauri::command]
pub fn pick_directory_dialog(app: AppHandle) -> Result<Option<String>, String> {
    let result = app.dialog().file().blocking_pick_folder();
    Ok(result.map(filepath_to_string))
}
