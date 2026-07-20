//! Standalone fnrpc HTTP server on xitca-http + xitca-server.
//!
//! A thin HTTP transport layer. Supports both single-router zero-overhead mode
//! and multi-router mode with static file serving.
//!
//! # Single router (zero `Box::pin`)
//!
//! ```ignore
//! use fnrpc::router::RpcRouterBuilder;
//! use fnrpc_web::App;
//!
//! let router = RpcRouterBuilder::<()>::new()
//!     .route_fn(health_check)
//!     .build();
//!
//! App::new(router, |_| ())
//!     .run("0.0.0.0:3000")
//!     .await
//!     .unwrap();
//! ```
//!
//! # Multiple routes with static files
//!
//! ```ignore
//! use fnrpc::router::RpcRouterBuilder;
//! use fnrpc_web::App;
//!
//! let router = RpcRouterBuilder::<()>::new()
//!     .route_fn(health_check)
//!     .build();
//!
//! App::build(|_| ())
//!     .rpc("/api/{*path}", router)
//!     .static_dir("/static", "./www")
//!     .run("0.0.0.0:3000")
//!     .await
//!     .unwrap();
//! ```

use std::borrow::Cow;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;

use fnrpc::error::RpcErr;
use fnrpc::router::ErasedHandler;
use fnrpc::router::RpcRouter;
use futures::StreamExt;
use xitca_http::body::{BodyExt, Frame, RequestBody, ResponseBody, StreamBody};
use xitca_http::bytes::Bytes;
use xitca_http::http::Extensions;
use xitca_http::http::header::{HeaderValue, CONTENT_TYPE};
use xitca_http::http::{HeaderMap, Method, Request, RequestExt, Response, StatusCode};
use xitca_http::HttpServiceBuilder;
use xitca_router::Router;
use xitca_server::Builder;
use xitca_service::{fn_service, ServiceExt};

// ── URL percent-decoding ────────────────────────────────

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

// ── App (single router, zero-overhead) ─────────────────

/// Thin HTTP transport layer for fnrpc — single-router mode.
///
/// Calls [`RpcRouter::dispatch`] directly — zero `Box::pin`, zero indirection.
/// This is the fastest path through fnrpc-web, ~20% fewer allocations than
/// xitca-web and ~2.8× fewer than axum.
///
/// For multi-router mode (RPC + static files), use [`App::build`].
pub struct App<Ctx: Send + Sync + 'static> {
    router: RpcRouter<Ctx>,
    ctx_factory: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
}

impl<Ctx: Send + Sync + 'static> App<Ctx> {
    /// Create a single-router app.
    pub fn new(router: RpcRouter<Ctx>, ctx_factory: impl Fn(&HeaderMap) -> Ctx + Send + Sync + 'static) -> Self {
        Self {
            router,
            ctx_factory: Arc::new(ctx_factory),
        }
    }

    /// Process a single request in-process (for testing/benchmarking).
    pub async fn call(&self, req: Request<RequestExt<RequestBody>>) -> Response<ResponseBody> {
        single_call(&self.router, &self.ctx_factory, req).await
    }

    /// Start the server.
    pub async fn run(self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let router = Arc::new(self.router);
        let ctx_factory = self.ctx_factory;
        run_single(router, ctx_factory, addr).await
    }

    /// Create a multi-router builder.
    pub fn build(ctx_factory: impl Fn(&HeaderMap) -> Ctx + Send + Sync + 'static) -> AppBuilder<Ctx> {
        AppBuilder {
            ctx_factory: Arc::new(ctx_factory),
            router: Router::new(),
        }
    }
}

// ── AppBuilder (multi-router) ──────────────────────────

/// Builder for multi-router apps.
///
/// Created via [`App::build`]. Supports RPC routes and optional static file serving.
///
/// Uses `xitca_router::Router` for radix-tree routing. Each handler is stored as
/// `Box<dyn ErasedHandler>`. One `Box::pin` per request at the route dispatch boundary
/// (same as xitca-web's `RouterService`).
///
/// # Performance
///
/// Multi-router mode adds ~205B/3blks compared to single-router mode:
/// - `xitca_router::Router::at` match + params (1 blk)
/// - `Box<dyn ErasedHandler>::call` vtable + `Box::pin` (1 blk)
/// - `Box::new(router)` storage in radix tree (1 blk)
///
/// Single router mode (`App::new`) avoids all of these — zero `Box::pin`, zero overhead.
pub struct AppBuilder<Ctx: Send + Sync + 'static> {
    ctx_factory: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
    router: Router<Box<dyn ErasedHandler<Ctx>>>,
}

impl<Ctx: Send + Sync + 'static> AppBuilder<Ctx> {
    /// Add an RPC route at the given path pattern.
    pub fn rpc(mut self, path: &str, router: RpcRouter<Ctx>) -> Self {
        let handler: Box<dyn ErasedHandler<Ctx>> = Box::new(router);
        self.router.insert(path.to_string(), handler).unwrap();
        self
    }

    /// Add a static file directory.
    #[cfg(feature = "file")]
    pub fn static_dir(mut self, path_prefix: &str, dir: impl Into<PathBuf>) -> Self {
        let dir = Arc::new(dir.into());
        let prefix_len = path_prefix.trim_end_matches('/').len();
        let handler = StaticDirHandler { dir, prefix_len };
        self.router.insert(
            format!("{}/{{*path}}", path_prefix.trim_end_matches('/')),
            Box::new(handler),
        ).unwrap();
        self
    }

    /// Process a single request (for testing/benchmarking).
    pub async fn call(&self, req: Request<RequestExt<RequestBody>>) -> Response<ResponseBody> {
        let ctx = (self.ctx_factory)(req.headers());
        multi_call(&self.router, &ctx, req).await
    }

    /// Start the server.
    pub async fn run(self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let router = Arc::new(self.router);
        let ctx_factory = self.ctx_factory;

        let svc = fn_service(move |req: Request<RequestExt<RequestBody>>| {
            let router = Arc::clone(&router);
            let ctx_factory = Arc::clone(&ctx_factory);
            async move {
                let ctx = ctx_factory(req.headers());
                let result = multi_call(&router, &ctx, req).await;
                Ok::<_, Infallible>(result)
            }
        })
        .enclosed(HttpServiceBuilder::new().io_uring());

        Builder::new().bind("fnrpc-web", addr, svc)?.build().await?;
        Ok(())
    }
}

// ── Static file handler ─────────────────────────────────

/// Handler wrapper for RpcRouter in multi-router mode.
/// Strips the route prefix before dispatching to the inner router.
#[allow(dead_code)]
struct RpcRouterHandler<Ctx: Send + Sync + 'static> {
    router: RpcRouter<Ctx>,
    prefix_len: usize,
}

impl<Ctx: Send + Sync + 'static> fnrpc::middleware::RpcService<Ctx> for RpcRouterHandler<Ctx> {
    type Response = (Cow<'static, [u8]>, bool);
    type Error = RpcErr;

    async fn call(
        &self,
        ctx: &Ctx,
        path: &str,
        input: &[u8],
        is_get: bool,
        _extensions: &mut Extensions,
    ) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
        let dispatch_path = if self.prefix_len > 0 && path.len() > self.prefix_len {
            &path[self.prefix_len..]
        } else {
            path
        };
        let dispatch_path = dispatch_path.strip_prefix('/').unwrap_or(dispatch_path);
        self.router.dispatch(ctx, dispatch_path, input, is_get).await
    }
}

#[cfg(feature = "file")]
struct StaticDirHandler {
    dir: Arc<PathBuf>,
    prefix_len: usize,
}

#[cfg(feature = "file")]
impl<Ctx: Send + Sync + 'static> fnrpc::middleware::RpcService<Ctx> for StaticDirHandler {
    type Response = (Cow<'static, [u8]>, bool);
    type Error = RpcErr;

    async fn call(
        &self,
        _ctx: &Ctx,
        path: &str,
        _input: &[u8],
        _is_get: bool,
        _extensions: &mut Extensions,
    ) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
        let relative = if self.prefix_len > 0 && path.len() > self.prefix_len {
            &path[self.prefix_len..]
        } else {
            path.strip_prefix('/').unwrap_or(path)
        };
        let file_path = self.dir.join(relative.trim_start_matches('/'));
        match tokio::fs::read(&file_path).await {
            Ok(data) => Ok((Cow::Owned(data), false)),
            Err(_) => Err(RpcErr::not_found("file not found")),
        }
    }
}

// ── Shared helpers ──────────────────────────────────────

async fn single_call<Ctx: Send + Sync + 'static>(
    router: &RpcRouter<Ctx>,
    ctx_factory: &Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
    mut req: Request<RequestExt<RequestBody>>,
) -> Response<ResponseBody> {
    let ctx = ctx_factory(req.headers());
    let path = req.uri().path().strip_prefix('/').unwrap_or("").to_owned();

    if router.has_subscribe(&path) {
        let input: Cow<'_, [u8]> = if req.method() == Method::GET {
            // Extract and URL-decode the "input" query parameter
            let input_str = req.uri().query().unwrap_or("").split('&').find_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                let key = parts.next()?;
                let val = parts.next()?;
                if key == "input" { Some(percent_decode(val)) } else { None }
            }).unwrap_or_default();
            // Unpack meta envelope (BigInt → number, etc.) and re-serialize
            let val: serde_json::Value = serde_json::from_str(&input_str).unwrap_or(serde_json::Value::Null);
            let unpacked = fnrpc::serializer::unpack_meta(val);
            serde_json::to_vec(&unpacked).unwrap_or_default().into()
        } else {
            let mut body_buf = Vec::new();
            while let Some(chunk) = req.body_mut().data().await {
                match chunk {
                    Ok(c) => body_buf.extend_from_slice(c.as_ref()),
                    Err(_) => {
                        let body = serde_json::to_vec(&RpcErr::bad_request("body read error")).unwrap_or_default();
                        return Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                            .body(ResponseBody::bytes(Bytes::copy_from_slice(&body)))
                            .unwrap();
                    }
                }
            }
            body_buf.into()
        };
        // Unpack meta envelope (BigInt → number, etc.) and re-serialize
        let val: serde_json::Value = serde_json::from_slice(&input).unwrap_or(serde_json::Value::Null);
        let unpacked = fnrpc::serializer::unpack_meta(val);
        let input = serde_json::to_vec(&unpacked).unwrap_or_default();
        return build_sse_response(router.dispatch_subscribe(&ctx, &path, &input));
    }

    let input: Cow<'_, [u8]> = if req.method() == Method::GET {
        req.uri().query().unwrap_or("").as_bytes().into()
    } else {
        let mut body_buf = Vec::new();
        while let Some(chunk) = req.body_mut().data().await {
            match chunk {
                Ok(c) => body_buf.extend_from_slice(c.as_ref()),
                Err(_) => {
                    let body = serde_json::to_vec(&RpcErr::bad_request("body read error")).unwrap_or_default();
                    return Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                        .body(ResponseBody::bytes(Bytes::copy_from_slice(&body)))
                        .unwrap();
                }
            }
        }
        body_buf.into()
    };

    let is_get = req.method() == Method::GET;
    let result = router.dispatch(&ctx, &path, &input, is_get).await;
    build_response(result)
}

async fn multi_call<Ctx: Send + Sync + 'static>(
    router: &Router<Box<dyn ErasedHandler<Ctx>>>,
    ctx: &Ctx,
    mut req: Request<RequestExt<RequestBody>>,
) -> Response<ResponseBody> {
    let path = req.uri().path().to_string();
    let matched = router.at(&path).ok();
    if let Some(m) = matched {
        let input: Cow<'_, [u8]> = if req.method() == Method::GET {
            req.uri().query().unwrap_or("").as_bytes().into()
        } else {
            let mut buf = Vec::new();
            while let Some(chunk) = req.body_mut().data().await {
                match chunk {
                    Ok(c) => buf.extend_from_slice(c.as_ref()),
                    Err(_) => {
                        let body = serde_json::to_vec(&RpcErr::bad_request("body read error")).unwrap_or_default();
                        return Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                            .body(ResponseBody::bytes(Bytes::copy_from_slice(&body)))
                            .unwrap();
                    }
                }
            }
            buf.into()
        };

        let is_get = req.method() == Method::GET;
        let mut extensions = Extensions::new();
        let result = m.value.call(ctx, &path, &input, is_get, &mut extensions).await;
        build_response(result)
    } else {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(ResponseBody::bytes(Bytes::from_static(b"not found")))
            .unwrap()
    }
}

fn build_response(result: Result<(Cow<'static, [u8]>, bool), RpcErr>) -> Response<ResponseBody> {
    match result {
        Ok((bytes, is_json)) => {
            let mut builder = Response::builder().status(StatusCode::OK);
            if is_json {
                builder = builder.header(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            }
            let resp_body = match &bytes {
                Cow::Borrowed(b"null") => ResponseBody::bytes(Bytes::from_static(b"null")),
                Cow::Borrowed(slice) => ResponseBody::bytes(Bytes::from_static(slice)),
                Cow::Owned(vec) => ResponseBody::bytes(Bytes::copy_from_slice(vec)),
            };
            builder.body(resp_body).unwrap()
        }
        Err(e) => {
            let status = match e.code.as_str() {
                "BAD_REQUEST" => StatusCode::BAD_REQUEST,
                "NOT_FOUND" => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            let body = serde_json::to_vec(&e).unwrap_or_default();
            Response::builder()
                .status(status)
                .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                .body(ResponseBody::bytes(Bytes::from(body)))
                .unwrap()
        }
    }
}

/// Build an SSE response from a subscribe stream result.
///
/// On success, returns a streaming response with `Content-Type: text/event-stream`.
/// Each stream item is formatted as `data: <json>\n\n`.
/// On error, returns a JSON error response.
fn build_sse_response(
    result: Result<
        Pin<Box<dyn futures::Stream<Item = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'static>>,
        RpcErr,
    >,
) -> Response<ResponseBody> {
    match result {
        Ok(stream) => {
            let sse_stream = stream.map(|item| match item {
                Ok(bytes) => Ok::<_, Infallible>(Frame::Data(Bytes::from(format!(
                    "data: {}\n\n",
                    String::from_utf8_lossy(&bytes)
                )))),
                Err(e) => Ok::<_, Infallible>(Frame::Data(Bytes::from(format!(
                    "data: {}\n\n",
                    serde_json::to_string(&e).unwrap_or_default()
                )))),
            });
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))
                .header("cache-control", HeaderValue::from_static("no-cache"))
                .body(ResponseBody::body(StreamBody::new(sse_stream)).into_boxed())
                .unwrap()
        }
        Err(e) => {
            let status = match e.code.as_str() {
                "NOT_FOUND" => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            let body = serde_json::to_vec(&e).unwrap_or_default();
            Response::builder()
                .status(status)
                .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                .body(ResponseBody::bytes(Bytes::from(body)))
                .unwrap()
        }
    }
}

async fn run_single<Ctx: Send + Sync + 'static>(
    router: Arc<RpcRouter<Ctx>>,
    ctx_factory: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
    addr: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let svc = fn_service(move |mut req: Request<RequestExt<RequestBody>>| {
        let router = Arc::clone(&router);
        let ctx_factory = Arc::clone(&ctx_factory);
        async move {
            let ctx = ctx_factory(req.headers());
            let path = req.uri().path().strip_prefix('/').unwrap_or("").to_owned();

            if router.has_subscribe(&path) {
                let raw_input: Cow<'_, [u8]> = if req.method() == Method::GET {
                    // Extract and URL-decode the "input" query parameter
                    let input_str = req.uri().query().unwrap_or("").split('&').find_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        let key = parts.next()?;
                        let val = parts.next()?;
                        if key == "input" { Some(percent_decode(val)) } else { None }
                    }).unwrap_or_default();
                    input_str.into_bytes().into()
                } else {
                    let mut body_buf = Vec::new();
                    while let Some(chunk) = req.body_mut().data().await {
                        match chunk {
                            Ok(c) => body_buf.extend_from_slice(c.as_ref()),
                            Err(_) => {
                                let body = serde_json::to_vec(&RpcErr::bad_request("body read error")).unwrap_or_default();
                                return Ok(Response::builder()
                                    .status(StatusCode::BAD_REQUEST)
                                    .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                                    .body(ResponseBody::bytes(Bytes::copy_from_slice(&body)))
                                    .unwrap());
                            }
                        }
                    }
                    body_buf.into()
                };
                // Unpack meta envelope (BigInt → number, etc.) and re-serialize
                let val: serde_json::Value = serde_json::from_slice(&raw_input).unwrap_or(serde_json::Value::Null);
                let unpacked = fnrpc::serializer::unpack_meta(val);
                let input = serde_json::to_vec(&unpacked).unwrap_or_default();
                return Ok::<_, Infallible>(build_sse_response(router.dispatch_subscribe(&ctx, &path, &input)));
            }

            let input: Cow<'_, [u8]> = if req.method() == Method::GET {
                req.uri().query().unwrap_or("").as_bytes().into()
            } else {
                let mut body_buf = Vec::new();
                while let Some(chunk) = req.body_mut().data().await {
                    match chunk {
                        Ok(c) => body_buf.extend_from_slice(c.as_ref()),
                        Err(_) => {
                            let body = serde_json::to_vec(&RpcErr::bad_request("body read error")).unwrap_or_default();
                            return Ok(Response::builder()
                                .status(StatusCode::BAD_REQUEST)
                                .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                                .body(ResponseBody::bytes(Bytes::copy_from_slice(&body)))
                                .unwrap());
                        }
                    }
                }
                body_buf.into()
            };

            let is_get = req.method() == Method::GET;
            let result = router.dispatch(&ctx, &path, &input, is_get).await;
            Ok::<_, Infallible>(build_response(result))
        }
    })
    .enclosed(HttpServiceBuilder::new().io_uring());

    Builder::new().bind("fnrpc-web", addr, svc)?.build().await?;
    Ok(())
}
