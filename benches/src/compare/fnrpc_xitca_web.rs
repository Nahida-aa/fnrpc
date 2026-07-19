//! fnrpc mounted on xitca-web — allocation comparison with fnrpc-web and xitca-web.

use dhat::{HeapStats, Profiler};
use fnrpc::router::RpcRouterBuilder;
use fnrpc_xitca::{FnrpcState, handle};
use xitca_unsafe_collection::futures::NowOrPanic;
use xitca_web::body::RequestBody;
use xitca_web::http::{Method, RequestExt, Uri, WebRequest};
use xitca_web::route::get;
use xitca_web::service::{Service, fn_service};
use xitca_web::App;

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

fn build_get(uri: &Uri) -> WebRequest<RequestBody> {
    let req_ext: RequestExt<RequestBody> = RequestExt::default();
    xitca_web::http::Request::builder()
        .method(Method::GET)
        .uri(uri.clone())
        .body(req_ext)
        .unwrap()
}

fn build_post(uri: &Uri, body: &[u8]) -> WebRequest<RequestBody> {
    let body: RequestBody = xitca_web::bytes::Bytes::copy_from_slice(body).into();
    let mut req = WebRequest::new(RequestExt::default().map_body(|_: RequestBody| body));
    *req.method_mut() = Method::POST;
    *req.uri_mut() = uri.clone();
    req
}

fn prebuild_get(uri: &Uri, n: usize) -> Vec<WebRequest<RequestBody>> {
    (0..n).map(|_| build_get(uri)).collect()
}

// ── Build service ────────────────────────────────────

fn build_svc(router: fnrpc::router::RpcRouter<()>) -> impl Service<WebRequest<RequestBody>> {
    let state = FnrpcState::new(router, |_| ());
    let app = App::new()
        .with_state(state)
        .at("/{*path}", get(fn_service(handle::<()>)));
    app.finish().call(()).now_or_panic().unwrap()
}

// ── Benchmarks ────────────────────────────────────────

pub(crate) async fn bench_macro(n: usize) {
    let router = RpcRouterBuilder::<()>::new().route_fn(echo_macro).build();
    let svc = build_svc(router);
    let uri_echo_get: Uri = r#"/echo?input=%22hello%22"#.parse().unwrap();
    let reqs = prebuild_get(&uri_echo_get, n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = svc.call(req).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-xitca-web/echo_macro: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}

pub(crate) async fn bench_noop_raw(n: usize) {
    let router = RpcRouterBuilder::<()>::new().route_bytes(noop_raw).build();
    let svc = build_svc(router);
    let uri_noop_raw: Uri = "/noop_raw".parse().unwrap();
    let reqs = prebuild_get(&uri_noop_raw, n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = svc.call(req).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-xitca-web/noop_raw: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}
