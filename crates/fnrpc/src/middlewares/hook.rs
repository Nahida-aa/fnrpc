use std::borrow::Cow;
use std::sync::Arc;

use http::Extensions;

use crate::error::RpcErr;
use crate::middleware::{RpcLayer, RpcService};

type BeforeHook<Ctx> =
    Arc<dyn for<'a> Fn(&Ctx, &str, &'a [u8], bool) -> Result<&'a [u8], RpcErr> + Send + Sync>;

type AfterHook<Ctx> =
    Arc<dyn Fn(&Ctx, &str, &mut Result<(Cow<'static, [u8]>, bool), RpcErr>) + Send + Sync>;

/// A convenience layer with before/after hooks.
///
/// Use this when you don't need to hold state or write a full [`RpcLayer`]
/// implementation — just attach closures for before/after logic.
///
/// The before-hook receives `&Ctx, &str, &[u8], bool` and returns
/// `Result<&[u8], RpcErr>`. Return `Ok(input)` to pass through unchanged
/// (zero allocation). Return `Err(RpcErr)` to short-circuit.
///
/// # Examples
///
/// ```ignore
/// use fnrpc::middleware::hook::HookLayer;
///
/// RpcRouterBuilder::new()
///     .route_fn(health)
///     .layer(
///         HookLayer::new()
///             .before(|ctx, path, input, is_get| {
///                 tracing::info!("calling {path}");
///                 Ok(input)
///             })
///             .after(|ctx, path, result| {
///                 tracing::info!("{path} returned");
///             }),
///     )
///     .build();
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

impl<Ctx> Default for HookLayer<Ctx> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct HookService<Ctx, S> {
    inner: S,
    before: Option<BeforeHook<Ctx>>,
    after: Option<AfterHook<Ctx>>,
}

impl<Ctx: Send + Sync + 'static, S> RpcService<Ctx> for HookService<Ctx, S>
where
    S: RpcService<Ctx, Response = (Cow<'static, [u8]>, bool), Error = RpcErr> + Send + Sync,
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
