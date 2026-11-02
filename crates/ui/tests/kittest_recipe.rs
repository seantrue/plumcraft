//! The recipe in the Watched panel: what made the value shown, from the engine's display. It says
//! what differs from the defaults, copies as JSON, and anything malformed is left out.

mod common;

use common::{add, click, harness, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_proto::catalogue::Media;
use plumcraft_proto::display::Display;
use plumcraft_session::run::WatchRole;
use plumcraft_ui::EditorApp;
use serde_json::{Value, json};

/// A node watched in the panel, its value carrying `recipe`.
fn watched(recipe: Option<Value>) -> Harness<'static, EditorApp> {
    let mut s = session();
    let node = add(&mut s, "text.upper", 40.0, 40.0);
    s.connected = true;
    s.execute("run.watch", &json!({"node": node})).unwrap();
    let port = s.run.panel().unwrap().0.port.clone();
    s.run.set_display(WatchRole::Panel, Some(Display { node, port, media: Media::Text, summary: "a value".into(), recipe, ..Default::default() }));
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    h
}

fn pencil() -> Value {
    json!({"style": "pencil", "params": {"Scales.scales": "[1, 2]", "grad.sigma": 1.5},
           "settings": {"blackandwhite": {"scales": 3}}, "graph": {"nodes": {}}})
}

fn open(h: &mut Harness<'static, EditorApp>, title: &str) {
    click(h, title);
    h.run_steps(4);
}

#[test]
fn the_panel_says_what_made_the_value_and_copies_it() {
    let mut h = watched(Some(pencil()));
    assert!(h.query_by_label("[1, 2]").is_none(), "closed: the viewer keeps the height");
    open(&mut h, "Recipe · pencil");
    assert!(h.query_by_label("Scales.scales").is_some());
    assert!(h.query_by_label("[1, 2]").is_some(), "text without its quotes");
    assert!(h.query_by_label("1.5").is_some());
    assert!(h.query_by_label("blackandwhite.scales").is_none(), "settings are one more click");
    open(&mut h, "Settings");
    assert!(h.query_by_label("blackandwhite.scales").is_some());

    click(&mut h, "Copy recipe");
    h.run_steps(3);
    let copied: Value = serde_json::from_str(h.state().copied.as_deref().expect("on the clipboard")).unwrap();
    assert_eq!(copied, pencil(), "the whole of it, graph included");
}

#[test]
fn a_command_reads_it() {
    let mut h = watched(Some(pencil()));
    assert_eq!(h.state_mut().run("viewer.recipe", &json!({})).unwrap()["recipe"], pencil());
}

#[test]
fn every_value_at_its_default_says_so() {
    let mut h = watched(Some(json!({"params": {}})));
    open(&mut h, "Recipe");
    assert!(h.query_by_label("Every value is its default.").is_some());
}

#[test]
fn no_recipe_shows_nothing() {
    let h = watched(None);
    assert!(h.query_by_label_contains("Recipe").is_none());
}

#[test]
fn a_malformed_recipe_is_left_out_or_shown_as_far_as_it_fits() {
    for bad in [json!(7), json!("recipe"), json!([1, 2]), json!(null)] {
        let h = watched(Some(bad.clone()));
        assert!(h.query_by_label_contains("Recipe").is_none(), "{bad}");
    }
    let mut h = watched(Some(json!({"style": 7, "params": "many", "settings": {"a": 3, "b": {"c": [1]}}})));
    open(&mut h, "Recipe");
    assert!(h.query_by_label("Every value is its default.").is_some(), "params that aren't a map");
    open(&mut h, "Settings");
    assert!(h.query_by_label("b.c").is_some(), "the section that fits");
    assert!(h.query_by_label("[1]").is_some());
}
