use std::pin::Pin;
use std::sync::Arc;

use dhat::{HeapStats, Profiler};
use fnrpc::error::RpcErr;
use fnrpc::handler::{RpcFn, RpcFnExt};
use fnrpc_web::App;
use xitca_http::http::{Method, Request, RequestExt, Uri};
use xitca_http::body::RequestBody;

#[derive(Clone)]
struct Echo;
impl RpcFn<()> for Echo {
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

pub(crate) async fn bench(n: usize) {
    let app = App::new(|_| ())
        .register(Echo);

    let uri_echo_get: Uri = r#"/echo?input=%22hello%22"#.parse().unwrap();

    fn build_get(uri: &Uri) -> Request<RequestExt<RequestBody>> {
        let req_ext: RequestExt<RequestBody> = RequestExt::default();
        Request::builder()
            .method(Method::GET)
            .uri(uri.clone())
            .body(req_ext)
            .unwrap()
    }

    // — echo_get —
    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for _ in 0..n {
        let _ = app.call_request(build_get(&uri_echo_get)).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-web/echo_get: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
    let _ = std::fs::copy(
        "./benches/target/dhat-heap.json",
        "./benches/target/dhat-fnrpc-web-echo-get.json",
    );
}
