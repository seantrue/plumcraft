//! Runs (spec section 8): the request, its ack, the reply, the snapshot behind `run.get`, and
//! `run.peek`, `run.plan` and `output.save`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PROTOCOL;
use crate::data::{DataRef, PortValue};
use crate::de::{lenient, lenient_map, lenient_opt, lenient_vec, null_as_default};
use crate::display::{Display, Rendition, WatchRequest};
use crate::error::NodeError;
use crate::event::NodeState;
use crate::handle::Handle;

/// The graph a run executes: the workflow JSON inline, or a handle from `graph.put`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GraphRef {
    Inline { graph: Value },
    Handle { handle: Handle },
}

impl Default for GraphRef {
    fn default() -> Self {
        GraphRef::Inline { graph: Value::Null }
    }
}

/// Thumbnail and viewer sizes (long side, pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct PreviewSizes {
    pub node_px: u32,
    pub inspect_px: u32,
}

impl Default for PreviewSizes {
    fn default() -> Self {
        PreviewSizes { node_px: 160, inspect_px: 1024 }
    }
}

/// How a run uses the memo. The exactness and conformance gates send `off` with a fresh store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MemoMode {
    /// Recompute the targets, keep upstream results.
    Refresh,
    Off,
    /// The default; also what an unknown mode reads as.
    #[default]
    #[serde(other)]
    Use,
}

/// `{"v": 2, "op": "run", …}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct RunRequest {
    pub v: u32,
    pub op: String,
    #[serde(deserialize_with = "null_as_default")]
    pub run_id: String,
    /// Monotonic per session; records and replies of older generations are ignored.
    #[serde(deserialize_with = "null_as_default")]
    pub generation: u64,
    /// 0–9: runs 9, saves and ops 5.
    pub priority: u8,
    pub graph: GraphRef,
    /// Empty: the graph's sinks. Watched nodes are added by the engine.
    #[serde(deserialize_with = "null_as_default")]
    pub targets: Vec<String>,
    #[serde(deserialize_with = "lenient_vec")]
    pub watch: Vec<WatchRequest>,
    #[serde(deserialize_with = "lenient")]
    pub preview: PreviewSizes,
    #[serde(deserialize_with = "lenient")]
    pub memo: MemoMode,
    /// `{"node.port": value}`: external bindings that override a port's wire or widget.
    /// Joins graphs across engines (decision D14) and lets a client feed a node a value; spec 7.6 rule 10.
    #[serde(deserialize_with = "lenient_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, PortValue>,
    /// `node.port`s whose value the engine keeps for this run if it can't send it (spec 7.6 rule 11).
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "Vec::is_empty")]
    pub hold: Vec<String>,
    /// Folders besides its store where this run's `path` locators may be read (spec 7.6 rule 10).
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "Vec::is_empty")]
    pub trusted_roots: Vec<String>,
    /// Survive the session going idle, for `run_ttl_s`.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "std::ops::Not::not")]
    pub detach: bool,
    /// The life of the graph this run belongs to, from its opening to the next: what the engine
    /// keeps for it (scratch files) goes on `graph.release`. Empty: unscoped (spec 5.3).
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "String::is_empty")]
    pub scope: String,
    /// The hash algorithm for the handles this run makes (spec 4.3): `b3` only when `hash.b3`
    /// was negotiated with every engine that reads them. Empty or anything else: md5. An engine
    /// without the capability ignores it.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "String::is_empty")]
    pub algo: String,
}

impl Default for RunRequest {
    fn default() -> Self {
        RunRequest {
            v: PROTOCOL,
            op: "run".into(),
            run_id: String::new(),
            generation: 0,
            priority: 9,
            graph: GraphRef::default(),
            targets: Vec::new(),
            watch: Vec::new(),
            preview: PreviewSizes::default(),
            memo: MemoMode::Use,
            inputs: BTreeMap::new(),
            hold: Vec::new(),
            trusted_roots: Vec::new(),
            detach: false,
            scope: String::new(),
            algo: String::new(),
        }
    }
}

impl RunRequest {
    /// The algorithm this run's handles use: [`Algo::B3`] only when `algo` is `b3`.
    pub fn handle_algo(&self) -> crate::handle::Algo {
        if self.algo == "b3" { crate::handle::Algo::B3 } else { crate::handle::Algo::Md5 }
    }

    pub fn new(run_id: impl Into<String>, generation: u64, graph: Value) -> Self {
        RunRequest { run_id: run_id.into(), generation, graph: GraphRef::Inline { graph }, ..Default::default() }
    }
}

/// The immediate answer to `run`; the result arrives as a `run.reply` event.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct RunAck {
    pub accepted: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub run_id: String,
    /// Runs ahead of this one.
    #[serde(deserialize_with = "null_as_default")]
    pub queued_behind: u32,
}

/// A node's memo identity in this run (spec 14).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct MemoInfo {
    #[serde(deserialize_with = "null_as_default")]
    pub plan_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_key: Option<String>,
    /// Where a cached result came from: `memory`, `store` or `content` (found by its inputs'
    /// bytes, not how they were made). Absent when the node ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// What computing this result took, when it was computed or a record says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub computed_ms: Option<f64>,
}

/// The store tree as a run left it. Its size comes from the reaper's last walk and can be
/// unknown; the disk's free space is always read, and is what the store refuses writes by.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct TreeUsage {
    #[serde(deserialize_with = "lenient_opt", skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(deserialize_with = "lenient_opt", skip_serializing_if = "Option::is_none")]
    pub files: Option<u64>,
    /// The tree's budget.
    #[serde(deserialize_with = "null_as_default")]
    pub max_bytes: u64,
    /// Free space on the store's disk, and the floor under which the store refuses writes.
    #[serde(deserialize_with = "lenient_opt", skip_serializing_if = "Option::is_none")]
    pub free_bytes: Option<u64>,
    #[serde(deserialize_with = "null_as_default")]
    pub min_free_bytes: u64,
}

impl TreeUsage {
    /// Is the disk close to the floor? Within half the floor again, so a warning comes before
    /// the first refusal.
    pub fn low_disk(&self) -> bool {
        self.free_bytes.is_some_and(|free| free < self.min_free_bytes.saturating_add(self.min_free_bytes / 2))
    }
}

/// What a run's caches did (capability `run.stats`, spec 14.6).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct RunStats {
    /// Nodes that ran.
    #[serde(deserialize_with = "null_as_default")]
    pub computed: u64,
    /// Nodes answered by the in-memory memo, by a persisted record, and by a content key.
    #[serde(deserialize_with = "null_as_default")]
    pub from_memory: u64,
    #[serde(deserialize_with = "null_as_default")]
    pub from_store: u64,
    #[serde(deserialize_with = "null_as_default")]
    pub from_content: u64,
    /// What the nodes that ran took, and what the hits would have taken.
    #[serde(deserialize_with = "null_as_default")]
    pub computed_ms: f64,
    #[serde(deserialize_with = "null_as_default")]
    pub saved_ms: f64,
    /// Records written, so a later run or process can skip the node.
    #[serde(deserialize_with = "null_as_default")]
    pub kept: u64,
    /// Persistable nodes with no usable record.
    #[serde(deserialize_with = "null_as_default")]
    pub store_missed: u64,
    /// Entries the in-memory memo dropped.
    #[serde(deserialize_with = "null_as_default")]
    pub evicted: u64,
    /// The store's own counters over the run, by contract name (`shm_hit`, `shm_declined_space`).
    #[serde(deserialize_with = "lenient_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub store: BTreeMap<String, u64>,
    /// The tree, when its usage is known.
    #[serde(deserialize_with = "lenient_opt", skip_serializing_if = "Option::is_none")]
    pub tree: Option<TreeUsage>,
}

impl RunStats {
    /// Nodes a cache answered.
    pub fn hits(&self) -> u64 {
        self.from_memory + self.from_store + self.from_content
    }

    /// Writes the store refused, for any reason.
    pub fn declined(&self) -> u64 {
        self.store.iter().filter(|(k, _)| k.starts_with("shm_declined_")).map(|(_, v)| *v).sum()
    }

    /// Add `other`'s counts; the tree is the latest known.
    pub fn add(&mut self, other: &RunStats) {
        self.computed += other.computed;
        self.from_memory += other.from_memory;
        self.from_store += other.from_store;
        self.from_content += other.from_content;
        self.computed_ms += other.computed_ms;
        self.saved_ms += other.saved_ms;
        self.kept += other.kept;
        self.store_missed += other.store_missed;
        self.evicted += other.evicted;
        for (k, v) in &other.store {
            *self.store.entry(k.clone()).or_default() += v;
        }
        if other.tree.is_some() {
            self.tree.clone_from(&other.tree);
        }
    }
}

/// One node's result.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct NodeReport {
    #[serde(deserialize_with = "lenient")]
    pub state: NodeState,
    #[serde(deserialize_with = "null_as_default")]
    pub ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker: Option<String>,
    #[serde(deserialize_with = "lenient_opt")]
    pub error: Option<NodeError>,
    /// Where each input's value came from: wire | bundle | widget | default.
    #[serde(deserialize_with = "lenient_map")]
    pub provenance: BTreeMap<String, String>,
    #[serde(deserialize_with = "lenient_opt")]
    pub thumb: Option<Rendition>,
    /// Text the node shows (a summary, a warning about an unused override).
    pub text: Option<String>,
    /// Targets only.
    #[serde(deserialize_with = "lenient_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, PortValue>,
    /// Editor payloads by editor kind (`curves`, `mask`); opaque to core.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "BTreeMap::is_empty")]
    pub editor: BTreeMap<String, Value>,
    #[serde(deserialize_with = "lenient_opt", skip_serializing_if = "Option::is_none")]
    pub memo: Option<MemoInfo>,
}

/// `run.reply`: authoritative. The reducer fills every node without a record from it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct RunReply {
    #[serde(deserialize_with = "null_as_default")]
    pub v: u32,
    #[serde(deserialize_with = "null_as_default")]
    pub run_id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub generation: u64,
    /// Every node `done` or `cached`.
    #[serde(deserialize_with = "null_as_default")]
    pub ok: bool,
    /// A run-level failure: `graph`, `engine` or `cancelled`.
    #[serde(deserialize_with = "lenient_opt")]
    pub error: Option<NodeError>,
    #[serde(deserialize_with = "null_as_default")]
    pub order: Vec<String>,
    #[serde(deserialize_with = "null_as_default")]
    pub elapsed_ms: f64,
    #[serde(deserialize_with = "lenient_map")]
    pub nodes: BTreeMap<String, NodeReport>,
    /// In request order; an entry with only `error` set says why there is no display.
    #[serde(deserialize_with = "lenient_vec")]
    pub watch: Vec<Display>,
    /// What the run's caches did (spec 14.6): the whole run's, across its engines.
    #[serde(deserialize_with = "lenient_opt", skip_serializing_if = "Option::is_none")]
    pub stats: Option<RunStats>,
    /// A coordinator's run: each engine's share.
    #[serde(deserialize_with = "lenient_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub engine_stats: BTreeMap<String, RunStats>,
}

impl RunReply {
    /// The host's synthesised failure (spec 12.3): exactly one outcome per request, always.
    pub fn failed(run_id: &str, generation: u64, error: NodeError) -> Self {
        RunReply { v: PROTOCOL, run_id: run_id.into(), generation, ok: false, error: Some(error), ..Default::default() }
    }
}

/// Where a run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Accepted,
    Running,
    Finished,
    Cancelled,
    Failed,
}

/// A node as `run.get` sees it now.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct NodeSnapshot {
    #[serde(deserialize_with = "lenient")]
    pub state: NodeState,
    pub ms: Option<f64>,
    pub fraction: Option<f64>,
    pub stage: Option<String>,
    #[serde(deserialize_with = "lenient_opt")]
    pub thumb: Option<Rendition>,
    #[serde(deserialize_with = "lenient_opt")]
    pub error: Option<NodeError>,
}

/// An appending value's bytes so far.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Partial {
    /// The next append's offset.
    pub offset: u64,
    #[serde(deserialize_with = "lenient_opt")]
    pub partial: Option<DataRef>,
}

/// `run.get`: the resync path, mandatory on every engine (spec 8.5).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct RunSnapshot {
    #[serde(deserialize_with = "null_as_default")]
    pub v: u32,
    #[serde(deserialize_with = "null_as_default")]
    pub run_id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub generation: u64,
    #[serde(deserialize_with = "lenient")]
    pub phase: Phase,
    /// The last envelope `seq` this snapshot includes.
    #[serde(deserialize_with = "null_as_default")]
    pub last_seq: u64,
    #[serde(deserialize_with = "lenient_map")]
    pub nodes: BTreeMap<String, NodeSnapshot>,
    /// One per watch entry; `None` until it exists.
    #[serde(deserialize_with = "null_as_default")]
    pub watch: Vec<Option<Display>>,
    /// `"node.port"` → bytes so far.
    #[serde(deserialize_with = "lenient_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub partials: BTreeMap<String, Partial>,
    /// Once finished.
    #[serde(deserialize_with = "lenient_opt")]
    pub reply: Option<RunReply>,
}

/// `run.peek`: open any (node, port, index) of a run without a new run.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct PeekRequest {
    #[serde(deserialize_with = "null_as_default")]
    pub run_id: String,
    #[serde(deserialize_with = "lenient")]
    pub watch: WatchRequest,
}

/// `{display}` now, `{pending}` (it arrives on a `preview` record), or `{missing}`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct PeekReply {
    #[serde(deserialize_with = "lenient_opt", skip_serializing_if = "Option::is_none")]
    pub display: Option<Display>,
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
    #[serde(deserialize_with = "lenient_opt", skip_serializing_if = "Option::is_none")]
    pub error: Option<NodeError>,
}

impl PeekReply {
    /// Without `run.peek`, the host reruns with the memo instead.
    pub fn unsupported() -> Self {
        PeekReply { error: Some(NodeError::new(NodeError::ENGINE, "run.peek is not supported")), ..Default::default() }
    }
}

/// `run.plan`: order, plan keys and cached states without running.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct PlanReply {
    #[serde(deserialize_with = "null_as_default")]
    pub run_id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub order: Vec<String>,
    #[serde(deserialize_with = "lenient_map")]
    pub nodes: BTreeMap<String, PlanNode>,
    #[serde(deserialize_with = "lenient_vec", skip_serializing_if = "Vec::is_empty")]
    pub missing_sources: Vec<Handle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct PlanNode {
    #[serde(deserialize_with = "null_as_default")]
    pub plan_key: String,
    #[serde(deserialize_with = "null_as_default")]
    pub cached: bool,
}

/// A path-hinted widget value rewritten for an engine that can't read the UI's files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Source {
    pub handle: Handle,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub media: String,
}

/// `output.save`: write (or return) a node's output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct SaveRequest {
    pub v: u32,
    pub op: String,
    pub graph: GraphRef,
    #[serde(deserialize_with = "null_as_default")]
    pub node: String,
    #[serde(deserialize_with = "null_as_default")]
    pub port: String,
    pub index: Option<i64>,
    /// Omitted when `data.path` is off: the files come back as blobs.
    pub path: Option<String>,
    #[serde(deserialize_with = "null_as_default")]
    pub format: String,
    #[serde(deserialize_with = "null_as_default")]
    pub options: serde_json::Map<String, Value>,
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "std::ops::Not::not")]
    pub sheet: bool,
    /// What crosses into this engine's share of a larger graph, as `RunRequest.inputs`.
    #[serde(deserialize_with = "lenient_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, PortValue>,
    /// As `RunRequest.trusted_roots`.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "Vec::is_empty")]
    pub trusted_roots: Vec<String>,
    /// The whole graph that share came from: what a saved recipe holds, so it remakes the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe_graph: Option<Value>,
}

impl Default for SaveRequest {
    fn default() -> Self {
        SaveRequest {
            v: PROTOCOL,
            op: "output.save".into(),
            graph: GraphRef::default(),
            node: String::new(),
            port: String::new(),
            index: None,
            path: None,
            format: String::new(),
            options: Default::default(),
            sheet: false,
            inputs: BTreeMap::new(),
            trusted_roots: Vec::new(),
            recipe_graph: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct SavedFile {
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(rename = "ref", deserialize_with = "lenient_opt")]
    pub data: Option<DataRef>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct SaveReply {
    #[serde(deserialize_with = "null_as_default")]
    pub ok: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub paths: Vec<String>,
    #[serde(deserialize_with = "lenient_vec", skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<SavedFile>,
    /// Every file carries its recipe.
    #[serde(deserialize_with = "null_as_default")]
    pub recipe: bool,
    /// A contact sheet's item count.
    #[serde(deserialize_with = "null_as_default")]
    pub items: u64,
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(deserialize_with = "lenient_opt")]
    pub error: Option<NodeError>,
}

impl SaveReply {
    pub fn unsupported() -> Self {
        SaveReply { error: Some(NodeError::new(NodeError::ENGINE, "output.save is not supported")), ..Default::default() }
    }

    pub fn failed(error: NodeError) -> Self {
        SaveReply { error: Some(error), ..Default::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_shape() {
        let mut r = RunRequest::new("r1", 17, serde_json::json!({"version": 1, "nodes": {}}));
        r.watch.push(WatchRequest { node: "n7".into(), port: "value".into(), ..Default::default() });
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["v"], 2);
        assert_eq!(v["op"], "run");
        assert_eq!(v["graph"]["kind"], "inline");
        assert_eq!(v["priority"], 9);
        assert_eq!(v["memo"], "use");
        assert!(v.get("inputs").is_none() && v.get("detach").is_none());
        let back: RunRequest = serde_json::from_value(v).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn spec_reply_parses() {
        let r: RunReply = serde_json::from_str(
            r#"{"v": 2, "run_id": "0b5e", "generation": 17, "ok": true, "error": null,
                "order": ["n1", "n2", "n7"], "elapsed_ms": 812.4,
                "nodes": {"n7": {"state": "done", "ms": 412.0, "worker": "Plan@gpu-host:4182",
                                 "provenance": {"sigma": "wire", "iterations": "bundle"},
                                 "outputs": {"value": {"kind": "json", "value": 3}, "bad": {"kind": "??"}},
                                 "editor": {"curves": {"names": ["R"]}},
                                 "memo": {"plan_key": "md5:x", "content_key": "md5:y"}},
                          "n9": {"state": "teleported"}},
                "watch": [{"media": "scalar", "value": 3}, {"error": {"kind": "op", "message": "no such port"}}]}"#,
        )
        .unwrap();
        assert_eq!(r.nodes["n7"].state, NodeState::Done);
        assert_eq!(r.nodes["n7"].outputs.len(), 1);
        assert_eq!(r.nodes["n9"].state, NodeState::Unknown);
        assert_eq!(r.watch.len(), 2);
        assert_eq!(r.watch[1].error.as_ref().unwrap().kind, "op");
    }

    #[test]
    fn snapshot_and_peek() {
        let s: RunSnapshot = serde_json::from_str(
            r#"{"v": 2, "run_id": "r", "generation": 1, "phase": "running", "last_seq": 1043,
                "nodes": {"n7": {"state": "progress", "fraction": 0.42, "stage": "dabs 3/7"}},
                "watch": [null, {"media": "text"}], "partials": {"n9.text": {"offset": 5120}}, "reply": null}"#,
        )
        .unwrap();
        assert_eq!(s.phase, Phase::Running);
        assert_eq!(s.nodes["n7"].fraction, Some(0.42));
        assert_eq!(s.watch.len(), 2);
        assert_eq!(s.partials["n9.text"].offset, 5120);
        let p: PeekReply = serde_json::from_str(r#"{"pending": true}"#).unwrap();
        assert!(p.pending && p.display.is_none());
    }

    #[test]
    fn synthesised_failure() {
        let r = RunReply::failed("r", 3, NodeError::engine("the engine stopped (signal 11); restarting"));
        assert!(!r.ok);
        assert_eq!(r.error.as_ref().unwrap().kind, "engine");
    }
}
