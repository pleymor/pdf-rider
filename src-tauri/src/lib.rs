mod commands;
mod pdf;
mod print_queue;

use std::sync::Mutex;

pub use print_queue::StartupArgs;

pub struct AppState {
    pub startup_args: Mutex<StartupArgs>,
    pub print_queue: Mutex<print_queue::PrintQueue>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cwd = std::env::current_dir().unwrap_or_default();
    let startup = print_queue::parse_args(std::env::args().skip(1), &cwd); // skip the executable name

    let mut builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    if startup.should_print {
        // Explorer starts one process per selected file: let the first one
        // print them all. Only `--print` processes take part, so opening
        // files keeps one window per file.
        builder = builder.plugin(tauri_plugin_single_instance::init(print_queue::on_handover));
    } else {
        // Keep Explorer's print verb pointing at this executable
        commands::shell::refresh_print_verb();
    }

    builder
        .manage(AppState {
            print_queue: Mutex::new(print_queue::PrintQueue::new(startup.print_files.clone())),
            startup_args: Mutex::new(startup),
        })
        .invoke_handler(tauri::generate_handler![
            commands::compress::compress_pdf,
            commands::dialog::open_pdf_dialog,
            commands::dialog::save_pdf_dialog,
            commands::dialog::pick_directory_dialog,
            commands::pdf::get_page_count,
            commands::pdf::save_annotated_pdf,
            commands::pdf::strip_annotation_streams,
            commands::pdf::read_annotations,
            commands::pdf::modify_pages,
            commands::pdf::extract_pdf_pages,
            commands::pdf::split_pdf,
            commands::print::list_printers,
            commands::print::print_pages,
            print_queue::take_print_jobs,
            print_queue::finish_print_session,
            commands::shell::get_startup_args,
            commands::shell::check_pdf_association,
            commands::shell::register_pdf_handler,
            commands::shell::unregister_pdf_handler,
            commands::shell::register_print_verb,
            commands::shell::open_default_apps_settings,
            commands::shell::open_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}
