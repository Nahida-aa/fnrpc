//! Middleware system for fnrpc.
//!
//! - [`RpcService`] — the core callable trait (RPIT-based, zero `Box::pin`
//!   inside the monomorphized chain).
//! - [`ErasedRpcService`] — dyn-compatible wrapper for storage in `Arc`.
//! - [`RpcLayer`] — a composable middleware wrapper (generic over inner `S`).
//! - [`HookLayer`] — before/after hooks.
//! - [`TracingLayer`] — structured logging (feature = `"tracing"`).

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use http::Extensions;
use serde_json::Value;

use crate::error::RpcErr;

/// Core service trait — call a method with JSON input, get JSON output.
///
/// Uses **RPIT (return-position `impl Trait`)** — no `#[async_trait]`,
/// no hidden `Box::pin` allocation per call inside the monomorphized chain.
///
/// **This trait is NOT `dyn`-compatible.** Use [`ErasedRpcService`] for
/// type-erased storage (a single `Box::pin` at the dyn boundary).
///
/// The [`RpcRouterBuilder`](crate::router::RpcRouterBuilder) builds a concrete
/// monomorphized chain of layers around a [`InnerService`](crate::router::InnerService).
/// At [`build()`](crate::router::RpcRouterBuilder::build) time the chain is
/// wrapped into a single `Arc<dyn ErasedRpcService>` for storage.
///
/// The `extensions` parameter is a per-request type-map shared across the
/// middleware chain. Middleware can insert typed values (e.g. authenticated
/// user info) for downstream middleware or the transport layer to read.
pub trait RpcService<Ctx> {
    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        path: &'a str,
        input: Value,
        extensions: &'a mut Extensions,
    ) -> impl Future<Output = Result<Value, RpcErr>> + 'a;
}

/// Dyn-compatible version of [`RpcService`] for storage behind `Arc`.
///
/// Wraps the RPIT-based `RpcService::call` with a single `Box::pin` at the
/// dyn boundary. Inside the monomorphized chain there are zero allocations.
pub trait ErasedRpcService<Ctx>: Send + Sync {
    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        path: &'a str,
        input: Value,
        extensions: &'a mut Extensions,
    ) -> Pin<Box<dyn Future<Output = Result<Value, RpcErr>> + 'a>>;
}

impl<Ctx: Send + Sync + 'static, T: RpcService<Ctx> + Send + Sync + 'static> ErasedRpcService<Ctx>
    for T
{
    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        path: &'a str,
        input: Value,
        extensions: &'a mut Extensions,
    ) -> Pin<Box<dyn Future<Output = Result<Value, RpcErr>> + 'a>> {
        Box::pin(RpcService::call(self, ctx, path, input, extensions))
    }
}

/// A composable middleware layer, generic over its inner service type `S`.
///
/// Instead of returning a `Box<dyn ErasedRpcService>`, the implementation
/// specifies a concrete [`Service`](Self::Service) type so the entire chain
/// is monomorphized at compile time.
///
/// # Ordering
///
/// Layers are applied LIFO — the last layer added to
/// [`RpcRouterBuilder`](crate::router::RpcRouterBuilder) becomes the
/// outermost (first to receive the call, last to produce the response).
///
/// # When to implement [`RpcLayer`] vs using [`HookLayer`]
///
/// | Situation | Recommendation |
///|---|---|
/// | Simple before/after logic | [`HookLayer`] (closures, no boilerplate) |
/// | Need to hold state (counters, config) | Implement [`RpcLayer`] yourself |
/// | Need to short-circuit | Either — `HookLayer::before` returns `Err`, or custom returns early |
/// | Want to replace the entire call | Implement [`RpcLayer`] — you control whether/when to call `inner` |
///
/// # Example — latency timer
///
/// ```ignore
/// use std::time::Instant;
/// use fnrpc::middleware::{RpcLayer, RpcService};
///
/// struct LatencyLayer;
///
/// struct LatencyService<Ctx, S> {
///     inner: S,
/// }
///
/// impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx>> RpcService<Ctx> for LatencyService<Ctx, S> {
///     async fn call(&self, ctx: &Ctx, path: &str, input: Value, extensions: &mut Extensions) -> Result<Value, RpcErr> {
///         let start = Instant::now();
///         let result = self.inner.call(ctx, path, input, extensions).await;
///         let elapsed = start.elapsed();
///         tracing::info!("{path} took {elapsed:?}");
///         result
///     }
/// }
///
/// impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx>> RpcLayer<Ctx, S> for LatencyLayer {
///     type Service = LatencyService<Ctx, S>;
///     fn layer(&self, inner: S) -> LatencyService<Ctx, S> {
///         LatencyService { inner }
///     }
/// }
/// ```
///
/// # Example — auth guard with extensions (short-circuit)
///
/// ```ignore
/// use http::Extensions;
///
/// struct AuthService<Ctx, S> {
///     inner: S,
/// }
///
/// impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx>> RpcService<Ctx> for AuthService<Ctx, S> {
///     async fn call(&self, ctx: &Ctx, path: &str, input: Value, extensions: &mut Extensions) -> Result<Value, RpcErr> {
///         let user = authenticate(ctx)?;
///         extensions.insert(user);
///         self.inner.call(ctx, path, input, extensions).await
///     }
/// }
/// ```
pub trait RpcLayer<Ctx, S: RpcService<Ctx>>: Send + Sync {
    /// The concrete service type produced by this layer.
    type Service: RpcService<Ctx>;

    /// Wrap `inner` with this layer's logic, returning a new service.
    fn layer(&self, inner: S) -> Self::Service;
}

// ── Hook layer (convenience) ──────────────────────────────

type BeforeHook<Ctx> =
    Arc<dyn Fn(&Ctx, &str, &mut Value) -> Result<(), RpcErr> + Send + Sync>;

type AfterHook<Ctx> =
    Arc<dyn Fn(&Ctx, &str, &mut Result<Value, RpcErr>) + Send + Sync>;

/// A convenience layer with before/after hooks.
///
/// Use this when you don't need to hold state or write a full [`RpcLayer`]
/// implementation — just attach closures for before/after logic.
///
/// # Examples
///
/// Logging (before + after):
///
/// ```ignore
/// RpcRouterBuilder::new()
///     .query(health)
///     .layer(
///         HookLayer::new()
///             .before(|ctx, path, input| {
///                 tracing::info!("calling {path}");
///                 Ok(())
///             })
///             .after(|ctx, path, result| {
///                 tracing::info!("{path} returned");
///             }),
///     )
///     .build();
/// ```
///
/// Auth guard — short-circuit with `Err`:
///
/// ```ignore
/// HookLayer::new()
///     .before(|ctx, path, input| {
///         if !is_admin(ctx) {
///             return Err(RpcErr::new("FORBIDDEN", "admin only"));
///         }
///         Ok(())
///     });
/// ```
///
/// Modify input before reaching the handler:
///
/// ```ignore
/// HookLayer::new()
///     .before(|ctx, path, input| {
///         if let Some(obj) = input.as_object_mut() {
///             obj.insert("timestamp".into(), Value::from(chrono::Utc::now().timestamp()));
///         }
///         Ok(())
///     });
/// ```
///
/// Override the output in an after-hook:
///
/// ```ignore
/// HookLayer::new()
///     .after(|ctx, path, result| {
///         if let Ok(val) = result {
///             val["queriedAt"] = json!(chrono::Utc::now().to_rfc3339());
///         }
///     });
/// ```
pub struct HookLayer<Ctx> {
    before: Option<BeforeHook<Ctx>>,
    after: Option<AfterHook<Ctx>>,
}

impl<Ctx> HookLayer<Ctx> {
    pub fn new() -> Self {
        Self {
            before: None,
            after: None,
        }
    }

    /// Register a before-hook that runs before the inner service.
    ///
    /// The hook can mutate `input` and return `Err(RpcErr)` to short-circuit.
    ///
    /// # Short-circuit
    ///
    /// ```ignore
    /// HookLayer::new()
    ///     .before(|_ctx, _path, _input| {
    ///         Err(RpcErr::new("RATE_LIMITED", "too many requests"))
    ///     });
    /// ```
    ///
    /// # Modify input
    ///
    /// ```ignore
    /// HookLayer::new()
    ///     .before(|_ctx, _path, input| {
    ///         input["source"] = json!("web");
    ///         Ok(())
    ///     });
    /// ```
    pub fn before<F>(mut self, f: F) -> Self
    where
        F: Fn(&Ctx, &str, &mut Value) -> Result<(), RpcErr> + Send + Sync + 'static,
    {
        self.before = Some(Arc::new(f));
        self
    }

    /// Register an after-hook that runs after the inner service completes.
    ///
    /// The hook receives a mutable reference to the result (writable).
    /// Use this to enrich, filter, or log the response.
    ///
    /// # Enrich output
    ///
    /// ```ignore
    /// HookLayer::new()
    ///     .after(|_ctx, path, result| {
    ///         if let Ok(val) = result {
    ///             val["cached"] = json!(false);
    ///         }
    ///     });
    /// ```
    pub fn after<F>(mut self, f: F) -> Self
    where
        F: Fn(&Ctx, &str, &mut Result<Value, RpcErr>) + Send + Sync + 'static,
    {
        self.after = Some(Arc::new(f));
        self
    }
}

pub struct HookService<Ctx, S> {
    inner: S,
    before: Option<BeforeHook<Ctx>>,
    after: Option<AfterHook<Ctx>>,
}

impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx>> RpcService<Ctx> for HookService<Ctx, S> {
    async fn call(
        &self,
        ctx: &Ctx,
        path: &str,
        mut input: Value,
        extensions: &mut Extensions,
    ) -> Result<Value, RpcErr> {
        if let Some(ref before) = self.before {
            before(ctx, path, &mut input)?;
        }
        let mut result = self.inner.call(ctx, path, input, extensions).await;
        if let Some(ref after) = self.after {
            after(ctx, path, &mut result);
        }
        result
    }
}

impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx>> RpcLayer<Ctx, S> for HookLayer<Ctx> {
    type Service = HookService<Ctx, S>;

    fn layer(&self, inner: S) -> Self::Service {
        HookService {
            inner,
            before: self.before.clone(),
            after: self.after.clone(),
        }
    }
}

// ── ClosureService (closure-based middleware) ──────────────

/// A middleware service wrapping an arbitrary closure.
///
/// Created by [`RpcRouterBuilder::layer_fn`](crate::router::RpcRouterBuilder::layer_fn).
/// The closure is stored inline. Each call allocates one `Box::pin` for the
/// returned future — matching xitca-web's `BoxedServiceObject` boundary cost.
///
/// The closure receives `(&S, &Ctx, &str, Value, &mut Extensions)` and returns
/// `Result<Value, RpcErr>`.  Call `inner.call(ctx, path, input, extensions).await`
/// to delegate to the inner service.  Wrap the body in `Box::pin(async move { ... })`.
///
/// # Example — auth guard
///
/// ```ignore
/// RpcRouterBuilder::<MyCtx>::new()
///     .route(protected_handler)
///     .layer_fn(|inner, ctx, path, input, extensions| {
///         Box::pin(async move {
///             if !ctx.is_authenticated() {
///                 return Err(RpcErr::new("UNAUTHORIZED", "login required"));
///             }
///             inner.call(ctx, path, input, extensions).await
///         })
///     })
///     .build();
/// ```
pub struct ClosureService<Ctx, S, F> {
    pub(crate) inner: S,
    pub(crate) func: F,
    pub(crate) _marker: PhantomData<Ctx>,
}

impl<Ctx: Send + Sync + 'static, S: Send + Sync + 'static, F> RpcService<Ctx>
    for ClosureService<Ctx, S, F>
where
    F: for<'a> Fn(&'a S, &'a Ctx, &'a str, Value, &'a mut Extensions)
            -> Pin<Box<dyn Future<Output = Result<Value, RpcErr>> + Send + 'a>>
        + Send + Sync + 'static,
{
    async fn call(
        &self,
        ctx: &Ctx,
        path: &str,
        input: Value,
        extensions: &mut Extensions,
    ) -> Result<Value, RpcErr> {
        (self.func)(&self.inner, ctx, path, input, extensions).await
    }
}

// ── Tracing layer (feature = "tracing") ───────────────────

/// A logging layer that emits structured [`tracing`] events per call.
///
/// Logs path, input, output/error, and latency for every dispatched call.
/// Only available with `feature = "tracing"`.
#[cfg(feature = "tracing")]
pub struct TracingLayer;

#[cfg(feature = "tracing")]
pub struct TracingService<Ctx, S> {
    inner: S,
}

#[cfg(feature = "tracing")]
impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx>> RpcService<Ctx> for TracingService<Ctx, S> {
    async fn call(
        &self,
        ctx: &Ctx,
        path: &str,
        input: Value,
        extensions: &mut Extensions,
    ) -> Result<Value, RpcErr> {
        let start = std::time::Instant::now();
        let input_str = serde_json::to_string(&input).unwrap_or_default();
        let result = self.inner.call(ctx, path, input, extensions).await;
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        match &result {
            Ok(output) => {
                let output_str = serde_json::to_string(output).unwrap_or_default();
                tracing::info!(
                    path = %path,
                    input = %input_str,
                    output = %output_str,
                    latency_ms = %latency_ms,
                    "rpc_call",
                );
            }
            Err(e) => {
                tracing::error!(
                    path = %path,
                    input = %input_str,
                    error = %e,
                    latency_ms = %latency_ms,
                    "rpc_call",
                );
            }
        }
        result
    }
}

#[cfg(feature = "tracing")]
impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx>> RpcLayer<Ctx, S> for TracingLayer {
    type Service = TracingService<Ctx, S>;

    fn layer(&self, inner: S) -> Self::Service {
        TracingService { inner }
    }
}
