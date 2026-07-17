use dhat::{HeapStats, Profiler};
use xitca_web::App;
use xitca_web::WebContext;
use xitca_web::body::ResponseBody;
use xitca_web::bytes::Bytes;
use xitca_web::error::Error;
use xitca_web::http::request;
use xitca_web::http::{StatusCode, WebResponse};
use xitca_web::route::get;
use xitca_web::service::{Service, fn_service};

async fn handler_noop(_ctx: WebContext<'_, ()>) -> Result<WebResponse, Error> {
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::bytes(Bytes::from_static(b"ok")))
        .unwrap())
}

pub(crate) async fn run(label: &str, n: usize) {
    let app = App::new().at("/", get(fn_service(handler_noop)));
    let svc = app.finish().call(()).await.unwrap();

    // — plain/noop —
    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = request::Builder::default()
            .uri("/")
            .body(Default::default())
            .unwrap();
        let _ = svc.call(req).await.unwrap();
    }
    let s = HeapStats::get();
    eprintln!(
        "{label}/noop: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}
