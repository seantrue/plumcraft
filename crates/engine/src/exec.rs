//! The graph executor: workflow JSON in the research software's shape (`{"version": 1, "nodes": {id: {kind,
//! values}}, "wires": [{from: {node, port}, to: {node, port}}]}`), run in topological order with
//! every status record the interface requires (spec 7.6, 9.4).
//!
//! Inputs resolve as a wire, then the node's widget value, then the port's default (spec 7.6
//! rule 5, without bundles yet). A failing node is `error` and everything downstream is
//! `skipped` (rule 7). UI-only fields (positions, frames, titles) are ignored (rule 8).
//!
//! **Lists (rule 2, lifted from the research software's executor):** a [`Data::List`] reaching an input
//! that takes one item runs the node once per item, unless the node takes `input_list`. Each
//! list carries its axes: a list a node makes is one axis, `<node>.<port>` (or the axes the
//! node named, so its outputs zip), and a node mapped over lists gives its outputs the union of
//! their axes. Lists meeting at a node zip along an axis they share and form a product over
//! axes they don't (two Vary nodes into two settings: every combination, the first varying
//! slowest). Single values broadcast. A Vary Choices node (`flags.vary_choices`) gets the
//! choices of the ENUM inputs it feeds as `_choices`, part of its memo key.
//!
//! **The memo (spec 14):** every node gets a plan key from its kind, version, the engine salt,
//! its resolved parameters (coerced to INT or FLOAT by port type) and its upstream nodes' plan
//! keys. A node whose key is in the [`Memo`] is not computed: it sends `preview` (thumb and
//! watched displays from the stored outputs) and `cached`, never `started`. The run's `memo`
//! mode chooses: `use` (the default), `refresh` (recompute the targets), `off` (neither read
//! nor write). Nodes declaring `memo.scope: none` are never stored, and pass a per-run identity
//! downstream, so nothing below them is reused either. Errors are never stored. A stored
//! result whose file is gone is a miss, never an error (spec 4.6; `files_present`).
//!
//! **Persisting (spec 14.5):** a `machine`-scope node whose own work (less the setup it reports
//! with [`Cx::setup_took`]) took the engine's threshold ([`Engine::persist_ms`], 1 s by
//! default) leaves a record in the store ([`crate::persist`]), unless writing and reading its
//! outputs back would cost about as much; a workflow node's `cache` (`always`, `never`)
//! overrides. A node not in memory looks for a record before it computes, so a slow result
//! survives a restart until the store's TTL or budget reaps it.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::time::Instant;

use plumcraft_proto::catalogue::{Media, MemoScope, NodeDef, PathHint};
use plumcraft_proto::display::{Display, Rendition, WatchRequest};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::{NodeState, StatusRecord};
use plumcraft_proto::memo::{self, Numeric};
use plumcraft_proto::run::{GraphRef, MemoInfo, MemoMode, NodeReport, RunReply, RunRequest, RunStats};
use plumcraft_proto::slice;
use plumcraft_proto::{PROTOCOL, PortValue};
use serde::Deserialize;
use serde_json::Value;

use crate::data::{self, Plane};
use crate::{BlobSource, CancelToken, Cx, Data, Emit, Engine, Outputs, now_ts};

/// The longest text an excerpt rendition carries (spec 10.3).
const EXCERPT_CHARS: usize = 4096;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Workflow {
    nodes: BTreeMap<String, WorkflowNode>,
    wires: Vec<WorkflowWire>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WorkflowNode {
    kind: String,
    values: serde_json::Map<String, Value>,
    /// Whether to persist this node's result (spec 14.5): `auto` (the default: when its own
    /// work took the engine's threshold and the write is worth it), `always` or `never`.
    /// Anything else is `auto`.
    #[serde(deserialize_with = "lenient_cache")]
    cache: Cache,
}

/// A node's persist choice (the workflow node's `cache`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Cache {
    #[default]
    Auto,
    Always,
    Never,
}

fn lenient_cache<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Cache, D::Error> {
    let v = Value::deserialize(d).unwrap_or(Value::Null);
    Ok(match v.as_str() {
        Some("always") => Cache::Always,
        Some("never") => Cache::Never,
        _ => Cache::Auto,
    })
}

/// Bytes the store writes and reads back per ms, roughly (an SSD's ~200 MB/s): a result is
/// persisted only when writing it and reading it back would cost well under computing it.
const STORE_BYTES_PER_MS: f64 = 200_000.0;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WorkflowWire {
    from: End,
    to: End,
    /// A slice of the list it carries (spec 7.6 rule 3). Junk here is the node's `config` error.
    #[serde(deserialize_with = "lenient_slice")]
    slice: Option<String>,
}

/// A wire's `slice`: text, and anything else (or blank) is no slice.
fn lenient_slice<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned))
}

impl WorkflowWire {
    /// Its slice's canonical text, as the memo key names it; a bad slice as written.
    fn slice_text(&self) -> Option<String> {
        self.slice.as_deref().map(|s| slice::parse(s).map_or_else(|_| s.to_owned(), |p| p.to_string()))
    }

    /// The value this wire delivers from `value` (spec 7.6 rule 3): a range keeps a list a list
    /// (a sweep of its own, shorter than the list it came from), an index takes one item.
    fn deliver(&self, value: &Data) -> Result<Data, NodeError> {
        let Some(spec) = self.slice.as_deref() else { return Ok(value.clone()) };
        let bad = |why: String| NodeError::new(NodeError::CONFIG, format!("input {:?}: {why}", self.to.port));
        let s = slice::parse(spec).map_err(bad)?;
        let Data::List(l) = value else { return Err(bad(format!("slice {spec:?} needs a list, but the wire carries a single value"))) };
        let picks = s.picks(l.items.len()).map_err(bad)?;
        if s.is_index() {
            return picks.first().and_then(|&i| l.items.get(i)).cloned().ok_or_else(|| bad(format!("slice {spec:?} picks nothing")));
        }
        let names: Vec<String> = l.axes_or_plain().into_iter().map(|(name, _)| name).collect();
        Ok(Data::List(crate::List {
            items: picks.iter().filter_map(|&i| l.items.get(i).cloned()).collect(),
            axes: vec![(format!("{}[{s}]", names.join("+")), picks.len())],
            labels: picks.iter().filter_map(|&i| l.labels.get(i).cloned()).collect(),
        }))
    }
}

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(default)]
struct End {
    node: String,
    port: String,
}

/// Memo entries kept; the oldest goes first.
const MEMO_ENTRIES: usize = 1024;

/// Results by plan key, in memory for the engine's life (spec 14.4: `session` and `machine`
/// scopes alike, until persistence lands). The host drops it when it rebuilds the engine.
#[derive(Default)]
pub struct Memo {
    /// Outputs, and what computing them took.
    entries: HashMap<String, (Outputs, f64)>,
    order: VecDeque<String>,
    /// Entries dropped since the last run's stats.
    evicted: u64,
}

impl Memo {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn get(&self, key: &str) -> Option<&(Outputs, f64)> {
        self.entries.get(key)
    }

    fn put(&mut self, key: String, outputs: Outputs, ms: f64) {
        if self.entries.insert(key.clone(), (outputs, ms)).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > MEMO_ENTRIES {
            if let Some(old) = self.order.pop_front() {
                self.entries.remove(&old);
                self.evicted += 1;
            }
        }
    }

    /// Entries dropped since the last call.
    fn take_evicted(&mut self) -> u64 {
        std::mem::take(&mut self.evicted)
    }
}

/// A value's identity by its bytes (spec 14.3), as a bound value's is (`bind::identity`). Blobs
/// carry their handle and JSON is its own; an array is hashed only when `hash` says the node is
/// worth it (`memo.content`), and otherwise there is no content key.
fn content_identity(d: &Data, hash: bool) -> Option<Value> {
    match d {
        Data::Json(v) => Some(serde_json::json!({"json": v})),
        Data::Blob(b) => Some(Value::from(b.handle.to_string())),
        Data::Array(a) if hash => plumcraft_proto::Handle::md5_nd1(&a.dtype, &a.shape, &a.bytes).ok().map(|h| Value::from(h.to_string())),
        Data::Audio(a) if hash => {
            let bytes = a.bytes();
            plumcraft_proto::Handle::md5_nd1("<f4", &[a.frames(), u64::from(a.channels)], &bytes).ok().map(|h| Value::from(h.to_string()))
        }
        Data::Array(_) | Data::Audio(_) => None,
        Data::List(l) => l.items.iter().map(|i| content_identity(i, hash)).collect::<Option<Vec<_>>>().map(Value::Array),
    }
}

/// A node's content key (spec 14.3): its plan key with each wired input named by its bytes
/// rather than by what made them. `None` for a node with nothing wired, or an input not hashed.
fn content_key<'a>(
    kind: &str,
    def: &NodeDef,
    salt: &str,
    params: &serde_json::Map<String, Value>,
    inputs: &crate::Inputs,
    wired: impl Iterator<Item = &'a str>,
) -> Option<String> {
    let identities =
        wired.map(|port| Some((port.to_owned(), content_identity(inputs.get(port)?, def.memo.content)?))).collect::<Option<serde_json::Map<_, _>>>()?;
    (!identities.is_empty()).then(|| memo::plan_key(kind, def.version, salt, None, params, &identities))
}

/// Whether a stored result's files are all still there (spec 4.6: a reaped value is a miss,
/// never an error). A [`BlobSource::File`] under the engine's work root that its reaper took is
/// put back from the commonkit store when the store still holds it; otherwise the entry is a
/// miss and the node runs again. A work-root file still there is touched, so the reaper keeps
/// what the memo uses. A file anywhere else (a user's file, from Load Files) is only checked.
fn files_present(d: &Data, work_root: Option<&std::path::Path>, store: Option<&plumcraft_store::Store>) -> bool {
    match d {
        Data::List(l) => l.items.iter().all(|i| files_present(i, work_root, store)),
        Data::Blob(b) => match &b.source {
            BlobSource::File(p) => {
                let ours = work_root.is_some_and(|w| p.starts_with(w));
                let there = std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() == b.meta.size);
                match (there, ours) {
                    (true, true) => {
                        let _ = std::fs::File::options().append(true).open(p).and_then(|f| f.set_modified(std::time::SystemTime::now()));
                        true
                    }
                    (true, false) => true,
                    (false, true) => store.and_then(|s| s.read_handle(&b.handle)).is_some_and(|r| restore(p, &r.bytes)),
                    (false, false) => false,
                }
            }
            BlobSource::Bytes(_) => true,
        },
        Data::Json(_) | Data::Array(_) | Data::Audio(_) => true,
    }
}

/// Write `bytes` to `p` whole (a temp file beside it, then a rename).
fn restore(p: &std::path::Path, bytes: &[u8]) -> bool {
    let (Some(dir), Some(name)) = (p.parent(), p.file_name()) else { return false };
    let tmp = dir.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let ok = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&tmp, bytes)).and_then(|()| std::fs::rename(&tmp, p)).is_ok();
    if !ok {
        let _ = std::fs::remove_file(&tmp);
    }
    ok
}

/// How a node finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Done,
    Failed,
}

struct Run<'a> {
    emit: &'a mut dyn Emit,
    publisher: String,
    seq: u64,
}

impl Run<'_> {
    fn record(&mut self, mut rec: StatusRecord) {
        self.seq += 1;
        rec.seq = self.seq;
        rec.publisher = self.publisher.clone();
        rec.ts = now_ts();
        self.emit.status(rec);
    }

    fn state(&mut self, node: &str, state: NodeState) {
        self.record(StatusRecord::new(node, state));
    }
}

/// Run a request to completion and return the authoritative reply. Never panics on a bad graph:
/// that is a reply with a `graph` error.
pub fn run(engine: &Engine, req: &RunRequest, emit: &mut dyn Emit, cancel: &CancelToken, publisher: &str, memo: &mut Memo, plane: &Plane<'_>) -> RunReply {
    let started = Instant::now();
    let mut reply = RunReply { v: PROTOCOL, run_id: req.run_id.clone(), generation: req.generation, ..Default::default() };
    let mut workflow = match parse(engine, req) {
        Ok(w) => w,
        Err(e) => {
            reply.error = Some(e);
            return reply;
        }
    };
    let order = match topo_order(&workflow) {
        Ok(o) => o,
        Err(e) => {
            reply.error = Some(e);
            return reply;
        }
    };
    // Bindings (spec 7.6 rule 10) replace the wires into the ports they name, so what fed those
    // ports is neither waited for nor computed.
    let bound = crate::bind::by_node(&req.inputs, |n| workflow.nodes.contains_key(n));
    workflow.wires.retain(|w| !bound.get(&w.to.node).is_some_and(|ports| ports.contains_key(&w.to.port)));
    let wanted = needed(&workflow, &req.targets, &req.watch);
    let order: Vec<String> = order.into_iter().filter(|n| wanted.contains(n)).collect();
    let mut r = Run { emit, publisher: publisher.to_owned(), seq: 0 };
    let mut values: BTreeMap<String, Outputs> = BTreeMap::new();
    let mut outcome: BTreeMap<String, Outcome> = BTreeMap::new();
    let mut watch: Vec<Option<Display>> = vec![None; req.watch.len()];
    let mut keys: BTreeMap<String, String> = BTreeMap::new();
    let mut stats = RunStats::default();

    for id in &order {
        let Some(wn) = workflow.nodes.get(id) else { continue };
        let Some(node) = engine.node(&wn.kind) else { continue };
        if cancel.is_cancelled() {
            r.state(id, NodeState::Cancelled);
            reply.nodes.insert(id.clone(), NodeReport { state: NodeState::Cancelled, ..Default::default() });
            continue;
        }
        let def = node.def();
        let upstream: Vec<&WorkflowWire> = workflow.wires.iter().filter(|w| &w.to.node == id).collect();
        if let Some(bad) = upstream.iter().find(|w| outcome.get(&w.from.node) != Some(&Outcome::Done)) {
            let mut rec = StatusRecord::new(id, NodeState::Skipped);
            let err = NodeError::new(NodeError::SKIPPED, format!("upstream {} did not finish", bad.from.node));
            rec.error = Some(err.clone());
            r.record(rec);
            outcome.insert(id.clone(), Outcome::Failed);
            reply.nodes.insert(id.clone(), NodeReport { state: NodeState::Skipped, error: Some(err), ..Default::default() });
            continue;
        }
        let t0 = Instant::now();
        let no_bindings = BTreeMap::new();
        let binds = bound.get(id).unwrap_or(&no_bindings);
        let bound_data: Result<Outputs, NodeError> =
            binds.iter().map(|(port, v)| crate::bind::to_data(engine.store(), &req.trusted_roots, port, v).map(|d| (port.clone(), d))).collect();
        let mut resolved = bound_data.and_then(|b| resolve_inputs(&def, wn, &upstream, &values, &b, engine.store()));
        if let Ok((inputs, params)) = &mut resolved
            && let Some(v) = node.volatile(inputs)
        {
            params.insert(VOLATILE_KEY.into(), v);
        }
        if def.flags.vary_choices
            && let Ok((inputs, params)) = &mut resolved
        {
            let offered = offered_choices(engine, &workflow, id);
            match offered {
                Ok(c) => {
                    params.insert(CHOICES_KEY.into(), Value::from(c.clone()));
                    inputs.insert(CHOICES_KEY.into(), Data::Json(Value::from(c)));
                }
                Err(e) => resolved = Err(e),
            }
        }
        let key = resolved.as_ref().ok().map(|(_, params)| {
            let inputs: serde_json::Map<String, Value> = upstream
                .iter()
                .map(|w| (w.to.port.clone(), memo::input_identity(keys.get(&w.from.node).map_or("", String::as_str), &w.from.port, w.slice_text().as_deref())))
                .chain(binds.iter().map(|(port, v)| (port.clone(), crate::bind::identity(v))))
                .collect();
            memo::plan_key(&wn.kind, def.version, &engine.info().salt, None, params, &inputs)
        });
        let stores = req.memo != MemoMode::Off && def.memo.scope != MemoScope::None;
        let reads = stores && (req.memo == MemoMode::Use || !req.targets.iter().any(|t| t == id));
        if let Some(k) = &key {
            // Downstream of a `none` node, the input is new every run: pass a per-run identity
            // down the chain so nothing below it hits on a stale value.
            let identity = if def.memo.scope == MemoScope::None { format!("memo:none:{}:{}:{id}", req.run_id, req.generation) } else { k.clone() };
            keys.insert(id.clone(), identity);
        }
        let persists = def.memo.scope == MemoScope::Machine && wn.cache != Cache::Never;
        let store = engine.store().filter(|_| persists);
        // Memory, then a persisted record (spec 14.5), for `machine`-scope nodes.
        let mut lookup = |k: &str| -> Option<(Outputs, f64, bool)> {
            let held = memo.get(k).filter(|(o, _)| o.values().all(|d| files_present(d, engine.work_root(), plane.store)));
            if let Some((o, ms)) = held {
                return Some((o.clone(), *ms, true));
            }
            let (o, ms) = crate::persist::load(store?, k).filter(|(o, _)| def.outputs.iter().all(|p| o.contains_key(&p.name)))?;
            memo.put(k.to_owned(), o.clone(), ms);
            Some((o, ms, false))
        };
        let found = key.as_deref().filter(|_| reads).and_then(&mut lookup);
        // The plan key missed: the same node on the same bytes, however they were made (spec 14.3).
        let content = resolved.as_ref().ok().filter(|_| stores && found.is_none()).and_then(|(inputs, params)| {
            let wired = upstream.iter().map(|w| w.to.port.as_str()).chain(binds.keys().map(String::as_str));
            content_key(&wn.kind, &def, &engine.info().salt, params, inputs, wired).filter(|k| key.as_ref() != Some(k))
        });
        let by_content = found.is_none();
        let found = found.or_else(|| content.as_deref().filter(|_| reads).and_then(&mut lookup));
        if found.is_none() && reads && store.is_some() {
            stats.store_missed += 1;
        }
        if let Some((outputs, computed_ms, held)) = found {
            if by_content && let Some(k) = &key {
                memo.put(k.clone(), outputs.clone(), computed_ms);
            }
            let source = match (by_content, held) {
                (true, _) => {
                    stats.from_content += 1;
                    "content"
                }
                (false, true) => {
                    stats.from_memory += 1;
                    "memory"
                }
                (false, false) => {
                    stats.from_store += 1;
                    "store"
                }
            };
            stats.saved_ms += computed_ms;
            let hit = outputs;
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let thumb = preview(&mut r, engine, &def, id, req, &hit, &mut watch, plane);
            let mut rec = StatusRecord::new(id, NodeState::Cached);
            rec.ms = Some(ms);
            r.record(rec);
            let outputs = target_outputs(engine, &def, req, id, &hit, plane);
            let report = NodeReport {
                state: NodeState::Cached,
                ms,
                thumb,
                outputs,
                memo: Some(MemoInfo {
                    plan_key: key.clone().unwrap_or_default(),
                    content_key: content,
                    source: Some(source.into()),
                    computed_ms: Some(computed_ms),
                }),
                ..Default::default()
            };
            reply.nodes.insert(id.clone(), report);
            outcome.insert(id.clone(), Outcome::Done);
            values.insert(id.clone(), hit);
            continue;
        }
        r.state(id, NodeState::Started);
        let mut setup = std::time::Duration::ZERO;
        let mut work_ms = 0.0;
        let result = resolved.and_then(|(inputs, _)| {
            let mut forward = |rec: StatusRecord| r.record(rec);
            let t1 = Instant::now();
            let out = call(node, &def, id, &inputs, cancel, &mut forward, &mut setup).map(|o| name_lists(id, o));
            work_ms = t1.elapsed().saturating_sub(setup).as_secs_f64() * 1000.0;
            out
        });
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        match result {
            Ok(outputs) if cancel.is_cancelled() => {
                drop(outputs);
                r.state(id, NodeState::Cancelled);
                outcome.insert(id.clone(), Outcome::Failed);
                reply.nodes.insert(id.clone(), NodeReport { state: NodeState::Cancelled, ms, ..Default::default() });
            }
            Ok(outputs) => {
                // The preview, the moment the node finishes and before anything downstream starts.
                let thumb = preview(&mut r, engine, &def, id, req, &outputs, &mut watch, plane);
                let mut done = StatusRecord::new(id, NodeState::Done);
                done.ms = Some(ms);
                r.record(done);
                let report_outputs = target_outputs(engine, &def, req, id, &outputs, plane);
                let memo_info = key.clone().map(|plan_key| MemoInfo { plan_key, content_key: content.clone(), source: None, computed_ms: Some(work_ms) });
                stats.computed += 1;
                stats.computed_ms += work_ms;
                reply
                    .nodes
                    .insert(id.clone(), NodeReport { state: NodeState::Done, ms, thumb, outputs: report_outputs, memo: memo_info, ..Default::default() });
                outcome.insert(id.clone(), Outcome::Done);
                if let Some(k) = key.filter(|_| stores) {
                    // Slow enough, and cheap to write and read back next to recomputing it.
                    let worth = |threshold: f64| work_ms >= threshold && crate::persist::size_of(&outputs) as f64 / STORE_BYTES_PER_MS * 2.0 < work_ms;
                    let persist = match wn.cache {
                        Cache::Always => true,
                        Cache::Never => false,
                        Cache::Auto => engine.persist_ms().is_some_and(worth),
                    };
                    if let Some(st) = store.filter(|_| persist) {
                        stats.kept += u64::from(crate::persist::save(st, &k, &outputs, work_ms));
                        if let Some(c) = &content {
                            crate::persist::save(st, c, &outputs, work_ms);
                        }
                    }
                    if let Some(c) = content {
                        memo.put(c, outputs.clone(), work_ms);
                    }
                    memo.put(k, outputs.clone(), work_ms);
                }
                values.insert(id.clone(), outputs);
            }
            Err(err) => {
                let state = if err.kind == NodeError::CANCELLED { NodeState::Cancelled } else { NodeState::Error };
                let mut rec = StatusRecord::new(id, state);
                rec.ms = Some(ms);
                rec.error = Some(err.clone());
                r.record(rec);
                outcome.insert(id.clone(), Outcome::Failed);
                reply.nodes.insert(id.clone(), NodeReport { state, ms, error: Some(err), ..Default::default() });
            }
        }
    }
    reply.order = order;
    reply.ok = !reply.nodes.is_empty() && reply.nodes.values().all(|n| matches!(n.state, NodeState::Done | NodeState::Cached));
    if cancel.is_cancelled() {
        reply.ok = false;
        reply.error = Some(NodeError::cancelled());
    }
    reply.watch = req
        .watch
        .iter()
        .zip(watch)
        .map(|(w, d)| {
            d.unwrap_or_else(|| Display {
                node: w.node.clone(),
                port: w.port.clone(),
                error: Some(NodeError::new(NodeError::OP, "no value")),
                ..Default::default()
            })
        })
        .collect();
    reply.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    stats.evicted = memo.take_evicted();
    if let Some(st) = engine.store() {
        stats.store = st.drain_stats();
        stats.tree = Some(st.usage());
    }
    reply.stats = Some(stats);
    reply
}

/// The input key under which a Vary Choices node gets the choices of the inputs it feeds.
pub const CHOICES_KEY: &str = "_choices";
/// The memo parameter under which a node's [`crate::Node::volatile`] identity goes.
pub const VOLATILE_KEY: &str = "_volatile";
/// The most runs a node mapped over lists makes.
pub const MAX_RUNS: usize = 100_000;
/// The most items a list display's sheet describes.
pub const MAX_SHEET: usize = 256;

/// The choices of the ENUM inputs node `id` feeds: every one it is wired into must offer the
/// same.
fn offered_choices(engine: &Engine, w: &Workflow, id: &str) -> Result<Vec<Value>, NodeError> {
    let mut offered: Option<Vec<Value>> = None;
    for wire in w.wires.iter().filter(|x| x.from.node == id) {
        let Some(port) =
            w.nodes.get(&wire.to.node).and_then(|n| engine.node(&n.kind)).and_then(|n| n.def().inputs.into_iter().find(|p| p.name == wire.to.port))
        else {
            continue;
        };
        let Some(choices) = port.choices.filter(|c| !c.is_empty()) else { continue };
        match &offered {
            Some(have) if *have != choices => {
                return Err(NodeError::new(
                    NodeError::CONFIG,
                    format!("Vary Choices feeds inputs with different choices ({} and {}.{})", wire.to.node, wire.to.node, wire.to.port),
                ));
            }
            Some(_) => {}
            None => offered = Some(choices),
        }
    }
    Ok(offered.unwrap_or_default())
}

/// How a node maps over its list inputs: the union of their axes (in order of first
/// appearance), and for each run which item of each list input it takes.
#[allow(clippy::type_complexity)]
fn mapping_plan(lists: &[(&str, &crate::List)]) -> Result<(Vec<(String, usize)>, Vec<Vec<usize>>), NodeError> {
    let mut axes: Vec<(String, usize)> = Vec::new();
    for (port, l) in lists {
        for (name, n) in l.axes_or_plain() {
            match axes.iter().find(|a| a.0 == name) {
                None => axes.push((name, n)),
                Some(have) if have.1 != n => {
                    let what = if name.is_empty() { "the lists".to_owned() } else { name };
                    return Err(NodeError::new(NodeError::CONFIG, format!("list inputs differ in length: {what} is {} long and {port:?} brings {n}", have.1)));
                }
                Some(_) => {}
            }
        }
    }
    let total = axes.iter().try_fold(1usize, |t, (_, n)| t.checked_mul(*n)).filter(|t| *t <= MAX_RUNS);
    let total = total.ok_or_else(|| NodeError::new(NodeError::CONFIG, format!("the lists make more than {MAX_RUNS} runs")))?;
    let mut picks = Vec::with_capacity(total);
    for t in 0..total {
        // the index along each axis, row-major (the first axis varies slowest)
        let mut idx = vec![0usize; axes.len()];
        let mut rest = t;
        for (slot, (_, n)) in idx.iter_mut().zip(&axes).rev() {
            *slot = rest % n.max(&1);
            rest /= n.max(&1);
        }
        let pick = lists
            .iter()
            .map(|(_, l)| {
                l.axes_or_plain()
                    .iter()
                    .fold(0usize, |j, (name, n)| j * n + axes.iter().position(|a| &a.0 == name).and_then(|i| idx.get(i)).copied().unwrap_or(0))
            })
            .collect();
        picks.push(pick);
    }
    Ok((axes, picks))
}

/// Run a node: once, or once per item when lists reach inputs that take one item (each run's
/// progress scaled into the whole), its outputs then lists over the runs' axes.
/// Run a node (once, or once per item of the lists it is mapped over), adding the time its
/// calls spent getting ready ([`Cx::setup_took`]) to `setup`.
fn call(
    node: &dyn crate::Node,
    def: &NodeDef,
    id: &str,
    inputs: &crate::Inputs,
    cancel: &CancelToken,
    records: &mut dyn FnMut(StatusRecord),
    setup: &mut std::time::Duration,
) -> Result<Outputs, NodeError> {
    let whole = |k: &str| def.flags.input_list || def.inputs.iter().any(|p| p.name == k && p.list);
    let lists: Vec<(&str, &crate::List)> = inputs.iter().filter(|(k, _)| !whole(k)).filter_map(|(k, v)| v.list().map(|l| (k.as_str(), l))).collect();
    if lists.is_empty() {
        let mut cx = Cx { node_id: id, cancel, records, last_progress: None, setup: std::time::Duration::ZERO };
        let out = node.run(inputs, &mut cx);
        *setup += cx.setup;
        return out;
    }
    let (axes, picks) = mapping_plan(&lists)?;
    let n = picks.len();
    let mut per_item: Vec<Outputs> = Vec::with_capacity(n);
    let mut labels: Vec<String> = Vec::with_capacity(n);
    let mut last: Option<Instant> = None;
    for (i, pick) in picks.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(NodeError::cancelled());
        }
        let mut item = crate::Inputs::new();
        let mut label: Vec<String> = Vec::new();
        for (k, v) in inputs {
            match lists.iter().position(|(p, _)| p == k) {
                Some(at) => {
                    let (l, j) = (lists[at].1, pick.get(at).copied().unwrap_or(0));
                    if let Some(x) = l.items.get(j) {
                        item.insert(k.clone(), x.clone());
                    }
                    let lab = item_label(k, l, j);
                    if !lab.is_empty() && !label.contains(&lab) {
                        label.push(lab);
                    }
                }
                None => {
                    item.insert(k.clone(), v.clone());
                }
            }
        }
        labels.push(label.join(" · "));
        let mut scaled = |mut rec: StatusRecord| {
            if rec.state == NodeState::Progress {
                rec.fraction = Some((i as f64 + rec.fraction.unwrap_or(0.0)) / n as f64);
                rec.stage = Some(match rec.stage.take() {
                    Some(s) => format!("item {} of {n}: {s}", i + 1),
                    None => format!("item {} of {n}", i + 1),
                });
            }
            records(rec);
        };
        if last.is_none_or(|t| t.elapsed().as_millis() >= 100) {
            last = Some(Instant::now());
            let mut rec = StatusRecord::new(id, NodeState::Progress);
            rec.fraction = Some(0.0);
            scaled(rec);
        }
        let mut cx = Cx { node_id: id, cancel, records: &mut scaled, last_progress: None, setup: std::time::Duration::ZERO };
        let out = node.run(&item, &mut cx);
        *setup += cx.setup;
        let out =
            out.map_err(|e| if e.kind == NodeError::CANCELLED { e } else { NodeError { message: format!("item {} of {n}: {}", i + 1, e.message), ..e } })?;
        per_item.push(out);
    }
    // An output of one item per run spans the runs' axes; one that is itself a list per run is
    // a new, flat list (named as it leaves the node).
    let mut names: Vec<String> = def.outputs.iter().map(|p| p.name.clone()).collect();
    for o in &per_item {
        names.extend(o.keys().filter(|k| !names.contains(k)).cloned().collect::<Vec<_>>());
    }
    let mut out = Outputs::new();
    for name in names {
        let vals: Vec<&Data> = per_item.iter().filter_map(|o| o.get(&name)).collect();
        if vals.len() != per_item.len() {
            continue;
        }
        let list = if vals.iter().all(|v| v.list().is_none()) {
            crate::List { items: vals.into_iter().cloned().collect(), axes: axes.clone(), labels: labels.clone() }
        } else {
            let mut flat = crate::List::default();
            for (v, run) in vals.into_iter().zip(&labels) {
                match v {
                    Data::List(l) => {
                        for (j, x) in l.items.iter().enumerate() {
                            flat.items.push(x.clone());
                            flat.labels.push([run.as_str(), l.label(j)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "));
                        }
                    }
                    one => {
                        flat.items.push(one.clone());
                        flat.labels.push(run.clone());
                    }
                }
            }
            flat
        };
        out.insert(name, Data::List(list));
    }
    Ok(out)
}

/// What item `j` of list input `port` says about a run: the item's own label (what made it
/// upstream) when it has one, else `port value` for a scalar (a swept setting).
pub fn item_label(port: &str, l: &crate::List, j: usize) -> String {
    if !l.label(j).is_empty() {
        return l.label(j).to_owned();
    }
    match l.items.get(j) {
        Some(Data::Json(v @ (Value::Number(_) | Value::Bool(_)))) => format!("{port} {}", short_number(v)),
        Some(Data::Json(Value::String(s))) if s.chars().count() <= 40 && !s.contains('\n') => format!("{port} {s}"),
        // a tuple of numbers (Vary Range 2D's xy): `port (x, y)`
        Some(Data::List(t)) if !t.items.is_empty() && t.items.iter().all(|i| matches!(i, Data::Json(Value::Number(_)))) => {
            format!("{port} ({})", t.items.iter().filter_map(Data::json).map(short_number).collect::<Vec<_>>().join(", "))
        }
        _ => String::new(),
    }
}

/// A number as a label shows it: at most 6 decimals, trailing zeros dropped.
pub fn short_number(v: &Value) -> String {
    match v.as_f64() {
        Some(f) if v.is_f64() && f.is_finite() => {
            let t = format!("{f:.6}");
            let t = t.trim_end_matches('0').trim_end_matches('.');
            if t == "-0" { "0".into() } else { t.to_owned() }
        }
        _ => v.to_string(),
    }
}

/// Give each list a node made without axes (or with axes that don't fit it) its own axis,
/// `<node>.<port>`, so it zips with itself downstream.
fn name_lists(id: &str, mut outputs: Outputs) -> Outputs {
    for (port, v) in &mut outputs {
        if let Data::List(l) = v {
            let fits = !l.axes.is_empty() && l.axes.iter().try_fold(1usize, |t, (_, n)| t.checked_mul(*n)) == Some(l.items.len());
            if !fits {
                l.axes = vec![(format!("{id}.{port}"), l.items.len())];
            }
        }
    }
    outputs
}

/// Send a node's `preview` record (thumb and watched displays) and return the thumb.
#[allow(clippy::too_many_arguments)]
fn preview(
    r: &mut Run<'_>,
    engine: &Engine,
    def: &NodeDef,
    id: &str,
    req: &RunRequest,
    outputs: &Outputs,
    watch: &mut [Option<Display>],
    plane: &Plane<'_>,
) -> Option<Rendition> {
    let mut pv = StatusRecord::new(id, NodeState::Preview);
    pv.thumb = def.outputs.first().and_then(|p| outputs.get(&p.name).and_then(|v| thumb(engine, &p.ty, v, plane)));
    for (i, w) in req.watch.iter().enumerate().filter(|(_, w)| w.node == id) {
        let d = display(engine, def, id, w, outputs, plane);
        pv.display.push(d.clone());
        if let Some(slot) = watch.get_mut(i) {
            *slot = Some(d);
        }
    }
    let thumb = pv.thumb.clone();
    r.record(pv);
    thumb
}

/// The outputs a reply carries: a target's only, each crossing as [`Plane::port_value`] decides.
fn target_outputs(engine: &Engine, def: &NodeDef, req: &RunRequest, id: &str, outputs: &Outputs, plane: &Plane<'_>) -> BTreeMap<String, PortValue> {
    if !req.targets.iter().any(|t| t == id) {
        return BTreeMap::new();
    }
    outputs.iter().map(|(k, v)| (k.clone(), plane.port_value(port_media(engine, def, k), v))).collect()
}

fn port_media(engine: &Engine, def: &NodeDef, port: &str) -> Media {
    def.output(port).and_then(|p| engine.wire_type(&p.ty)).map(|t| t.media).unwrap_or_default()
}

fn graph_error(message: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::GRAPH, message)
}

fn parse(engine: &Engine, req: &RunRequest) -> Result<Workflow, NodeError> {
    let GraphRef::Inline { graph } = &req.graph else {
        return Err(graph_error("graphs by handle aren't supported by this engine"));
    };
    let w: Workflow = serde_json::from_value(graph.clone()).map_err(|e| graph_error(format!("malformed workflow: {e}")))?;
    for (id, n) in &w.nodes {
        if engine.node(&n.kind).is_none() {
            return Err(graph_error(format!("node {id}: unknown kind {:?}", n.kind)));
        }
    }
    for wire in &w.wires {
        for end in [&wire.from, &wire.to] {
            if !w.nodes.contains_key(&end.node) {
                return Err(graph_error(format!("wire names unknown node {:?}", end.node)));
            }
        }
    }
    Ok(w)
}

/// Kahn's algorithm, ties broken by node id so the order is stable.
fn topo_order(w: &Workflow) -> Result<Vec<String>, NodeError> {
    let mut indegree: BTreeMap<&str, usize> = w.nodes.keys().map(|k| (k.as_str(), 0)).collect();
    for wire in &w.wires {
        if let Some(d) = indegree.get_mut(wire.to.node.as_str()) {
            *d += 1;
        }
    }
    let mut ready: BTreeSet<&str> = indegree.iter().filter(|(_, d)| **d == 0).map(|(k, _)| *k).collect();
    let mut order = Vec::with_capacity(w.nodes.len());
    while let Some(next) = ready.pop_first() {
        order.push(next.to_owned());
        for wire in w.wires.iter().filter(|x| x.from.node == next) {
            if let Some(d) = indegree.get_mut(wire.to.node.as_str()) {
                *d = d.saturating_sub(1);
                if *d == 0 {
                    ready.insert(wire.to.node.as_str());
                }
            }
        }
    }
    if order.len() != w.nodes.len() {
        return Err(graph_error("the graph has a cycle"));
    }
    Ok(order)
}

/// The nodes a run computes: the targets (or, when there are none, the graph's sinks), the
/// watched nodes, and all their ancestors.
fn needed(w: &Workflow, targets: &[String], watch: &[WatchRequest]) -> BTreeSet<String> {
    // Empty targets mean the graph's sinks (spec 8.1); watched nodes are added either way.
    let sinks: Vec<String> =
        if targets.is_empty() { w.nodes.keys().filter(|n| !w.wires.iter().any(|x| &x.from.node == *n)).cloned().collect() } else { targets.to_vec() };
    let mut want: VecDeque<String> = sinks.into_iter().chain(watch.iter().map(|x| x.node.clone())).filter(|n| w.nodes.contains_key(n)).collect();
    let mut out = BTreeSet::new();
    while let Some(n) = want.pop_front() {
        if out.insert(n.clone()) {
            want.extend(w.wires.iter().filter(|x| x.to.node == n).map(|x| x.from.node.clone()));
        }
    }
    out
}

/// The node's inputs, and its parameters (the resolved non-wired values) for the memo key. A
/// parameter on an INT or FLOAT port is coerced to that type first (spec 14.1 rule 1), and the
/// node receives the coerced value, so the key and the computation agree.
fn resolve_inputs(
    def: &NodeDef,
    wn: &WorkflowNode,
    upstream: &[&WorkflowWire],
    values: &BTreeMap<String, Outputs>,
    bound: &Outputs,
    store: Option<&plumcraft_store::Store>,
) -> Result<(crate::Inputs, serde_json::Map<String, Value>), NodeError> {
    let mut inputs = crate::Inputs::new();
    let mut params = serde_json::Map::new();
    for port in &def.inputs {
        // A variadic input (spec 7.6 rule 4): each wired slot (`c1`, `c2`, …) arrives under its
        // own name; an unwired slot is absent.
        if let Some(var) = &port.variadic {
            let mut any = false;
            for w in upstream.iter().filter(|w| is_slot(var, &w.to.port)) {
                if let Some(v) = values.get(&w.from.node).and_then(|o| o.get(&w.from.port)) {
                    inputs.insert(w.to.port.clone(), w.deliver(v)?);
                    any = true;
                }
            }
            for (slot, v) in bound.iter().filter(|(slot, _)| is_slot(var, slot)) {
                inputs.insert(slot.clone(), v.clone());
                any = true;
            }
            if !any && var.min > 0 && !port.optional {
                return Err(NodeError::new(NodeError::CONFIG, format!("input {:?}: connect at least one", port.name)));
            }
            continue;
        }
        let wired = match bound.get(&port.name) {
            Some(v) => Some(v.clone()),
            None => {
                let wire = upstream.iter().find(|w| w.to.port == port.name);
                wire.and_then(|w| values.get(&w.from.node).and_then(|o| o.get(&w.from.port)).map(|v| w.deliver(v))).transpose()?
            }
        };
        let numeric = match port.ty.as_str() {
            "INT" => Some(Numeric::Int),
            "FLOAT" => Some(Numeric::Float),
            _ => None,
        };
        let param = || {
            wn.values
                .get(&port.name)
                .filter(|v| !v.is_null())
                .cloned()
                .or_else(|| (!port.default.is_null()).then(|| port.default.clone()))
                .map(|v| memo::coerce(&v, numeric))
        };
        let value = match wired {
            Some(v) => Some(v),
            None => match param() {
                // An uploaded source (spec 8.1, 13.2): the node gets a local file with its bytes,
                // and the key is its handle, as a local file with the same bytes would be.
                Some(v) if port.path.as_ref().is_some_and(PathHint::is_source) && v.get("source").is_some() => {
                    let (handle, path) = materialize_source(store, &v["source"])?;
                    params.insert(port.name.clone(), Value::from(handle));
                    Some(Data::Json(Value::from(path)))
                }
                Some(v) => {
                    // A file source is keyed by its content, never its path (spec 14.3).
                    let identity = if port.path.as_ref().is_some_and(PathHint::is_source) {
                        v.as_str().and_then(source_identity).map_or_else(|| v.clone(), Value::from)
                    } else {
                        v.clone()
                    };
                    params.insert(port.name.clone(), identity);
                    Some(Data::Json(v))
                }
                None => None,
            },
        };
        match value {
            Some(v) => {
                inputs.insert(port.name.clone(), v);
            }
            None if port.optional => {}
            None => return Err(NodeError::new(NodeError::CONFIG, format!("input {:?} has no value", port.name))),
        }
    }
    Ok((inputs, params))
}

/// Whether `name` is one of a variadic input's slots: one of its `names`, or its `template`
/// (`c{n}`) with `n` from 1 to `max` (no upper bound when `max` is 0).
fn is_slot(v: &plumcraft_proto::catalogue::Variadic, name: &str) -> bool {
    if v.names.iter().any(|n| n == name) {
        return true;
    }
    let Some((pre, post)) = v.template.split_once("{n}") else { return false };
    let Some(digits) = name.strip_prefix(pre).and_then(|r| r.strip_suffix(post)) else { return false };
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) && digits.parse::<u32>().is_ok_and(|n| n >= 1 && (v.max == 0 || n <= v.max))
}

/// The largest source file hashed for a memo key.
const MAX_SOURCE: u64 = 1 << 30;

/// Write an uploaded source's bytes to a file the node can open: `{store}/.sources/<hex>/<name>`,
/// once per handle. Returns the handle and the path.
fn materialize_source(store: Option<&plumcraft_store::Store>, src: &Value) -> Result<(String, String), NodeError> {
    let missing = |why: &str| NodeError::new(NodeError::CONFIG, format!("source: {why}"));
    let handle: plumcraft_proto::handle::Handle =
        src.get("handle").and_then(Value::as_str).and_then(|h| h.parse().ok()).ok_or_else(|| missing("no valid handle"))?;
    let store = store.ok_or_else(|| missing("this engine has no store"))?;
    let blob = store.read_handle(&handle).ok_or_else(|| missing(&format!("{handle} was not uploaded")))?;
    // Only the last path component of the client's name, and never `..`: the name is a label.
    let name = src
        .get("name")
        .and_then(Value::as_str)
        .and_then(|n| n.rsplit(['/', '\\']).next())
        .filter(|n| !n.is_empty() && *n != "." && *n != "..")
        .unwrap_or("source");
    let dir = store.root().join(".sources").join(handle.digest());
    let path = dir.join(name);
    if !path.is_file() {
        std::fs::create_dir_all(&dir).map_err(|e| NodeError::new(NodeError::OP, format!("source: {e}")))?;
        // unique per call: two runs in this process may write the same source at once
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let tmp = dir.join(format!(".{name}.{}-{}.tmp", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::write(&tmp, &blob.bytes).and_then(|()| plumcraft_store::rename_addressed(&tmp, &path)).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            NodeError::new(NodeError::OP, format!("source: {e}"))
        })?;
    }
    Ok((handle.to_string(), path.display().to_string()))
}

/// A file's content handle (`md5:raw:`), or `None` when it can't be read (the node then fails
/// reading it, and its key falls back to the path).
fn source_identity(path: &str) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_SOURCE {
        return None;
    }
    std::fs::read(path).ok().map(|b| plumcraft_proto::handle::Handle::md5_raw(&b).to_string())
}

/// A thumbnail for a value of a wire type: an excerpt for text, peaks for audio, a blob's own
/// `rgba8` thumb.
fn thumb(engine: &Engine, ty: &str, v: &Data, plane: &Plane<'_>) -> Option<Rendition> {
    if let Data::Blob(b) = v {
        return plane.blob_thumb(b);
    }
    if let Data::List(l) = v {
        return l.items.first().and_then(|first| thumb(engine, ty, first, plane));
    }
    if let Data::Array(a) = v {
        return crate::arrays::thumb(a, THUMB_PX).and_then(|t| plane.rgba8(&t, "", ""));
    }
    match engine.wire_type(ty).map(|w| w.media) {
        Some(Media::Text) => v.as_str().map(excerpt),
        Some(Media::Audio) => v.audio().and_then(data::peaks),
        _ => None,
    }
}

/// A node thumbnail's long side (spec F47: `node_px`).
pub(crate) const THUMB_PX: u32 = 160;

fn excerpt(s: &str) -> Rendition {
    Rendition::Excerpt { text: s.chars().take(EXCERPT_CHARS).collect(), offset: 0, total_chars: s.chars().count() as u64, lines: s.lines().count() as u64 }
}

/// A watched value: text as an excerpt (plus `full` when it is too long to send inline), a
/// table inline or by `full`, an array always by `full`, a scalar as `value`. A list shows the
/// item `index` asks for (negative from the end), with `items` and a sheet line per item (its
/// label and summary; thumbs when `sheet` is asked for).
fn display(engine: &Engine, def: &NodeDef, id: &str, w: &WatchRequest, outputs: &Outputs, plane: &Plane<'_>) -> Display {
    let ty = def.output(&w.port).map(|p| p.ty.clone()).unwrap_or_default();
    let mut media = engine.wire_type(&ty).map(|t| t.media).unwrap_or_default();
    // a port of any value: the media of the value itself (a list: its first item's)
    if ty == crate::ANY
        && let Some(v) = outputs.get(&w.port)
    {
        media = value_media(v.list().and_then(|l| l.items.first()).unwrap_or(v));
    }
    let mut d = Display { media, ty: ty.clone(), node: id.into(), port: w.port.clone(), items: 1, view: w.view.clone(), ..Default::default() };
    match outputs.get(&w.port) {
        None => d.error = Some(NodeError::new(NodeError::OP, format!("no output {:?}", w.port))),
        Some(Data::List(l)) => {
            let n = l.items.len();
            d.items = n as u64;
            let at = if w.index < 0 { n as i64 + w.index } else { w.index };
            let at = usize::try_from(at.max(0)).unwrap_or(0).min(n.saturating_sub(1));
            d.index = at as i64;
            for (j, item) in l.items.iter().take(MAX_SHEET).enumerate() {
                let mut cell = Display { media, ..Default::default() };
                summarize(item, media, &mut cell);
                let thumb = if w.sheet { thumb(engine, &ty, item, plane) } else { None };
                d.sheet.push(plumcraft_proto::display::SheetCell { label: l.label(j).to_owned(), summary: cell.summary, thumb });
            }
            match l.items.get(at) {
                Some(item) => {
                    fill(item, media, plane, w.px, &mut d);
                    let label = l.label(at);
                    let label = if label.is_empty() { String::new() } else { format!(" ({label})") };
                    d.summary = format!("item {} of {n}{label}: {}", at + 1, d.summary);
                }
                None => d.summary = "an empty list".into(),
            }
        }
        Some(v) => fill(v, media, plane, w.px, &mut d),
    }
    d
}

/// The media kind a value shows as when its wire type doesn't say (`ANY`).
fn value_media(v: &Data) -> Media {
    match v {
        Data::Json(Value::String(_)) => Media::Text,
        Data::Json(_) | Data::List(_) => Media::Scalar,
        Data::Array(_) => Media::Ndarray,
        Data::Audio(_) => Media::Audio,
        Data::Blob(b) => match &b.meta.form {
            plumcraft_proto::data::Form::Doc { .. } => Media::Doc,
            _ if b.meta.media == "image/svg+xml" => Media::Vector,
            _ if b.meta.media.starts_with("image/") => Media::Image,
            _ => Media::Bytes,
        },
    }
}

/// A one-line summary of a value.
fn summarize(v: &Data, media: Media, d: &mut Display) {
    d.summary = match v {
        Data::Json(Value::String(s)) => format!("text, {} characters, {} lines", s.chars().count(), s.lines().count()),
        Data::Json(v) if media == Media::Table => {
            let (rows, columns) = data::table_shape(v);
            format!("table, {rows} rows: {}", columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "))
        }
        Data::Json(v) => v.to_string().chars().take(200).collect(),
        Data::Array(a) => {
            let picture = crate::arrays::layout(a).is_some();
            let what = if a.table {
                "table"
            } else if picture && crate::arrays::is_integer(&a.dtype) {
                "labels"
            } else {
                "array"
            };
            let space = a.space.as_deref().map(|s| format!(" {s}")).unwrap_or_default();
            let shape = a.shape.iter().map(u64::to_string).collect::<Vec<_>>().join("×");
            format!("{what}{space} {shape} {}", a.dtype)
        }
        Data::Audio(a) => format!("audio {} Hz, {} ch, {:.2} s", a.sample_rate, a.channels, a.duration_secs()),
        Data::Blob(b) => format!("{}, {} bytes", b.meta.media, b.meta.size),
        Data::List(l) => match tuple_json(l) {
            Some(Value::Array(a)) => {
                format!("({})", a.iter().map(|v| v.as_str().map_or_else(|| short_number(v), str::to_owned)).collect::<Vec<_>>().join(", "))
            }
            _ => format!("a tuple of {}", l.items.len()),
        },
    };
}

/// A tuple (a list inside a list) of plain values as a JSON array.
fn tuple_json(l: &crate::List) -> Option<Value> {
    l.items.iter().map(|i| i.json().cloned()).collect::<Option<Vec<_>>>().map(Value::Array)
}

/// Fill a display with one (non-list) value: its summary, renditions, `full` and `value`. An
/// array gets the pictures of the view asked for (at most `px` a side) and per-channel stats.
fn fill(v: &Data, media: Media, plane: &Plane<'_>, px: u32, d: &mut Display) {
    summarize(v, media, d);
    let full = |r: Result<plumcraft_proto::data::DataRef, NodeError>, d: &mut Display| match r {
        Ok(data) => d.full = Some(data),
        Err(e) => d.error = Some(e),
    };
    match v {
        Data::Json(Value::String(s)) => {
            d.renditions.push(excerpt(s));
            if s.len() > plane.max_inline {
                full(plane.text_ref(s), d);
            }
        }
        Data::Json(v) if media == Media::Table => {
            if data::json_len(v) > plane.max_inline {
                full(plane.table_ref(v), d);
            } else {
                d.value = Some(v.clone());
            }
        }
        Data::Json(v) if media == Media::Tree && data::json_len(v) > plane.max_inline => full(plane.json_ref(v), d),
        Data::Json(v) => d.value = Some(v.clone()),
        Data::Array(a) => {
            let turn = d.view.params.get("turn").and_then(Value::as_f64).unwrap_or(1.0);
            let px = if px == 0 { 1024 } else { px.min(4096) };
            for p in crate::arrays::views(a, &d.view.id, turn) {
                d.renditions.extend(plane.rgba8(&crate::arrays::rgba8(&p, px), "view", &p.caption));
            }
            d.stats = crate::arrays::stats(a).unwrap_or_default();
            full(plane.array_ref(a), d);
        }
        Data::Audio(a) => {
            d.media = Media::Audio;
            d.renditions.extend(data::peaks(a));
            full(plane.audio_ref(a), d);
        }
        Data::Blob(b) => {
            d.renditions.extend(plane.blob_thumb(b));
            full(plane.blob_ref(b), d);
        }
        // a tuple (a list inside a list): its values when they are plain, else its first item
        Data::List(l) => match tuple_json(l) {
            Some(v) => d.value = Some(v),
            None => {
                let summary = d.summary.clone();
                if let Some(first) = l.items.first() {
                    fill(first, media, plane, px, d);
                }
                d.summary = format!("{summary}, the first: {}", d.summary);
            }
        },
    }
}
