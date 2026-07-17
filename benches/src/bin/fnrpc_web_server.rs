use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use fnrpc::error::RpcErr;
use fnrpc::handler::{RawRpcFn, RpcFn};
use fnrpc::router::RpcRouterBuilder;
use fnrpc_web::{RpcWebConfig, run};
use serde::Serialize;

type AppCtx = Arc<RwLock<HashMap<String, f64>>>;

// ── JSON handlers ───────────────────────────────────────

struct Noop;
impl RpcFn<AppCtx> for Noop {
    type Input = ();
    type Output = ();
    const NAME: &'static str = "noop";
    fn exec(_ctx: &AppCtx, _input: ()) -> Result<(), RpcErr> { Ok(()) }
}

struct Echo;
impl RpcFn<AppCtx> for Echo {
    type Input = String;
    type Output = String;
    const NAME: &'static str = "echo";
    fn exec(_ctx: &AppCtx, input: String) -> Result<String, RpcErr> { Ok(input) }
}

/// Simulates the tt benchmark's `/in?key=` endpoint.
struct Lookup;
impl RawRpcFn<AppCtx> for Lookup {
    const NAME: &'static str = "in";
    fn exec(ctx: &AppCtx, input: &[u8]) -> Result<Vec<u8>, RpcErr> {
        let query_str = std::str::from_utf8(input).unwrap_or("");
        let key = query_str
            .split('&')
            .find_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                if parts.next() == Some("key") {
                    parts.next()
                } else {
                    None
                }
            })
            .unwrap_or("");
        let n = ctx.read().unwrap().get(key).copied().unwrap_or(0.0);
        // Use serde_json for fair comparison with other frameworks
        let output = LookupOutput { entity: key.to_string(), n };
        let bytes = serde_json::to_vec(&output).unwrap_or_default();
        Ok(bytes)
    }
}

#[derive(Serialize)]
struct LookupOutput {
    entity: String,
    n: f64,
}

// ── Raw handler ─────────────────────────────────────────

struct RawNoop;
impl RawRpcFn<AppCtx> for RawNoop {
    const NAME: &'static str = "raw_noop";
    fn exec(_ctx: &AppCtx, _input: &[u8]) -> Result<Vec<u8>, RpcErr> { Ok(b"ok".to_vec()) }
}

// ── Server setup ────────────────────────────────────────

fn parse_args() -> u16 {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--port" {
            if let Some(p) = args.get(i + 1) {
                return p.parse().unwrap_or(19111);
            }
        }
    }
    19111
}

#[tokio::main]
async fn main() {
    let port = parse_args();

    let data = Arc::new(RwLock::new(HashMap::from([
        ("actix".to_string(), 1.0),
        ("axum".to_string(), 2.0),
        ("gin".to_string(), 3.0),
        ("fnrpc".to_string(), 4.0),
    ])));

    let config = RpcWebConfig {
        router: RpcRouterBuilder::<AppCtx>::new()
            .query(Noop)
            .query(Echo)
            .raw(Lookup)
            .raw(RawNoop)
            .build(),
        ctx_from_headers: Arc::new(move |_| data.clone()),
    };
    let config = Arc::new(config);

    println!("Starting fnrpc-web server on :{port}");
    run(config, &format!("0.0.0.0:{port}")).await.expect("failed to bind");
}
