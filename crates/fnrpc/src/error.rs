use std::borrow::Cow;
use std::fmt;

use serde::Serialize;
use serde_json::Value;
use specta::{
    Type, Types,
    datatype::{DataType, Field, NamedDataType, Primitive, Struct},
};

#[derive(Debug, Clone, Serialize)]
pub struct RpcErr {
    pub name: &'static str,
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}

impl Type for RpcErr {
    fn definition(types: &mut Types) -> DataType {
        DataType::Reference(NamedDataType::init_with_sentinel(
            "fnrpc::error::RpcErr",
            &[],
            false,
            false,
            types,
            |_types, ndt| {
                ndt.name = Cow::Borrowed("RpcErr");
                ndt.module_path = Cow::Borrowed("fnrpc::error");
                ndt.ty = Some(
                    Struct::named()
                        .field("name", Field::new(DataType::Reference(specta_typescript::define("\"RpcErr\""))))
                        .field("code", Field::new(DataType::Primitive(Primitive::str)))
                        .field(
                            "message",
                            Field::new(DataType::Primitive(Primitive::str)),
                        )
                        .field(
                            "data",
                            Field::new(DataType::Nullable(Box::new(
                                DataType::Reference(specta_typescript::define("unknown")),
                            ))),
                        )
                        .build(),
                );
            },
            |_types| {
                Struct::named()
                    .field("name", Field::new(DataType::Primitive(Primitive::str)))
                    .field("code", Field::new(DataType::Primitive(Primitive::str)))
                    .field(
                        "message",
                        Field::new(DataType::Primitive(Primitive::str)),
                    )
                    .field(
                        "data",
                        Field::new(DataType::Nullable(Box::new(
                            DataType::Reference(specta_typescript::define("unknown")),
                        ))),
                    )
                    .build()
            },
        ))
    }
}

impl RpcErr {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: "RpcErr",
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
