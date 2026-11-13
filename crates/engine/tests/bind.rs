//! Bindings (`RunRequest.inputs`, spec 7.6 rule 10): one engine's outputs, bound into a second
//! engine's run over the same store, arrive as the same values (json, text, table-like json,
//! array with its space, audio, encoded bytes, lists), whether the DataRef holds them inline or
//! in the store; a binding replaces a wire and what fed it isn't run; the memo key is the
//! content, so the same bytes hit whatever their locators; a variadic slot can be bound; keys
//! that name nothing are ignored; missing, handle-less, opaque and over-deep values are node
//! errors, never a crash.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use plumcraft_engine::data::Plane;
use plumcraft_engine::exec::{self, Memo};
use plumcraft_engine::{Array, Audio, Blob, BlobSource, CancelToken, Cx, Data, Emit, Engine, Inputs, List, Node, Outputs};
use plumcraft_proto::catalogue::{Media, NodeDef, Port, Variadic, WireType};
use plumcraft_proto::data::{BlobMeta, DataRef, Form};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::{NodeState, StatusRecord};
use plumcraft_proto::run::{RunReply, RunRequest};
use plumcraft_proto::session::EngineInfo;
use plumcraft_proto::{Handle, PortValue};
use serde_json::{Value, json};

const KINDS: &[&str] = &["json", "text", "array", "audio", "blob", "list"];

/// One value of each kind, from `n`.
struct Make(Arc<AtomicUsize>);

impl Node for Make {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "t.make".into(),
            inputs: vec![Port { name: "n".into(), ty: "INT".into(), default: json!(1), ..Default::default() }],
            outputs: KINDS.iter().map(|k| Port { name: (*k).into(), ty: "ANY".into(), ..Default::default() }).collect(),
            ..Default::default()
        }
    }

    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let n = i.get("n").and_then(Data::json).and_then(Value::as_u64).unwrap_or(0);
        let bytes = format!("<svg n='{n}'/>").repeat(50).into_bytes();
        let meta = BlobMeta { media: "image/svg+xml".into(), size: bytes.len() as u64, form: Form::Vector { width: 1.0, height: 1.0 }, aliases: Vec::new() };
        let blob = Blob { handle: Handle::raw(&bytes), meta, source: BlobSource::Bytes(Arc::from(bytes)), publish: true, thumb: None };
        let mut array = Array::new("<f4", vec![2, 2, 3], (0..12).flat_map(|k| (k as f32 + n as f32).to_le_bytes()).collect::<Vec<u8>>());
        array.space = Some("Lab".into());
        array.names = vec!["L".into(), "a".into(), "b".into()];
        Ok(Outputs::from([
            ("json".to_owned(), Data::Json(json!({"n": n, "xs": [1, 2.5]}))),
            ("text".to_owned(), Data::Json(Value::from(format!("line {n}\n").repeat(40)))),
            ("array".to_owned(), Data::Array(array)),
            ("audio".to_owned(), Data::Audio(Audio { sample_rate: 8000, channels: 2, samples: Arc::from(vec![0.5f32, -0.5, n as f32, 0.0]) })),
            ("blob".to_owned(), Data::Blob(Box::new(blob))),
            ("list".to_owned(), Data::List(List::new(vec![Data::Json(json!(n)), Data::Json(json!("two"))]))),
        ]))
    }
}

/// Passes `x` through, and says how many variadic slots it got.
struct Echo(Arc<AtomicUsize>);

impl Node for Echo {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "t.echo".into(),
            inputs: vec![
                Port { name: "x".into(), ty: "ANY".into(), default: json!("widget"), ..Default::default() },
                Port {
                    name: "c".into(),
                    ty: "ANY".into(),
                    optional: true,
                    variadic: Some(Variadic { template: "c{n}".into(), min: 0, max: 0, ..Default::default() }),
                    ..Default::default()
                },
            ],
            outputs: vec![
                Port { name: "x".into(), ty: "ANY".into(), ..Default::default() },
                Port { name: "slots".into(), ty: "ANY".into(), ..Default::default() },
            ],
            ..Default::default()
        }
    }

    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let slots: Vec<&String> = i.keys().filter(|k| k.starts_with('c')).collect();
        Ok(Outputs::from([("x".to_owned(), i.get("x").cloned().unwrap_or(Data::Json(Value::Null))), ("slots".to_owned(), Data::Json(json!(slots)))]))
    }
}

#[derive(Default)]
struct Log;

impl Emit for Log {
    fn status(&mut self, _: StatusRecord) {}
}

fn root(name: &str) -> PathBuf {
    let d = plumcraft_scratch::root().join(format!("bind-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Rig {
    engine: Engine,
    made: Arc<AtomicUsize>,
    echoed: Arc<AtomicUsize>,
    memo: Memo,
}

fn rig(store: &std::path::Path) -> Rig {
    let (made, echoed) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let engine = Engine::new(EngineInfo { id: "t".into(), salt: "s".into(), ..Default::default() })
        .with_store(Some(store.to_path_buf()))
        .with_wire_type(WireType { id: "ANY".into(), media: Media::Tree, ..Default::default() })
        .with_node(Box::new(Make(made.clone())))
        .with_node(Box::new(Echo(echoed.clone())));
    Rig { engine, made, echoed, memo: Memo::default() }
}

impl Rig {
    /// Run `graph` with `inputs`, targeting `targets`; values up to `max_inline` bytes go inline.
    fn run(&mut self, graph: Value, inputs: Value, targets: &[&str], max_inline: usize) -> RunReply {
        let req: RunRequest = serde_json::from_value(json!({
            "run_id": "r", "generation": 1,
            "graph": {"kind": "inline", "graph": graph},
            "targets": targets, "inputs": inputs,
        }))
        .unwrap();
        let plane = Plane { store: self.engine.store(), shm: true, urls: false, max_inline };
        exec::run(&self.engine, &req, &mut Log, &CancelToken::default(), "test", &mut self.memo, &plane)
    }
}

fn echo_graph() -> Value {
    json!({"version": 1, "nodes": {"e": {"kind": "t.echo"}}, "wires": []})
}

fn made(rig: &mut Rig, n: u64, max_inline: usize) -> RunReply {
    let r = rig.run(json!({"version": 1, "nodes": {"m": {"kind": "t.make", "values": {"n": n}}}, "wires": []}), json!({}), &["m"], max_inline);
    assert!(r.ok, "{r:?}");
    r
}

fn bound(v: &PortValue) -> Value {
    json!({"e.x": v})
}

#[test]
fn every_kind_crosses_unchanged_inline_or_in_the_store() {
    let store = root("kinds");
    for max_inline in [0, 1 << 20] {
        let mut a = rig(&store);
        let from = made(&mut a, 3, max_inline);
        let outs = &from.nodes["m"].outputs;
        let mut b = rig(&store);
        for kind in KINDS {
            let v = &outs[*kind];
            let r = b.run(echo_graph(), bound(v), &["e"], max_inline);
            assert!(r.ok, "{kind} (inline up to {max_inline}): {r:?}");
            assert_eq!(&r.nodes["e"].outputs["x"], v, "{kind} (inline up to {max_inline}) arrives as it left");
        }
    }
    // In the store, the text and the bytes went as blobs, and so did the array and audio.
    let mut a = rig(&store);
    let outs = made(&mut a, 3, 0).nodes["m"].outputs.clone();
    for kind in ["text", "array", "audio", "blob"] {
        assert!(matches!(outs[kind], PortValue::Blob { .. }), "{kind}: {:?}", outs[kind]);
    }
}

#[test]
fn an_array_keeps_its_space_and_names() {
    let store = root("space");
    let mut a = rig(&store);
    let v = made(&mut a, 1, 0).nodes["m"].outputs["array"].clone();
    let PortValue::Blob { data } = &v else { panic!("{v:?}") };
    assert!(matches!(&data.meta.form, Form::Array { space: Some(s), names, .. } if s == "Lab" && names.len() == 3));
    let mut b = rig(&store);
    let r = b.run(echo_graph(), bound(&v), &["e"], 0);
    let PortValue::Blob { data: back } = &r.nodes["e"].outputs["x"] else { panic!() };
    assert_eq!(back.meta.form, data.meta.form);
}

#[test]
fn a_binding_replaces_the_wire_and_its_source_is_not_run() {
    let store = root("wire");
    let mut a = rig(&store);
    let v = PortValue::Json { value: json!("bound") };
    let graph = json!({
        "version": 1,
        "nodes": {"m": {"kind": "t.make"}, "e": {"kind": "t.echo"}},
        "wires": [{"from": {"node": "m", "port": "json"}, "to": {"node": "e", "port": "x"}}],
    });
    let r = a.run(graph, bound(&v), &["e"], 1 << 20);
    assert!(r.ok, "{r:?}");
    assert_eq!(r.nodes["e"].outputs["x"], v);
    assert_eq!(r.order, vec!["e".to_owned()], "only the bound node runs");
    assert_eq!(a.made.load(Ordering::SeqCst), 0);
}

#[test]
fn the_memo_key_is_the_content() {
    let store = root("memo");
    let mut a = rig(&store);
    let in_store = made(&mut a, 5, 0).nodes["m"].outputs["array"].clone();
    let inline = made(&mut a, 5, 1 << 20).nodes["m"].outputs["array"].clone();
    assert_ne!(in_store, inline, "the same array, by different locators");
    let mut b = rig(&store);
    assert_eq!(b.run(echo_graph(), bound(&in_store), &["e"], 0).nodes["e"].state, NodeState::Done);
    assert_eq!(b.run(echo_graph(), bound(&inline), &["e"], 0).nodes["e"].state, NodeState::Cached, "same bytes: a hit");
    let other = made(&mut a, 6, 0).nodes["m"].outputs["array"].clone();
    assert_eq!(b.run(echo_graph(), bound(&other), &["e"], 0).nodes["e"].state, NodeState::Done, "other bytes: computed");
    // A json binding is keyed by its value, and differs from the widget it replaces.
    let j = PortValue::Json { value: json!("widget") };
    assert_eq!(b.run(echo_graph(), json!({}), &["e"], 0).nodes["e"].state, NodeState::Done);
    assert_eq!(b.run(echo_graph(), bound(&j), &["e"], 0).nodes["e"].state, NodeState::Done, "bound, not the widget's key");
    assert_eq!(b.run(echo_graph(), bound(&j), &["e"], 0).nodes["e"].state, NodeState::Cached);
    assert_eq!(b.echoed.load(Ordering::SeqCst), 4);
}

#[test]
fn a_variadic_slot_can_be_bound() {
    let store = root("slot");
    let mut a = rig(&store);
    let v = PortValue::Json { value: json!(1) };
    let r = a.run(echo_graph(), json!({"e.c2": v, "e.c0": v, "e.cx": v}), &["e"], 1 << 20);
    assert!(r.ok, "{r:?}");
    assert_eq!(r.nodes["e"].outputs["slots"], PortValue::Json { value: json!(["c2"]) }, "c0 and cx aren't slots");
}

#[test]
fn keys_naming_nothing_are_ignored() {
    let store = root("unknown");
    let mut a = rig(&store);
    let v = PortValue::Json { value: json!(1) };
    let r = a.run(echo_graph(), json!({"zz.x": v, "e.nope": v, "nodot": v, ".x": v, "e.": v}), &["e"], 1 << 20);
    assert!(r.ok, "{r:?}");
    assert_eq!(r.nodes["e"].outputs["x"], PortValue::Json { value: json!("widget") });
}

#[test]
fn bad_bindings_are_node_errors() {
    let store = root("bad");
    let mut a = rig(&store);
    let meta = BlobMeta {
        media: "application/x-npy".into(),
        size: 4,
        form: Form::Array { dtype: "<f4".into(), shape: vec![1], space: None, names: vec![], table: false },
        aliases: vec![],
    };
    let gone = PortValue::Blob { data: DataRef { handle: Some(Handle::raw(b"never stored")), meta: meta.clone(), at: vec![] } };
    let no_handle = PortValue::Blob { data: DataRef { handle: None, meta, at: vec![] } };
    let opaque = PortValue::Opaque { ty: "STYLE".into(), summary: "s".into(), held: None };
    let mut deep = PortValue::Json { value: json!(0) };
    for _ in 0..20 {
        deep = PortValue::List { items: vec![deep] };
    }
    for (v, kind) in [(gone, NodeError::MISSING_SOURCE), (no_handle, NodeError::CONFIG), (opaque, NodeError::CONFIG), (deep, NodeError::CONFIG)] {
        let r = a.run(echo_graph(), bound(&v), &["e"], 1 << 20);
        let n = &r.nodes["e"];
        assert_eq!(n.state, NodeState::Error, "{v:?}");
        assert_eq!(n.error.as_ref().map(|e| e.kind.as_str()), Some(kind), "{v:?}");
    }
    // A hostile request: the key as a map, a value that isn't a PortValue. The run still runs.
    let req: Result<RunRequest, _> = serde_json::from_value(json!({
        "run_id": "r", "generation": 1, "graph": {"kind": "inline", "graph": echo_graph()},
        "inputs": {"e.x": {"kind": "nonsense"}, "e.c1": 7},
    }));
    if let Ok(req) = req {
        let plane = Plane { store: a.engine.store(), shm: true, urls: false, max_inline: 1 << 20 };
        let r = exec::run(&a.engine, &req, &mut Log, &CancelToken::default(), "t", &mut Memo::default(), &plane);
        assert!(r.ok, "{r:?}");
    }
}

#[test]
fn a_path_outside_the_store_is_read_only_where_the_run_trusts_it() {
    use plumcraft_engine::bind::to_data;
    use plumcraft_proto::data::Locator;
    let root = plumcraft_scratch::dir("bind-store");
    let away = plumcraft_scratch::dir("bind-away");
    let store = plumcraft_store::Store::new(&root);
    let shape = [2u64, 2];
    let bytes: Vec<u8> = [1.0f32, 2.0, 3.0, 4.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let path = away.join("a.npy");
    let mut file = b"\x93NUMPY\x01\x00".to_vec();
    let header = "{'descr': '<f4', 'fortran_order': False, 'shape': (2, 2), }";
    let pad = 64 - (10 + header.len() + 1) % 64;
    let text = format!("{header}{}\n", " ".repeat(pad));
    file.extend((text.len() as u16).to_le_bytes());
    file.extend(text.as_bytes());
    file.extend(&bytes);
    std::fs::write(&path, &file).unwrap();
    let handle = Handle::nd1("<f4", &shape, &bytes).unwrap();
    let meta = BlobMeta {
        size: bytes.len() as u64,
        form: Form::Array { dtype: "<f4".into(), shape: shape.to_vec(), space: None, names: Vec::new(), table: false },
        ..Default::default()
    };
    let value = |h: Handle| PortValue::Blob {
        data: DataRef { handle: Some(h), meta: meta.clone(), at: vec![Locator::Path { path: path.display().to_string(), offset: 0 }] },
    };
    assert!(to_data(Some(&store), &[], "a", &value(handle.clone())).is_err(), "not named: not read");
    let trusted = [away.display().to_string()];
    assert!(matches!(to_data(Some(&store), &trusted, "a", &value(handle)), Ok(plumcraft_engine::Data::Array(_))), "named by the run");
    let other = Handle::nd1("<f4", &shape, &[0u8; 16]).unwrap();
    assert!(to_data(Some(&store), &trusted, "a", &value(other)).is_err(), "the bytes aren't the handle's");
}
