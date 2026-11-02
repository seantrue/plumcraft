//! Parameter bundles: a node's input named by `flags.bundle_port` takes a bundle of parameters
//! from upstream. Detach a node from the bundle wired into it (`bundle.detach`, copying the values
//! in), or reset a parameter a wire overrides back to the inherited value (`bundle.reset`).
//!
//! Precedence when a node runs, per parameter: a wire into that port, then the incoming bundle,
//! then the node's own widget, then blank (spec 7.6 rule 5). So a bundled value can be overridden
//! by a wire, and a node's own widgets only count again once it is detached.
//!
//! Lifted from the research software's `crates/engine/src/bundle_cmds.rs`. Which port carries a bundle, and
//! what it is called, come from the catalogue: core has no bundle of its own.

use plumcraft_graph::{Graph, GraphError, NodeId, PortRef, ports};
use plumcraft_proto::catalogue::{NodeDef, ParamRef, SchemaReply};
use serde_json::{Map, Value, json};

use crate::commands::{CommandSpec, str_param};
use crate::{Result, Session, SessionError};

fn def_of<'a>(graph: &Graph, catalog: &'a SchemaReply, node: &NodeId) -> Option<&'a NodeDef> {
    graph.node(node).and_then(|n| catalog.node(&n.kind))
}

/// The input that carries a bundle into a node of type `def`, when its type takes one.
fn bundle_port(def: &NodeDef) -> Option<&str> {
    def.flags.bundle_port.as_deref().filter(|p| !p.is_empty())
}

/// What the catalogue calls `def`'s bundle input: its label, else its name, capitalised.
pub fn bundle_name(def: &NodeDef) -> Option<String> {
    let port = bundle_port(def)?;
    let label = def.input(port).and_then(|p| p.label.as_deref()).filter(|l| !l.is_empty()).unwrap_or(port);
    let mut c = label.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect())
}

/// The node feeding `node`'s bundle input.
fn bundle_source(graph: &Graph, catalog: &SchemaReply, node: &NodeId) -> Option<NodeId> {
    let port = bundle_port(def_of(graph, catalog, node)?)?;
    graph.wire_into(&PortRef::new(node.clone(), port)).map(|w| w.from.node.clone())
}

/// Does `node` take a bundle, and is one wired in?
pub fn has_bundle(graph: &Graph, catalog: &SchemaReply, node: &NodeId) -> bool {
    bundle_source(graph, catalog, node).is_some()
}

/// The value `param` has in the bundle wired into `node`, as far as the graph says: the nearest
/// node up the chain of bundle wires that has that parameter, with its own value (or the value it
/// inherits itself). `None` when there is no bundle, nothing up the chain has the parameter, or
/// the value comes from a wire (known only when the graph runs).
pub fn static_bundle_value(graph: &Graph, catalog: &SchemaReply, node: &NodeId, param: &ParamRef) -> Option<Value> {
    let mut at = node.clone();
    for _ in 0..64 {
        let src = bundle_source(graph, catalog, &at)?;
        let n = graph.node(&src)?;
        let def = catalog.node(&n.kind)?;
        if let Some(port) = def.inputs.iter().find(|p| p.param_ref.as_ref() == Some(param)) {
            if graph.wire_into(&PortRef::new(src.clone(), port.name.clone())).is_some() {
                return None;
            }
            if let Some(v) = static_bundle_value(graph, catalog, &src, param) {
                return Some(v);
            }
            return Some(ports::current_value(def, n, &port.name, port));
        }
        at = src;
    }
    None
}

/// Does any node up the bundle chain from `node` have `param`?
pub(crate) fn upstream_has(graph: &Graph, catalog: &SchemaReply, node: &NodeId, param: &ParamRef) -> bool {
    let mut at = node.clone();
    for _ in 0..64 {
        let Some(src) = bundle_source(graph, catalog, &at) else { return false };
        if def_of(graph, catalog, &src).is_some_and(|d| d.inputs.iter().any(|p| p.param_ref.as_ref() == Some(param))) {
            return true;
        }
        at = src;
    }
    false
}

/// What [`detach`] copied.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Detached {
    /// Port -> the value copied into its widget.
    pub copied: Map<String, Value>,
    /// Parameters the bundle may carry whose value isn't known without a run (wired upstream).
    pub unresolved: Vec<String>,
}

/// Copy the bundle's values into `node`'s widgets (those not fed by their own wire), then
/// disconnect its bundle input.
pub fn detach(g: &mut Graph, catalog: &SchemaReply, node: &NodeId) -> Result<Detached> {
    let def = def_of(g, catalog, node).ok_or_else(|| GraphError::UnknownNode(node.clone()))?;
    let Some(port) = bundle_port(def).filter(|_| has_bundle(g, catalog, node)) else {
        return Err(SessionError::Params(format!("{node} has no bundle wired in")));
    };
    let mut out = Detached::default();
    let mut sets = Vec::new();
    for p in &def.inputs {
        let Some(param) = &p.param_ref else { continue };
        if !p.has_widget() || g.wire_into(&PortRef::new(node.clone(), p.name.clone())).is_some() {
            continue;
        }
        match static_bundle_value(g, catalog, node, param) {
            Some(v) => sets.push((p.name.clone(), v)),
            None if upstream_has(g, catalog, node, param) => out.unresolved.push(p.name.clone()),
            None => {}
        }
    }
    for (name, v) in sets {
        g.set_value_checked(catalog, &PortRef::new(node.clone(), name.clone()), v.clone())?;
        out.copied.insert(name, v);
    }
    g.disconnect(&PortRef::new(node.clone(), port))?;
    Ok(out)
}

fn any_bundled(s: &Session) -> std::result::Result<(), String> {
    let g = s.graph();
    if g.nodes.keys().any(|n| has_bundle(g, s.catalog(), n)) { Ok(()) } else { Err("no node has a bundle wired in".into()) }
}

fn bundle_detach(s: &mut Session, p: &Value) -> Result<Value> {
    let node = NodeId::from(str_param(p, "node")?);
    let name = def_of(s.graph(), s.catalog(), &node).and_then(bundle_name).unwrap_or_else(|| "Bundle".into());
    let out = s.edit(&format!("Detach from {name}"), |g, catalog| detach(g, catalog, &node))?;
    Ok(json!({"copied": out.copied, "unresolved": out.unresolved}))
}

fn bundle_reset(s: &mut Session, p: &Value) -> Result<Value> {
    let at = PortRef::new(str_param(p, "node")?, str_param(p, "port")?);
    let wire = s.edit("Reset to Inherited", |g, catalog| {
        let def = def_of(g, catalog, &at.node).ok_or_else(|| GraphError::UnknownNode(at.node.clone()))?;
        if def.input(&at.port).and_then(|p| p.param_ref.as_ref()).is_none() {
            return Err(SessionError::Params(format!("{at} is not in a bundle")));
        }
        if !has_bundle(g, catalog, &at.node) {
            return Err(SessionError::Params(format!("{} has no bundle to inherit from", at.node)));
        }
        Ok(g.disconnect(&at)?)
    })?;
    Ok(json!({"wire": crate::graph_cmds::wire_json(&wire)}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "bundle.detach",
            label: "Detach from Bundle",
            menu: &[],
            shortcut: None,
            params: "{node: node id} -> {copied: {port: value}, unresolved: [port]} (copies the bundle's values into the node's widgets, then disconnects its bundle input, flags.bundle_port)",
            enabled: any_bundled,
            run: bundle_detach,
            journal: true,
        },
        CommandSpec {
            id: "bundle.reset",
            label: "Reset to Inherited",
            menu: &[],
            shortcut: None,
            params: "{node: node id, port: bundled port} -> {wire} (removes the wire that overrides the bundle's value)",
            enabled: any_bundled,
            run: bundle_reset,
            journal: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bundle input not called `settings`, a parameter in it, and one that isn't.
    fn catalog() -> SchemaReply {
        serde_json::from_value(json!({"nodes": [
            {"id": "t.ink", "display": "Ink", "flags": {"bundle_port": "tuning"},
             "inputs": [{"name": "tuning", "type": "x.t.tune", "label": "fine tuning"},
                        {"name": "ink.width", "type": "FLOAT", "default": 1.0, "param_ref": {"group": "ink", "name": "width"}},
                        {"name": "ink.tint", "type": "STRING", "default": "red", "param_ref": {"group": "ink", "name": "tint"}},
                        {"name": "gain", "type": "FLOAT", "default": 0.5}],
             "outputs": [{"name": "tuning", "type": "x.t.tune"}, {"name": "level", "type": "FLOAT"}]},
            {"id": "t.pass", "display": "Pass", "flags": {"bundle_port": "tuning"},
             "inputs": [{"name": "tuning", "type": "x.t.tune"}],
             "outputs": [{"name": "tuning", "type": "x.t.tune"}]},
            {"id": "t.plain", "display": "Plain", "inputs": [{"name": "tuning", "type": "x.t.tune"}]}]}))
        .unwrap()
    }

    fn add(s: &mut Session, kind: &str) -> String {
        s.execute("node.add", &json!({"kind": kind})).unwrap()["id"].as_str().unwrap().to_owned()
    }

    fn link(s: &mut Session, from: &str, port: &str, to: &str, into: &str) {
        s.execute("wire.connect", &json!({"from": {"node": from, "port": port}, "to": {"node": to, "port": into}})).unwrap();
    }

    fn width() -> ParamRef {
        ParamRef { group: "ink".into(), name: "width".into() }
    }

    #[test]
    fn detach_copies_the_bundles_values_and_undoes() {
        let mut s = Session::new(catalog());
        let (up, down) = (add(&mut s, "t.ink"), add(&mut s, "t.ink"));
        s.execute("node.set", &json!({"node": up, "port": "ink.width", "value": 4.0})).unwrap();
        s.execute("node.set", &json!({"node": up, "port": "gain", "value": 9.0})).unwrap();
        s.execute("node.set", &json!({"node": down, "port": "ink.tint", "value": "blue"})).unwrap();
        link(&mut s, &up, "tuning", &down, "tuning");
        let before = s.graph().clone();
        let r = s.execute("bundle.detach", &json!({"node": down})).unwrap();
        assert_eq!(r["copied"], json!({"ink.width": 4.0, "ink.tint": "red"}), "a bundled value, its source's default too, and not `gain`");
        let g = s.graph();
        let d = NodeId::from(down.as_str());
        assert!(!has_bundle(g, s.catalog(), &d));
        assert_eq!(g.node(&d).unwrap().values["ink.width"], json!(4.0));
        assert_eq!(g.node(&d).unwrap().values["ink.tint"], json!("red"));
        assert_eq!(g.node(&d).unwrap().values["gain"], json!(0.5), "not in the bundle: untouched");
        assert_eq!(s.undo_label(), Some("Detach from Fine tuning"), "named by the catalogue");
        s.execute("edit.undo", &Value::Null).unwrap();
        assert_eq!(s.graph(), &before, "one undo step");
        assert!(s.execute("bundle.detach", &json!({"node": up})).is_err(), "nothing to detach from");
        assert_eq!(s.graph(), &before);
    }

    #[test]
    fn a_bundle_passes_through_nodes_without_the_parameter_and_the_nearest_wins() {
        let mut s = Session::new(catalog());
        let (far, near, pass, down) = (add(&mut s, "t.ink"), add(&mut s, "t.ink"), add(&mut s, "t.pass"), add(&mut s, "t.ink"));
        s.execute("node.set", &json!({"node": far, "port": "ink.width", "value": 2.0})).unwrap();
        s.execute("node.set", &json!({"node": near, "port": "ink.width", "value": 3.0})).unwrap();
        link(&mut s, &far, "tuning", &near, "tuning");
        link(&mut s, &near, "tuning", &pass, "tuning");
        link(&mut s, &pass, "tuning", &down, "tuning");
        let d = NodeId::from(down.as_str());
        assert_eq!(static_bundle_value(s.graph(), s.catalog(), &d, &width()), Some(json!(2.0)), "near inherits far's");
        s.execute("wire.disconnect", &json!({"to": {"node": near, "port": "tuning"}})).unwrap();
        assert_eq!(static_bundle_value(s.graph(), s.catalog(), &d, &width()), Some(json!(3.0)));
        assert_eq!(static_bundle_value(s.graph(), s.catalog(), &NodeId::from(far.as_str()), &width()), None, "no bundle wired in");
    }

    #[test]
    fn a_value_wired_upstream_is_not_known_and_detach_says_so() {
        let mut s = Session::new(catalog());
        let (src, up, down) = (add(&mut s, "t.ink"), add(&mut s, "t.ink"), add(&mut s, "t.ink"));
        link(&mut s, &src, "level", &up, "ink.width");
        link(&mut s, &up, "tuning", &down, "tuning");
        assert_eq!(static_bundle_value(s.graph(), s.catalog(), &NodeId::from(down.as_str()), &width()), None);
        let r = s.execute("bundle.detach", &json!({"node": down})).unwrap();
        assert_eq!(r["unresolved"], json!(["ink.width"]));
        assert_eq!(r["copied"], json!({"ink.tint": "red"}));
    }

    #[test]
    fn reset_removes_the_overriding_wire() {
        let mut s = Session::new(catalog());
        let (up, down, src) = (add(&mut s, "t.ink"), add(&mut s, "t.ink"), add(&mut s, "t.ink"));
        link(&mut s, &up, "tuning", &down, "tuning");
        assert!(s.execute("bundle.reset", &json!({"node": down, "port": "ink.width"})).is_err(), "not overridden yet");
        link(&mut s, &src, "level", &down, "ink.width");
        s.execute("bundle.reset", &json!({"node": down, "port": "ink.width"})).unwrap();
        assert!(s.graph().wire_into(&PortRef::new(down.as_str(), "ink.width")).is_none());
        assert_eq!(s.undo_label(), Some("Reset to Inherited"));
        link(&mut s, &src, "level", &down, "gain");
        assert!(s.execute("bundle.reset", &json!({"node": down, "port": "gain"})).is_err(), "only bundled ports");
        link(&mut s, &src, "level", &up, "ink.width");
        assert!(s.execute("bundle.reset", &json!({"node": up, "port": "ink.width"})).is_err(), "only with a bundle to inherit");
    }

    #[test]
    fn the_catalogue_decides_what_a_bundle_is() {
        let mut s = Session::new(catalog());
        let (up, plain) = (add(&mut s, "t.ink"), add(&mut s, "t.plain"));
        assert!(s.execute("bundle.detach", &json!({"node": plain})).unwrap_err().to_string().contains("not available"), "no bundle anywhere");
        link(&mut s, &up, "tuning", &plain, "tuning");
        assert!(!has_bundle(s.graph(), s.catalog(), &NodeId::from(plain.as_str())), "a port of that name without the flag is no bundle");
        assert!(s.execute("bundle.detach", &json!({"node": plain})).is_err());
    }
}
