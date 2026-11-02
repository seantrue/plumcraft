//! Run plans (docs/research/15-multi-engine.md, section 3.5), on hand-made catalogues: A → B → A
//! is three fragments in order, joined by crossings; branches with no path between them don't
//! wait for each other; a fragment targets the run's targets and every node another fragment
//! needs; the run's own bindings and watch entries go to their node's fragment, and a bound
//! port's wire is dropped (its source isn't run); the fragment graph holds only its nodes and
//! wires, without groups; a pin to an engine without the kind is a graph error. (Pins and
//! preferences on real catalogues: `tests/merge.rs`.)

use std::collections::{BTreeMap, BTreeSet};

use plumcraft_coordinator::merge;
use plumcraft_coordinator::plan::{self, Plan};
use plumcraft_proto::catalogue::{Media, NodeDef, Port, SchemaReply, WireType};
use plumcraft_proto::run::RunRequest;
use serde_json::{Value, json};

/// An engine whose nodes `<id>.f` take and make an `X` (declared alike everywhere).
fn engine(id: &str) -> SchemaReply {
    let port = |n: &str| Port { name: n.into(), ty: "X".into(), optional: true, ..Default::default() };
    let mut s = SchemaReply::default();
    s.engine.id = id.into();
    s.wire_types.push(WireType { id: "X".into(), media: Media::Ndarray, ..Default::default() });
    s.nodes.push(NodeDef { id: format!("{id}.f"), inputs: vec![port("i"), port("j")], outputs: vec![port("o")], ..Default::default() });
    s
}

fn plan_of(g: Value, extra: Value) -> Plan {
    let parts: Vec<(String, SchemaReply)> = ["a", "b"].iter().map(|e| ((*e).to_owned(), engine(e))).collect();
    let merged = merge::catalogue(&parts);
    let mut req = json!({"run_id": "r", "generation": 1, "graph": {"kind": "inline", "graph": g}});
    if let (Some(r), Some(x)) = (req.as_object_mut(), extra.as_object()) {
        r.extend(x.clone());
    }
    let req: RunRequest = serde_json::from_value(req).unwrap();
    plan::build(&req, &merged, &parts.into_iter().collect::<BTreeMap<_, _>>()).unwrap()
}

fn graph(nodes: &[(&str, &str)], wires: &[(&str, &str)]) -> Value {
    let nodes: serde_json::Map<String, Value> = nodes.iter().map(|(id, e)| ((*id).to_owned(), json!({"kind": format!("{e}.f")}))).collect();
    let wires: Vec<Value> = wires.iter().map(|(a, b)| json!({"from": {"node": a, "port": "o"}, "to": {"node": b, "port": "i"}})).collect();
    json!({"version": 1, "nodes": nodes, "wires": wires, "groups": {"g1": {"members": ["x"]}}})
}

fn names(v: &Value) -> BTreeSet<String> {
    v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default()
}

#[test]
fn a_to_b_to_a_is_three_fragments_in_order() {
    let p = plan_of(graph(&[("x", "a"), ("y", "b"), ("z", "a"), ("w", "a")], &[("x", "y"), ("y", "z"), ("z", "w")]), json!({}));
    let shape: Vec<(&str, Vec<&str>)> = p.fragments.iter().map(|f| (f.engine.as_str(), f.nodes.iter().map(String::as_str).collect())).collect();
    assert_eq!(shape, [("a", vec!["x"]), ("b", vec!["y"]), ("a", vec!["z", "w"])]);
    assert_eq!(p.crossings.len(), 2);
    assert_eq!((p.fragments[1].after.iter().copied().collect::<Vec<_>>(), p.fragments[2].after.iter().copied().collect::<Vec<_>>()), (vec![0], vec![1]));
    // The last fragment keeps its inside wire; each fragment's targets: the sink, or what crosses.
    assert_eq!(p.fragments[2].graph["wires"].as_array().unwrap().len(), 1);
    assert_eq!(p.fragments[0].targets, ["x"]);
    assert_eq!(p.fragments[1].targets, ["y"]);
    assert_eq!(p.fragments[2].targets, ["w"]);
    assert!(p.fragments.iter().all(|f| f.graph.get("groups").is_none()), "groups stay with the editor");
    assert_eq!(p.order, ["x", "y", "z", "w"]);
}

/// A slice inside a fragment reaches its engine on the wire; one between fragments stays with
/// the crossing, which the coordinator applies.
#[test]
fn slices_stay_on_their_wires() {
    let mut g = graph(&[("x", "a"), ("y", "a"), ("z", "b")], &[("x", "y"), ("y", "z")]);
    for (w, s) in g["wires"].as_array_mut().unwrap().iter_mut().zip(["1:", "-1"]) {
        w["slice"] = json!(s);
    }
    let p = plan_of(g, json!({}));
    assert_eq!(p.fragments[0].graph["wires"][0]["slice"], json!("1:"));
    assert_eq!(p.crossings[0].slice.as_deref(), Some("-1"));
}

#[test]
fn branches_with_no_path_between_them_do_not_wait() {
    // a1 → b1 and a2 → b2, then both into a3: b1 and b2 share a fragment, which waits for the
    // first; a3 waits for that one.
    let p = plan_of(
        graph(&[("a1", "a"), ("b1", "b"), ("a2", "a"), ("b2", "b"), ("a3", "a")], &[("a1", "b1"), ("a2", "b2"), ("b1", "a3"), ("b2", "a3")]),
        json!({}),
    );
    assert_eq!(p.fragments.len(), 3);
    assert_eq!(names(&p.fragments[0].graph["nodes"]), ["a1", "a2"].map(String::from).into());
    assert_eq!(names(&p.fragments[1].graph["nodes"]), ["b1", "b2"].map(String::from).into());
    assert!(p.fragments[0].after.is_empty());
    // Two independent chains on different engines: two fragments, neither waits.
    let p = plan_of(graph(&[("x", "a"), ("y", "b")], &[]), json!({}));
    assert_eq!(p.fragments.len(), 2);
    assert!(p.fragments.iter().all(|f| f.after.is_empty()));
}

#[test]
fn bindings_watch_and_targets_go_to_their_fragment() {
    let bound = json!({"kind": "json", "value": 1});
    let p = plan_of(
        graph(&[("x", "a"), ("y", "b"), ("z", "b")], &[("x", "y"), ("y", "z")]),
        json!({"targets": ["z"], "inputs": {"y.i": bound, "nobody.i": bound}, "watch": [{"node": "z", "port": "o"}, {"node": "y", "port": "o"}]}),
    );
    // y's input is bound, so the wire from x is dropped, and x isn't computed.
    assert_eq!(p.fragments.len(), 1, "{p:?}");
    let f = &p.fragments[0];
    assert_eq!(f.engine, "b");
    assert_eq!(f.inputs.keys().collect::<Vec<_>>(), ["y.i"]);
    assert_eq!(f.watch.iter().map(|(k, w)| (*k, w.node.as_str())).collect::<Vec<_>>(), [(0, "z"), (1, "y")]);
    assert_eq!(f.targets, ["z"]);
    assert!(p.crossings.is_empty());
}

#[test]
fn a_pin_to_an_engine_without_the_kind_is_a_graph_error() {
    let parts: Vec<(String, SchemaReply)> = ["a", "b"].iter().map(|e| ((*e).to_owned(), engine(e))).collect();
    let merged = merge::catalogue(&parts);
    let req: RunRequest =
        serde_json::from_value(json!({"run_id": "r", "graph": {"kind": "inline", "graph": {"nodes": {"x": {"kind": "a.f", "engine": "b"}}, "wires": []}}}))
            .unwrap();
    let e = plan::build(&req, &merged, &parts.into_iter().collect()).unwrap_err();
    assert!(e.message.contains("engine b has no node a.f"), "{}", e.message);
}
