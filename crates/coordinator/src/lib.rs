//! The coordinator (`docs/research/15-multi-engine.md`): one engine made of several. It is a
//! [`Connection`] over sub-engine [`Connection`]s, so every client (the editor, `plumcraft run`,
//! the viewer, MCP) drives several engines exactly as it drives one, and never knows which it has.
//!
//! - **One sub-engine: a pass-through.** Every call, the state and every blob read go straight
//!   to it, unchanged: the same catalogue, the same requests, the same events
//!   (`crates/text/tests/conformance.rs` runs every case through a coordinator to hold it to that).
//! - **Several: the coordinator is the engine.** `schema` is the merged catalogue ([`merge`]);
//!   `run` splits the graph into fragments joined by bindings ([`plan`]) and runs them on their
//!   engines ([`runner`]); `output.save` goes to the engine that computes the node;
//!   `run.cancel`, `run.get` and `events.poll` answer from its own run table and event ring; `hello` and `session.confirm` go to every engine and answer as the first,
//!   under the coordinator's identity. A blob comes from whichever engine has it.
//! - **None:** every call is `unavailable`.
//!
//! Switching between one and several restarts the client's event numbering (the coordinator's
//! ring replaces the engine's): a client resumes from 0, as after an engine restart.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod merge;
pub mod plan;
pub mod runner;

/// Saves so far: each runs its fragments under a run id of its own.
static SAVES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

use plumcraft_host::Connection;
use plumcraft_proto::PROTOCOL;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::data::DataRef;
use plumcraft_proto::error::{ErrorCode, NodeError, ProtoError};
use plumcraft_proto::run::{GraphRef, RunRequest, SaveReply, SaveRequest};
use plumcraft_proto::session::EngineState;
use plumcraft_store::Blob;
use serde_json::{Value, json};

use crate::merge::Merged;
use crate::runner::Runs;

/// A sub-engine: its id (the engine's catalogue id, `text`, `array`, `vectorcraft`) and its
/// connection.
#[derive(Clone)]
pub struct SubEngine {
    pub id: String,
    pub conn: Arc<dyn Connection>,
}

/// One engine made of several, in the order they were added.
#[derive(Default)]
pub struct Coordinator {
    engines: RwLock<Vec<SubEngine>>,
    /// The merged catalogue and the parts it came from; rebuilt when the engines change or a
    /// client asks for `schema`.
    merged: Mutex<Option<(Merged, BTreeMap<String, SchemaReply>)>>,
    runs: Arc<Runs>,
}

impl Coordinator {
    pub fn new() -> Self {
        Self::default()
    }

    /// With `conn` added as `id`.
    pub fn with_engine(self, id: impl Into<String>, conn: Arc<dyn Connection>) -> Self {
        self.add(id, conn);
        self
    }

    /// Add a sub-engine; one with the same id is replaced in place (a reconnect keeps its order).
    pub fn add(&self, id: impl Into<String>, conn: Arc<dyn Connection>) {
        let id = id.into();
        self.forget_catalogue();
        let mut v = self.write();
        match v.iter_mut().find(|e| e.id == id) {
            Some(e) => e.conn = conn,
            None => v.push(SubEngine { id, conn }),
        }
    }

    /// Remove a sub-engine, returning it.
    pub fn remove(&self, id: &str) -> Option<SubEngine> {
        self.forget_catalogue();
        let mut v = self.write();
        let at = v.iter().position(|e| e.id == id)?;
        Some(v.remove(at))
    }

    /// The sub-engines, in order.
    pub fn engines(&self) -> Vec<SubEngine> {
        self.read().clone()
    }

    /// Each sub-engine's id and state, in order.
    pub fn states(&self) -> Vec<(String, EngineState)> {
        self.read().iter().map(|e| (e.id.clone(), e.conn.state())).collect()
    }

    /// The one sub-engine, when there is exactly one.
    fn only(&self) -> Option<Arc<dyn Connection>> {
        match self.read().as_slice() {
            [one] => Some(one.conn.clone()),
            _ => None,
        }
    }

    fn forget_catalogue(&self) {
        *self.merged.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// The merged catalogue (fetched from every engine when there is none, or `fresh`).
    pub fn catalogue(&self, fresh: bool) -> Result<(Merged, BTreeMap<String, SchemaReply>), ProtoError> {
        let mut cached = self.merged.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(c) = cached.as_ref().filter(|_| !fresh) {
            return Ok(c.clone());
        }
        let mut parts = Vec::new();
        for e in self.engines() {
            let v = e.conn.call("schema", json!({})).map_err(|err| ProtoError::new(err.code, format!("engine {}: {}", e.id, err.message)))?;
            let s: SchemaReply =
                serde_json::from_value(v).map_err(|err| ProtoError::new(ErrorCode::Internal, format!("engine {}'s catalogue: {err}", e.id)))?;
            parts.push((e.id, s));
        }
        let merged = merge::catalogue(&parts);
        let c = (merged, parts.into_iter().collect());
        *cached = Some(c.clone());
        Ok(c)
    }

    /// What merging the catalogues found wrong (a wire type two engines declare differently);
    /// empty with one engine, or when the catalogues can't be fetched.
    pub fn warnings(&self) -> Vec<String> {
        if self.read().len() < 2 {
            return Vec::new();
        }
        self.catalogue(false).map(|(m, _)| m.warnings).unwrap_or_default()
    }

    /// A call when several engines are connected: the coordinator is the engine.
    fn call_several(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        let to_json = |v: Result<Value, serde_json::Error>| v.map_err(|e| ProtoError::new(ErrorCode::Internal, e.to_string()));
        let s = |k: &str| params.get(k).and_then(Value::as_str).map(str::to_owned);
        let n = |k: &str| params.get(k).and_then(Value::as_u64).unwrap_or(0);
        match method {
            "hello" | "session.confirm" => {
                let mut first = None;
                for e in self.engines() {
                    let r = e.conn.call(method, params.clone()).map_err(|err| ProtoError::new(err.code, format!("engine {}: {}", e.id, err.message)))?;
                    first.get_or_insert(r);
                }
                let mut r = first.unwrap_or_else(|| json!({}));
                if method == "hello"
                    && let Some(o) = r.as_object_mut()
                {
                    o.insert("engine".into(), to_json(serde_json::to_value(&self.catalogue(false)?.0.schema.engine))?);
                }
                Ok(r)
            }
            "ping" => Ok(json!({"v": PROTOCOL, "pong": true})),
            // A pack operation (spec 15) goes to the engine `params.engine` names, else to the
            // first that advertises it.
            m if m.starts_with("x.") => {
                let engines = self.engines();
                let named = s("engine").and_then(|id| engines.iter().find(|e| e.id == id).cloned());
                let target = named.or_else(|| {
                    let states = self.states();
                    engines
                        .iter()
                        .find(|e| states.iter().any(|(id, st)| id == &e.id && matches!(st, EngineState::Connected { caps, .. } if caps.has(m))))
                        .cloned()
                });
                match target {
                    Some(e) => e.conn.call(method, params.clone()),
                    None => Err(ProtoError::new(ErrorCode::UnknownOp, format!("no engine offers {:?}", m.chars().take(64).collect::<String>()))),
                }
            }
            // every engine may hold something of this graph; one without the capability refuses
            "graph.release" => {
                for e in self.engines() {
                    let _ = e.conn.call(method, params.clone());
                }
                Ok(json!({}))
            }
            "schema" => {
                let (m, _) = self.catalogue(true)?;
                if s("digest").is_some_and(|d| !d.is_empty() && d == m.schema.engine.schema_digest) {
                    return Ok(json!({"unchanged": true}));
                }
                to_json(serde_json::to_value(&m.schema))
            }
            "run" => {
                let req: RunRequest = serde_json::from_value(params).map_err(|e| ProtoError::new(ErrorCode::BadParams, e.to_string()))?;
                let (merged, parts) = self.catalogue(false)?;
                let plan = plan::build(&req, &merged, &parts);
                let engines = self.engines().into_iter().map(|e| (e.id, e.conn)).collect();
                to_json(serde_json::to_value(runner::start(&self.runs, req, plan, engines)?))
            }
            "output.save" => {
                let req: SaveRequest = serde_json::from_value(params).map_err(|e| ProtoError::new(ErrorCode::BadParams, e.to_string()))?;
                to_json(serde_json::to_value(self.save(req)?))
            }
            "run.cancel" => {
                self.runs.cancel(&s("run_id").unwrap_or_default());
                Ok(json!({}))
            }
            "run.get" => to_json(serde_json::to_value(self.runs.get(&s("run_id").unwrap_or_default())?)),
            "events.poll" => to_json(serde_json::to_value(self.runs.ring.poll(n("since"), n("wait_ms"), s("run_id").as_deref()))),
            other => Err(ProtoError::new(ErrorCode::UnknownOp, format!("unknown op {:?}", other.chars().take(64).collect::<String>()))),
        }
    }

    /// `output.save` over several engines. The engine that computes the node saves it, given its
    /// share of the graph, as a run would send it. What that share needs from other engines is
    /// computed first and bound in, and the recipe holds the whole graph, since that is what
    /// remakes the file (spec 15).
    fn save(&self, mut req: SaveRequest) -> Result<SaveReply, ProtoError> {
        let (merged, parts) = self.catalogue(false)?;
        let whole = match &req.graph {
            GraphRef::Inline { graph } => graph.clone(),
            GraphRef::Handle { .. } => Value::Null,
        };
        let id = SAVES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let run =
            RunRequest { run_id: format!("save-{}-{id}", std::process::id()), graph: req.graph.clone(), targets: vec![req.node.clone()], ..Default::default() };
        let plan = match plan::build(&run, &merged, &parts) {
            Ok(p) => p,
            Err(e) => return Ok(SaveReply::failed(e)),
        };
        let Some(last) = plan.fragments.iter().position(|f| f.nodes.contains(&req.node)) else {
            return Ok(SaveReply::failed(NodeError::new(NodeError::CONFIG, format!("{} isn't in the graph", req.node))));
        };
        let (engine, share, several) = match plan.fragments.get(last) {
            Some(f) => (f.engine.clone(), f.graph.clone(), plan.fragments.len() > 1),
            None => return Ok(SaveReply::failed(NodeError::new(NodeError::CONFIG, format!("{} isn't in the graph", req.node)))),
        };
        if parts.get(&engine).is_none_or(|c| c.save_formats.is_empty()) {
            return Ok(SaveReply::failed(NodeError::new(NodeError::CONFIG, format!("engine {engine} doesn't save files"))));
        }
        let engines: BTreeMap<String, Arc<dyn Connection>> = self.engines().into_iter().map(|e| (e.id, e.conn)).collect();
        let Some(conn) = engines.get(&engine).cloned() else {
            return Ok(SaveReply::failed(NodeError::engine(format!("engine {engine} is not connected"))));
        };
        req.graph = GraphRef::Inline { graph: share };
        let prep = if several {
            match runner::prepare_save(&run, plan, last, engines) {
                Ok(p) => {
                    req.inputs = p.inputs.clone();
                    req.trusted_roots = p.trusted_roots.clone();
                    req.recipe_graph = Some(with_engines(whole, &merged));
                    Some(p)
                }
                Err(e) => return Ok(SaveReply::failed(e)),
            }
        } else {
            None
        };
        let body = serde_json::to_value(&req).map_err(|e| ProtoError::new(ErrorCode::Internal, e.to_string()))?;
        let reply = conn.call("output.save", body);
        if let Some(p) = prep {
            p.finish();
        }
        serde_json::from_value(reply?).map_err(|e| ProtoError::new(ErrorCode::Internal, format!("engine {engine}'s save reply: {e}")))
    }

    // A poisoned lock still holds a usable list: a panic elsewhere never stops the coordinator.
    fn read(&self) -> RwLockReadGuard<'_, Vec<SubEngine>> {
        self.engines.read().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Vec<SubEngine>> {
        self.engines.write().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// `graph` as a recipe holds it: naming the engines it needs in its `engines` (spec 8.2), its own
/// list first, so that a picture opened where one is missing can say which.
fn with_engines(mut graph: Value, merged: &merge::Merged) -> Value {
    let mut engines: Vec<String> = graph.get("engines").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    let nodes = graph.get("nodes").and_then(Value::as_object).into_iter().flatten();
    for node in nodes.map(|(_, n)| n) {
        let pinned = node.get("engine").and_then(Value::as_str);
        let kind = node.get("kind").and_then(Value::as_str).unwrap_or_default();
        if let Ok(e) = merge::owner(kind, pinned, &engines, &merged.owners)
            && !engines.contains(&e)
        {
            engines.push(e);
        }
    }
    if let Some(g) = graph.as_object_mut() {
        g.insert("engines".into(), json!(engines));
    }
    graph
}

impl Connection for Coordinator {
    fn uploads_pending(&self, params: &Value) -> Vec<(String, String)> {
        self.read().iter().flat_map(|e| e.conn.uploads_pending(params)).collect()
    }

    fn allow_uploads(&self, files: &[String]) {
        for e in self.read().iter() {
            e.conn.allow_uploads(files);
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        if let Some(one) = self.only() {
            return one.call(method, params);
        }
        if self.read().is_empty() {
            return Err(ProtoError::new(ErrorCode::Unavailable, "no engine is connected"));
        }
        self.call_several(method, params)
    }

    /// The one sub-engine's state. With none, `off`. With several, the first that isn't
    /// connected; else connected, with the merged catalogue's node count (the engines' total
    /// before it is fetched) and the coordinator's version. Never fetches anything itself.
    fn state(&self) -> EngineState {
        let states = self.states();
        if states.len() < 2 {
            return states.into_iter().next().map(|(_, s)| s).unwrap_or_default();
        }
        if let Some((_, bad)) = states.iter().find(|(_, s)| !matches!(s, EngineState::Connected { .. })) {
            return bad.clone();
        }
        let merged = self.merged.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().map(|(m, _)| m.schema.nodes.len() as u64);
        let total: u64 = states.iter().map(|(_, s)| if let EngineState::Connected { nodes, .. } = s { *nodes } else { 0 }).sum();
        let caps = states.into_iter().find_map(|(_, s)| if let EngineState::Connected { caps, .. } = s { Some(caps) } else { None }).unwrap_or_default();
        EngineState::Connected { version: env!("CARGO_PKG_VERSION").into(), nodes: merged.unwrap_or(total), caps }
    }

    /// The first sub-engine that has the blob.
    fn resolve(&self, r: &DataRef) -> Option<Blob> {
        self.engines().iter().find_map(|e| e.conn.resolve(r))
    }
}
