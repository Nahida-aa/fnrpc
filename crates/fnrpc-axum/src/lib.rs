//! Axum integration for fnrpc.
//!
//! Provides [`FnrpcState`] (Axum shared state) and [`handle`] (route handler)
//! that dispatches fnrpc queries, mutations, and subscriptions over HTTP.
//!
//! # Route setup
//!
//! ```ignore
//! Router::new()
//!     .route("/fnrpc/{*path}", axum::routing::get(handle::<Ctx>).post(handle::<Ctx>))
//!     .with_state(Arc::new(FnrpcState { router, ctx_from_headers }))
//! ```

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

/// Shared state for the fnrpc Axum handler.
///
/// Holds the router and a way to construct the context from incoming request headers.
pub struct FnrpcState<Ctx> {
    pub router: Arc<fnrpc::router::RpcRouter<Ctx>>,
    /// Function that builds `Ctx` from the incoming HTTP headers.
    ///
    /// Useful for extracting auth tokens, user IDs, etc.
    pub ctx_from_headers: Arc<dyn Fn(HeaderMap) -> Ctx + Send + Sync>,
}

/// Axum handler for fnrpc requests.
///
/// Mount this at a wildcard route (e.g. `"/fnrpc/{*path}"`) and register
/// both GET and POST methods. The handler inspects the procedure metadata
/// to determine dispatch:
///
/// - Subscribe: returns an SSE stream.
/// - Query (GET): reads input from query params.
/// - Mutate (POST): reads input from request body.
///
/// Subscribe procedures send SSE events with incrementing `id` fields
/// for client-side reconnection support.
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
            let input_raw: Value = match method {
                Method::POST => body.map(|j| j.0).unwrap_or(Value::Null),
                _ => {
                    let raw = params
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| "null".into());
                    serde_json::from_str(&raw).unwrap_or(Value::Null)
                }
            };
            let input = unpack_meta(&input_raw);

            match state.router.get_sub_handler(&path) {
                Some(handler) => {
                    let ctx = (state.ctx_from_headers)(headers);

                    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);

                    tokio::spawn(async move {
                        let mut stream = handler.call(&ctx, input);
                        let mut event_id = 0u64;
                        while let Some(item) = stream.next().await {
                            event_id += 1;
                            let event = match item {
                                Ok(val) => Event::default()
                                    .id(event_id.to_string())
                                    .json_data(val)
                                    .unwrap(),
                                Err(e) => Event::default()
                                    .id(event_id.to_string())
                                    .data(format!(
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

