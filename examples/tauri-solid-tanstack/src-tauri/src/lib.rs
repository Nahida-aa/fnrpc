mod ctx;
pub mod feat;
pub mod integrations;
// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = ctx::AppState {
        app_dir: std::path::PathBuf::from("."),
    };
    let fnrpc_router =
        integrations::fnrpc_func::build_fn_rpc().layer(fnrpc::middleware::TracingLayer);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(fnrpc_router)
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            integrations::fnrpc_tauri::rpc_fn,
            integrations::fnrpc_tauri::rpc_subscribe,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
