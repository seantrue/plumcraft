//! The craft adapter through the interface on VectorCraft (note 14, M2.5 and M2.6), in mode 1
//! (compiled in) and mode 2 (`plumcraft-craft-engine` on a socket). Every case is driven only
//! through [`plumcraft_host::Connection::call`]. Cases skip with a message when VectorCraft or the
//! corgi is missing.

mod common;

use std::time::{Duration, Instant};

use common::*;
use plumcraft_proto::Handle;
use plumcraft_proto::catalogue::{Media, MemoScope};
use plumcraft_proto::data::{Form, Locator};
use plumcraft_proto::display::Rendition;
use plumcraft_proto::event::{Event, NodeState};
use plumcraft_proto::session::HelloReply;
use serde_json::{Value, json};

type Mk = fn(&Env) -> Conn;

fn mode1(e: &Env) -> Conn {
    inproc(e).0
}

fn mode2(e: &Env) -> Conn {
    Box::new(process_with(e, "vectorcraft"))
}

/// `hello` and `schema`: the node set note 14 lists, typed ports, the salt names the binary.
fn hello_and_schema(mk: Mk) {
    let Some(env) = Env::new("schema") else { return };
    let h = mk(&env);
    let hello: HelloReply =
        serde_json::from_value(call(&h, "hello", json!({"v": 2, "op": "hello", "protocol": {"min": 2, "max": 2}, "caps": ["events.poll"]}))).unwrap();
    assert_eq!(hello.engine.id, "vectorcraft");
    let (exe_digest, _) = plumcraft_craft::cas::hash_file(&env.manifest.executable().unwrap()).unwrap();
    assert!(hello.engine.salt.starts_with(&format!("craft:vectorcraft:{}:{}", hello.engine.version, exe_digest.digest())), "{}", hello.engine.salt);
    assert!(!hello.data.work_root.is_empty(), "the CAS is a declared work root");
    let schema = call(&h, "schema", json!({}));
    let nodes = schema["nodes"].as_array().unwrap();
    let ids: Vec<&str> = nodes.iter().filter_map(|n| n["id"].as_str()).collect();
    let fx = ids.iter().filter(|i| i.starts_with("vc.fx.")).count();
    let blocks = nodes.iter().filter(|n| n["category"][0] == "Building blocks").filter_map(|n| n["id"].as_str()).collect::<Vec<_>>();
    let curated = ids.iter().filter(|i| !i.starts_with("vc.fx.") && !blocks.contains(i) && **i != "vc.run").count();
    assert_eq!(blocks.len(), 17, "the SDK's building blocks: 8 Vary, 5 sequences (with Table to Rows and Rows to Table), 4 files");
    eprintln!("listed: {curated} curated, {fx} effects, vc.run: {}", ids.contains(&"vc.run"));
    assert_eq!(curated, 25);
    assert!(fx >= 119, "VectorCraft's effect.list grows with its builds (119 on 2026-10-30): {fx}");
    assert!(ids.contains(&"vc.run"));
    let wt = |id: &str| schema["wire_types"].as_array().unwrap().iter().find(|w| w["id"] == id).cloned().unwrap();
    assert_eq!(wt("vectorcraft.doc")["media"], "doc");
    assert_eq!(wt("SVG")["media"], "vector");
    assert_eq!(wt("IMAGE")["media"], "image");
    let node = |id: &str| nodes.iter().find(|n| n["id"] == id).unwrap().clone();
    let trace = node("vc.trace");
    let preset = trace["inputs"].as_array().unwrap().iter().find(|p| p["name"] == "preset").unwrap();
    assert_eq!(preset["choices"].as_array().unwrap().len(), 15, "the presets from imageTrace.presets");
    assert!(trace["inputs"].as_array().unwrap().iter().any(|p| p["name"] == "select" && p["default"] == "all"));
    assert_eq!(trace["memo"]["content"], true);
    let pdf: plumcraft_proto::catalogue::NodeDef = serde_json::from_value(node("vc.export_pdf")).unwrap();
    assert_eq!(pdf.memo.scope, MemoScope::None);
    let blur = node("vc.fx.blur.gaussian");
    assert!(blur["inputs"].as_array().unwrap().iter().any(|p| p["name"] == "radius" && p["type"] == "FLOAT"), "{blur}");
}

/// The exit of M2.5: raster → Trace (6 Colors) → Recolor → SVG on the corgi, every state in
/// order, and the SVG byte-identical to the same chain through `vectorcraft-cli run --export`.
fn trace_chain_states_and_bytes(mk: Mk) {
    let Some(env) = Env::new("chain") else { return };
    let h = mk(&env);
    let t0 = Instant::now();
    run(&h, "chain", trace_chain(&env), json!({"targets": ["t", "c", "s"], "watch": [{"node": "s", "port": "svg"}]}));
    let (events, reply) = collect(&h, "chain", 0);
    eprintln!("the chain took {:?}", t0.elapsed());
    assert!(reply.ok, "{reply:?}");
    assert_eq!(reply.order, ["o", "t", "c", "s"]);
    let pos = |node: &str, state: NodeState| {
        events.iter().position(|e| matches!(&e.event, Event::Status(r) if r.node_id == node && r.state == state)).unwrap_or_else(|| panic!("{node} {state:?}"))
    };
    for n in ["o", "t", "c", "s"] {
        assert!(pos(n, NodeState::Started) < pos(n, NodeState::Preview) && pos(n, NodeState::Preview) < pos(n, NodeState::Done), "{n}");
    }
    assert!(pos("t", NodeState::Preview) < pos("c", NodeState::Started), "the preview precedes downstream start");
    assert!(events.iter().any(|e| matches!(&e.event, Event::Status(r) if r.node_id == "t" && r.state == NodeState::Progress)), "the adapter's own phases");
    // The doc: a content handle, published to commonkit like any value the UI sees (so the
    // store's budget and TTL govern it; the CAS file is the craft's working copy).
    let doc = blob(&reply, "t", "doc");
    assert!(doc.handle.as_ref().unwrap().to_string().starts_with("md5:raw:"));
    assert_eq!(doc.meta.form, Form::Doc { app: "vectorcraft".into() });
    assert!(doc.at.iter().any(|l| matches!(l, Locator::Commonkit { .. })), "{:?}", doc.at);
    let saved = h.resolve(&doc).expect("the doc resolves from the store").bytes;
    assert_eq!(Handle::md5_raw(&saved), *doc.handle.as_ref().unwrap(), "the handle is the MD5 of the native save");
    assert!(matches!(reply.nodes["t"].thumb, Some(Rendition::Rgba8 { w: 120, .. } | Rendition::Rgba8 { h: 120, .. })), "{:?}", reply.nodes["t"].thumb);
    assert_eq!(json_out(&reply, "t", "paths"), json!(1097));
    // The SVG: published, resolvable, and the CLI's bytes.
    let svg = blob(&reply, "s", "svg");
    assert_eq!(svg.meta.media, "image/svg+xml");
    assert!(matches!(svg.meta.form, Form::Vector { width, height } if width > 800.0 && height > 800.0), "{:?}", svg.meta.form);
    let bytes = h.resolve(&svg).expect("the SVG resolves").bytes.into_vec();
    let cli = cli_trace_chain(&env);
    assert_eq!(bytes.len(), cli.len());
    assert!(bytes == cli, "the SVG differs from vectorcraft-cli run's");
    eprintln!("SVG byte-identical to vectorcraft-cli run: {} bytes, {}", bytes.len(), Handle::md5_raw(&bytes));
    let d = &reply.watch[0];
    assert_eq!(d.media, Media::Vector);
    assert_eq!(d.full.as_ref().and_then(|f| f.handle.clone()), svg.handle);
}

/// A second identical run with the memo on is all `cached`, nothing started, the same handles.
fn a_cached_rerun_is_all_cached(mk: Mk) {
    let Some(env) = Env::new("memo") else { return };
    let h = mk(&env);
    let g = trace_chain(&env);
    let first = go(&h, "m1", g.clone(), &["t", "s"], "use");
    assert!(first.ok && first.nodes.values().all(|n| n.state == NodeState::Done), "{:?}", states(&first));
    run(&h, "m2", g, json!({"targets": ["t", "s"], "memo": "use"}));
    let t = Instant::now();
    let (events, again) = collect(&h, "m2", 0);
    assert!(again.nodes.values().all(|n| n.state == NodeState::Cached), "{:?}", states(&again));
    assert!(!events.iter().any(|e| matches!(&e.event, Event::Status(r) if r.state == NodeState::Started)));
    assert_eq!(blob(&again, "s", "svg").handle, blob(&first, "s", "svg").handle);
    assert_eq!(blob(&again, "t", "doc").handle, blob(&first, "t", "doc").handle);
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
}

/// The same graph twice with the memo off: the same handles for every doc and the SVG.
fn handles_are_stable(mk: Mk) {
    let Some(env) = Env::new("stable") else { return };
    let h = mk(&env);
    let handles = |id: &str| {
        let r = go(&h, id, trace_chain(&env), &["o", "t", "c", "s"], "off");
        assert!(r.ok, "{r:?}");
        (blob(&r, "o", "doc").handle, blob(&r, "t", "doc").handle, blob(&r, "c", "doc").handle, blob(&r, "s", "svg").handle)
    };
    let a = handles("h1");
    std::thread::sleep(Duration::from_millis(1100)); // past a second: the save dates would show
    assert_eq!(a, handles("h2"));
}

/// Cancel mid-trace: High Fidelity Photo on the 4× corgi ignores MCP cancel, so the adapter
/// kills the process (D41). The run ends `cancelled` within 500 ms (spec 19 case 6) and the
/// next run works.
fn cancel_mid_trace(mk: Mk) {
    let Some(env) = Env::new("cancel") else { return };
    let h = mk(&env);
    run(&h, "c1", long_trace(&env), json!({"targets": ["g_svg"]}));
    let since = wait_for(&h, "c1", "d_trace", NodeState::Started, Duration::from_secs(60));
    let busy = env.craft_pids();
    std::thread::sleep(Duration::from_millis(400));
    let t = Instant::now();
    call(&h, "run.cancel", json!({"run_id": "c1"}));
    let (_, reply) = collect(&h, "c1", since);
    let took = t.elapsed();
    eprintln!("cancel mid-trace: reply {took:?} after run.cancel");
    assert!(took < Duration::from_millis(500), "cancel took {took:?}");
    assert_eq!(reply.error.as_ref().unwrap().kind, "cancelled");
    assert_eq!(reply.nodes["d_trace"].state, NodeState::Cancelled);
    assert_eq!(reply.nodes["g_svg"].state, NodeState::Cancelled);
    std::thread::sleep(Duration::from_millis(100));
    let after = env.craft_pids();
    assert!(busy.iter().any(|p| !after.contains(p)), "the tracing process was killed: {busy:?} → {after:?}");
    let r = go(&h, "after", chain(&[("o", "vc.open", json!({"file": env.corgi_str()})), ("k", "vc.colors", json!({}))]), &["k"], "off");
    assert!(r.ok, "{r:?}");
}

/// D46: `kill -9` of the craft mid-node fails that node with kind `engine`, skips what is below
/// it, lets an independent branch run, and the next run works.
fn a_crash_fails_only_that_node(mk: Mk) {
    let Some(env) = Env::new("crash") else { return };
    let h = mk(&env);
    run(&h, "k1", long_trace(&env), json!({"targets": ["g_svg", "f_colors"]}));
    let since = wait_for(&h, "k1", "d_trace", NodeState::Started, Duration::from_secs(60));
    std::thread::sleep(Duration::from_millis(400));
    let pids = env.craft_pids();
    assert!(!pids.is_empty());
    let t = Instant::now();
    for p in &pids {
        let _ = std::process::Command::new("kill").args(["-9", &p.to_string()]).status();
    }
    let (events, reply) = collect(&h, "k1", since);
    let failed_at = events.iter().find(|e| matches!(&e.event, Event::Status(r) if r.node_id == "d_trace" && r.state == NodeState::Error));
    assert!(failed_at.is_some());
    eprintln!("crash: run reply {:?} after kill -9", t.elapsed());
    let err = reply.nodes["d_trace"].error.clone().unwrap();
    assert_eq!(err.kind, "engine", "{err:?}");
    assert!(err.message.contains("stopped"), "{}", err.message);
    assert_eq!(reply.nodes["g_svg"].state, NodeState::Skipped);
    assert_eq!(reply.nodes["f_colors"].state, NodeState::Done, "the independent branch ran: {:?}", reply.nodes["f_colors"]);
    assert!(reply.error.is_none(), "a craft crash is a node error, not a run error (D46)");
    let r = go(&h, "k2", chain(&[("o", "vc.open", json!({"file": env.corgi_str()})), ("t", "vc.trace", json!({"preset": "3 Colors"}))]), &["t"], "off");
    assert!(r.ok, "{r:?}");
}

/// Errors are data: an unknown preset is `config` (the craft's message kept), downstream
/// skipped; an empty selection is `config` naming `select`.
fn errors_are_data(mk: Mk) {
    let Some(env) = Env::new("errors") else { return };
    let h = mk(&env);
    let g = graph(
        json!({
            "o": {"kind": "vc.open", "values": {"file": env.corgi_str()}},
            "t": {"kind": "vc.trace", "values": {"preset": "No Such Preset"}},
            "s": {"kind": "vc.export_svg"},
            "z": {"kind": "vc.simplify", "values": {"select": "color:#123456"}},
            "w": {"kind": "vc.run", "values": {"command": "object.path.simplify", "params": {"levl": 3}}},
            "x": {"kind": "vc.run", "values": {"command": "file.package", "params": {}}}
        }),
        &[("o", "doc", "t", "doc"), ("t", "doc", "s", "doc"), ("o", "doc", "z", "doc"), ("o", "doc", "w", "doc"), ("o", "doc", "x", "doc")],
    );
    let r = go(&h, "e", g, &["s", "z", "w", "x"], "off");
    assert!(!r.ok);
    let kind = |n: &str| r.nodes[n].error.as_ref().map(|e| (e.kind.clone(), e.message.clone()));
    assert_eq!(kind("t").unwrap().0, "config", "{:?}", kind("t"));
    assert_eq!(r.nodes["s"].state, NodeState::Skipped);
    let (k, m) = kind("z").unwrap();
    assert!(k == "config" && m.contains("select"), "{k}: {m}");
    let (k, m) = kind("w").unwrap();
    assert!(k == "config" && m.contains("levl"), "{k}: {m}");
    let (k, m) = kind("x").unwrap();
    assert!(k == "config" && m.contains("not one of the commands"), "the escape hatch refuses commands off its allowlist: {k}: {m}");
}

macro_rules! modes {
    ($($case:ident),* $(,)?) => {
        mod mode1_inproc {
            $(#[test] fn $case() { super::$case(super::mode1) })*
        }
        mod mode2_process {
            $(#[test] fn $case() { super::$case(super::mode2) })*
        }
    };
}

modes!(
    hello_and_schema,
    trace_chain_states_and_bytes,
    a_cached_rerun_is_all_cached,
    handles_are_stable,
    cancel_mid_trace,
    a_crash_fails_only_that_node,
    errors_are_data
);

/// Open question 2 of note 14: a warm document (the process still holds what the last node
/// saved) gives the same bytes as a cold reopen of the saved file.
#[test]
fn warm_equals_cold() {
    let Some(env) = Env::new("warm") else { return };
    let (h, slot) = inproc(&env);
    let g = trace_chain(&env);
    let warm = go(&h, "w", g.clone(), &["c", "s"], "off");
    assert!(warm.ok);
    // Cold: the trace from the memo, then every process killed, so the recolour reopens it.
    let base = chain(&[("o", "vc.open", json!({"file": env.corgi_str()})), ("t", "vc.trace", json!({"preset": "6 Colors"}))]);
    assert!(go(&h, "b", base, &["t"], "use").ok);
    let shared = slot.lock().unwrap().as_ref().unwrap().0.clone();
    shared.pool.shutdown();
    let spawned = shared.pool.spawned();
    let cold = go(&h, "c", g, &["c", "s"], "use");
    assert!(cold.ok, "{cold:?}");
    assert!(shared.pool.spawned() > spawned, "the recolour ran in a fresh process");
    assert_eq!(blob(&cold, "c", "doc").handle, blob(&warm, "c", "doc").handle);
    assert_eq!(blob(&cold, "s", "svg").handle, blob(&warm, "s", "svg").handle);
}

/// The prose grammar holds for every command VectorCraft lists (note 14, M2.4), and reports how
/// many keys it types.
#[test]
fn the_grammar_parses_every_command() {
    let Some(env) = Env::new("grammar") else { return };
    let (_, report, _) = plumcraft_craft::build(&env.manifest, &env.options()).unwrap();
    assert!(report.commands.len() >= 800, "{}", report.commands.len());
    let (mut keys, mut typed) = (0, 0);
    for (id, doc) in &report.commands {
        let k = plumcraft_craft::grammar::doc_keys(doc).unwrap_or_else(|e| panic!("{id}: {e}"));
        for (name, _, text) in &k.keys {
            keys += 1;
            // Targets and I/O keys are the adapter's own (note 14, 2.4: `select`, staged files).
            let owned = matches!(*name, "id" | "ids" | "path" | "folder" | "dataBase64" | "files");
            if owned || !plumcraft_craft::grammar::infer(text).ty.is_empty() {
                typed += 1;
            }
        }
    }
    eprintln!("{} commands, {keys} top-level keys, {typed} typed or owned by the adapter ({:.0} %)", report.commands.len(), 100.0 * typed as f64 / keys as f64);
    assert!(typed * 2 > keys);
    assert!(report.dropped.is_empty(), "{:?}", report.dropped);
}

/// R6 (note 14, 1.7): the >120 s stall after a late cancel. Reproduce the M1 sequence on the
/// raw client (a big HFP trace, a cancel naming the finished request, then document inspects)
/// and bound every call: the adapter never waits longer than its timeout, and kills a process
/// that doesn't answer.
#[test]
fn a_late_cancel_never_stalls_unbounded() {
    use plumcraft_craft::mcp::{Client, NoWatch};
    let Some(env) = Env::new("stall") else { return };
    let cas = plumcraft_craft::cas::Cas::new(&env.root).unwrap();
    let (_, src) = cas.stage_file(&env.corgi).unwrap();
    let mut cmd = env.manifest.program();
    cmd.extend([
        "--automation-read-root".into(),
        cas.read_root().display().to_string(),
        "--automation-write-root".into(),
        cas.write_root().display().to_string(),
    ]);
    let mut c = Client::spawn(&cmd, &[("SOURCE_DATE_EPOCH".into(), "0".into())]).unwrap();
    c.initialize(Duration::from_secs(20)).unwrap();
    let t = Duration::from_secs(60);
    let run = |c: &Client, id: &str, p: Value| c.tool("command_run", json!({"id": id, "params": p}), t, &mut NoWatch).unwrap();
    let big = cas.tmp_dir().unwrap().join("big.png");
    run(&c, "document.open", json!({"path": src.display().to_string()}));
    run(&c, "document.export", json!({"path": big.display().to_string(), "format": "png", "scale": 4.0}));
    let (_, big_src) = cas.stage_file(&big).unwrap();
    run(&c, "file.closeAll", json!({}));
    run(&c, "document.open", json!({"path": big_src.display().to_string()}));
    run(&c, "select.all", json!({}));
    let t0 = Instant::now();
    let traced = run(&c, "imageTrace.makeAndExpand", json!({"preset": "High Fidelity Photo"}));
    eprintln!("HFP trace of the 4× corgi: {:?}, {}", t0.elapsed(), traced.text.chars().take(120).collect::<String>());
    // The late cancel: names a request that has already been answered.
    c.notify("notifications/cancelled", json!({"requestId": 9, "reason": "late"})).unwrap();
    for i in 0..2 {
        let t1 = Instant::now();
        let r = c.tool("doc_inspect", json!({"depth": 1}), Duration::from_secs(30), &mut NoWatch);
        eprintln!("doc_inspect {i} after the late cancel: {:?} in {:?}", r.as_ref().map(|x| x.text.len()).map_err(|e| e.to_string()), t1.elapsed());
        assert!(t1.elapsed() < Duration::from_secs(31), "bounded by the call timeout");
        if r.is_err() {
            assert!(c.is_dead(), "a process that didn't answer is killed");
            return;
        }
    }
    assert!(c.ping(Duration::from_secs(2)));
}

/// Genericity (note 14, 3.10, M2.6): the SoundCraft manifest drives SoundCraft's own tool names
/// through the same client, pool, CAS and nodes: it lists SoundCraft's commands and runs
/// `session.new` end to end, then a query on the saved session.
#[test]
fn soundcraft_runs_from_its_manifest() {
    let Some(m) = soundcraft() else { return };
    let root = craft_root("soundcraft");
    let opts = plumcraft_craft::Options { root: Some(root), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(&m, &opts).unwrap();
    eprintln!("SoundCraft {}: {} commands listed; nodes {:?}; dropped {:?}", report.server_version, report.commands.len(), report.curated, report.dropped);
    assert!(report.commands.len() > 100 && report.commands.contains_key("session.new"));
    // The pack grew past the genericity smoke test (2026-10-28, pack_soundcraft.rs covers the rest).
    assert!(report.curated.iter().take(2).eq(["sc.new", "sc.inspect"].iter()), "{:?}", report.curated);
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(30));
    let h: Conn = Box::new(plumcraft_host::InProc(h));
    let r = go(&h, "sc", chain(&[("n", "sc.new", json!({"sample_rate": 44100})), ("i", "sc.inspect", json!({}))]), &["n", "i"], "off");
    assert!(r.ok, "{r:?}");
    let doc = blob(&r, "n", "doc");
    assert_eq!(doc.meta.form, Form::Doc { app: "soundcraft".into() });
    assert_eq!(json_out(&r, "i", "result")["sample_rate"], 44100);
    let again = go(&h, "sc2", chain(&[("n", "sc.new", json!({"sample_rate": 44100}))]), &["n"], "off");
    assert_eq!(blob(&again, "n", "doc").handle, doc.handle, "a SoundCraft session saves byte-stable too");
}
