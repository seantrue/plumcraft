//! A list of lists in the Watched panel (interface 7.6 rule 2, decision 6A): one stepper over the
//! runs and one over the items in the run shown, each asking `run.item` for its own level.

mod common;

use common::{add, harness, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_proto::catalogue::Media;
use plumcraft_proto::display::Display;
use plumcraft_session::run::WatchRole;
use plumcraft_ui::EditorApp;
use serde_json::json;

/// A node watched in the panel, showing run 2 of 3, item 1 of 2.
fn watched() -> Harness<'static, EditorApp> {
    let mut s = session();
    let node = add(&mut s, "text.upper", 40.0, 40.0);
    s.connected = true;
    s.execute("run.watch", &json!({"node": node})).unwrap();
    let port = s.run.panel().unwrap().0.port.clone();
    let d = Display {
        node,
        port,
        media: Media::Text,
        items: 3,
        index: 1,
        path: vec![0],
        levels: vec![3, 2],
        summary: "run 2 of 3 · item 1 of 2: a value".into(),
        ..Default::default()
    };
    s.run.set_display(WatchRole::Panel, Some(d));
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    h
}

#[test]
fn a_list_of_lists_steps_its_runs_and_the_items_in_a_run() {
    let mut h = watched();
    assert!(h.query_by_label("run 2 of 3").is_some(), "the outer stepper steps runs");
    assert!(h.query_by_label("item 1 of 2").is_some(), "the inner stepper steps the run's items");
    // the next item in this run: the same run, the path one on
    let next: Vec<_> = h.query_all_by_label("▶").map(|n| n.rect().center()).collect();
    assert_eq!(next.len(), 2, "two steppers");
    common::click_at(&mut h, next[1]);
    h.run_steps(3);
    let w = h.state().session.run.panel().unwrap().0.clone();
    assert_eq!((w.index, w.path.clone()), (1, vec![1]));
}

#[test]
fn a_flat_list_has_one_stepper_of_items() {
    let steps = plumcraft_ui::watched::level_steps(&Display { items: 3, ..Default::default() });
    assert!(steps.is_empty());
    let d = Display { items: 3, path: vec![1, 2], levels: vec![3, 2, 4], ..Default::default() };
    let steps = plumcraft_ui::watched::level_steps(&d);
    assert_eq!(steps.iter().map(|s| (s.word, s.at, s.of)).collect::<Vec<_>>(), [("run", 1, 2), ("item", 2, 4)]);
    assert_eq!(steps[0].path(0), vec![0], "a middle level's step goes back to the first item below it");
    assert_eq!(steps[1].path(3), vec![1, 3]);
}
