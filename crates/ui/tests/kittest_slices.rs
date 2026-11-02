//! Slices on wires (crate::slice_view): the wire menu's Slice…, the label drawn on the wire, what
//! a slice does to staleness, and Clear Slice.

mod common;

use common::add;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::PortRef;
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use serde_json::json;

/// A list -> Upper case, far apart: one wire carrying a list.
fn harness() -> (Harness<'static, EditorApp>, PortRef) {
    let mut c = common::catalog();
    let split = json!({"id": "test.split", "display": "Split", "family": "test",
        "inputs": [], "outputs": [{"name": "parts", "type": "TEXT", "list": true}]});
    c.nodes.push(serde_json::from_value(split).unwrap());
    let mut s = Session::new(c);
    let a = add(&mut s, "test.split", 0.0, 0.0);
    let u = add(&mut s, "text.upper", 700.0, 200.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "parts"}, "to": {"node": u, "port": "text"}})).unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, app: &mut EditorApp| app.show(ui), EditorApp::new(s));
    h.run_steps(6);
    (h, PortRef::new(u.as_str(), "text"))
}

fn press(h: &mut Harness<'_, EditorApp>, at: egui::Pos2, button: egui::PointerButton) {
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(3);
}

fn slice(h: &Harness<'_, EditorApp>, to: &PortRef) -> Option<String> {
    h.state().session.graph().wire_into(to).unwrap().slice.clone()
}

fn mid(h: &Harness<'_, EditorApp>) -> egui::Pos2 {
    let style = h.ctx.global_style();
    let catalog = h.state().session.catalog().clone();
    h.state().canvas.wire_midpoints(&style, &catalog)[0].0
}

#[test]
fn a_wire_is_sliced_from_its_menu_labelled_and_cleared() {
    let (mut h, to) = harness();
    let at = mid(&h);
    press(&mut h, at, egui::PointerButton::Secondary);
    assert!(h.state().canvas.wire_menu.is_some());
    h.get_by_label("Slice…").click();
    h.run_steps(3);
    assert!(h.state().canvas.slice_edit.is_some(), "the editor opens");
    h.state_mut().canvas.slice_edit.as_mut().unwrap().text = "-1".into();
    h.run_steps(2);
    h.get_by_label("Set").click();
    h.run_steps(4);
    assert_eq!(slice(&h, &to).as_deref(), Some("-1"));
    assert_eq!(h.state().session.undo_label(), Some("Slice Wire"));
    assert!(h.query_by_label("[-1]").is_some(), "the label is on the wire");
    // a bad one is refused by the editor, before any command
    h.state_mut().canvas.slice_edit = Some(plumcraft_ui::slice_view::SliceEdit::new(PortRef::new("x", "y"), to.clone(), Some("junk"), at));
    h.run_steps(3);
    h.get_by_label("Set").click();
    h.run_steps(3);
    assert_eq!(slice(&h, &to).as_deref(), Some("-1"), "junk can't be set");
    h.state_mut().canvas.slice_edit = None;
    h.run_steps(2);
    let _ = h.state_mut().try_run("edit.undo", &json!({}));
    h.run_steps(3);
    assert_eq!(slice(&h, &to), None);
    let _ = h.state_mut().try_run("wire.slice", &json!({"to": to, "slice": ":-1"}));
    h.run_steps(3);
    // (the label sits on the middle of the wire, so its menu is opened directly)
    let from = h.state().session.graph().wire_into(&to).unwrap().from.clone();
    h.state_mut().canvas.wire_menu = Some(plumcraft_ui::canvas::WireMenu { screen_pos: mid(&h), from, to: to.clone(), fresh: true });
    h.run_steps(3);
    h.get_by_label("Clear Slice").click();
    h.run_steps(3);
    assert_eq!(slice(&h, &to), None);
    assert_eq!(h.state().session.undo_label(), Some("Clear Slice"));
}
