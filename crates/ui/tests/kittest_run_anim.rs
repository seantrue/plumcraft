//! Run colours: while a run computes the graph, each node is shown waiting, running, then
//! done or cached, and with slow motion a fast node is shown running for a while after it really
//! finished. The settings are commands, kept in the preferences.

mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use common::{add, harness};
use egui_kittest::Harness;
use plumcraft_proto::event::{Envelope, Event, NodeState, StatusRecord};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use plumcraft_ui::run_anim::Shown;
use serde_json::json;

fn two() -> (Session, String, String) {
    let mut s = common::session();
    let a = add(&mut s, "test.knobs", 40.0, 40.0);
    let b = add(&mut s, "test.knobs", 400.0, 40.0);
    s.connected = true;
    (s, a, b)
}

fn start(h: &mut Harness<'_, EditorApp>) {
    // (no engine: the app clears this each frame, so it is set just before the run)
    h.state_mut().session.connected = true;
    h.state_mut().session.execute("run.start", &json!({})).unwrap();
    // (the request isn't sent: with no engine the run would end at once)
    let _ = h.state_mut().session.run.take_outbox();
    h.run_steps(2);
}

/// A publisher's records count up (older ones are dropped).
static SEQ: AtomicU64 = AtomicU64::new(1);

/// A status record of the run in flight, as the engine sends it.
fn status(h: &mut Harness<'_, EditorApp>, node: &str, state: NodeState) {
    let s = &mut h.state_mut().session;
    let run_id = s.run.run_id.clone().unwrap();
    let rec = StatusRecord { node_id: node.to_owned(), publisher: "p".into(), seq: SEQ.fetch_add(1, Ordering::Relaxed), state, ..Default::default() };
    let e = Envelope::new(1, 0.0, Some((&run_id, s.run.generation)), Event::Status(Box::new(rec)));
    assert!(s.run.apply_event(&e, Instant::now()), "{node} {state:?}");
    h.run_steps(2);
}

fn shown(h: &Harness<'_, EditorApp>, node: &str) -> Option<Shown> {
    h.state().run_anim.shown(node, Instant::now())
}

#[test]
fn a_run_shows_each_node_waiting_running_then_done() {
    let (s, a, b) = two();
    let mut h = harness(EditorApp::new(s));
    assert_eq!(shown(&h, &a), None, "at rest: nothing shown");
    start(&mut h);
    assert_eq!(shown(&h, &a), Some(Shown::Waiting));
    assert_eq!(shown(&h, &b), Some(Shown::Waiting));
    status(&mut h, &a, NodeState::Started);
    assert_eq!(shown(&h, &a), Some(Shown::Running));
    status(&mut h, &a, NodeState::Done);
    status(&mut h, &b, NodeState::Cached);
    assert_eq!(shown(&h, &a), Some(Shown::Done));
    assert_eq!(shown(&h, &b), Some(Shown::Cached));
    assert!(h.state().run_anim.strength(Instant::now()) > 0.0, "still showing as the run ends");
}

#[test]
fn slow_motion_keeps_a_fast_node_running_a_while() {
    let (s, a, _) = two();
    let mut h = harness(EditorApp::new(s));
    h.state_mut().session.execute("view.slowMotion", &json!({"ms": 1000})).unwrap();
    start(&mut h);
    status(&mut h, &a, NodeState::Started);
    status(&mut h, &a, NodeState::Done);
    assert!(h.state().session.run.node(&a).unwrap().is_finished(), "it really finished");
    assert_eq!(shown(&h, &a), Some(Shown::Running), "but is shown running");
    std::thread::sleep(Duration::from_millis(1100));
    h.run_steps(1);
    assert_eq!(shown(&h, &a), Some(Shown::Done));
}

#[test]
fn the_settings_are_commands_kept_in_the_preferences() {
    let (s, _, _) = two();
    let mut h = harness(EditorApp::new(s));
    assert!(!h.state().session.views.run_colors_off, "on by default");
    h.state_mut().session.execute("view.runColors", &json!({})).unwrap();
    h.state_mut().session.execute("view.slowMotion", &json!({"ms": 250})).unwrap();
    h.run_steps(2);
    let p = h.state().prefs.prefs();
    assert_eq!((p.run_colors, p.slow_ms), (Some(false), Some(250)));
    let s = &mut h.state_mut().session;
    assert!(s.execute("view.slowMotion", &json!({"ms": 6000})).is_err(), "too slow");
    assert!(s.execute("view.slowMotion", &json!({"ms": -1})).is_err());
    assert!(s.execute("view.runColors", &json!({"on": "yes"})).is_err());
}
