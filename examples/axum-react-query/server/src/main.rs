mod axum_router;
mod ctx;
mod rpc_func;

use axum::serve;
use std::path::PathBuf;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let app_state = ctx::AppState {
        app_dir: PathBuf::from("."),
    };

    let fnrpc_router = rpc_func::build_fn_rpc_router().layer(fnrpc::middleware::TracingLayer);

    let router = axum_router::build_axum_router(fnrpc_router, app_state);

    let listener = TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind");
    println!("Server listening on http://localhost:3000");
    serve(listener, router).await.expect("failed to serve");
}
