//! Step 4 (docs/research/15-multi-engine.md, section 3.5): a run plan. One request for a graph
//! that spans engines becomes fragments, each an ordinary request to one engine; a wire between
//! fragments becomes a binding (`RunRequest.inputs`, spec 7.6 rule 10) filled from the upstream
//! fragment's reply. Pure: the request and the catalogues in, the plan out.
//!
//! **Fragments** are (engine, depth) pairs. A node's depth is the largest of its inputs' depths,
//! one more where an input comes from another engine; so every wire goes to a fragment at the
//! same depth (the same engine: inside it) or a deeper one, and the fragments form no cycle even
//! when the engines do (A → B → A gives three fragments).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use plumcraft_proto::PortValue;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::display::WatchRequest;
use plumcraft_proto::error::NodeError;
use plumcraft_proto::run::{GraphRef, RunRequest};
use serde_json::{Map, Value};

use crate::merge::{self, Merged};

/// A port: (node, port).
pub type PortAt = (String, String);

/// A wire between fragments.
#[derive(Debug, Clone, PartialEq)]
pub struct Crossing {
    pub from: PortAt,
    pub to: PortAt,
    /// The wire's slice, which the coordinator applies: a binding replaces the wire.
    pub slice: Option<String>,
    pub from_fragment: usize,
    pub to_fragment: usize,
}

/// One engine's share of a run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fragment {
    pub engine: String,
    /// Its nodes, in the run's order.
    pub nodes: Vec<String>,
    /// `node.port`s a later fragment of the same engine reads: the engine may hold them.
    pub hold: Vec<String>,
    /// The workflow it sends: its nodes and the wires between them.
    pub graph: Value,
    /// The run's targets among its nodes, and every node whose value another fragment needs.
    pub targets: Vec<String>,
    /// The run's watch entries on its nodes: (index in the run's request, entry).
    pub watch: Vec<(usize, WatchRequest)>,
    /// The run's own bindings on its nodes.
    pub inputs: BTreeMap<String, PortValue>,
    /// The fragments it waits for.
    pub after: BTreeSet<usize>,
}

/// A run, split.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    /// In an order where each comes after those it waits for.
    pub fragments: Vec<Fragment>,
    pub crossings: Vec<Crossing>,
    /// Every node the run computes, in order.
    pub order: Vec<String>,
}

#[derive(Debug, Clone)]
struct Wire {
    from: PortAt,
    to: PortAt,
    slice: Option<String>,
}

fn graph_error(message: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::GRAPH, message)
}

fn port_at(v: &Value) -> Option<PortAt> {
    Some((v.get("node")?.as_str()?.to_owned(), v.get("port")?.as_str()?.to_owned()))
}

/// Split `req` over the engines whose catalogues are `catalogues`, merged as `merged`.
pub fn build(req: &RunRequest, merged: &Merged, catalogues: &BTreeMap<String, SchemaReply>) -> Result<Plan, NodeError> {
    let GraphRef::Inline { graph } = &req.graph else {
        return Err(graph_error("graphs by handle aren't supported by the coordinator"));
    };
    let nodes = graph.get("nodes").and_then(Value::as_object).ok_or_else(|| graph_error("malformed workflow: no nodes"))?;
    let preferred: Vec<String> =
        graph.get("engines").and_then(Value::as_array).map(|a| a.iter().filter_map(|e| e.as_str().map(str::to_owned)).collect()).unwrap_or_default();

    // Which engine runs each node.
    let mut engine_of: BTreeMap<String, String> = BTreeMap::new();
    for (id, n) in nodes {
        let kind = n.get("kind").and_then(Value::as_str).unwrap_or_default();
        let pinned = n.get("engine").and_then(Value::as_str);
        let e = merge::owner(kind, pinned, &preferred, &merged.owners).map_err(|why| graph_error(format!("node {id}: {why}")))?;
        engine_of.insert(id.clone(), e);
    }

    // The wires, less those into ports the run binds itself (spec 7.6 rule 10: dropped).
    let mut wires = Vec::new();
    for w in graph.get("wires").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
        let (Some(from), Some(to)) = (w.get("from").and_then(port_at), w.get("to").and_then(port_at)) else {
            return Err(graph_error("malformed workflow: a wire without both ends"));
        };
        for end in [&from.0, &to.0] {
            if !nodes.contains_key(end) {
                return Err(graph_error(format!("wire names unknown node {end:?}")));
            }
        }
        if !req.inputs.contains_key(&format!("{}.{}", to.0, to.1)) {
            let slice = w.get("slice").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
            wires.push(Wire { from, to, slice });
        }
    }

    // A wire between engines carries a type both mean alike.
    for w in &wires {
        let (a, b) = (&engine_of[&w.from.0], &engine_of[&w.to.0]);
        if a == b {
            continue;
        }
        let kind = |id: &str| nodes.get(id).and_then(|n| n.get("kind")).and_then(Value::as_str).unwrap_or_default().to_owned();
        let out_ty = merged.schema.node(&kind(&w.from.0)).and_then(|d| d.output(&w.from.1)).map(|p| p.ty.clone()).unwrap_or_default();
        let in_ty = merged.schema.node(&kind(&w.to.0)).and_then(|d| d.input(&w.to.1)).map(|p| p.ty.clone());
        let (Some(ca), Some(cb)) = (catalogues.get(a), catalogues.get(b)) else { continue };
        let why = merge::portable(&out_ty, (a, ca), (b, cb))
            .and_then(|()| match &in_ty {
                Some(t) if !cb.accepts(&out_ty, t) => Err(format!("{b} doesn't take a {out_ty} into a {t} input")),
                _ => Ok(()),
            })
            .err();
        if let Some(why) = why {
            return Err(graph_error(format!("wire {}.{} → {}.{} crosses from {a} to {b}: {why}", w.from.0, w.from.1, w.to.0, w.to.1)));
        }
    }

    // What the run computes: the targets (or the sinks), the watched nodes, their ancestors.
    let sinks: Vec<String> = nodes.keys().filter(|n| !wires.iter().any(|w| &w.from.0 == *n)).cloned().collect();
    let targets: Vec<String> = if req.targets.is_empty() { sinks } else { req.targets.iter().filter(|t| nodes.contains_key(*t)).cloned().collect() };
    let mut needed = BTreeSet::new();
    let mut want: VecDeque<String> = targets.iter().cloned().chain(req.watch.iter().map(|w| w.node.clone()).filter(|n| nodes.contains_key(n))).collect();
    while let Some(n) = want.pop_front() {
        if needed.insert(n.clone()) {
            want.extend(wires.iter().filter(|w| w.to.0 == n).map(|w| w.from.0.clone()));
        }
    }

    // Order (Kahn, ties by id, as the engines order) over the whole graph, so a cycle is an
    // error wherever it is (as in the engines); then the run's share of it, and depth.
    let mut indegree: BTreeMap<&String, usize> = nodes.keys().map(|n| (n, 0)).collect();
    for w in &wires {
        if let Some(d) = indegree.get_mut(&w.to.0) {
            *d += 1;
        }
    }
    let mut ready: BTreeSet<&String> = indegree.iter().filter(|(_, d)| **d == 0).map(|(n, _)| *n).collect();
    let mut order: Vec<String> = Vec::new();
    while let Some(n) = ready.pop_first() {
        order.push(n.clone());
        for w in wires.iter().filter(|w| &w.from.0 == n) {
            if let Some(d) = indegree.get_mut(&w.to.0) {
                *d = d.saturating_sub(1);
                if *d == 0 {
                    ready.insert(&w.to.0);
                }
            }
        }
    }
    if order.len() != nodes.len() {
        return Err(graph_error("the graph has a cycle"));
    }
    order.retain(|n| needed.contains(n));
    let wires: Vec<Wire> = wires.into_iter().filter(|w| needed.contains(&w.to.0)).collect();
    let mut depth: BTreeMap<&str, usize> = BTreeMap::new();
    for n in &order {
        let d = wires
            .iter()
            .filter(|w| &w.to.0 == n)
            .map(|w| depth.get(w.from.0.as_str()).copied().unwrap_or(0) + usize::from(engine_of[&w.from.0] != engine_of[n]))
            .max()
            .unwrap_or(0);
        depth.insert(n, d);
    }

    // Fragments, numbered by depth then by the first of their nodes in the run's order.
    let mut keys: Vec<(usize, String)> = Vec::new();
    for n in &order {
        let k = (depth[n.as_str()], engine_of[n].clone());
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys.sort_by_key(|(d, _)| *d);
    let index = |n: &str| keys.iter().position(|(d, e)| *d == depth[n] && e == &engine_of[n]).unwrap_or(0);
    let mut fragments: Vec<Fragment> = keys.iter().map(|(_, e)| Fragment { engine: e.clone(), ..Default::default() }).collect();
    let mut frag_of: BTreeMap<&str, usize> = BTreeMap::new();
    for n in &order {
        let i = index(n);
        frag_of.insert(n, i);
        if let Some(f) = fragments.get_mut(i) {
            f.nodes.push(n.clone());
        }
    }

    let mut crossings = Vec::new();
    let mut inside: Vec<Vec<&Wire>> = vec![Vec::new(); fragments.len()];
    for w in &wires {
        let (a, b) = (frag_of[w.from.0.as_str()], frag_of[w.to.0.as_str()]);
        if a == b {
            if let Some(v) = inside.get_mut(a) {
                v.push(w);
            }
        } else {
            crossings.push(Crossing { from: w.from.clone(), to: w.to.clone(), slice: w.slice.clone(), from_fragment: a, to_fragment: b });
            if let Some(f) = fragments.get_mut(b) {
                f.after.insert(a);
            }
        }
    }

    let owners: Vec<String> = fragments.iter().map(|f| f.engine.clone()).collect();
    for (i, f) in fragments.iter_mut().enumerate() {
        let mine: BTreeSet<&String> = f.nodes.iter().collect();
        let mut t: Vec<String> = targets.iter().filter(|n| mine.contains(n)).cloned().collect();
        for c in crossings.iter().filter(|c| c.from_fragment == i) {
            if !t.contains(&c.from.0) {
                t.push(c.from.0.clone());
            }
        }
        f.targets = t;
        let same_engine = |c: &&Crossing| c.from_fragment == i && owners.get(c.to_fragment) == Some(&f.engine);
        f.hold = crossings.iter().filter(same_engine).map(|c| format!("{}.{}", c.from.0, c.from.1)).collect();
        f.watch = req.watch.iter().enumerate().filter(|(_, w)| mine.contains(&w.node)).map(|(k, w)| (k, w.clone())).collect();
        f.inputs = req
            .inputs
            .iter()
            .filter(|(k, _)| k.rsplit_once('.').is_some_and(|(n, _)| mine.contains(&n.to_owned())))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut g: Map<String, Value> = graph.as_object().cloned().unwrap_or_default();
        g.remove("groups");
        g.remove("engines");
        let ns: Map<String, Value> = nodes.iter().filter(|(k, _)| mine.contains(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
        g.insert("nodes".into(), Value::Object(ns));
        let ws: Vec<Value> = inside
            .get(i)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|w| {
                let mut wire = serde_json::json!({"from": {"node": w.from.0, "port": w.from.1}, "to": {"node": w.to.0, "port": w.to.1}});
                if let Some(s) = &w.slice {
                    wire["slice"] = Value::from(s.as_str());
                }
                wire
            })
            .collect();
        g.insert("wires".into(), Value::Array(ws));
        f.graph = Value::Object(g);
    }
    Ok(Plan { fragments, crossings, order })
}
