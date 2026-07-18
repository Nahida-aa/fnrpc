//! Standalone fnrpc HTTP server on xitca-http + xitca-server.
//!
//! A thin HTTP transport layer. Routing and handler dispatch are handled
//! by [`fnrpc::router::RpcRouter`].
//!
//! # Example
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

use std::convert::Infallible;
use std::sync::Arc;

use fnrpc::router::RpcRouter;
use serde_json::Value;
use xitca_http::body::{BodyExt, RequestBody, ResponseBody};
use xitca_http::bytes::Bytes;
use xitca_http::http::header::{HeaderValue, CONTENT_TYPE};
use xitca_http::http::{HeaderMap, Method, Request, RequestExt, Response, StatusCode};
use xitca_http::HttpServiceBuilder;
use xitca_server::Builder;
use xitca_service::{fn_service, ServiceExt};

use futures::StreamExt;

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

// ── App ──────────────────────────────────────────────────

/// Thin HTTP transport layer for fnrpc.
///
/// Wraps a [`RpcRouter`] with HTTP request parsing and response building.
/// No routing logic — all dispatch goes through `RpcRouter::call_handler`.
pub struct App<Ctx: Send + Sync + 'static> {
    router: RpcRouter<Ctx>,
    ctx_factory: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
}

impl<Ctx: Send + Sync + 'static> App<Ctx> {
    /// Create a new app with a router and context factory.
    pub fn new(router: RpcRouter<Ctx>, ctx_factory: impl Fn(&HeaderMap) -> Ctx + Send + Sync + 'static) -> Self {
        Self {
            router,
            ctx_factory: Arc::new(ctx_factory),
        }
    }

    /// Process a single request in-process (for testing/benchmarking).
    pub async fn call(&self, req: Request<RequestExt<RequestBody>>) -> Response<ResponseBody> {
        let ctx = (self.ctx_factory)(req.headers());
        let mut req = req;

        // Only allocate body_buf for POST
        let input: std::borrow::Cow<'_, [u8]> = if req.method() == Method::GET {
            req.uri().query().unwrap_or("").as_bytes().into()
        } else {
            let mut body_buf = Vec::new();
            while let Some(chunk) = req.body_mut().data().await {
                match chunk {
                    Ok(c) => body_buf.extend_from_slice(c.as_ref()),
                    Err(_) => {
                        let body = serde_json::to_vec(&fnrpc::error::RpcErr::bad_request("body read error")).unwrap_or_default();
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
        let result = self.router.call_handler(path, &ctx, &input, is_get).await;

        match result {
            Ok((bytes, is_json)) => {
                let mut builder = Response::builder().status(StatusCode::OK);
                if is_json {
                    builder = builder.header(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                }
                let resp_body = ResponseBody::bytes(Bytes::from(bytes));
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

    /// Start the server (binds to address and serves via network).
    pub async fn run(self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let router = Arc::new(self.router);
        let ctx_factory = self.ctx_factory;

        let svc = fn_service(move |mut req: Request<RequestExt<RequestBody>>| {
            let router = Arc::clone(&router);
            let ctx_factory = Arc::clone(&ctx_factory);
            async move {
                let ctx = ctx_factory(req.headers());

                // Only allocate body_buf for POST
                let input: std::borrow::Cow<'_, [u8]> = if req.method() == Method::GET {
                    req.uri().query().unwrap_or("").as_bytes().into()
                } else {
                    let mut body_buf = Vec::new();
                    while let Some(chunk) = req.body_mut().data().await {
                        match chunk {
                            Ok(c) => body_buf.extend_from_slice(c.as_ref()),
                            Err(_) => {
                                let body = serde_json::to_vec(&fnrpc::error::RpcErr::bad_request("body read error")).unwrap_or_default();
                                return Ok(Response::builder()
                                    .status(StatusCode::BAD_REQUEST)
                                    .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                                    .body(ResponseBody::<RequestBody>::bytes(Bytes::copy_from_slice(&body)))
                                    .unwrap());
                            }
                        }
                    }
                    body_buf.into()
                };

                let path = req.uri().path().strip_prefix('/').unwrap_or("");
                let is_get = req.method() == Method::GET;
                let result = router.call_handler(path, &ctx, &input, is_get).await;

                match result {
                    Ok((bytes, is_json)) => {
                        let mut builder = Response::builder().status(StatusCode::OK);
                        if is_json {
                            builder = builder.header(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                        }
                        let resp_body = ResponseBody::bytes(Bytes::from(bytes));
                        Ok::<_, Infallible>(builder.body(resp_body).unwrap())
                    }
                    Err(e) => {
                        let status = match e.code.as_str() {
                            "BAD_REQUEST" => StatusCode::BAD_REQUEST,
                            "NOT_FOUND" => StatusCode::NOT_FOUND,
                            _ => StatusCode::INTERNAL_SERVER_ERROR,
                        };
                        let body = serde_json::to_vec(&e).unwrap_or_default();
                        Ok(Response::builder()
                            .status(status)
                            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                            .body(ResponseBody::bytes(Bytes::from(body)))
                            .unwrap())
                    }
                }
            }
        })
        .enclosed(HttpServiceBuilder::new().io_uring());

        Builder::new().bind("fnrpc-web", addr, svc)?.build().await?;
        Ok(())
    }
}
