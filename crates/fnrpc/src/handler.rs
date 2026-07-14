use std::pin::Pin;

use async_trait::async_trait;
use futures::stream::Stream;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use specta::datatype::{DataType, FunctionResultVariant};
use specta::{Generics, Type, TypeCollection};

use crate::error::RpcErr;

/// TypeScript type reference info for a single type (input or output).
#[derive(Debug, Clone)]
pub struct TsTypeInfo {
    /// TypeScript type reference name (e.g. `"HealthCheckOutput"`) or inline expression.
    pub ts_ref: String,
}

/// Compute the TS type reference name for a type.
fn type_ts<T: Type>() -> TsTypeInfo {
    let mut type_map = TypeCollection::default();
    let data_type = T::inline(&mut type_map, Generics::NONE);

    let ts_ref = match &data_type {
        DataType::Struct(s) => s.name().to_string(),
        DataType::Enum(e) => e.name().to_string(),
        _ => specta_typescript::datatype(
            &Default::default(),
            &FunctionResultVariant::Value(data_type),
            &type_map,
        )
        .unwrap_or_else(|_| "unknown".to_string()),
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
    /// Populate a shared `TypeCollection` and collect top-level input/output DataTypes.
    fn populate_types(
        &self,
        type_map: &mut TypeCollection,
        top_level: &mut Vec<DataType>,
    );
    async fn call(&self, ctx: &Ctx, input: Value) -> Result<Value, RpcErr>;
}

/// Typed RPC function trait.
///
/// Implement this directly, or use the `#[rpc_query]` / `#[rpc_mutation]` proc macros.
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

    fn populate_types(
        &self,
        type_map: &mut TypeCollection,
        top_level: &mut Vec<DataType>,
    ) {
        let input = F::Input::inline(type_map, Generics::NONE);
        let output = F::Output::inline(type_map, Generics::NONE);
        top_level.push(input);
        top_level.push(output);
    }

    async fn call(&self, ctx: &Ctx, input: Value) -> Result<Value, RpcErr> {
        let input: F::Input = serde_json::from_value(input)
            .map_err(|e| RpcErr(format!("deserialize input: {e}")))?;
        let output = F::exec(ctx, input).await?;
        Ok(serde_json::to_value(output)
            .map_err(|e| RpcErr(format!("serialize output: {e}")))?)
    }
}

// ── Subscription traits ────────────────────────────────────

/// Typed RPC subscription trait.
///
/// Implement this directly, or use the `#[rpc_subscription]` proc macro.
pub trait RpcSubscription<Ctx>: Send + Sync {
    type Input: DeserializeOwned + Type;
    type Output: Serialize + Type + 'static;
    const NAME: &'static str;
    const KIND: &'static str = "subscription";

    fn exec(
        ctx: &Ctx,
        input: Self::Input,
    ) -> Pin<Box<dyn Stream<Item = Result<Self::Output, RpcErr>> + Send + '_>>;
}

/// Object-safe erased subscription handler stored in the router.
pub trait ErasedSubscriptionHandler<Ctx>: Send + Sync {
    fn name(&self) -> &'static str;
    fn input_ts(&self) -> TsTypeInfo;
    fn output_ts(&self) -> TsTypeInfo;
    fn populate_types(
        &self,
        type_map: &mut TypeCollection,
        top_level: &mut Vec<DataType>,
    );
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

    fn populate_types(
        &self,
        type_map: &mut TypeCollection,
        top_level: &mut Vec<DataType>,
    ) {
        let input = F::Input::inline(type_map, Generics::NONE);
        let output = F::Output::inline(type_map, Generics::NONE);
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
                return Box::pin(futures::stream::once(futures::future::ready(Err(
                    RpcErr(format!("deserialize input: {e}")),
                ))))
            }
        };
        let stream = F::exec(ctx, input);
        Box::pin(stream.map(|item| match item {
            Ok(v) => serde_json::to_value(v)
                .map_err(|e| RpcErr(format!("serialize output: {e}"))),
            Err(e) => Err(e),
        }))
    }
}
