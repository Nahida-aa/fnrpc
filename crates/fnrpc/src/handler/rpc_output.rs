use std::pin::Pin;

use crate::{error::RpcErr, output::RpcOutput};

// 作为一次为了之后可能需要的功能的实验
// 新增 trait，和 RawRpcFn 平行，不动 RawRpcFn
pub trait RpcOutputFn<Ctx>: Send + Sync {
    const KEY: &'static str;
    const METHOD: &'static str = "GET";
    fn exec<'a>(
        ctx: &'a Ctx,
        input: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<RpcOutput, RpcErr>> + Send + 'a>>;
}

// 新的 trait，和 BytesHandlerFn 平行，但返回 RpcOutput
/// Object-safe handler trait for `RpcOutput`-returning raw handlers.
///
/// The `RpcOutput`-returning analogue of [`BytesHandlerFn`]. Used internally by
/// `route_raw` to erase the concrete `RpcOutputFn` handler.
/// Object-safe handler trait for `RpcOutput`-returning raw handlers.
///
/// The `RpcOutput`-returning analogue of `crate::handler::BytesHandlerFn`.
/// Used internally by `route_raw` to erase the concrete `RpcOutputFn` handler.
pub trait RpcOutputHandlerFn<Ctx>: Send + Sync {
    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        input: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<RpcOutput, RpcErr>> + Send + 'a>>;
}
