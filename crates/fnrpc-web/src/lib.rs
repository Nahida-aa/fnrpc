//! Standalone fnrpc HTTP app on xitca-http + xitca-server.
//!
//! Each procedure is registered via `.register()` and collected into a
//! single HTTP service at `.run()` time.
//!
//! # Example
//!
//! ```ignore
//! use fnrpc_web::App;
//!
//! App::new(|_| ())
//!     .register(health_check)
//!     .run("0.0.0.0:3000")
//!     .await
//!     .unwrap();
//! ```

use std::borrow::Cow;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use fnrpc::handler::RpcFnExt;
use serde_json::Value;
use xitca_http::body::{BodyExt, RequestBody, ResponseBody};
use xitca_http::bytes::Bytes;
use xitca_http::http::header::{HeaderValue, CONTENT_TYPE};
use xitca_http::http::{HeaderMap, Method, Request, RequestExt, Response, StatusCode};
use xitca_http::HttpServiceBuilder;
use xitca_server::Builder;
use xitca_service::{fn_service, Service, ServiceExt};

use futures::StreamExt;

// ── Procedure type info (for TS codegen) ─────────────────

/// Metadata for a registered procedure.
#[derive(Clone)]
pub struct ProcedureMeta {
    pub key: &'static str,
    pub kind: &'static str,
    pub method: &'static str,
    pub input: fnrpc::handler::TsTypeInfo,
    pub output: fnrpc::handler::TsTypeInfo,
}

// ── Server ────────────────────────────────────────────────

/// HTTP server builder for fnrpc.
pub struct App<Ctx: Send + Sync + 'static> {
    ctx_factory: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
    procedures: Vec<ProcedureMeta>,
    handlers: Vec<Arc<dyn for<'a> Fn(&'a Ctx, &'a [u8]) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, fnrpc::error::RpcErr>> + Send + 'a>> + Send + Sync>>,
}

impl<Ctx: Send + Sync + 'static> App<Ctx> {
    pub fn new(ctx_factory: impl Fn(&HeaderMap) -> Ctx + Send + Sync + 'static) -> Self {
        Self {
            ctx_factory: Arc::new(ctx_factory),
            procedures: Vec::new(),
            handlers: Vec::new(),
        }
    }

    /// Register a typed RPC function.
    pub fn register<H>(mut self, handler: H) -> Self
    where
        H: fnrpc::handler::RpcFn<Ctx> + Send + Sync + 'static,
    {
        self.procedures.push(ProcedureMeta {
            key: H::KEY,
            kind: H::KIND,
            method: H::METHOD,
            input: fnrpc::gen_ts_client::type_ts::<H::Input>(),
            output: fnrpc::gen_ts_client::type_ts::<H::Output>(),
        });

        let handler = Arc::new(handler);
        self.handlers.push(Arc::new(move |ctx: &Ctx, input: &[u8]| {
            let handler = Arc::clone(&handler);
            Box::pin(async move {
                let result = handler.call_bytes(ctx, input).await?;
                Ok(result.into_owned())
            })
        }));

        self
    }

    /// Return procedure metadata for TypeScript codegen.
    pub fn procedures(&self) -> &[ProcedureMeta] {
        &self.procedures
    }

    /// Process a single HTTP request in-process (for testing/benchmarking).
    ///
    /// This runs the same handler dispatch as the real server, but without
    /// network or `HttpServiceBuilder` overhead.
    pub async fn call_request(&self, req: Request<RequestExt<RequestBody>>) -> Response<ResponseBody> {
        let ctx = (self.ctx_factory)(req.headers());
        let mut req = req;

        // Read body for POST
        let mut body_buf = Vec::new();
        if req.method() == Method::POST {
            while let Some(chunk) = req.body_mut().data().await {
                match chunk {
                    Ok(c) => body_buf.extend_from_slice(c.as_ref()),
                    Err(_) => {
                        let body = serde_json::to_vec(&fnrpc::error::RpcErr::bad_request("body read error")).unwrap_or_default();
                        return xitca_http::http::Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                            .body(ResponseBody::bytes(Bytes::copy_from_slice(&body)))
                            .unwrap();
                    }
                }
            }
        }

        // Input source
        let input = if req.method() == Method::GET {
            req.uri().query().unwrap_or("").as_bytes().to_vec()
        } else {
            body_buf
        };

        // Dispatch to first handler
        let result = match self.handlers.first() {
            Some(h) => Some(h(&ctx, &input).await),
            None => None,
        };

        match result {
            Some(Ok(bytes)) => {
                let mut builder = xitca_http::http::Response::builder().status(StatusCode::OK);
                builder = builder.header(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                let resp_body = match bytes.as_slice() {
                    b"null" => ResponseBody::bytes(Bytes::from_static(b"null")),
                    _ => ResponseBody::bytes(Bytes::copy_from_slice(&bytes)),
                };
                builder.body(resp_body).unwrap()
            }
            Some(Err(e)) => {
                let status = match e.code.as_str() {
                    "BAD_REQUEST" => StatusCode::BAD_REQUEST,
                    "NOT_FOUND" => StatusCode::NOT_FOUND,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                let body = serde_json::to_vec(&e).unwrap_or_default();
                xitca_http::http::Response::builder()
                    .status(status)
                    .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                    .body(ResponseBody::bytes(Bytes::copy_from_slice(&body)))
                    .unwrap()
            }
            None => {
                let body = serde_json::json!({"error": "not found"});
                let bytes = serde_json::to_vec(&body).unwrap_or_default();
                xitca_http::http::Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                    .body(ResponseBody::bytes(Bytes::copy_from_slice(&bytes)))
                    .unwrap()
            }
        }
    }

    /// Build a full HTTP service (with `HttpServiceBuilder`) and bind to address.
    pub async fn run(self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let ctx_factory = self.ctx_factory;
        let handlers = Arc::new(self.handlers);

        let svc = fn_service(move |mut req: Request<RequestExt<RequestBody>>| {
            let ctx_factory = Arc::clone(&ctx_factory);
            let handlers = Arc::clone(&handlers);
            async move {
                let ctx = ctx_factory(req.headers());

                let mut body_buf = Vec::new();
                if req.method() == Method::POST {
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
                }

                let input = if req.method() == Method::GET {
                    req.uri().query().unwrap_or("").as_bytes().to_vec()
                } else {
                    body_buf
                };

                let result = match handlers.first() {
                    Some(h) => Some(h(&ctx, &input).await),
                    None => None,
                };

                match result {
                    Some(Ok(bytes)) => {
                        let mut builder = Response::builder().status(StatusCode::OK);
                        builder = builder.header(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                        let resp_body = match bytes.as_slice() {
                            b"null" => ResponseBody::bytes(Bytes::from_static(b"null")),
                            _ => ResponseBody::bytes(Bytes::copy_from_slice(&bytes)),
                        };
                        Ok::<_, Infallible>(builder.body(resp_body).unwrap())
                    }
                    Some(Err(e)) => {
                        let status = match e.code.as_str() {
                            "BAD_REQUEST" => StatusCode::BAD_REQUEST,
                            "NOT_FOUND" => StatusCode::NOT_FOUND,
                            _ => StatusCode::INTERNAL_SERVER_ERROR,
                        };
                        let body = serde_json::to_vec(&e).unwrap_or_default();
                        Ok(Response::builder()
                            .status(status)
                            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                            .body(ResponseBody::bytes(Bytes::copy_from_slice(&body)))
                            .unwrap())
                    }
                    None => {
                        let body = serde_json::json!({"error": "not found"});
                        let bytes = serde_json::to_vec(&body).unwrap_or_default();
                        Ok(Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                            .body(ResponseBody::bytes(Bytes::copy_from_slice(&bytes)))
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
