//! Helpers shared by the audio engine's test suites: connections in each mode, graphs, runs, and
//! reading audio back out of a reply.
#![allow(dead_code)]

use std::time::{Duration, Instant};

use plumcraft_engine::host::{Host, PollReply};
use plumcraft_engine::{Engine, Node};
use plumcraft_host::http::{HttpHost, HttpOptions};
use plumcraft_host::process::{Manifest, ProcessHost};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::PortValue;
use plumcraft_proto::data::{DataRef, Form};
use plumcraft_proto::event::{Envelope, Event};
use plumcraft_proto::run::{RunAck, RunReply};
use plumcraft_proto::session::EngineState;
use serde_json::{Value, json};

pub type Conn = Box<dyn Connection>;

/// The suites' private store root (blobs are content-addressed, so sharing is safe).
pub fn test_root() -> std::path::PathBuf {
    let root = plumcraft_scratch::root().join("audio-tests");
    let _ = std::fs::create_dir_all(&root);
    root
}

/// A fresh directory under the test root.
pub fn scratch(name: &str) -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let d = test_root().join(format!("{name}-{}", N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// commonkit declines every publish under 8 GB free; the suite must not depend on the disk.
pub fn engine_env() -> Vec<(String, String)> {
    vec![("COMMONKIT_STORE_MIN_FREE_GB".into(), "0".into())]
}

/// Mode 1, with extra test nodes.
pub fn inproc_with(extra: fn() -> Vec<Box<dyn Node>>) -> Conn {
    let h = Host::start(
        Box::new(move || {
            let mut e: Engine = plumcraft_audio::engine().with_store(Some(test_root()));
            for n in extra() {
                e = e.with_node(n);
            }
            Ok(e)
        }),
        None,
    );
    assert!(matches!(h.wait_ready(Duration::from_secs(5)), EngineState::Connected { .. }));
    Box::new(InProc(h))
}

/// Mode 1: the engine compiled in.
pub fn inproc() -> Conn {
    inproc_with(Vec::new)
}

pub fn manifest() -> Manifest {
    Manifest {
        name: "audio".into(),
        command: vec![env!("CARGO_BIN_EXE_plumcraft-audio-engine").into()],
        env: engine_env(),
        hint: "cargo build -p plumcraft-audio".into(),
        store: Some(test_root()),
        cwd: None,
    }
}

/// Mode 2: the engine binary on a Unix socket.
pub fn process() -> Conn {
    let h = ProcessHost::start(manifest()).unwrap();
    let state = h.wait_ready(Duration::from_secs(10));
    assert!(matches!(state, EngineState::Connected { .. }), "{state:?}");
    Box::new(h)
}

/// Mode 3, remote: no same-host proof, sources upload, values download by URL into a fresh CAS.
pub fn spawn_http() -> HttpHost {
    let cas = scratch("cas");
    HttpHost::spawn(&manifest(), HttpOptions { remote: true, cas }).unwrap_or_else(|e| panic!("{e}"))
}

pub fn http() -> Conn {
    Box::new(spawn_http())
}

pub fn call(h: &dyn Connection, method: &str, params: Value) -> Value {
    h.call(method, params).unwrap_or_else(|e| panic!("{method}: {e}"))
}

pub fn graph(nodes: Value, wires: &[(&str, &str, &str, &str)]) -> Value {
    let wires: Vec<Value> = wires.iter().map(|(a, ap, b, bp)| json!({"from": {"node": a, "port": ap}, "to": {"node": b, "port": bp}})).collect();
    json!({"version": 1, "nodes": nodes, "wires": wires})
}

/// A chain of audio nodes wired `audio` → `audio`: `[(id, kind, values)]`.
pub fn chain(steps: &[(&str, &str, Value)]) -> Value {
    let nodes: serde_json::Map<String, Value> = steps.iter().map(|(id, kind, v)| ((*id).to_owned(), json!({"kind": kind, "values": v}))).collect();
    let wires: Vec<(&str, &str, &str, &str)> = steps.windows(2).map(|w| (w[0].0, "audio", w[1].0, "audio")).collect();
    graph(Value::Object(nodes), &wires)
}

/// Send a run (memo off unless `extra` says otherwise).
pub fn run(h: &dyn Connection, id: &str, generation: u64, g: Value, extra: Value) -> RunAck {
    let mut req = json!({"v": 2, "op": "run", "run_id": id, "generation": generation, "graph": {"kind": "inline", "graph": g}, "memo": "off"});
    if let (Some(r), Some(x)) = (req.as_object_mut(), extra.as_object()) {
        r.extend(x.clone());
    }
    serde_json::from_value(call(h, "run", req)).unwrap()
}

/// Poll until the run's reply arrives, returning every envelope seen.
pub fn collect(h: &dyn Connection, run_id: &str, since: u64) -> (Vec<Envelope>, RunReply) {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut since = since;
    let mut all = Vec::new();
    while Instant::now() < deadline {
        let p: PollReply = serde_json::from_value(call(h, "events.poll", json!({"since": since, "wait_ms": 500, "run_id": run_id}))).unwrap();
        since = p.last_seq;
        for e in p.events {
            if let Event::RunReply(r) = &e.event
                && r.run_id == run_id
            {
                let r = (**r).clone();
                all.push(e);
                return (all, r);
            }
            all.push(e);
        }
    }
    panic!("no reply for {run_id}");
}

/// Run a graph to its reply with the given targets.
pub fn go(h: &dyn Connection, id: &str, g: Value, targets: &[&str]) -> RunReply {
    run(h, id, 1, g, json!({"targets": targets}));
    collect(h, id, 0).1
}

pub fn blob(reply: &RunReply, node: &str, port: &str) -> DataRef {
    match reply.nodes.get(node).and_then(|n| n.outputs.get(port)) {
        Some(PortValue::Blob { data }) => data.clone(),
        other => panic!("{node}.{port} is not a blob: {other:?} ({:?})", reply.nodes.get(node).map(|n| (&n.state, &n.error))),
    }
}

pub fn json_out(reply: &RunReply, node: &str, port: &str) -> Value {
    match reply.nodes.get(node).and_then(|n| n.outputs.get(port)) {
        Some(PortValue::Json { value }) => value.clone(),
        other => panic!("{node}.{port} is not JSON: {other:?} ({:?})", reply.nodes.get(node).map(|n| (&n.state, &n.error))),
    }
}

pub fn float_out(reply: &RunReply, node: &str, port: &str) -> f64 {
    json_out(reply, node, port).as_f64().unwrap()
}

/// Decoded audio from a reply.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    pub rate: u32,
    pub channels: Vec<Vec<f32>>,
}

impl Sound {
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    pub fn peak(&self) -> f32 {
        self.channels.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()))
    }
}

/// An audio output, resolved through the connection, checked against its `Form::Audio`.
pub fn sound(h: &dyn Connection, reply: &RunReply, node: &str) -> Sound {
    let r = blob(reply, node, "audio");
    let Form::Audio { sample_rate, channels, frames, dtype } = r.meta.form.clone() else { panic!("{node}: not audio: {:?}", r.meta.form) };
    assert_eq!(dtype, "<f4");
    let b = h.resolve(&r).unwrap_or_else(|| panic!("{node}: the audio doesn't resolve"));
    assert_eq!(b.shape, [frames, u64::from(channels)]);
    assert_eq!(r.handle, Some(plumcraft_proto::Handle::nd1("<f4", &b.shape, &b.bytes).unwrap()), "the handle names the bytes");
    let samples: Vec<f32> = b.bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    let n = usize::from(channels);
    let mut ch = vec![Vec::new(); n];
    for (k, s) in samples.iter().enumerate() {
        ch[k % n].push(*s);
    }
    Sound { rate: sample_rate, channels: ch }
}
