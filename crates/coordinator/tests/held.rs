//! Values that can't leave their engine (spec 7.6 rule 11): A → B → A, a coordinator run with a
//! core that keeps one value between its two fragments. It comes back as the same value, the hold
//! is released when the run ends, an engine that can't hold says why, and a value that would go to
//! another engine is refused rather than quietly skipped.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use plumcraft_coordinator::Coordinator;
use plumcraft_engine::data::Plane;
use plumcraft_engine::host::Host;
use plumcraft_engine::{CancelToken, Emit, Engine, EngineCore};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::PortValue;
use plumcraft_proto::catalogue::{NodeDef, Port, SaveFormat, SchemaReply};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::NodeState;
use plumcraft_proto::run::{GraphRef, NodeReport, RunReply, RunRequest, RunSnapshot, SaveReply, SaveRequest};
use plumcraft_proto::session::{EngineInfo, EngineState};
use serde_json::{Value, json};

#[derive(Default)]
struct Log {
    scopes_seen: Vec<String>,
    scopes_released: Vec<String>,
    kept: BTreeMap<String, u32>,
    released: Vec<String>,
    /// What each save was given: the thing still held when it came, and the request.
    saved: Vec<(bool, SaveRequest)>,
}

/// `h.make` makes a thing, which has no wire form, and a word; `h.use` reads both.
struct Holder {
    log: Arc<Mutex<Log>>,
    can_hold: bool,
}

impl EngineCore for Holder {
    fn run(&mut self, req: RunRequest, _: &Plane<'_>, _: &mut dyn Emit, _: &CancelToken) -> RunReply {
        let GraphRef::Inline { graph } = &req.graph else { panic!() };
        self.log.lock().unwrap().scopes_seen.push(req.scope.clone());
        let mut reply = RunReply { run_id: req.run_id.clone(), generation: req.generation, ok: true, ..Default::default() };
        for (id, n) in graph["nodes"].as_object().unwrap() {
            let mut r = NodeReport { state: NodeState::Done, ..Default::default() };
            if n["kind"] == "h.make" {
                let wanted = self.can_hold && req.hold.iter().any(|h| h == &format!("{id}.thing"));
                if wanted {
                    *self.log.lock().unwrap().kept.entry("thing-1".into()).or_default() += 1;
                }
                r.outputs.insert("thing".into(), PortValue::Opaque { ty: "X".into(), summary: "a thing".into(), held: wanted.then(|| "thing-1".to_owned()) });
                r.outputs.insert("word".into(), PortValue::Json { value: json!("corgi") });
            } else {
                let thing = req.inputs.get(&format!("{id}.thing"));
                let word = req.inputs.get(&format!("{id}.word"));
                let seen = format!("{thing:?} {word:?}");
                assert!(matches!(thing, Some(PortValue::Opaque { held: Some(h), .. }) if self.log.lock().unwrap().kept.get(h) == Some(&1)), "{seen}");
                r.outputs.insert("seen".into(), PortValue::Json { value: json!(seen) });
            }
            reply.nodes.insert(id.clone(), r);
        }
        reply
    }

    fn release_scope(&mut self, scope: &str) {
        self.log.lock().unwrap().scopes_released.push(scope.to_owned());
    }

    fn save(&mut self, req: SaveRequest, _: &Plane<'_>, _: &CancelToken) -> SaveReply {
        let mut log = self.log.lock().unwrap();
        let held = matches!(req.inputs.get("use.thing"), Some(PortValue::Opaque { held: Some(h), .. }) if log.kept.contains_key(h));
        log.saved.push((held, req));
        SaveReply { ok: true, recipe: true, ..Default::default() }
    }

    fn release(&mut self, ids: &[String]) {
        let mut log = self.log.lock().unwrap();
        for id in ids {
            log.kept.remove(id);
            log.released.push(id.clone());
        }
    }
}

fn port(name: &str, ty: &str) -> Port {
    Port { name: name.into(), ty: ty.into(), ..Default::default() }
}

fn holder(id: &'static str, can_hold: bool, log: Arc<Mutex<Log>>) -> Arc<dyn Connection> {
    let h = Host::start(
        Box::new(move || {
            let make = NodeDef { id: format!("{id}.make"), outputs: vec![port("thing", "X"), port("word", "TEXT")], ..Default::default() };
            let take = NodeDef {
                id: format!("{id}.use"),
                inputs: vec![port("thing", "X"), port("word", "TEXT")],
                outputs: vec![port("seen", "STRING")],
                ..Default::default()
            };
            let save_formats = vec![SaveFormat { id: "f".into(), ..Default::default() }];
            let catalogue = SchemaReply { nodes: vec![make, take], save_formats, ..Default::default() };
            let info = EngineInfo { id: id.into(), name: id.into(), ..Default::default() };
            let log = Arc::clone(&log);
            Ok(Engine::new(info).with_store(None).with_core(catalogue, Box::new(move || Ok(Box::new(Holder { log: Arc::clone(&log), can_hold })))))
        }),
        None,
    );
    assert!(matches!(h.wait_ready(Duration::from_secs(5)), EngineState::Connected { .. }));
    Arc::new(InProc(h))
}

fn text() -> Arc<dyn Connection> {
    let h = Host::start(Box::new(|| Ok(plumcraft_text::engine().with_store(None))), None);
    assert!(matches!(h.wait_ready(Duration::from_secs(5)), EngineState::Connected { .. }));
    Arc::new(InProc(h))
}

/// make → use directly, and make → (the text engine) → use: three fragments.
fn graph(prefix: &str, other: &str) -> Value {
    json!({
        "version": 1,
        "nodes": {
            "make": {"kind": format!("{prefix}.make")},
            "shout": {"kind": "text.upper"},
            "use": {"kind": format!("{other}.use")},
        },
        "wires": [
            {"from": {"node": "make", "port": "thing"}, "to": {"node": "use", "port": "thing"}},
            {"from": {"node": "make", "port": "word"}, "to": {"node": "shout", "port": "text"}},
            {"from": {"node": "shout", "port": "text"}, "to": {"node": "use", "port": "word"}},
        ],
    })
}

fn run(c: &Coordinator, id: &str, g: Value) -> Option<RunReply> {
    let ack = c.call("run", json!({"v": 2, "op": "run", "run_id": id, "generation": 1, "graph": {"kind": "inline", "graph": g}, "targets": ["use"]}));
    let until = Instant::now() + Duration::from_secs(10);
    ack.ok()?;
    while Instant::now() < until {
        let snap: RunSnapshot = serde_json::from_value(c.call("run.get", json!({"run_id": id})).unwrap()).unwrap();
        if snap.reply.is_some() {
            return snap.reply;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("no reply for {id}");
}

fn released(log: &Mutex<Log>, want: usize) {
    let until = Instant::now() + Duration::from_secs(2);
    while log.lock().unwrap().released.len() < want && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_held_value_comes_back_to_its_engine_and_is_released() {
    let log = Arc::new(Mutex::new(Log::default()));
    let c = Coordinator::new().with_engine("h", holder("h", true, Arc::clone(&log))).with_engine("text", text());
    let r = run(&c, "held", graph("h", "h")).unwrap();
    assert!(r.ok, "{:?} {:?}", r.error, r.nodes);
    let seen = format!("{:?}", r.nodes["use"].outputs["seen"]);
    assert!(seen.contains("thing-1") && seen.contains("CORGI"), "{seen}");
    released(&log, 1);
    let log = log.lock().unwrap();
    assert_eq!(log.released, ["thing-1"]);
    assert!(log.kept.is_empty(), "nothing stays held after the run");
}

#[test]
fn an_engine_that_cant_hold_says_why() {
    let c = Coordinator::new().with_engine("h", holder("h", false, Arc::default())).with_engine("text", text());
    let r = run(&c, "no-hold", graph("h", "h")).unwrap();
    assert_eq!(r.nodes["use"].state, NodeState::Skipped);
    let why = r.nodes["use"].error.as_ref().map_or(String::new(), |e: &NodeError| e.message.clone());
    assert!(why.contains("can't leave its engine") && why.contains("a thing"), "{why}");
}

#[test]
fn a_held_value_is_never_asked_for_across_engines() {
    let log = Arc::new(Mutex::new(Log::default()));
    let c = Coordinator::new()
        .with_engine("h", holder("h", true, Arc::clone(&log)))
        .with_engine("j", holder("j", true, Arc::default()))
        .with_engine("text", text());
    let r = run(&c, "across", graph("h", "j")).unwrap();
    let why = r.error.map_or(String::new(), |e| e.message);
    assert!(why.contains("make.thing") && why.contains("from h to j"), "refused at the check, saying which wire: {why}");
    assert!(log.lock().unwrap().kept.is_empty(), "the wire leaves the engine, so nothing was held for it");
}

/// A graph's life crosses the coordinator: each fragment's run names it, and its end reaches every
/// engine, whether or not it takes the op.
#[test]
fn a_graphs_scope_reaches_every_engine() {
    let (one, two) = (Arc::new(Mutex::new(Log::default())), Arc::new(Mutex::new(Log::default())));
    let c = Coordinator::new()
        .with_engine("h", holder("h", true, Arc::clone(&one)))
        .with_engine("j", holder("j", true, Arc::clone(&two)))
        .with_engine("text", text());
    c.call(
        "run",
        json!({"v": 2, "op": "run", "run_id": "scoped", "generation": 1, "scope": "life-1",
        "graph": {"kind": "inline", "graph": graph("h", "h")}, "targets": ["use"]}),
    )
    .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while serde_json::from_value::<RunSnapshot>(c.call("run.get", json!({"run_id": "scoped"})).unwrap()).unwrap().reply.is_none() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(one.lock().unwrap().scopes_seen, ["life-1", "life-1"], "both of h's fragments");
    c.call("graph.release", json!({"scope": "life-1"})).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while one.lock().unwrap().scopes_released.is_empty() || two.lock().unwrap().scopes_released.is_empty() {
        assert!(Instant::now() < until, "the release reaches each");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_save_after_another_engine_gets_its_values_and_the_whole_graph_then_releases() {
    let log = Arc::new(Mutex::new(Log::default()));
    let c = Coordinator::new().with_engine("h", holder("h", true, Arc::clone(&log))).with_engine("text", text());
    let g = graph("h", "h");
    let body = json!({"graph": {"kind": "inline", "graph": g}, "node": "use", "port": "seen", "path": "/x/a.f"});
    let reply = c.call("output.save", body).unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    released(&log, 1);
    let log = log.lock().unwrap();
    let [(held, req)] = log.saved.as_slice() else { panic!("{:?}", log.saved.len()) };
    assert!(held, "the thing was still held when the save came");
    assert!(matches!(req.inputs.get("use.word"), Some(PortValue::Json { value }) if value == "CORGI"), "the other engine's value: {:?}", req.inputs);
    let GraphRef::Inline { graph: share } = &req.graph else { panic!() };
    assert_eq!(share["nodes"].as_object().unwrap().len(), 1, "this engine's share, which runs");
    let mut whole = g.clone();
    whole["engines"] = json!(["h", "text"]);
    assert_eq!(req.recipe_graph.as_ref(), Some(&whole), "and the graph that remakes the file, naming the engines it needs");
    assert_eq!(log.released, ["thing-1"], "released after the save, not before");
    assert!(log.kept.is_empty());
}
