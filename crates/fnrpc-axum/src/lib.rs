use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json};
use fnrpc::serializer::unpack_meta;
use futures::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// Shared state for fnrpc axum integration.
pub struct FnrpcState<Ctx> {
    pub router: Arc<fnrpc::router::RpcRouter<Ctx>>,
    pub ctx_from_headers: Arc<dyn Fn(HeaderMap) -> Ctx + Send + Sync>,
}

/// Axum handler for fnrpc requests.
///
/// Handles both GET (query) and POST (query/mutate) requests,
/// as well as SSE streaming for subscriptions.
pub async fn handle<Ctx>(
    method: Method,
    State(state): State<Arc<FnrpcState<Ctx>>>,
    Path(path): Path<String>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    body: Option<axum::extract::Json<Value>>,
) -> axum::response::Response
where
    Ctx: Send + Sync + 'static,
{
    let kind = state.router.get_procedure_kind(&path);

    match kind {
        Some("subscribe") => {
            let raw = params
                .get("input")
                .cloned()
                .unwrap_or_else(|| "null".into());
            let input_raw: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
            let input = unpack_meta(&input_raw);

            match state.router.get_sub_handler(&path) {
                Some(handler) => {
                    let ctx = (state.ctx_from_headers)(headers);

                    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);

                    tokio::spawn(async move {
                        let mut stream = handler.call(&ctx, input);
                        while let Some(item) = stream.next().await {
                            let event = match item {
                                Ok(val) => Event::default().json_data(val).unwrap(),
                                Err(e) => Event::default().data(format!(
                                    "__error:{}",
                                    serde_json::to_string(&e).unwrap()
                                )),
                            };
                            if tx.send(Ok(event)).await.is_err() {
                                break;
                            }
                        }
                    });

                    Sse::new(ReceiverStream::new(rx))
                        .keep_alive(KeepAlive::default())
                        .into_response()
                }
                None => (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "code": "NOT_FOUND",
                        "message": format!("unknown path: {path}")
                    })),
                )
                    .into_response(),
            }
        }
        Some(_) => {
            let ctx = (state.ctx_from_headers)(headers);

            let input_raw = match method {
                Method::GET => {
                    let raw = params
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| "null".into());
                    serde_json::from_str(&raw).unwrap_or(Value::Null)
                }
                Method::POST => body.map(|j| j.0).unwrap_or(Value::Null),
                _ => Value::Null,
            };
            let input = unpack_meta(&input_raw);

            match state.router.dispatch(&ctx, &path, input).await {
                Ok(val) => Json(val).into_response(),
                Err(e) => {
                    let status = match e.code.as_str() {
                        "BAD_REQUEST" => StatusCode::BAD_REQUEST,
                        "NOT_FOUND" => StatusCode::NOT_FOUND,
                        _ => StatusCode::INTERNAL_SERVER_ERROR,
                    };
                    (status, Json(e)).into_response()
                }
            }
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown path: {path}") })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::Router;
    use fnrpc::error::RpcErr;
    use fnrpc::handler::RpcFn;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_router<Ctx, F>(router: fnrpc::router::RpcRouter<Ctx>, ctx_from_headers: F) -> Router
    where
        Ctx: Send + Sync + 'static,
        F: Fn(HeaderMap) -> Ctx + Send + Sync + 'static,
    {
        Router::new()
            .route(
                "/{*path}",
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

        #[async_trait::async_trait]
        impl RpcFn<()> for Greet {
            type Input = String;
            type Output = String;
            const NAME: &'static str = "greet";

            async fn exec(_ctx: &(), input: String) -> Result<String, RpcErr> {
                Ok(format!("Hello {input}!"))
            }
        }

        let router = fnrpc::router::RpcRouter::<()>::new().query(Greet);
        let app = test_router(router, |_headers| ());

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/greet?input=%22world%22")
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

        #[async_trait::async_trait]
        impl RpcFn<()> for Add {
            type Input = (i32, i32);
            type Output = i32;
            const NAME: &'static str = "add";

            async fn exec(_ctx: &(), input: (i32, i32)) -> Result<i32, RpcErr> {
                Ok(input.0 + input.1)
            }
        }

        let router = fnrpc::router::RpcRouter::<()>::new().query(Add);
        let app = test_router(router, |_headers| ());

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/add")
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
        let router = fnrpc::router::RpcRouter::<()>::new();
        let app = test_router(router, |_headers| ());

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/nonexistent")
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
            ) -> Pin<Box<dyn futures::Stream<Item = Result<u32, RpcErr>> + Send + '_>>
            {
                Box::pin(futures::stream::iter((1..=input).map(|n| Ok(n))))
            }
        }

        let router = fnrpc::router::RpcRouter::<()>::new().subscribe(Tick);
        let app = test_router(router, |_headers| ());

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/tick?input=3")
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

        #[async_trait::async_trait]
        impl RpcFn<MyCtx> for CtxGreet {
            type Input = String;
            type Output = String;
            const NAME: &'static str = "ctx_greet";

            async fn exec(ctx: &MyCtx, input: String) -> Result<String, RpcErr> {
                Ok(format!("{}{}", ctx.prefix, input))
            }
        }

        let router = fnrpc::router::RpcRouter::<MyCtx>::new().query(CtxGreet);
        let app = test_router(router, |_headers| MyCtx {
            prefix: "yo ".to_string(),
        });

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/ctx_greet?input=%22world%22")
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
}
