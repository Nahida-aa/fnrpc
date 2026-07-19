use fnrpc::error::RpcErr;
use fnrpc::handler::{RpcFn, RpcFnExt, SubscribeExt};
use fnrpc::middleware::HookLayer;
use fnrpc::middleware::NextExt;
use fnrpc::router::RpcRouterBuilder;
use std::sync::Arc;
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
    let router = RpcRouterBuilder::<AppCtx>::new().route_fn(CtxGreet).build();

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
    let router = RpcRouterBuilder::<()>::new().route_fn(Greet).build();
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
    let router = RpcRouterBuilder::<()>::new().route_fn(macro_mutate).build();
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
    let router = RpcRouterBuilder::<()>::new().route_fn(multi_param).build();
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

#[tokio::test]
async fn test_middleware_before_hook_short_circuit() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let router = RpcRouterBuilder::<()>::new()
        .route_fn(macro_health)
        .layer(
            HookLayer::new()
                .before(move |_ctx, _path, _input, _is_get| {
                    cc.fetch_add(1, Ordering::SeqCst);
                    Err(RpcErr::new("BLOCKED", "blocked by middleware"))
                }),
        )
        .build();

    let result = router.dispatch(&(), "macro_health", b"null", false).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, "BLOCKED");
    // Before hook was called
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_middleware_modify_input() {
    let router = RpcRouterBuilder::<()>::new()
        .route_fn(macro_health)
        .layer(
            HookLayer::new()
                .before(|_ctx, _path, input, _is_get| {
                    // Return input unchanged
                    Ok(std::borrow::Cow::Borrowed(input))
                }),
        )
        .build();

    let result = router.dispatch(&(), "macro_health", b"null", false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_middleware_after_hook() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let after_called = Arc::new(AtomicUsize::new(0));
    let ac = Arc::clone(&after_called);

    let router = RpcRouterBuilder::<()>::new()
        .route_fn(macro_health)
        .layer(
            HookLayer::new()
                .after(move |_ctx, _path, _result| {
                    ac.fetch_add(1, Ordering::SeqCst);
                }),
        )
        .build();

    let result = router.dispatch(&(), "macro_health", b"null", false).await;
    assert!(result.is_ok());
    assert_eq!(after_called.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_middleware_chain_order() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let order = Arc::new(AtomicUsize::new(0));
    let o1 = Arc::clone(&order);
    let o2 = Arc::clone(&order);

    // Layer order: LIFO — last added layer wraps the previous ones.
    // .layer(L1) → service = L1.layer(InnerService)
    // .layer(L2) → service = L2.layer(L1.layer(InnerService))
    // Execution: L2.before → L1.before → handler → L1.after → L2.after
    let router = RpcRouterBuilder::<()>::new()
        .route_fn(macro_health)
        .layer(
            HookLayer::new()
                .before(move |_ctx, _path, _input, _is_get| {
                    o1.store(1, Ordering::SeqCst);
                    Ok(std::borrow::Cow::Borrowed(_input))
                }),
        )
        .layer(
            HookLayer::new()
                .before(move |_ctx, _path, _input, _is_get| {
                    o2.store(2, Ordering::SeqCst);
                    Ok(std::borrow::Cow::Borrowed(_input))
                }),
        )
        .build();

    let _ = router.dispatch(&(), "macro_health", b"null", false).await.unwrap();
    // L2 (o2, inner, added last) runs before-hook first, then L1 (o1, outer, added first)
    // So final value = 1 (set by L1.before which runs second)
    assert_eq!(order.load(Ordering::SeqCst), 1);
}

// ── Raw bytes handler tests ──────────────────────────────

#[fnrpc::rpc_bytes]
async fn test_noop_raw(input: &[u8]) -> &'static [u8] {
    b"ok"
}

#[tokio::test]
async fn test_noop_raw_dispatch() {
    let router = RpcRouterBuilder::<()>::new().route_bytes(test_noop_raw).build();
    let (bytes, is_json) = router.dispatch(&(), "test_noop_raw", b"hello", false).await.unwrap();
    assert_eq!(&*bytes, b"ok");
    assert!(!is_json); // raw bytes handler
}

// ── Echo GET handler test ──────────────────────────────

#[fnrpc::rpc_query]
async fn test_echo_get(input: String) -> String {
    input
}

#[tokio::test]
async fn test_echo_get_dispatch() {
    let router = RpcRouterBuilder::<()>::new().route_fn(test_echo_get).build();
    // GET dispatch: bytes are query string, handler parses `input` param
    let (bytes, is_json) = router.dispatch(&(), "test_echo_get", b"input=%22hello%22", true).await.unwrap();
    assert_eq!(&*bytes, br#""hello""#);
    assert!(is_json); // RpcFn handler returns JSON
}

// ── Echo POST handler test ─────────────────────────────

#[fnrpc::rpc_mutate]
async fn test_echo_post(input: String) -> String {
    input
}

#[tokio::test]
async fn test_echo_post_dispatch() {
    let router = RpcRouterBuilder::<()>::new().route_fn(test_echo_post).build();
    let (bytes, is_json) = router.dispatch(&(), "test_echo_post", br#""world""#, false).await.unwrap();
    assert_eq!(&*bytes, br#""world""#);
    assert!(is_json);
}

// ── Echo with middleware test ──────────────────────────

#[tokio::test]
async fn test_echo_with_middleware() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let mw_called = Arc::new(AtomicUsize::new(0));
    let mc = Arc::clone(&mw_called);

    let router = RpcRouterBuilder::<()>::new()
        .route_fn(test_echo_get)
        .layer(
            HookLayer::new()
                .before(move |_ctx, _path, _input, _is_get| {
                    mc.fetch_add(1, Ordering::SeqCst);
                    Ok(std::borrow::Cow::Borrowed(_input))
                }),
        )
        .build();

    let (bytes, is_json) = router.dispatch(&(), "test_echo_get", b"input=%22hi%22", true).await.unwrap();
    assert_eq!(&*bytes, br#""hi""#);
    assert!(is_json);
    assert_eq!(mw_called.load(Ordering::SeqCst), 1);
}

// ── layer_fn test ────────────────────────────────────────

#[tokio::test]
async fn test_layer_fn_middleware() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let router = RpcRouterBuilder::<()>::new()
        .route_fn(test_echo_get)
        .layer_fn(move |inner, ctx, path, input, is_get, extensions| {
            let cc = Arc::clone(&cc);
            Box::pin(async move {
                cc.fetch_add(1, Ordering::SeqCst);
                inner.next(ctx, path, input, is_get, extensions).await
            })
        })
        .build();

    let (bytes, _is_json) = router.dispatch(&(), "test_echo_get", b"input=%22layer_fn%22", true).await.unwrap();
    assert_eq!(&*bytes, br#""layer_fn""#);
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}
