//! The coordinator, step 2 (docs/research/15-multi-engine.md): with one sub-engine every call,
//! the state and every blob read pass through unchanged; with none every call is `unavailable`;
//! with several the coordinator answers itself (`hello` going to every engine), the state is
//! the first that isn't connected, and a blob comes from whichever engine has it. Runs across
//! real engines: `tests/several.rs`. Sub-engines are added, replaced in
//! place and removed. The full interface through a coordinator is `crates/text/tests/conformance.rs`
//! (`mode1_coordinated`).

use std::sync::{Arc, Mutex};

use plumcraft_coordinator::Coordinator;
use plumcraft_host::Connection;
use plumcraft_proto::data::{BlobMeta, DataRef};
use plumcraft_proto::error::{ErrorCode, ProtoError};
use plumcraft_proto::session::EngineState;
use plumcraft_proto::{Caps, Handle};
use plumcraft_store::Blob;
use serde_json::{Value, json};

/// An engine that answers every call with its name and the call, and has one blob.
struct Fake {
    name: &'static str,
    state: EngineState,
    blob: Option<Handle>,
    calls: Mutex<Vec<(String, Value)>>,
}

impl Fake {
    fn new(name: &'static str) -> Arc<Fake> {
        Arc::new(Fake { name, state: connected(), blob: None, calls: Mutex::new(Vec::new()) })
    }
}

fn connected() -> EngineState {
    EngineState::Connected { version: "1".into(), nodes: 3, caps: Caps::default() }
}

impl Connection for Fake {
    fn call(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        self.calls.lock().unwrap().push((method.to_owned(), params.clone()));
        if method == "boom" {
            return Err(ProtoError::new(ErrorCode::NotFound, "boom"));
        }
        Ok(json!({"engine": self.name, "method": method, "params": params}))
    }

    fn state(&self) -> EngineState {
        self.state.clone()
    }

    fn resolve(&self, r: &DataRef) -> Option<Blob> {
        (r.handle.is_some() && r.handle == self.blob).then(|| Blob { dtype: "|u1".into(), shape: vec![1], bytes: (self.name.as_bytes().to_vec()).into() })
    }
}

fn data_ref(h: &Handle) -> DataRef {
    DataRef { handle: Some(h.clone()), meta: BlobMeta::default(), at: Vec::new() }
}

#[test]
fn one_engine_is_a_pass_through() {
    let a = Fake::new("a");
    let c = Coordinator::new().with_engine("a", a.clone());
    let params = json!({"run_id": "r", "generation": 7, "graph": {"kind": "inline", "graph": {}}});
    assert_eq!(c.call("run", params.clone()).unwrap(), json!({"engine": "a", "method": "run", "params": params}));
    let e = c.call("boom", json!({})).unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (ErrorCode::NotFound, "boom"), "errors pass through too");
    assert_eq!(a.calls.lock().unwrap().len(), 2);
    assert_eq!(c.state(), connected());
}

#[test]
fn no_engine_is_unavailable_and_off() {
    let c = Coordinator::new();
    assert_eq!(c.call("schema", json!({})).unwrap_err().code, ErrorCode::Unavailable);
    assert_eq!(c.state(), EngineState::Off);
    assert!(c.resolve(&data_ref(&Handle::raw(b"x"))).is_none());
}

#[test]
fn several_engines_make_the_coordinator_the_engine() {
    let (a, b) = (Fake::new("a"), Fake::new("b"));
    let c = Coordinator::new().with_engine("a", a.clone()).with_engine("b", b.clone());
    assert_eq!(c.call("bogus.op", json!({})).unwrap_err().code, ErrorCode::UnknownOp, "answered by the coordinator");
    assert_eq!(c.call("run", json!({"graph": 7})).unwrap_err().code, ErrorCode::BadParams);
    assert_eq!(c.call("ping", json!({})).unwrap()["pong"], true);
    assert!(a.calls.lock().unwrap().is_empty() && b.calls.lock().unwrap().is_empty(), "nothing is sent");
    // `hello` goes to every engine.
    c.call("hello", json!({"v": 2})).unwrap();
    for f in [&a, &b] {
        assert!(f.calls.lock().unwrap().iter().any(|(m, _)| m == "hello"), "{}", f.name);
    }
    // The state: connected, counting the merged catalogue's nodes (`hello` fetched it; the
    // fakes' catalogues are empty) before the engines' own counts (3 each); else the first
    // that isn't connected.
    assert!(matches!(c.state(), EngineState::Connected { nodes: 0, .. }), "{:?}", c.state());
    let fresh = Coordinator::new().with_engine("a", Fake::new("a")).with_engine("b", Fake::new("b"));
    assert!(matches!(fresh.state(), EngineState::Connected { nodes: 6, .. }), "{:?}", fresh.state());
    let down = EngineState::EngineDown { reason: "gone".into(), queues: None, stderr_tail: None, hint: String::new() };
    c.add("b", Arc::new(Fake { name: "b", state: down.clone(), blob: None, calls: Mutex::new(Vec::new()) }));
    assert_eq!(c.state(), down);
    assert_eq!(c.states().iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
}

#[test]
fn a_blob_comes_from_whichever_engine_has_it() {
    let h = Handle::raw(b"b's");
    let b = Arc::new(Fake { name: "b", state: connected(), blob: Some(h.clone()), calls: Mutex::new(Vec::new()) });
    let c = Coordinator::new().with_engine("a", Fake::new("a")).with_engine("b", b);
    assert_eq!(c.resolve(&data_ref(&h)).map(|b| b.bytes.into_vec()), Some(b"b".to_vec()));
    assert!(c.resolve(&data_ref(&Handle::raw(b"nobody's"))).is_none());
}

#[test]
fn engines_are_added_replaced_in_place_and_removed() {
    let c = Coordinator::new().with_engine("a", Fake::new("a")).with_engine("b", Fake::new("b"));
    c.add("a", Fake::new("a2"));
    assert_eq!(c.engines().iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["a", "b"], "a reconnect keeps its place");
    assert!(c.remove("nope").is_none());
    assert_eq!(c.remove("b").map(|e| e.id), Some("b".to_owned()));
    // One left, the replacement: a pass-through to it.
    assert_eq!(c.call("ping", json!({})).unwrap()["engine"], "a2");
}

/// The pass-through's cost per call (`cargo test -p plumcraft-coordinator -- --ignored
/// --nocapture`): the same calls straight to a fake engine and through a coordinator.
#[test]
#[ignore = "a measurement"]
fn pass_through_cost() {
    let a = Fake::new("a");
    let c = Coordinator::new().with_engine("a", a.clone());
    let n = 200_000;
    let time = |conn: &dyn Connection| {
        let t = std::time::Instant::now();
        for _ in 0..n {
            let _ = conn.call("ping", Value::Null);
        }
        t.elapsed().as_secs_f64() * 1e9 / f64::from(n)
    };
    let (direct, through) = (time(a.as_ref()), time(&c));
    println!("direct {direct:.0} ns/call, through the coordinator {through:.0} ns/call, +{:.0} ns", through - direct);
}

/// An engine with a catalogue, for `output.save`: `formats` says whether it saves.
struct Saver {
    id: &'static str,
    formats: bool,
    calls: Mutex<Vec<Value>>,
}

impl Saver {
    fn new(id: &'static str, formats: bool) -> Arc<Saver> {
        Arc::new(Saver { id, formats, calls: Mutex::new(Vec::new()) })
    }
}

impl Connection for Saver {
    fn call(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        match method {
            "schema" => {
                let saves = if self.formats { json!([{"id": "f", "label": "F", "extensions": ["f"]}]) } else { json!([]) };
                let (id, array) = (self.id, json!({"id": "ARRAY", "media": "ndarray"}));
                Ok(json!({"engine": {"id": id}, "wire_types": [array], "save_formats": saves, "nodes": [
                    {"id": format!("{id}.src"), "outputs": [{"name": "out", "type": "ARRAY"}]},
                    {"id": format!("{id}.sink"), "inputs": [{"name": "in", "type": "ARRAY", "socket": "socket"}], "outputs": [{"name": "out", "type": "ARRAY"}]}]}))
            }
            "output.save" => {
                self.calls.lock().unwrap().push(params);
                Ok(json!({"ok": true, "paths": ["/x/a.f"], "recipe": true}))
            }
            other => Err(ProtoError::new(ErrorCode::UnknownOp, other)),
        }
    }

    fn state(&self) -> EngineState {
        connected()
    }

    fn resolve(&self, _: &DataRef) -> Option<Blob> {
        None
    }
}

fn save_of(node: &str, nodes: Value, wires: Value) -> Value {
    json!({"graph": {"kind": "inline", "graph": {"version": 1, "nodes": nodes, "wires": wires}}, "node": node, "port": "out", "path": "/x/a.f"})
}

#[test]
fn a_save_goes_to_the_engine_that_computes_the_node_with_its_share_of_the_graph() {
    let (a, b) = (Saver::new("a", true), Saver::new("b", true));
    let c = Coordinator::new().with_engine("a", a.clone()).with_engine("b", b.clone());
    let nodes = json!({"one": {"kind": "a.src"}, "two": {"kind": "a.sink"}, "other": {"kind": "b.src"}});
    let wires = json!([{"from": {"node": "one", "port": "out"}, "to": {"node": "two", "port": "in"}}]);
    let reply = c.call("output.save", save_of("two", nodes, wires)).unwrap();
    assert_eq!((reply["ok"].as_bool(), reply["recipe"].as_bool()), (Some(true), Some(true)));
    assert!(b.calls.lock().unwrap().is_empty());
    let sent = a.calls.lock().unwrap().pop().unwrap();
    let names: Vec<&String> = sent["graph"]["graph"]["nodes"].as_object().unwrap().keys().collect();
    assert_eq!(names, ["one", "two"], "what the node needs, and nothing of the other engine's");
    assert_eq!((sent["node"].as_str(), sent["path"].as_str()), (Some("two"), Some("/x/a.f")));
}

#[test]
fn a_save_stops_when_the_engine_before_it_fails() {
    // These engines answer no run, so the fragment `end` needs never comes.
    let (a, b) = (Saver::new("a", true), Saver::new("b", true));
    let c = Coordinator::new().with_engine("a", a.clone()).with_engine("b", b.clone());
    let nodes = json!({"src": {"kind": "a.src"}, "end": {"kind": "b.sink"}});
    let wires = json!([{"from": {"node": "src", "port": "out"}, "to": {"node": "end", "port": "in"}}]);
    let reply = c.call("output.save", save_of("end", nodes, wires)).unwrap();
    assert_eq!(reply["ok"], false);
    assert!(b.calls.lock().unwrap().is_empty(), "nothing saved");
}

#[test]
fn a_save_on_an_engine_with_no_formats_or_a_node_that_isnt_there_is_refused() {
    let (a, b) = (Saver::new("a", true), Saver::new("b", false));
    let c = Coordinator::new().with_engine("a", a.clone()).with_engine("b", b);
    let reply = c.call("output.save", save_of("n", json!({"n": {"kind": "b.src"}}), json!([]))).unwrap();
    assert!(reply["error"]["message"].as_str().unwrap().contains("doesn't save"), "{reply}");
    let reply = c.call("output.save", save_of("gone", json!({"n": {"kind": "a.src"}}), json!([]))).unwrap();
    assert!(reply["error"]["message"].as_str().unwrap().contains("isn't in the graph"), "{reply}");
    let e = c.call("output.save", json!({"index": "two"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadParams, "hostile params");
    assert!(a.calls.lock().unwrap().is_empty());
}
