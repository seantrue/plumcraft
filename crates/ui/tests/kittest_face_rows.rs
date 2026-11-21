//! A row hidden from a node's face (`node.face {shown: false}`, what Show on Face saves) leaves
//! every other row where the core's scene has it (B45): each label beside its own field, each
//! pin its own port's, and an edit on the face changes the port beside it and no other. Checked
//! by a person's drag and typing on two node kinds, with wires on rows after the hidden one, and
//! on a graph that already carries hidden rows when it is opened.

mod common;

use common::{add, harness_pictured, on_screen, shot, value};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::Graph;
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use serde_json::{Value, json};

const SIZE: egui::Vec2 = egui::vec2(1200.0, 760.0);

/// The kittests' catalogue plus `test.word`, a STRING out, to wire into scalar rows.
fn catalog() -> plumcraft_proto::catalogue::SchemaReply {
    let mut c = common::catalog();
    let word = json!({"id": "test.word", "display": "Word", "category": ["Testing"], "family": "test",
        "inputs": [{"name": "word", "type": "STRING", "default": "plum"}],
        "outputs": [{"name": "word", "type": "STRING"}]});
    c.nodes.push(serde_json::from_value(word).unwrap());
    c
}

fn session() -> Session {
    Session::new(catalog())
}

fn harness(s: Session) -> Harness<'static, EditorApp> {
    let mut h = harness_pictured(EditorApp::new(s), SIZE);
    h.run_steps(4);
    h
}

fn hide(h: &mut Harness<'static, EditorApp>, node: &str, port: &str) {
    h.state_mut().session.execute("node.face", &json!({"node": node, "port": port, "shown": false})).unwrap();
    h.run_steps(6);
}

/// Every face egui draws has the scene's rows, in the scene's order: pin `i` is row `i`.
fn rows_agree_with_the_scene(h: &Harness<'static, EditorApp>) {
    let scene = h.state().scene_now();
    for (_, n) in h.state().canvas.snarl.node_ids() {
        let face = scene.nodes.iter().find(|f| f.id == n.id.0).expect("the node has a face");
        let drawn: Vec<&str> = n.inputs.iter().map(|r| r.key.as_str()).collect();
        let shown: Vec<&str> = face.inputs.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(drawn, shown, "node {}'s rows", n.id.0);
    }
}

/// The wires snarl draws go into the rows the graph's wires name.
fn wires_land_on_their_ports(h: &Harness<'static, EditorApp>) {
    let snarl = &h.state().canvas.snarl;
    let mut landed = Vec::new();
    for (out, inp) in snarl.wires() {
        let from = snarl.get_node(out.node).unwrap();
        let to = snarl.get_node(inp.node).unwrap();
        landed.push(format!("{}→{}.{}", from.id.0, to.id.0, to.inputs[inp.input].key));
    }
    let mut wanted: Vec<String> = h.state().session.graph().wires.iter().map(|w| format!("{}→{}.{}", w.from.node, w.to.node, w.to.port)).collect();
    landed.sort();
    wanted.sort();
    assert_eq!(landed, wanted);
}

/// The centre of the widget with `role` level with the label `label` (on the screen).
fn field_beside(h: &Harness<'static, EditorApp>, label: &str, role: Role) -> egui::Pos2 {
    let y = on_screen(h, h.get_by_label(label).rect()).center().y;
    let fields: Vec<egui::Rect> = h.get_all_by_role(role).map(|n| on_screen(h, n.rect())).collect();
    let best = fields.iter().min_by(|a, b| (a.center().y - y).abs().total_cmp(&(b.center().y - y).abs())).expect("a field");
    assert!((best.center().y - y).abs() < 6.0, "a field level with {label}: {best:?} at y {y}");
    best.center()
}

/// The widget level with `label` is that port's own: the text field beside it holds its value.
fn text_beside(h: &Harness<'static, EditorApp>, label: &str) -> String {
    let at = field_beside(h, label, Role::TextInput);
    h.get_all_by_role(Role::TextInput).find(|n| on_screen(h, n.rect()).contains(at)).and_then(|n| n.value()).unwrap_or_default()
}

fn drag_right(h: &mut Harness<'static, EditorApp>, start: egui::Pos2) {
    h.drag_at(start);
    h.run_steps(1);
    for dx in [5.0, 10.0, 20.0, 30.0] {
        h.hover_at(start + egui::vec2(dx, 0.0));
        h.run_steps(1);
    }
    h.drop_at(start + egui::vec2(30.0, 0.0));
    h.run_steps(4);
}

#[test]
fn a_drag_beside_amount_changes_amount_with_count_hidden() {
    let mut s = session();
    let w = add(&mut s, "text.widgets", 300.0, 40.0);
    let lit = add(&mut s, "text.literal", 0.0, 40.0);
    let word = add(&mut s, "test.word", 0.0, 260.0);
    // wires into rows before and after the hidden one
    s.execute("wire.connect", &json!({"from": {"node": lit, "port": "text"}, "to": {"node": w, "port": "text"}})).unwrap();
    s.execute("wire.connect", &json!({"from": {"node": word, "port": "word"}, "to": {"node": w, "port": "note"}})).unwrap();
    let mut h = harness(s);
    shot(&mut h, "b45-every-widget-all-rows");
    hide(&mut h, &w, "count");
    shot(&mut h, "b45-every-widget-count-hidden");
    rows_agree_with_the_scene(&h);
    wires_land_on_their_ports(&h);
    assert!(h.query_by_label("count").is_none(), "count is off the face");
    let count = value(&h, &w, "count");
    let start = field_beside(&h, "amount", Role::SpinButton);
    drag_right(&mut h, start);
    shot(&mut h, "b45-every-widget-after-drag-beside-amount");
    assert_eq!(value(&h, &w, "count"), count, "count is untouched");
    let amount = value(&h, &w, "amount").as_f64().expect("amount was set");
    assert!(amount > 0.5, "amount went up: {amount}");
    // the check box level with "flag" is flag's: a click turns flag off, nothing else
    let at = field_beside(&h, "flag", Role::CheckBox);
    common::click_at(&mut h, at);
    assert_eq!(value(&h, &w, "flag"), json!(false));
    assert_eq!(value(&h, &w, "count"), count);
    rows_agree_with_the_scene(&h);
}

#[test]
fn typing_beside_with_changes_with_when_find_is_hidden() {
    let mut s = session();
    let r = add(&mut s, "text.replace", 300.0, 40.0);
    let lit = add(&mut s, "text.literal", 0.0, 40.0);
    s.execute("node.set", &json!({"node": r, "port": "find", "value": "corgi"})).unwrap();
    s.execute("wire.connect", &json!({"from": {"node": lit, "port": "text"}, "to": {"node": r, "port": "text"}})).unwrap();
    let mut h = harness(s);
    shot(&mut h, "b45-replace-all-rows");
    hide(&mut h, &r, "find");
    shot(&mut h, "b45-replace-find-hidden");
    rows_agree_with_the_scene(&h);
    wires_land_on_their_ports(&h);
    assert_eq!(text_beside(&h, "with"), "", "the field beside 'with' is with's (empty), not find's ('corgi')");
    let at = field_beside(&h, "with", Role::TextInput);
    common::click_at(&mut h, at);
    h.event(egui::Event::Text("dog".into()));
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    shot(&mut h, "b45-replace-after-typing-beside-with");
    assert_eq!(value(&h, &r, "with"), json!("dog"));
    assert_eq!(value(&h, &r, "find"), json!("corgi"), "find is untouched");
}

#[test]
fn a_wire_into_a_row_after_the_hidden_one_lands_on_its_port() {
    let mut s = session();
    let r = add(&mut s, "text.replace", 300.0, 40.0);
    let lit = add(&mut s, "test.word", 0.0, 40.0);
    let mut h = harness(s);
    hide(&mut h, &r, "find");
    // wired after it was hidden: into `with`, the row after `find`
    h.state_mut().session.execute("wire.connect", &json!({"from": {"node": lit, "port": "word"}, "to": {"node": r, "port": "with"}})).unwrap();
    h.run_steps(6);
    rows_agree_with_the_scene(&h);
    wires_land_on_their_ports(&h);
    // a wire into the hidden row brings it back (a wired input always shows its pin)
    h.state_mut().session.execute("wire.connect", &json!({"from": {"node": lit, "port": "word"}, "to": {"node": r, "port": "find"}})).unwrap();
    h.run_steps(6);
    rows_agree_with_the_scene(&h);
    wires_land_on_their_ports(&h);
    let keys: Vec<String> = h.state().canvas.snarl.node_ids().find(|(_, n)| n.id.0 == r).unwrap().1.inputs.iter().map(|x| x.key.clone()).collect();
    assert_eq!(keys, ["text", "find", "with"]);
}

#[test]
fn a_graph_opened_with_hidden_rows_draws_and_edits_them_right() {
    // a graph as the Apple apps save it after Show on Face is turned off for two rows
    let graph = Graph::from_json(
        &json!({
            "version": plumcraft_graph::WORKFLOW_VERSION,
            "nodes": {
                "n1": {"kind": "text.widgets", "pos": [300.0, 40.0], "face": {"count": false, "label": false}},
                "n2": {"kind": "test.word", "pos": [0.0, 40.0]}
            },
            "wires": [{"from": {"node": "n2", "port": "word"}, "to": {"node": "n1", "port": "tint"}}]
        })
        .to_string(),
    )
    .unwrap();
    let s = Session::with_graph(catalog(), graph);
    let mut h = harness(s);
    shot(&mut h, "b45-opened-with-hidden-rows");
    rows_agree_with_the_scene(&h);
    wires_land_on_their_ports(&h);
    let start = field_beside(&h, "amount", Role::SpinButton);
    drag_right(&mut h, start);
    assert!(value(&h, "n1", "amount").as_f64().is_some_and(|a| a > 0.5));
    assert_eq!(value(&h, "n1", "count"), Value::Null);
    assert_eq!(value(&h, "n1", "label"), Value::Null);
}

#[test]
fn show_on_face_in_the_parameters_window_hides_and_shows_a_row_through_the_core() {
    let mut s = session();
    let w = add(&mut s, "text.widgets", 300.0, 40.0);
    let word = add(&mut s, "test.word", 0.0, 260.0);
    s.execute("wire.connect", &json!({"from": {"node": word, "port": "word"}, "to": {"node": w, "port": "note"}})).unwrap();
    s.execute("node.parameters", &json!({"node": w})).unwrap();
    let mut h = harness(s);
    shot(&mut h, "show-on-face-before");
    let face = |h: &Harness<'static, EditorApp>| h.state().session.graph().node(&w.as_str().into()).unwrap().face.clone();
    h.get_by_label("Show count on Face").click();
    h.run_steps(6);
    shot(&mut h, "show-on-face-count-hidden");
    assert_eq!(face(&h).get("count"), Some(&false), "node.face {{count, shown: false}}");
    rows_agree_with_the_scene(&h);
    assert!(h.state().canvas.snarl.node_ids().any(|(_, n)| n.id.0 == w && n.inputs.iter().all(|r| r.key != "count")), "off the face");
    // a wired input can't be hidden
    h.get_by_label("Show note on Face").click();
    h.run_steps(4);
    assert_eq!(face(&h).get("note"), None, "note is wired: its box is disabled");
    h.get_by_label("Show count on Face").click();
    h.run_steps(6);
    assert_eq!(face(&h).get("count"), Some(&true));
    rows_agree_with_the_scene(&h);
}
