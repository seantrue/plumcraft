//! The command registry, lifted from the research software's `crates/engine/src/commands.rs` (itself modelled
//! on photocraft's).

use plumcraft_graph::{Graph, GroupId, NodeId, PortRef};
use plumcraft_proto::PortValue;
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
    /// What the application shows on nodes, outside any run: a line in the node's footer and a
    /// value for the Watched panel ([`Overlay`]). Read every frame while the canvas draws, so
    /// keep it cheap; the editor repaints a few times a second while there are any.
    pub node_overlays: Option<NodeOverlays>,
    /// Marks on wires, by the input each wire goes into ([`WireMark`]). Read every frame.
    pub wire_marks: Option<WireMarks>,
    /// A segment of the application's own in the status bar. Read every frame.
    pub status: Option<fn() -> Option<String>>,
    /// How the status segment is drawn ([`Tone`]: its background); without it, `Info`.
    pub status_tone: Option<fn() -> Tone>,
}

/// [`Hooks::node_overlays`]: each node's [`Overlay`].
pub type NodeOverlays = fn() -> Vec<(NodeId, Overlay)>;

/// [`Hooks::wire_marks`]: a [`WireMark`] by the input its wire goes into.
pub type WireMarks = fn(&Graph) -> Vec<(PortRef, WireMark)>;

/// What an application shows on one node, outside any run ([`Hooks::node_overlays`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overlay {
    /// Drawn in the node's footer, under what a run put there.
    pub text: Option<String>,
    /// Shown by the Watched panel's viewers while the node is watched, in place of the run's
    /// value: the application's is the newer. `None` leaves the panel to the run.
    pub value: Option<PortValue>,
}

/// How a [`WireMark`] or the status segment is drawn: a mark's outline, or the segment's
/// background, takes the theme's accent, warning or error colour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Info,
    Warn,
    Error,
}

impl Tone {
    /// A tone from its name in a catalogue text (`texts::ENGINE_STATUS_TONE`): `info`, `warn` or
    /// `error`, in any case and with spaces around it. Anything else, the empty text included, is
    /// `Info`: the text comes from an engine, so it is never trusted to be one of the three.
    pub fn from_text(text: &str) -> Tone {
        match text.trim().to_ascii_lowercase().as_str() {
            "warn" => Tone::Warn,
            "error" => Tone::Error,
            _ => Tone::Info,
        }
    }

    /// The tone's name, as [`Tone::from_text`] reads it.
    pub fn name(self) -> &'static str {
        match self {
            Tone::Info => "info",
            Tone::Warn => "warn",
            Tone::Error => "error",
        }
    }
}

/// A short label an application draws on a wire ([`Hooks::wire_marks`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WireMark {
    pub label: String,
    pub tone: Tone,
    /// Shown on hover.
    pub tooltip: Option<String>,
}

/// The application's node overlays (none without the hook).
pub fn node_overlays() -> Vec<(NodeId, Overlay)> {
    hooks().node_overlays.map(|f| f()).unwrap_or_default()
}

/// The application's marks on `graph`'s wires (none without the hook).
pub fn wire_marks(graph: &Graph) -> Vec<(PortRef, WireMark)> {
    hooks().wire_marks.map(|f| f(graph)).unwrap_or_default()
}

/// The application's status bar segment (none without the hook, or when it has nothing to say).
pub fn app_status() -> Option<String> {
    hooks().status.and_then(|f| f()).filter(|s| !s.is_empty())
}

/// How the application's status segment is drawn (`Info` without the hook).
pub fn app_status_tone() -> Tone {
    hooks().status_tone.map_or(Tone::Info, |f| f())
}

static HOOKS: std::sync::OnceLock<Hooks> = std::sync::OnceLock::new();

/// Set the application's hooks, once, at startup.
pub fn set_hooks(hooks: Hooks) -> std::result::Result<(), String> {
    HOOKS.set(hooks).map_err(|_| "the hooks are set at startup".to_owned())
}

pub(crate) fn hooks() -> Hooks {
    HOOKS.get().copied().unwrap_or_default()
}

thread_local! {
    /// The document whose work this thread is doing ([`with_doc`]).
    static CURRENT_DOC: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// The document the editor is working on as a hook is called (`docs/shell-interface.md` 4.3,
/// decision 3): `None` outside one. The hooks keep their signatures, so an application's hook
/// that must tell two documents apart asks here (`before` and `after` also have the session's
/// [`Session::doc_id`]).
pub fn current_doc() -> Option<u64> {
    CURRENT_DOC.with(std::cell::Cell::get)
}

/// Run `f` as document `doc`'s work: the hooks it calls see `doc` in [`current_doc`]. Nests; the
/// document before is restored afterwards, whatever `f` does.
pub fn with_doc<R>(doc: u64, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<u64>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT_DOC.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(CURRENT_DOC.with(|c| c.replace(Some(doc))));
    f()
}

/// A form a command asks with before it runs from a menu or a shortcut: a small window of
/// fields whose values become its params. The command itself is unchanged: the control channel,
/// scripts and the CLI still name its params, so everything stays a command (golden rule 4). An
/// application gives one for a command whose params the window can't work out (a name, an id, a
/// token), as Engine › Attach does by hand.
#[derive(Debug, Clone)]
pub struct CommandForm {
    /// The command it runs.
    pub command: &'static str,
    /// The window's title.
    pub title: &'static str,
    /// A line under the title saying what the command does.
    pub intro: &'static str,
    pub fields: Vec<FormField>,
    /// The button that runs it.
    pub submit: &'static str,
    /// The fields' values when the window opens, by key (from the open graph, say); `None`
    /// opens them empty.
    pub prefill: Option<Prefill>,
}

/// [`CommandForm::prefill`]: the fields' values by key, from the session.
pub type Prefill = fn(&Session) -> serde_json::Map<String, Value>;

/// One field of a [`CommandForm`]: a param, as text.
#[derive(Debug, Clone, PartialEq)]
pub struct FormField {
    /// The param it fills.
    pub key: &'static str,
    pub label: &'static str,
    /// Shown greyed while it is empty.
    pub hint: &'static str,
    pub kind: FieldKind,
    /// The command is refused by the window, saying so, while this is empty.
    pub required: bool,
}

/// How a [`FormField`] is typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldKind {
    /// One line.
    #[default]
    Line,
    /// One line, shown as dots (a token, a password).
    Secret,
    /// Several lines.
    Text,
    /// One of a list of choices, drawn as a pop-up or combo box beside the label; with `other`,
    /// a line of text too for any value not on the list (Install Engine…'s repository: the
    /// public engines, or any other). The param is the chosen choice's `value`, or the text.
    Pick {
        choices: &'static [PickChoice],
        /// May the person type a value that isn't one of `choices`?
        other: bool,
    },
}

/// One choice of a [`FieldKind::Pick`] field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickChoice {
    /// What the param is set to when it is chosen.
    pub value: &'static str,
    /// What the list shows.
    pub label: &'static str,
    /// More about it (a tooltip, a subtitle); may be empty.
    pub hint: &'static str,
}

impl CommandForm {
    /// The params its fields make: each field's text, trimmed, by key; an empty field is left
    /// out, so the command's default applies. `Err` names the first required field left empty.
    pub fn params(&self, values: &[String]) -> std::result::Result<Value, String> {
        let mut out = serde_json::Map::new();
        for (field, value) in self.fields.iter().zip(values) {
            let value = value.trim();
            if value.is_empty() {
                if field.required {
                    return Err(format!("{} is needed", field.label));
                }
                continue;
            }
            if let FieldKind::Pick { choices, other: false } = field.kind
                && !choices.iter().any(|c| c.value == value)
            {
                return Err(format!("{} must be one of the choices offered", field.label));
            }
            out.insert(field.key.to_owned(), Value::String(value.to_owned()));
        }
        Ok(Value::Object(out))
    }

    /// The fields' values as the window opens: [`CommandForm::prefill`]'s, as text.
    pub fn initial(&self, s: &Session) -> Vec<String> {
        let given = self.prefill.map(|f| f(s)).unwrap_or_default();
        self.fields
            .iter()
            .map(|f| match given.get(f.key) {
                Some(Value::String(t)) => t.clone(),
                Some(Value::Null) | None => String::new(),
                Some(other) => other.to_string(),
            })
            .collect()
    }
}

static FORMS: std::sync::Mutex<Vec<CommandForm>> = std::sync::Mutex::new(Vec::new());

/// Add an application's forms, at startup. A second form for one command is refused.
pub fn register_forms(forms: Vec<CommandForm>) -> std::result::Result<(), String> {
    let mut held = FORMS.lock().map_err(|_| "the form registry is poisoned".to_owned())?;
    for f in forms {
        if held.iter().any(|h| h.command == f.command) {
            return Err(format!("{} has a form already", f.command));
        }
        held.push(f);
    }
    Ok(())
}

/// The form `command` asks with from a menu, if it has one.
pub fn form(command: &str) -> Option<CommandForm> {
    FORMS.lock().ok().and_then(|forms| forms.iter().find(|f| f.command == command).cloned())
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
        v.extend(crate::settings_cmds::specs());
        v.extend(crate::labels_cmds::specs());
        v.extend(crate::named_cmds::specs());
        v.extend(crate::file_cmds::quit_specs());
        v.extend(crate::group_cmds::specs());
        v.extend(crate::curve_cmds::specs());
        v.extend(crate::run_cmds::specs());
        v.extend(crate::save_cmds::specs());
        v.extend(crate::trust::specs());
        v.extend(crate::bundle_cmds::specs());
        v.extend(crate::engine_cmds::specs());
        v.extend(crate::view_cmds::specs());
        v.extend(crate::viewer_cmds::specs());
        v.extend(crate::viewer_state::specs());
        v.extend(crate::chart_cmds::specs());
        v.extend(crate::derive_cmds::specs());
        v.extend(crate::chart_view::specs());
        v.extend(crate::doc_cmds::specs());
        v.extend(crate::doc_edits::specs());
        v.extend(crate::open_cmds::specs());
        v.extend(crate::convert::specs());
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
/// A move's `drag` (N3 slice 1): what names the drag that sent it, so its moves are one undo step
/// ([`crate::Session::edit_move`]): a string of at most 64 characters or a whole number; none or
/// null names none.
pub(crate) fn drag_param(p: &Value) -> Result<Option<String>> {
    match p.get("drag") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() && s.chars().count() <= 64 => Ok(Some(s.clone())),
        Some(Value::Number(n)) if n.as_u64().is_some() => Ok(Some(n.to_string())),
        Some(_) => Err(SessionError::Params("\"drag\" must name the drag: a string of 1 to 64 characters or a whole number".into())),
    }
}

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

    /// plumcraft #86: an engine's `engine_status_tone` names a tone; anything else is `Info`.
    #[test]
    fn a_tone_from_its_name_and_info_from_anything_else() {
        assert_eq!(Tone::from_text("info"), Tone::Info);
        assert_eq!(Tone::from_text("warn"), Tone::Warn);
        assert_eq!(Tone::from_text("error"), Tone::Error);
        assert_eq!(Tone::from_text(" Warn \n"), Tone::Warn);
        assert_eq!(Tone::from_text("ERROR"), Tone::Error);
        for junk in ["", "  ", "warning!", "fatal", "\u{0}", "info,error", &"x".repeat(10_000)] {
            assert_eq!(Tone::from_text(junk), Tone::Info, "{junk:?}");
        }
    }

    /// Decision 5 (N4 person gate): a pick field takes one of its choices, or with `other` any
    /// text; without `other` anything else is refused, naming the field.
    #[test]
    fn a_pick_field_takes_its_choices_and_other_text_only_when_offered() {
        const CHOICES: &[PickChoice] = &[PickChoice { value: "a/one", label: "One", hint: "" }, PickChoice { value: "a/two", label: "Two", hint: "x" }];
        let pick = |other| CommandForm {
            command: "test.pick",
            title: "Pick",
            intro: "",
            fields: vec![FormField { key: "repo", label: "Repository", hint: "", kind: FieldKind::Pick { choices: CHOICES, other }, required: true }],
            submit: "Go",
            prefill: None,
        };
        assert_eq!(pick(false).params(&[" a/two ".into()]).unwrap(), json!({"repo": "a/two"}));
        assert!(pick(false).params(&["b/three".into()]).unwrap_err().contains("Repository must be one of"));
        assert_eq!(pick(true).params(&["b/three".into()]).unwrap(), json!({"repo": "b/three"}));
        assert!(pick(true).params(&["  ".into()]).unwrap_err().contains("Repository is needed"));
    }

    fn form() -> CommandForm {
        let field = |key, label, kind, required| FormField { key, label, hint: "", kind, required };
        CommandForm {
            command: "test.form",
            title: "Test",
            intro: "",
            fields: vec![
                field("id", "Id", FieldKind::Line, true),
                field("token", "Token", FieldKind::Secret, false),
                field("about", "About", FieldKind::Text, false),
            ],
            submit: "Go",
            prefill: Some(|_| serde_json::Map::from_iter([("id".to_owned(), json!("wc-2")), ("about".to_owned(), json!(7))])),
        }
    }

    #[test]
    fn a_forms_fields_become_params_trimmed_and_empty_ones_left_out() {
        let f = form();
        let v = |a: &str, b: &str, c: &str| vec![a.to_owned(), b.to_owned(), c.to_owned()];
        assert_eq!(f.params(&v(" wc-2 ", "", "")).unwrap(), json!({"id": "wc-2"}));
        assert_eq!(f.params(&v("x", "secret", "two\nlines")).unwrap(), json!({"id": "x", "token": "secret", "about": "two\nlines"}));
        assert_eq!(f.params(&v("  ", "t", "")).unwrap_err(), "Id is needed");
    }

    #[test]
    fn a_form_opens_with_its_prefill_as_text() {
        let s = Session::new(plumcraft_proto::catalogue::SchemaReply::default());
        assert_eq!(form().initial(&s), ["wc-2", "", "7"]);
        let bare = CommandForm { prefill: None, ..form() };
        assert_eq!(bare.initial(&s), ["", "", ""]);
    }
    use serde_json::json;

    #[test]
    fn hooks_default_to_none_and_say_nothing() {
        // (no test in this binary sets the hooks)
        let h = Hooks::default();
        assert!(h.before.is_none() && h.after.is_none() && h.hold_runs.is_none());
        assert!(h.node_overlays.is_none() && h.wire_marks.is_none() && h.status.is_none());
        assert!(node_overlays().is_empty());
        assert!(wire_marks(&Graph::new()).is_empty());
        assert_eq!(app_status(), None);
        assert_eq!(Overlay::default(), Overlay { text: None, value: None });
        assert_eq!(WireMark::default().tone, Tone::Info);
    }

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
