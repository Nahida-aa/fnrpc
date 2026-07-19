//! Handler traits for RPC functions.
//!
//! - [`RpcFn`] for typed query & mutate procedures.
//! - [`RpcFnExt`] provides default [`call_bytes`](RpcFnExt::call_bytes),
//!   [`call`](RpcFnExt::call), and [`call_value`](RpcFnExt::call_value) impls.
//! - [`RawRpcFn`] for raw byte-buffer handlers.
//! - [`RpcSubscribe`] / [`SubscribeExt`] for subscriptions.

use std::any::TypeId;
use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures::StreamExt;
use futures::stream::Stream;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use specta::Type;

use crate::codec::{JsonCodec, RpcCodec};
use crate::error::RpcErr;

/// TypeScript type reference info for a single type (input or output).
///
/// Produced by [`type_ts`](crate::gen_ts_client::type_ts) and used during codegen to determine the
/// TypeScript type name for a given Rust type.
#[derive(Debug, Clone)]
pub struct TsTypeInfo {
    /// TypeScript type reference name (e.g. `"HealthCheckOutput"`) or inline expression.
    pub ts_ref: String,
}

// ── RpcFn (typed, serde-based) ────────────────────────────

/// Typed RPC function trait using serde serialization.
///
/// # Defaults
///
/// - [`KIND`](Self::KIND) = `"query"`
/// - [`METHOD`](Self::METHOD) = `"GET"`
///
/// Override these constants to register as a mutate (POST) procedure.
pub trait RpcFn<Ctx>: Send + Sync {
    type Input: DeserializeOwned + Type + 'static;
    type Output: Serialize + Type + 'static;
    const KEY: &'static str;
    const KIND: &'static str = "query";
    /// HTTP method: "GET" (default, input from query string) or "POST" (input from body).
    const METHOD: &'static str = "GET";

    fn exec(ctx: &Ctx, input: Self::Input) -> Pin<Box<dyn Future<Output = Result<Self::Output, RpcErr>> + Send + '_>>;
}

// ── RpcFnExt ──────────────────────────────────────────────

/// Extension trait providing default [`call_bytes`](Self::call_bytes),
/// [`call`](Self::call), and [`call_value`](Self::call_value) implementations.
///
/// These methods handle serialization/deserialization and are the primary
/// dispatch interface used by transport layers (fnrpc-web, fnrpc-axum, etc.).
///
/// All return `impl Future` — zero boxing, monomorphized at compile time.
pub trait RpcFnExt<Ctx>: RpcFn<Ctx> {
    /// Primary dispatch: raw bytes in, raw bytes out.
    fn call_bytes<'a>(
        &'a self,
        ctx: &'a Ctx,
        input: &'a [u8],
    ) -> impl Future<Output = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'a;

    /// Dispatch a call, returning a JSON value.
    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        input: Value,
    ) -> impl Future<Output = Result<Value, RpcErr>> + Send + 'a;

    /// Dispatch from a JSON [`Value`], returning serialized bytes.
    fn call_value<'a>(
        &'a self,
        ctx: &'a Ctx,
        input: Value,
    ) -> impl Future<Output = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'a;
}

impl<Ctx: Send + Sync + 'static, T: RpcFn<Ctx>> RpcFnExt<Ctx> for T {
    fn call_bytes<'a>(
        &'a self,
        ctx: &'a Ctx,
        input: &'a [u8],
    ) -> impl Future<Output = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'a {
        async move {
            let input = if is_unit_type::<T::Input>() {
                unsafe { std::mem::zeroed() }
            } else {
                JsonCodec::decode::<T::Input>(input)?
            };
            let output = T::exec(ctx, input).await?;
            Ok(JsonCodec::encode(&output).map(Cow::Owned).unwrap())
        }
    }

    fn call<'a>(
        &'a self,
        ctx: &'a Ctx,
        input: Value,
    ) -> impl Future<Output = Result<Value, RpcErr>> + Send + 'a {
        async move {
            let input = if is_unit_type::<T::Input>() {
                unsafe { std::mem::zeroed() }
            } else {
                serde_json::from_value(input)
                    .map_err(|e| RpcErr::bad_request(format!("deserialize input: {e}")))?
            };
            let output = T::exec(ctx, input).await?;
            Ok(serde_json::to_value(output)
                .map_err(|e| RpcErr::internal(format!("serialize output: {e}")))?)
        }
    }

    fn call_value<'a>(
        &'a self,
        ctx: &'a Ctx,
        input: Value,
    ) -> impl Future<Output = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'a {
        async move {
            let input = if is_unit_type::<T::Input>() {
                unsafe { std::mem::zeroed() }
            } else {
                serde_json::from_value(input)
                    .map_err(|e| RpcErr::bad_request(format!("deserialize input: {e}")))?
            };
            let output = T::exec(ctx, input).await?;
            Ok(serde_json::to_vec(&output)
                .map(Cow::Owned)
                .map_err(|e| RpcErr::internal(format!("serialize output: {e}")))?)
        }
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
/// Raw handlers are not included in codegen.
pub trait RawRpcFn<Ctx>: Send + Sync {
    const KEY: &'static str;
    fn exec<'a>(ctx: &'a Ctx, input: &'a [u8]) -> Pin<Box<dyn Future<Output = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'a>>;
}

// ── Subscription traits ────────────────────────────────────

/// Typed RPC subscribe trait.
pub trait RpcSubscribe<Ctx>: Send + Sync {
    type Input: DeserializeOwned + Type;
    type Output: Serialize + Type + 'static;
    const KEY: &'static str;
    const METHOD: &'static str = "GET";

    fn exec(
        ctx: &Ctx,
        input: Self::Input,
    ) -> Pin<Box<dyn Stream<Item = Result<Self::Output, RpcErr>> + Send + 'static>>;
}

/// Extension trait providing [`call`](SubscribeExt::call) and
/// [`call_bytes`](SubscribeExt::call_bytes) for subscribe handlers.
pub trait SubscribeExt<Ctx>: RpcSubscribe<Ctx> {
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

impl<Ctx: Send + Sync + 'static, F: RpcSubscribe<Ctx>> SubscribeExt<Ctx> for F {
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

// ── HandlerFn trait (avoids Arc::clone by borrowing &self) ──

/// Object-safe handler trait that returns futures borrowing `&self`.
/// Replaces `Arc<dyn Fn>` to avoid atomic reference counting overhead.
pub trait HandlerFn<Ctx>: Send + Sync {
    fn call<'a>(&'a self, ctx: &'a Ctx, input: Value) -> Pin<Box<dyn Future<Output = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'a>>;
}

/// Object-safe bytes handler trait.
pub trait BytesHandlerFn<Ctx>: Send + Sync {
    fn call<'a>(&'a self, ctx: &'a Ctx, input: &'a [u8]) -> Pin<Box<dyn Future<Output = Result<Cow<'static, [u8]>, RpcErr>> + Send + 'a>>;
}

// ── Handler enum (unified dispatch) ──────────────────────

/// A unified handler that can be either a typed RPC function or a bytes handler.
pub enum Handler<Ctx: Send + Sync + 'static> {
    /// Typed RPC function — input/output via JSON Value.
    Rpc {
        f: Box<dyn HandlerFn<Ctx>>,
        skip_query: bool,
    },
    /// Bytes handler — raw input/output.
    Bytes(Box<dyn BytesHandlerFn<Ctx>>),
}

impl<Ctx: Send + Sync + 'static> Handler<Ctx> {
    /// Call the handler. Returns (bytes, is_json) where is_json indicates
    /// whether the response should have Content-Type: application/json.
    pub async fn call(&self, ctx: &Ctx, input: &[u8], is_get: bool) -> Result<(Cow<'static, [u8]>, bool), RpcErr> {
        match self {
            Handler::Rpc { f, skip_query } => {
                let input_val: Value = if *skip_query {
                    Value::Null
                } else if is_get {
                    let query_str = std::str::from_utf8(input).unwrap_or("");
                    query_str.split('&').find_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        let key = parts.next()?;
                        let val = parts.next()?;
                        if key == "input" {
                            let decoded = percent_decode(val);
                            serde_json::from_str(&decoded).ok()
                        } else {
                            None
                        }
                    }).unwrap_or(Value::Null)
                } else {
                    serde_json::from_slice(input).unwrap_or(Value::Null)
                };
                f.call(ctx, input_val).await.map(|b| (b, true))
            }
            Handler::Bytes(f) => f.call(ctx, input).await.map(|b| (b, false)),
        }
    }
}

fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        match b {
            b'+' => result.push(' '),
            b'%' => {
                let hi = bytes.next().and_then(|c| hex_val(c));
                let lo = bytes.next().and_then(|c| hex_val(c));
                match (hi, lo) {
                    (Some(h), Some(l)) => result.push((h << 4 | l) as char),
                    _ => result.push('%'),
                }
            }
            _ => result.push(b as char),
        }
    }
    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
