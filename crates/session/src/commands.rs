//! The command registry, lifted from the research software's `crates/engine/src/commands.rs` (itself modelled
//! on photocraft's).

use plumcraft_graph::{GroupId, NodeId, PortRef};
use serde_json::{Value, json};

use crate::{Result, Session, SessionError};

pub type Run = fn(&mut Session, &Value) -> Result<Value>;
pub type Enabled = fn(&Session) -> std::result::Result<(), String>;

/// Metadata and implementation of one command.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu placement, e.g. `["Edit"]`. Empty: not in menus (canvas gestures, context menus).
    pub menu: &'static [&'static str],
    /// Default shortcut in egui-style notation (`Cmd+Shift+Z`), mapped per platform by the UI.
    pub shortcut: Option<&'static str>,
    /// Human- and agent-readable parameter description (JSON-ish).
    pub params: &'static str,
    pub enabled: Enabled,
    pub run: Run,
    /// Record in the session journal (false for queries and canvas drags).
    pub journal: bool,
}

impl std::fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandSpec").field("id", &self.id).field("menu", &self.menu).field("shortcut", &self.shortcut).finish_non_exhaustive()
    }
}

pub fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}

/// What an application does around commands and runs, for work of its own that outlives a command
/// (a file still being written, say). Each is short: it runs on the UI thread.
#[derive(Debug, Clone, Copy, Default)]
pub struct Hooks {
    /// Before any command runs (id, session).
    pub before: Option<fn(&str, &Session)>,
    /// After a command succeeded (id, session).
    pub after: Option<fn(&str, &Session)>,
    /// True while no run or save may go to an engine yet: they wait in their outboxes.
    pub hold_runs: Option<fn() -> bool>,
}

static HOOKS: std::sync::OnceLock<Hooks> = std::sync::OnceLock::new();

/// Set the application's hooks, once, at startup.
pub fn set_hooks(hooks: Hooks) -> std::result::Result<(), String> {
    HOOKS.set(hooks).map_err(|_| "the hooks are set at startup".to_owned())
}

pub(crate) fn hooks() -> Hooks {
    HOOKS.get().copied().unwrap_or_default()
}

/// Is `id` a command the application added, rather than PlumCraft's?
pub fn is_added(id: &str) -> bool {
    command_specs();
    APP_IDS.get().is_some_and(|ids| ids.contains(&id))
}

/// The ids of what [`register`] added.
static APP_IDS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();

static REGISTRY: std::sync::OnceLock<Vec<CommandSpec>> = std::sync::OnceLock::new();
/// What an application adds, until the registry is built.
static ADDED: std::sync::Mutex<Vec<CommandSpec>> = std::sync::Mutex::new(Vec::new());

/// Add an application's commands, after PlumCraft's. Before anything reads the registry, and an
/// id already taken is left out: an application can't replace a command.
pub fn register(commands: Vec<CommandSpec>) -> std::result::Result<(), String> {
    if REGISTRY.get().is_some() {
        return Err("commands are registered at startup".into());
    }
    ADDED.lock().map_err(|_| "the command registry is poisoned".to_owned())?.extend(commands);
    Ok(())
}

/// All commands, in menu order.
pub fn command_specs() -> &'static [CommandSpec] {
    REGISTRY.get_or_init(|| {
        // Edit first, so Undo and Redo top the Edit menu.
        let mut v = crate::edit_cmds::specs();
        v.extend(crate::graph_cmds::specs());
        v.extend(crate::expand_cmds::specs());
        v.extend(crate::net_cmds::specs());
        v.extend(crate::frame_cmds::specs());
        v.extend(crate::layout_cmds::specs());
        v.extend(crate::file_cmds::specs());
        v.extend(crate::insert_cmds::specs());
        v.extend(crate::file_cmds::quit_specs());
        v.extend(crate::group_cmds::specs());
        v.extend(crate::run_cmds::specs());
        v.extend(crate::save_cmds::specs());
        v.extend(crate::trust::specs());
        v.extend(crate::bundle_cmds::specs());
        v.extend(crate::engine_cmds::specs());
        v.extend(crate::view_cmds::specs());
        v.extend(crate::viewer_cmds::specs());
        v.extend(crate::doc_cmds::specs());
        v.extend(crate::open_cmds::specs());
        v.extend(crate::file_cmds::about_specs());
        let added = ADDED.lock().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default();
        let _ = APP_IDS.set(added.iter().map(|c| c.id).collect());
        for c in added {
            if v.iter().any(|x| x.id == c.id) {
                eprintln!("command {}: the id is taken", c.id);
            } else {
                v.push(c);
            }
        }
        v
    })
}

pub fn find(id: &str) -> Option<&'static CommandSpec> {
    command_specs().iter().find(|c| c.id == id)
}

/// A `command.list` row for `c`.
pub fn list_entry(s: &Session, c: &CommandSpec) -> Value {
    let reason = (c.enabled)(s).err();
    json!({"id": c.id, "label": c.label, "menu": c.menu, "shortcut": c.shortcut, "params": c.params, "enabled": reason.is_none(), "disabledReason": reason})
}

// ---------- param helpers: params are hostile input ----------

pub(crate) fn str_param<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p.get(key).and_then(Value::as_str).ok_or_else(|| SessionError::Params(format!("{key:?} must be a string")))
}

pub(crate) fn port_param(p: &Value, key: &str) -> Result<PortRef> {
    let obj = p.get(key).ok_or_else(|| SessionError::Params(format!("{key:?} is required: {{\"node\": id, \"port\": name}}")))?;
    Ok(PortRef::new(str_param(obj, "node")?, str_param(obj, "port")?))
}

/// An optional string.
pub(crate) fn opt_str_param<'a>(p: &'a Value, key: &str) -> Result<Option<&'a str>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_str().map(Some).ok_or_else(|| SessionError::Params(format!("{key:?} must be a string"))),
    }
}

/// A list of ids (`["n1", "n2"]`); missing means empty.
pub(crate) fn id_list_param(p: &Value, key: &str) -> Result<Vec<String>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            items.iter().map(|v| v.as_str().map(str::to_owned).ok_or_else(|| SessionError::Params(format!("{key:?} must be a list of ids")))).collect()
        }
        Some(_) => Err(SessionError::Params(format!("{key:?} must be a list of ids"))),
    }
}

pub(crate) fn group_list_param(p: &Value, key: &str) -> Result<Vec<GroupId>> {
    Ok(id_list_param(p, key)?.iter().map(|g| GroupId::from(g.as_str())).collect())
}

pub(crate) fn group_param(p: &Value, key: &str) -> Result<GroupId> {
    Ok(GroupId::from(str_param(p, key)?))
}

/// An optional boolean (missing or null: `default`).
pub(crate) fn opt_bool_param(p: &Value, key: &str, default: bool) -> Result<bool> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_bool().ok_or_else(|| SessionError::Params(format!("{key:?} must be true or false"))),
    }
}

/// A required boolean.
pub(crate) fn bool_param(p: &Value, key: &str) -> Result<bool> {
    p.get(key).and_then(Value::as_bool).ok_or_else(|| SessionError::Params(format!("{key:?} must be true or false")))
}

pub(crate) fn node_list_param(p: &Value, key: &str) -> Result<Vec<NodeId>> {
    Ok(id_list_param(p, key)?.into_iter().map(NodeId).collect())
}

/// An optional `[x, y]` of finite numbers.
pub(crate) fn pos_param(p: &Value, key: &str) -> Result<Option<[f32; 2]>> {
    let Some(v) = p.get(key) else { return Ok(None) };
    let bad = || SessionError::Params(format!("{key:?} must be [x, y] with finite numbers"));
    let [x, y] = v.as_array().map(Vec::as_slice).ok_or_else(bad)? else { return Err(bad()) };
    let coord = |c: &Value| c.as_f64().map(|f| f as f32).filter(|f| f.is_finite()).ok_or_else(bad);
    Ok(Some([coord(x)?, coord(y)?]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pos_param_rejects_junk() {
        assert_eq!(pos_param(&json!({"pos": [1, 2.5]}), "pos").unwrap(), Some([1.0, 2.5]));
        assert_eq!(pos_param(&json!({}), "pos").unwrap(), None);
        for bad in [json!({"pos": [1]}), json!({"pos": "1,2"}), json!({"pos": [1e300, 0]}), json!({"pos": [null, 0]})] {
            assert!(pos_param(&bad, "pos").is_err(), "{bad}");
        }
    }

    #[test]
    fn list_entry_reports_disabled() {
        let s = Session::default();
        let e = list_entry(&s, find("node.add").unwrap());
        assert_eq!(e["enabled"], json!(false));
        assert!(e["disabledReason"].as_str().unwrap().contains("catalogue"));
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = command_specs().iter().map(|c| c.id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }
}
