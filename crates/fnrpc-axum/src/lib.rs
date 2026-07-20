//! Axum integration for fnrpc.
//!
//! Provides a [`handle`] async fn that dispatches an HTTP request through
//! a [`fnrpc::router::RpcRouter`]. Use it to mount fnrpc endpoints into
//! an Axum application:
//!
//! ```ignore
//! use std::sync::Arc;
//! use axum::{Router, routing::get};
//! use fnrpc::router::RpcRouterBuilder;
//! use fnrpc_axum::{FnrpcState, handle};
//!
//! let router = RpcRouterBuilder::<MyCtx>::new()
//!     .route_fn(my_handler)
//!     .build();
//!
//! let state = FnrpcState::new(router, |_headers| MyCtx { ... });
//!
//! let app = Router::new()
//!     .route("/{*path}", get(handle::<MyCtx>).post(handle::<MyCtx>))
//!     .with_state(Arc::new(state));
//! ```

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use fnrpc::error::RpcErr;
use fnrpc::middleware::RpcService;
use fnrpc::router::RpcRouter;
use http_body_util::BodyExt;

/// Application state holding a router and a context factory.
pub struct FnrpcState<Ctx: Send + Sync + 'static, S: RpcService<Ctx> + Send + Sync + 'static = fnrpc::router::InnerService<Ctx>> {
    router: Arc<RpcRouter<Ctx, S>>,
    ctx_factory: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
}

impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx> + Send + Sync + 'static> FnrpcState<Ctx, S> {
    /// Create a new state with a router and a context factory.
    pub fn new(
        router: RpcRouter<Ctx, S>,
        ctx_factory: impl Fn(&HeaderMap) -> Ctx + Send + Sync + 'static,
    ) -> Self {
        Self {
            router: Arc::new(router),
            ctx_factory: Arc::new(ctx_factory),
        }
    }
}

/// Dispatch an RPC call through the router stored in Axum application state.
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

    let input: Vec<u8> = if method == Method::GET {
        // GET: use query string as input bytes (handler parses `input` param)
        raw_query.unwrap_or_default().into_bytes()
    } else {
        // POST: read body
        use axum::body::HttpBody;
        let mut buf = Vec::new();
        let mut body = body;
        while let Some(chunk) = body.frame().await {
            match chunk {
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
