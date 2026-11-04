//! Engine commands (menu "Engine"): switch the engine and how it is hosted, reconnect, attach
//! to a running HTTP engine. None of them is journaled: they don't change the graph.
//!
//! The session can't start engines (it never talks to one): these commands only record the
//! request in [`Engines`], as run commands leave theirs in the run outbox. Whatever hosts the
//! session takes it ([`Engines::take_request`]), drops the old connection, connects the new
//! one and gives the session its catalogue (`set_catalog`). The host also says which engines
//! it can start ([`Engines::choices`]) and which one it has ([`Engines::current`]).
//!
//! **Several engines** (`docs/research/15-multi-engine.md`): `engine.add` asks for another engine
//! beside the current one and `engine.remove` for one added to go ([`Engines::take_change`]);
//! the host puts them behind one coordinator, so the session still sees one catalogue. Switch
//! and attach replace them all with one engine; reconnect keeps the added ones.
//!
//! **The Engine menu uses `engine.use`:** picking an engine's connection starts it when nothing
//! is connected, adds it beside the others when it isn't there, and changes its connection when
//! it is, keeping the others. `engine.clear` stops them all, so a complete change is a clear
//! and a pick.

use serde::Serialize;
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, opt_str_param, str_param};
use crate::{Result, Session, SessionError};

/// The longest URL `engine.attach` takes.
pub const MAX_URL_CHARS: usize = 2048;
/// The longest token `engine.attach` takes.
pub const MAX_TOKEN_CHARS: usize = 4096;

/// An engine the host can start, hosted one way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineChoice {
    /// The engine's id (`text`, `audio`).
    pub engine: String,
    /// How it is hosted (`inproc`, `process`, `python`, `http`, `http-python`).
    pub mode: String,
    /// The engine's menu heading ("Text").
    pub engine_label: String,
    /// The hosting's menu item ("Rust · in-process (mode 1)").
    pub mode_label: String,
}

/// What to connect to.
#[derive(Clone, PartialEq, Eq)]
pub enum EngineTarget {
    /// One of the host's [`EngineChoice`]s.
    Choice { engine: String, mode: String },
    /// An HTTP engine already running at `url`.
    Attach { url: String, token: String },
}

// The token stays out of logs and journals.
impl std::fmt::Debug for EngineTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineTarget::Choice { engine, mode } => f.debug_struct("Choice").field("engine", engine).field("mode", mode).finish(),
            EngineTarget::Attach { url, .. } => f.debug_struct("Attach").field("url", url).finish_non_exhaustive(),
        }
    }
}

impl EngineTarget {
    /// As JSON, without the token (`ui.inspect`, command results).
    pub fn to_json(&self) -> Value {
        match self {
            EngineTarget::Choice { engine, mode } => json!({"engine": engine, "mode": mode}),
            EngineTarget::Attach { url, .. } => json!({"engine": null, "mode": "attach", "url": url}),
        }
    }
}

/// The engines the host offers, the one it has, and a switch asked for.
#[derive(Debug, Clone, Default)]
pub struct Engines {
    /// What the host can start (empty: switching is not offered).
    pub choices: Vec<EngineChoice>,
    /// The engine connected, or last tried.
    pub current: Option<EngineTarget>,
    /// Attaching to a running HTTP engine is offered.
    pub can_attach: bool,
    /// Installing an engine from GitHub is offered (the host has an installer).
    pub can_install: bool,
    /// The repository being installed, while it is.
    pub installing: Option<String>,
    /// Engines connected beside [`Engines::current`], in the order they were added.
    pub added: Vec<EngineTarget>,
    request: Option<EngineTarget>,
    changes: Vec<EngineChange>,
    /// Pick engines automatically (`engine.auto`): an engine started on the editor's own account
    /// is hosted the fastest way it can be ([`mode_rank`]), and a graph opened or inserted starts
    /// the engines it names that aren't connected.
    pub auto: bool,
    /// Engines a graph asked for that were already started on its account (each once, so one
    /// that fails to start isn't tried every time): cleared when nothing is missing.
    pub(crate) tried: std::collections::BTreeSet<String>,
}

/// How fast a hosting is: mode 1 (compiled in) first, then a separate process (Rust before
/// Python), then HTTP. An unknown hosting comes last.
pub fn mode_rank(mode: &str) -> usize {
    ["inproc", "process", "python", "http", "http-python"].iter().position(|m| *m == mode).unwrap_or(usize::MAX)
}

/// An engine to add beside the current one, or an added one to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineChange {
    Add(EngineTarget),
    /// By engine id.
    Remove(String),
    /// Read the engine manifests again (spec 16.6): the host's list of choices changes.
    Reload,
    /// Stop every engine: the editor keeps the graph, with none connected.
    Clear,
    /// Download an engine from GitHub and install it (`https://github.com/owner/repo`).
    Install(String),
}

impl Engines {
    /// The fastest way the host can start `engine` ([`mode_rank`]).
    pub fn best(&self, engine: &str) -> Option<&EngineChoice> {
        self.choices.iter().filter(|c| c.engine == engine).min_by_key(|c| mode_rank(&c.mode))
    }

    /// Take the switch asked for, if any (the host connects it).
    pub fn take_request(&mut self) -> Option<EngineTarget> {
        self.request.take()
    }

    /// The next add or remove asked for, if any (the host performs it).
    pub fn take_change(&mut self) -> Option<EngineChange> {
        (!self.changes.is_empty()).then(|| self.changes.remove(0))
    }

    /// Is a switch, add or remove waiting for the host?
    pub fn is_pending(&self) -> bool {
        self.request.is_some() || !self.changes.is_empty()
    }

    /// The engine ids connected or asked for: the current one and the added ones.
    pub fn engine_ids(&self) -> Vec<&str> {
        self.current.iter().chain(&self.added).filter_map(|t| if let EngineTarget::Choice { engine, .. } = t { Some(engine.as_str()) } else { None }).collect()
    }

    /// The choice connected (or last tried), when it is one.
    pub fn current_choice(&self) -> Option<&EngineChoice> {
        match &self.current {
            Some(EngineTarget::Choice { engine, mode }) => self.find(engine, mode),
            _ => None,
        }
    }

    pub fn find(&self, engine: &str, mode: &str) -> Option<&EngineChoice> {
        self.choices.iter().find(|c| c.engine == engine && c.mode == mode)
    }

    /// The connection `engine` is in use by, the first engine's or an added one's.
    pub fn in_use(&self, engine: &str) -> Option<&str> {
        self.current.iter().chain(&self.added).find_map(|t| match t {
            EngineTarget::Choice { engine: e, mode } if e == engine => Some(mode.as_str()),
            _ => None,
        })
    }

    /// Is `engine`/`mode` the current choice?
    pub fn is_current(&self, engine: &str, mode: &str) -> bool {
        matches!(&self.current, Some(EngineTarget::Choice { engine: e, mode: m }) if e == engine && m == mode)
    }
}

fn can_switch(s: &Session) -> std::result::Result<(), String> {
    if s.engines.choices.is_empty() { Err("this editor can't start engines".into()) } else { Ok(()) }
}

fn can_reconnect(s: &Session) -> std::result::Result<(), String> {
    if s.engines.current.is_some() { Ok(()) } else { Err("no engine to reconnect".into()) }
}

fn can_attach(s: &Session) -> std::result::Result<(), String> {
    if s.engines.can_attach { Ok(()) } else { Err("this editor can't attach to engines".into()) }
}

fn switch(s: &mut Session, p: &Value) -> Result<Value> {
    let (engine, mode) = (str_param(p, "engine")?, str_param(p, "mode")?);
    if s.engines.find(engine, mode).is_none() {
        let valid: Vec<String> = s.engines.choices.iter().map(|c| format!("{}/{}", c.engine, c.mode)).collect();
        let shown = |t: &str| t.chars().take(64).collect::<String>();
        return Err(SessionError::Params(format!("no engine {:?} hosted {:?} (the choices: {})", shown(engine), shown(mode), valid.join(", "))));
    }
    let target = EngineTarget::Choice { engine: engine.to_owned(), mode: mode.to_owned() };
    let out = target.to_json();
    s.engines.request = Some(target);
    Ok(out)
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let (engine, mode) = (str_param(p, "engine")?, str_param(p, "mode")?);
    if s.engines.find(engine, mode).is_none() {
        let shown = |t: &str| t.chars().take(64).collect::<String>();
        return Err(SessionError::Params(format!("no engine {:?} hosted {:?}", shown(engine), shown(mode))));
    }
    if s.engines.engine_ids().contains(&engine) {
        return Err(SessionError::Params(format!("{engine} is already connected")));
    }
    let target = EngineTarget::Choice { engine: engine.to_owned(), mode: mode.to_owned() };
    let out = target.to_json();
    s.engines.changes.push(EngineChange::Add(target));
    Ok(out)
}

/// The Engine menu's pick: start, add, or change the connection of one engine (module docs).
fn use_engine(s: &mut Session, p: &Value) -> Result<Value> {
    let engine = str_param(p, "engine")?;
    // no mode: the fastest the host has
    let best = s.engines.best(engine).map(|c| c.mode.clone());
    let mode = match p.get("mode") {
        Some(_) => str_param(p, "mode")?,
        None => best.as_deref().unwrap_or_default(),
    };
    if s.engines.find(engine, mode).is_none() {
        let shown = |t: &str| t.chars().take(64).collect::<String>();
        return Err(SessionError::Params(format!("no engine {:?} hosted {:?}", shown(engine), shown(mode))));
    }
    let target = EngineTarget::Choice { engine: engine.to_owned(), mode: mode.to_owned() };
    let mut out = target.to_json();
    let first = matches!(&s.engines.current, Some(EngineTarget::Choice { engine: e, .. }) if e == engine);
    let how = if !s.connected {
        // nothing to keep: this is the engine now
        s.engines.request = Some(target);
        "start"
    } else if s.engines.in_use(engine) == Some(mode) {
        "unchanged"
    } else if first {
        // the first engine is switched; the added ones come back beside it
        s.engines.request = Some(target);
        let again: Vec<EngineChange> = s.engines.added.iter().cloned().map(EngineChange::Add).collect();
        s.engines.changes.extend(again);
        "change"
    } else if s.engines.in_use(engine).is_some() {
        s.engines.changes.push(EngineChange::Remove(engine.to_owned()));
        s.engines.changes.push(EngineChange::Add(target));
        "change"
    } else {
        s.engines.changes.push(EngineChange::Add(target));
        "add"
    };
    out["how"] = json!(how);
    Ok(out)
}

/// `engine.auto {on?}`: pick engines automatically, or not (no `on`: say which). Turning it on
/// starts what the graph there is needs.
fn engine_auto(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(on) = p.get("on") {
        s.engines.auto = on.as_bool().ok_or_else(|| SessionError::Params("on must be true or false".into()))?;
    }
    let started = if s.engines.auto { start_needed(s) } else { Vec::new() };
    Ok(json!({"on": s.engines.auto, "started": started}))
}

/// With [`Engines::auto`] on: start each engine the graph names that isn't connected and the host
/// can start, once, hosted the fastest way ([`Engines::best`]). The first starts the editor's
/// engine when none is connected; the rest go beside it. Returns what was asked for.
pub(crate) fn start_needed(s: &mut Session) -> Vec<String> {
    let Some(missing) = s.engines.auto.then(|| s.missing_engines()).flatten() else {
        s.engines.tried.clear();
        return Vec::new();
    };
    let mut started = Vec::new();
    for e in missing.engines {
        if s.engines.tried.contains(&e) {
            continue;
        }
        s.engines.tried.insert(e.clone());
        let Some(choice) = s.engines.best(&e) else { continue };
        let target = EngineTarget::Choice { engine: e.clone(), mode: choice.mode.clone() };
        if !s.connected && s.engines.request.is_none() {
            s.engines.request = Some(target);
        } else {
            s.engines.changes.push(EngineChange::Add(target));
        }
        started.push(e);
    }
    started
}

fn can_clear(s: &Session) -> std::result::Result<(), String> {
    if s.engines.current.is_some() { Ok(()) } else { Err("no engine to clear".into()) }
}

fn clear(s: &mut Session, _: &Value) -> Result<Value> {
    // whatever was asked for before is moot
    s.engines.request = None;
    s.engines.changes.clear();
    s.engines.changes.push(EngineChange::Clear);
    Ok(json!({"engines": []}))
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let engine = str_param(p, "engine")?;
    let added = s.engines.added.iter().any(|t| matches!(t, EngineTarget::Choice { engine: e, .. } if e == engine));
    if !added {
        return Err(SessionError::Params(format!("{:?} isn't an added engine (Switch replaces the first)", engine.chars().take(64).collect::<String>())));
    }
    s.engines.changes.push(EngineChange::Remove(engine.to_owned()));
    Ok(json!({"engine": engine}))
}

fn can_remove(s: &Session) -> std::result::Result<(), String> {
    if s.engines.added.is_empty() { Err("no engine was added".into()) } else { Ok(()) }
}

fn reconnect(s: &mut Session, _: &Value) -> Result<Value> {
    let target = s.engines.current.clone().ok_or_else(|| SessionError::Params("no engine to reconnect".into()))?;
    let out = target.to_json();
    s.engines.request = Some(target);
    Ok(out)
}

fn attach(s: &mut Session, p: &Value) -> Result<Value> {
    let url = str_param(p, "url")?.trim();
    let token = opt_str_param(p, "token")?.unwrap_or_default().trim();
    let scheme_ok = url.starts_with("http://") || url.starts_with("https://");
    if !scheme_ok || url.chars().count() > MAX_URL_CHARS || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(SessionError::Params("\"url\" must be an http:// or https:// URL (http://host:port)".into()));
    }
    if token.chars().count() > MAX_TOKEN_CHARS || token.chars().any(char::is_control) {
        return Err(SessionError::Params("\"token\" must be the engine's bearer token".into()));
    }
    let target = EngineTarget::Attach { url: url.trim_end_matches('/').to_owned(), token: token.to_owned() };
    let out = target.to_json();
    s.engines.request = Some(target);
    Ok(out)
}

/// The longest owner or repository name GitHub allows.
const MAX_GITHUB_NAME: usize = 100;

/// `owner/repo`, or `https://github.com/owner/repo` (with `.git` or a trailing `/`), as the
/// repository's https URL; `None` for anything else.
pub fn github_repo(text: &str) -> Option<String> {
    let t = text.trim();
    let rest = t.strip_prefix("https://github.com/").or_else(|| t.strip_prefix("github.com/")).unwrap_or(t);
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (owner, repo) = rest.split_once('/')?;
    let ok = |n: &str| !n.is_empty() && n.len() <= MAX_GITHUB_NAME && !n.starts_with('.') && n.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    (ok(owner) && ok(repo)).then(|| format!("https://github.com/{owner}/{repo}"))
}

fn can_install(s: &Session) -> std::result::Result<(), String> {
    match (s.engines.can_install, &s.engines.installing) {
        (false, _) => Err("this editor can't install engines".into()),
        (true, Some(r)) => Err(format!("installing {r}")),
        (true, None) => Ok(()),
    }
}

fn install(s: &mut Session, p: &Value) -> Result<Value> {
    can_install(s).map_err(SessionError::Params)?;
    let raw = str_param(p, "repo")?;
    let url = github_repo(raw).ok_or_else(|| {
        SessionError::Params(format!(
            "{:?} isn't a GitHub repository: give owner/repo or https://github.com/owner/repo",
            raw.chars().take(200).collect::<String>()
        ))
    })?;
    s.engines.changes.push(EngineChange::Install(url.clone()));
    Ok(json!({"installing": url}))
}

fn reload(s: &mut Session, _: &Value) -> Result<Value> {
    s.engines.changes.push(EngineChange::Reload);
    Ok(json!({"reloading": true}))
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let e = &s.engines;
    let added: Vec<Value> = e.added.iter().map(EngineTarget::to_json).collect();
    Ok(
        json!({"choices": e.choices, "current": e.current.as_ref().map(EngineTarget::to_json), "added": added, "canAttach": e.can_attach, "canInstall": e.can_install, "installing": e.installing, "pending": e.is_pending()}),
    )
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "engine.switch",
            label: "Switch Engine",
            // the Engine menu lists the choices themselves (they come from the host)
            menu: &[],
            shortcut: None,
            params: "{engine, mode} (one of engine.list's choices) -> {engine, mode}: cancel the run in flight, stop the engine, start this one and take its catalogue; the graph is kept, and nodes the new catalogue lacks are reported",
            enabled: can_switch,
            run: switch,
            journal: false,
        },
        CommandSpec {
            id: "engine.add",
            label: "Add Engine",
            // the Engine menu lists the choices itself
            menu: &[],
            shortcut: None,
            params: "{engine, mode} (one of engine.list's choices, an engine not connected) -> {engine, mode}: start it beside the engines there are; their catalogues merge, and a graph may use nodes of both (docs/research/15-multi-engine.md)",
            enabled: can_switch,
            run: add,
            journal: false,
        },
        CommandSpec {
            id: "engine.use",
            label: "Use Engine",
            // the Engine menu lists the choices themselves
            menu: &[],
            shortcut: None,
            params: "{engine, mode?} (one of engine.list's choices; no mode: the fastest hosting there is) -> {engine, mode, how}: what the Engine menu does: with nothing connected start it (how: start); not connected, add it beside the others (add); connected another way, change its connection and keep the others (change); else nothing (unchanged)",
            enabled: can_switch,
            run: use_engine,
            journal: false,
        },
        CommandSpec {
            id: "engine.auto",
            label: "Pick Engines Automatically",
            // the Engine menu draws it as a checkbox
            menu: &[],
            shortcut: None,
            params: "{on?: bool} -> {on, started}: an engine the editor starts on its own is hosted the fastest way it can be (in-process, then a process, then HTTP), and a graph opened or inserted starts the engines it names that aren't connected; no on: say which",
            enabled: always,
            run: engine_auto,
            journal: false,
        },
        CommandSpec {
            id: "engine.remove",
            label: "Remove Engine",
            menu: &[],
            shortcut: None,
            params: "{engine} (an added engine's id) -> {engine}: stop it; nodes only it offers are reported",
            enabled: can_remove,
            run: remove,
            journal: false,
        },
        CommandSpec {
            id: "engine.reconnect",
            label: "Reconnect",
            menu: &["Engine"],
            shortcut: None,
            params: "{} -> {engine, mode}: stop the engine and start it again (a fresh process), keeping the graph",
            enabled: can_reconnect,
            run: reconnect,
            journal: false,
        },
        CommandSpec {
            id: "engine.attach",
            label: "Attach…",
            menu: &["Engine"],
            shortcut: None,
            params: "{url: \"http://host:port\", token?} -> {mode: \"attach\", url}: connect to an HTTP engine already running (started with --listen), as a remote UI",
            enabled: can_attach,
            run: attach,
            journal: false,
        },
        CommandSpec {
            id: "engine.manifests.reload",
            label: "Reload Engine List",
            menu: &["Engine"],
            shortcut: None,
            params: "{} -> {reloading: true}: read the engine manifests again (--engine-manifest and ~/.config/plumcraft/engines/*.toml), so an engine just installed is offered without a restart; ui.inspect's engine and engine.list show the new choices",
            enabled: can_switch,
            run: reload,
            journal: false,
        },
        CommandSpec {
            id: "engine.clear",
            label: "Clear Engines",
            menu: &["Engine"],
            shortcut: None,
            params: "{} -> {engines: []}: stop every engine; the graph stays, with none connected, and the Engine menu picks the next",
            enabled: can_clear,
            run: clear,
            journal: false,
        },
        CommandSpec {
            id: "engine.install",
            label: "Install Engine…",
            menu: &["Engine"],
            shortcut: None,
            params: "{repo: \"owner/repo\" | \"https://github.com/owner/repo\"} -> {installing: url}: download a Python engine from GitHub and install it (`uv tool install git+URL`, then its own `register`), then read the engine manifests again; it runs in the background, and the status bar says when it is done (ui.inspect's engine.installing until then). It runs the package's code: install only what you trust",
            enabled: can_install,
            run: install,
            journal: false,
        },
        CommandSpec {
            id: "engine.list",
            label: "Engines",
            menu: &[],
            shortcut: None,
            params: "{} -> {choices: [{engine, mode, engineLabel, modeLabel}], current: {engine, mode, url?} | null, added: [{engine, mode}], canAttach, pending}",
            enabled: always,
            run: list,
            journal: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered() -> Session {
        let mut s = Session::default();
        for (engine, mode) in [("text", "inproc"), ("text", "python"), ("audio", "inproc")] {
            s.engines.choices.push(EngineChoice { engine: engine.into(), mode: mode.into(), engine_label: engine.into(), mode_label: mode.into() });
        }
        s.engines.current = Some(EngineTarget::Choice { engine: "text".into(), mode: "inproc".into() });
        s.engines.can_attach = true;
        s
    }

    #[test]
    fn switching_records_the_request_for_the_host() {
        let mut s = offered();
        let r = s.execute("engine.switch", &json!({"engine": "text", "mode": "python"})).unwrap();
        assert_eq!(r, json!({"engine": "text", "mode": "python"}));
        assert!(s.engines.is_pending());
        assert_eq!(s.engines.take_request(), Some(EngineTarget::Choice { engine: "text".into(), mode: "python".into() }));
        assert!(!s.engines.is_pending());
        assert!(s.journal().is_empty(), "not journaled");
    }

    #[test]
    fn only_offered_choices_switch() {
        let mut s = offered();
        let e = s.execute("engine.switch", &json!({"engine": "audio", "mode": "python"})).unwrap_err().to_string();
        assert!(e.contains("text/python") && e.contains("audio/inproc"), "{e}");
        for bad in [json!({}), json!({"engine": 1, "mode": "x"}), json!(null), json!({"engine": "x".repeat(10_000), "mode": "y"})] {
            assert!(s.execute("engine.switch", &bad).is_err(), "{bad}");
        }
        assert!(!s.engines.is_pending());
        let mut bare = Session::default();
        assert!(bare.execute("engine.switch", &json!({"engine": "text", "mode": "inproc"})).unwrap_err().to_string().contains("can't start engines"));
        assert!(bare.execute("engine.reconnect", &Value::Null).is_err());
        assert!(bare.execute("engine.attach", &json!({"url": "http://127.0.0.1:1"})).is_err());
    }

    #[test]
    fn reconnect_asks_for_the_current_engine() {
        let mut s = offered();
        s.execute("engine.reconnect", &Value::Null).unwrap();
        assert_eq!(s.engines.take_request(), s.engines.current.clone());
    }

    #[test]
    fn attach_checks_its_url_and_keeps_the_token_out_of_results() {
        let mut s = offered();
        let r = s.execute("engine.attach", &json!({"url": " http://127.0.0.1:8123/ ", "token": "sekrit"})).unwrap();
        assert_eq!(r, json!({"engine": null, "mode": "attach", "url": "http://127.0.0.1:8123"}));
        let t = s.engines.take_request().unwrap();
        assert!(!format!("{t:?}").contains("sekrit"));
        assert_eq!(t, EngineTarget::Attach { url: "http://127.0.0.1:8123".into(), token: "sekrit".into() });
        for bad in [
            json!({"url": "ftp://x"}),
            json!({"url": "http://a b"}),
            json!({"url": format!("http://{}", "a".repeat(3000))}),
            json!({"url": "http://x", "token": 5}),
        ] {
            assert!(s.execute("engine.attach", &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn adding_and_removing_record_changes_for_the_host() {
        let mut s = offered();
        assert!(s.execute("engine.add", &json!({"engine": "text", "mode": "python"})).unwrap_err().to_string().contains("already connected"));
        assert!(s.execute("engine.remove", &json!({"engine": "audio"})).is_err(), "nothing added yet");
        assert_eq!(s.execute("engine.add", &json!({"engine": "audio", "mode": "inproc"})).unwrap(), json!({"engine": "audio", "mode": "inproc"}));
        assert!(s.engines.is_pending());
        let add = s.engines.take_change().unwrap();
        assert_eq!(add, EngineChange::Add(EngineTarget::Choice { engine: "audio".into(), mode: "inproc".into() }));
        // The host connected it.
        s.engines.added.push(EngineTarget::Choice { engine: "audio".into(), mode: "inproc".into() });
        assert_eq!(s.engines.engine_ids(), ["text", "audio"]);
        assert!(s.execute("engine.add", &json!({"engine": "audio", "mode": "inproc"})).is_err(), "already there");
        assert!(s.execute("engine.remove", &json!({"engine": "text"})).is_err(), "the first is switched, not removed");
        s.execute("engine.remove", &json!({"engine": "audio"})).unwrap();
        assert_eq!(s.engines.take_change(), Some(EngineChange::Remove("audio".into())));
        assert_eq!(s.engines.take_change(), None);
        assert_eq!(s.execute("engine.list", &Value::Null).unwrap()["added"], json!([{"engine": "audio", "mode": "inproc"}]));
        assert!(s.journal().is_empty(), "not journaled");
        for bad in [json!({}), json!({"engine": "nope", "mode": "inproc"}), json!({"engine": 3})] {
            assert!(s.execute("engine.add", &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn picking_automatically_starts_what_a_graph_needs_the_fastest_way() {
        let mut s = offered();
        // audio is offered over HTTP and as a process too: the process is faster than HTTP,
        // in-process faster still
        for mode in ["http", "process"] {
            s.engines.choices.push(EngineChoice { engine: "audio".into(), mode: mode.into(), engine_label: "audio".into(), mode_label: mode.into() });
        }
        s.engines.choices.push(EngineChoice { engine: "film".into(), mode: "http".into(), engine_label: "film".into(), mode_label: "http".into() });
        assert_eq!(s.engines.best("audio").map(|c| c.mode.as_str()), Some("inproc"));
        assert_eq!(s.engines.best("film").map(|c| c.mode.as_str()), Some("http"), "the only way there is");
        s.connected = true;
        s.graph = plumcraft_graph::Graph::from_json(
            r#"{"version": 1, "engines": ["audio", "film", "nowhere"], "nodes": {"a": {"kind": "audio.tone"}, "f": {"kind": "fc.new"}}}"#,
        )
        .unwrap();
        // off: nothing starts
        assert!(start_needed(&mut s).is_empty() && s.engines.take_change().is_none());
        let r = s.execute("engine.auto", &json!({"on": true})).unwrap();
        assert_eq!(r["started"], json!(["audio", "film"]), "what the host can start; not `nowhere`");
        assert_eq!(s.engines.take_change(), Some(EngineChange::Add(EngineTarget::Choice { engine: "audio".into(), mode: "inproc".into() })));
        assert_eq!(s.engines.take_change(), Some(EngineChange::Add(EngineTarget::Choice { engine: "film".into(), mode: "http".into() })));
        assert!(start_needed(&mut s).is_empty(), "each once: one that failed isn't tried every time");
        // nothing connected: the first is the editor's engine, the rest go beside it
        let mut s2 = offered();
        s2.engines.current = None;
        s2.engines.auto = true;
        s2.graph = plumcraft_graph::Graph::from_json(
            r#"{"version": 1, "engines": ["audio", "text"], "nodes": {"a": {"kind": "audio.tone"}, "t": {"kind": "text.upper"}}}"#,
        )
        .unwrap();
        assert_eq!(start_needed(&mut s2), ["audio", "text"]);
        assert_eq!(s2.engines.take_request(), Some(EngineTarget::Choice { engine: "audio".into(), mode: "inproc".into() }));
        assert_eq!(s2.engines.take_change(), Some(EngineChange::Add(EngineTarget::Choice { engine: "text".into(), mode: "inproc".into() })));
        // engine.use without a mode: the fastest
        let mut s3 = offered();
        s3.connected = true;
        assert_eq!(s3.execute("engine.use", &json!({"engine": "audio"})).unwrap()["mode"], "inproc");
    }

    #[test]
    fn use_starts_adds_or_changes_an_engine() {
        let text = |m: &str| EngineTarget::Choice { engine: "text".into(), mode: m.into() };
        let audio = EngineTarget::Choice { engine: "audio".into(), mode: "inproc".into() };
        // nothing connected: start it
        let mut s = offered();
        let r = s.execute("engine.use", &json!({"engine": "text", "mode": "python"})).unwrap();
        assert_eq!(r["how"], "start");
        assert_eq!(s.engines.take_request(), Some(text("python")));
        // connected: another engine is added, the same one is left
        s.connected = true;
        assert_eq!(s.execute("engine.use", &json!({"engine": "text", "mode": "inproc"})).unwrap()["how"], "unchanged");
        assert!(!s.engines.is_pending());
        assert_eq!(s.execute("engine.use", &json!({"engine": "audio", "mode": "inproc"})).unwrap()["how"], "add");
        assert_eq!(s.engines.take_change(), Some(EngineChange::Add(audio.clone())));
        // the first engine's connection changes and the added one comes back
        s.engines.added.push(audio.clone());
        assert_eq!(s.execute("engine.use", &json!({"engine": "text", "mode": "python"})).unwrap()["how"], "change");
        assert_eq!(s.engines.take_request(), Some(text("python")));
        assert_eq!(s.engines.take_change(), Some(EngineChange::Add(audio.clone())));
        // an added engine's connection changes: out, and in again the new way
        s.engines.current = Some(audio.clone());
        s.engines.added = vec![text("inproc")];
        s.execute("engine.use", &json!({"engine": "text", "mode": "python"})).unwrap();
        assert_eq!(s.engines.take_change(), Some(EngineChange::Remove("text".into())));
        assert_eq!(s.engines.take_change(), Some(EngineChange::Add(text("python"))));
        assert_eq!(s.engines.take_change(), None);
        for bad in [json!({}), json!({"engine": "nope", "mode": "inproc"}), json!({"engine": "audio", "mode": "python"})] {
            assert!(s.execute("engine.use", &bad).is_err(), "{bad}");
        }
        assert!(s.journal().is_empty(), "not journaled");
    }

    #[test]
    fn clear_stops_every_engine_and_drops_what_was_asked() {
        let mut s = offered();
        s.execute("engine.switch", &json!({"engine": "text", "mode": "python"})).unwrap();
        s.execute("engine.clear", &Value::Null).unwrap();
        assert_eq!(s.engines.take_request(), None);
        assert_eq!(s.engines.take_change(), Some(EngineChange::Clear));
        s.engines.current = None;
        assert!(s.execute("engine.clear", &Value::Null).unwrap_err().to_string().contains("no engine"));
    }

    #[test]
    fn install_takes_a_github_repository_and_one_at_a_time() {
        assert_eq!(github_repo("seantrue/plumcraft-duckdb-engine").as_deref(), Some("https://github.com/seantrue/plumcraft-duckdb-engine"));
        assert_eq!(github_repo(" https://github.com/a/b.git/ ").as_deref(), Some("https://github.com/a/b"));
        assert_eq!(github_repo("github.com/a-1/b_2.x").as_deref(), Some("https://github.com/a-1/b_2.x"));
        for bad in ["", "a", "a/", "/b", "a/b/c", "https://gitlab.com/a/b", "a/b; rm -rf ~", "a/../b", "../a/b", "a b/c", &format!("a/{}", "x".repeat(101))] {
            assert_eq!(github_repo(bad), None, "{bad}");
        }
        let mut s = offered();
        assert!(s.execute("engine.install", &json!({"repo": "a/b"})).unwrap_err().to_string().contains("can't install"));
        s.engines.can_install = true;
        assert!(s.execute("engine.install", &json!({"repo": "nope"})).is_err());
        assert_eq!(s.execute("engine.install", &json!({"repo": "a/b"})).unwrap(), json!({"installing": "https://github.com/a/b"}));
        assert_eq!(s.engines.take_change(), Some(EngineChange::Install("https://github.com/a/b".into())));
        s.engines.installing = Some("https://github.com/a/b".into());
        assert!(s.execute("engine.install", &json!({"repo": "c/d"})).unwrap_err().to_string().contains("installing"));
        assert!(s.journal().is_empty(), "not journaled");
    }

    #[test]
    fn list_reports_choices_and_current() {
        let mut s = offered();
        let r = s.execute("engine.list", &Value::Null).unwrap();
        assert_eq!(r["choices"].as_array().unwrap().len(), 3);
        assert_eq!(r["choices"][1]["modeLabel"], "python");
        assert_eq!(r["current"], json!({"engine": "text", "mode": "inproc"}));
        assert!(s.engines.is_current("text", "inproc") && !s.engines.is_current("text", "python"));
        assert_eq!(s.engines.current_choice().map(|c| c.mode.as_str()), Some("inproc"));
    }
}
