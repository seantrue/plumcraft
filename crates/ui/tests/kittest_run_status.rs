//! What a node's header says about its runs: "not run" until a run completes it (B21), never
//! "stale" for a node with no result.

mod common;

use std::time::Instant;

use common::{add, harness};
use egui_kittest::kittest::Queryable;
use plumcraft_proto::event::NodeState;
use plumcraft_proto::run::{NodeReport, RunReply};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use plumcraft_ui::run_view::NOT_RUN;
use serde_json::json;

/// Start a run and answer it with `state` for `node`.
fn ran(s: &mut Session, node: &str, state: NodeState) {
    s.connected = true;
    let req = s.execute("run.start", &json!({})).unwrap();
    let _ = s.run.take_outbox();
    let mut reply = RunReply { run_id: req["run_id"].as_str().unwrap().to_owned(), generation: s.run.generation, ok: true, ..Default::default() };
    reply.nodes.insert(node.to_owned(), NodeReport { state, ms: 5.0, ..Default::default() });
    assert!(s.run.apply_reply(&reply, Instant::now()));
}

fn one() -> (Session, String) {
    let mut s = common::session();
    let n = add(&mut s, "test.knobs", 40.0, 40.0);
    (s, n)
}

#[test]
fn a_node_that_has_never_run_says_not_run() {
    let (s, _) = one();
    let h = harness(EditorApp::new(s));
    assert!(h.query_by_label(NOT_RUN).is_some());
    assert!(h.query_by_label("stale").is_none());
}

#[test]
fn a_node_that_failed_and_was_edited_says_not_run_not_stale() {
    let (mut s, n) = one();
    ran(&mut s, &n, NodeState::Error);
    s.execute("node.set", &json!({"node": n, "port": "times", "value": 4})).unwrap();
    assert!(s.run.node(&n).unwrap().is_stale(), "the edit is tracked");
    let h = harness(EditorApp::new(s));
    assert!(h.query_by_label(NOT_RUN).is_some(), "no run has completed it");
    assert!(h.query_by_label("stale").is_none(), "it has no result to be stale");
}

#[test]
fn a_node_that_completed_and_was_edited_still_says_stale() {
    let (mut s, n) = one();
    ran(&mut s, &n, NodeState::Done);
    let mut h = harness(EditorApp::new(s));
    assert!(h.query_by_label(NOT_RUN).is_none(), "done");
    assert!(h.query_by_label("stale").is_none());
    h.state_mut().session.execute("node.set", &json!({"node": n, "port": "times", "value": 4})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label("stale").is_some());
    assert!(h.query_by_label(NOT_RUN).is_none());
}
