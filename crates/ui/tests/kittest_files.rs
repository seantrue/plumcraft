//! E2 in the window: undo and redo from the keyboard and the Edit menu, save and open through
//! fake file dialogs, the unsaved-changes prompt before New and Open, the window title, and
//! copy and paste through egui's clipboard events.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::{add, click, harness, session};
use egui_kittest::kittest::Queryable;
use plumcraft_proto::catalogue::PathHint;
use plumcraft_ui::{AfterDiscard, EditorApp, FileDialogs, Prompt};
use serde_json::json;

/// What the fake dialogs answer, and what they were asked.
#[derive(Default)]
struct Answers {
    open: Option<PathBuf>,
    save: Option<PathBuf>,
    asked_save: Vec<Option<PathBuf>>,
    asked_open: usize,
}

struct Fake(Arc<Mutex<Answers>>);

impl FileDialogs for Fake {
    fn pick_path(&mut self, _: &PathHint, _: &str) -> Option<PathBuf> {
        None
    }
    fn open_workflow(&mut self) -> Option<PathBuf> {
        let mut a = self.0.lock().unwrap();
        a.asked_open += 1;
        a.open.clone()
    }
    fn save_workflow(&mut self, current: Option<&Path>) -> Option<PathBuf> {
        let mut a = self.0.lock().unwrap();
        a.asked_save.push(current.map(Path::to_path_buf));
        a.save.clone()
    }
}

fn with_fake(app: EditorApp) -> (EditorApp, Arc<Mutex<Answers>>) {
    let answers = Arc::new(Mutex::new(Answers::default()));
    let mut app = app;
    app.dialogs = Box::new(Fake(Arc::clone(&answers)));
    (app, answers)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = plumcraft_scratch::root().join(format!("e2-ui-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn press(h: &mut egui_kittest::Harness<'static, EditorApp>, mods: egui::Modifiers, key: egui::Key) {
    h.key_press_modifiers(mods, key);
    h.run_steps(3);
}

fn control(h: &mut egui_kittest::Harness<'static, EditorApp>, method: &str, params: &serde_json::Value) -> serde_json::Value {
    let ctx = h.ctx.clone();
    h.state_mut().control_now(&ctx, method, params)
}

const CMD: egui::Modifiers = egui::Modifiers::COMMAND;
const CMD_SHIFT: egui::Modifiers = egui::Modifiers { shift: true, ..egui::Modifiers::COMMAND };

#[test]
fn cmd_z_undoes_and_cmd_shift_z_redoes() {
    let mut s = session();
    add(&mut s, "text.literal", 20.0, 20.0);
    let mut h = harness(EditorApp::new(s));
    h.state_mut().run("node.add", &json!({"kind": "text.upper", "pos": [400, 20]}));
    h.run_steps(3);
    h.get_by_label("2 nodes, 0 wires");
    // with nothing to redo, Cmd+Shift+Z does nothing (it isn't Undo)
    press(&mut h, CMD_SHIFT, egui::Key::Z);
    h.get_by_label("2 nodes, 0 wires");
    press(&mut h, CMD, egui::Key::Z);
    h.get_by_label("1 nodes, 0 wires");
    assert_eq!(h.state().session.redo_label(), Some("Add Node"));
    press(&mut h, CMD_SHIFT, egui::Key::Z);
    h.get_by_label("2 nodes, 0 wires");
    press(&mut h, CMD, egui::Key::Z);
    press(&mut h, CMD, egui::Key::Z);
    h.get_by_label("0 nodes, 0 wires");
    assert!(!h.state().enabled("edit.undo"));
    // the Edit menu runs them too
    h.get_by_label("Edit").click();
    h.run_steps(2);
    h.get_by_label_contains("Redo").click();
    h.run_steps(3);
    h.get_by_label("1 nodes, 0 wires");
}

#[test]
fn save_and_open_through_the_dialogs_and_the_title_follows() {
    let dir = TempDir::new("save");
    let path = dir.0.join("corgi.json");
    let mut s = session();
    let a = add(&mut s, "text.literal", 20.0, 20.0);
    let (app, answers) = with_fake(EditorApp::new(s));
    let mut h = harness(app);
    assert_eq!(h.state().window_title(), "Untitled • — PlumCraft");
    // Cmd+S on an unsaved graph asks where; a cancelled dialog saves nothing
    press(&mut h, CMD, egui::Key::S);
    assert_eq!(answers.lock().unwrap().asked_save, vec![None]);
    assert!(h.state().session.path().is_none());
    answers.lock().unwrap().save = Some(path.clone());
    press(&mut h, CMD, egui::Key::S);
    assert_eq!(h.state().session.path(), Some(path.as_path()));
    assert!(path.is_file());
    assert_eq!(h.state().window_title(), "corgi.json — PlumCraft");
    // an edit marks it; Cmd+S saves to the file without asking
    h.state_mut().run("node.set", &json!({"node": a, "port": "text", "value": "a corgi"}));
    h.run_steps(2);
    assert_eq!(h.state().window_title(), "corgi.json • — PlumCraft");
    press(&mut h, CMD, egui::Key::S);
    assert_eq!(answers.lock().unwrap().asked_save.len(), 2, "not asked again");
    assert_eq!(h.state().window_title(), "corgi.json — PlumCraft");
    assert!(std::fs::read_to_string(&path).unwrap().contains("a corgi"));
    // Cmd+Shift+S asks, starting from the current file
    let other = dir.0.join("other.json");
    answers.lock().unwrap().save = Some(other.clone());
    press(&mut h, CMD_SHIFT, egui::Key::S);
    assert_eq!(answers.lock().unwrap().asked_save.last().cloned(), Some(Some(path.clone())));
    assert_eq!(h.state().window_title(), "other.json — PlumCraft");

    // Cmd+O in another window opens it, with its name
    let (app, answers) = with_fake(EditorApp::new(session()));
    let mut h = harness(app);
    answers.lock().unwrap().open = Some(path.clone());
    press(&mut h, CMD, egui::Key::O);
    assert_eq!(h.state().session.path(), Some(path.as_path()));
    assert_eq!(h.state().window_title(), "corgi.json — PlumCraft");
    h.get_by_label("1 nodes, 0 wires");
    assert_eq!(common::value(&h, &a, "text"), json!("a corgi"), "the value came back");
    assert_eq!(h.state().inspect(&h.ctx)["path"], json!(path));
}

#[test]
fn new_and_open_ask_before_dropping_unsaved_changes() {
    let dir = TempDir::new("prompt");
    let saved = dir.0.join("saved.json");
    let mut s = session();
    add(&mut s, "text.literal", 20.0, 20.0);
    s.execute("graph.saveAs", &json!({"path": saved})).unwrap();
    add(&mut s, "text.upper", 400.0, 20.0);
    let (app, answers) = with_fake(EditorApp::new(s));
    let mut h = harness(app);
    // File › New Graph: the prompt; Cancel keeps everything
    h.get_by_label("File").click();
    h.run_steps(2);
    h.get_by_label_contains("New Graph").click();
    h.run_steps(3);
    assert_eq!(h.state().prompt, Some(Prompt::Discard { then: AfterDiscard::New }));
    h.get_by_label("Unsaved changes");
    // the keyboard waits while the prompt is up
    press(&mut h, CMD, egui::Key::Z);
    h.get_by_label("2 nodes, 0 wires");
    click(&mut h, "Cancel");
    assert!(h.state().prompt.is_none());
    h.get_by_label("2 nodes, 0 wires");
    // again, and Discard: a new graph
    press(&mut h, CMD, egui::Key::N);
    assert!(h.state().prompt.is_some());
    click(&mut h, "Discard");
    h.get_by_label("0 nodes, 0 wires");
    assert!(h.state().session.path().is_none());
    assert_eq!(h.state().window_title(), "Untitled — PlumCraft");
    // clean: New and Open don't ask
    press(&mut h, CMD, egui::Key::N);
    assert!(h.state().prompt.is_none());
    // Cmd+O over changes: the prompt first, then the dialog
    h.state_mut().run("node.add", &json!({"kind": "text.lower"}));
    answers.lock().unwrap().open = Some(saved.clone());
    press(&mut h, CMD, egui::Key::O);
    assert_eq!(h.state().prompt, Some(Prompt::Discard { then: AfterDiscard::Open }));
    assert_eq!(answers.lock().unwrap().asked_open, 0, "asked before the dialog");
    // the control channel can answer it
    let r = control(&mut h, "ui.prompt", &json!({"answer": "discard"}));
    assert_eq!(r["ok"], true, "{r}");
    h.run_steps(3);
    assert_eq!(h.state().session.path(), Some(saved.as_path()));
    h.get_by_label("1 nodes, 0 wires");
    let r = control(&mut h, "ui.prompt", &json!({"answer": "discard"}));
    assert_eq!(r["ok"], false, "no prompt open");
    // and drive it: ui.invoke graph.new over changes opens the prompt
    h.state_mut().run("node.add", &json!({"kind": "text.lower"}));
    let r = control(&mut h, "ui.invoke", &json!({"command": "graph.new"}));
    assert_eq!(r["result"]["prompt"]["kind"], "unsavedChanges", "{r}");
    let r = control(&mut h, "ui.prompt", &json!({"answer": "cancel"}));
    assert_eq!((r["result"]["prompt"].clone(), r["result"]["dirty"].clone()), (json!(null), json!(true)));
    assert_eq!(r["result"]["undo"], "Add Node");
    assert!(control(&mut h, "ui.invoke", &json!({"command": "nope"}))["ok"] == false);
}

#[test]
fn copy_and_paste_through_the_clipboard_events() {
    let mut s = session();
    let a = add(&mut s, "text.literal", 20.0, 20.0);
    let b = add(&mut s, "text.upper", 400.0, 20.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.state_mut().canvas.picked.set(vec![a.as_str().into(), b.as_str().into()]);
    h.event(egui::Event::Copy);
    h.run_steps(2);
    let text = h.state().copied.clone().expect("copied");
    h.event(egui::Event::Paste(text));
    h.run_steps(3);
    h.get_by_label("4 nodes, 2 wires");
    let picked: Vec<String> = h.state().canvas.picked.nodes().iter().map(|n| n.0.clone()).collect();
    assert_eq!(picked.len(), 2);
    assert!(!picked.contains(&a) && !picked.contains(&b), "the copies are selected");
    // Cut: one undo step
    h.event(egui::Event::Cut);
    h.run_steps(3);
    h.get_by_label("2 nodes, 1 wires");
    assert_eq!(h.state().session.undo_label(), Some("Cut Nodes"));
    press(&mut h, CMD, egui::Key::Z);
    h.get_by_label("4 nodes, 2 wires");
    // junk on the clipboard is an error in the status bar, not a crash
    h.event(egui::Event::Paste("not a workflow".into()));
    h.run_steps(3);
    assert!(h.state().last_error.as_deref().is_some_and(|e| e.contains("workflow")));
}

/// The window's close button, as eframe reports it: a close request on the root viewport.
fn request_close(h: &mut egui_kittest::Harness<'static, EditorApp>) {
    h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    h.run_steps(1);
}

/// What the window asked of eframe last frame.
fn commands(h: &egui_kittest::Harness<'static, EditorApp>) -> Vec<egui::ViewportCommand> {
    h.output().viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
}

/// P04 (2026-10-27): "closing app with unsaved graph should confirm".
#[test]
fn closing_over_unsaved_changes_asks_first() {
    let dir = TempDir::new("quit");
    let mut s = session();
    add(&mut s, "text.literal", 20.0, 20.0);
    let (app, answers) = with_fake(EditorApp::new(s));
    let mut h = harness(app);
    // the close button: cancelled, and the prompt asks
    request_close(&mut h);
    assert!(commands(&h).contains(&egui::ViewportCommand::CancelClose), "{:?}", commands(&h));
    assert_eq!(h.state().prompt, Some(Prompt::Discard { then: AfterDiscard::Quit }));
    assert!(!h.state().closing);
    h.run_steps(2);
    h.get_by_label("Unsaved changes");
    // Cancel keeps the window and the graph
    click(&mut h, "Cancel");
    assert!(h.state().prompt.is_none() && !h.state().closing);
    h.get_by_label("1 nodes, 0 wires");
    // Cmd+Q (app.quit) asks too; Save with the dialog dismissed keeps the window
    press(&mut h, CMD, egui::Key::Q);
    assert_eq!(h.state().prompt, Some(Prompt::Discard { then: AfterDiscard::Quit }));
    click(&mut h, "Save");
    assert_eq!(answers.lock().unwrap().asked_save.len(), 1);
    assert!(!h.state().closing, "nothing saved: still open");
    // over the control channel: ui.invoke app.quit, then Discard closes
    let r = control(&mut h, "ui.invoke", &json!({"command": "app.quit"}));
    assert_eq!(r["result"]["prompt"]["kind"], "unsavedChanges", "{r}");
    assert_eq!(r["result"]["prompt"]["answers"], json!(["save", "discard", "cancel"]));
    let r = control(&mut h, "ui.prompt", &json!({"answer": "discard"}));
    assert_eq!(r["result"]["closing"], true, "{r}");
    h.run_steps(1);
    assert!(commands(&h).contains(&egui::ViewportCommand::Close), "{:?}", commands(&h));
    // once free to close, a close request isn't cancelled
    request_close(&mut h);
    assert!(!commands(&h).contains(&egui::ViewportCommand::CancelClose));

    // Save from the prompt: saved, then closed
    let mut s = session();
    add(&mut s, "text.literal", 20.0, 20.0);
    let (app, answers) = with_fake(EditorApp::new(s));
    answers.lock().unwrap().save = Some(dir.0.join("kept.json"));
    let mut h = harness(app);
    request_close(&mut h);
    h.run_steps(2);
    click(&mut h, "Save");
    assert!(dir.0.join("kept.json").exists());
    assert!(h.state().closing && !h.state().session.is_dirty());

    // nothing unsaved: the close goes ahead, no prompt
    let mut h = harness(EditorApp::new(session()));
    request_close(&mut h);
    assert!(!commands(&h).contains(&egui::ViewportCommand::CancelClose));
    assert!(h.state().prompt.is_none() && h.state().closing);
}
