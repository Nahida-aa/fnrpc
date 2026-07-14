use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::RpcErr;

/// Core service trait — call a method with JSON input, get JSON output.
#[async_trait]
pub trait FnService<Ctx>: Send + Sync {
    async fn call(&self, ctx: &Ctx, path: &str, input: Value) -> Result<Value, RpcErr>;
}

/// Blanket impl so `Box<dyn FnService<Ctx>>` works as a service.
#[async_trait]
impl<Ctx: Send + Sync> FnService<Ctx> for Box<dyn FnService<Ctx>> {
    async fn call(&self, ctx: &Ctx, path: &str, input: Value) -> Result<Value, RpcErr> {
        (**self).call(ctx, path, input).await
    }
}

/// A composable middleware layer.
///
/// ```ignore
/// router.use(MyLayer);
/// ```
pub trait FnLayer<Ctx>: Send + Sync {
    fn layer(&self, inner: Box<dyn FnService<Ctx>>) -> Box<dyn FnService<Ctx>>;
}

// ── Hook layer (convenience) ──────────────────────────────

type BeforeHook<Ctx> =
    Arc<dyn Fn(&Ctx, &str, &mut Value) -> Result<(), RpcErr> + Send + Sync>;

type AfterHook<Ctx> =
    Arc<dyn Fn(&Ctx, &str, &mut Result<Value, RpcErr>) + Send + Sync>;

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

    pub fn before<F>(mut self, f: F) -> Self
    where
        F: Fn(&Ctx, &str, &mut Value) -> Result<(), RpcErr> + Send + Sync + 'static,
    {
        self.before = Some(Arc::new(f));
        self
    }

    pub fn after<F>(mut self, f: F) -> Self
    where
        F: Fn(&Ctx, &str, &mut Result<Value, RpcErr>) + Send + Sync + 'static,
    {
        self.after = Some(Arc::new(f));
        self
    }
}

struct HookService<Ctx> {
    inner: Box<dyn FnService<Ctx>>,
    before: Option<BeforeHook<Ctx>>,
    after: Option<AfterHook<Ctx>>,
}

#[async_trait]
impl<Ctx: Send + Sync + 'static> FnService<Ctx> for HookService<Ctx> {
    async fn call(&self, ctx: &Ctx, path: &str, mut input: Value) -> Result<Value, RpcErr> {
        if let Some(ref before) = self.before {
            before(ctx, path, &mut input)?;
        }
        let mut result = self.inner.call(ctx, path, input).await;
        if let Some(ref after) = self.after {
            after(ctx, path, &mut result);
        }
        result
    }
}

impl<Ctx: Send + Sync + 'static> FnLayer<Ctx> for HookLayer<Ctx> {
    fn layer(&self, inner: Box<dyn FnService<Ctx>>) -> Box<dyn FnService<Ctx>> {
        Box::new(HookService {
            inner,
            before: self.before.clone(),
            after: self.after.clone(),
        })
    }
}

// ── Tracing layer (feature = "tracing") ───────────────────

#[cfg(feature = "tracing")]
pub struct TracingLayer;

#[cfg(feature = "tracing")]
struct TracingService<Ctx> {
    inner: Box<dyn FnService<Ctx>>,
}

#[cfg(feature = "tracing")]
#[async_trait]
impl<Ctx: Send + Sync + 'static> FnService<Ctx> for TracingService<Ctx> {
    async fn call(&self, ctx: &Ctx, path: &str, input: Value) -> Result<Value, RpcErr> {
        let start = std::time::Instant::now();
        let input_str = serde_json::to_string(&input).unwrap_or_default();
        let result = self.inner.call(ctx, path, input).await;
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
impl<Ctx: Send + Sync + 'static> FnLayer<Ctx> for TracingLayer {
    fn layer(&self, inner: Box<dyn FnService<Ctx>>) -> Box<dyn FnService<Ctx>> {
        Box::new(TracingService { inner })
    }
}
