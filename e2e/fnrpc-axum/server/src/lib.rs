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

/// Echo the input bigint struct back to the client. The server encodes the
/// response as a `{ json, meta }` envelope so the client restores `BigInt`
/// values at full precision (no precision loss, no `String` workaround).
#[fnrpc::rpc_query]
pub async fn big_echo(input: BigInput) -> BigInput {
    input
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
pub async fn big_echo_mutate(input: BigInput) -> BigInput {
    input
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BigOutput {
    pub id: u64,
    pub big: i128,
    pub list: Vec<u64>,
}

/// Return a bigint struct so the client can assert full precision on the
/// response (BigInt envelope, restored via `meta`).
#[fnrpc::rpc_query]
pub async fn big_out(_input: ()) -> BigOutput {
    BigOutput {
        id: 18446744073709551615u64,
        big: 170141183460469231731687303715884105727i128,
        list: vec![1, 18446744073709551615],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TickInput {
    pub start: u64,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TickOutput {
    pub n: u64,
}

/// Build the tick stream: a head message carrying `start`, then `count`
/// sequence numbers (`TickOutput.n`, a `u64`).
fn tick_stream(input: TickInput) -> impl futures::Stream<Item = TickOutput> {
    let start = input.start;
    let count = input.count;
    let head = TickOutput { n: start };
    futures::stream::once(async move { head }).chain(futures::stream::unfold(
        0u64,
        move |i| async move {
            if i >= count {
                None
            } else {
                Some((TickOutput { n: i }, i + 1))
            }
        },
    ))
}

/// SSE subscription (GET) that emits each tick's sequence number as a `u64`
/// (`TickOutput.n`), proving the response-direction BigInt envelope works
/// over SSE, plus a head message embedding the exact `start` value (proving
/// BigInt precision on the request).
#[fnrpc::rpc_subscribe]
pub fn tick_seq(input: TickInput) -> impl futures::Stream<Item = TickOutput> {
    tick_stream(input)
}

/// Same as `tick_seq` but served over POST (input in the request body instead
/// of the query string). Proves the POST SSE path also carries the
/// response-direction BigInt envelope.
#[fnrpc::rpc_subscribe("post")]
pub fn tick_seq_post(input: TickInput) -> impl futures::Stream<Item = TickOutput> {
    tick_stream(input)
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
        .route_fn(big_out)
        .subscribe(tick_seq)
        .subscribe(tick_seq_post)
        .build()
}
