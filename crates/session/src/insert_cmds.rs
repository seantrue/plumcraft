//! Inserting a workflow into the open graph: `graph.insert` (File › Insert Graph…) and
//! `graph.insertGroup` (File › Insert as Group…). The file's nodes, wires, groups and frames come
//! in under fresh ids, as one undo step ([`plumcraft_graph::Graph::insert_graph`]); as a group,
//! everything inserted is wrapped in a new group named after the file.
//!
//! - **Where:** with `pos`, the inserted graph's top-left goes there; otherwise below what is
//!   already on the canvas, so nothing lands on top of it.
//! - **Paths:** a relative path input meant the inserted file's folder; it is rebased onto the
//!   open graph's folder, or made absolute when the open graph has no file yet.
//!
//! Lifted from the research software's `crates/engine/src/insert_cmds.rs` (2026-10-27, the research software `8d96021`;
//! plan 10.3, E3). A bundle (`.zip`) unpacks beside itself first, as `graph.open` does.

use std::path::Path;

use plumcraft_graph::{Graph, GroupId, NodeId};
use plumcraft_proto::catalogue::SchemaReply;
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, opt_bool_param, opt_str_param, pos_param};
use crate::file_cmds::path_param;
use crate::{Result, Session, SessionError};

/// Space between what is on the canvas and a graph inserted below it.
pub const INSERT_GAP: f32 = 200.0;

/// The inserted graph's relative path inputs, rebased from `from` (its folder) onto `to` (the
/// open graph's folder), or made absolute when there is no `to`.
fn rebase(graph: &Graph, catalog: &SchemaReply, from: &Path, to: Option<&Path>) -> Graph {
    match to {
        Some(to) => crate::paths::rebase_paths(graph, catalog, from, to),
        None => crate::paths::with_absolute_paths(graph, catalog, Some(from)),
    }
}

/// The offset that puts `other` at `pos`, or below the open graph's content.
fn shift(open: &Graph, other: &Graph, pos: Option<[f32; 2]>) -> [f32; 2] {
    let min = other
        .nodes
        .values()
        .map(|n| n.pos)
        .chain(other.frames.values().map(|f| f.pos))
        .fold([f32::INFINITY, f32::INFINITY], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
    if !min[0].is_finite() || !min[1].is_finite() {
        return [0.0, 0.0];
    }
    let at = match pos {
        Some(p) => p,
        None => {
            let points = open.nodes.values().map(|n| n.pos).chain(open.frames.values().map(|f| [f.pos[0], f.pos[1] + f.size[1]]));
            let (left, bottom) = points.fold((f32::INFINITY, f32::NEG_INFINITY), |(l, b), p| (l.min(p[0]), b.max(p[1])));
            if left.is_finite() && bottom.is_finite() { [left, bottom + INSERT_GAP] } else { min }
        }
    };
    let s = [at[0] - min[0], at[1] - min[1]];
    if s[0].is_finite() && s[1].is_finite() { s } else { [0.0, 0.0] }
}

/// A group name from a file name: its stem, trimmed to a good name ([`plumcraft_graph::check_name`]).
fn name_from(path: &Path) -> String {
    let stem: String = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let clean: String = stem.chars().filter(|c| !c.is_control()).take(plumcraft_graph::MAX_TITLE_CHARS).collect();
    let clean = clean.trim();
    if clean.is_empty() { "inserted".to_owned() } else { clean.to_owned() }
}

fn insert(s: &mut Session, p: &Value, as_group: bool) -> Result<Value> {
    let path = path_param(p, "path")?;
    let as_group = as_group || opt_bool_param(p, "asGroup", false)?;
    let (file, path, bundle) = match crate::file_cmds::opener_for(&path) {
        Some(o) => (crate::file_cmds::read_opened(&path, &o)?, path, None),
        None => crate::archive::read_workflow_or_bundle(path)?,
    };
    let refused = if file.nodes.is_empty() {
        Some(SessionError::Params(format!("{} has no nodes to insert", path.display())))
    } else {
        file.check(s.catalog()).err().map(|e| SessionError::File(format!("can't insert {}: {e}", path.display())))
    };
    if let Some(e) = refused {
        // what a refused bundle unpacked isn't left behind
        if let (Some(_), Some(dest)) = (bundle, path.parent()) {
            let _ = std::fs::remove_dir_all(dest);
        }
        return Err(e);
    }
    s.check_trust_of(&file);
    let name = match opt_str_param(p, "name")? {
        Some(n) => n.to_owned(),
        None => name_from(&path),
    };
    let from = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let to = s.path().and_then(Path::parent).map(Path::to_path_buf);
    let other = rebase(&file, s.catalog(), &from, to.as_deref());
    let offset = shift(s.graph(), &other, pos_param(p, "pos")?);
    let label = if as_group { "Insert as Group" } else { "Insert Graph" };
    let (ins, group) = s.edit(label, |g, catalog| {
        let ins = g.insert_graph(catalog, &other, offset)?;
        let group = if as_group {
            let (nodes, groups) = ins.top_level(&other);
            // it arrives as one collapsed node, where the inserted graph's top-left lands
            let id = g.create_group(&name, &nodes, &groups)?;
            g.set_collapsed(&id, true)?;
            Some(id)
        } else {
            None
        };
        Ok((ins, group))
    })?;
    let nodes: Vec<&NodeId> = ins.nodes.values().collect();
    let groups: Vec<&GroupId> = ins.groups.values().collect();
    Ok(json!({
        "path": path.display().to_string(),
        "nodes": nodes,
        "groups": groups,
        "frames": ins.frames,
        "group": group,
        "nets": ins.nets,
        "dropped": ins.dropped.iter().map(|(w, why)| json!({"wire": crate::graph_cmds::wire_json(w), "reason": why})).collect::<Vec<_>>(),
    }))
}

fn graph_insert(s: &mut Session, p: &Value) -> Result<Value> {
    insert(s, p, false)
}

fn graph_insert_group(s: &mut Session, p: &Value) -> Result<Value> {
    insert(s, p, true)
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "graph.insert",
            label: "Insert Graph…",
            menu: &["File"],
            shortcut: None,
            params: "{path, pos?: [x, y], asGroup?: bool, name?: string} -> {path, nodes, groups, frames, group, dropped} (adds a workflow file to the open graph under fresh ids, its groups and frames kept; below what is there unless pos is given; with asGroup, all of it wrapped in a new collapsed group named after the file, or name. One undo step)",
            enabled: always,
            run: graph_insert,
            journal: true,
        },
        CommandSpec {
            id: "graph.insertGroup",
            label: "Insert as Group…",
            menu: &["File"],
            shortcut: None,
            params: "{path, pos?: [x, y], name?: string} -> {path, nodes, groups, frames, group, dropped} (graph.insert with asGroup: true)",
            enabled: always,
            run: graph_insert_group,
            journal: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> SchemaReply {
        serde_json::from_value(json!({"v": 2, "nodes": [
            {"id": "text.literal", "inputs": [{"name": "text", "type": "STRING", "default": "", "socket": "widget"}], "outputs": [{"name": "text", "type": "TEXT"}]},
            {"id": "text.upper", "inputs": [{"name": "text", "type": "TEXT", "socket": "socket"}], "outputs": [{"name": "text", "type": "TEXT"}]},
            {"id": "text.load", "inputs": [{"name": "path", "type": "STRING", "default": "", "socket": "widget", "path": {"mode": "open"}}], "outputs": [{"name": "text", "type": "TEXT"}]}
        ]}))
        .unwrap()
    }

    fn add(s: &mut Session, kind: &str, pos: [f32; 2]) -> String {
        s.execute("node.add", &json!({"kind": kind, "pos": pos})).unwrap()["id"].as_str().unwrap().to_owned()
    }

    /// A workflow file: load (a relative path) -> upper, literal; load and upper in a group
    /// "shout" with upper's output named.
    fn workflow(dir: &Path) -> std::path::PathBuf {
        let mut s = Session::new(catalog());
        let load = add(&mut s, "text.load", [0.0, 0.0]);
        let up = add(&mut s, "text.upper", [300.0, 300.0]);
        add(&mut s, "text.literal", [600.0, 0.0]);
        s.execute("node.set", &json!({"node": load, "port": "path", "value": "notes/corgi.txt"})).unwrap();
        s.execute("wire.connect", &json!({"from": {"node": load, "port": "text"}, "to": {"node": up, "port": "text"}})).unwrap();
        let gid = s.execute("group.create", &json!({"nodes": [load, up], "name": "shout"})).unwrap()["id"].as_str().unwrap().to_owned();
        s.execute("group.output", &json!({"group": gid, "from": {"node": up, "port": "text"}})).unwrap();
        // written as is (graph.saveAs would re-anchor the relative path to the editor's folder)
        let path = dir.join("Pencil Sketch.json");
        std::fs::write(&path, s.graph().to_json().unwrap()).unwrap();
        path
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let d = plumcraft_scratch::root().join(format!("e3-{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_workflow_is_inserted_below_the_graph() {
        let dir = tempdir("insert");
        let file = workflow(&dir);
        let mut s = Session::new(catalog());
        let mine = add(&mut s, "text.upper", [100.0, 50.0]);
        let r = s.execute("graph.insert", &json!({"path": file})).unwrap();
        assert_eq!(s.undo_label(), Some("Insert Graph"));
        let g = s.graph();
        assert_eq!((g.nodes.len(), g.wires.len()), (4, 1));
        assert!(r["group"].is_null());
        let gid = GroupId::from(r["groups"][0].as_str().unwrap());
        assert_eq!(g.group(&gid).unwrap().name, "shout");
        assert_eq!(g.group(&gid).unwrap().outputs[0].name, "text");
        let ys: Vec<f32> = r["nodes"].as_array().unwrap().iter().map(|n| g.node(&NodeId::from(n.as_str().unwrap())).unwrap().pos[1]).collect();
        assert_eq!(ys.iter().copied().fold(f32::INFINITY, f32::min), 50.0 + INSERT_GAP);
        assert!(g.node(&NodeId::from(mine.as_str())).is_some());
        // the open graph has no file: the relative path is now absolute
        let load = r["nodes"].as_array().unwrap().iter().map(|n| NodeId::from(n.as_str().unwrap())).find(|n| g.node(n).unwrap().kind == "text.load").unwrap();
        assert_eq!(g.node(&load).unwrap().values["path"], json!(crate::paths::stored(&dir.join("notes/corgi.txt"))));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.graph().nodes.len(), 1);
        assert!(s.graph().groups.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_workflow_is_inserted_as_a_group_at_a_place() {
        let dir = tempdir("insert-group");
        let file = workflow(&dir);
        let mut s = Session::new(catalog());
        let r = s.execute("graph.insertGroup", &json!({"path": file, "pos": [1000.0, 2000.0]})).unwrap();
        assert_eq!(s.undo_label(), Some("Insert as Group"));
        let g = s.graph();
        let outer = GroupId::from(r["group"].as_str().unwrap());
        assert_eq!(g.group(&outer).unwrap().name, "Pencil Sketch");
        // P04 (2026-10-27): it arrives as one collapsed node, at the place given
        assert!(g.group(&outer).unwrap().collapsed, "arrives collapsed");
        assert_eq!(g.group(&outer).unwrap().pos, [1000.0, 2000.0]);
        let inner = GroupId::from(r["groups"][0].as_str().unwrap());
        assert_eq!(g.group(&inner).unwrap().parent.as_ref(), Some(&outer));
        assert_eq!(g.group_members(&outer).len(), 3);
        let min = g.nodes.values().map(|n| n.pos).fold([f32::INFINITY, f32::INFINITY], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
        assert_eq!(min, [1000.0, 2000.0]);
        let r = s.execute("graph.insert", &json!({"path": file, "asGroup": true, "name": "second"})).unwrap();
        assert_eq!(s.graph().group(&GroupId::from(r["group"].as_str().unwrap())).unwrap().name, "second");
        assert!(s.execute("graph.insertGroup", &json!({"path": file, "name": "not\na name"})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_files_change_nothing() {
        let dir = tempdir("insert-bad");
        let mut s = Session::new(catalog());
        add(&mut s, "text.upper", [0.0, 0.0]);
        let before = s.graph().clone();
        let junk = dir.join("junk.json");
        std::fs::write(&junk, "{not json").unwrap();
        let alien = dir.join("alien.json");
        std::fs::write(&alien, r#"{"version": 1, "nodes": {"n1": {"kind": "vi.nothing"}}}"#).unwrap();
        let empty = dir.join("empty.json");
        std::fs::write(&empty, r#"{"version": 1}"#).unwrap();
        for p in [json!({"path": junk}), json!({"path": alien}), json!({"path": empty}), json!({"path": dir.join("missing.json")}), json!({})] {
            assert!(s.execute("graph.insert", &p).is_err(), "{p}");
        }
        assert_eq!(s.graph(), &before);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
