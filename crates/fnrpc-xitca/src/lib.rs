//! xitca-web integration for fnrpc.
//!
//! Provides a [`handle`] function that dispatches an HTTP request through
//! a [`fnrpc::router::RpcRouter`]. Use it to mount fnrpc endpoints into
//! a xitca-web application:
//!
//! ```ignore
//! use fnrpc::router::RpcRouterBuilder;
//! use fnrpc_xitca::{FnrpcState, handle};
//! use xitca_web::{App, route::get, service::fn_service};
//!
//! let router = RpcRouterBuilder::<MyCtx>::new()
//!     .route_fn(my_handler)
//!     .build();
//!
//! let state = FnrpcState::new(router, |_headers| MyCtx { ... });
//!
//! App::new()
//!     .with_state(state)
//!     .at("/{*path}", get(fn_service(handle)).post(fn_service(handle)))
//!     .serve()
//!     .bind("0.0.0.0:3000")?
//!     .run()
//!     .wait()
//!     .unwrap();
//! ```

use std::borrow::Cow;
use std::sync::Arc;

use fnrpc::middleware::RpcService;
use fnrpc::router::RpcRouter;
use futures::StreamExt;
use xitca_web::body::{BodyExt, Frame, RequestBody, ResponseBody, StreamBody};
use xitca_web::bytes::Bytes;
use xitca_web::http::header::{HeaderValue, CONTENT_TYPE};
use xitca_web::http::HeaderMap;
use xitca_web::http::{Method, StatusCode, WebResponse};
use xitca_web::WebContext;

/// Application state holding a router and a context factory.
///
/// Pass this to `App::with_state(state)` when setting up the xitca-web application.
pub struct FnrpcState<Ctx: Send + Sync + 'static> {
    router: Arc<RpcRouter<Ctx>>,
    ctx_factory: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
}

impl<Ctx: Send + Sync + 'static> Clone for FnrpcState<Ctx> {
    fn clone(&self) -> Self {
        Self {
            router: Arc::clone(&self.router),
            ctx_factory: Arc::clone(&self.ctx_factory),
        }
    }
}

impl<Ctx: Send + Sync + 'static> FnrpcState<Ctx> {
    /// Create a new state with a router and a context factory.
    ///
    /// The context factory receives the request headers and returns the
    /// application context (e.g., database connection, auth info).
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

/// Dispatch an RPC call through the router stored in xitca-web's application state.
///
/// The application state must be a [`FnrpcState<Ctx>`](FnrpcState). Pass it via
/// `App::with_state(state)`.
///
/// Regular query/mutate handlers return a buffered JSON response.
/// Subscribe handlers return a streaming SSE (`text/event-stream`) response.
pub async fn handle<Ctx>(
    mut ctx: WebContext<'_, FnrpcState<Ctx>>,
) -> Result<WebResponse, xitca_web::error::Error>
where
    Ctx: Send + Sync + 'static,
{
    let (path, method, input) = {
        let req = ctx.req();
        let method = req.method().clone();
        let path = req.uri().path().strip_prefix('/').unwrap_or("").to_string();
        let input: Cow<'_, [u8]> = if method == Method::GET {
            req.uri().query().unwrap_or("").as_bytes().into()
        } else {
            let body = ctx.body_get_mut();
            let mut buf = Vec::new();
            while let Some(chunk) = body.data().await {
                let chunk = chunk.map_err(|_| {
                    xitca_web::error::Error::from(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "body read error",
                    ))
                })?;
                buf.extend_from_slice(chunk.as_ref());
            }
            buf.into()
        };
        (path, method, input)
    };

    let state: &FnrpcState<Ctx> = ctx.state();
    let app_ctx = (state.ctx_factory)(ctx.req().headers());

    // Check if this path is a subscribe handler
    if state.router.has_subscribe(&path) {
        // For subscribe, re-parse input from query string on GET to extract
        // the URL-decoded "input" parameter, matching dispatch's behavior.
        let raw_input = if method == Method::GET {
            std::str::from_utf8(&input).unwrap_or("").split('&').find_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                let key = parts.next()?;
                let val = parts.next()?;
                if key == "input" { Some(percent_decode(val)) } else { None }
            }).unwrap_or_default().into_bytes()
        } else {
            input.to_vec()
        };
        // Unpack meta envelope (BigInt → number, etc.) and re-serialize
        let val: serde_json::Value = serde_json::from_slice(&raw_input).unwrap_or(serde_json::Value::Null);
        let unpacked = fnrpc::serializer::unpack_meta(val);
        let sub_input = serde_json::to_vec(&unpacked).unwrap_or_default();
        match state.router.dispatch_subscribe(&app_ctx, &path, &sub_input) {
            Ok(stream) => {
                let sse_stream = stream.map(|item| {
                    let data = match item {
                        Ok(bytes) => format!("data: {}\n\n", String::from_utf8_lossy(&bytes)),
                        Err(e) => format!("data: {}\n\n", serde_json::to_string(&e).unwrap_or_default()),
                    };
                    Ok::<_, std::convert::Infallible>(Frame::Data(Bytes::from(data)))
                });
                let body = ResponseBody::body(StreamBody::new(sse_stream)).into_boxed();
                Ok(xitca_web::http::Response::builder()
                    .status(StatusCode::OK)
                    .header(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))
                    .header("cache-control", HeaderValue::from_static("no-cache"))
                    .body(body)
                    .unwrap())
            }
            Err(e) => {
                let status = match e.code.as_str() {
                    "NOT_FOUND" => StatusCode::NOT_FOUND,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                let body = serde_json::to_vec(&e).unwrap_or_default();
                Ok(xitca_web::http::Response::builder()
                    .status(status)
                    .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                    .body(ResponseBody::bytes(Bytes::from(body)))
                    .unwrap())
            }
        }
    } else {
        let is_get = method == Method::GET;
        let result = state.router.dispatch(&app_ctx, &path, &input, is_get).await;

        match result {
            Ok((bytes, is_json)) => {
                let mut builder = xitca_web::http::Response::builder().status(StatusCode::OK);
                if is_json {
                    builder = builder.header(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                }
                let resp_body = match &bytes {
                    Cow::Borrowed(b"null") => ResponseBody::bytes(Bytes::from_static(b"null")),
                    Cow::Borrowed(slice) => ResponseBody::bytes(Bytes::from_static(*slice)),
                    Cow::Owned(vec) => ResponseBody::bytes(Bytes::copy_from_slice(vec)),
                };
                Ok(builder.body(resp_body).unwrap())
            }
            Err(e) => {
                let status = match e.code.as_str() {
                    "BAD_REQUEST" => StatusCode::BAD_REQUEST,
                    "NOT_FOUND" => StatusCode::NOT_FOUND,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                let body = serde_json::to_vec(&e).unwrap_or_default();
                Ok(xitca_web::http::Response::builder()
                    .status(status)
                    .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                    .body(ResponseBody::bytes(Bytes::from(body)))
                    .unwrap())
            }
        }
    }
}

/// Minimal percent-decoding for query values.
fn percent_decode(s: &str) -> String {
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
