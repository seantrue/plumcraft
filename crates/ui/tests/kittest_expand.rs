//! Expanding a command in the editor, driven as a person would: its menu offers Expand only
//! where the catalogue has an expansion, the node is replaced by the nodes it is made of, what
//! couldn't come across is said in the status bar, and Undo brings the command back.

mod common;

use common::{add, click, harness};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::{GroupId, PortRef};
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use serde_json::json;

fn catalog() -> SchemaReply {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "t.src", "display": "Source", "inputs": [], "outputs": [{"name": "text", "type": "TEXT"}]},
            {"id": "t.cmd", "display": "Steps", "inputs": [
                {"name": "text", "type": "TEXT", "socket": "socket"},
                {"name": "steps.gain", "type": "FLOAT", "default": 1.0, "socket": "both"}],
             "outputs": [{"name": "text", "type": "TEXT"}, {"name": "note", "type": "TEXT"}]},
            {"id": "t.step", "display": "Step", "inputs": [
                {"name": "text", "type": "TEXT", "socket": "socket", "optional": true},
                {"name": "gain", "type": "FLOAT", "default": 0.0, "socket": "both"}],
             "outputs": [{"name": "text", "type": "TEXT"}]},
            {"id": "t.end", "display": "End", "inputs": [{"name": "text", "type": "TEXT", "socket": "socket", "optional": true}], "outputs": []}],
        "expansions": {"t.cmd": {
            "nodes": {"one": {"kind": "t.step", "pos": [0, 0]}, "two": {"kind": "t.step", "pos": [300, 0]}},
            "wires": [{"from": {"node": "one", "port": "text"}, "to": {"node": "two", "port": "text"}}],
            "inputs": {"text": [["one", "text"]]}, "settings": {"steps.gain": [["two", "gain"]]},
            "outputs": {"text": ["two", "text"]}}}}))
    .unwrap()
}

/// Source → Steps → End, gain 4.
fn chain() -> (Session, String, String, String) {
    let mut s = Session::new(catalog());
    let src = add(&mut s, "t.src", 20.0, 20.0);
    let cmd = add(&mut s, "t.cmd", 300.0, 20.0);
    let end = add(&mut s, "t.end", 640.0, 20.0);
    s.execute("wire.connect", &json!({"from": {"node": src, "port": "text"}, "to": {"node": cmd, "port": "text"}})).unwrap();
    s.execute("wire.connect", &json!({"from": {"node": cmd, "port": "text"}, "to": {"node": end, "port": "text"}})).unwrap();
    s.execute("node.set", &json!({"node": cmd, "port": "steps.gain", "value": 4.0})).unwrap();
    (s, src, cmd, end)
}

/// Open the menu of the `which`th node from the left, on the top row.
fn open_menu(h: &mut Harness<'static, EditorApp>, which: usize) {
    let mut buttons: Vec<egui::Rect> = h.get_all_by_label("Node actions").map(|n| n.rect()).filter(|r| r.min.y < 200.0).collect();
    buttons.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    common::click_canvas(h, buttons[which]);
    h.run_steps(3);
}

fn wires_from(h: &Harness<'_, EditorApp>, node: &str) -> usize {
    h.state().session.graph().wires.iter().filter(|w| w.from.node.0 == node).count()
}

#[test]
fn expand_replaces_the_command_with_the_nodes_it_is_made_of_and_undo_brings_it_back() {
    let (s, src, cmd, end) = chain();
    let mut h = harness(EditorApp::new(s));
    h.state_mut().canvas.request_fit();
    h.run_steps(4);
    assert_eq!(h.query_all_by_label("Step").count(), 0);
    open_menu(&mut h, 1);
    click(&mut h, "Expand into Group Steps");
    h.run_steps(4);

    let g = h.state().session.graph();
    assert!(g.node(&cmd.as_str().into()).is_none(), "the command is gone");
    assert_eq!(g.nodes.len(), 4);
    let (gid, grp) = g.groups.iter().next().expect("a group");
    assert_eq!(grp.name, "Steps");
    let two = &grp.promoted[0].port.node;
    assert_eq!(g.node(two).unwrap().values["gain"], json!(4.0), "the command's value came across");
    let into_end = g.wire_into(&PortRef::new(end.as_str(), "text")).expect("End is still fed");
    assert_eq!(&into_end.from.node, two);
    assert_eq!(wires_from(&h, &src), 1, "Source feeds the first step");
    assert_eq!(h.query_all_by_label("Step 2").count(), 1, "and the canvas shows them, with their serials: {:?}", h.state().session.graph().nodes);
    assert_eq!(h.state().session.undo_label(), Some("Expand Steps"));
    assert!(h.state().last_error.is_none(), "{:?}", h.state().last_error);
    let gid: GroupId = gid.clone();

    h.state_mut().run("edit.undo", &json!(null)).unwrap();
    h.run_steps(4);
    let g = h.state().session.graph();
    assert!(g.node(&cmd.as_str().into()).is_some(), "one undo step brings it back");
    assert!(g.group(&gid).is_none());
    assert_eq!(wires_from(&h, &cmd), 1);
}

#[test]
fn only_a_node_with_an_expansion_offers_it() {
    let (s, ..) = chain();
    let mut h = harness(EditorApp::new(s));
    open_menu(&mut h, 0);
    assert!(h.query_by_label("Rename…").is_some(), "the menu is open");
    assert!(h.query_by_label_contains("Expand").is_none(), "Source has no expansion");
}

#[test]
fn a_wire_that_cannot_come_across_is_said_in_the_status_bar() {
    let (mut s, _, cmd, _) = chain();
    let other = add(&mut s, "t.end", 640.0, 300.0);
    s.execute("wire.connect", &json!({"from": {"node": cmd, "port": "note"}, "to": {"node": other, "port": "text"}})).unwrap();
    let mut h = harness(EditorApp::new(s));
    open_menu(&mut h, 1);
    click(&mut h, "Expand without a Group");
    h.run_steps(4);
    assert!(h.state().session.graph().groups.is_empty());
    assert_eq!(h.state().session.graph().nodes.len(), 5);
    let said = h.state().last_error.clone().expect("the status bar says so");
    assert!(said.contains("1 wire(s) dropped") && said.contains("no output for note"), "{said}");
    assert!(h.query_by_label_contains("1 wire(s) dropped").is_some(), "on screen");
}

#[test]
fn the_node_menu_expands_the_selected_node() {
    let (s, _, cmd, _) = chain();
    let mut h = harness(EditorApp::new(s));
    h.state_mut().canvas.picked.set(vec![cmd.as_str().into()]);
    h.run_steps(3);
    click(&mut h, "Node");
    h.run_steps(2);
    click(&mut h, "Expand");
    h.run_steps(4);
    assert!(h.state().session.graph().node(&cmd.as_str().into()).is_none());
    assert_eq!(h.state().session.graph().nodes.len(), 4);
}
