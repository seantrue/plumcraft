//! Convert in the session (`docs/research/20-convert.md`): Insert Convert on a refused wire
//! (`wire.convert`), and the core Convert node expanded into its converters before a run.
//!
//! **Expansion.** No engine runs a Convert node. Before a run (or a save) goes out, each one is
//! replaced by its path's nodes ([`plumcraft_graph::Graph::convert_path`]). The last takes the
//! Convert's own id, so its state, thumbnail and time are the Convert's; the others are
//! `<id>_convert<k>`. The engine picks where each runs, as for any node without a pin. A
//! Convert with no path (an end unwired, or nothing converts) is left out with its wires, so
//! the rest of the graph still runs.
//!
//! **Translation.** What comes back names the converters, so [`Expanded`] puts the Convert
//! back: an output of the last step is the Convert's `value`, and a step that fails fails the
//! Convert, with the step's error. The other steps' records are dropped.

use std::collections::BTreeMap;

use plumcraft_graph::{Graph, Node, NodeId, PortRef, Wire};
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::convert::{self, KIND, PORT};
use plumcraft_proto::display::{Display, WatchRequest};
use plumcraft_proto::event::{NodeState, StatusRecord};
use plumcraft_proto::run::RunReply;
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::{Result, Session, SessionError};

/// Where each Convert's value comes from in the expanded graph, and which steps are whose.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Expanded {
    /// Convert id → the node and output its value is (the last step's; or, for a path of no
    /// steps, the Convert's own source).
    pub outputs: BTreeMap<String, (String, String)>,
    /// A step's node id → its Convert's id (not the last step, which has the Convert's id).
    pub steps: BTreeMap<String, String>,
}

impl Expanded {
    pub fn is_empty(&self) -> bool {
        self.outputs.is_empty()
    }

    /// `(node, port)` as the expanded graph names it.
    pub fn out(&self, node: &str, port: &str) -> (String, String) {
        match self.outputs.get(node) {
            Some(at) if port == PORT => at.clone(),
            _ => (node.to_owned(), port.to_owned()),
        }
    }

    /// `(node, port)` as the open graph names it.
    pub fn back(&self, node: &str, port: &str) -> (String, String) {
        match self.outputs.iter().find(|(_, at)| at.0 == node && at.1 == port) {
            Some((conv, _)) => (conv.clone(), PORT.to_owned()),
            None => (node.to_owned(), port.to_owned()),
        }
    }

    pub fn watch_out(&self, watch: &[WatchRequest]) -> Vec<WatchRequest> {
        watch
            .iter()
            .map(|w| {
                let (node, port) = self.out(&w.node, &w.port);
                WatchRequest { node, port, ..w.clone() }
            })
            .collect()
    }

    pub fn targets_out(&self, targets: Vec<String>) -> Vec<String> {
        targets.into_iter().map(|t| self.outputs.get(&t).map_or(t, |at| at.0.clone())).collect()
    }

    fn display_back(&self, d: &mut Display) {
        let (node, port) = self.back(&d.node, &d.port);
        (d.node, d.port) = (node, port);
    }

    /// A status record as the open graph names it; `None` for a step's that isn't news (only
    /// a failing step speaks for its Convert).
    pub fn record_back(&self, rec: &StatusRecord) -> Option<StatusRecord> {
        let mut r = rec.clone();
        if let Some(conv) = self.steps.get(&rec.node_id) {
            if !matches!(rec.state, NodeState::Error) {
                return None;
            }
            r.node_id = conv.clone();
        }
        for d in &mut r.display {
            self.display_back(d);
        }
        Some(r)
    }

    /// A reply as the open graph names it.
    pub fn reply_back(&self, reply: &RunReply) -> RunReply {
        if self.is_empty() {
            return reply.clone();
        }
        let mut r = reply.clone();
        for (step, conv) in &self.steps {
            if let Some(report) = r.nodes.remove(step)
                && matches!(report.state, NodeState::Error)
            {
                r.nodes.insert(conv.clone(), report);
            }
        }
        for (conv, (node, port)) in &self.outputs {
            if node == conv
                && let Some(report) = r.nodes.get_mut(conv)
                && let Some(v) = report.outputs.remove(port)
            {
                report.outputs.insert(PORT.to_owned(), v);
            }
        }
        for d in &mut r.watch {
            self.display_back(d);
        }
        r.order = r.order.iter().filter(|id| !self.steps.contains_key(*id)).cloned().collect();
        r
    }
}

fn step_id(graph: &Graph, conv: &NodeId, k: usize) -> NodeId {
    let mut id = NodeId(format!("{}_convert{k}", conv.0));
    while graph.nodes.contains_key(&id) {
        id = NodeId(format!("{}_", id.0));
    }
    id
}

/// `graph` with every Convert replaced by its converters (module docs).
pub fn expand(graph: &Graph, catalog: &SchemaReply) -> (Graph, Expanded) {
    let mut out = graph.clone();
    let mut map = Expanded::default();
    let converts: Vec<NodeId> = graph.nodes.iter().filter(|(_, n)| n.kind == KIND).map(|(id, _)| id.clone()).collect();
    for conv in converts {
        let path = graph.convert_path(catalog, &conv);
        let input = graph.wire_into(&PortRef::new(conv.clone(), PORT)).cloned();
        let pos = graph.nodes.get(&conv).map_or([0.0, 0.0], |n| n.pos);
        out.nodes.remove(&conv);
        let incoming: Option<Wire> = input.clone();
        out.wires.retain(|w| w.to.node != conv && w.from.node != conv);
        let leaving: Vec<Wire> = graph.wires.iter().filter(|w| w.from.node == conv).cloned().collect();
        let (Some(path), Some(incoming)) = (path, incoming) else { continue };
        let groups: Vec<_> = graph.groups.iter().filter(|(_, g)| g.nodes.contains(&conv)).map(|(id, _)| id.clone()).collect();
        // the value's source as it goes along
        let mut at = incoming.from.clone();
        for (k, step) in path.iter().enumerate() {
            let id = if k + 1 == path.len() { conv.clone() } else { step_id(&out, &conv, k) };
            let Ok(node) = serde_json::from_value::<Node>(json!({"kind": step.kind, "pos": pos})) else { continue };
            out.nodes.insert(id.clone(), node);
            for g in &groups {
                if let Some(group) = out.groups.get_mut(g) {
                    group.nodes.insert(id.clone());
                }
            }
            out.wires.push(Wire::new(at.clone(), PortRef::new(id.clone(), step.input)));
            if id != conv {
                map.steps.insert(id.0.clone(), conv.0.clone());
            }
            at = PortRef::new(id, step.output);
        }
        for w in leaving {
            out.wires.push(Wire { from: at.clone(), ..w });
        }
        for g in out.groups.values_mut() {
            for o in &mut g.outputs {
                if o.from.node == conv && o.from.port == PORT {
                    o.from = at.clone();
                }
            }
        }
        for n in out.nets.values_mut() {
            if n.from.node == conv && n.from.port == PORT {
                n.from = at.clone();
            }
        }
        map.outputs.insert(conv.0.clone(), (at.node.0.clone(), at.port.clone()));
    }
    (out, map)
}

/// The graph a run sends: paths absolute against `base`, Converts expanded. Leaves the
/// expansion with the run state, which translates what comes back.
pub(crate) fn runnable(s: &mut Session, base: Option<&std::path::Path>) -> Graph {
    let abs = crate::paths::with_absolute_paths(s.graph(), s.catalog(), base);
    let (graph, map) = expand(&abs, s.catalog());
    s.run.convert = map;
    graph
}

/// The ways `from`'s output can reach `to`'s input, best first, as `[{kinds, label, types,
/// lossless}]`: what `wire.convert` would place. Empty when the types meet already, or nothing
/// converts between them.
pub fn offers(s: &Session, from: &PortRef, to: &PortRef) -> Vec<Value> {
    let (Some(out_ty), Some(in_ty)) = (output_type(s, from), input_type(s, to)) else { return Vec::new() };
    if s.catalog().accepts(&out_ty, &in_ty) {
        return Vec::new();
    }
    s.catalog()
        .convert_paths(&out_ty, &in_ty)
        .iter()
        .filter(|p| !p.is_empty())
        .map(|p| {
            json!({"kinds": p.iter().map(|c| c.kind).collect::<Vec<_>>(), "label": convert::label(p), "types": convert::types(p), "lossless": convert::lossless(p)})
        })
        .collect()
}

fn output_type(s: &Session, at: &PortRef) -> Option<String> {
    let n = s.graph().nodes.get(&at.node)?;
    s.catalog().node(&n.kind)?.output(&at.port).map(|p| p.ty.clone())
}

fn input_type(s: &Session, at: &PortRef) -> Option<String> {
    let n = s.graph().nodes.get(&at.node)?;
    let def = s.catalog().node(&n.kind)?;
    plumcraft_graph::resolve_input(def, n, &at.node, &at.port).ok().map(|r| r.port.ty.clone())
}

fn port_ref(p: &Value, key: &str) -> Result<PortRef> {
    let v = p.get(key).ok_or_else(|| SessionError::Params(format!("\"{key}\" is required: {{node, port}}")))?;
    let node = v.get("node").and_then(Value::as_str).ok_or_else(|| SessionError::Params(format!("\"{key}.node\" must be a node id")))?;
    let port = v.get("port").and_then(Value::as_str).ok_or_else(|| SessionError::Params(format!("\"{key}.port\" must be a port name")))?;
    Ok(PortRef::new(NodeId(node.to_owned()), port))
}

/// `wire.convert {from, to, kinds?}`: place a path's converters between an output and an input
/// the types refuse, and wire them, as one undo step.
fn wire_convert(s: &mut Session, p: &Value) -> Result<Value> {
    let (from, to) = (port_ref(p, "from")?, port_ref(p, "to")?);
    let offered = offers(s, &from, &to);
    let picked = match p.get("kinds") {
        None | Some(Value::Null) => offered.first().cloned(),
        Some(Value::Array(want)) => offered.iter().find(|o| o.get("kinds").and_then(Value::as_array) == Some(want)).cloned(),
        Some(_) => return Err(SessionError::Params("\"kinds\" must be a list of node kinds, one of the offered paths".into())),
    };
    let Some(picked) = picked else {
        let why = if offered.is_empty() { "no connected engine converts between them" } else { "that isn't one of the paths (wire.convertOffers lists them)" };
        return Err(SessionError::Params(format!("can't convert {from} for {to}: {why}")));
    };
    let kinds: Vec<String> =
        picked.get("kinds").and_then(Value::as_array).map(|k| k.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    let (a, b) = (s.graph().nodes.get(&from.node).map(|n| n.pos), s.graph().nodes.get(&to.node).map(|n| n.pos));
    let (Some(a), Some(b)) = (a, b) else { return Err(SessionError::Params("no such node".into())) };
    let catalog = s.catalog().clone();
    let label = picked.get("label").and_then(Value::as_str).unwrap_or_default().to_owned();
    let placed = s.edit(&format!("Insert {label}"), |g, _| {
        let mut at = from.clone();
        let mut placed = Vec::new();
        let n = kinds.len() as f32 + 1.0;
        for (k, kind) in kinds.iter().enumerate() {
            let def = catalog.node(kind).ok_or_else(|| SessionError::Params(format!("no node type {kind}")))?;
            let t = (k as f32 + 1.0) / n;
            let id = g.add_node(def, [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            let c =
                catalog.converters().into_iter().find(|c| c.kind == kind.as_str()).ok_or_else(|| SessionError::Params(format!("{kind} isn't a converter")))?;
            g.connect(&catalog, at.clone(), PortRef::new(id.clone(), c.input))?;
            at = PortRef::new(id.clone(), c.output);
            placed.push(id.0);
        }
        g.connect(&catalog, at, to.clone())?;
        Ok(placed)
    })?;
    let mut out = json!({"nodes": placed, "label": label, "types": picked["types"]});
    if picked["lossless"] == false {
        out["warning"] = json!(format!("{label} loses something on the way ({}): a step isn't lossless", picked["types"].as_str().unwrap_or_default()));
    }
    Ok(out)
}

fn convert_offers(s: &mut Session, p: &Value) -> Result<Value> {
    let (from, to) = (port_ref(p, "from")?, port_ref(p, "to")?);
    Ok(json!({"paths": offers(s, &from, &to)}))
}

/// `node.convert {node}`: give a Convert node the best path for what it is wired between now.
fn repick(s: &mut Session, p: &Value) -> Result<Value> {
    let id = NodeId(crate::commands::str_param(p, "node")?.to_owned());
    if !s.graph().is_convert(&id) {
        return Err(SessionError::Params(format!("{id} isn't a Convert node")));
    }
    let catalog = s.catalog().clone();
    let changed = s.edit("Pick Convert's path", |g, _| Ok(g.repin_convert(&catalog, &id, true)))?;
    let path = s.graph().convert_path(s.catalog(), &id);
    let kinds: Vec<&str> = path.iter().flatten().map(|c| c.kind).collect();
    let label = path.as_deref().map(convert::label);
    let mut out = json!({"node": id.0, "kinds": kinds, "label": label, "changed": changed});
    if path.as_deref().is_some_and(|p| !convert::lossless(p)) {
        out["warning"] = json!(format!("{} loses something on the way: a step isn't lossless", label.unwrap_or_default()));
    }
    Ok(out)
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "wire.convert",
            label: "Insert Convert",
            menu: &[],
            shortcut: None,
            params: "{from: {node, port}, to: {node, port}, kinds?: [kind]} -> {nodes, label, types, warning?}: place the converters of a path between an output and an input whose types don't meet, wired, as one undo step (docs/research/20-convert.md). Without kinds, the best path; with them, one of wire.convertOffers' paths. warning: a step isn't lossless",
            enabled: crate::commands::always,
            run: wire_convert,
            journal: true,
        },
        CommandSpec {
            id: "wire.convertOffers",
            label: "Conversion Paths",
            menu: &[],
            shortcut: None,
            params: "{from: {node, port}, to: {node, port}} -> {paths: [{kinds, label, types, lossless}]}: the ways the connected engines' converters take that output's type to that input's, best first (fewest steps, then lossless, then fewest changes of engine); empty when the types meet or nothing converts",
            enabled: crate::commands::always,
            run: convert_offers,
            journal: false,
        },
        CommandSpec {
            id: "node.convert",
            label: "Pick Convert's Path",
            menu: &[],
            shortcut: None,
            params: "{node} (a Convert node) -> {node, kinds, label, changed, warning?}: give it the best path for the types it is wired between now, replacing the one it keeps; one undo step",
            enabled: crate::commands::always,
            run: repick,
            journal: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use plumcraft_proto::catalogue::{NodeDef, Port, Socket};
    use plumcraft_proto::convert::Converts;
    use plumcraft_proto::run::NodeReport;

    use super::*;

    fn def(id: &str, ins: &[(&str, &str)], outs: &[(&str, &str)], converts: Option<(&str, &str, bool)>) -> NodeDef {
        NodeDef {
            id: id.into(),
            display: id.into(),
            inputs: ins.iter().map(|(n, t)| Port { name: (*n).into(), ty: (*t).into(), socket: Some(Socket::Socket), ..Default::default() }).collect(),
            outputs: outs.iter().map(|(n, t)| Port { name: (*n).into(), ty: (*t).into(), ..Default::default() }).collect(),
            converts: converts.map(|(f, t, l)| Converts::new(f, t, l)),
            ..Default::default()
        }
    }

    fn session() -> Session {
        let mut s = Session::default();
        s.set_catalog(SchemaReply {
            nodes: vec![
                def("session", &[], &[("doc", "DOC")], None),
                def("bounce", &[("doc", "DOC")], &[("audio", "WAV")], Some(("DOC", "WAV", false))),
                def("from_wav", &[("wav", "WAV")], &[("audio", "AUDIO")], Some(("WAV", "AUDIO", true))),
                def("gain", &[("audio", "AUDIO")], &[("audio", "AUDIO")], None),
            ],
            ..Default::default()
        });
        s
    }

    fn add(s: &mut Session, kind: &str, x: f32) -> String {
        s.execute("node.add", &json!({"kind": kind, "pos": [x, 0]})).unwrap()["id"].as_str().unwrap().to_owned()
    }

    fn wire(s: &mut Session, from: (&str, &str), to: (&str, &str)) -> Result<Value> {
        s.execute("wire.connect", &json!({"from": {"node": from.0, "port": from.1}, "to": {"node": to.0, "port": to.1}}))
    }

    #[test]
    fn a_refused_wire_is_offered_its_converters_and_gets_them_in_one_step() {
        let mut s = session();
        assert!(s.catalog().node(KIND).is_some() && s.catalog().nodes.iter().any(|n| n.id == KIND), "Convert is offered");
        let (doc, gain) = (add(&mut s, "session", 0.0), add(&mut s, "gain", 600.0));
        assert!(wire(&mut s, (&doc, "doc"), (&gain, "audio")).is_err(), "DOC into AUDIO");
        let at = json!({"from": {"node": doc, "port": "doc"}, "to": {"node": gain, "port": "audio"}});
        let offers = s.execute("wire.convertOffers", &at).unwrap();
        assert_eq!(offers["paths"][0]["kinds"], json!(["bounce", "from_wav"]), "{offers}");
        assert_eq!(offers["paths"][0]["types"], "DOC → WAV → AUDIO");
        let nodes = s.graph().nodes.len();
        let r = s.execute("wire.convert", &at).unwrap();
        assert_eq!(r["nodes"].as_array().map(Vec::len), Some(2), "{r}");
        assert!(r["warning"].as_str().is_some_and(|w| w.contains("isn't lossless")), "Bounce loses the document: {r}");
        assert_eq!(s.graph().nodes.len(), nodes + 2);
        assert_eq!(s.graph().wires.len(), 3);
        assert!(s.graph().wire_problems(s.catalog()).is_empty());
        // one undo takes it all back
        s.execute("edit.undo", &Value::Null).unwrap();
        assert_eq!((s.graph().nodes.len(), s.graph().wires.len()), (nodes, 0));
        // nothing to offer between types that meet, or that nothing converts
        let back = json!({"from": {"node": gain, "port": "audio"}, "to": {"node": doc, "port": "doc"}});
        assert!(s.execute("wire.convert", &back).is_err());
        assert!(s.execute("wire.convert", &json!({"from": {"node": doc, "port": "doc"}, "to": {"node": gain, "port": "audio"}, "kinds": ["gain"]})).is_err());
    }

    #[test]
    fn a_convert_runs_as_its_steps_and_reports_as_itself() {
        let mut s = session();
        let (doc, conv, gain) = (add(&mut s, "session", 0.0), add(&mut s, KIND, 300.0), add(&mut s, "gain", 600.0));
        wire(&mut s, (&doc, "doc"), (&conv, PORT)).unwrap();
        wire(&mut s, (&conv, PORT), (&gain, "audio")).unwrap();
        let (g, map) = expand(s.graph(), s.catalog());
        assert!(!g.nodes.values().any(|n| n.kind == KIND), "no Convert goes to an engine");
        let step = format!("{conv}_convert0");
        assert_eq!(g.nodes.get(&NodeId(step.clone())).map(|n| n.kind.as_str()), Some("bounce"));
        assert_eq!(g.nodes.get(&NodeId(conv.clone())).map(|n| n.kind.as_str()), Some("from_wav"), "the last step has the Convert's id");
        let has = |f: (&str, &str), t: (&str, &str)| {
            g.wires.iter().any(|w| (w.from.node.0.as_str(), w.from.port.as_str(), w.to.node.0.as_str(), w.to.port.as_str()) == (f.0, f.1, t.0, t.1))
        };
        assert!(has((&doc, "doc"), (&step, "doc")) && has((&step, "audio"), (&conv, "wav")) && has((&conv, "audio"), (&gain, "audio")), "{:?}", g.wires);
        assert_eq!(map.out(&conv, PORT), (conv.clone(), "audio".to_owned()));
        assert_eq!(map.watch_out(&[WatchRequest { node: conv.clone(), port: PORT.into(), ..Default::default() }])[0].port, "audio");

        // what comes back is the Convert's
        let mut reply = RunReply { ok: true, ..Default::default() };
        let done = NodeReport {
            state: NodeState::Done,
            outputs: BTreeMap::from([("audio".to_owned(), plumcraft_proto::PortValue::Json { value: json!(1) })]),
            ..Default::default()
        };
        reply.nodes.insert(conv.clone(), done);
        reply.nodes.insert(step.clone(), NodeReport { state: NodeState::Done, ..Default::default() });
        reply.order = vec![step.clone(), conv.clone()];
        let back = map.reply_back(&reply);
        assert!(back.nodes.get(&conv).is_some_and(|r| r.outputs.contains_key(PORT)) && !back.nodes.contains_key(&step));
        assert_eq!(back.order, std::slice::from_ref(&conv));
        // a failing step fails the Convert, with the step's error
        let err = plumcraft_proto::error::NodeError { message: "bounce broke".into(), ..Default::default() };
        reply.nodes.insert(step.clone(), NodeReport { state: NodeState::Error, error: Some(err), ..Default::default() });
        reply.nodes.insert(conv.clone(), NodeReport { state: NodeState::Skipped, ..Default::default() });
        let back = map.reply_back(&reply);
        assert_eq!(back.nodes.get(&conv).and_then(|r| r.error.as_ref()).map(|e| e.message.as_str()), Some("bounce broke"));
        let rec = StatusRecord { node_id: step.clone(), state: NodeState::Started, ..Default::default() };
        assert!(map.record_back(&rec).is_none(), "a step's progress isn't news");
        let rec = StatusRecord { node_id: step, state: NodeState::Error, ..Default::default() };
        assert_eq!(map.record_back(&rec).map(|r| r.node_id), Some(conv.clone()));

        // run.start sends the expanded graph and counts only the open graph's nodes
        s.connected = true;
        s.execute("run.start", &json!({})).unwrap();
        let sent = s.run.take_outbox();
        let Some(crate::run::Outgoing::Run(req)) = sent.into_iter().next() else { panic!("a run went out") };
        let plumcraft_proto::run::GraphRef::Inline { graph } = &req.graph else { panic!("inline") };
        assert!(graph.to_string().contains("bounce") && !graph.to_string().contains(KIND));
        assert_eq!(s.run.progress().1, 3);
    }

    #[test]
    fn a_convert_with_an_end_unwired_is_left_out_and_the_rest_runs() {
        let mut s = session();
        let (doc, conv) = (add(&mut s, "session", 0.0), add(&mut s, KIND, 300.0));
        wire(&mut s, (&doc, "doc"), (&conv, PORT)).unwrap();
        let (g, map) = expand(s.graph(), s.catalog());
        assert!(!g.nodes.contains_key(&NodeId(conv)) && g.nodes.contains_key(&NodeId(doc)) && g.wires.is_empty());
        assert!(map.is_empty());
    }

    #[test]
    fn node_convert_repicks_the_path() {
        let mut s = session();
        let (doc, conv, gain) = (add(&mut s, "session", 0.0), add(&mut s, KIND, 300.0), add(&mut s, "gain", 600.0));
        wire(&mut s, (&doc, "doc"), (&conv, PORT)).unwrap();
        wire(&mut s, (&conv, PORT), (&gain, "audio")).unwrap();
        let r = s.execute("node.convert", &json!({"node": conv})).unwrap();
        assert_eq!(r["kinds"], json!(["bounce", "from_wav"]), "{r}");
        assert!(r["warning"].is_string());
        assert!(s.execute("node.convert", &json!({"node": doc})).is_err());
    }
}
