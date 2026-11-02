//! Renaming a node, driven through egui_kittest: from its menu and from the parameter editor's
//! header, in a window of its own (a text field can't live in a menu).

mod common;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use plumcraft_ui::EditorApp;
use serde_json::{Value, json};

use common::{add, click, click_in_node, harness, session};

fn title(h: &Harness<'_, EditorApp>, n: &str) -> Option<String> {
    h.state().session.graph().node(&n.into()).unwrap().title.clone()
}

fn one() -> (Harness<'static, EditorApp>, String) {
    let mut s = session();
    let id = add(&mut s, "text.upper", 40.0, 40.0);
    let mut h = harness(EditorApp::new(s));
    h.run_steps(3);
    (h, id)
}

/// The node's menu › Rename…
fn open_from_menu(h: &mut Harness<'static, EditorApp>) {
    click_in_node(h, "Node actions");
    h.run_steps(2);
    h.get_by_label("Rename…").click();
    h.run_steps(4);
}

/// Replace what the window's field holds, by the keyboard.
fn type_title(h: &mut Harness<'static, EditorApp>, text: &str) {
    assert!(h.state().rename.is_some(), "the window is open");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(2);
    h.event(egui::Event::Text(text.to_owned()));
    h.run_steps(3);
}

fn enter(h: &mut Harness<'static, EditorApp>) {
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
}

#[test]
fn the_nodes_menu_opens_the_window_and_enter_applies_as_one_undo_step() {
    let (mut h, id) = one();
    let shown = |h: &Harness<'_, EditorApp>, t: &str| h.query_by_label(t).is_some();
    assert!(shown(&h, "Upper case"), "titled by its type");
    open_from_menu(&mut h);
    let dialog = h.state().rename.as_ref().expect("the window");
    assert_eq!((dialog.text.as_str(), dialog.hint.as_str()), ("", "Upper case"));
    type_title(&mut h, "Shout");
    enter(&mut h);
    assert!(h.state().rename.is_none(), "it closes once applied");
    assert_eq!(title(&h, &id).as_deref(), Some("Shout"));
    assert!(shown(&h, "Shout") && !shown(&h, "Upper case"), "the canvas shows the new title");
    assert_eq!(h.state().session.undo_label(), Some("Rename Node"));
    h.state_mut().run("edit.undo", &Value::Null);
    h.run_steps(3);
    assert_eq!(title(&h, &id), None);
    assert!(shown(&h, "Upper case"));
}

#[test]
fn the_field_has_the_keyboard_as_it_opens() {
    let (mut h, _) = one();
    open_from_menu(&mut h);
    h.event(egui::Event::Text("x".to_owned()));
    h.run_steps(3);
    assert_eq!(h.state().rename.as_ref().unwrap().text, "x");
    assert!(h.state().canvas.palette.is_none(), "the canvas's keys stay out of it");
}

#[test]
fn the_rename_button_applies_and_is_off_until_the_title_changes() {
    let (mut h, id) = one();
    open_from_menu(&mut h);
    assert!(h.get_by_label("Rename").accesskit_node().is_disabled(), "nothing typed");
    type_title(&mut h, "Shout");
    assert!(!h.get_by_label("Rename").accesskit_node().is_disabled());
    click(&mut h, "Rename");
    h.run_steps(3);
    assert_eq!(title(&h, &id).as_deref(), Some("Shout"));
    assert!(h.state().rename.is_none());
}

#[test]
fn a_blank_title_goes_back_to_the_type() {
    let (mut h, id) = one();
    h.state_mut().session.execute("node.title", &json!({"node": id, "title": "Shout"})).unwrap();
    h.run_steps(3);
    open_from_menu(&mut h);
    assert_eq!(h.state().rename.as_ref().unwrap().text, "Shout", "it opens on the title");
    h.state_mut().rename.as_mut().unwrap().text = "   ".into();
    h.run_steps(2);
    click(&mut h, "Rename");
    h.run_steps(3);
    assert_eq!(title(&h, &id), None);
    assert!(h.query_by_label("Upper case").is_some());
}

#[test]
fn esc_and_cancel_change_nothing() {
    let (mut h, id) = one();
    for leave in ["esc", "cancel"] {
        open_from_menu(&mut h);
        type_title(&mut h, "Nope");
        let journal = h.state().session.journal().len();
        match leave {
            "esc" => {
                h.key_press(egui::Key::Escape);
                h.run_steps(3);
            }
            _ => click(&mut h, "Cancel"),
        }
        assert!(h.state().rename.is_none(), "{leave}");
        assert_eq!(title(&h, &id), None, "{leave}");
        assert_eq!(h.state().session.journal().len(), journal, "{leave}");
    }
}

#[test]
fn enter_on_what_is_there_closes_without_an_edit() {
    let (mut h, _) = one();
    open_from_menu(&mut h);
    let journal = h.state().session.journal().len();
    enter(&mut h);
    assert!(h.state().rename.is_none());
    assert_eq!(h.state().session.journal().len(), journal);
}

#[test]
fn enter_on_a_title_too_long_keeps_the_window_and_the_keyboard() {
    let (mut h, id) = one();
    open_from_menu(&mut h);
    type_title(&mut h, &"x".repeat(201));
    enter(&mut h);
    assert!(h.state().rename.is_some(), "it says why, and stays");
    assert_eq!(title(&h, &id), None);
    assert!(h.get_by_label("Rename").accesskit_node().is_disabled());
    h.event(egui::Event::Key { key: egui::Key::Backspace, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(3);
    assert_eq!(h.state().rename.as_ref().unwrap().text.len(), 200, "the field still has the keyboard");
}

#[test]
fn delete_while_the_window_is_open_doesnt_touch_the_selection() {
    let (mut h, id) = one();
    h.state_mut().canvas.picked.set(vec![id.as_str().into()]);
    open_from_menu(&mut h);
    click(&mut h, "Title");
    h.key_press(egui::Key::Backspace);
    h.run_steps(3);
    assert!(h.state().session.graph().node(&id.as_str().into()).is_some());
}

#[test]
fn the_node_going_closes_the_window() {
    let (mut h, id) = one();
    open_from_menu(&mut h);
    h.state_mut().run("node.remove", &json!({"nodes": [id]}));
    h.run_steps(3);
    assert!(h.state().rename.is_none());
}

#[test]
fn a_collapsed_groups_node_has_no_node_rename() {
    let mut s = session();
    let id = add(&mut s, "text.upper", 40.0, 40.0);
    let g = s.execute("group.create", &json!({"nodes": [id], "name": "G"})).unwrap()["id"].as_str().unwrap().to_owned();
    s.execute("group.collapse", &json!({"group": g, "collapsed": true})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(3);
    click_in_node(&mut h, "Node actions");
    h.run_steps(2);
    h.get_by_label("Rename…").click();
    h.run_steps(4);
    assert!(h.state().rename.is_none(), "that item is the group's, with its own window");
    assert!(h.query_by_label("Rename group G").is_some());
}

// ---------- the parameter editor's header ----------

fn params() -> (Harness<'static, EditorApp>, String) {
    let mut s = session();
    let id = add(&mut s, "test.dials", 40.0, 40.0);
    s.execute("node.parameters", &json!({"node": id})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    (h, id)
}

#[test]
fn the_editors_header_renames_and_its_title_follows() {
    let (mut h, id) = params();
    assert!(h.query_by_label("Dials · parameters").is_some());
    click(&mut h, "Rename…");
    h.run_steps(4);
    let dialog = h.state().rename.as_ref().expect("the window");
    assert_eq!(dialog.hint, "Dials");
    type_title(&mut h, "Knobs of mine");
    enter(&mut h);
    assert_eq!(title(&h, &id).as_deref(), Some("Knobs of mine"));
    assert!(h.query_by_label("Knobs of mine · parameters").is_some(), "the window's title");
    assert!(h.state().session.views.parameters.is_some(), "the editor stays open");
}

#[test]
fn a_long_title_is_cut_and_the_button_stays_in_the_window() {
    let (mut h, id) = params();
    let width = |h: &Harness<'_, EditorApp>| h.get_by_label("Rename…").rect().right();
    let before = width(&h);
    h.state_mut().session.execute("node.title", &json!({"node": id, "title": "w".repeat(200)})).unwrap();
    h.run_steps(6);
    assert!((width(&h) - before).abs() < 2.0, "the window keeps its width: {before} -> {}", width(&h));
    // and the title on the button's row stops short of it
    let button = h.get_by_label("Rename…").rect();
    let on_its_row: Vec<egui::Rect> =
        h.query_all_by_label_contains("wwww").map(|n| n.rect()).filter(|r| r.height() < 40.0 && r.y_range().contains(button.center().y)).collect();
    assert_eq!(on_its_row.len(), 1, "the title, not the window: {on_its_row:?}");
    assert!(on_its_row[0].right() <= button.left(), "{:?} runs into {button:?}", on_its_row[0]);
}
