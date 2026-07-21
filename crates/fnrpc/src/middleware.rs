//! Middleware system for fnrpc.
//!
//! Inspired by xitca-web's zero-cost middleware architecture:
//! - [`RpcService`] — the core callable trait (RPIT-based, zero `Box::pin`
//!   inside the monomorphized chain). Operates on `&[u8]` for zero serde overhead.
//! - [`RpcLayer`] — a composable middleware layer (generic over inner `S`).
//! - [`PipelineT`] — zero-cost middleware composition via phantom markers.
//! - [`AsyncFnMiddleware`] — adapt an async function as middleware.
//! - [`middlewares::hook::HookLayer`] — before/after hooks (convenience).
//! - [`middlewares::tracing::TracingLayer`] — structured logging (feature = `"tracing"`).
//!
//! # How middleware works
//!
//! Middleware follows xitca's two-phase pattern:
//! 1. **Build phase**: [`RpcLayer`] receives the inner service `S` via
//!    [`layer`](RpcLayer::layer) and returns a wrapped service.
//! 2. **Run phase**: The wrapped service's [`RpcService::call`] handles requests.
//!
//! Layers are applied LIFO — the last layer added becomes the outermost.

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use http::Extensions;

use crate::error::RpcErr;
use crate::output::RpcOutput;

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
    /// The response type (always `Result<Cow<'static, [u8]>, RpcErr>`).
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
    ) -> impl Future<Output = Result<Self::Response, Self::Error>> + Send + 'a;
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
impl<Ctx, S, F> RpcLayer<Ctx, S> for AsyncFnMiddleware<F>
where
    Ctx: Send + Sync + 'static,
    S: RpcService<Ctx, Response = RpcOutput, Error = RpcErr> + Send + Sync + 'static,
    F: for<'a> Fn(
            &'a S,
            &'a Ctx,
            &'a str,
            &'a [u8],
            bool,
            &'a mut Extensions,
        ) -> Pin<Box<dyn Future<Output = Result<RpcOutput, RpcErr>> + Send + 'a>>
        + Clone
        + Send
        + Sync
        + 'static,
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
    S: RpcService<Ctx, Response = RpcOutput, Error = RpcErr> + Send + Sync,
    F: for<'a> Fn(
            &'a S,
            &'a Ctx,
            &'a str,
            &'a [u8],
            bool,
            &'a mut Extensions,
        ) -> Pin<Box<dyn Future<Output = Result<RpcOutput, RpcErr>> + Send + 'a>>
        + Send
        + Sync,
{
    type Response = RpcOutput;
    type Error = RpcErr;

    async fn call(
        &self,
        ctx: &Ctx,
        path: &str,
        input: &[u8],
        is_get: bool,
        extensions: &mut Extensions,
    ) -> Result<RpcOutput, RpcErr> {
        (self.func)(&self.inner, ctx, path, input, is_get, extensions).await
    }
}

// ── ServiceExt trait ──────────────────────────────────

/// Extension trait for [`RpcService`] providing combinator methods.
pub trait ServiceExt<Ctx>: RpcService<Ctx, Response = RpcOutput, Error = RpcErr> {
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
            )
                -> Pin<Box<dyn Future<Output = Result<RpcOutput, RpcErr>> + Send + 'a>>
            + Clone
            + Send
            + Sync
            + 'static,
        Ctx: Send + Sync + 'static,
    {
        AsyncFnMiddleware(func).layer(self)
    }
}

impl<Ctx, S> ServiceExt<Ctx> for S where S: RpcService<Ctx, Response = RpcOutput, Error = RpcErr> {}

/// Extension trait providing a [`next`](NextExt::next) method for middleware closures.
///
/// In a [`layer_fn`](crate::router::RpcRouterBuilder::layer_fn) closure, call
/// `inner.next(ctx, path, input, is_get, extensions).await` to delegate to the
/// inner service — no need to import or qualify the trait method.
#[allow(async_fn_in_trait)]
pub trait NextExt<Ctx>: RpcService<Ctx, Response = RpcOutput, Error = RpcErr> {
    /// Delegate to the next (inner) service in the middleware chain.
    async fn next<'a>(
        &'a self,
        ctx: &'a Ctx,
        path: &'a str,
        input: &'a [u8],
        is_get: bool,
        extensions: &'a mut Extensions,
    ) -> Result<RpcOutput, RpcErr> {
        RpcService::call(self, ctx, path, input, is_get, extensions).await
    }
}

impl<Ctx, S> NextExt<Ctx> for S where S: RpcService<Ctx, Response = RpcOutput, Error = RpcErr> {}

// ── ClosureService (closure-based middleware) ─────────

/// A middleware service wrapping an arbitrary closure.
///
/// Created by [`RpcRouterBuilder::layer_fn`](crate::router::RpcRouterBuilder::layer_fn).
/// The closure receives `(&S, &Ctx, &str, &[u8], bool, &mut Extensions)` and returns
/// `Pin<Box<dyn Future<...>>>`. Call `inner.call(ctx, path, input, is_get, extensions).await`
/// to delegate to the inner service.
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
        ) -> Pin<Box<dyn Future<Output = Result<RpcOutput, RpcErr>> + Send + 'a>>
        + Send
        + Sync
        + 'static,
{
    type Response = RpcOutput;
    type Error = RpcErr;

    async fn call(
        &self,
        ctx: &Ctx,
        path: &str,
        input: &[u8],
        is_get: bool,
        extensions: &mut Extensions,
    ) -> Result<RpcOutput, RpcErr> {
        (self.func)(&self.inner, ctx, path, input, is_get, extensions).await
    }
}
