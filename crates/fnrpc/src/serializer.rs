//! Server-side BigInt decoding driven by the handler's own schema.
//!
//! A `BigInt`-style Rust integer (`u64`/`i64`/`usize`/...) cannot be carried
//! through JSON as a number without losing precision, so on the wire it is
//! represented as a string. The TS client encodes these as strings via
//! [`fnrpc_client::toRustJson`] and sends a **plain JSON value** (no envelope),
//! so the server receives plain JSON with bigint fields already as strings.
//!
//! The server knows each field's true type from its own [`specta::Type`] schema
//! ([`crate::handler::RpcFn::Input`]), so it converts those string leaves back
//! into JSON numbers *itself*. It never relies on a client-supplied envelope or
//! `meta` to locate bigint fields — the schema is the single source of truth on
//! the request side.
//!
//! The schema is reflected over its **serde-resolved** view (see
//! [`wire_type`]), not specta's raw Rust view. That matters: `T::definition`
//! reports Rust field names and no serde rewrites, while the JSON is produced
//! by serde. Reflecting over the raw view yields paths that do not exist in the
//! payload for renamed, flattened, generic, or enum-tagged shapes.
//!
//! The response side is asymmetric: the client has no schema, so the server
//! **always** emits a fixed `{ json, meta }` envelope. `meta` lists the paths
//! of BigInt leaves (empty `[]` when there are none) so the client knows where
//! to restore `BigInt`s. The shape is constant — never bare JSON — so the
//! client parses it the same way every time. See [`encode_bigint_by_schema`].
//!
//! The client-side analogue of the envelope codec lives in the `fnrpc-client`
//! crate ([`fnrpc_client::unpack_meta`]).

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use serde_json::Value;
use specta::datatype::{
    DataType, Enum, Fields, Generic, List, NamedFields, NamedReferenceType, Primitive, Reference,
    UnnamedFields,
};
use specta::{Format as _, Type, Types};
use specta_serde::{Phase, PhasesFormat, select_phase_datatype};

/// Convert a bigint-typed JSON value from its wire (string) form into a JSON
/// number, using the server's own schema rather than a client `meta` envelope.
///
/// The client always sends a plain JSON value (via [`fnrpc_client::toRustJson`])
/// with bigint fields already encoded as strings, so `input` is decoded as-is —
/// there is no request-side envelope to unwrap.
///
/// Only fields whose type in `T`'s schema is a BigInt-style integer are
/// touched; any such field that arrives as a string is converted to a number.
/// Fields that already arrive as numbers (e.g. from a client that already
/// narrowed them) are left untouched.
///
/// The BIGINT type ID (`0`) is defined in the TS serializer and in
/// `fnrpc-client` (`fnrpc_client::unpack_meta`); this decoder does not need it
/// because it is driven entirely by the schema, not by `meta`.
pub fn decode_bigint_by_schema<T: Type + 'static>(input: Value) -> Value {
    let paths = bigint_paths::<T>(Phase::Deserialize);

    let mut payload = input;
    for path in paths.iter() {
        apply_at(&mut payload, path, 0);
    }
    payload
}

/// The serde-aware ("wire") view of `T` in the given direction.
///
/// `T::definition` yields specta's *Rust* view of the type: Rust field names,
/// no serde renames, no flattened fields hoisted, no enum tagging applied.
/// The JSON on the wire is produced by **serde**, so reflecting over the raw
/// view produces paths that do not exist in the payload (see the module docs).
///
/// `PhasesFormat` is the same rewrite the codegen pipeline applies before
/// exporting TypeScript, so walking its output yields paths whose names match
/// the real JSON keys. `select_phase_datatype` then picks the direction we
/// need: what the client *sent* follows the deserialize shape, what the server
/// *returns* follows the serialize shape.
///
/// Not every type graph survives phase resolution; when it fails we fall back
/// to the raw graph so a request still produces a response (with the same
/// possibly-wrong paths as before) instead of failing outright.
fn wire_type<T: Type + 'static>(phase: Phase) -> (Types, DataType) {
    let mut types = Types::default();
    let dt = T::definition(&mut types);

    match PhasesFormat.map_types(&types) {
        Ok(resolved) => {
            let resolved = resolved.into_owned();
            let dt = select_phase_datatype(&dt, &resolved, phase);
            (resolved, dt)
        }
        Err(_) => (types, dt),
    }
}

/// Cached BigInt leaf paths, keyed by `(type, direction)`.
///
/// The paths depend only on `T`'s schema, never on the value, so they are
/// computed once per `(T, phase)` instead of on every request — resolving the
/// serde phases clones the whole type registry, which is far too expensive to
/// redo per call.
type PathCache = Mutex<HashMap<(TypeId, bool), Arc<Vec<Vec<Segment>>>>>;

static BIGINT_PATH_CACHE: OnceLock<PathCache> = OnceLock::new();

fn bigint_paths<T: Type + 'static>(phase: Phase) -> Arc<Vec<Vec<Segment>>> {
    let key = (TypeId::of::<T>(), phase == Phase::Serialize);
    let cache = BIGINT_PATH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(paths) = guard.get(&key) {
        return Arc::clone(paths);
    }

    let (types, dt) = wire_type::<T>(phase);
    let mut paths: Vec<Vec<Segment>> = Vec::new();
    collect_bigint_paths(
        &dt,
        &mut Vec::new(),
        &mut paths,
        &types,
        phase,
        &[],
        0,
    );
    let paths = Arc::new(paths);
    guard.insert(key, Arc::clone(&paths));
    paths
}

/// Type ID used in the response `meta` array to mark a BigInt leaf.
///
/// Must match `BIGINT` in `packages/fnrpc-client/src/serializer.ts` so the
/// TS client's `deserialize` can restore `BigInt` values.
pub const BIGINT_TYPE_ID: u8 = 0;

/// One entry in the response `meta` array: `[type_id, ...path]`.
///
/// `path` segments are field names (`string`) or array indices (`usize`),
/// mirroring the TS `MetaItem` layout consumed by `deserialize`.
pub(crate) type MetaItem = (u8, Vec<Segment>);

/// Encode a handler output into a wire value that preserves BigInt precision,
/// driven entirely by the handler's own schema (symmetric to
/// [`decode_bigint_by_schema`] on the request side).
///
/// The response is **always** wrapped as `{ "json": <json>, "meta": [...] }`,
/// regardless of whether any BigInt leaves are present. `meta` is an array of
/// `[type_id, ...path]` entries; when there are no BigInt fields it is simply
/// an empty array `[]`.
///
/// The envelope shape is fixed on purpose: the wire protocol must not switch
/// form based on runtime reflection (e.g. "bare JSON when no bigint"), because
/// any such heuristic is untrustworthy and forces the client to sniff for the
/// envelope. A constant structure lets the client always parse `{ json, meta }`
/// and rebuild `BigInt`s from `meta` without guessing.
pub fn encode_bigint_by_schema<T: Type + Serialize + 'static>(output: &T) -> Value {
    let mut json = match serde_json::to_value(output) {
        Ok(v) => v,
        Err(_) => return Value::Null,
    };

    let paths = bigint_paths::<T>(Phase::Serialize);

    for path in paths.iter() {
        to_string_at(&mut json, path, 0);
    }

    let meta: Vec<MetaItem> = paths
        .iter()
        .map(|p| (BIGINT_TYPE_ID, p.clone()))
        .collect();

    let mut envelope = serde_json::Map::new();
    envelope.insert("json".to_string(), json);
    envelope.insert(
        "meta".to_string(),
        serde_json::Value::Array(
            meta.into_iter()
                .map(|(id, segs)| {
                    let mut item = Vec::new();
                    item.push(serde_json::Value::Number(id.into()));
                    for seg in segs {
                        item.push(match seg {
                            Segment::Field(s) => serde_json::Value::String(s),
                            Segment::Index(i) => serde_json::Value::Number(i.into()),
                            Segment::AnyElem | Segment::AnyKey => {
                                serde_json::Value::String("*".to_string())
                            }
                        });
                    }
                    serde_json::Value::Array(item)
                })
                .collect(),
        ),
    );
    serde_json::Value::Object(envelope)
}

/// One step in a value path. `Field`/`Index` are exact; `AnyElem`/`AnyKey`
/// fan out across all elements / map values respectively.
#[derive(Debug, Clone)]
pub(crate) enum Segment {
    Field(String),
    Index(usize),
    AnyElem,
    AnyKey,
}

/// Walk the serde-resolved type graph and record a path for every BigInt leaf.
///
/// `generics` carries the type arguments in scope at this use site (e.g.
/// `T = u64` for `Wrapper<u64>`), so a generic parameter inside a definition
/// can be resolved to the concrete argument it stands for.
fn collect_bigint_paths(
    dt: &DataType,
    cur: &mut Vec<Segment>,
    out: &mut Vec<Vec<Segment>>,
    types: &Types,
    phase: Phase,
    generics: &[(Generic, DataType)],
    depth: usize,
) {
    // Bound recursion for pathological self-referential types.
    if depth > 32 {
        return;
    }

    match dt {
        DataType::Primitive(p) => {
            if is_bigint(p) {
                out.push(cur.clone());
            }
        }
        // A generic parameter standing in for a concrete argument, e.g. the
        // `T` inside `Wrapper<u64>`. Without substitution the BigInt in the
        // argument is invisible — the definition only ever mentions `T`.
        DataType::Generic(g) => {
            if let Some((_, substituted)) = generics.iter().find(|(param, _)| param == g) {
                collect_bigint_paths(substituted, cur, out, types, phase, generics, depth + 1);
            }
        }
        DataType::Struct(s) => match &s.fields {
            Fields::Named(NamedFields { fields, .. }) => {
                for (name, field) in fields {
                    if let Some(ty) = &field.ty {
                        cur.push(Segment::Field(name.to_string()));
                        collect_bigint_paths(ty, cur, out, types, phase, generics, depth + 1);
                        cur.pop();
                    }
                }
            }
            Fields::Unnamed(UnnamedFields { fields, .. }) => {
                // serde serialises a single-field tuple struct transparently:
                // `struct UserId(u64)` is a bare number on the wire, not `[n]`.
                // Only a genuine 2+ tuple becomes a JSON array.
                if fields.len() == 1 {
                    if let Some(ty) = fields[0].ty.as_ref() {
                        collect_bigint_paths(ty, cur, out, types, phase, generics, depth + 1);
                    }
                } else {
                    for (idx, field) in fields.iter().enumerate() {
                        if let Some(ty) = &field.ty {
                            cur.push(Segment::Index(idx));
                            collect_bigint_paths(ty, cur, out, types, phase, generics, depth + 1);
                            cur.pop();
                        }
                    }
                }
            }
            Fields::Unit => {}
        },
        DataType::List(l) => {
            cur.push(Segment::AnyElem);
            collect_bigint_paths(list_ty(l), cur, out, types, phase, generics, depth + 1);
            cur.pop();
        }
        DataType::Map(m) => {
            // Map keys are almost never bigint; convert every value.
            cur.push(Segment::AnyKey);
            collect_bigint_paths(m.value_ty(), cur, out, types, phase, generics, depth + 1);
            cur.pop();
        }
        DataType::Tuple(t) => {
            for (idx, elem) in t.elements.iter().enumerate() {
                cur.push(Segment::Index(idx));
                collect_bigint_paths(elem, cur, out, types, phase, generics, depth + 1);
                cur.pop();
            }
        }
        DataType::Nullable(inner) => {
            collect_bigint_paths(inner, cur, out, types, phase, generics, depth + 1);
        }
        DataType::Reference(r) => match r {
            Reference::Named(named) => {
                // `PhasesFormat` may have split this type into `*_Serialize` /
                // `*_Deserialize` variants; select ours before descending.
                let selected = select_phase_datatype(
                    &DataType::Reference(Reference::Named(named.clone())),
                    types,
                    phase,
                );
                let DataType::Reference(Reference::Named(named)) = selected else {
                    return;
                };

                // Type arguments instantiated at this use site.
                let args: Vec<(Generic, DataType)> = match &named.inner {
                    NamedReferenceType::Reference { generics, .. } => generics.clone(),
                    _ => Vec::new(),
                };

                match &named.inner {
                    NamedReferenceType::Inline { dt: inline, .. } => {
                        collect_bigint_paths(inline, cur, out, types, phase, &args, depth + 1);
                    }
                    _ => {
                        if let Some(ty) = types.get(&named).and_then(|ndt| ndt.ty.as_ref()) {
                            collect_bigint_paths(ty, cur, out, types, phase, &args, depth + 1);
                        }
                    }
                }
            }
            Reference::Opaque(_) => {}
        },
        DataType::Enum(e) => {
            collect_enum_paths(e, cur, out, types, phase, generics, depth + 1);
        }
        DataType::Intersection(parts) => {
            for part in parts {
                collect_bigint_paths(part, cur, out, types, phase, generics, depth + 1);
            }
        }
    }
}

/// Walk every variant of an enum.
///
/// After `PhasesFormat`, serde's enum representation has already been lowered
/// into the shape: an externally tagged variant becomes a named field keyed by
/// the variant name (`Small(u64)` → `{ Small: u64 }`), so the generic
/// named-field rule below produces the right path without special-casing
/// tagging. Untagged variants keep their payload inline, which is also correct.
fn collect_enum_paths(
    e: &Enum,
    cur: &mut Vec<Segment>,
    out: &mut Vec<Vec<Segment>>,
    types: &Types,
    phase: Phase,
    generics: &[(Generic, DataType)],
    depth: usize,
) {
    for (_name, variant) in &e.variants {
        match &variant.fields {
            Fields::Named(NamedFields { fields, .. }) => {
                for (fname, field) in fields {
                    if let Some(ty) = &field.ty {
                        cur.push(Segment::Field(fname.to_string()));
                        collect_bigint_paths(ty, cur, out, types, phase, generics, depth + 1);
                        cur.pop();
                    }
                }
            }
            Fields::Unnamed(UnnamedFields { fields, .. }) => {
                // Same transparency rule as struct newtypes: a single-field
                // variant carries its payload directly, not inside an array.
                if fields.len() == 1 {
                    if let Some(ty) = fields[0].ty.as_ref() {
                        collect_bigint_paths(ty, cur, out, types, phase, generics, depth + 1);
                    }
                } else {
                    for (idx, field) in fields.iter().enumerate() {
                        if let Some(ty) = &field.ty {
                            cur.push(Segment::Index(idx));
                            collect_bigint_paths(ty, cur, out, types, phase, generics, depth + 1);
                            cur.pop();
                        }
                    }
                }
            }
            Fields::Unit => {}
        }
    }
}

fn list_ty(l: &List) -> &DataType {
    &l.ty
}

fn is_bigint(p: &Primitive) -> bool {
    matches!(
        p,
        Primitive::i64
            | Primitive::u64
            | Primitive::i128
            | Primitive::u128
            | Primitive::isize
            | Primitive::usize
    )
}

/// Recursively apply the conversion described by `path` to `value`.
fn apply_at(value: &mut Value, path: &[Segment], i: usize) {
    if i >= path.len() {
        convert_string_to_number(value);
        return;
    }
    match &path[i] {
        Segment::Field(key) => {
            if let Value::Object(map) = value {
                if let Some(child) = map.get_mut(key) {
                    apply_at(child, path, i + 1);
                }
            }
        }
        Segment::Index(idx) => {
            if let Value::Array(arr) = value {
                if let Some(child) = arr.get_mut(*idx) {
                    apply_at(child, path, i + 1);
                }
            }
        }
        Segment::AnyElem => {
            if let Value::Array(arr) = value {
                for child in arr.iter_mut() {
                    apply_at(child, path, i + 1);
                }
            }
        }
        Segment::AnyKey => {
            if let Value::Object(map) = value {
                for child in map.values_mut() {
                    apply_at(child, path, i + 1);
                }
            }
        }
    }
}

fn convert_string_to_number(v: &mut Value) {
    let Value::String(s) = v else {
        return;
    };
    *v = match parse_bigint_string(s) {
        Some(n) => n,
        None => return,
    };
}

fn convert_number_to_string(v: &mut Value) {
    if let Value::Number(n) = v {
        // Serialize the number to its full textual form, preserving u64/i128
        // magnitude (serde_json `arbitrary_precision` keeps it exact, not
        // truncated to f64).
        let s = n.to_string();
        *v = Value::String(s);
    }
}

/// Recursively convert the BigInt leaf described by `path` into a JSON string,
/// symmetric to [`apply_at`] but in the opposite direction.
fn to_string_at(value: &mut Value, path: &[Segment], i: usize) {
    if i >= path.len() {
        convert_number_to_string(value);
        return;
    }
    match &path[i] {
        Segment::Field(key) => {
            if let Value::Object(map) = value {
                if let Some(child) = map.get_mut(key) {
                    to_string_at(child, path, i + 1);
                }
            }
        }
        Segment::Index(idx) => {
            if let Value::Array(arr) = value {
                if let Some(child) = arr.get_mut(*idx) {
                    to_string_at(child, path, i + 1);
                }
            }
        }
        Segment::AnyElem => {
            if let Value::Array(arr) = value {
                for child in arr.iter_mut() {
                    to_string_at(child, path, i + 1);
                }
            }
        }
        Segment::AnyKey => {
            if let Value::Object(map) = value {
                for child in map.values_mut() {
                    to_string_at(child, path, i + 1);
                }
            }
        }
    }
}

/// Parse a bigint wire string into the most precise JSON number representation.
fn parse_bigint_string(s: &str) -> Option<Value> {
    if let Ok(n) = s.parse::<u64>() {
        return Some(Value::Number(n.into()));
    }
    if let Ok(n) = s.parse::<i64>() {
        return Some(Value::Number(n.into()));
    }
    // i128/u128 require serde_json's `arbitrary_precision` feature.
    if let Ok(n) = s.parse::<u128>() {
        return Some(Value::Number(n.into()));
    }
    if let Ok(n) = s.parse::<i128>() {
        return Some(Value::Number(n.into()));
    }
    if let Ok(n) = s.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(n) {
            return Some(Value::Number(num));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use specta::Type;

    #[derive(Type, serde::Deserialize, serde::Serialize)]
    struct Sample {
        id: u64,
        name: String,
        nested: Inner,
        list: Vec<i64>,
        opt: Option<u128>,
        map: std::collections::HashMap<String, usize>,
    }

    #[derive(Type, serde::Deserialize, serde::Serialize)]
    struct Inner {
        count: i64,
    }

    #[test]
    fn plain_json_with_string_bigint_is_decoded_by_schema() {
        // Client sends plain JSON (no meta) with bigint fields as strings.
        let input = json!({
            "id": "18446744073709551615",
            "name": "hello",
            "nested": { "count": "9007199254740993" },
            "list": ["1", "2", "3"],
            "opt": "123",
            "map": { "a": "5", "b": "6" }
        });
        let out = decode_bigint_by_schema::<Sample>(input);
        assert_eq!(out["id"], json!(18446744073709551615u64));
        assert_eq!(out["nested"]["count"], json!(9007199254740993i64));
        assert_eq!(out["list"], json!([1, 2, 3]));
        assert_eq!(out["opt"], json!(123u128));
        assert_eq!(out["map"]["a"], json!(5));
        assert_eq!(out["name"], json!("hello"));
    }

    #[test]
    fn request_is_plain_json_no_envelope() {
        // The TS client sends plain JSON via `toRustJson` (bigint fields already
        // string-encoded), never a `{ json, meta }` envelope. Schema decoding
        // must reconstruct bigint directly from that bare JSON — there is no
        // envelope to unwrap on the request side.
        let input = json!({
            "id": "42",
            "name": "x",
            "nested": { "count": "7" },
            "list": ["8"],
            "opt": "9",
            "map": { "k": "10" }
        });
        let out = decode_bigint_by_schema::<Sample>(input);
        assert_eq!(out["id"], json!(42));
        assert_eq!(out["nested"]["count"], json!(7));
        assert_eq!(out["list"], json!([8]));
        assert_eq!(out["map"]["k"], json!(10));
    }

    #[test]
    fn already_numeric_bigint_passes_through() {
        // A client that already narrowed to a number (precision-losing path):
        // schema decoding only touches strings, so the number is left as-is.
        let input = json!({ "id": 42, "name": "x", "nested": { "count": 7 }, "list": [8], "opt": 9, "map": { "k": 10 } });
        let out = decode_bigint_by_schema::<Sample>(input);
        assert_eq!(out["id"], json!(42));
    }

    #[test]
    fn non_envelope_passthrough_on_null() {
        assert_eq!(decode_bigint_by_schema::<Sample>(Value::Null), Value::Null);
    }

    // ── Encode (response side) ──

    #[derive(Type, serde::Serialize)]
    struct BigOut {
        id: u64,
        big: i128,
        list: Vec<u64>,
    }

    #[test]
    fn plain_value_with_bigint_is_encoded_to_envelope() {
        let out = BigOut {
            id: 18446744073709551615u64,
            big: 170141183460469231731687303715884105727i128,
            list: vec![1, 18446744073709551615],
        };
        let encoded = encode_bigint_by_schema(&out);
        assert_eq!(encoded["json"]["id"], json!("18446744073709551615"));
        assert_eq!(
            encoded["json"]["big"],
            json!("170141183460469231731687303715884105727")
        );
        assert_eq!(
            encoded["json"]["list"],
            json!(["1", "18446744073709551615"])
        );
        // meta marks three bigint leaves (id, big, list.*).
        assert!(encoded["meta"].is_array());
        let meta = encoded["meta"].as_array().unwrap();
        assert_eq!(meta.len(), 3);
        // Each meta item is [0, ...path].
        assert_eq!(meta[0], json!([0, "id"]));
        assert_eq!(meta[1], json!([0, "big"]));
        assert_eq!(meta[2], json!([0, "list", "*"]));
    }

    #[test]
    fn no_bigint_still_wrapped_as_envelope() {
        #[derive(Type, serde::Serialize)]
        struct Plain {
            name: String,
            count: i32,
        }
        let out = Plain {
            name: "x".to_string(),
            count: 1,
        };
        // Even without bigint, the response is always the fixed `{ json, meta }`
        // envelope (with `meta: []`) — never bare JSON.
        let encoded = encode_bigint_by_schema(&out);
        assert_eq!(
            encoded,
            json!({ "json": { "name": "x", "count": 1 }, "meta": [] })
        );
    }

    #[test]
    fn envelope_roundtrip_with_decode() {
        let out = BigOut {
            id: 18446744073709551615u64,
            big: 170141183460469231731687303715884105727i128,
            list: vec![1, 2, 18446744073709551615],
        };
        let encoded = encode_bigint_by_schema(&out);
        // `encode` emits a `{ json, meta }` response envelope. The bare JSON
        // payload is what would cross the wire back as a request (the client
        // re-sends plain JSON), and `decode` reconstructs bigint from it by
        // schema alone — no envelope unwrapping on the request side.
        let json = encoded["json"].clone();
        let decoded = decode_bigint_by_schema::<BigOut>(json);
        assert_eq!(decoded["id"], json!(18446744073709551615u64));
        assert_eq!(
            decoded["big"],
            json!(170141183460469231731687303715884105727i128)
        );
        assert_eq!(decoded["list"], json!([1, 2, 18446744073709551615u64]));
    }
}
