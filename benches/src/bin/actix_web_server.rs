//! Standalone actix-web server for latency benchmarking.
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use actix_web::{App, HttpResponse, HttpServer, web, HttpRequest};

type AppCtx = Arc<RwLock<HashMap<String, f64>>>;

async fn handler_noop_json() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::Value::Null)
}

async fn handler_plaintext() -> HttpResponse {
    HttpResponse::Ok().content_type("text/plain").body("Hello, World!")
}

async fn handler_json_te() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({"message":"Hello, World!"}))
}

async fn handler_lookup(req: HttpRequest, data: web::Data<AppCtx>) -> HttpResponse {
    let key = req.query_string()
        .split('&')
        .find_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            if parts.next() == Some("key") { parts.next() } else { None }
        })
        .unwrap_or("");
    let n = data.read().unwrap().get(key).copied().unwrap_or(0.0);
    HttpResponse::Ok().json(serde_json::json!({"entity": key, "n": n}))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let port: u16 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(8080);

    let data: AppCtx = Arc::new(RwLock::new(HashMap::from([
        ("actix".to_string(), 1.0), ("axum".to_string(), 2.0),
        ("gin".to_string(), 3.0), ("fnrpc".to_string(), 4.0),
    ])));

    eprintln!("Starting actix-web server on :{port}");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(data.clone()))
            .route("/noop-json", web::get().to(handler_noop_json))
            .route("/plaintext", web::get().to(handler_plaintext))
            .route("/json", web::get().to(handler_json_te))
            .route("/in", web::get().to(handler_lookup))
    })
    .bind(format!("0.0.0.0:{port}"))?
    .workers(4)
    .run()
    .await
}
