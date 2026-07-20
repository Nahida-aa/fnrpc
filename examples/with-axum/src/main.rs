//! Axum + fnrpc example.
//!
//! Run: cargo run -p with-axum
//! Test: curl "http://127.0.0.1:3000/greet?input=%22world%22"
//!       curl -X POST "http://127.0.0.1:3000/echo" -d '"hello"'

use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use fnrpc::router::RpcRouterBuilder;
use fnrpc_axum::{FnrpcState, handle};

// ── Handlers ──────────────────────────────────────────

/// A query handler — GET, input from query string.
#[fnrpc::rpc_query]
async fn greet(input: String) -> String {
    format!("Hello {input}!")
}

/// A mutate handler — POST, input from body.
#[fnrpc::rpc_mutate]
async fn echo(input: String) -> String {
    input
}

// ── Main ──────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let router = RpcRouterBuilder::<()>::new()
        .route_fn(greet)
        .route_fn(echo)
        .build();

    let state = Arc::new(FnrpcState::new(router, |_| ()));

    let app = Router::new()
        .route("/{*path}", get(handle::<()>).post(handle::<()>))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
