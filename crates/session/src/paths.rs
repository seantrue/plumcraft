//! Files a workflow refers to through its path inputs (ports with a `path` hint: a file a node
//! reads, one it writes, a folder). A relative path is relative to the workflow's folder, so a
//! workflow and the files beside it move together. The engine gets absolute paths
//! ([`with_absolute_paths`], used by `run.start`); a path chosen in a dialog is stored relative
//! when it lies inside the workflow's folder ([`chosen_path`]), and Save As to another folder
//! re-anchors relative paths ([`rebase_paths`]).
//!
//! Lifted from the research software's `crates/engine/src/paths.rs` (2026-10-27, the research software `8d96021`; plan
//! 10.3, E2) without edit folders, plus `rebase_paths` from its `bundle.rs`.

use std::path::{Path, PathBuf};

use plumcraft_graph::{Graph, NodeId, resolve_input};
use plumcraft_proto::catalogue::SchemaReply;
use serde_json::Value;

/// The user's home: `$HOME`, else `%USERPROFILE%` (Windows sets no `HOME`).
pub fn home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"].into_iter().filter_map(std::env::var_os).find(|h| !h.is_empty()).map(PathBuf::from)
}

/// `os.path.expanduser`: a leading `~` (alone, or before a separator: `/`, and `\` on Windows)
/// becomes [`home_dir`]. Anything else, and `~` with no home, is unchanged.
pub fn expand_user(path: &str) -> PathBuf {
    let Some(rest) = path.strip_prefix('~') else { return PathBuf::from(path) };
    let separator = |c: char| c == '/' || (cfg!(windows) && c == '\\');
    match home_dir() {
        Some(home) if rest.is_empty() || rest.starts_with(separator) => home.join(rest.trim_start_matches(separator)),
        _ => PathBuf::from(path),
    }
}

/// Is `p` a path to leave as it is (absolute, or from the home folder)? On Windows a path
/// with a root but no drive (`/tmp/x.png`) counts too: joining it to a folder would put it
/// on that folder's drive.
pub fn is_anchored(p: &str) -> bool {
    let path = Path::new(p);
    path.is_absolute() || path.has_root() || p.starts_with('~')
}

/// `p` as a workflow stores it: with `/` between folders on every platform, so a workflow saved
/// on Windows opens on macOS and Linux. (On Unix `\` is a character of a file name, not a
/// separator, and is kept.)
pub fn stored(p: &Path) -> String {
    let text = p.to_string_lossy();
    if cfg!(windows) { text.replace('\\', "/") } else { text.into_owned() }
}

/// Every path input with a text value: (node, port, value). Nodes of kinds the catalogue
/// doesn't have are skipped (their ports are unknown).
pub fn path_values(graph: &Graph, catalog: &SchemaReply) -> Vec<(NodeId, String, String)> {
    let mut out = Vec::new();
    for (id, node) in &graph.nodes {
        let Some(def) = catalog.node(&node.kind) else { continue };
        for (key, value) in &node.values {
            let Some(text) = value.as_str().filter(|t| !t.trim().is_empty()) else { continue };
            if resolve_input(def, node, id, key).is_ok_and(|r| r.port.path.is_some()) {
                out.push((id.clone(), key.clone(), text.to_owned()));
            }
        }
    }
    out
}

/// `graph` with each relative path input made absolute against `base` (the workflow's
/// folder). Without a base, paths are left as they are.
pub fn with_absolute_paths(graph: &Graph, catalog: &SchemaReply, base: Option<&Path>) -> Graph {
    let Some(base) = base else { return graph.clone() };
    let mut g = graph.clone();
    for (id, port, text) in path_values(graph, catalog) {
        let value = each_line(&text, |p| if is_anchored(p) { p.to_owned() } else { stored(&base.join(p)) });
        if value != text
            && let Some(n) = g.nodes.get_mut(&id)
        {
            n.values.insert(port, Value::String(value));
        }
    }
    g
}

/// `f` applied to each line of a path value (a `files` value holds one path per line; any other
/// is one line). Blank lines are dropped from a value of several lines.
pub fn each_line(text: &str, f: impl Fn(&str) -> String) -> String {
    if !text.contains('\n') {
        return f(text);
    }
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(f).collect::<Vec<_>>().join("\n")
}

/// The folder relative path inputs are relative to: the workflow's own, or, for a workflow
/// not saved yet, the folder the editor was started in.
/// A workflow file's JSON with its relative path inputs made absolute against the file's folder:
/// what the CLI and the viewer send, so every tool reads a workflow's paths the same way as the
/// editor does. A workflow that doesn't parse as a graph is returned unchanged (the engine
/// reports what's wrong with it).
pub fn anchor_workflow(workflow: &serde_json::Value, catalog: &SchemaReply, workflow_path: &Path) -> serde_json::Value {
    let Ok(graph) = serde_json::from_value::<Graph>(workflow.clone()) else { return workflow.clone() };
    let anchored = with_absolute_paths(&graph, catalog, base_folder(Some(workflow_path)).as_deref());
    serde_json::to_value(anchored).unwrap_or_else(|_| workflow.clone())
}

pub fn base_folder(workflow: Option<&Path>) -> Option<PathBuf> {
    match workflow.and_then(Path::parent) {
        Some(dir) if !dir.as_os_str().is_empty() => Some(dir.to_path_buf()),
        _ => std::env::current_dir().ok(),
    }
}

/// A path a dialog returned, as the path input stores it: relative to `base` when it lies
/// inside it (so the workflow can move with its files), else as it is. macOS's save panel
/// turns a `/` typed into the name into `:` (`prints/a.png` comes back as a file
/// `prints:a.png`): typed that way, it means folders, so for a save path (`save`) `:` in the
/// name becomes `/` again.
pub fn chosen_path(chosen: &Path, base: Option<&Path>, save: bool) -> String {
    let fixed = match (save, chosen.parent(), chosen.file_name().and_then(|n| n.to_str())) {
        (true, Some(dir), Some(name)) if name.contains(':') => name.split(':').filter(|part| !part.is_empty()).fold(dir.to_path_buf(), |p, part| p.join(part)),
        _ => chosen.to_path_buf(),
    };
    match base {
        Some(b) => relative_to(&fixed, b),
        None => stored(&fixed),
    }
}

/// `path` relative to `base` when it lies inside it (for storing in a workflow), else as it is.
pub fn relative_to(path: &Path, base: &Path) -> String {
    stored(path.strip_prefix(base).unwrap_or(path))
}

/// `graph`'s relative path inputs, re-anchored from folder `old` to `new` (Save As to another
/// folder): relative to `new` when the file lies inside it, absolute otherwise.
pub fn rebase_paths(graph: &Graph, catalog: &SchemaReply, old: &Path, new: &Path) -> Graph {
    if old == new {
        return graph.clone();
    }
    let mut g = graph.clone();
    for (node, port, text) in path_values(graph, catalog) {
        let value = each_line(&text, |p| if is_anchored(p) { p.to_owned() } else { relative_to(&old.join(p), new) });
        if value != text
            && let Some(n) = g.nodes.get_mut(&node)
        {
            n.values.insert(port, Value::String(value));
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_graph::PortRef;
    use serde_json::json;

    fn catalog() -> SchemaReply {
        plumcraft_text::engine().schema()
    }

    #[test]
    fn a_leading_tilde_expands_to_the_home_folder() {
        let Some(home) = home_dir() else { return };
        assert_eq!(expand_user("~"), home);
        assert_eq!(expand_user("~/a/b.txt"), home.join("a/b.txt"));
        assert_eq!(expand_user("~user/x"), PathBuf::from("~user/x"));
        assert_eq!(expand_user("/x/~/y"), PathBuf::from("/x/~/y"));
    }

    #[test]
    fn relative_path_inputs_resolve_against_the_workflow_folder() {
        let c = catalog();
        let mut g = Graph::new();
        let load = g.add_node(c.node("text.load").unwrap(), [0.0, 0.0]);
        let abs = g.add_node(c.node("text.load").unwrap(), [0.0, 0.0]);
        let home = g.add_node(c.node("text.load").unwrap(), [0.0, 0.0]);
        let lit = g.add_node(c.node("text.literal").unwrap(), [0.0, 0.0]);
        g.set_value(&PortRef::new(load.clone(), "path"), json!("notes/corgi.txt")).unwrap();
        g.set_value(&PortRef::new(abs.clone(), "path"), json!("/tmp/x.txt")).unwrap();
        g.set_value(&PortRef::new(home.clone(), "path"), json!("~/x.txt")).unwrap();
        g.set_value(&PortRef::new(lit.clone(), "text"), json!("notes/not-a-path.txt")).unwrap();
        assert_eq!(path_values(&g, &c).len(), 3, "a literal's text is no path");
        let r = with_absolute_paths(&g, &c, Some(Path::new("/work/flows")));
        let v = |n: &NodeId, p: &str| r.node(n).unwrap().values[p].clone();
        assert_eq!(v(&load, "path"), json!("/work/flows/notes/corgi.txt"));
        assert_eq!(v(&abs, "path"), json!("/tmp/x.txt"));
        assert_eq!(v(&home, "path"), json!("~/x.txt"));
        assert_eq!(v(&lit, "text"), json!("notes/not-a-path.txt"));
        assert_eq!(with_absolute_paths(&g, &c, None), g, "no base: unchanged");
        // Save As elsewhere: relative inside the new folder, absolute outside it
        let moved = rebase_paths(&g, &c, Path::new("/work/flows"), Path::new("/work"));
        assert_eq!(moved.node(&load).unwrap().values["path"], json!("flows/notes/corgi.txt"));
        let away = rebase_paths(&g, &c, Path::new("/work/flows"), Path::new("/other"));
        assert_eq!(away.node(&load).unwrap().values["path"], json!("/work/flows/notes/corgi.txt"));
        assert_eq!(rebase_paths(&g, &c, Path::new("/a"), Path::new("/a")), g);
    }

    #[test]
    fn chosen_paths_are_stored_relative_inside_the_folder() {
        let base = Path::new("/work/flows");
        assert_eq!(chosen_path(Path::new("/work/flows/out/x.txt"), Some(base), false), "out/x.txt");
        assert_eq!(chosen_path(Path::new("/elsewhere/x.txt"), Some(base), false), "/elsewhere/x.txt");
        assert_eq!(chosen_path(Path::new("/work/flows/x.txt"), None, false), "/work/flows/x.txt");
        // a save dialog's typed folders are folders; an open path keeps its name
        assert_eq!(chosen_path(Path::new("/work/flows/prints:a.wav"), Some(base), true), "prints/a.wav");
        assert_eq!(chosen_path(Path::new("/work/flows/a:b.txt"), Some(base), false), "a:b.txt");
        assert_eq!(relative_to(Path::new("/elsewhere/a.txt"), base), "/elsewhere/a.txt");
    }

    #[test]
    fn a_files_value_is_one_path_per_line() {
        let abs = |p: &str| if is_anchored(p) { p.to_owned() } else { format!("/base/{p}") };
        assert_eq!(each_line("a.png\n\n /x/b.png \nc/d.png", abs), "/base/a.png\n/x/b.png\n/base/c/d.png");
        assert_eq!(each_line("one.png", abs), "/base/one.png");
    }

    #[test]
    fn an_unsaved_workflows_paths_are_relative_to_the_start_folder() {
        assert_eq!(base_folder(Some(Path::new("/work/flows/a.json"))), Some(PathBuf::from("/work/flows")));
        assert_eq!(base_folder(Some(Path::new("a.json"))), std::env::current_dir().ok());
        assert_eq!(base_folder(None), std::env::current_dir().ok());
    }
}
