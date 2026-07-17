use std::sync::Arc;

use fnrpc::serializer::unpack_meta;
use futures::StreamExt;
use serde_json::Value;
use xitca_web::body::{ResponseBody, StreamDataBody};
use xitca_web::bytes::Bytes;
use xitca_web::error::Error;
use xitca_web::handler::json::Json;
use xitca_web::http::{
    header::CONTENT_TYPE, StatusCode, WebResponse,
};
use xitca_web::WebContext;

/// Shared state for the fnrpc xitca-web handler.
pub struct FnrpcState<Ctx> {
    pub router: fnrpc::router::RpcRouter<Ctx>,
    pub ctx_from_headers: Arc<dyn Fn(&xitca_web::http::HeaderMap) -> Ctx + Send + Sync>,
}

fn json_response(status: StatusCode, body: Value) -> WebResponse {
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    WebResponse::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json")
        .body(ResponseBody::bytes(bytes))
        .unwrap()
}

fn rpc_err_to_response(e: fnrpc::error::RpcErr) -> WebResponse {
    let status = match e.code.as_str() {
        "BAD_REQUEST" => StatusCode::BAD_REQUEST,
        "NOT_FOUND" => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    json_response(status, serde_json::to_value(e).unwrap_or_default())
}

fn extract_input(query_str: &str) -> Value {
    for (key, val) in url::form_urlencoded::parse(query_str.as_bytes()) {
        if key.as_ref() == "input" {
            return serde_json::from_str(val.as_ref()).unwrap_or(Value::Null);
        }
    }
    Value::Null
}

/// Dispatch a fnrpc request.
///
/// Uses [`get_handler`](RpcRouter::get_handler) + direct handler call —
/// bypasses the middleware stack and saves one `Box::pin` allocation
/// vs [`dispatch_send`](RpcRouter::dispatch_send).
///
/// Use with [`fn_service`](xitca_web::service::fn_service):
/// ```ignore
/// .at("/{*path}", get(fn_service(dispatch::<MyCtx>)).post(fn_service(dispatch::<MyCtx>)))
/// ```
pub async fn dispatch<Ctx: Send + Sync + 'static>(
    ctx: WebContext<'_, Arc<FnrpcState<Ctx>>>,
) -> Result<WebResponse, Error> {
    let state: xitca_web::handler::state::StateRef<'_, Arc<FnrpcState<Ctx>>> =
        ctx.extract().await?;
    let uri: xitca_web::handler::uri::UriRef<'_> = ctx.extract().await?;
    let headers: &xitca_web::http::HeaderMap = ctx.extract().await?;

    let ctx_value = (state.ctx_from_headers)(headers);

    let path = uri.path().strip_prefix('/').unwrap_or("");
    let query_str = uri.query().unwrap_or("");

    // Body or query extraction — no HashMap allocation
    let input_raw = match ctx.extract::<Json<Value>>().await {
        Ok(body) => body.0,
        Err(_) => extract_input(query_str),
    };
    let input = unpack_meta(&input_raw);

    // Fast path: direct handler call
    if let Some(handler) = state.router.get_handler(path) {
        match handler.call(&ctx_value, input).await {
            Ok(val) => Ok(json_response(StatusCode::OK, val)),
            Err(e) => Ok(rpc_err_to_response(e)),
        }
    } else if let Some(handler) = state.router.get_sub_handler(path) {
        let stream = handler.call(&ctx_value, input);
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
            Ok::<_, Error>(Bytes::from(data))
        });
        let res = WebResponse::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .body(ResponseBody::boxed(StreamDataBody::new(sse)))
            .unwrap();
        Ok(res)
    } else {
        Ok(json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": format!("unknown path: {path}") }),
        ))
    }
}
