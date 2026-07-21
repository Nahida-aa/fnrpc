// Allow dead_code for macro-generated impl structs that are used
// indirectly via trait dispatch.
#![allow(dead_code)]

use fnrpc::error::RpcErr;
use fnrpc::handler::{RpcFn, RpcFnExt};
use fnrpc::middlewares::hook::HookLayer;
use fnrpc::router::RpcRouterBuilder;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::pin::Pin;
use std::sync::Arc;

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
    let _router = RpcRouterBuilder::<AppCtx>::new().route_fn(CtxGreet).build();

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
        .layer(
            HookLayer::new().before(move |_ctx, _path, _input, _is_get| {
                cc.fetch_add(1, Ordering::SeqCst);
                Err(RpcErr::new("BLOCKED", "blocked by middleware"))
            }),
        )
        .route_fn(macro_health)
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
        .layer(HookLayer::new().before(|_ctx, _path, input, _is_get| {
            // Return input unchanged
            Ok(input)
        }))
        .route_fn(macro_health)
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
        .layer(HookLayer::new().after(move |_ctx, _path, _result| {
            ac.fetch_add(1, Ordering::SeqCst);
        }))
        .route_fn(macro_health)
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
        .layer(
            HookLayer::new().before(move |_ctx, _path, _input, _is_get| {
                o1.store(1, Ordering::SeqCst);
                Ok(_input)
            }),
        )
        .layer(
            HookLayer::new().before(move |_ctx, _path, _input, _is_get| {
                o2.store(2, Ordering::SeqCst);
                Ok(_input)
            }),
        )
        .route_fn(macro_health)
        .build();

    let _ = router
        .dispatch(&(), "macro_health", b"null", false)
        .await
        .unwrap();
    // L1 (first added, rev() makes it outermost) runs before-hook first, then L2 (inner)
    // So final value = 2 (set by L2.before which runs second)
    assert_eq!(order.load(Ordering::SeqCst), 2);
}

// ── Raw bytes handler tests ──────────────────────────────

#[fnrpc::rpc_bytes]
async fn test_noop_raw(_input: &[u8]) -> &'static [u8] {
    b"ok"
}

#[tokio::test]
async fn test_noop_raw_dispatch() {
    let router = RpcRouterBuilder::<()>::new()
        .route_bytes(test_noop_raw)
        .build();
    let (bytes, is_json) = router
        .dispatch(&(), "test_noop_raw", b"hello", false)
        .await
        .unwrap();
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
    let router = RpcRouterBuilder::<()>::new()
        .route_fn(test_echo_get)
        .build();
    // GET dispatch: bytes are query string, handler parses `input` param
    let (bytes, is_json) = router
        .dispatch(&(), "test_echo_get", b"input=%22hello%22", true)
        .await
        .unwrap();
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
    let router = RpcRouterBuilder::<()>::new()
        .route_fn(test_echo_post)
        .build();
    let (bytes, is_json) = router
        .dispatch(&(), "test_echo_post", br#""world""#, false)
        .await
        .unwrap();
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
        .layer(
            HookLayer::new().before(move |_ctx, _path, _input, _is_get| {
                mc.fetch_add(1, Ordering::SeqCst);
                Ok(_input)
            }),
        )
        .route_fn(test_echo_get)
        .build();

    let (bytes, is_json) = router
        .dispatch(&(), "test_echo_get", b"input=%22hi%22", true)
        .await
        .unwrap();
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
        .layer_fn(move |inner, ctx, path, input, is_get, extensions| {
            let cc = Arc::clone(&cc);
            Box::pin(async move {
                cc.fetch_add(1, Ordering::SeqCst);
                use fnrpc::router::ErasedHandler;
                inner.call(ctx, path, input, is_get, extensions).await
            })
        })
        .route_fn(test_echo_get)
        .build();

    let (bytes, _is_json) = router
        .dispatch(&(), "test_echo_get", b"input=%22layer_fn%22", true)
        .await
        .unwrap();
    assert_eq!(&*bytes, br#""layer_fn""#);
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

// ── TracingLayer test ─────────────────────────────────────

#[cfg(feature = "tracing")]
#[tokio::test]
async fn test_tracing_layer() {
    use fnrpc::middlewares::tracing::TracingLayer;

    let router = RpcRouterBuilder::<()>::new()
        .route_fn(test_echo_get)
        .layer(TracingLayer)
        .build();

    let (bytes, is_json) = router
        .dispatch(&(), "test_echo_get", b"input=%22tracing%22", true)
        .await
        .unwrap();
    assert_eq!(&*bytes, br#""tracing""#);
    assert!(is_json);
}

// ── BigInt end-to-end (client→server) decode-by-schema test ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
struct BigIdInput {
    id: u64,
    signed: i128,
    list: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
struct BigIdOutput {
    id: u64,
    signed: i128,
    list: Vec<u64>,
}

struct BigId;

impl RpcFn<()> for BigId {
    type Input = BigIdInput;
    type Output = BigIdOutput;
    const KEY: &'static str = "big_id";

    fn exec(
        _ctx: &(),
        input: Self::Input,
    ) -> Pin<Box<dyn futures::Future<Output = Result<Self::Output, RpcErr>> + Send + '_>> {
        Box::pin(async move {
            Ok(BigIdOutput {
                id: input.id,
                signed: input.signed,
                list: input.list,
            })
        })
    }
}

#[tokio::test]
async fn test_bigint_decoded_by_schema_end_to_end() {
    // Simulate the exact wire body the TS client now sends via `toRustJson`:
    // bigint fields are JSON *strings*, no `meta` envelope.
    let wire = br#"{"id":"18446744073709551615","signed":"170141183460469231731687303715884105727","list":["1","2","18446744073709551615"]}"#;

    let router = RpcRouterBuilder::<()>::new().route_fn(BigId).build();
    let (bytes, is_json) = router.dispatch(&(), "big_id", wire, false).await.unwrap();
    assert!(is_json);

    let output: BigIdOutput = serde_json::from_slice(&bytes).unwrap();
    // Full u64 / i128 precision preserved through schema-driven decode.
    assert_eq!(output.id, 18446744073709551615u64);
    assert_eq!(output.signed, 170141183460469231731687303715884105727i128);
    assert_eq!(output.list, vec![1u64, 2, 18446744073709551615]);
}

#[tokio::test]
async fn test_bigint_plain_value_call_decoded_by_schema() {
    // The `call(Value)` path must also decode string bigints by schema.
    let input = serde_json::json!({
        "id": "18446744073709551615",
        "signed": "170141183460469231731687303715884105727",
        "list": ["1", "2"]
    });
    let output = BigId.call(&(), input).await.unwrap();
    let out: BigIdOutput = serde_json::from_value(output).unwrap();
    assert_eq!(out.id, 18446744073709551615u64);
    assert_eq!(out.signed, 170141183460469231731687303715884105727i128);
}

// ── Subscribe type registration in codegen ─────────────

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
struct SubInput {
    start: u32,
    label: String,
}

#[fnrpc::rpc_subscribe]
fn sub_struct(input: SubInput) -> impl futures::Stream<Item = String> {
    let label = input.label;
    futures::stream::iter(0..input.start).map(move |n| format!("{label}{n}"))
}

#[test]
fn test_subscribe_input_type_is_generated() {
    // Regression: a subscribe handler's input type must appear as a generated
    // `export type` definition, not just a dangling reference in `Procedures`.
    // Previously `subscribe` skipped registering types into the shared
    // registry, so `SubInput` was referenced but never defined.
    let router = RpcRouterBuilder::<()>::new().subscribe(sub_struct).build();
    let bindings = fnrpc::gen_ts_client::generate_ts_client(&router, "http://localhost");

    assert!(
        bindings.contains("export type SubInput"),
        "subscribe input type `SubInput` was not emitted in generated bindings:\n{bindings}"
    );
    assert!(
        bindings.contains("sub_struct: { kind: \"subscribe\";"),
        "subscribe procedure missing from Procedures:\n{bindings}"
    );
    assert!(
        bindings.contains("input: SubInput;"),
        "subscribe procedure does not reference its input type:\n{bindings}"
    );
}
