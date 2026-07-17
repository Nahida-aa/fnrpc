use std::sync::Arc;

use dhat::{HeapStats, Profiler};
use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouterBuilder;
use xitca_web::http::request;
use xitca_web::route::get;
use xitca_web::service::{fn_service, Service};
use xitca_web::App;

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

pub(crate) async fn run(label: &str, n: usize) {
    let state = Arc::new(fnrpc_xitca::FnrpcState {
        router: RpcRouterBuilder::<()>::new().query(Noop).query(Echo).build(),
        ctx_from_headers: Arc::new(|_| ()),
    });

    let app = App::new()
        .with_state(state)
        .at(
            "/{*path}",
            get(fn_service(fnrpc_xitca::dispatch::<()>))
                .post(fn_service(fnrpc_xitca::dispatch::<()>)),
        );
    let svc = app.finish().call(()).await.unwrap();

    // — fnrpc-xitca/noop —
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = request::Builder::default()
            .uri("/fnrpc/noop?input=null")
            .body(Default::default())
            .unwrap();
        let _ = svc.call(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("{label}/noop: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);

    // — fnrpc-xitca/echo —
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = request::Builder::default()
            .uri(r#"/fnrpc/echo?input=%22hello%22"#)
            .body(Default::default())
            .unwrap();
        let _ = svc.call(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("{label}/echo: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);

    // — fnrpc-xitca/not_found —
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = request::Builder::default()
            .uri("/fnrpc/nonexistent")
            .body(Default::default())
            .unwrap();
        let _ = svc.call(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("{label}/not_found: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
}
