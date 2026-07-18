//! Standalone fnrpc HTTP server on xitca-web.
//!
//! Each procedure is registered as an independent route — zero type erasure,
//! zero `Box::pin` in the dispatch path.
//!
//! # Example
//!
//! ```ignore
//! use fnrpc_web::Server;
//!
//! Server::new(|_| ())
//!     .register(health_check)
//!     .register(create_user)
//!     .run("0.0.0.0:3000")
//!     .await
//!     .unwrap();
//! ```

use std::convert::Infallible;
use std::sync::Arc;

use fnrpc::handler::RpcFnExt;
use xitca_http::body::{BodyExt, RequestBody, ResponseBody};
use xitca_http::bytes::Bytes;
use xitca_http::http::header::{HeaderValue, CONTENT_TYPE};
use xitca_http::http::{HeaderMap, Method, Request, RequestExt, Response, StatusCode};
use xitca_web::handler::handler_service;
use xitca_web::route::{get, post};
use xitca_web::service::fn_service;
use xitca_web::App;

/// HTTP server builder for fnrpc.
///
/// Each `.register()` call adds an independent route — the handler type
/// is monomorphized at compile time, producing zero type erasure and
/// zero `Box::pin` in the dispatch path.
pub struct Server<Ctx: Send + Sync + 'static> {
    app: App<Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>>,
    ctx_factory: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
}

impl<Ctx: Send + Sync + 'static> Server<Ctx> {
    /// Create a new server with a context factory.
    pub fn new(ctx_factory: impl Fn(&HeaderMap) -> Ctx + Send + Sync + 'static) -> Self {
        let ctx_factory = Arc::new(ctx_factory);
        Self {
            app: App::new().with_state(ctx_factory.clone()),
            ctx_factory,
        }
    }

    /// Register a typed RPC function as an HTTP route.
    ///
    /// The handler type `H` is monomorphized — the HTTP service is
    /// compiled specifically for this handler with zero boxing overhead.
    pub fn register<H>(mut self, handler: H) -> Self
    where
        H: fnrpc::handler::RpcFn<Ctx> + Clone + Send + Sync + 'static,
    {
        let route = fn_service(move |mut req: Request<RequestExt<RequestBody>>| {
            let handler = handler.clone();
            async move {
                let ctx = req.extensions().get::<Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>>()
                    .unwrap()(req.headers());

                let result = if H::METHOD == "GET" {
                    let query_bytes = req.uri().query().unwrap_or("").as_bytes().to_vec();
                    handler.call_bytes(&ctx, &query_bytes).await
                } else {
                    let mut body_buf = Vec::new();
                    while let Some(chunk) = req.body_mut().data().await {
                        let chunk = chunk.map_err(|_| {
                            fnrpc::error::RpcErr::bad_request("body read error")
                        })?;
                        body_buf.extend_from_slice(chunk.as_ref());
                    }
                    handler.call_bytes(&ctx, &body_buf).await
                };

                match result {
                    Ok(bytes) => {
                        let mut builder = Response::builder().status(StatusCode::OK);
                        builder = builder.header(
                            CONTENT_TYPE,
                            HeaderValue::from_static("application/json"),
                        );
                        let resp_body = match &bytes {
                            std::borrow::Cow::Borrowed(b"null") => ResponseBody::bytes(Bytes::from_static(b"null")),
                            std::borrow::Cow::Borrowed(slice) => ResponseBody::bytes(Bytes::from_static(slice)),
                            std::borrow::Cow::Owned(ref vec) if vec == b"null" => ResponseBody::bytes(Bytes::from_static(b"null")),
                            std::borrow::Cow::Owned(vec) => ResponseBody::bytes(Bytes::from(vec)),
                        };
                        Ok::<Response<ResponseBody>, Infallible>(builder.body(resp_body).unwrap())
                    }
                    Err(e) => {
                        let status = match e.code.as_str() {
                            "BAD_REQUEST" => StatusCode::BAD_REQUEST,
                            "NOT_FOUND" => StatusCode::NOT_FOUND,
                            _ => StatusCode::INTERNAL_SERVER_ERROR,
                        };
                        let body = serde_json::to_vec(&e).unwrap_or_default();
                        let resp = Response::builder()
                            .status(status)
                            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                            .body(ResponseBody::bytes(Bytes::from(body)))
                            .unwrap();
                        Ok(resp)
                    }
                }
            }
        });

        let app = if H::METHOD == "POST" {
            self.app.at(H::KEY, post(handler_service(move |_: ()| Ok::<_, Infallible>(route.clone()))))
        } else {
            self.app.at(H::KEY, get(handler_service(move |_: ()| Ok::<_, Infallible>(route.clone()))))
        };

        Self { app, ctx_factory: self.ctx_factory }
    }

    /// Start the server.
    pub async fn run(self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.app
            .serve()
            .enable_io_uring()
            .bind(addr)?
            .run()
            .await
            .map_err(|e| e.into())
    }
}
