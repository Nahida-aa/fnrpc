use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use fnrpc::error::RpcErr;
use fnrpc::handler::{RawRpcFn, RpcFn};
use fnrpc::router::RpcRouterBuilder;
use fnrpc::rpc_query;
use fnrpc_web::{RpcWebConfig, run};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::future::Future;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

type AppCtx = Arc<RwLock<HashMap<String, f64>>>;

// ── Small payload: noop ────────────────────────────────

// struct Noop;
// impl RpcFn<AppCtx> for Noop {
//     type Input = ();
//     type Output = ();
//     const KEY: &'static str = "noop";
//     fn exec<'a>(
//         _ctx: &'a AppCtx,
//         _input: (),
//     ) -> impl Future<Output = Result<(), RpcErr>> + Send + 'a {
//         async move { Ok(()) }
//     }
// }

#[fnrpc::rpc_query]
async fn noop() -> () {}

// ── Small payload: echo string (POST) ──────────────────

// struct Echo;
// impl RpcFn<AppCtx> for Echo {
//     type Input = String;
//     type Output = String;
//     const KEY: &'static str = "echo";
//     const METHOD: &'static str = "POST";
//     fn exec<'a>(
//         _ctx: &'a AppCtx,
//         input: String,
//     ) -> impl Future<Output = Result<String, RpcErr>> + Send + 'a {
//         async move { Ok(input) }
//     }
// }

#[fnrpc::rpc_query]
async fn echo(input: String) -> String {
    input
}

// ── Medium payload: user profile (~200B JSON, POST) ───

#[derive(Serialize, Deserialize, Type)]
struct MediumPayload {
    id: u32,
    name: String,
    email: String,
    tags: Vec<String>,
    score: f64,
}

// struct Medium;
// impl RpcFn<AppCtx> for Medium {
//     type Input = MediumPayload;
//     type Output = MediumPayload;
//     const KEY: &'static str = "medium";
//     const METHOD: &'static str = "POST";
//     fn exec<'a>(
//         _ctx: &'a AppCtx,
//         input: MediumPayload,
//     ) -> impl Future<Output = Result<MediumPayload, RpcErr>> + Send + 'a {
//         async move { Ok(input) }
//     }
// }

#[fnrpc::rpc_mutate]
async fn medium(input: MediumPayload) -> MediumPayload {
    input
}

// ── Large payload: batch data (~10KB JSON, POST) ──────

#[derive(Serialize, Deserialize, Type)]
struct LargePayload {
    items: Vec<LargeItem>,
}

#[derive(Serialize, Deserialize, Type)]
struct LargeItem {
    id: u32,
    name: String,
    description: String,
    price: f64,
    quantity: u32,
    category: String,
    tags: Vec<String>,
    metadata: HashMap<String, String>,
}

#[fnrpc::rpc_mutate]
async fn large(input: LargePayload) -> LargePayload {
    input
}

// struct Large;
// impl RpcFn<AppCtx> for Large {
//     type Input = LargePayload;
//     type Output = LargePayload;
//     const KEY: &'static str = "large";
//     const METHOD: &'static str = "POST";
//     fn exec<'a>(
//         _ctx: &'a AppCtx,
//         input: LargePayload,
//     ) -> impl Future<Output = Result<LargePayload, RpcErr>> + Send + 'a {
//         async move { Ok(input) }
//     }
// }

// ── Lookup (HashMap read + JSON response) ──────────────

// struct Lookup;
// impl RawRpcFn<AppCtx> for Lookup {
//     const KEY: &'static str = "in";
//     fn exec(ctx: &AppCtx, input: &[u8]) -> Result<Vec<u8>, RpcErr> {
//         let query_str = std::str::from_utf8(input).unwrap_or("");
//         let key = query_str
//             .split('&')
//             .find_map(|pair| {
//                 let mut parts = pair.splitn(2, '=');
//                 if parts.next() == Some("key") {
//                     parts.next()
//                 } else {
//                     None
//                 }
//             })
//             .unwrap_or("");
//         let n = ctx.read().unwrap().get(key).copied().unwrap_or(0.0);
//         let output = LookupOutput {
//             entity: key.to_string(),
//             n,
//         };
//         serde_json::to_vec(&output).map_err(|_| RpcErr::internal("serialize error"))
//     }
// }
// #[fnrpc::rpc_bytes]
// async fn in_fn(input: &[u8]) -> Vec<u8> {
//     let query_str = std::str::from_utf8(input).unwrap_or("");
//     let key = query_str
//         .split('&')
//         .find_map(|pair| {
//             let mut parts = pair.splitn(2, '=');
//             if parts.next() == Some("key") {
//                 parts.next()
//             } else {
//                 None
//             }
//         })
//         .unwrap_or("");
//     let n = ctx.read().unwrap().get(key).copied().unwrap_or(0.0);
//     let output = LookupOutput {
//         entity: key.to_string(),
//         n,
//     };
// }

#[derive(Serialize, Type)]
struct LookupOutput {
    entity: String,
    n: f64,
}

// ── TechEmpower-style endpoints ────────────────────────

// struct JsonEndpoint;
// impl RpcFn<AppCtx> for JsonEndpoint {
//     type Input = ();
//     type Output = JsonMessage;
//     const KEY: &'static str = "json";
//     fn exec<'a>(
//         _ctx: &'a AppCtx,
//         _input: (),
//     ) -> impl Future<Output = Result<JsonMessage, RpcErr>> + Send + 'a {
//         async move {
//             Ok(JsonMessage {
//                 message: "Hello, World!",
//             })
//         }
//     }
// }

#[fnrpc::rpc_query]
async fn json_get() -> JsonMessage {
    JsonMessage {
        message: "Hello, World!",
    }
}

#[derive(Serialize, Type)]
struct JsonMessage {
    message: &'static str,
}

#[fnrpc::rpc_bytes]
async fn plaintext(_input: &[u8]) -> &'static [u8] {
    b"Hello, World!"
}
// struct PlaintextEndpoint;
// impl RawRpcFn<AppCtx> for PlaintextEndpoint {
//     const KEY: &'static str = "plaintext";
//     fn exec(_ctx: &AppCtx, _input: &[u8]) -> Result<Vec<u8>, RpcErr> {
//         Ok(b"Hello, World!".to_vec())
//     }
// }

// ── Raw handler ─────────────────────────────────────────

// struct RawNoop;
// impl RawRpcFn<AppCtx> for RawNoop {
//     const KEY: &'static str = "raw_noop";
//     fn exec(_ctx: &AppCtx, _input: &[u8]) -> Result<Vec<u8>, RpcErr> {
//         Ok(b"ok".to_vec())
//     }
// }

#[fnrpc::rpc_bytes]
async fn raw_noop(_input: &[u8]) -> &'static [u8] {
    b"ok"
}

// ── Generate test data ──────────────────────────────────

fn make_large_payload() -> LargePayload {
    let items: Vec<LargeItem> = (0..20).map(|i| {
        let mut metadata = HashMap::new();
        metadata.insert("color".into(), "red".into());
        metadata.insert("size".into(), "XL".into());
        metadata.insert("weight".into(), "1.5kg".into());
        LargeItem {
            id: i, name: format!("product-{i}"),
            description: "A high-quality item with excellent features and durable construction suitable for various uses.".into(),
            price: 19.99 + i as f64, quantity: 100 + i, category: "electronics".into(),
            tags: vec!["new".into(), "popular".into(), "discount".into()], metadata,
        }
    }).collect();
    LargePayload { items }
}

fn make_medium_payload() -> MediumPayload {
    MediumPayload {
        id: 42,
        name: "Alice Johnson".into(),
        email: "alice@example.com".into(),
        tags: vec!["premium".into(), "vip".into(), "early-adopter".into()],
        score: 98.5,
    }
}

// ── Server setup ────────────────────────────────────────

fn parse_args() -> (u16, bool) {
    let mut port = 19111u16;
    let mut dhat_enabled = false;
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                i += 1;
                if let Some(p) = args.get(i) {
                    port = p.parse().unwrap_or(19111);
                }
            }
            "--dhat" => dhat_enabled = true,
            _ => {}
        }
        i += 1;
    }
    (port, dhat_enabled)
}

#[tokio::main]
async fn main() {
    let (port, dhat_enabled) = parse_args();

    #[cfg(feature = "dhat-heap")]
    let _prof = if dhat_enabled {
        Some(dhat::Profiler::new_heap())
    } else {
        None
    };

    let data = Arc::new(RwLock::new(HashMap::from([
        ("actix".into(), 1.0),
        ("axum".into(), 2.0),
        ("gin".into(), 3.0),
        ("fnrpc".into(), 4.0),
    ])));

    let medium_json = serde_json::to_vec(&make_medium_payload()).unwrap();
    let large_json = serde_json::to_vec(&make_large_payload()).unwrap();
    eprintln!("medium payload: {} bytes", medium_json.len());
    eprintln!("large payload: {} bytes", large_json.len());

    let config = RpcWebConfig {
        router: RpcRouterBuilder::<AppCtx>::new()
            .route_fn(noop)
            .route_fn(echo)
            .route_fn(medium)
            .route_fn(large)
            // .raw(Lookup)
            .route_bytes(raw_noop)
            .route_fn(json_get)
            .route_bytes(plaintext)
            .build(),
        ctx_from_headers: Arc::new(move |_| data.clone()),
    };
    let config = Arc::new(config);

    println!("Starting fnrpc-web server on :{port}");
    run(config, &format!("0.0.0.0:{port}"))
        .await
        .expect("failed to bind");
}
