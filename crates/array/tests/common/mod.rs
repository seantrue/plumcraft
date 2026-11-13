//! Running array graphs through the executor, as the engine runs them, and reading results back.
#![allow(dead_code)]

use std::path::PathBuf;

use plumcraft_array::img::Img;
use plumcraft_engine::data::Plane;
use plumcraft_engine::exec::{self, Memo};
use plumcraft_engine::{Array, CancelToken, Emit, Engine};
use plumcraft_proto::PortValue;
use plumcraft_proto::data::{DataRef, Form};
use plumcraft_proto::event::{NodeState, StatusRecord};
use plumcraft_proto::run::{RunReply, RunRequest};
use serde_json::{Value, json};

pub struct Log;

impl Emit for Log {
    fn status(&mut self, _: StatusRecord) {}
}

/// A temporary store root for one test.
pub fn root(name: &str) -> PathBuf {
    let d = plumcraft_scratch::root().join(format!("array-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The array engine over the store at `root`, never persisting.
pub fn engine(root: &std::path::Path) -> Engine {
    plumcraft_array::engine().with_store(Some(root.to_path_buf())).with_persist_ms(None)
}

/// Run `nodes` (with `wires` as (from, port, to, port)) to `targets`, memo off.
pub fn run(e: &Engine, nodes: Value, wires: &[(&str, &str, &str, &str)], targets: &[&str]) -> RunReply {
    let wires: Vec<Value> = wires.iter().map(|(a, p, b, q)| json!({"from": {"node": a, "port": p}, "to": {"node": b, "port": q}})).collect();
    let req: RunRequest = serde_json::from_value(json!({
        "run_id": "r", "generation": 1, "memo": "off",
        "graph": {"kind": "inline", "graph": {"version": 1, "nodes": nodes, "wires": wires}},
        "targets": targets,
    }))
    .unwrap();
    let plane = Plane { store: e.store(), shm: true, urls: false, max_inline: 1 << 20 };
    exec::run(e, &req, &mut Log, &CancelToken::default(), "test", &mut Memo::default(), &plane)
}

/// Assert every target finished, and return the reply.
pub fn ok(r: RunReply) -> RunReply {
    for (id, n) in &r.nodes {
        assert!(matches!(n.state, NodeState::Done | NodeState::Cached), "{id}: {:?} {:?}", n.state, n.error);
    }
    r
}

/// A node's error message (it must have failed).
pub fn error(r: &RunReply, node: &str) -> String {
    let n = &r.nodes[node];
    assert_eq!(n.state, NodeState::Error, "{node} should fail: {n:?}");
    n.error.as_ref().map(|e| e.message.clone()).unwrap_or_default()
}

fn data_ref(r: &RunReply, node: &str, port: &str) -> DataRef {
    match r.nodes[node].outputs.get(port) {
        Some(PortValue::Blob { data }) => data.clone(),
        other => panic!("{node}.{port}: {other:?}"),
    }
}

/// A target's array output, read back as the core type (dtype, shape, space, names, values).
pub fn array(e: &Engine, r: &RunReply, node: &str, port: &str) -> Array {
    let d = data_ref(r, node, port);
    let b = e.store().unwrap().resolve(&d, true).expect("the array resolves");
    let (space, names, table) = match &d.meta.form {
        Form::Array { space, names, table, .. } => (space.clone(), names.clone(), *table),
        other => panic!("{node}.{port}: not an array: {other:?}"),
    };
    Array { space, names, table, ..Array::new(b.dtype, b.shape, b.bytes.into_vec()) }
}

/// A target's array output as an [`Img`].
pub fn img(e: &Engine, r: &RunReply, node: &str, port: &str) -> Img {
    Img::from_array(&array(e, r, node, port)).unwrap()
}

/// A target's blob output's bytes.
pub fn blob(e: &Engine, r: &RunReply, node: &str, port: &str) -> (DataRef, Vec<u8>) {
    let d = data_ref(r, node, port);
    let b = e.store().unwrap().resolve(&d, true).expect("the blob resolves");
    (d, b.bytes.into_vec())
}

/// A target's JSON output.
pub fn json_out(r: &RunReply, node: &str, port: &str) -> Value {
    match r.nodes[node].outputs.get(port) {
        Some(PortValue::Json { value }) => value.clone(),
        other => panic!("{node}.{port}: {other:?}"),
    }
}

/// Values close to each other (absolute `tol`), NaN equal to NaN.
pub fn close(a: &[f32], b: &[f32], tol: f32) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x.is_nan() && y.is_nan()) || (x - y).abs() <= tol)
}
