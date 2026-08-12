//! Snapshot of *Specta's* default TypeScript export behaviour — independent of
//! fnrpc's `FnrpcFormat` (bigint remap + serde attribute pass-through).
//!
//! Purpose: lock down what Specta emits out of the box, so we notice when a
//! Specta upgrade changes the mapping (and so we remember *why* fnrpc needs its
//! own `FnrpcFormat` on top). This is documentation-as-test: the assertions pin
//! the important fragments, and the `*_dump` tests print the full output for
//! human inspection (`cargo test -p fnrpc --test specta_defaults -- --nocapture`).
//!
//! Run: `cargo test -p fnrpc --test specta_defaults`

use serde::{Deserialize, Serialize};
use specta::datatype::DataType;
use specta::Type;
use specta_typescript::Typescript;

// ── fixtures ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[allow(dead_code)]
pub struct Plain {
    pub s: String,
    pub b: bool,
    pub f: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[allow(dead_code)]
pub enum Color {
    Red,
    Green,
    Blue,
    #[serde(rename = "light_blue")]
    LightBlue,
    Count(u32),
}

/// `serde_json::Value` is *not* a black box `any` in Specta — it expands into a
/// precise recursive union of every JSON variant. The `Number` branch is an
/// untagged enum whose `i64`/`u64` variants are what Specta forbids exporting
/// by default, so fnrpc still needs its remap step to render it as `number`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[allow(dead_code)]
pub struct WithJson {
    pub raw: serde_json::Value,
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// What Specta emits by default (serde attributes applied via `PhasesFormat`,
/// but **no** fnrpc bigint remap). Safe for types without BigInt-style integers.
fn export_default<T: Type>() -> Result<String, String> {
    let mut types = specta::Types::default();
    let _ = T::definition(&mut types);
    Typescript::default()
        .export(&types, specta_serde::PhasesFormat)
        .map_err(|e| e.to_string())
}

/// Replicates fnrpc's `FnrpcFormat` remap (bigint → `bigint`) so we can render
/// types that the *raw* Specta export would forbid (e.g. `serde_json::Value`,
/// which carries `i64`/`u64` Number branches).
fn export_resolved<T: Type>() -> Result<String, String> {
    use specta::datatype::Primitive;
    use specta::Format;

    let mut types = specta::Types::default();
    let _ = T::definition(&mut types);

    let bigint =
        <specta_typescript::BigInt as specta::Type>::definition(&mut specta::Types::default());
    let types = specta_serde::PhasesFormat.map_types(&types).unwrap();
    let remapper = specta_util::Remapper::new()
        .rule(Primitive::u64.into(), bigint.clone())
        .rule(Primitive::i64.into(), bigint.clone())
        .rule(Primitive::u128.into(), bigint.clone())
        .rule(Primitive::i128.into(), bigint.clone())
        .rule(Primitive::usize.into(), bigint.clone())
        .rule(Primitive::isize.into(), bigint.clone());
    let types = remapper.remap_types(types.into_owned());

    struct Noop;
    impl Format for Noop {
        fn map_types(
            &self,
            t: &specta::Types,
        ) -> Result<std::borrow::Cow<'_, specta::Types>, specta::FormatError> {
            Ok(std::borrow::Cow::Owned(t.clone()))
        }
        fn map_type(
            &self,
            _t: &specta::Types,
            dt: &DataType,
        ) -> Result<std::borrow::Cow<'_, DataType>, specta::FormatError> {
            Ok(std::borrow::Cow::Owned(dt.clone()))
        }
    }
    Typescript::default()
        .export(&types, Noop)
        .map_err(|e| e.to_string())
}

fn assert_contains(generated: &str, needle: &str) {
    assert!(
        generated.contains(needle),
        "specta default export is missing:\n  {needle}\n\nfull output:\n{generated}"
    );
}

// ── plain primitives (no bigint involved) ────────────────────────────────────

#[test]
fn default_plain_primitives() {
    let out = export_default::<Plain>().expect("Plain should export cleanly");
    // f64 maps to `number | null` even in default Specta (it forbids a bare f64).
    assert_contains(&out, "\tf: number | null,");
    assert_contains(&out, "\ts: string,");
    assert_contains(&out, "\tb: boolean,");
}

#[test]
fn default_enum_mapping() {
    let out = export_default::<Color>().expect("Color should export cleanly");
    // Unit variants become string literals; serde rename IS applied via PhasesFormat.
    assert_contains(&out, "\"Red\"");
    assert_contains(&out, "\"light_blue\"");
    // Data-carrying variant becomes an object literal.
    assert_contains(&out, "{ Count: number }");
}

// ── the important one: serde_json::Value is a precise recursive union ────────

#[test]
fn default_json_value_is_precise_union() {
    // Must go through the remap, otherwise Specta forbids the i64/u64 branches.
    let out = export_resolved::<WithJson>().expect("WithJson export should succeed after remap");
    // Not an `any` — it enumerates every JSON variant. Since the rc.26-era
    // serde fixes, the enum collapses to a flat structural union (Number is
    // modeled as an untagged finite number, so it renders as `number`).
    assert_contains(&out, "export type Value =");
    assert_contains(&out, "null | boolean | number | string | Value[] | { [key in string]: Value }");
    // No residual variant names (Null/Bool/Number/...) leak into the union.
    assert!(
        !out.contains("({ Bool: boolean })"),
        "old object-variant shape leaked into Value:\n{out}"
    );
}

/// Documents the *gotcha*: the default Specta export forbids BigInt-style
/// integers, so a bare `serde_json::Value` (via its `i64`/`u64` Number branches)
/// cannot be exported without fnrpc's remap. We assert the error is surfaced
/// rather than silently producing wrong types. This is exactly why
/// `gen_ts_client.rs::FnrpcFormat` exists.
#[test]
fn default_json_value_forbids_bigint() {
    let mut types = specta::Types::default();
    let _: DataType = serde_json::Value::definition(&mut types);
    let err = Typescript::default()
        .export(&types, specta_serde::PhasesFormat)
        .expect_err("default export of serde_json::Value must fail on bigint");
    assert!(
        err.to_string().contains("BigInt") || err.to_string().contains("forbids"),
        "expected a BigInt-forbidden error, got:\n{err}"
    );
}

// ── human-readable dumps (run with --nocapture) ──────────────────────────────

#[test]
fn dump_json_value_resolved() {
    match export_resolved::<WithJson>() {
        Ok(s) => eprintln!("=== serde_json::Value (resolved via fnrpc-style remap) ===\n{s}"),
        Err(e) => eprintln!("=== serde_json::Value resolved export ERROR ===\n{e}"),
    }
}

