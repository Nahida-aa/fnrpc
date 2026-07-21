//! End-to-end example library: shared router + procedures for the
//! `fnrpc-axum` + `axum` e2e server. Exposed as a library so both the
//! `main` server binary and the `gen_fnrpc` codegen binary share exactly
//! the same router (so generated bindings match the served procedures).
//!
//! The request wire format is plain JSON with bigint fields encoded as
//! strings (no `meta` envelope); the server decodes them back into `u64`/`i128`
//! using its own schema (`fnrpc::serializer::decode_bigint_by_schema`).
//!
//! To keep the example self-verifying without depending on the *response*
//! bigint handling (which is a separate, later piece of work), the handlers
//! return a `String` confirmation that embeds the exact received values,
//! or (for the SSE subscription) emit confirmations as streamed strings.

use fnrpc::router::RpcRouterBuilder;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BigInput {
    pub id: u64,
    pub big: i128,
    pub list: Vec<u64>,
}

/// Echo a confirmation string with the exact bigint values the server decoded.
///
/// Returning a `String` (rather than the bigint struct) lets the TS client
/// assert full precision on the response without relying on response-side
/// BigInt envelope handling.
#[fnrpc::rpc_query]
pub async fn big_echo(input: BigInput) -> String {
    format!("id={} big={} list={:?}", input.id, input.big, input.list)
}

/// 原始
#[fnrpc::rpc_query]
pub async fn big_echo_primitive(input: u64) -> String {
    format!("input={input}",)
}
#[fnrpc::rpc_query("post")]
pub async fn big_echo_primitive_post(input: u64) -> String {
    format!("input={input}",)
}

#[fnrpc::rpc_mutate]
pub async fn big_echo_primitive_mutate(input: u64) -> String {
    format!("input={input}",)
}

#[fnrpc::rpc_mutate]
pub async fn big_echo_mutate(input: BigInput) -> String {
    format!("id={} big={} list={:?}", input.id, input.big, input.list)
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TickInput {
    pub start: u64,
    pub count: u64,
}

/// SSE subscription that emits a head message with the exact `start` value
/// (proving BigInt precision on the request), followed by `count` tick
/// messages. Lets the TS client assert both BigInt precision and the
/// subscribe/SSE transport end-to-end.
#[fnrpc::rpc_subscribe]
pub fn tick_seq(input: TickInput) -> impl futures::Stream<Item = String> {
    let start = input.start;
    let count = input.count;
    let head = format!("start={start}");
    futures::stream::once(async move { head }).chain(futures::stream::unfold(
        0u64,
        move |i| async move {
            if i >= count {
                None
            } else {
                Some((format!("n={i}"), i + 1))
            }
        },
    ))
}

/// Build the shared router used by both the server (`main`) and the codegen
/// binary (`gen_fnrpc`), so the generated bindings exactly match the served
/// procedures.
pub fn build_fn_rpc_router() -> fnrpc::router::RpcRouter<()> {
    RpcRouterBuilder::<()>::new()
        .route_fn(big_echo)
        .route_fn(big_echo_primitive)
        .route_fn(big_echo_primitive_post)
        .route_fn(big_echo_primitive_mutate)
        .route_fn(big_echo_mutate)
        .subscribe(tick_seq)
        .build()
}
