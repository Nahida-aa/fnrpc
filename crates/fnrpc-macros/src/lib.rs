//! Proc macros for fnrpc.
//!
//! These attributes transform plain Rust functions into typed RPC handlers
//! that are registered with [`RpcRouter`](fnrpc::router::RpcRouter).

mod query;
mod subscribe;

use proc_macro::TokenStream;

/// Register an async function as a query RPC.
///
/// The function becomes a `RpcFn<Ctx>`-implementing struct with
/// [`KIND = "query"`](fnrpc::handler::RpcFn::KIND).
///
/// # Parameters
///
/// - First `&Ctx` param → context type; omit for `Ctx = ()`.
/// - Remaining params → single input type or tuple (multi-param).
///
/// # Return type
///
/// - `Result<T, RpcErr>` → `T` is output, error forwarded as-is.
/// - `Result<T, E>` (non-RpcErr) → `E` wrapped in `RpcErr::internal`.
/// - `T` (no Result) → wrapped in `Ok(T)`.
///
/// # Examples
///
/// The shortest way — no input, trivial return:
///
/// ```ignore
/// #[rpc_query]
/// async fn health_check() -> &'static str {
///     "ok"
/// }
/// ```
///
/// 64-bit integers (`u64`/`i64`) in parameters or return values are
/// automatically serialized as BigInt on the JavaScript side:
///
/// ```ignore
/// #[rpc_query]
/// async fn fib(n: u64) -> u64 {
///     // ...
/// }
/// ```
///
/// Plain error message — any non-`RpcErr` error is wrapped in
/// [`RpcErr::internal`](fnrpc::error::RpcErr::internal) via `.to_string()`:
///
/// ```ignore
/// #[rpc_query]
/// async fn divide(a: i32, b: i32) -> Result<i32, String> {
///     if b == 0 {
///         return Err("division by zero".into());
///     }
///     Ok(a / b)
/// }
/// ```
///
/// Custom error code — return `Result<T, RpcErr>` with a convenience
/// constructor like [`RpcErr::bad_request`](fnrpc::error::RpcErr::bad_request):
///
/// ```ignore
/// #[rpc_query]
/// async fn divide2(a: i32, b: i32) -> Result<i32, RpcErr> {
///     if b == 0 {
///         return Err(RpcErr::bad_request("cannot divide by zero"));
///     }
///     Ok(a / b)
/// }
/// ```
///
/// Multiple parameters — the macro wraps them into a tuple input type:
///
/// ```ignore
/// #[rpc_query]
/// async fn add(a: i64, b: i64) -> i64 {
///     a + b
/// }
/// ```
///
/// Shared context (`&Ctx`) — access app state:
///
/// ```ignore
/// #[rpc_query]
/// async fn count_users(db: &Database) -> u64 {
///     db.users().await.len() as u64
/// }
/// ```
///
/// The expanded form is equivalent to writing the `RpcFn` impl yourself:
///
/// ```ignore
/// struct health_check;
///
/// #[async_trait::async_trait]
/// impl<T: Send + Sync + 'static> RpcFn<T> for health_check {
///     type Input = ();
///     type Output = &'static str;
///     const NAME: &'static str = "health_check";
///     const KIND: &'static str = "query";
///
///     async fn exec(_ctx: &T, _input: ()) -> Result<Self::Output, RpcErr> {
///         Ok("ok")
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn rpc_query(_attr: TokenStream, item: TokenStream) -> TokenStream {
    query::rpc_fn_impl("query", item)
}

/// Register an async function as a mutate RPC.
///
/// Same semantics as [`rpc_query`], but produces `KIND = "mutate"`.
/// The transport sends input as `POST` body instead of URL query params.
///
/// See [`rpc_query`] for examples — all apply identically.
#[proc_macro_attribute]
pub fn rpc_mutate(_attr: TokenStream, item: TokenStream) -> TokenStream {
    query::rpc_fn_impl("mutate", item)
}

/// Register a **sync** function returning a `Stream` as a subscribe RPC.
///
/// The function must be `fn` (not `async fn`) and may return any type that
/// implements [`futures::Stream`] — most commonly written as
/// `impl futures::Stream<Item = T>`.
///
/// The macro wraps the return value with `Box::pin(...)` and maps items to
/// `Result<T, RpcErr>`, so the caller sees a
/// `Pin<Box<dyn Stream<Item = Result<T, RpcErr>> + Send + '_>>`.
///
/// # Attribute argument
///
/// - No arg (default): HTTP method `GET`, input via query params.
/// - `"post"`: HTTP method `POST`, input via body.
///
/// # Examples
///
/// The shortest way — return `impl Stream`:
///
/// ```ignore
/// #[rpc_subscribe]
/// pub fn tick(interval_ms: u64) -> impl futures::Stream<Item = u64> {
///     futures::stream::unfold(0u64, move |count| async move {
///         tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
///         Some((count, count + 1))
///     })
/// }
/// ```
///
/// 64-bit integers (`u64`/`i64`) in parameters or return values are
/// automatically serialized as BigInt on the JavaScript side — no
/// special handling needed.
///
/// POST method — input comes via request body instead of query params:
///
/// ```ignore
/// #[rpc_subscribe("post")]
/// fn echo_stream(prefix: String) -> impl futures::Stream<Item = String> {
///     futures::stream::unfold(0u32, move |count| {
///         let prefix = prefix.clone();
///         async move {
///             tokio::time::sleep(Duration::from_secs(1)).await;
///             Some((format!("{prefix} #{count}"), count + 1))
///         }
///     })
/// }
/// ```
///
/// Or with a plain error message (stream item is `Result<T, String>`):
///
/// The macro wraps the error via `.to_string()` into
/// [`RpcErr::internal`](fnrpc::error::RpcErr::internal) — the error code is
/// always `INTERNAL_SERVER_ERROR`. For precise error codes and structured
/// payloads, use `Result<T, RpcErr>` as the item type.
///
/// ```ignore
/// #[rpc_subscribe]
/// fn subscribe(name: String) -> impl futures::Stream<Item = Result<String, String>> {
///     futures::stream::unfold(0u32, move |count| async move {
///         tokio::time::sleep(Duration::from_secs(1)).await;
///         Some((Ok(format!("hello #{count}")), count + 1))
///     })
/// }
/// ```
///
/// The expanded form is equivalent to writing the `Pin<Box<dyn ...>>` yourself:
///
/// ```ignore
/// #[rpc_subscribe]
/// fn watch_user(id: String) -> Pin<Box<dyn futures::Stream<Item = Result<String, RpcErr>> + Send + '_>> {
///     // ... Box::pin(stream.map(Ok))
/// }
/// ```
#[proc_macro_attribute]
pub fn rpc_subscribe(_attr: TokenStream, item: TokenStream) -> TokenStream {
    subscribe::rpc_subscribe_impl(_attr, item)
}
