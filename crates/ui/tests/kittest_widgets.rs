//! Node widgets, pin labels and the palette, driven through egui_kittest, against the text
//! engine's catalogue.

mod common;

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use plumcraft_proto::catalogue::PathHint;
use plumcraft_ui::canvas::SOCKET_LABEL_WIDTH;
use plumcraft_ui::{EditorApp, PathDialogs};
use serde_json::json;

use common::{add, click_canvas, harness, on_screen, session, value};

#[test]
fn a_float_drag_commits_once_on_release() {
    let mut s = session();
    let slow = add(&mut s, "text.slow", 40.0, 40.0);
    let mut h = harness(EditorApp::new(s));
    let start = on_screen(&h, h.get_by_role(Role::SpinButton).rect()).center();
    let journal_before = h.state().session.journal().len();
    h.drag_at(start);
    h.run_steps(1);
    for dx in [5.0, 10.0, 20.0, 30.0] {
        h.hover_at(start + egui::vec2(dx, 0.0));
        h.run_steps(1);
    }
    assert_eq!(value(&h, &slow, "seconds"), json!(1.0), "nothing is set while dragging");
    assert_eq!(h.state().session.journal().len(), journal_before);
    h.drop_at(start + egui::vec2(30.0, 0.0));
    h.run_steps(3);
    let seconds = value(&h, &slow, "seconds").as_f64().unwrap();
    assert!(seconds > 1.0, "{seconds}");
    assert_eq!((seconds * 10.0).round() / 10.0, seconds, "one decimal, as the default has");
    let sets = h.state().session.journal().iter().filter(|e| e.id == "node.set").count();
    assert_eq!(sets, 1, "one journal entry per drag");
}

#[test]
fn an_enum_choice_is_set_and_its_sub_port_follows() {
    let mut s = session();
    let knobs = add(&mut s, "test.knobs", 40.0, 40.0);
    let mut h = harness(EditorApp::new(s));
    assert!(h.query_by_label("mode › flourish").is_none());
    let combo = h.get_by_value("plain").rect();
    click_canvas(&mut h, combo);
    h.get_by_label("fancy").click();
    h.run_steps(4);
    assert_eq!(value(&h, &knobs, "mode"), json!("fancy"));
    h.get_by_label("mode › flourish");
    let combo = h.get_by_value("fancy").rect();
    click_canvas(&mut h, combo);
    h.get_by_label("plain").click();
    h.run_steps(4);
    assert!(h.query_by_label("mode › flourish").is_none());
}

#[test]
fn a_bool_toggles_and_a_string_commits_on_enter() {
    let mut s = session();
    let knobs = add(&mut s, "test.knobs", 40.0, 40.0);
    let text = add(&mut s, "text.literal", 40.0, 360.0);
    let mut h = harness(EditorApp::new(s));
    let check = h.get_by_role(Role::CheckBox).rect();
    click_canvas(&mut h, check);
    assert_eq!(value(&h, &knobs, "loud"), json!(false));

    let field = h.get_by_role(Role::TextInput).rect();
    click_canvas(&mut h, field);
    h.event(egui::Event::Text("corgi".into()));
    h.run_steps(1);
    assert_eq!(value(&h, &text, "text"), json!(""), "not committed while typing");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(value(&h, &text, "text"), json!("corgi"));
}

/// A file dialog that answers with a fixed path and records what it was asked.
struct PickPath(std::rc::Rc<std::cell::RefCell<Vec<(String, String)>>>);

impl PathDialogs for PickPath {
    fn pick_path(&mut self, hint: &PathHint, current: &str) -> Option<std::path::PathBuf> {
        self.0.borrow_mut().push((hint.mode.clone(), current.to_owned()));
        Some("/stories/corgi.txt".into())
    }
}

#[test]
fn a_path_widget_picks_a_file_with_a_dialog() {
    let mut s = session();
    let load = add(&mut s, "text.load", 40.0, 40.0);
    s.execute("node.set", &json!({"node": load, "port": "path", "value": "/old/story.txt"})).unwrap();
    let mut h = harness(EditorApp::new(s));
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    h.state_mut().dialogs = Box::new(PickPath(asked.clone()));
    let button = h.get_by_label("…").rect();
    click_canvas(&mut h, button);
    assert_eq!(value(&h, &load, "path"), json!("/stories/corgi.txt"));
    assert_eq!(*asked.borrow(), [("open".to_owned(), "/old/story.txt".to_owned())], "the port's hint and the current path");
    // only path ports get the button
    let mut s = session();
    add(&mut s, "text.replace", 40.0, 40.0);
    let h = harness(EditorApp::new(s));
    assert!(h.query_by_label("…").is_none());
}

#[test]
fn every_pin_has_a_label_and_wired_widgets_say_where_from() {
    let mut s = session();
    let text = add(&mut s, "text.literal", 40.0, 40.0);
    let join = add(&mut s, "text.join", 420.0, 40.0);
    let words = add(&mut s, "text.words", 820.0, 40.0);
    add(&mut s, "text.letters", 40.0, 330.0);
    s.execute("wire.connect", &json!({"from": {"node": text, "port": "text"}, "to": {"node": join, "port": "a"}})).unwrap();
    s.execute("wire.connect", &json!({"from": {"node": join, "port": "text"}, "to": {"node": words, "port": "text"}})).unwrap();
    let h = harness(EditorApp::new(s));
    for label in ["a", "b", "sep", "count", "table", "text"] {
        assert!(h.query_all_by_label_contains(label).count() >= 1, "no label {label:?}");
    }
    // A socket whose name doesn't say its type says it after the name; one whose name does, doesn't.
    assert!(h.query_by_label("a · TEXT").is_some(), "join's `a` names its type");
    assert!(h.query_by_label("table · TABLE").is_none(), "a name that is its type isn't repeated");
    // A widget row's caption sits left of its widget, never over it.
    let caption = h.get_by_label("sep").rect();
    let fields: Vec<egui::Rect> = h.query_all_by_role(Role::TextInput).map(|n| n.rect()).collect();
    let widget = fields.iter().find(|r| (r.center().y - caption.center().y).abs() < 4.0).expect("sep's field");
    assert!(caption.right() <= widget.left(), "{caption:?} overlaps {widget:?}");
}

#[test]
fn long_labels_truncate_with_the_full_name_on_hover() {
    let mut catalog = common::catalog();
    catalog.nodes.push(
        serde_json::from_value(json!({"id": "long", "display": "Long", "inputs": [
            {"name": "use.a_very_long_input_name_with_arguments.and_an_element_name_too", "type": "TEXT", "socket": "socket"}]}))
        .unwrap(),
    );
    let mut s = plumcraft_session::Session::new(catalog);
    add(&mut s, "long", 40.0, 40.0);
    let h = harness(EditorApp::new(s));
    let node = h.get_by_label_contains("use.a_very_long");
    assert!(node.rect().width() <= SOCKET_LABEL_WIDTH + 1.0, "{:?}", node.rect());
}

#[test]
fn the_palette_searches_and_adds_at_the_cursor() {
    let mut h = harness(EditorApp::new(session()));
    h.hover_at(egui::pos2(500.0, 400.0));
    h.run_steps(1);
    h.key_press(egui::Key::Tab);
    h.run_steps(3);
    assert!(h.state().canvas.palette.is_some(), "Tab opens the palette");
    h.get_by_label("Upper case");
    h.get_by_label("Word count");
    h.event(egui::Event::Text("upper".into()));
    h.run_steps(3);
    assert_eq!(h.state().canvas.palette.as_ref().unwrap().query, "upper");
    assert!(h.query_by_label("Word count").is_none(), "filtered out");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(h.state().canvas.palette.is_none());
    let g = h.state().session.graph();
    assert_eq!(g.nodes.len(), 1);
    let (_, node) = g.nodes.iter().next().unwrap();
    assert_eq!(node.kind, "text.upper");
    // Placed where the palette opened (graph coordinates of the cursor).
    let at = h.state().canvas.to_global * egui::pos2(node.pos[0], node.pos[1]);
    assert!((at - egui::pos2(500.0, 400.0)).length() < 2.0, "{at:?}");
}

#[test]
fn right_click_opens_the_palette_and_escape_closes_it() {
    let mut h = harness(EditorApp::new(session()));
    let at = egui::pos2(600.0, 300.0);
    h.hover_at(at);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(3);
    assert!(h.state().canvas.palette.is_some());
    h.get_by_label("Knobs");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().canvas.palette.is_none());
}
