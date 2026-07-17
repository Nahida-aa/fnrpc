use dhat::{HeapStats, Profiler};
use serde_json::Value;
use xitca_web::App;
use xitca_web::WebContext;
use xitca_web::body::{BodyExt, RequestBody, ResponseBody};
use xitca_web::http::{Method, RequestExt, StatusCode, WebResponse};
use xitca_web::http::header::{CONTENT_TYPE, HeaderValue};
use xitca_web::route::{get, post};
use xitca_web::service::{Service, fn_service};

/// Raw noop — no Content-Type header, plain text body.
/// Matches the original xitca-web baseline (177B/3blks).
async fn handler_noop_raw(_ctx: WebContext<'_, ()>) -> Result<WebResponse, xitca_web::error::Error> {
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::bytes(xitca_web::bytes::Bytes::from_static(b"ok")))
        .unwrap())
}

/// JSON noop — returns `null` with Content-Type: application/json.
async fn handler_noop_json(_ctx: WebContext<'_, ()>) -> Result<WebResponse, xitca_web::error::Error> {
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(ResponseBody::bytes(xitca_web::bytes::Bytes::from_static(b"null")))
        .unwrap())
}

/// Echo via POST body — reads body, deserializes, serializes back with JSON header.
async fn handler_echo_post(mut ctx: WebContext<'_, ()>) -> Result<WebResponse, xitca_web::error::Error> {
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
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(ResponseBody::bytes(bytes))
        .unwrap())
}

/// Echo via GET query param — reads `input` from query string, returns JSON.
async fn handler_echo_get(ctx: WebContext<'_, ()>) -> Result<WebResponse, xitca_web::error::Error> {
    let uri: xitca_web::handler::uri::UriRef<'_> = ctx.extract().await.unwrap();
    let query_str = uri.query().unwrap_or("");
    let val: Value = query_str
        .split('&')
        .find_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?;
            let val = parts.next()?;
            if key == "input" {
                let decoded = urlencoding_decode(val);
                serde_json::from_str(&decoded).ok()
            } else {
                None
            }
        })
        .unwrap_or(Value::Null);
    let bytes = serde_json::to_vec(&val).unwrap_or_default();
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(ResponseBody::bytes(bytes))
        .unwrap())
}

/// Minimal percent-decoding for query values.
fn urlencoding_decode(s: &str) -> String {
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
        .at("/noop-raw", get(fn_service(handler_noop_raw)))
        .at("/noop-json", get(fn_service(handler_noop_json)))
        .at("/echo", post(fn_service(handler_echo_post)))
        .at("/echo-get", get(fn_service(handler_echo_get)));
    let svc = app.finish().call(()).await.unwrap();

    // — noop_raw (GET, no header) —
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let req = make_get_req("/noop-raw");
        let _ = svc.call(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("xitca-web/noop_raw: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-xitca-web-noop-raw.json");

    // — noop_json (GET, JSON header) —
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let req = make_get_req("/noop-json");
        let _ = svc.call(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("xitca-web/noop_json: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-xitca-web-noop-json.json");

    // — echo (GET) —
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let req = make_get_req(r#"/echo-get?input=%22hello%22"#);
        let _ = svc.call(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("xitca-web/echo_get: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-xitca-web-echo-get.json");

    // — echo (POST) —
    let body_data = br#""hello""#;
    let _p = Profiler::builder().file_name("benches/target/dhat-heap.json").build();
    for _ in 0..n {
        let body = RequestBody::from(xitca_web::bytes::Bytes::copy_from_slice(body_data));
        let req = make_post_req("/echo", body);
        let _ = svc.call(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!("xitca-web/echo_post: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
    let _ = std::fs::copy("./benches/target/dhat-heap.json", "./benches/target/dhat-xitca-web-echo-post.json");
}
