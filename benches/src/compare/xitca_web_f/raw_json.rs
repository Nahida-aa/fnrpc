use dhat::{HeapStats, Profiler};
use http::{HeaderValue, header::CONTENT_TYPE};
use xitca_http::ResponseBody;
use xitca_web::bytes::Bytes;
use xitca_web::http::StatusCode;
use xitca_web::route::get;
use xitca_web::service::fn_service;
use xitca_web::service::{Service, ServiceExt};
use xitca_web::{App, WebContext, http::WebResponse};

use crate::compare::utils::prebuild_get;

/// JSON noop — returns `null` with Content-Type: application/json.
pub async fn null_json(_ctx: WebContext<'_, ()>) -> Result<WebResponse, xitca_web::error::Error> {
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(ResponseBody::bytes(Bytes::from_static(b"null")))
        .unwrap())
}

/// Fair comparison point vs fnrpc-web's `null_json`: same body (`b"null"`)
/// and same `Content-Type: application/json` header, served by xitca's native
/// handler (no fnrpc `RpcOutput` wrapper).
pub async fn bench_null_json(n: usize) {
    let app = App::new()
        .at("/null_json", get(fn_service(null_json)))
        .finish();
    let svc = app.call(()).await.unwrap();
    let reqs = prebuild_get("/null_json", n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = svc.call(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!(
        "xitca-web/null_json: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}
