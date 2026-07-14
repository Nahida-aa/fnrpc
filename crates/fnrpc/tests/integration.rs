use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouter;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use specta::Type;

// --- Test types ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
struct GreetInput {
    name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
struct GreetOutput {
    message: String,
}

// --- Manual RpcFn impl ---

struct Greet;

#[async_trait::async_trait]
impl RpcFn<()> for Greet {
    type Input = GreetInput;
    type Output = GreetOutput;
    const NAME: &'static str = "greet";

    async fn exec(_ctx: &(), input: Self::Input) -> Result<Self::Output, RpcErr> {
        Ok(GreetOutput {
            message: format!("hello {}", input.name),
        })
    }
}

// --- Function with context ---

struct AppCtx {
    prefix: String,
}

struct CtxGreet;

#[async_trait::async_trait]
impl RpcFn<AppCtx> for CtxGreet {
    type Input = GreetInput;
    type Output = GreetOutput;
    const NAME: &'static str = "ctx_greet";

    async fn exec(ctx: &AppCtx, input: Self::Input) -> Result<Self::Output, RpcErr> {
        Ok(GreetOutput {
            message: format!("{}{}", ctx.prefix, input.name),
        })
    }
}

// --- Non-Result return type (auto-wrapped in Ok) ---

#[fnrpc::rpc_query]
async fn macro_health() -> String {
    "ok".to_string()
}

#[fnrpc::rpc_query]
async fn macro_health_ctx(_ctx: &()) -> String {
    "ok".to_string()
}

// --- rpc_query macro test ---

#[fnrpc::rpc_query]
async fn macro_greet(input: GreetInput) -> Result<GreetOutput, String> {
    Ok(GreetOutput {
        message: format!("macro hello {}", input.name),
    })
}

// --- rpc_mutation macro test ---

#[fnrpc::rpc_mutation]
async fn macro_mutate(input: GreetInput) -> Result<GreetOutput, String> {
    Ok(GreetOutput {
        message: format!("mutated {}", input.name),
    })
}

// --- rpc_query with context inferred from &T parameter ---

#[fnrpc::rpc_query]
async fn macro_ctx_greet(ctx: &AppCtx, input: GreetInput) -> Result<GreetOutput, String> {
    Ok(GreetOutput {
        message: format!("{}{}", ctx.prefix, input.name),
    })
}

#[tokio::test]
async fn test_manual_rpc() {
    let router = RpcRouter::<()>::new().route(Greet);

    let input = serde_json::json!({ "name": "world" });

    // Dispatch
    let result = router.dispatch(&(), "greet", input).await.unwrap();
    let output: GreetOutput = serde_json::from_value(result).unwrap();
    assert_eq!(output.message, "hello world");

    // Unknown method
    let err = router.dispatch(&(), "nonexistent", serde_json::json!(null)).await;
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("unknown path"));
}

#[tokio::test]
async fn test_ctx_rpc() {
    let router = RpcRouter::<AppCtx>::new().route(CtxGreet);

    let ctx = AppCtx {
        prefix: "yo ".to_string(),
    };
    let input = serde_json::json!({ "name": "world" });

    let result = router.dispatch(&ctx, "ctx_greet", input).await.unwrap();
    let output: GreetOutput = serde_json::from_value(result).unwrap();
    assert_eq!(output.message, "yo world");
}

#[tokio::test]
async fn test_macro_rpc() {
    let router = RpcRouter::<()>::new().route(macro_greet__FnRpc);

    let input = serde_json::json!({ "name": "world" });

    let result = router.dispatch(&(), "macro_greet", input).await.unwrap();
    let output: GreetOutput = serde_json::from_value(result).unwrap();
    assert_eq!(output.message, "macro hello world");
}

#[tokio::test]
async fn test_ts_info() {
    use fnrpc::handler::ErasedHandler;
    let handler = Greet;

    let input_info = handler.input_ts();
    assert_eq!(input_info.ts_ref, "GreetInput");

    let output_info = handler.output_ts();
    assert_eq!(output_info.ts_ref, "GreetOutput");
}

#[tokio::test]
async fn test_macro_mutation_kind() {
    use fnrpc::handler::ErasedHandler;
    let handler = macro_mutate__FnRpc;

    // ErasedHandler (blanket impl) provides access to kind()
    let erased: Box<dyn ErasedHandler<()>> = Box::new(handler);
    assert_eq!(erased.kind(), "mutation");
}

#[tokio::test]
async fn test_macro_health_no_ctx() {
    let router = RpcRouter::<()>::new().route(macro_health__FnRpc);
    let result = router.dispatch(&(), "macro_health", serde_json::json!(null)).await.unwrap();
    assert_eq!(result, serde_json::json!("ok"));
}

#[tokio::test]
async fn test_macro_health_with_ctx() {
    let router = RpcRouter::<()>::new().route(macro_health_ctx__FnRpc);
    let result = router.dispatch(&(), "macro_health_ctx", serde_json::json!(null)).await.unwrap();
    assert_eq!(result, serde_json::json!("ok"));
}

#[tokio::test]
async fn test_macro_ctx_rpc() {
    let router = RpcRouter::<AppCtx>::new().route(macro_ctx_greet__FnRpc);

    let ctx = AppCtx {
        prefix: "yo ".to_string(),
    };
    let input = serde_json::json!({ "name": "world" });

    let result = router.dispatch(&ctx, "macro_ctx_greet", input).await.unwrap();
    let output: GreetOutput = serde_json::from_value(result).unwrap();
    assert_eq!(output.message, "yo world");
}

// ── Subscription tests ─────────────────────────────────

#[fnrpc::rpc_subscription]
fn sub_count(input: u32) -> impl futures::Stream<Item = u32> {
    futures::stream::iter(1..=input)
}

#[fnrpc::rpc_subscription]
fn sub_count_ctx(ctx: &AppCtx, input: u32) -> impl futures::Stream<Item = Result<String, String>> {
    let prefix = ctx.prefix.clone();
    let items: Vec<_> = (1..=input).map(|n| Ok(format!("{prefix}{n}"))).collect();
    futures::stream::iter(items)
}

#[tokio::test]
async fn test_subscription() {
    let router = RpcRouter::<()>::new().subscribe(sub_count__FnRpc);

    let stream = router
        .dispatch_subscribe(&(), "sub_count", serde_json::json!(3))
        .unwrap();
    let items: Vec<i32> = stream
        .map(|v| serde_json::from_value::<i32>(v.unwrap()).unwrap())
        .collect()
        .await;
    assert_eq!(items, vec![1, 2, 3]);
}

#[tokio::test]
async fn test_subscription_ctx() {
    let ctx = AppCtx {
        prefix: "n".to_string(),
    };
    let router = RpcRouter::<AppCtx>::new().subscribe(sub_count_ctx__FnRpc);

    let stream = router
        .dispatch_subscribe(&ctx, "sub_count_ctx", serde_json::json!(2))
        .unwrap();
    let items: Vec<String> = stream
        .map(|v| serde_json::from_value::<String>(v.unwrap()).unwrap())
        .collect()
        .await;
    assert_eq!(items, vec!["n1".to_string(), "n2".to_string()]);
}

#[tokio::test]
async fn test_subscription_unknown_path() {
    let router = RpcRouter::<()>::new();
    let err = router
        .dispatch_subscribe(&(), "nonexistent", serde_json::json!(null));
    match err {
        Err(e) => assert!(e.to_string().contains("unknown subscription")),
        Ok(_) => panic!("expected error"),
    }
}

// ── Multi-parameter tests ──────────────────────────────

#[fnrpc::rpc_query]
async fn multi_param(a: i32, b: i32, c: String) -> String {
    format!("{}{}{}", a, b, c)
}

#[fnrpc::rpc_query]
async fn multi_param_ctx(ctx: &AppCtx, a: i32, b: i32) -> String {
    format!("{}{}", a + b, ctx.prefix)
}

#[tokio::test]
async fn test_multi_param() {
    let router = RpcRouter::<()>::new().route(multi_param__FnRpc);

    let input = serde_json::json!([1, 2, "hello"]);
    let result = router.dispatch(&(), "multi_param", input).await.unwrap();
    assert_eq!(result, serde_json::json!("12hello"));
}

#[tokio::test]
async fn test_multi_param_ctx() {
    let ctx = AppCtx {
        prefix: "x".to_string(),
    };
    let router = RpcRouter::<AppCtx>::new().route(multi_param_ctx__FnRpc);

    let input = serde_json::json!([3, 4]);
    let result = router.dispatch(&ctx, "multi_param_ctx", input).await.unwrap();
    assert_eq!(result, serde_json::json!("7x"));
}

#[tokio::test]
async fn test_multi_param_ts_info() {
    use fnrpc::handler::ErasedHandler;
    // multi_param has no ctx param, so RpcFn<T> is generic over T.
    // Use a fully qualified path via Box<dyn ErasedHandler<()>> to pin Ctx.
    let erased: Box<dyn ErasedHandler<()>> = Box::new(multi_param__FnRpc);

    let input_info = erased.input_ts();
    // (i32, i32, String) should inline as [number, number, string]
    assert_eq!(input_info.ts_ref, "[number, number, string]");

    let output_info = erased.output_ts();
    assert_eq!(output_info.ts_ref, "string");
}

// ── Middleware tests ──────────────────────────────────────

use fnrpc::middleware::{FnLayer, HookLayer};

/// Layer that prepends "layered:" to the output string.
struct PrefixLayer;

impl FnLayer<()> for PrefixLayer {
    fn layer(&self, inner: Box<dyn fnrpc::middleware::FnService<()>>) -> Box<dyn fnrpc::middleware::FnService<()>> {
        Box::new(PrefixService { inner })
    }
}

struct PrefixService {
    inner: Box<dyn fnrpc::middleware::FnService<()>>,
}

#[async_trait::async_trait]
impl fnrpc::middleware::FnService<()> for PrefixService {
    async fn call(&self, ctx: &(), path: &str, input: serde_json::Value) -> Result<serde_json::Value, fnrpc::error::RpcErr> {
        let result = self.inner.call(ctx, path, input).await?;
        let s = result.as_str().unwrap_or("");
        Ok(serde_json::json!(format!("layered:{s}")))
    }
}

#[tokio::test]
async fn test_custom_layer() {
    let router = RpcRouter::<()>::new()
        .route(macro_health__FnRpc)
        .layer(PrefixLayer);

    let result = router.dispatch(&(), "macro_health", serde_json::json!(null)).await.unwrap();
    assert_eq!(result, serde_json::json!("layered:ok"));
}

#[tokio::test]
async fn test_hook_layer() {
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log_clone = log.clone();
    let router = RpcRouter::<()>::new()
        .route(macro_health__FnRpc)
        .layer(
            HookLayer::new()
                .before(move |_ctx, path, _input| {
                    log_clone.lock().unwrap().push(format!("before:{path}"));
                    Ok(())
                })
                .after(move |_ctx, _path, result| {
                    if let Ok(val) = result {
                        *val = serde_json::json!("hooked");
                    }
                }),
        );

    let result = router.dispatch(&(), "macro_health", serde_json::json!(null)).await.unwrap();
    assert_eq!(result, serde_json::json!("hooked"));
    assert_eq!(log.lock().unwrap()[0], "before:macro_health");
}

#[tokio::test]
async fn test_multiple_layers() {
    let router = RpcRouter::<()>::new()
        .route(macro_health__FnRpc)
        .layer(PrefixLayer)
        .layer(HookLayer::new().after(|_ctx, _path, result| {
            if let Ok(val) = result {
                *val = serde_json::json!("wrapped");
            }
        }));

    // HookLayer (last added) is outermost, so after-hook runs after PrefixLayer
    let result = router.dispatch(&(), "macro_health", serde_json::json!(null)).await.unwrap();
    assert_eq!(result, serde_json::json!("wrapped"));
}

#[tokio::test]
async fn test_ts_client() {
    let router = RpcRouter::<()>::new()
        .route(Greet)
        .route(multi_param__FnRpc);

    let client = router.generate_ts_client("/rpc");
    assert!(client.contains("greet"), "should contain method name");
    assert!(client.contains("GreetInput"), "should contain input type");
    assert!(client.contains("GreetOutput"), "should contain output type");
    assert!(client.contains("Procedures"), "should generate Procedures interface");
    assert!(client.contains("\"query\""), "should contain kind");
    assert!(client.contains("multi_param"), "should contain multi_param method");
    assert!(
        client.contains("[number, number, string]"),
        "multi_param should have tuple input"
    );
}
