//! One algorithm per process (#91, #92; spec 4.3): a run publishes its arrays and text under
//! handles of commonkit's current algorithm (`HASH`/`COMMONKIT_HASH`), hashed once, and the store
//! reads them back checked against them. The engine says exactly that algorithm in
//! `data.algos`, and a hello listing anything else is refused (D8). The suite runs once per
//! algorithm.

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

fn run(e: &Engine, id: &str) -> RunReply {
    let req: RunRequest = serde_json::from_value(json!({
        "run_id": id, "generation": 1, "memo": "off",
        "graph": {"kind": "inline", "graph": {"version": 1, "nodes": {"n": {"kind": "t.make"}}, "wires": []}},
        "targets": ["n"],
    }))
    .unwrap();
    let plane = Plane { store: e.store(), shm: true, urls: false, max_inline: 64 << 10 };
    exec::run(e, &req, &mut Log, &CancelToken::default(), "test", &mut Memo::default(), &plane)
}

fn handle(r: &RunReply, port: &str) -> (Handle, plumcraft_proto::data::DataRef) {
    match &r.nodes["n"].outputs[port] {
        PortValue::Blob { data } => (data.handle.clone().unwrap(), data.clone()),
        other => panic!("{port}: not a blob: {other:?}"),
    }
}

#[test]
fn a_run_publishes_current_handles_the_store_reads_back() {
    let root = plumcraft_scratch::dir("one-algo-handles");
    let e = engine(&root);
    let store = Store::new(&root);
    let want = Algo::current();
    // A request's old `algo` field is ignored: nothing a client sends picks the algorithm.
    for id in ["r-1", "r-2"] {
        let r = run(&e, id);
        assert!(r.ok, "{id}: {:?}", r.nodes["n"].error);
        for port in ["array", "text"] {
            let (h, data) = handle(&r, port);
            assert_eq!((h.algo(), h.digest().len()), (want, want.hex_len()), "{port}: {h}");
            let blob = store.resolve(&data, true).unwrap_or_else(|| panic!("{port}: not readable"));
            assert!(plumcraft_store::verifies(&h, &blob.dtype, &blob.shape, &blob.bytes), "{port}: the bytes are the handle");
        }
    }
}

#[test]
fn hello_says_exactly_the_current_algorithm() {
    let root = plumcraft_scratch::dir("one-algo-hello");
    let e = engine(&root);
    let hello: Hello = serde_json::from_value(json!({"op": "hello", "caps": ["hash.b3"]})).unwrap();
    let r = e.hello(&hello, "s").unwrap();
    assert!(!r.caps.has("hash.b3"), "retired: the algorithm isn't negotiated");
    assert_eq!(r.data.algos, [Algo::current().as_str()]);
    assert_eq!(r.data.refusal(), None);
}

/// D8: an engine is refused unless it lists exactly the current algorithm; one from before
/// 0.7.0 (both, or none) is refused too.
#[test]
fn hello_refuses_any_other_algorithm_list() {
    use plumcraft_proto::session::DataInfo;
    let ours = Algo::current().as_str();
    let other = if Algo::current() == Algo::Md5 { "b3" } else { "md5" };
    let refused = |algos: &[&str]| DataInfo { algos: algos.iter().map(|a| (*a).to_owned()).collect(), ..Default::default() }.refusal();
    assert_eq!(refused(&[ours]), None);
    for bad in [&["b3", "md5"][..], &["md5", "b3"], &[other], &[]] {
        let why = refused(bad).unwrap_or_else(|| panic!("{bad:?} accepted"));
        assert!(why.contains(&format!("editor {ours}")) && why.contains("COMMONKIT_HASH"), "{why}");
    }
}
