use std::pin::Pin;

use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::Stream;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use specta::Type;
use specta::datatype::{DataType, Primitive, Reference};

use crate::error::RpcErr;

/// TypeScript type reference info for a single type (input or output).
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
#[async_trait]
pub trait ErasedHandler<Ctx>: Send + Sync {
    fn name(&self) -> &'static str;
    fn kind(&self) -> &'static str;
    fn input_ts(&self) -> TsTypeInfo;
    fn output_ts(&self) -> TsTypeInfo;
    /// Populate a shared type registry and collect top-level input/output DataTypes.
    fn populate_types(&self, types: &mut specta::Types, top_level: &mut Vec<DataType>);
    async fn call(&self, ctx: &Ctx, input: Value) -> Result<Value, RpcErr>;
}

/// Typed RPC function trait.
///
/// Implement this directly, or use the `#[rpc_query]` / `#[rpc_mutate]` proc macros.
#[async_trait]
pub trait RpcFn<Ctx>: Send + Sync {
    type Input: DeserializeOwned + Type;
    type Output: Serialize + Type;
    const NAME: &'static str;
    const KIND: &'static str = "query";

    async fn exec(ctx: &Ctx, input: Self::Input) -> Result<Self::Output, RpcErr>;
}

/// Blanket impl: any `RpcFn<Ctx>` becomes an `ErasedHandler<Ctx>`.
#[async_trait]
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

    async fn call(&self, ctx: &Ctx, input: Value) -> Result<Value, RpcErr> {
        let input: F::Input =
            serde_json::from_value(input).map_err(|e| RpcErr(format!("deserialize input: {e}")))?;
        let output = F::exec(ctx, input).await?;
        Ok(serde_json::to_value(output).map_err(|e| RpcErr(format!("serialize output: {e}")))?)
    }
}

// ── Subscription traits ────────────────────────────────────

/// Typed RPC subscribe trait.
///
/// Implement this directly, or use the `#[rpc_subscribe]` proc macro.
pub trait RpcSubscription<Ctx>: Send + Sync {
    type Input: DeserializeOwned + Type;
    type Output: Serialize + Type + 'static;
    const NAME: &'static str;
    const KIND: &'static str = "subscribe";

    fn exec(
        ctx: &Ctx,
        input: Self::Input,
    ) -> Pin<Box<dyn Stream<Item = Result<Self::Output, RpcErr>> + Send + '_>>;
}

/// Object-safe erased subscribe handler stored in the router.
pub trait ErasedSubscriptionHandler<Ctx>: Send + Sync {
    fn name(&self) -> &'static str;
    fn input_ts(&self) -> TsTypeInfo;
    fn output_ts(&self) -> TsTypeInfo;
    fn populate_types(&self, types: &mut specta::Types, top_level: &mut Vec<DataType>);
    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        input: Value,
    ) -> Pin<Box<dyn Stream<Item = Result<Value, RpcErr>> + Send + 'a>>;
}

/// Blanket impl: any `RpcSubscription<Ctx>` becomes an `ErasedSubscriptionHandler<Ctx>`.
impl<Ctx, F> ErasedSubscriptionHandler<Ctx> for F
where
    F: RpcSubscription<Ctx> + Send + Sync,
    Ctx: Send + Sync,
    <F as RpcSubscription<Ctx>>::Output: 'static,
{
    fn name(&self) -> &'static str {
        F::NAME
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

    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        input: Value,
    ) -> Pin<Box<dyn Stream<Item = Result<Value, RpcErr>> + Send + 'a>> {
        let input = match serde_json::from_value(input) {
            Ok(v) => v,
            Err(e) => {
                return Box::pin(futures::stream::once(futures::future::ready(Err(RpcErr(
                    format!("deserialize input: {e}"),
                )))));
            }
        };
        let stream = F::exec(ctx, input);
        Box::pin(stream.map(|item| match item {
            Ok(v) => serde_json::to_value(v).map_err(|e| RpcErr(format!("serialize output: {e}"))),
            Err(e) => Err(e),
        }))
    }
}
