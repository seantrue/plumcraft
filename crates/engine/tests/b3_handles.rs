//! BLAKE3 handles (B10, spec 4.3): a run whose request asks for `b3` publishes its arrays and
//! text under `b3:` handles, hashed once, and the store reads them back checked against them;
//! a run that doesn't ask keeps md5. The Rust engine lists `hash.b3` and says `b3` first in
//! `data.algos`.

use plumcraft_engine::data::Plane;
use plumcraft_engine::exec::{self, Memo};
use plumcraft_engine::{Array, CancelToken, Cx, Data, Emit, Engine, Inputs, Node, Outputs};
use plumcraft_proto::PortValue;
use plumcraft_proto::catalogue::{NodeDef, Port};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::StatusRecord;
use plumcraft_proto::handle::{Algo, Handle};
use plumcraft_proto::run::{RunReply, RunRequest};
use plumcraft_proto::session::{EngineInfo, Hello};
use plumcraft_store::Store;
use serde_json::json;

/// 1 MiB of float32 and a long string: both too big to go inline, so both reach the store.
struct Make;

impl Node for Make {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "t.make".into(),
            outputs: vec![
                Port { name: "array".into(), ty: "ARRAY".into(), ..Default::default() },
                Port { name: "text".into(), ty: "STRING".into(), ..Default::default() },
            ],
            ..Default::default()
        }
    }

    fn run(&self, _: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let bytes: Vec<u8> = (0..(1u32 << 20)).map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8).collect();
        let a = Array::new("<f4", vec![512, 512], bytes);
        Ok(Outputs::from([("array".to_owned(), Data::Array(a)), ("text".to_owned(), Data::Json(json!("x".repeat(200_000))))]))
    }
}

#[derive(Default)]
struct Log;

impl Emit for Log {
    fn status(&mut self, _: StatusRecord) {}
}

fn engine(store: &std::path::Path) -> Engine {
    Engine::new(EngineInfo { id: "t".into(), salt: "t".into(), ..Default::default() }).with_store(Some(store.to_path_buf())).with_node(Box::new(Make))
}

fn run(e: &Engine, algo: &str) -> RunReply {
    let req: RunRequest = serde_json::from_value(json!({
        "run_id": format!("r-{algo}"), "generation": 1, "algo": algo, "memo": "off",
        "graph": {"kind": "inline", "graph": {"version": 1, "nodes": {"n": {"kind": "t.make"}}, "wires": []}},
        "targets": ["n"],
    }))
    .unwrap();
    let plane = Plane { store: e.store(), shm: true, urls: false, max_inline: 64 << 10, algo: req.handle_algo() };
    exec::run(e, &req, &mut Log, &CancelToken::default(), "test", &mut Memo::default(), &plane)
}

fn handle(r: &RunReply, port: &str) -> (Handle, plumcraft_proto::data::DataRef) {
    match &r.nodes["n"].outputs[port] {
        PortValue::Blob { data } => (data.handle.clone().unwrap(), data.clone()),
        other => panic!("{port}: not a blob: {other:?}"),
    }
}

#[test]
fn a_b3_run_publishes_b3_handles_the_store_reads_back() {
    let root = plumcraft_scratch::dir("b3-handles");
    let e = engine(&root);
    let store = Store::new(&root);
    for (algo, want) in [("b3", Algo::B3), ("", Algo::Md5), ("sha1", Algo::Md5)] {
        let r = run(&e, algo);
        assert!(r.ok, "{algo}: {:?}", r.nodes["n"].error);
        for port in ["array", "text"] {
            let (h, data) = handle(&r, port);
            assert_eq!(h.algo(), want, "{algo:?} {port}: {h}");
            let blob = store.resolve(&data, true).unwrap_or_else(|| panic!("{algo:?} {port}: not readable"));
            assert!(plumcraft_store::verifies(&h, &blob.dtype, &blob.shape, &blob.bytes), "{algo:?} {port}: the bytes are the handle");
        }
    }
    // the two algorithms name the same array differently, and both are stored
    assert_ne!(handle(&run(&e, "b3"), "array").0, handle(&run(&e, ""), "array").0);
}

#[test]
fn the_engine_lists_hash_b3_and_prefers_b3() {
    let root = plumcraft_scratch::dir("b3-hello");
    let e = engine(&root);
    let hello: Hello = serde_json::from_value(json!({"op": "hello", "caps": ["hash.b3"]})).unwrap();
    let r = e.hello(&hello, "s").unwrap();
    assert!(r.caps.has("hash.b3"));
    assert_eq!(r.data.algos, ["b3", "md5"]);
    let without: Hello = serde_json::from_value(json!({"op": "hello"})).unwrap();
    assert!(!e.hello(&without, "s").unwrap().caps.has("hash.b3"), "both sides must list it");
}
