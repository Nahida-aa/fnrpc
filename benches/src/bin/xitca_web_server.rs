//! Standalone xitca-web server for latency benchmarking.
use std::net::TcpListener;

use xitca_web::App;
use xitca_web::body::ResponseBody;
use xitca_web::http::{StatusCode, WebResponse};
use xitca_web::http::header::{CONTENT_TYPE, HeaderValue};
use xitca_web::route::get;
use xitca_web::service::fn_service;
use xitca_web::WebContext;

async fn handler_noop_json(_ctx: WebContext<'_, ()>) -> Result<WebResponse, xitca_web::error::Error> {
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(ResponseBody::bytes(xitca_web::bytes::Bytes::from_static(b"null")))
        .unwrap())
}

async fn handler_noop_raw(_ctx: WebContext<'_, ()>) -> Result<WebResponse, xitca_web::error::Error> {
    Ok(WebResponse::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::bytes(xitca_web::bytes::Bytes::from_static(b"ok")))
        .unwrap())
}

fn find_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn main() {
    let port: u16 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
        eprintln!("Usage: xitca_web_server <port>");
        std::process::exit(1);
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        eprintln!("Starting xitca-web server on :{port}");

        App::new()
            .at("/noop-json", get(fn_service(handler_noop_json)))
            .at("/noop-raw", get(fn_service(handler_noop_raw)))
            .serve()
            .bind(format!("0.0.0.0:{port}"))
            .unwrap()
            .run()
            .await
            .unwrap();
    });
}
