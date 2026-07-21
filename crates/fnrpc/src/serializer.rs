//! Server-side BigInt decoding driven by the handler's own schema.
//!
//! A `BigInt`-style Rust integer (`u64`/`i64`/`usize`/...) cannot be carried
//! through JSON as a number without losing precision, so on the wire it is
//! represented as a string. The server knows the field's true type from its own
//! [`specta::Type`] schema ([`crate::handler::RpcFn::Input`]), so it converts
//! those string leaves back into JSON numbers *itself* — it does **not** trust
//! a client-supplied `meta` envelope to tell it which fields are bigint.
//!
//! Clients may send either:
//! - a `{ json, meta }` envelope (the `meta` is ignored here), or
//! - a plain JSON value with bigint fields already encoded as strings,
//!   and this decoder handles both identically.
//!
//! The client-side analogue of the envelope codec lives in the `fnrpc-client`
//! crate ([`fnrpc_client::unpack_meta`]).

use serde::Serialize;
use serde_json::Value;
use specta::datatype::{
    DataType, Enum, Fields, List, NamedFields, Primitive, Reference, UnnamedFields,
};
use specta::{Type, Types};

/// Convert a bigint-typed JSON value from its wire (string) form into a JSON
/// number, using the server's own schema rather than a client `meta` envelope.
///
/// - If `input` is a `{ json, meta }` envelope, only the `json` part is used
///   (the `meta` is ignored).
/// - If `input` is a plain JSON value, it is decoded as-is.
/// - Only fields whose type in `T`'s schema is a BigInt-style integer are
///   touched; any such field that arrives as a string is converted to a number.
///   Fields that already arrive as numbers (e.g. from a client that already
///   narrowed them) are left untouched.
///
/// The BIGINT type ID (`0`) is defined in the TS serializer and in
/// `fnrpc-client` (`fnrpc_client::unpack_meta`); this decoder does not need it
/// because it is driven entirely by the schema, not by `meta`.
pub fn decode_bigint_by_schema<T: Type>(input: Value) -> Value {
    // Determine the JSON payload: an envelope's `json` field, or the value
    // itself when the client sent plain JSON (no envelope).
    let payload = match &input {
        Value::Object(obj) if obj.contains_key("json") => obj["json"].clone(),
        _ => input,
    };

    let mut types = Types::default();
    let dt = T::definition(&mut types);

    let mut paths: Vec<Vec<Segment>> = Vec::new();
    collect_bigint_paths(&dt, &mut Vec::new(), &mut paths, &types, 0);

    let mut payload = payload;
    for path in &paths {
        apply_at(&mut payload, path, 0);
    }
    payload
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
/// - If the output contains no BigInt-style integer leaves, the bare JSON
///   value is returned (no envelope) — fully backward compatible.
/// - Otherwise the BigInt leaves are converted to strings and a `meta` array
///   records their paths, producing `{ "json": <json>, "meta": [...] }`.
///
/// The client reconstructs `BigInt` from the `meta` paths; no client-side
/// schema or negotiation is needed.
pub fn encode_bigint_by_schema<T: Type + Serialize>(output: &T) -> Value {
    let mut json = match serde_json::to_value(output) {
        Ok(v) => v,
        Err(_) => return Value::Null,
    };

    let mut types = Types::default();
    let dt = T::definition(&mut types);

    let mut paths: Vec<Vec<Segment>> = Vec::new();
    collect_bigint_paths(&dt, &mut Vec::new(), &mut paths, &types, 0);

    if paths.is_empty() {
        return json;
    }

    for path in &paths {
        to_string_at(&mut json, path, 0);
    }

    let meta: Vec<MetaItem> = paths.into_iter().map(|p| (BIGINT_TYPE_ID, p)).collect();

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

fn collect_bigint_paths(
    dt: &DataType,
    cur: &mut Vec<Segment>,
    out: &mut Vec<Vec<Segment>>,
    types: &Types,
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
        DataType::Struct(s) => match &s.fields {
            Fields::Named(NamedFields { fields, .. }) => {
                for (name, field) in fields {
                    if let Some(ty) = &field.ty {
                        cur.push(Segment::Field(name.to_string()));
                        collect_bigint_paths(ty, cur, out, types, depth + 1);
                        cur.pop();
                    }
                }
            }
            Fields::Unnamed(UnnamedFields { fields, .. }) => {
                for (idx, field) in fields.iter().enumerate() {
                    if let Some(ty) = &field.ty {
                        cur.push(Segment::Index(idx));
                        collect_bigint_paths(ty, cur, out, types, depth + 1);
                        cur.pop();
                    }
                }
            }
            Fields::Unit => {}
        },
        DataType::List(l) => {
            cur.push(Segment::AnyElem);
            collect_bigint_paths(list_ty(l), cur, out, types, depth + 1);
            cur.pop();
        }
        DataType::Map(m) => {
            // Map keys are almost never bigint; convert every value.
            cur.push(Segment::AnyKey);
            collect_bigint_paths(m.value_ty(), cur, out, types, depth + 1);
            cur.pop();
        }
        DataType::Tuple(t) => {
            for (idx, elem) in t.elements.iter().enumerate() {
                cur.push(Segment::Index(idx));
                collect_bigint_paths(elem, cur, out, types, depth + 1);
                cur.pop();
            }
        }
        DataType::Nullable(inner) => {
            collect_bigint_paths(inner, cur, out, types, depth + 1);
        }
        DataType::Reference(r) => match r {
            Reference::Named(named) => match &named.inner {
                specta::datatype::NamedReferenceType::Inline { dt: inline, .. } => {
                    collect_bigint_paths(inline, cur, out, types, depth + 1);
                }
                _ => {
                    if let Some(ndt) = types.get(named) {
                        if let Some(ty) = &ndt.ty {
                            collect_bigint_paths(ty, cur, out, types, depth + 1);
                        }
                    }
                }
            },
            Reference::Opaque(_) => {}
        },
        DataType::Enum(e) => {
            collect_enum_paths(e, cur, out, types, depth + 1);
        }
        DataType::Intersection(parts) => {
            for part in parts {
                collect_bigint_paths(part, cur, out, types, depth + 1);
            }
        }
        DataType::Generic(_) => {}
    }
}

fn collect_enum_paths(
    e: &Enum,
    cur: &mut Vec<Segment>,
    out: &mut Vec<Vec<Segment>>,
    types: &Types,
    depth: usize,
) {
    for (_name, variant) in &e.variants {
        match &variant.fields {
            Fields::Named(NamedFields { fields, .. }) => {
                for (fname, field) in fields {
                    if let Some(ty) = &field.ty {
                        cur.push(Segment::Field(fname.to_string()));
                        collect_bigint_paths(ty, cur, out, types, depth + 1);
                        cur.pop();
                    }
                }
            }
            Fields::Unnamed(UnnamedFields { fields, .. }) => {
                for (idx, field) in fields.iter().enumerate() {
                    if let Some(ty) = &field.ty {
                        cur.push(Segment::Index(idx));
                        collect_bigint_paths(ty, cur, out, types, depth + 1);
                        cur.pop();
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
    fn envelope_json_meta_is_ignored() {
        // Even with a bogus/empty meta, schema decoding reconstructs bigint.
        let input = json!({
            "json": {
                "id": "42",
                "name": "x",
                "nested": { "count": "7" },
                "list": ["8"],
                "opt": "9",
                "map": { "k": "10" }
            },
            "meta": []
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
    fn no_bigint_passthrough_as_plain_json() {
        #[derive(Type, serde::Serialize)]
        struct Plain {
            name: String,
            count: i32,
        }
        let out = Plain {
            name: "x".to_string(),
            count: 1,
        };
        // A non-bigint value should pass through as bare JSON (no envelope).
        let encoded = encode_bigint_by_schema(&out);
        assert_eq!(encoded, json!({ "name": "x", "count": 1 }));
    }

    #[test]
    fn envelope_roundtrip_with_decode() {
        let out = BigOut {
            id: 18446744073709551615u64,
            big: 170141183460469231731687303715884105727i128,
            list: vec![1, 2, 18446744073709551615],
        };
        let encoded = encode_bigint_by_schema(&out);
        // Client stores the string form; server decodes it back by schema.
        let decoded = decode_bigint_by_schema::<BigOut>(encoded);
        assert_eq!(decoded["id"], json!(18446744073709551615u64));
        assert_eq!(
            decoded["big"],
            json!(170141183460469231731687303715884105727i128)
        );
        assert_eq!(decoded["list"], json!([1, 2, 18446744073709551615u64]));
    }
}
