use std::sync::Arc;

use dhat::{HeapStats, Profiler};
use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouterBuilder;
use tower::ServiceExt;

struct Noop;
impl RpcFn<()> for Noop {
    type Input = ();
    type Output = ();
    const NAME: &'static str = "noop";
    fn exec(_ctx: &(), _input: ()) -> Result<(), RpcErr> {
        Ok(())
    }
}

struct Echo;
impl RpcFn<()> for Echo {
    type Input = String;
    type Output = String;
    const NAME: &'static str = "echo";
    fn exec(_ctx: &(), input: String) -> Result<String, RpcErr> {
        Ok(input)
    }
}

pub(crate) async fn run(label: &str, n: usize) {
    let router = RpcRouterBuilder::<()>::new().query(Noop).query(Echo).build();
    let app = axum::Router::new()
        .route(
            "/fnrpc/{*path}",
            axum::routing::get(fnrpc_axum::handle::<()>)
                .post(fnrpc_axum::handle::<()>),
        )
        .with_state(Arc::new(fnrpc_axum::FnrpcState {
            router: Arc::new(router),
            ctx_from_headers: Arc::new(|_| ()),
        }));

    // — fnrpc-axum/noop —
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = axum::http::Request::builder()
            .uri("/fnrpc/noop?input=null")
            .body(axum::body::Body::empty())
            .unwrap();
        let _ = app.clone().oneshot(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("{label}/noop: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);

    // — fnrpc-axum/echo —
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = axum::http::Request::builder()
            .uri(r#"/fnrpc/echo?input=%22hello%22"#)
            .body(axum::body::Body::empty())
            .unwrap();
        let _ = app.clone().oneshot(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("{label}/echo: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);

    // — fnrpc-axum/not_found —
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = axum::http::Request::builder()
            .uri("/fnrpc/nonexistent")
            .body(axum::body::Body::empty())
            .unwrap();
        let _ = app.clone().oneshot(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("{label}/not_found: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
}
