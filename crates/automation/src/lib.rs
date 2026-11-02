//! The control channel: its transport ([`server`]: loopback TCP, token auth, JSON lines) and
//! the session-only methods ([`dispatch`]). See `docs/control-protocol.md`.
//!
//! ```json
//! {"id": 1, "method": "command.run", "params": {"command": "node.add", "params": {"kind": "text.upper"}}}
//! {"id": 1, "ok": true, "result": {"id": "n1"}}
//! ```
//!
//! The `ui.*` methods need the window and are handled in `plumcraft-ui`'s `control` module.
//!
//! Lifted from the research software's `crates/automation` (2026-10-27, the research software `8d96021`; plan 10.3, E1.4).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod server;

use plumcraft_session::Session;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A request on the control channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlRequest {
    #[serde(default)]
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// The reply to one request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlReply {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub result: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ControlReply {
    pub fn ok(id: u64, result: Value) -> Self {
        ControlReply { id, ok: true, result, error: None }
    }

    pub fn err(id: u64, error: impl Into<String>) -> Self {
        ControlReply { id, ok: false, result: Value::Null, error: Some(error.into()) }
    }
}

/// Parse one line of the channel. A malformed line gets an error reply (id 0), not a panic.
pub fn parse_line(line: &str) -> Result<ControlRequest, ControlReply> {
    serde_json::from_str(line).map_err(|e| ControlReply::err(0, format!("malformed request: {e}")))
}

/// The command table as `command.list` rows.
pub fn command_rows(session: &Session) -> Value {
    let rows: Vec<Value> = plumcraft_session::command_specs().iter().map(|c| plumcraft_session::commands::list_entry(session, c)).collect();
    json!(rows)
}

/// Handle a request against a session.
pub fn dispatch(session: &mut Session, req: &ControlRequest) -> ControlReply {
    match req.method.as_str() {
        "engine.commands" | "command.list" => ControlReply::ok(req.id, command_rows(session)),
        "engine.execute" | "command.run" => {
            let Some(command) = req.params.get("command").and_then(Value::as_str) else {
                return ControlReply::err(req.id, "params.command must be a command id");
            };
            let params = req.params.get("params").cloned().unwrap_or(Value::Null);
            match session.execute(command, &params) {
                Ok(v) => ControlReply::ok(req.id, v),
                Err(e) => ControlReply::err(req.id, e.to_string()),
            }
        }
        "graph.get" => ControlReply::ok(req.id, serde_json::to_value(session.graph()).unwrap_or(Value::Null)),
        other => ControlReply::err(req.id, format!("unknown method {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_reply_round_trip() {
        let req = parse_line(r#"{"id": 7, "method": "command.run", "params": {"command": "run.auto"}}"#).unwrap();
        assert_eq!(req.id, 7);
        let text = serde_json::to_string(&req).unwrap();
        assert_eq!(serde_json::from_str::<ControlRequest>(&text).unwrap(), req);
        let reply = ControlReply::err(7, "nope");
        let back: ControlReply = serde_json::from_str(&serde_json::to_string(&reply).unwrap()).unwrap();
        assert_eq!(back, reply);
        assert_eq!(serde_json::to_value(ControlReply::ok(1, Value::Null)).unwrap(), json!({"id": 1, "ok": true}));
    }

    #[test]
    fn dispatch_runs_commands() {
        let mut s = Session::new(plumcraft_text::engine().schema());
        let run = |s: &mut Session, line: &str| dispatch(s, &parse_line(line).unwrap());
        let r = run(&mut s, r#"{"id": 1, "method": "command.run", "params": {"command": "node.add", "params": {"kind": "text.upper"}}}"#);
        assert!(r.ok, "{r:?}");
        assert!(r.result["id"].is_string(), "{r:?}");
        let r = run(&mut s, r#"{"id": 2, "method": "command.list"}"#);
        assert!(r.result.as_array().unwrap().iter().any(|c| c["id"] == "node.add" && c["enabled"] == true));
        // run.start needs an engine connected: listed, disabled, with the reason
        assert!(r.result.as_array().unwrap().iter().any(|c| c["id"] == "run.start" && c["enabled"] == false && c["disabledReason"].is_string()));
        let r = run(&mut s, r#"{"id": 3, "method": "graph.get"}"#);
        assert_eq!(r.result["nodes"].as_object().unwrap().len(), 1);
        assert!(!run(&mut s, r#"{"id": 4, "method": "command.run", "params": {}}"#).ok);
        assert!(!run(&mut s, r#"{"id": 5, "method": "command.run", "params": {"command": "no.such"}}"#).ok);
        assert!(!run(&mut s, r#"{"id": 6, "method": "bogus"}"#).ok);
        assert!(parse_line("not json").is_err());
        assert!(parse_line(r#"{"id": -1, "method": "x"}"#).is_err());
    }
}
