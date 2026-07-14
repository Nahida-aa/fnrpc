// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[tokio::main]
async fn main() {
    tokio::spawn(tauri_solid_tanstack_lib::run_axum());
    tauri_solid_tanstack_lib::run();
}
