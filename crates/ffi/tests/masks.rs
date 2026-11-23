//! The `mask` value editor through the C ABI (S4b): Paint Mask names it, a run sends the picture
//! (and the mask wired in), `mask.stroke` paints and keeps each stroke as a new file beside the
//! document, one undo step each, `mask.clear` drops the edit, and a document with nowhere to write
//! says so until `mask.folder` names a folder. Scratch folders only.

mod common;

use common::{Client, store_root};
use serde_json::{Value, json};

fn result(c: &mut Client, cmd: &str, p: Value) -> Result<Value, String> {
    let r = c.call("command.run", json!({"command": cmd, "params": p}));
    if r["ok"] == true { Ok(r["result"].clone()) } else { Err(r["error"].as_str().unwrap_or_default().to_owned()) }
}

fn fs_names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).map(|r| r.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// An edit file of `node`'s (any bytes: pruning reads names, sidecars and times), with a sidecar
/// naming `editor` (none: no sidecar), modified `age_s` ago.
fn edit_file(dir: &std::path::Path, node: &str, hash: u64, editor: Option<&str>, age_s: u64) -> String {
    let name = format!("{node}-{hash:016x}.png");
    let then = std::time::SystemTime::now() - std::time::Duration::from_secs(age_s);
    let age = |p: &std::path::Path| std::fs::File::options().write(true).open(p).unwrap().set_times(std::fs::FileTimes::new().set_modified(then)).unwrap();
    std::fs::write(dir.join(&name), b"png").unwrap();
    age(&dir.join(&name));
    if let Some(e) = editor {
        let side = dir.join(format!("{name}.json"));
        std::fs::write(&side, json!({"editor": e, "node": node, "w": 1, "h": 1}).to_string()).unwrap();
        age(&side);
    }
    name
}

fn run(c: &mut Client) {
    c.run("run.start", json!({}));
    let done = c.call("ui.waitRun", json!({"timeout": 30}));
    assert_eq!(done["ok"], true, "{done}");
}

#[test]
fn paint_mask_strokes_are_files_beside_the_document_one_undo_step_each() {
    let root = std::path::PathBuf::from(store_root("masks"));
    let folder = root.join("doc");
    std::fs::create_dir_all(&folder).unwrap();
    let mut c = Client::open(&json!({"store_root": root.join("store"), "engines": {"default": "array", "mode": "inproc"}}));
    let src = c.run("node.add", json!({"kind": "array.mandelbrot", "pos": [0, 0]}))["id"].as_str().unwrap().to_owned();
    let pm = c.run("node.add", json!({"kind": "array.paint_mask", "pos": [300, 0]}))["id"].as_str().unwrap().to_owned();
    c.run("wire.connect", json!({"from": {"node": src, "port": "escape"}, "to": {"node": pm, "port": "picture"}}));
    // before a run there is nothing to paint on
    let e = result(&mut c, "mask.stroke", json!({"node": pm, "points": [[10, 10]]})).unwrap_err();
    assert!(e.contains("run it once"), "{e}");
    run(&mut c);
    let d = c.run("node.editorData", json!({"node": pm}));
    assert_eq!((d["kind"].as_str(), d["port"].as_str()), (Some("mask"), Some("edit")), "{d}");
    let (w, h) = (d["data"]["w"].as_u64().unwrap(), d["data"]["h"].as_u64().unwrap());
    assert_eq!(d["data"]["picture"]["kind"], "rgba8");
    assert_eq!((d["data"]["picture"]["w"].as_u64(), d["data"]["picture"]["h"].as_u64()), (Some(w), Some(h)), "the picture is the mask's size");
    // an unsaved graph has nowhere to write
    let e = result(&mut c, "mask.stroke", json!({"node": pm, "points": [[10, 10]]})).unwrap_err();
    assert!(e.contains("no folder for mask files"), "{e}");
    // saved: beside it
    let doc = folder.join("painted.json");
    c.run("graph.saveAs", json!({"path": doc}));
    let r = c.run("mask.stroke", json!({"node": pm, "points": [[10, 10], [60, 40]], "radius": 8}));
    let first = r["value"].as_str().unwrap().to_owned();
    assert!(first.starts_with("painted.edits/") && first.ends_with(".png"), "{r}");
    let file = folder.join(&first);
    assert!(file.is_file() && folder.join(format!("{first}.json")).is_file());
    let side: Value = serde_json::from_slice(&std::fs::read(folder.join(format!("{first}.json"))).unwrap()).unwrap();
    assert_eq!((side["editor"].as_str(), side["w"].as_u64()), (Some("plumcraft"), Some(w)));
    assert_eq!(c.call("ui.inspect", json!({}))["result"]["undo"], "Mask Stroke");
    let second = c.run("mask.stroke", json!({"node": pm, "tool": "erase", "points": [[10, 10]], "radius": 4}))["value"].as_str().unwrap().to_owned();
    assert_ne!(second, first);
    assert_eq!(c.run("mask.get", json!({"node": pm}))["file"].as_str(), Some(folder.join(&second).to_str().unwrap()));
    // one undo takes one stroke back; the file it named is still there
    c.run("edit.undo", json!({}));
    assert_eq!(c.run("mask.get", json!({"node": pm}))["value"], first);
    // the edit reaches the node's output
    run(&mut c);
    assert_eq!(c.call("ui.inspect", json!({}))["result"]["lastError"], Value::Null);
    // a stroke that leaves nothing changed clears the edit; clear drops it
    c.run("mask.clear", json!({"node": pm}));
    assert_eq!(c.run("mask.get", json!({"node": pm}))["value"], "");
    assert_eq!(c.call("ui.inspect", json!({}))["result"]["undo"], "Clear Mask");
    let r = c.run("mask.stroke", json!({"node": pm, "tool": "erase", "points": [[10, 10]]}));
    assert_eq!(r["value"], "", "erasing nothing leaves no edit");
    c.close();
}

#[test]
fn a_chosen_folder_takes_the_files_and_a_wired_mask_is_the_start() {
    let root = std::path::PathBuf::from(store_root("masks-folder"));
    let chosen = root.join("chosen");
    std::fs::create_dir_all(&chosen).unwrap();
    let mut c = Client::open(&json!({"store_root": root.join("store"), "engines": {"default": "array", "mode": "inproc"}}));
    let src = c.run("node.add", json!({"kind": "array.mandelbrot", "pos": [0, 0]}))["id"].as_str().unwrap().to_owned();
    let pm = c.run("node.add", json!({"kind": "array.paint_mask", "pos": [300, 0]}))["id"].as_str().unwrap().to_owned();
    c.run("wire.connect", json!({"from": {"node": src, "port": "escape"}, "to": {"node": pm, "port": "picture"}}));
    c.run("wire.connect", json!({"from": {"node": src, "port": "escape"}, "to": {"node": pm, "port": "mask"}}));
    run(&mut c);
    let d = c.run("node.editorData", json!({"node": pm}));
    assert_eq!(d["data"]["base"]["kind"], "rgba8", "a wired mask is sent as the base: {d}");
    assert!(result(&mut c, "mask.folder", json!({"folder": root.join("nope")})).is_err());
    c.run("mask.folder", json!({"folder": chosen}));
    assert_eq!(c.call("ui.inspect", json!({}))["result"]["undo"], "Mask Folder");
    let v = c.run("mask.stroke", json!({"node": pm, "points": [[5, 5]], "radius": 3}))["value"].as_str().unwrap().to_owned();
    assert!(v.starts_with(chosen.to_str().unwrap()), "an unsaved graph's files go in the chosen folder, by absolute path: {v}");
    // Clean Up lists what nothing reaches: the stroke just made is reachable, so nothing
    let r = c.run("mask.prune", json!({"age_s": 0}));
    assert_eq!((r["dry_run"].as_bool(), r["reachable"].as_u64(), r["removed"].as_array().map(Vec::len)), (Some(true), Some(1), Some(0)), "{r}");
    // a remembered folder that is a symbolic link is refused, not followed
    let link = root.join("link");
    std::os::unix::fs::symlink(&chosen, &link).unwrap();
    c.run("mask.folder", json!({"folder": link}));
    let r = c.run("mask.prune", json!({"dry_run": false, "age_s": 0}));
    assert!(r["refused"].as_array().is_some_and(|a| !a.is_empty()), "{r}");
    assert!(fs_names(&chosen).iter().any(|n| n.ends_with(".png")), "nothing removed through the link");
    // a wired edit port can't be painted
    let e = result(&mut c, "mask.stroke", json!({"node": src, "points": [[5, 5]]})).unwrap_err();
    assert!(e.contains("names no mask editor"), "{e}");
    c.close();
}

#[test]
fn clean_up_removes_only_old_unreachable_files_of_its_own() {
    let root = std::path::PathBuf::from(store_root("masks-prune"));
    let folder = root.join("doc");
    std::fs::create_dir_all(&folder).unwrap();
    let mut c = Client::open(&json!({"store_root": root.join("store"), "engines": {"default": "array", "mode": "inproc"}}));
    let src = c.run("node.add", json!({"kind": "array.mandelbrot", "pos": [0, 0]}))["id"].as_str().unwrap().to_owned();
    let pm = c.run("node.add", json!({"kind": "array.paint_mask", "pos": [300, 0]}))["id"].as_str().unwrap().to_owned();
    c.run("wire.connect", json!({"from": {"node": src, "port": "escape"}, "to": {"node": pm, "port": "picture"}}));
    run(&mut c);
    c.run("graph.saveAs", json!({"path": folder.join("painted.json")}));
    let a = c.run("mask.stroke", json!({"node": pm, "points": [[10, 10]], "radius": 6}))["value"].as_str().unwrap().to_owned();
    let b = c.run("mask.stroke", json!({"node": pm, "points": [[90, 90]], "radius": 6}))["value"].as_str().unwrap().to_owned();
    let edits = folder.join("painted.edits");
    let day = 2 * 24 * 60 * 60;
    // the strokes' own files, aged: a is reached by undo, b by the graph
    for f in [&a, &b] {
        let p = folder.join(f);
        let then = std::time::SystemTime::now() - std::time::Duration::from_secs(day);
        for q in [p.clone(), std::path::PathBuf::from(format!("{}.json", p.display()))] {
            std::fs::File::options().write(true).open(&q).unwrap().set_times(std::fs::FileTimes::new().set_modified(then)).unwrap();
        }
    }
    let gone = edit_file(&edits, &pm, 1, Some("plumcraft"), day);
    let young = edit_file(&edits, &pm, 2, Some("plumcraft"), 0);
    let bare = edit_file(&edits, &pm, 3, None, day);
    let theirs = edit_file(&edits, &pm, 4, Some("another-editor"), day);
    let named = edit_file(&edits, &pm, 5, Some("plumcraft"), day);
    std::fs::write(folder.join("neighbour.json"), json!({"nodes": {"x": {"values": {"edit": format!("painted.edits/{named}")}}}}).to_string()).unwrap();
    let before = fs_names(&edits);
    // a dry run (the default) lists, and removes nothing
    let r = c.run("mask.prune", json!({}));
    assert_eq!(r["removed"], json!([format!("painted.edits/{gone}"), format!("painted.edits/{gone}.json")]), "{r}");
    assert_eq!((r["young"].as_u64(), r["foreign"].as_u64()), (Some(1), Some(2)), "{r}");
    assert_eq!(fs_names(&edits), before);
    // for real: only that file and its sidecar go
    let r = c.run("mask.prune", json!({"dry_run": false}));
    assert_eq!(r["removed"].as_array().map(Vec::len), Some(2), "{r}");
    let left = fs_names(&edits);
    assert!(!left.contains(&gone) && !left.contains(&format!("{gone}.json")));
    for keep in [&young, &bare, &theirs, &named] {
        assert!(left.contains(keep), "{keep} kept: {left:?}");
    }
    for f in [&a, &b] {
        assert!(folder.join(f).is_file(), "{f} kept (undo, the graph)");
    }
    c.close();
}
