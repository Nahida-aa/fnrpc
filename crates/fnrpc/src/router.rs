//! The RPC router — collects handlers and dispatches calls.
//!
//! Use [`RpcRouterBuilder`] to register handlers and middleware, then
//! [`build`](RpcRouterBuilder::build) to get a stored [`RpcRouter`].

use std::collections::BTreeMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use http::Extensions;
use serde_json::Value;

use crate::error::RpcErr;
use crate::handler::{ErasedHandler, ErasedSubscribeHandler};
use crate::middleware::{ErasedFnService, FnLayer, FnService, LayerFnService};

// ── RpcRouterBuilder (concrete, monomorphized chain) ─────

/// Builder for an [`RpcRouter`].
///
/// Registers [`query`](RpcRouterBuilder::query),
/// [`mutate`](RpcRouterBuilder::mutate), and
/// [`subscribe`](RpcRouterBuilder::subscribe) handlers, then wraps the
/// chain with [`layer`](RpcRouterBuilder::layer). Call
/// [`build`](RpcRouterBuilder::build) to produce a type-erased
/// [`RpcRouter`] ready for dispatch.
///
/// # Example
///
/// ```ignore
/// let router = RpcRouterBuilder::<AppCtx>::new()
///     .query(health_check)
///     .mutate(create_user)
///     .subscribe(watch_user)
///     .layer(HookLayer::new().before(log_invoke))
///     .build();
/// ```
pub struct RpcRouterBuilder<Ctx, S = RouterService<Ctx>> {
    service: S,
    handlers: Arc<RwLock<BTreeMap<&'static str, Arc<dyn ErasedHandler<Ctx>>>>>,
    subscribes: BTreeMap<&'static str, Arc<dyn ErasedSubscribeHandler<Ctx>>>,
}

// ── Construction + handler registration ──────────────────

impl<Ctx: Send + Sync + 'static> RpcRouterBuilder<Ctx> {
    /// Create an empty router builder.
    pub fn new() -> Self {
        let handlers = Arc::new(RwLock::new(BTreeMap::new()));
        Self {
            service: RouterService {
                handlers: handlers.clone(),
            },
            handlers,
            subscribes: BTreeMap::new(),
        }
    }
}

impl<Ctx: Send + Sync + 'static, S: FnService<Ctx> + 'static> RpcRouterBuilder<Ctx, S> {
    /// Register a handler by its [`ErasedHandler::name`].
    ///
    /// Normally you use the typed helpers [`query`](Self::query),
    /// [`mutate`](Self::mutate), or [`subscribe`](Self::subscribe) instead.
    pub fn route<H: ErasedHandler<Ctx> + 'static>(self, handler: H) -> Self {
        let name = handler.name();
        self.handlers.write().unwrap().insert(name, Arc::new(handler));
        self
    }

    /// Register a query handler (convenience for [`route`](Self::route)).
    pub fn query<H: ErasedHandler<Ctx> + 'static>(self, handler: H) -> Self {
        self.route(handler)
    }

    /// Register a mutate handler (convenience for [`route`](Self::route)).
    pub fn mutate<H: ErasedHandler<Ctx> + 'static>(self, handler: H) -> Self {
        self.route(handler)
    }

    /// Register a subscribe handler.
    pub fn subscribe<H: ErasedSubscribeHandler<Ctx> + 'static>(mut self, handler: H) -> Self {
        let name = handler.name();
        self.subscribes.insert(name, Arc::new(handler));
        self
    }

    /// Attach a middleware layer.
    ///
    /// Similar to Hono's `.use()`, TanStack Start's `.middleware()`,
    /// or Tower's `.layer()`.
    ///
    /// # Ordering
    ///
    /// Layers compose LIFO — the last layer added is the outermost
    /// (first to intercept the call, last to produce the response).
    ///
    /// # Scope
    ///
    /// Middleware only applies to **query and mutate** procedures
    /// dispatched via [`dispatch`](RpcRouter::dispatch). Subscribe handlers
    /// bypass the middleware stack — they are looked up directly via
    /// [`get_sub_handler`](RpcRouter::get_sub_handler).
    ///
    /// # JSON-level interface
    ///
    /// Layers receive the raw JSON [`Value`](serde_json::Value) input/output,
    /// not the deserialised procedure types. This keeps them transport-agnostic.
    /// A before-hook can mutate the input; an after-hook can inspect or rewrite
    /// the result.
    ///
    /// # Short-circuiting
    ///
    /// A before-hook may return `Err(RpcErr)` to abort the call immediately
    /// without invoking the inner service.
    ///
    /// # Built-in layers
    ///
    /// - [`HookLayer`] — before/after hooks via closures
    /// - [`TracingLayer`] — structured tracing (feature = `"tracing"`)
    ///
    /// # Example
    ///
    /// ```ignore
    /// RpcRouterBuilder::new()
    ///     .query(health)
    ///     .layer(HookLayer::new()
    ///         .before(|ctx, path, input| {
    ///             tracing::info!("{path} called");
    ///             Ok(())
    ///         })
    ///     )
    ///     .layer(TracingLayer)
    ///     .build();
    /// ```
    pub fn layer<L: FnLayer<Ctx, S> + 'static>(self, layer: L) -> RpcRouterBuilder<Ctx, L::Service> {
        RpcRouterBuilder {
            service: layer.layer(self.service),
            handlers: self.handlers,
            subscribes: self.subscribes,
        }
    }

    /// Attach a middleware layer from a closure.
    ///
    /// A lighter-weight alternative to [`layer`](Self::layer) — no need to
    /// define a struct + implement [`FnLayer`]. The closure receives
    /// `(&S, &Ctx, &str, Value, &mut Extensions)` and must return a
    /// `Pin<Box<dyn Future + Send + '_>>`.  Wrap the body in
    /// `Box::pin(async move { ... })`.
    ///
    /// Each call allocates one `Box::pin` for the returned future — matching
    /// xitca-web's `BoxedServiceObject` boundary cost.  For **zero-allocation**
    /// middleware implement [`FnLayer`] directly.
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
    pub fn layer_fn<F>(self, func: F) -> RpcRouterBuilder<Ctx, LayerFnService<Ctx, S, F>>
    where
        F: for<'a> Fn(&'a S, &'a Ctx, &'a str, Value, &'a mut Extensions)
                -> Pin<Box<dyn Future<Output = Result<Value, RpcErr>> + Send + 'a>>
            + Send + Sync + 'static,
    {
        RpcRouterBuilder {
            service: LayerFnService {
                inner: self.service,
                func,
                _marker: PhantomData,
            },
            handlers: self.handlers,
            subscribes: self.subscribes,
        }
    }

    /// Finalize the middleware chain and produce a type-erased [`RpcRouter`].
    ///
    /// After this call the concrete service type `S` is erased behind
    /// `Arc<dyn ErasedFnService<Ctx>>`, making the router cheap to clone and
    /// store in Axum state.
    pub fn build(self) -> RpcRouter<Ctx>
    where
        S: Send + Sync + 'static,
    {
        RpcRouter {
            inner: Arc::new(self.service) as Arc<dyn ErasedFnService<Ctx>>,
            subscribes: Arc::new(self.subscribes),
            handlers: self.handlers,
        }
    }
}

// ── RpcRouter (type-erased, stored in Axum state) ────────

/// A collection of RPC handlers organised by name.
///
/// Produced by [`RpcRouterBuilder::build`]. This type is concrete and
/// storeable (no generic middleware-chain parameter leaking into public API).
///
/// # Dispatch
///
/// ```ignore
/// router.dispatch(&ctx, "health", input).await
/// ```
pub struct RpcRouter<Ctx> {
    inner: Arc<dyn ErasedFnService<Ctx>>,
    pub(crate) subscribes: Arc<BTreeMap<&'static str, Arc<dyn ErasedSubscribeHandler<Ctx>>>>,
    pub(crate) handlers: Arc<RwLock<BTreeMap<&'static str, Arc<dyn ErasedHandler<Ctx>>>>>,
}

impl<Ctx> Clone for RpcRouter<Ctx> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            subscribes: self.subscribes.clone(),
            handlers: self.handlers.clone(),
        }
    }
}

impl<Ctx: Send + Sync + 'static> RpcRouter<Ctx> {
    /// Dispatch a query/mutate call through the middleware stack.
    ///
    /// Returns `Ok(Value)` on success or `Err(RpcErr)` on failure.
    /// For subscribe calls, use [`get_sub_handler`](Self::get_sub_handler) instead.
    pub async fn dispatch(&self, ctx: &Ctx, path: &str, input: Value) -> Result<Value, RpcErr> {
        let mut extensions = Extensions::new();
        self.inner
            .call(ctx, path, input, &mut extensions)
            .await
    }

    /// Dispatch a query/mutate call and return a [`Send`] future.
    ///
    /// Required by multi-threaded runtimes (Axum, Tower, etc.) that
    /// demand the dispatched future be [`Send`].
    ///
    /// # Safety
    ///
    /// This is safe when `Ctx: Send + Sync + 'static` because every
    /// service stored in the router is `Send + Sync + 'static`, and its
    /// call future captures only `&self` (which is Send) and `&Ctx` (which
    /// is Send).  If a custom [`ErasedFnService`] implementation ever
    /// produced a non-`Send` future this would be undefined behaviour.
    ///
    /// All framework-internal implementations (`RouterService`,
    /// `HookService`, `TracingService`, `LayerFnService`, etc.) produce
    /// `Send` futures.
    pub async fn dispatch_send(
        &self,
        ctx: &Ctx,
        path: &str,
        input: Value,
    ) -> Result<Value, RpcErr> {
        let mut extensions = Extensions::new();
        let fut = self.inner.call(ctx, path, input, &mut extensions);
        // SAFETY: see doc — the erased service is Send+Sync+'static,
        // producing a transitively Send future when Ctx is Send+Sync.
        unsafe {
            std::mem::transmute::<
                Pin<Box<dyn Future<Output = Result<Value, RpcErr>> + '_>>,
                Pin<Box<dyn Future<Output = Result<Value, RpcErr>> + Send + '_>>,
            >(fut)
        }
        .await
    }

    /// Retrieve a subscribe handler by path (owned `Arc` for `'static` usage).
    pub fn get_sub_handler(&self, path: &str) -> Option<Arc<dyn ErasedSubscribeHandler<Ctx>>> {
        self.subscribes.get(path).cloned()
    }

    /// Return the procedure kind for a given path: `"query"`, `"mutate"`, `"subscribe"`, or `None`.
    pub fn get_procedure_kind(&self, path: &str) -> Option<&'static str> {
        let handlers = self.handlers.read().unwrap();
        if let Some(h) = handlers.get(path) {
            Some(h.kind())
        } else if self.subscribes.contains_key(path) {
            Some("subscribe")
        } else {
            None
        }
    }

    /// Return the HTTP method for a given path: `"GET"` for query/subscribe, `"POST"` for mutate.
    pub fn get_procedure_method(&self, path: &str) -> &'static str {
        let handlers = self.handlers.read().unwrap();
        if let Some(handler) = handlers.get(path) {
            match handler.kind() {
                "mutate" => "POST",
                _ => "GET",
            }
        } else if let Some(s) = self.subscribes.get(path) {
            s.method()
        } else {
            "GET"
        }
    }
}

// ── RouterService (inner dispatcher) ─────────────────────

/// Inner service that dispatches to handlers directly.
///
/// This is the default service type for [`RpcRouterBuilder`] and is not
/// intended for direct use.
#[doc(hidden)]
pub struct RouterService<Ctx> {
    pub(crate) handlers: Arc<RwLock<BTreeMap<&'static str, Arc<dyn ErasedHandler<Ctx>>>>>,
}

impl<Ctx: Send + Sync + 'static> FnService<Ctx> for RouterService<Ctx> {
    async fn call(
        &self,
        ctx: &Ctx,
        path: &str,
        input: Value,
        _extensions: &mut Extensions,
    ) -> Result<Value, RpcErr> {
        let handler = self
            .handlers
            .read()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| RpcErr::not_found(format!("unknown path: {path}")))?;
        handler.call(ctx, input).await
    }
}
