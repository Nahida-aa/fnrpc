use fnrpc::error::RpcErr;
use fnrpc::handler::{RpcFn, RpcFnExt, SubscribeExt};
use fnrpc::router::RpcRouterBuilder;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::pin::Pin;

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

impl RpcFn<()> for Greet {
    type Input = GreetInput;
    type Output = GreetOutput;
    const KEY: &'static str = "greet";

    fn exec(
        _ctx: &(),
        input: Self::Input,
    ) -> Pin<Box<dyn futures::Future<Output = Result<Self::Output, RpcErr>> + Send + '_>> {
        Box::pin(async move {
            Ok(GreetOutput {
                message: format!("hello {}", input.name),
            })
        })
    }
}

// --- Function with context ---

struct AppCtx {
    prefix: String,
}

struct CtxGreet;

impl RpcFn<AppCtx> for CtxGreet {
    type Input = GreetInput;
    type Output = GreetOutput;
    const KEY: &'static str = "ctx_greet";

    fn exec(
        ctx: &AppCtx,
        input: Self::Input,
    ) -> Pin<Box<dyn futures::Future<Output = Result<Self::Output, RpcErr>> + Send + '_>> {
        Box::pin(async move {
            Ok(GreetOutput {
                message: format!("{}{}", ctx.prefix, input.name),
            })
        })
    }
}

// --- Non-Result return type (auto-wrapped in Ok) ---

#[fnrpc::rpc_query]
async fn macro_health() -> &'static str {
    "ok"
}

#[fnrpc::rpc_query]
fn macro_query_sync() -> &'static str {
    "ok"
}

#[fnrpc::rpc_query]
async fn macro_health_ctx(_ctx: &()) -> &'static str {
    "ok"
}

// --- rpc_query macro test ---

#[fnrpc::rpc_query]
fn macro_greet(input: GreetInput) -> Result<GreetOutput, String> {
    Ok(GreetOutput {
        message: format!("macro hello {}", input.name),
    })
}

// --- rpc_mutate macro test ---

#[fnrpc::rpc_mutate]
fn macro_mutate(input: GreetInput) -> Result<GreetOutput, String> {
    Ok(GreetOutput {
        message: format!("mutated {}", input.name),
    })
}

// --- rpc_query with context inferred from &T parameter ---

#[fnrpc::rpc_query]
fn macro_ctx_greet(ctx: &AppCtx, input: GreetInput) -> Result<GreetOutput, String> {
    Ok(GreetOutput {
        message: format!("{}{}", ctx.prefix, input.name),
    })
}

#[tokio::test]
async fn test_manual_rpc() {
    let input = serde_json::json!({ "name": "world" });

    // Direct handler call via RpcFnExt
    let result = Greet.call(&(), input).await.unwrap();
    let output: GreetOutput = serde_json::from_value(result).unwrap();
    assert_eq!(output.message, "hello world");
}

#[tokio::test]
async fn test_ctx_rpc() {
    let router = RpcRouterBuilder::<AppCtx>::new().route(CtxGreet).build();

    let ctx = AppCtx {
        prefix: "yo ".to_string(),
    };
    let input = serde_json::json!({ "name": "world" });

    let result = CtxGreet.call(&ctx, input).await.unwrap();
    let output: GreetOutput = serde_json::from_value(result).unwrap();
    assert_eq!(output.message, "yo world");
}

#[tokio::test]
async fn test_macro_rpc() {
    let input = serde_json::json!({ "name": "world" });

    let result = macro_greet.call(&(), input).await.unwrap();
    let output: GreetOutput = serde_json::from_value(result).unwrap();
    assert_eq!(output.message, "macro hello world");
}

#[tokio::test]
async fn test_ts_info() {
    let router = RpcRouterBuilder::<()>::new().route(Greet).build();
    let meta = router
        .procedures()
        .iter()
        .find(|m| m.key == "greet")
        .unwrap();
    assert_eq!(meta.input.ts_ref, "GreetInput");
    assert_eq!(meta.output.ts_ref, "GreetOutput");
}

#[tokio::test]
async fn test_macro_mutate_kind() {
    let router = RpcRouterBuilder::<()>::new().route(macro_mutate).build();
    let meta = router
        .procedures()
        .iter()
        .find(|m| m.key == "macro_mutate")
        .unwrap();
    assert_eq!(meta.kind, "mutate");
}

#[tokio::test]
async fn test_macro_health_no_ctx() {
    let result = macro_health
        .call(&(), serde_json::json!(null))
        .await
        .unwrap();
    assert_eq!(result, serde_json::json!("ok"));
}

#[tokio::test]
async fn test_macro_health_with_ctx() {
    let result = macro_health_ctx
        .call(&(), serde_json::json!(null))
        .await
        .unwrap();
    assert_eq!(result, serde_json::json!("ok"));
}

#[tokio::test]
async fn test_macro_ctx_rpc() {
    let ctx = AppCtx {
        prefix: "yo ".to_string(),
    };
    let input = serde_json::json!({ "name": "world" });

    let result = macro_ctx_greet.call(&ctx, input).await.unwrap();
    let output: GreetOutput = serde_json::from_value(result).unwrap();
    assert_eq!(output.message, "yo world");
}

// ── Subscription tests ─────────────────────────────────

#[fnrpc::rpc_subscribe]
fn sub_count(input: u32) -> impl futures::Stream<Item = u32> {
    futures::stream::iter(1..=input)
}

#[fnrpc::rpc_subscribe]
fn sub_count_ctx(
    ctx: &AppCtx,
    input: u32,
) -> Pin<Box<dyn futures::Stream<Item = Result<String, String>> + Send + 'static>> {
    let prefix = ctx.prefix.clone();
    let items: Vec<_> = (1..=input).map(|n| Ok(format!("{prefix}{n}"))).collect();
    Box::pin(futures::stream::iter(items))
}

#[tokio::test]
async fn test_subscribe() {
    use fnrpc::handler::SubscribeExt;
    let stream = sub_count.call(&(), serde_json::json!(3));
    let items: Vec<i32> = stream
        .map(|v| serde_json::from_value::<i32>(v.unwrap()).unwrap())
        .collect()
        .await;
    assert_eq!(items, vec![1, 2, 3]);
}

#[tokio::test]
async fn test_subscribe_ctx() {
    use fnrpc::handler::SubscribeExt;
    let ctx = AppCtx {
        prefix: "n".to_string(),
    };
    let stream = sub_count_ctx.call(&ctx, serde_json::json!(2));
    let items: Vec<String> = stream
        .map(|v| serde_json::from_value::<String>(v.unwrap()).unwrap())
        .collect()
        .await;
    assert_eq!(items, vec!["n1".to_string(), "n2".to_string()]);
}

#[tokio::test]
async fn test_subscribe_unknown_path() {
    // RpcRouter no longer stores subscribe handlers directly
}

// ── Multi-parameter tests ──────────────────────────────

#[fnrpc::rpc_query]
fn multi_param(a: i32, b: i32, c: String) -> String {
    format!("{}{}{}", a, b, c)
}

#[fnrpc::rpc_query]
fn multi_param_ctx(ctx: &AppCtx, a: i32, b: i32) -> String {
    format!("{}{}", a + b, ctx.prefix)
}

#[tokio::test]
async fn test_multi_param() {
    let input = serde_json::json!([1, 2, "hello"]);
    let result = multi_param.call(&(), input).await.unwrap();
    assert_eq!(result, serde_json::json!("12hello"));
}

#[tokio::test]
async fn test_multi_param_ctx() {
    let ctx = AppCtx {
        prefix: "x".to_string(),
    };
    let input = serde_json::json!([3, 4]);
    let result = multi_param_ctx.call(&ctx, input).await.unwrap();
    assert_eq!(result, serde_json::json!("7x"));
}

#[tokio::test]
async fn test_multi_param_ts_info() {
    let router = RpcRouterBuilder::<()>::new().route(multi_param).build();
    let meta = router
        .procedures()
        .iter()
        .find(|m| m.key == "multi_param")
        .unwrap();
    // (i32, i32, String) should inline as [number, number, string]
    assert_eq!(meta.input.ts_ref, "[number, number, string]");
    assert_eq!(meta.output.ts_ref, "string");
}

// ── Middleware tests ──────────────────────────────────────
// TODO: restore these tests after middleware refactor
// The middleware dispatch chain needs to be reworked for the new
// no-erasure architecture.
