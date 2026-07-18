//! Standalone axum server for latency benchmarking.
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde_json::Value;

type AppCtx = Arc<RwLock<HashMap<String, f64>>>;

async fn handler_noop_json() -> Json<Value> {
    Json(Value::Null)
}

async fn handler_json_te() -> Json<Value> {
    Json(serde_json::json!({"message": "Hello, World!"}))
}

async fn handler_plaintext() -> (StatusCode, &'static str) {
    (StatusCode::OK, "Hello, World!")
}

async fn handler_lookup(
    State(data): State<AppCtx>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<Value> {
    let key = params.get("key").map(|s| s.as_str()).unwrap_or("");
    let n = data.read().unwrap().get(key).copied().unwrap_or(0.0);
    Json(serde_json::json!({"entity": key, "n": n}))
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(8080);

    let data: AppCtx = Arc::new(RwLock::new(HashMap::from([
        ("actix".to_string(), 1.0),
        ("axum".to_string(), 2.0),
        ("gin".to_string(), 3.0),
        ("fnrpc".to_string(), 4.0),
    ])));

    eprintln!("Starting axum server on :{port}");

    let app = Router::new()
        .route("/noop-json", get(handler_noop_json))
        .route("/plaintext", get(handler_plaintext))
        .route("/json", get(handler_json_te))
        .route("/in", get(handler_lookup))
        .with_state(data);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap();

    axum::serve(listener, app)
        .await
        .unwrap();
}
