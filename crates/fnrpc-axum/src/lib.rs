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

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{OriginalUri, Path, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json};
use fnrpc::serializer::unpack_meta;
use futures::StreamExt;
use serde_json::Value;

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

fn extract_input(query_str: &str) -> Value {
    for (key, val) in url::form_urlencoded::parse(query_str.as_bytes()) {
        if key == "input" {
            return serde_json::from_str(&val).unwrap_or(Value::Null);
        }
    }
    Value::Null
}

/// Axum handler for fnrpc requests.
///
/// Uses [`get_handler`](RpcRouter::get_handler) + direct handler call —
/// bypasses the middleware stack and saves one `Box::pin` allocation
/// vs [`dispatch_send`](RpcRouter::dispatch_send).
///
/// Mount this at a wildcard route (e.g. `"/fnrpc/{*path}"`) and register
/// both GET and POST methods.
///
/// Subscribe procedures send SSE events with incrementing `id` fields
/// for client-side reconnection support.
pub async fn handle<Ctx>(
    method: Method,
    State(state): State<Arc<FnrpcState<Ctx>>>,
    Path(path): Path<String>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    body: Option<axum::extract::Json<Value>>,
) -> axum::response::Response
where
    Ctx: Send + Sync + 'static,
{
    let ctx = (state.ctx_from_headers)(headers);

    // Extract input — no HashMap allocation for query params
    let input_raw: Value = match method {
        Method::POST => body.map(|j| j.0).unwrap_or(Value::Null),
        _ => extract_input(uri.query().unwrap_or("")),
    };
    let input = unpack_meta(&input_raw);

    // Fast path: direct handler call
    if let Some(handler) = state.router.get_handler(&path) {
        match handler.call(&ctx, input).await {
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
    } else if let Some(handler) = state.router.get_sub_handler(&path) {
        let stream = handler.call(&ctx, input);
        let mut event_id = 0u64;
        let sse = stream.map(move |item| {
            event_id += 1;
            let event = match item {
                Ok(val) => Event::default()
                    .id(event_id.to_string())
                    .json_data(val)
                    .unwrap(),
                Err(e) => Event::default()
                    .id(event_id.to_string())
                    .data(format!("__error:{}", serde_json::to_string(&e).unwrap())),
            };
            Ok::<_, Infallible>(event)
        });

        Sse::new(sse)
            .keep_alive(KeepAlive::default())
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown path: {path}") })),
        )
            .into_response()
    }
}
