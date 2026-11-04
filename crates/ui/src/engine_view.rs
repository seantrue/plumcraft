//! The Engine menu and switching engines while the editor runs.
//!
//! The split: the session's `engine.*` commands ([`plumcraft_session::engine_cmds`]) only
//! record the request, as run commands queue theirs; the editor performs it
//! ([`EditorApp::pump_engine`]), right after the command runs and at the start of every frame.
//! Only the app knows how to start an engine, so it hands the editor a [`Connector`] and the
//! list of choices it can start ([`EditorApp::with_engines`]); attaching to a running HTTP engine
//! needs nothing from the app (`HttpHost::attach`).
//!
//! A switch cancels the run in flight (the cancel goes to the old engine), drops the old link
//! (its pump is joined, so a spawned engine is stopped: `ProcessHost`'s shutdown, `HttpHost`'s
//! lifeline), forgets the old engine's run state (node states, the last reply, thumbnails, the
//! watched value; what is watched stays), connects the new one and takes its catalogue. The
//! graph is kept: nodes or wires the new catalogue doesn't have are reported in the status bar,
//! as `graph.open` reports a file's problems, and nothing is deleted. A failed connect says why
//! and leaves the editor usable on the old catalogue, with no engine; the Engine menu still
//! works.
//!
//! **Several engines** (`docs/research/15-multi-engine.md`): picking an engine in the Engine menu
//! that isn't connected (`engine.use`) starts it beside the ones there are. The first add puts the engines behind a
//! [`plumcraft_coordinator::Coordinator`], and the editor links to that instead: one catalogue
//! (merged), one run state, a graph whose nodes run on whichever engine offers them. Remove
//! stops an added engine. Each add or remove relinks, so the run in flight is cancelled and the
//! results forgotten, as on a switch. Switch and Attach go back to one engine; Reconnect
//! restarts the first and adds the others again; Clear Engines stops them all.

use std::sync::Arc;
use std::time::Instant;

use plumcraft_coordinator::Coordinator;
use plumcraft_host::Connection;
use plumcraft_host::http::{HttpHost, HttpOptions};
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::session::EngineState;
use plumcraft_session::{EngineChange, EngineChoice, EngineTarget};
use serde_json::{Value, json};

use crate::EditorApp;
use crate::link::Link;
use crate::run_view::Thumbs;

/// Starts engine `engine` hosted as `mode` (one of the app's [`EngineChoice`]s) and links to it;
/// `repaint` is woken when its events arrive.
pub type Connector = Box<dyn FnMut(&str, &str, Option<&egui::Context>) -> Result<Link, String>>;

/// Reads the engine manifests again: the choices there are now, and what couldn't be read.
pub type Reloader = Box<dyn FnMut() -> (Vec<EngineChoice>, Vec<String>)>;

/// Downloads an engine from GitHub (its https URL) and installs it, off the UI thread: what to
/// tell the person, or why it failed. The app supplies it ([`EditorApp::with_installer`]).
pub type Installer = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

/// Public engines the Install Engine… window offers, as `(label, owner/repo)`.
pub const KNOWN_ENGINES: &[(&str, &str)] = &[
    ("DuckDB: data files into tables, reshaped with SQL", "seantrue/plumcraft-duckdb-engine"),
    ("OpenCV: filters, edges, thresholds, morphology, geometry", "seantrue/plumcraft-opencv-engine"),
    ("Atlas: a sample engine with every type", "seantrue/plumcraft-atlas-engine"),
];

/// The Install Engine… window's field.
#[derive(Debug, Clone, Default)]
pub struct InstallForm {
    pub repo: String,
}

/// An install running in the background.
pub struct Installing {
    pub url: String,
    rx: std::sync::mpsc::Receiver<Result<String, String>>,
}

/// The Attach… window's fields.
#[derive(Debug, Clone, Default)]
pub struct AttachForm {
    pub url: String,
    pub token: String,
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

    /// Start installing `url` on a thread of its own; [`EditorApp::poll_install`] takes the end.
    fn start_install(&mut self, url: String) -> Result<Value, String> {
        let Some(installer) = self.installer.clone() else { return Err("this editor can't install engines".into()) };
        if let Some(i) = &self.installing {
            return Err(format!("installing {}", i.url));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (u, ctx) = (url.clone(), self.repaint.clone());
        std::thread::Builder::new()
            .name("plumcraft-install".into())
            .spawn(move || {
                let _ = tx.send(installer(&u));
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            })
            .map_err(|e| format!("can't start the install: {e}"))?;
        self.session.engines.installing = Some(url.clone());
        self.installing = Some(Installing { url: url.clone(), rx });
        self.last_error = None;
        Ok(json!({"installing": url}))
    }

    /// Take a finished install: read the engine manifests again, so the Engine menu offers what
    /// was installed, and say how it went. Returns whether one finished.
    pub fn poll_install(&mut self) -> bool {
        let Some(i) = &self.installing else { return false };
        let outcome = match i.rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the install stopped without saying why".into()),
        };
        let url = i.url.clone();
        self.installing = None;
        self.session.engines.installing = None;
        self.last_error = Some(match outcome {
            Ok(said) => match self.reload_engines() {
                Ok(_) => format!("{said}. The Engine menu offers it now."),
                Err(e) => format!("{said}, but the engine list didn't reload: {e}"),
            },
            Err(e) => format!("installing {url}: {e}"),
        });
        true
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

    /// `engine.manifests.reload`: the new choices; what couldn't be read goes to the status bar.
    fn reload_engines(&mut self) -> Result<Value, String> {
        let Some(reload) = &mut self.reloader else { return Err("this editor has no engine manifests to read".into()) };
        let (choices, problems) = reload();
        let engines = choices.iter().map(|c| c.engine.as_str()).collect::<std::collections::BTreeSet<_>>().len();
        self.session.engines.choices = choices;
        self.last_error = problems.first().map(|p| format!("engine manifests: {p}"));
        Ok(json!({"engines": engines, "problems": problems}))
    }

    /// Perform a switch the session recorded (`engine.switch`, `engine.reconnect`,
    /// `engine.attach`). `None` when none was asked for; else the outcome.
    pub fn pump_engine(&mut self) -> Option<Result<Value, String>> {
        if let Some(target) = self.session.engines.take_request() {
            // Reconnect restarts the first engine and adds the others again; a switch drops them.
            let reconnect = self.session.engines.current.as_ref() == Some(&target);
            let added = std::mem::take(&mut self.session.engines.added);
            let out = self.switch_engine(target);
            if reconnect && out.is_ok() {
                for t in added {
                    if let Err(e) = self.add_engine(t) {
                        return Some(Err(e));
                    }
                }
            }
            return Some(out);
        }
        Some(match self.session.engines.take_change()? {
            EngineChange::Add(t) => self.add_engine(t),
            EngineChange::Remove(id) => self.remove_engine(&id),
            EngineChange::Reload => self.reload_engines(),
            EngineChange::Clear => Ok(self.clear_engines()),
            EngineChange::Install(url) => self.start_install(url),
        })
    }

    /// Stop every engine (`engine.clear`). The graph stays, on the catalogue it had, with none
    /// connected; the Engine menu starts the next.
    pub fn clear_engines(&mut self) -> Value {
        self.coordinator = None;
        self.session.engines.added.clear();
        self.forget_runs(Instant::now());
        self.session.engines.current = None;
        self.last_error = None;
        self.canvas.invalidate();
        json!({"engines": [], "label": self.engine_label()})
    }

    /// Start `target` beside the engines there are (see the module docs).
    pub fn add_engine(&mut self, target: EngineTarget) -> Result<Value, String> {
        let EngineTarget::Choice { engine, mode } = &target else { return Err("only an engine the editor starts can be added".into()) };
        let Some(first) = self.link.as_ref().map(|l| Arc::clone(&l.conn)) else {
            return Err("connect an engine first (Engine menu)".into());
        };
        let wanted = self.target_label(&target);
        let ctx = self.repaint.clone();
        let linked = match &mut self.connector {
            Some(connect) => connect(engine, mode, ctx.as_ref()),
            None => Err("this editor can't start engines".into()),
        };
        let conn = match linked {
            // The coordinator holds the connection; this link's own pump stops.
            Ok(link) => Arc::clone(&link.conn),
            Err(e) => {
                let e = format!("engine: {wanted}: {e}");
                self.last_error = Some(e.clone());
                return Err(e);
            }
        };
        let coordinator = match &self.coordinator {
            Some(c) => Arc::clone(c),
            None => {
                let first_id = match &self.session.engines.current {
                    Some(EngineTarget::Choice { engine, .. }) => engine.clone(),
                    _ => self.session.catalog().engine.id.clone(),
                };
                Arc::new(Coordinator::new().with_engine(first_id, first))
            }
        };
        coordinator.add(engine.clone(), conn);
        self.session.engines.added.push(target.clone());
        self.relink(coordinator, &wanted).map(|mut out| {
            out["added"] = target.to_json();
            out
        })
    }

    /// Stop an added engine.
    pub fn remove_engine(&mut self, id: &str) -> Result<Value, String> {
        let Some(coordinator) = self.coordinator.clone() else { return Err("no engine was added".into()) };
        coordinator.remove(id);
        self.session.engines.added.retain(|t| !matches!(t, EngineTarget::Choice { engine, .. } if engine == id));
        self.relink(coordinator, id).map(|mut out| {
            out["removed"] = json!(id);
            out
        })
    }

    /// Link to `coordinator` in place of the link there is, and take its merged catalogue.
    fn relink(&mut self, coordinator: Arc<Coordinator>, what: &str) -> Result<Value, String> {
        self.forget_runs(Instant::now());
        let n = coordinator.engines().len();
        let conn: Arc<dyn Connection> = coordinator.clone();
        self.coordinator = Some(coordinator);
        let link = Link::start(Arc::clone(&conn), format!("{n} engines"), self.repaint.clone())?;
        let catalog = match conn.call("schema", json!({})).map(serde_json::from_value::<SchemaReply>) {
            Ok(Ok(c)) => Ok(c),
            Ok(Err(e)) => Err(format!("the engines' catalogue: {e}")),
            Err(e) => Err(format!("schema: {}", e.message)),
        };
        self.engine = link.state();
        self.sync_engine();
        self.session.docs.resolver = Some(Arc::new(move |r: &plumcraft_proto::data::DataRef| conn.resolve(r).map(|b| b.bytes.to_vec())));
        self.link = Some(link);
        let catalog = catalog.inspect_err(|e| self.last_error = Some(format!("engine: {what}: {e}")))?;
        self.set_catalog(catalog);
        let problems: Vec<String> = self.session.problems().into_iter().collect();
        let warnings = self.coordinator.as_ref().map(|c| c.warnings()).unwrap_or_default();
        self.last_error = problems
            .first()
            .map(|first| format!("{what}: the graph has {} problem(s) with these engines: {first}", problems.len()))
            .or_else(|| warnings.first().map(|w| format!("{what}: the engines disagree: {w}")));
        Ok(
            json!({"engines": self.session.engines.engine_ids(), "connected": self.session.connected, "label": self.engine_label(), "problems": problems, "warnings": warnings}),
        )
    }

    /// Cancel the run in flight (telling the engine) and forget the results; what is watched
    /// stays.
    fn forget_runs(&mut self, now: Instant) {
        if self.session.run.running {
            self.session.run.cancel();
        }
        for out in self.session.run.take_outbox() {
            if let (plumcraft_session::run::Outgoing::Cancel(run_id), Some(link)) = (out, &self.link) {
                let _ = link.conn.call("run.cancel", json!({"run_id": run_id}));
            }
        }
        // Dropping the link joins its pump; an engine with no other holder stops.
        self.link = None;
        self.engine = EngineState::Off;
        self.session.connected = false;
        let watch = std::mem::take(&mut self.session.run.watch);
        let roles = std::mem::take(&mut self.session.run.roles);
        self.session.run.reset(now);
        self.session.run.last_wall = None;
        self.session.run.watched = vec![None; watch.len()];
        self.session.run.watch = watch;
        self.session.run.roles = roles;
        self.watched.clear();
        self.thumbs = Thumbs::default();
        self.rerun_pending = false;
    }

    /// Connect to `target` in place of the engines there are (see the module docs).
    pub fn switch_engine(&mut self, target: EngineTarget) -> Result<Value, String> {
        let now = Instant::now();
        self.coordinator = None;
        self.session.engines.added.clear();
        // The run in flight is cancelled, the old engine told, and its results forgotten.
        self.forget_runs(now);

        let ctx = self.repaint.clone();
        let linked = match &target {
            EngineTarget::Attach { url, token } => attach(url, token, ctx.as_ref()),
            EngineTarget::Choice { engine, mode } => match &mut self.connector {
                Some(connect) => connect(engine, mode, ctx.as_ref()),
                None => Err("this editor can't start engines".into()),
            },
        };
        let wanted = self.target_label(&target);
        self.session.engines.current = Some(target.clone());
        let link = match linked {
            Ok(link) => link,
            Err(e) => {
                let e = format!("engine: {wanted}: {e}");
                self.last_error = Some(e.clone());
                self.canvas.invalidate();
                return Err(e);
            }
        };
        let catalog = match link.conn.call("schema", json!({})).map(serde_json::from_value::<SchemaReply>) {
            Ok(Ok(c)) => Ok(c),
            Ok(Err(e)) => Err(format!("the engine's catalogue: {e}")),
            Err(e) => Err(format!("schema: {}", e.message)),
        };
        self.engine = link.state();
        self.sync_engine();
        self.link = Some(link);
        let catalog = match catalog {
            Ok(c) => c,
            Err(e) => {
                self.last_error = Some(format!("engine: {wanted}: {e}"));
                return Err(e);
            }
        };
        self.set_catalog(catalog);
        let problems: Vec<String> = self.session.problems().into_iter().collect();
        self.last_error = problems.first().map(|first| format!("{wanted}: the graph has {} problem(s) with this engine: {first}", problems.len()));
        let mut out = target.to_json();
        out["connected"] = json!(self.session.connected);
        out["label"] = json!(self.engine_label());
        out["problems"] = json!(problems);
        Ok(out)
    }

    /// What `target` is called in the status bar and the title.
    fn target_label(&self, target: &EngineTarget) -> String {
        match target {
            EngineTarget::Attach { url, .. } => format!("HTTP · attached · {url}"),
            EngineTarget::Choice { engine, mode } => match self.session.engines.find(engine, mode) {
                Some(c) => format!("{} · {}", c.engine_label, c.mode_label),
                None => format!("{engine} · {mode}"),
            },
        }
    }

    /// The engine the editor has (or tried to have), for the window title.
    pub fn engine_title(&self) -> Option<String> {
        let current = self.session.engines.current.as_ref()?;
        let mut label = self.target_label(current);
        for t in &self.session.engines.added {
            label = format!("{label} + {}", self.target_label(t));
        }
        Some(if self.link.is_some() { label } else { format!("{label}, not connected") })
    }

    /// The Engine menu's choices, after its fixed commands: Remove for each added engine, then
    /// every engine in a list that scrolls, each a submenu of its connections with the one in
    /// use checked. A pick is `engine.use`: it starts, adds or changes that engine, so Clear
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
        let e = &self.session.engines;
        let mut v = json!({
            "label": self.engine_label(),
            "state": self.engine,
            "connected": self.session.connected,
            "engine": null,
            "mode": null,
            "choices": e.choices,
            "pending": e.is_pending(),
            "installing": e.installing,
        });
        if let Some(t) = &e.current {
            let tj = t.to_json();
            v["engine"] = tj["engine"].clone();
            v["mode"] = tj["mode"].clone();
            if let Some(url) = tj.get("url") {
                v["url"] = url.clone();
            }
            v["hosting"] = json!(self.target_label(t));
        }
        v["warnings"] = json!(self.coordinator.as_ref().map(|c| c.warnings()).unwrap_or_default());
        // Every engine and its state: the coordinator's, or the one engine's.
        v["engines"] = match &self.coordinator {
            Some(c) => c.states().into_iter().map(|(id, state)| json!({"id": id, "state": state})).collect(),
            None => e.engine_ids().into_iter().map(|id| json!({"id": id, "state": self.engine})).collect(),
        };
        v
    }
}

/// Attach to a running HTTP engine, as a remote UI would (values by URL, sources by upload).
fn attach(url: &str, token: &str, ctx: Option<&egui::Context>) -> Result<Link, String> {
    let host = HttpHost::attach(url, token, HttpOptions { remote: true, ..Default::default() })?;
    let conn: Arc<dyn Connection> = Arc::new(host);
    Link::start(conn, format!("HTTP · attached · {url}"), ctx.cloned())
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

/// A menu list that scrolls once it would run off the bottom of the window.
fn scrolling(ui: &mut egui::Ui, salt: &str, add: impl FnOnce(&mut egui::Ui)) {
    let room = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 24.0).max(120.0);
    // a menu offers less than the window, so the list asks for what is left below it
    egui::ScrollArea::vertical().id_salt(salt).min_scrolled_height(room).max_height(room).auto_shrink([true, true]).show(ui, add);
}
