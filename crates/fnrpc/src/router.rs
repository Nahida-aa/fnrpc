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
use crate::middleware::RpcLayer;

/// Metadata for a single procedure, used by TypeScript codegen.
#[derive(Debug, Clone)]
pub struct ProcedureMeta {
    pub key: &'static str,
    pub kind: &'static str,
    pub method: &'static str,
    pub input: TsTypeInfo,
    pub output: TsTypeInfo,
}

// ── ErasedHandler ─────────────────────────────────────

/// Object-safe handler trait for storage behind `Arc`.
/// One `Box::pin` per request at the dispatch boundary.
pub trait ErasedHandler<Ctx>: Send + Sync {
    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        path: &'a str,
        input: &'a [u8],
        is_get: bool,
        extensions: &'a mut Extensions,
    ) -> Pin<Box<dyn Future<Output = Result<(Cow<'static, [u8]>, bool), RpcErr>> + Send + 'a>>;
}

/// Blanket impl: any `RpcService` can be used as `ErasedHandler`.
impl<Ctx: Send + Sync + 'static, T> ErasedHandler<Ctx> for T
where
    T: crate::middleware::RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr>
        + Send + Sync + 'static,
{
    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        path: &'a str,
        input: &'a [u8],
        is_get: bool,
        extensions: &'a mut Extensions,
    ) -> Pin<Box<dyn Future<Output = Result<(Cow<'static, [u8]>, bool), RpcErr>> + Send + 'a>> {
        Box::pin(crate::middleware::RpcService::call(
            self, ctx, path, input, is_get, extensions,
        ))
    }
}

/// Reverse blanket impl: `Box<dyn ErasedHandler<Ctx>>` implements `RpcService<Ctx>`.
/// This allows middleware layers (like `HookLayer`) to wrap erased handlers.
impl<Ctx: Send + Sync + 'static> crate::middleware::RpcService<Ctx>
    for Box<dyn ErasedHandler<Ctx>>
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
        (**self).call(ctx, path, input, is_get, extensions).await
    }
}

// ── RpcRouter ─────────────────────────────────────────

/// A handler slot that can be either a raw handler (no middleware, zero `Box::pin`)
/// or an erased handler (with middleware, one `Box::pin` at the dispatch boundary).
///
/// - [`HandlerSlot::Raw`]: Stores `Arc<Handler<Ctx>>` directly. Used when no middleware
///   layers have been added. `dispatch` calls `Handler::call` directly — zero allocation.
/// - [`HandlerSlot::Erased`]: Stores `Arc<dyn ErasedHandler<Ctx>>`. Used when middleware
///   has been applied. `dispatch` calls through the vtable — one `Box::pin`.
enum HandlerSlot<Ctx: Send + Sync + 'static> {
    Raw(Arc<Handler<Ctx>>),
    Erased(Arc<dyn ErasedHandler<Ctx>>),
}

/// A collection of RPC handlers with radix-tree routing.
///
/// Produced by [`RpcRouterBuilder::build`].
///
/// # Two-phase routing
///
/// 1. **Build phase**: [`RpcRouterBuilder`] collects routes and middleware layers.
///    Middleware is applied to each handler at `route_fn` time (not wrapped around
///    the whole router). This matches xitca's approach.
///
/// 2. **Request phase**: [`dispatch`](RpcRouter::dispatch) looks up the handler
///    from a read-only radix tree and calls it. No locks, no `Arc<Mutex>`.
///
/// # Zero-overhead dispatch
///
/// - **Without middleware**: Handler stored as [`HandlerSlot::Raw`] → calls
///   `Handler::call` directly → **zero `Box::pin`**, zero allocation.
/// - **With middleware**: Handler stored as [`HandlerSlot::Erased`] → calls
///   through `ErasedHandler::call` vtable → **one `Box::pin`** at dispatch boundary.
///
/// `RpcRouter` also implements [`RpcService`] so it can be used directly as an
/// `ErasedHandler` in multi-router mode (see `fnrpc-web`'s `AppBuilder`).
pub struct RpcRouter<Ctx: Send + Sync + 'static> {
    pub(crate) procedures: Vec<ProcedureMeta>,
    /// Frozen radix tree.
    handler_router: Arc<Router<HandlerSlot<Ctx>>>,
    /// Subscribe handlers, keyed by path.
    subscribe_handlers: Vec<(&'static str, Arc<dyn crate::handler::ErasedSubscribeHandler<Ctx>>)>,
    /// Shared specta type registry for TypeScript codegen.
    pub(crate) types: specta::Types,
}

impl<Ctx: Send + Sync + 'static> RpcRouter<Ctx> {
    /// Iterate over all procedure metadata for TypeScript codegen.
    pub fn procedures(&self) -> &[ProcedureMeta] {
        &self.procedures
    }

    /// Generate TypeScript client code.
    pub fn generate_ts_client(&self, rpc_url: &str) -> String {
        gen_ts_client::generate_ts_client(self, rpc_url)
    }

    /// Dispatch a call through the handler's middleware chain.
    ///
    /// If no middleware was applied, calls the handler directly — zero `Box::pin`.
    /// With middleware, one `Box::pin` at the dispatch boundary.
    pub async fn dispatch(&self, ctx: &Ctx, path: &str, input: &[u8], is_get: bool) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
        let slot = match self.handler_router.at(path).ok() {
            Some(m) => m.value,
            None => return Err(RpcErr::not_found(format!("unknown path: {path}"))),
        };
        match slot {
            HandlerSlot::Raw(handler) => {
                handler.call(ctx, input, is_get).await
            }
            HandlerSlot::Erased(handler) => {
                let mut extensions = Extensions::new();
                handler.call(ctx, path, input, is_get, &mut extensions).await
            }
        }
    }

    /// Dispatch a subscribe call and return a stream of values.
    pub fn dispatch_subscribe(
        &self,
        ctx: &Ctx,
        path: &str,
        input: &[u8],
    ) -> Result<
        Pin<Box<dyn futures::Stream<Item = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'static>>,
        RpcErr,
    > {
        let path = path.strip_prefix('/').unwrap_or(path);
        for (key, handler) in &self.subscribe_handlers {
            if *key == path {
                return Ok(handler.call_bytes(ctx, input));
            }
        }
        Err(RpcErr::not_found(format!("unknown subscribe path: {path}")))
    }

    /// Check whether a subscribe handler exists for the given path.
    ///
    /// This is used by transport layers to decide whether to call
    /// [`dispatch_subscribe`](Self::dispatch_subscribe) vs
    /// [`dispatch`](Self::dispatch). It performs the same linear scan
    /// as `dispatch_subscribe` but returns a boolean — no stream is created.
    pub fn has_subscribe(&self, path: &str) -> bool {
        let path = path.strip_prefix('/').unwrap_or(path);
        self.subscribe_handlers.iter().any(|(key, _)| *key == path)
    }

    /// Convert this router into a boxed erased handler (for multi-router mode).
    ///
    /// The returned handler dispatches through this router's radix tree.
    /// Note: this method is primarily used by `fnrpc-web`'s `AppBuilder`.
    pub fn into_handler(self) -> Box<dyn ErasedHandler<Ctx>> {
        // Implemented via ClosureService to avoid ErasedHandler blanket impl conflicts.
        // ClosureService implements RpcService, which gets auto-converted to ErasedHandler.
        let router = Arc::new(self);
        Box::new(RouterIntoHandler { router })
    }
}

/// RpcRouter implements RpcService for direct use as an ErasedHandler
/// in multi-router mode (e.g., `fnrpc-web`'s `AppBuilder`).
///
/// Uses the last path segment as dispatch key (e.g. `/api/greet` → `greet`),
/// which matches the handler's `KEY` constant. This avoids needing prefix
/// information at this level — prefix stripping is handled by the caller.
impl<Ctx: Send + Sync + 'static> crate::middleware::RpcService<Ctx> for RpcRouter<Ctx> {
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
        // Use the last path segment as dispatch key (e.g. "/api/greet" → "greet")
        let dispatch_path = path.trim_start_matches('/').split('/').last().unwrap_or(path);
        // Direct handler lookup — same as dispatch() but avoids the extra function call
        let slot = match self.handler_router.at(dispatch_path).ok() {
            Some(m) => m.value,
            None => return Err(RpcErr::not_found(format!("unknown path: {path}"))),
        };
        match slot {
            HandlerSlot::Raw(handler) => handler.call(ctx, input, is_get).await,
            HandlerSlot::Erased(handler) => {
                let mut ext = Extensions::new();
                handler.call(ctx, path, input, is_get, &mut ext).await
            }
        }
    }
}

/// Helper struct for `RpcRouter::into_handler`.
/// Defined in this module so it can access `handler_router`.
/// The `Send + Sync` bounds are satisfied by `Arc<RpcRouter>`.
struct RouterIntoHandler<Ctx: Send + Sync + 'static> {
    router: Arc<RpcRouter<Ctx>>,
}

impl<Ctx: Send + Sync + 'static> crate::middleware::RpcService<Ctx> for RouterIntoHandler<Ctx> {
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
        // Strip leading slash for handler lookup (keys are bare like "echo", not "/echo")
        let lookup_path = path.strip_prefix('/').unwrap_or(path);
        match self.router.handler_router.at(lookup_path).ok() {
            Some(m) => match m.value {
                HandlerSlot::Raw(handler) => handler.call(ctx, input, is_get).await,
                HandlerSlot::Erased(handler) => handler.call(ctx, path, input, is_get, extensions).await,
            },
            None => Err(RpcErr::not_found(format!("unknown path: {path}"))),
        }
    }
}

// ── RpcRouterBuilder ────────────────────────────────────

/// Builder for an [`RpcRouter`].
///
/// # Middleware
///
/// Middleware layers are applied to each handler at registration time,
/// not wrapped around the entire router. This matches xitca's approach
/// and enables zero-overhead dispatch when no middleware is used.
///
/// **Layer order matters**: Add layers via [`layer`](RpcRouterBuilder::layer)
/// **before** registering handlers via [`route_fn`](RpcRouterBuilder::route_fn).
/// Layers only affect handlers registered after them. LIFO — last added = outermost.
///
/// # Handler storage
///
/// - Without middleware: stored as `HandlerSlot::Raw` → zero `Box::pin` on dispatch.
/// - With middleware: stored as `HandlerSlot::Erased` → one `Box::pin` on dispatch.
pub struct RpcRouterBuilder<Ctx: Send + Sync + 'static> {
    procedures: Vec<ProcedureMeta>,
    /// Mutable router — routes and their (handler + middleware) are inserted during build.
    router: Router<HandlerSlot<Ctx>>,
    /// Shared specta type registry for TypeScript codegen.
    types: specta::Types,
    /// Pending middleware layers to apply to each handler.
    middlewares: Vec<Box<dyn Fn(Box<dyn ErasedHandler<Ctx>>) -> Box<dyn ErasedHandler<Ctx>> + Send + Sync>>,
    /// Subscribe handlers.
    subscribe_handlers: Vec<(&'static str, Arc<dyn crate::handler::ErasedSubscribeHandler<Ctx>>)>,
}

impl<Ctx: Send + Sync + 'static> RpcRouterBuilder<Ctx> {
    /// Create an empty router builder.
    pub fn new() -> Self {
        let mut types = specta::Types::default();
        crate::error::RpcErr::definition(&mut types);
        Self {
            procedures: Vec::new(),
            router: Router::new(),
            types,
            middlewares: Vec::new(),
            subscribe_handlers: Vec::new(),
        }
    }

    /// Register a typed RPC function (query or mutate).
    ///
    /// The handler is wrapped with all pending middleware layers before
    /// being inserted into the router.
    pub fn route_fn<H: RpcFn<Ctx> + 'static>(mut self, handler: H) -> Self {
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
        impl<Ctx: Send + Sync + 'static, H: RpcFn<Ctx>> crate::middleware::RpcService<Ctx>
            for RpcHandler<Ctx, H>
        {
            type Response = (Cow<'static, [u8]>, bool);
            type Error = RpcErr;

            async fn call(
                &self,
                ctx: &Ctx,
                _path: &str,
                input: &[u8],
                is_get: bool,
                _extensions: &mut Extensions,
            ) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
                let input_val: Value = if is_get {
                    let query_str = std::str::from_utf8(input).unwrap_or("");
                    query_str.split('&').find_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        let key = parts.next()?;
                        let val = parts.next()?;
                        if key == "input" {
                            let decoded = percent_decode(val);
                            serde_json::from_str(&decoded).ok()
                        } else {
                            None
                        }
                    }).unwrap_or(Value::Null)
                } else {
                    serde_json::from_slice(input).unwrap_or(Value::Null)
                };
                let result = self.0.call_value(ctx, input_val).await?;
                Ok((result, true))
            }
        }

        if self.middlewares.is_empty() {
            // No middleware — store raw handler for zero Box::pin dispatch
            let raw_handler = Arc::new(Handler::Rpc {
                f: Box::new(RpcHandler(handler, PhantomData)),
                skip_query,
            });
            self.router.insert(H::KEY.to_string(), HandlerSlot::Raw(raw_handler)).unwrap();
        } else {
            // Wrap with middleware layers and erase
            let mut handler: Box<dyn ErasedHandler<Ctx>> = Box::new(RpcHandler(handler, PhantomData));
            for mw in self.middlewares.iter().rev() {
                handler = mw(handler);
            }
            self.router.insert(H::KEY.to_string(), HandlerSlot::Erased(Arc::from(handler))).unwrap();
        }
        self
    }

    /// Register a bytes handler.
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
        impl<Ctx: Send + Sync + 'static, F: crate::handler::RawRpcFn<Ctx>> crate::middleware::RpcService<Ctx>
            for BytesHandler<Ctx, F>
        {
            type Response = (Cow<'static, [u8]>, bool);
            type Error = RpcErr;

            async fn call(
                &self,
                ctx: &Ctx,
                _path: &str,
                input: &[u8],
                is_get: bool,
                _extensions: &mut Extensions,
            ) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
                let result = F::exec(ctx, input).await?;
                Ok((result, false))
            }
        }

        let bytes_handler = BytesHandler(handler, PhantomData);

        if self.middlewares.is_empty() {
            let raw_handler = Arc::new(Handler::Bytes(Box::new(bytes_handler)));
            self.router.insert(F::KEY.to_string(), HandlerSlot::Raw(raw_handler)).unwrap();
        } else {
            let mut handler: Box<dyn ErasedHandler<Ctx>> = Box::new(bytes_handler);
            for mw in self.middlewares.iter().rev() {
                handler = mw(handler);
            }
            self.router.insert(F::KEY.to_string(), HandlerSlot::Erased(Arc::from(handler))).unwrap();
        }
        self
    }

    /// Register a subscribe handler.
    pub fn subscribe<H: crate::handler::RpcSubscribe<Ctx> + 'static>(
        mut self,
        handler: H,
    ) -> Self {
        use crate::handler::SubscribeExt;
        self.procedures.push(ProcedureMeta {
            key: H::KEY,
            kind: "subscribe",
            method: H::METHOD,
            input: gen_ts_client::type_ts::<H::Input>(),
            output: gen_ts_client::type_ts::<H::Output>(),
        });
        // Create an erased subscribe handler for runtime dispatch
        struct ErasedSub<H>(H);
        impl<Ctx: Send + Sync + 'static, H: crate::handler::RpcSubscribe<Ctx>> crate::handler::ErasedSubscribeHandler<Ctx>
            for ErasedSub<H>
        {
            fn call_bytes(
                &self,
                ctx: &Ctx,
                input: &[u8],
            ) -> Pin<Box<dyn futures::Stream<Item = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'static>>
            {
                use crate::handler::SubscribeExt;
                self.0.call_bytes(ctx, input)
            }
        }
        self.subscribe_handlers.push((H::KEY, Arc::new(ErasedSub(handler))));
        self
    }

    /// Attach a middleware layer.
    ///
    /// The layer is recorded and applied to all subsequently registered
    /// handlers at `route_fn` / `route_bytes` time. Layers applied **before**
    /// `route_fn` affect that handler; layers applied after do not.
    ///
    /// Layers are applied LIFO — the last layer added becomes the outermost.
    ///
    /// # Note
    ///
    /// The inner type is `Box<dyn ErasedHandler<Ctx>>`. Any [`RpcLayer`] whose
    /// inner and outer types are `RpcService<Ctx, Response = (Cow, bool), Error = RpcErr>`
    /// can be used. Most built-in layers (e.g. [`HookLayer`](crate::middlewares::hook::HookLayer))
    /// satisfy this automatically.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use fnrpc::middlewares::hook::HookLayer;
    ///
    /// RpcRouterBuilder::<()>::new()
    ///     .layer(HookLayer::new().before(|_ctx, _path, input, _is_get| Ok(input)))
    ///     .route_fn(health)
    ///     .build();
    /// ```
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: RpcLayer<Ctx, Box<dyn ErasedHandler<Ctx>>> + 'static,
        L::Service: crate::middleware::RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr>
            + Send + Sync + 'static,
    {
        self.middlewares.push(Box::new(move |handler| {
            let wrapped: L::Service = layer.layer(handler);
            Box::new(wrapped) as Box<dyn ErasedHandler<Ctx>>
        }));
        self
    }

    /// Attach a closure-based middleware layer.
    ///
    /// The closure receives `(&Box<dyn ErasedHandler<Ctx>>, &Ctx, &str, &[u8], bool, &mut Extensions)`
    /// and returns `Pin<Box<dyn Future<...>>>`.
    /// Call `handler.call(ctx, path, input, is_get, extensions).await` to delegate.
    pub fn layer_fn<F>(mut self, func: F) -> Self
    where
        F: for<'a> Fn(
                &'a Box<dyn ErasedHandler<Ctx>>,
                &'a Ctx,
                &'a str,
                &'a [u8],
                bool,
                &'a mut Extensions,
            ) -> Pin<Box<dyn Future<Output = Result<(Cow<'static, [u8]>, bool), RpcErr>> + Send + 'a>>
            + Clone + Send + Sync + 'static,
    {
        self.middlewares.push(Box::new(move |handler| {
            let handler = Arc::new(handler);
            Box::new(ClosureMw { handler, func: func.clone() }) as Box<dyn ErasedHandler<Ctx>>
        }));
        self
    }

    /// Finalize and produce a [`RpcRouter`].
    pub fn build(self) -> RpcRouter<Ctx> {
        RpcRouter {
            procedures: self.procedures,
            handler_router: Arc::new(self.router),
            subscribe_handlers: self.subscribe_handlers,
            types: self.types,
        }
    }
}

impl<Ctx: Send + Sync + 'static> Default for RpcRouterBuilder<Ctx> {
    fn default() -> Self {
        Self::new()
    }
}

// ── Percent decoding (moved from handler.rs for route_fn use) ──

fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        match b {
            b'+' => result.push(' '),
            b'%' => {
                let hi = bytes.next().and_then(|c| hex_val(c));
                let lo = bytes.next().and_then(|c| hex_val(c));
                match (hi, lo) {
                    (Some(h), Some(l)) => result.push((h << 4 | l) as char),
                    _ => result.push('%'),
                }
            }
            _ => result.push(b as char),
        }
    }
    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Middleware wrapper for `layer_fn`.
struct ClosureMw<Ctx: Send + Sync + 'static, F> {
    handler: Arc<Box<dyn ErasedHandler<Ctx>>>,
    func: F,
}

impl<Ctx: Send + Sync + 'static, F> crate::middleware::RpcService<Ctx> for ClosureMw<Ctx, F>
where
    F: for<'a> Fn(
            &'a Box<dyn ErasedHandler<Ctx>>,
            &'a Ctx,
            &'a str,
            &'a [u8],
            bool,
            &'a mut Extensions,
        ) -> Pin<Box<dyn Future<Output = Result<(Cow<'static, [u8]>, bool), RpcErr>> + Send + 'a>>
        + Send + Sync,
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
        (self.func)(&self.handler, ctx, path, input, is_get, extensions).await
    }
}

impl<Ctx: Send + Sync + 'static, F: Clone> Clone for ClosureMw<Ctx, F> {
    fn clone(&self) -> Self {
        Self {
            handler: Arc::clone(&self.handler),
            func: self.func.clone(),
        }
    }
}
