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
    /// Engines connected beside [`Engines::current`], in the order they were added.
    pub added: Vec<EngineTarget>,
    request: Option<EngineTarget>,
    changes: Vec<EngineChange>,
}

/// An engine to add beside the current one, or an added one to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineChange {
    Add(EngineTarget),
    /// By engine id.
    Remove(String),
    /// Read the engine manifests again (spec 16.6): the host's list of choices changes.
    Reload,
}

impl Engines {
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

fn reload(s: &mut Session, _: &Value) -> Result<Value> {
    s.engines.changes.push(EngineChange::Reload);
    Ok(json!({"reloading": true}))
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let e = &s.engines;
    let added: Vec<Value> = e.added.iter().map(EngineTarget::to_json).collect();
    Ok(
        json!({"choices": e.choices, "current": e.current.as_ref().map(EngineTarget::to_json), "added": added, "canAttach": e.can_attach, "pending": e.is_pending()}),
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
