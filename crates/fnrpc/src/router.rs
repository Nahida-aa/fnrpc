//! The RPC router — collects procedure metadata and dispatches calls.
//!
//! Use [`RpcRouterBuilder`] to register handlers, then
//! [`build`](RpcRouterBuilder::build) to get a stored [`RpcRouter`].

use std::any::TypeId;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use http::Extensions;
use serde_json::Value;
use xitca_router::Router;

use crate::error::RpcErr;
use crate::handler::{Handler, RpcFn, RpcFnExt, TsTypeInfo};
use crate::gen_ts_client;
use crate::middleware::ErasedRpcService;

/// Metadata for a single procedure, used by TypeScript codegen.
#[derive(Debug, Clone)]
pub struct ProcedureMeta {
    pub key: &'static str,
    pub kind: &'static str,
    pub method: &'static str,
    pub input: TsTypeInfo,
    pub output: TsTypeInfo,
}

type HandlerFn<Ctx> = Arc<dyn for<'a> Fn(&'a Ctx, Value) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, RpcErr>> + Send + 'a>> + Send + Sync>;

/// A collection of RPC handlers with radix-tree routing.
///
/// Produced by [`RpcRouterBuilder::build`].
pub struct RpcRouter<Ctx: Send + Sync + 'static> {
    pub(crate) procedures: Vec<ProcedureMeta>,
    pub(crate) inner: Arc<dyn ErasedRpcService<Ctx>>,
    router: Router<Handler<Ctx>>,
}

impl<Ctx: Send + Sync + 'static> Clone for RpcRouter<Ctx> {
    fn clone(&self) -> Self {
        Self {
            procedures: self.procedures.clone(),
            inner: self.inner.clone(),
            router: self.router.clone(),
        }
    }
}

impl<Ctx: Send + Sync + 'static> RpcRouter<Ctx> {
    /// Look up a handler by path and call it.
    ///
    /// * For `Handler::Rpc`: `input` is raw query bytes (GET) or body bytes (POST),
    ///   `is_get` controls query string parsing.
    /// * For `Handler::Bytes`: `input` is passed directly.
    pub async fn call_handler(&self, path: &str, ctx: &Ctx, input: &[u8], is_get: bool) -> Result<Vec<u8>, RpcErr> {
        match self.router.at(path).ok() {
            Some(m) => m.value.call(ctx, input, is_get).await,
            None => Err(RpcErr::not_found(format!("unknown path: {path}"))),
        }
    }

    /// Iterate over all procedure metadata for TypeScript codegen.
    pub fn procedures(&self) -> &[ProcedureMeta] {
        &self.procedures
    }

    /// Dispatch a call through the middleware stack.
    pub async fn dispatch(&self, ctx: &Ctx, path: &str, input: Value) -> Result<Value, RpcErr> {
        let mut extensions = Extensions::new();
        self.inner.call(ctx, path, input, &mut extensions).await
    }

    /// Dispatch a call and return a [`Send`] future.
    pub async fn dispatch_send(
        &self,
        ctx: &Ctx,
        path: &str,
        input: Value,
    ) -> Result<Value, RpcErr> {
        let mut extensions = Extensions::new();
        let fut = self.inner.call(ctx, path, input, &mut extensions);
        unsafe {
            std::mem::transmute::<
                Pin<Box<dyn Future<Output = Result<Value, RpcErr>> + '_>>,
                Pin<Box<dyn Future<Output = Result<Value, RpcErr>> + Send + '_>>,
            >(fut)
        }
        .await
    }

    /// Generate TypeScript client code.
    pub fn generate_ts_client(&self, rpc_url: &str) -> String {
        gen_ts_client::generate_ts_client(self, rpc_url)
    }
}

// ── RpcRouterBuilder ──────────────────────────────────────

/// Builder for an [`RpcRouter`].
pub struct RpcRouterBuilder<Ctx: Send + Sync + 'static> {
    procedures: Vec<ProcedureMeta>,
    router: Router<Handler<Ctx>>,
}

impl<Ctx: Send + Sync + 'static> RpcRouterBuilder<Ctx> {
    /// Create an empty router builder.
    pub fn new() -> Self {
        Self {
            procedures: Vec::new(),
            router: Router::new(),
        }
    }

    /// Register a typed RPC function (query or mutate).
    pub fn route<H: RpcFn<Ctx> + 'static>(mut self, handler: H) -> Self {
        self.procedures.push(ProcedureMeta {
            key: H::KEY,
            kind: H::KIND,
            method: H::METHOD,
            input: gen_ts_client::type_ts::<H::Input>(),
            output: gen_ts_client::type_ts::<H::Output>(),
        });

        let skip_query = TypeId::of::<H::Input>() == TypeId::of::<()>();
        let inner = Arc::new(handler);
        let handler_fn = Handler::Rpc {
            f: Arc::new(move |ctx: &Ctx, input: Value| {
                let inner = Arc::clone(&inner);
                Box::pin(async move {
                    let result = inner.call_value(ctx, input).await?;
                    Ok(result.into_owned())
                })
            }),
            skip_query,
        };
        self.router.insert(H::KEY.to_string(), handler_fn).unwrap();
        self
    }

    /// Register a bytes handler (bypasses JSON serialization).
    pub fn route_bytes<F: crate::handler::RawRpcFn<Ctx> + 'static>(mut self, handler: F) -> Self {
        let inner = Arc::new(handler);
        let handler_fn = Handler::Bytes(Arc::new(move |ctx: &Ctx, input: &[u8]| {
            let inner = Arc::clone(&inner);
            Box::pin(async move {
                let result = F::exec(ctx, input)?;
                Ok(result)
            })
        }));
        self.router.insert(F::KEY.to_string(), handler_fn).unwrap();
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

    /// Finalize and produce a type-erased [`RpcRouter`].
    pub fn build(self) -> RpcRouter<Ctx> {
        struct NoopService<C>(PhantomData<C>);
        impl<C: Send + Sync + 'static> crate::middleware::RpcService<C> for NoopService<C> {
            async fn call(
                &self,
                _ctx: &C,
                _path: &str,
                _input: Value,
                _extensions: &mut Extensions,
            ) -> Result<Value, RpcErr> {
                Err(RpcErr::not_found("no handler registered for dispatch"))
            }
        }
        RpcRouter {
            procedures: self.procedures,
            inner: Arc::new(NoopService(PhantomData)) as Arc<dyn ErasedRpcService<Ctx>>,
            router: self.router,
        }
    }
}

impl<Ctx: Send + Sync + 'static> Default for RpcRouterBuilder<Ctx> {
    fn default() -> Self {
        Self::new()
    }
}
