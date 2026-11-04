//! The editor without a window: `run FILE` computes a workflow's results and writes them,
//! `check FILE` says what is wrong with a workflow before it runs, `tidy FILE` lays it out as
//! Edit › Tidy does, and `nodes` lists the catalogue. Any application of the editor has all four.
//!
//! A workflow with no Result node runs its writing nodes (Write Files, Save WAV: `output_node`)
//! instead, on whatever engines they live in, so a graph of craft or array nodes that ends in
//! Write Files runs here exactly as Run does in the window. `--out` points their folders there.
//!
//! It is the editor's own path with nothing drawn: `graph.open` reads the file (through the
//! application's openers, so a picture runs from its recipe) and `run.save` makes each request, so
//! a file written here is the one Save Result writes. Engines start as the window starts them,
//! several behind a coordinator.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use plumcraft_coordinator::Coordinator;
use plumcraft_engine::host::PollReply;
use plumcraft_graph::NodeId;
use plumcraft_host::Connection;
use plumcraft_proto::catalogue::Socket;
use plumcraft_proto::catalogue::{SchemaReply, texts};
use plumcraft_proto::event::{Event, NodeState};
use plumcraft_proto::run::{RunReply, SaveReply};
use plumcraft_proto::session::EngineState;
use plumcraft_session::run::Outgoing;
use plumcraft_session::save_cmds::{format_of, suggested_name};
use plumcraft_session::{EngineTarget, Session};
use plumcraft_ui::link::Link;
use serde_json::{Value, json};

use crate::{EngineKind, Launcher, Mode, choices};

/// Flags the engine choice has read, and whether each takes a value.
const ENGINE_FLAGS: &[(&str, bool)] =
    &[("--engine", true), ("--engine-manifest", true), ("--inproc", false), ("--process", false), ("--python", false), ("--http", false)];

/// What `run` was asked.
#[derive(Debug, Default, PartialEq)]
struct Ask {
    file: PathBuf,
    out: PathBuf,
    /// Outputs named by `--node`; every Result node's when empty.
    targets: Vec<String>,
    format: Option<String>,
    sheet: bool,
    /// `--out` was given (it then also points the writing nodes' folders there).
    out_given: bool,
}

/// `run`'s arguments, after the word `run`.
fn parse(args: &[String]) -> Result<Ask, String> {
    let mut ask = Ask { out: PathBuf::from("."), ..Ask::default() };
    let mut file = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let value = |it: &mut std::slice::Iter<'_, String>, what: &str| it.next().cloned().ok_or_else(|| format!("{a} needs {what}"));
        match a.as_str() {
            "--out" => {
                ask.out = PathBuf::from(value(&mut it, "a folder")?);
                ask.out_given = true;
            }
            "--node" => ask.targets.push(value(&mut it, "NODE or NODE.PORT")?),
            "--format" => ask.format = Some(value(&mut it, "an extension")?),
            "--sheet" => ask.sheet = true,
            s if ENGINE_FLAGS.iter().any(|(f, _)| *f == s) => {
                if ENGINE_FLAGS.iter().any(|(f, takes)| *f == s && *takes) {
                    it.next();
                }
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if file.is_none() => file = Some(PathBuf::from(s)),
            s => return Err(format!("one file at a time: {s}")),
        }
    }
    ask.file = file.ok_or("run needs a file")?;
    Ok(ask)
}

/// The engines, connected, and the one connection they are together. The links hold them up.
fn connect(kind: EngineKind, mode: Mode, extras: &[EngineKind], manifests: Vec<PathBuf>) -> Result<(Vec<Link>, Arc<dyn Connection>), String> {
    let launcher = Launcher { manifests, ..Default::default() };
    let mut links = Vec::new();
    for (k, m) in std::iter::once((kind, mode)).chain(extras.iter().map(|e| {
        let m = if e.hosts(mode) && !matches!(kind, EngineKind::Pack(_)) { mode } else { e.default_mode() };
        (*e, m)
    })) {
        let link = launcher.connect(k, m, None).map_err(|e| format!("engine {}: {e}", k.id()))?;
        match link.state() {
            EngineState::Connected { .. } | EngineState::Busy { .. } => links.push((k.id(), link)),
            other => return Err(format!("engine {}: {}", k.id(), said(&other))),
        }
    }
    let conn: Arc<dyn Connection> = match links.as_slice() {
        [(_, one)] => Arc::clone(&one.conn),
        several => Arc::new(several.iter().fold(Coordinator::new(), |c, (id, l)| c.with_engine(id.clone(), Arc::clone(&l.conn)))),
    };
    Ok((links.into_iter().map(|(_, l)| l).collect(), conn))
}

/// Why an engine isn't connected, in words.
fn said(state: &EngineState) -> String {
    match state {
        EngineState::EngineDown { reason, hint, .. } if !hint.is_empty() => format!("{reason}. Fix: {hint}"),
        EngineState::EngineDown { reason, .. } | EngineState::Unavailable { reason } => reason.clone(),
        EngineState::Unreachable { url, reason, .. } => format!("{url}: {reason}"),
        other => format!("{other:?}"),
    }
}

fn catalogue(conn: &dyn Connection) -> Result<SchemaReply, String> {
    let v = conn.call("schema", json!({})).map_err(|e| e.message)?;
    serde_json::from_value(v).map_err(|e| format!("the engine's catalogue: {e}"))
}

/// One line per node: id, display name, category.
fn node_lines(schema: &SchemaReply) -> Vec<String> {
    let width = schema.nodes.iter().map(|n| n.id.len()).max().unwrap_or(0);
    schema.nodes.iter().map(|n| format!("{:<width$}  {:<16}  {}", n.id, n.display, n.category.join(" › "))).collect()
}

pub(crate) fn nodes(program: &str, kind: EngineKind, mode: Mode, extras: &[EngineKind], manifests: Vec<PathBuf>) -> ExitCode {
    let listed = connect(kind, mode, extras, manifests).and_then(|(links, conn)| {
        let c = catalogue(conn.as_ref());
        drop(links);
        c
    });
    match listed {
        Ok(c) => {
            for line in node_lines(&c) {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{program}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The outputs to write: as `--node` names them, else each Result node's first, as its Save
/// Result… does.
fn targets(s: &Session, ask: &Ask) -> Result<Vec<(NodeId, String)>, String> {
    if ask.targets.is_empty() {
        let results = s.graph().nodes.iter().filter_map(|(id, n)| {
            let port = s.catalog().node(&n.kind).filter(|d| d.flags.result)?.outputs.first()?.name.clone();
            Some((id.clone(), port))
        });
        let found: Vec<_> = results.collect();
        return if found.is_empty() {
            Err("this graph has no Result node and no Write Files node: name an output with --node NODE[.PORT]".into())
        } else {
            Ok(found)
        };
    }
    ask.targets
        .iter()
        .map(|t| {
            let whole = NodeId::from(t.as_str());
            if s.graph().node(&whole).is_some() {
                let port = plumcraft_session::default_output(s, &whole).ok_or_else(|| format!("{t} has no outputs"))?;
                return Ok((whole, port));
            }
            let (node, port) = t.rsplit_once('.').ok_or_else(|| format!("no node {t}"))?;
            let id = NodeId::from(node);
            if s.graph().node(&id).is_none() {
                return Err(format!("no node {node}"));
            }
            Ok((id, port.to_owned()))
        })
        .collect()
}

/// Where `node`'s file goes: the name Save Result… offers, with `--format`'s extension, and the
/// node's id when two would collide.
fn file_for(s: &Session, ask: &Ask, node: &NodeId, taken: &mut Vec<PathBuf>) -> PathBuf {
    let title = plumcraft_session::node_title(s.graph(), s.catalog(), node);
    let name = PathBuf::from(suggested_name(s.catalog(), &title, None));
    let name = ask.format.as_deref().map_or_else(|| name.clone(), |e| name.with_extension(e.trim_start_matches('.')));
    let mut path = ask.out.join(&name);
    if taken.contains(&path) {
        let stem = name.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = name.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        path = ask.out.join(format!("{stem}-{node}{ext}"));
    }
    taken.push(path.clone());
    path
}

/// A file opened: its engines must all be here; other problems are the run's to report.
fn check_opened(file: &Path, opened: &Value, extras: &str) -> Result<(), String> {
    if let Some(m) = opened.get("missingEngines") {
        let engines: Vec<&str> = m["engines"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let kinds: Vec<&str> = m["kinds"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let how = if engines.is_empty() { String::new() } else { format!(": run with --engine {extras},{}", engines.join(",")) };
        let what = if engines.is_empty() { "an engine this run hasn't got".to_owned() } else { format!("the engine {}", engines.join(", ")) };
        return Err(format!("{} needs {what} ({}){how}", file.display(), kinds.join(", ")));
    }
    for p in opened["problems"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        eprintln!("{}: {p}", file.display());
    }
    Ok(())
}

/// The engines' links (kept alive while it's used), their connection, and the session.
type Opened = (Vec<Link>, Arc<dyn Connection>, Session);

/// Start the engines and open `file` in a session on them, as the window does.
fn open(program: &str, file: &Path, kind: EngineKind, mode: Mode, extras: &[EngineKind], manifests: Vec<PathBuf>) -> Result<Opened, String> {
    let (links, conn) = connect(kind, mode, extras, manifests)?;
    let catalog = catalogue(conn.as_ref())?;
    if let Some(notice) = catalog.text(texts::ENGINE_NOTICE).filter(|n| !n.is_empty()) {
        eprintln!("{program}: {notice}");
    }
    let mut s = Session::new(catalog);
    s.connected = true;
    s.engines.choices = choices();
    s.engines.current = Some(EngineTarget::Choice { engine: kind.id(), mode: mode.id().into() });
    s.engines.added = extras.iter().map(|e| EngineTarget::Choice { engine: e.id(), mode: e.default_mode().id().into() }).collect();
    let opened = s.execute("graph.open", &json!({"path": file})).map_err(|e| e.to_string())?;
    let ids: Vec<String> = std::iter::once(kind).chain(extras.iter().copied()).map(EngineKind::id).collect();
    check_opened(file, &opened, &ids.join(","))?;
    Ok((links, conn, s))
}

/// The nodes that write files themselves (`output_node`: Write Files, Save WAV), by id.
fn writing_nodes(s: &Session) -> Vec<String> {
    s.graph().nodes.iter().filter(|(_, n)| s.catalog().node(&n.kind).is_some_and(|d| d.flags.output_node)).map(|(id, _)| id.0.clone()).collect()
}

/// The longest a run of the writing nodes may take.
const RUN_LIMIT: std::time::Duration = std::time::Duration::from_secs(900);

/// Run the writing nodes (and so everything upstream of them), printing each node as it ends.
/// With `--out`, each one's folder input is pointed there first.
fn run_writers(s: &mut Session, conn: &dyn Connection, ask: &Ask, writers: Vec<String>) -> Result<(), String> {
    if ask.out_given {
        let out = std::path::absolute(&ask.out).map_err(|e| e.to_string())?;
        for id in &writers {
            let kind = s.graph().node(&NodeId::from(id.as_str())).map(|n| n.kind.clone()).unwrap_or_default();
            let folder =
                s.catalog().node(&kind).and_then(|d| d.inputs.iter().find(|p| p.path.as_ref().is_some_and(|h| h.mode == "dir")).map(|p| p.name.clone()));
            if let Some(port) = folder {
                s.execute("node.set", &json!({"node": id, "port": port, "value": out.display().to_string()})).map_err(|e| e.to_string())?;
            }
        }
    }
    s.execute("run.start", &json!({"targets": writers, "watch": []})).map_err(|e| e.to_string())?;
    let t0 = std::time::Instant::now();
    let mut run_id = None;
    for o in s.run.take_outbox() {
        if let Outgoing::Run(req) = o {
            run_id = Some(req.run_id.clone());
            let body = serde_json::to_value(&req).map_err(|e| e.to_string())?;
            conn.call("run", body).map_err(|e| e.message)?;
        }
    }
    let run_id = run_id.ok_or("nothing to run (see graph.review: a workflow from elsewhere waits to be trusted)")?;
    let mut since = 0;
    while t0.elapsed() < RUN_LIMIT {
        let v = conn.call("events.poll", json!({"since": since, "wait_ms": 500, "run_id": run_id})).map_err(|e| e.message)?;
        let p: PollReply = serde_json::from_value(v).map_err(|e| format!("events.poll: {e}"))?;
        since = p.last_seq;
        for env in p.events {
            match env.event {
                Event::RunReply(r) => return finished(&r, t0),
                Event::Status(st) if matches!(st.state, NodeState::Error | NodeState::Done | NodeState::Cached) => {
                    let why = st.error.as_ref().map(|e| format!("  {}", e.message)).unwrap_or_default();
                    println!("{:7.1}s  {:<16} {:?}{why}", t0.elapsed().as_secs_f64(), st.node_id, st.state);
                }
                _ => {}
            }
        }
    }
    let _ = conn.call("run.cancel", json!({"run_id": run_id}));
    Err(format!("no reply in {} s", RUN_LIMIT.as_secs()))
}

/// A run's end: an error naming every node that didn't finish.
fn finished(r: &RunReply, t0: std::time::Instant) -> Result<(), String> {
    let bad: Vec<String> = r
        .nodes
        .iter()
        .filter(|(_, n)| !matches!(n.state, NodeState::Done | NodeState::Cached))
        .map(|(k, n)| format!("{k}: {:?}{}", n.state, n.error.as_ref().map(|e| format!(" ({})", e.message)).unwrap_or_default()))
        .collect();
    if r.ok && bad.is_empty() {
        println!("done in {:.1} s", t0.elapsed().as_secs_f64());
        Ok(())
    } else {
        Err(r.error.as_ref().map(|e| e.message.clone()).into_iter().chain(bad).collect::<Vec<_>>().join("\n  "))
    }
}

/// What is wrong with the opened workflow, short of running it: unknown node types and inputs,
/// wires the type rules refuse, wire-only inputs left unwired, and nodes no output depends on
/// (they never run, which usually means a forgotten wire).
fn problems(s: &Session) -> Vec<String> {
    let (g, cat) = (s.graph(), s.catalog());
    let mut out = Vec::new();
    for (id, n) in &g.nodes {
        let Some(def) = cat.node(&n.kind) else {
            out.push(format!("node {id}: no node type {:?} in these engines", n.kind));
            continue;
        };
        for k in n.values.keys().filter(|k| !def.inputs.iter().any(|p| p.name == **k)) {
            out.push(format!("node {id} ({}): no input {k:?}", n.kind));
        }
        for p in def.inputs.iter().filter(|p| p.socket_mode() == Socket::Socket && !p.optional) {
            let slot =
                |port: &str| port == p.name || port.strip_prefix(p.name.as_str()).is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()));
            if !g.wires.iter().any(|w| w.to.node == *id && slot(&w.to.port)) {
                out.push(format!("node {id} ({}): input {:?} needs a wire", n.kind, p.name));
            }
        }
    }
    // a refused wire says what would fix it, when the engines' converters can
    out.extend(g.wire_problems(cat).into_iter().map(|(w, why)| {
        let fix =
            plumcraft_session::convert::offers(s, &w.from, &w.to).into_iter().next().and_then(|o| o.get("label").and_then(|l| l.as_str()).map(str::to_owned));
        match fix {
            Some(label) => format!("wire {} -> {}: {why}; wire.convert would insert {label}", w.from, w.to),
            None => format!("wire {} -> {}: {why}", w.from, w.to),
        }
    }));
    let sinks: Vec<String> =
        g.nodes.iter().filter(|(_, n)| cat.node(&n.kind).is_some_and(|d| d.flags.result || d.flags.output_node)).map(|(id, _)| id.0.clone()).collect();
    if !sinks.is_empty() {
        let mut live: std::collections::BTreeSet<String> = sinks.into_iter().collect();
        let mut grew = true;
        while grew {
            grew = false;
            for w in &g.wires {
                if live.contains(&w.to.node.0) && live.insert(w.from.node.0.clone()) {
                    grew = true;
                }
            }
        }
        out.extend(
            g.nodes.keys().filter(|id| !live.contains(&id.0)).map(|id| format!("node {id}: no Result or Write Files node depends on it, so it never runs")),
        );
    }
    out
}

/// The one workflow `check` and `tidy` take, after the word.
fn one_file(args: &[String]) -> Result<PathBuf, String> {
    let mut file = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            s if ENGINE_FLAGS.iter().any(|(f, takes)| *f == s && *takes) => {
                it.next();
            }
            s if ENGINE_FLAGS.iter().any(|(f, _)| *f == s) => {}
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if file.is_none() => file = Some(PathBuf::from(s)),
            s => return Err(format!("one file at a time: {s}")),
        }
    }
    file.ok_or_else(|| "it needs a file".into())
}

/// `check FILE`: every problem, one per line; exit 1 when there is any.
pub(crate) fn check(program: &str, args: &[String], kind: EngineKind, mode: Mode, extras: &[EngineKind], manifests: Vec<PathBuf>) -> ExitCode {
    let file = match one_file(args) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{program}: check: {e}\n\n{}", crate::usage(program));
            return ExitCode::from(2);
        }
    };
    match open(program, &file, kind, mode, extras, manifests) {
        Ok((links, _, s)) => {
            let p = problems(&s);
            drop(links);
            for line in &p {
                println!("{}: {line}", file.display());
            }
            if p.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
        }
        Err(e) => {
            eprintln!("{program}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `tidy FILE`: Edit › Tidy, then Save, in place.
pub(crate) fn tidy(program: &str, args: &[String], kind: EngineKind, mode: Mode, extras: &[EngineKind], manifests: Vec<PathBuf>) -> ExitCode {
    let file = match one_file(args) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{program}: tidy: {e}\n\n{}", crate::usage(program));
            return ExitCode::from(2);
        }
    };
    let tidied = open(program, &file, kind, mode, extras, manifests).and_then(|(links, _, mut s)| {
        s.execute("graph.tidy", &json!({})).map_err(|e| e.to_string())?;
        s.execute("graph.save", &json!({})).map_err(|e| e.to_string())?;
        drop(links);
        Ok(())
    });
    match tidied {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{program}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Open, then write each output. What it wrote is printed as it goes, so a failure part way says
/// how far it got.
fn write_all(program: &str, ask: &Ask, kind: EngineKind, mode: Mode, extras: &[EngineKind], manifests: Vec<PathBuf>) -> Result<(), String> {
    let (links, conn, mut s) = open(program, &ask.file, kind, mode, extras, manifests)?;

    if let Some(e) = &ask.format
        && format_of(s.catalog(), Path::new(&format!("x.{}", e.trim_start_matches('.')))).is_none()
    {
        let known: Vec<&str> = s.catalog().save_formats.iter().flat_map(|f| f.extensions.iter().map(String::as_str)).collect();
        return Err(format!("no format {e}: one of {}", known.join(", ")));
    }
    let has_result = s.graph().nodes.values().any(|n| s.catalog().node(&n.kind).is_some_and(|d| d.flags.result));
    let writers = writing_nodes(&s);
    if ask.targets.is_empty() && !has_result && !writers.is_empty() {
        let r = run_writers(&mut s, conn.as_ref(), ask, writers);
        drop(links);
        return r;
    }
    let chosen = targets(&s, ask)?;
    std::fs::create_dir_all(&ask.out).map_err(|e| format!("{}: {e}", ask.out.display()))?;
    let mut taken = Vec::new();
    for (node, port) in chosen {
        let path = std::path::absolute(file_for(&s, ask, &node, &mut taken)).map_err(|e| e.to_string())?;
        s.execute("run.save", &json!({"node": node, "port": port, "path": path, "sheet": ask.sheet})).map_err(|e| e.to_string())?;
        for req in s.save.take_outbox() {
            let body = serde_json::to_value(&req).map_err(|e| e.to_string())?;
            let reply = conn.call("output.save", body).map_err(|e| format!("{node}.{port}: {}", e.message))?;
            let reply: SaveReply = serde_json::from_value(reply).map_err(|e| format!("the save reply: {e}"))?;
            s.save.apply_reply(reply.clone());
            if !reply.ok {
                let why = reply.error.map_or_else(|| "not saved".to_owned(), |e| e.message);
                return Err(format!("{node}.{port}: {why}"));
            }
            for p in &reply.paths {
                println!("{p}");
            }
            if !reply.recipe {
                eprintln!("{program}: {node}.{port}: saved without its recipe");
            }
        }
    }
    drop(links);
    Ok(())
}

pub(crate) fn run(program: &str, args: &[String], kind: EngineKind, mode: Mode, extras: &[EngineKind], manifests: Vec<PathBuf>) -> ExitCode {
    let ask = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{program}: {e}\n\n{}", crate::usage(program));
            return ExitCode::from(2);
        }
    };
    match write_all(program, &ask, kind, mode, extras, manifests) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{program}: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn run_reads_its_arguments_and_leaves_the_engines_to_the_engine_flags() {
        let a = parse(&strings(&[
            "flow.json",
            "--engine",
            "vi,vectorcraft",
            "--out",
            "d",
            "--node",
            "a.b",
            "--node",
            "c",
            "--format",
            ".jpg",
            "--sheet",
            "--process",
        ]))
        .unwrap();
        assert_eq!(a.file, PathBuf::from("flow.json"));
        assert_eq!(a.out, PathBuf::from("d"));
        assert_eq!(a.targets, ["a.b", "c"]);
        assert_eq!(a.format.as_deref(), Some(".jpg"));
        assert!(a.sheet);
        for bad in [&["--out", "d"][..], &["a", "b"], &["a", "--bogus"], &["a", "--out"], &["a", "--node"], &[]] {
            assert!(parse(&strings(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_file_needing_an_engine_says_how_to_ask_for_it() {
        let opened = json!({"missingEngines": {"engines": ["vectorcraft"], "kinds": ["vc.open"]}, "problems": []});
        let e = check_opened(Path::new("a.png"), &opened, "vi").unwrap_err();
        assert!(e.contains("vectorcraft") && e.contains("vc.open") && e.contains("--engine vi,vectorcraft"), "{e}");
        let unnamed = json!({"missingEngines": {"engines": [], "kinds": ["vc.open"]}});
        assert!(check_opened(Path::new("a.png"), &unnamed, "vi").unwrap_err().contains("vc.open"));
        assert!(check_opened(Path::new("a.png"), &json!({"problems": []}), "vi").is_ok());
    }
}
