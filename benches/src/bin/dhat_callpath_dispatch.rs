//! Call-path dhat test that isolates the `let mut extensions = Extensions::new()`
//! line inside `RpcRouter::dispatch` (router.rs:171).
//!
//! We bypass the HTTP `App` entirely and call `RpcRouter::dispatch` directly with
//! two handlers registered into DIFFERENT slots:
//!   - `text`      -> HandlerSlot::Raw    (route_bytes)
//!   - `text_raw`  -> HandlerSlot::Erased  (route_raw)
//!
//! Each path is traced into its OWN profiler file. `dhat_analyze` then shows
//! whether `Extensions::new` appears in the Erased trace but NOT in the Raw
//! trace — directly attributing the extra block to router.rs:171.
//!
//! Usage:
//!   cargo run -p benches --bin dhat_callpath_dispatch --features dhat-heap -- 5000
//!
//! Analyze:
//!   cargo run -p benches --bin dhat_analyze --features dhat-heap -- benches/target/dhat-dispatch-raw.json
//!   cargo run -p benches --bin dhat_analyze --features dhat-heap -- benches/target/dhat-dispatch-erased.json

use dhat::{Alloc, HeapStats, Profiler};
use fnrpc::router::RpcRouterBuilder;

#[global_allocator]
static ALLOC: Alloc = Alloc;

// Raw slot: route_bytes -> HandlerSlot::Raw
#[fnrpc::rpc_bytes("text")]
async fn text(_input: &[u8]) -> &'static [u8] {
    b"ok"
}

// Erased slot: route_raw -> HandlerSlot::Erased (dispatch builds Extensions::new)
#[fnrpc::rpc_raw]
async fn text_raw(_input: &[u8]) -> fnrpc::RpcOutput {
    fnrpc::RpcOutput::ok(b"ok")
}

/// Dispatch `path` N times through `router`, tracing into `file`.
async fn measure_dispatch(file: &str, router: &fnrpc::router::RpcRouter<()>, path: &str, n: usize) {
    let ctx = ();
    let input: &[u8] = b"";
    let is_get = true;

    // Profiler starts here — only `dispatch` internals are traced.
    let _p = Profiler::builder().file_name(file).build();
    for _ in 0..n {
        let _ = router.dispatch(&ctx, path, input, is_get).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "{} ({}): {:>10}B, {:>6} blks  (~{:.1}B, {:.1} blks/op)",
        file,
        path,
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

    let raw_router = RpcRouterBuilder::<()>::new().route_bytes(text).build();
    let erased_router = RpcRouterBuilder::<()>::new().route_raw(text_raw).build();

    rt.block_on(measure_dispatch(
        "benches/target/dhat-dispatch-raw.json",
        &raw_router,
        "text",
        n,
    ));
    rt.block_on(measure_dispatch(
        "benches/target/dhat-dispatch-erased.json",
        &erased_router,
        "text_raw",
        n,
    ));

    eprintln!();
    eprintln!("Now run dhat_analyze on each file and look for `Extensions::new`:");
    eprintln!("  - raw.json    should NOT contain Extensions::new");
    eprintln!("  - erased.json SHOULD contain Extensions::new (the +1 block)");
}
