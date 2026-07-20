use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouterBuilder;
use fnrpc_web::App;
use std::pin::Pin;

// ── Test handlers ─────────────────────────────────────

struct Greet;
impl RpcFn<()> for Greet {
    type Input = String;
    type Output = String;
    const KEY: &'static str = "greet";
    fn exec(
        _ctx: &(),
        input: String,
    ) -> Pin<Box<dyn futures::Future<Output = Result<String, RpcErr>> + Send + '_>> {
        Box::pin(async move { Ok(format!("Hello {input}!")) })
    }
}

struct Echo;
impl RpcFn<()> for Echo {
    type Input = String;
    type Output = String;
    const KEY: &'static str = "echo";
    fn exec(
        _ctx: &(),
        input: String,
    ) -> Pin<Box<dyn futures::Future<Output = Result<String, RpcErr>> + Send + '_>> {
        Box::pin(async move { Ok(input) })
    }
}

// ── Request building ─────────────────────────────────

fn get_req(path: &str) -> xitca_http::http::Request<xitca_http::http::RequestExt<xitca_http::body::RequestBody>> {
    xitca_http::http::Request::builder()
        .method(xitca_http::http::Method::GET)
        .uri(path)
        .body(xitca_http::http::RequestExt::default())
        .unwrap()
}

fn post_req(path: &str, body: &[u8]) -> xitca_http::http::Request<xitca_http::http::RequestExt<xitca_http::body::RequestBody>> {
    use xitca_http::body::RequestBody;
    let req_ext: xitca_http::http::RequestExt<RequestBody> =
        xitca_http::http::RequestExt::default()
            .map_body(|_: RequestBody| xitca_http::bytes::Bytes::copy_from_slice(body).into());
    let mut req = xitca_http::http::Request::new(req_ext);
    *req.method_mut() = xitca_http::http::Method::POST;
    *req.uri_mut() = path.parse().unwrap();
    req
}

// ── Single router tests ──────────────────────────────

#[tokio::test]
async fn test_single_get() {
    let router = RpcRouterBuilder::<()>::new().route_fn(Greet).build();
    let app = App::new(router, |_| ());
    let res = app.call(get_req("/greet?input=%22world%22")).await;
    assert_eq!(res.status(), xitca_http::http::StatusCode::OK);
}

#[tokio::test]
async fn test_single_post() {
    let router = RpcRouterBuilder::<()>::new().route_fn(Echo).build();
    let app = App::new(router, |_| ());
    let res = app.call(post_req("/echo", br#""hello""#)).await;
    assert_eq!(res.status(), xitca_http::http::StatusCode::OK);
}

#[tokio::test]
async fn test_single_not_found() {
    let router = RpcRouterBuilder::<()>::new().route_fn(Greet).build();
    let app = App::new(router, |_| ());
    let res = app.call(get_req("/nonexistent")).await;
    assert_eq!(res.status(), xitca_http::http::StatusCode::NOT_FOUND);
}

// ── Subscribe (SSE) tests ─────────────────────────────

#[fnrpc::rpc_subscribe]
fn tick(interval_ms: u64) -> impl futures::Stream<Item = u64> {
    futures::stream::unfold(0u64, move |count| async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
        Some((count, count + 1))
    })
}

#[tokio::test]
async fn test_subscribe_sse() {
    use xitca_http::body::BodyExt;

    let router = RpcRouterBuilder::<()>::new().subscribe(tick).build();
    let app = App::new(router, |_| ());
    // Use 1ms interval so the stream yields quickly, read first event
    let res = app.call(get_req("/tick?input=1")).await;
    assert_eq!(res.status(), xitca_http::http::StatusCode::OK);
    assert_eq!(
        res.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    // Read the body stream and verify SSE data
    let mut body = res.into_body();
    // xitca-http BodyExt::data() returns the next data frame
    if let Some(Ok(chunk)) = body.data().await {
        let s = String::from_utf8_lossy(&chunk);
        assert!(s.starts_with("data: "), "expected SSE data frame, got: {s:?}");
    }
}

#[tokio::test]
async fn test_subscribe_not_found() {
    let router = RpcRouterBuilder::<()>::new().subscribe(tick).build();
    let app = App::new(router, |_| ());
    let res = app.call(get_req("/nonexistent")).await;
    assert_eq!(res.status(), xitca_http::http::StatusCode::NOT_FOUND);
}

// ── Multi router tests ───────────────────────────────

#[tokio::test]
async fn test_multi_rpc_match() {
    let router = RpcRouterBuilder::<()>::new().route_fn(Greet).build();
    let app = App::build(|_| ())
        .rpc("/api/{*path}", router)
        .rpc("/echo", RpcRouterBuilder::<()>::new().route_fn(Echo).build());
    let res = app.call(get_req("/echo?input=%22hi%22")).await;
    assert_eq!(res.status(), xitca_http::http::StatusCode::OK);
}

#[tokio::test]
async fn test_multi_rpc_subpath() {
    let router = RpcRouterBuilder::<()>::new().route_fn(Greet).build();
    let app = App::build(|_| ())
        .rpc("/api/{*path}", router);
    // Note: {*path} matching passes the matched segment to InnerService.
    // For now, test with exact path.
    let res = app.call(get_req("/api/greet?input=%22world%22")).await;
    assert_eq!(res.status(), xitca_http::http::StatusCode::OK);
}

#[tokio::test]
async fn test_multi_not_found() {
    let router = RpcRouterBuilder::<()>::new().route_fn(Greet).build();
    let app = App::build(|_| ())
        .rpc("/api/{*path}", router);
    let res = app.call(get_req("/nonexistent")).await;
    assert_eq!(res.status(), xitca_http::http::StatusCode::NOT_FOUND);
}

// ── Static file tests (requires --features file) ─────

#[cfg(feature = "file")]
#[tokio::test]
async fn test_static_file_served() {
    use std::io::Write;
    // Create a temp file
    let dir = std::env::temp_dir().join("fnrpc_web_test");
    let _ = std::fs::create_dir_all(&dir);
    let file_path = dir.join("test.txt");
    let mut f = std::fs::File::create(&file_path).unwrap();
    f.write_all(b"hello static").unwrap();

    let app = App::build(|_| ())
        .static_dir("/static", &dir);
    let res = app.call(get_req("/static/test.txt")).await;
    assert_eq!(res.status(), xitca_http::http::StatusCode::OK);

    // Cleanup
    let _ = std::fs::remove_file(&file_path);
}

#[cfg(feature = "file")]
#[tokio::test]
async fn test_static_file_not_found() {
    let dir = std::env::temp_dir().join("fnrpc_web_test");
    let _ = std::fs::create_dir_all(&dir);

    let app = App::build(|_| ())
        .static_dir("/static", &dir);
    let res = app.call(get_req("/static/nonexistent.txt")).await;
    assert_eq!(res.status(), xitca_http::http::StatusCode::NOT_FOUND);
}
