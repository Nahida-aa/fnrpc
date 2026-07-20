//! The RPC router — collects procedure metadata and dispatches calls.
//!
//! Use [`RpcRouterBuilder`] to register handlers, then
//! [`build`](RpcRouterBuilder::build) to get a stored [`RpcRouter`].

use std::any::TypeId;
use std::borrow::Cow;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use http::Extensions;
use serde_json::Value;
use specta::Type;
use xitca_router::Router;

use crate::error::RpcErr;
use crate::handler::{BytesHandlerFn, Handler, HandlerFn, RpcFn, RpcFnExt, TsTypeInfo};
use crate::gen_ts_client;
use crate::middleware::{RpcLayer, RpcService};

/// Metadata for a single procedure, used by TypeScript codegen.
#[derive(Debug, Clone)]
pub struct ProcedureMeta {
    pub key: &'static str,
    pub kind: &'static str,
    pub method: &'static str,
    pub input: TsTypeInfo,
    pub output: TsTypeInfo,
}

/// A collection of RPC handlers with radix-tree routing.
///
/// Produced by [`RpcRouterBuilder::build`].
///
/// Generic over the service type `S` — the middleware chain is monomorphized
/// at compile time with zero indirection overhead.
pub struct RpcRouter<Ctx: Send + Sync + 'static, S: RpcService<Ctx> + Send + Sync + 'static = InnerService<Ctx>> {
    pub(crate) procedures: Vec<ProcedureMeta>,
    pub inner: S,
    pub(crate) types: specta::Types,
    _ctx: PhantomData<Ctx>,
}

impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx> + Send + Sync> RpcRouter<Ctx, S> {
    /// Iterate over all procedure metadata for TypeScript codegen.
    pub fn procedures(&self) -> &[ProcedureMeta] {
        &self.procedures
    }

    /// Generate TypeScript client code.
    pub fn generate_ts_client(&self, rpc_url: &str) -> String {
        gen_ts_client::generate_ts_client(self, rpc_url)
    }
}

impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr> + Send + Sync>
    RpcRouter<Ctx, S>
{
    /// Look up a handler by path and call it directly, bypassing middleware.
    pub async fn call_handler(
        &self,
        path: &str,
        ctx: &Ctx,
        input: &[u8],
        is_get: bool,
    ) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
        self.dispatch(ctx, path, input, is_get).await
    }

    /// Dispatch a call through the middleware stack.
    ///
    /// This call is fully monomorphized — zero `Box::pin`, zero vtable dispatch.
    pub async fn dispatch(&self, ctx: &Ctx, path: &str, input: &[u8], is_get: bool) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
        let mut extensions = Extensions::new();
        self.inner.call(ctx, path, input, is_get, &mut extensions).await
    }
}

// ── InnerService ────────────────────────────────────────

/// Inner service that dispatches to handlers via radix tree.
/// Used as the base of the middleware chain.
///
/// Handlers are stored in `Arc<Handler<Ctx>>` so we can clone the `Arc`
/// out of the router, drop the lock, and then call the handler without
/// holding the lock across `.await`.
#[doc(hidden)]
pub struct InnerService<Ctx: Send + Sync + 'static> {
    router: Arc<std::sync::Mutex<Router<Arc<Handler<Ctx>>>>>,
}

impl<Ctx: Send + Sync + 'static> RpcService<Ctx> for InnerService<Ctx> {
    type Response = (Cow<'static, [u8]>, bool);
    type Error = RpcErr;

    async fn call(
        &self,
        ctx: &Ctx,
        path: &str,
        input: &[u8],
        is_get: bool,
        _extensions: &mut Extensions,
    ) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
        // Clone the Arc<Handler> out of the router, drop the lock, then call.
        let handler = {
            let router = self.router.lock().unwrap();
            match router.at(path).ok() {
                Some(m) => Arc::clone(m.value),
                None => return Err(RpcErr::not_found(format!("unknown path: {path}"))),
            }
        };
        let (bytes, is_json) = handler.call(ctx, input, is_get).await?;
        Ok((bytes, is_json))
    }
}

// ── RpcRouterBuilder ────────────────────────────────────

/// Builder for an [`RpcRouter`].
///
/// Uses a shared `Arc<Mutex<Router<Arc<Handler<Ctx>>>>>` that both the builder
/// and [`InnerService`] reference. Handlers are stored as `Arc` so they can be
/// cheaply cloned out of the router at request time.
pub struct RpcRouterBuilder<Ctx: Send + Sync + 'static, S: RpcService<Ctx> + Send + Sync + 'static = InnerService<Ctx>> {
    procedures: Vec<ProcedureMeta>,
    /// Shared router — builder and InnerService both hold clones of this Arc.
    shared_router: Arc<std::sync::Mutex<Router<Arc<Handler<Ctx>>>>>,
    /// Shared specta type registry for TypeScript codegen.
    types: specta::Types,
    service: S,
}

impl<Ctx: Send + Sync + 'static> RpcRouterBuilder<Ctx> {
    /// Create an empty router builder.
    pub fn new() -> Self {
        let shared_router = Arc::new(std::sync::Mutex::new(Router::new()));
        let mut types = specta::Types::default();
        // Register RpcErr so it appears in TypeScript type definitions
        crate::error::RpcErr::definition(&mut types);
        Self {
            procedures: Vec::new(),
            shared_router: Arc::clone(&shared_router),
            types,
            service: InnerService {
                router: shared_router,
            },
        }
    }
}

impl<Ctx: Send + Sync + 'static, S: RpcService<Ctx> + Send + Sync + 'static> RpcRouterBuilder<Ctx, S> {
    /// Register a typed RPC function (query or mutate).
    ///
    /// The handler must implement [`RpcFn<Ctx>`]. Use the proc macros
    /// `#[rpc_query]` or `#[rpc_mutate]` to generate the implementation.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use fnrpc::router::RpcRouterBuilder;
    ///
    /// #[fnrpc::rpc_query]
    /// async fn health() -> &'static str { "ok" }
    ///
    /// RpcRouterBuilder::<()>::new()
    ///     .route_fn(health)
    ///     .build();
    /// ```
    pub fn route_fn<H: RpcFn<Ctx> + 'static>(mut self, handler: H) -> Self {
        // Register input/output types into shared specta registry
        let input_dt = H::Input::definition(&mut self.types);
        let output_dt = H::Output::definition(&mut self.types);

        self.procedures.push(ProcedureMeta {
            key: H::KEY,
            kind: H::KIND,
            method: H::METHOD,
            input: TsTypeInfo {
                ts_ref: gen_ts_client::resolve_ts_ref(&input_dt, &self.types),
            },
            output: TsTypeInfo {
                ts_ref: gen_ts_client::resolve_ts_ref(&output_dt, &self.types),
            },
        });

        let skip_query = TypeId::of::<H::Input>() == TypeId::of::<()>();
        struct RpcHandler<Ctx, H: RpcFn<Ctx>>(H, PhantomData<Ctx>);
        impl<Ctx: Send + Sync + 'static, H: RpcFn<Ctx>> HandlerFn<Ctx> for RpcHandler<Ctx, H> {
            fn call<'a>(
                &'a self,
                ctx: &'a Ctx,
                input: Value,
            ) -> Pin<Box<dyn Future<Output = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'a>>
            {
                Box::pin(async move {
                    let result = self.0.call_value(ctx, input).await?;
                    Ok(result)
                })
            }
        }
        let handler_fn = Handler::Rpc {
            f: Box::new(RpcHandler(handler, PhantomData)),
            skip_query,
        };
        self.shared_router.lock().unwrap().insert(H::KEY.to_string(), Arc::new(handler_fn)).unwrap();
        self
    }

    /// Register a bytes handler (bypasses JSON serialization).
    ///
    /// Use `#[rpc_bytes]` to generate the implementation.
    /// Raw handlers are not included in TypeScript codegen.
    pub fn route_bytes<F: crate::handler::RawRpcFn<Ctx> + 'static>(mut self, handler: F) -> Self {
        struct BytesHandler<Ctx, F: crate::handler::RawRpcFn<Ctx>>(F, PhantomData<Ctx>);
        impl<Ctx: Send + Sync + 'static, F: crate::handler::RawRpcFn<Ctx>> BytesHandlerFn<Ctx>
            for BytesHandler<Ctx, F>
        {
            fn call<'a>(
                &'a self,
                ctx: &'a Ctx,
                input: &'a [u8],
            ) -> Pin<Box<dyn Future<Output = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'a>>
            {
                Box::pin(async move {
                    let result = F::exec(ctx, input).await?;
                    Ok(result)
                })
            }
        }
        let handler_fn = Handler::Bytes(Box::new(BytesHandler(handler, PhantomData)));
        self.shared_router.lock().unwrap().insert(F::KEY.to_string(), Arc::new(handler_fn)).unwrap();
        self
    }

    /// Register a subscribe handler.
    pub fn subscribe<H: crate::handler::RpcSubscribe<Ctx> + 'static>(
        mut self,
        _handler: H,
    ) -> Self {
        self.procedures.push(ProcedureMeta {
            key: H::KEY,
            kind: "subscribe",
            method: H::METHOD,
            input: gen_ts_client::type_ts::<H::Input>(),
            output: gen_ts_client::type_ts::<H::Output>(),
        });
        self
    }

    /// Finalize and produce a [`RpcRouter`] with a concrete service type.
    ///
    /// The returned router is fully monomorphized — zero `Box::pin`, zero vtable dispatch.
    pub fn build(self) -> RpcRouter<Ctx, S>
    where
        S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr>,
    {
        RpcRouter {
            procedures: self.procedures,
            inner: self.service,
            types: self.types,
            _ctx: PhantomData,
        }
    }

    /// Attach a middleware layer.
    ///
    /// Layers are applied LIFO — the last layer added becomes the outermost.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use fnrpc::middlewares::hook::HookLayer;
    ///
    /// RpcRouterBuilder::<()>::new()
    ///     .route_fn(health)
    ///     .layer(HookLayer::new().before(|_ctx, _path, input, _is_get| Ok(input)))
    ///     .build();
    /// ```
    pub fn layer<L: RpcLayer<Ctx, S> + 'static>(
        self,
        layer: L,
    ) -> RpcRouterBuilder<Ctx, L::Service> {
        RpcRouterBuilder {
            procedures: self.procedures,
            shared_router: self.shared_router,
            types: self.types,
            service: layer.layer(self.service),
        }
    }

    /// Attach a closure-based middleware layer.
    pub fn layer_fn<F>(self, func: F) -> RpcRouterBuilder<Ctx, crate::middleware::ClosureService<Ctx, S, F>>
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
        S: Send + Sync + 'static,
    {
        RpcRouterBuilder {
            procedures: self.procedures,
            shared_router: self.shared_router,
            types: self.types,
            service: crate::middleware::ClosureService {
                inner: self.service,
                func,
                _marker: std::marker::PhantomData,
            },
        }
    }
}

impl<Ctx: Send + Sync + 'static> Default for RpcRouterBuilder<Ctx> {
    fn default() -> Self {
        Self::new()
    }
}
