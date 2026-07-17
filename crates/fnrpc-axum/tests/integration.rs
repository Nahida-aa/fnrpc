use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode};
use axum::Router;
use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc_axum::{handle, FnrpcState};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

fn test_router<Ctx, F>(router: fnrpc::router::RpcRouter<Ctx>, ctx_from_headers: F) -> Router
where
    Ctx: Send + Sync + 'static,
    F: Fn(HeaderMap) -> Ctx + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/fnrpc/{*path}",
            axum::routing::get(handle::<Ctx>).post(handle::<Ctx>),
        )
        .with_state(Arc::new(FnrpcState {
            router: Arc::new(router),
            ctx_from_headers: Arc::new(ctx_from_headers),
        }))
}

#[tokio::test]
async fn test_query_get() {
    struct Greet;

    impl RpcFn<()> for Greet {
        type Input = String;
        type Output = String;
        const NAME: &'static str = "greet";

        async fn exec(_ctx: &(), input: String) -> Result<String, RpcErr> {
            Ok(format!("Hello {input}!"))
        }
    }

    let router = fnrpc::router::RpcRouterBuilder::<()>::new().query(Greet).build();
    let app = test_router(router, |_headers| ());

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/fnrpc/greet?input=%22world%22")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let val: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(val, "Hello world!");
}

#[tokio::test]
async fn test_query_post() {
    struct Add;

    impl RpcFn<()> for Add {
        type Input = (i32, i32);
        type Output = i32;
        const NAME: &'static str = "add";

        async fn exec(_ctx: &(), input: (i32, i32)) -> Result<i32, RpcErr> {
            Ok(input.0 + input.1)
        }
    }

    let router = fnrpc::router::RpcRouterBuilder::<()>::new().query(Add).build();
    let app = test_router(router, |_headers| ());

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                    .uri("/fnrpc/add")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&[3i32, 5]).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let val: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(val, 8);
}

#[tokio::test]
async fn test_not_found() {
    let router = fnrpc::router::RpcRouterBuilder::<()>::new().build();
    let app = test_router(router, |_headers| ());

    let response = app
        .oneshot(
            axum::http::Request::builder()
                    .uri("/fnrpc/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_subscribe() {
    use fnrpc::handler::RpcSubscribe;
    use std::pin::Pin;

    struct Tick;

    impl RpcSubscribe<()> for Tick {
        type Input = u32;
        type Output = u32;
        const NAME: &'static str = "tick";

        fn exec(
            _ctx: &(),
            input: u32,
        ) -> Pin<Box<dyn futures::Stream<Item = Result<u32, RpcErr>> + Send + 'static>>
        {
            Box::pin(futures::stream::iter((1..=input).map(|n| Ok(n))))
        }
    }

    let router = fnrpc::router::RpcRouterBuilder::<()>::new().subscribe(Tick).build();
    let app = test_router(router, |_headers| ());

    let response = app
        .oneshot(
            axum::http::Request::builder()
                    .uri("/fnrpc/tick?input=3")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_with_context() {
    #[derive(Clone)]
    struct MyCtx {
        prefix: String,
    }

    struct CtxGreet;

    impl RpcFn<MyCtx> for CtxGreet {
        type Input = String;
        type Output = String;
        const NAME: &'static str = "ctx_greet";

        async fn exec(ctx: &MyCtx, input: String) -> Result<String, RpcErr> {
            Ok(format!("{}{}", ctx.prefix, input))
        }
    }

    let router = fnrpc::router::RpcRouterBuilder::<MyCtx>::new().query(CtxGreet).build();
    let app = test_router(router, |_headers| MyCtx {
        prefix: "yo ".to_string(),
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/fnrpc/ctx_greet?input=%22world%22")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let val: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(val, "yo world");
}
