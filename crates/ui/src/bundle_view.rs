//! Parameter bundles on the canvas and in the parameter editor: which of a node's values it
//! inherits from the bundle wired into its `flags.bundle_port` input, which a wire overrides, and
//! the actions to detach the node from the bundle (`bundle.detach`, copying the values in) or to
//! reset an override (`bundle.reset`).
//!
//! Precedence, per parameter: a wire into the port, then the bundle, then the widget, then blank
//! (spec 7.6 rule 5). The run's provenance says which applied; a node never run, or edited since,
//! has only what the graph says.
//!
//! Lifted from the research software's `crates/ui-egui/src/bundle_view.rs`. What a bundle is called comes from
//! the catalogue.

use plumcraft_graph::{Graph, NodeId, PortRef};
use plumcraft_proto::catalogue::{Port, SchemaReply};
use plumcraft_session::bundle_cmds::{bundle_name, has_bundle, static_bundle_value};
use plumcraft_session::run::RunState;
use serde_json::{Value, json};

use crate::canvas::Action;
use crate::theme::Tokens;

/// How a parameter gets its value.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// From the bundle (with its value, when the graph knows it).
    Inherited(Option<Value>),
    /// A wire into the port overrides the bundle.
    Overridden,
    /// Not bundled: the widget, a plain wire, or the default.
    Own,
}

/// Where the latest run took `key` from, while that still holds: an edit since says nothing.
pub fn provenance<'a>(run: &'a RunState, node: &NodeId, key: &str) -> Option<&'a str> {
    run.node(&node.0).filter(|n| !n.is_stale()).and_then(|n| n.provenance.get(key)).map(String::as_str)
}

/// Where parameter `at` (port `port`) gets its value: the run's provenance when there is one,
/// else what the graph says.
pub fn source(graph: &Graph, catalog: &SchemaReply, at: &PortRef, port: &Port, provenance: Option<&str>) -> Source {
    let Some(param) = &port.param_ref else { return Source::Own };
    if !has_bundle(graph, catalog, &at.node) {
        return Source::Own;
    }
    let wired = graph.wire_into(at).is_some();
    match provenance {
        Some("bundle") => Source::Inherited(static_bundle_value(graph, catalog, &at.node, param)),
        Some("wire") if wired => Source::Overridden,
        Some(_) => Source::Own,
        None if wired => Source::Overridden,
        None => static_bundle_value(graph, catalog, &at.node, param).map_or(Source::Own, |v| Source::Inherited(Some(v))),
    }
}

/// What `node`'s bundle is called, in lower case for the middle of a sentence.
pub fn bundle_word(graph: &Graph, catalog: &SchemaReply, node: &NodeId) -> String {
    graph.node(node).and_then(|n| catalog.node(&n.kind)).and_then(bundle_name).map_or_else(|| "bundle".to_owned(), |n| n.to_lowercase())
}

/// After a wired row: an "overrides …" badge whose button resets the parameter to the bundle's
/// value. Returns the command when clicked.
pub fn override_badge(ui: &mut egui::Ui, tokens: &Tokens, word: &str, at: &PortRef) -> Option<Action> {
    ui.label(egui::RichText::new(format!("overrides {word}")).small().color(tokens.accent))
        .on_hover_text(format!("This wire overrides the value in the {word} wired into this node"));
    let r = ui.small_button("Reset").on_hover_text("Reset to inherited: remove the wire, so the bundle's value applies");
    r.clicked().then(|| ("bundle.reset", json!({"node": at.node, "port": at.port})))
}

/// The "Detach from …" button, on a node with a bundle wired in.
pub fn detach_button(ui: &mut egui::Ui, graph: &Graph, catalog: &SchemaReply, node: &NodeId) -> Option<Action> {
    if !has_bundle(graph, catalog, node) {
        return None;
    }
    let name = graph.node(node).and_then(|n| catalog.node(&n.kind)).and_then(bundle_name)?;
    let hover = format!("Copy the {}'s values into this node's widgets and disconnect its {} input", name.to_lowercase(), name.to_lowercase());
    ui.button(format!("Detach from {name}")).on_hover_text(hover).clicked().then(|| ("bundle.detach", json!({"node": node})))
}

#[cfg(test)]
mod tests {
    use plumcraft_graph::Graph;
    use plumcraft_proto::catalogue::Port;
    use plumcraft_session::Session;

    use super::*;

    fn catalog() -> SchemaReply {
        serde_json::from_value(json!({"nodes": [
            {"id": "t.ink", "display": "Ink", "flags": {"bundle_port": "tuning"},
             "inputs": [{"name": "tuning", "type": "x.t.tune"},
                        {"name": "ink.width", "type": "FLOAT", "default": 1.0, "param_ref": {"group": "ink", "name": "width"}},
                        {"name": "gain", "type": "FLOAT", "default": 0.5}],
             "outputs": [{"name": "tuning", "type": "x.t.tune"}, {"name": "level", "type": "FLOAT"}]}]}))
        .unwrap()
    }

    fn input(c: &SchemaReply, name: &str) -> Port {
        c.node("t.ink").unwrap().input(name).unwrap().clone()
    }

    #[test]
    fn sources_follow_precedence() {
        let c = catalog();
        let mut s = Session::new(c.clone());
        let mut add = || s.execute("node.add", &json!({"kind": "t.ink"})).unwrap()["id"].as_str().unwrap().to_owned();
        let (up, down, src) = (add(), add(), add());
        let at = PortRef::new(down.as_str(), "ink.width");
        let port = input(&c, "ink.width");
        let of = |g: &Graph, p: Option<&str>| source(g, &c, &at, &port, p);
        assert_eq!(of(s.graph(), None), Source::Own, "no bundle");
        assert_eq!(of(s.graph(), Some("bundle")), Source::Own, "a run from before a detach says nothing");
        s.execute("node.set", &json!({"node": up, "port": "ink.width", "value": 5.0})).unwrap();
        s.execute("wire.connect", &json!({"from": {"node": up, "port": "tuning"}, "to": {"node": down, "port": "tuning"}})).unwrap();
        assert_eq!(of(s.graph(), None), Source::Inherited(Some(json!(5.0))));
        assert_eq!(of(s.graph(), Some("bundle")), Source::Inherited(Some(json!(5.0))));
        assert_eq!(of(s.graph(), Some("widget")), Source::Own);
        s.execute("wire.connect", &json!({"from": {"node": src, "port": "level"}, "to": {"node": down, "port": "ink.width"}})).unwrap();
        assert_eq!(of(s.graph(), None), Source::Overridden);
        assert_eq!(of(s.graph(), Some("wire")), Source::Overridden);
        // a port that isn't in the bundle is always its own
        let gain = input(&c, "gain");
        assert_eq!(source(s.graph(), &c, &PortRef::new(down.as_str(), "gain"), &gain, None), Source::Own);
    }
}
