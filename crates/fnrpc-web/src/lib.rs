use std::convert::Infallible;
use std::sync::Arc;

use fnrpc::serializer::unpack_meta;
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
pub struct FnrpcConfig<Ctx> {
    pub router: fnrpc::router::RpcRouter<Ctx>,
    pub ctx_from_headers: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
}

/// Build a JSON response from a `Value`.
///
/// Prefer [`json_response_bytes`] when you already have serialized bytes
/// (e.g. from [`ErasedHandler::call_bytes`](fnrpc::handler::ErasedHandler::call_bytes)).
fn json_response(status: StatusCode, body: Value) -> Response<ResponseBody> {
    let mut res = if body.is_null() {
        Response::new(ResponseBody::bytes(Bytes::from_static(b"null")))
    } else {
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        Response::new(ResponseBody::bytes(bytes))
    };
    *res.status_mut() = status;
    res.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    res
}

/// Build a JSON response from pre-serialized bytes.
///
/// Bypasses the intermediate `Value` allocation that [`json_response`] requires.
/// For the `b"null"` case, uses a static `Bytes` to avoid any allocation.
fn json_response_bytes(status: StatusCode, body: Vec<u8>) -> Response<ResponseBody> {
    let mut res = if body == b"null" {
        Response::new(ResponseBody::bytes(Bytes::from_static(b"null")))
    } else {
        Response::new(ResponseBody::bytes(Bytes::from(body)))
    };
    *res.status_mut() = status;
    res.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    res
}

fn rpc_err_to_response(e: fnrpc::error::RpcErr) -> Response<ResponseBody> {
    let status = match e.code.as_str() {
        "BAD_REQUEST" => StatusCode::BAD_REQUEST,
        "NOT_FOUND" => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    // Serialize directly to bytes, bypassing Value intermediate
    let body = serde_json::to_vec(&e).unwrap_or_default();
    let mut res = Response::new(ResponseBody::bytes(Bytes::from(body)));
    *res.status_mut() = status;
    res.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    res
}

fn extract_input(query_str: &str) -> Value {
    // Fast path: scan for "input=" without allocating a parser.
    // URL-decode the value manually (handles %xx and + → space).
    let mut remaining = query_str;
    loop {
        let Some(eq_pos) = remaining.find("input=") else { break };
        let after_eq = &remaining[eq_pos + 6..];
        let end = after_eq.find('&').unwrap_or(after_eq.len());
        let raw = &after_eq[..end];
        if !raw.is_empty() {
            // Percent-decode the raw value
            let decoded: String = percent_decode(raw);
            if let Ok(val) = serde_json::from_str(&decoded) {
                return val;
            }
        }
        remaining = &after_eq[end..];
        if remaining.is_empty() {
            break;
        }
        if end < after_eq.len() {
            // skip past '&'
            remaining = &remaining[1..];
        }
    }
    Value::Null
}

/// Minimal percent-decoding for query string values.
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

fn input_from_body(buf: &[u8]) -> Value {
    serde_json::from_slice::<Value>(buf).unwrap_or(Value::Null)
}

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

/// Handle a single fnrpc HTTP request.
///
/// Uses [`get_handler`](RpcRouter::get_handler) + direct handler call —
/// bypasses the middleware stack and saves one `Box::pin` allocation
/// vs [`dispatch_send`](RpcRouter::dispatch_send).
///
/// Uses [`call_bytes`](fnrpc::handler::ErasedHandler::call_bytes) to
/// avoid the intermediate `Value` allocation.
pub async fn handle<Ctx, B>(
    config: &FnrpcConfig<Ctx>,
    mut req: Request<B>,
) -> Response<ResponseBody>
where
    Ctx: Send + Sync + 'static,
    B: BodyExt + Unpin,
    B::Data: AsRef<[u8]>,
{
    let method = req.method().clone();

    // Body read first (mutable borrow of req).  Path/query borrows
    // happen after, so there's no borrow conflict.
    let mut body_buf = Vec::new();
    if method == Method::POST {
        while let Some(chunk) = req.body_mut().data().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(_) => {
                    return json_response(
                        StatusCode::BAD_REQUEST,
                        serde_json::json!({"code":"BAD_REQUEST","message":"body read error"}),
                    );
                }
            };
            body_buf.extend_from_slice(chunk.as_ref());
        }
    }

    // Immutable borrows of req (no alloc — borrowed &str)
    let path = req.uri().path().strip_prefix('/').unwrap_or("");
    let input_raw = if method == Method::POST {
        input_from_body(&body_buf)
    } else {
        extract_input(req.uri().query().unwrap_or(""))
    };
    let input = unpack_meta(input_raw);
    let ctx = (config.ctx_from_headers)(req.headers());

    // Radix-tree lookup — no path clone needed
    if let Some(handler) = config.router.get_handler(path) {
        match handler.call_bytes(&ctx, input) {
            Ok(bytes) => json_response_bytes(StatusCode::OK, bytes),
            Err(e) => rpc_err_to_response(e),
        }
    } else if let Some(handler) = config.router.get_sub_handler(path) {
        sse_response(handler, &ctx, input)
    } else {
        json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": format!("unknown path: {path}") }),
        )
    }
}

/// Run a fnrpc HTTP server.
///
/// Uses bare `xitca-http` + `xitca-server` with no framework layer.
pub async fn run<Ctx>(
    config: Arc<FnrpcConfig<Ctx>>,
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
