//! `plumcraft`: run a workflow on the text engine and watch what crosses the interface.
//!
//! ```text
//! plumcraft run [WORKFLOW.json] [ENGINE] [--watch NODE.PORT]... [--cancel-after SECONDS] [--raw]
//! plumcraft nodes [ENGINE]
//! plumcraft conform --engine-manifest FILE [--modes process,http]
//!
//! ENGINE: (none) compiled in · --process · --python · --http [--python] · --attach http://HOST:PORT
//!         · --engine-manifest FILE [--http]
//! ```
//!
//! `--process` runs the engine as a separate process on a Unix socket (mode 2) instead of
//! compiled in (mode 1); `--python` runs the same engine written in Python (`python/`) as that
//! process, under `uv run`. `--http` starts the engine over HTTP (mode 3) and talks to it as a UI
//! on another machine would: sources upload by handle, values download by URL into the UI's CAS.
//! `--attach` reaches an HTTP engine already running, with `PLUMCRAFT_ENGINE_TOKEN`.
//! `--engine-manifest` starts the engine a manifest names (spec 16.6), on a socket, or over HTTP
//! with `--http`; without a workflow, `run` opens its `demo`. Nothing else changes.
//!
//! `conform` runs the generic conformance cases (spec 19, `plumcraft_conform`) against the engine
//! a manifest starts, on each binding, and exits 1 if one fails.
//!
//! Without a workflow it runs a built-in demo. Every call goes through a `Connection`, whatever
//! the mode; status comes from long-polling `events.poll`, reduced with the normative reducer.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;
use std::time::{Duration, Instant};

use plumcraft_conform::Binding;
use plumcraft_engine::host::{Host, PollReply};
use plumcraft_host::http::{HttpHost, HttpOptions};
use plumcraft_host::manifest::Start;
use plumcraft_host::process::{Manifest, ProcessHost};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::PortValue;
use plumcraft_proto::display::Rendition;
use plumcraft_proto::event::{Event, LiveUpdate, NodeState};
use plumcraft_proto::reducer::Reducer;
use plumcraft_proto::run::RunReply;
use plumcraft_proto::session::{EngineState, HelloReply};
use serde_json::{Value, json};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("nodes") => nodes(&args[1..]),
        Some("run") => run(&args[1..]),
        Some("conform") => conform(&args[1..]),
        _ => {
            eprintln!(
                "usage: plumcraft run [WORKFLOW.json] [ENGINE] [--watch NODE.PORT]... [--cancel-after SECONDS] [--raw]\n       plumcraft nodes [ENGINE]\n       plumcraft conform --engine-manifest FILE [--modes process,http]\n  ENGINE: --process | --python | --http [--python] | --attach http://HOST:PORT | --engine-manifest FILE [--http]"
            );
            ExitCode::from(2)
        }
    }
}

/// Where the text engine runs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    /// Compiled in (mode 1).
    InProc,
    /// A child process on a socket (mode 2): the Rust binary, or the Python engine.
    Socket { python: bool },
    /// A child process over HTTP (mode 3), as a UI on another machine sees it: sources upload,
    /// values download by URL into the UI's CAS.
    Http { python: bool },
    /// An HTTP engine already running, here or elsewhere (`--attach http://host:port`; the
    /// token from `PLUMCRAFT_ENGINE_TOKEN`).
    Attach(String),
    /// The engine an engine manifest names (spec 16.6), on a socket or over HTTP.
    File { path: std::path::PathBuf, http: bool },
}

fn mode(args: &[String]) -> Mode {
    let python = args.iter().any(|a| a == "--python");
    if let Some(file) = args.iter().position(|a| a == "--engine-manifest").and_then(|i| args.get(i + 1)) {
        Mode::File { path: file.into(), http: args.iter().any(|a| a == "--http") }
    } else if let Some(url) = args.iter().position(|a| a == "--attach").and_then(|i| args.get(i + 1)) {
        Mode::Attach(url.clone())
    } else if args.iter().any(|a| a == "--http") {
        Mode::Http { python }
    } else if python || args.iter().any(|a| a == "--process") {
        Mode::Socket { python }
    } else {
        Mode::InProc
    }
}

fn manifest(python: bool) -> Result<Manifest, String> {
    if python {
        let (command, env) = plumcraft_text::python_command();
        return Ok(Manifest {
            name: "text (Python)".into(),
            command,
            env,
            hint: "needs uv (cd python && uv sync), or set PLUMCRAFT_UV".into(),
            store: None,
            cwd: None,
        });
    }
    // The engine binary is built beside this one.
    let exe = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("plumcraft-text-engine");
    Ok(Manifest {
        name: "text".into(),
        command: vec![exe.display().to_string()],
        env: Vec::new(),
        hint: "cargo build -p plumcraft-text".into(),
        store: None,
        cwd: None,
    })
}

fn start(mode: &Mode) -> Result<Box<dyn Connection>, String> {
    let lang = |python: bool| if python { "Python" } else { "Rust" };
    match mode {
        Mode::InProc => {
            let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
            match host.wait_ready(Duration::from_secs(5)) {
                EngineState::Connected { .. } => Ok(Box::new(InProc(host))),
                other => Err(format!("engine not ready: {other:?}")),
            }
        }
        Mode::Socket { python } => {
            let host = ProcessHost::start(manifest(*python)?).map_err(|e| e.to_string())?;
            match host.wait_ready(Duration::from_secs(10)) {
                EngineState::Connected { .. } => {
                    eprintln!("({} engine process {} on a Unix socket)", lang(*python), host.pid().unwrap_or(0));
                    Ok(Box::new(host))
                }
                other => Err(format!("engine not ready: {other:?}")),
            }
        }
        Mode::Http { python } => {
            let host = HttpHost::spawn(&manifest(*python)?, HttpOptions { remote: true, ..Default::default() })?;
            eprintln!("({} engine over HTTP at {}, remote: sources upload, values download)", lang(*python), host.base());
            Ok(Box::new(host))
        }
        Mode::Attach(url) => {
            let token = std::env::var(plumcraft_engine::serve::ENV_TOKEN).unwrap_or_default();
            let host = HttpHost::attach(url, &token, HttpOptions::default())?;
            let how = if host.data_session().shm { "same host" } else { "remote" };
            eprintln!("(attached to {url}, {how})");
            Ok(Box::new(host))
        }
        Mode::File { path, http } => match plumcraft_host::manifest::load(path)?.start {
            Start::Command(m) if *http => {
                let name = m.name.clone();
                let host = HttpHost::spawn(&m, HttpOptions { remote: true, ..Default::default() })?;
                eprintln!("({name} over HTTP at {}, remote: sources upload, values download)", host.base());
                Ok(Box::new(host))
            }
            Start::Command(m) => {
                let name = m.name.clone();
                let host = ProcessHost::start(m).map_err(|e| e.to_string())?;
                // uv may have an environment to build first
                match host.wait_ready(Duration::from_secs(60)) {
                    EngineState::Connected { .. } => {
                        eprintln!("({name}, process {} on a Unix socket)", host.pid().unwrap_or(0));
                        Ok(Box::new(host))
                    }
                    other => Err(format!("engine not ready: {other:?}")),
                }
            }
            Start::Attach { url, token } => {
                let host = HttpHost::attach(&url, &token, HttpOptions::default())?;
                eprintln!("(attached to {url})");
                Ok(Box::new(host))
            }
        },
    }
}

/// `plumcraft conform`: every generic case, per binding, one line each.
fn conform(args: &[String]) -> ExitCode {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1));
    let Some(file) = value("--engine-manifest") else { return fail("conform needs --engine-manifest FILE") };
    let resolved = match plumcraft_host::manifest::load(std::path::Path::new(file)) {
        Ok(r) => r,
        Err(e) => return fail(&e),
    };
    let Start::Command(m) = &resolved.start else { return fail("conform starts the engine itself, and this manifest has no command") };
    let slow = resolved.manifest.conformance.slow.clone().unwrap_or_else(|| format!("{}.slow", resolved.manifest.id));
    let mut bindings = Vec::new();
    for name in value("--modes").map_or("process,http", String::as_str).split(',') {
        match name.trim() {
            "process" => bindings.push(Binding::Process),
            "http" => bindings.push(Binding::Http),
            other => return fail(&format!("--modes {other:?}: process, http")),
        }
    }
    let (mut held, mut failed) = (0, 0);
    for binding in bindings {
        let verdicts = match plumcraft_conform::check(m, &slow, binding) {
            Ok(v) => v,
            Err(e) => return fail(&format!("{}: {e}", binding.name())),
        };
        for v in verdicts {
            match &v.outcome {
                Ok(()) => {
                    held += 1;
                    println!("ok    {}", v.case);
                }
                Err(e) => {
                    failed += 1;
                    println!("FAIL  {}: {e}", v.case);
                }
            }
        }
    }
    println!("{held} of {} cases hold", held + failed);
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) }
}

fn nodes(args: &[String]) -> ExitCode {
    let host = match start(&mode(args)) {
        Ok(h) => h,
        Err(e) => return fail(&e),
    };
    let schema = host.call("schema", json!({})).unwrap_or(Value::Null);
    for n in schema["nodes"].as_array().into_iter().flatten() {
        let ports = |k: &str| {
            n[k].as_array()
                .into_iter()
                .flatten()
                .map(|p| format!("{}:{}", p["name"].as_str().unwrap_or("?"), p["type"].as_str().unwrap_or("?")))
                .collect::<Vec<_>>()
                .join(", ")
        };
        println!("{:<14} {:<12} ({}) -> ({})", n["id"].as_str().unwrap_or("?"), n["display"].as_str().unwrap_or(""), ports("inputs"), ports("outputs"));
    }
    ExitCode::SUCCESS
}

/// A blob in one line: its handle, size and where it was read from (or that it is gone).
fn describe_blob(host: &dyn Connection, r: &plumcraft_proto::data::DataRef) -> String {
    let handle = r.handle.as_ref().map_or_else(|| "(no handle)".to_owned(), ToString::to_string);
    let place = match r.at.first() {
        Some(plumcraft_proto::data::Locator::Inline { .. }) => "inline",
        Some(plumcraft_proto::data::Locator::Commonkit { .. }) => "commonkit",
        Some(plumcraft_proto::data::Locator::Url { .. }) => "by URL",
        Some(_) => "elsewhere",
        None => "nowhere",
    };
    match host.resolve(r) {
        Some(b) => format!("blob {handle}, {} bytes {} {}, {place}, read and verified", b.bytes.len(), b.dtype, plumcraft_proto::handle::shape_repr(&b.shape)),
        None => format!("blob {handle}, {} bytes, {place}: gone or not readable here", r.meta.size),
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("plumcraft: {msg}");
    ExitCode::FAILURE
}

struct Opts {
    workflow: Value,
    watch: Vec<(String, String)>,
    cancel_after: Option<f64>,
    raw: bool,
    mode: Mode,
    /// The workflow file, when one was given: its relative paths are relative to its folder.
    path: Option<std::path::PathBuf>,
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts { workflow: Value::Null, watch: Vec::new(), cancel_after: None, raw: false, mode: mode(args), path: None };
    let mut path = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--raw" => o.raw = true,
            "--process" | "--python" | "--http" => {}
            "--attach" | "--engine-manifest" => {
                it.next();
            }
            "--watch" => {
                let w = it.next().ok_or("--watch needs NODE.PORT")?;
                let (n, p) = w.split_once('.').ok_or("--watch needs NODE.PORT")?;
                o.watch.push((n.into(), p.into()));
            }
            "--cancel-after" => o.cancel_after = Some(it.next().and_then(|s| s.parse().ok()).ok_or("--cancel-after needs seconds")?),
            other if path.is_none() => path = Some(other.to_owned()),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    // A manifest's engine has its own demo; the built-in one is the text engine's.
    let builtin = path.is_none() && !matches!(o.mode, Mode::File { .. });
    if let (None, Mode::File { path: file, .. }) = (&path, &o.mode) {
        let r = plumcraft_host::manifest::load(file)?;
        let demo = r.demo.ok_or_else(|| format!("{}: no workflow given, and the manifest has no demo", file.display()))?;
        path = Some(demo.display().to_string());
    }
    let text = match &path {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?,
        None => plumcraft_text::DEMO_WORKFLOW.to_owned(),
    };
    o.workflow = serde_json::from_str(&text).map_err(|e| format!("workflow: {e}"))?;
    o.path = path.as_deref().map(std::path::PathBuf::from);
    if o.watch.is_empty() && builtin {
        o.watch.push(("shout".into(), "text".into()));
    }
    Ok(o)
}

fn run(args: &[String]) -> ExitCode {
    let o = match parse(args) {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    let host = match start(&o.mode) {
        Ok(h) => h,
        Err(e) => return fail(&e),
    };
    let mut o = o;
    if let Some(path) = &o.path
        && let Ok(Ok(catalog)) = host.call("schema", json!({})).map(serde_json::from_value)
    {
        o.workflow = plumcraft_session::paths::anchor_workflow(&o.workflow, &catalog, path);
    }
    let t0 = Instant::now();
    let stamp = || format!("{:>7.3}s", t0.elapsed().as_secs_f64());

    let hello = host.call("hello", json!({"v": 2, "op": "hello", "protocol": {"min": 2, "max": 2}, "client": {"name": "plumcraft-cli", "version": env!("CARGO_PKG_VERSION")}, "caps": ["events.push", "events.poll", "events.resume", "status.progress", "display.stream", "codec.json"]}));
    let hello: HelloReply = match hello.map(serde_json::from_value) {
        Ok(Ok(h)) => h,
        Ok(Err(e)) => return fail(&e.to_string()),
        Err(e) => return fail(&e.to_string()),
    };
    println!("{} hello    engine {} {}, protocol {}, session {}", stamp(), hello.engine.name, hello.engine.version, hello.protocol, hello.session);
    println!("{}          caps: {}", " ".repeat(8), hello.caps.iter().collect::<Vec<_>>().join(" "));

    let watch: Vec<Value> = o.watch.iter().map(|(n, p)| json!({"node": n, "port": p})).collect();
    // Unique per client, not per process: an attached engine outlives this CLI and remembers
    // its earlier runs.
    let run_id = format!("cli-{}-{}", std::process::id(), plumcraft_engine::now_ts().to_bits());
    let req = json!({"v": 2, "op": "run", "run_id": run_id, "generation": 1, "graph": {"kind": "inline", "graph": o.workflow}, "watch": watch});
    match host.call("run", req) {
        Ok(ack) => println!("{} run      accepted: {}", stamp(), ack),
        Err(e) => return fail(&e.to_string()),
    }

    let mut reducer = Reducer::new(&run_id, 1);
    let mut since = 0;
    let mut cancelled = false;
    let reply: RunReply = loop {
        if let Some(after) = o.cancel_after
            && !cancelled
            && t0.elapsed().as_secs_f64() >= after
        {
            cancelled = true;
            let _ = host.call("run.cancel", json!({"run_id": run_id}));
            println!("{} cancel   sent", stamp());
        }
        let p: PollReply = match host.call("events.poll", json!({"since": since, "wait_ms": 100})).map(serde_json::from_value) {
            Ok(Ok(p)) => p,
            _ => return fail("events.poll failed"),
        };
        since = p.last_seq;
        let mut done = None;
        for e in p.events {
            if o.raw {
                println!("{}", serde_json::to_string(&e).unwrap_or_default());
            }
            match &e.event {
                Event::Status(rec) => {
                    reducer.apply(e.run_id.as_deref().unwrap_or(""), e.generation.unwrap_or(0), rec);
                    if o.raw {
                        continue;
                    }
                    let detail = match rec.state {
                        NodeState::Progress => format!("{:>3.0}%  {}", rec.fraction.unwrap_or(0.0) * 100.0, rec.stage.as_deref().unwrap_or("")),
                        NodeState::Preview => {
                            let thumb = match &rec.thumb {
                                Some(Rendition::Excerpt { text, .. }) => format!("thumb {:?}", text.chars().take(48).collect::<String>()),
                                _ => String::new(),
                            };
                            let shown = rec.display.iter().map(|d| format!("  [watched {}.{}: {}]", d.node, d.port, d.summary)).collect::<String>();
                            format!("{thumb}{shown}")
                        }
                        NodeState::Done => format!("{:.1} ms", rec.ms.unwrap_or(0.0)),
                        NodeState::Error | NodeState::Skipped | NodeState::Cancelled => {
                            rec.error.as_ref().map(|x| format!("{}: {}", x.kind, x.message)).unwrap_or_default()
                        }
                        _ => String::new(),
                    };
                    println!("{} #{:<4} {:<8} {:<9} {}", stamp(), e.seq, rec.node_id, format!("{:?}", rec.state).to_lowercase(), detail);
                }
                Event::Live(u) => {
                    if let LiveUpdate::Log { node_id, text, .. } = u.as_ref() {
                        println!("{} #{:<4} {:<8} log       {}", stamp(), e.seq, node_id, text);
                    }
                }
                Event::RunReply(r) if r.run_id == run_id => done = Some((**r).clone()),
                _ => {}
            }
        }
        if let Some(r) = done {
            break r;
        }
        if t0.elapsed() > Duration::from_secs(600) {
            return fail("no reply after 10 minutes");
        }
    };
    reducer.finish(&reply);

    println!(
        "{} reply    ok={} elapsed {:.1} ms{}",
        stamp(),
        reply.ok,
        reply.elapsed_ms,
        reply.error.as_ref().map(|e| format!(", {}: {}", e.kind, e.message)).unwrap_or_default()
    );
    println!();
    println!("node      state      ms");
    for id in &reply.order {
        if let Some(n) = reducer.node(id) {
            println!("{:<9} {:<10} {:>7.1}", id, format!("{:?}", n.state).to_lowercase(), n.ms.unwrap_or(0.0));
        }
    }
    for d in &reply.watch {
        println!();
        println!("watched {}.{} ({:?}): {}", d.node, d.port, d.media, d.summary);
        for r in &d.renditions {
            if let Rendition::Excerpt { text, .. } = r {
                for line in text.lines() {
                    println!("  | {line}");
                }
            }
        }
        if let Some(v) = &d.value {
            println!("  value: {v}");
        }
        if let Some(f) = &d.full {
            println!("  full: {}", describe_blob(host.as_ref(), f));
        }
        if let Some(e) = &d.error {
            println!("  error: {}", e.message);
        }
    }
    let outputs: Vec<String> = reply
        .nodes
        .iter()
        .flat_map(|(id, n)| n.outputs.iter().map(move |(p, v)| (id, p, v)))
        .filter_map(|(id, p, v)| match v {
            PortValue::Json { value } => Some(format!("{id}.{p} = {value}")),
            PortValue::Blob { data } => Some(format!("{id}.{p} = {}", describe_blob(host.as_ref(), data))),
            PortValue::Opaque { ty, summary, .. } => Some(format!("{id}.{p} = ({ty}) {summary}")),
            PortValue::List { .. } => None,
        })
        .collect();
    if !outputs.is_empty() {
        println!();
        outputs.iter().for_each(|o| println!("{o}"));
    }
    if reply.ok { ExitCode::SUCCESS } else { ExitCode::from(1) }
}
