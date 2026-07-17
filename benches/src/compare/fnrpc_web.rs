use std::sync::Arc;

use dhat::{HeapStats, Profiler};
use fnrpc::error::RpcErr;
use fnrpc::handler::{RawRpcFn, RpcFn};
use fnrpc::router::RpcRouterBuilder;
use fnrpc_web::{handle, RpcWebConfig};
use xitca_http::body::RequestBody;
use xitca_http::bytes::Bytes;
use xitca_http::http::{Method, Request, RequestExt, Uri};

struct Noop;
impl RpcFn<()> for Noop {
    type Input = ();
    type Output = ();
    const NAME: &'static str = "noop";
    fn exec(_ctx: &(), _input: ()) -> Result<(), RpcErr> { Ok(()) }
}

struct Echo;
impl RpcFn<()> for Echo {
    type Input = String;
    type Output = String;
    const NAME: &'static str = "echo";
    fn exec(_ctx: &(), input: String) -> Result<String, RpcErr> { Ok(input) }
}

struct RawNoop;
impl RawRpcFn<()> for RawNoop {
    const NAME: &'static str = "raw_noop";
    fn exec(_ctx: &(), _input: &[u8]) -> Result<Vec<u8>, RpcErr> { Ok(b"ok".to_vec()) }
}

pub(crate) async fn bench(n: usize) {
    let config = RpcWebConfig {
        router: RpcRouterBuilder::<()>::new()
            .query(Noop)
            .query(Echo)
            .raw(RawNoop)
            .build(),
        ctx_from_headers: Arc::new(|_| ()),
    };

    // Pre-parse URIs outside profiler to exclude URI allocation cost
    let uri_noop: Uri = "/noop?input=null".parse().unwrap();
    let uri_raw: Uri = "/raw_noop".parse().unwrap();
    let uri_echo: Uri = r#"/echo?input=%22hello%22"#.parse().unwrap();
    let uri_echo_post: Uri = "/echo".parse().unwrap();
    let uri_nf: Uri = "/nonexistent".parse().unwrap();

    let body_data: Vec<u8> = br#""hello""#.to_vec();

    // Build requests outside profiler too — reuse URI and body
    fn build_get(uri: &Uri) -> Request<RequestBody> {
        let mut req = Request::new(RequestBody::None);
        *req.method_mut() = Method::GET;
        *req.uri_mut() = uri.clone();
        req
    }

    fn build_post(uri: &Uri, body: &[u8]) -> Request<RequestExt<RequestBody>> {
        let body: RequestBody = Bytes::copy_from_slice(body).into();
        let mut req = Request::new(
            RequestExt::default().map_body(|_: RequestBody| body),
        );
        *req.method_mut() = Method::POST;
        *req.uri_mut() = uri.clone();
        req
    }

    // — noop_json —
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let _ = handle(&config, build_get(&uri_noop)).await;
    }
    let s = HeapStats::get();
    eprintln!("fnrpc-web/noop_json: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-fnrpc-web-noop-json.json");

    // — noop_raw —
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let _ = handle(&config, build_get(&uri_raw)).await;
    }
    let s = HeapStats::get();
    eprintln!("fnrpc-web/noop_raw: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-fnrpc-web-noop-raw.json");

    // — echo_get —
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let _ = handle(&config, build_get(&uri_echo)).await;
    }
    let s = HeapStats::get();
    eprintln!("fnrpc-web/echo_get: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-fnrpc-web-echo-get.json");

    // — echo_post —
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let _ = handle(&config, build_post(&uri_echo_post, &body_data)).await;
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
        let _ = handle(&config, build_get(&uri_nf)).await;
    }
    let s = HeapStats::get();
    eprintln!("fnrpc-web/not_found: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-fnrpc-web-notfound.json");
}
