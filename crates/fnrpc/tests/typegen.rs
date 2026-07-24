//! End-to-end test for the TypeScript *type generation* step.
//!
//! fnrpc proxies Specta: it turns a Rust `RpcRouter` into `bindings.ts`, which
//! contains (a) specta-exported type definitions and (b) fnrpc-specific
//! `Procedures` / `__procedureMeta` metadata. Specta's own docs are thin on the
//! "complex Rust type → TS" mapping, and that mapping lives in *our* layer —
//! so this test locks it down without spawning a server or writing a file.
//!
//! It calls `generate_ts_client` directly (which returns the TS source as a
//! `String`) and asserts on the generated text.
//!
//! Run: `cargo test -p fnrpc --test typegen`

use fnrpc::gen_ts_client::generate_ts_client;
use fnrpc::router::RpcRouterBuilder;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Inner {
    pub name: String,
}

/// A unit + data enum, to lock down how sum types map to TS.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum Status {
    Pending,
    Active,
    Done,
    #[serde(rename = "in_review")]
    InReview,
    WithCount(u32),
}

/// Exercise the type matrix that Specta's docs under-document and that fnrpc
/// maps itself: big integers, vectors of big integers, optional big integers,
/// nested structs, enums, and f64 (which fnrpc maps to `number | null`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Scalars {
    pub u: u64,
    pub i: i128,
    pub s: String,
    pub b: bool,
    pub opt: Option<u64>,
    pub vec: Vec<u64>,
    pub nest: Inner,
    pub status: Status,
    pub f: f64,
}

#[fnrpc::rpc_query]
pub async fn scalars_get(input: Scalars) -> Scalars {
    input
}

#[fnrpc::rpc_query("post")]
pub async fn scalars_post(input: Scalars) -> Scalars {
    input
}

#[fnrpc::rpc_mutate]
pub async fn scalars_mutate(input: Scalars) -> Scalars {
    input
}

#[fnrpc::rpc_subscribe]
pub fn scalars_stream(input: Scalars) -> impl futures::Stream<Item = Scalars> {
    futures::stream::once(async move { input })
}

/// Assert `generated` contains `needle`, with a readable failure message.
fn assert_contains(generated: &str, needle: &str) {
    assert!(
        generated.contains(needle),
        "generated bindings.ts is missing:\n  {needle}\n\nfull output:\n{generated}"
    );
}

fn build_router() -> fnrpc::router::RpcRouter<()> {
    RpcRouterBuilder::<()>::new()
        .route_fn(scalars_get)
        .route_fn(scalars_post)
        .route_fn(scalars_mutate)
        .subscribe(scalars_stream)
        .build()
}

#[test]
fn generates_expected_ts_types_and_metadata() {
    let router = build_router();
    let generated = generate_ts_client(&router);

    // ── specta type definitions ──────────────────────────────────────────
    // fnrpc maps big integers to `bigint` (Specta forbids this by default; we
    // override it), and delegates the rest to Specta's inline mapping.
    assert_contains(&generated, "export type Inner = {");
    assert_contains(&generated, "export type Scalars = {");

    // big integers → bigint (our proxy layer). Specta emits tab-indented fields.
    assert_contains(&generated, "\tu: bigint,");
    assert_contains(&generated, "\ti: bigint,");

    // primitives delegated to specta
    assert_contains(&generated, "\ts: string,");
    assert_contains(&generated, "\tb: boolean,");

    // Option<u64> → bigint | null (our bigint override applies inside Option)
    assert_contains(&generated, "\topt: bigint | null,");

    // Vec<u64> → bigint[] (bigint override applies inside Vec)
    assert_contains(&generated, "\tvec: bigint[],");

    // nested struct → referenced by its named type
    assert_contains(&generated, "\tnest: Inner,");

    // enum → referenced by its named type (sum-type mapping exercised below)
    assert_contains(&generated, "\tstatus: Status,");

    // f64 → `number | null` (fnrpc-specific override; Specta would forbid it
    // or map differently, so we lock our choice down explicitly)
    assert_contains(&generated, "\tf: number | null,");

    // ── enum sum type ─────────────────────────────────────────────────────
    // A Rust enum becomes a TS union of its variants. serde renames ARE applied
    // (via `specta_serde::PhasesFormat`), so the `#[serde(rename = "in_review")]`
    // variant surfaces as `"in_review"`, not the Rust identifier `"InReview"`.
    assert_contains(&generated, "export type Status =");
    assert_contains(&generated, "\"Pending\"");
    assert_contains(&generated, "\"Active\"");
    assert_contains(&generated, "\"in_review\"");
    // data-carrying variant `WithCount(u32)` maps to an object `{ WithCount: number }`
    assert_contains(&generated, "{ WithCount: number }");

    // ── fnrpc-specific Procedures interface ──────────────────────────────
    assert_contains(&generated, "export type Procedures = {");

    // GET query carries method "GET" (within an inline object literal)
    assert_contains(
        &generated,
        "scalars_get: { kind: \"query\"; method: \"GET\"; input: Scalars; output: Scalars; error: RpcErr };",
    );

    // POST query carries method "POST" — this is the part Specta cannot
    // express and fnrpc must generate from the macro attribute.
    assert_contains(
        &generated,
        "scalars_post: { kind: \"query\"; method: \"POST\"; input: Scalars; output: Scalars; error: RpcErr };",
    );

    // mutate carries kind "mutate"
    assert_contains(
        &generated,
        "scalars_mutate: { kind: \"mutate\"; method: \"POST\"; input: Scalars; output: Scalars; error: RpcErr };",
    );

    // subscribe is present with its input/output types
    assert_contains(
        &generated,
        "scalars_stream: { kind: \"subscribe\"; method: \"GET\"; input: Scalars; output: Scalars; error: RpcErr };",
    );

    // ── runtime metadata ─────────────────────────────────────────────────
    assert_contains(&generated, "export const __procedureMeta = {");
    assert_contains(
        &generated,
        "scalars_post: { kind: \"query\", method: \"POST\" },",
    );

    // ── error type ────────────────────────────────────────────────────────
    // `RpcErr` is the canonical error mirrored as `RpcError` on the TS client.
    // It must be exported as a named type (not just referenced via `error:`).
    // `name` comes from `&'static str`, `data` from our `Option<Unknown>` hint.
    assert_contains(&generated, "export type RpcErr = {");
    assert_contains(&generated, "\tname: string,");
    assert_contains(&generated, "\tcode: string,");
    assert_contains(&generated, "\tmessage: string,");
    assert_contains(&generated, "\tdata: unknown | null,");
}
