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

use std::time::{Duration, Instant};

use plumcraft_editor_core::switchboard::{self, Effects, Next, Switchboard};
use plumcraft_editor_core::worker::Done;
use plumcraft_session::{EngineChoice, EngineTarget};
use serde_json::{Value, json};

use crate::EditorApp;
use crate::control::{Reply, err, ok};
use crate::link::{Link, repaint_waker};
use crate::run_view::Thumbs;

/// The longest a caller that may wait (outside a frame) waits for an engine to start: the
/// slowest start the launcher allows (60 s), and then some.
pub const START_WAIT: Duration = Duration::from_secs(120);

pub use plumcraft_editor_core::install::{Installer, Installing, KNOWN_ENGINES};
pub use plumcraft_editor_core::switchboard::Reloader;

/// Starts engine `engine` hosted as `mode` (one of the app's [`EngineChoice`]s) and links to it;
/// `repaint` is woken when its events arrive. It is called on the editor's engine worker, so it
/// is `Send`.
pub type Connector = Box<dyn FnMut(&str, &str, Option<&egui::Context>) -> Result<Link, String> + Send>;

/// A control request waiting for an engine switch to finish.
pub(crate) struct EngineWait {
    reply: Reply,
    ctx: egui::Context,
    /// `command.run`'s: the command's own result (for its `how`); else `ui.invoke`'s command.
    command: Option<Value>,
    invoked: Option<Value>,
    /// The switch's latest outcome.
    last: Option<Value>,
}

/// What the worker carries for a switch: the connector, and the context new links repaint.
type Carry = (Option<Connector>, Option<egui::Context>);

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
            switching: &mut $app.switching,
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

    /// Engine switching, without waiting (every frame): take what the worker brought back, and
    /// begin what the session recorded (`engine.switch`, `engine.reconnect`, `engine.attach`, …).
    /// `None` when nothing finished; else the last outcome.
    pub fn pump_engine(&mut self) -> Option<Result<Value, String>> {
        let mut out = Vec::new();
        self.step_engines(&mut out);
        out.pop()
    }

    /// Is an engine switch asked for, or under way?
    pub fn engine_busy(&self) -> bool {
        self.switching.busy() || self.session.engines.is_pending()
    }

    /// Wait (at most `timeout`) until no switch is asked for or under way and the engine worker
    /// has sent everything given it (runs, cancels): whether it came to that. For tests and
    /// scripts, never a frame.
    pub fn wait_engines(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let _ = self.pump_engine();
            if !self.engine_busy() && self.engine_worker.idle() {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || !self.engine_worker.wait(left) {
                return false;
            }
        }
    }

    /// Perform what the session asked of the engines, waiting for it here (a caller outside a
    /// frame): the last outcome, or the first error. The engines still start on the worker.
    pub(crate) fn engines_now(&mut self) -> Option<Result<Value, String>> {
        let deadline = Instant::now() + START_WAIT;
        let mut done = None;
        loop {
            let mut out = Vec::new();
            self.step_engines(&mut out);
            for r in out {
                if r.is_err() {
                    return Some(r);
                }
                done = Some(r);
            }
            if !self.switching.busy() {
                return done;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || !self.engine_worker.wait(left) {
                return Some(Err("the engine didn't start in time".into()));
            }
        }
    }

    /// Lend the engine parts to a [`Switchboard`] for `f`, then put what it changed on the
    /// canvas and the Watched panel.
    fn board<R>(&mut self, f: impl FnOnce(&mut Switchboard<'_>) -> R) -> R {
        let mut b = board!(self);
        let out = f(&mut b);
        let fx = b.effects;
        self.engine_effects(fx);
        out
    }

    /// Take what the worker brought back, finish it, begin what is asked next; outcomes to `out`.
    pub(crate) fn step_engines(&mut self, out: &mut Vec<Result<Value, String>>) {
        if !self.engine_worker.has_waker()
            && let Some(ctx) = &self.repaint
        {
            self.engine_worker.set_waker(Some(repaint_waker(ctx.clone())));
        }
        for done in self.engine_worker.drain() {
            match done {
                Done::RunFailed { run_id, generation, why } => {
                    // only the latest run fails: a superseded one's refusal is old news
                    let run = &self.session.run;
                    if run.run_id.as_deref() == Some(run_id.as_str()) && run.generation == generation {
                        self.session.run.fail(&format!("run: {why}"));
                    }
                }
                Done::Switched { worked, back } => {
                    if let Ok(carry) = back.downcast::<Carry>() {
                        self.connector = carry.0;
                    }
                    let next = self.board(|b| b.finish(*worked));
                    self.go(next, out);
                }
            }
        }
        while !self.switching.busy() && self.session.engines.is_pending() {
            let Some(next) = self.board(|b| b.begin()) else { break };
            self.go(next, out);
        }
        let retired = self.switching.take_retired();
        self.engine_worker.retire(retired);
        if !self.engine_waits.is_empty() && !self.engine_busy() {
            self.answer_engine_waits();
        }
    }

    /// Carry an operation on: in place for an engine compiled in, else on the worker.
    fn go(&mut self, mut next: Next, out: &mut Vec<Result<Value, String>>) {
        loop {
            match next {
                Next::Done(r) => {
                    self.engine_outcome(&r);
                    out.push(r);
                    return;
                }
                Next::Work(mut w) if w.in_process() => {
                    // nothing to wait for but this program's own code (no process, no network):
                    // started here, so a pick from the menu is connected in the same frame
                    let retired = w.take_retired();
                    self.engine_worker.retire(retired);
                    let ctx = self.repaint.clone();
                    let connector = &mut self.connector;
                    let worked = w.run(&mut |engine: &str, mode: &str| match connector {
                        Some(c) => c(engine, mode, ctx.as_ref()).map(Link::into_core),
                        None => Err("this editor can't start engines".into()),
                    });
                    next = self.board(|b| b.finish(worked));
                }
                Next::Work(w) => {
                    let carry: Carry = (self.connector.take(), self.repaint.clone());
                    self.engine_worker.switch(w, carry, |(connector, ctx): &mut Carry, engine: &str, mode: &str| match connector {
                        Some(c) => c(engine, mode, ctx.as_ref()).map(Link::into_core),
                        None => Err("this editor can't start engines".into()),
                    });
                    return;
                }
            }
        }
    }

    /// An outcome, for the control requests waiting on it and the Attach… window.
    fn engine_outcome(&mut self, r: &Result<Value, String>) {
        if self.attaching {
            self.attaching = false;
            // connected: the window's done; refused: it stays for another try
            if r.is_ok() {
                self.attach_form = None;
            }
        }
        match r {
            Ok(v) => self.engine_waits.iter_mut().for_each(|w| w.last = Some(v.clone())),
            // the first error is the answer, as a command that waits gets it
            Err(e) => {
                let (failed, waiting): (Vec<EngineWait>, Vec<EngineWait>) =
                    std::mem::take(&mut self.engine_waits).into_iter().partition(|w| w.command.is_some());
                self.engine_waits = waiting;
                for w in failed {
                    (w.reply)(err(e.clone()));
                }
            }
        }
    }

    /// The engines are idle: answer the control requests that waited for them.
    fn answer_engine_waits(&mut self) {
        for w in std::mem::take(&mut self.engine_waits) {
            let v = match (&w.command, w.last) {
                (Some(command), Some(mut out)) => {
                    if let (Some(how), Some(o)) = (command.get("how"), out.as_object_mut()) {
                        o.insert("how".into(), how.clone());
                    }
                    ok(out)
                }
                (Some(command), None) => ok(command.clone()),
                (None, _) => {
                    let mut out = self.inspect(&w.ctx);
                    if let Some(id) = &w.invoked {
                        out["invoked"] = id.clone();
                    }
                    ok(out)
                }
            };
            (w.reply)(v);
        }
    }

    /// A control request (`command.run`, `ui.invoke`, `ui.prompt`) whose command asked for an
    /// engine switch that is under way: reply when it is done. `v` is what it would reply now.
    pub(crate) fn wait_for_switch(&mut self, ctx: &egui::Context, method: &str, v: Value, reply: Reply) {
        let (command, invoked) = match method {
            "command.run" | "engine.execute" => (Some(v["result"].clone()), None),
            _ => (None, v["result"].get("invoked").cloned()),
        };
        self.engine_waits.push(EngineWait { reply, ctx: ctx.clone(), command, invoked, last: None });
        ctx.request_repaint();
    }

    /// Stop every engine (`engine.clear`). The graph stays, on the catalogue it had, with none
    /// connected; the Engine menu starts the next. (The old engines stop on the worker.)
    pub fn clear_engines(&mut self) -> Value {
        let v = self.board(|b| b.clear());
        let retired = self.switching.take_retired();
        self.engine_worker.retire(retired);
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
        switchboard::target_label(&self.session, target)
    }

    /// The engine the editor has (or tried to have), for the window title.
    pub fn engine_title(&self) -> Option<String> {
        switchboard::engine_title_connecting(&self.session, self.link.is_some(), self.switching.busy())
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

    /// `ui.inspect`'s `engine`.
    pub(crate) fn engine_inspect(&self) -> Value {
        let mut v = switchboard::inspect(&self.session, self.link.as_ref(), &self.engine, self.coordinator.as_ref());
        // asked for, or under way on the worker
        v["pending"] = json!(self.engine_busy());
        if self.switching.busy() {
            v["label"] = json!(self.engine_label());
        }
        v
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
