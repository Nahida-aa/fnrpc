//! xitca-web + fnrpc example.
//!
//! Run: cargo run -p with-xitca
//! Test: curl "http://127.0.0.1:3000/greet?input=%22world%22"
//!       curl -X POST "http://127.0.0.1:3000/echo" -d '"hello"'

use fnrpc::router::RpcRouterBuilder;
use fnrpc_xitca::{FnrpcState, handle};
use xitca_web::route::get;
use xitca_web::service::fn_service;
use xitca_web::{App, WebContext};

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

fn main() -> std::io::Result<()> {
    let router = RpcRouterBuilder::<()>::new()
        .route_fn(greet)
        .route_fn(echo)
        .build();

    let state = FnrpcState::new(router, |_| ());

    App::new()
        .with_state(state)
        .at("/{*path}", get(fn_service(handle::<()>)).post(fn_service(handle::<()>)))
        .serve()
        .bind("0.0.0.0:3000")?
        .run()
        .wait()
}
