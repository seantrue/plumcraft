//! What a node's header says about its runs: "not run" until a run completes it (B21), never
//! "stale" for a node with no result; "queued" and "running" while a run computes it, over
//! "stale" (B20).

mod common;

use std::time::Instant;

use common::{add, harness};
use egui_kittest::kittest::Queryable;
use plumcraft_proto::event::{Envelope, Event, NodeState, StatusRecord};
use plumcraft_proto::run::{NodeReport, RunReply};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use plumcraft_ui::run_view::{NOT_RUN, shows_stale};
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

/// A status record of the run in flight, as the engine sends it.
fn status(s: &mut Session, node: &str, state: NodeState, fraction: Option<f64>) {
    let run_id = s.run.run_id.clone().unwrap();
    let rec = StatusRecord { node_id: node.to_owned(), publisher: "p".into(), seq: u64::from(state.rank()), state, fraction, ..Default::default() };
    let e = Envelope::new(1, 0.0, Some((&run_id, s.run.generation)), Event::Status(Box::new(rec)));
    assert!(s.run.apply_event(&e, Instant::now()));
}

/// The common case: an edit makes a node stale, and the run that follows computes it again. While
/// it does, it says queued, then running, not stale.
#[test]
fn a_stale_node_that_a_run_is_computing_says_queued_then_running() {
    let (mut s, n) = one();
    ran(&mut s, &n, NodeState::Done);
    s.execute("node.set", &json!({"node": n, "port": "times", "value": 4})).unwrap();
    s.execute("run.start", &json!({})).unwrap();
    let _ = s.run.take_outbox();
    let mut h = harness(EditorApp::new(s));
    assert!(h.query_by_label("queued").is_some());
    assert!(h.query_by_label("stale").is_none(), "queued, not stale");
    assert!(!shows_stale(h.state().session.run.node(&n).unwrap()), "nor its border");

    status(&mut h.state_mut().session, &n, NodeState::Started, None);
    h.run_steps(4);
    assert!(h.query_by_label("running").is_some());
    assert!(h.query_by_label("stale").is_none(), "running, not stale");
    assert!(!shows_stale(h.state().session.run.node(&n).unwrap()));

    status(&mut h.state_mut().session, &n, NodeState::Progress, Some(0.4));
    h.run_steps(4);
    assert!(h.query_by_label("running 40%").is_some(), "how far it has got");
}

/// A run cancelled before it reached the node leaves the edit's mark.
#[test]
fn a_stale_node_the_run_did_not_reach_says_stale_again() {
    let (mut s, n) = one();
    ran(&mut s, &n, NodeState::Done);
    s.execute("node.set", &json!({"node": n, "port": "times", "value": 4})).unwrap();
    s.execute("run.start", &json!({})).unwrap();
    s.run.fail("the engine went away");
    let h = harness(EditorApp::new(s));
    assert!(h.query_by_label("stale").is_some());
    assert!(shows_stale(h.state().session.run.node(&n).unwrap()));
}
