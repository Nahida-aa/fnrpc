use dhat::{HeapStats, Profiler};
use tower::ServiceExt;

async fn handler_noop() -> &'static str {
    "ok"
}

pub(crate) async fn run(label: &str, n: usize) {
    let app = axum::Router::new()
        .route("/", axum::routing::get(handler_noop));

    // — plain/noop —
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = axum::http::Request::builder()
            .uri("/")
            .body(axum::body::Body::empty())
            .unwrap();
        let _ = app.clone().oneshot(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("{label}/noop: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
}
