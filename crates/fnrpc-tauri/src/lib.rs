//! Tauri integration for fnrpc.
//!
//! Provides a [`FnrpcTauriState`] managed state and a [`generate_handler!`] macro
//! that registers three Tauri commands (`rpc_fn`, `rpc_sub`, `rpc_cancel_sub`)
//! for serving fnrpc procedures over Tauri IPC.
//!
//! Supports both unary (query/mutate) and subscription procedures.
//! Subscriptions stream values through Tauri's [`Channel`] and can be cancelled
//! from the frontend via the async iterator's `return()` method.
//!
//! # Usage
//!
//! ```ignore
//! use fnrpc::router::RpcRouterBuilder;
//! use fnrpc_tauri::{FnrpcTauriState, generate_handler};
//!
//! let router = RpcRouterBuilder::<MyCtx>::new()
//!     .route_fn(my_handler)
//!     .subscribe(my_sub)
//!     .build();
//!
//! tauri::Builder::default()
//!     .manage(FnrpcTauriState::new(router, || MyCtx { ... }))
//!     .invoke_handler(generate_handler!())
//!     .run(tauri::generate_context!())
//!     .expect("error while running tauri application");
//! ```

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use fnrpc::router::RpcRouter;
use fnrpc::serializer::unpack_meta;
use futures::StreamExt;
use serde_json::Value;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

// ── FnrpcTauriState ─────────────────────────────────

/// Tauri-managed state holding an fnrpc router and a context factory.
///
/// Register this with [`tauri::Builder::manage`] before using
/// [`generate_handler!`] to register the IPC commands.
///
/// # Type parameters
///
/// * `Ctx` — Application context type, created by `ctx_factory` for each request.
pub struct FnrpcTauriState<Ctx: Send + Sync + 'static> {
    pub(crate) router: Arc<RpcRouter<Ctx>>,
    pub(crate) ctx_factory: Arc<dyn Fn() -> Ctx + Send + Sync>,
}

impl<Ctx: Send + Sync + 'static> FnrpcTauriState<Ctx> {
    /// Create a new state with a router and a context factory.
    ///
    /// The context factory is called for each request to produce the
    /// application context (e.g., database connection, auth info).
    /// Unlike the HTTP transport crates, the factory takes no arguments
    /// because Tauri IPC does not carry HTTP headers.
    pub fn new(router: RpcRouter<Ctx>, ctx_factory: impl Fn() -> Ctx + Send + Sync + 'static) -> Self {
        Self {
            router: Arc::new(router),
            ctx_factory: Arc::new(ctx_factory),
        }
    }

    /// Create a new state from an already-arc'd router (for sharing with
    /// other transport layers like `fnrpc-axum`).
    pub fn from_arc(
        router: Arc<RpcRouter<Ctx>>,
        ctx_factory: impl Fn() -> Ctx + Send + Sync + 'static,
    ) -> Self {
        Self {
            router,
            ctx_factory: Arc::new(ctx_factory),
        }
    }
}

// ── Subscription cancellation tracking ──────────────

/// Tracks active subscriptions so they can be cancelled on client disconnect.
static ACTIVE_SUBS: LazyLock<Mutex<HashMap<u32, CancellationToken>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ── generate_handler! macro ─────────────────────────

/// Generate Tauri invoke handler functions for the three fnrpc commands.
///
/// This macro generates three concrete `#[tauri::command]` functions
/// (prefixed with `__fnrpc_`) and returns an array of
/// [`tauri::ipc::InvokeHandler`] that can be passed directly to
/// [`tauri::Builder::invoke_handler`].
///
/// # Arguments
///
/// * `$ctx_ty` — The application context type (e.g. `MyCtx`).
///
/// Combine with custom commands:
///
/// ```ignore
/// use fnrpc_tauri::{FnrpcTauriState, generate_handler};
///
/// #[tauri::command]
/// async fn my_custom() -> String { "hello".into() }
///
/// tauri::Builder::default()
///     .manage(FnrpcTauriState::<MyCtx>::new(router, || MyCtx { ... }))
///     .invoke_handler(generate_handler!(MyCtx))
///     .invoke_handler(tauri::generate_handler![my_custom])
///     .run(tauri::generate_context!())
/// ```
#[macro_export]
macro_rules! generate_handler {
    ($ctx_ty:ty) => {
        {
            #[tauri::command]
            async fn __fnrpc_rpc_fn(
                state: tauri::State<'_, $crate::FnrpcTauriState<$ctx_ty>>,
                path: String,
                input: serde_json::Value,
            ) -> Result<serde_json::Value, String> {
                $crate::rpc_fn_impl(&state, &path, input).await
            }

            #[tauri::command]
            async fn __fnrpc_rpc_sub(
                state: tauri::State<'_, $crate::FnrpcTauriState<$ctx_ty>>,
                path: String,
                input: serde_json::Value,
                channel: tauri::ipc::Channel<String>,
            ) -> Result<(), String> {
                $crate::rpc_sub_impl(&state, &path, input, channel).await
            }

            #[tauri::command]
            async fn __fnrpc_rpc_cancel_sub(channel_id: u32) -> Result<(), String> {
                $crate::rpc_cancel_sub_impl(channel_id).await
            }

            tauri::generate_handler![
                __fnrpc_rpc_fn,
                __fnrpc_rpc_sub,
                __fnrpc_rpc_cancel_sub,
            ]
        }
    };
}

// ── Command implementations ─────────────────────────

/// Internal implementation of the `rpc_fn` command.
///
/// Dispatches a unary (query/mutate) call through the router:
/// 1. Unpacks the BigInt meta envelope from the client
/// 2. Serializes the input to JSON bytes
/// 3. Calls `router.dispatch()` (always POST-style, no query string)
/// 4. Deserializes the response back to a JSON value
pub async fn rpc_fn_impl<Ctx: Send + Sync + 'static>(
    state: &FnrpcTauriState<Ctx>,
    path: &str,
    input: Value,
) -> Result<Value, String> {
    let ctx = (state.ctx_factory)();
    let input = unpack_meta(input);
    let input_bytes = serde_json::to_vec(&input).map_err(|e| e.to_string())?;
    let (result, _is_json) = state
        .router
        .dispatch(&ctx, path, &input_bytes, false)
        .await
        .map_err(|e| serde_json::to_string(&e).unwrap())?;
    serde_json::from_slice(&result).map_err(|e| e.to_string())
}

/// Internal implementation of the `rpc_sub` command.
///
/// Starts a subscription stream:
/// 1. Unpacks the BigInt meta envelope
/// 2. Calls `router.dispatch_subscribe()` to get a value stream
/// 3. Spawns a background task that forwards stream items through the
///    Tauri [`Channel`]
/// 4. Registers a [`CancellationToken`] so the subscription can be
///    cancelled via [`rpc_cancel_sub_impl`]
///
/// The background task stops when:
/// - The stream ends naturally
/// - An error occurs
/// - The cancellation token is triggered (via [`rpc_cancel_sub_impl`])
pub async fn rpc_sub_impl<Ctx: Send + Sync + 'static>(
    state: &FnrpcTauriState<Ctx>,
    path: &str,
    input: Value,
    channel: Channel<String>,
) -> Result<(), String> {
    let ctx = (state.ctx_factory)();
    let input = unpack_meta(input);
    let input_bytes = serde_json::to_vec(&input).map_err(|e| e.to_string())?;
    let mut stream = state
        .router
        .dispatch_subscribe(&ctx, path, &input_bytes)
        .map_err(|e| serde_json::to_string(&e).unwrap())?;

    let cancel = CancellationToken::new();
    ACTIVE_SUBS.lock().unwrap().insert(channel.id(), cancel.clone());

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                item = stream.next() => {
                    match item {
                        Some(Ok(bytes)) => {
                            if let Ok(s) = String::from_utf8(bytes.into_owned()) {
                                if channel.send(s).is_err() {
                                    break;
                                }
                            }
                        }
                        Some(Err(e)) => {
                            let _ = channel.send(serde_json::to_string(&e).unwrap());
                            break;
                        }
                        None => break,
                    }
                }
                _ = cancel.cancelled() => {
                    break;
                }
            }
        }
        ACTIVE_SUBS.lock().unwrap().remove(&channel.id());
    });

    Ok(())
}

/// Internal implementation of the `rpc_cancel_sub` command.
///
/// Cancels an active subscription by its channel ID.
/// Called from the frontend when the async iterator's `return()` is invoked.
pub async fn rpc_cancel_sub_impl(channel_id: u32) -> Result<(), String> {
    if let Some(cancel) = ACTIVE_SUBS.lock().unwrap().remove(&channel_id) {
        cancel.cancel();
    }
    Ok(())
}
