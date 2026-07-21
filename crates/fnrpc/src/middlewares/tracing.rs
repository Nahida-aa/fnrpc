use std::borrow::Cow;
use std::marker::PhantomData;

use http::Extensions;

use crate::error::RpcErr;
use crate::output::RpcOutput;
use crate::middleware::{RpcLayer, RpcService};

/// A logging layer that emits structured [`tracing`] events per call.
///
/// Only available with `feature = "tracing"`.
pub struct TracingLayer;

pub struct TracingService<Ctx, S> {
    inner: S,
    _marker: PhantomData<Ctx>,
}

impl<Ctx: Send + Sync + 'static, S> RpcService<Ctx> for TracingService<Ctx, S>
where
    S: RpcService<Ctx, Response = RpcOutput, Error = RpcErr> + Send + Sync,
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
        let start = std::time::Instant::now();
        let result = self.inner.call(ctx, path, input, is_get, extensions).await;
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        // Only allocate strings when tracing is enabled at info level
        if tracing::level_enabled!(tracing::Level::INFO) {
            match &result {
                Ok((output, _is_json)) => {
                    let output_str = String::from_utf8_lossy(output);
                    let input_str = String::from_utf8_lossy(input);
                    tracing::info!(
                        path = %path,
                        input = %input_str,
                        output = %output_str,
                        latency_ms = %latency_ms,
                        "rpc_call",
                    );
                }
                Err(e) => {
                    let input_str = String::from_utf8_lossy(input);
                    tracing::error!(
                        path = %path,
                        input = %input_str,
                        error = %e,
                        latency_ms = %latency_ms,
                        "rpc_call",
                    );
                }
            }
        }
        result
    }
}

impl<Ctx: Send + Sync + 'static, S> RpcLayer<Ctx, S> for TracingLayer
where
    S: RpcService<Ctx, Response = RpcOutput, Error = RpcErr> + Send + Sync,
{
    type Service = TracingService<Ctx, S>;

    fn layer(&self, inner: S) -> Self::Service {
        TracingService {
            inner,
            _marker: PhantomData,
        }
    }
}
