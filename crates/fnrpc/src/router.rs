//! The RPC router — collects handlers and dispatches calls.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::error::RpcErr;
use crate::handler::{ErasedHandler, ErasedSubscribeHandler};
use crate::middleware::{FnLayer, FnService};

/// A collection of RPC handlers organised by name.
///
/// # Example
///
/// ```ignore
/// let router = RpcRouter::<Ctx>::new()
///     .query(health_check)
///     .mutate(create_user)
///     .subscribe(watch_user)
///     .layer(HookLayer::new().before(log_invoke));
/// ```
pub struct RpcRouter<Ctx> {
    pub(crate) inner: Arc<RpcRouterInner<Ctx>>,
}

pub(crate) struct RpcRouterInner<Ctx> {
    pub(crate) handlers: BTreeMap<&'static str, Arc<dyn ErasedHandler<Ctx>>>,
    pub(crate) subscribes: BTreeMap<&'static str, Arc<dyn ErasedSubscribeHandler<Ctx>>>,
    layers: Vec<Box<dyn FnLayer<Ctx>>>,
}

impl<Ctx> Clone for RpcRouter<Ctx> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<Ctx> RpcRouter<Ctx>
where
    Ctx: Send + Sync + 'static,
{
    /// Create an empty router.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RpcRouterInner {
                handlers: BTreeMap::new(),
                subscribes: BTreeMap::new(),
                layers: Vec::new(),
            }),
        }
    }

    /// Register a handler by its [`ErasedHandler::name`].
    ///
    /// Normally you use the typed helpers [`query`](Self::query),
    /// [`mutate`](Self::mutate), or [`subscribe`](Self::subscribe) instead.
    pub fn route<H: ErasedHandler<Ctx> + 'static>(self, handler: H) -> Self {
        let name = handler.name();
        let mut inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| unreachable!("consumed self => sole owner"));
        inner.handlers.insert(name, Arc::new(handler));
        Self {
            inner: Arc::new(inner),
        }
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
    pub fn subscribe<H: ErasedSubscribeHandler<Ctx> + 'static>(self, handler: H) -> Self {
        let name = handler.name();
        let mut inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| unreachable!("consumed self => sole owner"));
        inner.subscribes.insert(name, Arc::new(handler));
        Self {
            inner: Arc::new(inner),
        }
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
    /// dispatched via [`dispatch`](Self::dispatch). Subscribe handlers
    /// bypass the middleware stack — they are looked up directly via
    /// [`get_sub_handler`](Self::get_sub_handler).
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
    /// router
    ///     .layer(HookLayer::new()
    ///         .before(|ctx, path, input| {
    ///             tracing::info!("{path} called");
    ///             Ok(())
    ///         })
    ///     )
    ///     .layer(TracingLayer);
    /// ```
    pub fn layer<L: FnLayer<Ctx> + 'static>(self, layer: L) -> Self {
        let mut inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| unreachable!("consumed self => sole owner"));
        inner.layers.push(Box::new(layer));
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Dispatch a query/mutate call through the middleware stack.
    ///
    /// Returns `Ok(Value)` on success or `Err(RpcErr)` on failure.
    /// For subscribe calls, use [`get_sub_handler`](Self::get_sub_handler) instead.
    pub async fn dispatch(&self, ctx: &Ctx, path: &str, input: Value) -> Result<Value, RpcErr> {
        let mut svc: Box<dyn FnService<Ctx>> = Box::new(RouterService {
            handlers: self.inner.handlers.clone(),
        });
        for layer in self.inner.layers.iter() {
            svc = layer.layer(svc);
        }
        svc.call(ctx, path, input).await
    }

    /// Retrieve a subscribe handler by path (owned `Arc` for `'static` usage).
    pub fn get_sub_handler(&self, path: &str) -> Option<Arc<dyn ErasedSubscribeHandler<Ctx>>> {
        self.inner.subscribes.get(path).cloned()
    }

    /// Return the procedure kind for a given path: `"query"`, `"mutate"`, `"subscribe"`, or `None`.
    pub fn get_procedure_kind(&self, path: &str) -> Option<&'static str> {
        if self.inner.handlers.contains_key(path) {
            self.inner.handlers.get(path).map(|h| h.kind())
        } else if self.inner.subscribes.contains_key(path) {
            Some("subscribe")
        } else {
            None
        }
    }

    /// Return the HTTP method for a given path: `"GET"` for query/subscribe, `"POST"` for mutate.
    pub fn get_procedure_method(&self, path: &str) -> &'static str {
        if self.inner.handlers.contains_key(path) {
            let handler = self.inner.handlers.get(path).unwrap();
            match handler.kind() {
                "mutate" => "POST",
                _ => "GET",
            }
        } else if self.inner.subscribes.contains_key(path) {
            self.inner.subscribes.get(path).map(|s| s.method()).unwrap_or("GET")
        } else {
            "GET"
        }
    }

}

/// Inner service that dispatches to handlers directly.
struct RouterService<Ctx> {
    handlers: BTreeMap<&'static str, Arc<dyn ErasedHandler<Ctx>>>,
}

#[async_trait::async_trait]
impl<Ctx: Send + Sync + 'static> FnService<Ctx> for RouterService<Ctx> {
    async fn call(&self, ctx: &Ctx, path: &str, input: Value) -> Result<Value, RpcErr> {
        let handler = self
            .handlers
            .get(path)
            .ok_or_else(|| RpcErr::not_found(format!("unknown path: {path}")))?;
        handler.call(ctx, input).await
    }
}
