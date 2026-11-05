//! The generic conformance cases (spec 19): what any engine must do on any binding, driven only
//! through [`Connection::call`], so every request and reply crosses JSON. A case knows a
//! [`Fixture`], not an engine: two graphs and a way to connect. The text engine's suite
//! (`crates/text/tests/conformance.rs`) runs these with its own fixture, and adds what only it can
//! show; `plumcraft conform --engine-manifest FILE` runs them against any engine, with graphs
//! built from its catalogue ([`Graphs::from_catalogue`]).
//!
//! The cases return why they failed, rather than panic: this is a library, and a failing engine
//! is an answer, not a crash.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use plumcraft_engine::host::PollReply;
use plumcraft_host::Connection;
use plumcraft_host::http::{HttpHost, HttpOptions};
use plumcraft_host::process::{Manifest, ProcessHost};
use plumcraft_proto::catalogue::{NodeDef, Port, SchemaReply, Socket};
use plumcraft_proto::event::{Envelope, Event, NodeState};
use plumcraft_proto::reducer::Reducer;
use plumcraft_proto::run::{Phase, RunAck, RunReply, RunSnapshot};
use plumcraft_proto::session::{EngineState, HelloReply};
use serde_json::{Value, json};

pub type Conn = Box<dyn Connection>;
pub type Outcome = Result<(), String>;

/// How long a run may take in a case.
const REPLY_WAIT: Duration = Duration::from_secs(20);
/// How long an engine may take to be ready: uv may have an environment to build.
const READY_WAIT: Duration = Duration::from_secs(60);

macro_rules! ensure {
    ($cond:expr, $($why:tt)*) => {
        if !$cond {
            return Err(format!($($why)*));
        }
    };
}

/// The graphs the cases run.
pub struct Graphs {
    /// Three nodes `a → b → c`, each finishing at once. `b`'s output is watched.
    pub chain: Value,
    /// `b`'s output port.
    pub watch_port: String,
    /// `a → s → z`, where `s` takes this many seconds, reporting progress and honouring cancel.
    pub slow: Box<dyn Fn(f64) -> Value + Send + Sync>,
}

/// An engine to check: a way to connect, fresh for each case, and its graphs.
pub struct Fixture {
    pub connect: Box<dyn Fn() -> Result<Conn, String> + Send + Sync>,
    pub graphs: Graphs,
}

/// A workflow from its nodes and wires.
pub fn graph(nodes: Value, wires: &[(&str, &str, &str, &str)]) -> Value {
    let wires: Vec<Value> = wires.iter().map(|(a, ap, b, bp)| json!({"from": {"node": a, "port": ap}, "to": {"node": b, "port": bp}})).collect();
    json!({"version": 1, "nodes": nodes, "wires": wires, "frames": {}, "next_id": 9})
}

impl Graphs {
    /// The graphs for an engine, from its catalogue and its slow node (one with a `seconds`
    /// input, and an input of the type it outputs). A source is any node that makes that type
    /// from its widgets alone. Say what's missing rather than guess.
    pub fn from_catalogue(schema: &SchemaReply, slow: &str) -> Result<Graphs, String> {
        let def = schema.nodes.iter().find(|n| n.id == slow).ok_or_else(|| format!("no node {slow}"))?;
        ensure!(def.inputs.iter().any(|p| p.name == "seconds"), "{slow} has no seconds input");
        let out = def.outputs.first().ok_or_else(|| format!("{slow} has no output"))?;
        let input = def.inputs.iter().find(|p| p.ty == out.ty).ok_or_else(|| format!("{slow} has no input of its output's type, {}", out.ty))?;
        let source = schema
            .nodes
            .iter()
            .filter(|n| n.id != slow)
            .find_map(|n| source_port(n, &out.ty).map(|p| (n.id.clone(), p)))
            .ok_or_else(|| format!("no node makes a {} from its widgets alone", out.ty))?;
        let (source, source_port) = source;
        let (slow, out, input) = (slow.to_owned(), out.name.clone(), input.name.clone());
        let shape = move |first: &str, mid: &str, last: &str, seconds: f64| {
            let nodes = json!({first: {"kind": source, "values": {}}, mid: {"kind": slow, "values": {"seconds": seconds}}, last: {"kind": slow, "values": {"seconds": 0.0}}});
            graph(nodes, &[(first, &source_port, mid, &input), (mid, &out, last, &input)])
        };
        let chain = shape("a", "b", "c", 0.0);
        let watch_port = def.outputs.first().map_or_else(String::new, |p| p.name.clone());
        Ok(Graphs { chain, watch_port, slow: Box::new(move |s| shape("a", "s", "z", s)) })
    }
}

/// The output of `n` of type `ty`, if `n` runs on its widgets alone: no wires, and no file to
/// choose, which a default doesn't give.
fn source_port(n: &NodeDef, ty: &str) -> Option<String> {
    let wired = |p: &Port| p.socket == Some(Socket::Socket) && !p.optional && p.default.is_null();
    if n.inputs.iter().any(|p| wired(p) || p.path.is_some()) {
        return None;
    }
    n.outputs.iter().find(|p| p.ty == ty).map(|p| p.name.clone())
}

fn call(h: &dyn Connection, method: &str, params: Value) -> Result<Value, String> {
    h.call(method, params).map_err(|e| format!("{method}: {e}"))
}

fn parse<T: serde::de::DeserializeOwned>(what: &str, v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("{what}: {e}"))
}

/// Send a run. Conformance runs with the memo off (spec 14.5) unless a case asks for it.
pub fn run(h: &dyn Connection, id: &str, generation: u64, g: Value, extra: Value) -> Result<RunAck, String> {
    let mut req = json!({"v": 2, "op": "run", "run_id": id, "generation": generation, "graph": {"kind": "inline", "graph": g}, "memo": "off"});
    if let (Some(r), Some(x)) = (req.as_object_mut(), extra.as_object()) {
        r.extend(x.clone());
    }
    parse("the run's ack", call(h, "run", req)?)
}

/// Poll until the run's reply arrives, returning every envelope seen.
pub fn collect(h: &dyn Connection, run_id: &str, since: u64) -> Result<(Vec<Envelope>, RunReply), String> {
    let deadline = Instant::now() + REPLY_WAIT;
    let mut since = since;
    let mut all = Vec::new();
    while Instant::now() < deadline {
        let p: PollReply = parse("events.poll", call(h, "events.poll", json!({"since": since, "wait_ms": 500, "run_id": run_id}))?)?;
        since = p.last_seq;
        for e in p.events {
            let reply = match &e.event {
                Event::RunReply(r) if r.run_id == run_id => Some((**r).clone()),
                _ => None,
            };
            all.push(e);
            if let Some(r) = reply {
                return Ok((all, r));
            }
        }
    }
    Err(format!("no reply for {run_id} in {REPLY_WAIT:?}"))
}

/// The state a client reaches from these events.
pub fn reduce(run_id: &str, generation: u64, events: &[Envelope]) -> Reducer {
    let mut r = Reducer::new(run_id, generation);
    for e in events {
        match &e.event {
            Event::Status(rec) => {
                r.apply(e.run_id.as_deref().unwrap_or(""), e.generation.unwrap_or(0), rec);
            }
            Event::RunReply(reply) => {
                r.finish(reply);
            }
            _ => {}
        }
    }
    r
}

fn position(events: &[Envelope], node: &str, state: NodeState) -> Result<usize, String> {
    events.iter().position(|e| matches!(&e.event, Event::Status(r) if r.node_id == node && r.state == state)).ok_or_else(|| format!("no {state:?} for {node}"))
}

/// Spec 19 items 1 and 2: `hello` negotiates and refuses a version; the catalogue has nodes; an
/// unknown op is refused. The catalogue, for what a suite wants to add.
pub fn hello_and_schema(fx: &Fixture) -> Result<Value, String> {
    let h = (fx.connect)()?;
    let hello = json!({"v": 2, "op": "hello", "protocol": {"min": 2, "max": 2}, "caps": ["events.push", "events.poll", "status.progress", "display.stream"]});
    let reply: HelloReply = parse("hello", call(&h, "hello", hello)?)?;
    ensure!(reply.protocol == 2, "hello chose protocol {}", reply.protocol);
    ensure!(reply.caps.has("events.poll") && reply.caps.has("status.progress") && reply.caps.has("display.stream"), "hello lost a capability both have");
    ensure!(!reply.caps.has("events.resume"), "hello gave a capability the client didn't list");
    match h.call("hello", json!({"v": 2, "op": "hello", "protocol": {"min": 3, "max": 3}})) {
        Ok(_) => return Err("hello for protocol 3 only was accepted".into()),
        Err(e) => ensure!(e.code.code() == -32000, "a version mismatch is error `version`, not {}", e.code.code()),
    }
    let schema = call(&h, "schema", json!({}))?;
    ensure!(schema["nodes"].as_array().is_some_and(|n| !n.is_empty()), "the catalogue has no nodes");
    match h.call("bogus.op", json!({})) {
        Ok(_) => return Err("an unknown op was accepted".into()),
        Err(e) => ensure!(e.code.code() == -32601, "an unknown op is -32601, not {}", e.code.code()),
    }
    Ok(schema)
}

/// Spec 19 item 4: envelope seq rising, every node terminal, generation echoed, preview before
/// downstream started, ms on done; the watched display on the preview record before the reply.
/// The reply, for what a suite wants to add.
pub fn record_invariants(fx: &Fixture) -> Result<RunReply, String> {
    let h = (fx.connect)()?;
    let ack = run(&h, "r1", 7, fx.graphs.chain.clone(), json!({"targets": ["c"], "watch": [{"node": "b", "port": fx.graphs.watch_port}]}))?;
    ensure!(ack.accepted, "the run wasn't accepted");
    let (events, reply) = collect(&h, "r1", 0)?;
    ensure!(reply.ok, "{reply:?}");
    ensure!(reply.order == ["a", "b", "c"], "order {:?}", reply.order);
    ensure!(events.windows(2).all(|w| matches!(w, [a, b] if a.seq < b.seq)), "envelope seq isn't strictly rising");
    for n in ["a", "b", "c"] {
        ensure!(position(&events, n, NodeState::Started)? < position(&events, n, NodeState::Preview)?, "{n}: preview before started");
        ensure!(position(&events, n, NodeState::Preview)? < position(&events, n, NodeState::Done)?, "{n}: done before preview");
        let timed = events.iter().any(|e| matches!(&e.event, Event::Status(r) if r.node_id == n && r.state == NodeState::Done && r.ms.is_some()));
        ensure!(timed, "{n}: no ms on done");
    }
    ensure!(position(&events, "a", NodeState::Preview)? < position(&events, "b", NodeState::Started)?, "preview must precede downstream started");
    let watched = events.iter().find_map(|e| match &e.event {
        Event::Status(r) if r.node_id == "b" && r.state == NodeState::Preview => r.display.first().cloned(),
        _ => None,
    });
    let streamed = watched.ok_or("no watched display on b's preview")?;
    let last = reply.watch.first().ok_or("no watched display in the reply")?;
    ensure!(streamed.renditions.first() == last.renditions.first(), "the display streamed isn't the reply's");
    ensure!(events.iter().filter(|e| e.run_id.is_some()).all(|e| e.generation == Some(7)), "the generation isn't echoed");
    ensure!(reduce("r1", 7, &events).nodes().values().all(|n| n.state.is_terminal()), "a node ended unfinished");
    Ok(reply)
}

/// Spec 5.5 and 19 item 5: during a run, run.get, events.poll and ping answer within 200 ms;
/// progress is visible mid-run.
pub fn off_the_run_path(fx: &Fixture) -> Outcome {
    let h = (fx.connect)()?;
    run(&h, "long", 1, (fx.graphs.slow)(1.5), json!({}))?;
    std::thread::sleep(Duration::from_millis(400));
    for (method, params) in [
        ("run.get", json!({"run_id": "long"})),
        ("events.poll", json!({"since": 0, "wait_ms": 0})),
        ("ping", json!({})),
        ("hello", json!({"v": 2, "op": "hello"})),
        ("schema", json!({})),
    ] {
        let t = Instant::now();
        call(&h, method, params)?;
        ensure!(t.elapsed() < Duration::from_millis(200), "{method} took {:?}", t.elapsed());
    }
    let snap: RunSnapshot = parse("run.get", call(&h, "run.get", json!({"run_id": "long"}))?)?;
    ensure!(snap.phase == Phase::Running, "phase {:?} mid-run", snap.phase);
    let s = snap.nodes.get("s").ok_or("run.get has no s")?;
    ensure!(s.state == NodeState::Progress, "s is {:?} mid-run", s.state);
    ensure!(s.fraction.is_some_and(|f| f > 0.0 && f < 1.0) && s.stage.is_some(), "no progress mid-run: {:?} {:?}", s.fraction, s.stage);
    ensure!(collect(&h, "long", 0)?.1.ok, "the run failed");
    Ok(())
}

/// Spec 19 item 6: cancel mid-run ends unreached nodes `cancelled` within 500 ms; idempotent.
pub fn cancel_mid_run(fx: &Fixture) -> Outcome {
    let h = (fx.connect)()?;
    run(&h, "c1", 1, (fx.graphs.slow)(30.0), json!({}))?;
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    call(&h, "run.cancel", json!({"run_id": "c1"}))?;
    call(&h, "run.cancel", json!({"run_id": "c1"}))?;
    let (events, reply) = collect(&h, "c1", 0)?;
    ensure!(t.elapsed() < Duration::from_millis(500), "cancel took {:?}", t.elapsed());
    ensure!(!reply.ok, "a cancelled run replied ok");
    ensure!(reply.error.as_ref().is_some_and(|e| e.kind == "cancelled"), "error {:?}", reply.error);
    for n in ["s", "z"] {
        let state = reply.nodes.get(n).map(|x| x.state);
        ensure!(state == Some(NodeState::Cancelled), "{n} ended {state:?}");
    }
    let z = reduce("c1", 1, &events).node("z").map(|n| n.state);
    ensure!(z == Some(NodeState::Cancelled), "z reduced to {z:?}");
    Ok(())
}

/// A cancel that arrives before its run leaves a tombstone: the run ends without computing.
pub fn cancel_before_run(fx: &Fixture) -> Outcome {
    let h = (fx.connect)()?;
    call(&h, "run.cancel", json!({"run_id": "early"}))?;
    run(&h, "early", 1, (fx.graphs.slow)(30.0), json!({}))?;
    let t = Instant::now();
    let (events, reply) = collect(&h, "early", 0)?;
    ensure!(t.elapsed() < Duration::from_millis(500), "it took {:?}", t.elapsed());
    ensure!(reply.error.is_some_and(|e| e.kind == "cancelled"), "it wasn't cancelled");
    ensure!(!events.iter().any(|e| matches!(&e.event, Event::Status(r) if r.state == NodeState::Started)), "something was computed");
    Ok(())
}

/// Spec 19 item 5: push (every event), long-poll and run.get alone reach the same final state.
pub fn every_delivery_reduces_the_same(fx: &Fixture) -> Outcome {
    let h = (fx.connect)()?;
    run(&h, "d", 4, fx.graphs.chain.clone(), json!({}))?;
    let (events, reply) = collect(&h, "d", 0)?;
    let pushed = reduce("d", 4, &events);
    // Lose every other status record: the authoritative reply still settles it.
    let lossy: Vec<Envelope> = events.iter().enumerate().filter(|(i, e)| i % 2 == 0 || matches!(e.event, Event::RunReply(_))).map(|(_, e)| e.clone()).collect();
    let lossy = reduce("d", 4, &lossy);
    // Polling run.get alone.
    let snap: RunSnapshot = parse("run.get", call(&h, "run.get", json!({"run_id": "d"}))?)?;
    let polled_reply = snap.reply.ok_or("run.get has no reply for a finished run")?;
    let mut polled = Reducer::new("d", 4);
    polled.finish(&polled_reply);
    ensure!(pushed.states() == lossy.states(), "a lossy delivery ended differently");
    ensure!(pushed.states() == polled.states(), "run.get alone ended differently");
    ensure!(pushed.states().values().all(|s| *s == NodeState::Done), "{:?}", pushed.states());
    ensure!(polled_reply == reply, "run.get's reply isn't the pushed one");
    Ok(())
}

/// Resume: polling from a remembered seq returns only what came after it.
pub fn resume_from_seq(fx: &Fixture) -> Outcome {
    let h = (fx.connect)()?;
    run(&h, "p1", 1, fx.graphs.chain.clone(), json!({}))?;
    let (first, _) = collect(&h, "p1", 0)?;
    let mark = first.last().ok_or("the first run sent nothing")?.seq;
    run(&h, "p2", 2, fx.graphs.chain.clone(), json!({}))?;
    let (second, _) = collect(&h, "p2", mark)?;
    ensure!(second.iter().all(|e| e.seq > mark), "an event from before the mark came back");
    ensure!(second.iter().all(|e| e.run_id.as_deref() == Some("p2")), "another run's event came back");
    Ok(())
}

/// Spec 19 item 7: a second identical run is `cached` everywhere and starts nothing.
pub fn memo_reuse(fx: &Fixture) -> Outcome {
    let h = (fx.connect)()?;
    run(&h, "m1", 1, fx.graphs.chain.clone(), json!({"memo": "use"}))?;
    let (_, first) = collect(&h, "m1", 0)?;
    ensure!(first.ok, "{first:?}");
    run(&h, "m2", 2, fx.graphs.chain.clone(), json!({"memo": "use"}))?;
    let (events, again) = collect(&h, "m2", 0)?;
    ensure!(
        again.nodes.values().all(|n| n.state == NodeState::Cached),
        "not all cached: {:?}",
        again.nodes.iter().map(|(k, n)| (k, n.state)).collect::<Vec<_>>()
    );
    ensure!(!events.iter().any(|e| matches!(&e.event, Event::Status(r) if r.state == NodeState::Started)), "a hit was started");
    Ok(())
}

/// Spec 19 item 14: `kill -9` mid-run gives the run in flight a failed reply (`kind: engine`), the
/// host reports the engine down, restarts it, reloads its schema, and the next run works.
pub fn kill_9_mid_run_recovers(h: &ProcessHost, graphs: &Graphs) -> Outcome {
    let first = h.pid();
    run(h, "doomed", 1, (graphs.slow)(30.0), json!({}))?;
    std::thread::sleep(Duration::from_millis(300));
    h.kill_engine();
    let t = Instant::now();
    let (events, reply) = collect(h, "doomed", 0)?;
    ensure!(t.elapsed() < Duration::from_secs(2), "the failure took {:?}", t.elapsed());
    let err = reply.error.ok_or("no error in the reply")?;
    ensure!(err.kind == "engine", "kind {}", err.kind);
    // SIGKILL on Unix; Windows has no signals, and `taskkill /F` ends a process with exit code 1
    let killed = if cfg!(windows) { "exit 1" } else { "signal 9" };
    ensure!(err.message.contains(killed), "{}", err.message);
    let running = events.iter().any(|e| matches!(&e.event, Event::Status(r) if r.node_id == "s" && r.state == NodeState::Progress));
    ensure!(running, "it wasn't really running");
    // run.get still answers for the dead run, from the host.
    let snap: RunSnapshot = parse("run.get", call(h, "run.get", json!({"run_id": "doomed"}))?)?;
    ensure!(snap.phase == Phase::Failed, "phase {:?}", snap.phase);
    // Restarted: a new process, connected, with its schema.
    let state = h.wait_ready(READY_WAIT);
    let EngineState::Connected { nodes, .. } = state else { return Err(format!("not restarted: {state:?}")) };
    ensure!(nodes >= 1, "restarted with no nodes");
    ensure!(h.pid() != first, "not a new process");
    run(h, "after", 2, graphs.chain.clone(), json!({}))?;
    ensure!(collect(h, "after", 0)?.1.ok, "the restarted engine doesn't run");
    Ok(())
}

/// Spec 16.4 and 18: every request but `health` needs the token; a wrong one is refused.
pub fn a_wrong_token_is_refused(good: &HttpHost, cas: PathBuf) -> Outcome {
    match HttpHost::attach(good.base(), "not-the-token", HttpOptions { remote: true, cas }) {
        Ok(_) => Err("a wrong token was accepted".into()),
        Err(e) => {
            ensure!(e.contains("token"), "{e}");
            Ok(())
        }
    }
}

// ---------- every case, against an engine a manifest starts ----------

/// One case's result.
#[derive(Debug, Clone)]
pub struct Verdict {
    pub case: String,
    pub outcome: Outcome,
}

/// A binding `plumcraft conform` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// Mode 2.
    Process,
    /// Mode 3.
    Http,
}

impl Binding {
    pub fn name(self) -> &'static str {
        match self {
            Binding::Process => "process",
            Binding::Http => "http",
        }
    }
}

/// `m` with a private store, kept out of the machine's shared one and of its free-space floor.
fn private(m: &Manifest) -> Manifest {
    let mut m = m.clone();
    m.store = Some(plumcraft_scratch::dir("conform"));
    m.env.push(("COMMONKIT_STORE_MIN_FREE_GB".into(), "0".into()));
    m
}

fn spawn(m: &Manifest) -> Result<ProcessHost, String> {
    let h = ProcessHost::start(private(m)).map_err(|e| e.to_string())?;
    let state = h.wait_ready(READY_WAIT);
    ensure!(matches!(state, EngineState::Connected { .. }), "{state:?}");
    Ok(h)
}

fn serve(m: &Manifest) -> Result<HttpHost, String> {
    HttpHost::spawn(&private(m), HttpOptions { remote: true, cas: plumcraft_scratch::dir("conform-cas") })
}

/// The graphs for the engine `m` starts.
pub fn graphs_for(m: &Manifest, slow: &str) -> Result<Graphs, String> {
    let h = spawn(m)?;
    let schema: SchemaReply = parse("schema", call(&h, "schema", json!({}))?)?;
    Graphs::from_catalogue(&schema, slow)
}

/// Every generic case against the engine `m` starts, on `binding`. `slow` names its slow node.
pub fn check(m: &Manifest, slow: &str, binding: Binding) -> Result<Vec<Verdict>, String> {
    let m2 = m.clone();
    let connect: Box<dyn Fn() -> Result<Conn, String> + Send + Sync> = match binding {
        Binding::Process => Box::new(move || spawn(&m2).map(|h| Box::new(h) as Conn)),
        Binding::Http => Box::new(move || serve(&m2).map(|h| Box::new(h) as Conn)),
    };
    let fx = Fixture { connect, graphs: graphs_for(m, slow)? };
    let mut verdicts = Vec::new();
    let mut add = |case: &str, outcome: Outcome| verdicts.push(Verdict { case: format!("{}: {case}", binding.name()), outcome });
    add("hello and schema", hello_and_schema(&fx).map(|_| ()));
    add("record invariants", record_invariants(&fx).map(|_| ()));
    add("answers off the run path", off_the_run_path(&fx));
    add("cancel mid-run", cancel_mid_run(&fx));
    add("cancel before its run", cancel_before_run(&fx));
    add("every delivery reduces the same", every_delivery_reduces_the_same(&fx));
    add("resume from seq", resume_from_seq(&fx));
    add("memo reuse", memo_reuse(&fx));
    match binding {
        Binding::Process => add("kill -9 mid-run recovers", spawn(m).and_then(|h| kill_9_mid_run_recovers(&h, &fx.graphs))),
        Binding::Http => add("a wrong token is refused", serve(m).and_then(|h| a_wrong_token_is_refused(&h, plumcraft_scratch::dir("conform-cas-wrong")))),
    }
    Ok(verdicts)
}
