//! The hooks an application of the editor sets (crate::app_hooks): its commands reach the
//! window's engine, its line shows in a node's footer and its value in the Watched panel, its
//! marks on wires and its segment in the status bar. A file of its own: the hooks and the
//! command registry are one per process, so `setup` sets them once, for every test here.

mod common;

use std::sync::{Arc, Once};
use std::time::Duration;

use common::{add, harness, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::{Graph, NodeId, PortRef};
use plumcraft_proto::PortValue;
use plumcraft_session::commands::{self, CommandSpec, Hooks, Overlay, Tone, WireMark, always};
use plumcraft_session::{Result, Session};
use plumcraft_ui::EditorApp;
use serde_json::{Value, json};

const NODE_TEXT: &str = "app: 42 items so far";
const VALUE_TEXT: &str = "hello from the application";
const MARK: &str = "app: map";
const STATUS: &str = "app: 3 jobs running";

// Every session here starts at n1: the first node gets the overlay, wires into n2.text the mark.
fn overlays() -> Vec<(NodeId, Overlay)> {
    vec![(NodeId("n1".into()), Overlay { text: Some(NODE_TEXT.into()), value: Some(PortValue::Json { value: json!(VALUE_TEXT) }) })]
}

fn marks(g: &Graph) -> Vec<(PortRef, WireMark)> {
    g.wires
        .iter()
        .filter(|w| w.to == PortRef::new("n2", "text"))
        .map(|w| (w.to.clone(), WireMark { label: MARK.into(), tone: Tone::Warn, tooltip: Some("an application's note".into()) }))
        .collect()
}

fn status() -> Option<String> {
    Some(STATUS.into())
}

/// An application's command that asks the window's engine for its catalogue.
fn ask_engine(s: &mut Session, _: &Value) -> Result<Value> {
    let Some(conn) = s.connection() else { return Ok(json!({"linked": false})) };
    let nodes = conn.call("schema", json!({})).ok().and_then(|r| r["nodes"].as_array().map(Vec::len)).unwrap_or(0);
    Ok(json!({"linked": true, "nodes": nodes}))
}

fn setup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let spec = CommandSpec {
            id: "app.askEngine",
            label: "Ask Engine",
            menu: &[],
            shortcut: None,
            params: "{} -> {linked, nodes}",
            enabled: always,
            run: ask_engine,
            journal: false,
        };
        commands::register(vec![spec]).unwrap();
        commands::set_hooks(Hooks { node_overlays: Some(overlays), wire_marks: Some(marks), status: Some(status), ..Default::default() }).unwrap();
    });
}

/// A literal (n1) wired into an upper case (n2).
fn wired() -> Session {
    let mut s = session();
    let a = add(&mut s, "text.literal", 0.0, 0.0);
    let u = add(&mut s, "text.upper", 520.0, 120.0);
    assert_eq!((a.as_str(), u.as_str()), ("n1", "n2"));
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": u, "port": "text"}})).unwrap();
    s
}

#[test]
fn the_overlay_text_shows_in_the_node_footer() {
    setup();
    let h = harness(EditorApp::new(wired()));
    assert!(h.query_by_label(NODE_TEXT).is_some(), "the application's line is on the node");
}

#[test]
fn the_wire_mark_and_the_status_segment_show() {
    setup();
    let h = harness(EditorApp::new(wired()));
    assert!(h.query_by_label(MARK).is_some(), "the mark is on the wire");
    assert!(h.query_by_label(STATUS).is_some(), "the segment is in the status bar");
}

#[test]
fn the_watched_panel_shows_the_application_value() {
    setup();
    let mut h = harness(EditorApp::new(wired()));
    h.state_mut().try_run("run.watch", &json!({"node": "n1"})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label("from the application").is_some(), "the panel's summary");
    assert!(h.query_all_by_label(VALUE_TEXT).count() > 0, "the value, in the text viewer");
    // a node the application has nothing for shows the run's (not run yet)
    h.state_mut().try_run("run.watch", &json!({"node": "n2"})).unwrap();
    h.run_steps(4);
    assert_eq!(h.query_all_by_label_contains("from the application").count(), 0);
}

#[test]
fn an_application_command_reaches_the_linked_engine() {
    use plumcraft_engine::host::Host;
    use plumcraft_host::{Connection, InProc};
    use plumcraft_ui::link::Link;

    setup();
    let mut h = harness(EditorApp::new(wired()));
    assert_eq!(h.state_mut().try_run("app.askEngine", &json!({})).unwrap(), json!({"linked": false}));
    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    let conn: Arc<dyn Connection> = Arc::new(InProc(host));
    let mut h = harness(EditorApp::new(wired()).with_link(Link::start(conn, "in-process", None).unwrap()));
    let r = h.state_mut().try_run("app.askEngine", &json!({})).unwrap();
    assert_eq!(r["linked"], json!(true));
    assert!(r["nodes"].as_u64().unwrap() > 0, "{r}");
    // stopping the engines takes the connection away from the commands too
    h.state_mut().clear_engines();
    h.run_steps(2);
    assert!(h.state().session.connection().is_none());
    assert_eq!(h.state_mut().try_run("app.askEngine", &json!({})).unwrap(), json!({"linked": false}));
}

/// A picture of all three on the canvas, in the default (Paper) theme, to look at.
#[test]
fn a_picture_of_the_hooks() {
    setup();
    let mut app = EditorApp::new(wired());
    let _ = app.try_run("run.watch", &json!({"node": "n1"}));
    let mut h: Harness<'static, EditorApp> =
        Harness::builder().with_size(egui::vec2(1300.0, 600.0)).wgpu().build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
    h.state().setup_context(&h.ctx.clone());
    h.run_steps(4);
    h.state_mut().canvas.request_fit();
    h.run_steps(6);
    let img = h.render().unwrap();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../claude-debug");
    std::fs::create_dir_all(&dir).unwrap();
    img.save(dir.join("app-hooks-paper.png")).unwrap();
}
