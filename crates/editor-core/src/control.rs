//! The control channel's requests, answered by the core for every shell (`docs/control-protocol.md`;
//! `docs/research/24-native-shells.md` section 9: "the control channel moves into the core").
//!
//! One dispatcher serves the egui editor's `--control` port, the C ABI's `plum_request` and the C
//! ABI's own control port, so a shell built on the C ABI is driven by the same probes, over the
//! same protocol, as the egui editor, and no shell implements a method of it:
//!
//! - [`handle`] takes one request and a [`Reply`], called exactly once, now or later;
//!   [`now`] answers the methods that reply at once.
//! - The few things only a shell can do are its [`Shell`] methods: the state it lends for
//!   `ui.inspect` ([`Shell::view`]) and its own block of it ([`Shell::inspect_shell`]: geometry,
//!   windows, what is on screen), pixels (`ui.screenshot`), widget state (`ui.set`) and the audio
//!   device (`ui.audio`). A shell without them gets a valid `ui.inspect` of the core's parts, a
//!   `screenshot` ask for `ui.screenshot` ([`ask_screenshot`]), and "not available from this
//!   shell" for the other two.
//! - Replies that wait are parked in [`Parked`]: a request whose command switched engines replies
//!   once switched ([`settle`], "switch before they reply"), `ui.waitRun` once the run is done or
//!   its time is up ([`tick`]), and a screenshot asked of the shell once it answers or the ask
//!   times out.
//! - Engine switching without waiting is one loop over the core's switchboard and engine worker
//!   for every shell ([`Driver`], [`step`]): what a shell does differently is only how a switch's
//!   waiting part starts ([`Driver::start`]).
//!
//! Lifted from `plumcraft-ui`'s `control.rs` and `engine_view.rs` (N1 slice 5), which had the
//! dispatcher, and from `plumcraft-ffi`'s core, which had a second copy of parts of it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use plumcraft_coordinator::Coordinator;
use plumcraft_graph::Graph;
use plumcraft_proto::catalogue::texts;
use plumcraft_proto::session::EngineState;
use plumcraft_session::Session;
use plumcraft_session::commands::Tone;
use serde_json::{Map, Value, json};

use crate::asks::{Answer, AskId, Asks, By, Kind};
use crate::invoke::{self, Cont, Host, PromptAnswer, Then};
use crate::link::Link;
use crate::prefs;
use crate::run_anim::RunAnim;
use crate::switchboard::{self, Next, Switchboard, Switching, Work, Worked};
use crate::worker::{Done, EngineWorker};

/// Sends one reply (`{"ok": true, "result"}` or `{"ok": false, "error"}`).
pub type Reply = Box<dyn FnOnce(Value) + Send>;

/// The longest `ui.waitRun` may wait: under the transport's 30 s reply timeout.
pub const WAIT_RUN_MAX_S: f64 = 29.0;

/// How long a shell has to answer a `screenshot` ask (`docs/shell-interface.md` 11.1).
pub const SHOT_WAIT: Duration = Duration::from_secs(2);

/// The window title's app name, unless an application names itself.
pub const APP_TITLE: &str = "PlumCraft";

/// The name of a graph with no file, until something names it.
pub const UNTITLED: &str = "Untitled";

pub fn ok(result: Value) -> Value {
    json!({"ok": true, "result": result})
}

pub fn err(message: impl Into<String>) -> Value {
    json!({"ok": false, "error": message.into()})
}

/// Only `allowed` keys (none for no params): an unknown field is an error, not ignored.
pub fn check_fields(params: &Value, allowed: &[&str]) -> Result<(), String> {
    match params {
        Value::Null => Ok(()),
        Value::Object(o) => match o.keys().find(|k| !allowed.contains(&k.as_str())) {
            Some(k) => Err(format!("unknown field {k:?} (allowed: {})", allowed.join(", "))),
            None => Ok(()),
        },
        _ => Err("params must be a JSON object".into()),
    }
}

/// Where a screenshot goes when the request names no path.
pub fn default_screenshot_path(token: u64) -> PathBuf {
    std::env::temp_dir().join("plumcraft-screenshots").join(format!("plumcraft-{}-{token}.png", std::process::id()))
}

/// The command table as `command.list` rows.
pub fn command_rows(session: &Session, facts: &crate::menu::Facts) -> Value {
    let mut rows: Vec<Value> = plumcraft_session::command_specs().iter().map(|c| plumcraft_session::commands::list_entry(session, c)).collect();
    // the canvas's commands (N2 slice 1), after the registry's, with what this shell can't do
    rows.extend(crate::canvas_cmds::commands().iter().map(|c| crate::canvas_cmds::list_entry(session, facts, c)));
    json!(rows)
}

/// What a shell lends the core for `ui.inspect`, read-only: the parts of the document it keeps
/// beside the session (until the core has a `Document` that owns them).
pub struct View<'a> {
    pub link: Option<&'a Link>,
    pub state: &'a EngineState,
    pub coordinator: Option<&'a Arc<Coordinator>>,
    pub switching: &'a Switching,
    pub last_error: Option<&'a str>,
    pub asks: &'a Asks<Cont>,
    /// The title's name for a graph with no file ([`UNTITLED`], a demo's, what it came from).
    pub document: &'a str,
    /// The title's app name ([`APP_TITLE`] unless an application names itself).
    pub app_title: &'a str,
    /// The run animation, where the shell shows one.
    pub run_anim: Option<&'a RunAnim>,
    /// The preferences, where the shell keeps them.
    pub prefs: Option<&'a prefs::Store>,
    /// The theme shown, where the shell themes.
    pub theme: Option<&'a str>,
}

/// A `ui.screenshot`, checked.
#[derive(Debug, Clone, PartialEq)]
pub struct ShotRequest {
    /// Where to write the PNG (none: [`default_screenshot_path`]).
    pub path: Option<PathBuf>,
    /// Draw offscreen rather than capture the window.
    pub offscreen: bool,
    /// A window the core keeps (`view.windows`' key: `editor:<node>`, `params`, `cache`) to draw
    /// instead of the canvas, for a shell that shows it in a window of its own (the Mac's); a
    /// shell that draws such windows inside its own (egui) shows them in its picture anyway.
    pub window: Option<String>,
}

/// What only a shell can do, for the control channel. Everything has a default but what the
/// core can't know: the state it lends ([`Shell::view`]), where its parked replies live, and how
/// to forget the last error.
pub trait Shell: Host {
    /// The document's state the shell keeps, for `ui.inspect`.
    fn view(&self) -> View<'_>;
    /// The control requests waiting for their reply.
    fn parked(&mut self) -> &mut Parked;
    /// Forget the status bar's last error (a command invoked starts afresh).
    fn clear_last_error(&mut self);
    /// Run `f` as a control request: what it starts on an engine must not make the shell wait
    /// (the egui shell is "in a frame" meanwhile). A shell that never waits runs `f` as it is.
    fn request<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R
    where
        Self: Sized,
    {
        f(self)
    }
    /// Did the last command run ask for an engine switch? (Recorded by the session until the
    /// shell begins it.)
    /// Who a `ui.invoke` handled now is for: a script's (the default: what it asks is left for
    /// the script to answer), or the person at the shell, when the shell sent it for its own
    /// menu or toolbar (the C ABI's `ui.invoke {by: "person"}`, N3 slice 4), so its asks are the
    /// shell's to show natively.
    fn invoke_by(&self) -> By {
        By::Script
    }
    fn engine_asked(&self) -> bool {
        self.session().engines.is_pending()
    }
    /// Show what the core asked, after a request may have asked something.
    fn presented(&mut self) {}
    /// Something changed that the shell may want to draw.
    fn redraw(&self) {}
    /// The shell's own part of `ui.inspect`: geometry, windows, what is on screen. Merged into
    /// the core's; a key here never names one of the core's.
    fn inspect_shell(&self) -> Map<String, Value> {
        Map::new()
    }
    /// `ui.screenshot`: reply once the picture is written. By default the shell is asked for it
    /// (the `screenshot` ask).
    fn screenshot(&mut self, shot: ShotRequest, reply: Reply)
    where
        Self: Sized,
    {
        ask_screenshot(self, shot, reply);
    }
    /// `ui.set {zoom?, pan?, fit?, palette?, focus?}`: the canvas's state as `ui.inspect`'s
    /// `canvas` reports it.
    fn ui_set(&mut self, _params: &Value) -> Result<Value, String> {
        Err(not_here("ui.set"))
    }
    /// `ui.audio {play?, loop?}`: the audio view's state.
    fn ui_audio(&mut self, _params: &Value) -> Result<Value, String> {
        Err(not_here("ui.audio"))
    }
    /// What the menu model needs from the shell (`menu.get`): its selection, its canvas's
    /// centre, its platform. By default: nothing selected, on a desktop.
    fn menu_facts(&self) -> crate::menu::Facts {
        crate::menu::Facts { live: self.view().link.is_some(), ..crate::menu::Facts::default() }
    }
    /// Where the menu bar's generation is kept (`menu.get`'s `gen`); none: no `gen`.
    fn menu_revision(&mut self) -> Option<&mut crate::menu::Revision> {
        None
    }
    /// The scene as it is now (`scene.get`, N2 slice 2); none: this shell publishes none.
    fn scene(&self) -> Option<crate::scene::Scene> {
        None
    }
    /// Where the published scene and its revisions are kept.
    fn scene_published(&mut self) -> Option<&mut crate::scene::Published> {
        None
    }
    /// The tokens the scene names, in the theme its scenes are built in (`theme.get`, N2 slice 4:
    /// the scene sends a token by name alone). By default the core's default theme, which a
    /// shell with no theme of its own builds its scenes in.
    fn theme_tokens(&self) -> Option<Value> {
        Some(theme_reply(self.view().theme.unwrap_or("paper"), &crate::theme::DEFAULT))
    }
    /// The document this shell shows, when it has one (the scene's geometry, the hot calls'
    /// snapshot: N2 slice 3).
    fn document_mut(&mut self) -> Option<&mut crate::document::Document> {
        None
    }
    /// `settings.get {fromDisk?}` (N4 slice 4): the settings as a model this shell renders
    /// ([`crate::settings::model`]). By default the document's shared look's, if it has one.
    fn settings_model(&mut self, params: &Value) -> Option<Result<Value, String>> {
        let d = self.document_mut()?;
        d.look.as_ref()?;
        Some(crate::settings::get(d, params))
    }
}

/// Publish the scene as it is now, and the hot calls' snapshot with it: the document's
/// [`crate::document::Document::publish_scene`] where the shell has a document, else its own
/// published scene. None: this shell publishes none.
fn publish<S: Shell>(s: &mut S) -> Option<u64> {
    let scene = s.scene()?;
    if let Some(d) = s.document_mut() {
        return Some(d.publish_scene(scene));
    }
    s.scene_published().map(|p| p.publish(scene))
}

/// The snapshot the `geometry.*` queries read, made from the scene published now.
fn snapshot<S: Shell>(s: &mut S, method: &str) -> Result<std::sync::Arc<crate::geometry::Snapshot>, Value> {
    publish(s).ok_or_else(|| err(not_here(method)))?;
    s.document_mut().map(|d| std::sync::Arc::clone(&d.snapshot)).ok_or_else(|| err(not_here(method)))
}

/// `theme.get`'s reply: `{shell, name, tokens: {name: [r, g, b, a]}}`, every token a scene may
/// name (`crate::theme::Tokens::scene_tokens`).
pub fn theme_reply(name: &str, tokens: &crate::theme::Tokens) -> Value {
    json!({"shell": crate::scene::SHELL, "name": name, "tokens": tokens.scene_tokens()})
}

/// `ui.gesture` (N2 slice 4, `docs/shell-interface.md` 6.3 and 8.13): hit-test the gesture
/// against the scene published now, decide what it means ([`crate::gesture::plan`]), and run
/// it: `{commands: [{command, params, ok, result | error}], hit?, drag?, update?, context?,
/// menu?, cancelled?}`. Every command runs as any other does (its error is kept for the status
/// bar); a later one runs even when an earlier failed.
pub fn gesture<S: Shell>(s: &mut S, params: &Value) -> Value {
    let req = match crate::gesture::parse(params) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let snap = match snapshot(s, "ui.gesture") {
        Ok(snap) => snap,
        Err(e) => return e,
    };
    let plan = {
        let Some(d) = s.document_mut() else { return err(not_here("ui.gesture")) };
        let facts = crate::gesture::Facts { graph: d.session.graph(), selection: d.selection.get(), drag: &d.drag };
        match crate::gesture::plan(&facts, &snap, &req) {
            Ok(p) => p,
            Err(e) => return err(e),
        }
    };
    let mut commands = Vec::with_capacity(plan.commands.len());
    for (command, p) in plan.commands {
        // what asks first (the footer's Save Result…, a path row's Browse: [`asks_first`]) is invoked, as its menu item
        // is: the core asks where, for whoever made the gesture (the person at a shell's canvas,
        // a script over the port)
        if asks_first(command, &p) {
            let by = s.invoke_by();
            invoke::invoke_with(s, command, &p, by);
            s.presented();
            commands.push(json!({"command": command, "params": p, "ok": true, "result": {"invoked": command}}));
            continue;
        }
        let r = s.run(command, &p);
        commands.push(match r {
            Ok(v) => json!({"command": command, "params": p, "ok": true, "result": v}),
            Err(e) => json!({"command": command, "params": p, "ok": false, "error": e}),
        });
    }
    let mut out = plan.reply;
    out.insert("commands".into(), Value::Array(commands));
    if let Some(cx) = plan.menu {
        let facts = s.menu_facts();
        match crate::menu::get(s.session(), &facts, &json!({"context": cx}), None) {
            Ok(m) => {
                out.insert("menu".into(), m);
            }
            Err(e) => {
                out.insert("menuError".into(), json!(e));
            }
        }
    }
    s.redraw();
    ok(Value::Object(out))
}

/// A command that asks first, so it is invoked (as `ui.invoke` does) wherever it comes from: a
/// gesture, `command.run`, a menu item run as it is. The result node's Save Result…
/// (`run.save {ask: true}`) and a path row's Browse (`row.browse`, B133).
pub fn asks_first(command: &str, p: &Value) -> bool {
    (command == "run.save" && p.get("ask").and_then(Value::as_bool) == Some(true)) || command == "row.browse"
}

/// A method this shell has nothing to answer with.
pub fn not_here(method: &str) -> String {
    format!("{method} isn't available from this shell")
}

/// A request waiting for an engine switch to be done.
struct SwitchWait {
    reply: Reply,
    /// `command.run`'s: the command's own result (for its `how`); else `ui.invoke`'s command.
    command: Option<Value>,
    invoked: Option<Value>,
    /// The switch's latest outcome.
    last: Option<Value>,
}

/// A `ui.waitRun` waiting for the current run to finish.
struct RunWait {
    deadline: Instant,
    reply: Reply,
}

/// A `ui.screenshot` waiting on the shell's `screenshot` ask.
struct ShotWait {
    ask: AskId,
    reply: Reply,
}

/// Control requests whose reply waits: on an engine switch, a run, the shell's screenshot.
#[derive(Default)]
pub struct Parked {
    switches: Vec<SwitchWait>,
    runs: Vec<RunWait>,
    shots: Vec<ShotWait>,
    next_token: u64,
}

impl std::fmt::Debug for Parked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Parked({} switches, {} runs, {} shots)", self.switches.len(), self.runs.len(), self.shots.len())
    }
}

impl Parked {
    /// Requests still waiting for their reply.
    pub fn pending(&self) -> usize {
        self.switches.len() + self.runs.len() + self.shots.len()
    }

    /// Requests waiting on time passing (a run, a screenshot): the shell should look again soon.
    pub fn ticking(&self) -> bool {
        !self.runs.is_empty() || !self.shots.is_empty()
    }

    /// A number for a screenshot that names no path.
    pub fn next_token(&mut self) -> u64 {
        self.next_token = self.next_token.wrapping_add(1);
        self.next_token
    }

    /// Answer every request still waiting with `why` (its document closed).
    pub fn close(&mut self, why: &str) {
        for w in std::mem::take(&mut self.switches) {
            (w.reply)(err(why));
        }
        for w in std::mem::take(&mut self.runs) {
            (w.reply)(err(why));
        }
        for w in std::mem::take(&mut self.shots) {
            (w.reply)(err(why));
        }
    }

    /// An engine operation's outcome: the waits keep the latest; the first error is the answer
    /// to a command that waits, as a command that waits for it gets it.
    pub fn outcome(&mut self, r: &Result<Value, String>) {
        match r {
            Ok(v) => self.switches.iter_mut().for_each(|w| w.last = Some(v.clone())),
            Err(e) => {
                let (failed, waiting): (Vec<SwitchWait>, Vec<SwitchWait>) = std::mem::take(&mut self.switches).into_iter().partition(|w| w.command.is_some());
                self.switches = waiting;
                for w in failed {
                    (w.reply)(err(e.clone()));
                }
            }
        }
    }
}

/// Is an engine switch asked for, or under way?
pub fn engine_busy(s: &impl Shell) -> bool {
    s.view().switching.busy() || s.session().engines.is_pending()
}

/// Handle one control request. `reply` is called exactly once, now or later.
pub fn handle<S: Shell>(s: &mut S, method: &str, params: &Value, reply: Reply) {
    match method {
        "ui.screenshot" => {
            if let Err(e) = check_fields(params, &["path", "offscreen", "window"]) {
                return reply(err(e));
            }
            let offscreen = match params.get("offscreen") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => return reply(err("offscreen must be a boolean")),
            };
            let path = match params.get("path") {
                None | Some(Value::Null) => None,
                Some(Value::String(p)) if !p.trim().is_empty() => Some(PathBuf::from(p)),
                Some(_) => return reply(err("path must be a file path")),
            };
            let window = match params.get("window") {
                None | Some(Value::Null) => None,
                Some(Value::String(w)) if !w.trim().is_empty() => Some(w.clone()),
                Some(_) => return reply(err("window must be a core window's key")),
            };
            s.screenshot(ShotRequest { path, offscreen, window }, reply);
        }
        "ui.waitRun" => {
            if let Err(e) = check_fields(params, &["timeout"]) {
                return reply(err(e));
            }
            let timeout = match params.get("timeout") {
                None | Some(Value::Null) => 25.0,
                Some(v) => match v.as_f64().filter(|t| t.is_finite() && *t >= 0.0) {
                    Some(t) => t.min(WAIT_RUN_MAX_S),
                    None => return reply(err("timeout must be a number of seconds")),
                },
            };
            let deadline = Instant::now() + Duration::from_secs_f64(timeout);
            s.parked().runs.push(RunWait { deadline, reply });
            s.redraw();
        }
        // a command that switches engines replies once it has switched, and the shell doesn't
        // wait for that (docs/control-protocol.md: engine.* "switch before they reply")
        "command.run" | "engine.execute" | "ui.invoke" | "ui.prompt" | "ask.answer" => {
            let v = s.request(|s| now(s, method, params));
            if s.engine_asked() && engine_busy(s) && v["ok"] == true {
                let (command, invoked) = match method {
                    "command.run" | "engine.execute" => (Some(v["result"].clone()), None),
                    _ => (None, v["result"].get("invoked").cloned()),
                };
                s.parked().switches.push(SwitchWait { reply, command, invoked, last: None });
                s.redraw();
            } else {
                reply(v);
            }
        }
        _ => {
            let v = now(s, method, params);
            reply(v);
        }
    }
}

/// Handle a request that replies at once (every method but `ui.screenshot` and `ui.waitRun`).
pub fn now<S: Shell>(s: &mut S, method: &str, params: &Value) -> Value {
    match method {
        "command.run" | "engine.execute" => {
            if let Err(e) = check_fields(params, &["command", "params"]) {
                return err(e);
            }
            let Some(command) = params.get("command").and_then(Value::as_str) else {
                return err("params.command must be a command id");
            };
            let p = params.get("params").cloned().unwrap_or(Value::Null);
            // a command that asks first ([`asks_first`]: the result node's Save Result…, its menu
            // item's `run.save {…, ask: true}`) is invoked, as `ui.invoke` does: a shell that
            // runs its menu items with `command.run` gets the asks, not a save with no path
            if asks_first(command, &p) {
                let by = s.invoke_by();
                invoke::invoke_with(s, command, &p, by);
                s.presented();
                s.redraw();
                return ok(json!({"invoked": command}));
            }
            let r = s.run(command, &p);
            s.redraw();
            match r {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }
        "command.list" | "engine.commands" => ok(command_rows(s.session(), &s.menu_facts())),
        "graph.get" => ok(serde_json::to_value(s.session().graph()).unwrap_or(Value::Null)),
        "ui.inspect" => ok(inspect(s)),
        "ui.audio" => match s.ui_audio(params) {
            Ok(v) => {
                s.redraw();
                ok(v)
            }
            Err(e) => err(e),
        },
        "ui.invoke" => {
            if let Err(e) = check_fields(params, &["command", "params"]) {
                return err(e);
            }
            let Some(id) = params.get("command").and_then(Value::as_str) else { return err("command must be a command id") };
            let cparams = params.get("params").cloned().unwrap_or(Value::Null);
            if !(cparams.is_null() || cparams.is_object()) {
                return err("params must be an object");
            }
            let (id, enabled) = match (plumcraft_session::find(id), crate::canvas_cmds::find(id)) {
                (Some(spec), _) => (spec.id, (spec.enabled)(s.session())),
                (None, Some(c)) => {
                    (c.id, crate::canvas_cmds::shell_refuses(&s.menu_facts(), c.id).map_or_else(|| (c.enabled)(s.session()), |why| Err(why.to_owned())))
                }
                (None, None) => return err(format!("unknown command {:?}", id.chars().take(64).collect::<String>())),
            };
            if let Err(why) = enabled {
                return err(format!("{id} is not available: {why}"));
            }
            s.clear_last_error();
            // what it asks (a file, a prompt) is the script's to answer, or the person's when
            // the shell invoked it for them: the reply comes at once, with the ask in `asks`
            // (ask.answer)
            let by = s.invoke_by();
            invoke::invoke_with(s, id, &cparams, by);
            s.presented();
            s.redraw();
            let mut out = inspect(s);
            out["invoked"] = json!(id);
            ok(out)
        }
        "ui.prompt" => {
            if let Err(e) = check_fields(params, &["answer"]) {
                return err(e);
            }
            let answer = match params.get("answer").and_then(Value::as_str) {
                Some("discard") => PromptAnswer::Discard,
                Some("save") => PromptAnswer::Save,
                Some("cancel") => PromptAnswer::Cancel,
                Some("trust") => PromptAnswer::Trust,
                _ => return err("answer must be \"save\", \"discard\", \"trust\" or \"cancel\""),
            };
            let answered = invoke::answer_prompt(s, answer);
            s.presented();
            if !answered {
                return err("no prompt takes that answer");
            }
            s.redraw();
            ok(inspect(s))
        }
        "ask.answer" => {
            if let Err(e) = check_fields(params, &["ask", "answer", "cancel"]) {
                return err(e);
            }
            let Some(id) = params.get("ask").and_then(Value::as_u64) else { return err("ask must be an ask's id (ui.inspect's asks)") };
            let answer = match (params.get("cancel"), params.get("answer")) {
                (Some(Value::Bool(true)), None) => None,
                (None | Some(Value::Bool(false)), Some(answer)) => Some(answer),
                (Some(Value::Bool(_)), Some(_)) => return err("give answer or cancel: true, not both"),
                (Some(_), _) => return err("cancel must be true"),
                (None, None) => return err("answer is required (or cancel: true)"),
            };
            let r = if s.view().asks.get(id).is_some_and(|a| a.kind == Kind::Screenshot) {
                answer_screenshot(s, id, answer)
            } else {
                match answer {
                    Some(a) => invoke::answer(s, id, a),
                    None => invoke::cancel(s, id),
                }
            };
            s.presented();
            s.redraw();
            match r {
                Ok(()) => ok(inspect(s)),
                Err(e) => err(e),
            }
        }
        // the menu model (N1 slice 8): the bar, or a context's menu
        "menu.get" => {
            let facts = s.menu_facts();
            let model = crate::menu::get(s.session(), &facts, params, None);
            match model {
                Ok(mut v) => {
                    if v.get("menus").is_some()
                        && let Some(r) = s.menu_revision()
                    {
                        v["gen"] = json!(r.note(&v["menus"]));
                    }
                    ok(v)
                }
                Err(e) => err(e),
            }
        }
        // the scene (N2 slice 2): everything, or what changed since a generation
        "scene.get" => {
            let since = match crate::scene::parse_since(params) {
                Ok(s) => s,
                Err(e) => return err(e),
            };
            if publish(s).is_none() {
                return err(not_here("scene.get"));
            }
            match s.scene_published() {
                Some(p) => {
                    p.watched = true;
                    ok(p.get(since))
                }
                None => err(not_here("scene.get")),
            }
        }
        // the geometry's queries (N2 slice 3), what the C ABI's hot calls answer typed: what is
        // under a point, where a dragged wire may land, the paths of wires on faces being moved
        "geometry.hit" => {
            let (x, y, slop) = match crate::geometry::parse_point(params, &[]) {
                Ok(p) => p,
                Err(e) => return err(e),
            };
            match snapshot(s, method) {
                Ok(snap) => ok(snap.hit_json(&snap.hit(x, y, slop))),
                Err(e) => e,
            }
        }
        "geometry.wireTarget" => {
            let (x, y, slop) = match crate::geometry::parse_point(params, &["from"]) {
                Ok(p) => p,
                Err(e) => return err(e),
            };
            let snap = match snapshot(s, method) {
                Ok(snap) => snap,
                Err(e) => return e,
            };
            match crate::geometry::parse_from(&snap, params) {
                Ok((face, output, pin)) => ok(snap.hit_json(&snap.wire_target(face, output, pin, x, y, slop))),
                Err(e) => err(e),
            }
        }
        "geometry.wirePaths" => {
            let snap = match snapshot(s, method) {
                Ok(snap) => snap,
                Err(e) => return e,
            };
            match crate::geometry::parse_paths(&snap, params) {
                Ok((faces, dx, dy)) => ok(snap.wire_paths_json(&faces, dx, dy)),
                Err(e) => err(e),
            }
        }
        // the drag under way with the pointer at `at` (N5 slice 4): `plum_gesture_move`'s typed
        // answer as JSON, for a driver over this port (the conformance suite's `hot` steps);
        // nothing changes
        "geometry.dragMove" => {
            let (x, y, slop) = match crate::geometry::parse_point(params, &[]) {
                Ok(p) => p,
                Err(e) => return err(e),
            };
            if !(x.is_finite() && y.is_finite()) {
                return err("at must be [x, y] numbers");
            }
            let snap = match snapshot(s, method) {
                Ok(snap) => snap,
                Err(e) => return e,
            };
            let Some(drag) = s.document_mut().and_then(|d| d.drag.get()) else {
                return err("no drag under way (begin one first)");
            };
            let slop = if slop.is_nan() { 0.0 } else { slop.clamp(0.0, 1.0e4) };
            let at = crate::plain::Point::new(crate::wire_path::coord(x), crate::wire_path::coord(y));
            ok(crate::gesture::typed_json(&drag.update(&snap, at, slop)))
        }
        // where the core has something (N2 slice 4): a face, its pin, a wire's middle, a frame's
        // tab, at the geometry it has, so a script (the conformance suite) aims a gesture at what
        // any shell measured
        "geometry.locate" => {
            let snap = match snapshot(s, method) {
                Ok(snap) => snap,
                Err(e) => return e,
            };
            let measured = s.document_mut().map(|d| d.geometry.clone()).unwrap_or_default();
            match crate::geometry::locate(&snap, &measured, params) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }
        // a recognised gesture (N2 slice 4): the core decides what it means and runs it
        "ui.gesture" => gesture(s, params),
        // what the node palette offers (N3 slice 1): for a dropped wire, only kinds that take it
        "palette.items" => match s.document_mut() {
            Some(d) => match crate::palette::items(
                d.session.graph(),
                d.session.catalog(),
                &plumcraft_session::engine_cmds::startable_defs(&d.session),
                d.palette.as_ref(),
                params,
            ) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            },
            None => err(not_here(method)),
        },
        // the parameters inspector's rows (N4 slice 3): every input of a node, shown or not
        "parameters.get" => match s.document_mut() {
            Some(d) => {
                let texts = std::collections::HashMap::new();
                let tokens = crate::theme::DEFAULT;
                let inputs = d.scene_inputs(&tokens, None, &texts, &[]);
                match crate::windows::parameters_json(&d.session, &inputs, params) {
                    Ok(v) => ok(v),
                    Err(e) => err(e),
                }
            }
            None => err(not_here(method)),
        },
        // a pop-out's value and its viewer's state (N4 slice 3)
        "popout.get" => match crate::windows::popout_json(s.session(), params) {
            Ok(v) => ok(v),
            Err(e) => err(e),
        },
        // the status bar's messages in full, for the Messages window (N4 slice 5)
        "messages.get" => match s.document_mut() {
            Some(d) => match crate::windows::messages_json(&d.session, d.last_error.as_deref(), params) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            },
            None => err(not_here(method)),
        },
        // the Cache Report, the graph's Parameters window and a value editor, as models a shell
        // draws (N4 slice 6)
        "report.get" => match crate::reports::get(s.session(), params) {
            Ok(v) => ok(v),
            Err(e) => err(e),
        },
        // Compare's two sides, A and B (N4 slice 6)
        "compare.get" => match crate::windows::compare_json(s.session(), params) {
            Ok(v) => ok(v),
            Err(e) => err(e),
        },
        // a page of what a pop-out's viewer shows, read on the media service's workers (N4 slice 5)
        "viewer.data" => match s.document_mut() {
            Some(d) => match crate::viewers::data(&d.session, &d.media_service, params) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            },
            None => err(not_here(method)),
        },
        // the colours the scene names by token (N2 slice 4, question 13)
        "theme.get" => match s.theme_tokens() {
            Some(v) => ok(v),
            None => err(not_here("theme.get")),
        },
        // autosave (N4 slice 4): on or off, a file to save to, its state and what is on disk
        "document.autosave" => match s.document_mut() {
            Some(d) => match crate::autosave::control(d, params) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            },
            None => err(not_here(method)),
        },
        // the settings as a model the shell renders (N4 slice 4)
        "settings.get" => match s.settings_model(params) {
            Some(Ok(v)) => ok(v),
            Some(Err(e)) => err(e),
            None => err(not_here(method)),
        },
        // the node icons' SVGs, for a shell that draws them itself (N4 slice 2)
        "icon.get" => match crate::icons::reply(params) {
            Ok(v) => ok(v),
            Err(e) => err(e),
        },
        // the log (N1 slice 8): read it, or add the shell's own entry
        "log.tail" => match crate::log::tail_request(params) {
            Ok(v) => ok(v),
            Err(e) => err(e),
        },
        "log.write" => match crate::log::write_request(params, Some(s.session().doc_id())) {
            Ok(v) => ok(v),
            Err(e) => err(e),
        },
        "ui.set" => match s.ui_set(params) {
            Ok(v) => {
                s.redraw();
                ok(v)
            }
            Err(e) => err(e),
        },
        other => err(format!("unknown method {other:?}")),
    }
}

/// `ui.inspect`: the core's parts, and the shell's block merged in.
pub fn inspect<S: Shell>(s: &S) -> Value {
    let mut m = inspect_core(s);
    m.extend(s.inspect_shell());
    Value::Object(m)
}

/// `ui.inspect`'s parts the core owns: the document, its engine, its run, its asks, and the run
/// animation and preferences where the shell has them.
pub fn inspect_core<S: Shell>(s: &S) -> Map<String, Value> {
    let v = s.view();
    let session = s.session();
    let g = session.graph();
    let catalog = session.catalog();
    let nodes: Vec<Value> = g
        .nodes
        .iter()
        .map(|(id, n)| json!({"id": id, "kind": n.kind, "title": plumcraft_session::node_title(g, catalog, id), "pos": n.pos, "view": n.view}))
        .collect();
    let mut m = Map::new();
    let mut put = |k: &str, v: Value| {
        m.insert(k.to_owned(), v);
    };
    put("title", json!(window_title(session, &v)));
    put("document", json!(document_name(session, v.document)));
    put("path", json!(session.path()));
    put("dirty", json!(session.is_dirty()));
    put("undo", json!(session.undo_label()));
    put("redo", json!(session.redo_label()));
    put("prompt", invoke::open_prompt(v.asks).map_or(Value::Null, |(_, p)| json!({"kind": p.kind(), "text": p.text(), "answers": p.answers()})));
    put("asks", v.asks.inspect());
    put("closing", json!(s.closing()));
    put("graph", json!({"dirty": session.is_dirty(), "nodes": nodes, "wires": g.wires.len(), "generation": session.generation()}));
    put("engine", engine_inspect(session, &v));
    put("banner", json!(banner_text(session, v.state)));
    put("engineStatus", engine_status(session, v.state).map_or(Value::Null, |(text, tone)| json!({"text": text, "tone": tone.name()})));
    put("about", json!(session.views.about.then_some(&session.about)));
    put("catalog", json!({"nodes": catalog.nodes.len(), "engine": catalog.engine.name, "version": catalog.engine.version}));
    put("lastError", json!(v.last_error));
    put("run", session.run.summary(Instant::now()));
    if let Some(a) = v.run_anim {
        put("runAnim", run_anim_inspect(session, a));
    }
    put("theme", json!(v.theme));
    put("prefs", v.prefs.map_or(Value::Null, |p| json!({"path": p.path(), "theme": p.prefs().theme, "dirs": p.prefs().dirs})));
    put("frames", frames_inspect(g));
    // the selected nodes (the selected collapsed groups are the shell's `groups.selected`)
    put("selection", json!(s.selection().0));
    put("docs", session.docs.inspect());
    put("save", session.save.summary());
    // the watched value as the core has it (N3 slice 1); a shell with a Watched panel of its own
    // adds what its panel shows (the egui editor's `watched`)
    put("watched", crate::values::watched_json(&session.run));
    m
}

/// `ui.inspect`'s `engine`: the switchboard's, pending while a switch is asked for or under way,
/// and the label saying "connecting" meanwhile.
pub fn engine_inspect(session: &Session, v: &View<'_>) -> Value {
    let mut out = switchboard::inspect(session, v.link, v.state, v.coordinator);
    out["pending"] = json!(v.switching.busy() || session.engines.is_pending());
    if v.switching.busy() {
        out["label"] = json!(switchboard::engine_label_connecting(session, v.link, v.state, true));
    }
    out
}

/// The run animation: whether the colours are on, the animation speed, whether a replay is
/// showing, how strongly it shows, and each node of the run as shown ([`crate::run_anim`]).
pub fn run_anim_inspect(session: &Session, a: &RunAnim) -> Value {
    let now = Instant::now();
    let shown: Map<String, Value> = session.run.needed.iter().filter_map(|id| a.shown(id, now).map(|s| (id.clone(), json!(s.name())))).collect();
    json!({"colors": !session.views.run_colors_off, "animMs": session.views.anim_ms(), "replay": a.is_replay(), "generation": a.generation(),
           "strength": a.strength(now), "shown": shown})
}

/// `ui.inspect`'s `frames`: each frame, and the nodes and groups it holds.
pub fn frames_inspect(graph: &Graph) -> Value {
    let list: Vec<Value> = graph
        .frames
        .iter()
        .map(|(id, f)| {
            let held = graph.frame_held(id);
            json!({"id": id, "title": f.title, "pos": f.pos, "size": f.size, "color": f.color, "nodes": held.nodes, "groups": held.groups})
        })
        .collect();
    Value::Array(list)
}

/// The window title's name: the workflow's file name, or `fallback` without one.
pub fn document_name(session: &Session, fallback: &str) -> String {
    session.path().and_then(std::path::Path::file_name).map_or_else(|| fallback.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// The window title: the workflow's name and a dot while it has unsaved changes, the app, and
/// the engine.
pub fn window_title(session: &Session, v: &View<'_>) -> String {
    let dot = if session.is_dirty() { " •" } else { "" };
    let name = document_name(session, v.document);
    match switchboard::engine_title_connecting(session, v.link.is_some(), v.switching.busy()) {
        Some(engine) => format!("{name}{dot} — {} ({engine})", v.app_title),
        None => format!("{name}{dot} — {}", v.app_title),
    }
}

/// What's wrong with the engine and how to fix it, when something is.
pub fn banner_text(session: &Session, state: &EngineState) -> Option<String> {
    let with_hint = |what: String, hint: &str| if hint.is_empty() { what } else { format!("{what}. Fix: {hint}") };
    match state {
        EngineState::EngineDown { reason, hint, stderr_tail, .. } => {
            let tail = stderr_tail.as_deref().filter(|t| !t.is_empty()).map(|t| format!("\n{t}")).unwrap_or_default();
            Some(format!("{}{tail}", with_hint(format!("The engine is down: {reason}"), hint)))
        }
        EngineState::Unreachable { url, reason, hint, .. } => Some(with_hint(format!("The engine at {url} can't be reached: {reason}"), hint)),
        EngineState::Unavailable { reason } => Some(format!("The engine is up but can't run: {reason}")),
        // connected, with something to say: what this graph lacks, and the engine's own notice
        _ => {
            let known = !session.catalog().nodes.is_empty();
            let missing = known.then(|| session.missing_engines()).flatten().map(|m| m.text);
            let notice = session.catalog().text(texts::ENGINE_NOTICE).filter(|n| !n.is_empty()).map(str::to_owned);
            match (missing, notice) {
                (Some(m), Some(n)) => Some(format!("{m}\n{n}")),
                (Some(m), None) => Some(m),
                (None, n) => n,
            }
        }
    }
}

/// The engine's status line (`texts::ENGINE_STATUS`, plumcraft #86) and its tone, while the
/// engine is connected and the text isn't empty.
pub fn engine_status(session: &Session, state: &EngineState) -> Option<(String, Tone)> {
    if !state.is_connected() {
        return None;
    }
    let c = session.catalog();
    let text = c.text(texts::ENGINE_STATUS).filter(|t| !t.trim().is_empty())?;
    Some((text.to_owned(), Tone::from_text(c.text(texts::ENGINE_STATUS_TONE).unwrap_or_default())))
}

/// One message in the status bar: what the editor tells the person, which the shell only shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Msg {
    pub tone: Tone,
    pub text: String,
    /// More detail, shown on hover and in the Messages window.
    pub detail: Option<String>,
}

impl Msg {
    pub fn new(tone: Tone, text: impl Into<String>) -> Msg {
        Msg { tone, text: text.into(), detail: None }
    }

    pub fn detail(mut self, d: impl Into<String>) -> Msg {
        let d = d.into();
        self.detail = (!d.is_empty()).then_some(d);
        self
    }
}

/// The status bar's messages: the run's errors, the application's segment (`app`: its text and
/// tone, where the shell shows one), a save's outcome, the last thing that went wrong.
pub fn status_messages(session: &Session, last_error: Option<&str>, app: Option<(&str, Tone)>) -> Vec<Msg> {
    let run = &session.run;
    let mut msgs = Vec::new();
    let errors = run.errors();
    if errors > 0 {
        let names: Vec<String> =
            run.nodes.iter().filter(|(_, n)| n.state == Some(plumcraft_proto::event::NodeState::Error)).map(|(id, _)| id.clone()).collect();
        let text = if errors == 1 { "1 error".to_owned() } else { format!("{errors} errors") };
        msgs.push(Msg::new(Tone::Error, text).detail(format!("in {}", names.join(", "))));
    }
    if let Some(e) = &run.run_error {
        msgs.push(Msg::new(Tone::Error, e.clone()));
    }
    if let Some((text, tone)) = app {
        msgs.push(Msg::new(tone, text));
    }
    if session.save.pending.is_none()
        && let Some((ask, reply)) = &session.save.last
    {
        msgs.push(match (&reply.error, reply.ok) {
            (None, true) => {
                let shown = reply.paths.first().map_or(ask.path.as_str(), String::as_str);
                let text = match (ask.sheet, reply.paths.len()) {
                    (true, _) => format!("saved a sheet of {} as {}", reply.items, short_path(shown)),
                    (false, 0 | 1) => format!("saved {}", short_path(shown)),
                    (false, n) => format!("saved {n} files as {}", short_path(&ask.path)),
                };
                // no word when a file carries no recipe: none is embedded by default (2026-11-22)
                let text = if reply.recipe { format!("{text}, with its recipe") } else { text };
                Msg::new(Tone::Info, text).detail(reply.paths.join("\n"))
            }
            (Some(e), _) => Msg::new(Tone::Error, format!("not saved: {}", e.message)).detail(ask.path.clone()),
            (None, false) => Msg::new(Tone::Error, "not saved").detail(ask.path.clone()),
        });
    }
    if let Some(err) = last_error {
        msgs.push(Msg::new(Tone::Error, err.to_owned()));
    }
    msgs
}

/// A path as the status bar shows it: its folder's name and its own.
pub fn short_path(path: &str) -> String {
    let p = std::path::Path::new(path);
    match (p.parent().and_then(std::path::Path::file_name), p.file_name()) {
        (Some(dir), Some(name)) => format!("{}/{}", dir.to_string_lossy(), name.to_string_lossy()),
        (None, Some(name)) => name.to_string_lossy().into_owned(),
        _ => path.to_owned(),
    }
}

/// `ui.screenshot` for a shell that draws its own pixels: the `screenshot` ask
/// (`{path, offscreen}`), answered `{path, width, height, source?}` or `{error}` within
/// [`SHOT_WAIT`]; the request replies then.
pub fn ask_screenshot<S: Shell>(s: &mut S, shot: ShotRequest, reply: Reply) {
    let token = s.parked().next_token();
    let path = shot.path.unwrap_or_else(|| default_screenshot_path(token));
    let mut params = json!({"path": path, "offscreen": shot.offscreen});
    if let Some(w) = shot.window {
        params["window"] = json!(w);
    }
    let (ask, _) = s.asks().raise_with(Kind::Screenshot, params, By::Script, Some(SHOT_WAIT), Cont { then: Then::Notice, remember: None });
    s.parked().shots.push(ShotWait { ask, reply });
    s.presented();
    s.redraw();
}

/// The shell's answer to a `screenshot` ask: the `ui.screenshot` waiting on it replies.
fn answer_screenshot<S: Shell>(s: &mut S, id: AskId, answer: Option<&Value>) -> Result<(), String> {
    let result = match answer {
        Some(a) => {
            let (_, _, parsed) = s.asks().answer(id, a).map_err(|r| r.to_string())?;
            match parsed {
                Answer::Shot(v) => ok(v),
                Answer::Failed(why) => err(why),
                _ => err("the shell's screenshot answer was not a picture"),
            }
        }
        None => {
            s.asks().cancel(id).map_err(|r| r.to_string())?;
            err("the shell didn't take the screenshot (cancelled)")
        }
    };
    let parked = s.parked();
    if let Some(i) = parked.shots.iter().position(|w| w.ask == id) {
        let w = parked.shots.remove(i);
        (w.reply)(result);
    }
    Ok(())
}

/// Answer what waited on time passing: `ui.waitRun`s whose run finished (or whose time ran out),
/// and screenshots whose ask ended unanswered (timed out, withdrawn). True while some still wait,
/// so the shell looks again soon.
pub fn tick<S: Shell>(s: &mut S) -> bool {
    if !s.parked().ticking() {
        return false;
    }
    let now = Instant::now();
    let busy = s.session().run.is_busy(now);
    let summary = (!busy).then(|| s.session().run.summary(now));
    let runs = std::mem::take(&mut s.parked().runs);
    let mut keep = Vec::new();
    for w in runs {
        match &summary {
            Some(v) => (w.reply)(ok(v.clone())),
            None if now >= w.deadline => (w.reply)(err("timeout: the run is still going")),
            None => keep.push(w),
        }
    }
    s.parked().runs = keep;
    let shots = std::mem::take(&mut s.parked().shots);
    let (open, gone): (Vec<ShotWait>, Vec<ShotWait>) = shots.into_iter().partition(|w| s.view().asks.get(w.ask).is_some());
    for w in gone {
        (w.reply)(err(format!("timeout: the shell didn't deliver the screenshot within {} s", SHOT_WAIT.as_secs())));
    }
    s.parked().shots = open;
    s.parked().ticking()
}

/// The engines are idle: answer the requests that waited for a switch.
pub fn settle<S: Shell>(s: &mut S) {
    if s.parked().switches.is_empty() || engine_busy(s) {
        return;
    }
    for w in std::mem::take(&mut s.parked().switches) {
        let v = match (&w.command, w.last) {
            (Some(command), Some(mut out)) => {
                if let (Some(how), Some(o)) = (command.get("how"), out.as_object_mut()) {
                    o.insert("how".into(), how.clone());
                }
                ok(out)
            }
            (Some(command), None) => ok(command.clone()),
            (None, _) => {
                let mut out = inspect(s);
                if let Some(id) = &w.invoked {
                    out["invoked"] = id.clone();
                }
                ok(out)
            }
        };
        (w.reply)(v);
    }
}

/// Engine switching for a shell: its engine parts lent to the switchboard, its engine worker,
/// and how a switch's waiting part starts.
pub trait Driver: Shell {
    /// The document's engine worker.
    fn worker(&mut self) -> &mut EngineWorker;
    fn switching(&mut self) -> &mut Switching;
    /// Lend the engine parts to a [`Switchboard`] for `f` (and apply what it changed).
    fn board<R>(&mut self, f: impl FnOnce(&mut Switchboard<'_>) -> R) -> R
    where
        Self: Sized;
    /// Start a switch's waiting part: on the worker (`None`; it comes back as
    /// [`Done::Switched`]), or in place when nothing waits (`Some`, finished next).
    fn start(&mut self, work: Work) -> Option<Worked>;
    /// What the worker carried for a switch, given back.
    fn take_back(&mut self, _back: Box<dyn std::any::Any + Send>) {}
    /// An operation's outcome, for what the shell shows of it (a window that attached).
    fn outcome(&mut self, _r: &Result<Value, String>) {}
}

/// Engine switching without waiting: take what the worker brought back (a refused run, a
/// switch's end), finish it, begin what the session asked for next, retire the old engines on
/// the worker, and answer the requests waiting for it once nothing is left to do. Outcomes go to
/// `out`.
pub fn step<D: Driver>(d: &mut D, out: &mut Vec<Result<Value, String>>) {
    for done in d.worker().drain() {
        match done {
            Done::RunFailed { run_id, generation, why } => {
                // only the latest run fails: a superseded one's refusal is old news
                let run = &d.session().run;
                if run.run_id.as_deref() == Some(run_id.as_str()) && run.generation == generation {
                    d.session_mut().run.fail(&format!("run: {why}"));
                }
            }
            Done::Switched { worked, back } => {
                d.take_back(back);
                let next = d.board(|b| b.finish(*worked));
                go(d, next, out);
            }
        }
    }
    while !d.switching().busy() && d.session().engines.is_pending() {
        let Some(next) = d.board(|b| b.begin()) else { break };
        go(d, next, out);
    }
    // an install under way (B71: every shell follows it here, not the egui shell alone)
    if d.session().engines.installing.is_some() {
        d.board(|b| b.poll_install());
    }
    let retired = d.switching().take_retired();
    d.worker().retire(retired);
    settle(d);
}

/// Carry an operation on until it is done or waits on the worker.
fn go<D: Driver>(d: &mut D, mut next: Next, out: &mut Vec<Result<Value, String>>) {
    loop {
        match next {
            Next::Done(r) => {
                d.parked().outcome(&r);
                d.outcome(&r);
                out.push(r);
                return;
            }
            Next::Work(w) => match d.start(w) {
                Some(worked) => next = d.board(|b| b.finish(worked)),
                None => return,
            },
        }
    }
}

/// Perform what the session asked of the engines, waiting for it here (a caller that may wait,
/// at most `limit`): the last outcome, or the first error. The engines still start on the worker.
pub fn engines_now<D: Driver>(d: &mut D, limit: Duration) -> Option<Result<Value, String>> {
    let deadline = Instant::now() + limit;
    let mut done = None;
    loop {
        let mut out = Vec::new();
        step(d, &mut out);
        for r in out {
            if r.is_err() {
                return Some(r);
            }
            done = Some(r);
        }
        if !d.switching().busy() {
            return done;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || !d.worker().wait(left) {
            return Some(Err("the engine didn't start in time".into()));
        }
    }
}

/// Wait (at most `timeout`) until no switch is asked for or under way and the engine worker has
/// sent everything given it (runs, cancels): whether it came to that. For tests and scripts.
pub fn wait_engines<D: Driver>(d: &mut D, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let mut out = Vec::new();
        step(d, &mut out);
        if !engine_busy(d) && d.worker().idle() {
            return true;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || !d.worker().wait(left) {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::mpsc::channel;

    /// A shell with nothing of its own: a session, its asks, nothing connected.
    struct Bare {
        session: Session,
        asks: Asks<Cont>,
        parked: Parked,
        last_error: Option<String>,
        state: EngineState,
        switching: Switching,
    }

    impl Bare {
        fn new() -> Bare {
            Bare {
                session: Session::new(plumcraft_text::engine().schema()),
                asks: Asks::default(),
                parked: Parked::default(),
                last_error: None,
                state: EngineState::Off,
                switching: Switching::default(),
            }
        }
    }

    impl Host for Bare {
        fn session(&self) -> &Session {
            &self.session
        }
        fn session_mut(&mut self) -> &mut Session {
            &mut self.session
        }
        fn asks(&mut self) -> &mut Asks<Cont> {
            &mut self.asks
        }
        fn run(&mut self, id: &str, params: &Value) -> Result<Value, String> {
            self.session.execute(id, params).map_err(|e| e.to_string())
        }
        fn say(&mut self, text: String) {
            self.last_error = Some(text);
        }
    }

    impl Shell for Bare {
        fn view(&self) -> View<'_> {
            View {
                link: None,
                state: &self.state,
                coordinator: None,
                switching: &self.switching,
                last_error: self.last_error.as_deref(),
                asks: &self.asks,
                document: UNTITLED,
                app_title: APP_TITLE,
                run_anim: None,
                prefs: None,
                theme: None,
            }
        }
        fn parked(&mut self) -> &mut Parked {
            &mut self.parked
        }
        fn clear_last_error(&mut self) {
            self.last_error = None;
        }
    }

    /// Send a request; its reply, if it came at once.
    fn send(s: &mut Bare, method: &str, params: Value) -> Option<Value> {
        let got = Arc::new(Mutex::new(None));
        let put = Arc::clone(&got);
        handle(s, method, &params, Box::new(move |v| *put.lock().unwrap() = Some(v)));
        got.lock().unwrap().take()
    }

    #[test]
    fn a_shell_with_no_geometry_still_inspects_the_core_parts() {
        let mut s = Bare::new();
        let r = send(&mut s, "command.run", json!({"command": "node.add", "params": {"kind": "text.upper"}})).unwrap();
        assert_eq!(r["ok"], true, "{r}");
        let v = send(&mut s, "ui.inspect", json!({})).unwrap();
        let i = &v["result"];
        assert_eq!(i["graph"]["nodes"].as_array().map(Vec::len), Some(1), "{v}");
        assert_eq!(i["title"], "Untitled • — PlumCraft", "{v}");
        for key in ["document", "dirty", "undo", "asks", "prompt", "engine", "catalog", "run", "docs", "save", "lastError", "frames", "closing"] {
            assert!(i.get(key).is_some(), "ui.inspect has {key}: {v}");
        }
        assert!(i.get("canvas").is_none(), "no geometry from a shell that reports none");
        // the shell's own methods say they aren't here
        for m in ["ui.set", "ui.audio"] {
            let r = send(&mut s, m, json!({})).unwrap();
            assert_eq!(r["error"], not_here(m), "{r}");
        }
        let rows = send(&mut s, "command.list", Value::Null).unwrap();
        assert!(rows["result"].as_array().is_some_and(|r| r.iter().any(|c| c["id"] == "node.add")));
    }

    #[test]
    fn a_screenshot_is_asked_of_the_shell_and_replies_with_its_answer_or_times_out() {
        let mut s = Bare::new();
        let (tx, rx) = channel();
        let tx2 = tx.clone();
        handle(&mut s, "ui.screenshot", &json!({"path": "/tmp/x.png"}), Box::new(move |v| tx.send(v).unwrap()));
        assert!(rx.try_recv().is_err(), "it waits for the shell");
        let asks = send(&mut s, "ui.inspect", json!({})).unwrap()["result"]["asks"].clone();
        assert_eq!((asks[0]["kind"].as_str(), asks[0]["params"]["path"].as_str()), (Some("screenshot"), Some("/tmp/x.png")), "{asks}");
        let id = asks[0]["ask"].as_u64().unwrap();
        // hostile answers leave it open
        for bad in [json!({"path": 3}), json!({"path": "/tmp/x.png", "width": -1}), json!({"pixels": []}), json!("x"), json!({"path": "", "width": 1})] {
            let r = send(&mut s, "ask.answer", json!({"ask": id, "answer": bad})).unwrap();
            assert_eq!(r["ok"], false, "{bad}: {r}");
        }
        let r = send(&mut s, "ask.answer", json!({"ask": id, "answer": {"path": "/tmp/x.png", "width": 4, "height": 3}})).unwrap();
        assert_eq!(r["ok"], true, "{r}");
        let shot = rx.try_recv().unwrap();
        assert_eq!(shot, json!({"ok": true, "result": {"path": "/tmp/x.png", "width": 4, "height": 3, "source": "window"}}));
        // unanswered: it times out
        handle(&mut s, "ui.screenshot", &json!({}), Box::new(move |v| tx2.send(v).unwrap()));
        let id = s.asks.first(Kind::Screenshot).unwrap().id;
        s.asks.expire(Instant::now() + SHOT_WAIT + Duration::from_millis(1));
        assert!(!tick(&mut s), "nothing left waiting");
        let late = rx.try_recv().unwrap();
        assert!(late["error"].as_str().unwrap().contains("timeout"), "{late}");
        let r = send(&mut s, "ask.answer", json!({"ask": id, "answer": {"path": "/tmp/y.png", "width": 1, "height": 1}})).unwrap();
        assert!(r["error"].as_str().unwrap().contains("already timedOut") || r["error"].as_str().unwrap().contains("was already"), "{r}");
    }

    #[test]
    fn a_wait_for_the_run_replies_when_nothing_runs() {
        let mut s = Bare::new();
        let (tx, rx) = channel();
        handle(&mut s, "ui.waitRun", &json!({"timeout": 1}), Box::new(move |v| tx.send(v).unwrap()));
        assert!(!tick(&mut s));
        assert_eq!(rx.try_recv().unwrap()["ok"], true);
        assert_eq!(send(&mut s, "ui.waitRun", json!({"timeout": "soon"})).unwrap()["ok"], false);
    }

    #[test]
    fn hostile_requests_are_refused_and_never_panic() {
        let mut s = Bare::new();
        let junk = [
            Value::Null,
            json!(7),
            json!("x"),
            json!([1, 2]),
            json!({"command": 7}),
            json!({"command": "node.add", "params": "x"}),
            json!({"command": "x".repeat(100_000)}),
            json!({"ask": -1, "answer": {}}),
            json!({"ask": 1.5, "cancel": true}),
            json!({"ask": u64::MAX, "cancel": "yes"}),
            json!({"answer": {"deep": [[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]}}),
            json!({"path": 5, "offscreen": "no"}),
            json!({"timeout": -1}),
            json!({"timeout": 1e308}),
            json!({"zoom": "x", "pan": [1]}),
            json!({"\u{0}": 1}),
        ];
        let methods = [
            "command.run",
            "engine.execute",
            "command.list",
            "graph.get",
            "ui.inspect",
            "ui.invoke",
            "ui.prompt",
            "ask.answer",
            "ui.set",
            "ui.audio",
            "ui.screenshot",
            "ui.waitRun",
            "",
            "\u{202e}",
            "nope",
        ];
        for m in methods {
            for p in &junk {
                handle(&mut s, m, p, Box::new(|v| assert!(v.get("ok").is_some_and(Value::is_boolean), "{v}")));
            }
        }
        // whatever was parked replies, and the dispatcher still works
        s.asks.expire(Instant::now() + Duration::from_secs(60));
        let _ = tick(&mut s);
        assert_eq!(send(&mut s, "ui.inspect", Value::Null).unwrap()["ok"], true);
    }
}
