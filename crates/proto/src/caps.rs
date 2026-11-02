//! Capabilities (spec 5.3): strings both sides list in `hello`. A feature is on when both list
//! it (or, for engine-only capabilities, when the engine does). Capability strings replace minor
//! versions; unknown strings are kept and ignored.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The capability strings this crate knows. Commands' `enabled` checks read these, never engine
/// names.
pub mod known {
    pub const EVENTS_PUSH: &str = "events.push";
    pub const EVENTS_POLL: &str = "events.poll";
    pub const EVENTS_RESUME: &str = "events.resume";
    pub const RUN_PLAN: &str = "run.plan";
    pub const RUN_PEEK: &str = "run.peek";
    pub const RUN_WATCH: &str = "run.watch";
    pub const RUN_MULTI: &str = "run.multi";
    pub const GRAPH_PUT: &str = "graph.put";
    pub const STATUS_QUEUED: &str = "status.queued";
    pub const STATUS_PROGRESS: &str = "status.progress";
    pub const DISPLAY_STREAM: &str = "display.stream";
    pub const LIVE_SNAPSHOT: &str = "live.snapshot";
    pub const LIVE_APPEND: &str = "live.append";
    pub const DATA_SHM: &str = "data.shm";
    pub const DATA_PATH: &str = "data.path";
    pub const DATA_CAS: &str = "data.cas";
    pub const DATA_MEM: &str = "data.mem";
    pub const BLOB_DERIVE: &str = "blob.derive";
    pub const BLOB_RANGE: &str = "blob.range";
    pub const MEMO_LOOKUP: &str = "memo.lookup";
    pub const MEMO_PERSIST: &str = "memo.persist";
    pub const MEMO_AUDIT: &str = "memo.audit";
    pub const RUN_STATS: &str = "run.stats";
    pub const ADMIN: &str = "admin";
    pub const HASH_B3: &str = "hash.b3";
    pub const OUTPUT_SAVE: &str = "output.save";
    pub const EDITOR_CURVES: &str = "editor.curves";
    pub const EDITOR_MASK: &str = "editor.mask";
    pub const CODEC_JSON: &str = "codec.json";
    pub const CODEC_MSGPACK: &str = "codec.msgpack";

    /// Listed by the engine alone: on when the engine lists it (spec 5.3, "Who lists it").
    pub const ENGINE_ONLY: &[&str] = &[
        RUN_PLAN,
        RUN_PEEK,
        RUN_WATCH,
        RUN_MULTI,
        GRAPH_PUT,
        DATA_CAS,
        BLOB_DERIVE,
        BLOB_RANGE,
        MEMO_LOOKUP,
        MEMO_PERSIST,
        MEMO_AUDIT,
        ADMIN,
        OUTPUT_SAVE,
        EDITOR_CURVES,
        EDITOR_MASK,
    ];

    /// Is this a pack operation's capability (`x.<pack>.<op>`)? Those are engine-only too.
    pub fn is_pack_op(cap: &str) -> bool {
        let mut parts = cap.split('.');
        matches!((parts.next(), parts.next(), parts.next(), parts.next()), (Some("x"), Some(p), Some(o), None) if !p.is_empty() && !o.is_empty())
    }
}

/// The most capability strings kept from one message, and the longest; the rest are dropped
/// (hostile input, spec 18).
const MAX_CAPS: usize = 512;
const MAX_CAP_LEN: usize = 96;

/// A set of capability strings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(transparent)]
pub struct Caps(BTreeSet<String>);

impl<'de> Deserialize<'de> for Caps {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(de)?;
        let items = match v {
            serde_json::Value::Array(items) => items,
            _ => Vec::new(),
        };
        Ok(Caps(items.into_iter().filter_map(|i| i.as_str().filter(|s| !s.is_empty() && s.len() <= MAX_CAP_LEN).map(str::to_owned)).take(MAX_CAPS).collect()))
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Caps {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Caps".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "array",
            "description": "Capability strings (spec 5.3); unknown strings are ignored",
            "items": {"type": "string", "minLength": 1, "maxLength": MAX_CAP_LEN},
            "uniqueItems": true,
            "maxItems": MAX_CAPS
        })
    }
}

impl Caps {
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(caps: I) -> Self {
        Caps(caps.into_iter().map(Into::into).collect())
    }

    pub fn has(&self, cap: &str) -> bool {
        self.0.contains(cap)
    }

    pub fn insert(&mut self, cap: impl Into<String>) {
        self.0.insert(cap.into());
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// What a session may use: a capability both list, or an engine-only one the engine lists.
    pub fn negotiate(client: &Caps, engine: &Caps) -> Caps {
        Caps(engine.0.iter().filter(|c| client.has(c) || known::ENGINE_ONLY.contains(&c.as_str()) || known::is_pack_op(c)).cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_intersects_shared_and_keeps_engine_only() {
        let client = Caps::new(["events.push", "events.poll", "data.shm", "live.append"]);
        let engine = Caps::new(["events.push", "events.poll", "run.peek", "data.shm", "x.vi.export", "editor.curves", "hash.b3", "live.snapshot"]);
        let s = Caps::negotiate(&client, &engine);
        assert!(s.has("events.push") && s.has("data.shm"));
        assert!(s.has("run.peek") && s.has("x.vi.export") && s.has("editor.curves"));
        assert!(!s.has("hash.b3"), "both must list a shared capability");
        assert!(!s.has("live.append"), "the engine didn't list it");
        assert!(!s.has("live.snapshot"));
    }

    #[test]
    fn hostile_lists() {
        let c: Caps = serde_json::from_str(r#"["a", 3, null, "", "b", {"x": 1}]"#).unwrap();
        assert_eq!(c.len(), 2);
        let c: Caps = serde_json::from_str(r#""events.push""#).unwrap();
        assert!(c.is_empty());
        let long = format!("[\"{}\"]", "x".repeat(200));
        assert!(serde_json::from_str::<Caps>(&long).unwrap().is_empty());
        assert!(known::is_pack_op("x.vi.export") && !known::is_pack_op("x.vi") && !known::is_pack_op("x..y") && !known::is_pack_op("x.a.b.c"));
    }
}
