use std::sync::Arc;

use axum::Router;
use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouterBuilder;
use fnrpc_axum::{FnrpcState, handle};
use futures::StreamExt;
use http_body_util::BodyExt;
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

#[fnrpc::rpc_subscribe]
fn tick(interval_ms: u64) -> impl futures::Stream<Item = u64> {
    futures::stream::unfold(0u64, move |count| async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
        Some((count, count + 1))
    })
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

#[tokio::test]
async fn test_subscribe_sse() {
    let router = RpcRouterBuilder::<()>::new().subscribe(tick).build();
    let state = Arc::new(FnrpcState::new(router, |_| ()));
    let app = Router::new()
        .route("/{*path}", axum::routing::get(handle::<()>))
        .with_state(state);

    let res = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/tick?input=1")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    assert_eq!(
        res.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    // Read first SSE event from body
    let mut body = res.into_body();
    if let Some(Ok(frame)) = body.frame().await {
        if let Ok(data) = frame.into_data() {
            let s = String::from_utf8_lossy(&data);
            assert!(s.starts_with("data: "), "expected SSE data frame, got: {s:?}");
        }
    }
}
