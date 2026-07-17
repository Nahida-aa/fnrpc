//! Pluggable serialization codecs for RPC protocols.
//!
//! The [`RpcCodec`] trait abstracts how handler Input/Output types are
//! serialized to/from raw bytes. Built-in implementations:
//!
//! - [`JsonCodec`] — JSON via serde_json (default)
//!
//! Custom codecs (protobuf, msgpack, cbor) can be added by implementing
//! [`RpcCodec`] for your own types.

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::RpcErr;

/// A serialization codec for RPC protocols.
///
/// Implementations define how handler Input/Output types are
/// serialized and deserialized to/from raw bytes.
pub trait RpcCodec: Send + Sync + 'static {
    /// Encode a serializable value to bytes.
    fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, RpcErr>;

    /// Decode a value from bytes.
    fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, RpcErr>;

    /// Content-Type header value for this codec.
    fn content_type() -> &'static str;
}

// ── Built-in: JSON codec ──────────────────────────────────

/// JSON codec using serde_json (default).
pub struct JsonCodec;

impl RpcCodec for JsonCodec {
    fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, RpcErr> {
        serde_json::to_vec(value).map_err(|e| RpcErr::internal(format!("serialize: {e}")))
    }

    fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, RpcErr> {
        serde_json::from_slice(bytes).map_err(|e| RpcErr::bad_request(format!("deserialize: {e}")))
    }

    fn content_type() -> &'static str {
        "application/json"
    }
}

// ── Built-in: Identity codec (passthrough) ─────────────────

/// Identity codec — passes bytes through unchanged.
///
/// Used by [`RawRpcFn`](crate::handler::RawRpcFn) handlers that operate
/// directly on byte buffers. Not intended for general use with `RpcFn`.
pub struct NoCodec;

impl RpcCodec for NoCodec {
    fn encode<T: Serialize>(_value: &T) -> Result<Vec<u8>, RpcErr> {
        panic!("NoCodec::encode requires RawRpcFn dispatch — use call_value instead");
    }

    fn decode<T: DeserializeOwned>(_bytes: &[u8]) -> Result<T, RpcErr> {
        panic!("NoCodec::decode requires RawRpcFn dispatch — use call_value instead");
    }

    fn content_type() -> &'static str {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_codec_roundtrip() {
        let val = "hello".to_string();
        let bytes = JsonCodec::encode(&val).unwrap();
        let decoded: String = JsonCodec::decode(&bytes).unwrap();
        assert_eq!(decoded, val);
    }

    #[test]
    fn test_json_codec_struct() {
        use serde::Deserialize;

        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Foo {
            x: i32,
            y: String,
        }

        let val = Foo { x: 42, y: "hi".into() };
        let bytes = JsonCodec::encode(&val).unwrap();
        let decoded: Foo = JsonCodec::decode(&bytes).unwrap();
        assert_eq!(decoded, val);
    }

    #[test]
    fn test_json_codec_content_type() {
        assert_eq!(JsonCodec::content_type(), "application/json");
    }

    #[test]
    fn test_json_codec_decode_error() {
        let err = JsonCodec::decode::<i32>(b"not-json").unwrap_err();
        assert_eq!(err.code, "BAD_REQUEST");
    }
}
