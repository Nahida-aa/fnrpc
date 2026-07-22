//! Axum baseline — allocation comparison with fnrpc-axum.

use dhat::{HeapStats, Profiler};
use axum::Router;
use axum::routing::get;
use axum::response::Json;
use serde_json::Value;
use std::convert::Infallible;
use tower::ServiceExt;

// ── Handlers ──────────────────────────────────────────

async fn handler_echo_get(query: axum::extract::Query<Value>) -> Json<Value> {
    Json(query.0)
}

async fn handler_noop_raw() -> &'static str {
    "ok"
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

pub async fn bench(n: usize) {
    let app = Router::new()
        .route("/echo", get(handler_echo_get))
        .route("/noop_raw", get(handler_noop_raw));

    let uri_echo_get = "/echo?input=%22hello%22";
    let reqs = prebuild_get(uri_echo_get, n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = app.clone().oneshot(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!(
        "axum/echo_get: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}
