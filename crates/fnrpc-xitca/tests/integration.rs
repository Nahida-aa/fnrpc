use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouterBuilder;
use fnrpc_xitca::{FnrpcState, handle};
use futures::StreamExt;
use xitca_unsafe_collection::futures::NowOrPanic;
use xitca_web::body::{BodyExt, RequestBody};
use xitca_web::http::{Method, StatusCode};
use xitca_web::route::get;
use xitca_web::service::{Service, fn_service};
use xitca_web::{App, WebContext};
use std::pin::Pin;

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

// ── Test helpers ─────────────────────────────────────

fn build_get(path: &str) -> xitca_web::http::WebRequest<RequestBody> {
    let req: xitca_web::http::Request<xitca_web::http::RequestExt<RequestBody>> =
        xitca_web::http::Request::builder()
            .method(Method::GET)
            .uri(path)
            .body(xitca_web::http::RequestExt::default())
            .unwrap();
    req
}

// ── Tests ────────────────────────────────────────────

#[tokio::test]
async fn test_get_query() {
    let router = RpcRouterBuilder::<()>::new()
        .route_fn(Greet)
        .build();
    let state = FnrpcState::new(router, |_| ());
    let app = App::new()
        .with_state(state)
        .at("/{*path}", get(fn_service(handle::<()>)));
    let svc = app.finish().call(()).now_or_panic().unwrap();
    let req = build_get("/greet?input=%22world%22");
    let res = svc.call(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_not_found() {
    let router = RpcRouterBuilder::<()>::new()
        .route_fn(Greet)
        .build();
    let state = FnrpcState::new(router, |_| ());
    let app = App::new()
        .with_state(state)
        .at("/{*path}", get(fn_service(handle::<()>)));
    let svc = app.finish().call(()).now_or_panic().unwrap();
    let req = build_get("/nonexistent");
    let res = svc.call(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_subscribe_sse() {
    let router = RpcRouterBuilder::<()>::new()
        .subscribe(tick)
        .build();
    let state = FnrpcState::new(router, |_| ());
    let app = App::new()
        .with_state(state)
        .at("/{*path}", get(fn_service(handle::<()>)));
    let svc = app.finish().call(()).now_or_panic().unwrap();
    let req = build_get("/tick?input=1");
    let res = svc.call(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    // Read first SSE event from body
    let mut body = res.into_body();
    if let Some(Ok(chunk)) = body.data().await {
        let s = String::from_utf8_lossy(&chunk);
        assert!(s.starts_with("data: "), "expected SSE data frame, got: {s:?}");
    }
}
