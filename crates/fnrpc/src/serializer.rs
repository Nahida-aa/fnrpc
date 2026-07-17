//! Server-side BigInt envelope unwrapping.
//!
//! On the client side, `BigInt` values are serialised as strings with
//! an attached metadata envelope (`{ json, meta }`). This module
//! reverses that transformation so Rust handlers receive the original
//! numeric values.
//!
//! See also the TS [`serialize`](https://docs.rs/fnrpc-client/latest/fnrpc_client/fn.serialize.html) function.

use serde_json::Value;

/// BIGINT type ID — must match `BIGINT` constant in TS serializer.
const BIGINT_TYPE_ID: u64 = 0;

/// If `input` is a `{ json, meta }` envelope from the client-side serializer,
/// unwrap it by applying all meta transformations and return the plain `json`.
///
/// If `input` is a plain value (no envelope), return it unchanged.
///
/// Takes ownership of `input` to avoid an unconditional clone on the common path.
pub fn unpack_meta(input: Value) -> Value {
    let Value::Object(obj) = input else {
        // Plain value (not an envelope) — no clone needed since we own it
        return input;
    };

    let json = match obj.get("json") {
        Some(json) => json,
        None => {
            // Object without "json" field — return as-is
            return Value::Object(obj);
        }
    };

    let meta = match obj.get("meta") {
        Some(Value::Array(meta)) => meta,
        _ => {
            // Has "json" but no valid "meta" — return the json value
            return json.clone();
        }
    };

    let mut result = json.clone();

    for item in meta {
        let parts = match item {
            Value::Array(parts) if !parts.is_empty() => parts,
            _ => continue,
        };

        let type_id = match parts[0].as_u64() {
            Some(id) => id,
            None => continue,
        };

        if parts.len() < 2 {
            // Root-level fix (no path segments, e.g. top-level BigInt)
            apply_root_fix(&mut result, type_id);
        } else {
            let segments: Vec<&Value> = parts[1..].iter().collect();
            apply_meta_fix(&mut result, &segments, type_id);
        }
    }

    result
}

fn apply_root_fix(root: &mut Value, type_id: u64) {
    match type_id {
        BIGINT_TYPE_ID => {
            let s = match root.as_str() {
                Some(s) => s,
                None => return,
            };
            if let Ok(n) = s.parse::<u64>() {
                *root = Value::Number(n.into());
            } else if let Ok(n) = s.parse::<i64>() {
                *root = Value::Number(n.into());
            } else if let Ok(n) = s.parse::<f64>() {
                if let Some(num) = serde_json::Number::from_f64(n) {
                    *root = Value::Number(num);
                }
            }
        }
        _ => {}
    }
}

fn apply_meta_fix(root: &mut Value, segments: &[&Value], type_id: u64) {
    let mut current = root;

    // traverse to parent of the target value
    for i in 0..segments.len().saturating_sub(1) {
        let seg = segments[i];
        current = match (current, seg) {
            (Value::Object(map), Value::String(key)) => match map.get_mut(key.as_str()) {
                Some(v) => v,
                None => return,
            },
            (Value::Array(arr), Value::Number(idx)) => {
                let i = idx.as_u64().unwrap_or(u64::MAX) as usize;
                match arr.get_mut(i) {
                    Some(v) => v,
                    None => return,
                }
            }
            _ => return,
        };
    }

    let last_seg = match segments.last() {
        Some(s) => s,
        None => return,
    };

    let target = match (current, last_seg) {
        (Value::Object(map), Value::String(key)) => map.get_mut(key.as_str()),
        (Value::Array(arr), Value::Number(idx)) => {
            let i = idx.as_u64().unwrap_or(u64::MAX) as usize;
            arr.get_mut(i)
        }
        _ => None,
    };

    let target = match target {
        Some(t) => t,
        None => return,
    };

    match type_id {
        BIGINT_TYPE_ID => {
            // Convert BIGINT string back to JSON number preserving integer nature.
            let s = match target.as_str() {
                Some(s) => s,
                None => return,
            };
            if let Ok(n) = s.parse::<u64>() {
                *target = Value::Number(n.into());
            } else if let Ok(n) = s.parse::<i64>() {
                *target = Value::Number(n.into());
            } else if let Ok(n) = s.parse::<f64>() {
                if let Some(num) = serde_json::Number::from_f64(n) {
                    *target = Value::Number(num);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_plain_passthrough() {
        let input = json!({ "name": "hello" });
        let expected = input.clone();
        assert_eq!(unpack_meta(input), expected);
    }

    #[test]
    fn test_null_input() {
        assert_eq!(unpack_meta(Value::Null), Value::Null);
    }

    #[test]
    fn test_number_input() {
        let input = json!(42);
        let expected = input.clone();
        assert_eq!(unpack_meta(input), expected);
    }

    #[test]
    fn test_unpack_bigint() {
        // Simulate: serialize({ value: BigInt(500) }) → { json: { value: "500" }, meta: [[0, "value"]] }
        let input = json!({
            "json": { "value": "500" },
            "meta": [[0, "value"]]
        });
        let result = unpack_meta(input);
        assert_eq!(result, json!({ "value": 500 }));
    }

    #[test]
    fn test_unpack_bigint_nested() {
        let input = json!({
            "json": { "a": { "b": "42" } },
            "meta": [[0, "a", "b"]]
        });
        let result = unpack_meta(input);
        assert_eq!(result, json!({ "a": { "b": 42 } }));
    }

    #[test]
    fn test_unpack_bigint_root() {
        // Simulate: serialize(BigInt(500)) → { json: "500", meta: [[0]] }
        let input = json!({
            "json": "500",
            "meta": [[0]]
        });
        let result = unpack_meta(input);
        assert_eq!(result, json!(500));
    }

    #[test]
    fn test_no_envelope() {
        let input = json!({ "value": "500" });
        let expected = input.clone();
        assert_eq!(unpack_meta(input), expected);
    }

    #[test]
    fn test_no_meta() {
        let input = json!({ "json": { "value": "500" } });
        // No meta → return json as-is
        let result = unpack_meta(input);
        assert_eq!(result, json!({ "value": "500" }));
    }
}
