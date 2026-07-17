//! Handler traits for RPC functions.
//!
//! - [`RpcFn`] / [`ErasedHandler`] for query & mutate.
//! - [`RawRpcFn`] for raw byte-buffer handlers.
//! - [`RpcSubscribe`] / [`ErasedSubscribeHandler`] for subscriptions.

use std::any::TypeId;
use std::borrow::Cow;
use std::pin::Pin;
use std::sync::Arc;

use futures::StreamExt;
use futures::stream::Stream;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use specta::Type;
use specta::datatype::DataType;

use crate::codec::{JsonCodec, RpcCodec};
use crate::error::RpcErr;

/// TypeScript type reference info for a single type (input or output).
///
/// Produced by [`type_ts`] and used during codegen to determine the
/// TypeScript type name for a given Rust type.
#[derive(Debug, Clone)]
pub struct TsTypeInfo {
    /// TypeScript type reference name (e.g. `"HealthCheckOutput"`) or inline expression.
    pub ts_ref: String,
}


// ── ErasedHandler (object-safe dispatch) ──────────────────

/// Object-safe erased handler stored in the router.
///
/// The [`call_bytes`](ErasedHandler::call_bytes) method is the primary
/// dispatch path — raw bytes in, raw bytes out.
/// [`call`](ErasedHandler::call) and [`call_value`](ErasedHandler::call_value)
/// are convenience wrappers that work with JSON [`Value`].
pub trait ErasedHandler<Ctx>: Send + Sync {
    fn name(&self) -> &'static str;
    fn kind(&self) -> &'static str;
    fn input_ts(&self) -> TsTypeInfo;
    fn output_ts(&self) -> TsTypeInfo;
    fn populate_types(&self, types: &mut specta::Types, top_level: &mut Vec<DataType>);

    /// Content-Type for responses produced by this handler.
    fn content_type(&self) -> Option<&'static str>;

    /// Primary dispatch: raw bytes in, raw bytes out.
    ///
    /// Default impl: JSON decode → [`call`](Self::call) → JSON re-encode.
    /// Override for zero-copy raw protocols.
    fn call_bytes(&self, ctx: &Ctx, input: &[u8]) -> Result<Cow<'static, [u8]>, RpcErr> {
        let value: Value = serde_json::from_slice(input)
            .map_err(|e| RpcErr::bad_request(format!("deserialize: {e}")))?;
        self.call(ctx, value)
            .and_then(|v| serde_json::to_vec(&v).map(Cow::Owned).map_err(|e| RpcErr::internal(format!("serialize: {e}"))))
    }

    /// Dispatch a call, returning a JSON value.
    fn call(&self, ctx: &Ctx, input: Value) -> Result<Value, RpcErr> {
        let bytes = serde_json::to_vec(&input)
            .map_err(|e| RpcErr::bad_request(format!("serialize input: {e}")))?;
        let result = self.call_bytes(ctx, &bytes)?;
        serde_json::from_slice(&result)
            .map_err(|e| RpcErr::internal(format!("deserialize result: {e}")))
    }

    /// Dispatch from a JSON [`Value`], returning serialized bytes.
    fn call_value(&self, ctx: &Ctx, input: Value) -> Result<Cow<'static, [u8]>, RpcErr> {
        let bytes = serde_json::to_vec(&input)
            .map_err(|e| RpcErr::bad_request(format!("serialize input: {e}")))?;
        self.call_bytes(ctx, &bytes)
    }
}

// ── RpcFn (typed, serde-based) ────────────────────────────

/// Typed RPC function trait using serde serialization.
pub trait RpcFn<Ctx>: Send + Sync {
    type Input: DeserializeOwned + 'static;
    type Output: Serialize + 'static;
    const NAME: &'static str;
    const KIND: &'static str = "query";

    fn exec(ctx: &Ctx, input: Self::Input) -> Result<Self::Output, RpcErr>;

    /// Wrap this handler as an erased handler (Arc'd).
    fn into_erased(self) -> Arc<dyn ErasedHandler<Ctx>>
    where
        Self: Sized + 'static,
        Ctx: Send + Sync + 'static,
    {
        Arc::new(RpcFnWrapper(self))
    }
}

struct RpcFnWrapper<F>(F) where F: Send + Sync;

impl<Ctx: Send + Sync + 'static, F: RpcFn<Ctx>> ErasedHandler<Ctx> for RpcFnWrapper<F> {
    fn name(&self) -> &'static str { F::NAME }
    fn kind(&self) -> &'static str { F::KIND }
    fn input_ts(&self) -> TsTypeInfo { TsTypeInfo { ts_ref: "unknown".into() } }
    fn output_ts(&self) -> TsTypeInfo { TsTypeInfo { ts_ref: "unknown".into() } }

    fn populate_types(&self, _types: &mut specta::Types, _top_level: &mut Vec<DataType>) {}

    fn content_type(&self) -> Option<&'static str> { Some("application/json") }

    fn call_bytes(&self, ctx: &Ctx, input: &[u8]) -> Result<Cow<'static, [u8]>, RpcErr> {
        let input = if is_unit_type::<F::Input>() {
            unsafe { std::mem::zeroed() }
        } else {
            JsonCodec::decode::<F::Input>(input)?
        };
        let output = F::exec(ctx, input)?;
        JsonCodec::encode(&output).map(Cow::Owned)
    }

    fn call(&self, ctx: &Ctx, input: Value) -> Result<Value, RpcErr> {
        let input = if is_unit_type::<F::Input>() {
            unsafe { std::mem::zeroed() }
        } else {
            serde_json::from_value(input)
                .map_err(|e| RpcErr::bad_request(format!("deserialize input: {e}")))?
        };
        let output = F::exec(ctx, input)?;
        Ok(serde_json::to_value(output)
            .map_err(|e| RpcErr::internal(format!("serialize output: {e}")))?)
    }

    fn call_value(&self, ctx: &Ctx, input: Value) -> Result<Cow<'static, [u8]>, RpcErr> {
        let input = if is_unit_type::<F::Input>() {
            unsafe { std::mem::zeroed() }
        } else {
            serde_json::from_value(input)
                .map_err(|e| RpcErr::bad_request(format!("deserialize input: {e}")))?
        };
        let output = F::exec(ctx, input)?;
        serde_json::to_vec(&output)
            .map(Cow::Owned)
            .map_err(|e| RpcErr::internal(format!("serialize output: {e}")))
    }
}

// ── Helper: detect unit input type ──────────────────────────

#[inline(always)]
fn is_unit_type<T: 'static>() -> bool {
    TypeId::of::<T>() == TypeId::of::<()>()
}

// ── RawRpcFn (zero-serialization byte handlers) ───────────

/// A raw RPC function that operates directly on byte buffers.
///
/// Unlike [`RpcFn`], this trait bypasses serde serialization entirely.
/// Register with [`RpcRouterBuilder::raw`](crate::router::RpcRouterBuilder::raw).
///
/// Raw handlers bypass the middleware stack and are not included in codegen.
pub trait RawRpcFn<Ctx>: Send + Sync {
    const NAME: &'static str;
    fn exec(ctx: &Ctx, input: &[u8]) -> Result<Vec<u8>, RpcErr>;
}

// ── Subscription traits ────────────────────────────────────

/// Typed RPC subscribe trait.
pub trait RpcSubscribe<Ctx>: Send + Sync {
    type Input: DeserializeOwned + Type;
    type Output: Serialize + Type + 'static;
    const NAME: &'static str;
    const KIND: &'static str = "subscribe";
    const METHOD: &'static str = "GET";

    fn exec(
        ctx: &Ctx,
        input: Self::Input,
    ) -> Pin<Box<dyn Stream<Item = Result<Self::Output, RpcErr>> + Send + 'static>>;
}

/// Object-safe erased subscribe handler stored in the router.
pub trait ErasedSubscribeHandler<Ctx>: Send + Sync {
    fn name(&self) -> &'static str;
    fn method(&self) -> &'static str;
    fn input_ts(&self) -> TsTypeInfo;
    fn output_ts(&self) -> TsTypeInfo;
    fn populate_types(&self, types: &mut specta::Types, top_level: &mut Vec<DataType>);
    fn call(
        &self,
        ctx: &Ctx,
        input: Value,
    ) -> Pin<Box<dyn Stream<Item = Result<Value, RpcErr>> + Send + 'static>>;
    fn call_bytes(
        &self,
        ctx: &Ctx,
        input: &[u8],
    ) -> Pin<Box<dyn Stream<Item = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'static>>;
}

// ── RoutedHandler (zero-sized marker for route registration) ──

/// Trait for route-registered handler (xitca-web–style).
///
/// Implemented by the zero-sized marker struct generated by
/// [`#[rpc_query]`](fnrpc_macros::rpc_query) and
/// [`#[rpc_mutate]`](fnrpc_macros::rpc_mutate).
pub trait RoutedHandler<Ctx>: RpcFn<Ctx> {
    fn path() -> &'static str;
    fn method() -> &'static str;
}

/// Trait for route-registered subscribe handler.
pub trait RoutedSubscribeHandler<Ctx>: ErasedSubscribeHandler<Ctx> {
    fn path() -> &'static str;
    fn method() -> &'static str;
}

/// Blanket impl: any `RpcSubscribe<Ctx>` becomes an `ErasedSubscribeHandler<Ctx>`.
impl<Ctx, F> ErasedSubscribeHandler<Ctx> for F
where
    F: RpcSubscribe<Ctx> + Send + Sync,
    Ctx: Send + Sync,
    <F as RpcSubscribe<Ctx>>::Output: 'static,
{
    fn name(&self) -> &'static str { F::NAME }
    fn method(&self) -> &'static str { F::METHOD }
    fn input_ts(&self) -> TsTypeInfo { TsTypeInfo { ts_ref: "unknown".into() } }
    fn output_ts(&self) -> TsTypeInfo { TsTypeInfo { ts_ref: "unknown".into() } }

    fn populate_types(&self, _types: &mut specta::Types, _top_level: &mut Vec<DataType>) {}

    fn call(
        &self,
        ctx: &Ctx,
        input: Value,
    ) -> Pin<Box<dyn Stream<Item = Result<Value, RpcErr>> + Send + 'static>> {
        let input = match serde_json::from_value(input) {
            Ok(v) => v,
            Err(e) => {
                return Box::pin(futures::stream::once(futures::future::ready(Err(
                    RpcErr::bad_request(format!("deserialize input: {e}")),
                ))));
            }
        };
        let stream = F::exec(ctx, input);
        Box::pin(stream.map(|item| match item {
            Ok(v) => serde_json::to_value(v)
                .map_err(|e| RpcErr::internal(format!("serialize output: {e}"))),
            Err(e) => Err(e),
        }))
    }

    fn call_bytes(
        &self,
        ctx: &Ctx,
        input: &[u8],
    ) -> Pin<Box<dyn Stream<Item = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'static>> {
        let input = match JsonCodec::decode::<F::Input>(input) {
            Ok(v) => v,
            Err(e) => return Box::pin(futures::stream::once(futures::future::ready(Err(e)))),
        };
        let stream = F::exec(ctx, input);
        Box::pin(stream.map(|item| match item {
            Ok(v) => JsonCodec::encode(&v).map(Cow::Owned),
            Err(e) => Err(e),
        }))
    }
}
