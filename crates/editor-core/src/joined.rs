//! Joined lists (B164): what a wire or an input gets from a step that ran once per item of a
//! list and made a list each run.
//!
//! Every executor joins such a step's lists from all its runs into one list (`docs/interface.md`
//! 7.6 rule 2; nested lists, research note 27, will keep each run's list apart). Two wirings then
//! read that joined list otherwise than a person expects, and the core says so, on the wire's menu
//! and the input row's hover:
//!
//! - **an index slice** (`1`, `-1`) on the wire takes one item of all the runs' items together, so
//!   one run's result goes on, not one from each run;
//! - **a port that takes a whole list** gets every run's items as one list, not a list per run.
//!
//! What the note rests on is the graph and the catalogue alone: the source step's output is a list
//! port (`Port.list`), and a list reaches one of its inputs that takes one item, so it ran once per
//! item. A list reaches an input when the wire into it comes from a list output, or from a step
//! that itself ran once per item, unless the wire's index slice takes one item. The note shows
//! only after the source step has run (done or cached). How many runs and items there were isn't
//! said: the interface carries a list's total only for watched values, and no run counts at all.

use std::collections::HashSet;

use plumcraft_graph::{Graph, NodeId, PortRef, Wire};
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::event::NodeState;
use plumcraft_session::RunState;

/// How deep the search upstream goes (a long chain of steps each run once per item).
const MAX_DEPTH: usize = 64;

/// The note for the wire into `to`, when it reads a list joined from several runs in one of the
/// two ways above; `None` otherwise.
pub fn note(graph: &Graph, catalog: &SchemaReply, run: &RunState, to: &PortRef) -> Option<String> {
    let w = graph.wire_into(to)?;
    let src = &w.from.node;
    let kind = &graph.node(src)?.kind;
    let def = catalog.node(kind)?;
    // the source's output is a list each run, and the source has run
    if !def.outputs.iter().any(|p| p.name == w.from.port && p.list) {
        return None;
    }
    if !run.node(&src.0).and_then(|n| n.state).is_some_and(|s| matches!(s, NodeState::Done | NodeState::Cached)) {
        return None;
    }
    if !mapped(graph, catalog, src, &mut HashSet::new(), 0) {
        return None;
    }
    let title = plumcraft_session::node_title(graph, catalog, src);
    if let Some(spec) = w.slice.as_deref().filter(|s| plumcraft_proto::slice::parse(s).is_ok_and(|p| p.is_index())) {
        return Some(format!(
            "Slice {spec} takes one item. {title} runs once per item of a list and joins every run's list into one, so this is one run's result, not one from each run (B164)."
        ));
    }
    if w.slice.is_none() && takes_whole(graph, catalog, to) {
        return Some(format!("This input takes a whole list, and gets {title}'s runs joined into one list, not a list per run (B164)."));
    }
    None
}

/// Whether input `to` takes a whole list (its port's `list`, or the node's `input_list`).
fn takes_whole(graph: &Graph, catalog: &SchemaReply, to: &PortRef) -> bool {
    let Some(def) = graph.node(&to.node).and_then(|n| catalog.node(&n.kind)) else { return false };
    def.flags.input_list || def.inputs.iter().any(|p| p.name == to.port && p.list)
}

/// Whether node `id` runs once per item: a list reaches one of its inputs that takes one item.
fn mapped(graph: &Graph, catalog: &SchemaReply, id: &NodeId, seen: &mut HashSet<NodeId>, depth: usize) -> bool {
    if depth > MAX_DEPTH || !seen.insert(id.clone()) {
        return false;
    }
    let Some(def) = graph.node(id).and_then(|n| catalog.node(&n.kind)) else { return false };
    if def.flags.input_list {
        return false;
    }
    graph.wires.iter().filter(|w| &w.to.node == id).any(|w| {
        let single = def.inputs.iter().any(|p| p.name == w.to.port && !p.list);
        single && carries_list(graph, catalog, w, seen, depth + 1)
    })
}

/// Whether wire `w` carries a list: from a list output or from a step run once per item, and not
/// cut to one item by an index slice.
fn carries_list(graph: &Graph, catalog: &SchemaReply, w: &Wire, seen: &mut HashSet<NodeId>, depth: usize) -> bool {
    if w.slice.as_deref().is_some_and(|s| plumcraft_proto::slice::parse(s).is_ok_and(|p| p.is_index())) {
        return false;
    }
    let Some(def) = graph.node(&w.from.node).and_then(|n| catalog.node(&n.kind)) else { return false };
    def.outputs.iter().any(|p| p.name == w.from.port && p.list) || mapped(graph, catalog, &w.from.node, seen, depth)
}

#[cfg(test)]
mod tests {
    use plumcraft_session::Session;
    use plumcraft_session::run::NodeRun;
    use serde_json::json;

    use super::*;

    /// A Vary String into Find Files' folder (a step that makes a list each run), then an index
    /// slice into another Find Files' folder and the whole list into Zip; and an ordinary list (Find Files on one
    /// folder) sliced the same way, which stays quiet.
    #[test]
    fn a_slice_or_a_whole_list_port_over_runs_joined_into_one_list_says_so() {
        let mut s = Session::new(plumcraft_text::engine().schema());
        let add = |s: &mut Session, kind: &str| s.execute("node.add", &json!({"kind": kind, "pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
        let (vary, find, upper, zip) = (add(&mut s, "vary.string"), add(&mut s, "file.find"), add(&mut s, "file.find"), add(&mut s, "seq.zip"));
        let (plain, plain_upper) = (add(&mut s, "file.find"), add(&mut s, "file.find"));
        let wire = |s: &mut Session, a: &str, ap: &str, b: &str, bp: &str| {
            s.execute("wire.connect", &json!({"from": {"node": a, "port": ap}, "to": {"node": b, "port": bp}})).unwrap();
        };
        wire(&mut s, &vary, "values", &find, "folder");
        wire(&mut s, &find, "files", &upper, "folder");
        wire(&mut s, &find, "files", &zip, "a");
        wire(&mut s, &plain, "files", &plain_upper, "folder");
        for to in [&upper, &plain_upper] {
            s.execute("wire.slice", &json!({"to": {"node": to, "port": "folder"}, "slice": "1"})).unwrap();
        }
        let at = |n: &str, p: &str| PortRef::new(NodeId::from(n), p);
        let say = |s: &Session, n: &str, p: &str| note(s.graph(), s.catalog(), &s.run, &at(n, p));
        // before a run: nothing
        assert_eq!(say(&s, &upper, "folder"), None);
        for n in [&vary, &find, &plain] {
            s.run.nodes.insert(n.clone(), NodeRun { state: Some(NodeState::Done), ..Default::default() });
        }
        let sliced = say(&s, &upper, "folder").unwrap();
        assert!(sliced.starts_with("Slice 1 takes one item. Find Files 1 runs once per item of a list") && sliced.contains("one run's result"), "{sliced}");
        let whole = say(&s, &zip, "a").unwrap();
        assert!(whole.starts_with("This input takes a whole list, and gets Find Files 1"), "{whole}");
        assert_eq!(say(&s, &plain_upper, "folder"), None, "an ordinary list, sliced: quiet");
        // a range slice isn't one item: quiet
        s.execute("wire.slice", &json!({"to": {"node": upper, "port": "folder"}, "slice": "::2"})).unwrap();
        assert_eq!(say(&s, &upper, "folder"), None);
        // the wire's menu and the input row's hover carry it
        s.execute("wire.slice", &json!({"to": {"node": upper, "port": "folder"}, "slice": "1"})).unwrap();
        let cx = crate::menu::Context::Wire { to: at(&upper, "folder"), bend: None };
        let menu = crate::menu::context(&crate::menu::Parts::of(&s), &crate::menu::Facts::default(), &cx);
        assert!(menu.iter().any(|i| i.label.starts_with("Slice 1 takes one item")), "{:?}", menu.iter().map(|i| &i.label).collect::<Vec<_>>());
    }
}
