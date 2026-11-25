//! B166 (the maintainer: "open recent never has a list of recent graphs"): File › Open Recent is
//! the core's. A graph opened or saved under a file of its own is the newest in the list, kept in
//! the application's preferences and shared by a core's documents; the menu model builds File ›
//! Open Recent from it for every shell (newest first, a file no longer there disabled with why,
//! Clear Menu); its items are commands (`graph.openRecent {path}`, `graph.clearRecent {}`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use plumcraft_editor_core::control::Shell;
use plumcraft_editor_core::document::{Document, execute};
use plumcraft_editor_core::menu::{self, Item};
use plumcraft_editor_core::prefs;
use plumcraft_editor_core::settings::{Look, SharedLook, System};
use plumcraft_session::Session;
use serde_json::{Value, json};

fn folder(name: &str) -> PathBuf {
    let d = plumcraft_scratch::dir("open-recent").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::canonicalize(&d).unwrap()
}

/// A document of a core whose look (and so its preferences) is `look`, as the C ABI's are.
fn doc(look: &SharedLook, windows: bool) -> Document {
    let mut d = Document::new(Session::new(plumcraft_text::engine().schema()));
    d.look = Some(Arc::clone(look));
    d.document_windows = windows;
    d
}

fn look_at(file: &Path) -> SharedLook {
    Arc::new(Mutex::new(Look::new(prefs::Store::at(file.to_path_buf()), System::default())))
}

/// File › Open Recent as the menu model builds it for `d`.
fn open_recent(d: &Document) -> Item {
    let bar = menu::bar(&d.session, &Shell::menu_facts(d));
    let file = bar.iter().find(|m| m.label == "File").expect("a File menu");
    file.items.iter().find(|i| i.id == menu::OPEN_RECENT).cloned().expect("File › Open Recent")
}

fn labels(m: &Item) -> Vec<String> {
    m.items.iter().map(|i| if i.separator { "—".to_owned() } else { i.label.clone() }).collect()
}

#[test]
fn graphs_opened_and_saved_are_in_open_recent_newest_first_and_kept() {
    let dir = folder("kept");
    let file = dir.join("prefs.json");
    let look = look_at(&file);
    let mut d = doc(&look, true);
    assert_eq!(labels(&open_recent(&d)), ["Clear Menu"], "none yet");
    assert!(!open_recent(&d).items[0].enabled, "Clear Menu disabled with nothing to clear");

    // saved under three names, one of them a .json graph, then the first opened again
    for name in ["a.plum", "b.json", "c.plum"] {
        execute(&mut d, "graph.saveAs", &json!({"path": dir.join(name)})).unwrap();
    }
    execute(&mut d, "graph.open", &json!({"path": dir.join("a.plum")})).unwrap();
    let m = open_recent(&d);
    assert_eq!(labels(&m), ["a.plum", "c.plum", "b.json", "—", "Clear Menu"], "newest first, once each");
    assert_eq!(m.items[0].command.as_deref(), Some("graph.openRecent"));
    assert_eq!(m.items[0].params, Some(json!({"path": dir.join("a.plum")})));
    assert_eq!(m.items[0].hover.as_deref(), Some(dir.join("a.plum").to_str().unwrap()), "the whole path on hover");

    // graph.recent says the same
    let r = execute(&mut d, "graph.recent", &json!({})).unwrap();
    let paths: Vec<&str> = r["recent"].as_array().unwrap().iter().map(|e| e["path"].as_str().unwrap()).collect();
    assert_eq!(paths, [dir.join("a.plum"), dir.join("c.plum"), dir.join("b.json")].iter().map(|p| p.to_str().unwrap()).collect::<Vec<_>>());

    // a rename takes the old name's place
    execute(&mut d, "graph.rename", &json!({"name": "renamed"})).unwrap();
    assert_eq!(labels(&open_recent(&d)), ["renamed.plum", "c.plum", "b.json", "—", "Clear Menu"]);

    // every document of the core shares the list, and it is in the application's preferences
    let other = doc(&look, true);
    assert_eq!(labels(&open_recent(&other)), ["renamed.plum", "c.plum", "b.json", "—", "Clear Menu"], "shared");
    let relaunched = doc(&look_at(&file), true);
    assert_eq!(labels(&open_recent(&relaunched)), ["renamed.plum", "c.plum", "b.json", "—", "Clear Menu"], "kept across a relaunch");

    // a file no longer there is disabled, with why
    std::fs::remove_file(dir.join("c.plum")).unwrap();
    let m = open_recent(&d);
    assert!(!m.items[1].enabled);
    assert!(m.items[1].reason.as_deref().is_some_and(|r| r.contains("no longer there")), "{:?}", m.items[1].reason);
    assert!(execute(&mut d, "graph.openRecent", &json!({"path": dir.join("c.plum")})).unwrap_err().contains("no longer there"));

    // Clear Menu
    execute(&mut d, "graph.clearRecent", &json!({})).unwrap();
    assert_eq!(labels(&open_recent(&d)), ["Clear Menu"]);
    assert_eq!(labels(&open_recent(&doc(&look_at(&file), true))), ["Clear Menu"], "cleared for good");
}

#[test]
fn open_recent_opens_as_open_does_and_takes_only_its_own_paths() {
    let dir = folder("open");
    let look = look_at(&dir.join("prefs.json"));
    let mut d = doc(&look, true);
    for name in ["one.plum", "two.json"] {
        execute(&mut d, "graph.saveAs", &json!({"path": dir.join(name)})).unwrap();
    }
    // a window per document: a graph other than this one's opens in a window of its own
    let r = execute(&mut d, "graph.openRecent", &json!({"path": dir.join("one.plum")})).unwrap();
    assert_eq!(r["window"], "new");
    assert_eq!(d.opens.last().map(|(p, _)| p.clone()), Some(Some(dir.join("one.plum"))));
    assert_eq!(d.session.path(), Some(dir.join("two.json").as_path()), "this window keeps its graph");
    // this window's own graph: nothing to open
    let r = execute(&mut d, "graph.openRecent", &json!({"path": dir.join("two.json")})).unwrap();
    assert_eq!(r["already"], true);
    // only a path the list has: a script can't open anything through it
    let elsewhere = dir.join("elsewhere.plum");
    std::fs::write(&elsewhere, r#"{"version": 1, "nodes": {}, "wires": []}"#).unwrap();
    assert!(execute(&mut d, "graph.openRecent", &json!({"path": elsewhere})).unwrap_err().contains("isn't in Open Recent"));
    for bad in [json!({}), json!({"path": 7}), json!({"path": ""}), json!({"path": "x", "more": 1})] {
        assert!(execute(&mut d, "graph.openRecent", &bad).is_err(), "{bad}");
    }

    // one window (the egui editor): opened here, after the unsaved-changes prompt when there are any
    let mut single = doc(&look, false);
    execute(&mut single, "node.add", &json!({"kind": "text.literal", "pos": [0, 0]})).unwrap();
    let r = execute(&mut single, "graph.openRecent", &json!({"path": dir.join("one.plum")})).unwrap();
    assert_eq!(r["asked"], "unsavedChanges");
    let asks: Vec<Value> = serde_json::from_value(serde_json::to_value(single.asks.inspect()).unwrap()).unwrap();
    assert!(asks.iter().any(|a| a["params"]["kind"] == "unsavedChanges"), "{asks:?}");
    assert!(single.session.path().is_none(), "nothing opened before the answer");
    let mut clean = doc(&look, false);
    let r = execute(&mut clean, "graph.openRecent", &json!({"path": dir.join("one.plum")})).unwrap();
    assert_eq!(r["window"], "this");
    assert_eq!(clean.session.path(), Some(dir.join("one.plum").as_path()));
}
