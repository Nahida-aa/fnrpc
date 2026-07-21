use std::borrow::Cow;

use http::{HeaderMap, StatusCode};

#[derive(Debug)]
pub struct RpcOutput {
    pub data: Cow<'static, [u8]>, // 载荷，中性命名，不叫 body
    pub http: Option<HttpInfo>,   // 仅 HTTP 需要的附加信息，tauri/SSE 留 None
}

#[derive(Debug, Default)]
pub struct HttpInfo {
    pub status: Option<StatusCode>, // None → 传输默认 200
    pub headers: Option<HeaderMap>, // None → 不加额外头
}

impl RpcOutput {
    /// A plain output with default HTTP status (200) and no extra headers.
    pub fn ok(data: impl Into<Cow<'static, [u8]>>) -> Self {
        RpcOutput {
            data: data.into(),
            http: None,
        }
    }

    /// Set an explicit HTTP status code (keeps any previously set headers).
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.http.get_or_insert_with(HttpInfo::default).status = Some(status);
        self
    }

    /// Set an explicit response header (keeps any previously set status).
    ///
    /// Takes the xitca/http style concrete types directly:
    ///
    /// ```ignore
    /// RpcOutput::ok(b"null")
    ///     .header(http::header::CONTENT_TYPE, http::HeaderValue::from_static("application/json"))
    /// ```
    pub fn header(mut self, key: http::HeaderName, value: http::HeaderValue) -> Self {
        let info = self.http.get_or_insert_with(HttpInfo::default);
        info.headers
            .get_or_insert_with(HeaderMap::new)
            .append(key, value);
        self
    }

    /// Convenience form accepting `&'static str` for both name and value
    /// (the common case).
    pub fn header_str(mut self, key: &'static str, value: &'static str) -> Self {
        self.header(
            http::HeaderName::from_static(key),
            http::HeaderValue::from_static(value),
        )
    }
}

impl HttpInfo {
    pub fn default() -> Self {
        HttpInfo {
            status: None,
            headers: None,
        }
    }
}
