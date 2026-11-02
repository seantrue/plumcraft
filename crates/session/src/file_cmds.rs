//! Workflow files: `graph.open` (a workflow, or a `.zip` bundle, [`crate::archive`]),
//! `graph.save`, `graph.saveAs`, `graph.bundle` and `graph.info`. A workflow file is
//! [`plumcraft_graph::Graph`] JSON, positions, titles and views included.
//!
//! An application can open other files as workflows ([`FileOpener`]): what reads a graph out of a
//! picture is its knowledge, not core's. Such a file opens unsaved, so Save asks where and never
//! writes over it. Files are hostile input, and an opener's more than most.
//!
//! Files are hostile input: the size is bounded and the version and wires are checked before the
//! open graph is replaced. A file whose nodes or wires don't fit the catalogue (an engine that
//! lacks a node type, a file from another engine) still opens: its problems are reported, the
//! canvas shows what it can, and edits may not add new ones ([`crate::graph_problems`]).
//! Nodes a file gives no position are placed in columns ([`crate::layout`]). Saving writes a
//! temp file next to the target and renames it over, so a failed save never leaves a
//! half-written workflow.
//!
//! Lifted from the research software's `crates/engine/src/file_cmds.rs` (2026-10-27, the research software `8d96021`; plan
//! 10.3, E2), its picture recipes behind [`FileOpener`]. `graph.open` refuses to drop unsaved
//! changes unless asked (`discard: true`), as `graph.new` does; the UI asks first.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use plumcraft_graph::Graph;
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, opt_str_param, str_param};
use crate::{Result, Session, SessionError};

/// The largest workflow file `graph.open` reads (and the largest clipboard `edit.paste` takes).
pub const MAX_WORKFLOW_BYTES: u64 = 64 * 1024 * 1024;

/// The most problems `graph.open` lists (it counts them all).
pub const MAX_PROBLEMS_LISTED: usize = 50;

/// Reads a graph out of a file that isn't a workflow: an application's, by extension. It answers
/// with workflow JSON, or says why it can't, in words for the person who chose the file. A file
/// it opens is hostile input: it bounds what it reads.
#[derive(Debug, Clone, Copy)]
pub struct FileOpener {
    /// The extensions, without the dot, in any case.
    pub extensions: &'static [&'static str],
    /// What the Open dialog calls these files.
    pub label: &'static str,
    /// The file's graph, as workflow JSON.
    pub read: fn(&Path) -> std::result::Result<String, String>,
}

static OPENERS: std::sync::RwLock<Vec<FileOpener>> = std::sync::RwLock::new(Vec::new());

/// Add an application's openers. A workflow's extensions stay workflows, and an extension already
/// taken is refused, naming it.
pub fn register_openers(openers: Vec<FileOpener>) -> std::result::Result<(), String> {
    let mut all = OPENERS.write().map_err(|_| "the file openers are poisoned".to_owned())?;
    for o in openers {
        if let Some(e) = o.extensions.iter().find(|e| {
            e.eq_ignore_ascii_case("json") || e.eq_ignore_ascii_case("zip") || all.iter().any(|a| a.extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
        }) {
            return Err(format!("the file opener {:?}: .{e} is taken", o.label));
        }
        all.push(o);
    }
    Ok(())
}

/// The openers an application registered.
pub fn openers() -> Vec<FileOpener> {
    OPENERS.read().map(|o| o.clone()).unwrap_or_default()
}

/// The opener for `path`'s extension.
pub fn opener_for(path: &Path) -> Option<FileOpener> {
    let ext = path.extension()?.to_str()?;
    openers().into_iter().find(|o| o.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)))
}

/// The graph opener `o` reads out of `path`. What it answers is a workflow from a file, so it gets
/// a workflow's limits; a panic in it (it read a hostile file) is a refusal like any other.
pub fn read_opened(path: &Path, o: &FileOpener) -> Result<Graph> {
    let meta = std::fs::metadata(path).map_err(|e| file_err("open", path, e))?;
    if !meta.is_file() {
        return Err(file_err("open", path, "not a file"));
    }
    let read = o.read;
    let text = std::panic::catch_unwind(|| read(path)).unwrap_or_else(|_| Err("the file couldn't be read".to_owned()));
    let text = text.map_err(|e| file_err("open", path, e))?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > MAX_WORKFLOW_BYTES {
        return Err(file_err("open", path, format!("larger than {MAX_WORKFLOW_BYTES} bytes")));
    }
    parse_workflow(&text).map_err(|e| file_err("open", path, e))
}

/// A file as a graph, through its opener if one takes it: what `graph.open` reads, without a
/// session (the command line).
pub fn read_any(path: &Path) -> Result<Graph> {
    match opener_for(path) {
        Some(o) => read_opened(path, &o),
        None => read_workflow(path),
    }
}

/// A file path parameter, absolute (relative to the editor's folder when given relative), so
/// the workflow's folder is known.
pub(crate) fn path_param(p: &Value, key: &str) -> Result<PathBuf> {
    let s = str_param(p, key)?;
    if s.trim().is_empty() || s.contains('\0') {
        return Err(SessionError::Params(format!("{key:?} must be a file path")));
    }
    let path = crate::paths::expand_user(s);
    std::path::absolute(&path).map_err(|e| SessionError::Params(format!("{key:?}: {e}")))
}

fn file_err(what: &str, path: &Path, e: impl std::fmt::Display) -> SessionError {
    SessionError::File(format!("can't {what} {}: {e}", path.display()))
}

/// Read a workflow file, bounded to [`MAX_WORKFLOW_BYTES`] (whatever the file claims to be),
/// placing nodes it gives no position.
pub fn read_workflow(path: &Path) -> Result<Graph> {
    let meta = std::fs::metadata(path).map_err(|e| file_err("open", path, e))?;
    if !meta.is_file() {
        return Err(file_err("open", path, "not a file"));
    }
    let too_big = || file_err("open", path, format!("larger than {MAX_WORKFLOW_BYTES} bytes"));
    if meta.len() > MAX_WORKFLOW_BYTES {
        return Err(too_big());
    }
    let file = std::fs::File::open(path).map_err(|e| file_err("open", path, e))?;
    let mut text = String::new();
    file.take(MAX_WORKFLOW_BYTES.saturating_add(1)).read_to_string(&mut text).map_err(|e| file_err("read", path, e))?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > MAX_WORKFLOW_BYTES {
        return Err(too_big());
    }
    parse_workflow(&text).map_err(|e| file_err("open", path, e))
}

/// Workflow JSON as a graph, nodes without a position placed in columns.
pub fn parse_workflow(text: &str) -> Result<Graph> {
    let mut workflow: Value = serde_json::from_str(text).map_err(|e| SessionError::File(format!("not a workflow: {e}")))?;
    crate::layout::place_unplaced(&mut workflow);
    Ok(Graph::from_json(&workflow.to_string())?)
}

/// Write `text` to `path` atomically: a temp file in the same directory, synced, then renamed.
pub fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let name = path.file_name().ok_or_else(|| file_err("save to", path, "no file name"))?;
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".{}.tmp-{}", name.to_string_lossy(), std::process::id()));
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(text.as_bytes())?;
        f.sync_all()
    });
    if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(file_err("save", path, e));
    }
    Ok(())
}

fn summary(s: &Session) -> Value {
    let g = s.graph();
    json!({
        "path": s.path().map(|p| p.display().to_string()),
        "dirty": s.is_dirty(),
        "nodes": g.nodes.len(),
        "wires": g.wires.len(),
        "undo": s.undo_label(),
        "redo": s.redo_label(),
    })
}

fn graph_open(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    crate::graph_cmds::check_discard(s, p)?;
    // A file an application opens gives a graph with no file of its own: Save asks where, and
    // never writes over the picture it came from.
    if let Some(opener) = opener_for(&path) {
        let graph = read_opened(&path, &opener)?;
        s.replace_opened(graph, path.clone());
        s.forget_path();
        let mut out = opened(s);
        out["openedFrom"] = json!(path.display().to_string());
        return Ok(out);
    }
    // A bundle unpacks into a new folder beside it, and its workflow opens from there.
    let (graph, path, bundle) = crate::archive::read_workflow_or_bundle(path)?;
    s.replace_opened(graph, path);
    let mut out = opened(s);
    if let Some(b) = bundle {
        out["unpackedFrom"] = json!(b.display().to_string());
    }
    Ok(out)
}

/// What `graph.open` says of a graph just opened: its problems, and the engines it lacks.
fn opened(s: &Session) -> Value {
    let problems = s.problems();
    let mut out = summary(s);
    out["problemCount"] = json!(problems.len());
    out["problems"] = json!(problems.into_iter().take(MAX_PROBLEMS_LISTED).collect::<Vec<_>>());
    if let Some(m) = s.missing_engines() {
        out["missingEngines"] = json!(m);
    }
    out
}

/// `graph.bundle {path}`: the workflow and the files it refers to, in one `.zip`.
fn graph_bundle(s: &mut Session, p: &Value) -> Result<Value> {
    let mut path = path_param(p, "path")?;
    if !crate::archive::is_zip(&path) {
        path.set_extension("zip");
    }
    let name = s.path().and_then(Path::file_name).map_or_else(|| "workflow.json".to_owned(), |n| n.to_string_lossy().into_owned());
    let base = crate::paths::base_folder(s.path());
    let report = crate::archive::write_bundle(s.graph(), s.catalog(), &name, base.as_deref(), &path)?;
    Ok(json!({
        "path": path.display().to_string(),
        "entries": report.entries,
        "missing": report.missing.iter().map(|m| json!({"node": m.node, "port": m.port, "path": m.path, "why": m.why})).collect::<Vec<_>>(),
    }))
}

fn has_nodes(s: &Session) -> std::result::Result<(), String> {
    if s.graph().nodes.is_empty() { Err("the graph is empty".into()) } else { Ok(()) }
}

fn save_to(s: &mut Session, path: PathBuf) -> Result<Value> {
    let text = s.graph().to_json()?;
    write_atomic(&path, &text)?;
    s.mark_saved(path);
    s.remember_saved();
    Ok(summary(s))
}

fn graph_save(s: &mut Session, p: &Value) -> Result<Value> {
    let path = match opt_str_param(p, "path")? {
        Some(_) => path_param(p, "path")?,
        None => s.path().map(Path::to_path_buf).ok_or_else(|| SessionError::File("this graph has no file yet: use graph.saveAs {path}".into()))?,
    };
    save_to(s, path)
}

fn graph_save_as(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    // Relative path inputs were relative to the old folder (the editor's, unsaved): keep them
    // on the same files.
    let old = crate::paths::base_folder(s.path());
    let new = path.parent().map(Path::to_path_buf);
    if let (Some(old), Some(new)) = (old, new) {
        let rebased = crate::paths::rebase_paths(s.graph(), s.catalog(), &old, &new);
        if &rebased != s.graph() {
            s.edit("Save As", |g, _| {
                *g = rebased;
                Ok(())
            })?;
        }
    }
    save_to(s, path)
}

fn graph_info(s: &mut Session, _: &Value) -> Result<Value> {
    let mut out = summary(s);
    out["problemCount"] = json!(s.problems().len());
    Ok(out)
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "graph.open",
            label: "Open…",
            menu: &["File"],
            shortcut: Some("Cmd+O"),
            params: "{path, discard?: bool} -> {path, dirty, nodes, wires, undo, redo, problemCount, problems: [text] (nodes and wires the catalogue doesn't fit; the file still opens), unpackedFrom?, openedFrom? (the file an application's opener read, say a picture's recipe: the graph opens unsaved, so Save asks where), missingEngines?: {engines, kinds, text} (what the catalogue lacks, and which engines the file names for it)} (a workflow, a .zip bundle, unpacked into a new folder beside it, or a file an application opens; replaces the graph and clears undo; refused over unsaved changes unless discard is true: the UI asks first)",
            enabled: always,
            run: graph_open,
            journal: true,
        },
        CommandSpec {
            id: "graph.save",
            label: "Save",
            menu: &["File"],
            shortcut: Some("Cmd+S"),
            params: "{path?} -> {path, dirty, nodes, wires, undo, redo} (to the current file when path is omitted; an error if there is none)",
            enabled: always,
            run: graph_save,
            journal: true,
        },
        CommandSpec {
            id: "graph.saveAs",
            label: "Save As…",
            menu: &["File"],
            shortcut: Some("Cmd+Shift+S"),
            params: "{path} -> {path, dirty, nodes, wires, undo, redo} (relative path inputs are re-anchored to the new folder: an undo step when any changes)",
            enabled: always,
            run: graph_save_as,
            journal: true,
        },
        CommandSpec {
            id: "graph.bundle",
            label: "Export Bundle…",
            menu: &["File"],
            shortcut: None,
            params: "{path: .zip} -> {path, entries, missing: [{node, port, path, why}]} (the workflow and the files it reads in one zip, written atomically; graph.open unpacks one into a new folder beside it)",
            enabled: has_nodes,
            run: graph_bundle,
            journal: true,
        },
        CommandSpec {
            id: "graph.info",
            label: "Graph Info",
            menu: &[],
            shortcut: None,
            params: "{} -> {path, dirty, nodes, wires, undo, redo, problemCount}",
            enabled: always,
            run: graph_info,
            journal: false,
        },
    ]
}

fn app_quit(_: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"quit": true}))
}

fn app_about(s: &mut Session, _: &Value) -> Result<Value> {
    s.views.about = true;
    Ok(
        json!({"title": s.about.title, "version": s.about.version, "parts": s.about.parts, "engine": s.catalog().engine.name, "components": s.catalog().engine.components}),
    )
}

/// `app.about`, in the Help menu.
pub(crate) fn about_specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "app.about",
        label: "About",
        menu: &["Help"],
        shortcut: None,
        params: "{} -> {title, version, parts: [[name, version]], engine, components} (opens the About window: the application, what it was built from, and the connected engine's components)",
        enabled: always,
        run: app_about,
        journal: false,
    }]
}

/// `app.quit`, last in the File menu. The session has nothing to do: the editor closes its
/// window, first asking over unsaved changes (headless, it does nothing).
pub(crate) fn quit_specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "app.quit",
        label: "Quit",
        menu: &["File"],
        shortcut: Some("Cmd+Q"),
        params: "{} -> {quit: true} (the editor closes its window, as its close button does; over unsaved changes it asks first: Save, Discard or Cancel. Headless it changes nothing)",
        enabled: always,
        run: app_quit,
        journal: false,
    }]
}
