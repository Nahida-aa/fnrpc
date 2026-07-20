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
    let app_state_axum = app_state.clone();
    let fnrpc_router = Arc::new(integrations::fnrpc_func::build_fn_rpc_router());

    // Start axum HTTP server in background, sharing the same router
    let axum_router = integrations::fnrpc_axum::build_axum_router(
        Arc::clone(&fnrpc_router),
        app_state_axum,
    );
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:19110")
            .await
            .expect("failed to bind");
        axum::serve(listener, axum_router)
            .await
            .expect("failed to serve");
    });

    let tauri_state = fnrpc_tauri::FnrpcTauriState::from_arc(fnrpc_router, move || {
        use axum::http::HeaderMap;
        ctx::Ctx {
            state: app_state.clone(),
            headers: HeaderMap::new(),
        }
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(tauri_state)
        .invoke_handler(fnrpc_tauri::generate_handler!(ctx::Ctx))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
