//! Middleware system for fnrpc.
//!
//! Inspired by xitca-web's zero-cost middleware architecture:
//! - [`RpcService`] — the core callable trait (RPIT-based, zero `Box::pin`
//!   inside the monomorphized chain). Operates on `&[u8]` for zero serde overhead.
//! - [`RpcLayer`] — a composable middleware layer (generic over inner `S`).
//! - [`PipelineT`] — zero-cost middleware composition via phantom markers.
//! - [`AsyncFnMiddleware`] — adapt an async function as middleware.
//! - [`HookLayer`] — before/after hooks (convenience).
//! - [`TracingLayer`] — structured logging (feature = `"tracing"`).
//!
//! # How middleware works
//!
//! Middleware follows xitca's two-phase pattern:
//! 1. **Build phase**: [`RpcLayer`] receives the inner service `S` via
//!    [`layer`](RpcLayer::layer) and returns a wrapped service.
//! 2. **Run phase**: The wrapped service's [`RpcService::call`] handles requests.
//!
//! Layers are applied LIFO — the last layer added becomes the outermost.

use std::borrow::Cow;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use http::Extensions;

use crate::error::RpcErr;

// ── Core Service trait ──────────────────────────────────

/// Core service trait — dispatch an RPC call with raw bytes.
///
/// Uses **RPIT (return-position `impl Trait`)** — no `#[async_trait]`,
/// no hidden `Box::pin` allocation per call inside the monomorphized chain.
///
/// Operates on `&[u8]` instead of `Value` to eliminate serde_json
/// serialization/deserialization overhead in the middleware chain.
///
/// The entire middleware chain is monomorphized at compile time — zero
/// indirection, zero vtable dispatch. See [`RpcRouter`](crate::router::RpcRouter)
/// for the stored router type.
pub trait RpcService<Ctx> {
    /// The response type (always `Result<(Cow<'static, [u8]>, bool), RpcErr>`).
    type Response;
    /// The error type (always `RpcErr`).
    type Error;

    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        path: &'a str,
        input: &'a [u8],
        is_get: bool,
        extensions: &'a mut Extensions,
    ) -> impl Future<Output = Result<Self::Response, Self::Error>> + 'a;
}

// ── RpcLayer trait ─────────────────────────────────────

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
/// |---|---|
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
/// impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr>>
///     RpcService<Ctx> for LatencyService<Ctx, S>
/// {
///     type Response = Cow<'static, [u8]>;
///     type Error = RpcErr;
///
///     async fn call(&self, ctx: &Ctx, path: &str, input: &[u8], is_get: bool, extensions: &mut Extensions) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
///         let start = Instant::now();
///         let result = self.inner.call(ctx, path, input, is_get, extensions).await;
///         tracing::info!("{path} took {:?}", start.elapsed());
///         result
///     }
/// }
///
/// impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr>>
///     RpcLayer<Ctx, S> for LatencyLayer
/// {
///     type Service = LatencyService<Ctx, S>;
///     fn layer(&self, inner: S) -> LatencyService<Ctx, S> {
///         LatencyService { inner }
///     }
/// }
/// ```
pub trait RpcLayer<Ctx, S: RpcService<Ctx>>: Send + Sync {
    /// The concrete service type produced by this layer.
    type Service: RpcService<Ctx> + Send + Sync;

    /// Wrap `inner` with this layer's logic, returning a new service.
    fn layer(&self, inner: S) -> Self::Service;
}

// ── PipelineT (zero-cost composition) ──────────────────

/// Phantom marker types for different pipeline semantics.
pub mod marker {
    /// A middleware builder composition marker.
    pub struct BuildEnclosed;
}

/// A two-field pipeline type for zero-cost middleware composition.
///
/// The phantom `M` parameter selects different `Service` implementations
/// at compile time — no runtime dispatch overhead.
///
/// `PipelineT<F, T, BuildEnclosed>` is used at build time only:
/// `F` is the outer service builder, `T` is the middleware layer.
pub struct PipelineT<F, S, M = ()> {
    pub first: F,
    pub second: S,
    _marker: PhantomData<M>,
}

impl<F, S, M> PipelineT<F, S, M> {
    pub const fn new(first: F, second: S) -> Self {
        Self {
            first,
            second,
            _marker: PhantomData,
        }
    }
}

impl<F: Clone, S: Clone, M> Clone for PipelineT<F, S, M> {
    fn clone(&self) -> Self {
        Self {
            first: self.first.clone(),
            second: self.second.clone(),
            _marker: PhantomData,
        }
    }
}

// ── AsyncFnMiddleware (function-as-middleware adapter) ──

/// Wraps an async function as a middleware layer.
///
/// The function receives `(&S, &Ctx, &str, &[u8], bool, &mut Extensions)` where
/// `S` is the inner service. Call `inner.call(ctx, path, input, is_get, extensions).await`
/// to delegate to the inner service.
///
/// # Example
///
/// ```ignore
/// use std::sync::Arc;
/// use fnrpc::middleware::{RpcLayer, RpcService, AsyncFnMiddleware};
///
/// async fn logging_mw<S>(inner: &S, ctx: &(), path: &str, input: &[u8], is_get: bool, extensions: &mut http::Extensions) -> Result<std::borrow::Cow<'static, [u8]>, fnrpc::error::RpcErr>
/// where
///     S: RpcService<(), Response = std::borrow::Cow<'static, [u8]>, Error = fnrpc::error::RpcErr>,
/// {
///     println!("calling {path}");
///     let result = inner.call(ctx, path, input, is_get, extensions).await;
///     println!("{path} done");
///     result
/// }
///
/// let layer = AsyncFnMiddleware(logging_mw);
/// ```
pub struct AsyncFnMiddleware<F>(pub F);

impl<F> Clone for AsyncFnMiddleware<F>
where
    F: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// Build phase: AsyncFnMiddleware as RpcLayer — wraps S into AsyncFnService<S, F>
impl<Ctx: Send + Sync + 'static, S, F> RpcLayer<Ctx, S> for AsyncFnMiddleware<F>
where
    Ctx: Send + Sync + 'static,
    S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr> + Send + Sync + 'static,
    F: for<'a> Fn(
            &'a S,
            &'a Ctx,
            &'a str,
            &'a [u8],
            bool,
            &'a mut Extensions,
        ) -> Pin<Box<dyn Future<Output = Result<(Cow<'static, [u8]>, bool), RpcErr>> + Send + 'a>>
        + Clone + Send + Sync + 'static,
{
    type Service = AsyncFnService<Ctx, S, F>;

    fn layer(&self, inner: S) -> Self::Service {
        AsyncFnService {
            inner,
            func: self.0.clone(),
            _marker: PhantomData,
        }
    }
}

/// Run phase: the middleware service produced by AsyncFnMiddleware.
pub struct AsyncFnService<Ctx, S, F> {
    inner: S,
    func: F,
    _marker: PhantomData<Ctx>,
}

impl<Ctx: Send + Sync + 'static, S, F> RpcService<Ctx> for AsyncFnService<Ctx, S, F>
where
    S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr>,
    F: for<'a> Fn(
        &'a S,
        &'a Ctx,
        &'a str,
        &'a [u8],
        bool,
        &'a mut Extensions,
    ) -> Pin<Box<dyn Future<Output = Result<(Cow<'static, [u8]>, bool), RpcErr>> + Send + 'a>>,
{
    type Response = (Cow<'static, [u8]>, bool);
    type Error = RpcErr;

    async fn call(
        &self,
        ctx: &Ctx,
        path: &str,
        input: &[u8],
        is_get: bool,
        extensions: &mut Extensions,
    ) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
        (self.func)(&self.inner, ctx, path, input, is_get, extensions).await
    }
}

// ── ServiceExt trait ──────────────────────────────────

/// Extension trait for [`RpcService`] providing combinator methods.
pub trait ServiceExt<Ctx>: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr> {
    /// Wrap this service with an async function middleware.
    fn enclosed_fn<F>(self, func: F) -> AsyncFnService<Ctx, Self, F>
    where
        Self: Sized + Send + Sync + 'static,
        F: for<'a> Fn(
                &'a Self,
                &'a Ctx,
                &'a str,
                &'a [u8],
                bool,
                &'a mut Extensions,
            ) -> Pin<Box<dyn Future<Output = Result<(Cow<'static, [u8]>, bool), RpcErr>> + Send + 'a>>
            + Clone + Send + Sync + 'static,
        Ctx: Send + Sync + 'static,
    {
        AsyncFnMiddleware(func).layer(self)
    }
}

impl<Ctx, S> ServiceExt<Ctx> for S where
    S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr>
{
}

/// Extension trait providing a [`next`](NextExt::next) method for middleware closures.
///
/// In a [`layer_fn`](crate::router::RpcRouterBuilder::layer_fn) closure, call
/// `inner.next(ctx, path, input, is_get, extensions).await` to delegate to the
/// inner service — no need to import or qualify the trait method.
pub trait NextExt<Ctx>: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr> {
    /// Delegate to the next (inner) service in the middleware chain.
    async fn next<'a>(
        &'a self,
        ctx: &'a Ctx,
        path: &'a str,
        input: &'a [u8],
        is_get: bool,
        extensions: &'a mut Extensions,
    ) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
        RpcService::call(self, ctx, path, input, is_get, extensions).await
    }
}

impl<Ctx, S> NextExt<Ctx> for S where
    S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr>
{
}

// ── Hook layer (convenience) ──────────────────────────

type BeforeHook<Ctx> =
    Arc<dyn for<'a> Fn(&Ctx, &str, &'a [u8], bool) -> Result<&'a [u8], RpcErr> + Send + Sync>;

type AfterHook<Ctx> =
    Arc<dyn Fn(&Ctx, &str, &mut Result<(Cow<'static, [u8]>, bool), RpcErr>) + Send + Sync>;

/// A convenience layer with before/after hooks.
///
/// Use this when you don't need to hold state or write a full [`RpcLayer`]
/// implementation — just attach closures for before/after logic.
///
/// The before-hook receives `&Ctx, &str, &mut Vec<u8>, bool` — the input bytes
/// are mutable so you can modify them before passing to the inner service.
///
/// # Examples
///
/// Logging (before + after):
///
/// ```ignore
/// RpcRouterBuilder::new()
///     .route_fn(health)
///     .layer(
///         HookLayer::new()
///             .before(|ctx, path, input, is_get| {
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
///     .before(|ctx, path, input, is_get| {
///         if !is_admin(ctx) {
///             return Err(RpcErr::new("FORBIDDEN", "admin only"));
///         }
///         Ok(())
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
    /// The hook receives `(&Ctx, &str, &[u8], bool)` and returns
    /// `Result<&[u8], RpcErr>`. Return `Ok(input)` to pass through unchanged
    /// (zero allocation). Return `Err(RpcErr)` to short-circuit.
    pub fn before<F>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&Ctx, &str, &'a [u8], bool) -> Result<&'a [u8], RpcErr> + Send + Sync + 'static,
    {
        self.before = Some(Arc::new(f));
        self
    }

    /// Register an after-hook that runs after the inner service completes.
    ///
    /// The hook receives a mutable reference to the result (writable).
    pub fn after<F>(mut self, f: F) -> Self
    where
        F: Fn(&Ctx, &str, &mut Result<(Cow<'static, [u8]>, bool), RpcErr>) + Send + Sync + 'static,
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

impl<Ctx: Send + Sync + 'static, S> RpcService<Ctx> for HookService<Ctx, S>
where
    S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr>,
{
    type Response = (Cow<'static, [u8]>, bool);
    type Error = RpcErr;

    async fn call(
        &self,
        ctx: &Ctx,
        path: &str,
        input: &[u8],
        is_get: bool,
        extensions: &mut Extensions,
    ) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
        if let Some(ref before) = self.before {
            let input = before(ctx, path, input, is_get)?;
            let mut result = self.inner.call(ctx, path, input, is_get, extensions).await;
            if let Some(ref after) = self.after {
                after(ctx, path, &mut result);
            }
            result
        } else {
            let mut result = self.inner.call(ctx, path, input, is_get, extensions).await;
            if let Some(ref after) = self.after {
                after(ctx, path, &mut result);
            }
            result
        }
    }
}

impl<Ctx: Send + Sync + 'static, S> RpcLayer<Ctx, S> for HookLayer<Ctx>
where
    S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr> + Send + Sync,
{
    type Service = HookService<Ctx, S>;

    fn layer(&self, inner: S) -> Self::Service {
        HookService {
            inner,
            before: self.before.clone(),
            after: self.after.clone(),
        }
    }
}

// ── ClosureService (closure-based middleware) ─────────

/// A middleware service wrapping an arbitrary closure.
///
/// Created by [`RpcRouterBuilder::layer_fn`](crate::router::RpcRouterBuilder::layer_fn).
/// The closure receives `(&S, &Ctx, &str, &[u8], bool, &mut Extensions)` and returns
/// `Pin<Box<dyn Future<...>>>`. Call `inner.call(ctx, path, input, is_get, extensions).await`
/// to delegate to the inner service.
///
/// # Example — auth guard
///
/// ```ignore
/// RpcRouterBuilder::<MyCtx>::new()
///     .route_fn(protected_handler)
///     .layer_fn(|inner, ctx, path, input, is_get, extensions| {
///         Box::pin(async move {
///             if !ctx.is_authenticated() {
///                 return Err(RpcErr::new("UNAUTHORIZED", "login required"));
///             }
///             inner.call(ctx, path, input, is_get, extensions).await
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
    F: for<'a> Fn(
            &'a S,
            &'a Ctx,
            &'a str,
            &'a [u8],
            bool,
            &'a mut Extensions,
        ) -> Pin<Box<dyn Future<Output = Result<(Cow<'static, [u8]>, bool), RpcErr>> + Send + 'a>>
        + Send + Sync + 'static,
{
    type Response = (Cow<'static, [u8]>, bool);
    type Error = RpcErr;

    async fn call(
        &self,
        ctx: &Ctx,
        path: &str,
        input: &[u8],
        is_get: bool,
        extensions: &mut Extensions,
    ) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
        (self.func)(&self.inner, ctx, path, input, is_get, extensions).await
    }
}

// ── Tracing layer (feature = "tracing") ───────────────

/// A logging layer that emits structured [`tracing`] events per call.
///
/// Only available with `feature = "tracing"`.
#[cfg(feature = "tracing")]
pub struct TracingLayer;

#[cfg(feature = "tracing")]
pub struct TracingService<Ctx, S> {
    inner: S,
}

#[cfg(feature = "tracing")]
impl<Ctx: Send + Sync + 'static, S> RpcService<Ctx> for TracingService<Ctx, S>
where
    S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr>,
{
    type Response = (Cow<'static, [u8]>, bool);
    type Error = RpcErr;

    async fn call(
        &self,
        ctx: &Ctx,
        path: &str,
        input: &[u8],
        is_get: bool,
        extensions: &mut Extensions,
    ) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
        let start = std::time::Instant::now();
        let result = self.inner.call(ctx, path, input, is_get, extensions).await;
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        // Only allocate strings when tracing is enabled at info level
        if tracing::level_enabled!(tracing::Level::INFO) {
            match &result {
                Ok((output, _is_json)) => {
                    let output_str = String::from_utf8_lossy(output);
                    let input_str = String::from_utf8_lossy(input);
                    tracing::info!(
                        path = %path,
                        input = %input_str,
                        output = %output_str,
                        latency_ms = %latency_ms,
                        "rpc_call",
                    );
                }
                Err(e) => {
                    let input_str = String::from_utf8_lossy(input);
                    tracing::error!(
                        path = %path,
                        input = %input_str,
                        error = %e,
                        latency_ms = %latency_ms,
                        "rpc_call",
                    );
                }
            }
        }
        result
    }
}

#[cfg(feature = "tracing")]
impl<Ctx: Send + Sync + 'static, S> RpcLayer<Ctx, S> for TracingLayer
where
    S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr>,
{
    type Service = TracingService<Ctx, S>;

    fn layer(&self, inner: S) -> Self::Service {
        TracingService { inner }
    }
}
