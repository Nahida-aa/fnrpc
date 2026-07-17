use std::convert::Infallible;
use std::sync::Arc;

use fnrpc::serializer::unpack_meta;
use futures::StreamExt;
use serde_json::Value;
use xitca_http::body::{BodyExt, RequestBody, ResponseBody, StreamDataBody};
use xitca_http::bytes::Bytes;
use xitca_http::http::{
    header::CONTENT_TYPE, HeaderMap, Method, Request, RequestExt, Response, StatusCode,
};
use xitca_http::HttpServiceBuilder;
use xitca_service::{fn_service, ServiceExt};
use xitca_server::Builder;

/// Shared state for the fnrpc standalone server.
pub struct FnrpcConfig<Ctx> {
    pub router: fnrpc::router::RpcRouter<Ctx>,
    pub ctx_from_headers: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
}

fn json_response(status: StatusCode, body: Value) -> Response<ResponseBody> {
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    let mut res = Response::new(ResponseBody::bytes(bytes));
    *res.status_mut() = status;
    res.headers_mut()
        .insert(CONTENT_TYPE, "application/json".parse().unwrap());
    res
}

fn rpc_err_to_response(e: fnrpc::error::RpcErr) -> Response<ResponseBody> {
    let status = match e.code.as_str() {
        "BAD_REQUEST" => StatusCode::BAD_REQUEST,
        "NOT_FOUND" => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    json_response(status, serde_json::to_value(e).unwrap_or_default())
}

fn extract_input(query_str: &str) -> Value {
    let raw = url::form_urlencoded::parse(query_str.as_bytes())
        .find(|(k, _)| k == "input")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| "null".to_string());
    serde_json::from_str(&raw).unwrap_or(Value::Null)
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
pub async fn handle<Ctx>(
    config: &FnrpcConfig<Ctx>,
    mut req: Request<RequestExt<RequestBody>>,
) -> Response<ResponseBody>
where
    Ctx: Send + Sync + 'static,
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
    let input = unpack_meta(&input_raw);
    let ctx = (config.ctx_from_headers)(req.headers());

    // Radix-tree lookup — no path clone needed
    if let Some(handler) = config.router.get_handler(path) {
        match handler.call(&ctx, input).await {
            Ok(val) => json_response(StatusCode::OK, val),
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
