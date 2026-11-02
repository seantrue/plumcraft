//! The editor without a window: `run FILE` computes a workflow's results and writes them, and
//! `nodes` lists the catalogue. Any application of the editor has both.
//!
//! It is the editor's own path with nothing drawn: `graph.open` reads the file (through the
//! application's openers, so a picture runs from its recipe) and `run.save` makes each request, so
//! a file written here is the one Save Result writes. Engines start as the window starts them,
//! several behind a coordinator.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use plumcraft_coordinator::Coordinator;
use plumcraft_graph::NodeId;
use plumcraft_host::Connection;
use plumcraft_proto::catalogue::{SchemaReply, texts};
use plumcraft_proto::run::SaveReply;
use plumcraft_proto::session::EngineState;
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
}

/// `run`'s arguments, after the word `run`.
fn parse(args: &[String]) -> Result<Ask, String> {
    let mut ask = Ask { out: PathBuf::from("."), ..Ask::default() };
    let mut file = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let value = |it: &mut std::slice::Iter<'_, String>, what: &str| it.next().cloned().ok_or_else(|| format!("{a} needs {what}"));
        match a.as_str() {
            "--out" => ask.out = PathBuf::from(value(&mut it, "a folder")?),
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
        return if found.is_empty() { Err("this graph has no Result node: name an output with --node NODE[.PORT]".into()) } else { Ok(found) };
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

/// Open, then write each output. What it wrote is printed as it goes, so a failure part way says
/// how far it got.
fn write_all(program: &str, ask: &Ask, kind: EngineKind, mode: Mode, extras: &[EngineKind], manifests: Vec<PathBuf>) -> Result<(), String> {
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
    let opened = s.execute("graph.open", &json!({"path": ask.file})).map_err(|e| e.to_string())?;
    let ids: Vec<String> = std::iter::once(kind).chain(extras.iter().copied()).map(EngineKind::id).collect();
    check_opened(&ask.file, &opened, &ids.join(","))?;

    if let Some(e) = &ask.format
        && format_of(s.catalog(), Path::new(&format!("x.{}", e.trim_start_matches('.')))).is_none()
    {
        let known: Vec<&str> = s.catalog().save_formats.iter().flat_map(|f| f.extensions.iter().map(String::as_str)).collect();
        return Err(format!("no format {e}: one of {}", known.join(", ")));
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
