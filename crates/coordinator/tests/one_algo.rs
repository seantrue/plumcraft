//! #91, #92 (spec 4.3): a run leaves nothing hashed in any algorithm but the process's
//! (commonkit's `HASH`/`COMMONKIT_HASH`), anywhere: the handles it sends, the store's segments,
//! the persisted memo records and the handles and plan keys they name, content-keyed records,
//! file and SVG blobs, audio overviews. Every run starts from an empty store of its own (the
//! maintainer, 2026-11-12: "Clear out store before starting tests on MD5 or BLAKE. Any
//! appearance of unintended hashes is a bug"). The suite runs once per algorithm; the scan is
//! `plumcraft scan-store`'s own.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use plumcraft_coordinator::Coordinator;
use plumcraft_engine::host::{Host, PollReply};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::data::PortValue;
use plumcraft_proto::event::Event;
use plumcraft_proto::handle::Algo;
use plumcraft_proto::run::RunReply;
use plumcraft_proto::session::EngineState;
use serde_json::{Value, json};

#[allow(dead_code)]
#[path = "../../../apps/plumcraft-cli/src/scan_store.rs"]
mod scan_store;

/// A new, empty store for one run, checked empty.
fn empty_store(tag: &str) -> PathBuf {
    let d = plumcraft_scratch::root().join(format!("one-algo-{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0, "{}: not empty", d.display());
    d
}

/// Fails with every digest in `root` that isn't `algo`'s, and every handle in `reply` that isn't.
fn assert_only(algo: Algo, root: &Path, reply: &RunReply) {
    let mut bad = scan_store::foreign_digests(root, algo).unwrap();
    let mut handles = Vec::new();
    collect_handles(&serde_json::to_value(reply).unwrap(), &mut handles);
    assert!(!handles.is_empty());
    bad.extend(handles.into_iter().filter(|h| !h.starts_with(&format!("{}:", algo.as_str()))).map(|h| format!("reply: {h}")));
    assert!(bad.is_empty(), "a run under {} left other hashes:\n  {}", algo.as_str(), bad.join("\n  "));
}

fn collect_handles(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == "handle"
                    && let Some(s) = x.as_str()
                {
                    out.push(s.to_owned());
                } else {
                    collect_handles(x, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_handles(x, out)),
        _ => {}
    }
}

fn host(make: fn() -> plumcraft_engine::Engine, store: &Path) -> Arc<dyn Connection> {
    let store = store.to_path_buf();
    let h = Host::start(Box::new(move || Ok(make().with_store(Some(store.clone())))), None);
    assert!(matches!(h.wait_ready(Duration::from_secs(5)), EngineState::Connected { .. }));
    Arc::new(InProc(h))
}

fn engines(store: &Path) -> Coordinator {
    Coordinator::new()
        .with_engine("text", host(plumcraft_text::engine, store))
        .with_engine("array", host(plumcraft_array::engine, store))
        .with_engine("audio", host(plumcraft_audio::engine, store))
}

fn run(c: &Coordinator, id: &str, g: &Value, extra: Value) -> RunReply {
    let mut req = json!({"v": 2, "op": "run", "run_id": id, "generation": 1, "graph": {"kind": "inline", "graph": g}, "memo": "use"});
    if let (Some(r), Some(x)) = (req.as_object_mut(), extra.as_object()) {
        r.extend(x.clone());
    }
    c.call("run", req).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut since = 0;
    while Instant::now() < deadline {
        let p: PollReply = serde_json::from_value(c.call("events.poll", json!({"since": since, "wait_ms": 300, "run_id": id})).unwrap()).unwrap();
        since = p.last_seq;
        for e in p.events {
            if let Event::RunReply(r) = e.event {
                return *r;
            }
        }
    }
    panic!("no reply for {id}");
}

/// A graph with every kind of value: arrays, a text → array crossing, an encoded picture,
/// an SVG drawn by a node and one loaded from a file, a picture loaded from a file, audio (with
/// its overview, watched) and a WAV. Every node persists (`cache: always`).
fn everything(files: &Path) -> Value {
    std::fs::write(files.join("dot.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><circle cx="4" cy="4" r="3"/></svg>"#).unwrap();
    // a 1×1 PNG
    let png: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89,
        0, 0, 0, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0, 0, 0, 0,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    std::fs::write(files.join("dot.png"), png).unwrap();
    let mut nodes = json!({
        "m": {"kind": "array.mandelbrot", "values": {"max_iterations": 16}},
        "png": {"kind": "array.encode_image", "values": {"format": "png"}},
        "svg": {"kind": "array.array_to_svg"},
        "lit": {"kind": "text.literal", "values": {"text": "corgi"}},
        "codes": {"kind": "text.codes"},
        "twice": {"kind": "array.arithmetic", "values": {"operation": "multiply", "constant": 2.0}},
        "svgfile": {"kind": "file.load", "values": {"file": files.join("dot.svg").display().to_string()}},
        "pngfile": {"kind": "file.load", "values": {"file": files.join("dot.png").display().to_string()}},
        "tone": {"kind": "audio.tone", "values": {"seconds": 0.25}},
        "wav": {"kind": "audio.to_wav"},
    });
    for n in nodes.as_object_mut().unwrap().values_mut() {
        n["cache"] = json!("always");
    }
    let w = |a: &str, ap: &str, b: &str, bp: &str| json!({"from": {"node": a, "port": ap}, "to": {"node": b, "port": bp}});
    json!({"version": 1, "nodes": nodes, "wires": [
        w("m", "escape", "png", "image"),
        w("m", "escape", "svg", "image"),
        w("lit", "text", "codes", "text"),
        w("codes", "codes", "twice", "a"),
        w("tone", "audio", "wav", "audio"),
    ]})
}

/// Fix the wires to the engines' real port names (`text.codes`' output, `audio.tone`'s).
fn ports(c: &Coordinator, mut g: Value) -> Value {
    let s: plumcraft_proto::catalogue::SchemaReply = serde_json::from_value(c.call("schema", json!({})).unwrap()).unwrap();
    for w in g["wires"].as_array_mut().unwrap() {
        let from = w["from"]["node"].as_str().unwrap().to_owned();
        let kind = g_kind(&from);
        if let Some(def) = s.node(kind)
            && def.outputs.len() == 1
        {
            w["from"]["port"] = json!(def.outputs[0].name);
        }
    }
    g
}

fn g_kind(node: &str) -> &'static str {
    match node {
        "m" => "array.mandelbrot",
        "lit" => "text.literal",
        "codes" => "text.codes",
        "tone" => "audio.tone",
        _ => "",
    }
}

const TARGETS: [&str; 6] = ["png", "svg", "twice", "svgfile", "pngfile", "wav"];

fn every_value_run(tag: &str, ask: &str) -> (PathBuf, RunReply) {
    let store = empty_store(tag);
    let files = plumcraft_scratch::dir(&format!("one-algo-files-{tag}"));
    let c = engines(&store);
    let g = ports(&c, everything(&files));
    let watch = json!([{"node": "tone", "port": "audio"}]);
    // `algo`: an old client's per-request field, which picks nothing now.
    let reply = run(&c, &format!("{tag}-1"), &g, json!({"targets": TARGETS, "watch": watch, "algo": ask}));
    assert!(reply.ok, "{reply:?}");
    for t in TARGETS {
        assert!(reply.nodes[t].outputs.values().all(|v| matches!(v, PortValue::Blob { .. })), "{t}: {:?}", reply.nodes[t].outputs);
    }
    (store, reply)
}

/// #91, #92: a run from an empty store leaves only the process's algorithm, even when an old
/// client asks for the other one.
#[test]
fn a_run_leaves_only_the_current_algorithm() {
    for ask in ["", "b3", "md5"] {
        let (store, reply) = every_value_run(&format!("ask-{}", if ask.is_empty() { "none" } else { ask }), ask);
        assert_only(Algo::current(), &store, &reply);
    }
}

/// The suite runs isolated, in the algorithm it was asked for (the hash facade, §3.1).
#[test]
fn hash_is_what_was_asked() {
    commonkit::testing::assert_isolated();
    if let Ok(asked) = std::env::var("COMMONKIT_HASH") {
        assert_eq!(Algo::from_name(asked.trim()), Some(Algo::current()), "COMMONKIT_HASH={asked}");
    }
}
