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

