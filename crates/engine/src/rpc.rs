//! JSON-RPC 2.0 framing for the local socket binding (spec 16.3): one JSON object per line, at
//! most `max_frame` bytes. Requests carry `id`; events are notifications with method `event`;
//! cancel may be a notification. Shared by the engine's socket front and the UI's process host.

use std::io::{BufRead, Write};

use plumcraft_proto::error::{ErrorCode, ProtoError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The default frame limit (spec 3, rule 1).
pub const MAX_FRAME: usize = 16 << 20;

/// A request or a notification (no `id`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    pub fn call(id: u64, method: &str, params: Value) -> Self {
        Request { jsonrpc: "2.0".into(), id: Some(id), method: method.into(), params }
    }

    pub fn notify(method: &str, params: Value) -> Self {
        Request { jsonrpc: "2.0".into(), id: None, method: method.into(), params }
    }
}

/// A reply: exactly one of `result` and `error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtoError>,
}

impl Response {
    pub fn new(id: Option<u64>, outcome: Result<Value, ProtoError>) -> Self {
        match outcome {
            Ok(v) => Response { jsonrpc: "2.0".into(), id, result: Some(v), error: None },
            Err(e) => Response { jsonrpc: "2.0".into(), id, result: None, error: Some(e) },
        }
    }

    pub fn into_result(self) -> Result<Value, ProtoError> {
        match (self.result, self.error) {
            (_, Some(e)) => Err(e),
            (Some(v), None) => Ok(v),
            (None, None) => Ok(Value::Null),
        }
    }
}

/// One incoming line, classified.
#[derive(Debug)]
pub enum Frame {
    Request(Request),
    Response(Response),
}

/// Parse a line as a request/notification (has `method`) or a response.
pub fn parse(line: &[u8]) -> Result<Frame, ProtoError> {
    let v: Value = serde_json::from_slice(line).map_err(|e| ProtoError::new(ErrorCode::Parse, e.to_string()))?;
    if v.get("method").is_some() {
        serde_json::from_value(v).map(Frame::Request).map_err(|e| ProtoError::new(ErrorCode::InvalidRequest, e.to_string()))
    } else {
        serde_json::from_value(v).map(Frame::Response).map_err(|e| ProtoError::new(ErrorCode::InvalidRequest, e.to_string()))
    }
}

/// Read one line of at most `max` bytes, without the newline. `Ok(None)` at end of stream; an
/// over-long line is an error and the caller drops the connection (spec 18, "Frames").
pub fn read_frame(r: &mut impl BufRead, max: usize) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let buf = r.fill_buf()?;
        if buf.is_empty() {
            return Ok(if line.is_empty() { None } else { Some(line) });
        }
        match buf.iter().position(|&b| b == b'\n') {
            Some(i) => {
                if line.len() + i > max {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
                }
                line.extend_from_slice(&buf[..i]);
                r.consume(i + 1);
                return Ok(Some(line));
            }
            None => {
                let n = buf.len();
                if line.len() + n > max {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
                }
                line.extend_from_slice(buf);
                r.consume(n);
            }
        }
    }
}

/// Write one value as a line.
pub fn write_frame(w: &mut impl Write, v: &impl Serialize) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(v).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    w.write_all(&bytes)?;
    w.flush()
}

/// Compare secrets in constant time (spec 18, "Tokens").
pub fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames() {
        let mut input: &[u8] = b"{\"a\":1}\n{\"b\":2}\npartial";
        assert_eq!(read_frame(&mut input, 100).unwrap().unwrap(), b"{\"a\":1}");
        assert_eq!(read_frame(&mut input, 100).unwrap().unwrap(), b"{\"b\":2}");
        assert_eq!(read_frame(&mut input, 100).unwrap().unwrap(), b"partial");
        assert!(read_frame(&mut input, 100).unwrap().is_none());
        let mut big: &[u8] = &[b'x'; 64];
        assert!(read_frame(&mut big, 10).is_err());
    }

    #[test]
    fn classify() {
        assert!(matches!(parse(br#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{}}"#), Ok(Frame::Request(r)) if r.id == Some(1)));
        assert!(matches!(parse(br#"{"jsonrpc":"2.0","method":"event","params":{}}"#), Ok(Frame::Request(r)) if r.id.is_none()));
        assert!(matches!(parse(br#"{"jsonrpc":"2.0","id":1,"result":{}}"#), Ok(Frame::Response(_))));
        assert!(parse(b"not json").is_err());
        assert!(same_token("abc", "abc") && !same_token("abc", "abd") && !same_token("abc", "ab"));
    }
}
