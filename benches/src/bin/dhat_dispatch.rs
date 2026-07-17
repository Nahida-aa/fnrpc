use dhat::{Alloc, HeapStats, Profiler};
use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouterBuilder;
use serde_json::Value;

struct Noop;
impl RpcFn<()> for Noop {
    type Input = ();
    type Output = ();
    const NAME: &'static str = "noop";
    async fn exec(_ctx: &(), _input: ()) -> Result<(), RpcErr> {
        Ok(())
    }
}

#[global_allocator]
static ALLOC: Alloc = Alloc;

fn bench(label: &str, f: impl Fn()) {
    let _prof = Profiler::new_heap();
    f();
    let stats = HeapStats::get();
    let n = stats.total_blocks;
    let b = stats.total_bytes;
    eprintln!("{label}: {b}B, {n} blks");
}

fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let router = RpcRouterBuilder::<()>::new().query(Noop).build();

    // Just a single call instead of loop
    bench("dispatch_send_x1", || {
        rt.block_on(async {
            router.dispatch_send(&(), "noop", Value::Null).await.unwrap();
        });
    });

    // Warm the caches
    rt.block_on(async {
        for _ in 0..50 {
            router.dispatch_send(&(), "noop", Value::Null).await.unwrap();
        }
    });

    bench("dispatch_send_x1_cold_ext", || {
        rt.block_on(async {
            router.dispatch_send(&(), "noop", Value::Null).await.unwrap();
        });
    });
}
