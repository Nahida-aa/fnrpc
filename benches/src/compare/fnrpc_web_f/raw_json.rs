use crate::compare::utils::prebuild_get;
use dhat::{HeapStats, Profiler};
use fnrpc::{RpcOutput, router::RpcRouterBuilder};
use fnrpc_web::App;
use http::{HeaderValue, header::CONTENT_TYPE};

#[fnrpc::rpc_raw]
async fn null_json(_input: &[u8]) -> RpcOutput {
    RpcOutput::ok(b"null").header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
}

/// Fair comparison point vs xitca-web's `null_json`: same body (`b"null"`)
/// and same `Content-Type: application/json` header, but served through
/// fnrpc's `route_raw` → `RpcOutput` path instead of xitca's native handler.
pub async fn bench_null_json(n: usize) {
    let router = RpcRouterBuilder::<()>::new().route_raw(null_json).build();
    let app = App::new(router, |_| ());
    let reqs = prebuild_get("/null_json", n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = app.call(req).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "fnrpc-web/null_json: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}
