//! File › Export Bundle and opening one, driven through the menus with fake dialogs: the zip
//! carries the file a node reads, opens in a new folder beside it, and what can't go in is said.
//! Hostile bundles are `crates/session/tests/hostile_bundles.rs`.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::{add, click, harness};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_proto::catalogue::{PathHint, SchemaReply};
use plumcraft_session::Session;
use plumcraft_session::paths::with_absolute_paths;
use plumcraft_ui::{EditorApp, FileDialogs};
use serde_json::json;

fn catalog() -> SchemaReply {
    serde_json::from_value(json!({"nodes": [
        {"id": "t.read", "display": "Read", "inputs": [{"name": "path", "type": "STRING", "default": "", "path": {"mode": "open"}}], "outputs": []}]}))
    .unwrap()
}

#[derive(Default)]
struct Answers {
    bundle: Option<PathBuf>,
    open: Option<PathBuf>,
    asked_for: Vec<Option<PathBuf>>,
}

struct Fake(Arc<Mutex<Answers>>);

impl FileDialogs for Fake {
    fn pick_path(&mut self, _: &PathHint, _: &str) -> Option<PathBuf> {
        None
    }
    fn open_workflow(&mut self) -> Option<PathBuf> {
        self.0.lock().unwrap().open.clone()
    }
    fn save_bundle(&mut self, current: Option<&Path>) -> Option<PathBuf> {
        let mut a = self.0.lock().unwrap();
        a.asked_for.push(current.map(Path::to_path_buf));
        a.bundle.clone()
    }
}

fn folder(name: &str) -> PathBuf {
    let d = plumcraft_scratch::dir("kittest-export-bundle").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn file_menu(h: &mut Harness<'static, EditorApp>, item: &str) {
    click(h, "File");
    h.run_steps(2);
    let at = h.get_by_label_contains(item).rect().center();
    common::click_at(h, at);
    h.run_steps(4);
}

/// A saved workflow reading `paper.txt` beside it and `gone.txt`, which isn't there.
fn editor(dir: &Path, answers: Answers) -> (Harness<'static, EditorApp>, Arc<Mutex<Answers>>) {
    std::fs::write(dir.join("paper.txt"), "the words").unwrap();
    let mut s = Session::new(catalog());
    let read = add(&mut s, "t.read", 40.0, 40.0);
    let gone = add(&mut s, "t.read", 40.0, 240.0);
    s.execute("graph.saveAs", &json!({"path": dir.join("flow.json")})).unwrap();
    s.execute("node.set", &json!({"node": read, "port": "path", "value": "paper.txt"})).unwrap();
    s.execute("node.set", &json!({"node": gone, "port": "path", "value": "gone.txt"})).unwrap();
    s.execute("graph.save", &json!({})).unwrap();
    let answers = Arc::new(Mutex::new(answers));
    let mut app = EditorApp::new(s);
    app.dialogs = Box::new(Fake(Arc::clone(&answers)));
    (harness(app), answers)
}

#[test]
fn a_bundle_goes_out_through_the_file_menu_and_comes_back_where_its_paths_work() {
    let dir = folder("round");
    let zip = dir.join("out/flow.zip");
    std::fs::create_dir_all(zip.parent().unwrap()).unwrap();
    let (mut h, asked) = editor(&dir, Answers { bundle: Some(zip.clone()), open: Some(zip.clone()), ..Answers::default() });
    file_menu(&mut h, "Export Bundle…");
    assert!(zip.is_file(), "{:?}", h.state().last_error);
    assert_eq!(asked.lock().unwrap().asked_for, [Some(dir.join("flow.json"))], "offered beside the workflow");
    let said = h.state().last_error.clone().expect("what couldn't go in is said");
    assert!(said.contains("1 path(s)") && said.contains("gone.txt"), "{said}");
    assert!(h.query_by_label_contains("couldn't go in").is_some(), "on screen");

    std::fs::remove_file(dir.join("paper.txt")).unwrap();
    file_menu(&mut h, "Open…");
    let s = &h.state().session;
    assert_eq!(s.path(), Some(dir.join("out/flow/flow.json").as_path()), "unpacked into a new folder beside it");
    let abs = with_absolute_paths(s.graph(), s.catalog(), s.path().and_then(Path::parent));
    let read: Vec<_> = abs.nodes.values().filter_map(|n| std::fs::read_to_string(n.values["path"].as_str().unwrap()).ok()).collect();
    assert_eq!(read, ["the words"], "the path now leads to the bundle's own copy");
}

#[test]
fn an_empty_graph_has_nothing_to_bundle_and_a_hostile_one_opens_nothing() {
    let dir = folder("hostile");
    let (mut h, answers) = editor(&dir, Answers::default());
    let evil = dir.join("evil.zip");
    {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(std::fs::File::create(&evil).unwrap());
        z.start_file("w.json", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(b"{}").unwrap();
        z.start_file("../evil.txt", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(b"x").unwrap();
        z.finish().unwrap();
    }
    answers.lock().unwrap().open = Some(evil);
    let before = h.state().session.graph().clone();
    file_menu(&mut h, "Open…");
    assert_eq!(h.state().session.graph(), &before);
    assert!(h.state().last_error.as_deref().unwrap_or_default().contains("leaves its folder"), "{:?}", h.state().last_error);
    assert!(!dir.parent().unwrap().join("evil.txt").exists());

    h.state_mut().run("graph.new", &json!({"discard": true})).unwrap();
    h.run_steps(3);
    let reason = h.state_mut().try_run("graph.bundle", &json!({"path": dir.join("x.zip")})).unwrap_err();
    assert!(reason.contains("empty"), "{reason}");
}
