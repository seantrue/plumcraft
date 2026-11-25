//! FR5 in egui: a collapsed group's face lists its promoted rows under the title of the node they
//! come from (the core's `section`), and a promoted row's caption menu renames it.

mod common;

use common::{add, harness, session};
use egui_kittest::kittest::Queryable;
use plumcraft_ui::EditorApp;
use serde_json::json;

#[test]
fn a_groups_promoted_rows_sit_under_their_nodes_title() {
    let mut s = session();
    let a = add(&mut s, "text.replace", 40.0, 40.0);
    s.execute("node.title", &json!({"node": a, "title": "Swap"})).unwrap();
    let g = s.execute("group.create", &json!({"nodes": [a], "name": "words", "collapsed": true})).unwrap()["id"].as_str().unwrap().to_owned();
    s.execute("group.promote", &json!({"group": g, "port": {"node": a, "port": "find"}})).unwrap();
    s.execute("group.promote", &json!({"group": g, "port": {"node": a, "port": "with"}})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(6);
    assert_eq!(h.query_all_by_label("Swap ›").count(), 1, "one header for the node's two rows");
    // the header follows a rename of the node
    h.state_mut().run("node.title", &json!({"node": a, "title": "Swap words"})).unwrap();
    h.run_steps(6);
    assert!(h.query_by_label("Swap words ›").is_some());
    // the row's label renamed, the header stays
    h.state_mut().run("group.editPromoted", &json!({"group": g, "label": "find", "newLabel": "look for"})).unwrap();
    h.run_steps(6);
    assert!(h.query_by_label_contains("look for").is_some() && h.query_by_label("Swap words ›").is_some());
}
