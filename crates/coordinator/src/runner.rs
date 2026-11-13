//! Step 4 (docs/research/15-multi-engine.md, section 3.5): running a [`Plan`]. Each run gets a
//! driver thread; each fragment, once the fragments it waits for have replied, is sent to its
//! engine as an ordinary request (run id `<run>~<fragment>`), with the crossing values bound
//! into its `inputs`, by its own thread, so fragments with no path between them run at once.
//! The fragment threads forward their engine's events into the coordinator's ring under the
//! client's run id and generation; the driver merges the replies into one.
//!
//! A node whose crossing value never came (its source failed, was skipped or cancelled, or left
//! no value) is reported `skipped`, with what is downstream of it in its fragment, and isn't
//! sent. Cancel goes to every fragment in flight; fragments not yet sent are `cancelled`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use plumcraft_host::Connection;
use plumcraft_host::ring::Ring;
use plumcraft_proto::display::Display;
use plumcraft_proto::error::{ErrorCode, NodeError, ProtoError};
use plumcraft_proto::event::{Envelope, Event, NodeState, StatusRecord};
use plumcraft_proto::run::{NodeReport, Phase, RunAck, RunReply, RunRequest, RunSnapshot};
use plumcraft_proto::slice;
use plumcraft_proto::{PROTOCOL, PortValue};
use serde_json::{Value, json};

use crate::plan::{Fragment, Plan};

/// How long a fragment's engine may fail to answer `events.poll` before its fragment fails.
pub const SILENT_FOR: Duration = Duration::from_secs(30);
/// Finished runs kept for `run.get`.
const KEEP_RUNS: usize = 64;
/// Cancels for runs not seen yet, kept so a cancel that overtakes its run still applies.
const KEEP_TOMBSTONES: usize = 256;

struct Entry {
    generation: u64,
    phase: Phase,
    reply: Option<RunReply>,
    cancel: Arc<AtomicBool>,
    order: u64,
}

/// The coordinator's runs and its event ring.
#[derive(Default)]
pub struct Runs {
    pub ring: Ring,
    table: Mutex<Table>,
}

#[derive(Default)]
struct Table {
    runs: BTreeMap<String, Entry>,
    tombstones: Vec<String>,
    started: u64,
}

impl Runs {
    fn table(&self) -> std::sync::MutexGuard<'_, Table> {
        self.table.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `run.cancel`: idempotent; a cancel for a run not seen yet waits for it.
    pub fn cancel(&self, run_id: &str) {
        let mut t = self.table();
        match t.runs.get(run_id) {
            Some(e) => e.cancel.store(true, Ordering::SeqCst),
            None => {
                t.tombstones.push(run_id.to_owned());
                let excess = t.tombstones.len().saturating_sub(KEEP_TOMBSTONES);
                t.tombstones.drain(..excess);
            }
        }
    }

    /// `run.get`.
    pub fn get(&self, run_id: &str) -> Result<RunSnapshot, ProtoError> {
        let t = self.table();
        let e = t.runs.get(run_id).ok_or_else(|| ProtoError::new(ErrorCode::NotFound, format!("no run {run_id}")))?;
        Ok(RunSnapshot { v: PROTOCOL, run_id: run_id.into(), generation: e.generation, phase: e.phase, reply: e.reply.clone(), ..Default::default() })
    }

    fn set(&self, run_id: &str, phase: Phase, reply: Option<RunReply>) {
        if let Some(e) = self.table().runs.get_mut(run_id) {
            e.phase = phase;
            if reply.is_some() {
                e.reply = reply;
            }
        }
    }
}

/// `run`: accept at once; the reply arrives as a `run.reply` event and from `run.get`.
pub fn start(runs: &Arc<Runs>, req: RunRequest, plan: Result<Plan, NodeError>, engines: BTreeMap<String, Arc<dyn Connection>>) -> Result<RunAck, ProtoError> {
    if req.run_id.is_empty() {
        return Err(ProtoError::new(ErrorCode::BadParams, "run_id is required"));
    }
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut t = runs.table();
        if let Some(at) = t.tombstones.iter().position(|r| r == &req.run_id) {
            t.tombstones.remove(at);
            cancel.store(true, Ordering::SeqCst);
        }
        t.started += 1;
        let order = t.started;
        t.runs.insert(req.run_id.clone(), Entry { generation: req.generation, phase: Phase::Accepted, reply: None, cancel: cancel.clone(), order });
        // Forget the oldest finished runs.
        while t.runs.len() > KEEP_RUNS {
            let oldest = t.runs.iter().filter(|(_, e)| e.reply.is_some()).min_by_key(|(_, e)| e.order).map(|(k, _)| k.clone());
            let Some(k) = oldest else { break };
            t.runs.remove(&k);
        }
    }
    let run_id = req.run_id.clone();
    let r = runs.clone();
    std::thread::Builder::new()
        .name(format!("plumcraft-coordinator-{}", run_id.chars().take(16).collect::<String>()))
        .spawn(move || drive(&r, &req, plan, &engines, &cancel))
        .map_err(|e| ProtoError::new(ErrorCode::Internal, e.to_string()))?;
    Ok(RunAck { accepted: true, run_id, queued_behind: 0 })
}

/// What a wire between fragments delivers: `value` through its slice, if it has one (spec 7.6
/// rule 3). Applied here because a binding replaces the wire, so the engine never sees it.
fn sliced(value: &PortValue, spec: Option<&str>) -> Result<PortValue, String> {
    let Some(spec) = spec else { return Ok(value.clone()) };
    let s = slice::parse(spec)?;
    let PortValue::List { items } = value else { return Err(format!("slice {spec:?} needs a list, but the wire carries a single value")) };
    let picks = s.picks(items.len())?;
    if s.is_index() {
        return picks.first().and_then(|&i| items.get(i)).cloned().ok_or_else(|| format!("slice {spec:?} picks nothing"));
    }
    Ok(PortValue::List { items: picks.iter().filter_map(|&i| items.get(i).cloned()).collect() })
}

fn report(state: NodeState, error: Option<NodeError>) -> NodeReport {
    NodeReport { state, error, ..Default::default() }
}

fn drive(runs: &Arc<Runs>, req: &RunRequest, plan: Result<Plan, NodeError>, engines: &BTreeMap<String, Arc<dyn Connection>>, cancel: &Arc<AtomicBool>) {
    let started = Instant::now();
    let run = (req.run_id.as_str(), req.generation);
    let mut reply = RunReply { v: PROTOCOL, run_id: req.run_id.clone(), generation: req.generation, ..Default::default() };
    let plan = match plan {
        Ok(p) => p,
        Err(e) => {
            reply.error = Some(e);
            reply.watch = req.watch.iter().map(|w| no_value(&w.node, &w.port)).collect();
            return finish(runs, reply, cancel);
        }
    };
    runs.set(&req.run_id, Phase::Running, None);
    let n = plan.fragments.len();
    let mut replies: Vec<Option<RunReply>> = vec![None; n];
    let mut sent = vec![false; n];
    let mut extra: BTreeMap<String, NodeReport> = BTreeMap::new();
    let (tx, rx) = mpsc::channel::<(usize, RunReply)>();
    let mut in_flight = 0usize;
    loop {
        for (i, f) in plan.fragments.iter().enumerate() {
            if sent[i] || !f.after.iter().all(|a| replies.get(*a).is_some_and(Option::is_some)) {
                continue;
            }
            sent[i] = true;
            if cancel.load(Ordering::SeqCst) {
                for node in &f.nodes {
                    runs.ring.push_event(Some(run), Event::Status(Box::new(StatusRecord::new(node, NodeState::Cancelled))));
                    extra.insert(node.clone(), report(NodeState::Cancelled, None));
                }
                replies[i] = Some(RunReply::default());
                continue;
            }
            // The crossing values, and the nodes left without one.
            let (inputs, missing) = crossing_inputs(&plan, i, &replies);
            let skipped = downstream(f, missing.keys().cloned().collect());
            for node in f.nodes.iter().filter(|n| skipped.contains(*n)) {
                let why = missing.get(node).cloned().unwrap_or_else(|| "upstream did not finish".to_owned());
                let err = NodeError::new(NodeError::SKIPPED, why);
                let mut rec = StatusRecord::new(node, NodeState::Skipped);
                rec.error = Some(err.clone());
                runs.ring.push_event(Some(run), Event::Status(Box::new(rec)));
                extra.insert(node.clone(), report(NodeState::Skipped, Some(err)));
            }
            if skipped.len() == f.nodes.len() {
                replies[i] = Some(RunReply::default());
                continue;
            }
            let Some(conn) = engines.get(&f.engine).cloned() else {
                let err = NodeError::new(NodeError::ENGINE, format!("engine {} is not connected", f.engine));
                for node in f.nodes.iter().filter(|n| !skipped.contains(*n)) {
                    extra.insert(node.clone(), report(NodeState::Error, Some(err.clone())));
                }
                replies[i] = Some(RunReply { error: Some(err), ..Default::default() });
                continue;
            };
            let sub = request(req, f, i, &skipped, inputs, readable_roots(&plan, i, engines));
            let (tx, sub_id, frag_nodes) = (tx.clone(), format!("{}~{i}", req.run_id), f.nodes.clone());
            let client = (req.run_id.clone(), req.generation);
            let (runs, cancel) = (Arc::clone(runs), Arc::clone(cancel));
            in_flight += 1;
            let spawned = std::thread::Builder::new().name(format!("plumcraft-fragment-{i}")).spawn(move || {
                let r = fragment(conn.as_ref(), sub, &sub_id, (&client.0, client.1), &runs.ring, &cancel, &frag_nodes);
                let _ = tx.send((i, r));
            });
            if let Err(e) = spawned {
                in_flight -= 1;
                replies[i] = Some(failed(&f.nodes, &format!("couldn't start: {e}")));
            }
        }
        if in_flight == 0 {
            break;
        }
        let Ok((i, r)) = rx.recv() else { break };
        in_flight -= 1;
        if let Some(slot) = replies.get_mut(i) {
            *slot = Some(r);
        }
    }

    release_held(&plan, &replies, engines);

    // One reply from the fragments'.
    let replied: Vec<&RunReply> = replies.iter().flatten().collect();
    for r in &replied {
        for (k, v) in &r.nodes {
            reply.nodes.insert(k.clone(), v.clone());
        }
    }
    for (k, v) in extra {
        reply.nodes.entry(k).or_insert(v);
    }
    // Each engine's share of the caches, and the run's.
    for (f, r) in plan.fragments.iter().zip(&replies) {
        if let Some(s) = r.as_ref().and_then(|r| r.stats.as_ref()) {
            reply.engine_stats.entry(f.engine.clone()).or_default().add(s);
            reply.stats.get_or_insert_with(Default::default).add(s);
        }
    }
    reply.order = plan.order.iter().filter(|n| reply.nodes.contains_key(*n)).cloned().collect();
    reply.watch = req
        .watch
        .iter()
        .enumerate()
        .map(|(k, w)| {
            plan.fragments
                .iter()
                .zip(&replies)
                .find_map(|(f, r)| {
                    let j = f.watch.iter().position(|(idx, _)| *idx == k)?;
                    r.as_ref()?.watch.get(j).cloned()
                })
                .unwrap_or_else(|| no_value(&w.node, &w.port))
        })
        .collect();
    reply.error = replied.iter().find_map(|r| r.error.clone().filter(|e| e.kind != NodeError::CANCELLED));
    if cancel.load(Ordering::SeqCst) {
        reply.error = Some(NodeError::cancelled());
    }
    reply.ok = reply.error.is_none() && !reply.nodes.is_empty() && reply.nodes.values().all(|n| matches!(n.state, NodeState::Done | NodeState::Cached));
    reply.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    finish(runs, reply, cancel);
}

fn finish(runs: &Runs, reply: RunReply, cancel: &AtomicBool) {
    let phase = if cancel.load(Ordering::SeqCst) {
        Phase::Cancelled
    } else if reply.ok {
        Phase::Finished
    } else {
        Phase::Failed
    };
    let run = (reply.run_id.clone(), reply.generation);
    runs.set(&run.0, phase, Some(reply.clone()));
    runs.ring.push_event(Some((&run.0, run.1)), Event::RunReply(Box::new(reply)));
}

fn no_value(node: &str, port: &str) -> Display {
    Display { node: node.into(), port: port.into(), error: Some(NodeError::new(NodeError::OP, "no value")), ..Default::default() }
}

/// `nodes` and everything downstream of them inside fragment `f`.
fn downstream(f: &Fragment, mut nodes: BTreeSet<String>) -> BTreeSet<String> {
    let wires: Vec<(String, String)> = f
        .graph
        .get("wires")
        .and_then(Value::as_array)
        .map(|ws| ws.iter().filter_map(|w| Some((w["from"]["node"].as_str()?.to_owned(), w["to"]["node"].as_str()?.to_owned()))).collect())
        .unwrap_or_default();
    loop {
        let more: Vec<String> = wires.iter().filter(|(a, b)| nodes.contains(a) && !nodes.contains(b)).map(|(_, b)| b.clone()).collect();
        if more.is_empty() {
            return nodes;
        }
        nodes.extend(more);
    }
}

/// Fragment `i`'s request, without the nodes in `skipped`.
fn request(req: &RunRequest, f: &Fragment, i: usize, skipped: &BTreeSet<String>, inputs: BTreeMap<String, PortValue>, trusted: Vec<String>) -> Value {
    let mut graph = f.graph.clone();
    if let Some(ns) = graph.get_mut("nodes").and_then(Value::as_object_mut) {
        ns.retain(|k, _| !skipped.contains(k));
    }
    if let Some(ws) = graph.get_mut("wires").and_then(Value::as_array_mut) {
        ws.retain(|w| [&w["from"]["node"], &w["to"]["node"]].iter().all(|n| n.as_str().is_none_or(|n| !skipped.contains(n))));
    }
    let keep = |n: &str| !skipped.contains(n);
    let targets: Vec<&String> = f.targets.iter().filter(|t| keep(t)).collect();
    // Every entry, skipped nodes' too (they come back "no value"), so the reply's positions
    // match the fragment's.
    let watch: Vec<&plumcraft_proto::display::WatchRequest> = f.watch.iter().map(|(_, w)| w).collect();
    let inputs: BTreeMap<&String, &PortValue> = inputs.iter().filter(|(k, _)| k.rsplit_once('.').is_some_and(|(n, _)| keep(n))).collect();
    json!({
        "v": PROTOCOL, "op": "run",
        "run_id": format!("{}~{i}", req.run_id), "generation": req.generation, "priority": req.priority,
        "graph": {"kind": "inline", "graph": graph},
        "targets": targets, "watch": watch, "preview": req.preview, "memo": req.memo, "inputs": inputs, "hold": f.hold, "trusted_roots": trusted, "scope": req.scope,
    })
}

/// Where the values crossing into fragment `i` may lie. The coordinator says so, having heard it
/// from the engines that made them: an engine never takes a folder from a value.
pub fn readable_roots(plan: &Plan, i: usize, engines: &BTreeMap<String, Arc<dyn Connection>>) -> Vec<String> {
    let from: BTreeSet<&String> =
        plan.crossings.iter().filter(|c| c.to_fragment == i).filter_map(|c| plan.fragments.get(c.from_fragment).map(|f| &f.engine)).collect();
    let roots: BTreeSet<String> = from.into_iter().filter_map(|e| engines.get(e)).flat_map(|c| c.readable_roots()).collect();
    roots.into_iter().collect()
}

/// What fragment `i` gets from the fragments before it, and the nodes left without a value.
fn crossing_inputs(plan: &Plan, i: usize, replies: &[Option<RunReply>]) -> (BTreeMap<String, PortValue>, BTreeMap<String, String>) {
    let mut inputs = plan.fragments.get(i).map(|f| f.inputs.clone()).unwrap_or_default();
    let mut missing: BTreeMap<String, String> = BTreeMap::new();
    for c in plan.crossings.iter().filter(|c| c.to_fragment == i) {
        let from = replies.get(c.from_fragment).and_then(Option::as_ref).and_then(|r| r.nodes.get(&c.from.0));
        let value = from.filter(|n| matches!(n.state, NodeState::Done | NodeState::Cached)).and_then(|n| n.outputs.get(&c.from.1));
        match value {
            Some(PortValue::Opaque { ty, summary, held: None }) => {
                missing.entry(c.to.0.clone()).or_insert_with(|| format!("input {:?}: a {ty} from {} can't leave its engine ({summary})", c.to.1, c.from.0));
            }
            Some(v) => match sliced(v, c.slice.as_deref()) {
                Ok(v) => {
                    inputs.insert(format!("{}.{}", c.to.0, c.to.1), v);
                }
                Err(why) => {
                    missing.entry(c.to.0.clone()).or_insert_with(|| format!("input {:?}: {why}", c.to.1));
                }
            },
            None => {
                missing.entry(c.to.0.clone()).or_insert_with(|| format!("upstream {} did not finish", c.from.0));
            }
        }
    }
    (inputs, missing)
}

/// The run is over: what its engines held for it can go.
fn release_held(plan: &Plan, replies: &[Option<RunReply>], engines: &BTreeMap<String, Arc<dyn Connection>>) {
    let mut held: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for c in plan.crossings.iter().filter(|c| plan.fragments.get(c.from_fragment).map(|f| &f.engine) == plan.fragments.get(c.to_fragment).map(|f| &f.engine)) {
        let reply = replies.get(c.from_fragment).and_then(Option::as_ref);
        if let (Some(f), Some(PortValue::Opaque { held: Some(id), .. })) =
            (plan.fragments.get(c.from_fragment), reply.and_then(|r| r.nodes.get(&c.from.0)).and_then(|n| n.outputs.get(&c.from.1)))
        {
            held.entry(&f.engine).or_default().push(id);
        }
    }
    for (engine, ids) in held {
        if let Some(conn) = engines.get(engine) {
            let _ = conn.call("values.release", json!({"ids": ids}));
        }
    }
}

/// Everything a save needs from the other engines: every fragment but `last`, in turn, ending
/// with the bindings for `last`. The caller ends it with [`SavePrep::finish`], once the save is
/// done. A fragment that fails stops it, and what was held is released.
pub fn prepare_save(req: &RunRequest, plan: Plan, last: usize, engines: BTreeMap<String, Arc<dyn Connection>>) -> Result<SavePrep, NodeError> {
    let n = plan.fragments.len();
    let mut prep = SavePrep { replies: vec![None; n], inputs: BTreeMap::new(), trusted_roots: Vec::new(), plan, engines };
    let (ring, cancel) = (Ring::default(), AtomicBool::new(false));
    let client = (req.run_id.as_str(), req.generation);
    loop {
        let next = (0..n).find(|&i| {
            i != last
                && prep.replies.get(i).is_some_and(Option::is_none)
                && prep.plan.fragments.get(i).is_some_and(|f| f.after.iter().all(|a| prep.replies.get(*a).is_some_and(Option::is_some)))
        });
        let Some(i) = next else { break };
        let (inputs, missing) = crossing_inputs(&prep.plan, i, &prep.replies);
        if let Some((node, why)) = missing.into_iter().next() {
            prep.finish();
            return Err(NodeError::new(NodeError::SKIPPED, format!("{node}: {why}")));
        }
        let Some((f, conn)) = prep.plan.fragments.get(i).and_then(|f| Some((f, prep.engines.get(&f.engine)?.clone()))) else {
            prep.finish();
            return Err(NodeError::engine("an engine this graph needs is not connected"));
        };
        let sub = request(req, f, i, &BTreeSet::new(), inputs, readable_roots(&prep.plan, i, &prep.engines));
        let r = fragment(conn.as_ref(), sub, &format!("{}~{i}", req.run_id), client, &ring, &cancel, &f.nodes);
        let failed = r.error.clone().or_else(|| r.nodes.values().find_map(|n| n.error.clone()));
        if let Some(slot) = prep.replies.get_mut(i) {
            *slot = Some(r);
        }
        if let Some(e) = failed {
            prep.finish();
            return Err(e);
        }
    }
    let (inputs, missing) = crossing_inputs(&prep.plan, last, &prep.replies);
    if let Some((node, why)) = missing.into_iter().next() {
        prep.finish();
        return Err(NodeError::new(NodeError::SKIPPED, format!("{node}: {why}")));
    }
    prep.inputs = inputs;
    prep.trusted_roots = readable_roots(&prep.plan, last, &prep.engines);
    Ok(prep)
}

/// The other engines' share of a save, done: what crosses into the last, and what they hold.
pub struct SavePrep {
    pub inputs: BTreeMap<String, PortValue>,
    pub trusted_roots: Vec<String>,
    pub plan: Plan,
    replies: Vec<Option<RunReply>>,
    engines: BTreeMap<String, Arc<dyn Connection>>,
}

impl SavePrep {
    /// The save is over: release what was held for it.
    pub fn finish(&self) {
        release_held(&self.plan, &self.replies, &self.engines);
    }
}

/// A fragment's failure, as the engine would have replied (spec 12.3): every node `error`.
fn failed(nodes: &[String], why: &str) -> RunReply {
    let err = NodeError::new(NodeError::ENGINE, why);
    RunReply { error: Some(err.clone()), nodes: nodes.iter().map(|n| (n.clone(), report(NodeState::Error, Some(err.clone())))).collect(), ..Default::default() }
}

/// Send one fragment and follow it to its reply, forwarding its events under the client's run.
fn fragment(conn: &dyn Connection, sub: Value, sub_id: &str, client: (&str, u64), ring: &Ring, cancel: &AtomicBool, nodes: &[String]) -> RunReply {
    if let Err(e) = conn.call("run", sub) {
        return failed(nodes, &format!("the engine refused the run: {}", e.message));
    }
    let mut since = 0u64;
    let mut cancel_sent = false;
    let mut silent_since: Option<Instant> = None;
    loop {
        if !cancel_sent && cancel.load(Ordering::SeqCst) {
            let _ = conn.call("run.cancel", json!({"run_id": sub_id}));
            cancel_sent = true;
        }
        let polled = conn.call("events.poll", json!({"since": since, "wait_ms": 200, "run_id": sub_id}));
        let Ok(p) = polled else {
            let at = *silent_since.get_or_insert_with(Instant::now);
            if at.elapsed() > SILENT_FOR {
                return failed(nodes, "the engine stopped answering");
            }
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        silent_since = None;
        since = p.get("last_seq").and_then(Value::as_u64).unwrap_or(since);
        let events: Vec<Envelope> = p.get("events").cloned().and_then(|e| serde_json::from_value(e).ok()).unwrap_or_default();
        for e in events {
            match e.event {
                Event::RunReply(r) if r.run_id == sub_id => return *r,
                // Older events were dropped: the reply may be among them; ask for it.
                Event::Resync(_) => {
                    let snap = conn.call("run.get", json!({"run_id": sub_id})).ok().and_then(|v| serde_json::from_value::<RunSnapshot>(v).ok());
                    if let Some(r) = snap.and_then(|s| s.reply) {
                        return r;
                    }
                }
                Event::RunReply(_) | Event::EngineState(_) | Event::SchemaChanged { .. } => {}
                other => ring.push(Envelope::new(0, e.ts, Some(client), other)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(n: i64) -> PortValue {
        PortValue::List { items: (0..n).map(|v| PortValue::Json { value: json!(v) }).collect() }
    }

    #[test]
    fn a_crossing_is_sliced_as_an_engine_would() {
        assert_eq!(sliced(&list(3), None).unwrap(), list(3));
        assert_eq!(sliced(&list(4), Some(":3")).unwrap(), list(3));
        assert_eq!(sliced(&list(3), Some("-1")).unwrap(), PortValue::Json { value: json!(2) });
        assert!(sliced(&list(3), Some("9")).unwrap_err().contains("out of range"));
        assert!(sliced(&PortValue::Json { value: json!(1) }, Some("0")).unwrap_err().contains("needs a list"));
        assert!(sliced(&list(3), Some("junk")).is_err());
    }
}
