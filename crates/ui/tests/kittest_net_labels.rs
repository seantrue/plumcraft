//! Net labels on the canvas (crate::net_view): Draw as Label… from the wire menu, the tags the
//! canvas draws instead of the wire, selecting a net, dragging a tag, dropping one on a pin,
//! Watch from a tag, Draw as Wire, and a labelled wire into a collapsed group.

mod common;

use common::{add, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::{NetKey, PortRef};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use plumcraft_ui::net_view::Tag;
use serde_json::json;

/// 60 frames a second, so two clicks can make a double click.
fn build(s: Session) -> Harness<'static, EditorApp> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, app: &mut EditorApp| app.show(ui), EditorApp::new(s));
    h.run_steps(6);
    h
}

/// A literal -> Upper case, far apart: one wire.
fn harness() -> (Harness<'static, EditorApp>, PortRef) {
    let mut s = session();
    let a = add(&mut s, "text.literal", 0.0, 0.0);
    let u = add(&mut s, "text.upper", 700.0, 200.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": u, "port": "text"}})).unwrap();
    (build(s), PortRef::new(u.as_str(), "text"))
}

fn press(h: &mut Harness<'_, EditorApp>, at: egui::Pos2, button: egui::PointerButton) {
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(3);
}

fn label(h: &Harness<'_, EditorApp>, to: &PortRef) -> (Option<String>, Option<[i32; 2]>) {
    let w = h.state().session.graph().wire_into(to).unwrap();
    (w.label.clone(), w.label_at)
}

#[test]
fn a_wire_is_drawn_as_labels_selected_moved_and_drawn_as_a_wire_again() {
    let (mut h, to) = harness();
    let style = h.ctx.global_style();
    let catalog = h.state().session.catalog().clone();
    let mid = h.state().canvas.wire_midpoints(&style, &catalog)[0].0;
    press(&mut h, mid, egui::PointerButton::Secondary);
    h.get_by_label("Draw as Label…").click();
    h.run_steps(3);
    // the editor suggests the output's port name
    assert_eq!(h.state().canvas.nets.edit.as_ref().map(|e| e.text.as_str()), Some("text"));
    h.get_by_label("OK").click();
    h.run_steps(4);
    assert_eq!(label(&h, &to), (Some("text".into()), None));
    assert_eq!(h.state().session.undo_label(), Some("Draw as Label"));
    // the canvas draws tags now, not snarl's wire
    assert_eq!(h.state().canvas.snarl.wires().count(), 0);
    let use_tag = Tag::Use(to.clone());
    let tag = h.state().canvas.nets.tag_rect(&use_tag).expect("the use's tag is drawn");
    assert!(h.state().canvas.nets.tag_rect(&Tag::Driver(NetKey::top("text"))).is_some(), "and the driver's");
    // a click selects the net
    press(&mut h, tag.center(), egui::PointerButton::Primary);
    assert_eq!(h.state().canvas.nets.selected, Some(NetKey::top("text")));
    // drag the tag 60 up: an offset from the pin, saved
    let start = h.state().canvas.nets.tag_rect(&use_tag).unwrap().center();
    h.hover_at(start);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.run_steps(1);
    for k in 1..=4 {
        h.event(egui::Event::PointerMoved(start - egui::vec2(0.0, 15.0 * k as f32)));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton {
        pos: start - egui::vec2(0.0, 60.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(3);
    let scale = h.state().canvas.to_global.scaling;
    let (_, at) = label(&h, &to);
    let at = at.expect("moved");
    assert!((at[1] + (60.0 / scale) as i32).abs() <= 2 && (at[0] + 14).abs() <= 2, "{at:?}");
    // Esc clears the selection
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert_eq!(h.state().canvas.nets.selected, None);
    // the tag's menu: Draw as Wire
    let tag = h.state().canvas.nets.tag_rect(&use_tag).unwrap();
    press(&mut h, tag.center(), egui::PointerButton::Secondary);
    assert!(h.state().canvas.nets.menu.is_some(), "the tag's menu opens");
    assert!(h.state().canvas.wire_menu.is_none() && h.state().canvas.palette.is_none(), "and nothing else");
    // a labelled wire can still be watched, as its wire menu could
    h.get_by_label("Watch").click();
    h.run_steps(4);
    let from = h.state().session.graph().wire_into(&to).unwrap().from.clone();
    assert_eq!(h.state().session.run.watch.first().map(|w| w.node.as_str()), Some(from.node.0.as_str()));
    let tag = h.state().canvas.nets.tag_rect(&use_tag).unwrap();
    press(&mut h, tag.center(), egui::PointerButton::Secondary);
    h.get_by_label("Draw as Wire").click();
    h.run_steps(4);
    assert_eq!(label(&h, &to), (None, None));
    assert!(!h.state().session.graph().has_nets());
    assert_eq!(h.state().canvas.snarl.wires().count(), 1, "snarl draws it again");
}

/// Press at `from`, move to `to` in steps, let go there.
fn drag(h: &mut Harness<'_, EditorApp>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.run_steps(1);
    for k in 1..=6 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0)));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(4);
}

/// A literal -> Upper case drawn through net `picture`, an Upper case and a Repeat (whose
/// `times` text can't feed) to drop on.
fn drop_harness() -> (Harness<'static, EditorApp>, PortRef, PortRef) {
    let mut s = session();
    let a = add(&mut s, "text.literal", 0.0, 0.0);
    let u = add(&mut s, "text.upper", 500.0, 0.0);
    let v = add(&mut s, "text.upper", 500.0, 260.0);
    let r = add(&mut s, "text.repeat", 900.0, 260.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": u, "port": "text"}})).unwrap();
    s.execute("wire.label", &json!({"to": {"node": u, "port": "text"}, "name": "picture"})).unwrap();
    (build(s), PortRef::new(v.as_str(), "text"), PortRef::new(r.as_str(), "times"))
}

#[test]
fn dropping_a_tag_on_a_pin_wires_it_from_the_net() {
    let (mut h, grad, pencil) = drop_harness();
    let net = NetKey::top("picture");
    let driver = h.state().canvas.nets.tag_rect(&Tag::Driver(net.clone())).expect("the driver's tag").center();
    let pin = h.state().canvas.input_pin_on_screen(&grad).expect("the pin is drawn");
    drag(&mut h, driver, pin);
    let w = h.state().session.graph().wire_into(&grad).cloned().expect("wired");
    assert_eq!(w.label.as_deref(), Some("picture"), "drawn through the net");
    assert_eq!(h.state().session.undo_label(), Some("Connect to Net"));
    assert_eq!(h.state().session.graph().nets["picture"].at, None, "the tag stays where it was");
    assert_eq!(h.state().session.graph().net_wires(&net).count(), 2);
    // a use's tag works too, and a pin the net can't feed is refused, with the error shown
    let use_tag = h.state().canvas.nets.tag_rect(&Tag::Use(grad.clone())).expect("the new use's tag").center();
    let pin = h.state().canvas.input_pin_on_screen(&pencil).expect("the pin is drawn");
    let before = h.state().session.graph().wires.clone();
    drag(&mut h, use_tag, pin);
    assert_eq!(h.state().session.graph().wires, before, "nothing changed");
    assert!(h.state().last_error.as_deref().is_some_and(|e| e.contains("times")), "{:?}", h.state().last_error);
    assert!(h.state().session.graph().wire_into(&grad).unwrap().label_at.is_none(), "the tag didn't move either");
}

#[test]
fn a_labelled_wire_into_a_collapsed_group_is_drawn_as_a_wire() {
    let mut s = session();
    let a = add(&mut s, "text.literal", 0.0, 0.0);
    let u = add(&mut s, "text.upper", 700.0, 200.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": u, "port": "text"}})).unwrap();
    s.execute("wire.label", &json!({"to": {"node": u, "port": "text"}, "name": "picture"})).unwrap();
    s.execute("group.create", &json!({"nodes": [u], "name": "inner"})).unwrap();
    s.execute("group.collapse", &json!({"group": "g1", "collapsed": true})).unwrap();
    let mut h = build(s);
    assert_eq!(h.state().canvas.snarl.wires().count(), 1, "a wire to the group's pin");
    assert!(h.state().canvas.nets.tag_rect(&Tag::Driver(NetKey::top("picture"))).is_none(), "no tag");
    // open, it is a label again
    let _ = h.state_mut().try_run("group.collapse", &json!({"group": "g1", "collapsed": false}));
    h.run_steps(4);
    assert_eq!(h.state().canvas.snarl.wires().count(), 0);
    assert!(h.state().canvas.nets.tag_rect(&Tag::Driver(NetKey::top("picture"))).is_some());
}
