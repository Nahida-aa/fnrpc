//! The RPC router — collects procedure metadata and dispatches calls.
//!
//! Use [`RpcRouterBuilder`] to register handlers, then
//! [`build`](RpcRouterBuilder::build) to get a stored [`RpcRouter`].
//!
//! Unlike tRPC-style routers, this is purely a metadata collection layer.
//! Actual HTTP routing is handled by the transport crate (fnrpc-web, etc.)
//! which registers each procedure as an independent route.

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use http::Extensions;
use serde_json::Value;

use crate::error::RpcErr;
use crate::handler::{RpcFn, TsTypeInfo};
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

/// A collection of procedure metadata.
///
/// Produced by [`RpcRouterBuilder::build`].
///
/// This struct stores procedure metadata for TypeScript codegen.
/// Actual HTTP routing is handled by the transport layer.
pub struct RpcRouter<Ctx> {
    pub(crate) procedures: Vec<ProcedureMeta>,
    pub(crate) inner: Arc<dyn ErasedRpcService<Ctx>>,
}

impl<Ctx> Clone for RpcRouter<Ctx> {
    fn clone(&self) -> Self {
        Self {
            procedures: self.procedures.clone(),
            inner: self.inner.clone(),
        }
    }
}

impl<Ctx: Send + Sync + 'static> RpcRouter<Ctx> {
    /// Iterate over all procedure metadata for TypeScript codegen.
    pub fn procedures(&self) -> &[ProcedureMeta] {
        &self.procedures
    }

    /// Dispatch a call through the middleware stack.
    ///
    /// This is used by middleware tests and advanced integration scenarios.
    /// For HTTP routing, use the transport layer (fnrpc-web, fnrpc-axum, etc.)
    /// which registers each procedure as an independent route.
    pub async fn dispatch(&self, ctx: &Ctx, path: &str, input: Value) -> Result<Value, RpcErr> {
        let mut extensions = Extensions::new();
        self.inner.call(ctx, path, input, &mut extensions).await
    }

    /// Dispatch a call and return a [`Send`] future.
    ///
    /// Required by multi-threaded runtimes (Axum, Tower, etc.).
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
///
/// Registers procedures and subscribe handlers.
pub struct RpcRouterBuilder<Ctx> {
    procedures: Vec<ProcedureMeta>,
    _marker: PhantomData<Ctx>,
}

impl<Ctx: Send + Sync + 'static> RpcRouterBuilder<Ctx> {
    /// Create an empty router builder.
    pub fn new() -> Self {
        Self {
            procedures: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Register a typed RPC function (query or mutate).
    pub fn register<H: RpcFn<Ctx> + 'static>(mut self, _handler: H) -> Self {
        self.procedures.push(ProcedureMeta {
            key: H::KEY,
            kind: H::KIND,
            method: H::METHOD,
            input: gen_ts_client::type_ts::<H::Input>(),
            output: gen_ts_client::type_ts::<H::Output>(),
        });
        self
    }

    /// Register a raw byte-buffer handler.
    pub fn register_raw<F: crate::handler::RawRpcFn<Ctx> + 'static>(self, _handler: F) -> Self {
        // Raw handlers are not included in codegen — no metadata collected.
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
        // Build a no-op middleware chain.
        // The inner service is unused in the new architecture — handlers
        // are registered directly with the transport layer.
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
        }
    }
}

impl<Ctx: Send + Sync + 'static> Default for RpcRouterBuilder<Ctx> {
    fn default() -> Self {
        Self::new()
    }
}
