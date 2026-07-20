//! Axum integration for fnrpc.
//!
//! Provides a [`handle`] async fn that dispatches an HTTP request through
//! a [`fnrpc::router::RpcRouter`]. Both regular query/mutate and subscribe
//! (SSE) procedures are supported. Subscribe handlers return a streaming
//! `text/event-stream` response.
//!
//! Use it to mount fnrpc endpoints into an Axum application:
//!
//! ```ignore
//! use std::sync::Arc;
//! use axum::{Router, routing::get};
//! use fnrpc::router::RpcRouterBuilder;
//! use fnrpc_axum::{FnrpcState, handle};
//!
//! let router = RpcRouterBuilder::<MyCtx>::new()
//!     .route_fn(my_handler)
//!     .subscribe(my_sub)
//!     .build();
//!
//! let state = FnrpcState::new(router, |_headers| MyCtx { ... });
//!
//! let app = Router::new()
//!     .route("/{*path}", get(handle::<MyCtx>).post(handle::<MyCtx>))
//!     .with_state(Arc::new(state));
//! ```
//!
//! Subscribe handlers are automatically detected by path and served as SSE
//! streams. No separate route configuration is needed.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use fnrpc::error::RpcErr;
use fnrpc::router::RpcRouter;
use futures::StreamExt;
use http_body_util::BodyExt;

/// Application state holding a router and a context factory.
pub struct FnrpcState<Ctx: Send + Sync + 'static> {
    router: Arc<RpcRouter<Ctx>>,
    ctx_factory: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
}

impl<Ctx: Send + Sync + 'static> FnrpcState<Ctx> {
    /// Create a new state with a router and a context factory.
    pub fn new(
        router: RpcRouter<Ctx>,
        ctx_factory: impl Fn(&HeaderMap) -> Ctx + Send + Sync + 'static,
    ) -> Self {
        Self {
            router: Arc::new(router),
            ctx_factory: Arc::new(ctx_factory),
        }
    }
}

/// Dispatch an RPC call through the router stored in Axum application state.
///
/// Regular query/mutate handlers return a buffered JSON response.
/// Subscribe handlers return a streaming SSE (`text/event-stream`) response.
pub async fn handle<Ctx>(
    State(state): State<Arc<FnrpcState<Ctx>>>,
    headers: HeaderMap,
    method: Method,
    Path(path): Path<String>,
    RawQuery(raw_query): RawQuery,
    body: Body,
) -> Response
where
    Ctx: Send + Sync + 'static,
{
    let ctx = (state.ctx_factory)(&headers);

    // Check if this path is a subscribe handler
    if state.router.has_subscribe(&path) {
        let raw_input: Vec<u8> = if method == Method::GET {
            // Extract and URL-decode the "input" query parameter
            raw_query
                .unwrap_or_default()
                .split('&')
                .find_map(|pair| {
                    let mut parts = pair.splitn(2, '=');
                    let key = parts.next()?;
                    let val = parts.next()?;
                    if key == "input" { Some(urlencoding_decode(val)) } else { None }
                })
                .unwrap_or_default()
                .into_bytes()
        } else {
            let mut buf = Vec::new();
            let mut body = body;
            while let Some(frame) = body.frame().await {
                match frame {
                    Ok(frame) => if let Ok(data) = frame.into_data() {
                        buf.extend_from_slice(&data)
                    },
                    Err(_) => {
                        let err = RpcErr::bad_request("body read error");
                        return (StatusCode::BAD_REQUEST, axum::Json(err)).into_response();
                    }
                }
            }
            buf
        };

        // Unpack meta envelope (BigInt → number, etc.) and re-serialize
        let val: serde_json::Value = serde_json::from_slice(&raw_input).unwrap_or(serde_json::Value::Null);
        let unpacked = fnrpc::serializer::unpack_meta(val);
        let input = serde_json::to_vec(&unpacked).unwrap_or_default();

        match state.router.dispatch_subscribe(&ctx, &path, &input) {
            Ok(stream) => {
                let sse_stream = stream.map(|item| {
                    let data = match item {
                        Ok(bytes) => format!("data: {}\n\n", String::from_utf8_lossy(&bytes)),
                        Err(e) => format!("data: {}\n\n", serde_json::to_string(&e).unwrap_or_default()),
                    };
                    Ok::<_, std::convert::Infallible>(data)
                });
                let body = axum::body::Body::from_stream(sse_stream);
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .header("cache-control", "no-cache")
                    .body(body)
                    .unwrap()
            }
            Err(e) => {
                let status = match e.code.as_str() {
                    "NOT_FOUND" => StatusCode::NOT_FOUND,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, axum::Json(e)).into_response()
            }
        }
    } else {
        let input: Vec<u8> = if method == Method::GET {
            raw_query.unwrap_or_default().into_bytes()
        } else {
            let mut buf = Vec::new();
            let mut body = body;
            while let Some(frame) = body.frame().await {
                match frame {
                    Ok(frame) => if let Ok(data) = frame.into_data() {
                        buf.extend_from_slice(&data)
                    },
                    Err(_) => {
                        let err = RpcErr::bad_request("body read error");
                        return (StatusCode::BAD_REQUEST, axum::Json(err)).into_response();
                    }
                }
            }
            buf
        };

        let is_get = method == Method::GET;
        let result = state.router.dispatch(&ctx, &path, &input, is_get).await;

        match result {
            Ok((bytes, is_json)) => {
                let mut builder = Response::builder().status(StatusCode::OK);
                if is_json {
                    builder = builder.header("content-type", "application/json");
                }
                builder.body(axum::body::Body::from(bytes.into_owned())).unwrap()
            }
            Err(e) => {
                let status = match e.code.as_str() {
                    "BAD_REQUEST" => StatusCode::BAD_REQUEST,
                    "NOT_FOUND" => StatusCode::NOT_FOUND,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, axum::Json(e)).into_response()
            }
        }
    }
}

/// Minimal percent-decoding for query values.
fn urlencoding_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        match b {
            b'+' => result.push(' '),
            b'%' => {
                let hi = bytes.next().and_then(|c| hex_val(c));
                let lo = bytes.next().and_then(|c| hex_val(c));
                match (hi, lo) {
                    (Some(h), Some(l)) => result.push((h << 4 | l) as char),
                    _ => result.push('%'),
                }
            }
            _ => result.push(b as char),
        }
    }
    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
