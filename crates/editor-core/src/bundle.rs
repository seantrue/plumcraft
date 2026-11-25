//! Parameter bundles: which of a node's values it inherits from the bundle wired into its
//! `flags.bundle_port` input, and which a wire overrides. The pure part of the egui shell's
//! `bundle_view.rs`, moved here in N2 slice 2 for the scene ([`crate::scene`]); `plumcraft-ui`
//! re-exports it.
//!
//! Precedence, per parameter: a wire into the port, then the bundle, then the widget, then blank
//! (spec 7.6 rule 5). The run's provenance says which applied; a node never run, or edited since,
//! has only what the graph says.

use plumcraft_graph::{Graph, NodeId, PortRef};
use plumcraft_proto::catalogue::{Port, SchemaReply};
use plumcraft_session::bundle_cmds::{bundle_name, has_bundle, static_bundle_value};
use plumcraft_session::run::RunState;
use serde_json::Value;

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
