//! Parameter bundles in the editor, driven as a person would: a value from a bundle shows as the
//! bundle's, on the node and in the parameter editor; a wire over it gets a badge whose Reset
//! removes the wire; Detach copies the values in and the widgets come back. The bundle input here
//! isn't called `settings`, and the catalogue is what says so.

mod common;

use std::time::Instant;

use common::{add, click, click_in_node, harness};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::PortRef;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::event::NodeState;
use plumcraft_proto::run::{NodeReport, RunReply};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use serde_json::json;

fn catalog() -> SchemaReply {
    serde_json::from_value(json!({
        "wire_types": [{"id": "x.t.tune", "media": "tree"}],
        "nodes": [
            {"id": "t.ink", "display": "Ink", "category": ["T"], "flags": {"bundle_port": "tuning"},
             "inputs": [{"name": "tuning", "type": "x.t.tune", "socket": "socket", "optional": true},
                        {"name": "ink.width", "type": "FLOAT", "default": 1.0, "param_ref": {"group": "ink", "name": "width"}},
                        {"name": "gain", "type": "FLOAT", "default": 0.5}],
             "outputs": [{"name": "tuning", "type": "x.t.tune"}, {"name": "level", "type": "FLOAT"}]}]}))
    .unwrap()
}

/// Up (width 7) → Down, by the bundle input; and a spare node to wire from.
fn bundled() -> (Session, String, String, String) {
    let mut s = Session::new(catalog());
    let up = add(&mut s, "t.ink", 20.0, 20.0);
    let down = add(&mut s, "t.ink", 500.0, 20.0);
    let spare = add(&mut s, "t.ink", 20.0, 400.0);
    s.execute("node.set", &json!({"node": up, "port": "ink.width", "value": 7.0})).unwrap();
    s.execute("node.set", &json!({"node": down, "port": "ink.width", "value": 2.0})).unwrap();
    s.execute("wire.connect", &json!({"from": {"node": up, "port": "tuning"}, "to": {"node": down, "port": "tuning"}})).unwrap();
    (s, up, down, spare)
}

fn has(h: &Harness<'_, EditorApp>, label: &str) -> bool {
    h.query_by_label_contains(label).is_some()
}

fn width_in(h: &Harness<'_, EditorApp>, node: &str) -> serde_json::Value {
    h.state().session.graph().node(&node.into()).unwrap().values["ink.width"].clone()
}

#[test]
fn a_bundled_value_shows_as_the_bundles_in_place_of_the_widget() {
    let (s, _, _, _) = bundled();
    let h = harness(EditorApp::new(s));
    assert!(has(&h, "from tuning"), "the canvas says where it comes from");
    assert_eq!(h.query_all_by_label("7.0").count(), 1, "the bundle's value, once: up's own is a widget");
}

#[test]
fn the_parameter_editor_shows_the_bundles_value_and_detach_gives_the_widget_back() {
    let (mut s, up, down, _) = bundled();
    s.execute("node.parameters", &json!({"node": down})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    assert!(has(&h, "7.0  ← tuning"), "the chip: the bundle's value, not the widget's 2");
    assert!(!h.state().params.rects.iter().any(|(k, _)| k == "ink.width"), "read-only while inherited");
    assert!(h.state().params.rects.iter().any(|(k, _)| k == "gain"), "what isn't in the bundle stays editable");

    click(&mut h, "Detach from Tuning");
    h.run_steps(4);
    assert_eq!(width_in(&h, &down), json!(7.0), "the value came with it");
    assert!(h.state().session.graph().wire_into(&PortRef::new(down.as_str(), "tuning")).is_none(), "the wire went");
    assert!(!has(&h, "← tuning"));
    assert!(!has(&h, "Detach from"), "nothing left to detach from");
    assert!(h.state().params.rects.iter().any(|(k, _)| k == "ink.width"), "the widget is back");
    assert_eq!(h.state().session.undo_label(), Some("Detach from Tuning"));

    h.state_mut().run("edit.undo", &json!(null)).unwrap();
    h.run_steps(4);
    assert_eq!(width_in(&h, &down), json!(2.0), "one undo step");
    assert!(has(&h, "7.0  ← tuning"));
    let _ = up;
}

/// A run's provenance (which outlives the wire it describes).
fn ran(s: &mut Session, nodes: &[(&str, &str, &str)]) {
    s.connected = true;
    let req = s.execute("run.start", &json!({})).unwrap();
    let _ = s.run.take_outbox();
    let mut reply = RunReply { run_id: req["run_id"].as_str().unwrap().to_owned(), generation: s.run.generation, ok: true, ..Default::default() };
    for (id, port, how) in nodes {
        let mut r = NodeReport { state: NodeState::Done, ..Default::default() };
        r.provenance.insert((*port).to_owned(), (*how).to_owned());
        reply.nodes.insert((*id).to_owned(), r);
    }
    assert!(s.run.apply_reply(&reply, Instant::now()));
}

#[test]
fn after_a_run_detach_doesnt_leave_the_field_read_only() {
    let (mut s, _, down, _) = bundled();
    ran(&mut s, &[(&down, "ink.width", "bundle")]);
    s.execute("node.parameters", &json!({"node": down})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    assert!(has(&h, "7.0  ← tuning"));
    click(&mut h, "Detach from Tuning");
    h.run_steps(4);
    assert!(h.state().params.rects.iter().any(|(k, _)| k == "ink.width"), "the run's provenance says bundle, but the bundle is gone");
}

#[test]
fn a_wire_over_a_bundled_value_gets_a_badge_and_reset_removes_the_wire() {
    let (mut s, _, down, spare) = bundled();
    s.execute("wire.connect", &json!({"from": {"node": spare, "port": "level"}, "to": {"node": down, "port": "ink.width"}})).unwrap();
    let mut h = harness(EditorApp::new(s));
    assert!(has(&h, "overrides tuning"), "on the node");
    click_in_node(&mut h, "Reset");
    h.run_steps(4);
    let g = h.state().session.graph();
    assert!(g.wire_into(&PortRef::new(down.as_str(), "ink.width")).is_none(), "the wire went");
    assert!(g.wire_into(&PortRef::new(down.as_str(), "tuning")).is_some(), "the bundle stayed");
    assert!(!has(&h, "overrides tuning"));
    assert!(has(&h, "from tuning"), "and the bundle's value is back");
    assert_eq!(h.state().session.undo_label(), Some("Reset to Inherited"));
}

#[test]
fn the_parameter_editor_has_the_badge_too() {
    let (mut s, _, down, spare) = bundled();
    s.execute("wire.connect", &json!({"from": {"node": spare, "port": "level"}, "to": {"node": down, "port": "ink.width"}})).unwrap();
    s.execute("node.parameters", &json!({"node": down})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    // the node's row and the editor's: the editor's is the window's, on top
    let rects: Vec<egui::Rect> = h.query_all_by_label("Reset").map(|n| n.rect()).collect();
    assert_eq!(rects.len(), 2, "{rects:?}");
    // the window opens beside its node, so pick the button inside the window, not the rightmost
    let window = h.get_by_label_contains("· parameters").rect();
    let inside = rects.iter().map(|r| r.center()).find(|c| window.contains(*c)).expect("a Reset in the window");
    common::click_at(&mut h, inside);
    h.run_steps(4);
    assert!(h.state().session.graph().wire_into(&PortRef::new(down.as_str(), "ink.width")).is_none());
}

#[test]
fn a_port_that_isnt_in_a_bundle_has_no_badge() {
    let (mut s, _, down, spare) = bundled();
    s.execute("wire.connect", &json!({"from": {"node": spare, "port": "level"}, "to": {"node": down, "port": "gain"}})).unwrap();
    let h = harness(EditorApp::new(s));
    assert!(!has(&h, "overrides"));
}

#[test]
fn the_nodes_menu_detaches() {
    let (s, _, down, _) = bundled();
    let mut h = harness(EditorApp::new(s));
    let rightmost = h.get_all_by_label("Node actions").map(|n| n.rect()).max_by(|a, b| a.left().total_cmp(&b.left())).unwrap();
    common::click_canvas(&mut h, rightmost);
    h.run_steps(4);
    h.get_by_label("Detach from Tuning").click();
    h.run_steps(4);
    assert_eq!(width_in(&h, &down), json!(7.0));
    assert!(h.state().session.graph().wire_into(&PortRef::new(down.as_str(), "tuning")).is_none());
}

#[test]
fn no_bundle_flag_no_bundle() {
    let mut c = catalog();
    c.nodes[0].flags.bundle_port = None;
    let mut s = Session::new(c);
    let up = add(&mut s, "t.ink", 20.0, 20.0);
    let down = add(&mut s, "t.ink", 500.0, 20.0);
    s.execute("wire.connect", &json!({"from": {"node": up, "port": "tuning"}, "to": {"node": down, "port": "tuning"}})).unwrap();
    s.execute("node.parameters", &json!({"node": down})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    assert!(!has(&h, "from tuning") && !has(&h, "Detach"), "a port named as one isn't one");
    assert!(h.state().params.rects.iter().any(|(k, _)| k == "ink.width"));
}
