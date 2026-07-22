//! Call-path dhat analysis for the `route_bytes` text path in fnrpc-web.
//!
//! Generates per-backtrace allocation JSON so we can pinpoint exactly which
//! source lines produce the ~3 blocks/op measured for `fnrpc-web/text`.
//!
//! Usage:
//!   cargo run -p benches --bin dhat_callpath_text --features dhat-heap -- 5000
//!
//! Then analyze each JSON:
//!   cargo run -p benches --bin dhat_analyze --features dhat-heap -- benches/target/dhat-text-short.json
//!   cargo run -p benches --bin dhat_analyze --features dhat-heap -- benches/target/dhat-text-long.json
//!
//! The two files use paths of DIFFERENT lengths that BOTH match a registered
//! handler. The dispatch path `String` (lib.rs:291 `path.to_owned()`) is the
//! only one of the three blocks whose byte size scales with path length, so
//! comparing the `str::to_owned` block's bytes between the two files positively
//! identifies it.

use dhat::{Alloc, HeapStats, Profiler};
use fnrpc::router::RpcRouterBuilder;
use fnrpc_web::App;
use xitca_http::http::{Method, Request, RequestExt, Uri};

#[global_allocator]
static ALLOC: Alloc = Alloc;

const SHORT: &str = "/text";
const LONG: &str = "/text_with_a_much_longer_path_name";

#[fnrpc::rpc_bytes("text")]
async fn text_short(_input: &[u8]) -> &'static [u8] {
    b"ok"
}

#[fnrpc::rpc_bytes("text_with_a_much_longer_path_name")]
async fn text_long(_input: &[u8]) -> &'static [u8] {
    b"ok"
}

fn make_req(uri: &str) -> Request<RequestExt<xitca_http::body::RequestBody>> {
    Request::builder()
        .method(Method::GET)
        .uri(uri.parse::<Uri>().unwrap())
        .body(RequestExt::default())
        .unwrap()
}

/// Measure N requests through `route_bytes`, writing a call-path JSON to `file`.
/// The `Request` is prebuilt (outside the profiler) so per-request allocation
/// noise from request construction is excluded — only fnrpc's handler-path
/// allocations appear.
async fn measure(file: &str, uri: &str, n: usize) {
    let router = RpcRouterBuilder::<()>::new()
        .route_bytes(text_short)
        .route_bytes(text_long)
        .build();
    let app = App::new(router, |_| ());

    // Prebuild all N requests OUTSIDE the profiler so the per-request
    // `RequestExt`/`Extension` allocation noise is excluded. Only fnrpc's
    // handler-path allocations appear in the trace (matching dhat_compare's
    // clean 3 blks/op).
    let reqs: Vec<_> = (0..n).map(|_| make_req(uri)).collect();

    // Profiler starts here — only request-path allocations are traced.
    let _p = Profiler::builder().file_name(file).build();
    for req in reqs {
        let _ = app.call(req).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "{} ({}): {:>10}B, {:>6} blks  (~{:.1}B, {:.1} blks/op)",
        file,
        uri,
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5000);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(measure("benches/target/dhat-text-short.json", SHORT, n));
    rt.block_on(measure("benches/target/dhat-text-long.json", LONG, n));
}
