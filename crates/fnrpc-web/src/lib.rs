use std::convert::Infallible;
use std::sync::Arc;

use futures::StreamExt;
use serde_json::Value;
use xitca_http::body::{BodyExt, RequestBody, ResponseBody, StreamDataBody};
use xitca_http::bytes::Bytes;
use xitca_http::http::{
    header::{CONTENT_TYPE, HeaderValue}, HeaderMap, Method, Request, RequestExt, Response, StatusCode,
};
use xitca_http::HttpServiceBuilder;
use xitca_service::{fn_service, ServiceExt};
use xitca_server::Builder;

/// Shared state for the fnrpc standalone server.
pub struct RpcWebConfig<Ctx> {
    pub router: fnrpc::router::RpcRouter<Ctx>,
    pub ctx_from_headers: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
}

// ── Response builders ─────────────────────────────────────

/// Build a response from raw bytes, optionally setting Content-Type.
fn raw_response(status: StatusCode, body: Vec<u8>, content_type: Option<&'static str>) -> Response<ResponseBody> {
    let mut builder = Response::builder().status(status);
    if let Some(ct) = content_type {
        builder = builder.header(CONTENT_TYPE, HeaderValue::from_static(ct));
    }
    builder.body(ResponseBody::bytes(Bytes::from(body))).unwrap()
}

fn error_response(e: fnrpc::error::RpcErr) -> Response<ResponseBody> {
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

fn not_found_response(path: &str) -> Response<ResponseBody> {
    let body = serde_json::json!({ "error": format!("unknown path: {path}") });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(ResponseBody::bytes(Bytes::from(bytes)))
        .unwrap()
}

// ── SSE ───────────────────────────────────────────────────

fn sse_response<Ctx>(
    handler: Arc<dyn fnrpc::handler::ErasedSubscribeHandler<Ctx>>,
    ctx: &Ctx,
    input: Value,
) -> Response<ResponseBody>
where
    Ctx: Send + Sync + 'static,
{
    let stream = handler.call(ctx, input);
    let mut event_id = 0u64;
    let sse = stream.map(move |item| {
        event_id += 1;
        let data = match item {
            Ok(val) => format!(
                "id: {}\ndata: {}\n\n",
                event_id,
                serde_json::to_string(&val).unwrap()
            ),
            Err(e) => format!(
                "id: {}\ndata: __error:{}\n\n",
                event_id,
                serde_json::to_string(&e).unwrap()
            ),
        };
        Ok::<Bytes, std::convert::Infallible>(Bytes::from(data))
    });

    let mut res = Response::new(ResponseBody::boxed(StreamDataBody::new(sse)));
    *res.status_mut() = StatusCode::OK;
    res.headers_mut()
        .insert("content-type", "text/event-stream".parse().unwrap());
    res.headers_mut()
        .insert("cache-control", "no-cache".parse().unwrap());
    res
}

// ── Main handler ──────────────────────────────────────────

/// Handle a single fnrpc HTTP request.
///
/// Dispatches via [`ErasedHandler::call_bytes`] — the concrete handler
/// implementation decides the serialization protocol:
///
/// - [`RpcFn`](fnrpc::handler::RpcFn) handlers (registered via
///   [`query`](fnrpc::router::RpcRouterBuilder::query)/[`mutate`])
///   use [`JsonCodec`](fnrpc::codec::JsonCodec) and return
///   `Content-Type: application/json`.
/// - [`RawRpcFn`](fnrpc::handler::RawRpcFn) handlers (registered via
///   [`raw`](fnrpc::router::RpcRouterBuilder::raw)) pass bytes through
///   with no serialization and no Content-Type header.
///
/// No runtime protocol detection — the protocol is determined at
/// handler registration time.
pub async fn handle<Ctx, B>(
    config: &RpcWebConfig<Ctx>,
    mut req: Request<B>,
) -> Response<ResponseBody>
where
    Ctx: Send + Sync + 'static,
    B: BodyExt + Unpin,
    B::Data: AsRef<[u8]>,
{
    let method = req.method().clone();

    // Read body
    let mut body_buf = Vec::new();
    if method == Method::POST {
        while let Some(chunk) = req.body_mut().data().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(_) => {
                    return error_response(
                        fnrpc::error::RpcErr::bad_request("body read error"),
                    );
                }
            };
            body_buf.extend_from_slice(chunk.as_ref());
        }
    }

    let path = req.uri().path().strip_prefix('/').unwrap_or("");
    let ctx = (config.ctx_from_headers)(req.headers());

    // For GET requests with JSON handlers, encode the query string as
    // the input bytes so call_bytes can deserialize it.
    let input_bytes = if method == Method::GET && body_buf.is_empty() {
        req.uri().query().unwrap_or("").as_bytes()
    } else {
        body_buf.as_slice()
    };

    if let Some(handler) = config.router.get_handler(path) {
        match handler.call_bytes(&ctx, input_bytes) {
            Ok(bytes) => raw_response(StatusCode::OK, bytes, handler.content_type()),
            Err(e) => error_response(e),
        }
    } else if method == Method::GET {
        // Subscriptions (SSE) — only on GET
        // For now, subscriptions still use the JSON Value path
        let input_raw: Value = serde_json::from_slice(input_bytes).unwrap_or(Value::Null);
        if let Some(handler) = config.router.get_sub_handler(path) {
            sse_response(handler, &ctx, input_raw)
        } else {
            not_found_response(path)
        }
    } else {
        not_found_response(path)
    }
}

/// Run a fnrpc HTTP server.
pub async fn run<Ctx>(
    config: Arc<RpcWebConfig<Ctx>>,
    addr: &str,
) -> Result<(), Box<dyn std::error::Error>>
where
    Ctx: Send + Sync + 'static,
{
    let handler = fn_service(move |req: Request<RequestExt<RequestBody>>| {
        let config = config.clone();
        async move { Ok::<Response<ResponseBody>, Infallible>(handle(&config, req).await) }
    })
    .enclosed(HttpServiceBuilder::new());

    Builder::new().bind("fnrpc-web", addr, handler)?.build().await?;

    Ok(())
}
