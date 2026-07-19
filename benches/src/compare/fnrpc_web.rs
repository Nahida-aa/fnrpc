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

use fnrpc::middleware::HookLayer;

pub(crate) async fn bench_macro_mw(n: usize) {
    let router = RpcRouterBuilder::<()>::new()
        .route_fn(echo_macro)
        .layer(HookLayer::new().before(|_ctx, _path, _input| Ok(())))
        .build();
    let app = App::new(router, |_| ());
    let uri_echo_get: Uri = r#"/echo?input=%22hello%22"#.parse().unwrap();
    let reqs = prebuild_get(&uri_echo_get, n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in &reqs {
        let _ = app.call(build_get(&uri_echo_get)).await;
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
