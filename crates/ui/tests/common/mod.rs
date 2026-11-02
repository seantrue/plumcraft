//! Shared by the kittest files: the text engine's catalogue (plus a test node with the scalar
//! widgets it lacks), harness building and clicks on the canvas.

#![allow(dead_code)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use serde_json::{Value, json};

/// The text engine's catalogue, plus `test.knobs`: an ENUM with a choice sub-port, a BOOL and
/// an INT (the text engine's nodes have only FLOAT, STRING and path widgets).
pub fn catalog() -> SchemaReply {
    let mut c = plumcraft_text::engine().schema();
    let knobs = json!({"id": "test.knobs", "display": "Knobs", "category": ["Testing"], "family": "test",
        "inputs": [
            {"name": "mode", "type": "ENUM", "default": "plain", "choices": ["plain", "fancy"],
             "options": {"plain": [], "fancy": [{"name": "flourish", "type": "FLOAT", "default": 1.5}]}},
            {"name": "loud", "type": "BOOL", "default": true},
            {"name": "times", "type": "INT", "default": 3, "min": 0, "max": 10}],
        "outputs": [{"name": "text", "type": "TEXT"}]});
    c.nodes.push(serde_json::from_value(knobs).unwrap());
    let dials = json!({"id": "test.dials", "display": "Dials", "category": ["Testing"], "family": "test",
        "description": "Everything the parameter editor draws.\n\nThe rest is on hover.",
        "inputs": [
            {"name": "input", "type": "TEXT", "socket": "socket"},
            {"name": "preset", "type": "ENUM", "default": "fine", "choices": ["fine", "broad"], "socket": "both",
             "fills": {"fine": {"width": 1.0, "ink": "0,0,0", "spare": 0.0}, "broad": {"width": 8.0, "ink": "1,0,0", "spare": 2.0}},
             "shows": {"fine": ["width", "ink", "strokes.on", "strokes.kind", "picks"], "broad": ["width", "ink", "spare", "strokes.on", "strokes.kind", "picks"]}},
            {"name": "width", "type": "FLOAT", "default": 1.0, "min": 0.0, "max": 100.0, "soft_max": 10.0, "tooltip": "How wide", "socket": "both"},
            {"name": "ink", "type": "STRING", "default": "0,0,0", "widget": "color", "socket": "both"},
            {"name": "spare", "type": "FLOAT", "default": 0.0, "min": 0.0, "max": 5.0, "socket": "both"},
            {"name": "strokes.on", "type": "BOOL", "default": true, "param_ref": {"group": "strokes", "name": "on"}, "socket": "both"},
            {"name": "strokes.kind", "type": "ENUM", "default": "a", "choices": ["a", "b", "c", "d", "e", "f", "g"], "param_ref": {"group": "strokes", "name": "kind"}, "socket": "both"},
            {"name": "strokes.size", "type": "INT", "default": 3, "min": 1, "max": 9, "param_ref": {"group": "strokes", "name": "size"},
             "active_when": {"strokes.on": [true, false]}, "socket": "both"},
            {"name": "picks", "type": "STRING", "default": "", "widget": "multichoice", "choices": ["x", "y", "z"], "socket": "both"}],
        "outputs": [{"name": "text", "type": "TEXT"}]});
    c.nodes.push(serde_json::from_value(dials).unwrap());
    c
}

pub fn session() -> Session {
    Session::new(catalog())
}

pub fn add(s: &mut Session, kind: &str, x: f32, y: f32) -> String {
    s.execute("node.add", &json!({"kind": kind, "pos": [x, y]})).unwrap()["id"].as_str().unwrap().to_owned()
}

pub fn harness(app: EditorApp) -> Harness<'static, EditorApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
    h.run_steps(6);
    h
}

pub fn value(h: &Harness<'_, EditorApp>, node: &str, port: &str) -> Value {
    h.state().session.graph().node(&node.into()).unwrap().values.get(port).cloned().unwrap_or(Value::Null)
}

/// Accessibility rects of widgets on the canvas are in graph coordinates (egui doesn't apply the
/// canvas layer's transform to them); map one to the screen.
pub fn on_screen(h: &Harness<'_, EditorApp>, r: egui::Rect) -> egui::Rect {
    let t = h.state().canvas.to_global;
    egui::Rect::from_min_max(t * r.min, t * r.max)
}

/// Press and release the primary button at `at`, as a person would.
pub fn click_at(h: &mut Harness<'static, EditorApp>, at: egui::Pos2) {
    h.hover_at(at);
    h.run_steps(2);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(2);
}

/// Click a canvas widget by its accessibility rect.
pub fn click_canvas(h: &mut Harness<'static, EditorApp>, r: egui::Rect) {
    let at = on_screen(h, r).center();
    click_at(h, at);
}

/// Click a widget inside a node by its label.
pub fn click_in_node(h: &mut Harness<'static, EditorApp>, label: &str) {
    let r = h.get_by_label(label).rect();
    click_canvas(h, r);
}

/// Click a widget outside the canvas (menus, popups) by its label.
pub fn click(h: &mut Harness<'static, EditorApp>, label: &str) {
    let at = h.get_by_label(label).rect().center();
    click_at(h, at);
}
