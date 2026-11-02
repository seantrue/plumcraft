//! Preferences across runs, as a person meets them: an editor is closed and another opened on the
//! same file. The theme chosen in the View menu comes back, and each kind of dialog starts where
//! that kind last ended. Nothing here touches the real preferences: an `EditorApp` has no file
//! until it is given one.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::{add, click, harness, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_proto::catalogue::PathHint;
use plumcraft_ui::{EditorApp, FileDialogs, prefs, theme};
use serde_json::{Value, json};

/// What the fake dialogs answer, and where each was told to start.
#[derive(Default)]
struct Answers {
    answer: Option<PathBuf>,
    started: Vec<Option<PathBuf>>,
}

struct Fake(Arc<Mutex<Answers>>);

impl Fake {
    fn answer(&self) -> Option<PathBuf> {
        self.0.lock().unwrap().answer.clone()
    }
}

impl FileDialogs for Fake {
    fn pick_path(&mut self, _: &PathHint, _: &str) -> Option<PathBuf> {
        self.answer()
    }
    fn start_in(&mut self, dir: Option<PathBuf>) {
        self.0.lock().unwrap().started.push(dir);
    }
    fn open_workflow(&mut self) -> Option<PathBuf> {
        self.answer()
    }
    fn save_workflow(&mut self, _: Option<&Path>) -> Option<PathBuf> {
        self.answer()
    }
    fn save_bundle(&mut self, _: Option<&Path>) -> Option<PathBuf> {
        self.answer()
    }
}

fn folder(name: &str) -> PathBuf {
    let d = plumcraft_scratch::dir("kittest-prefs").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// An editor on the preferences at `file`, as `run` makes one.
fn editor(file: &Path) -> (Harness<'static, EditorApp>, Arc<Mutex<Answers>>) {
    let answers = Arc::new(Mutex::new(Answers::default()));
    let mut s = session();
    add(&mut s, "text.upper", 40.0, 40.0);
    let mut app = EditorApp::new(s);
    app.dialogs = Box::new(Fake(Arc::clone(&answers)));
    let mut h = harness(app);
    let ctx = h.ctx.clone();
    h.state_mut().use_prefs(prefs::Store::at(file.to_path_buf()), &ctx);
    h.run_steps(3);
    (h, answers)
}

fn window_fill(h: &Harness<'_, EditorApp>) -> egui::Color32 {
    h.ctx.style_of(egui::Theme::Dark).visuals.panel_fill
}

#[test]
fn the_theme_chosen_in_the_menu_comes_back() {
    let dir = folder("theme");
    let file = dir.join("prefs.json");
    let (mut h, _) = editor(&file);
    assert_eq!(h.state().tokens.name, theme::DEFAULT.name, "a first run");
    let other = theme::ALL.iter().find(|t| t.name != theme::DEFAULT.name).unwrap();
    let before = window_fill(&h);
    click(&mut h, "View");
    h.run_steps(2);
    h.hover_at(h.get_by_label_contains("Theme").rect().center());
    h.run_steps(4);
    click(&mut h, other.label);
    h.run_steps(4);
    assert_eq!(h.state().tokens.name, other.name);
    assert_ne!(window_fill(&h), before, "the window changed");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["theme"], other.name, "on disk at once: a crash can't lose it");
    drop(h);

    let (h, _) = editor(&file);
    assert_eq!(h.state().tokens.name, other.name, "the next run opens with it");
}

#[test]
fn an_unknown_theme_in_the_file_is_ignored_and_a_command_for_one_is_said() {
    let dir = folder("unknown");
    let file = dir.join("prefs.json");
    std::fs::write(&file, r#"{"theme": "neon"}"#).unwrap();
    let (mut h, _) = editor(&file);
    assert_eq!(h.state().tokens.name, theme::DEFAULT.name);
    h.state_mut().run("view.theme", &json!({"name": "neon"})).unwrap();
    h.run_steps(3);
    assert!(h.state().last_error.as_deref().unwrap_or_default().contains("no theme"), "{:?}", h.state().last_error);
    assert_eq!(h.state().tokens.name, theme::DEFAULT.name);
}

#[test]
fn each_kind_of_dialog_starts_where_that_kind_last_ended() {
    let dir = folder("dialogs");
    let file = dir.join("prefs.json");
    let (reads, writes) = (dir.join("reads"), dir.join("writes"));
    std::fs::create_dir_all(&reads).unwrap();
    std::fs::create_dir_all(&writes).unwrap();
    std::fs::write(reads.join("w.json"), r#"{"version": 1, "nodes": {}, "wires": []}"#).unwrap();

    let (mut h, answers) = editor(&file);
    answers.lock().unwrap().answer = Some(reads.join("w.json"));
    h.state_mut().invoke("graph.insert");
    h.run_steps(3);
    answers.lock().unwrap().answer = Some(writes.join("kept.json"));
    h.state_mut().invoke("graph.saveAs");
    h.run_steps(3);
    assert_eq!(answers.lock().unwrap().started, [None, None], "nothing remembered yet");
    drop(h);

    let (mut h, answers) = editor(&file);
    answers.lock().unwrap().answer = None;
    h.state_mut().invoke("graph.insert");
    h.state_mut().invoke("graph.saveAs");
    assert_eq!(answers.lock().unwrap().started, [Some(reads), Some(writes)], "reads and writes apart");
}

#[test]
fn a_cancelled_dialog_remembers_nothing() {
    let dir = folder("cancelled");
    let file = dir.join("prefs.json");
    let (mut h, _) = editor(&file);
    h.state_mut().invoke("graph.insert");
    assert!(h.state().prefs.prefs().dirs.is_empty());
    assert!(!file.exists(), "and writes nothing");
}
