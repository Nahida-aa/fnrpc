use std::sync::Arc;

use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc_web::{handle, FnrpcConfig};
use xitca_http::body::RequestBody;
use xitca_http::bytes::Bytes;
use xitca_http::http::{Method, Request, RequestExt, StatusCode};

#[tokio::test]
async fn test_query_get() {
    struct Greet;

    impl RpcFn<()> for Greet {
        type Input = String;
        type Output = String;
        const NAME: &'static str = "greet";

        fn exec(_ctx: &(), input: String) -> Result<String, RpcErr> {
            Ok(format!("Hello {input}!"))
        }
    }

    let router = fnrpc::router::RpcRouterBuilder::<()>::new().query(Greet).build();
    let config = FnrpcConfig {
        router,
        ctx_from_headers: Arc::new(|_| ()),
    };

    let mut req = Request::new(RequestExt::<RequestBody>::default());
    *req.method_mut() = Method::GET;
    *req.uri_mut() = "/greet?input=%22world%22".parse().unwrap();

    let res = handle(&config, req);
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_query_post() {
    struct Add;

    impl RpcFn<()> for Add {
        type Input = (i32, i32);
        type Output = i32;
        const NAME: &'static str = "add";

        fn exec(_ctx: &(), input: (i32, i32)) -> Result<i32, RpcErr> {
            Ok(input.0 + input.1)
        }
    }

    let router = fnrpc::router::RpcRouterBuilder::<()>::new().query(Add).build();
    let config = FnrpcConfig {
        router,
        ctx_from_headers: Arc::new(|_| ()),
    };

    let body: RequestBody = Bytes::from(serde_json::to_vec(&[3i32, 5]).unwrap()).into();
    let mut req = Request::new(
        RequestExt::default().map_body(|_: RequestBody| body),
    );
    *req.method_mut() = Method::POST;
    *req.uri_mut() = "/add".parse().unwrap();
    req.headers_mut()
        .insert("content-type", "application/json".parse().unwrap());

    let res = handle(&config, req);
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_not_found() {
    let router = fnrpc::router::RpcRouterBuilder::<()>::new().build();
    let config = FnrpcConfig {
        router,
        ctx_from_headers: Arc::new(|_| ()),
    };

    let mut req = Request::new(RequestExt::<RequestBody>::default());
    *req.method_mut() = Method::GET;
    *req.uri_mut() = "/nonexistent".parse().unwrap();

    let res = handle(&config, req);
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
    let config = FnrpcConfig {
        router,
        ctx_from_headers: Arc::new(|_| ()),
    };

    let mut req = Request::new(RequestExt::<RequestBody>::default());
    *req.method_mut() = Method::GET;
    *req.uri_mut() = "/tick?input=3".parse().unwrap();

    let res = handle(&config, req);
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

        fn exec(ctx: &MyCtx, input: String) -> Result<String, RpcErr> {
            Ok(format!("{}{}", ctx.prefix, input))
        }
    }

    let router = fnrpc::router::RpcRouterBuilder::<MyCtx>::new()
        .query(CtxGreet)
        .build();
    let config = FnrpcConfig {
        router,
        ctx_from_headers: Arc::new(|_| MyCtx {
            prefix: "yo ".to_string(),
        }),
    };

    let mut req = Request::new(RequestExt::<RequestBody>::default());
    *req.method_mut() = Method::GET;
    *req.uri_mut() = "/ctx_greet?input=%22world%22".parse().unwrap();

    let res = handle(&config, req);
    assert_eq!(res.status(), StatusCode::OK);
}
