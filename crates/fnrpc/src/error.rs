//! Error type for fnrpc.
//!
//! [`RpcErr`] is the canonical error returned by all RPC handlers.
//! It serialises to JSON and is mirrored as [`RpcError`] on the TS side.

use std::fmt;

use serde::Serialize;
use serde_json::Value;
use specta::Type;

/// An RPC error returned by any handler (query, mutate, subscribe).
///
/// Maps to [`RpcError`](https://docs.rs/fnrpc-client/latest/fnrpc_client/class.RpcError.html)
/// in the TypeScript client.
///
/// # TS mirror
///
/// ```typescript
/// class RpcError extends Error {
///   name: "RpcErr";
///   code: string;
///   message: string;
///   data: unknown;
/// }
/// ```
#[derive(Debug, Clone, Serialize, Type)]
pub struct RpcErr {
    pub name: &'static str,
    pub code: String,
    pub message: String,
    #[specta(type = Option<specta_typescript::Unknown>)]
    pub data: Option<Value>,
}

impl RpcErr {
    /// Create an error with any code and message.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: "RpcErr",
            code: code.into(),
            message: message.into(),
            data: None,
        }
    }

    /// Attach arbitrary JSON data to this error.
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// Shorthand for `RpcErr::new("INTERNAL_SERVER_ERROR", message)`.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("INTERNAL_SERVER_ERROR", message)
    }

    /// Shorthand for `RpcErr::new("BAD_REQUEST", message)`.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new("BAD_REQUEST", message)
    }

    /// Shorthand for `RpcErr::new("NOT_FOUND", message)`.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("NOT_FOUND", message)
    }
}

impl fmt::Display for RpcErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RpcErr {}

impl From<String> for RpcErr {
    fn from(s: String) -> Self {
        Self::internal(s)
    }
}

impl From<&str> for RpcErr {
    fn from(s: &str) -> Self {
        Self::internal(s)
    }
}
