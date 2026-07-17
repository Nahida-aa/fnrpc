use actix_web::{App, HttpResponse, test};
use dhat::{HeapStats, Profiler};

pub(crate) async fn run(label: &str, n: usize) {
    let app = test::init_service(
        App::new().route("/", actix_web::web::get().to(|| async { HttpResponse::Ok() }))
    ).await;

    let _p = Profiler::new_heap();
    for _ in 0..n {
        let req = test::TestRequest::get().uri("/").to_request();
        let _ = test::call_service(&app, req).await;
    }
    let s = HeapStats::get();
    eprintln!("{label}/noop: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        s.total_bytes, s.total_blocks,
        s.total_bytes as f64 / n as f64, s.total_blocks as f64 / n as f64);
    drop(_p);
}
