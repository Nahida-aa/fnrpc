//! Handler traits for RPC functions.
//!
//! - [`RpcFn`] / [`ErasedHandler`] for query & mutate.
//! - [`RpcSubscribe`] / [`ErasedSubscribeHandler`] for subscriptions.

use std::any::TypeId;
use std::pin::Pin;

use futures::StreamExt;
use futures::stream::Stream;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use specta::Type;
use specta::datatype::{DataType, Primitive, Reference};

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

/// Compute the TS type reference name for a type.
fn type_ts<T: Type>() -> TsTypeInfo {
    let mut types = specta::Types::default();
    let data_type = T::definition(&mut types);

    let ts_ref = match &data_type {
        DataType::Struct(_) | DataType::Enum(_) => types
            .into_sorted_iter()
            .next()
            .map(|ndt| ndt.name.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        DataType::Reference(Reference::Named(r)) => {
            if let Some(ndt) = types.get(r) {
                if ndt.ty.is_some() {
                    ndt.name.to_string()
                } else {
                    let exporter = specta_typescript::Typescript::default();
                    specta_typescript::primitives::inline(&exporter, &types, &data_type)
                        .unwrap_or_else(|_| "unknown".to_string())
                }
            } else {
                "unknown".to_string()
            }
        }
        // BigInt types: exporter forbids them by default (precision loss concern);
        // the TS bindings use `bigint` via `Configuration::enable_lossless_bigints()`.
        DataType::Primitive(p)
            if matches!(
                p,
                Primitive::u64
                    | Primitive::i64
                    | Primitive::u128
                    | Primitive::i128
                    | Primitive::usize
                    | Primitive::isize
            ) =>
        {
            "bigint".to_string()
        }
        // f64 → `number | null`: JSON cannot represent NaN/Infinity/-Infinity,
        // serde_json serialises them as `null`.  The exporter always does this;
        // it is NOT controlled by the semantic config above.
        DataType::Primitive(Primitive::f64) => "number | null".to_string(),
        _ => {
            let exporter = specta_typescript::Typescript::default();
            specta_typescript::primitives::inline(&exporter, &types, &data_type)
                .unwrap_or_else(|_| "unknown".to_string())
        }
    };

    TsTypeInfo { ts_ref }
}

/// Object-safe erased handler stored in the router.
///
/// Blanket-implemented for all [`RpcFn<Ctx>`] types.
/// The router uses this trait to dispatch calls without knowing
/// the concrete input/output types at compile time.
///
/// The [`call`](ErasedHandler::call) method returns a
/// [`BoxFuture`](std::pin::Pin)`<Box<dyn Future + Send>>` — one `Box::pin`
/// allocation per call at the `dyn` boundary.  Inside that one allocation
/// the [`RpcFn::exec`] future is stored inline (no second `Box::pin`),
/// saving ~1 B per dispatch vs the old `#[async_trait]` approach.
///
/// The [`call_bytes`](ErasedHandler::call_bytes) method returns serialized
/// JSON bytes directly, avoiding the intermediate [`Value`] allocation
/// that [`call`](ErasedHandler::call) requires.
pub trait ErasedHandler<Ctx>: Send + Sync {
    /// Procedure name (matches the original Rust function name).
    fn name(&self) -> &'static str;
    /// Procedure kind — `"query"` or `"mutate"`.
    fn kind(&self) -> &'static str;
    /// TypeScript type reference for the input.
    fn input_ts(&self) -> TsTypeInfo;
    /// TypeScript type reference for the output.
    fn output_ts(&self) -> TsTypeInfo;
    /// Populate a shared specta type registry with this handler's types.
    fn populate_types(&self, types: &mut specta::Types, top_level: &mut Vec<DataType>);
    /// Dispatch a call, returning a JSON value.
    fn call(&self, ctx: &Ctx, input: Value) -> Result<Value, RpcErr>;
    /// Dispatch a call, returning serialized JSON bytes directly.
    ///
    /// Default implementation calls [`call`](Self::call) and then
    /// serializes the result. Override in blanket impls to bypass
    /// the intermediate `Value` allocation.
    fn call_bytes(&self, ctx: &Ctx, input: Value) -> Result<Vec<u8>, RpcErr> {
        self.call(ctx, input)
            .and_then(|v| serde_json::to_vec(&v).map_err(|e| RpcErr::internal(format!("serialize: {e}"))))
    }
}

/// Typed RPC function trait.
///
/// Implement this directly, or use the [`#[rpc_query]`] / [`#[rpc_mutate]`] proc macros.
///
/// # Associated types
///
/// - `Input`: deserialised from JSON; must implement [`DeserializeOwned`] + [`Type`].
/// - `Output`: serialised to JSON; must implement [`Serialize`] + [`Type`].
///
/// # Constants
///
/// - `NAME`: maps to the procedure path in the router.
/// - `KIND`: `"query"` (default) or `"mutate"`.
///
/// # Zero-allocation dispatch
///
/// `exec` returns `impl Future<Output = …>` via RPITIT — no `#[async_trait]`,
/// no hidden `Box::pin`.  The concrete future type is known at compile time
/// and stored inline inside [`ErasedHandler::call`]'s single `Box::pin`.
pub trait RpcFn<Ctx>: Send + Sync {
    type Input: DeserializeOwned + Type + 'static;
    type Output: Serialize + Type + 'static;
    const NAME: &'static str;
    const KIND: &'static str = "query";

    /// Execute the RPC.
    fn exec(ctx: &Ctx, input: Self::Input) -> Result<Self::Output, RpcErr>;
}

/// Blanket impl: any `RpcFn<Ctx>` becomes an `ErasedHandler<Ctx>`.
///
/// Zero-allocation dispatch — no `Box::pin`, no `BoxFuture`.
impl<Ctx, F> ErasedHandler<Ctx> for F
where
    F: RpcFn<Ctx> + Send + Sync,
    Ctx: Send + Sync,
{
    fn name(&self) -> &'static str {
        F::NAME
    }

    fn kind(&self) -> &'static str {
        F::KIND
    }

    fn input_ts(&self) -> TsTypeInfo {
        type_ts::<F::Input>()
    }

    fn output_ts(&self) -> TsTypeInfo {
        type_ts::<F::Output>()
    }

    fn populate_types(&self, types: &mut specta::Types, top_level: &mut Vec<DataType>) {
        let input = F::Input::definition(types);
        let output = F::Output::definition(types);
        top_level.push(input);
        top_level.push(output);
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

    fn call_bytes(&self, ctx: &Ctx, input: Value) -> Result<Vec<u8>, RpcErr> {
        let input = if is_unit_type::<F::Input>() {
            unsafe { std::mem::zeroed() }
        } else {
            serde_json::from_value(input)
                .map_err(|e| RpcErr::bad_request(format!("deserialize input: {e}")))?
        };
        let output = F::exec(ctx, input)?;
        serde_json::to_vec(&output)
            .map_err(|e| RpcErr::internal(format!("serialize output: {e}")))
    }
}

// ── Helper: detect unit input type ──────────────────────────

/// Check at compile time whether `T` is `()` (unit).
/// Used to skip `serde_json::from_value` for unit inputs.
#[inline(always)]
fn is_unit_type<T: 'static>() -> bool {
    TypeId::of::<T>() == TypeId::of::<()>()
}

// ── Subscription traits ────────────────────────────────────

/// Typed RPC subscribe trait.
///
/// Implement this directly, or use the [`#[rpc_subscribe]`] proc macro.
///
/// Unlike [`RpcFn`], this trait is **sync** — it returns a `Stream` directly
/// rather than an async block. The stream itself can contain async work.
///
/// The returned stream must not borrow `ctx` — it is `'static`. If the
/// stream needs data from `Ctx`, clone it inside the function body before
/// constructing the stream. This avoids a per-event mpsc channel in the
/// transport layer.
///
/// # Constants
///
/// - `METHOD`: HTTP method — `"GET"` (default) or `"POST"` when
///   `#[rpc_subscribe("post")]` is used.
pub trait RpcSubscribe<Ctx>: Send + Sync {
    type Input: DeserializeOwned + Type;
    type Output: Serialize + Type + 'static;
    const NAME: &'static str;
    const KIND: &'static str = "subscribe";
    const METHOD: &'static str = "GET";

    /// Create a stream that yields items for this subscription.
    ///
    /// Must return a `'static` stream (no borrowing from `ctx`).
    fn exec(
        ctx: &Ctx,
        input: Self::Input,
    ) -> Pin<Box<dyn Stream<Item = Result<Self::Output, RpcErr>> + Send + 'static>>;
}

/// Object-safe erased subscribe handler stored in the router.
///
/// Blanket-implemented for all [`RpcSubscribe<Ctx>`] types.
pub trait ErasedSubscribeHandler<Ctx>: Send + Sync {
    fn name(&self) -> &'static str;
    /// HTTP method for this subscription — `"GET"` or `"POST"`.
    fn method(&self) -> &'static str;
    fn input_ts(&self) -> TsTypeInfo;
    fn output_ts(&self) -> TsTypeInfo;
    fn populate_types(&self, types: &mut specta::Types, top_level: &mut Vec<DataType>);
    /// Dispatch a subscription, returning a `'static` JSON-value stream.
    fn call(
        &self,
        ctx: &Ctx,
        input: Value,
    ) -> Pin<Box<dyn Stream<Item = Result<Value, RpcErr>> + Send + 'static>>;
}

// ── TypedHandler (zero-sized marker for typed registration) ──

/// Trait for typed handler registration (xitca-web–style).
///
/// Implemented by the zero-sized marker struct generated by
/// [`#[rpc_query]`](fnrpc_macros::rpc_query) and
/// [`#[rpc_mutate]`](fnrpc_macros::rpc_mutate).
///
/// Use with [`RpcRouterBuilder::at`](crate::router::RpcRouterBuilder::at).
pub trait TypedHandler<Ctx>: ErasedHandler<Ctx> {
    fn path() -> &'static str;
    fn method() -> &'static str;
}

/// Trait for typed subscribe handler registration.
///
/// Implemented by the zero-sized marker struct generated by
/// [`#[rpc_subscribe]`](fnrpc_macros::rpc_subscribe).
///
/// Use with [`RpcRouterBuilder::at_sub`](crate::router::RpcRouterBuilder::at_sub).
pub trait TypedSubscribeHandler<Ctx>: ErasedSubscribeHandler<Ctx> {
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
    fn name(&self) -> &'static str {
        F::NAME
    }

    fn method(&self) -> &'static str {
        F::METHOD
    }

    fn input_ts(&self) -> TsTypeInfo {
        type_ts::<F::Input>()
    }

    fn output_ts(&self) -> TsTypeInfo {
        type_ts::<F::Output>()
    }

    fn populate_types(&self, types: &mut specta::Types, top_level: &mut Vec<DataType>) {
        let input = F::Input::definition(types);
        let output = F::Output::definition(types);
        top_level.push(input);
        top_level.push(output);
    }

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
}
