//! Server binary for the fnrpc-axum e2e example. See `lib.rs` for the
//! procedures and shared router.
//!
//! Run:   cargo run --manifest-path e2e/fnrpc-axum/server/Cargo.toml
//! Test:  cd e2e/fnrpc-axum/client && bun run

use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use fnrpc_axum::{FnrpcState, handle};

#[tokio::main]
async fn main() {
    let router = e2e_fnrpc_axum_server::build_fn_rpc_router();

    let state = Arc::new(FnrpcState::new(router, |_| ()));

    let app = Router::new()
        .route("/{*path}", get(handle::<()>).post(handle::<()>))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("fnrpc-axum e2e server listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
