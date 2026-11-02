//! The seam for an application's own value editors, through the real UI, with a test editor: a
//! port of a type core doesn't know opens it from the parameter editor, from the node's footer and
//! from its menu; its edits are commands, one undo step each; it follows the graph and what a run
//! sent; and with nothing registered for the kind nothing is offered.

mod common;

use std::sync::Once;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use plumcraft_ui::value_editors::{self, Context, EditorKind, ValueEditor};
use serde_json::{Value, json};

use common::{add, click, click_at, click_in_node, harness, value};

/// The test editor: says its value and what the run sent, and bumps the value by one.
#[derive(Default)]
struct Dial {
    seen: i64,
    forgot: u32,
}

fn number(v: &Value) -> i64 {
    v.as_str().and_then(|s| s.parse().ok()).unwrap_or(0)
}

impl ValueEditor for Dial {
    fn show(&mut self, ui: &mut egui::Ui, cx: &Context<'_>) -> Vec<(&'static str, Value)> {
        self.seen = number(cx.value);
        ui.label(format!("dial reads {}", self.seen));
        ui.label(format!("the run said {}", cx.data.map_or("nothing".to_owned(), Value::to_string)));
        ui.label(format!("workflow {}", cx.path.is_some()));
        if ui.button("Bump").clicked() {
            return vec![("node.set", json!({"node": cx.node, "port": cx.port, "value": (self.seen + 1).to_string()}))];
        }
        Vec::new()
    }

    fn inspect(&self) -> Value {
        json!({"seen": self.seen, "forgot": self.forgot})
    }

    fn forget_textures(&mut self) {
        self.forgot += 1;
    }
}

fn setup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let make = || Box::new(Dial::default()) as Box<dyn ValueEditor>;
        let kind = |kind, title, label| EditorKind { kind, title, label, help: "Turn it", summary: |v| format!("dial {}", number(v)), make };
        value_editors::register(vec![kind("test.dial", "Dial", "Edit Dial…"), kind("test.notes", "Notes", "Edit Notes…")]).unwrap();
    });
}

fn catalog() -> SchemaReply {
    let mut c = common::catalog();
    let nodes = json!([
        {"id": "test.dialled", "display": "Dialled", "category": ["Testing"], "family": "test",
         "inputs": [{"name": "dial", "type": "x.test.DIAL", "default": "3", "socket": "both"}],
         "outputs": [{"name": "out", "type": "x.test.DIAL"}],
         "editor": {"kind": "test.dial", "port": "dial"}},
        {"id": "test.noted", "display": "Noted", "category": ["Testing"], "family": "test",
         "inputs": [{"name": "note", "type": "STRING", "default": "12", "socket": "both"}],
         "outputs": [],
         "editor": {"kind": "test.notes", "port": "note"}},
        {"id": "test.orphan", "display": "Orphan", "category": ["Testing"], "family": "test",
         "inputs": [{"name": "dial", "type": "x.test.DIAL", "default": "5", "socket": "both"}],
         "outputs": [],
         "editor": {"kind": "nobody.draws.this", "port": "dial"}}
    ]);
    for n in nodes.as_array().unwrap() {
        c.nodes.push(serde_json::from_value(n.clone()).unwrap());
    }
    c
}

fn app(kind: &str) -> (Harness<'static, EditorApp>, String) {
    setup();
    let mut s = Session::new(catalog());
    let id = add(&mut s, kind, 40.0, 40.0);
    (harness(EditorApp::new(s)), id)
}

fn has(h: &Harness<'_, EditorApp>, label: &str) -> bool {
    h.query_all_by_label_contains(label).next().is_some()
}

#[test]
fn a_port_of_an_unknown_type_opens_its_editor_from_the_parameter_editor() {
    let (mut h, id) = app("test.dialled");
    h.state_mut().run("node.parameters", &json!({"node": id})).unwrap();
    h.run_steps(4);
    assert!(has(&h, "dial 3"), "what the application says of the value, since core can't");
    assert!(!h.state().session.views.editors.contains(&id.as_str().into()));

    // the parameter editor's is the button in the window, after the node's
    let buttons: Vec<egui::Rect> = h.get_all_by_label("Edit Dial…").map(|n| n.rect()).collect();
    assert_eq!(buttons.len(), 2, "the footer's and the parameter editor's");
    let at = buttons.iter().max_by(|a, b| a.left().total_cmp(&b.left())).unwrap().center();
    click_at(&mut h, at);
    h.run_steps(4);
    assert!(has(&h, "dial reads 3"), "the editor's window, on the port's value");
    assert!(has(&h, "Dial — "), "named for the kind and the node");
    assert_eq!(h.state().value_editors.inspect(), json!([{"node": id, "kind": "test.dial", "state": {"seen": 3, "forgot": 0}}]));
}

#[test]
fn an_edit_is_one_undo_step_and_the_editor_follows_an_undo() {
    let (mut h, id) = app("test.dialled");
    click_in_node(&mut h, "Edit Dial…");
    h.run_steps(4);
    let depth = h.state().session.undo_depth();
    click(&mut h, "Bump");
    h.run_steps(3);
    assert_eq!(value(&h, &id, "dial"), json!("4"));
    assert_eq!(h.state().session.undo_depth(), depth + 1, "one gesture, one undo step");
    assert!(h.state().session.journal().iter().any(|e| e.id == "node.set" && e.ok), "and a journal entry");
    assert!(has(&h, "dial reads 4"));
    h.state_mut().run("edit.undo", &json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(value(&h, &id, "dial"), json!("3"));
    assert!(has(&h, "dial reads 3"), "an undo reaches the open window");
}

#[test]
fn the_nodes_menu_offers_it_too() {
    let (mut h, id) = app("test.dialled");
    let actions = h.get_by_label("Node actions").rect();
    common::click_canvas(&mut h, actions);
    h.run_steps(3);
    let buttons: Vec<egui::Rect> = h.get_all_by_label("Edit Dial…").map(|n| n.rect()).collect();
    assert_eq!(buttons.len(), 2, "the footer's, and now the menu's");
    // the menu's rect is the screen's, and the footer's the canvas layer's: the second drawn
    let menu = buttons[1];
    click_at(&mut h, menu.center());
    h.run_steps(4);
    assert!(has(&h, "dial reads 3"));
    assert!(h.state().session.views.editors.contains(&id.as_str().into()));
}

#[test]
fn a_port_of_a_known_type_keeps_its_widget_beside_the_button() {
    let (mut h, id) = app("test.noted");
    h.state_mut().run("node.parameters", &json!({"node": id})).unwrap();
    h.run_steps(4);
    assert_eq!(h.state().params.rects.iter().filter(|(k, _)| k == "note").count(), 1, "still an editable row");
    assert_eq!(h.get_all_by_label("Edit Notes…").count(), 2, "the footer's and the row's");
    assert!(!has(&h, "dial 12"), "the widget shows the value; no summary in its place");
}

#[test]
fn a_kind_nobody_registered_offers_nothing_and_the_port_stays_read_only() {
    let (mut h, id) = app("test.orphan");
    h.state_mut().run("node.parameters", &json!({"node": id})).unwrap();
    h.run_steps(4);
    assert!(!has(&h, "Edit "), "no button anywhere");
    assert!(h.state().params.rects.is_empty(), "no editable row");
    // a client can name it all the same: it is told, and nothing stays open
    h.state_mut().run("node.editor", &json!({"node": id})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().last_error.as_deref(), Some("this application has no nobody.draws.this editor"));
    assert!(h.state().session.views.editors.is_empty());
    assert!(h.state().value_editors.is_empty());
}

#[test]
fn a_port_on_a_wire_offers_nothing_and_an_open_editor_says_why() {
    setup();
    let mut s = Session::new(catalog());
    let (up, down) = (add(&mut s, "test.dialled", 40.0, 40.0), add(&mut s, "test.dialled", 420.0, 40.0));
    let mut h = harness(EditorApp::new(s));
    h.state_mut().run("node.editor", &json!({"node": down})).unwrap();
    h.run_steps(3);
    assert!(has(&h, "dial reads 3"));
    assert_eq!(h.get_all_by_label("Edit Dial…").count(), 2);
    h.state_mut().run("wire.connect", &json!({"from": {"node": up, "port": "out"}, "to": {"node": down, "port": "dial"}})).unwrap();
    h.run_steps(4);
    assert_eq!(h.get_all_by_label("Edit Dial…").count(), 1, "only the unwired node's footer");
    assert!(has(&h, "dial comes from a wire"), "the open window says so");
    assert!(!has(&h, "dial reads"), "and has nothing to edit");
    let e = h.state_mut().session.execute("node.editor", &json!({"node": up})).map(|_| ());
    assert!(e.is_ok());
    let e = h.state_mut().session.execute("node.editor", &json!({"node": down})).unwrap_err().to_string();
    assert!(e.contains("comes from a wire"), "{e}");
}

#[test]
fn it_sees_what_the_last_run_sent_for_its_kind_only() {
    let (mut h, id) = app("test.dialled");
    h.state_mut().run("node.editor", &json!({"node": id})).unwrap();
    h.run_steps(3);
    assert!(has(&h, "the run said nothing"));
    let run = h.state_mut().session.run.nodes.entry(id).or_default();
    run.editor.insert("test.dial".into(), json!({"bins": [1, 2]}));
    run.editor.insert("test.other".into(), json!("not mine"));
    h.run_steps(3);
    assert!(has(&h, r#"the run said {"bins":[1,2]}"#), "its kind's payload, opaque to core");
}

#[test]
fn closing_the_window_or_losing_the_node_closes_the_editor() {
    let (mut h, id) = app("test.dialled");
    h.state_mut().run("node.editor", &json!({"node": id})).unwrap();
    h.run_steps(3);
    assert!(!h.state().value_editors.is_empty());
    h.state_mut().run("node.editor", &json!({"node": id, "open": false})).unwrap();
    h.run_steps(3);
    assert!(!has(&h, "dial reads"));
    assert!(h.state().value_editors.is_empty());

    h.state_mut().run("node.editor", &json!({"node": id})).unwrap();
    h.run_steps(3);
    h.state_mut().run("node.remove", &json!({"id": id})).unwrap();
    h.run_steps(3);
    assert!(h.state().session.views.editors.is_empty(), "a node gone takes its editor");
    assert!(h.state().value_editors.is_empty());
    assert!(!has(&h, "dial reads"));
}

/// The offscreen renderer has a context of its own, where the window's textures mean nothing.
#[test]
fn an_editor_lets_go_of_its_textures_around_an_offscreen_render() {
    let (mut h, id) = app("test.dialled");
    h.state_mut().run("node.editor", &json!({"node": id})).unwrap();
    h.run_steps(3);
    plumcraft_ui::offscreen::render(h.state_mut(), egui::vec2(800.0, 500.0), 1.0, None).unwrap();
    assert_eq!(h.state().value_editors.inspect()[0]["state"]["forgot"], 2, "before it draws, and after");
}

/// `STAGE7_SHOTS=dir`: the window as a person sees it, for reading.
#[test]
fn shot_for_review() {
    let Ok(dir) = std::env::var("STAGE7_SHOTS") else { return };
    setup();
    let mut s = Session::new(catalog());
    let id = add(&mut s, "test.dialled", 40.0, 40.0);
    s.execute("node.parameters", &json!({"node": id})).unwrap();
    s.execute("node.editor", &json!({"node": id})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 760.0)).wgpu().build_ui_state(|ui, app: &mut EditorApp| app.show(ui), EditorApp::new(s));
    h.run_steps(8);
    h.render().unwrap().save(std::path::Path::new(&dir).join("seam.png")).unwrap();
}
