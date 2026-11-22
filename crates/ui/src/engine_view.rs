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
//! context its link repaints; the document holds it as its starter), the Attach… and Install
//! Engine… windows, and putting the core's [`Effects`] on the canvas and
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

pub use plumcraft_editor_core::install::{Installer, Installing, KNOWN_ENGINES, Progress};
pub use plumcraft_editor_core::switchboard::Reloader;
use plumcraft_session::commands::FieldKind;

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
        // which engine offers a kind a file doesn't name, for Pick Engines Automatically
        self.doc.session.engines.offered = plumcraft_editor_core::engines::offered(&choices);
        // and their node types, so the palette lists a kind whose engine isn't started; choosing
        // one starts that engine and adds the node once it has connected (N4 slice 1, D3)
        self.doc.session.engines.offered_defs = plumcraft_editor_core::engines::offered_defs(&choices);
        self.doc.hold_adds = true;
        self.doc.session.engines.choices = choices;
        self.doc.session.engines.current = current.map(|(engine, mode)| EngineTarget::Choice { engine, mode });
        self.doc.session.engines.can_attach = true;
        self.doc.starter = Some(starter(connector, std::sync::Arc::clone(&self.repaint_cell)));
        self
    }

    /// Offer the engines `launcher`'s manifests name and installing engines, as the editor core
    /// decides for every shell ([`Document::offer_installs`], B71): the application's own setup.
    pub fn with_launcher(mut self, launcher: &plumcraft_editor_core::engines::Launcher) -> Self {
        self.doc.offer_installs(launcher);
        self
    }

    /// A stand-in installer, offered as if the platform could install (tests): what
    /// [`EditorApp::with_launcher`] gives the core's own.
    pub fn with_installer(mut self, installer: Installer) -> Self {
        self.doc.session.engines.can_install = true;
        self.doc.session.engines.install_refused = None;
        self.doc.installer = Some(installer);
        self
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
                // the core's form (invoke::engine_form): its pick list of the public engines, with
                // any other repository typed (decision 5), as every shell draws it
                if let Some(f) = plumcraft_editor_core::invoke::engine_form("engine.install") {
                    for field in f.fields.iter().filter(|f| f.key == "repo") {
                        ui.horizontal(|ui| {
                            ui.label(field.label);
                            match field.kind {
                                FieldKind::Pick { choices, other } => {
                                    crate::form_view::pick_field(ui, "plumcraft-install-repo", choices, other, &mut form.repo, field.hint, 320.0);
                                }
                                _ => {
                                    ui.add(egui::TextEdit::singleline(&mut form.repo).hint_text(field.hint).desired_width(320.0));
                                }
                            }
                        });
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

    /// How `engine.manifests.reload` reads the manifests again (a test's stand-in;
    /// [`EditorApp::with_launcher`] gives the core's own).
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

    /// The engine the editor has (or tried to have), for the window title.
    pub fn engine_title(&self) -> Option<String> {
        switchboard::engine_title_connecting(&self.doc.session, self.doc.link.is_some(), self.doc.switching.busy())
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
            // nothing of the old graph stays watched (nor selected or entered: the core's)
            self.follow_core();
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
