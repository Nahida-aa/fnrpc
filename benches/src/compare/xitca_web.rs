use dhat::{HeapStats, Profiler};
use serde_json::Value;
use xitca_web::App;
use xitca_web::WebContext;
use xitca_web::body::{BodyExt, RequestBody, ResponseBody};
use xitca_web::http::{Method, RequestExt, StatusCode, WebResponse};
use xitca_web::route::{get, post};
use xitca_web::service::{Service, fn_service};

async fn handler_noop(_ctx: WebContext<'_, ()>) -> Result<WebResponse, xitca_web::error::Error> {
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::bytes(xitca_web::bytes::Bytes::from_static(b"ok")))
        .unwrap())
}

async fn handler_echo(mut ctx: WebContext<'_, ()>) -> Result<WebResponse, xitca_web::error::Error> {
    let body = ctx.body_get_mut();
    let mut buf = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.map_err(|e| xitca_web::error::Error::from(e))?;
        buf.extend_from_slice(chunk.as_ref());
    }
    let val: Value = serde_json::from_slice(&buf).unwrap_or(Value::Null);
    let bytes = serde_json::to_vec(&val).unwrap_or_default();
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::bytes(bytes))
        .unwrap())
}

fn make_post_req(uri: &str, body: RequestBody) -> http::Request<RequestExt<RequestBody>> {
    let req_ext: RequestExt<RequestBody> = <RequestExt<RequestBody>>::default().replace_body(body).0;
    http::Request::builder()
        .method(Method::POST)
        .uri(uri)
        .body(req_ext)
        .unwrap()
}

fn make_get_req(uri: &str) -> http::Request<RequestExt<RequestBody>> {
    let req_ext: RequestExt<RequestBody> = RequestExt::default();
    http::Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(req_ext)
        .unwrap()
}

pub(crate) async fn bench(n: usize) {
    let app = App::new()
        .at("/", get(fn_service(handler_noop)))
        .at("/echo", post(fn_service(handler_echo)));
    let svc = app.finish().call(()).await.unwrap();

    // — noop (GET) —
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = make_get_req("/");
        let _ = svc.call(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("xitca-web/noop: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("dhat-heap.json", "dhat-xitca-web-noop.json");

    // — echo (POST) —
    let body_data = br#""hello""#;
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let body = RequestBody::from(xitca_web::bytes::Bytes::copy_from_slice(body_data));
        let req = make_post_req("/echo", body);
        let _ = svc.call(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("xitca-web/echo: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("dhat-heap.json", "dhat-xitca-web-echo.json");
}
