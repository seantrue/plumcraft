//! Sessions (spec section 5): `hello`, its reply, `session.confirm`, limits and the engine
//! state the host reports.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::PROTOCOL;
use crate::caps::Caps;
use crate::de::{lenient, lenient_opt, null_as_default};
use crate::error::{ErrorCode, ProtoError};

/// A range of protocol majors one side speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct ProtocolRange {
    pub min: u32,
    pub max: u32,
}

impl Default for ProtocolRange {
    fn default() -> Self {
        ProtocolRange { min: PROTOCOL, max: PROTOCOL }
    }
}

impl ProtocolRange {
    /// The highest major both ranges contain, or why there is none (error `version`).
    pub fn choose(client: ProtocolRange, engine: ProtocolRange) -> Result<u32, ProtoError> {
        let hi = client.max.min(engine.max);
        let lo = client.min.max(engine.min);
        if hi >= lo {
            Ok(hi)
        } else {
            Err(ProtoError::new(ErrorCode::Version, format!("engine speaks protocol {}–{}, this app {}–{}", engine.min, engine.max, client.min, client.max)))
        }
    }
}

/// Who the client is.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct ClientInfo {
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub version: String,
    /// The base it is built on (`plumcraft 0.1.0`).
    #[serde(deserialize_with = "null_as_default")]
    pub base: String,
}

/// The client's host, for the same-host proof and store roots.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct HostInfo {
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub boot: String,
    #[serde(deserialize_with = "null_as_default")]
    pub stores: Vec<String>,
}

/// One direction of the same-host proof (spec 5.4): a nonce file the other side reads.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Proof {
    #[serde(deserialize_with = "null_as_default")]
    pub path: String,
    #[serde(deserialize_with = "null_as_default")]
    pub nonce: String,
    /// In the engine's reply: it read the client's nonce file and it matched.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "std::ops::Not::not")]
    pub client_proof_read: bool,
}

/// UI → engine: open (or resume) a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Hello {
    pub v: u32,
    pub op: String,
    #[serde(deserialize_with = "lenient")]
    pub protocol: ProtocolRange,
    #[serde(deserialize_with = "lenient")]
    pub client: ClientInfo,
    /// The session to resume, and the last envelope `seq` the client applied.
    pub session: Option<String>,
    pub since: Option<u64>,
    #[serde(deserialize_with = "lenient")]
    pub caps: Caps,
    #[serde(deserialize_with = "lenient")]
    pub host: HostInfo,
    #[serde(deserialize_with = "lenient_opt")]
    pub proof: Option<Proof>,
    /// Required on sockets and non-loopback HTTP. Never logged ([`Hello::redacted`]).
    pub token: Option<String>,
}

impl Default for Hello {
    fn default() -> Self {
        Hello {
            v: PROTOCOL,
            op: "hello".into(),
            protocol: ProtocolRange::default(),
            client: ClientInfo::default(),
            session: None,
            since: None,
            caps: Caps::default(),
            host: HostInfo::default(),
            proof: None,
            token: None,
        }
    }
}

impl Hello {
    /// A copy safe to log.
    pub fn redacted(&self) -> Hello {
        Hello { token: self.token.as_ref().map(|_| "<redacted>".into()), ..self.clone() }
    }
}

/// Who answered, and what its results depend on.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct EngineInfo {
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub version: String,
    /// Component versions (`impressionist`, `yapocis`, …).
    #[serde(deserialize_with = "crate::de::lenient_map")]
    pub components: BTreeMap<String, String>,
    /// The engine's code digest, part of every memo key (spec 14).
    #[serde(deserialize_with = "null_as_default")]
    pub salt: String,
    /// Lets the UI skip a catalogue it has cached.
    #[serde(deserialize_with = "null_as_default")]
    pub schema_digest: String,
}

/// Where the engine's data lives.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct DataInfo {
    /// Hash algorithms accepted, preferred first.
    #[serde(deserialize_with = "null_as_default")]
    pub algos: Vec<String>,
    #[serde(deserialize_with = "null_as_default")]
    pub stores: Vec<String>,
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "String::is_empty")]
    pub work_root: String,
    /// A remote fleet's blob server (spec 16.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_url: Option<String>,
}

/// Session limits (spec 5.2). Absent fields take these defaults.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Limits {
    pub max_frame: u64,
    pub max_inline: u64,
    pub max_status_inline: u64,
    pub max_display_inline: u64,
    pub max_runs: u32,
    pub max_blob: u64,
    pub ring_events: u64,
    pub ring_seconds: u64,
    pub run_ttl_s: u64,
    pub snapshot_runs: u32,
    pub idle_cancel_s: u64,
    pub inactivity_s: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_frame: 16 << 20,
            max_inline: 64 << 10,
            max_status_inline: 4 << 10,
            max_display_inline: 64 << 10,
            max_runs: 1,
            max_blob: 4 << 30,
            ring_events: 4096,
            ring_seconds: 60,
            run_ttl_s: 600,
            snapshot_runs: 16,
            idle_cancel_s: 30,
            inactivity_s: 30,
        }
    }
}

/// Whether a resumed session's missed events were replayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Replay {
    Ok,
    /// The ring no longer covers `since`: the engine emits `resync`; the client calls `run.get`.
    Gap,
}

/// Engine → UI: the reply to [`Hello`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct HelloReply {
    pub v: u32,
    /// The major both chose.
    pub protocol: u32,
    #[serde(deserialize_with = "null_as_default")]
    pub session: String,
    #[serde(deserialize_with = "lenient")]
    pub engine: EngineInfo,
    #[serde(deserialize_with = "lenient")]
    pub caps: Caps,
    #[serde(deserialize_with = "lenient")]
    pub data: DataInfo,
    #[serde(deserialize_with = "lenient_opt")]
    pub proof: Option<Proof>,
    #[serde(deserialize_with = "lenient")]
    pub limits: Limits,
    #[serde(deserialize_with = "lenient_opt")]
    pub replay: Option<Replay>,
}

impl Default for HelloReply {
    fn default() -> Self {
        HelloReply {
            v: PROTOCOL,
            protocol: PROTOCOL,
            session: String::new(),
            engine: EngineInfo::default(),
            caps: Caps::default(),
            data: DataInfo::default(),
            proof: None,
            limits: Limits::default(),
            replay: None,
        }
    }
}

/// UI → engine after `hello`: the same-host verdicts (spec 5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct SessionConfirm {
    /// The UI read the engine's nonce: engine `commonkit`/`path` locators are usable.
    pub shm: bool,
    /// The engine read the UI's nonce: the engine may read paths the UI names.
    pub path: bool,
}

/// What the host reports about its engine (spec 12.4). Engines push only states only they know
/// (`unavailable`); every other state is the host's.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EngineState {
    #[default]
    Off,
    Connecting {
        #[serde(default)]
        url: String,
    },
    /// A broker or server that can't be reached, with the fix.
    Unreachable {
        #[serde(default)]
        url: String,
        #[serde(default)]
        reason: String,
        #[serde(default)]
        retry_s: u64,
        #[serde(default)]
        hint: String,
    },
    /// The engine isn't running or crashed, with the fix (from the engine manifest).
    EngineDown {
        #[serde(default)]
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        queues: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stderr_tail: Option<String>,
        #[serde(default)]
        hint: String,
    },
    LoadingSchema,
    /// Up but unusable (no GPU device, protocol mismatch).
    Unavailable {
        #[serde(default)]
        reason: String,
    },
    /// Missed pings during a run.
    Busy {
        #[serde(default)]
        since: f64,
    },
    Connected {
        #[serde(default)]
        version: String,
        #[serde(default)]
        nodes: u64,
        #[serde(default)]
        caps: Caps,
    },
}

impl EngineState {
    pub fn is_connected(&self) -> bool {
        matches!(self, EngineState::Connected { .. } | EngineState::Busy { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_choice() {
        let r = |min, max| ProtocolRange { min, max };
        assert_eq!(ProtocolRange::choose(r(2, 2), r(1, 3)).unwrap(), 2);
        assert_eq!(ProtocolRange::choose(r(1, 3), r(2, 4)).unwrap(), 3);
        let e = ProtocolRange::choose(r(2, 2), r(3, 3)).unwrap_err();
        assert_eq!(e.code, ErrorCode::Version);
        assert!(e.message.contains("protocol 3"));
    }

    #[test]
    fn spec_hello_parses_and_redacts() {
        let h: Hello = serde_json::from_str(
            r#"{"v": 2, "op": "hello", "protocol": {"min": 2, "max": 2},
                "client": {"name": "research", "version": "0.9.0", "base": "plumcraft 0.1.0"},
                "session": null, "since": null,
                "caps": ["events.push", "events.poll", "data.shm"],
                "host": {"id": "abc", "boot": "b", "stores": ["/s"]},
                "proof": {"path": "/s/.hello/c-1.nonce", "nonce": "5f1e"},
                "token": "secret"}"#,
        )
        .unwrap();
        assert!(h.caps.has("data.shm"));
        assert_eq!(h.redacted().token.as_deref(), Some("<redacted>"));
        assert!(!format!("{:?}", h.redacted()).contains("secret"));
    }

    #[test]
    fn reply_defaults_and_hostile_parts() {
        let r: HelloReply =
            serde_json::from_str(r#"{"v": 2, "session": "s-1", "limits": {"max_runs": 4}, "replay": "sideways", "proof": 7, "caps": {"no": 1}}"#).unwrap();
        assert_eq!(r.limits.max_runs, 4);
        assert_eq!(r.limits.max_frame, 16 << 20, "absent limits keep their defaults");
        assert_eq!(r.replay, None);
        assert_eq!(r.proof, None);
        assert!(r.caps.is_empty());
    }

    #[test]
    fn engine_states() {
        let s = EngineState::Unreachable { url: "amqp://localhost".into(), reason: "refused".into(), retry_s: 4, hint: "brew services start rabbitmq".into() };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["state"], "unreachable");
        assert_eq!(serde_json::from_value::<EngineState>(v).unwrap(), s);
        assert!(serde_json::from_str::<EngineState>(r#"{"state": "melting"}"#).is_err());
    }
}
