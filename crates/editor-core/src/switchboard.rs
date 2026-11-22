//! Switching engines while the editor runs: the state machine behind the Engine menu.
//!
//! The split: the session's `engine.*` commands ([`plumcraft_session::engine_cmds`]) only record
//! the request, as run commands queue theirs; the shell performs it ([`Switchboard::pump`]), right
//! after the command runs and at the start of every frame. Only the application knows how to start
//! an engine, so the shell hands the pump a connect function (`engine` and `mode` ids to a
//! [`Link`]: [`crate::engines::Launcher::connect_ids`], or the application's own); attaching to a
//! running HTTP engine needs nothing from the application ([`crate::engines::attach`]).
//!
//! A switch cancels the run in flight (the cancel goes to the old engine), drops the old link
//! (its pump is joined, so a spawned engine is stopped: `ProcessHost`'s shutdown, `HttpHost`'s
//! lifeline), forgets the old engine's run state (node states, the last reply, the watched value;
//! what is watched stays), connects the new one and takes its catalogue. The graph is kept: nodes
//! or wires the new catalogue doesn't have are reported, as `graph.open` reports a file's
//! problems, and nothing is deleted. A failed connect says why and leaves the editor usable on the
//! old catalogue, with no engine; the Engine menu still works.
//!
//! **Several engines** (`docs/research/15-multi-engine.md`): picking an engine that isn't
//! connected (`engine.use`) starts it beside the ones there are. The first add puts the engines
//! behind a [`Coordinator`], and the editor links to that instead: one catalogue (merged), one run
//! state, a graph whose nodes run on whichever engine offers them. Remove stops an added engine.
//! Each add or remove relinks, so the run in flight is cancelled and the results forgotten, as on
//! a switch. Switch and Attach go back to one engine; Reconnect restarts the first and adds the
//! others again; Clear Engines stops them all.
//!
//! Moved out of `plumcraft-ui`'s `engine_view` (stage N1, slice 2). The parts it works on are
//! still the shell's fields (the egui editor's are public, and its tests read them), so a
//! [`Switchboard`] borrows them for one operation; what the shell must redo afterwards (forget its
//! caches of the old run, rebuild its canvas) comes back in [`Effects`].
//!
//! **Off the shell's thread** (slice 3; `docs/shell-interface.md` 3.1 rule 6). An operation comes
//! in three parts: [`Switchboard::begin`] does on the shell's thread what waits for nothing
//! (forget the old run, retire the old engine, say what is coming); the part that waits for
//! engines, [`Work`] (stop the old engine, start the new, read its catalogue), goes to the
//! document's engine worker ([`crate::worker::EngineWorker::switch`]); [`Switchboard::finish`]
//! takes what came back on the shell's thread and gives the outcome, or the next part (a
//! reconnect adds its engines again one by one). Meanwhile the session's engine request is taken
//! and [`Switching::busy`] is true: the shell shows that as connecting. [`Switchboard::pump`] does
//! all three in place, for callers that may wait.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use plumcraft_coordinator::Coordinator;
use plumcraft_host::Connection;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::data::DataRef;
use plumcraft_proto::session::EngineState;
use plumcraft_session::{EngineChange, EngineChoice, EngineTarget, Session};
use serde_json::{Value, json};

use crate::engines::{EngineKind, Manifests, Mode};
use crate::install::{Installer, Installing};
use crate::link::{self, Link, Waker};

/// Reads the engine manifests again: the choices there are now, and what couldn't be read.
pub type Reloader = Box<dyn FnMut() -> (Vec<EngineChoice>, Vec<String>)>;

/// Start engine `engine` hosted as `mode` (one of the [`EngineChoice`]s) and link to it.
pub type Connect<'a> = dyn FnMut(&str, &str) -> Result<Link, String> + 'a;

/// What the shell must do after an operation, for what it keeps beside the core's state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Effects {
    /// The run's results were forgotten: drop what was read of them (the watched value, the
    /// thumbnails) and any rerun waiting.
    pub runs_forgotten: bool,
    /// A catalogue was set; `Some(true)` when there was none before, so the nodes only get their
    /// real sizes now (the egui shell fits the view then).
    pub catalog_set: Option<bool>,
    /// What the canvas shows of the engines changed: rebuild it.
    pub redraw: bool,
}

/// An old engine on its way out: the runs to cancel on it, then its link and coordinator to drop.
/// [`Retired::stop`] does both; the shell has its engine worker do it, off the shell's thread
/// (`run.cancel` is a call, dropping a link joins its pump, and dropping a spawned engine's last
/// holder stops the process).
#[derive(Default)]
pub struct Retired {
    link: Option<Link>,
    cancels: Vec<String>,
    coordinator: Option<Arc<Coordinator>>,
}

impl Retired {
    pub fn is_empty(&self) -> bool {
        self.link.is_none() && self.coordinator.is_none()
    }

    /// Cancel its runs, then let it go.
    pub fn stop(self) {
        if let Some(l) = &self.link {
            for run_id in &self.cancels {
                let _ = l.conn.call("run.cancel", json!({"run_id": run_id}));
            }
        }
        drop(self);
    }
}

/// What an operation in flight is waiting for (the shell keeps it between [`Switchboard::begin`]
/// and [`Switchboard::finish`]).
enum Op {
    /// A switch or attach to `target`.
    Switch { target: EngineTarget, wanted: String },
    /// An added engine starting, before the coordinator takes it.
    AddEngine { target: EngineTarget, wanted: String, first: Arc<dyn Connection>, first_id: String, coordinator: Option<Arc<Coordinator>> },
    /// The coordinator relinked after an add or a remove.
    Relink { what: String, then: Then },
}

enum Then {
    Added(EngineTarget),
    Removed(String),
}

/// A document's engine switching between calls: what is in flight, a reconnect's engines still to
/// add, and old engines to stop. The shell keeps one per document and lends it with the rest.
#[derive(Default)]
pub struct Switching {
    in_flight: Option<Op>,
    /// A reconnect's: the engines to add again, and the switch's outcome (what it returns).
    reconnect: Option<(VecDeque<EngineTarget>, Value)>,
    retired: Vec<Retired>,
}

impl Switching {
    /// An operation's waiting part is with the worker.
    pub fn busy(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Old engines to stop ([`crate::worker::EngineWorker::retire`]).
    pub fn take_retired(&mut self) -> Vec<Retired> {
        std::mem::take(&mut self.retired)
    }
}

/// An operation's next step: finished, or waiting on an engine.
pub enum Next {
    Done(Result<Value, String>),
    /// Do this ([`Work::run`]) on the engine worker, then hand the result to
    /// [`Switchboard::finish`].
    Work(Work),
}

/// The part of an operation that waits for engines: stopping the old one, starting the new,
/// reading its catalogue. Sent to the engine worker.
pub struct Work {
    task: Task,
    retired: Vec<Retired>,
    /// The manifests of the thread that owns the document, for starting their engines.
    manifests: Manifests,
    waker: Option<Waker>,
}

enum Task {
    Connect(EngineTarget),
    Engine { engine: String, mode: String },
    Relink { coordinator: Arc<Coordinator>, add: Option<(String, Arc<dyn Connection>)>, remove: Option<String> },
}

/// What [`Work`] came back with.
pub struct Worked(Got);

/// A relink's new link, the engines' catalogue, and where they disagree.
type Relinked = Result<(Link, Result<SchemaReply, String>, Vec<String>), String>;

enum Got {
    Linked(Result<(Link, Result<SchemaReply, String>), String>),
    Engine(Result<Arc<dyn Connection>, String>),
    Relinked { coordinator: Arc<Coordinator>, linked: Relinked },
}

fn schema(conn: &dyn Connection) -> Result<SchemaReply, String> {
    match conn.call("schema", json!({})).map(serde_json::from_value::<SchemaReply>) {
        Ok(Ok(c)) => Ok(c),
        Ok(Err(e)) => Err(format!("the engine's catalogue: {e}")),
        Err(e) => Err(format!("schema: {}", e.message)),
    }
}

impl Work {
    /// Is it only starting an engine compiled into this program (no process, no network)? Such
    /// work waits for nothing but this program's own code.
    pub fn in_process(&self) -> bool {
        let compiled =
            |engine: &str, mode: &str| Mode::from_id(mode) == Some(Mode::InProc) && EngineKind::from_id(engine).is_some_and(|k| !k.spawns(Mode::InProc));
        match &self.task {
            Task::Connect(EngineTarget::Choice { engine, mode }) | Task::Engine { engine, mode } => compiled(engine, mode),
            _ => false,
        }
    }

    /// The old engines it stops first, to stop elsewhere instead (in-process work done in place).
    pub fn take_retired(&mut self) -> Vec<Retired> {
        std::mem::take(&mut self.retired)
    }

    /// Stop the old engines, then start and link the new (`connect` starts an engine the
    /// application knows; attaching needs nothing from it).
    pub fn run(self, connect: &mut Connect<'_>) -> Worked {
        self.manifests.install();
        self.retired.into_iter().for_each(Retired::stop);
        let linked = |l: Result<Link, String>| l.map(|link| (schema(&*link.conn), link)).map(|(c, l)| (l, c));
        Worked(match self.task {
            Task::Connect(EngineTarget::Attach { url, token }) => Got::Linked(linked(crate::engines::attach(&url, &token, self.waker))),
            Task::Connect(EngineTarget::Choice { engine, mode }) => Got::Linked(linked(connect(&engine, &mode))),
            // the coordinator holds the connection; this link's own pump stops (here)
            Task::Engine { engine, mode } => Got::Engine(connect(&engine, &mode).map(|link| Arc::clone(&link.conn))),
            Task::Relink { coordinator, add, remove } => {
                if let Some((id, conn)) = add {
                    coordinator.add(id, conn);
                }
                // what is removed stops here
                drop(remove.and_then(|id| coordinator.remove(&id)));
                let n = coordinator.engines().len();
                let conn: Arc<dyn Connection> = coordinator.clone();
                let linked = Link::start(Arc::clone(&conn), format!("{n} engines"), self.waker).map(|link| {
                    let catalog = schema(&*conn).map_err(|e| e.replace("the engine's catalogue", "the engines' catalogue"));
                    // read now, while the catalogue is fresh (the shell reads it again cached)
                    let warnings = coordinator.warnings();
                    (link, catalog, warnings)
                });
                Got::Relinked { coordinator, linked }
            }
        })
    }
}

/// The parts of a document an engine switch works on, borrowed from the shell for one operation.
pub struct Switchboard<'a> {
    pub session: &'a mut Session,
    /// The engine connection, when one was started.
    pub link: &'a mut Option<Link>,
    /// With engines added beside the first: the coordinator the link goes through.
    pub coordinator: &'a mut Option<Arc<Coordinator>>,
    /// The engine's state, as last read.
    pub state: &'a mut EngineState,
    /// The last error, which the status bar shows.
    pub last_error: &'a mut Option<String>,
    /// Reads the engine manifests again (`engine.manifests.reload`).
    pub reloader: &'a mut Option<Reloader>,
    /// Installs engines (`engine.install`).
    pub installer: &'a Option<Installer>,
    /// An install running in the background.
    pub installing: &'a mut Option<Installing>,
    /// What is in flight between calls.
    pub switching: &'a mut Switching,
    /// What a new link wakes when its events arrive.
    pub waker: Option<Waker>,
    /// What the shell must redo afterwards.
    pub effects: Effects,
}

impl Switchboard<'_> {
    /// Perform a switch the session recorded, here and now, waiting for the engines on this
    /// thread: `None` when none was asked for; else the outcome. The synchronous form of
    /// [`Switchboard::begin`], [`Work::run`] and [`Switchboard::finish`], for callers with no
    /// window to keep drawing (tests, scripts); a shell's own thread uses those three.
    pub fn pump(&mut self, connect: &mut Connect<'_>) -> Option<Result<Value, String>> {
        let mut next = self.begin()?;
        loop {
            match next {
                Next::Done(out) => {
                    self.switching.take_retired().into_iter().for_each(Retired::stop);
                    return Some(out);
                }
                Next::Work(w) => next = self.finish(w.run(connect)),
            }
        }
    }

    /// Begin the next operation the session recorded (`engine.switch`, `engine.reconnect`,
    /// `engine.attach`, `engine.use`, `engine.add`, `engine.remove`, `engine.clear`,
    /// `engine.manifests.reload`, `engine.install`): `None` when none was asked for, or one is
    /// in flight. What doesn't wait for an engine is done here; the rest is [`Next::Work`].
    pub fn begin(&mut self) -> Option<Next> {
        if self.switching.busy() {
            return None;
        }
        if let Some(target) = self.session.engines.take_request() {
            // Reconnect restarts the first engine and adds the others again; a switch drops them.
            let reconnect = self.session.engines.current.as_ref() == Some(&target);
            let added = std::mem::take(&mut self.session.engines.added);
            self.switching.reconnect = (reconnect && !added.is_empty()).then(|| (added.into(), Value::Null));
            return Some(self.begin_switch(target));
        }
        Some(match self.session.engines.take_change()? {
            EngineChange::Add(t) => self.begin_add(t),
            EngineChange::Remove(id) => self.begin_remove(&id),
            EngineChange::Reload => Next::Done(self.reload()),
            EngineChange::Clear => Next::Done(Ok(self.clear())),
            EngineChange::Install(url) => Next::Done(self.start_install(url)),
            EngineChange::CancelInstall => Next::Done(self.cancel_install()),
        })
    }

    /// Take what [`Work`] came back with: the operation's outcome, or its next step.
    pub fn finish(&mut self, worked: Worked) -> Next {
        let Some(op) = self.switching.in_flight.take() else { return Next::Done(Err("no engine switch was in flight".into())) };
        match (op, worked.0) {
            (Op::Switch { target, wanted }, Got::Linked(linked)) => {
                let out = self.linked(&target, &wanted, linked);
                match (out, self.switching.reconnect.take()) {
                    (Ok(out), Some((rest, _))) => self.next_add(rest, out),
                    (out, _) => Next::Done(out),
                }
            }
            (Op::AddEngine { target, wanted, first, first_id, coordinator }, Got::Engine(conn)) => match conn {
                Err(e) => {
                    let e = format!("engine: {wanted}: {e}");
                    *self.last_error = Some(e.clone());
                    self.chain(Err(e))
                }
                Ok(conn) => {
                    let EngineTarget::Choice { engine, .. } = &target else { return self.chain(Err("only an engine the editor starts can be added".into())) };
                    let engine = engine.clone();
                    let coordinator = coordinator.unwrap_or_else(|| Arc::new(Coordinator::new().with_engine(first_id, first)));
                    self.session.engines.added.push(target.clone());
                    self.begin_relink(coordinator, Some((engine, conn)), None, wanted, Then::Added(target))
                }
            },
            (Op::Relink { what, then }, Got::Relinked { coordinator, linked }) => {
                let out = self.relinked(coordinator, &what, linked).map(|mut out| {
                    match then {
                        Then::Added(t) => out["added"] = t.to_json(),
                        Then::Removed(id) => out["removed"] = json!(id),
                    }
                    out
                });
                self.chain(out)
            }
            _ => Next::Done(Err("an engine switch came back mismatched".into())),
        }
    }

    /// A reconnect's next engine to add again, or its end.
    fn next_add(&mut self, mut rest: VecDeque<EngineTarget>, out: Value) -> Next {
        match rest.pop_front() {
            None => Next::Done(Ok(out)),
            Some(t) => {
                self.switching.reconnect = Some((rest, out));
                match self.begin_add(t) {
                    Next::Done(r) => self.chain(r),
                    work => work,
                }
            }
        }
    }

    /// An add or remove done: within a reconnect, go on (or stop at an error, returning it);
    /// else that is the outcome.
    fn chain(&mut self, out: Result<Value, String>) -> Next {
        match (self.switching.reconnect.take(), out) {
            (Some((rest, first)), Ok(_)) => self.next_add(rest, first),
            (_, out) => Next::Done(out),
        }
    }

    /// Start installing `url` on a thread of its own; [`Switchboard::poll_install`] takes the end.
    pub fn start_install(&mut self, url: String) -> Result<Value, String> {
        let Some(installer) = self.installer.clone() else { return Err("this editor can't install engines".into()) };
        if let Some(i) = &self.installing {
            return Err(format!("installing {}", i.url));
        }
        let i = Installing::start(installer, url.clone(), self.waker.clone())?;
        let short = url.trim_start_matches("https://github.com/");
        let said = format!("Installing {short}: uv downloads and builds it, which can take a few minutes (Engine › Cancel Install stops it)");
        crate::log::info("install", &said);
        self.session.engines.installing = Some(url.clone());
        self.session.engines.install_status = Some(said.clone());
        *self.installing = Some(i);
        // the status line (and a native shell's banner) says it has begun
        *self.last_error = Some(said);
        Ok(json!({"installing": url}))
    }

    /// `engine.install.cancel`: ask the install under way to stop; [`Switchboard::poll_install`]
    /// takes its end.
    pub fn cancel_install(&mut self) -> Result<Value, String> {
        let Some(i) = &self.installing else { return Err("no engine is being installed".into()) };
        i.progress.cancel();
        crate::log::info("install", &format!("cancelling the install of {}", i.url));
        self.session.engines.install_status = Some(format!("Cancelling the install of {}…", i.url.trim_start_matches("https://github.com/")));
        Ok(json!({"cancelling": i.url}))
    }

    /// Follow the install under way: what it says it is doing goes to `engine.installing`'s
    /// status (and the log); once it has ended, read the engine manifests again, so the Engine
    /// menu offers what was installed, and say how it went (the status line, the log). Returns
    /// whether one finished. Every turn of every shell ([`crate::control::step`]).
    pub fn poll_install(&mut self) -> bool {
        let Some(i) = &self.installing else { return false };
        let Some(outcome) = i.poll() else {
            let now = i.progress.status();
            if now.is_some() && !i.progress.cancelled() && now != self.session.engines.install_status {
                if let Some(s) = &now {
                    crate::log::write(crate::log::Level::Debug, "install", s, None);
                }
                self.session.engines.install_status = now;
                self.effects.redraw = true;
            }
            return false;
        };
        let url = i.url.clone();
        *self.installing = None;
        self.session.engines.installing = None;
        self.session.engines.install_status = None;
        let (level, said) = match outcome {
            Ok(said) => match self.reload() {
                Ok(_) => (crate::log::Level::Info, format!("{said}. The Engine menu offers it now.")),
                Err(e) => (crate::log::Level::Warn, format!("{said}, but the engine list didn't reload: {e}")),
            },
            Err(e) if e == crate::install::CANCELLED => (crate::log::Level::Info, format!("The install of {url} was cancelled")),
            Err(e) => (crate::log::Level::Error, format!("Installing {url} failed: {e}")),
        };
        crate::log::write(level, "install", &said, None);
        *self.last_error = Some(said);
        self.effects.redraw = true;
        true
    }

    /// `engine.manifests.reload`: the new choices; what couldn't be read goes to the status bar.
    pub fn reload(&mut self) -> Result<Value, String> {
        let Some(reload) = self.reloader.as_mut() else { return Err("this editor has no engine manifests to read".into()) };
        let (choices, problems) = reload();
        let engines = choices.iter().map(|c| c.engine.as_str()).collect::<std::collections::BTreeSet<_>>().len();
        self.session.engines.choices = choices;
        *self.last_error = problems.first().map(|p| format!("engine manifests: {p}"));
        Ok(json!({"engines": engines, "problems": problems}))
    }

    /// Stop every engine (`engine.clear`). The graph stays, on the catalogue it had, with none
    /// connected; the Engine menu starts the next. (The engines stop on the worker:
    /// [`Switching::take_retired`].)
    pub fn clear(&mut self) -> Value {
        self.retire_coordinator();
        self.session.engines.added.clear();
        self.forget_runs(Instant::now());
        self.session.engines.current = None;
        *self.last_error = None;
        self.effects.redraw = true;
        json!({"engines": [], "label": self.label()})
    }

    /// The coordinator, when there is one, to stop with the old engines.
    pub fn retire_coordinator(&mut self) {
        if let Some(c) = self.coordinator.take() {
            self.switching.retired.push(Retired { coordinator: Some(c), ..Default::default() });
        }
    }

    fn work(&mut self, task: Task, op: Op) -> Next {
        self.switching.in_flight = Some(op);
        let retired = self.switching.take_retired();
        Next::Work(Work { task, retired, manifests: Manifests::here(), waker: self.waker.clone() })
    }

    /// Start `target` beside the engines there are (see the module docs).
    fn begin_add(&mut self, target: EngineTarget) -> Next {
        let EngineTarget::Choice { engine, mode } = &target else { return Next::Done(Err("only an engine the editor starts can be added".into())) };
        let Some(first) = self.link.as_ref().map(|l| Arc::clone(&l.conn)) else {
            return Next::Done(Err("connect an engine first (Engine menu)".into()));
        };
        let wanted = target_label(self.session, &target);
        let first_id = match &self.session.engines.current {
            Some(EngineTarget::Choice { engine, .. }) => engine.clone(),
            _ => self.session.catalog().engine.id.clone(),
        };
        let task = Task::Engine { engine: engine.clone(), mode: mode.clone() };
        let coordinator = self.coordinator.clone();
        self.work(task, Op::AddEngine { target, wanted, first, first_id, coordinator })
    }

    /// Stop an added engine.
    fn begin_remove(&mut self, id: &str) -> Next {
        let Some(coordinator) = self.coordinator.clone() else { return Next::Done(Err("no engine was added".into())) };
        self.session.engines.added.retain(|t| !matches!(t, EngineTarget::Choice { engine, .. } if engine == id));
        self.begin_relink(coordinator, None, Some(id.to_owned()), id.to_owned(), Then::Removed(id.to_owned()))
    }

    /// Link to `coordinator` (once `add` is added and `remove` removed) in place of the link
    /// there is, and take its merged catalogue. Until it is done there is no coordinator: what
    /// reads one (`ui.inspect`'s warnings) would otherwise ask its engines for their catalogues.
    fn begin_relink(
        &mut self,
        coordinator: Arc<Coordinator>,
        add: Option<(String, Arc<dyn Connection>)>,
        remove: Option<String>,
        what: String,
        then: Then,
    ) -> Next {
        self.forget_runs(Instant::now());
        *self.coordinator = None;
        self.work(Task::Relink { coordinator, add, remove }, Op::Relink { what, then })
    }

    fn relinked(&mut self, coordinator: Arc<Coordinator>, what: &str, linked: Relinked) -> Result<Value, String> {
        *self.coordinator = Some(Arc::clone(&coordinator));
        let (link, catalog, warnings) = linked?;
        *self.state = link.state();
        sync_state(self.session, self.state);
        let conn = Arc::clone(&link.conn);
        self.session.docs.resolver = Some(Arc::new(move |r: &DataRef| conn.resolve(r).map(|b| b.bytes.to_vec())));
        *self.link = Some(link);
        share_connection(self.session, self.link);
        let catalog = catalog.inspect_err(|e| *self.last_error = Some(format!("engine: {what}: {e}")))?;
        self.set_catalog(catalog);
        let problems: Vec<String> = self.session.problems().into_iter().collect();
        *self.last_error = problems
            .first()
            .map(|first| format!("{what}: the graph has {} problem(s) with these engines: {first}", problems.len()))
            .or_else(|| warnings.first().map(|w| format!("{what}: the engines disagree: {w}")));
        Ok(
            json!({"engines": self.session.engines.engine_ids(), "connected": self.session.connected, "label": self.label(), "problems": problems, "warnings": warnings}),
        )
    }

    /// Cancel the run in flight and forget the results; what is watched stays. The old engine
    /// is told and dropped later, off this thread ([`Switching::take_retired`]).
    pub fn forget_runs(&mut self, now: Instant) {
        if self.session.run.running {
            self.session.run.cancel();
        }
        let cancels: Vec<String> = self
            .session
            .run
            .take_outbox()
            .into_iter()
            .filter_map(|out| match out {
                plumcraft_session::run::Outgoing::Cancel(run_id) => Some(run_id),
                _ => None,
            })
            .collect();
        // The session's clones go first, so the retired link holds the last (an engine with no
        // other holder stops when it is dropped).
        self.session.set_connection(None);
        self.session.docs.resolver = None;
        if let Some(link) = self.link.take() {
            self.switching.retired.push(Retired { link: Some(link), cancels, ..Default::default() });
        }
        *self.state = EngineState::Off;
        self.session.connected = false;
        let watch = std::mem::take(&mut self.session.run.watch);
        let roles = std::mem::take(&mut self.session.run.roles);
        self.session.run.reset(now);
        self.session.run.last_wall = None;
        self.session.run.watched = vec![None; watch.len()];
        self.session.run.watch = watch;
        self.session.run.roles = roles;
        self.effects.runs_forgotten = true;
    }

    /// Connect to `target` in place of the engines there are (see the module docs): the old
    /// engines are retired, and the connect is [`Next::Work`].
    fn begin_switch(&mut self, target: EngineTarget) -> Next {
        self.retire_coordinator();
        self.session.engines.added.clear();
        // The run in flight is cancelled, the old engine told, and its results forgotten.
        self.forget_runs(Instant::now());
        let wanted = target_label(self.session, &target);
        // (said now: while it connects, the title and status bar name what is coming)
        self.session.engines.current = Some(target.clone());
        self.work(Task::Connect(target.clone()), Op::Switch { target, wanted })
    }

    fn linked(&mut self, target: &EngineTarget, wanted: &str, linked: Result<(Link, Result<SchemaReply, String>), String>) -> Result<Value, String> {
        let (link, catalog) = match linked {
            Ok(l) => l,
            Err(e) => {
                let e = format!("engine: {wanted}: {e}");
                *self.last_error = Some(e.clone());
                self.effects.redraw = true;
                return Err(e);
            }
        };
        *self.state = link.state();
        sync_state(self.session, self.state);
        *self.link = Some(link);
        share_connection(self.session, self.link);
        let catalog = match catalog {
            Ok(c) => c,
            Err(e) => {
                *self.last_error = Some(format!("engine: {wanted}: {e}"));
                return Err(e);
            }
        };
        self.set_catalog(catalog);
        let problems: Vec<String> = self.session.problems().into_iter().collect();
        *self.last_error = problems.first().map(|first| format!("{wanted}: the graph has {} problem(s) with this engine: {first}", problems.len()));
        let mut out = target.to_json();
        out["connected"] = json!(self.session.connected);
        out["label"] = json!(self.label());
        out["problems"] = json!(problems);
        Ok(out)
    }

    /// Connect to `target` in place of the engines there are, waiting here.
    pub fn switch(&mut self, target: EngineTarget, connect: &mut Connect<'_>) -> Result<Value, String> {
        self.switching.reconnect = None;
        self.run_here(|b| b.begin_switch(target), connect)
    }

    /// Start `target` beside the engines there are, waiting here.
    pub fn add(&mut self, target: EngineTarget, connect: &mut Connect<'_>) -> Result<Value, String> {
        self.run_here(|b| b.begin_add(target), connect)
    }

    /// Stop an added engine, waiting here.
    pub fn remove(&mut self, id: &str) -> Result<Value, String> {
        self.run_here(|b| b.begin_remove(id), &mut |_: &str, _: &str| Err("removing starts no engine".into()))
    }

    fn run_here(&mut self, begin: impl FnOnce(&mut Self) -> Next, connect: &mut Connect<'_>) -> Result<Value, String> {
        if self.switching.busy() {
            return Err("an engine is still starting".into());
        }
        let mut next = begin(self);
        loop {
            match next {
                Next::Done(out) => {
                    self.switching.take_retired().into_iter().for_each(Retired::stop);
                    return out;
                }
                Next::Work(w) => next = self.finish(w.run(connect)),
            }
        }
    }

    /// Replace the catalogue (the graph is kept).
    pub fn set_catalog(&mut self, catalog: SchemaReply) {
        self.effects.catalog_set = Some(set_catalog(self.session, catalog));
    }

    /// The status bar's engine label.
    pub fn label(&self) -> String {
        engine_label(self.session, self.link.as_ref(), self.state)
    }
}

/// Replace the session's catalogue (the graph is kept): whether there was none before.
pub fn set_catalog(session: &mut Session, catalog: SchemaReply) -> bool {
    let first = session.catalog().nodes.is_empty();
    session.set_catalog(catalog);
    first
}

/// Hand the link's connection to the session, for the application's commands
/// ([`plumcraft_session::Session::connection`]); none without a link.
pub fn share_connection(session: &mut Session, link: &Option<Link>) {
    let conn = link.as_ref().map(|l| Arc::clone(&l.conn));
    session.set_connection(conn);
}

/// What the session knows of the engine's state.
pub fn sync_state(session: &mut Session, state: &EngineState) {
    session.connected = state.is_connected();
    session.caps = if let EngineState::Connected { caps, .. } = state { caps.clone() } else { Default::default() };
}

/// What `target` is called in the status bar and the title.
pub fn target_label(session: &Session, target: &EngineTarget) -> String {
    match target {
        EngineTarget::Attach { url, .. } => format!("HTTP · attached · {url}"),
        EngineTarget::Choice { engine, mode } => match session.engines.find(engine, mode) {
            Some(c) => format!("{} · {}", c.engine_label, c.mode_label),
            None => format!("{engine} · {mode}"),
        },
    }
}

/// The engine the editor has (or tried to have), for the window title; `linked` when it has a
/// link.
pub fn engine_title(session: &Session, linked: bool) -> Option<String> {
    engine_title_connecting(session, linked, false)
}

/// [`engine_title`], saying so while a switch is under way ([`Switching::busy`]).
pub fn engine_title_connecting(session: &Session, linked: bool, connecting: bool) -> Option<String> {
    let current = session.engines.current.as_ref()?;
    let mut label = target_label(session, current);
    for t in &session.engines.added {
        label = format!("{label} + {}", target_label(session, t));
    }
    Some(match (linked, connecting) {
        (_, true) => format!("{label}, connecting"),
        (true, false) => label,
        (false, false) => format!("{label}, not connected"),
    })
}

/// The status bar's engine label.
pub fn engine_label(session: &Session, link: Option<&Link>, state: &EngineState) -> String {
    engine_label_connecting(session, link, state, false)
}

/// [`engine_label`], saying so while a switch is under way ([`Switching::busy`]).
pub fn engine_label_connecting(session: &Session, link: Option<&Link>, state: &EngineState, connecting: bool) -> String {
    match link {
        None => match engine_title_connecting(session, false, connecting) {
            Some(wanted) => format!("engine: {wanted}"),
            None => "engine: not connected".into(),
        },
        Some(link) => {
            let name = &session.catalog().engine;
            let who = if name.name.is_empty() { String::new() } else { format!("{} {} · ", name.name, name.version) };
            format!("engine: {who}{} · {}", link.label, link::state_text(state))
        }
    }
}

/// The engine cell's main line: the engine's name and state (the whole label without a link).
pub fn engine_short_label(session: &Session, link: Option<&Link>, state: &EngineState) -> String {
    engine_short_label_connecting(session, link, state, false)
}

/// [`engine_short_label`], saying so while a switch is under way ([`Switching::busy`]).
pub fn engine_short_label_connecting(session: &Session, link: Option<&Link>, state: &EngineState, connecting: bool) -> String {
    if link.is_none() {
        return engine_label_connecting(session, link, state, connecting);
    }
    let name = &session.catalog().engine;
    let state = if matches!(state, EngineState::Connected { .. }) { "connected".to_owned() } else { link::state_text(state) };
    if name.name.is_empty() { format!("engine: {state}") } else { format!("{} {} · {state}", name.name, name.version) }
}

/// `ui.inspect`'s `engine`.
pub fn inspect(session: &Session, link: Option<&Link>, state: &EngineState, coordinator: Option<&Arc<Coordinator>>) -> Value {
    let e = &session.engines;
    let mut v = json!({
        "label": engine_label(session, link, state),
        "state": state,
        "connected": session.connected,
        "engine": null,
        "mode": null,
        "choices": e.choices,
        "pending": e.is_pending(),
        "installing": e.installing,
        "installStatus": e.install_status,
        "canInstall": e.can_install,
        "installRefused": e.install_refused,
    });
    if let Some(t) = &e.current {
        let tj = t.to_json();
        v["engine"] = tj["engine"].clone();
        v["mode"] = tj["mode"].clone();
        if let Some(url) = tj.get("url") {
            v["url"] = url.clone();
        }
        v["hosting"] = json!(target_label(session, t));
    }
    v["warnings"] = json!(coordinator.map(|c| c.warnings()).unwrap_or_default());
    // Every engine and its state: the coordinator's, or the one engine's.
    v["engines"] = match coordinator {
        Some(c) => c.states().into_iter().map(|(id, state)| json!({"id": id, "state": state})).collect(),
        None => e.engine_ids().into_iter().map(|id| json!({"id": id, "state": state})).collect(),
    };
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::{Launcher, choices};

    /// A document's engine parts, owned, for a test to lend.
    #[derive(Default)]
    struct Doc {
        session: Session,
        link: Option<Link>,
        coordinator: Option<Arc<Coordinator>>,
        state: EngineState,
        last_error: Option<String>,
        reloader: Option<Reloader>,
        installer: Option<Installer>,
        installing: Option<Installing>,
        switching: Switching,
    }

    impl Doc {
        fn board(&mut self) -> Switchboard<'_> {
            Switchboard {
                session: &mut self.session,
                link: &mut self.link,
                coordinator: &mut self.coordinator,
                state: &mut self.state,
                last_error: &mut self.last_error,
                reloader: &mut self.reloader,
                installer: &self.installer,
                installing: &mut self.installing,
                switching: &mut self.switching,
                waker: None,
                effects: Effects::default(),
            }
        }

        fn command(&mut self, id: &str, params: Value, launcher: &Launcher) -> (Result<Value, String>, Effects) {
            self.session.execute(id, &params).unwrap();
            let mut connect = |e: &str, m: &str| launcher.connect_ids(e, m, None);
            let mut b = self.board();
            let out = b.pump(&mut connect).unwrap_or(Ok(Value::Null));
            (out, b.effects)
        }
    }

    #[test]
    fn switching_adding_and_clearing_without_a_window() {
        let launcher = Launcher::default();
        let mut d = Doc::default();
        d.session.engines.choices = choices();
        d.session.engines.can_attach = true;
        let (r, fx) = d.command("engine.switch", json!({"engine": "text", "mode": "inproc"}), &launcher);
        let r = r.unwrap();
        assert_eq!((r["engine"].as_str(), r["connected"].as_bool()), (Some("text"), Some(true)), "{r}");
        assert_eq!(fx.catalog_set, Some(true), "the first catalogue");
        assert!(fx.runs_forgotten);
        assert!(d.session.connected && d.link.is_some() && d.session.catalog().node("text.upper").is_some());
        assert!(engine_label(&d.session, d.link.as_ref(), &d.state).contains("Rust · mode 1 · in-process"));

        let (r, _) = d.command("engine.add", json!({"engine": "array", "mode": "inproc"}), &launcher);
        assert_eq!(r.unwrap()["engines"], json!(["text", "array"]));
        assert!(d.coordinator.is_some() && d.session.catalog().node("array.arithmetic").is_some());
        assert_eq!(engine_title(&d.session, true).as_deref(), Some("Text · Rust · in-process (mode 1) + Array · Rust · in-process (mode 1)"));
        let v = inspect(&d.session, d.link.as_ref(), &d.state, d.coordinator.as_ref());
        assert_eq!(v["engines"].as_array().map(Vec::len), Some(2), "{v}");

        let (r, _) = d.command("engine.remove", json!({"engine": "array"}), &launcher);
        assert_eq!(r.unwrap()["engines"], json!(["text"]));
        let (r, fx) = d.command("engine.clear", json!({}), &launcher);
        assert_eq!(r.unwrap()["label"], "engine: not connected");
        assert!(fx.redraw && d.link.is_none() && !d.session.connected);

        // a failed connect says why and leaves the graph and catalogue
        let refusing = Launcher { caps: crate::engines::Capabilities::NONE, ..Default::default() };
        let (r, fx) = d.command("engine.switch", json!({"engine": "text", "mode": "process"}), &refusing);
        let e = r.unwrap_err();
        assert!(e.contains("this device can't start programs"), "{e}");
        assert!(fx.redraw && d.last_error.as_deref() == Some(e.as_str()));
        assert!(d.session.catalog().node("text.upper").is_some());
    }

    impl Doc {
        /// `id` begun here, its waiting part done by `worker` with `launcher`, finished here:
        /// the outcome, and whether the operation was under way meanwhile.
        fn through(&mut self, worker: &mut crate::worker::EngineWorker, id: &str, params: Value, launcher: &Launcher) -> (Result<Value, String>, bool) {
            self.session.execute(id, &params).unwrap();
            let mut next = self.board().begin().unwrap();
            let mut busy = false;
            loop {
                match next {
                    Next::Done(out) => {
                        worker.retire(self.switching.take_retired());
                        return (out, busy);
                    }
                    Next::Work(w) => {
                        busy |= self.switching.busy();
                        let owner = std::thread::current().id();
                        worker.switch(w, launcher.clone(), move |l: &mut Launcher, e: &str, m: &str| {
                            assert_ne!(std::thread::current().id(), owner, "started on the owner's thread");
                            l.connect_ids(e, m, None)
                        });
                        assert!(worker.wait(std::time::Duration::from_secs(60)));
                        let Some(crate::worker::Done::Switched { worked, .. }) = worker.drain().pop() else { panic!("no switch came back") };
                        next = self.board().finish(*worked);
                    }
                }
            }
        }
    }

    #[test]
    fn a_switch_waits_on_the_worker_and_a_reconnect_adds_its_engines_again() {
        let launcher = Launcher::default();
        let mut worker = crate::worker::EngineWorker::new(None);
        let mut d = Doc::default();
        d.session.engines.choices = choices();
        let (r, busy) = d.through(&mut worker, "engine.switch", json!({"engine": "text", "mode": "inproc"}), &launcher);
        assert!(busy && !d.switching.busy(), "under way, then done");
        assert_eq!(r.unwrap()["connected"], true);
        let (r, _) = d.through(&mut worker, "engine.add", json!({"engine": "array", "mode": "inproc"}), &launcher);
        assert_eq!(r.unwrap()["engines"], json!(["text", "array"]));
        // reconnect: the first again, then the array engine added again; the switch's outcome
        let (r, _) = d.through(&mut worker, "engine.reconnect", json!({}), &launcher);
        let r = r.unwrap();
        assert_eq!((r["engine"].as_str(), r["mode"].as_str()), (Some("text"), Some("inproc")), "{r}");
        assert_eq!(d.session.engines.engine_ids(), ["text", "array"]);
        assert!(d.coordinator.is_some() && d.session.catalog().node("array.arithmetic").is_some() && d.session.connected);
        // a switch's begin says what is coming before it has come
        d.session.execute("engine.switch", &json!({"engine": "array", "mode": "inproc"})).unwrap();
        let Some(Next::Work(w)) = d.board().begin() else { panic!("a switch waits for an engine") };
        assert!(w.in_process(), "an engine compiled in");
        assert!(d.link.is_none() && d.coordinator.is_none() && !d.session.connected);
        assert_eq!(engine_title(&d.session, false).as_deref(), Some("Array · Rust · in-process (mode 1), not connected"));
        let Next::Done(r) = d.board().finish(w.run(&mut |e: &str, m: &str| launcher.connect_ids(e, m, None))) else { panic!("one step") };
        assert_eq!(r.unwrap()["engine"], "array");
        assert!(d.board().begin().is_none(), "nothing more asked");
        worker.retire(d.switching.take_retired());
    }

    /// B72: switching to an engine a manifest names, whose program exits at once (here as a
    /// Python engine run from the wrong folder does), fails the switch at once in both hostings,
    /// with its exit status and the end of its stderr; as a process it waited out its 60 s.
    /// The program runs in the manifest's own folder (`cwd = "."`, spec 16.6), not this one.
    #[test]
    fn an_engine_that_exits_at_once_fails_the_switch_at_once_saying_why() {
        let root = plumcraft_scratch::dir("b72-switch");
        let project = std::fs::canonicalize(&root).unwrap().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let command = if cfg!(windows) {
            r#"["cmd", "/C", "cd 1>&2 & echo No module named opencv_engine 1>&2 & exit 1"]"#
        } else {
            r#"["/bin/sh", "-c", "pwd -P >&2; echo 'No module named opencv_engine' >&2; exit 1"]"#
        };
        std::fs::write(project.join("gone.toml"), format!("id = \"gone\"\nmode = \"process\"\ncommand = {command}\ncwd = \".\"\n")).unwrap();
        assert_eq!(crate::engines::load_manifests(&[project.join("gone.toml")], None), Vec::<String>::new());
        let launcher = Launcher::default();
        let limit = if cfg!(windows) { std::time::Duration::from_secs(8) } else { std::time::Duration::from_secs(3) };
        for mode in ["process", "http"] {
            let mut d = Doc::default();
            d.session.engines.choices = choices();
            let t = std::time::Instant::now();
            let (r, _) = d.command("engine.switch", json!({"engine": "gone", "mode": mode}), &launcher);
            let took = t.elapsed();
            let e = r.unwrap_err();
            eprintln!("{mode} failed in {took:?}: {e}");
            assert!(took < limit, "{mode}: {took:?}: {e}");
            let before = if mode == "process" { "exited before connecting" } else { "exited before listening" };
            assert!(e.contains(before) && e.contains("exit 1"), "{mode}: {e}");
            assert!(e.contains("No module named opencv_engine"), "{mode}: its stderr: {e}");
            assert!(e.contains(&project.display().to_string()), "{mode}: it ran in the manifest's folder: {e}");
            assert!(d.link.is_none() && d.last_error.as_deref() == Some(e.as_str()));
        }
    }

    /// A copy of a project's manifest (`uv run --project .` in a folder with no
    /// `pyproject.toml`) is still offered, with a warning among the manifest problems, and its
    /// failed switch ends with what to do about it, in both hostings; the same manifest in its
    /// project neither warns nor advises. The stand-in `uv` fails as Python does there.
    #[cfg(unix)]
    #[test]
    fn a_copied_manifest_warns_on_load_and_its_failed_switch_says_what_to_do() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::fs::canonicalize(plumcraft_scratch::dir("manifest-copy-switch")).unwrap();
        let (copy, project) = (root.join("copy"), root.join("project"));
        let text =
            "id = \"copied\"\nmode = \"process\"\ncommand = [\"./uv\", \"run\", \"--project\", \".\", \"python\", \"-m\", \"opencv_engine\"]\ncwd = \".\"\n";
        for d in [&copy, &project] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("uv"), "#!/bin/sh\necho 'No module named opencv_engine' >&2\nexit 1\n").unwrap();
            std::fs::set_permissions(d.join("uv"), std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::write(d.join("copied.toml"), text).unwrap();
        }
        std::fs::write(project.join("pyproject.toml"), "[project]\nname = \"opencv-engine\"\n").unwrap();
        let launcher = Launcher::default();
        for (folder, copied) in [(&copy, true), (&project, false)] {
            let problems = crate::engines::load_manifests(&[folder.join("copied.toml")], None);
            if copied {
                assert_eq!(problems.len(), 1, "{problems:?}");
                assert!(problems[0].contains("copied.toml") && problems[0].contains("no pyproject.toml"), "{}", problems[0]);
            } else {
                assert_eq!(problems, Vec::<String>::new());
            }
            assert!(crate::engines::EngineKind::from_id("copied").is_some(), "it is still offered");
            for mode in ["process", "http"] {
                let mut d = Doc::default();
                d.session.engines.choices = choices();
                let (r, _) = d.command("engine.switch", json!({"engine": "copied", "mode": mode}), &launcher);
                let e = r.unwrap_err();
                assert!(e.contains("No module named opencv_engine"), "{mode}: {e}");
                assert_eq!(e.ends_with(&format!("\n{}", plumcraft_host::manifest::COPY_ADVICE)), copied, "{mode}, copied {copied}: {e}");
            }
        }
    }
}
