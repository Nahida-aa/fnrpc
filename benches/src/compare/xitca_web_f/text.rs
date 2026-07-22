use crate::compare::utils::prebuild_get;
use dhat::{HeapStats, Profiler};
use xitca_web::App;
use xitca_web::WebContext;
use xitca_web::body::RequestBody;
use xitca_web::http::{Method, Request, RequestExt, WebResponse};
use xitca_web::route::get;
use xitca_web::service::ServiceExt;
use xitca_web::service::{Service, fn_service};

/// Raw noop — no Content-Type header, plain text body.
async fn text(_ctx: WebContext<'_, ()>) -> Result<WebResponse, xitca_web::error::Error> {
    Ok(WebResponse::builder()
        .status(xitca_web::http::StatusCode::OK)
        .body(xitca_web::body::ResponseBody::bytes(
            xitca_web::bytes::Bytes::from_static(b"ok"),
        ))
        .unwrap())
}

pub async fn bench_text(n: usize) {
    let app = App::new().at("/text", get(fn_service(text))).finish();
    let svc = app.call(()).await.unwrap();
    let reqs = prebuild_get("/text", n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = svc.call(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!(
        "xitca-web-f/text: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}
