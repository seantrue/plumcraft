//! Errors (spec section 12): node errors are data in records and replies ([`NodeError`]);
//! protocol errors answer a request ([`ProtoError`], JSON-RPC codes).

use serde::{Deserialize, Serialize};

use crate::de::null_as_default;

/// A node's or a run's error, as data. `kind` is one of [`NodeError::KINDS`]; an unknown kind is
/// kept as text and shown as an operation error.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct NodeError {
    #[serde(deserialize_with = "null_as_default")]
    pub kind: String,
    #[serde(deserialize_with = "null_as_default")]
    pub message: String,
    /// The exception type, when there was one.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
}

impl NodeError {
    pub const CONFIG: &'static str = "config";
    pub const OP: &'static str = "op";
    pub const SKIPPED: &'static str = "skipped";
    pub const MISSING_SOURCE: &'static str = "missing_source";
    pub const GRAPH: &'static str = "graph";
    pub const ENGINE: &'static str = "engine";
    pub const CANCELLED: &'static str = "cancelled";
    pub const KINDS: &'static [&'static str] = &[Self::CONFIG, Self::OP, Self::SKIPPED, Self::MISSING_SOURCE, Self::GRAPH, Self::ENGINE, Self::CANCELLED];

    pub fn new(kind: &str, message: impl Into<String>) -> Self {
        NodeError { kind: kind.into(), message: message.into(), ty: None }
    }

    /// The host's synthesised failure when an engine can't answer (spec 12.3).
    pub fn engine(message: impl Into<String>) -> Self {
        Self::new(Self::ENGINE, message)
    }

    pub fn cancelled() -> Self {
        Self::new(Self::CANCELLED, "cancelled")
    }
}

/// A protocol error code (spec 12.2), with its JSON-RPC number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    Parse,
    InvalidRequest,
    UnknownOp,
    BadParams,
    Internal,
    Version,
    Busy,
    Cancelled,
    NotFound,
    TooLarge,
    Unauthorized,
    Unavailable,
    Integrity,
    Unsupported,
    /// A code this crate doesn't know.
    Other(i64),
}

impl ErrorCode {
    const TABLE: &'static [(ErrorCode, i64, &'static str, u16)] = &[
        (ErrorCode::Parse, -32700, "parse", 400),
        (ErrorCode::InvalidRequest, -32600, "invalid_request", 400),
        (ErrorCode::UnknownOp, -32601, "unknown_op", 404),
        (ErrorCode::BadParams, -32602, "bad_params", 422),
        (ErrorCode::Internal, -32603, "internal", 500),
        (ErrorCode::Version, -32000, "version", 409),
        (ErrorCode::Busy, -32001, "busy", 503),
        (ErrorCode::Cancelled, -32002, "cancelled", 409),
        (ErrorCode::NotFound, -32003, "not_found", 404),
        (ErrorCode::TooLarge, -32004, "too_large", 413),
        (ErrorCode::Unauthorized, -32005, "unauthorized", 401),
        (ErrorCode::Unavailable, -32006, "unavailable", 503),
        (ErrorCode::Integrity, -32007, "integrity", 422),
        (ErrorCode::Unsupported, -32008, "unsupported", 501),
    ];

    pub fn code(self) -> i64 {
        match self {
            ErrorCode::Other(n) => n,
            known => Self::TABLE.iter().find(|(c, ..)| *c == known).map_or(-32603, |(_, n, ..)| *n),
        }
    }

    pub fn from_code(n: i64) -> Self {
        Self::TABLE.iter().find(|(_, c, ..)| *c == n).map_or(ErrorCode::Other(n), |(e, ..)| *e)
    }

    /// The name used in AMQP `pk-error` headers and logs.
    pub fn name(self) -> &'static str {
        Self::TABLE.iter().find(|(c, ..)| *c == self).map_or("other", |(_, _, s, _)| s)
    }

    /// The HTTP status for the HTTP binding.
    pub fn http(self) -> u16 {
        Self::TABLE.iter().find(|(c, ..)| *c == self).map_or(500, |(.., h)| *h)
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_i64(self.code())
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Ok(ErrorCode::from_code(i64::deserialize(de)?))
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for ErrorCode {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ErrorCode".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "integer", "description": "A JSON-RPC error code (spec 12.2)"})
    }
}

/// `{kind, hint?}` attached to a protocol error.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct ErrorData {
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// A request that failed as a request: `{code, message, data?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[error("{} ({}): {message}", code.name(), code.code())]
pub struct ProtoError {
    pub code: ErrorCode,
    #[serde(default, deserialize_with = "null_as_default")]
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<ErrorData>,
}

impl ProtoError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        ProtoError { code, message: message.into(), data: None }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.data.get_or_insert_with(ErrorData::default).hint = Some(hint.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for (c, n, ..) in ErrorCode::TABLE {
            assert_eq!(c.code(), *n);
            assert_eq!(ErrorCode::from_code(*n), *c);
        }
        assert_eq!(ErrorCode::from_code(-1), ErrorCode::Other(-1));
        assert_eq!(ErrorCode::Other(-1).code(), -1);
        let e = ProtoError::new(ErrorCode::Busy, "running").with_hint("retry in 1 s");
        let back: ProtoError = serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
        assert_eq!(back, e);
        assert_eq!(serde_json::to_value(&e).unwrap()["code"], -32001);
        assert_eq!(e.code.http(), 503);
    }
}
