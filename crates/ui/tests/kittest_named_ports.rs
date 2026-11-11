//! Named ports in the editor (docs/research/23-named-ports.md 3): an input's right-click menu
//! names it through the name editor, the canvas shows the name on the input, and the menu then
//! renames and removes it. (Wiring a named port is refused: `plumcraft_graph::named`'s tests.)

mod common;

use common::{add, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::PortRef;
use plumcraft_ui::EditorApp;

fn build(s: plumcraft_session::Session) -> Harness<'static, EditorApp> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, app: &mut EditorApp| app.show(ui), EditorApp::new(s));
    h.run_steps(6);
    h
}

/// A real right-click at the caption's centre (the menu opens on a secondary click).
fn right_click(h: &mut Harness<'_, EditorApp>, label: &str) {
    // the caption, not the text field it labels: the leftmost widget with that label
    // the caption, not the text field it labels: the leftmost widget with that label. Its rect
    // is in the canvas layer's own coordinates; the pointer is on the screen.
    let local = h.query_all_by_label(label).map(|n| n.rect()).min_by(|a, b| a.left().total_cmp(&b.left())).expect("the caption").center();
    let at = h.state().canvas.to_global * local;
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(3);
}

#[test]
fn an_input_is_named_shown_renamed_and_unnamed_from_its_menu() {
    let mut s = session();
    let r = add(&mut s, "text.replace", 0.0, 0.0);
    let mut h = build(s);
    let at = PortRef::new(r.as_str(), "find");
    // right-click the caption, then Name This Port…
    right_click(&mut h, "find");
    h.get_by_label("Name This Port…").click();
    h.run_steps(3);
    assert_eq!(h.state().canvas.nets.edit.as_ref().map(|e| e.text.as_str()), Some("find"), "the editor suggests the port's name");
    h.state_mut().canvas.nets.edit.as_mut().unwrap().text = "needle".into();
    h.get_by_label("OK").click();
    h.run_steps(4);
    assert_eq!(h.state().session.graph().name_of_port(&at), Some("needle"));
    // drawn on the input
    h.get_by_label("needle →");
    // rename, then remove
    right_click(&mut h, "find");
    h.get_by_label("Rename Port…").click();
    h.run_steps(3);
    h.state_mut().canvas.nets.edit.as_mut().unwrap().text = "query".into();
    h.get_by_label("OK").click();
    h.run_steps(4);
    assert_eq!(h.state().session.graph().name_of_port(&at), Some("query"));
    right_click(&mut h, "find");
    h.get_by_label("Remove Name").click();
    h.run_steps(4);
    assert!(h.state().session.graph().named.is_empty());
}

#[test]
fn the_parameters_window_lists_reorders_and_removes() {
    let mut s = session();
    let r = add(&mut s, "text.replace", 0.0, 0.0);
    for (port, name) in [("find", "needle"), ("with", "thread")] {
        s.execute("graph.port.name", &serde_json::json!({"port": {"node": r, "port": port}, "name": name, "doc": format!("the {port}")})).unwrap();
    }
    s.execute("view.params", &serde_json::json!({"open": true})).unwrap();
    let mut h = build(s);
    h.get_by_label("needle");
    h.get_by_label("thread");
    // move "thread" up
    let downs: Vec<_> = h.query_all_by_label("↑").collect();
    assert_eq!(downs.len(), 2);
    h.query_all_by_label("↑").nth(1).unwrap().click();
    h.run_steps(3);
    let order: Vec<String> = h.state().session.graph().named.iter().map(|n| n.name.clone()).collect();
    assert_eq!(order, ["thread", "needle"]);
    h.query_all_by_label("Remove").next().unwrap().click();
    h.run_steps(3);
    assert_eq!(h.state().session.graph().named.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["needle"]);
}
