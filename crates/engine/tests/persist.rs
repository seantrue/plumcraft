//! Persisted memo records (spec 14.5): a slow node's result survives the engine (a fresh memo,
//! the same store) as `cached`; a fast one, a `cache: never` one, one too big to be worth
//! writing, and one whose time was setup are not persisted; `cache: always` is; a reaped value or
//! a hostile record is a miss that recomputes, never an error.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use plumcraft_engine::data::Plane;
use plumcraft_engine::exec::{self, Memo};
use plumcraft_engine::{Array, Audio, Blob, BlobSource, CancelToken, Cx, Data, Emit, Engine, Inputs, List, Node, Outputs};
use plumcraft_proto::Handle;
use plumcraft_proto::catalogue::{Media, MemoScope, MemoSpec, NodeDef, Port, WireType};
use plumcraft_proto::data::{BlobMeta, Form};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::{NodeState, StatusRecord};
use plumcraft_proto::run::{RunReply, RunRequest};
use plumcraft_proto::session::EngineInfo;
use plumcraft_store::Store;
use serde_json::{Value, json};

/// Sleeps `ms` (of which `setup` counts as setup), then makes one of every kind of value from
/// `n`, `big` bytes of array; counts its runs.
struct Slow {
    id: &'static str,
    scope: MemoScope,
    runs: Arc<AtomicUsize>,
}

impl Node for Slow {
    fn def(&self) -> NodeDef {
        let int = |n: &str, d: i64| Port { name: n.into(), ty: "INT".into(), default: json!(d), ..Default::default() };
        NodeDef {
            id: self.id.into(),
            inputs: vec![int("n", 1), int("ms", 0), int("setup", 0), int("big", 0)],
            outputs: ["json", "array", "blob", "audio", "list"]
                .iter()
                .map(|o| Port { name: (*o).into(), ty: "ANYTHING".into(), ..Default::default() })
                .collect(),
            memo: MemoSpec { scope: self.scope, ..Default::default() },
            ..Default::default()
        }
    }

    fn run(&self, i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let get = |k: &str| i.get(k).and_then(Data::json).and_then(Value::as_u64).unwrap_or(0);
        std::thread::sleep(Duration::from_millis(get("ms")));
        cx.setup_took(Duration::from_millis(get("setup")));
        let n = get("n");
        let text = format!("{{\"n\": {n}}} ").repeat(20_000);
        let thumb = Array::new("|u1", vec![2, 2, 4], vec![n as u8; 16]);
        let meta = BlobMeta { media: "application/json".into(), size: text.len() as u64, form: Form::Doc { app: "t".into() }, aliases: Vec::new() };
        let blob =
            Blob { handle: Handle::md5_raw(text.as_bytes()), meta, source: BlobSource::Bytes(Arc::from(text.into_bytes())), publish: true, thumb: Some(thumb) };
        let big = vec![(n % 251) as u8; get("big") as usize];
        Ok(Outputs::from([
            ("json".to_owned(), Data::Json(json!({"n": n, "list": [1, 2.5, "three"]}))),
            ("array".to_owned(), Data::Array(Array::new("|u1", vec![big.len() as u64], big))),
            ("blob".to_owned(), Data::Blob(Box::new(blob))),
            ("audio".to_owned(), Data::Audio(Audio { sample_rate: 48_000, channels: 2, samples: Arc::from(vec![0.5f32, -0.25, n as f32, 1.0]) })),
            (
                "list".to_owned(),
                Data::List(List {
                    items: vec![Data::Json(json!(n)), Data::Json(json!(n + 1))],
                    axes: vec![("t.axis".into(), 2)],
                    labels: vec!["a".into(), "b".into()],
                }),
            ),
        ]))
    }
}

#[derive(Default)]
struct Log;

impl Emit for Log {
    fn status(&mut self, _: StatusRecord) {}
}

fn root(name: &str) -> PathBuf {
    let d = plumcraft_scratch::root().join(format!("persist-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A fresh engine (as after a restart) over the store at `store`, persisting at 40 ms.
fn engine(store: &std::path::Path, salt: &str) -> (Engine, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let info = EngineInfo { id: "t".into(), salt: salt.into(), ..Default::default() };
    let e = Engine::new(info)
        .with_store(Some(store.to_path_buf()))
        .with_persist_ms(Some(40.0))
        .with_wire_type(WireType { id: "ANYTHING".into(), media: Media::Tree, ..Default::default() })
        .with_node(Box::new(Slow { id: "t.slow", scope: MemoScope::Machine, runs: runs.clone() }))
        .with_node(Box::new(Slow { id: "t.session", scope: MemoScope::Session, runs: runs.clone() }));
    (e, runs)
}

fn run(e: &Engine, node: Value, memo: &str) -> RunReply {
    let req: RunRequest = serde_json::from_value(json!({
        "run_id": "r", "generation": 1, "memo": memo,
        "graph": {"kind": "inline", "graph": {"version": 1, "nodes": {"n": node}, "wires": []}},
        "targets": ["n"],
    }))
    .unwrap();
    let plane = Plane { store: e.store(), shm: true, urls: false, max_inline: 1 << 20, algo: plumcraft_proto::handle::Algo::Md5 };
    exec::run(e, &req, &mut Log, &CancelToken::default(), "test", &mut Memo::default(), &plane)
}

/// Run `node` in one engine, then in a fresh one over the same store: (its state there, how many
/// times it ran in all).
fn twice(store: &std::path::Path, node: Value) -> (NodeState, usize) {
    let (a, runs_a) = engine(store, "s");
    assert_eq!(run(&a, node.clone(), "use").nodes["n"].state, NodeState::Done);
    let (b, runs_b) = engine(store, "s");
    let r = run(&b, node, "use");
    (r.nodes["n"].state, runs_a.load(Ordering::SeqCst) + runs_b.load(Ordering::SeqCst))
}

#[test]
fn a_slow_result_survives_a_restart_whole() {
    let store = root("slow");
    let node = json!({"kind": "t.slow", "values": {"n": 7, "ms": 60, "big": 1000}});
    let (a, _) = engine(&store, "s");
    let first = run(&a, node.clone(), "use");
    let (b, runs) = engine(&store, "s");
    let again = run(&b, node.clone(), "use");
    assert_eq!(again.nodes["n"].state, NodeState::Cached, "{again:?}");
    assert_eq!(runs.load(Ordering::SeqCst), 0, "not run");
    assert_eq!(again.nodes["n"].outputs, first.nodes["n"].outputs, "the same values, by handle");
    let (was, is) = (first.nodes["n"].memo.as_ref().unwrap(), again.nodes["n"].memo.as_ref().unwrap());
    assert_eq!(was.plan_key, is.plan_key);
    assert_eq!(is.source.as_deref(), Some("store"));
    assert!(is.computed_ms >= Some(60.0), "what it took then: {is:?}");
    let (kept, read) = (first.stats.as_ref().unwrap(), again.stats.as_ref().unwrap());
    assert_eq!((kept.computed, kept.kept), (1, 1));
    assert_eq!((read.computed, read.from_store), (0, 1));
    assert!(read.saved_ms >= 60.0);
    assert!(read.store.get("shm_hit").copied().unwrap_or(0) > 0, "commonkit's counters come too: {read:?}");
    let tree = read.tree.as_ref().unwrap();
    assert!(tree.free_bytes.is_some_and(|f| f > 0) && tree.min_free_bytes > 0, "the disk, even with no walk yet: {tree:?}");
    // A different salt (new code) is a different key: computed.
    let (c, runs) = engine(&store, "new code");
    assert_eq!(run(&c, node.clone(), "use").nodes["n"].state, NodeState::Done);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    // `refresh` recomputes a target; `off` neither reads nor writes.
    let (d, runs) = engine(&store, "s");
    assert_eq!(run(&d, node.clone(), "refresh").nodes["n"].state, NodeState::Done);
    assert_eq!(run(&d, node, "off").nodes["n"].state, NodeState::Done);
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[test]
fn fast_never_session_too_big_and_setup_bound_results_are_not_persisted() {
    let store = root("not");
    assert_eq!(twice(&store, json!({"kind": "t.slow", "values": {"n": 1, "ms": 0}})), (NodeState::Done, 2), "fast");
    assert_eq!(twice(&store, json!({"kind": "t.slow", "values": {"n": 2, "ms": 60}, "cache": "never"})), (NodeState::Done, 2), "never");
    assert_eq!(twice(&store, json!({"kind": "t.session", "values": {"n": 3, "ms": 60}})), (NodeState::Done, 2), "session scope");
    // 32 MB made in 60 ms: writing and reading it back (~320 ms) isn't worth it. (8 MB, ~80 ms,
    // left too little margin: on a Windows VM the 60 ms node measured over 80 ms in debug builds,
    // from sleep overshoot and the allocation, and was persisted two runs in three.)
    assert_eq!(twice(&store, json!({"kind": "t.slow", "values": {"n": 4, "ms": 60, "big": 32_000_000}})), (NodeState::Done, 2), "too big");
    // 60 ms, all of it setup (a process start): the work itself was trivial.
    assert_eq!(twice(&store, json!({"kind": "t.slow", "values": {"n": 5, "ms": 60, "setup": 60}})), (NodeState::Done, 2), "setup");
    // `always` persists a fast one, and a junk `cache` value is `auto`.
    assert_eq!(twice(&store, json!({"kind": "t.slow", "values": {"n": 6, "ms": 0}, "cache": "always"})), (NodeState::Cached, 1), "always");
    assert_eq!(twice(&store, json!({"kind": "t.slow", "values": {"n": 8, "ms": 0}, "cache": ["junk"]})), (NodeState::Done, 2), "junk is auto");
}

#[test]
fn a_reaped_value_or_a_hostile_record_is_a_miss_that_recomputes() {
    let store = root("hostile");
    let node = json!({"kind": "t.slow", "values": {"n": 9, "ms": 60}});
    let (a, _) = engine(&store, "s");
    let first = run(&a, node.clone(), "use");
    let key = first.nodes["n"].memo.as_ref().unwrap().plan_key.clone();
    let st = Store::new(&store);
    assert!(st.read_record(&key).is_some(), "persisted");

    // The blob it names is reaped: a miss, recomputed, and persisted again.
    let blob = match &first.nodes["n"].outputs["blob"] {
        plumcraft_proto::PortValue::Blob { data } => data.handle.clone().unwrap(),
        other => panic!("{other:?}"),
    };
    std::fs::remove_file(st.stored_path(&blob).unwrap()).unwrap();
    let (b, runs) = engine(&store, "s");
    assert_eq!(run(&b, node.clone(), "use").nodes["n"].state, NodeState::Done);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    let (c, _) = engine(&store, "s");
    assert_eq!(run(&c, node.clone(), "use").nodes["n"].state, NodeState::Cached);

    // Records any process could have written: each a miss, never a panic or wrong values.
    for junk in [
        b"not json".to_vec(),
        serde_json::to_vec(&json!({"v": 1, "plan_key": "memo:md5:00000000000000000000000000000000", "outputs": {}})).unwrap(),
        serde_json::to_vec(&json!({"v": 2, "plan_key": key, "outputs": {}})).unwrap(),
        serde_json::to_vec(&json!({"v": 1, "plan_key": key, "outputs": {"json": {"what": 1}}})).unwrap(),
        serde_json::to_vec(&json!({"v": 1, "plan_key": key, "outputs": {"json": {"json": "only one of five outputs"}}})).unwrap(),
        serde_json::to_vec(&json!({"v": 1, "plan_key": key, "outputs": {"array": {"array": {"handle": "md5:nd1:00000000000000000000000000000000", "dtype": "|u1", "shape": [1]}}}})).unwrap(),
        serde_json::to_vec(&json!({"v": 1, "plan_key": key, "outputs": {"l": (0..40).fold(json!({"json": 1}), |v, _| json!({"list": {"items": [v]}}))}})).unwrap(),
    ] {
        assert!(st.put_record(&key, &junk));
        let (d, runs) = engine(&store, "s");
        let r = run(&d, node.clone(), "use");
        assert_eq!((r.nodes["n"].state, runs.load(Ordering::SeqCst)), (NodeState::Done, 1), "{}", String::from_utf8_lossy(&junk));
    }
}
