//! The Engine menu and switching engines while the editor runs.
//!
//! The switching itself is the editor core's ([`plumcraft_editor_core::switchboard`], stage N1,
//! slice 2), whose docs say what a switch, an add, a remove, an attach and a clear do. The
//! session's `engine.*` commands only record the request; the editor performs it
//! ([`EditorApp::pump_engine`]), right after the command runs and at the start of every frame, by
//! lending its engine parts to a [`Switchboard`]. Only the app knows how to start an engine, so it
//! hands the editor a [`Connector`] and the list of choices it can start
//! ([`EditorApp::with_engines`]); attaching to a running HTTP engine needs nothing from the app.
//!
//! What stays here is what draws or needs an egui context: the [`Connector`] (it takes the
//! context its link repaints; the document holds it as its starter), the Engine menu's choices,
//! the Attach… and Install Engine… windows, and putting the core's [`Effects`] on the canvas and
//! the Watched panel. The parts themselves are the document's
//! ([`plumcraft_editor_core::document::Document`]), and the engine stepping is the core's.
//!
//! **The window never waits for an engine** (slice 3). Inside a frame, the part of a switch that
//! waits for engines (stopping the old, starting the new, its catalogue) goes to the editor's
//! engine worker ([`plumcraft_editor_core::worker`]), with the connector, and the frame goes on;
//! the title and status bar say what is connecting. The worker's answer is taken at the start of
//! a later frame ([`EditorApp::pump_engine`]). The one exception is an engine compiled into the
//! program (in-process, no process or network to wait for), which is started in place, as
//! before. A control request that switched replies once the switch is done
//! (`docs/control-protocol.md`: `engine.*` "switch before they reply"). Outside a frame (tests,
//! scripts, the API) a command that switches waits for the worker, as it always waited.

use std::time::Duration;

use plumcraft_editor_core::control::{self as core, Driver};
use plumcraft_editor_core::document::{self, Document, Holder, Starter};
use plumcraft_editor_core::switchboard::{self, Effects, Switchboard, Switching, Work, Worked};
use plumcraft_editor_core::worker::EngineWorker;
use plumcraft_session::{EngineChoice, EngineTarget};
use serde_json::{Value, json};

use crate::EditorApp;
use crate::link::Link;
use crate::run_view::Thumbs;

/// The longest a caller that may wait (outside a frame) waits for an engine to start: the
/// slowest start the launcher allows (60 s), and then some.
pub const START_WAIT: Duration = document::START_WAIT;

pub use plumcraft_editor_core::install::{Installer, Installing, KNOWN_ENGINES};
pub use plumcraft_editor_core::switchboard::Reloader;

/// Starts engine `engine` hosted as `mode` (one of the app's [`EngineChoice`]s) and links to it;
/// `repaint` is woken when its events arrive. It is called on the editor's engine worker, so it
/// is `Send`.
pub type Connector = Box<dyn FnMut(&str, &str, Option<&egui::Context>) -> Result<Link, String> + Send>;

/// The Install Engine… window's field.
#[derive(Debug, Clone, Default)]
pub struct InstallForm {
    pub repo: String,
}

/// The Attach… window's fields.
#[derive(Debug, Clone, Default)]
pub struct AttachForm {
    pub url: String,
    pub token: String,
}

/// The egui shell's [`Connector`] as the core's [`Starter`]: the links it starts repaint the
/// window's context (known from the first frame).
fn starter(mut connector: Connector, ctx: std::sync::Arc<std::sync::Mutex<Option<egui::Context>>>) -> Starter {
    Box::new(move |engine: &str, mode: &str, _waker| {
        let ctx = ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        connector(engine, mode, ctx.as_ref()).map(Link::into_core)
    })
}

impl EditorApp {
    /// Offer `choices` in the Engine menu, started by `connector`; `current` is the one the app
    /// connected at startup (or tried to).
    pub fn with_engines(mut self, choices: Vec<EngineChoice>, current: Option<(String, String)>, connector: Connector) -> Self {
        self.doc.session.engines.choices = choices;
        self.doc.session.engines.current = current.map(|(engine, mode)| EngineTarget::Choice { engine, mode });
        self.doc.session.engines.can_attach = true;
        self.doc.starter = Some(starter(connector, std::sync::Arc::clone(&self.repaint_cell)));
        self
    }

    /// How `engine.install` downloads and installs an engine; offers the command.
    pub fn with_installer(mut self, installer: Installer) -> Self {
        self.doc.session.engines.can_install = true;
        self.doc.installer = Some(installer);
        self
    }

    /// Take a finished install: read the engine manifests again, so the Engine menu offers what
    /// was installed, and say how it went. Returns whether one finished.
    pub fn poll_install(&mut self) -> bool {
        if self.doc.installing.is_none() {
            // every frame: nothing to lend
            return false;
        }
        self.board(|b| b.poll_install())
    }

    /// Run `f` on the editor's engine parts with its starter, waiting for engines here (the
    /// synchronous API), then put what it changed on the canvas and the Watched panel.
    fn with_board<R>(&mut self, f: impl FnOnce(&mut Switchboard<'_>, &mut switchboard::Connect<'_>) -> R) -> R {
        let waker = self.doc.waker().cloned();
        let mut starter = self.doc.starter.take();
        let out = {
            let mut connect = |engine: &str, mode: &str| match &mut starter {
                Some(s) => s(engine, mode, waker.clone()),
                None => Err("this editor can't start engines".into()),
            };
            self.doc.board(|b| f(b, &mut connect))
        };
        self.doc.starter = starter;
        let fx = self.doc.take_effects();
        self.engine_effects(fx);
        out
    }

    /// What the core's [`Effects`] mean for what the editor keeps beside the session.
    pub(crate) fn engine_effects(&mut self, fx: Effects) {
        if fx.runs_forgotten {
            self.watched.clear();
            self.thumbs = Thumbs::default();
            self.doc.rerun_pending = false;
        }
        if let Some(first) = fx.catalog_set {
            self.catalog_changed(first);
        }
        if fx.redraw {
            self.canvas.invalidate();
        }
    }

    /// The Install Engine… window: a repository, the public engines to pick from, then
    /// `engine.install`.
    pub(crate) fn install_window(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.install_form else { return };
        let (mut go, mut cancel) = (false, false);
        let mut open = true;
        egui::Window::new("Install an Engine")
            .id(egui::Id::new("plumcraft-install"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .frame(egui::Frame::window(&ctx.global_style()).fill(self.tokens.panel))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("A Python engine from GitHub, installed with uv and registered, so every editor offers it.")
                        .color(self.tokens.text_dim),
                );
                ui.horizontal(|ui| {
                    ui.label("Repository");
                    ui.add(egui::TextEdit::singleline(&mut form.repo).hint_text("owner/repo").desired_width(320.0));
                });
                ui.add_space(4.0);
                for (label, repo) in KNOWN_ENGINES {
                    if ui.radio(form.repo == *repo, *label).on_hover_text(format!("https://github.com/{repo}")).clicked() {
                        form.repo = (*repo).to_owned();
                    }
                }
                ui.add_space(4.0);
                ui.label(egui::RichText::new("It runs the package's own code: install only what you trust.").small().color(self.tokens.text_dim));
                ui.horizontal(|ui| {
                    go = ui.add_enabled(!form.repo.trim().is_empty(), egui::Button::new("Install")).clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if go {
            let p = json!({"repo": form.repo});
            if self.run("engine.install", &p).is_some() {
                self.install_form = None;
            }
        } else if cancel || !open {
            self.install_form = None;
        }
    }

    /// How `engine.manifests.reload` reads the manifests again.
    pub fn with_reloader(mut self, reloader: Reloader) -> Self {
        self.doc.reloader = Some(reloader);
        self
    }

    /// Engine switching, without waiting (every frame): take what the worker brought back, and
    /// begin what the session recorded (`engine.switch`, `engine.reconnect`, `engine.attach`, …).
    /// `None` when nothing finished; else the last outcome. The loop is the core's
    /// ([`plumcraft_editor_core::control::step`]), as the C ABI's core runs it. Once the first
    /// engine has connected, what waited for it opens ([`EditorApp::start_engines`]).
    pub fn pump_engine(&mut self) -> Option<Result<Value, String>> {
        let mut out = Vec::new();
        self.step_engines(&mut out);
        if document::settle_startup(self) {
            let fx = self.doc.take_effects();
            self.engine_effects(fx);
            self.sync_asks();
            if let Some(ready) = self.on_ready.take() {
                ready(self);
            }
        }
        out.pop()
    }

    /// Is an engine switch asked for, or under way?
    pub fn engine_busy(&self) -> bool {
        self.doc.engine_busy()
    }

    /// Wait (at most `timeout`) until no switch is asked for or under way and the engine worker
    /// has sent everything given it (runs, cancels): whether it came to that. For tests and
    /// scripts, never a frame.
    pub fn wait_engines(&mut self, timeout: Duration) -> bool {
        core::wait_engines(self, timeout)
    }

    /// Lend the engine parts to a [`Switchboard`] for `f`, then put what it changed on the
    /// canvas and the Watched panel.
    fn board<R>(&mut self, f: impl FnOnce(&mut Switchboard<'_>) -> R) -> R {
        let out = self.doc.board(f);
        let fx = self.doc.take_effects();
        self.engine_effects(fx);
        out
    }

    /// Take what the worker brought back, finish it, begin what is asked next; outcomes to `out`.
    pub(crate) fn step_engines(&mut self, out: &mut Vec<Result<Value, String>>) {
        core::step(self, out);
    }

    /// Stop every engine (`engine.clear`). The graph stays, on the catalogue it had, with none
    /// connected; the Engine menu starts the next. (The old engines stop on the worker.)
    pub fn clear_engines(&mut self) -> Value {
        let v = self.board(|b| b.clear());
        let retired = self.doc.switching.take_retired();
        Driver::worker(&mut self.doc).retire(retired);
        v
    }

    /// Start `target` beside the engines there are ([`switchboard`]).
    pub fn add_engine(&mut self, target: EngineTarget) -> Result<Value, String> {
        self.with_board(|b, connect| b.add(target, connect))
    }

    /// Stop an added engine.
    pub fn remove_engine(&mut self, id: &str) -> Result<Value, String> {
        self.with_board(|b, _| b.remove(id))
    }

    /// Connect to `target` in place of the engines there are ([`switchboard`]).
    pub fn switch_engine(&mut self, target: EngineTarget) -> Result<Value, String> {
        self.with_board(|b, connect| b.switch(target, connect))
    }

    /// What `target` is called in the status bar and the title.
    fn target_label(&self, target: &EngineTarget) -> String {
        switchboard::target_label(&self.doc.session, target)
    }

    /// The engine the editor has (or tried to have), for the window title.
    pub fn engine_title(&self) -> Option<String> {
        switchboard::engine_title_connecting(&self.doc.session, self.doc.link.is_some(), self.doc.switching.busy())
    }

    /// The Engine menu's choices, after its fixed commands: Remove for each added engine, then
    /// every engine in a list that scrolls, each a submenu of its connections with the one in
    /// use checked (or, picking engines automatically, one item that starts it the fastest way
    /// it can be hosted). A pick is `engine.use`: it starts, adds or changes that engine, so Clear
    /// Engines and a pick make a complete change. Returns the command clicked.
    pub(crate) fn engine_choices_menu(&self, ui: &mut egui::Ui) -> Option<crate::canvas::Action> {
        let engines = &self.doc.session.engines;
        if engines.choices.is_empty() {
            ui.label(egui::RichText::new("This editor can't start engines.").color(self.tokens.text_dim));
            return None;
        }
        let mut picked = None;
        let mut removable = false;
        for t in &engines.added {
            if let EngineTarget::Choice { engine, .. } = t {
                removable = true;
                let label = format!("Remove {}", self.target_label(t));
                if ui.button(label).on_hover_text(format!("engine.remove {{engine: {engine:?}}}")).clicked() {
                    picked = Some(("engine.remove", json!({"engine": engine})));
                    ui.close();
                }
            }
        }
        if removable {
            ui.separator();
        }
        // after a clear, or a connect that failed, nothing is in use
        let live = self.doc.link.is_some();
        let all: Vec<&EngineChoice> = engines.choices.iter().collect();
        scrolling(ui, "engine-choices", |ui| {
            for (label, modes) in by_engine(&all) {
                let using = modes.first().and_then(|c| engines.in_use(&c.engine)).filter(|_| live);
                let using = using.and_then(|m| modes.iter().copied().find(|c| c.mode == m));
                // an engine in use says how it's connected
                let heading = match using {
                    Some(c) => egui::RichText::new(format!("{label} · {}", c.mode_label)).strong(),
                    None => egui::RichText::new(label),
                };
                // picking automatically, an engine is one item: engine.use with no mode starts it
                // the fastest way it can be hosted (#24)
                if engines.auto {
                    let tip = format!("engine.use {{engine: {:?}}}", group_engine(&modes));
                    if ui.radio(using.is_some(), heading).on_hover_text(tip).clicked() {
                        picked = Some(("engine.use", json!({"engine": group_engine(&modes)})));
                        ui.close();
                    }
                    continue;
                }
                ui.menu_button(heading, |ui| {
                    for c in &modes {
                        let on = using.is_some_and(|u| u.mode == c.mode);
                        let tip = format!("engine.use {{engine: {:?}, mode: {:?}}}", c.engine, c.mode);
                        if ui.radio(on, &c.mode_label).on_hover_text(tip).clicked() {
                            picked = Some(("engine.use", json!({"engine": c.engine, "mode": c.mode})));
                            ui.close();
                        }
                    }
                });
            }
        });
        picked
    }

    /// The Attach… window: a URL and a token, then `engine.attach`.
    pub(crate) fn attach_window(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.attach_form else { return };
        let (mut go, mut cancel) = (false, false);
        let mut open = true;
        let attaching = self.attaching;
        egui::Window::new("Attach to an Engine")
            .id(egui::Id::new("plumcraft-attach"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .frame(egui::Frame::window(&ctx.global_style()).fill(self.tokens.panel))
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("An HTTP engine already running (started with --listen).").color(self.tokens.text_dim));
                egui::Grid::new("plumcraft-attach-grid").num_columns(2).show(ui, |ui| {
                    ui.label("URL");
                    ui.add(egui::TextEdit::singleline(&mut form.url).hint_text("http://127.0.0.1:8123").desired_width(280.0));
                    ui.end_row();
                    ui.label("Token");
                    ui.add(egui::TextEdit::singleline(&mut form.token).password(true).desired_width(280.0));
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    go = ui.add_enabled(!attaching, egui::Button::new(if attaching { "Connecting…" } else { "Attach" })).clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if go && !self.attaching {
            let p = json!({"url": form.url, "token": form.token});
            let r = self.run("engine.attach", &p);
            if self.engine_busy() {
                // connecting on the worker: the window closes once it has connected
                self.attaching = true;
            } else if r.is_some() {
                self.attach_form = None;
            }
        } else if cancel || !open {
            self.attaching = false;
            self.attach_form = None;
        }
    }
}

/// The window's engine switching, as the core drives it ([`plumcraft_editor_core::control::step`]):
/// the document's, with the window's canvas redone after each operation.
impl Driver for EditorApp {
    fn worker(&mut self) -> &mut EngineWorker {
        Driver::worker(&mut self.doc)
    }
    fn switching(&mut self) -> &mut Switching {
        &mut self.doc.switching
    }
    fn board<R>(&mut self, f: impl FnOnce(&mut Switchboard<'_>) -> R) -> R {
        EditorApp::board(self, f)
    }
    /// In place for an engine compiled in (the document's `start_in_place`), else on the worker.
    fn start(&mut self, w: Work) -> Option<Worked> {
        self.doc.start(w)
    }
    fn take_back(&mut self, back: Box<dyn std::any::Any + Send>) {
        self.doc.take_back(back);
    }
    /// The Attach… window: connected, it's done; refused, it stays for another try.
    fn outcome(&mut self, r: &Result<Value, String>) {
        if self.attaching {
            self.attaching = false;
            if r.is_ok() {
                self.attach_form = None;
            }
        }
    }
}

/// The window around the document: its look, fit, selection and Watched panel follow a command.
impl Holder for EditorApp {
    fn doc(&self) -> &Document {
        &self.doc
    }
    fn doc_mut(&mut self) -> &mut Document {
        &mut self.doc
    }
    fn shell_command(&mut self, id: &str, v: Value) -> Result<Value, String> {
        self.window_command(id, v)
    }
    fn after_command(&mut self, id: &str, _v: &Value) {
        if id == "graph.new" || id == "graph.open" {
            self.canvas.request_fit();
            // nothing of the old graph stays selected, entered or watched
            self.canvas.picked.clear();
            self.canvas.leave_group();
            self.watched.clear();
        }
        if id == "run.watch" {
            self.watched.clear();
        }
    }
    fn may_wait(&self) -> bool {
        !self.in_frame
    }
    fn note_engine_asked(&mut self, asked: bool) {
        self.engine_asked = asked;
    }
}

/// Choices grouped by engine, in the order they came: each engine's label and its connections.
fn by_engine<'a>(choices: &[&'a EngineChoice]) -> Vec<(&'a str, Vec<&'a EngineChoice>)> {
    let mut groups: Vec<(&str, Vec<&EngineChoice>)> = Vec::new();
    for c in choices {
        match groups.iter_mut().find(|(_, g)| g.first().is_some_and(|f| f.engine == c.engine)) {
            Some((_, g)) => g.push(c),
            None => groups.push((c.engine_label.as_str(), vec![c])),
        }
    }
    groups
}

/// The engine id a group of [`by_engine`] choices share.
fn group_engine<'a>(modes: &[&'a EngineChoice]) -> &'a str {
    modes.first().map_or("", |c| c.engine.as_str())
}

/// A menu list that scrolls once it would run off the bottom of the window.
fn scrolling(ui: &mut egui::Ui, salt: &str, add: impl FnOnce(&mut egui::Ui)) {
    let room = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 24.0).max(120.0);
    // a menu offers less than the window, so the list asks for what is left below it
    egui::ScrollArea::vertical().id_salt(salt).min_scrolled_height(room).max_height(room).auto_shrink([true, true]).show(ui, add);
}
