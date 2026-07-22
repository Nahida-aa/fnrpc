use std::{borrow::Cow, pin::Pin};

use crate::error::RpcErr;

/// Object-safe bytes handler trait.
pub trait BytesHandlerFn<Ctx>: Send + Sync {
    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        input: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'a>>;
}
