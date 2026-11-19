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
//! context its link repaints), the Engine menu's choices, the Attach… and Install Engine…
//! windows, and putting the core's [`Effects`] on the canvas and the Watched panel.

use plumcraft_editor_core::switchboard::{self, Effects, Switchboard};
use plumcraft_session::{EngineChoice, EngineTarget};
use serde_json::{Value, json};

use crate::EditorApp;
use crate::link::{Link, repaint_waker};
use crate::run_view::Thumbs;

pub use plumcraft_editor_core::install::{Installer, Installing, KNOWN_ENGINES};
pub use plumcraft_editor_core::switchboard::Reloader;

/// Starts engine `engine` hosted as `mode` (one of the app's [`EngineChoice`]s) and links to it;
/// `repaint` is woken when its events arrive.
pub type Connector = Box<dyn FnMut(&str, &str, Option<&egui::Context>) -> Result<Link, String>>;

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

/// The editor's engine parts, lent to the core's [`Switchboard`] for one operation (each field
/// borrowed apart, so the editor's [`Connector`] can be borrowed beside them).
macro_rules! board {
    ($app:expr) => {
        Switchboard {
            session: &mut $app.session,
            link: &mut $app.link,
            coordinator: &mut $app.coordinator,
            state: &mut $app.engine,
            last_error: &mut $app.last_error,
            reloader: &mut $app.reloader,
            installer: &$app.installer,
            installing: &mut $app.installing,
            waker: $app.repaint.clone().map(repaint_waker),
            effects: Effects::default(),
        }
    };
}

impl EditorApp {
    /// Offer `choices` in the Engine menu, started by `connector`; `current` is the one the app
    /// connected at startup (or tried to).
    pub fn with_engines(mut self, choices: Vec<EngineChoice>, current: Option<(String, String)>, connector: Connector) -> Self {
        self.session.engines.choices = choices;
        self.session.engines.current = current.map(|(engine, mode)| EngineTarget::Choice { engine, mode });
        self.session.engines.can_attach = true;
        self.connector = Some(connector);
        self
    }

    /// How `engine.install` downloads and installs an engine; offers the command.
    pub fn with_installer(mut self, installer: Installer) -> Self {
        self.session.engines.can_install = true;
        self.installer = Some(installer);
        self
    }

    /// Take a finished install: read the engine manifests again, so the Engine menu offers what
    /// was installed, and say how it went. Returns whether one finished.
    pub fn poll_install(&mut self) -> bool {
        if self.installing.is_none() {
            // every frame: nothing to lend
            return false;
        }
        let mut b = board!(self);
        let done = b.poll_install();
        let fx = b.effects;
        self.engine_effects(fx);
        done
    }

    /// Run `f` on the editor's engine parts with its connector, then put what it changed on the
    /// canvas and the Watched panel.
    fn with_board<R>(&mut self, f: impl FnOnce(&mut Switchboard<'_>, &mut switchboard::Connect<'_>) -> R) -> R {
        let ctx = self.repaint.clone();
        let connector = &mut self.connector;
        let mut connect = |engine: &str, mode: &str| match connector {
            Some(c) => c(engine, mode, ctx.as_ref()).map(Link::into_core),
            None => Err("this editor can't start engines".into()),
        };
        let mut b = board!(self);
        let out = f(&mut b, &mut connect);
        let fx = b.effects;
        self.engine_effects(fx);
        out
    }

    /// What the core's [`Effects`] mean for what the editor keeps beside the session.
    pub(crate) fn engine_effects(&mut self, fx: Effects) {
        if fx.runs_forgotten {
            self.watched.clear();
            self.thumbs = Thumbs::default();
            self.rerun_pending = false;
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
        self.reloader = Some(reloader);
        self
    }

    /// Perform a switch the session recorded (`engine.switch`, `engine.reconnect`,
    /// `engine.attach`, …). `None` when none was asked for; else the outcome.
    pub fn pump_engine(&mut self) -> Option<Result<Value, String>> {
        if !self.session.engines.is_pending() {
            // every frame: nothing to lend
            return None;
        }
        self.with_board(|b, connect| b.pump(connect))
    }

    /// Stop every engine (`engine.clear`). The graph stays, on the catalogue it had, with none
    /// connected; the Engine menu starts the next.
    pub fn clear_engines(&mut self) -> Value {
        self.with_board(|b, _| b.clear())
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
        switchboard::target_label(&self.session, target)
    }

    /// The engine the editor has (or tried to have), for the window title.
    pub fn engine_title(&self) -> Option<String> {
        switchboard::engine_title(&self.session, self.link.is_some())
    }

    /// The Engine menu's choices, after its fixed commands: Remove for each added engine, then
    /// every engine in a list that scrolls, each a submenu of its connections with the one in
    /// use checked (or, picking engines automatically, one item that starts it the fastest way
    /// it can be hosted). A pick is `engine.use`: it starts, adds or changes that engine, so Clear
    /// Engines and a pick make a complete change. Returns the command clicked.
    pub(crate) fn engine_choices_menu(&self, ui: &mut egui::Ui) -> Option<crate::canvas::Action> {
        let engines = &self.session.engines;
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
        let live = self.link.is_some();
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
                    go = ui.button("Attach").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if go {
            let p = json!({"url": form.url, "token": form.token});
            if self.run("engine.attach", &p).is_some() {
                self.attach_form = None;
            }
        } else if cancel || !open {
            self.attach_form = None;
        }
    }

    /// `ui.inspect`'s `engine`.
    pub(crate) fn engine_inspect(&self) -> Value {
        switchboard::inspect(&self.session, self.link.as_ref(), &self.engine, self.coordinator.as_ref())
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
