//! What a node shows of its last run, and of itself: how long a cached node took to compute, the
//! engine's text, labels of values nobody changed dimmed. And a wire end dragged to another input,
//! which moves the wire in one undo step.

mod common;

use std::time::Instant;

use common::{add, harness};
use egui_kittest::Harness;
use egui_kittest::kittest::{By, NodeT, Queryable};
use plumcraft_graph::PortRef;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::event::NodeState;
use plumcraft_proto::run::{MemoInfo, NodeReport, RunReply};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use serde_json::json;

fn catalog() -> SchemaReply {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "t.make", "display": "Make", "inputs": [
                {"name": "strength", "type": "FLOAT", "default": 1.0, "tooltip": "How hard"},
                {"name": "other", "type": "FLOAT", "default": 1.0}],
             "outputs": [{"name": "text", "type": "TEXT"}, {"name": "names", "type": "STRING", "list": true}]},
            {"id": "t.take", "display": "Take", "inputs": [
                {"name": "a", "type": "TEXT", "socket": "socket", "optional": true},
                {"name": "b", "type": "TEXT", "socket": "socket", "optional": true},
                {"name": "name", "type": "STRING", "socket": "socket", "optional": true},
                {"name": "names", "type": "STRING", "list": true, "socket": "socket", "optional": true}],
             "outputs": []}]}))
    .unwrap()
}

/// A finished run with `report` for `node`.
fn ran(s: &mut Session, node: &str, report: NodeReport) {
    s.connected = true;
    let req = s.execute("run.start", &json!({})).unwrap();
    let _ = s.run.take_outbox();
    let mut reply = RunReply { run_id: req["run_id"].as_str().unwrap().to_owned(), generation: s.run.generation, ok: true, ..Default::default() };
    reply.nodes.insert(node.to_owned(), report);
    assert!(s.run.apply_reply(&reply, Instant::now()));
}

fn one() -> (Session, String) {
    let mut s = Session::new(catalog());
    let n = add(&mut s, "t.make", 40.0, 40.0);
    (s, n)
}

#[test]
fn a_cached_node_shows_what_computing_it_took() {
    let (mut s, n) = one();
    let memo = MemoInfo { computed_ms: Some(4410.0), source: Some("memory".into()), ..Default::default() };
    ran(&mut s, &n, NodeReport { state: NodeState::Cached, ms: 0.01, memo: Some(memo), ..Default::default() });
    let h = harness(EditorApp::new(s));
    assert!(h.query_by_label("cached · 4.41 s").is_some(), "the compute time, not the fetch");
}

/// An engine that reports no compute time: what the node took when it last computed.
#[test]
fn a_cached_node_remembers_its_last_compute_when_the_engine_does_not_say() {
    let (mut s, n) = one();
    ran(&mut s, &n, NodeReport { state: NodeState::Done, ms: 900.0, ..Default::default() });
    ran(&mut s, &n, NodeReport { state: NodeState::Cached, ms: 0.01, ..Default::default() });
    ran(&mut s, &n, NodeReport { state: NodeState::Cached, ms: 0.02, ..Default::default() });
    let h = harness(EditorApp::new(s));
    assert!(h.query_by_label("cached · 900 ms").is_some(), "and it lasts through more cached runs");
}

#[test]
fn a_cached_node_with_no_compute_time_says_cached() {
    let (mut s, n) = one();
    ran(&mut s, &n, NodeReport { state: NodeState::Cached, ms: 0.01, ..Default::default() });
    let h = harness(EditorApp::new(s));
    assert!(h.query_by_label("cached").is_some());
}

/// The text run holding the test's words: a wrapped label is its value, not its label.
fn words(h: &Harness<'_, EditorApp>) -> Vec<String> {
    h.query_all(By::new().predicate(|n| n.value().is_some_and(|v| v.contains("word word")))).filter_map(|n| n.accesskit_node().value()).collect()
}

#[test]
fn the_engines_text_shows_on_the_node_and_goes_with_the_next_run() {
    let (mut s, n) = one();
    let long = format!("{} tail", "word ".repeat(40));
    ran(&mut s, &n, NodeReport { state: NodeState::Done, text: Some(long.clone()), ..Default::default() });
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    let shown = words(&h);
    assert!(!shown.is_empty(), "on the node");
    assert!(shown.iter().all(|t| t.chars().count() <= 121) && shown.iter().any(|t| t.ends_with('…')), "cut short, the rest on hover: {shown:?}");

    ran(&mut h.state_mut().session, &n, NodeReport { state: NodeState::Done, ..Default::default() });
    h.run_steps(4);
    assert!(words(&h).is_empty(), "the next run said nothing");
}

/// How far a label's text stands from what is behind it: the spread of its rect's green channel.
fn contrast(image: &image::RgbaImage, r: egui::Rect) -> u8 {
    let px = |v: f32| v.max(0.0) as u32;
    let (x0, y0, x1, y1) = (px(r.min.x), px(r.min.y), px(r.max.x).min(image.width()), px(r.max.y).min(image.height()));
    let green: Vec<u8> = (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).map(|(x, y)| image.get_pixel(x, y)[1]).collect();
    green.iter().max().unwrap_or(&0) - green.iter().min().unwrap_or(&0)
}

#[test]
fn a_value_nobody_changed_has_a_dimmer_label() {
    let (mut s, n) = one();
    s.execute("node.set", &json!({"node": n, "port": "other", "value": 2.0})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    let rect = |h: &Harness<'static, EditorApp>, label: &str| common::on_screen(h, h.get_by_label(label).rect());
    let (default, changed) = (rect(&h, "strength"), rect(&h, "other"));
    let image = h.render().expect("a picture");
    if let Some(dir) = std::env::var_os("PLUMCRAFT_SHOT_DIR") {
        image.save(std::path::Path::new(&dir).join("dim-labels.png")).unwrap();
    }
    let (dim, full) = (contrast(&image, default), contrast(&image, changed));
    assert!(dim + 30 < full, "the default's label stands out {dim}, the changed one's {full}");

    h.state_mut().run("node.set", &json!({"node": n, "port": "strength", "value": 3.0})).unwrap();
    h.run_steps(4);
    let image = h.render().expect("a picture");
    assert_eq!(contrast(&image, rect(&h, "strength")), full, "changed, it comes up");
}

/// Press at `from` with ⌘ held, move, release at `to`.
fn command_drag(h: &mut Harness<'static, EditorApp>, from: egui::Pos2, to: egui::Pos2) {
    let cmd = egui::Modifiers::COMMAND;
    h.hover_at(from);
    h.run_steps(1);
    // egui reads held modifiers from ModifiersChanged, not from the pointer event
    h.event(egui::Event::ModifiersChanged(cmd));
    h.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: cmd });
    h.run_steps(1);
    for k in 1..=6 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0)));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: cmd });
    h.run_steps(1);
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run_steps(4);
}

#[test]
fn dragging_a_wire_end_to_another_input_moves_it_in_one_undo_step() {
    let mut s = Session::new(catalog());
    let make = add(&mut s, "t.make", 40.0, 40.0);
    let take = add(&mut s, "t.take", 420.0, 40.0);
    let (a, b) = (PortRef::new(take.as_str(), "a"), PortRef::new(take.as_str(), "b"));
    s.execute("wire.connect", &json!({"from": {"node": make, "port": "text"}, "to": a})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    let (from, to) = (h.state().canvas.input_pin_on_screen(&a).unwrap(), h.state().canvas.input_pin_on_screen(&b).unwrap());
    let depth = h.state().session.undo_depth();
    command_drag(&mut h, from, to);
    let g = h.state().session.graph();
    assert!(g.wire_into(&a).is_none(), "the old end went");
    assert_eq!(g.wire_into(&b).map(|w| w.from.node.0.as_str()), Some(make.as_str()), "the new end came");
    assert_eq!(h.state().session.undo_depth(), depth + 1, "one step, not a disconnect and a connect");
    assert_eq!(h.state().session.undo_label(), Some("Reroute Wire"));

    h.state_mut().run("edit.undo", &json!(null)).unwrap();
    h.run_steps(4);
    assert!(h.state().session.graph().wire_into(&a).is_some(), "one undo puts it back");
}

#[test]
fn a_rerouted_wire_keeps_its_slice_where_it_fits_and_a_refusal_keeps_the_wire() {
    let mut s = Session::new(catalog());
    let make = add(&mut s, "t.make", 40.0, 40.0);
    let take = add(&mut s, "t.take", 420.0, 40.0);
    let at = |p: &str| json!({"node": take, "port": p});
    s.execute("wire.connect", &json!({"from": {"node": make, "port": "names"}, "to": at("names")})).unwrap();
    s.execute("wire.slice", &json!({"to": at("names"), "slice": ":-1"})).unwrap();
    s.execute("wire.connect", &json!({"from": {"node": make, "port": "names"}, "to": at("name")})).unwrap();
    s.execute("wire.disconnect", &json!({"to": at("name")})).unwrap();
    let r = s.execute("wire.reroute", &json!({"to": at("names"), "newTo": at("name")})).unwrap();
    assert_eq!(r["sliceDropped"], serde_json::Value::Null);
    let wire = s.graph().wire_into(&PortRef::new(take.as_str(), "name")).unwrap();
    assert_eq!(wire.slice.as_deref(), Some(":-1"), "the slice came along");

    let before = s.graph().clone();
    assert!(s.execute("wire.reroute", &json!({"to": at("name"), "newTo": at("a")})).is_err(), "a list of STRING into TEXT");
    assert_eq!(s.graph(), &before, "and the wire stays where it was");
    assert!(s.execute("wire.reroute", &json!({"to": at("b"), "newTo": at("a")})).is_err(), "not wired");
}
