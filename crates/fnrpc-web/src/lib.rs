use std::borrow::Cow;
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
fn raw_response(status: StatusCode, body: Cow<'static, [u8]>, content_type: Option<&'static str>) -> Response<ResponseBody> {
    let mut builder = Response::builder().status(status);
    if let Some(ct) = content_type {
        builder = builder.header(CONTENT_TYPE, HeaderValue::from_static(ct));
    }
    let resp_body = match body {
        Cow::Borrowed(b"null") => ResponseBody::bytes(Bytes::from_static(b"null")),
        Cow::Borrowed(slice) => ResponseBody::bytes(Bytes::from_static(slice)),
        Cow::Owned(ref vec) if vec == b"null" => ResponseBody::bytes(Bytes::from_static(b"null")),
        Cow::Owned(vec) => ResponseBody::bytes(Bytes::from(vec)),
    };
    builder.body(resp_body).unwrap()
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

// ── Input parsing (JSON path) ─────────────────────────────

/// Extract the `input` parameter from a URL query string.
fn extract_input(query_str: &str) -> Value {
    let mut remaining = query_str;
    loop {
        let Some(eq_pos) = remaining.find("input=") else { break };
        let after_eq = &remaining[eq_pos + 6..];
        let end = after_eq.find('&').unwrap_or(after_eq.len());
        let raw = &after_eq[..end];
        if !raw.is_empty() {
            let decoded: String = percent_decode(raw);
            if let Ok(val) = serde_json::from_str(&decoded) {
                return val;
            }
        }
        remaining = &after_eq[end..];
        if remaining.is_empty() { break; }
        if end < after_eq.len() { remaining = &remaining[1..]; }
    }
    Value::Null
}

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

    if let Some(handler) = config.router.get_handler(path) {
        // Data source determined by handler's method():
        // "GET" → input from query string (parse "input" param for JSON,
        //          raw bytes for non-JSON)
        // "POST" → input from body
        let is_get = handler.method() == "GET";
        let is_json = handler.content_type() == Some("application/json");
        let result = if is_get && is_json {
            let input_val = req.uri().query().map(|q| extract_input(q)).unwrap_or(Value::Null);
            handler.call_value(&ctx, input_val)
        } else if is_get {
            let query_bytes = req.uri().query().unwrap_or("").as_bytes().to_vec();
            body_buf = query_bytes;
            handler.call_bytes(&ctx, body_buf.as_slice())
        } else {
            handler.call_bytes(&ctx, body_buf.as_slice())
        };

        match result {
            Ok(bytes) => raw_response(StatusCode::OK, bytes, handler.content_type()),
            Err(e) => error_response(e),
        }
    } else if method == Method::GET {
        // Subscriptions (SSE) — only on GET
        let input_raw: Value = req.uri().query()
            .and_then(|q| serde_json::from_str(q).ok())
            .unwrap_or(Value::Null);
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
