use std::sync::Arc;

mod ctx;
pub mod feat;
pub mod integrations;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let app_state = ctx::AppState {
        app_dir: std::path::PathBuf::from("."),
    };
    let fnrpc_router = Arc::new(integrations::fnrpc_func::build_fn_rpc_router());

    // Start axum HTTP server in background, sharing the same router
    let axum_router = integrations::fnrpc_axum::build_axum_router(
        Arc::clone(&fnrpc_router),
        app_state.clone(),
    );
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:19110")
            .await
            .expect("failed to bind");
        axum::serve(listener, axum_router)
            .await
            .expect("failed to serve");
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(fnrpc_router)
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            integrations::fnrpc_tauri::rpc_fn,
            integrations::fnrpc_tauri::rpc_sub,
            integrations::fnrpc_tauri::rpc_cancel_sub,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
