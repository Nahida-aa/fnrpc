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
use std::future::Future;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use fnrpc::error::RpcErr;
use fnrpc::middleware::RpcService;
use fnrpc::router::RpcRouter;
use xitca_http::body::{BodyExt, RequestBody, ResponseBody};
use xitca_http::bytes::Bytes;
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

// ── ErasedHandler trait ─────────────────────────────────

/// Type-erased handler for multi-router dispatch.
/// One `Box::pin` per request at the router boundary.
trait ErasedHandler<Ctx: Send + Sync + 'static>: Send + Sync {
    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        path: &'a str,
        input: &'a [u8],
        is_get: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(Cow<'static, [u8]>, bool), RpcErr>> + 'a>>;
}

/// Wraps an RpcRouter into an ErasedHandler.
struct RpcHandler<Ctx: Send + Sync + 'static, S: RpcService<Ctx> + Send + Sync + 'static> {
    router: RpcRouter<Ctx, S>,
}

impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr> + Send + Sync>
    ErasedHandler<Ctx> for RpcHandler<Ctx, S>
{
    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        path: &'a str,
        input: &'a [u8],
        is_get: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(Cow<'static, [u8]>, bool), RpcErr>> + 'a>> {
        Box::pin(self.router.dispatch(ctx, path, input, is_get))
    }
}

// ── App (single router, zero-overhead) ─────────────────

/// Thin HTTP transport layer for fnrpc — single-router mode.
///
/// The middleware chain is monomorphized at compile time with zero indirection.
/// For multi-router mode (RPC + static files), use [`App::build`].
pub struct App<Ctx: Send + Sync + 'static, S: RpcService<Ctx> + Send + Sync + 'static> {
    router: RpcRouter<Ctx, S>,
    ctx_factory: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
}

impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr> + Send + Sync>
    App<Ctx, S>
{
    /// Create a single-router app (zero `Box::pin` overhead).
    pub fn new(router: RpcRouter<Ctx, S>, ctx_factory: impl Fn(&HeaderMap) -> Ctx + Send + Sync + 'static) -> Self {
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
}

impl<Ctx: Send + Sync + 'static> App<Ctx, fnrpc::router::InnerService<Ctx>> {
    /// Create a multi-router builder.
    ///
    /// Use `.rpc()` and `.static_dir()` to add routes, then `.run()` to start.
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
/// Uses radix tree routing — O(path_length) matching, no `for` loop at request time.
pub struct AppBuilder<Ctx: Send + Sync + 'static> {
    ctx_factory: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
    router: Router<Box<dyn ErasedHandler<Ctx>>>,
}

impl<Ctx: Send + Sync + 'static> AppBuilder<Ctx> {
    /// Add an RPC route at the given path pattern.
    ///
    /// The path pattern supports xitca-router syntax (e.g. `"/api/{*path}"`).
    pub fn rpc<S>(mut self, path: &str, router: RpcRouter<Ctx, S>) -> Self
    where
        S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr> + Send + Sync + 'static,
    {
        self.router.insert(path.to_string(), Box::new(RpcHandler { router })).unwrap();
        self
    }

    /// Add a static file directory.
    ///
    /// Files under `dir` will be served at URLs matching `path_prefix/*`.
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

#[cfg(feature = "file")]
struct StaticDirHandler {
    dir: Arc<PathBuf>,
    prefix_len: usize,
}

#[cfg(feature = "file")]
impl<Ctx: Send + Sync + 'static> ErasedHandler<Ctx> for StaticDirHandler {
    fn call<'a>(
        &'a self,
        _ctx: &'a Ctx,
        path: &'a str,
        _input: &'a [u8],
        _is_get: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(Cow<'static, [u8]>, bool), RpcErr>> + 'a>> {
        Box::pin(async move {
            let relative = path.strip_prefix('/')
                .and_then(|p| {
                    if self.prefix_len > 0 && p.len() > self.prefix_len {
                        Some(&p[self.prefix_len..])
                    } else {
                        Some(p)
                    }
                })
                .unwrap_or(path);
            let file_path = self.dir.join(relative.trim_start_matches('/'));
            match tokio::fs::read(&file_path).await {
                Ok(data) => Ok((Cow::Owned(data), false)),
                Err(_) => Err(RpcErr::not_found("file not found")),
            }
        })
    }
}

// ── Shared helpers ──────────────────────────────────────

async fn single_call<Ctx, S>(
    router: &RpcRouter<Ctx, S>,
    ctx_factory: &Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
    mut req: Request<RequestExt<RequestBody>>,
) -> Response<ResponseBody>
where
    Ctx: Send + Sync + 'static,
    S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr> + Send + Sync,
{
    let ctx = ctx_factory(req.headers());

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

    let path = req.uri().path().strip_prefix('/').unwrap_or("");
    let is_get = req.method() == Method::GET;
    let result = router.dispatch(&ctx, path, &input, is_get).await;
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
        // Use the catch-all param as dispatch path if present, else strip leading slash
        let dispatch_path = m.params.get("path")
            .map(|s| &s[..])
            .unwrap_or_else(|| path.strip_prefix('/').unwrap_or(&path));
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
        let result = m.value.call(ctx, dispatch_path, &input, is_get).await;
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

async fn run_single<Ctx, S>(
    router: Arc<RpcRouter<Ctx, S>>,
    ctx_factory: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
    addr: &str,
) -> Result<(), Box<dyn std::error::Error>>
where
    Ctx: Send + Sync + 'static,
    S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr> + Send + Sync,
{
    let svc = fn_service(move |mut req: Request<RequestExt<RequestBody>>| {
        let router = Arc::clone(&router);
        let ctx_factory = Arc::clone(&ctx_factory);
        async move {
            let ctx = ctx_factory(req.headers());

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

            let path = req.uri().path().strip_prefix('/').unwrap_or("");
            let is_get = req.method() == Method::GET;
            let result = router.dispatch(&ctx, path, &input, is_get).await;
            Ok::<_, Infallible>(build_response(result))
        }
    })
    .enclosed(HttpServiceBuilder::new().io_uring());

    Builder::new().bind("fnrpc-web", addr, svc)?.build().await?;
    Ok(())
}
