use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use futures::stream::Stream;
use serde_json::Value;

use crate::error::RpcErr;
use crate::handler::{ErasedHandler, ErasedSubscribeHandler};
use crate::middleware::{FnLayer, FnService};

pub struct RpcRouter<Ctx> {
    pub(crate) inner: Arc<RpcRouterInner<Ctx>>,
}

pub(crate) struct RpcRouterInner<Ctx> {
    pub(crate) handlers: HashMap<&'static str, Arc<dyn ErasedHandler<Ctx>>>,
    pub(crate) subscribes: HashMap<&'static str, Arc<dyn ErasedSubscribeHandler<Ctx>>>,
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
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RpcRouterInner {
                handlers: HashMap::new(),
                subscribes: HashMap::new(),
                layers: Vec::new(),
            }),
        }
    }

    pub fn route<H: ErasedHandler<Ctx> + 'static>(self, handler: H) -> Self {
        let name = handler.name();
        let mut inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| unreachable!("consumed self => sole owner"));
        inner.handlers.insert(name, Arc::new(handler));
        Self {
            inner: Arc::new(inner),
        }
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
    /// or Tower's `.layer()`. The last call becomes the outermost layer.
    pub fn layer<L: FnLayer<Ctx> + 'static>(self, layer: L) -> Self {
        let mut inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| unreachable!("consumed self => sole owner"));
        inner.layers.push(Box::new(layer));
        Self {
            inner: Arc::new(inner),
        }
    }

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

    /// Dispatch a subscribe by path, returning a stream of values.
    pub fn dispatch_subscribe<'a>(
        &'a self,
        ctx: &'a Ctx,
        path: &str,
        input: Value,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Value, RpcErr>> + Send + 'a>>, RpcErr> {
        let handler = self
            .inner
            .subscribes
            .get(path)
            .ok_or_else(|| RpcErr::not_found(format!("unknown subscribe path: {path}")))?;
        Ok(handler.call(ctx, input))
    }

    /// Return the procedure kind for a given path: `"query"`, `"mutate"`, `"subscribe"`, or `None`.
    pub fn get_procedure_kind(&self, path: &str) -> Option<&'static str> {
        if self.inner.handlers.contains_key(path) {
            // Handlers know their own kind ("query" or "mutate")
            self.inner.handlers.get(path).map(|h| h.kind())
        } else if self.inner.subscribes.contains_key(path) {
            Some("subscribe")
        } else {
            None
        }
    }

}

/// Inner service that dispatches to handlers directly.
struct RouterService<Ctx> {
    handlers: HashMap<&'static str, Arc<dyn ErasedHandler<Ctx>>>,
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
