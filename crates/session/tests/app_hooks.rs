//! The hooks an application of the editor sets (`commands::set_hooks`): its commands reach the
//! window's engine connection (`Session::connection`), and its node overlays, wire marks and
//! status segment are read through the session's accessors. A file of its own: the hooks and the
//! registry are one per process, so everything is set once, in `setup`.

use std::sync::{Arc, Once};

use plumcraft_graph::{Graph, NodeId, PortRef};
use plumcraft_host::Connection;
use plumcraft_proto::PortValue;
use plumcraft_proto::data::DataRef;
use plumcraft_proto::error::ProtoError;
use plumcraft_proto::session::EngineState;
use plumcraft_session::commands::{self, CommandSpec, Hooks, Overlay, Tone, WireMark, always};
use plumcraft_session::{Result, Session, SessionError};
use plumcraft_store::Blob;
use serde_json::{Value, json};

/// A connection that answers every call with the method's name.
struct Echo;

impl Connection for Echo {
    fn call(&self, method: &str, _params: Value) -> std::result::Result<Value, ProtoError> {
        Ok(json!({"method": method}))
    }
    fn state(&self) -> EngineState {
        EngineState::Off
    }
    fn resolve(&self, _r: &DataRef) -> Option<Blob> {
        None
    }
}

/// An application's command that calls the window's engine.
fn ask_engine(s: &mut Session, _: &Value) -> Result<Value> {
    let conn = s.connection().ok_or_else(|| SessionError::Params("no engine is linked".into()))?;
    conn.call("app.ping", json!({})).map_err(|e| SessionError::Params(e.message))
}

fn overlays() -> Vec<(NodeId, Overlay)> {
    vec![(NodeId("a".into()), Overlay { text: Some("42 items".into()), value: Some(PortValue::Json { value: json!(42) }) })]
}

fn marks(g: &Graph) -> Vec<(PortRef, WireMark)> {
    g.wires.iter().map(|w| (w.to.clone(), WireMark { label: "map".into(), tone: Tone::Warn, tooltip: None })).collect()
}

fn status() -> Option<String> {
    Some("3 jobs".into())
}

fn setup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let spec = CommandSpec {
            id: "app.askEngine",
            label: "Ask Engine",
            menu: &[],
            shortcut: None,
            params: "{} -> the engine's reply",
            enabled: always,
            run: ask_engine,
            journal: false,
        };
        commands::register(vec![spec]).unwrap();
        commands::set_hooks(Hooks { node_overlays: Some(overlays), wire_marks: Some(marks), status: Some(status), ..Default::default() }).unwrap();
    });
}

#[test]
fn an_application_command_reaches_the_linked_engine() {
    setup();
    let mut s = Session::default();
    assert!(s.connection().is_none());
    assert!(s.execute("app.askEngine", &json!({})).unwrap_err().to_string().contains("no engine"));
    s.set_connection(Some(Arc::new(Echo)));
    assert_eq!(s.execute("app.askEngine", &json!({})).unwrap(), json!({"method": "app.ping"}));
    // a clone of the session holds the same connection; unlinking drops it
    assert!(s.clone().connection().is_some());
    assert!(format!("{s:?}").contains("SharedConnection(linked)"));
    s.set_connection(None);
    assert!(s.connection().is_none());
}

#[test]
fn the_application_hooks_are_read_through_the_session() {
    setup();
    assert!(commands::set_hooks(Hooks::default()).is_err(), "set once, at startup");
    let o = commands::node_overlays();
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].1.text.as_deref(), Some("42 items"));
    let mut g = Graph::new();
    assert!(commands::wire_marks(&g).is_empty());
    g.wires.push(serde_json::from_value(json!({"from": {"node": "a", "port": "text"}, "to": {"node": "b", "port": "text"}})).unwrap());
    let m = commands::wire_marks(&g);
    assert_eq!(m.len(), 1);
    assert_eq!((m[0].0.node.0.as_str(), m[0].1.tone), ("b", Tone::Warn));
    assert_eq!(commands::app_status().as_deref(), Some("3 jobs"));
}
