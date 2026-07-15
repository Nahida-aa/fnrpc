use std::fmt;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct RpcErr {
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcErr {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("INTERNAL_SERVER_ERROR", message)
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new("BAD_REQUEST", message)
    }

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
