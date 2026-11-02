//! Through the window: File › Open on a file an application opens, what a graph lacks in the status
//! bar, the About window, an engine's notice in the banner, and a mark for export from a node's
//! menu.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once};

use common::{add, catalog, harness};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_proto::catalogue::{PathHint, texts};
use plumcraft_session::file_cmds::{FileOpener, register_openers};
use plumcraft_session::{About, Session};
use plumcraft_ui::{EditorApp, FileDialogs};
use serde_json::json;

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| e.to_string())?.strip_prefix("PIC\n").map(str::to_owned).ok_or_else(|| "it has no recipe".to_owned())
}

fn setup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| register_openers(vec![FileOpener { extensions: &["pic"], label: "picture", read }]).unwrap());
}

struct Dialogs {
    open: Arc<Mutex<Option<PathBuf>>>,
    save_asked: Arc<Mutex<usize>>,
}

impl FileDialogs for Dialogs {
    fn pick_path(&mut self, _: &PathHint, _: &str) -> Option<PathBuf> {
        None
    }
    fn open_workflow(&mut self) -> Option<PathBuf> {
        self.open.lock().unwrap().clone()
    }
    fn save_workflow(&mut self, _: Option<&Path>) -> Option<PathBuf> {
        *self.save_asked.lock().unwrap() += 1;
        None
    }
}

fn picture(name: &str, text: &str) -> PathBuf {
    let path = plumcraft_scratch::dir("kittest-stage7").join(name);
    std::fs::write(&path, text).unwrap();
    path
}

/// File › Open, as a person chooses it.
fn file_open(h: &mut Harness<'static, EditorApp>) {
    h.get_by_label("File").click();
    h.run_steps(2);
    h.get_by_label_contains("Open…").click();
    h.run_steps(4);
}

#[test]
fn file_open_on_a_picture_opens_its_graph_unsaved_and_says_what_it_lacks() {
    setup();
    let open = Arc::new(Mutex::new(None));
    let save_asked = Arc::new(Mutex::new(0));
    let mut app = EditorApp::new(Session::new(catalog()));
    app.dialogs = Box::new(Dialogs { open: Arc::clone(&open), save_asked: Arc::clone(&save_asked) });
    let mut h = harness(app);

    // a recipe with the nodes this engine has
    let good =
        picture("mine.pic", &format!("PIC\n{}", json!({"version": 1, "nodes": {"a": {"kind": "text.literal"}, "b": {"kind": "text.upper"}}, "wires": []})));
    *open.lock().unwrap() = Some(good);
    file_open(&mut h);
    h.get_by_label("2 nodes, 0 wires");
    h.get_by_label("mine.pic");
    assert!(h.state().window_title().starts_with("mine.pic"), "{}", h.state().window_title());
    assert_eq!(h.state().session.path(), None);
    assert_eq!(h.state().last_error, None);

    // Save doesn't write over the picture: it asks where
    h.state_mut().invoke("graph.save");
    h.run_steps(3);
    assert_eq!(*save_asked.lock().unwrap(), 1);

    // no recipe: a refusal, in the status bar, and the graph stays
    *open.lock().unwrap() = Some(picture("plain.pic", "just pixels"));
    file_open(&mut h);
    h.get_by_label("2 nodes, 0 wires");
    h.get_by_label_contains("it has no recipe");

    // a recipe needing an engine that isn't here names it
    let across = json!({"version": 1, "engines": ["text", "vectorcraft"], "nodes": {"a": {"kind": "text.literal"}, "b": {"kind": "vc.simplify"}}, "wires": []});
    *open.lock().unwrap() = Some(picture("across.pic", &format!("PIC\n{across}")));
    file_open(&mut h);
    h.get_by_label("2 nodes, 0 wires");
    let said = h.state().banner_text().unwrap();
    assert!(said.contains("the engine vectorcraft") && said.contains("vc.simplify"), "{said}");
    h.get_by_label(&said);
    let ctx = h.ctx.clone();
    assert_eq!(h.state_mut().control_now(&ctx, "ui.inspect", &json!({}))["result"]["banner"], json!(said));
    // and goes with the graph that needed it
    h.state_mut().run("graph.new", &json!({"discard": true}));
    h.run_steps(3);
    assert_eq!(h.state().banner_text(), None);
}

#[test]
fn about_opens_from_the_help_menu_and_says_what_this_is() {
    let mut app = EditorApp::new(Session::new(catalog()));
    app.session.about = About { title: "PlumThing".into(), version: "1.2.3".into(), parts: vec![("renderer".into(), "abc123".into())] };
    let mut h = harness(app);
    assert!(h.query_by_label("About PlumThing").is_none());
    h.get_by_label("Help").click();
    h.run_steps(2);
    h.get_by_label("About").click();
    h.run_steps(4);
    h.get_by_label("About PlumThing");
    h.get_by_label("Version 1.2.3");
    h.get_by_label("abc123");
    h.get_by_label_contains("Engine: ");
    assert!(h.state().session.views.about);
    let ctx = h.ctx.clone();
    assert_eq!(h.state_mut().control_now(&ctx, "ui.inspect", &json!({}))["result"]["about"]["title"], "PlumThing");
    h.get_by_label("Close").click();
    h.run_steps(4);
    assert!(h.query_by_label("About PlumThing").is_none());
    assert!(!h.state().session.views.about);
}

#[test]
fn an_engines_notice_is_the_banner() {
    let mut c = catalog();
    let app = EditorApp::new(Session::new(c.clone()));
    let h = harness(app);
    assert_eq!(h.state().banner_text(), None);
    c.texts.insert(texts::ENGINE_NOTICE.into(), "No GPU device (none here).".into());
    let mut h = harness(EditorApp::new(Session::new(c)));
    h.run_steps(2);
    h.get_by_label("No GPU device (none here).");
}

/// Right-click node `id`'s header.
fn open_menu(h: &mut Harness<'static, EditorApp>, id: &str) {
    let r = h.state().canvas.node_rects_by_id().into_iter().find(|(n, _)| n.0 == id).unwrap().1;
    let at = h.state().canvas.to_global * (r.min + egui::vec2(40.0, 12.0));
    h.hover_at(at);
    h.run_steps(2);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(4);
}

#[test]
fn a_node_menu_marks_a_markable_member_and_the_node_says_so() {
    let mut c = catalog();
    c.nodes.iter_mut().find(|n| n.id == "text.upper").unwrap().export.markable = true;
    c.texts.insert(texts::EXPORT_MARK.into(), "checkpoint".into());
    c.texts.insert(texts::EXPORT_MARK_HOVER.into(), "checkpoint in {group}".into());
    let mut s = Session::new(c);
    let up = add(&mut s, "text.upper", 100.0, 100.0);
    let plain = add(&mut s, "text.lower", 500.0, 100.0);
    s.execute("group.create", &json!({"nodes": [up, plain], "name": "G"})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.state_mut().canvas.request_fit();
    h.run_steps(4);
    assert!(h.query_by_label("checkpoint").is_none());

    // the markable node's menu offers it, in the catalogue's word
    open_menu(&mut h, &up);
    h.get_by_label("Checkpoint in G").click();
    h.run_steps(4);
    let group = |h: &Harness<'static, EditorApp>| h.state().session.graph().groups.values().next().unwrap().clone();
    assert!(group(&h).checkpoints.contains(&up.as_str().into()));
    h.get_by_label("checkpoint");
    assert_eq!(h.state().session.undo_label(), Some("Mark for Export"));

    // and takes it off
    open_menu(&mut h, &up);
    h.get_by_label("[x] Checkpoint in G").click();
    h.run_steps(4);
    assert!(group(&h).checkpoints.is_empty());
    assert!(h.query_by_label("checkpoint").is_none());

    // a node type the catalogue doesn't call markable offers nothing
    open_menu(&mut h, &plain);
    assert!(h.query_by_label_contains("Checkpoint in").is_none());
}
