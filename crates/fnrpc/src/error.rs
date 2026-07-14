use std::fmt;

#[derive(Debug)]
pub struct RpcErr(pub String);

impl fmt::Display for RpcErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for RpcErr {}

impl From<String> for RpcErr {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for RpcErr {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}
