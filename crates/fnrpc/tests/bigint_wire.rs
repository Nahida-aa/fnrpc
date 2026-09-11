//! End-to-end wire tests for BigInt handling.
//!
//! These drive the **real dispatch path** — `RpcRouterBuilder` → `dispatch` →
//! handler → `decode_bigint_by_schema` / `encode_bigint_by_schema` → serialized
//! response bytes — rather than calling the codec functions directly. The wire
//! bodies are what the TypeScript client actually sends via `toRustJson`:
//! BigInt fields as JSON *strings*, no envelope.
//!
//! The expected `meta` paths must name the **serde (wire)** keys, because that
//! is the only name the client — which has no schema — can look up.
//!
//! Run: `cargo test -p fnrpc --test bigint_wire`

use fnrpc::output::RpcOutput;
use fnrpc::router::RpcRouterBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;

fn envelope(out: &RpcOutput) -> Value {
    serde_json::from_slice(&out.data).expect("response must be a JSON envelope")
}

fn dispatch(router: &fnrpc::router::RpcRouter<()>, key: &str, wire: &[u8]) -> RpcOutput {
    // Single-threaded runtime: `dispatch` is the real async dispatch path, but
    // these tests only need to observe the bytes that come out of it.
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build test runtime")
        .block_on(router.dispatch(&(), key, wire, false))
        .unwrap_or_else(|e| panic!("dispatch `{key}` failed: {e}"))
}

// ── Baseline: plain struct, no serde attributes ────────────────────
// The one shape the path reflection currently gets right. Kept as a control so
// a failure in the cases below is known to be about the serde/typing shape and
// not about the harness.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
struct Plain {
    id: u64,
    name: String,
}

#[fnrpc::rpc_query("post")]
async fn plain_echo(input: Plain) -> Plain {
    input
}

#[test]
fn plain_struct_is_the_control_case() {
    let router = RpcRouterBuilder::<()>::new().route_fn(plain_echo).build();
    let out = dispatch(&router, "plain_echo", br#"{"id":"18446744073709551615","name":"x"}"#);
    let env = envelope(&out);
    assert_eq!(env["meta"], json!([[0, "id"]]));
    assert_eq!(env["json"]["id"], json!("18446744073709551615"));
}

// ── 1. `#[serde(rename_all = "camelCase")]` ────────────────────────
// The most common real-world trigger: `rename_all` is idiomatic in Rust web
// APIs, and the generated TS *does* show `userId` (PhasesFormat applies it),
// so the mismatch is invisible until a BigInt travels over the wire.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
struct Camel {
    user_id: u64,
    name: String,
}

#[fnrpc::rpc_query("post")]
async fn camel_echo(input: Camel) -> Camel {
    input
}

#[test]
fn rename_all_camelcase_bigint() {
    let router = RpcRouterBuilder::<()>::new().route_fn(camel_echo).build();
    let out = dispatch(
        &router,
        "camel_echo",
        br#"{"userId":"18446744073709551615","name":"x"}"#,
    );
    let env = envelope(&out);
    assert_eq!(
        env["meta"],
        json!([[0, "userId"]]),
        "meta must name the serde (wire) key, not the Rust field name"
    );
    assert_eq!(env["json"]["userId"], json!("18446744073709551615"));
}

// ── 2. `#[serde(flatten)]` ────────────────────────────────────────
// Flattened fields have no nesting on the wire, so the path must not contain
// the intermediate struct's field name.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
struct FlatInner {
    big: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
struct Flat {
    #[serde(flatten)]
    inner: FlatInner,
    tag: String,
}

#[fnrpc::rpc_query("post")]
async fn flat_echo(input: Flat) -> Flat {
    input
}

#[test]
fn flatten_bigint() {
    let router = RpcRouterBuilder::<()>::new().route_fn(flat_echo).build();
    let out = dispatch(
        &router,
        "flat_echo",
        br#"{"big":"18446744073709551615","tag":"t"}"#,
    );
    let env = envelope(&out);
    assert_eq!(
        env["meta"],
        json!([[0, "big"]]),
        "flattened fields are hoisted, so the path must not contain `inner`"
    );
    assert_eq!(env["json"]["big"], json!("18446744073709551615"));
}

// ── 3. Generic wrapper ────────────────────────────────────────────
// The BigInt lives in a generic *argument*, not in the generic definition, so
// reflection has to substitute type arguments before walking.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
struct Wrapper<T> {
    v: T,
    label: String,
}

#[fnrpc::rpc_query("post")]
async fn wrapper_echo(input: Wrapper<u64>) -> Wrapper<u64> {
    input
}

#[test]
fn generic_wrapper_bigint() {
    let router = RpcRouterBuilder::<()>::new().route_fn(wrapper_echo).build();
    let out = dispatch(
        &router,
        "wrapper_echo",
        br#"{"v":"18446744073709551615","label":"g"}"#,
    );
    let env = envelope(&out);
    assert_eq!(
        env["meta"],
        json!([[0, "v"]]),
        "generic argument substitution must reach the BigInt inside `Wrapper<u64>`"
    );
    assert_eq!(env["json"]["v"], json!("18446744073709551615"));
}

// ── 4. Enum payloads (externally tagged) ──────────────────────────
// serde's default external tagging puts the payload *under the variant name*:
// `{"Small": <u64>}`. Paths must therefore be prefixed with the variant, and
// a single unnamed field is addressed by the variant key alone — not by an
// index, because the wire value is not an array.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
enum Payload {
    Small(u64),
    Named { big: i64 },
    Empty,
}

#[fnrpc::rpc_query("post")]
async fn payload_echo(input: Payload) -> Payload {
    input
}

#[test]
fn enum_newtype_variant_bigint() {
    let router = RpcRouterBuilder::<()>::new().route_fn(payload_echo).build();
    let out = dispatch(
        &router,
        "payload_echo",
        br#"{"Small":"18446744073709551615"}"#,
    );
    let env = envelope(&out);
    // `meta` is schema-driven, so it lists every variant's BigInt leaves, not
    // just the one that happens to be active in this payload.
    let meta = env["meta"].as_array().expect("meta must be an array");
    assert!(
        meta.contains(&json!([0, "Small"])),
        "externally-tagged newtype payload is addressed by the variant key, not an index; got {meta:?}"
    );
    assert_eq!(env["json"]["Small"], json!("18446744073709551615"));
}

#[test]
fn enum_named_variant_bigint() {
    let router = RpcRouterBuilder::<()>::new().route_fn(payload_echo).build();
    let out = dispatch(
        &router,
        "payload_echo",
        br#"{"Named":{"big":"9223372036854775807"}}"#,
    );
    let env = envelope(&out);
    let meta = env["meta"].as_array().expect("meta must be an array");
    assert!(
        meta.contains(&json!([0, "Named", "big"])),
        "named-variant fields are nested under the variant key; got {meta:?}"
    );
    assert_eq!(env["json"]["Named"]["big"], json!("9223372036854775807"));
}

// ── 5. Newtype struct ─────────────────────────────────────────────
// serde serialises a newtype struct transparently, so `UserId(u64)` is a bare
// number on the wire — there is no `[0]` index to descend into.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
struct UserId(u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
struct HasId {
    id: UserId,
}

#[fnrpc::rpc_query("post")]
async fn newtype_echo(input: HasId) -> HasId {
    input
}

#[test]
fn newtype_struct_bigint() {
    let router = RpcRouterBuilder::<()>::new().route_fn(newtype_echo).build();
    let out = dispatch(
        &router,
        "newtype_echo",
        br#"{"id":"18446744073709551615"}"#,
    );
    let env = envelope(&out);
    assert_eq!(
        env["meta"],
        json!([[0, "id"]]),
        "a newtype is transparent on the wire, so no index segment applies"
    );
    assert_eq!(env["json"]["id"], json!("18446744073709551615"));
}

// ── 6. Envelope constancy ─────────────────────────────────────────
// The side that does *not* own the schema (the TS client) cannot tell an
// envelope from an ordinary object with `json`/`meta` fields. The only thing
// that makes parsing unambiguous is that the envelope is **always** present —
// so it must be emitted even when there is nothing to annotate.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
struct NoBigInt {
    name: String,
    count: i32,
}

#[fnrpc::rpc_query("post")]
async fn no_bigint_echo(input: NoBigInt) -> NoBigInt {
    input
}

#[test]
fn envelope_is_always_emitted_even_without_bigint() {
    let router = RpcRouterBuilder::<()>::new()
        .route_fn(no_bigint_echo)
        .build();
    let out = dispatch(&router, "no_bigint_echo", br#"{"name":"x","count":1}"#);
    let env = envelope(&out);
    // Both keys must exist so the client can parse unconditionally.
    assert!(env.get("json").is_some(), "envelope is missing `json`");
    assert_eq!(env["meta"], json!([]), "envelope is missing an empty `meta`");
}
