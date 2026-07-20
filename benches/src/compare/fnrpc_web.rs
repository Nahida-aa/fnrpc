use std::pin::Pin;

use dhat::{HeapStats, Profiler};
use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouterBuilder;
use fnrpc_web::App;
use xitca_http::body::RequestBody;
use xitca_http::bytes::Bytes;
use xitca_http::http::{Method, Request, RequestExt, Uri};

// ── 宏生成的 handler ──

#[fnrpc::rpc_query]
async fn echo_macro(input: String) -> String {
    input
}

// ── 手写的 handler ──

struct EchoManual;
impl RpcFn<()> for EchoManual {
    type Input = String;
    type Output = String;
    const KEY: &'static str = "echo";
    fn exec(
        _ctx: &(),
        input: String,
    ) -> Pin<Box<dyn futures::Future<Output = Result<String, RpcErr>> + Send + '_>> {
        Box::pin(async move { Ok(input) })
    }
}

#[fnrpc::rpc_mutate]
async fn echo_post(input: String) -> String {
    input
}

#[fnrpc::rpc_bytes]
async fn noop_raw(input: &[u8]) -> &'static [u8] {
    b"ok"
}

fn build_get(uri: &Uri) -> Request<RequestExt<RequestBody>> {
    let req_ext: RequestExt<RequestBody> = RequestExt::default();
    Request::builder()
        .method(Method::GET)
        .uri(uri.clone())
        .body(req_ext)
        .unwrap()
}

fn build_post(uri: &Uri, body: &[u8]) -> Request<RequestExt<RequestBody>> {
    let body: RequestBody = xitca_http::bytes::Bytes::copy_from_slice(body).into();
    let mut req = Request::new(RequestExt::default().map_body(|_: RequestBody| body));
    *req.method_mut() = Method::POST;
    *req.uri_mut() = uri.clone();
    req
}

fn prebuild_get(uri: &Uri, n: usize) -> Vec<Request<RequestExt<RequestBody>>> {
    (0..n).map(|_| build_get(uri)).collect()
}

fn prebuild_post(uri: &Uri, body: &[u8], n: usize) -> Vec<Request<RequestExt<RequestBody>>> {
    (0..n).map(|_| build_post(uri, body)).collect()
}

pub(crate) async fn bench_macro(n: usize) {
    let router = RpcRouterBuilder::<()>::new().route_fn(echo_macro).build();
    let app = App::new(router, |_| ());
    let uri_echo_get: Uri = r#"/echo?input=%22hello%22"#.parse().unwrap();
    let reqs = prebuild_get(&uri_echo_get, n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = app.call(req).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-web/echo_macro: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}

pub(crate) async fn bench_manual(n: usize) {
    let router = RpcRouterBuilder::<()>::new().route_fn(EchoManual).build();
    let app = App::new(router, |_| ());
    let uri_echo_get: Uri = r#"/echo?input=%22hello%22"#.parse().unwrap();
    let reqs = prebuild_get(&uri_echo_get, n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = app.call(req).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-web/echo_manual: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}

pub(crate) async fn bench_post(n: usize) {
    let router = RpcRouterBuilder::<()>::new().route_fn(echo_post).build();
    let app = App::new(router, |_| ());
    let uri_echo: Uri = "/echo".parse().unwrap();
    let body_data: Vec<u8> = br#""hello""#.to_vec();
    let reqs = prebuild_post(&uri_echo, &body_data, n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = app.call(req).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-web/echo_post: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}

pub(crate) async fn bench_noop_raw(n: usize) {
    let router = RpcRouterBuilder::<()>::new().route_bytes(noop_raw).build();
    let app = App::new(router, |_| ());
    let uri_noop_raw: Uri = "/noop_raw".parse().unwrap();
    let reqs = prebuild_get(&uri_noop_raw, n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = app.call(req).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-web/noop_raw: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}

use fnrpc::middlewares::hook::HookLayer;

pub(crate) async fn bench_macro_mw(n: usize) {
    let router = RpcRouterBuilder::<()>::new()
        .route_fn(echo_macro)
        .layer(HookLayer::new().before(|_ctx, _path, input, _is_get| {
            Ok(input)
        }))
        .build();
    let app = App::new(router, |_| ());
    let uri_echo_get: Uri = r#"/echo?input=%22hello%22"#.parse().unwrap();
    let reqs = prebuild_get(&uri_echo_get, n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = app.call(req).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-web/echo_macro_mw: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}

/// Benchmark: AppBuilder multi-router with RPC + static route, RPC path.
/// Measures the cost of routing through AppBuilder vs direct App::new.
// ── Subscribe handler ──────────────────────────────────

#[fnrpc::rpc_subscribe]
fn echo_sub(input: u32) -> impl futures::Stream<Item = u32> {
    futures::stream::iter(1..=input)
}

/// Benchmark subscribe dispatch via RpcRouter::dispatch_subscribe.
/// This measures the cost of looking up and calling a subscribe handler
/// through the erased trait object, including stream creation.
pub(crate) async fn bench_subscribe(n: usize) {
    use futures::StreamExt;
    use fnrpc::handler::SubscribeExt;
    let router = RpcRouterBuilder::<()>::new().subscribe(echo_sub).build();
    let input = br#"0"#; // valid JSON input for u32

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for _ in 0..n {
        let _ = router.dispatch_subscribe(&(), "echo_sub", input);
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-web/subscribe: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}

/// Benchmark SSE response built from a subscribe handler.
///
/// This simulates what a transport layer would do: call dispatch_subscribe,
/// format each item as SSE `data: ...\n\n`, and build a Response.
/// Unlike bench_subscribe, this includes the overhead of SSE framing and
/// Response construction — comparable to xitca-web/sse.
pub(crate) async fn bench_sse(n: usize) {
    use futures::StreamExt;
    use fnrpc::handler::SubscribeExt;
    use xitca_http::body::{Frame, ResponseBody, StreamBody};
    use xitca_http::bytes::Bytes;
    use xitca_http::http::{Response, StatusCode};
    use xitca_http::http::header::{CONTENT_TYPE, HeaderValue};

    let router = RpcRouterBuilder::<()>::new().subscribe(echo_sub).build();

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for _ in 0..n {
        let mut stream = router.dispatch_subscribe(&(), "echo_sub", br#"10"#).unwrap();
        // Build SSE response: read first item, format as SSE, construct Response
        // We only consume one item to keep the benchmark focused on dispatch + SSE framing,
        // not on the full stream iteration (which would dominate with 10 items).
        let item = stream.next().await;
        let body = match item {
            Some(Ok(bytes)) => {
                let sse = format!("data: {}\n\n", String::from_utf8_lossy(&bytes));
                ResponseBody::body(StreamBody::new(futures::stream::once(
                    futures::future::ready(Ok::<_, std::convert::Infallible>(Frame::Data(Bytes::from(sse)))),
                )))
            }
            _ => ResponseBody::body(StreamBody::new(futures::stream::once(
                futures::future::ready(Ok::<_, std::convert::Infallible>(Frame::Data(Bytes::from_static(b"data: error\n\n")))),
            ))),
        };
        let _ = Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))
            .body(body);
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-web/sse: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}

pub(crate) async fn bench_macro_multi(n: usize) {
    use std::path::PathBuf;
    let router = RpcRouterBuilder::<()>::new().route_fn(echo_macro).build();
    let app = App::build(|_| ())
        .rpc("/api/{*path}", router)
        .rpc("/echo", RpcRouterBuilder::<()>::new().route_fn(echo_macro).build())
        .static_dir("/static", "./");
    let uri_echo_get: Uri = r#"/echo?input=%22hello%22"#.parse().unwrap();
    let reqs = prebuild_get(&uri_echo_get, n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = app.call(req).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-web/echo_macro_multi: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}
