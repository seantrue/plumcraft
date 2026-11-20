//! N3 slice 1 on the egui editor, through real pointer and key input: a new wire dropped on
//! empty canvas opens the core's palette for what takes it (question 8; egui-snarl's
//! `has_dropped_wire_menu` reports the drop), a pick adds the node with the wire in one undo
//! step and Escape leaves nothing; the arrow keys nudge the selection, a held key one undo step.

mod common;

use common::{add, harness, session};
use egui_kittest::Harness;
use plumcraft_graph::{NodeId, PortRef};
use plumcraft_ui::EditorApp;
use serde_json::json;

fn pos(h: &Harness<'static, EditorApp>, id: &str) -> [f32; 2] {
    h.state().session.graph().node(&NodeId::from(id)).unwrap().pos
}

/// Drag a new wire from `a`'s output to empty canvas well below it.
fn drop_a_wire(h: &mut Harness<'static, EditorApp>, a: &str) {
    let catalog = h.state().session.catalog().clone();
    let from = h.state().canvas.pin_screen(&catalog, &PortRef::new(a, "text"), true).expect("the output's pin");
    let to = from + egui::vec2(120.0, 260.0);
    h.hover_at(from);
    h.run_steps(2);
    h.drag_at(from);
    h.run_steps(2);
    for i in 1..=4 {
        h.hover_at(from + (to - from) * (i as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(to);
    h.run_steps(4);
}

#[test]
fn a_wire_dropped_on_empty_canvas_opens_the_palette_for_it() {
    let mut s = session();
    let a = add(&mut s, "text.literal", 40.0, 40.0);
    let mut h = harness(EditorApp::new(s));
    let before = h.state().session.graph().clone();
    drop_a_wire(&mut h, &a);
    // the core keeps the palette with the wire; egui shows it, filtered to what takes the wire
    let wire = h.state().doc.palette.as_ref().and_then(|p| p.wire.clone()).expect("the core's palette has the dropped wire");
    assert_eq!(wire.from.as_ref().map(|f| (f.node.as_str(), f.port.as_str())), Some((a.as_str(), "text")));
    let fits = h.state().canvas.palette.as_ref().and_then(|p| p.fits.clone()).expect("egui's palette is open, filtered");
    let catalog = h.state().session.catalog().clone();
    let literal = catalog.nodes.iter().position(|d| d.id == "text.literal").unwrap();
    let upper = catalog.nodes.iter().position(|d| d.id == "text.upper").unwrap();
    assert!(fits.contains(&upper) && !fits.contains(&literal), "{fits:?}");
    assert_eq!(h.state().session.graph(), &before, "nothing is added until a kind is picked");
    // Escape leaves nothing behind
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().canvas.palette.is_none() && h.state().doc.palette.is_none());
    assert_eq!(h.state().session.graph(), &before);
    // again, and a pick (Enter takes the first offered) adds the node and the wire, one step
    drop_a_wire(&mut h, &a);
    assert!(h.state().canvas.palette.is_some());
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let g = h.state().session.graph().clone();
    assert_eq!((g.nodes.len(), g.wires.len()), (2, 1), "the node and its wire");
    assert_eq!(g.wires[0].from, PortRef::new(a.as_str(), "text"));
    assert_eq!(h.state().session.undo_label(), Some("Add and Connect"));
    assert!(h.state().canvas.palette.is_none() && h.state().doc.palette.is_none());
    h.state_mut().run("edit.undo", &json!({}));
    h.run_steps(3);
    assert_eq!(h.state().session.graph(), &before, "one undo takes both away");
}

#[test]
fn the_arrow_keys_nudge_the_selection_and_a_held_key_is_one_step() {
    let mut s = session();
    let a = add(&mut s, "text.literal", 40.0, 40.0);
    let b = add(&mut s, "text.upper", 400.0, 40.0);
    let mut h = harness(EditorApp::new(s));
    let undo = h.state().session.undo_label().map(str::to_owned);
    // nothing selected: the arrows are left alone
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(pos(&h, &a), [40.0, 40.0]);
    h.state_mut().run("selection.set", &json!({"nodes": [a, b]}));
    h.run_steps(2);
    // one press and its repeats (a held key): one step
    let key = |pressed, repeat, modifiers| egui::Event::Key { key: egui::Key::ArrowRight, physical_key: None, pressed, repeat, modifiers };
    h.event(key(true, false, egui::Modifiers::NONE));
    h.run_steps(1);
    for _ in 0..4 {
        h.event(key(true, true, egui::Modifiers::NONE));
        h.run_steps(1);
    }
    h.event(key(false, false, egui::Modifiers::NONE));
    h.run_steps(2);
    assert_eq!((pos(&h, &a), pos(&h, &b)), ([45.0, 40.0], [405.0, 40.0]));
    assert_eq!(h.state().session.undo_label(), Some("Move 2 Nodes"));
    // a new press with Shift: 10 units, a step of its own
    h.event(egui::Event::Key { key: egui::Key::ArrowDown, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::SHIFT });
    h.run_steps(2);
    assert_eq!(pos(&h, &a), [45.0, 50.0]);
    h.state_mut().run("edit.undo", &json!({}));
    h.run_steps(2);
    assert_eq!(pos(&h, &a), [45.0, 40.0]);
    h.state_mut().run("edit.undo", &json!({}));
    h.run_steps(2);
    assert_eq!((pos(&h, &a), pos(&h, &b)), ([40.0, 40.0], [400.0, 40.0]), "one undo takes the held key's nudges back");
    assert_eq!(h.state().session.undo_label().map(str::to_owned), undo);
}
