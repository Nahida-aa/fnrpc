use std::pin::Pin;

use dhat::{HeapStats, Profiler};
use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouterBuilder;
use fnrpc_web::App;
use xitca_http::body::RequestBody;
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

fn build_get(uri: &Uri) -> Request<RequestExt<RequestBody>> {
    let req_ext: RequestExt<RequestBody> = RequestExt::default();
    Request::builder()
        .method(Method::GET)
        .uri(uri.clone())
        .body(req_ext)
        .unwrap()
}

pub(crate) async fn bench_macro(n: usize) {
    let router = RpcRouterBuilder::<()>::new().route(echo_macro).build();
    let app = App::new(router, |_| ());
    let uri_echo_get: Uri = r#"/echo?input=%22hello%22"#.parse().unwrap();

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for _ in 0..n {
        let _ = app.call(build_get(&uri_echo_get)).await;
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
    let router = RpcRouterBuilder::<()>::new().route(EchoManual).build();
    let app = App::new(router, |_| ());
    let uri_echo_get: Uri = r#"/echo?input=%22hello%22"#.parse().unwrap();

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for _ in 0..n {
        let _ = app.call(build_get(&uri_echo_get)).await;
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
