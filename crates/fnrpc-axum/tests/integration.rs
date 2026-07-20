use std::sync::Arc;

use axum::Router;
use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouterBuilder;
use fnrpc_axum::{FnrpcState, handle};
use std::pin::Pin;
use tower::ServiceExt;

// ── Handlers ──────────────────────────────────────────

struct Greet;
impl RpcFn<()> for Greet {
    type Input = String;
    type Output = String;
    const KEY: &'static str = "greet";
    fn exec(
        _ctx: &(),
        input: String,
    ) -> Pin<Box<dyn futures::Future<Output = Result<String, RpcErr>> + Send + '_>> {
        Box::pin(async move { Ok(format!("Hello {input}!")) })
    }
}

// ── Tests ────────────────────────────────────────────

#[tokio::test]
async fn test_get_query() {
    let router = RpcRouterBuilder::<()>::new().route_fn(Greet).build();
    let state = Arc::new(FnrpcState::new(router, |_| ()));
    let app = Router::new()
        .route("/{*path}", axum::routing::get(handle::<()>))
        .with_state(state);

    let res = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/greet?input=%22world%22")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn test_not_found() {
    let router = RpcRouterBuilder::<()>::new().route_fn(Greet).build();
    let state = Arc::new(FnrpcState::new(router, |_| ()));
    let app = Router::new()
        .route("/{*path}", axum::routing::get(handle::<()>))
        .with_state(state);

    let res = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/nonexistent")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_post() {
    let router = RpcRouterBuilder::<()>::new().route_fn(Greet).build();
    let state = Arc::new(FnrpcState::new(router, |_| ()));
    let app = Router::new()
        .route("/{*path}", axum::routing::post(handle::<()>))
        .with_state(state);

    let res = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/greet")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(serde_json::to_vec(&"world").unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
}
