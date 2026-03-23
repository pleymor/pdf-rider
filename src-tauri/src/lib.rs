mod commands;
mod pdf;

use std::sync::Mutex;

#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupArgs {
    pub file_path: Option<String>,
    pub should_print: bool,
}

pub struct AppState {
    pub startup_args: Mutex<StartupArgs>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut args = std::env::args().skip(1); // skip the executable name
    let mut startup = StartupArgs::default();

    while let Some(arg) = args.next() {
        if arg == "--print" {
            startup.should_print = true;
            if let Some(path) = args.next() {
                startup.file_path = Some(path);
            }
        } else if !arg.starts_with("--") {
            startup.file_path = Some(arg);
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            startup_args: Mutex::new(startup),
        })
        .invoke_handler(tauri::generate_handler![
            commands::dialog::open_pdf_dialog,
            commands::dialog::save_pdf_dialog,
            commands::pdf::get_page_count,
            commands::pdf::save_annotated_pdf,
            commands::pdf::read_annotations,
            commands::shell::get_startup_args,
            commands::shell::check_pdf_association,
            commands::shell::register_pdf_handler,
            commands::shell::register_print_verb,
            commands::shell::open_default_apps_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}
