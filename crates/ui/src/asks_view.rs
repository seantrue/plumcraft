//! The egui shell's answers to the core's asks ([`plumcraft_editor_core::asks`]).
//!
//! The invoke state machine ([`plumcraft_editor_core::invoke`]) runs in the core and asks; this
//! shell answers with what it had before asks existed:
//!
//! - **File asks** (`file.open`, `file.save`, `folder.pick`) a person started are answered at
//!   once through [`FileDialogs`] (rfd's native dialogs, or a test's fakes), as the dialogs were
//!   called before. A script's (`ui.invoke`, `ask.answer`) are left for the script to answer:
//!   the window shows a small note with Choose… (the native dialog, taking it over) and Cancel.
//! - **Prompts** are the prompt windows ([`EditorApp::prompt`] mirrors the open one).
//! - **Forms** are the form window ([`crate::form_view`]).
//! - **`clipboard.set`** is egui's clipboard; **`clipboard.get`** asks the window for a Paste
//!   event, which answers it. A paste the window starts itself (⌘V) is the shell offering the
//!   clipboard to the core: `edit.paste` directly ([`invoke::paste`]).
//! - **`open.external`** (Open in …, Open With: the session's app starts, routed through the
//!   ask) is started here as the session started it before: `spawn_app`.
//! - **`share`** isn't asked by anything yet; answered "not available here".

use std::collections::HashSet;
use std::path::PathBuf;

use plumcraft_editor_core::asks::{Ask, AskId, Asks, By, Kind};
use plumcraft_editor_core::invoke::{self, Cont, Host, open_prompt};
use plumcraft_proto::catalogue::{PathHint, SaveFormat};
use plumcraft_session::Session;
use serde_json::{Map, Value, json};

use crate::{EditorApp, FileDialogs, prefs};

/// What the shell has presented and is waiting on (a paste, a form).
#[derive(Debug, Default)]
pub struct Presented {
    ids: HashSet<AskId>,
}

impl Host for EditorApp {
    fn session(&self) -> &Session {
        &self.doc.session
    }
    fn session_mut(&mut self) -> &mut Session {
        &mut self.doc.session
    }
    fn asks(&mut self) -> &mut Asks<Cont> {
        &mut self.doc.asks
    }
    fn run(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        self.try_run(id, params)
    }
    fn say(&mut self, text: String) {
        self.doc.last_error = Some(text);
    }
    fn prefs(&mut self) -> Option<&mut prefs::Store> {
        Some(&mut self.prefs)
    }
    fn selection(&self) -> (Vec<String>, Vec<plumcraft_graph::GroupId>) {
        Host::selection(&self.doc)
    }
    fn group_to_ungroup(&self) -> Option<plumcraft_graph::GroupId> {
        EditorApp::group_to_ungroup(self)
    }
    fn canvas_center(&self) -> Option<[f32; 2]> {
        let center = self.canvas.to_global.inverse() * self.canvas.rect.center();
        self.canvas.rect.is_finite().then_some([center.x, center.y])
    }
    /// Tidy's measurements: what the canvas reported to the core (N2 slice 3).
    fn measured(&self) -> (Map<String, Value>, Map<String, Value>) {
        Host::measured(&self.doc)
    }
    fn fit(&mut self) {
        self.canvas.request_fit();
    }
    /// The sizes the core has, as the canvas reported them (Frame Selection is the core's).
    fn sizes(&self, nodes: &[String], groups: &[plumcraft_graph::GroupId]) -> std::collections::BTreeMap<String, [f32; 2]> {
        Host::sizes(&self.doc, nodes, groups)
    }
    fn paste_pos(&self) -> Option<[f32; 2]> {
        self.paste_pos
    }
    fn close(&mut self) {
        self.closing = true;
        self.close_next = true;
    }
    fn closing(&self) -> bool {
        self.closing
    }
    fn open_window(&mut self, id: &str) -> bool {
        match id {
            "engine.install" => {
                if self.install_form.is_none() {
                    self.install_form = Some(crate::engine_view::InstallForm::default());
                }
                true
            }
            "engine.attach" => {
                if self.attach_form.is_none() {
                    self.attach_form = Some(crate::engine_view::AttachForm { url: "http://127.0.0.1:".into(), token: String::new() });
                }
                true
            }
            _ => false,
        }
    }
    fn allow_uploads(&mut self, files: &[String]) {
        if let Some(link) = &self.doc.link {
            link.conn.allow_uploads(files);
        }
    }
    fn take_held_run(&mut self) -> Option<Box<plumcraft_proto::run::RunRequest>> {
        self.doc.upload_held.take()
    }
    fn theme_name(&self) -> String {
        self.tokens.name.to_owned()
    }
    fn take_launches(&mut self) -> Vec<(PathBuf, Vec<String>, PathBuf)> {
        invoke::drain_launches(self.doc.launches.as_ref())
    }
}

/// `params[key]` as a path.
fn path_param(a: &Ask, key: &str) -> Option<PathBuf> {
    a.param_str(key).map(PathBuf::from)
}

/// Answer a file ask with the shell's [`FileDialogs`]: the answer, or `None` when the dialog was
/// dismissed (or the ask is one the dialogs have no way to ask).
fn ask_dialogs(dialogs: &mut dyn FileDialogs, a: &Ask) -> Option<Value> {
    let purpose = a.param_str("purpose").unwrap_or_default();
    // the folder this kind of dialog last used, as before every dialog that had one
    if let Some(start) = a.params.get("startIn")
        && purpose != "app"
    {
        dialogs.start_in(start.as_str().map(PathBuf::from));
    }
    let current = path_param(a, "current");
    let hint = || a.params.get("hint").and_then(|h| serde_json::from_value::<PathHint>(h.clone()).ok()).unwrap_or_default();
    let one = |p: Option<PathBuf>| p.map(|p| if a.kind == Kind::FileOpen { json!({"paths": [p]}) } else { json!({"path": p}) });
    match (a.kind, purpose) {
        (Kind::FileOpen, "workflow") => one(dialogs.open_workflow()),
        (Kind::FileSave, "workflow") => one(dialogs.save_workflow(current.as_deref())),
        (Kind::FileSave, "bundle") => one(dialogs.save_bundle(current.as_deref())),
        (Kind::FileOpen, "app") => one(dialogs.pick_app()),
        (Kind::FileSave, "result") => {
            let formats: Vec<SaveFormat> = a.params.get("formats").and_then(|f| serde_json::from_value(f.clone()).ok()).unwrap_or_default();
            let suggested = path_param(a, "suggested").unwrap_or_default();
            one(dialogs.save_output(&formats, &suggested))
        }
        (Kind::FileOpen, "theme") => one(dialogs.open_theme()),
        (Kind::FileSave, "theme") => one(dialogs.save_theme(a.param_str("name").unwrap_or("theme.plumtheme.json"))),
        (Kind::FileOpen, "files") => {
            let chosen = dialogs.pick_files(&hint(), a.param_str("current").unwrap_or_default());
            (!chosen.is_empty()).then(|| json!({"paths": chosen}))
        }
        (_, "path") => one(dialogs.pick_path(&hint(), a.param_str("current").unwrap_or_default())),
        _ => None,
    }
}

impl EditorApp {
    /// Run a command as its menu item does, for a person or a script ([`invoke::invoke`]).
    pub fn invoke_by(&mut self, id: &str, by: By) {
        invoke::invoke(self, id, by);
        self.present_asks();
    }

    /// Answer ask `id` (`ask.answer`), and present what follows.
    pub fn answer_ask(&mut self, id: AskId, answer: &Value) -> Result<(), String> {
        let r = invoke::answer(self, id, answer);
        self.present_asks();
        r
    }

    /// Cancel ask `id` (`ask.answer {cancel: true}`).
    pub fn cancel_ask(&mut self, id: AskId) -> Result<(), String> {
        let r = invoke::cancel(self, id);
        self.present_asks();
        r
    }

    /// Present what the core asked: a person's file asks through the dialogs at once, the
    /// clipboard, the prompt and form windows. Answering one may raise the next (the save dialog
    /// after the prompt's Save): it is presented in turn.
    pub fn present_asks(&mut self) {
        invoke::raise_launches(self);
        // (a chain is a few steps; the bound only guards against a loop)
        for _ in 0..32 {
            let next = self.doc.asks.open().find(|a| !self.presented.ids.contains(&a.id) && self.presents_now(a)).cloned();
            let Some(a) = next else { break };
            self.present(&a);
        }
        self.sync_asks();
    }

    /// Asks this shell answers as soon as they are raised.
    fn presents_now(&self, a: &Ask) -> bool {
        match a.kind {
            Kind::FileOpen | Kind::FileSave | Kind::FolderPick => a.by == By::Person,
            // a share is the person's to see; a script's is the script's to answer (11.2)
            Kind::Share => a.by == By::Person,
            Kind::ClipboardSet | Kind::ClipboardGet | Kind::OpenExternal | Kind::Form => true,
            // (this window takes its own screenshots: the core never asks it for one)
            Kind::Prompt | Kind::Text | Kind::Screenshot => false,
        }
    }

    fn present(&mut self, a: &Ask) {
        match a.kind {
            Kind::FileOpen | Kind::FileSave | Kind::FolderPick => self.present_file(a),
            Kind::ClipboardSet => {
                let text = a.param_str("text").unwrap_or_default().to_owned();
                if let Some(ctx) = &self.repaint {
                    ctx.copy_text(text.clone());
                }
                self.copied = Some(text);
                let _ = invoke::answer(self, a.id, &json!({}));
            }
            Kind::ClipboardGet => {
                // the window reads the system clipboard and sends a Paste event, which answers it
                self.presented.ids.insert(a.id);
                if let Some(ctx) = &self.repaint {
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                }
            }
            Kind::Form => {
                self.presented.ids.insert(a.id);
                if let Some(form) = a.param_str("command").and_then(plumcraft_session::commands::form) {
                    let values = form.fields.iter().map(|f| a.params["values"].get(f.key).and_then(Value::as_str).unwrap_or_default().to_owned()).collect();
                    let mut d = crate::form_view::FormDialog::new(form, values);
                    d.ask = Some(a.id);
                    self.command_form = Some(d);
                }
            }
            // started here, as the session started it before (decision 19)
            Kind::OpenExternal => {
                let (program, file) = (path_param(a, "program").unwrap_or_default(), path_param(a, "path").unwrap_or_default());
                let args: Vec<String> =
                    a.params.get("args").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
                let answer = match plumcraft_session::doc_cmds::spawn_app(&program, &args, &file) {
                    Ok(()) => json!({}),
                    Err(e) => json!({"error": format!("opening {}: {e}", file.display())}),
                };
                let _ = invoke::answer(self, a.id, &answer);
            }
            // egui has no share sheet: the file is shown in the file manager, to send from there
            Kind::Share => {
                let answer = match path_param(a, "file").or_else(|| path_param(a, "path")) {
                    Some(file) => match self.dialogs.reveal(&file) {
                        Ok(notice) => json!({"notice": notice}),
                        Err(e) => json!({"error": e}),
                    },
                    None => json!({"error": "nothing to share"}),
                };
                let _ = invoke::answer(self, a.id, &answer);
            }
            Kind::Prompt | Kind::Text | Kind::Screenshot => {}
        }
    }

    /// A file ask through the dialogs: answered, or cancelled when the dialog was dismissed.
    fn present_file(&mut self, a: &Ask) {
        match ask_dialogs(self.dialogs.as_mut(), a) {
            Some(answer) => {
                if let Err(e) = invoke::answer(self, a.id, &answer) {
                    self.doc.last_error = Some(e);
                }
            }
            None => {
                let _ = invoke::cancel(self, a.id);
            }
        }
    }

    /// The windows follow the asks: the prompt shown is the open prompt ask's, and the form
    /// closes once its ask has ended (a script answered it).
    pub(crate) fn sync_asks(&mut self) {
        self.prompt = open_prompt(&self.doc.asks).map(|(_, p)| p.clone());
        if let Some(d) = &mut self.command_form
            && let Some(id) = d.ask
        {
            match self.doc.asks.get(id) {
                None => self.command_form = None,
                Some(a) => d.error = a.param_str("error").map(str::to_owned),
            }
        }
        let open: HashSet<AskId> = self.doc.asks.open().map(|a| a.id).collect();
        self.presented.ids.retain(|id| open.contains(id));
    }

    /// Each frame: withdraw what lost its reason, time out what waited too long, present what
    /// is new.
    pub(crate) fn pump_asks(&mut self) {
        invoke::tidy(self);
        self.present_asks();
    }

    /// A Paste event's text: the answer to Edit › Paste when one is waiting, else the window
    /// offering the clipboard (⌘V) to the core.
    pub(crate) fn pasted(&mut self, text: &str) {
        match self.doc.asks.first(Kind::ClipboardGet).map(|a| a.id) {
            Some(id) => {
                let _ = invoke::answer(self, id, &json!({"text": text}));
            }
            None => invoke::paste(self, text),
        }
        self.sync_asks();
    }

    /// A script's file ask, while it waits: a note saying so, with the native dialog to take
    /// it over, and Cancel.
    pub(crate) fn script_asks_window(&mut self, ctx: &egui::Context) {
        let Some(a) = self.doc.asks.open().find(|a| a.kind.is_file() && a.by == By::Script).cloned() else { return };
        let (mut choose, mut cancel) = (false, false);
        egui::Window::new("Waiting for a script")
            .id(egui::Id::new("plumcraft-script-ask"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -36.0])
            .frame(egui::Frame::window(&ctx.global_style()).fill(self.tokens.panel))
            .show(ctx, |ui| {
                let title = a.param_str("title").unwrap_or(a.kind.name());
                ui.label(egui::RichText::new(format!("A script is answering “{title}” (ask {}).", a.id)).color(self.tokens.text));
                ui.horizontal(|ui| {
                    choose = ui.button("Choose…").on_hover_text("Answer it yourself, with the file dialog").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if choose {
            self.present_file(&a);
            self.present_asks();
        } else if cancel {
            let _ = self.cancel_ask(a.id);
        }
    }
}
