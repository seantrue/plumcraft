//! Events and node status (spec section 9): the envelope, the event kinds, status records and
//! their states, live data in flight (spec 10.6) and how each kind may be conflated.
//!
//! Any binding may lose, duplicate or reorder `node.status` and `node.live` events; the reducer
//! rules (spec 9.5) and the authoritative reply make the display correct anyway.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PROTOCOL;
use crate::data::{DataRef, PortValue};
use crate::de::{lenient_map, lenient_opt, lenient_vec, null_as_default};
use crate::display::{Display, Rendition};
use crate::error::NodeError;
use crate::run::RunReply;
use crate::session::EngineState;

/// A node's state (spec 9.4). Every UI accepts all nine; an unknown state is
/// [`NodeState::Unknown`] and its record is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    Queued,
    Started,
    Progress,
    /// Adds thumb, outputs and displays; never changes a terminal state.
    Preview,
    Done,
    Cached,
    Error,
    Skipped,
    Cancelled,
    #[default]
    #[serde(other)]
    Unknown,
}

impl NodeState {
    pub fn is_terminal(self) -> bool {
        matches!(self, NodeState::Done | NodeState::Cached | NodeState::Error | NodeState::Skipped | NodeState::Cancelled)
    }

    /// The order records from different publishers are reduced in (spec 9.5 rule 2): never by
    /// timestamp, because publishers' clocks differ. `preview` ranks with `progress`; it only
    /// adds data.
    pub fn rank(self) -> u8 {
        match self {
            NodeState::Unknown => 0,
            NodeState::Queued => 1,
            NodeState::Started => 2,
            NodeState::Progress | NodeState::Preview => 3,
            NodeState::Done | NodeState::Cached | NodeState::Error | NodeState::Skipped | NodeState::Cancelled => 4,
        }
    }
}

/// A `node.status` body (spec 9.3).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct StatusRecord {
    #[serde(deserialize_with = "null_as_default")]
    pub node_id: String,
    /// The record's own counter, per publisher and run (protocol 1's `seq`). With `publisher`,
    /// it orders a node's records and drops redeliveries.
    #[serde(deserialize_with = "null_as_default")]
    pub seq: u64,
    /// A stable id of the emitting process instance.
    #[serde(deserialize_with = "null_as_default")]
    pub publisher: String,
    #[serde(deserialize_with = "null_as_default")]
    pub ts: f64,
    #[serde(deserialize_with = "crate::de::lenient")]
    pub state: NodeState,
    /// `progress`: 0..1.
    pub fraction: Option<f64>,
    /// `progress`: short text under the bar.
    pub stage: Option<String>,
    /// MUST on `done`; also `cached` and `error`.
    pub ms: Option<f64>,
    pub worker: Option<String>,
    #[serde(deserialize_with = "lenient_opt")]
    pub error: Option<NodeError>,
    /// `preview`: rgba8 at ≤ `node_px`.
    #[serde(deserialize_with = "lenient_opt")]
    pub thumb: Option<Rendition>,
    /// `preview`, targets only.
    #[serde(deserialize_with = "lenient_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, PortValue>,
    /// `preview` of a watched node with `display.stream` on: one per watch entry for it.
    #[serde(deserialize_with = "lenient_vec", skip_serializing_if = "Vec::is_empty")]
    pub display: Vec<Display>,
}

impl StatusRecord {
    pub fn new(node_id: impl Into<String>, state: NodeState) -> Self {
        StatusRecord { node_id: node_id.into(), state, ..Default::default() }
    }
}

/// Live data in flight for a watched value (spec 10.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "stream", rename_all = "snake_case")]
pub enum LiveUpdate {
    /// A whole provisional value; the newest per (node, port) wins.
    Snapshot {
        node_id: String,
        port: String,
        #[serde(default)]
        seq: u64,
        #[serde(default)]
        provisional: bool,
        display: Box<Display>,
    },
    /// A growing value with a contiguous `offset`; a gap makes the viewer fetch `run.get`
    /// partials.
    Append {
        node_id: String,
        port: String,
        #[serde(default)]
        seq: u64,
        #[serde(default)]
        offset: u64,
        /// Text appended inline.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inline: Option<String>,
        /// Audio frames in the chunk.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frames: Option<u64>,
        /// A chunk too large to inline.
        #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
        data: Option<Box<DataRef>>,
    },
    /// A node-scoped message: status tooltip and run log, never the value.
    Log {
        node_id: String,
        #[serde(default)]
        port: Option<String>,
        #[serde(default)]
        level: String,
        #[serde(default)]
        text: String,
    },
}

impl LiveUpdate {
    pub fn node_id(&self) -> &str {
        match self {
            LiveUpdate::Snapshot { node_id, .. } | LiveUpdate::Append { node_id, .. } | LiveUpdate::Log { node_id, .. } => node_id,
        }
    }
}

/// A `log` event.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct LogRecord {
    #[serde(deserialize_with = "null_as_default")]
    pub level: String,
    #[serde(deserialize_with = "null_as_default")]
    pub logger: String,
    #[serde(deserialize_with = "null_as_default")]
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    /// The request this log belongs to (a long `x.vi.deliver`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op_id: Option<u64>,
}

/// The client must call `run.get`: events were lost or the ring no longer covers it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Resync {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(deserialize_with = "null_as_default")]
    pub reason: String,
}

/// An event body, by kind (spec 9.2).
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    EngineState(EngineState),
    SchemaChanged {
        schema_digest: String,
    },
    Status(Box<StatusRecord>),
    Live(Box<LiveUpdate>),
    RunReply(Box<RunReply>),
    Log(LogRecord),
    Resync(Resync),
    /// An unknown kind, or a known kind whose body didn't parse. Readers ignore it.
    Unknown {
        kind: String,
    },
}

impl Event {
    pub const ENGINE_STATE: &'static str = "engine.state";
    pub const SCHEMA_CHANGED: &'static str = "schema.changed";
    pub const NODE_STATUS: &'static str = "node.status";
    pub const NODE_LIVE: &'static str = "node.live";
    pub const RUN_REPLY: &'static str = "run.reply";
    pub const LOG: &'static str = "log";
    pub const RESYNC: &'static str = "resync";

    pub fn kind(&self) -> &str {
        match self {
            Event::EngineState(_) => Self::ENGINE_STATE,
            Event::SchemaChanged { .. } => Self::SCHEMA_CHANGED,
            Event::Status(_) => Self::NODE_STATUS,
            Event::Live(_) => Self::NODE_LIVE,
            Event::RunReply(_) => Self::RUN_REPLY,
            Event::Log(_) => Self::LOG,
            Event::Resync(_) => Self::RESYNC,
            Event::Unknown { kind } => kind,
        }
    }

    fn body(&self) -> Value {
        let v = match self {
            Event::EngineState(s) => serde_json::to_value(s),
            Event::SchemaChanged { schema_digest } => Ok(serde_json::json!({ "schema_digest": schema_digest })),
            Event::Status(r) => serde_json::to_value(r),
            Event::Live(u) => serde_json::to_value(u),
            Event::RunReply(r) => serde_json::to_value(r),
            Event::Log(l) => serde_json::to_value(l),
            Event::Resync(r) => serde_json::to_value(r),
            Event::Unknown { .. } => Ok(Value::Null),
        };
        v.unwrap_or(Value::Null)
    }

    fn from_body(kind: &str, body: Value) -> Event {
        let parsed = match kind {
            Self::ENGINE_STATE => serde_json::from_value(body).map(Event::EngineState),
            Self::SCHEMA_CHANGED => {
                Ok(Event::SchemaChanged { schema_digest: body.get("schema_digest").and_then(Value::as_str).unwrap_or_default().to_owned() })
            }
            Self::NODE_STATUS => serde_json::from_value::<StatusRecord>(body).map(|r| Event::Status(Box::new(r))),
            Self::NODE_LIVE => serde_json::from_value::<LiveUpdate>(body).map(|u| Event::Live(Box::new(u))),
            Self::RUN_REPLY => serde_json::from_value::<RunReply>(body).map(|r| Event::RunReply(Box::new(r))),
            Self::LOG => serde_json::from_value(body).map(Event::Log),
            Self::RESYNC => serde_json::from_value(body).map(Event::Resync),
            _ => return Event::Unknown { kind: kind.chars().take(64).collect() },
        };
        parsed.unwrap_or_else(|_| Event::Unknown { kind: kind.to_owned() })
    }

    /// How a queue may conflate this event (spec 9.2, 9.7).
    pub fn delivery(&self) -> Delivery {
        match self {
            Event::Status(r) if matches!(r.state, NodeState::Progress | NodeState::Preview) => Delivery::Latest(format!("{}\u{0}{:?}", r.node_id, r.state)),
            Event::Live(u) => match u.as_ref() {
                LiveUpdate::Snapshot { node_id, port, .. } => Delivery::Latest(format!("{node_id}\u{0}{port}\u{0}snapshot")),
                LiveUpdate::Append { .. } => Delivery::Ordered,
                LiveUpdate::Log { .. } => Delivery::DropOldest(256),
            },
            Event::Log(_) => Delivery::DropOldest(1024),
            _ => Delivery::Ordered,
        }
    }
}

/// A queue's treatment of an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// Never dropped, in order.
    Ordered,
    /// Only the newest per key (within a run) is kept.
    Latest(String),
    /// Kept in order, but the oldest go past this many per run.
    DropOldest(usize),
}

/// One event on the wire: `{"v": 2, "seq", "ts", "kind", "run_id"?, "generation"?, "body"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(from = "RawEnvelope", into = "RawEnvelope")]
pub struct Envelope {
    pub v: u32,
    /// Per session, strictly increasing, assigned in one place: delivery order and resume.
    /// Never used for dedup or for ordering within a node.
    pub seq: u64,
    pub ts: f64,
    pub run_id: Option<String>,
    pub generation: Option<u64>,
    pub event: Event,
}

impl Envelope {
    pub fn new(seq: u64, ts: f64, run: Option<(&str, u64)>, event: Event) -> Self {
        Envelope { v: PROTOCOL, seq, ts, run_id: run.map(|(r, _)| r.to_owned()), generation: run.map(|(_, g)| g), event }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
struct RawEnvelope {
    #[serde(deserialize_with = "null_as_default")]
    v: u32,
    #[serde(deserialize_with = "null_as_default")]
    seq: u64,
    #[serde(deserialize_with = "null_as_default")]
    ts: f64,
    #[serde(deserialize_with = "null_as_default")]
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "lenient_opt")]
    run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "lenient_opt")]
    generation: Option<u64>,
    body: Value,
}

impl Default for RawEnvelope {
    fn default() -> Self {
        RawEnvelope { v: 0, seq: 0, ts: 0.0, kind: String::new(), run_id: None, generation: None, body: Value::Null }
    }
}

impl From<RawEnvelope> for Envelope {
    fn from(r: RawEnvelope) -> Self {
        Envelope { v: r.v, seq: r.seq, ts: r.ts, run_id: r.run_id, generation: r.generation, event: Event::from_body(&r.kind, r.body) }
    }
}

impl From<Envelope> for RawEnvelope {
    fn from(e: Envelope) -> Self {
        RawEnvelope { v: e.v, seq: e.seq, ts: e.ts, kind: e.event.kind().to_owned(), run_id: e.run_id, generation: e.generation, body: e.event.body() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_status_event() {
        let e: Envelope = serde_json::from_str(
            r#"{"v": 2, "seq": 1043, "ts": 1792170000.123, "kind": "node.status", "run_id": "0b5e", "generation": 17,
                "body": {"node_id": "n7", "seq": 41, "publisher": "Step.gpu@gpu-host:4182#a1", "ts": 1792170000.123,
                         "state": "progress", "fraction": 0.42, "stage": "dabs 3/7", "ms": null, "worker": "Step.gpu@gpu-host:4182",
                         "error": null, "thumb": null, "outputs": null, "display": null}}"#,
        )
        .unwrap();
        let Event::Status(r) = &e.event else { panic!("not a status: {e:?}") };
        assert_eq!(r.state, NodeState::Progress);
        assert_eq!(r.seq, 41);
        assert_eq!(e.seq, 1043);
        assert_eq!(e.generation, Some(17));
        assert!(matches!(e.event.delivery(), Delivery::Latest(_)));
        let back: Envelope = serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn unknown_kinds_and_bad_bodies_are_ignorable() {
        let e: Envelope = serde_json::from_str(r#"{"v": 2, "seq": 1, "kind": "node.teleport", "body": {}}"#).unwrap();
        assert_eq!(e.event, Event::Unknown { kind: "node.teleport".into() });
        let e: Envelope = serde_json::from_str(r#"{"v": 2, "seq": 2, "kind": "node.live", "body": {"stream": "smell"}}"#).unwrap();
        assert!(matches!(e.event, Event::Unknown { .. }));
        let e: Envelope = serde_json::from_str(r#"{"v": 2, "seq": 3, "kind": "node.status", "body": {"node_id": "n", "state": "melting"}}"#).unwrap();
        let Event::Status(r) = e.event else { panic!() };
        assert_eq!(r.state, NodeState::Unknown);
    }

    #[test]
    fn live_updates() {
        for (text, delivery) in [
            (r#"{"stream": "append", "node_id": "n9", "port": "text", "seq": 118, "offset": 5120, "inline": "the next tokens"}"#, Delivery::Ordered),
            (r#"{"stream": "log", "node_id": "n7", "port": null, "level": "info", "text": "pyramid level 2"}"#, Delivery::DropOldest(256)),
            (
                r#"{"stream": "snapshot", "node_id": "n7", "port": "result", "seq": 3, "provisional": true, "display": {"media": "image"}}"#,
                Delivery::Latest("n7\u{0}result\u{0}snapshot".into()),
            ),
        ] {
            let u: LiveUpdate = serde_json::from_str(text).unwrap();
            assert_eq!(Event::Live(Box::new(u)).delivery(), delivery, "{text}");
        }
    }

    #[test]
    fn states_rank() {
        assert!(NodeState::Started.rank() < NodeState::Progress.rank());
        assert!(NodeState::Done.is_terminal() && !NodeState::Preview.is_terminal());
        assert_eq!(serde_json::to_value(NodeState::Cached).unwrap(), "cached");
    }
}
