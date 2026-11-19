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

use std::sync::Arc;
use std::time::Instant;

use plumcraft_coordinator::Coordinator;
use plumcraft_host::Connection;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::data::DataRef;
use plumcraft_proto::session::EngineState;
use plumcraft_session::{EngineChange, EngineChoice, EngineTarget, Session};
use serde_json::{Value, json};

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
    /// What a new link wakes when its events arrive.
    pub waker: Option<Waker>,
    /// What the shell must redo afterwards.
    pub effects: Effects,
}

impl Switchboard<'_> {
    /// Perform a switch the session recorded (`engine.switch`, `engine.reconnect`,
    /// `engine.attach`, `engine.use`, `engine.add`, `engine.remove`, `engine.clear`,
    /// `engine.manifests.reload`, `engine.install`). `None` when none was asked for; else the
    /// outcome.
    pub fn pump(&mut self, connect: &mut Connect<'_>) -> Option<Result<Value, String>> {
        if let Some(target) = self.session.engines.take_request() {
            // Reconnect restarts the first engine and adds the others again; a switch drops them.
            let reconnect = self.session.engines.current.as_ref() == Some(&target);
            let added = std::mem::take(&mut self.session.engines.added);
            let out = self.switch(target, connect);
            if reconnect && out.is_ok() {
                for t in added {
                    if let Err(e) = self.add(t, connect) {
                        return Some(Err(e));
                    }
                }
            }
            return Some(out);
        }
        Some(match self.session.engines.take_change()? {
            EngineChange::Add(t) => self.add(t, connect),
            EngineChange::Remove(id) => self.remove(&id),
            EngineChange::Reload => self.reload(),
            EngineChange::Clear => Ok(self.clear()),
            EngineChange::Install(url) => self.start_install(url),
        })
    }

    /// Start installing `url` on a thread of its own; [`Switchboard::poll_install`] takes the end.
    pub fn start_install(&mut self, url: String) -> Result<Value, String> {
        let Some(installer) = self.installer.clone() else { return Err("this editor can't install engines".into()) };
        if let Some(i) = &self.installing {
            return Err(format!("installing {}", i.url));
        }
        let i = Installing::start(installer, url.clone(), self.waker.clone())?;
        self.session.engines.installing = Some(url.clone());
        *self.installing = Some(i);
        *self.last_error = None;
        Ok(json!({"installing": url}))
    }

    /// Take a finished install: read the engine manifests again, so the Engine menu offers what
    /// was installed, and say how it went. Returns whether one finished.
    pub fn poll_install(&mut self) -> bool {
        let Some(i) = &self.installing else { return false };
        let Some(outcome) = i.poll() else { return false };
        let url = i.url.clone();
        *self.installing = None;
        self.session.engines.installing = None;
        *self.last_error = Some(match outcome {
            Ok(said) => match self.reload() {
                Ok(_) => format!("{said}. The Engine menu offers it now."),
                Err(e) => format!("{said}, but the engine list didn't reload: {e}"),
            },
            Err(e) => format!("installing {url}: {e}"),
        });
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
    /// connected; the Engine menu starts the next.
    pub fn clear(&mut self) -> Value {
        *self.coordinator = None;
        self.session.engines.added.clear();
        self.forget_runs(Instant::now());
        self.session.engines.current = None;
        *self.last_error = None;
        self.effects.redraw = true;
        json!({"engines": [], "label": self.label()})
    }

    /// Start `target` beside the engines there are (see the module docs).
    pub fn add(&mut self, target: EngineTarget, connect: &mut Connect<'_>) -> Result<Value, String> {
        let EngineTarget::Choice { engine, mode } = &target else { return Err("only an engine the editor starts can be added".into()) };
        let Some(first) = self.link.as_ref().map(|l| Arc::clone(&l.conn)) else {
            return Err("connect an engine first (Engine menu)".into());
        };
        let wanted = target_label(self.session, &target);
        let conn = match connect(engine, mode) {
            // The coordinator holds the connection; this link's own pump stops.
            Ok(link) => Arc::clone(&link.conn),
            Err(e) => {
                let e = format!("engine: {wanted}: {e}");
                *self.last_error = Some(e.clone());
                return Err(e);
            }
        };
        let coordinator = match &*self.coordinator {
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
    pub fn remove(&mut self, id: &str) -> Result<Value, String> {
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
        *self.coordinator = Some(coordinator);
        let link = Link::start(Arc::clone(&conn), format!("{n} engines"), self.waker.clone())?;
        let catalog = match conn.call("schema", json!({})).map(serde_json::from_value::<SchemaReply>) {
            Ok(Ok(c)) => Ok(c),
            Ok(Err(e)) => Err(format!("the engines' catalogue: {e}")),
            Err(e) => Err(format!("schema: {}", e.message)),
        };
        *self.state = link.state();
        sync_state(self.session, self.state);
        self.session.docs.resolver = Some(Arc::new(move |r: &DataRef| conn.resolve(r).map(|b| b.bytes.to_vec())));
        *self.link = Some(link);
        share_connection(self.session, self.link);
        let catalog = catalog.inspect_err(|e| *self.last_error = Some(format!("engine: {what}: {e}")))?;
        self.set_catalog(catalog);
        let problems: Vec<String> = self.session.problems().into_iter().collect();
        let warnings = self.coordinator.as_ref().map(|c| c.warnings()).unwrap_or_default();
        *self.last_error = problems
            .first()
            .map(|first| format!("{what}: the graph has {} problem(s) with these engines: {first}", problems.len()))
            .or_else(|| warnings.first().map(|w| format!("{what}: the engines disagree: {w}")));
        Ok(
            json!({"engines": self.session.engines.engine_ids(), "connected": self.session.connected, "label": self.label(), "problems": problems, "warnings": warnings}),
        )
    }

    /// Cancel the run in flight (telling the engine) and forget the results; what is watched
    /// stays.
    pub fn forget_runs(&mut self, now: Instant) {
        if self.session.run.running {
            self.session.run.cancel();
        }
        for out in self.session.run.take_outbox() {
            if let (plumcraft_session::run::Outgoing::Cancel(run_id), Some(link)) = (out, &*self.link) {
                let _ = link.conn.call("run.cancel", json!({"run_id": run_id}));
            }
        }
        // Dropping the link joins its pump; an engine with no other holder stops (the session's
        // clone for the application's commands goes first).
        self.session.set_connection(None);
        *self.link = None;
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

    /// Connect to `target` in place of the engines there are (see the module docs).
    pub fn switch(&mut self, target: EngineTarget, connect: &mut Connect<'_>) -> Result<Value, String> {
        let now = Instant::now();
        *self.coordinator = None;
        self.session.engines.added.clear();
        // The run in flight is cancelled, the old engine told, and its results forgotten.
        self.forget_runs(now);

        let linked = match &target {
            EngineTarget::Attach { url, token } => crate::engines::attach(url, token, self.waker.clone()),
            EngineTarget::Choice { engine, mode } => connect(engine, mode),
        };
        let wanted = target_label(self.session, &target);
        self.session.engines.current = Some(target.clone());
        let link = match linked {
            Ok(link) => link,
            Err(e) => {
                let e = format!("engine: {wanted}: {e}");
                *self.last_error = Some(e.clone());
                self.effects.redraw = true;
                return Err(e);
            }
        };
        let catalog = match link.conn.call("schema", json!({})).map(serde_json::from_value::<SchemaReply>) {
            Ok(Ok(c)) => Ok(c),
            Ok(Err(e)) => Err(format!("the engine's catalogue: {e}")),
            Err(e) => Err(format!("schema: {}", e.message)),
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
    let current = session.engines.current.as_ref()?;
    let mut label = target_label(session, current);
    for t in &session.engines.added {
        label = format!("{label} + {}", target_label(session, t));
    }
    Some(if linked { label } else { format!("{label}, not connected") })
}

/// The status bar's engine label.
pub fn engine_label(session: &Session, link: Option<&Link>, state: &EngineState) -> String {
    match link {
        None => match engine_title(session, false) {
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
    if link.is_none() {
        return engine_label(session, link, state);
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
}
