//! The interface conformance cases (plumcraft `docs/interface.md` section 19) that the thin text
//! engine can show. Every case runs in mode 1 (compiled in), mode 1 through a coordinator (a
//! pass-through with one engine), mode 2 and mode 3 with the Rust engine binary, and modes 2 and
//! 3 with the same engine written in Python (`python/`). All are driven only
//! through [`Connection::call`], so every request and reply crosses JSON. The cases never know
//! which engine or mode they run in. The cases any engine can show are `plumcraft_conform`'s, run
//! here with this engine's graphs and what only it can add; `plumcraft conform` runs them against
//! any engine.

use std::time::{Duration, Instant};

use plumcraft_conform::{Fixture, Graphs, graph, reduce};
use plumcraft_engine::host::Host;
use plumcraft_host::http::{HttpHost, HttpOptions};
use plumcraft_host::process::{Manifest, ProcessHost};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::catalogue::Media;
use plumcraft_proto::data::{DataRef, Locator};
use plumcraft_proto::display::Rendition;
use plumcraft_proto::event::{Envelope, Event, NodeState};
use plumcraft_proto::run::{RunAck, RunReply};
use plumcraft_proto::session::EngineState;
use plumcraft_proto::{Handle, PortValue};
use serde_json::{Value, json};

type Conn = Box<dyn Connection>;

/// The conformance suite's private commonkit root, shared by every case in this process (blobs
/// are content-addressed, so sharing is safe) and kept out of the machine's shared store.
fn test_root() -> std::path::PathBuf {
    let root = plumcraft_scratch::root().join("conformance");
    let _ = std::fs::create_dir_all(&root);
    root
}

/// commonkit declines every publish under 8 GB free; the suite must not depend on the disk.
fn engine_env() -> Vec<(String, String)> {
    vec![("COMMONKIT_STORE_MIN_FREE_GB".into(), "0".into())]
}

/// Mode 1: the engine compiled in.
fn inproc() -> Conn {
    let h = Host::start(Box::new(|| Ok(plumcraft_text::engine().with_store(Some(test_root())))), None);
    assert!(matches!(h.wait_ready(Duration::from_secs(5)), EngineState::Connected { .. }));
    Box::new(InProc(h))
}

/// Mode 1 through a coordinator with that one engine: a pass-through, so every case must give
/// exactly what the engine gives (docs/research/15-multi-engine.md, step 2).
fn coordinated() -> Conn {
    Box::new(plumcraft_coordinator::Coordinator::new().with_engine("text", std::sync::Arc::from(inproc())))
}

/// Mode 2: the engine binary as a child process on a Unix socket.
fn process() -> Conn {
    Box::new(spawn())
}

/// Mode 2 with the Python engine.
fn python() -> Conn {
    Box::new(spawn_python())
}

/// Mode 3: the Rust engine over HTTP, as a UI on another machine would see it (no same-host
/// proof: sources upload, values download by URL into the UI's own CAS).
fn http() -> Conn {
    Box::new(spawn_http(rust_manifest()))
}

fn rust_manifest() -> Manifest {
    Manifest {
        name: "text".into(),
        command: vec![env!("CARGO_BIN_EXE_plumcraft-text-engine").into()],
        env: engine_env(),
        hint: "cargo build -p plumcraft-text".into(),
        store: Some(test_root()),
        cwd: None,
    }
}

/// A fresh CAS per connection, so a download is observable.
fn spawn_http(m: Manifest) -> HttpHost {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let cas = test_root().join(format!("cas-{}", N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    HttpHost::spawn(&m, HttpOptions { remote: true, cas }).unwrap_or_else(|e| panic!("{e}"))
}

fn spawn() -> ProcessHost {
    start(rust_manifest())
}

fn python_manifest() -> Manifest {
    let (command, mut env) = plumcraft_text::python_command();
    env.extend(engine_env());
    Manifest {
        name: "text (Python)".into(),
        command,
        env,
        hint: "needs uv (cd python && uv sync), or set PLUMCRAFT_UV".into(),
        store: Some(test_root()),
        cwd: None,
    }
}

fn spawn_python() -> ProcessHost {
    start(python_manifest())
}

/// Mode 3 with the Python engine, remote as [`http`].
fn http_python() -> Conn {
    Box::new(spawn_http(python_manifest()))
}

fn start(m: Manifest) -> ProcessHost {
    let h = ProcessHost::start(m).unwrap();
    let state = h.wait_ready(Duration::from_secs(10));
    assert!(matches!(state, EngineState::Connected { .. }), "{state:?}");
    h
}

fn call(h: &dyn Connection, method: &str, params: Value) -> Value {
    h.call(method, params).unwrap_or_else(|e| panic!("{method}: {e}"))
}

/// literal → upper → words, and upper watched.
fn chain() -> Value {
    graph(
        json!({
            "a": {"kind": "text.literal", "values": {"text": "the corgi on the floral carpet"}, "pos": [0, 0]},
            "b": {"kind": "text.upper", "values": {}},
            "c": {"kind": "text.words", "values": {}}
        }),
        &[("a", "text", "b", "text"), ("b", "text", "c", "text")],
    )
}

/// Send a run. Conformance runs with the memo off (spec 14.5) unless a case asks for it.
fn run(h: &dyn Connection, id: &str, generation: u64, g: Value, extra: Value) -> RunAck {
    plumcraft_conform::run(h, id, generation, g, extra).unwrap()
}

/// Poll until the run's reply arrives, returning every envelope seen.
fn collect(h: &dyn Connection, run_id: &str, since: u64) -> (Vec<Envelope>, RunReply) {
    plumcraft_conform::collect(h, run_id, since).unwrap()
}

/// The generic cases' graphs, for this engine: the chain, and a slow node with one after it.
fn graphs() -> Graphs {
    Graphs { chain: chain(), watch_port: "text".into(), slow: Box::new(slow_graph) }
}

/// The generic cases (`plumcraft_conform`) on the engine `mk` connects.
fn fixture(mk: fn() -> Conn) -> Fixture {
    Fixture { connect: Box::new(move || Ok(mk())), graphs: graphs() }
}

fn hello_and_schema(mk: fn() -> Conn) {
    let schema = plumcraft_conform::hello_and_schema(&fixture(mk)).unwrap();
    let nodes: Vec<&str> = schema["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap()).collect();
    assert!(nodes.contains(&"text.upper") && nodes.contains(&"text.slow"));
}

/// Spec 19 item 4, and this chain's own result: six words, and an excerpt of the watched text.
fn record_invariants(mk: fn() -> Conn) {
    let reply = plumcraft_conform::record_invariants(&fixture(mk)).unwrap();
    assert_eq!(reply.nodes["c"].outputs["count"], plumcraft_proto::PortValue::Json { value: json!(6) });
    assert_eq!(reply.watch[0].renditions.len(), 1, "the watched text has an excerpt");
}

/// Errors as data: the failing node is `error`, downstream `skipped`, the reply not ok.
fn errors_and_skips(mk: fn() -> Conn) {
    let h = mk();
    let g = graph(
        json!({"a": {"kind": "text.literal", "values": {"text": "x"}}, "f": {"kind": "text.fail"}, "u": {"kind": "text.upper"}, "ok": {"kind": "text.lower"}}),
        &[("a", "text", "f", "text"), ("f", "text", "u", "text"), ("a", "text", "ok", "text")],
    );
    run(&h, "r2", 1, g, json!({}));
    let (_, reply) = collect(&h, "r2", 0);
    assert!(!reply.ok);
    assert_eq!(reply.nodes["f"].state, NodeState::Error);
    assert_eq!(reply.nodes["f"].error.as_ref().unwrap().kind, "op");
    assert_eq!(reply.nodes["u"].state, NodeState::Skipped);
    assert_eq!(reply.nodes["ok"].state, NodeState::Done, "an independent branch still runs");
}

fn graph_errors(mk: fn() -> Conn) {
    let h = mk();
    let cyc = graph(json!({"x": {"kind": "text.upper"}, "y": {"kind": "text.lower"}}), &[("x", "text", "y", "text"), ("y", "text", "x", "text")]);
    run(&h, "cyc", 1, cyc, json!({}));
    assert_eq!(collect(&h, "cyc", 0).1.error.unwrap().kind, "graph");
    run(&h, "unk", 2, graph(json!({"x": {"kind": "text.teleport"}}), &[]), json!({}));
    assert!(collect(&h, "unk", 0).1.error.unwrap().message.contains("teleport"));
    run(&h, "junk", 3, json!("not a graph"), json!({}));
    assert_eq!(collect(&h, "junk", 0).1.error.unwrap().kind, "graph");
}

fn slow_graph(seconds: f64) -> Value {
    graph(
        json!({"a": {"kind": "text.literal", "values": {"text": "wait"}}, "s": {"kind": "text.slow", "values": {"seconds": seconds}}, "z": {"kind": "text.upper"}}),
        &[("a", "text", "s", "text"), ("s", "text", "z", "text")],
    )
}

fn off_the_run_path(mk: fn() -> Conn) {
    plumcraft_conform::off_the_run_path(&fixture(mk)).unwrap();
}

fn cancel_mid_run(mk: fn() -> Conn) {
    plumcraft_conform::cancel_mid_run(&fixture(mk)).unwrap();
}

fn cancel_before_run(mk: fn() -> Conn) {
    plumcraft_conform::cancel_before_run(&fixture(mk)).unwrap();
}

fn every_delivery_reduces_the_same(mk: fn() -> Conn) {
    plumcraft_conform::every_delivery_reduces_the_same(&fixture(mk)).unwrap();
}

fn resume_from_seq(mk: fn() -> Conn) {
    plumcraft_conform::resume_from_seq(&fixture(mk)).unwrap();
}

/// Spec 19 item 12: a node that panics fails its run with `kind: engine`, the engine is rebuilt,
/// and the next run works.
#[test]
fn a_panicking_node_rebuilds_the_engine() {
    use plumcraft_engine::{Cx, Inputs, Node, Outputs};
    use plumcraft_proto::catalogue::NodeDef;
    use plumcraft_proto::error::NodeError;
    struct Boom;
    impl Node for Boom {
        fn def(&self) -> NodeDef {
            NodeDef { id: "test.boom".into(), ..Default::default() }
        }
        fn run(&self, _: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
            panic!("boom")
        }
    }
    let builds = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&builds);
    let h = Host::start(
        Box::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(plumcraft_text::engine().with_node(Box::new(Boom)))
        }),
        None,
    );
    h.wait_ready(Duration::from_secs(5));
    let c = InProc(h);
    run(&c, "bang", 1, graph(json!({"x": {"kind": "test.boom"}}), &[]), json!({}));
    let (events, reply) = collect(&c, "bang", 0);
    assert_eq!(reply.error.unwrap().kind, "engine");
    assert!(
        events.iter().any(|e| matches!(&e.event, Event::EngineState(EngineState::EngineDown { .. }))) || builds.load(std::sync::atomic::Ordering::SeqCst) == 2
    );
    assert!(matches!(c.0.wait_ready(Duration::from_secs(5)), EngineState::Connected { .. }));
    run(&c, "after", 2, chain(), json!({}));
    assert!(collect(&c, "after", 0).1.ok, "the rebuilt engine runs");
    assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), 2);
}

/// A node declaring `memo.scope: none` (time, network, randomness) is computed every run.
#[test]
fn memo_scope_none_is_never_cached() {
    use plumcraft_engine::{Cx, Inputs, Node, Outputs};
    use plumcraft_proto::catalogue::{MemoScope, MemoSpec, NodeDef, Port};
    use plumcraft_proto::error::NodeError;
    struct Clock;
    impl Node for Clock {
        fn def(&self) -> NodeDef {
            NodeDef {
                id: "test.clock".into(),
                outputs: vec![Port { name: "text".into(), ty: "TEXT".into(), ..Default::default() }],
                memo: MemoSpec { scope: MemoScope::None, ..Default::default() },
                ..Default::default()
            }
        }
        fn run(&self, _: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
            Ok(Outputs::from([("text".to_owned(), json!(format!("{:?}", Instant::now())).into())]))
        }
    }
    let h = Host::start(Box::new(|| Ok(plumcraft_text::engine().with_node(Box::new(Clock)))), None);
    h.wait_ready(Duration::from_secs(5));
    let c = InProc(h);
    let g = graph(json!({"t": {"kind": "test.clock"}, "u": {"kind": "text.upper"}}), &[("t", "text", "u", "text")]);
    for (i, id) in ["c1", "c2"].iter().enumerate() {
        run(&c, id, i as u64, g.clone(), json!({"memo": "use"}));
        let reply = collect(&c, id, 0).1;
        assert_eq!(reply.nodes["t"].state, NodeState::Done, "{id}: never cached");
        assert_eq!(reply.nodes["u"].state, NodeState::Done, "{id}: downstream of a fresh value recomputes");
    }
}

/// Spec 8.1: empty targets mean the graph's sinks, and watched nodes are added; a watch must not
/// narrow the run to the watched node's ancestors (found running the CLI demo, 2026-10-27).
fn empty_targets_run_every_sink_plus_watched(mk: fn() -> Conn) {
    let h = mk();
    let g = graph(
        json!({"a": {"kind": "text.literal", "values": {"text": "x y"}}, "up": {"kind": "text.upper"}, "n": {"kind": "text.words"}, "f": {"kind": "text.fail"}}),
        &[("a", "text", "up", "text"), ("a", "text", "n", "text"), ("a", "text", "f", "text")],
    );
    run(&h, "sinks", 1, g.clone(), json!({"watch": [{"node": "up", "port": "text"}]}));
    let (_, reply) = collect(&h, "sinks", 0);
    assert_eq!(reply.order, ["a", "f", "n", "up"]);
    assert_eq!(reply.nodes["f"].state, NodeState::Error);
    run(&h, "narrow", 2, g, json!({"targets": ["n"]}));
    let (_, reply) = collect(&h, "narrow", 0);
    assert_eq!(reply.order, ["a", "n"], "explicit targets still narrow the run");
}

/// Spec 14: a node whose plan key is unchanged is `cached`, not computed; an edit recomputes
/// only the edited node and what is downstream of it; `refresh` recomputes the targets; `off`
/// recomputes everything. The default mode is `use`.
fn memo_reuses_unchanged_nodes(mk: fn() -> Conn) {
    let h = mk();
    let g = |with: &str| {
        graph(
            json!({
                "a": {"kind": "text.literal", "values": {"text": "the corgi naps"}},
                "s": {"kind": "text.slow", "values": {"seconds": 0.3}},
                "r": {"kind": "text.replace", "values": {"find": "corgi", "with": with}},
                "u": {"kind": "text.upper"}
            }),
            &[("a", "text", "s", "text"), ("s", "text", "r", "text"), ("r", "text", "u", "text")],
        )
    };
    let states = |reply: &RunReply| reply.nodes.iter().map(|(k, n)| (k.clone(), n.state)).collect::<std::collections::BTreeMap<_, _>>();
    let go = |id: &str, generation: u64, graph: Value, memo: &str| {
        let mut extra = json!({"targets": ["u"], "watch": [{"node": "u", "port": "text"}]});
        if !memo.is_empty() {
            extra["memo"] = json!(memo);
        }
        run(&h, id, generation, graph, extra);
        let t = Instant::now();
        let (events, reply) = collect(&h, id, 0);
        (events, reply, t.elapsed())
    };
    let first = go("m1", 1, g("dog"), "use").1;
    assert!(first.ok && first.nodes.values().all(|n| n.state == NodeState::Done));
    assert!(first.nodes.values().all(|n| n.memo.as_ref().is_some_and(|m| m.plan_key.starts_with("memo:md5:"))));

    // Unchanged, default mode (no `memo` field sent): everything cached, nothing started.
    let mut req = json!({"v": 2, "op": "run", "run_id": "m2", "generation": 2, "graph": {"kind": "inline", "graph": g("dog")}, "targets": ["u"], "watch": [{"node": "u", "port": "text"}]});
    let t = Instant::now();
    call(&h, "run", req.take());
    let (events, again) = collect(&h, "m2", 0);
    assert!(t.elapsed() < Duration::from_millis(250), "cached runs don't sleep: {:?}", t.elapsed());
    assert!(again.ok);
    assert!(again.nodes.values().all(|n| n.state == NodeState::Cached), "{:?}", states(&again));
    assert!(!events.iter().any(|e| matches!(&e.event, Event::Status(r) if r.state == NodeState::Started)), "a hit is never started");
    assert_eq!(again.nodes["u"].outputs, first.nodes["u"].outputs, "a target's outputs come from the memo");
    assert_eq!(again.watch, first.watch, "the watched display is served from the memo");
    let key = |r: &RunReply| r.nodes["u"].memo.as_ref().map(|m| m.plan_key.clone());
    assert_eq!(key(&again), key(&first), "the same plan key");
    assert_eq!(again.nodes["u"].memo.as_ref().and_then(|m| m.source.as_deref()), Some("memory"));
    let pos = |node: &str, state: NodeState| events.iter().position(|e| matches!(&e.event, Event::Status(r) if r.node_id == node && r.state == state)).unwrap();
    assert!(pos("u", NodeState::Preview) < pos("u", NodeState::Cached), "preview before cached");
    assert_eq!(reduce("m2", 2, &events).states().values().filter(|s| **s == NodeState::Cached).count(), 4);

    // Edit `r`: upstream stays cached, `r` and downstream recompute.
    let (_, edited, _) = go("m3", 3, g("very good dog"), "use");
    assert_eq!(edited.nodes["a"].state, NodeState::Cached);
    assert_eq!(edited.nodes["s"].state, NodeState::Cached);
    assert_eq!(edited.nodes["r"].state, NodeState::Done);
    assert_eq!(edited.nodes["u"].state, NodeState::Done);
    assert_ne!(key(&edited), key(&first));

    // Refresh recomputes the target only; off recomputes everything.
    let refreshed = go("m4", 4, g("dog"), "refresh").1;
    assert_eq!(refreshed.nodes["u"].state, NodeState::Done);
    assert_eq!(refreshed.nodes["s"].state, NodeState::Cached);
    let (_, off, took) = go("m5", 5, g("dog"), "off");
    assert!(off.nodes.values().all(|n| n.state == NodeState::Done), "{:?}", states(&off));
    assert!(took >= Duration::from_millis(300), "off really runs the slow node");

    // A parameter's numeric type is part of its identity only through coercion: 0.3 and 0.30
    // are one value; an integer on a FLOAT port keys as that float.
    let mut int_secs = g("dog");
    int_secs["nodes"]["s"]["values"]["seconds"] = json!(0);
    let mut float_secs = g("dog");
    float_secs["nodes"]["s"]["values"]["seconds"] = json!(0.0);
    let ki = go("m6", 6, int_secs, "use").1;
    let kf = go("m7", 7, float_secs, "use").1;
    assert_eq!(kf.nodes["s"].state, NodeState::Cached, "0 on a FLOAT port is 0.0");
    assert_eq!(ki.nodes["s"].memo.as_ref().map(|m| &m.plan_key), kf.nodes["s"].memo.as_ref().map(|m| &m.plan_key));
}

/// The text a [`big`] graph's `u` node outputs: over the 64 KiB inline limit.
fn big_text() -> String {
    "the corgi naps on a floral carpet. ".repeat(3000).to_uppercase()
}

/// literal → repeat (over the inline limit) → upper, and code points of the big text.
fn big() -> Value {
    graph(
        json!({
            "a": {"kind": "text.literal", "values": {"text": "the corgi naps on a floral carpet. "}},
            "r": {"kind": "text.repeat", "values": {"times": 3000}},
            "u": {"kind": "text.upper"},
            "k": {"kind": "text.codes"}
        }),
        &[("a", "text", "r", "text"), ("r", "text", "u", "text"), ("u", "text", "k", "text")],
    )
}

fn blob(reply: &RunReply, node: &str, port: &str) -> DataRef {
    match reply.nodes.get(node).and_then(|n| n.outputs.get(port)) {
        Some(PortValue::Blob { data }) => data.clone(),
        other => panic!("{node}.{port} is not a blob: {other:?}"),
    }
}

/// Plan 10.2 D1: a value over `max_inline` crosses as a blob in the store, named by its handle,
/// and the UI reads it through the session's locators; the watched display carries an excerpt
/// and the full value by reference.
fn large_values_cross_as_blobs(mk: fn() -> Conn) {
    let h = mk();
    run(&h, "big", 1, big(), json!({"targets": ["u", "k", "a"], "watch": [{"node": "u", "port": "text"}]}));
    let (_, reply) = collect(&h, "big", 0);
    assert!(reply.ok, "{reply:?}");
    let text = big_text();
    assert!(text.len() > 64 << 10);
    let r = blob(&reply, "u", "text");
    assert_eq!(r.handle, Some(Handle::md5_raw(text.as_bytes())));
    // In the store on one machine; by URL to a UI elsewhere (mode 3, remote).
    // Text this repetitive is stored compressed (`plumz`), under the same handle.
    let local = r.at.iter().any(|l| matches!(l, Locator::Commonkit { key } if key.starts_with("plum:v1:plumz:")));
    let remote = r.at.iter().any(|l| matches!(l, Locator::Url { url, .. } if url.starts_with("/v1/blobs/md5/raw/")));
    assert!(local || remote, "{:?}", r.at);
    assert_eq!(h.resolve(&r).map(|b| b.bytes.into_vec()), Some(text.as_bytes().to_vec()));
    assert!(matches!(reply.nodes["a"].outputs["text"], PortValue::Json { .. }), "small values stay inline");
    // The code points of the big text: an array over the limit, keyed by `nd1`.
    let codes: Vec<u8> = text.chars().flat_map(|c| u32::from(c).to_le_bytes()).collect();
    let k = blob(&reply, "k", "codes");
    assert_eq!(k.handle, Some(Handle::md5_nd1("<u4", &[text.chars().count() as u64], &codes).unwrap()));
    let b = h.resolve(&k).expect("the array resolves");
    assert_eq!((b.dtype.as_str(), b.bytes.len()), ("<u4", codes.len()));
    // The watched display: an excerpt inline, the whole value by reference.
    let d = &reply.watch[0];
    assert!(matches!(d.renditions.first(), Some(Rendition::Excerpt { total_chars, .. }) if *total_chars == text.chars().count() as u64));
    assert_eq!(d.full.as_ref().and_then(|f| f.handle.clone()), r.handle);
}

/// Tables are named typed columns (media `table`); small arrays are blobs with inline bytes, so
/// their identity never depends on their size.
fn tables_and_small_arrays(mk: fn() -> Conn) {
    let h = mk();
    let g = graph(
        json!({"a": {"kind": "text.literal", "values": {"text": "Corgi, carpet!"}}, "l": {"kind": "text.letters"}, "k": {"kind": "text.codes"}}),
        &[("a", "text", "l", "text"), ("a", "text", "k", "text")],
    );
    run(&h, "tab", 1, g, json!({"targets": ["l", "k"], "watch": [{"node": "l", "port": "table"}, {"node": "k", "port": "codes"}]}));
    let (_, reply) = collect(&h, "tab", 0);
    let table = json!({"columns": [
        {"name": "letter", "type": "STRING", "values": ["a", "c", "e", "g", "i", "o", "p", "r", "t"]},
        {"name": "count", "type": "INT", "values": [1, 2, 1, 1, 1, 1, 1, 2, 1]}
    ]});
    assert_eq!(reply.nodes["l"].outputs["table"], PortValue::Json { value: table.clone() });
    assert_eq!(reply.watch[0].media, Media::Table);
    assert_eq!(reply.watch[0].value, Some(table));
    assert_eq!(reply.watch[0].summary, "table, 9 rows: letter, count");
    let k = blob(&reply, "k", "codes");
    assert!(matches!(k.at.as_slice(), [Locator::Inline { .. }]), "small: inline bytes");
    let codes: Vec<u8> = "Corgi, carpet!".chars().flat_map(|c| u32::from(c).to_le_bytes()).collect();
    assert_eq!(k.handle, Some(Handle::md5_nd1("<u4", &[14], &codes).unwrap()));
    assert_eq!(h.resolve(&k).map(|b| b.bytes.into_vec()), Some(codes));
    assert_eq!(reply.watch[1].media, Media::Ndarray);
    assert_eq!(reply.watch[1].summary, "array 14 <u4");
}

/// Spec 14.3: a file source is keyed by its content, never its path. Same bytes under another
/// name hit; new bytes under the same name miss.
fn file_sources_are_keyed_by_content(mk: fn() -> Conn) {
    let h = mk();
    let dir = test_root().join(format!("sources-{:?}", std::thread::current().id()).replace(['(', ')'], ""));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
    std::fs::write(&a, "a corgi\n").unwrap();
    std::fs::write(&b, "a corgi\n").unwrap();
    let g = |path: &std::path::Path| {
        graph(json!({"l": {"kind": "text.load", "values": {"path": path.display().to_string()}}, "u": {"kind": "text.upper"}}), &[("l", "text", "u", "text")])
    };
    let go = |id: &str, generation: u64, path: &std::path::Path| {
        run(&h, id, generation, g(path), json!({"targets": ["u"], "memo": "use"}));
        collect(&h, id, 0).1
    };
    let first = go("f1", 1, &a);
    assert_eq!(first.nodes["u"].outputs["text"], PortValue::Json { value: json!("A CORGI\n") });
    let renamed = go("f2", 2, &b);
    assert_eq!(renamed.nodes["l"].state, NodeState::Cached, "same bytes, other path: a hit");
    std::fs::write(&a, "two corgis\n").unwrap();
    let edited = go("f3", 3, &a);
    assert_eq!(edited.nodes["l"].state, NodeState::Done, "new bytes, same path: a miss");
    assert_eq!(edited.nodes["u"].outputs["text"], PortValue::Json { value: json!("TWO CORGIS\n") });
    let missing = go("f4", 4, &dir.join("nope.txt"));
    assert_eq!(missing.nodes["l"].state, NodeState::Error);
    assert_eq!(missing.nodes["l"].error.as_ref().unwrap().kind, "config");
}

/// Spec 4.6: a blob whose segment is gone is a miss, never an error; a rerun publishes it again.
/// Each call makes its own bytes: the modes run at once over one store, and another mode's
/// identical blob would be deleted from under it.
fn a_missing_blob_is_a_miss(mk: fn() -> Conn) {
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let text = format!("gone soon {}. ", CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
    let h = mk();
    let g = graph(
        json!({"a": {"kind": "text.literal", "values": {"text": text}}, "r": {"kind": "text.repeat", "values": {"times": 7001}}}),
        &[("a", "text", "r", "text")],
    );
    run(&h, "m1", 1, g.clone(), json!({"targets": ["r"], "memo": "use"}));
    let r = blob(&collect(&h, "m1", 0).1, "r", "text");
    // Reap it from the engine's store (the same file a `path` locator names on one machine).
    let segment = plumcraft_store::Store::new(test_root()).stored_path(r.handle.as_ref().unwrap()).unwrap();
    std::fs::remove_file(&segment).unwrap();
    assert!(h.resolve(&r).is_none(), "reaped: a miss");
    run(&h, "m2", 2, g, json!({"targets": ["r"], "memo": "use"}));
    let again = collect(&h, "m2", 0).1;
    assert_eq!(again.nodes["r"].state, NodeState::Cached);
    assert!(h.resolve(&blob(&again, "r", "text")).is_some(), "the rerun published it again");
}

/// Plan 10.3 E3 (spec 7.6 rule 8): groups are the editor's overlay. A workflow with groups
/// (nesting, promotions, outputs, collapsed, and the research software's VI-only `checkpoints`) runs to the
/// same reply as the same graph without them.
fn groups_are_ignored(mk: fn() -> Conn) {
    let h = mk();
    let mut grouped = chain();
    grouped["groups"] = json!({
        "g1": {"name": "shout and count", "nodes": ["b"], "collapsed": true, "pos": [10, 20],
               "promoted": [], "outputs": [{"from": {"node": "c", "port": "count"}, "name": "words"}], "checkpoints": ["b"]},
        "g2": {"name": "count", "nodes": ["c"], "parent": "g1", "collapsed": false, "pos": [0, 0]}
    });
    grouped["next_group"] = json!(3);
    let extra = json!({"targets": ["c"], "watch": [{"node": "b", "port": "text"}]});
    run(&h, "flat", 1, chain(), extra.clone());
    let (_, flat) = collect(&h, "flat", 0);
    run(&h, "grouped", 2, grouped, extra);
    let (_, with_groups) = collect(&h, "grouped", 0);
    assert!(flat.ok && with_groups.ok, "{with_groups:?}");
    assert_eq!(with_groups.order, flat.order);
    assert_eq!(with_groups.nodes.keys().collect::<Vec<_>>(), flat.nodes.keys().collect::<Vec<_>>());
    for (id, n) in &flat.nodes {
        let g = &with_groups.nodes[id];
        assert_eq!((g.state, &g.outputs, &g.error), (n.state, &n.outputs, &n.error), "{id}");
    }
    assert_eq!(with_groups.watch, flat.watch);
}

/// Every case above, in each mode and engine.
macro_rules! both_modes {
    ($($case:ident),* $(,)?) => {
        mod mode1_inproc {
            $(#[test] fn $case() { super::$case(super::inproc) })*
        }
        mod mode1_coordinated {
            $(#[test] fn $case() { super::$case(super::coordinated) })*
        }
        mod mode2_process {
            $(#[test] fn $case() { super::$case(super::process) })*
        }
        mod mode2_python {
            $(#[test] fn $case() { super::$case(super::python) })*
        }
        mod mode3_http {
            $(#[test] fn $case() { super::$case(super::http) })*
        }
        mod mode3_python {
            $(#[test] fn $case() { super::$case(super::http_python) })*
        }
    };
}

both_modes!(
    hello_and_schema,
    record_invariants,
    errors_and_skips,
    graph_errors,
    off_the_run_path,
    cancel_mid_run,
    cancel_before_run,
    every_delivery_reduces_the_same,
    resume_from_seq,
    empty_targets_run_every_sink_plus_watched,
    memo_reuses_unchanged_nodes,
    large_values_cross_as_blobs,
    tables_and_small_arrays,
    file_sources_are_keyed_by_content,
    a_missing_blob_is_a_miss,
    groups_are_ignored,
);

/// Spec 5.4 and 16.3: without the same-host proof a socket engine never hands out store
/// locators. A value over the inline limit is then `unavailable`; small values still cross.
fn without_the_proof_large_values_are_unavailable(h: &ProcessHost) {
    assert!(h.data_session().shm, "the proof passes on one machine");
    call(h, "session.confirm", json!({"shm": false, "path": false}));
    run(h, "np", 1, big(), json!({"targets": ["u", "a"]}));
    let reply = collect(h, "np", 0).1;
    match &reply.nodes["u"].outputs["text"] {
        PortValue::Opaque { summary, .. } => assert!(summary.contains("same-host proof"), "{summary}"),
        other => panic!("expected unavailable, got {other:?}"),
    }
    assert!(matches!(reply.nodes["a"].outputs["text"], PortValue::Json { .. }));
}

#[test]
fn mode2_without_the_proof_large_values_are_unavailable() {
    without_the_proof_large_values_are_unavailable(&spawn());
}

#[test]
fn mode2_python_without_the_proof_large_values_are_unavailable() {
    without_the_proof_large_values_are_unavailable(&spawn_python());
}

/// Spec 19 item 14: `kill -9` mid-run gives the run in flight a failed reply (`kind: engine`), the
/// host reports the engine down, restarts it, reloads its schema, and the next run works.
#[test]
fn mode2_kill_9_mid_run_recovers() {
    kill_9_mid_run_recovers(&spawn());
}

#[test]
fn mode2_python_kill_9_mid_run_recovers() {
    kill_9_mid_run_recovers(&spawn_python());
}

fn kill_9_mid_run_recovers(h: &ProcessHost) {
    plumcraft_conform::kill_9_mid_run_recovers(h, &graphs()).unwrap();
    let EngineState::Connected { nodes, .. } = h.state() else { panic!("{:?}", h.state()) };
    assert!(nodes >= 9);
}

/// The Rust and Python engines are the same engine to the UI: the same catalogue, and the same
/// reply and status sequence for the demo workflow, once timings and publishers are set aside.
#[test]
fn rust_and_python_engines_agree() {
    use plumcraft_proto::catalogue::SchemaReply;
    let (rust, py) = (spawn(), spawn_python());
    let schema = |h: &ProcessHost| -> SchemaReply { serde_json::from_value(call(h, "schema", json!({}))).unwrap() };
    let (rs, ps) = (schema(&rust), schema(&py));
    assert_eq!(rs.wire_types, ps.wire_types);
    assert_eq!(rs.nodes, ps.nodes);
    assert_eq!((rs.engine.id, rs.engine.name, rs.engine.version), (ps.engine.id, ps.engine.name, ps.engine.version));
    assert_ne!(rs.engine.salt, ps.engine.salt, "different code, different memo salt");

    let demo: Value = serde_json::from_str(plumcraft_text::DEMO_WORKFLOW).unwrap();
    let mut demo_fast = demo.clone();
    demo_fast["nodes"]["think"]["values"]["seconds"] = json!(0.0);
    let extra = json!({"targets": ["count", "shout", "after"], "watch": [{"node": "shout", "port": "text"}, {"node": "count", "port": "count"}]});
    let outcome = |h: &ProcessHost| {
        run(h, "demo", 5, demo_fast.clone(), extra.clone());
        let (events, mut reply) = collect(h, "demo", 0);
        reply.elapsed_ms = 0.0;
        for n in reply.nodes.values_mut() {
            n.ms = 0.0;
            n.memo = None; // plan keys include the engine salt, which differs by design
        }
        // Times and the store differ; how many nodes each cache answered must not.
        if let Some(s) = &mut reply.stats {
            (s.computed_ms, s.saved_ms) = (0.0, 0.0);
            s.store.clear();
            s.tree = None;
        }
        // Status as (node, state, error, has display) in order; progress depends on timing.
        let statuses: Vec<(String, NodeState, Option<String>, usize)> = events
            .iter()
            .filter_map(|e| match &e.event {
                Event::Status(r) if r.state != NodeState::Progress => {
                    Some((r.node_id.clone(), r.state, r.error.as_ref().map(|x| x.message.clone()), r.display.len()))
                }
                _ => None,
            })
            .collect();
        (reply, statuses)
    };
    let (r_reply, r_status) = outcome(&rust);
    let (p_reply, p_status) = outcome(&py);
    assert_eq!(r_status, p_status);
    assert_eq!(r_reply, p_reply);
    // Values where the two languages' string functions could differ.
    let tricky = graph(
        json!({"a": {"kind": "text.literal", "values": {"text": "Straße ǅ  x\u{1c}y\r\nz\n"}}, "u": {"kind": "text.upper"}, "l": {"kind": "text.lower"}, "w": {"kind": "text.words"}, "r": {"kind": "text.reverse"}}),
        &[("a", "text", "u", "text"), ("a", "text", "l", "text"), ("a", "text", "w", "text"), ("a", "text", "r", "text")],
    );
    let values = |h: &ProcessHost| {
        run(h, "tricky", 1, tricky.clone(), json!({"targets": ["u", "l", "w", "r"], "watch": [{"node": "a", "port": "text"}]}));
        let (_, reply) = collect(h, "tricky", 0);
        (reply.nodes.iter().map(|(k, n)| (k.clone(), n.outputs.clone(), n.thumb.clone())).collect::<Vec<_>>(), reply.watch)
    };
    assert_eq!(values(&rust), values(&py));
    // Data: a table, a small array, and big text and codes as blobs, handles and locators alike.
    let data = |h: &ProcessHost| {
        let mut g = big();
        g["nodes"]["l"] = json!({"kind": "text.letters"});
        g["wires"].as_array_mut().unwrap().push(json!({"from": {"node": "a", "port": "text"}, "to": {"node": "l", "port": "text"}}));
        run(
            h,
            "data",
            1,
            g,
            json!({"targets": ["u", "k", "l"], "watch": [{"node": "u", "port": "text"}, {"node": "k", "port": "codes"}, {"node": "l", "port": "table"}]}),
        );
        let (_, reply) = collect(h, "data", 0);
        (reply.nodes.iter().map(|(k, n)| (k.clone(), n.outputs.clone())).collect::<Vec<_>>(), reply.watch)
    };
    let (r_data, p_data) = (data(&rust), data(&py));
    assert!(matches!(r_data.0.iter().find(|(k, _)| k == "k").map(|(_, o)| &o["codes"]), Some(PortValue::Blob { .. })));
    assert_eq!(r_data, p_data);
    // The converters between core types (20-convert.md 6.3), each into its inverse.
    let pairs = graph(
        json!({
            "csv": {"kind": "text.literal", "values": {"text": "name,n,x,ok\nada,1,1.5,TRUE\nbob,,-2,false\n"}},
            "t": {"kind": "text.text_to_table"}, "back": {"kind": "text.table_to_text"},
            "a": {"kind": "text.table_to_array"}, "at": {"kind": "text.array_to_table"},
            "codes": {"kind": "text.codes"}, "ct": {"kind": "text.array_to_table"},
            "js": {"kind": "text.literal", "values": {"text": "{\"b\": [1, 2.5, \"é\", null], \"a\": {\"y\": true}}"}},
            "j": {"kind": "text.text_to_json"}, "jt": {"kind": "text.json_to_text"},
        }),
        &[
            ("csv", "text", "t", "text"),
            ("t", "table", "back", "table"),
            ("t", "table", "a", "table"),
            ("a", "array", "at", "array"),
            ("csv", "text", "codes", "text"),
            ("codes", "codes", "ct", "array"),
            ("js", "text", "j", "text"),
            ("j", "json", "jt", "json"),
        ],
    );
    let converted = |h: &ProcessHost| {
        run(h, "pairs", 1, pairs.clone(), json!({"targets": ["back", "at", "ct", "jt"]}));
        let (_, reply) = collect(h, "pairs", 0);
        reply.nodes.iter().map(|(k, n)| (k.clone(), n.outputs.clone(), n.error.as_ref().map(|e| e.message.clone()))).collect::<Vec<_>>()
    };
    let (r_pairs, p_pairs) = (converted(&rust), converted(&py));
    let out = |node: &str, port: &str| {
        r_pairs.iter().find(|(k, ..)| k == node).and_then(|(_, o, _)| match o.get(port) {
            Some(PortValue::Json { value }) => Some(value.clone()),
            _ => None,
        })
    };
    assert_eq!(out("back", "text"), Some(json!("name,n,x,ok\nada,1,1.5,true\nbob,,-2.0,false\n")), "{r_pairs:?}");
    assert_eq!(
        out("at", "table"),
        Some(json!({"columns": [{"name": "n", "type": "FLOAT", "values": [1.0, null]}, {"name": "x", "type": "FLOAT", "values": [1.5, -2.0]}]}))
    );
    assert_eq!(out("ct", "table").map(|t| t["columns"][0]["type"].clone()), Some(json!("INT")));
    assert_eq!(out("jt", "text"), Some(json!("{\n  \"a\": {\n    \"y\": true\n  },\n  \"b\": [\n    1,\n    2.5,\n    \"é\",\n    null\n  ]\n}")));
    assert_eq!(r_pairs, p_pairs);
}

/// Spec 13.2: a file source uploads once (the engine says it has it after that), and a value
/// downloads only when read, verified, into the UI's CAS.
fn sources_upload_once_and_values_download_lazily(h: &HttpHost) {
    let dir = test_root().join(format!("http-src-{}", h.base().replace([':', '/', '.'], "_")));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("story.txt");
    // Unique per engine: every engine in this suite shares one store, which dedupes uploads.
    let line = format!("a corgi naps at {}. ", h.base());
    std::fs::write(&f, line.repeat(3000)).unwrap();
    let g = graph(json!({"l": {"kind": "text.load", "values": {"path": f.display().to_string()}}, "u": {"kind": "text.upper"}}), &[("l", "text", "u", "text")]);
    let before = h.uploads();
    run(h, "s1", 1, g.clone(), json!({"targets": ["u"]}));
    let first = collect(h, "s1", 0).1;
    assert!(first.ok, "{first:?}");
    assert_eq!(h.uploads(), before + 1, "uploaded once");
    run(h, "s2", 2, g.clone(), json!({"targets": ["u"]}));
    assert!(collect(h, "s2", 0).1.ok);
    assert_eq!(h.uploads(), before + 1, "the engine had it: nothing uploaded");
    // Lazily: nothing is downloaded until read; then once, verified, into the CAS.
    let r = blob(&first, "u", "text");
    assert!(matches!(r.at.as_slice(), [Locator::Url { .. }]), "{:?}", r.at);
    let b = h.resolve(&r).expect("downloads");
    assert_eq!(&*b.bytes, line.to_uppercase().repeat(3000).as_bytes());
    std::fs::write(&f, format!("two corgis nap at {}. ", h.base())).unwrap();
    run(h, "s3", 3, g, json!({"targets": ["u"]}));
    assert_eq!(collect(h, "s3", 0).1.nodes["u"].outputs["text"], PortValue::Json { value: json!(format!("TWO CORGIS NAP AT {}. ", h.base().to_uppercase())) });
    assert_eq!(h.uploads(), before + 2, "new bytes upload");
}

#[test]
fn mode3_sources_upload_once_and_values_download_lazily() {
    sources_upload_once_and_values_download_lazily(&spawn_http(rust_manifest()));
}

#[test]
fn mode3_python_sources_upload_once_and_values_download_lazily() {
    sources_upload_once_and_values_download_lazily(&spawn_http(python_manifest()));
}

/// Spec 16.4 and 18: every request but `health` needs the token; a wrong one is refused.
#[test]
fn mode3_a_wrong_token_is_refused() {
    for m in [rust_manifest(), python_manifest()] {
        plumcraft_conform::a_wrong_token_is_refused(&spawn_http(m), test_root().join("cas-wrong")).unwrap();
    }
}

/// An engine that dies at once is reported down with its stderr, not hung on.
#[test]
fn mode2_an_engine_that_cannot_start_is_down_not_hung() {
    let m = Manifest {
        name: "broken".into(),
        command: if cfg!(windows) {
            vec!["cmd".into(), "/C".into(), "echo no models found 1>&2 & exit 3".into()]
        } else {
            vec!["/bin/sh".into(), "-c".into(), "echo 'no models found' >&2; exit 3".into()]
        },
        env: Vec::new(),
        hint: "install the models".into(),
        store: None,
        cwd: None,
    };
    let h = ProcessHost::start(m).unwrap();
    let t = Instant::now();
    // Five starts and 1.5 s of backoff before the host gives up: under 3 s here, but a process
    // start costs more on Windows.
    let wait = if cfg!(windows) { Duration::from_secs(10) } else { Duration::from_secs(3) };
    let state = h.wait_ready(wait);
    assert!(t.elapsed() <= wait + Duration::from_secs(1));
    assert!(!matches!(state, EngineState::Connected { .. }));
    let EngineState::EngineDown { reason, stderr_tail, hint, .. } = h.state() else { panic!("{:?}", h.state()) };
    assert!(reason.contains("exit 3"), "{reason}");
    assert!(stderr_tail.unwrap_or_default().contains("no models found"));
    assert_eq!(hint, "install the models");
    assert!(h.call("schema", json!({})).is_err(), "calls fail at once while the engine is down");
}
