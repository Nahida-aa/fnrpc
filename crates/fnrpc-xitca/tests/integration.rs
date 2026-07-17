use std::sync::Arc;

use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc_xitca::{dispatch, FnrpcState};
use xitca_web::body::RequestBody;
use xitca_web::http::request;
use xitca_web::http::{Method, RequestExt, StatusCode, WebRequest};
use xitca_web::route::get;
use xitca_web::service::{fn_service, Service};
use xitca_web::App;
use xitca_unsafe_collection::futures::NowOrPanic;

fn make_post_req(uri: &str, body: &[u8]) -> WebRequest {
    let request_body: RequestBody = xitca_web::bytes::Bytes::copy_from_slice(body).into();
    let mut req = WebRequest::new(
        RequestExt::default().map_body(|_: RequestBody| request_body),
    );
    *req.method_mut() = Method::POST;
    *req.uri_mut() = uri.parse().unwrap();
    req.headers_mut()
        .insert("content-type", "application/json".parse().unwrap());
    req
}

#[tokio::test]
async fn test_query_get() {
    struct Greet;

    impl RpcFn<()> for Greet {
        type Input = String;
        type Output = String;
        const NAME: &'static str = "greet";

        async fn exec(_ctx: &(), input: String) -> Result<String, RpcErr> {
            Ok(format!("Hello {input}!"))
        }
    }

    let router = fnrpc::router::RpcRouterBuilder::<()>::new().query(Greet).build();
    let state = Arc::new(FnrpcState {
        router,
        ctx_from_headers: Arc::new(|_| ()),
    });

    let app = App::new()
        .with_state(state)
        .at(
            "/{*path}",
            get(fn_service(dispatch::<()>)).post(fn_service(dispatch::<()>)),
        );

    let app_service = app.finish().call(()).now_or_panic().unwrap();

    let req = request::Builder::default()
        .uri("/greet?input=%22world%22")
        .body(Default::default())
        .unwrap();
    let res = app_service.call(req).now_or_panic().unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_query_post() {
    struct Add;

    impl RpcFn<()> for Add {
        type Input = (i32, i32);
        type Output = i32;
        const NAME: &'static str = "add";

        async fn exec(_ctx: &(), input: (i32, i32)) -> Result<i32, RpcErr> {
            Ok(input.0 + input.1)
        }
    }

    let router = fnrpc::router::RpcRouterBuilder::<()>::new().query(Add).build();
    let state = Arc::new(FnrpcState {
        router,
        ctx_from_headers: Arc::new(|_| ()),
    });

    let app = App::new()
        .with_state(state)
        .at(
            "/{*path}",
            get(fn_service(dispatch::<()>)).post(fn_service(dispatch::<()>)),
        );

    let app_service = app.finish().call(()).now_or_panic().unwrap();

    let req = make_post_req("/add", &serde_json::to_vec(&[3i32, 5]).unwrap());
    let res = app_service.call(req).now_or_panic().unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_not_found() {
    let router = fnrpc::router::RpcRouterBuilder::<()>::new().build();
    let state = Arc::new(FnrpcState {
        router,
        ctx_from_headers: Arc::new(|_| ()),
    });

    let app = App::new()
        .with_state(state)
        .at(
            "/{*path}",
            get(fn_service(dispatch::<()>)).post(fn_service(dispatch::<()>)),
        );

    let app_service = app.finish().call(()).now_or_panic().unwrap();

    let req = request::Builder::default()
        .uri("/nonexistent")
        .body(Default::default())
        .unwrap();
    let res = app_service.call(req).now_or_panic().unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_subscribe() {
    use fnrpc::handler::RpcSubscribe;
    use std::pin::Pin;

    struct Tick;

    impl RpcSubscribe<()> for Tick {
        type Input = u32;
        type Output = u32;
        const NAME: &'static str = "tick";

        fn exec(
            _ctx: &(),
            input: u32,
        ) -> Pin<Box<dyn futures::Stream<Item = Result<u32, RpcErr>> + Send + 'static>>
        {
            Box::pin(futures::stream::iter((1..=input).map(|n| Ok(n))))
        }
    }

    let router = fnrpc::router::RpcRouterBuilder::<()>::new().subscribe(Tick).build();
    let state = Arc::new(FnrpcState {
        router,
        ctx_from_headers: Arc::new(|_| ()),
    });

    let app = App::new()
        .with_state(state)
        .at(
            "/{*path}",
            get(fn_service(dispatch::<()>)).post(fn_service(dispatch::<()>)),
        );

    let app_service = app.finish().call(()).now_or_panic().unwrap();

    let req = request::Builder::default()
        .uri("/tick?input=3")
        .body(Default::default())
        .unwrap();
    let res = app_service.call(req).now_or_panic().unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_with_context() {
    #[derive(Clone)]
    struct MyCtx {
        prefix: String,
    }

    struct CtxGreet;

    impl RpcFn<MyCtx> for CtxGreet {
        type Input = String;
        type Output = String;
        const NAME: &'static str = "ctx_greet";

        async fn exec(ctx: &MyCtx, input: String) -> Result<String, RpcErr> {
            Ok(format!("{}{}", ctx.prefix, input))
        }
    }

    let router = fnrpc::router::RpcRouterBuilder::<MyCtx>::new()
        .query(CtxGreet)
        .build();
    let state = Arc::new(FnrpcState {
        router,
        ctx_from_headers: Arc::new(|_| MyCtx {
            prefix: "yo ".to_string(),
        }),
    });

    let app = App::new()
        .with_state(state)
        .at(
            "/{*path}",
            get(fn_service(dispatch::<MyCtx>)).post(fn_service(dispatch::<MyCtx>)),
        );

    let app_service = app.finish().call(()).now_or_panic().unwrap();

    let req = request::Builder::default()
        .uri("/ctx_greet?input=%22world%22")
        .body(Default::default())
        .unwrap();
    let res = app_service.call(req).now_or_panic().unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}
