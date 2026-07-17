//! Standalone xitca-web server for latency benchmarking.
//! Matches fnrpc_web_server endpoints for fair comparison.
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use serde::{Deserialize, Serialize};
use xitca_web::App;
use xitca_web::body::ResponseBody;
use xitca_web::http::{StatusCode, WebResponse};
use xitca_web::http::header::{CONTENT_TYPE, HeaderValue};
use xitca_web::handler::handler_service;
use xitca_web::handler::json::Json;
use xitca_web::route::{get, post};
use xitca_web::service::fn_service;
use xitca_web::WebContext;

type AppCtx = Arc<RwLock<HashMap<String, f64>>>;

// ── Noop ───────────────────────────────────────────────

async fn handler_noop_json(_ctx: WebContext<'_, AppCtx>) -> Result<WebResponse, xitca_web::error::Error> {
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(ResponseBody::bytes(xitca_web::bytes::Bytes::from_static(b"null")))
        .unwrap())
}

async fn handler_noop_raw(_ctx: WebContext<'_, AppCtx>) -> Result<WebResponse, xitca_web::error::Error> {
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::bytes(xitca_web::bytes::Bytes::from_static(b"ok")))
        .unwrap())
}

// ── Echo ───────────────────────────────────────────────

async fn handler_echo(Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, xitca_web::error::Error> {
    Ok(Json(body))
}

// ── Medium payload ─────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct MediumPayload {
    id: u32, name: String, email: String, tags: Vec<String>, score: f64,
}

async fn handler_medium(Json(body): Json<MediumPayload>) -> Result<Json<MediumPayload>, xitca_web::error::Error> {
    Ok(Json(body))
}

// ── Large payload ──────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct LargePayload { items: Vec<LargeItem> }

#[derive(Serialize, Deserialize)]
struct LargeItem {
    id: u32, name: String, description: String, price: f64,
    quantity: u32, category: String, tags: Vec<String>, metadata: HashMap<String, String>,
}

async fn handler_large(Json(body): Json<LargePayload>) -> Result<Json<LargePayload>, xitca_web::error::Error> {
    Ok(Json(body))
}

// ── Lookup (matches fnrpc-web: Arc<RwLock<HashMap>>) ───

async fn handler_lookup(
    ctx: WebContext<'_, AppCtx>,
) -> Result<WebResponse, xitca_web::error::Error> {
    let uri: xitca_web::handler::uri::UriRef<'_> = ctx.extract().await.unwrap();
    let query_str = uri.query().unwrap_or("");
    let key = query_str.split('&').find_map(|pair| {
        let mut parts = pair.splitn(2, '=');
        if parts.next() == Some("key") { parts.next() } else { None }
    }).unwrap_or("");

    let app_data = ctx.state();
    let n = app_data.read().unwrap().get(key).copied().unwrap_or(0.0);
    let output = serde_json::json!({"entity": key, "n": n});
    let bytes = serde_json::to_vec(&output).unwrap_or_default();
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(ResponseBody::bytes(bytes))
        .unwrap())
}

// ── Server ─────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut port = 0u16;
    let mut dhat_enabled = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => { i += 1; if let Some(p) = args.get(i) { port = p.parse().unwrap_or(0); } }
            "--dhat" => dhat_enabled = true,
            _ => { port = args[i].parse().unwrap_or(0); }
        }
        i += 1;
    }

    #[cfg(feature = "dhat-heap")]
    let _prof = if dhat_enabled { Some(dhat::Profiler::new_heap()) } else { None };

    let data = Arc::new(RwLock::new(HashMap::from([
        ("actix".into(), 1.0), ("axum".into(), 2.0),
        ("gin".into(), 3.0), ("fnrpc".into(), 4.0),
    ])));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all().build().unwrap();

    rt.block_on(async {
        eprintln!("Starting xitca-web server on :{port}");

        App::new()
            .with_state(data)
            .at("/noop-json", get(fn_service(handler_noop_json)))
            .at("/noop-raw", get(fn_service(handler_noop_raw)))
            .at("/echo", post(handler_service(handler_echo)))
            .at("/medium", post(handler_service(handler_medium)))
            .at("/large", post(handler_service(handler_large)))
            .at("/in", get(fn_service(handler_lookup)))
            .serve()
            .bind(format!("0.0.0.0:{port}"))
            .unwrap()
            .run()
            .await
            .unwrap();
    });
}
