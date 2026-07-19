use dhat::{HeapStats, Profiler};
use serde_json::Value;
use xitca_web::App;
use xitca_web::WebContext;
use xitca_web::body::{BodyExt, RequestBody, ResponseBody};
use xitca_web::handler::handler_service;
use xitca_web::http::header::{CONTENT_TYPE, HeaderValue};
use xitca_web::http::{Method, RequestExt, StatusCode, WebResponse};
use xitca_web::route::{get, post};
use xitca_web::service::{Service, ServiceExt, fn_service};
use xitca_service::ready::ReadyService;

/// Ping — returns "pong" with JSON content type.
async fn handler_ping(
    _ctx: WebContext<'_, ()>,
) -> Result<WebResponse, xitca_web::error::Error> {
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(ResponseBody::bytes(xitca_web::bytes::Bytes::from_static(b"\"pong\"")))
        .unwrap())
}

/// Raw noop — no Content-Type header, plain text body.
async fn handler_noop_raw(
    _ctx: WebContext<'_, ()>,
) -> Result<WebResponse, xitca_web::error::Error> {
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::bytes(xitca_web::bytes::Bytes::from_static(
            b"ok",
        )))
        .unwrap())
}

/// JSON noop — returns `null` with Content-Type: application/json.
async fn handler_noop_json(
    _ctx: WebContext<'_, ()>,
) -> Result<WebResponse, xitca_web::error::Error> {
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(ResponseBody::bytes(xitca_web::bytes::Bytes::from_static(
            b"null",
        )))
        .unwrap())
}

/// Echo via POST body — reads body, deserializes, serializes back with JSON header.
async fn handler_echo_post(
    mut ctx: WebContext<'_, ()>,
) -> Result<WebResponse, xitca_web::error::Error> {
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
    let req_ext: RequestExt<RequestBody> =
        <RequestExt<RequestBody>>::default().replace_body(body).0;
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
        .at("/ping", get(fn_service(handler_ping)))
        .at("/noop-raw", get(fn_service(handler_noop_raw)))
        .at("/noop-json", get(fn_service(handler_noop_json)))
        .at("/echo_post", post(fn_service(handler_echo_post)))
        .at("/echo-get", get(fn_service(handler_echo_get)));
    let svc = app.finish().call(()).await.unwrap();

    // Pre-parse URIs outside profiler
    let uri_raw: http::Uri = "/noop-raw".parse().unwrap();
    let uri_json: http::Uri = "/noop-json".parse().unwrap();
    let uri_echo_get: http::Uri = r#"/echo-get?input=%22hello%22"#.parse().unwrap();
    let uri_echo: http::Uri = "/echo".parse().unwrap();
    let body_data: Vec<u8> = br#""hello""#.to_vec();

    fn build_get(uri: &http::Uri) -> http::Request<RequestExt<RequestBody>> {
        let req_ext: RequestExt<RequestBody> = RequestExt::default();
        http::Request::builder()
            .method(Method::GET)
            .uri(uri.clone())
            .body(req_ext)
            .unwrap()
    }

    fn build_post(uri: &http::Uri, body: &[u8]) -> http::Request<RequestExt<RequestBody>> {
        let req_ext: RequestExt<RequestBody> = <RequestExt<RequestBody>>::default()
            .replace_body(RequestBody::from(xitca_web::bytes::Bytes::copy_from_slice(
                body,
            )))
            .0;
        http::Request::builder()
            .method(Method::POST)
            .uri(uri.clone())
            .body(req_ext)
            .unwrap()
    }

    // — noop_raw —
    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for _ in 0..n {
        let _ = svc.call(build_get(&uri_raw)).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!(
        "xitca-web/noop_raw: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
    let _ = std::fs::copy(
        "./benches/target/dhat-heap.json",
        "./benches/target/dhat-xitca-web-noop-raw.json",
    );

    // — noop_json —
    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for _ in 0..n {
        let _ = svc.call(build_get(&uri_json)).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!(
        "xitca-web/noop_json: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
    let _ = std::fs::copy(
        "./benches/target/dhat-heap.json",
        "./benches/target/dhat-xitca-web-noop-json.json",
    );

    // — echo_get —
    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for _ in 0..n {
        let _ = svc.call(build_get(&uri_echo_get)).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!(
        "xitca-web/echo_get: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
    let _ = std::fs::copy(
        "./benches/target/dhat-heap.json",
        "./benches/target/dhat-xitca-web-echo-get.json",
    );

    // — echo_post —
    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for _ in 0..n {
        let _ = svc.call(build_post(&uri_echo, &body_data)).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!(
        "xitca-web/echo_post: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
    let _ = std::fs::copy(
        "./benches/target/dhat-heap.json",
        "./benches/target/dhat-xitca-web-echo-post.json",
    );
}

// ── Benchmark with no-op middleware ──────────────────────
// Uses xitca's stable `enclosed` API with a concrete struct,
// NOT the nightly-only `enclosed_fn`.

/// No-op middleware — zero-size struct, no inner service state.
/// Uses `enclosed` API (stable Rust), not `enclosed_fn` (nightly).
#[derive(Clone, Copy)]
struct XitcaNoopMw;

impl<S, E> Service<Result<S, E>> for XitcaNoopMw {
    type Response = XitcaNoopMwService<S>;
    type Error = E;
    async fn call(&self, res: Result<S, E>) -> Result<Self::Response, Self::Error> {
        res.map(XitcaNoopMwService)
    }
}

struct XitcaNoopMwService<S>(S);

impl<S, Req> Service<Req> for XitcaNoopMwService<S>
where
    S: Service<Req>,
{
    type Response = S::Response;
    type Error = S::Error;
    async fn call(&self, req: Req) -> Result<Self::Response, Self::Error> {
        self.0.call(req).await
    }
}

impl<S> ReadyService for XitcaNoopMwService<S> {
    type Ready = ();
    async fn ready(&self) -> Self::Ready {}
}

pub(crate) async fn bench_mw(n: usize) {
    // xitca-web middleware on stable: use `enclosed` with a zero-size struct.
    let app = App::new()
        .at(
            "/echo-get",
            get(fn_service(handler_echo_get)),
        )
        .enclosed(XitcaNoopMw);
    let svc = app.finish().call(()).await.unwrap();
    let uri_echo_get: http::Uri = r#"/echo-get?input=%22hello%22"#.parse().unwrap();

    fn build_get(uri: &http::Uri) -> http::Request<RequestExt<RequestBody>> {
        let req_ext: RequestExt<RequestBody> = RequestExt::default();
        http::Request::builder()
            .method(Method::GET)
            .uri(uri.clone())
            .body(req_ext)
            .unwrap()
    }

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for _ in 0..n {
        let _ = svc.call(build_get(&uri_echo_get)).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!(
        "xitca-web/echo_get_mw: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
    let _ = std::fs::copy(
        "./benches/target/dhat-heap.json",
        "./benches/target/dhat-xitca-web-echo-get-mw.json",
    );
}
