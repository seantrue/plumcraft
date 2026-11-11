//! Named ports: the top-level graph's own parameters (`docs/research/23-named-ports.md`).
//!
//! A named port says "a value from outside lands here": a runner given `name = value` stores
//! the value as that input's widget value and runs the graph as it is. The name is not part of
//! what runs (run requests, memo identities and staleness read values, as before); the value
//! is. It is the surface a group's promoted parameters give a group ([`crate::groups::Promoted`]),
//! for the whole graph.
//!
//! **Rules**, kept after every edit and on load, as nets are (a file's names are hostile
//! input: a bad entry is dropped, the graph kept):
//!
//! - a name is trimmed, 1 to [`MAX_NAME`] characters, with no control character, no `=` and no
//!   `.` (so `NAME` and `NODE.PORT` on a command line never meet); unique among the graph's;
//! - its node exists and its port isn't wired (a wire would win, so the name would do nothing);
//!   with a catalogue ([`Graph::prune_named_for`]), the port is also one of the node's inputs
//!   that holds a value (it has a widget);
//! - one port has at most one name.
//!
//! Wiring a named port is refused with a message naming it ([`crate::GraphError::NamedPort`]),
//! never silently unnamed: the designer removes the name first.

use std::collections::BTreeSet;

use plumcraft_proto::catalogue::SchemaReply;
use serde::{Deserialize, Serialize};

use crate::{Graph, GraphError, PortRef, Result};

/// The longest name, in characters.
pub const MAX_NAME: usize = 40;
/// The longest doc line kept, in characters.
pub const MAX_DOC: usize = 400;

/// One of the graph's named ports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedPort {
    /// What a command line calls it (`--set input=in.jpg`).
    pub name: String,
    pub port: PortRef,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub doc: String,
    /// A narrower range than the port's own (numeric ports), as a group's promoted parameter has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

/// The `named` list of a file: each entry that reads, the rest skipped (a list of anything else,
/// or no list, is none). [`Graph::prune_named`] then applies the rules.
pub(crate) fn lenient<'de, D: serde::Deserializer<'de>>(de: D) -> std::result::Result<Vec<NamedPort>, D::Error> {
    let v = serde_json::Value::deserialize(de)?;
    Ok(match v {
        serde_json::Value::Array(items) => items.into_iter().filter_map(|i| serde_json::from_value(i).ok()).collect(),
        _ => Vec::new(),
    })
}

/// A name as it is kept: trimmed, or why it can't be one.
pub fn check_port_name(name: &str) -> std::result::Result<String, String> {
    let t = name.trim();
    if t.is_empty() || t.chars().count() > MAX_NAME {
        return Err(format!("a port's name is 1 to {MAX_NAME} characters"));
    }
    if let Some(c) = t.chars().find(|c| c.is_control() || matches!(c, '=' | '.')) {
        return Err(format!("a port's name can't hold {c:?}"));
    }
    Ok(t.to_owned())
}

fn clean_doc(doc: &str) -> String {
    doc.chars().filter(|c| !c.is_control() || *c == '\n').take(MAX_DOC).collect::<String>().trim().to_owned()
}

impl Graph {
    /// The named port called `name`.
    pub fn named_port(&self, name: &str) -> Option<&NamedPort> {
        self.named.iter().find(|n| n.name == name)
    }

    /// The name on input `port`, when it has one.
    pub fn name_of_port(&self, port: &PortRef) -> Option<&str> {
        self.named.iter().find(|n| &n.port == port).map(|n| n.name.as_str())
    }

    /// Restore the rules that need no catalogue (module docs): drop entries with a bad or
    /// repeated name, a node that's gone, a wired port, or a port already named; keep the rest
    /// in order, their docs cleaned and ranges sane.
    pub fn prune_named(&mut self) {
        let mut names = BTreeSet::new();
        let mut ports = BTreeSet::new();
        let wired: BTreeSet<&PortRef> = self.wires.iter().map(|w| &w.to).collect();
        let nodes = &self.nodes;
        let mut keep = Vec::with_capacity(self.named.len());
        for mut n in std::mem::take(&mut self.named) {
            let Ok(name) = check_port_name(&n.name) else { continue };
            if !nodes.contains_key(&n.port.node) || n.port.port.is_empty() || wired.contains(&n.port) {
                continue;
            }
            if !names.insert(name.clone()) || !ports.insert(n.port.clone()) {
                continue;
            }
            n.name = name;
            n.doc = clean_doc(&n.doc);
            n.min = n.min.filter(|v| v.is_finite());
            n.max = n.max.filter(|v| v.is_finite());
            if let (Some(lo), Some(hi)) = (n.min, n.max)
                && lo > hi
            {
                n.min = None;
                n.max = None;
            }
            keep.push(n);
        }
        self.named = keep;
    }

    /// [`Graph::prune_named`], and with the catalogue: each port is one of its node's inputs and
    /// holds a value. Returns the names dropped.
    pub fn prune_named_for(&mut self, catalog: &SchemaReply) -> Vec<String> {
        self.prune_named();
        let mut gone = Vec::new();
        let nodes = &self.nodes;
        self.named.retain(|n| {
            let ok = nodes.get(&n.port.node).is_some_and(|node| {
                catalog
                    .node(&node.kind)
                    .is_some_and(|def| crate::ports::resolve_input(def, node, &n.port.node, &n.port.port).is_ok_and(|r| r.port.has_widget()))
            });
            if !ok {
                gone.push(n.name.clone());
            }
            ok
        });
        gone
    }

    /// Name input `port` `name`, with a doc line. Refused for a bad name, a name taken, a port
    /// already named, a port that is wired, or one that holds no value.
    pub fn name_port(&mut self, catalog: &SchemaReply, port: PortRef, name: &str, doc: &str) -> Result<()> {
        let name = check_port_name(name).map_err(GraphError::BadPortName)?;
        if self.named_port(&name).is_some() {
            return Err(GraphError::BadPortName(format!("{name:?} already names a port")));
        }
        if let Some(had) = self.name_of_port(&port) {
            return Err(GraphError::BadPortName(format!("{port} is already named {had:?}")));
        }
        let node = self.nodes.get(&port.node).ok_or_else(|| GraphError::UnknownNode(port.node.clone()))?;
        let def = catalog.node(&node.kind).ok_or_else(|| GraphError::UnknownKind(node.kind.clone()))?;
        let input = crate::ports::resolve_input(def, node, &port.node, &port.port)?.port;
        if !input.has_widget() {
            return Err(GraphError::BadPortName(format!("{port} holds no value to set (it takes a wire only)")));
        }
        if self.wires.iter().any(|w| w.to == port) {
            return Err(GraphError::BadPortName(format!("{port} is wired: its wire would win over any value set by name")));
        }
        self.named.push(NamedPort { name, port, doc: clean_doc(doc), min: None, max: None });
        self.touch();
        Ok(())
    }

    /// Remove the name `name`; the port and its value stay.
    pub fn unname_port(&mut self, name: &str) -> Result<NamedPort> {
        let i = self.named.iter().position(|n| n.name == name).ok_or_else(|| GraphError::BadPortName(format!("no port is named {name:?}")))?;
        let gone = self.named.remove(i);
        self.touch();
        Ok(gone)
    }

    /// Call the port named `name` `to` instead.
    pub fn rename_port(&mut self, name: &str, to: &str) -> Result<()> {
        let to = check_port_name(to).map_err(GraphError::BadPortName)?;
        let i = self.named.iter().position(|n| n.name == name).ok_or_else(|| GraphError::BadPortName(format!("no port is named {name:?}")))?;
        if to != name && self.named_port(&to).is_some() {
            return Err(GraphError::BadPortName(format!("{to:?} already names a port")));
        }
        self.named[i].name = to;
        self.touch();
        Ok(())
    }

    /// Change a named port's doc line.
    pub fn set_port_doc(&mut self, name: &str, doc: &str) -> Result<()> {
        let n = self.named.iter_mut().find(|n| n.name == name).ok_or_else(|| GraphError::BadPortName(format!("no port is named {name:?}")))?;
        n.doc = clean_doc(doc);
        self.touch();
        Ok(())
    }

    /// Put the named ports in the order `names` gives; every name exactly once.
    pub fn reorder_named(&mut self, names: &[String]) -> Result<()> {
        let have: BTreeSet<&str> = self.named.iter().map(|n| n.name.as_str()).collect();
        let want: BTreeSet<&str> = names.iter().map(String::as_str).collect();
        if names.len() != self.named.len() || have != want {
            return Err(GraphError::BadPortName("the order must list every named port once".into()));
        }
        self.named.sort_by_key(|n| names.iter().position(|m| m == &n.name));
        self.touch();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeId;
    use plumcraft_proto::catalogue::{NodeDef, Port, Socket};
    use serde_json::json;

    fn catalog() -> SchemaReply {
        let p = |name: &str, socket: Socket| Port { name: name.into(), ty: "STRING".into(), socket: Some(socket), ..Port::default() };
        SchemaReply {
            nodes: vec![NodeDef {
                id: "t.n".into(),
                inputs: vec![p("path", Socket::Widget), p("text", Socket::Both), p("feed", Socket::Socket)],
                outputs: vec![Port { name: "out".into(), ty: "STRING".into(), ..Port::default() }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn two() -> (Graph, NodeId, NodeId) {
        let c = catalog();
        let mut g = Graph::new();
        let a = g.add_node(c.node("t.n").unwrap(), [0.0, 0.0]);
        let b = g.add_node(c.node("t.n").unwrap(), [200.0, 0.0]);
        (g, a, b)
    }

    #[test]
    fn a_port_is_named_renamed_ordered_and_unnamed() {
        let c = catalog();
        let (mut g, a, b) = two();
        g.name_port(&c, PortRef::new(a.clone(), "path"), "  input ", "The picture").unwrap();
        g.name_port(&c, PortRef::new(b.clone(), "text"), "caption", "").unwrap();
        assert_eq!(g.named_port("input").unwrap().port, PortRef::new(a.clone(), "path"));
        assert_eq!(g.name_of_port(&PortRef::new(b.clone(), "text")), Some("caption"));
        g.rename_port("caption", "title").unwrap();
        g.reorder_named(&["title".into(), "input".into()]).unwrap();
        assert_eq!(g.named.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["title", "input"]);
        // round trip through a file
        let back: Graph = Graph::from_json(&serde_json::to_string(&g).unwrap()).unwrap();
        assert_eq!(back.named, g.named);
        g.unname_port("title").unwrap();
        assert_eq!(g.named.len(), 1);
        // a graph with none writes no field
        g.unname_port("input").unwrap();
        assert!(!serde_json::to_string(&g).unwrap().contains("named"));
    }

    #[test]
    fn bad_names_and_ports_are_refused() {
        let c = catalog();
        let (mut g, a, b) = two();
        for bad in ["", "   ", "a=b", "node.port", "x\ny", &"n".repeat(MAX_NAME + 1)] {
            assert!(g.name_port(&c, PortRef::new(a.clone(), "path"), bad, "").is_err(), "{bad:?}");
        }
        assert!(g.name_port(&c, PortRef::new(a.clone(), "feed"), "f", "").is_err(), "a wire-only input holds no value");
        assert!(g.name_port(&c, PortRef::new(a.clone(), "nope"), "f", "").is_err());
        assert!(g.name_port(&c, PortRef::new(a.clone(), "out"), "f", "").is_err(), "an output");
        g.name_port(&c, PortRef::new(a.clone(), "path"), "input", "").unwrap();
        assert!(g.name_port(&c, PortRef::new(b.clone(), "path"), "input", "").is_err(), "a name twice");
        assert!(g.name_port(&c, PortRef::new(a.clone(), "path"), "other", "").is_err(), "a port twice");
        g.connect(&c, PortRef::new(a.clone(), "out"), PortRef::new(b.clone(), "text")).unwrap();
        assert!(g.name_port(&c, PortRef::new(b.clone(), "text"), "t", "").is_err(), "a wired port");
    }

    #[test]
    fn wiring_a_named_port_is_refused_naming_it() {
        let c = catalog();
        let (mut g, a, b) = two();
        g.name_port(&c, PortRef::new(b.clone(), "text"), "caption", "").unwrap();
        let e = g.connect(&c, PortRef::new(a, "out"), PortRef::new(b, "text")).unwrap_err();
        assert!(e.to_string().contains("caption"), "{e}");
    }

    #[test]
    fn a_hostile_file_keeps_its_graph_and_loses_bad_entries() {
        let (g, a, b) = two();
        let mut v = serde_json::to_value(&g).unwrap();
        v["named"] = json!([
            {"name": "ok", "port": {"node": a.0, "port": "path"}, "doc": "fine\u{0007}", "min": 3.0, "max": 1.0},
            {"name": "ok", "port": {"node": b.0, "port": "path"}},
            {"name": "a.b", "port": {"node": b.0, "port": "path"}},
            {"name": "x=y", "port": {"node": b.0, "port": "text"}},
            {"name": "gone", "port": {"node": "n99", "port": "path"}},
            {"name": "same", "port": {"node": a.0, "port": "path"}},
            {"name": "fine", "port": {"node": b.0, "port": "text"}, "max": null}
        ]);
        let back = Graph::from_json(&v.to_string()).unwrap();
        let names: Vec<&str> = back.named.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["ok", "fine"]);
        assert_eq!(back.named[0].doc, "fine");
        assert_eq!((back.named[0].min, back.named[0].max), (None, None), "a range upside down is dropped");
        // a deleted node takes its name with it
        let mut back = back;
        back.remove_node(&b).unwrap();
        assert_eq!(back.named.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["ok"]);
        // a catalogue drops a port that holds no value
        back.named.push(NamedPort { name: "w".into(), port: PortRef::new(a.clone(), "feed"), doc: String::new(), min: None, max: None });
        assert_eq!(back.prune_named_for(&catalog()), ["w"]);
        for junk in [json!("x"), json!([1, null, {"name": 3}]), json!({"name": "a"})] {
            let mut v = serde_json::to_value(&g).unwrap();
            v["named"] = junk.clone();
            let _ = Graph::from_json(&v.to_string());
        }
    }
}
