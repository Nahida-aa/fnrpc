use std::sync::Arc;

use dhat::{HeapStats, Profiler};
use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouterBuilder;
use fnrpc_web::{handle, FnrpcConfig};
use xitca_http::body::RequestBody;
use xitca_http::http::{Method, Request, RequestExt};

struct Noop;
impl RpcFn<()> for Noop {
    type Input = ();
    type Output = ();
    const NAME: &'static str = "noop";
    async fn exec(_ctx: &(), _input: ()) -> Result<(), RpcErr> {
        Ok(())
    }
}

struct Echo;
impl RpcFn<()> for Echo {
    type Input = String;
    type Output = String;
    const NAME: &'static str = "echo";
    async fn exec(_ctx: &(), input: String) -> Result<String, RpcErr> {
        Ok(input)
    }
}

fn make_get_req(uri: &str) -> Request<RequestExt<RequestBody>> {
    let mut req = Request::new(RequestExt::default());
    *req.method_mut() = Method::GET;
    *req.uri_mut() = uri.parse().unwrap();
    req
}

pub(crate) async fn run(label: &str, n: usize) {
    let config = FnrpcConfig {
        router: RpcRouterBuilder::<()>::new().query(Noop).query(Echo).build(),
        ctx_from_headers: Arc::new(|_| ()),
    };

    // — fnrpc-web/noop —
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = make_get_req("/fnrpc/noop?input=null");
        let _ = handle(&config, req).await;
    }
    let s = HeapStats::get();
    eprintln!("{label}/noop: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);

    // — fnrpc-web/echo —
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = make_get_req(r#"/fnrpc/echo?input=%22hello%22"#);
        let _ = handle(&config, req).await;
    }
    let s = HeapStats::get();
    eprintln!("{label}/echo: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);

    // — fnrpc-web/not_found —
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = make_get_req("/fnrpc/nonexistent");
        let _ = handle(&config, req).await;
    }
    let s = HeapStats::get();
    eprintln!("{label}/not_found: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
}
