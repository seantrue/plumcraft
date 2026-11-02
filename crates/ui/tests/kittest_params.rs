//! The parameter editor, driven through egui_kittest: one node's inputs in sections, with the
//! widgets a node has no room for, a filter, per-field reset, and preset fills.

mod common;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_ui::EditorApp;
use serde_json::{Value, json};

use common::{add, click_at, click_in_node, harness, session};

fn open() -> (Harness<'static, EditorApp>, String) {
    let mut s = session();
    let id = add(&mut s, "test.dials", 40.0, 40.0);
    s.execute("node.parameters", &json!({"node": id})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    (h, id)
}

fn has(h: &Harness<'_, EditorApp>, label: &str) -> bool {
    h.query_by_label_contains(label).is_some()
}

/// The value the graph computes with.
fn current(h: &Harness<'_, EditorApp>, node: &str, port: &str) -> Value {
    let s = &h.state().session;
    let n = s.graph().node(&node.into()).unwrap();
    let def = s.catalog().node(&n.kind).unwrap();
    plumcraft_graph::ports::current_value(def, n, port, def.input(port).unwrap())
}

#[test]
fn inputs_are_grouped_with_the_spare_fields_last() {
    let (h, _) = open();
    for section in ["Inputs  (1)", "Parameters  (", "Strokes  (3)", "More fields  (1)"] {
        assert!(has(&h, section), "no section {section}");
    }
    assert!(has(&h, "Everything the parameter editor draws."), "the description's first paragraph");
    assert!(!has(&h, "The rest is on hover."));
    assert!(has(&h, "not wired"));
}

#[test]
fn the_filter_narrows_the_rows_and_clears_for_another_node() {
    let (mut h, id) = open();
    h.state_mut().params.filter = "wid".into();
    h.run_steps(3);
    assert_eq!(h.state().params.rects.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["width"]);
    let other = h.state_mut().session.execute("node.add", &json!({"kind": "test.knobs", "pos": [400, 40]})).unwrap();
    h.state_mut().session.execute("node.parameters", &json!({"node": other["id"]})).unwrap();
    h.run_steps(3);
    assert!(h.state().params.filter.is_empty());
    let _ = id;
}

#[test]
fn a_choice_fills_its_ports_and_a_reset_returns_to_the_choices_value() {
    let (mut h, id) = open();
    h.get_by_label("broad").click();
    h.run_steps(4);
    assert_eq!(current(&h, &id, "preset"), json!("broad"));
    assert_eq!(current(&h, &id, "width"), json!(8.0), "the choice's value");
    h.state_mut().session.execute("node.set", &json!({"node": id, "port": "width", "value": 3.0})).unwrap();
    h.run_steps(3);
    assert_eq!(current(&h, &id, "width"), json!(3.0));
    let resets: Vec<_> = h.get_all_by_label("↺").collect();
    assert_eq!(resets.len(), 2, "what differs has a reset: the choice and the edited field");
    resets[1].click();
    h.run_steps(4);
    assert_eq!(current(&h, &id, "width"), json!(8.0), "back to the choice's, not the port's default");
    assert_eq!(h.get_all_by_label("↺").count(), 1);
}

#[test]
fn a_switch_is_one_undoable_edit() {
    let (mut h, id) = open();
    let before = h.state().session.journal().len();
    let cell = h.state().params.rects.iter().find(|(k, _)| k == "strokes.on").unwrap().1;
    click_at(&mut h, cell.left_center() + egui::vec2(12.0, 0.0));
    h.run_steps(3);
    assert_eq!(current(&h, &id, "strokes.on"), json!(false));
    assert_eq!(h.state().session.journal().len(), before + 1);
}

#[test]
fn a_setting_not_read_now_says_when_it_is() {
    let (mut h, id) = open();
    let d = h.state().session.catalog().node("test.dials").unwrap();
    let mut port = d.input("strokes.size").unwrap().clone();
    port.active_when = Some([("strokes.on".to_owned(), vec![json!(false)])].into());
    let note = plumcraft_ui::widgets::inactive_note(&port, |k| Some(current(&h, &id, k)));
    assert_eq!(note.as_deref(), Some("Not used now: read only when strokes.on is false."));
    h.state_mut().session.execute("node.set", &json!({"node": id, "port": "strokes.on", "value": false})).unwrap();
    assert_eq!(plumcraft_ui::widgets::inactive_note(&port, |k| Some(current(&h, &id, k))), None);
}

#[test]
fn several_picks_keep_their_order() {
    let (mut h, id) = open();
    h.get_by_label("z").click();
    h.run_steps(3);
    h.get_by_label("x").click();
    h.run_steps(3);
    assert_eq!(current(&h, &id, "picks"), json!("z, x"), "in the order picked");
}

#[test]
fn closing_the_window_is_a_command_and_a_gone_node_closes_it() {
    let (mut h, id) = open();
    assert!(h.state().session.views.parameters.is_some());
    h.state_mut().session.execute("node.remove", &json!({"nodes": [id]})).unwrap();
    h.run_steps(4);
    assert!(h.state().session.views.parameters.is_none());
}

#[test]
fn the_canvas_tucks_what_the_choice_doesnt_use() {
    let mut s = session();
    let id = add(&mut s, "test.dials", 40.0, 40.0);
    let mut h = harness(EditorApp::new(s));
    h.run_steps(3);
    assert!(has(&h, "1 more field"));
    assert!(h.query_by_label("spare").is_none(), "tucked");
    click_in_node(&mut h, "1 more field");
    h.run_steps(4);
    assert!(h.state().session.graph().node(&id.as_str().into()).unwrap().all_fields);
    assert!(has(&h, "Fewer fields") && has(&h, "spare"));
}
