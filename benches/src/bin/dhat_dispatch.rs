use dhat::{Alloc, HeapStats, Profiler};
use fnrpc::error::RpcErr;
use fnrpc::handler::{RawRpcFn, RpcFn};
use fnrpc::router::RpcRouterBuilder;
use serde_json::Value;
use std::future::Future;

struct Noop;
impl RpcFn<()> for Noop {
    type Input = ();
    type Output = ();
    const KEY: &'static str = "noop";
    fn exec<'a>(
        _ctx: &'a (),
        _input: (),
    ) -> impl Future<Output = Result<(), RpcErr>> + Send + 'a {
        async move { Ok(()) }
    }
}

struct RawNoop;
impl RawRpcFn<()> for RawNoop {
    const KEY: &'static str = "raw_noop";
    fn exec(_ctx: &(), _input: &[u8]) -> Result<Vec<u8>, RpcErr> { Ok(b"ok".to_vec()) }
}

#[global_allocator]
static ALLOC: Alloc = Alloc;

fn bench(label: &str, f: impl Fn()) {
    let _prof = Profiler::new_heap();
    f();
    let stats = HeapStats::get();
    eprintln!("{label}: {}B, {} blks", stats.total_bytes, stats.total_blocks);
}

fn main() {
    // — Router build allocation —
    bench("router_build", || {
        let _router = RpcRouterBuilder::<()>::new()
            .query(Noop)
            .raw(RawNoop)
            .build();
    });

    // — Router with HashMap context (like lookup) —
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    type AppCtx = Arc<RwLock<HashMap<String, f64>>>;

    struct Lookup;
    impl RawRpcFn<AppCtx> for Lookup {
        const KEY: &'static str = "lookup";
        fn exec(ctx: &AppCtx, input: &[u8]) -> Result<Vec<u8>, RpcErr> {
            let query_str = std::str::from_utf8(input).unwrap_or("");
            let key = query_str.split('&').find_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                if parts.next() == Some("key") { parts.next() } else { None }
            }).unwrap_or("");
            let n = ctx.read().unwrap().get(key).copied().unwrap_or(0.0);
            let output = serde_json::json!({"entity": key, "n": n});
            serde_json::to_vec(&output).map_err(|_| RpcErr::internal("err"))
        }
    }

    let data = Arc::new(RwLock::new(HashMap::from([
        ("actix".into(), 1.0), ("axum".into(), 2.0),
        ("gin".into(), 3.0), ("fnrpc".into(), 4.0),
    ])));

    bench("router_build_lookup", || {
        let _router = RpcRouterBuilder::<AppCtx>::new()
            .raw(Lookup)
            .build();
    });

    // — RpcWebConfig build (includes ctx_from_headers Arc) —
    bench("config_build", || {
        let _config = fnrpc_web::RpcWebConfig {
            router: RpcRouterBuilder::<()>::new().query(Noop).build(),
            ctx_from_headers: Arc::new(|_| ()),
        };
    });

    // — Full config with lookup context —
    let data2 = data.clone();
    bench("config_build_lookup", || {
        let d = data2.clone();
        let _config = fnrpc_web::RpcWebConfig {
            router: RpcRouterBuilder::<AppCtx>::new().raw(Lookup).build(),
            ctx_from_headers: Arc::new(move |_| d.clone()),
        };
    });
}
