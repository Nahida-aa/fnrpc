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

/// One step in a value path. `Field`/`Index` are exact; `AnyElem`/`AnyKey`
/// fan out across all elements / map values respectively.
#[derive(Debug, Clone)]
enum Segment {
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
}
