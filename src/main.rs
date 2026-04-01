// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod pdf;

slint::include_modules!();

fn main() {
    let ui = App::new().unwrap();

    app::setup(&ui);

    ui.run().unwrap();
}
