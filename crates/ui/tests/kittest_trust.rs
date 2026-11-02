//! A click on a node's title runs that node and what it needs, not the whole graph; a workflow from
//! elsewhere asks before it runs, in the window and over the control channel.

mod common;

use egui_kittest::kittest::Queryable;
use plumcraft_session::run::Outgoing;
use plumcraft_ui::{EditorApp, Prompt, PromptAnswer};
use serde_json::json;

use common::{add, harness, session};

fn sent_targets(app: &mut EditorApp) -> Vec<Vec<String>> {
    app.session.run.take_outbox().into_iter().filter_map(|o| if let Outgoing::Run(r) = o { Some(r.targets.clone()) } else { None }).collect()
}

#[test]
fn watching_a_node_runs_only_that_node() {
    let mut s = session();
    s.connected = true;
    let a = add(&mut s, "text.literal", 40.0, 40.0);
    let b = add(&mut s, "text.upper", 300.0, 40.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}})).unwrap();
    let mut app = EditorApp::new(s);
    app.run("run.watch", &json!({"node": a}));
    assert_eq!(sent_targets(&mut app), [[a.clone()]], "not the sink downstream of it");
}

#[test]
fn a_stranger_asks_in_the_window() {
    let at = plumcraft_scratch::dir("kittest-trust");
    let file = at.join("stranger.json");
    std::fs::write(&file, json!({"version": 1, "nodes": {"a": {"kind": "text.literal", "values": {"text": "hi"}}}}).to_string()).unwrap();
    let mut s = session();
    s.connected = true;
    s.set_trust_file(at.join("trusted.json"));
    s.execute("graph.open", &json!({"path": file})).unwrap();
    s.execute("run.start", &json!({})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(3);
    assert!(matches!(h.state().prompt, Some(Prompt::Review(_))), "the window asks");
    assert!(h.query_by_label("Trust and Run").is_some());
    assert!(!h.state_mut().answer_prompt(PromptAnswer::Discard), "that isn't an answer to this");
    assert!(h.state_mut().answer_prompt(PromptAnswer::Cancel));
    h.run_steps(3);
    assert!(h.state().prompt.is_none() && !h.state().session.run.running, "cancelled, and it doesn't ask again");
    // (a frame with no engine link says it is disconnected)
    h.state_mut().session.connected = true;
    h.state_mut().run("run.start", &json!({}));
    h.run_steps(3);
    assert!(matches!(h.state().prompt, Some(Prompt::Review(_))), "it asks again");
    assert!(h.state_mut().answer_prompt(PromptAnswer::Trust));
    h.run_steps(3);
    assert!(h.state().prompt.is_none() && !h.state().session.run.awaiting_review());
}

/// An engine that wants a file sent, as an attached one does.
struct Attached(plumcraft_host::InProc, std::sync::Mutex<bool>);

impl plumcraft_host::Connection for Attached {
    fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, plumcraft_proto::error::ProtoError> {
        self.0.call(method, params)
    }

    fn state(&self) -> plumcraft_proto::session::EngineState {
        self.0.state()
    }

    fn resolve(&self, r: &plumcraft_proto::data::DataRef) -> Option<plumcraft_store::Blob> {
        self.0.resolve(r)
    }

    fn uploads_pending(&self, _: &serde_json::Value) -> Vec<(String, String)> {
        if *self.1.lock().unwrap() { Vec::new() } else { vec![("http://engine.example:7311".into(), "/home/me/photo.jpg".into())] }
    }

    fn allow_uploads(&self, _: &[String]) {
        *self.1.lock().unwrap() = true;
    }
}

fn attached() -> EditorApp {
    let host = plumcraft_engine::host::Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(std::time::Duration::from_secs(5));
    let mut s = plumcraft_session::Session::new(serde_json::from_value(host.schema(None).unwrap()).unwrap());
    s.connected = true;
    add(&mut s, "text.literal", 40.0, 40.0);
    let conn = std::sync::Arc::new(Attached(plumcraft_host::InProc(host), std::sync::Mutex::new(false)));
    EditorApp::new(s).with_link(plumcraft_ui::link::Link::start(conn, "attached", None).unwrap())
}

#[test]
fn files_for_an_attached_engine_ask_first() {
    let mut h = harness(attached());
    h.state_mut().run("run.start", &json!({}));
    h.run_steps(3);
    assert!(matches!(&h.state().prompt, Some(Prompt::Uploads(p)) if p[0].1 == "/home/me/photo.jpg"), "the window asks");
    assert!(h.query_by_label("Send Files").is_some());
    assert!(h.state().session.run.running, "the run waits");
    assert!(h.state_mut().answer_prompt(PromptAnswer::Cancel));
    h.run_steps(3);
    assert!(!h.state().session.run.running, "and is cancelled with it");

    h.state_mut().run("run.start", &json!({}));
    h.run_steps(3);
    assert!(h.state_mut().answer_prompt(PromptAnswer::Trust));
    h.run_steps(3);
    assert!(h.state().prompt.is_none(), "it isn't asked again");
    for _ in 0..200 {
        h.run_steps(1);
        if !h.state().session.run.running {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the allowed run never finished");
}
