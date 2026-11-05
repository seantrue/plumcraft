//! Preferences across runs, as a person meets them: an editor is closed and another opened on the
//! same file. The theme chosen in Settings comes back, and each kind of dialog starts where
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
    h.ctx.global_style().visuals.panel_fill
}

/// File › Settings…, as a person opens it.
fn open_settings(h: &mut Harness<'static, EditorApp>) {
    click(h, "File");
    h.run_steps(2);
    // (the item's label carries its shortcut, ⌘,)
    let at = h.get_by_label_contains("Settings…").rect().center();
    common::click_at(h, at);
    h.run_steps(4);
}

#[test]
fn the_theme_chosen_in_settings_comes_back() {
    let dir = folder("theme");
    let file = dir.join("prefs.json");
    let (mut h, _) = editor(&file);
    assert_eq!(h.state().tokens.name, theme::DEFAULT.name, "a first run");
    assert_eq!(theme::DEFAULT.name, "paper");
    assert!(!h.ctx.global_style().visuals.dark_mode, "Paper is a light theme, and egui's visuals say so");
    let other = theme::DUSK;
    let before = window_fill(&h);
    open_settings(&mut h);
    assert_eq!(h.state().session.views.settings.as_deref(), Some("appearance"));
    click(&mut h, other.label);
    h.run_steps(4);
    assert_eq!(h.state().tokens.name, other.name);
    assert_ne!(window_fill(&h), before, "the window changed");
    assert!(h.ctx.global_style().visuals.dark_mode);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["theme"], other.name, "on disk at once: a crash can't lose it");
    drop(h);

    let (h, _) = editor(&file);
    assert_eq!(h.state().tokens.name, other.name, "the next run opens with it");
}

/// The themes before Paper, Pewter and Dusk: a file that says `medium` opens in Pewter, one
/// that says `dark` in Dusk.
#[test]
fn an_old_theme_name_opens_as_its_replacement() {
    for (old, new) in [("medium", "pewter"), ("dark", "dusk")] {
        let dir = folder(&format!("old-{old}"));
        let file = dir.join("prefs.json");
        std::fs::write(&file, format!(r#"{{"theme": "{old}"}}"#)).unwrap();
        let (h, _) = editor(&file);
        assert_eq!(h.state().tokens.name, new);
        assert_eq!(window_fill(&h), theme::by_name(new).unwrap().panel);
    }
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

/// The View menu has no Theme, nor Cache Report, and the Engine menu no checkbox: they are in
/// Settings, which ⌘, opens.
#[test]
fn the_settings_moved_out_of_the_menus() {
    let dir = folder("menus");
    let (mut h, _) = editor(&dir.join("prefs.json"));
    click(&mut h, "View");
    h.run_steps(2);
    assert!(h.query_by_label("Theme").is_none());
    assert!(h.query_by_label("Cache Report").is_none());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    click(&mut h, "Engine");
    h.run_steps(2);
    assert!(h.query_by_label("Pick Engines Automatically").is_none());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    h.run_steps(4);
    assert_eq!(h.state().session.views.settings.as_deref(), Some("appearance"), "⌘, opens Settings");
    click(&mut h, "Engines");
    h.run_steps(3);
    assert!(!h.state().session.engines.auto);
    click(&mut h, "Pick Engines Automatically");
    h.run_steps(3);
    assert!(h.state().session.engines.auto, "the checkbox runs engine.auto");
    click(&mut h, "Cache");
    h.run_steps(3);
    click(&mut h, "Open Cache Report");
    h.run_steps(3);
    assert!(h.state().session.views.cache_report);
}

/// Every setting is a command: the control channel opens a tab, reads the settings, and changes
/// the text size, a colour, a fruit and a family's pin, all kept in the file.
#[test]
fn settings_are_commands_and_are_kept() {
    let dir = folder("commands");
    let file = dir.join("prefs.json");
    let (mut h, _) = editor(&file);
    let ctx = h.ctx.clone();
    let run = |h: &mut Harness<'static, EditorApp>, id: &str, p: Value| {
        let r = h.state_mut().control_now(&ctx, "command.run", &json!({"command": id, "params": p}));
        h.run_steps(3);
        r
    };
    let r = run(&mut h, "app.settings", json!({"tab": "cache"}));
    assert_eq!(r["result"]["tab"], "cache", "{r}");
    assert_eq!(r["result"]["theme"], "paper");
    assert_eq!(r["result"]["themes"].as_array().map(Vec::len), Some(3));
    assert_eq!(r["result"]["appearance"]["textSize"], 14.0);
    assert_eq!(r["result"]["contrast"]["below"], json!([]), "Paper passes every pair");
    let r = run(&mut h, "view.textSize", json!({"size": 16}));
    assert_eq!(r["result"]["textSize"], 16.0, "{r}");
    assert_eq!(h.ctx.global_style().text_styles[&egui::TextStyle::Body].size, 16.0);
    let r = run(&mut h, "theme.setToken", json!({"token": "accent", "color": "#ff0000"}));
    assert_eq!(r["result"]["color"], "#ff0000", "{r}");
    assert!(r["result"]["checks"].as_array().is_some_and(|c| !c.is_empty()));
    assert_eq!(h.state().tokens.accent, egui::Color32::from_rgb(255, 0, 0));
    let r = run(&mut h, "theme.setToken", json!({"token": "nope", "color": "#ff0000"}));
    assert!(r["error"].as_str().unwrap_or_default().contains("no token"), "{r}");
    let r = run(&mut h, "theme.setFruit", json!({"fruit": "pear", "color": "#00ff00"}));
    assert_eq!(r["result"]["color"], "#00ff00", "{r}");
    let r = run(&mut h, "theme.pinFamily", json!({"family": "text", "to": "olive"}));
    assert_eq!(r["result"]["fruit"], theme::hex(theme::PAPER.olive), "{r}");
    let r = run(&mut h, "theme.pinFamily", json!({"family": "text", "to": "banana"}));
    assert!(r["error"].is_string(), "{r}");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["appearance"]["text_size"], 16.0);
    assert_eq!(v["appearance"]["overrides"]["paper"]["accent"], "#ff0000");
    assert_eq!(v["appearance"]["fruits"]["paper"]["pear"], "#00ff00");
    assert_eq!(v["appearance"]["families"]["text"], "olive");

    // a theme file out and back in
    let out = dir.join("mine.plumtheme.json");
    let r = run(&mut h, "theme.export", json!({"path": out}));
    assert_eq!(r["result"]["theme"], "paper", "{r}");
    let r = run(&mut h, "theme.reset", json!({}));
    assert_eq!(r["result"]["theme"], "paper", "{r}");
    assert_eq!(h.state().tokens.accent, theme::PAPER.accent);
    let r = run(&mut h, "theme.import", json!({"path": out}));
    assert_eq!(r["result"]["theme"], "paper", "{r}");
    assert_eq!(h.state().tokens.accent, egui::Color32::from_rgb(255, 0, 0));
    std::fs::write(dir.join("junk.plumtheme.json"), "{\"format\": 3}").unwrap();
    let r = run(&mut h, "theme.import", json!({"path": dir.join("junk.plumtheme.json")}));
    assert!(r["error"].as_str().unwrap_or_default().contains("not a theme file"), "{r}");
    drop(h);

    let (h, _) = editor(&file);
    assert_eq!(h.state().tokens.text_size, 16.0, "the next run keeps the look");
    assert_eq!(h.state().tokens.accent, egui::Color32::from_rgb(255, 0, 0));
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
