use std::sync::Arc;

use dhat::{HeapStats, Profiler};
use fnrpc::error::RpcErr;
use fnrpc::handler::{RawRpcFn, RpcFn};
use fnrpc::router::RpcRouterBuilder;
use fnrpc_web::{handle, RpcWebConfig};
use xitca_http::body::RequestBody;
use xitca_http::bytes::Bytes;
use xitca_http::http::{Method, Request, RequestExt};

// ── fnrpc-web JSON handlers ─────────────────────────────

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

// ── fnrpc-web raw handler ──────────────────────────────

struct RawNoop;
impl RawRpcFn<()> for RawNoop {
    const NAME: &'static str = "raw_noop";
    fn exec(_ctx: &(), _input: &[u8]) -> Result<&'static [u8], RpcErr> {
        Ok(b"ok")
    }
}

// ── Request helpers ────────────────────────────────────

fn make_get_req(uri: &str) -> Request<RequestBody> {
    let mut req = Request::new(RequestBody::None);
    *req.method_mut() = Method::GET;
    *req.uri_mut() = uri.parse().unwrap();
    req
}

fn make_raw_get_req(uri: &str) -> Request<RequestBody> {
    let mut req = Request::new(RequestBody::None);
    *req.method_mut() = Method::GET;
    *req.uri_mut() = uri.parse().unwrap();
    req
}

fn make_post_req(uri: &str, body: &[u8]) -> Request<RequestExt<RequestBody>> {
    let body: RequestBody = Bytes::copy_from_slice(body).into();
    let mut req = Request::new(
        RequestExt::default().map_body(|_: RequestBody| body),
    );
    *req.method_mut() = Method::POST;
    *req.uri_mut() = uri.parse().unwrap();
    req
}

// ── Benchmark ──────────────────────────────────────────

pub(crate) async fn bench(n: usize) {
    let config = RpcWebConfig {
        router: RpcRouterBuilder::<()>::new()
            .query(Noop)
            .query(Echo)
            .raw(RawNoop)
            .build(),
        ctx_from_headers: Arc::new(|_| ()),
    };

    // — noop_json (GET, JSON Content-Type) —
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let req = make_get_req("/noop?input=null");
        let _ = handle(&config, req).await;
    }
    let s = HeapStats::get();
    eprintln!("fnrpc-web/noop_json: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-fnrpc-web-noop-json.json");

    // — noop_raw (GET, raw Content-Type, RawRpcFn handler) —
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let req = make_raw_get_req("/raw_noop");
        let _ = handle(&config, req).await;
    }
    let s = HeapStats::get();
    eprintln!("fnrpc-web/noop_raw: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-fnrpc-web-noop-raw.json");

    // — echo (GET) —
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let req = make_get_req(r#"/echo?input=%22hello%22"#);
        let _ = handle(&config, req).await;
    }
    let s = HeapStats::get();
    eprintln!("fnrpc-web/echo_get: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-fnrpc-web-echo-get.json");

    // — echo (POST) —
    let body_data = br#""hello""#;
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let req = make_post_req("/echo", body_data);
        let _ = handle(&config, req).await;
    }
    let s = HeapStats::get();
    eprintln!("fnrpc-web/echo_post: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-fnrpc-web-echo-post.json");

    // — not_found —
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let req = make_get_req("/nonexistent");
        let _ = handle(&config, req).await;
    }
    let s = HeapStats::get();
    eprintln!("fnrpc-web/not_found: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-fnrpc-web-notfound.json");
}
