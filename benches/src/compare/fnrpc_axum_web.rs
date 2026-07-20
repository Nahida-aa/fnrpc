//! fnrpc mounted on Axum — allocation comparison with axum and fnrpc-web.

use std::sync::Arc;

use dhat::{HeapStats, Profiler};
use axum::Router;
use axum::routing::get;
use fnrpc::router::RpcRouterBuilder;
use fnrpc_axum::{FnrpcState, handle};
use tower::ServiceExt;

// ── Handlers ──────────────────────────────────────────

#[fnrpc::rpc_query]
async fn echo_macro(input: String) -> String {
    input
}

#[fnrpc::rpc_bytes]
async fn noop_raw(_input: &[u8]) -> &'static [u8] {
    b"ok"
}

// ── Request building ─────────────────────────────────

fn build_get(uri: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method(axum::http::Method::GET)
        .uri(uri)
        .body(axum::body::Body::empty())
        .unwrap()
}

fn prebuild_get(uri: &str, n: usize) -> Vec<axum::http::Request<axum::body::Body>> {
    (0..n).map(|_| build_get(uri)).collect()
}

// ── Benchmarks ────────────────────────────────────────

pub(crate) async fn bench_macro(n: usize) {
    let router = RpcRouterBuilder::<()>::new().route_fn(echo_macro).build();
    let state = Arc::new(FnrpcState::new(router, |_| ()));
    let app = Router::new()
        .route("/{*path}", get(handle::<()>))
        .with_state(state);

    let uri = "/echo?input=%22hello%22";
    let reqs = prebuild_get(uri, n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = app.clone().oneshot(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-axum/echo_macro: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}

pub(crate) async fn bench_noop_raw(n: usize) {
    let router = RpcRouterBuilder::<()>::new().route_bytes(noop_raw).build();
    let state = Arc::new(FnrpcState::new(router, |_| ()));
    let app = Router::new()
        .route("/{*path}", get(handle::<()>))
        .with_state(state);

    let uri = "/noop_raw";
    let reqs = prebuild_get(uri, n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = app.clone().oneshot(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-axum/noop_raw: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}
