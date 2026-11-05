//! E2: undo and redo, copy, cut and paste, workflow files, relative paths and the dirty flag,
//! against the text engine's catalogue.

use std::path::{Path, PathBuf};

use plumcraft_graph::{Graph, NodeId, PortRef};
use plumcraft_session::run::Outgoing;
use plumcraft_session::{Session, SessionError, UNDO_LIMIT};
use serde_json::{Value, json};

fn session() -> Session {
    let mut s = Session::new(plumcraft_text::engine().schema());
    s.connected = true;
    s
}

fn add(s: &mut Session, kind: &str) -> String {
    s.execute("node.add", &json!({"kind": kind})).unwrap()["id"].as_str().unwrap().to_owned()
}

fn at(node: &str, port: &str) -> Value {
    json!({"node": node, "port": port})
}

/// A fresh folder under the system temp folder, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = plumcraft_scratch::root().join(format!("e2-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.contains(".tmp-")).collect()
}

/// Every graph command is one undo step with its label: undo gives back the graph before it,
/// redo the graph after it.
#[test]
fn every_graph_command_undoes_and_redoes() {
    let mut s = session();
    let a = add(&mut s, "text.literal");
    let u = add(&mut s, "text.upper");
    let w = add(&mut s, "text.words");
    s.execute("wire.connect", &json!({"from": at(&a, "text"), "to": at(&u, "text")})).unwrap();
    let steps: Vec<(&str, Value, &str)> = vec![
        ("node.add", json!({"kind": "text.lower", "pos": [10, 20]}), "Add Node"),
        ("node.set", json!({"node": a, "port": "text", "value": "the corgi naps"}), "Set Value"),
        ("node.title", json!({"node": a, "title": "story"}), "Rename Node"),
        ("wire.connect", json!({"from": at(&u, "text"), "to": at(&w, "text")}), "Connect"),
        ("wire.disconnect", json!({"to": at(&u, "text")}), "Disconnect"),
        ("node.duplicate", json!({"nodes": [a, u]}), "Duplicate"),
        ("node.remove", json!({"nodes": [w]}), "Remove Node"),
        ("node.remove", json!({"nodes": [a, u]}), "Remove Nodes"),
        ("edit.cut", json!({"nodes": [u]}), "Cut Node"),
        ("edit.paste", json!({"text": s.execute("edit.copy", &json!({"nodes": [a, u]})).unwrap()["text"]}), "Paste Nodes"),
        ("graph.new", json!({"discard": true}), "New Graph"),
    ];
    for (id, params, label) in steps {
        let before = s.graph().clone();
        s.execute(id, &params).unwrap_or_else(|e| panic!("{id}: {e}"));
        let after = s.graph().clone();
        assert_ne!(after, before, "{id} changed the graph");
        assert_eq!(s.undo_label(), Some(label), "{id}");
        assert!(s.redo_label().is_none(), "{id}: a new edit clears redo");
        let r = s.execute("edit.undo", &Value::Null).unwrap();
        assert_eq!(r["undid"], label);
        assert_eq!(s.graph(), &before, "{id}: undone");
        assert_eq!(s.redo_label(), Some(label));
        s.execute("edit.redo", &Value::Null).unwrap();
        assert_eq!(s.graph(), &after, "{id}: redone");
        s.execute("edit.undo", &Value::Null).unwrap();
    }
}

#[test]
fn moves_and_views_are_saved_but_not_undo_steps() {
    let mut s = session();
    let a = add(&mut s, "text.literal");
    let dir = TempDir::new("moves");
    s.execute("graph.saveAs", &json!({"path": dir.path().join("m.json")})).unwrap();
    assert!(!s.is_dirty());
    s.execute("node.move", &json!({"id": a, "pos": [55, 66]})).unwrap();
    assert!(s.is_dirty(), "positions are saved, so a move makes the file dirty");
    assert_eq!(s.undo_label(), Some("Add Node"));
    s.execute("graph.save", &json!({})).unwrap();
    s.execute("node.view", &json!({"nodes": [a], "view": "compact"})).unwrap();
    assert!(s.is_dirty());
    assert_eq!(s.undo_label(), Some("Add Node"));
}

#[test]
fn a_cache_setting_is_saved_sent_with_the_workflow_and_undone() {
    let mut s = session();
    let a = add(&mut s, "text.literal");
    let b = add(&mut s, "text.upper");
    s.execute("node.cache", &json!({"nodes": [a, b], "cache": "always"})).unwrap();
    assert_eq!(s.undo_label(), Some("Cache Setting"));
    let g = serde_json::to_value(s.graph()).unwrap();
    assert_eq!((g["nodes"][&a]["cache"].clone(), g["nodes"][&b]["cache"].clone()), (json!("always"), json!("always")), "{g}");
    // Saved and read back; `auto` (the default) isn't written; an unknown value reads as auto.
    s.execute("node.cache", &json!({"id": b, "cache": "auto"})).unwrap();
    let dir = TempDir::new("cache");
    let path = dir.path().join("c.json");
    s.execute("graph.saveAs", &json!({"path": path})).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let back: Graph = serde_json::from_str(&text).unwrap();
    assert_eq!(back.node(&NodeId::from(a.as_str())).unwrap().cache, plumcraft_graph::NodeCache::Always);
    assert_eq!(text.matches("\"cache\"").count(), 1, "{text}");
    let odd: Graph = serde_json::from_str(&text.replace("\"always\"", "\"sometimes\"")).unwrap();
    assert_eq!(odd.node(&NodeId::from(a.as_str())).unwrap().cache, plumcraft_graph::NodeCache::Auto);
    s.execute("edit.undo", &Value::Null).unwrap();
    s.execute("edit.undo", &Value::Null).unwrap();
    assert!(s.graph().nodes.values().all(|n| n.cache == plumcraft_graph::NodeCache::Auto));
    for bad in [json!({"nodes": [a], "cache": "sometimes"}), json!({"nodes": ["nope"], "cache": "never"}), json!({"nodes": [a]})] {
        assert!(s.execute("node.cache", &bad).is_err(), "{bad}");
    }
}

#[test]
fn the_history_keeps_the_last_200_steps_and_a_new_edit_clears_redo() {
    let mut s = session();
    let a = add(&mut s, "text.literal");
    for i in 0..UNDO_LIMIT + 50 {
        s.execute("node.set", &json!({"node": a, "port": "text", "value": format!("v{i}")})).unwrap();
    }
    assert_eq!(s.undo_depth(), UNDO_LIMIT);
    for _ in 0..UNDO_LIMIT {
        s.execute("edit.undo", &Value::Null).unwrap();
    }
    assert!(s.execute("edit.undo", &Value::Null).unwrap_err().to_string().contains("nothing to undo"));
    // the oldest steps are gone: the value 200 steps back, not the node's default
    assert_eq!(s.graph().node(&NodeId::from(a.as_str())).unwrap().values["text"], json!("v49"));
    s.execute("edit.redo", &Value::Null).unwrap();
    s.execute("edit.redo", &Value::Null).unwrap();
    assert_eq!(s.redo_label(), Some("Set Value"));
    s.execute("node.set", &json!({"node": a, "port": "text", "value": "new"})).unwrap();
    assert!(s.redo_label().is_none());
    assert!(s.execute("edit.redo", &Value::Null).unwrap_err().to_string().contains("nothing to redo"));
}

#[test]
fn copy_and_paste_give_fresh_ids_and_keep_the_wires_among_them() {
    let mut s = session();
    let src = add(&mut s, "text.literal");
    let a = add(&mut s, "text.literal");
    let u = add(&mut s, "text.upper");
    s.execute("node.move", &json!({"id": a, "pos": [100, 50]})).unwrap();
    s.execute("node.move", &json!({"id": u, "pos": [400, 80]})).unwrap();
    s.execute("node.set", &json!({"node": a, "port": "text", "value": "corgi"})).unwrap();
    s.execute("wire.connect", &json!({"from": at(&a, "text"), "to": at(&u, "text")})).unwrap();
    let w = add(&mut s, "text.words");
    s.execute("wire.connect", &json!({"from": at(&src, "text"), "to": at(&w, "text")})).unwrap();
    let journal = s.journal().len();
    let c = s.execute("edit.copy", &json!({"nodes": [a, u, a]})).unwrap();
    assert_eq!((c["nodes"].as_u64(), c["wires"].as_u64()), (Some(2), Some(1)));
    assert_eq!(s.journal().len(), journal, "copy isn't journaled");
    let text = c["text"].as_str().unwrap().to_owned();
    assert!(Graph::from_json(&text).is_ok(), "the clipboard is workflow JSON");

    // into the same graph: fresh ids, 40 px on, the wire among them kept
    let r = s.execute("edit.paste", &json!({"text": text})).unwrap();
    let (na, nu) = (r["map"][&a].as_str().unwrap().to_owned(), r["map"][&u].as_str().unwrap().to_owned());
    assert!(![&src, &a, &u, &w].contains(&&na) && na != nu);
    let g = s.graph();
    assert_eq!(g.node(&NodeId::from(na.as_str())).unwrap().pos, [140.0, 90.0]);
    assert_eq!(g.node(&NodeId::from(na.as_str())).unwrap().values["text"], json!("corgi"));
    assert_eq!(g.wire_into(&PortRef::new(nu.as_str(), "text")).unwrap().from, PortRef::new(na.as_str(), "text"));
    assert_eq!(g.wires.len(), 3);
    assert_eq!(s.undo_label(), Some("Paste Nodes"));

    // into another graph, at a position: the top-left goes there, the layout is kept
    let mut t = session();
    let r = t.execute("edit.paste", &json!({"text": text, "pos": [1000, 500]})).unwrap();
    let pos = |id: &Value| t.graph().node(&NodeId::from(id.as_str().unwrap())).unwrap().pos;
    assert_eq!((pos(&r["map"][&a]), pos(&r["map"][&u])), ([1000.0, 500.0], [1300.0, 530.0]));

    // cut: one step that copies and removes
    let r = s.execute("edit.cut", &json!({"nodes": [na, nu]})).unwrap();
    assert_eq!(r["nodes"], 2);
    assert_eq!(s.graph().nodes.len(), 4);
    s.execute("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.graph().nodes.len(), 6);

    // junk and unknown kinds are refused, leaving the graph alone
    let before = s.graph().clone();
    for bad in [json!({"text": "not json"}), json!({"text": "{\"version\": 1}"}), json!({}), json!({"text": 5}), json!({"text": text, "pos": [1]})] {
        assert!(s.execute("edit.paste", &bad).is_err(), "{bad}");
    }
    let alien = r#"{"version": 1, "nodes": {"n1": {"kind": "text.teleport"}}}"#;
    assert!(s.execute("edit.paste", &json!({"text": alien})).unwrap_err().to_string().contains("text.teleport"));
    // a wire that no longer type-checks is dropped, not fatal
    let mistyped = json!({"version": 1, "nodes": {"n1": {"kind": "text.words"}, "n2": {"kind": "text.upper"}},
        "wires": [{"from": {"node": "n1", "port": "count"}, "to": {"node": "n2", "port": "text"}}]});
    let r = t.execute("edit.paste", &json!({"text": mistyped.to_string()})).unwrap();
    assert_eq!(r["dropped"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(s.graph(), &before);
    for bad in [json!({"nodes": ["n99"]}), json!({"nodes": []}), json!({})] {
        assert!(s.execute("edit.copy", &bad).is_err() && s.execute("edit.cut", &bad).is_err(), "{bad}");
    }
}

/// Save, open and save again: values, positions, titles, views and wires come back, and the
/// second file is byte for byte the first.
#[test]
fn save_and_open_round_trip() {
    let dir = TempDir::new("round");
    let path = dir.path().join("flow.json");
    let mut s = session();
    let a = add(&mut s, "text.literal");
    let u = add(&mut s, "text.upper");
    let u2 = add(&mut s, "text.upper");
    s.execute("node.set", &json!({"node": a, "port": "text", "value": "a corgi on a floral carpet"})).unwrap();
    s.execute("node.title", &json!({"node": a, "title": "story"})).unwrap();
    s.execute("node.move", &json!({"id": u, "pos": [321.5, -40]})).unwrap();
    s.execute("node.view", &json!({"nodes": [u2], "view": "collapsed"})).unwrap();
    s.execute("wire.connect", &json!({"from": at(&a, "text"), "to": at(&u, "text")})).unwrap();
    assert!(matches!(s.execute("graph.save", &json!({})), Err(SessionError::File(m)) if m.contains("saveAs")));
    let info = s.execute("graph.saveAs", &json!({"path": path})).unwrap();
    assert_eq!((info["dirty"].clone(), info["nodes"].clone()), (json!(false), json!(3)));
    assert_eq!(s.path(), Some(path.as_path()));
    assert!(leftovers(dir.path()).is_empty());

    let mut t = session();
    add(&mut t, "text.words");
    let info = t.execute("graph.open", &json!({"path": path, "discard": true})).unwrap();
    assert_eq!(
        (info["nodes"].clone(), info["wires"].clone(), info["dirty"].clone(), info["problemCount"].clone()),
        (json!(3), json!(1), json!(false), json!(0))
    );
    assert_eq!(t.graph().nodes, s.graph().nodes);
    assert_eq!(t.graph().wires, s.graph().wires);
    assert!(t.undo_label().is_none(), "opening clears the history");
    assert_eq!(t.execute("graph.info", &Value::Null).unwrap()["path"], json!(path.display().to_string()));
    let again = dir.path().join("again.json");
    t.execute("graph.saveAs", &json!({"path": again})).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), std::fs::read(&again).unwrap(), "byte for byte");
    // ids keep counting after the file's
    assert_eq!(add(&mut t, "text.lower"), "n4");
    // edit, save to the current file, reopen
    t.execute("node.set", &json!({"node": a, "port": "text", "value": "two corgis"})).unwrap();
    assert!(t.is_dirty());
    t.execute("graph.save", &Value::Null).unwrap();
    assert!(!t.is_dirty());
    s.execute("graph.open", &json!({"path": again})).unwrap();
    assert_eq!(s.graph().node(&NodeId::from(a.as_str())).unwrap().values["text"], json!("two corgis"));
}

#[test]
fn unsaved_changes_are_kept_unless_discarded() {
    let dir = TempDir::new("discard");
    let path = dir.path().join("d.json");
    let mut s = session();
    add(&mut s, "text.literal");
    assert!(s.is_dirty());
    assert!(matches!(s.execute("graph.new", &Value::Null), Err(SessionError::Unsaved)));
    assert_eq!(s.graph().nodes.len(), 1);
    s.execute("graph.saveAs", &json!({"path": path})).unwrap();
    add(&mut s, "text.upper");
    assert!(matches!(s.execute("graph.open", &json!({"path": path})), Err(SessionError::Unsaved)));
    assert!(s.execute("graph.open", &json!({"path": path, "discard": "yes"})).is_err());
    assert_eq!(s.graph().nodes.len(), 2);
    s.execute("graph.open", &json!({"path": path, "discard": true})).unwrap();
    assert_eq!(s.graph().nodes.len(), 1);
    // clean: no discard needed; a new graph forgets the file
    s.execute("graph.new", &Value::Null).unwrap();
    assert!(s.path().is_none() && !s.is_dirty());
    assert!(s.execute("graph.save", &Value::Null).is_err());
    s.execute("graph.open", &json!({"path": path})).unwrap();
}

/// Relative path inputs resolve against the workflow's folder when running, and Save As to
/// another folder keeps them on the same files.
#[test]
fn relative_paths_follow_the_workflow() {
    let dir = TempDir::new("paths");
    std::fs::create_dir_all(dir.path().join("notes")).unwrap();
    std::fs::write(dir.path().join("notes/corgi.txt"), "a corgi").unwrap();
    let mut s = session();
    let load = add(&mut s, "text.load");
    s.execute("graph.saveAs", &json!({"path": dir.path().join("w.json")})).unwrap();
    s.execute("node.set", &json!({"node": load, "port": "path", "value": "notes/corgi.txt"})).unwrap();
    s.execute("graph.save", &json!({})).unwrap();
    let sent = |s: &mut Session| -> Value {
        s.execute("run.start", &json!({})).unwrap();
        let req = s.run.take_outbox().into_iter().find_map(|o| if let Outgoing::Run(r) = o { Some(r) } else { None }).unwrap();
        let v = serde_json::to_value(&*req).unwrap();
        v["graph"]["graph"]["nodes"][&load]["values"]["path"].clone()
    };
    let abs = dir.path().join("notes/corgi.txt");
    assert_eq!(sent(&mut s), json!(slashed(&abs)));
    assert_eq!(s.graph().node(&NodeId::from(load.as_str())).unwrap().values["path"], json!("notes/corgi.txt"), "the graph keeps it relative");
    // Save As into a subfolder: the path goes up again; into the same folder: unchanged
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    s.execute("graph.saveAs", &json!({"path": dir.path().join("sub/w.json")})).unwrap();
    assert_eq!(s.undo_label(), Some("Save As"));
    let p = s.graph().node(&NodeId::from(load.as_str())).unwrap().values["path"].clone();
    assert_eq!(p, json!(slashed(&abs)), "outside the new folder: absolute");
    s.execute("graph.saveAs", &json!({"path": dir.path().join("w2.json")})).unwrap();
    assert_eq!(s.graph().node(&NodeId::from(load.as_str())).unwrap().values["path"], p, "absolute stays absolute");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.path().join("w.json")).unwrap()).unwrap();
    assert_eq!(saved["nodes"][&load]["values"]["path"], json!("notes/corgi.txt"), "saved relative");
    // a fresh file in the folder, set relative, runs absolute
    s.execute("node.set", &json!({"node": load, "port": "path", "value": "notes/corgi.txt"})).unwrap();
    assert_eq!(sent(&mut s), json!(slashed(&abs)));
}

/// Frames through a real file: what a person did to them is what the next session reads; opening
/// clears the undo history but not the frames; and a file's frames come in with Insert, below
/// what is on the canvas.
#[test]
fn frames_are_saved_read_back_and_inserted() {
    let dir = TempDir::new("frames");
    let path = dir.path().join("framed.json");
    let mut s = session();
    let n = add(&mut s, "text.literal");
    s.execute("node.move", &json!({"id": n, "pos": [50, 60]})).unwrap();
    let id = s.execute("frame.add", &json!({"title": "inputs", "nodes": [n], "sizes": {n.as_str(): [100, 40]}})).unwrap()["id"].as_str().unwrap().to_owned();
    s.execute("frame.set", &json!({"frame": id, "color": "#3f8f5a"})).unwrap();
    s.execute("frame.move", &json!({"frame": id, "pos": [500, 700]})).unwrap();
    s.execute("frame.resize", &json!({"frame": id, "size": [640, 480]})).unwrap();
    s.execute("graph.saveAs", &json!({"path": path})).unwrap();
    assert!(!s.is_dirty());
    let saved = s.graph().frames.clone();
    let node = s.graph().node(&NodeId::from(n.as_str())).unwrap().pos;

    let mut back = session();
    back.execute("graph.open", &json!({"path": path})).unwrap();
    assert_eq!(back.graph().frames, saved);
    assert_eq!(back.graph().node(&NodeId::from(n.as_str())).unwrap().pos, node, "the node went with its frame");
    assert!(!back.is_dirty());
    // ids go on from the file's
    let next = back.execute("frame.add", &json!({"pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
    assert_ne!(next, id);
    back.execute("edit.undo", &Value::Null).unwrap();
    assert_eq!(back.graph().frames, saved);

    let f = saved.values().next().unwrap();
    let r = back.execute("graph.insert", &json!({"path": path})).unwrap();
    let new = r["frames"][0].as_str().unwrap();
    assert_ne!(new, id);
    let inserted = &back.graph().frames[&new.into()];
    assert_eq!((&inserted.title, &inserted.color, inserted.size), (&f.title, &f.color, f.size));
    assert_eq!(inserted.pos[1], f.pos[1] + f.size[1] + plumcraft_session::insert_cmds::INSERT_GAP, "below the frame already there");
    back.execute("edit.undo", &Value::Null).unwrap();
    assert_eq!(back.graph().frames, saved, "one undo step");
}

/// Hostile files: errors that leave the session alone, never panics. A file whose nodes the
/// catalogue lacks opens with its problems reported, and stays editable.
#[test]
fn hostile_files() {
    let dir = TempDir::new("hostile");
    let mut s = session();
    add(&mut s, "text.literal");
    let before = s.graph().clone();
    let junk = dir.path().join("junk.json");
    let texts = [
        "",
        "{",
        "[]",
        "null",
        "\u{feff}{}",
        r#"{"version": 2, "nodes": {}}"#,
        r#"{"version": "1"}"#,
        r#"{"version": 1, "nodes": {"n1": {"kind": "text.upper"}}, "wires": [{"from": {"node": "n1", "port": "text"}, "to": {"node": "n9", "port": "text"}}]}"#,
        r#"{"version": 1, "nodes": {"n1": {"kind": 5}}}"#,
        r#"{"version": 1, "nodes": {}, "frames": {"f1": {"name": "no position"}}}"#,
        r#"{"version": 1, "nodes": {}, "frames": {"f1": {"pos": [0, 0], "size": [1e30, 10]}}}"#,
        r#"{"version": 1, "nodes": {}, "frames": {"f1": {"pos": [0, 0], "size": [-100, 10]}}}"#,
        r#"{"version": 1, "nodes": {}, "frames": {"f1": {"pos": [1e39, 0], "size": [100, 100]}}}"#,
        r#"{"version": 1, "nodes": {}, "frames": {"f1": {"pos": [0, 0], "size": [100, 100], "color": "red"}}}"#,
        r#"{"version": 1, "nodes": {}, "frames": ["f1"]}"#,
        r#"{"version": 1, "nodes": {}, "next_frame": -1}"#,
    ];
    for text in texts {
        std::fs::write(&junk, text).unwrap();
        assert!(s.execute("graph.open", &json!({"path": junk, "discard": true})).is_err(), "{text}");
    }
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    std::fs::write(&junk, deep).unwrap();
    assert!(s.execute("graph.open", &json!({"path": junk, "discard": true})).is_err(), "deep nesting");
    let huge = dir.path().join("huge.json");
    std::fs::File::create(&huge).unwrap().set_len(plumcraft_session::MAX_WORKFLOW_BYTES + 1).unwrap();
    assert!(s.execute("graph.open", &json!({"path": huge, "discard": true})).unwrap_err().to_string().contains("larger than"));
    assert!(s.execute("graph.open", &json!({"path": dir.path(), "discard": true})).is_err(), "a directory");
    assert!(s.execute("graph.open", &json!({"path": dir.path().join("missing.json"), "discard": true})).is_err());
    for bad in [json!({}), json!({"path": ""}), json!({"path": "a\u{0}b"}), json!({"path": 3})] {
        assert!(s.execute("graph.open", &bad).is_err(), "{bad}");
    }
    assert!(s.execute("graph.saveAs", &json!({"path": dir.path().join("no/such/dir.json")})).is_err());
    assert!(s.execute("graph.saveAs", &json!({"path": dir.path()})).is_err(), "over a directory");
    assert!(s.execute("graph.saveAs", &json!({"path": ""})).is_err());
    assert_eq!(s.graph(), &before);
    assert!(s.path().is_none() && s.is_dirty());
    assert!(leftovers(dir.path()).is_empty());

    // unknown kinds and a mistyped wire: it opens, with the problems listed
    let alien = dir.path().join("alien.json");
    std::fs::write(
        &alien,
        r#"{"version": 1, "nodes": {"n1": {"kind": "vi.op.load_image"}, "n2": {"kind": "text.words", "pos": [10, 10]}, "n3": {"kind": "text.upper"}},
            "wires": [{"from": {"node": "n2", "port": "count"}, "to": {"node": "n3", "port": "text"}}]}"#,
    )
    .unwrap();
    let r = s.execute("graph.open", &json!({"path": alien, "discard": true})).unwrap();
    assert_eq!(r["problemCount"], 2, "{r}");
    let listed = r["problems"].to_string();
    assert!(listed.contains("vi.op.load_image") && listed.contains("n2.count"), "{listed}");
    assert_ne!(s.graph().node(&NodeId::from("n3")).unwrap().pos, [0.0, 0.0], "unplaced nodes are placed");
    // still editable; an edit may not add a problem
    let lit = add(&mut s, "text.literal");
    s.execute("node.title", &json!({"node": "n1", "title": "from elsewhere"})).unwrap();
    assert!(s.execute("wire.connect", &json!({"from": at("n2", "count"), "to": at(&lit, "text")})).is_err());
    s.execute("wire.disconnect", &json!({"to": at("n3", "text")})).unwrap();
    assert_eq!(s.problems().len(), 1);
    s.execute("node.remove", &json!({"nodes": ["n1"]})).unwrap();
    assert!(s.problems().is_empty());
    assert!(s.graph().check(s.catalog()).is_ok());
}

/// An unsaved workflow's relative paths are relative to the editor's folder; Save As keeps
/// them on the same files.
#[test]
fn an_unsaved_workflows_paths_are_rebased_on_save_as() {
    let dir = TempDir::new("unsaved");
    let mut s = session();
    let load = add(&mut s, "text.load");
    s.execute("node.set", &json!({"node": load, "port": "path", "value": "README.md"})).unwrap();
    s.execute("graph.saveAs", &json!({"path": dir.path().join("w.json")})).unwrap();
    let cwd = std::env::current_dir().unwrap();
    assert_eq!(s.graph().node(&NodeId::from(load.as_str())).unwrap().values["path"], json!(slashed(&cwd.join("README.md"))));
}

/// A path as the session writes it: with `/` (on Windows too, which takes either).
fn slashed(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}
