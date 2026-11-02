//! Helpers for the suites that need a real craft: the binaries and the corgi are found or the
//! case is skipped with a message (so CI without VectorCraft stays green), each case gets its
//! own CAS root (so its craft processes can be found by `pgrep -f <root>`), and connections in
//! mode 1 and mode 2.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use plumcraft_craft::manifest::{Manifest, builtin};
use plumcraft_craft::nodes::Shared;
use plumcraft_craft::{Options, Report};
use plumcraft_engine::host::{Host, PollReply};
use plumcraft_host::process::ProcessHost;
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::PortValue;
use plumcraft_proto::data::DataRef;
use plumcraft_proto::event::{Envelope, Event, NodeState};
use plumcraft_proto::run::{RunAck, RunReply};
use plumcraft_proto::session::EngineState;
use serde_json::{Value, json};

pub type Conn = Box<dyn Connection>;

/// The private commonkit root of this test process.
pub fn store_root() -> PathBuf {
    let root = plumcraft_scratch::root().join("craft-tests");
    let _ = std::fs::create_dir_all(&root);
    root
}

/// A fresh craft root for one case.
pub fn craft_root(name: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let d = store_root().join(format!("craft-{name}-{}", N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The VectorCraft pack, or `None` (and a message) when its binary isn't built.
pub fn vectorcraft() -> Option<Manifest> {
    let m = builtin("vectorcraft").unwrap().unwrap();
    if m.executable().is_none() {
        eprintln!("SKIP: {} is not built ({}); {}", m.name, m.program().first().cloned().unwrap_or_default(), m.hint);
        return None;
    }
    Some(m)
}

pub fn soundcraft() -> Option<Manifest> {
    let m = builtin("soundcraft").unwrap().unwrap();
    if m.executable().is_none() {
        eprintln!("SKIP: soundcraft-cli is not on PATH; {}", m.hint);
        return None;
    }
    Some(m)
}

/// The test image, the corgi on a floral carpet: `$PLUMCRAFT_CORGI`, or the copy note 14's probes
/// made.
pub fn corgi() -> Option<PathBuf> {
    let candidates =
        [std::env::var("PLUMCRAFT_CORGI").ok().map(PathBuf::from), Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../claude-debug/craft-mcp/corgi.jpg"))];
    let found = candidates.into_iter().flatten().find(|p| p.is_file());
    if found.is_none() {
        eprintln!("SKIP: no corgi: set PLUMCRAFT_CORGI to test.jpg");
    }
    found
}

/// What a case needs: the pack, the corgi, its own root.
pub struct Env {
    pub manifest: Manifest,
    pub corgi: PathBuf,
    pub root: PathBuf,
}

impl Env {
    pub fn new(name: &str) -> Option<Env> {
        Some(Env { manifest: vectorcraft()?, corgi: corgi()?, root: craft_root(name) })
    }

    pub fn corgi_str(&self) -> String {
        self.corgi.display().to_string()
    }

    pub fn options(&self) -> Options {
        Options { root: Some(self.root.clone()), store: Some(Some(store_root())), include_excluded: false }
    }

    /// The craft processes of this case's pool.
    pub fn craft_pids(&self) -> Vec<u32> {
        let out = std::process::Command::new("pgrep")
            .args(["-f", &format!("automation-read-root {}", self.root.canonicalize().unwrap().display())])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect()
    }
}

/// Mode 1, with the engine's shared state (the pool) for the cases that look inside.
/// The engine's shared state and build report, filled when the host builds it.
pub type Slot = Arc<Mutex<Option<(Arc<Shared>, Report)>>>;

pub fn inproc(env: &Env) -> (Conn, Slot) {
    let slot: Slot = Arc::default();
    let (m, opts, s) = (env.manifest.clone(), env.options(), Arc::clone(&slot));
    let h = Host::start(
        Box::new(move || {
            let (e, report, shared) = plumcraft_craft::build(&m, &opts)?;
            *s.lock().unwrap() = Some((shared, report));
            Ok(e)
        }),
        None,
    );
    let state = h.wait_ready(Duration::from_secs(60));
    assert!(matches!(state, EngineState::Connected { .. }), "{state:?}");
    (Box::new(InProc(h)), slot)
}

/// Mode 2: `plumcraft-craft-engine --craft <pack>` on a Unix socket.
pub fn process_with(env: &Env, pack: &str) -> ProcessHost {
    let manifest = plumcraft_host::process::Manifest {
        name: pack.into(),
        command: vec![env!("CARGO_BIN_EXE_plumcraft-craft-engine").into(), "--craft".into(), pack.into()],
        env: vec![("COMMONKIT_STORE_MIN_FREE_GB".into(), "0".into()), ("PLUMCRAFT_CRAFT_ROOT".into(), env.root.display().to_string())],
        hint: "cargo build -p plumcraft-craft".into(),
        store: Some(store_root()),
        cwd: None,
    };
    let h = ProcessHost::start(manifest).unwrap();
    let state = h.wait_ready(Duration::from_secs(60));
    assert!(matches!(state, EngineState::Connected { .. }), "{state:?}");
    h
}

pub fn call(h: &dyn Connection, method: &str, params: Value) -> Value {
    h.call(method, params).unwrap_or_else(|e| panic!("{method}: {e}"))
}

pub fn graph(nodes: Value, wires: &[(&str, &str, &str, &str)]) -> Value {
    let wires: Vec<Value> = wires.iter().map(|(a, ap, b, bp)| json!({"from": {"node": a, "port": ap}, "to": {"node": b, "port": bp}})).collect();
    json!({"version": 1, "nodes": nodes, "wires": wires})
}

/// A chain wired `doc` → `doc`: `[(id, kind, values)]`.
pub fn chain(steps: &[(&str, &str, Value)]) -> Value {
    let nodes: serde_json::Map<String, Value> = steps.iter().map(|(id, kind, v)| ((*id).to_owned(), json!({"kind": kind, "values": v}))).collect();
    let wires: Vec<(&str, &str, &str, &str)> = steps.windows(2).map(|w| (w[0].0, "doc", w[1].0, "doc")).collect();
    graph(Value::Object(nodes), &wires)
}

/// Send a run (memo off unless `extra` says otherwise).
pub fn run(h: &dyn Connection, id: &str, g: Value, extra: Value) -> RunAck {
    let mut req = json!({"v": 2, "op": "run", "run_id": id, "generation": 1, "graph": {"kind": "inline", "graph": g}, "memo": "off"});
    if let (Some(r), Some(x)) = (req.as_object_mut(), extra.as_object()) {
        r.extend(x.clone());
    }
    serde_json::from_value(call(h, "run", req)).unwrap()
}

/// Poll until the run's reply, returning every envelope seen.
pub fn collect(h: &dyn Connection, run_id: &str, since: u64) -> (Vec<Envelope>, RunReply) {
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut since = since;
    let mut all = Vec::new();
    while Instant::now() < deadline {
        let p: PollReply = serde_json::from_value(call(h, "events.poll", json!({"since": since, "wait_ms": 200, "run_id": run_id}))).unwrap();
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

/// Wait until `node` reaches `state` in run `run_id`; returns the last seq seen.
pub fn wait_for(h: &dyn Connection, run_id: &str, node: &str, state: NodeState, within: Duration) -> u64 {
    let deadline = Instant::now() + within;
    let mut since = 0;
    while Instant::now() < deadline {
        let p: PollReply = serde_json::from_value(call(h, "events.poll", json!({"since": since, "wait_ms": 50, "run_id": run_id}))).unwrap();
        since = p.last_seq;
        for e in &p.events {
            if matches!(&e.event, Event::Status(r) if r.node_id == node && r.state == state) {
                return since;
            }
            assert!(!matches!(&e.event, Event::RunReply(_)), "the run ended before {node} was {state:?}");
        }
    }
    panic!("{node} never reached {state:?}");
}

pub fn go(h: &dyn Connection, id: &str, g: Value, targets: &[&str], memo: &str) -> RunReply {
    run(h, id, g, json!({"targets": targets, "memo": memo}));
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
        other => panic!("{node}.{port} is not JSON: {other:?}"),
    }
}

pub fn states(reply: &RunReply) -> Vec<(String, NodeState)> {
    reply.nodes.iter().map(|(k, n)| (k.clone(), n.state)).collect()
}

/// The recolour map the chain uses: the trace's darkest common colour to red.
pub fn red_map() -> Value {
    json!([{"from": ["rgb 81 74 61"], "to": "#ff0000"}])
}

/// The raster → Trace (6 Colors) → Recolor → SVG graph (note 14, M2.5).
pub fn trace_chain(env: &Env) -> Value {
    chain(&[
        ("o", "vc.open", json!({"file": env.corgi_str()})),
        ("t", "vc.trace", json!({"preset": "6 Colors"})),
        ("c", "vc.recolor", json!({"map": red_map()})),
        ("s", "vc.export_svg", json!({})),
    ])
}

/// The same chain through `vectorcraft-cli run … --export`, with its SVG.
pub fn cli_trace_chain(env: &Env) -> Vec<u8> {
    let exe = env.manifest.executable().unwrap();
    let out = env.root.join("cli-chain.svg");
    let status = std::process::Command::new(exe)
        .args(["run", "--in", &env.corgi_str(), "--cmd", "select.all", "--cmd", "imageTrace.makeAndExpand", "--params", r#"{"preset": "6 Colors"}"#])
        .args(["--cmd", "select.all", "--cmd", "recolor.apply", "--params", &json!({"map": red_map()}).to_string(), "--export", &out.display().to_string()])
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read(out).unwrap()
}

/// A long trace: the corgi exported at 4× (3244 px), reopened, traced as High Fidelity Photo
/// (about 3 s, and it ignores MCP cancel), then exported. `extra` adds nodes after it.
pub fn long_trace(env: &Env) -> Value {
    graph(
        json!({
            "a_open": {"kind": "vc.open", "values": {"file": env.corgi_str()}},
            "b_png": {"kind": "vc.export_png", "values": {"scale": 4.0}},
            "c_open": {"kind": "vc.open"},
            "d_trace": {"kind": "vc.trace", "values": {"preset": "High Fidelity Photo"}},
            "e_open": {"kind": "vc.open", "values": {"file": env.corgi_str()}},
            "f_colors": {"kind": "vc.colors"},
            "g_svg": {"kind": "vc.export_svg"}
        }),
        &[
            ("a_open", "doc", "b_png", "doc"),
            ("b_png", "image", "c_open", "image"),
            ("c_open", "doc", "d_trace", "doc"),
            ("d_trace", "doc", "g_svg", "doc"),
            ("e_open", "doc", "f_colors", "doc"),
        ],
    )
}
