//! The shell, driven through egui_kittest's accessibility tree: menus from the command
//! registry, the status bar, and a run end to end through an in-process engine.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{add, click_in_node, harness, session};
use egui_kittest::kittest::Queryable;
use plumcraft_engine::host::Host;
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::event::NodeState;
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use plumcraft_ui::link::Link;
use serde_json::json;

#[test]
fn shell_shows_menus_and_engine_status() {
    let h = harness(EditorApp::new(Session::default()));
    h.get_by_label("engine: not connected");
    for menu in ["File", "Edit", "View", "Node", "Run"] {
        h.get_by_label(menu);
    }
    h.get_by_label("0 nodes, 0 wires");
    h.get_by_label("not run yet");
}

#[test]
fn graph_edits_appear_on_the_canvas_and_menus_run_commands() {
    let mut s = session();
    let a = add(&mut s, "text.literal", 20.0, 20.0);
    let b = add(&mut s, "text.upper", 360.0, 20.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.get_by_label("Upper case");
    h.get_by_label("2 nodes, 1 wires");
    // run.start is disabled without an engine, so its shortcut does nothing
    assert!(!h.state().enabled("run.start"));
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Enter);
    h.run_steps(2);
    assert!(!h.state().session.run.running && h.state().session.run.outbox_is_empty());
    // File › New Graph is graph.new (after a Discard: the graph has unsaved changes)
    h.get_by_label("File").click();
    h.run_steps(2);
    h.get_by_label_contains("New Graph").click();
    h.run_steps(2);
    common::click(&mut h, "Discard");
    h.get_by_label("0 nodes, 0 wires");
    assert!(h.state().session.graph().nodes.is_empty());
    assert_eq!(h.state().session.journal().last().map(|e| e.id.as_str()), Some("graph.new"));
}

#[test]
fn the_selection_deletes_with_the_keyboard() {
    let mut s = session();
    let a = add(&mut s, "text.literal", 20.0, 20.0);
    add(&mut s, "text.upper", 400.0, 20.0);
    let mut h = harness(EditorApp::new(s));
    h.state_mut().canvas.picked.set(vec![a.as_str().into()]);
    h.key_press(egui::Key::Delete);
    h.run_steps(2);
    assert!(h.state().session.graph().node(&a.as_str().into()).is_none());
    h.get_by_label("1 nodes, 0 wires");
}

/// Clicking a node's title watches its output and runs the graph on the engine; the run's
/// states show on the nodes and the value in the Watched panel.
#[test]
fn clicking_a_title_watches_and_runs() {
    let mut s = session();
    let a = add(&mut s, "text.literal", 20.0, 20.0);
    s.execute("node.set", &json!({"node": a, "port": "text", "value": "the corgi naps"})).unwrap();
    let b = add(&mut s, "text.upper", 400.0, 20.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}})).unwrap();
    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    let conn: Arc<dyn Connection> = Arc::new(InProc(host));
    let app = EditorApp::new(s).with_link(Link::start(conn, "in-process", None).unwrap());
    let mut h = harness(app);
    assert!(h.state().session.connected);
    // the status bar's engine cell names the engine and its state; the whole label (the link,
    // the node count) is its hover (layout B, plumcraft #86)
    h.get_by_label(&format!("Text {} · connected", env!("CARGO_PKG_VERSION")));
    assert!(h.state().engine_label().starts_with("engine: Text"), "{}", h.state().engine_label());
    click_in_node(&mut h, "Upper case");
    assert_eq!(h.state().session.run.watch.first().map(|w| (w.node.as_str(), w.port.as_str())), Some((b.as_str(), "text")));
    let deadline = Instant::now() + Duration::from_secs(10);
    while (h.state().session.run.running || h.state().session.run.watched.first().is_none_or(Option::is_none)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(3);
    for id in [&a, &b] {
        assert_eq!(h.state().session.run.node(id).and_then(|n| n.state), Some(NodeState::Done), "{id}");
    }
    assert!(h.query_all_by_label("THE CORGI NAPS").count() >= 2, "on the node and in the Watched panel");
    h.get_by_label_contains("last run");
}
