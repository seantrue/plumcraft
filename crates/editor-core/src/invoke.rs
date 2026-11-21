//! Running a command as its menu item or shortcut does: the invoke state machine
//! (`docs/shell-interface.md` 11.4, `docs/research/24-native-shells.md` 4.1).
//!
//! A command run from the UI needs things the command's params don't carry: a path from a file
//! dialog, an answer to the unsaved-changes prompt, a form's fields, the canvas selection. The
//! machine fills them in. Where it needs the platform it raises an ask ([`crate::asks`]) with a
//! continuation ([`Then`]) and returns; the answer, whenever it comes, carries the chain on:
//!
//! ```text
//! graph.new over unsaved changes → prompt unsavedChanges → "save" → file.save (no file yet)
//!     → graph.saveAs {path} → graph.new {discard: false}
//! ```
//!
//! The machine runs commands through its [`Host`]: the editor that holds the session, its
//! selection and canvas, which runs a command as `try_run` does (an engine switch included: a
//! shell that may not wait has it finish later, and a control reply waits for it). Nothing here
//! waits for anything.

use std::path::{Path, PathBuf};
use std::time::Duration;

use plumcraft_graph::GroupId;
use plumcraft_proto::catalogue::{PathHint, SaveFormat};
use plumcraft_proto::run::RunRequest;
use plumcraft_session::trust::Review;
use plumcraft_session::{Session, paths};
use serde_json::{Map, Value, json};

use crate::asks::{Answer, Ask, AskId, Asks, By, Kind};
use crate::prefs;

/// The workflow file extension.
pub const WORKFLOW_EXT: &str = "json";
/// How long Edit › Paste waits for the shell to read the clipboard: an empty clipboard answers
/// nothing.
pub const PASTE_WAIT: Duration = Duration::from_secs(5);

/// What a "Discard" in [`Prompt::Discard`] goes on to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterDiscard {
    /// `graph.new`.
    New,
    /// Ask for a workflow (`file.open`), then `graph.open` it.
    Open,
    /// Close the window (quit).
    Quit,
}

impl AfterDiscard {
    pub fn name(&self) -> &'static str {
        match self {
            AfterDiscard::New => "new",
            AfterDiscard::Open => "open",
            AfterDiscard::Quit => "quit",
        }
    }
}

/// A question the window is asking: a `prompt` ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    /// The graph has unsaved changes: discard them to go on with `then`?
    Discard { then: AfterDiscard },
    /// A workflow from elsewhere is about to run, and this is what it touches.
    Review(Box<Review>),
    /// A run would send these local files to an engine nobody here started: `(where to, file)`.
    Uploads(Vec<(String, String)>),
}

/// An answer to a [`Prompt`]: its buttons, and `ui.prompt`'s `answer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptAnswer {
    /// Drop the unsaved changes and go on.
    Discard,
    /// Save first (the save dialog for a graph with no file), then go on once saved.
    Save,
    /// Do nothing.
    Cancel,
    /// Trust the workflow; its held run is dropped ([`Prompt::Review`]).
    Trust,
}

impl PromptAnswer {
    pub fn name(self) -> &'static str {
        match self {
            PromptAnswer::Discard => "discard",
            PromptAnswer::Save => "save",
            PromptAnswer::Cancel => "cancel",
            PromptAnswer::Trust => "trust",
        }
    }

    pub fn from_name(name: &str) -> Option<PromptAnswer> {
        [PromptAnswer::Discard, PromptAnswer::Save, PromptAnswer::Cancel, PromptAnswer::Trust].into_iter().find(|a| a.name() == name)
    }
}

impl Prompt {
    /// The prompt's id for the control channel (`ui.inspect`'s `prompt.kind`).
    pub fn kind(&self) -> &'static str {
        match self {
            Prompt::Discard { .. } => "unsavedChanges",
            Prompt::Review(_) => "reviewWorkflow",
            Prompt::Uploads(_) => "allowUploads",
        }
    }

    pub fn text(&self) -> &'static str {
        match self {
            Prompt::Discard { then: AfterDiscard::New } => "The graph has unsaved changes. Discard them and start a new graph?",
            Prompt::Discard { then: AfterDiscard::Open } => "The graph has unsaved changes. Discard them and open another workflow?",
            Prompt::Uploads(_) => "This run would send these files from your computer to an engine you attached to.",
            Prompt::Review(_) => "This workflow came from elsewhere, and it runs with your permissions. Check what it touches before you run it.",
            Prompt::Discard { then: AfterDiscard::Quit } => "The graph has unsaved changes. Discard them and quit?",
        }
    }

    /// Its window's heading.
    pub fn title(&self) -> &'static str {
        match self {
            Prompt::Discard { .. } => "Unsaved changes",
            Prompt::Review(_) => "Review before running",
            Prompt::Uploads(_) => "Send files to this engine?",
        }
    }

    /// What it may be answered.
    pub fn answers(&self) -> &'static [&'static str] {
        match self {
            Prompt::Discard { .. } => &["save", "discard", "cancel"],
            Prompt::Review(_) | Prompt::Uploads(_) => &["trust", "cancel"],
        }
    }

    /// The `prompt` ask's params: `{kind, title, text, answers, then?, detail?}`.
    pub fn params(&self) -> Value {
        let mut p = json!({"kind": self.kind(), "title": self.title(), "text": self.text(), "answers": self.answers()});
        match self {
            Prompt::Discard { then } => p["then"] = json!(then.name()),
            Prompt::Review(r) => p["detail"] = serde_json::to_value(r).unwrap_or(Value::Null),
            Prompt::Uploads(files) => p["detail"] = Value::Array(files.iter().map(|(to, file)| json!({"to": to, "file": file})).collect()),
        }
        p
    }
}

/// What to do with an ask's answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Then {
    /// A [`Prompt`] answered.
    Prompt(Prompt),
    /// `graph.open {path, discard}`.
    OpenWorkflow { discard: bool },
    /// `graph.saveAs {path}`, then go on with `after` once nothing is unsaved.
    SaveWorkflow { after: Option<AfterDiscard> },
    /// `graph.bundle {path}`.
    Bundle,
    /// `graph.insert` or `graph.insertGroup {path, pos}`, then select what came in.
    Insert { command: String, pos: Option<[f32; 2]> },
    /// `node.set` a path input to the path chosen, relative to `base` where it is inside it.
    Path { node: String, port: String, save: bool, base: Option<PathBuf> },
    /// `node.set` a `files` input to the paths chosen, one a line.
    Files { node: String, port: String, base: Option<PathBuf> },
    /// `value.open` with the app chosen (Open With › Choose an App…).
    App { params: Value },
    /// `run.save` to the path chosen (Save Result…).
    SaveResult { params: Value, base: Option<PathBuf> },
    /// `theme.import {path}`.
    ThemeImport,
    /// `theme.export {path}`.
    ThemeExport,
    /// `graph.saveCopy {path}` (Save a Copy…, N4 slice 4).
    SaveCopy,
    /// `graph.move {folder}` (Move…, N4 slice 4).
    Move,
    /// A command's form: run `command` with the fields.
    Form { command: String },
    /// `edit.paste` the clipboard's text.
    Paste,
    /// Nothing follows; a failure goes to the status bar.
    Notice,
}

/// An ask's continuation: what follows, and the preference key under which the folder of a
/// chosen file is remembered.
#[derive(Debug, Clone, PartialEq)]
pub struct Cont {
    pub then: Then,
    pub remember: Option<&'static str>,
}

impl Cont {
    fn new(then: Then) -> Self {
        Cont { then, remember: None }
    }
}

/// What the machine needs of the editor that hosts it. Everything but the first five has a
/// default for a host without a canvas (the C ABI's core, a headless session).
pub trait Host {
    fn session(&self) -> &Session;
    fn session_mut(&mut self) -> &mut Session;
    fn asks(&mut self) -> &mut Asks<Cont>;
    /// Run a command as the editor runs any (`try_run`: its error is kept for the status bar).
    fn run(&mut self, id: &str, params: &Value) -> Result<Value, String>;
    /// A word for the status bar.
    fn say(&mut self, text: String);
    /// Where the folder each kind of dialog last used is kept.
    fn prefs(&mut self) -> Option<&mut prefs::Store> {
        None
    }
    /// The selected nodes and collapsed groups.
    fn selection(&self) -> (Vec<String>, Vec<GroupId>) {
        (Vec::new(), Vec::new())
    }
    /// What Ungroup acts on.
    fn group_to_ungroup(&self) -> Option<GroupId> {
        None
    }
    /// The canvas centre in graph coordinates, where an insert lands.
    fn canvas_center(&self) -> Option<[f32; 2]> {
        None
    }
    /// The node sizes and pin positions the canvas measured (`graph.tidy`'s `sizes`, `pins`).
    fn measured(&self) -> (Map<String, Value>, Map<String, Value>) {
        (Map::new(), Map::new())
    }
    /// Fit the view to the graph.
    fn fit(&mut self) {}
    /// The sizes the core has for these nodes and collapsed groups (what the shell measured,
    /// `geometry.set`): Align, Distribute and Frame Selection's `sizes`. None: the session
    /// estimates each, as it does for a script.
    fn sizes(&self, _nodes: &[String], _groups: &[plumcraft_graph::GroupId]) -> std::collections::BTreeMap<String, [f32; 2]> {
        std::collections::BTreeMap::new()
    }
    /// Edit › Frame Selection: a frame around the selection, at the sizes the core has
    /// ([`Host::sizes`]; N2 slice 3, where it was the shell's).
    fn frame_selection(&mut self) {
        let (nodes, groups) = self.selection();
        if nodes.is_empty() && groups.is_empty() {
            self.say("select nodes to frame first".into());
            return;
        }
        let sizes = self.sizes(&nodes, &groups);
        let _ = self.run("frame.add", &json!({"nodes": nodes, "groups": groups, "sizes": sizes}));
    }
    /// Where a paste lands (the pointer, over the canvas).
    fn paste_pos(&self) -> Option<[f32; 2]> {
        None
    }
    /// Close the window (quit), unsaved changes dealt with.
    fn close(&mut self) {}
    /// The window may close already.
    fn closing(&self) -> bool {
        false
    }
    /// Open a window of the shell's own for `id` (Engine › Attach…, Install Engine…); false
    /// when it has none, and the command runs as it is.
    fn open_window(&mut self, _id: &str) -> bool {
        false
    }
    /// Allow these files to go to the attached engine.
    fn allow_uploads(&mut self, _files: &[String]) {}
    /// The run held back by [`Prompt::Uploads`], taken.
    fn take_held_run(&mut self) -> Option<Box<RunRequest>> {
        None
    }
    /// The theme in use, for an exported theme's file name.
    fn theme_name(&self) -> String {
        "theme".into()
    }
    /// Apps the session asked to start since the last call ([`route_launches`]).
    fn take_launches(&mut self) -> Vec<(PathBuf, Vec<String>, PathBuf)> {
        Vec::new()
    }
}

/// Apps the session asked to start (`program args… file`), waiting to become `open.external`
/// asks ([`route_launches`]).
pub type Launches = std::sync::Arc<std::sync::Mutex<Vec<(PathBuf, Vec<String>, PathBuf)>>>;

/// Route the session's app starts (`doc.open`, `value.open`: `/usr/bin/open`, `xdg-open`, a
/// craft's app) through `open.external` asks, so the shell starts them (decision 19 of
/// `docs/shell-interface.md`): the session records them, and [`raise_launches`] asks. Only a
/// session that still spawns apps itself is routed; one given a launcher (a test's) keeps it.
pub fn route_launches(s: &mut Session) -> Option<Launches> {
    if !s.docs.launches_by_default() {
        return None;
    }
    let q: Launches = Default::default();
    let into = std::sync::Arc::clone(&q);
    s.docs.set_launcher(std::sync::Arc::new(move |program: &Path, args: &[String], file: &Path| {
        into.lock().map_err(|_| "the launch queue is poisoned".to_owned())?.push((program.to_path_buf(), args.to_vec(), file.to_path_buf()));
        Ok(())
    }));
    Some(q)
}

/// The `open.external` asks for what the session asked to start: `{path, app?, program, args}`.
/// `app` is the application chosen (Open With); `program` and `args` are how a desktop starts it.
pub fn raise_launches(h: &mut impl Host) {
    for (program, args, file) in h.take_launches() {
        let app = match args.as_slice() {
            [a, app] if a == "-a" => Some(app.clone()),
            _ => None,
        };
        let params = json!({"path": file, "app": app, "program": program, "args": args});
        raise(h, Kind::OpenExternal, params, By::Person, Cont::new(Then::Notice));
    }
}

/// Take the queued launches from `q`.
pub fn drain_launches(q: Option<&Launches>) -> Vec<(PathBuf, Vec<String>, PathBuf)> {
    q.and_then(|q| q.lock().ok().map(|mut v| std::mem::take(&mut *v))).unwrap_or_default()
}

/// Raise an ask, dropping the continuations of what it withdrew.
pub fn raise(h: &mut impl Host, kind: Kind, params: Value, by: By, cont: Cont) -> AskId {
    h.asks().raise(kind, params, by, cont).0
}

/// The folder `key`'s dialogs last used, as a `startIn` param.
fn start_in(h: &mut impl Host, key: &str) -> Value {
    h.prefs().and_then(|p| p.dir(key)).map_or(Value::Null, |d| json!(d))
}

fn workflow_kinds() -> Value {
    // what the application opens beside workflows, in the one filter: macOS has no choice of them
    let openers = plumcraft_session::file_cmds::openers();
    let mut exts: Vec<&str> = vec![WORKFLOW_EXT, "zip"];
    exts.extend(openers.iter().flat_map(|o| o.extensions.iter().copied()));
    let name = std::iter::once("Workflow or bundle").chain(openers.iter().map(|o| o.label)).collect::<Vec<_>>().join(", ");
    json!([{"label": name, "extensions": exts}])
}

/// Ask for a workflow to read: `file.open {purpose: "workflow"}`.
fn ask_workflow(h: &mut impl Host, by: By, then: Then) -> AskId {
    let params = json!({"purpose": "workflow", "title": "Open Workflow", "kinds": workflow_kinds(), "multiple": false,
                        "startIn": start_in(h, prefs::keys::WORKFLOW_READ)});
    raise(h, Kind::FileOpen, params, by, Cont { then, remember: Some(prefs::keys::WORKFLOW_READ) })
}

/// Ask where to save the workflow: `file.save {purpose: "workflow"}`.
fn ask_save_workflow(h: &mut impl Host, by: By, after: Option<AfterDiscard>) -> AskId {
    let current = h.session().path().map(Path::to_path_buf);
    let name = current.as_deref().and_then(Path::file_name).and_then(|n| n.to_str()).unwrap_or("untitled.json").to_owned();
    let params = json!({"purpose": "workflow", "title": "Save Workflow", "kinds": [{"label": "Workflow", "extensions": [WORKFLOW_EXT]}],
                        "name": name, "current": current, "startIn": start_in(h, prefs::keys::WORKFLOW_WRITE)});
    raise(h, Kind::FileSave, params, by, Cont { then: Then::SaveWorkflow { after }, remember: Some(prefs::keys::WORKFLOW_WRITE) })
}

/// Run command `id` as its menu item or shortcut does, filling in what the UI knows: a
/// command's form, file asks for open and save, a prompt before unsaved changes are dropped,
/// the selection for node commands.
pub fn invoke(h: &mut impl Host, id: &str, by: By) {
    invoke_with(h, id, &Value::Null, by);
}

/// [`invoke`] with the command's own parameters (`ui.invoke {command, params}`, N4 slice 5: a
/// pop-out's Copy, Share… and Save As… name the pop-out).
pub fn invoke_with(h: &mut impl Host, id: &str, params: &Value, by: By) {
    // a command with a form asks for its params first
    if let Some(form) = plumcraft_session::commands::form(id) {
        if let Err(why) = plumcraft_session::find(id).map_or(Ok(()), |c| (c.enabled)(h.session())) {
            h.say(why);
            return;
        }
        let values = form.initial(h.session());
        raise(h, Kind::Form, form_params(&form, &values, None), by, Cont::new(Then::Form { command: form.command.to_owned() }));
        return;
    }
    let (nodes, groups) = h.selection();
    match id {
        "graph.new" | "graph.open" => {
            let then = if id == "graph.new" { AfterDiscard::New } else { AfterDiscard::Open };
            if h.session().is_dirty() {
                raise_prompt(h, Prompt::Discard { then }, by);
            } else {
                go_on(h, then, false, by);
            }
        }
        "app.quit" => quit(h, by),
        "engine.install" | "engine.attach" if h.open_window(id) => {}
        // a shell without a window of its own for them (the Apple apps, N4 slice 6): the core's
        // form asks for the address or the repository, then runs the command
        "engine.install" | "engine.attach" => {
            if let Err(why) = plumcraft_session::find(id).map_or(Ok(()), |c| (c.enabled)(h.session())) {
                h.say(why);
                return;
            }
            if let Some(form) = engine_form(id) {
                let values = form.initial(h.session());
                raise(h, Kind::Form, form_params(&form, &values, None), by, Cont::new(Then::Form { command: form.command.to_owned() }));
            }
        }
        "graph.save" if h.session().path().is_some() => {
            let _ = h.run(id, &json!({}));
        }
        "graph.save" | "graph.saveAs" => {
            ask_save_workflow(h, by, None);
        }
        // Save a Copy… and Move… (N4 slice 4): where, then the command
        "graph.saveCopy" => {
            let current = h.session().path().map(Path::to_path_buf);
            let stem = current.as_deref().and_then(Path::file_stem).and_then(|n| n.to_str()).unwrap_or("untitled").to_owned();
            let params = json!({"purpose": "copy", "title": "Save a Copy", "kinds": [{"label": "Workflow", "extensions": [WORKFLOW_EXT]}],
                                "name": format!("{stem} copy.json"), "current": current, "startIn": start_in(h, prefs::keys::WORKFLOW_WRITE)});
            raise(h, Kind::FileSave, params, by, Cont { then: Then::SaveCopy, remember: Some(prefs::keys::WORKFLOW_WRITE) });
        }
        "graph.move" => {
            if h.session().path().is_none() {
                h.say("save the graph first: it has no file yet".into());
            } else {
                let params = json!({"purpose": "move", "title": "Move To", "startIn": start_in(h, prefs::keys::WORKFLOW_WRITE)});
                raise(h, Kind::FolderPick, params, by, Cont { then: Then::Move, remember: Some(prefs::keys::WORKFLOW_WRITE) });
            }
        }
        // Settings › Import… and Export… (N4 slice 4: a native shell's settings ask for the file)
        "theme.import" | "theme.export" => {
            theme_file(h, id == "theme.import", by);
        }
        "graph.bundle" => {
            let current = h.session().path().map(Path::to_path_buf);
            let stem = current.as_deref().and_then(Path::file_stem).and_then(|n| n.to_str()).unwrap_or("untitled").to_owned();
            let params = json!({"purpose": "bundle", "title": "Export Bundle", "kinds": [{"label": "Bundle", "extensions": ["zip"]}],
                                "name": format!("{stem}.zip"), "current": current, "startIn": start_in(h, prefs::keys::BUNDLE_WRITE)});
            raise(h, Kind::FileSave, params, by, Cont { then: Then::Bundle, remember: Some(prefs::keys::BUNDLE_WRITE) });
        }
        "group.create" => {
            if nodes.is_empty() && groups.is_empty() {
                h.say("select the nodes to group first".into());
            } else {
                let _ = h.run(id, &json!({"nodes": nodes, "groups": groups}));
            }
        }
        "node.expand" => match nodes.as_slice() {
            [one] => {
                let _ = h.run(id, &json!({"node": one}));
            }
            _ => h.say("select one node to expand".into()),
        },
        "group.ungroup" => match h.group_to_ungroup() {
            Some(g) => {
                let _ = h.run(id, &json!({"group": g}));
            }
            None => h.say("select a group (or a node in one) to ungroup".into()),
        },
        "graph.insert" | "graph.insertGroup" => {
            let pos = h.canvas_center();
            ask_workflow(h, by, Then::Insert { command: id.to_owned(), pos });
        }
        "frame.add" => h.frame_selection(),
        // Edit › Cut and Copy: the selection (with the members of selected collapsed groups) as
        // workflow JSON on the clipboard; a cut's nodes go, and the selection with them
        "edit.cut" | "edit.copy" => {
            let members = crate::selection::Selection { nodes: nodes.iter().map(|n| plumcraft_graph::NodeId::from(n.as_str())).collect(), groups, net: None }
                .members(h.session().graph());
            let what = if id == "edit.cut" { "cut" } else { "copy" };
            if members.is_empty() {
                h.say(format!("select nodes to {what} first"));
                return;
            }
            let Ok(r) = h.run(id, &json!({"nodes": members})) else { return };
            let Some(text) = r.get("text").and_then(Value::as_str) else { return };
            copy(h, text, "workflow", by);
            if id == "edit.cut" {
                let _ = h.run("selection.clear", &Value::Null);
            }
        }
        // a pop-out's value out of the editor (N4 slice 5)
        "value.copy" => match h.run(id, params) {
            Ok(r) => {
                let text = r.get("text").and_then(Value::as_str).unwrap_or_default().to_owned();
                copy(h, &text, "text", by);
            }
            Err(e) => h.say(e),
        },
        "value.share" => match h.run(id, params) {
            Ok(r) => {
                raise(h, Kind::Share, r, by, Cont::new(Then::Notice));
            }
            Err(e) => h.say(e),
        },
        "value.saveAs" => {
            let pid = params.get("popout").and_then(Value::as_u64);
            let target =
                pid.and_then(|p| h.session().run.entry(plumcraft_session::run::WatchRole::Popout(p)).map(|(w, _)| (w.node.clone(), w.port.clone(), w.index)));
            match target {
                Some((node, port, index)) => {
                    save_result(h, json!({"node": node, "port": port, "index": index}), by);
                }
                None => h.say("Save Value As… needs a pop-out".into()),
            }
        }
        // File › Share… (N4 slice 3): the graph's file to the platform's share sheet
        "graph.share" => match h.run(id, &json!({})) {
            Ok(r) => {
                raise(h, Kind::Share, r, by, Cont::new(Then::Notice));
            }
            Err(e) => h.say(e),
        },
        // Edit › Paste: the shell reads the clipboard, then it is pasted
        "edit.paste" => {
            ask_paste(h, by);
        }
        // Edit › Delete: what isn't selected can't be deleted, which the status bar says
        "edit.delete" => {
            if let Err(e) = h.run(id, &Value::Null) {
                h.say(e);
            }
        }
        "graph.tidy" => {
            // the selection when there is one to arrange, else everything; with the sizes and
            // pins the canvas measured last frame
            let (sizes, pins) = h.measured();
            let nodes = if nodes.len() >= 2 { nodes } else { Vec::new() };
            if h.run(id, &json!({"nodes": nodes, "sizes": sizes, "pins": pins})).is_ok() {
                h.fit();
            }
        }
        "node.remove" | "node.duplicate" => {
            if nodes.is_empty() {
                h.say("select nodes first".into());
            } else if let Ok(r) = h.run(id, &json!({"nodes": nodes}))
                && id == "node.duplicate"
            {
                // the copies become the selection
                select_made(h, &r);
            }
        }
        // an application's command gets what is selected, and may answer with a word for the
        // status line
        added if plumcraft_session::commands::is_added(added) => {
            let group = h.group_to_ungroup();
            if let Some(said) =
                h.run(added, &json!({"nodes": nodes, "group": group})).ok().and_then(|r| r.get("notice").and_then(Value::as_str).map(str::to_owned))
            {
                h.say(said);
            }
        }
        _ => {
            let _ = h.run(id, params);
        }
    }
}

/// A form's ask params: `{command, title, intro, fields, submit, values, error?}`.
/// The forms Engine › Attach… and Install Engine… ask with in a shell that has no window of its
/// own for them (egui has: [`Host::open_window`]).
pub fn engine_form(command: &str) -> Option<plumcraft_session::commands::CommandForm> {
    use plumcraft_session::commands::{CommandForm, FieldKind, FormField};
    let line = |key, label, hint, required| FormField { key, label, hint, kind: FieldKind::Line, required };
    match command {
        "engine.attach" => Some(CommandForm {
            command: "engine.attach",
            title: "Attach to an Engine",
            intro: "Connect to an HTTP engine already running (started with --listen) as a remote UI.",
            fields: vec![
                line("url", "Address", "http://host:port", true),
                FormField { kind: FieldKind::Secret, ..line("token", "Token", "if the engine asks for one", false) },
            ],
            submit: "Attach",
            prefill: None,
        }),
        "engine.install" => Some(CommandForm {
            command: "engine.install",
            title: "Install Engine",
            intro: "Download a Python engine from GitHub and install it. It runs the package's code: install only what you trust.",
            fields: vec![line("repo", "Repository", "owner/repo or https://github.com/owner/repo", true)],
            submit: "Install",
            prefill: None,
        }),
        _ => None,
    }
}

pub fn form_params(form: &plumcraft_session::commands::CommandForm, values: &[String], error: Option<&str>) -> Value {
    use plumcraft_session::commands::FieldKind;
    let fields: Vec<Value> = form
        .fields
        .iter()
        .map(|f| {
            let kind = match f.kind {
                FieldKind::Line => "line",
                FieldKind::Secret => "secret",
                FieldKind::Text => "text",
            };
            json!({"key": f.key, "label": f.label, "hint": f.hint, "kind": kind, "required": f.required})
        })
        .collect();
    let values: Map<String, Value> = form.fields.iter().zip(values).map(|(f, v)| (f.key.to_owned(), json!(v))).collect();
    let mut p = json!({"command": form.command, "title": form.title, "intro": form.intro, "fields": fields, "submit": form.submit, "values": values});
    if let Some(e) = error {
        p["error"] = json!(e);
    }
    p
}

/// Raise `prompt`; it withdraws the prompt open before it, as the window's one prompt did.
pub fn raise_prompt(h: &mut impl Host, prompt: Prompt, by: By) -> AskId {
    let params = prompt.params();
    raise(h, Kind::Prompt, params, by, Cont::new(Then::Prompt(prompt)))
}

/// The open prompt, if any, with its id.
pub fn open_prompt(asks: &Asks<Cont>) -> Option<(AskId, &Prompt)> {
    asks.entries().find_map(|(a, c)| match &c.then {
        Then::Prompt(p) if a.kind == Kind::Prompt => Some((a.id, p)),
        _ => None,
    })
}

/// Quit: close the window, asking first over unsaved changes.
pub fn quit(h: &mut impl Host, by: By) {
    if h.session().is_dirty() && !h.closing() {
        raise_prompt(h, Prompt::Discard { then: AfterDiscard::Quit }, by);
    } else {
        h.close();
    }
}

/// New, or Open through a file ask, once unsaved changes may go.
fn go_on(h: &mut impl Host, then: AfterDiscard, discard: bool, by: By) {
    match then {
        AfterDiscard::New => {
            let _ = h.run("graph.new", &json!({"discard": discard}));
        }
        AfterDiscard::Open => {
            ask_workflow(h, by, Then::OpenWorkflow { discard });
        }
        AfterDiscard::Quit => h.close(),
    }
}

/// Answer the open prompt (`ui.prompt`, and the prompt window's buttons): false when no
/// prompt is open or it doesn't take that answer.
pub fn answer_prompt(h: &mut impl Host, choice: PromptAnswer) -> bool {
    let Some((id, _)) = open_prompt(h.asks()) else { return false };
    answer(h, id, &json!({"answer": choice.name()})).is_ok()
}

/// Answer ask `id` and carry its chain on. An error (an unknown or ended ask, a wrong shape) is
/// the caller's to report; nothing ran.
pub fn answer(h: &mut impl Host, id: AskId, value: &Value) -> Result<(), String> {
    let (ask, cont, answer) = h.asks().answer(id, value).map_err(|r| r.to_string())?;
    if let (Some(key), Some(first)) = (
        cont.remember,
        match &answer {
            Answer::Paths(p) => p.first(),
            Answer::Path(p) => Some(p),
            _ => None,
        },
    ) && let Some(prefs) = h.prefs()
    {
        prefs.remember(key, first);
    }
    go(h, ask, cont, answer);
    Ok(())
}

/// Cancel ask `id`: nothing follows (a cancelled prompt is its Cancel button).
pub fn cancel(h: &mut impl Host, id: AskId) -> Result<(), String> {
    let (ask, cont) = h.asks().cancel(id).map_err(|r| r.to_string())?;
    if let Then::Prompt(p) = cont.then {
        prompt_cancelled(h, &p);
    }
    let _ = ask;
    Ok(())
}

fn prompt_cancelled(h: &mut impl Host, p: &Prompt) {
    match p {
        Prompt::Discard { .. } => {}
        Prompt::Review(_) => h.session_mut().run.decline_review(),
        Prompt::Uploads(_) => {
            if h.take_held_run().is_some() {
                h.session_mut().run.cancel();
            }
        }
    }
}

/// Withdraw what lost its reason: a path ask whose node went; time out what waited too long.
pub fn tidy(h: &mut impl Host) {
    raise_launches(h);
    let now = std::time::Instant::now();
    h.asks().expire(now);
    let gone: Vec<AskId> = {
        let g = h.session().graph();
        let mut ids = Vec::new();
        // (borrowing the asks needs &mut: collect the nodes first)
        let nodes: std::collections::HashSet<String> = g.nodes.keys().map(|n| n.0.clone()).collect();
        let asks = h.asks();
        for (a, c) in asks.entries() {
            if let Then::Path { node, .. } | Then::Files { node, .. } = &c.then
                && !nodes.contains(node)
            {
                ids.push(a.id);
            }
        }
        ids
    };
    if !gone.is_empty() {
        h.asks().withdraw(|a| gone.contains(&a.id));
    }
}

/// Carry a chain on with an answer.
fn go(h: &mut impl Host, ask: Ask, cont: Cont, answer: Answer) {
    let by = ask.by;
    let path = |a: &Answer| match a {
        Answer::Path(p) => Some(p.clone()),
        Answer::Paths(p) => p.first().cloned(),
        _ => None,
    };
    match (cont.then, answer) {
        (Then::Prompt(p), Answer::Choice(c)) => match (p, PromptAnswer::from_name(&c)) {
            (Prompt::Discard { then }, Some(PromptAnswer::Discard)) => go_on(h, then, true, by),
            (Prompt::Discard { then }, Some(PromptAnswer::Save)) => {
                if h.session().path().is_some() {
                    let _ = h.run("graph.save", &json!({}));
                    if !h.session().is_dirty() {
                        go_on(h, then, false, by);
                    }
                } else {
                    ask_save_workflow(h, by, Some(then));
                }
            }
            (Prompt::Review(_), Some(PromptAnswer::Trust)) => {
                let _ = h.run("graph.trust", &json!({}));
            }
            (Prompt::Uploads(pending), Some(PromptAnswer::Trust)) => {
                let files: Vec<String> = pending.into_iter().map(|(_, f)| f).collect();
                h.allow_uploads(&files);
                if let Some(req) = h.take_held_run() {
                    h.session_mut().run.resend(req);
                }
            }
            (p, _) => prompt_cancelled(h, &p),
        },
        (Then::OpenWorkflow { discard }, a) => {
            if let Some(p) = path(&a) {
                let _ = h.run("graph.open", &json!({"path": p, "discard": discard}));
            }
        }
        (Then::SaveWorkflow { after }, a) => {
            if let Some(p) = path(&a) {
                let _ = h.run("graph.saveAs", &json!({"path": p}));
                if let Some(then) = after
                    && !h.session().is_dirty()
                {
                    go_on(h, then, false, by);
                }
            }
        }
        (Then::Bundle, a) => {
            if let Some(p) = path(&a) {
                let _ = h.run("graph.bundle", &json!({"path": p}));
            }
        }
        (Then::Insert { command, pos }, a) => {
            if let Some(p) = path(&a) {
                let mut params = json!({"path": p});
                if let Some(pos) = pos {
                    params["pos"] = json!(pos);
                }
                if let Ok(r) = h.run(&command, &params) {
                    select_made(h, &r);
                    if let Some(dropped) = r.get("dropped").and_then(Value::as_array).filter(|d| !d.is_empty()) {
                        h.say(format!("inserted; {} wire(s) no longer fit and were left out", dropped.len()));
                    }
                }
            }
        }
        (Then::Path { node, port, save, base }, a) => {
            if let Some(p) = path(&a) {
                let value = paths::chosen_path(&p, base.as_deref(), save);
                let _ = h.run("node.set", &json!({"node": node, "port": port, "value": value}));
            }
        }
        (Then::Files { node, port, base }, Answer::Paths(chosen)) => {
            let value: Vec<String> = chosen.iter().map(|p| paths::chosen_path(p, base.as_deref(), false)).collect();
            let _ = h.run("node.set", &json!({"node": node, "port": port, "value": value.join("\n")}));
        }
        (Then::App { mut params }, a) => {
            if let Some(app) = path(&a) {
                if let Some(o) = params.as_object_mut() {
                    o.remove("choose");
                    o.insert("app".into(), Value::from(app.display().to_string()));
                }
                let _ = h.run("value.open", &params);
            }
        }
        (Then::SaveResult { mut params, base }, a) => {
            if let Some(p) = path(&a) {
                // (a `/` typed into macOS's save panel comes back as `:`: folders)
                let p = paths::chosen_path(&p, base.as_deref(), true);
                if let Some(o) = params.as_object_mut() {
                    o.remove("ask");
                    o.insert("path".into(), Value::from(p));
                }
                let _ = h.run("run.save", &params);
            }
        }
        (Then::ThemeImport, a) => {
            if let Some(p) = path(&a) {
                let _ = h.run("theme.import", &json!({"path": p.display().to_string()}));
            }
        }
        (Then::ThemeExport, a) => {
            if let Some(p) = path(&a) {
                let _ = h.run("theme.export", &json!({"path": p.display().to_string()}));
            }
        }
        (Then::SaveCopy, a) => {
            if let Some(p) = path(&a) {
                let _ = h.run("graph.saveCopy", &json!({"path": p.display().to_string()}));
            }
        }
        (Then::Move, a) => {
            if let Some(p) = path(&a) {
                let _ = h.run("graph.move", &json!({"folder": p.display().to_string()}));
            }
        }
        (Then::Form { command }, Answer::Values(given)) => {
            let Some(form) = plumcraft_session::commands::form(&command).or_else(|| engine_form(&command)) else { return };
            let values: Vec<String> = form.fields.iter().map(|f| given.get(f.key).and_then(Value::as_str).unwrap_or_default().to_owned()).collect();
            let refused = match form.params(&values) {
                Err(why) => Some(why),
                Ok(params) => h.run(form.command, &params).err(),
            };
            // refused: the form stays open, saying why, with what was typed
            if let Some(why) = refused {
                let ask = Ask { params: form_params(&form, &values, Some(&why)), ..ask };
                h.asks().reopen(ask, Cont::new(Then::Form { command }));
            }
        }
        (Then::Paste, Answer::Text(text)) => paste(h, &text),
        (_, Answer::Failed(why)) => h.say(why),
        _ => {}
    }
}

/// Select what a command made (`selection.set`): the nodes it returned, or the collapsed group.
pub fn select_made(h: &mut impl Host, result: &Value) {
    if let Some(p) = crate::selection::made(result, h.session().graph()) {
        let _ = h.run("selection.set", &p);
    }
}

/// Paste workflow JSON where the shell says (the pointer), selecting the result.
pub fn paste(h: &mut impl Host, text: &str) {
    let mut params = json!({"text": text});
    if let Some(p) = h.paste_pos() {
        params["pos"] = json!(p);
    }
    if let Ok(r) = h.run("edit.paste", &params) {
        select_made(h, &r);
        if let Some(dropped) = r.get("dropped").and_then(Value::as_array).filter(|d| !d.is_empty()) {
            h.say(format!("pasted; {} wire(s) no longer fit and were left out", dropped.len()));
        }
    }
}

/// Edit › Paste: ask the shell for the clipboard's text, then paste it.
pub fn ask_paste(h: &mut impl Host, by: By) -> AskId {
    h.asks().raise_with(Kind::ClipboardGet, json!({"kinds": ["workflow", "text"]}), by, Some(PASTE_WAIT), Cont::new(Then::Paste)).0
}

/// Put `text` on the clipboard (`kind`: `workflow` for copied nodes, `text` otherwise).
pub fn copy(h: &mut impl Host, text: &str, kind: &str, by: By) -> AskId {
    raise(h, Kind::ClipboardSet, json!({"text": text, "kind": kind}), by, Cont::new(Then::Notice))
}

/// A path input's "…": a file (or folder) for it, starting near what it holds. Relative paths
/// are relative to the workflow's folder (or, unsaved, the editor's), so the ask starts there
/// and a choice inside it is stored relative.
pub fn browse(h: &mut impl Host, node: &str, port: &str, hint: &PathHint, current: &str, by: By) -> AskId {
    let base = paths::base_folder(h.session().path());
    let key = prefs::dir_key(hint);
    let kinds: Vec<Value> = hint.filters.iter().map(|f| json!({"label": f.name, "extensions": f.extensions})).collect();
    if hint.is_many() {
        let start = paths::each_line(current, |l| match &base {
            Some(b) if !paths::is_anchored(l) => b.join(l).to_string_lossy().into_owned(),
            _ => paths::expand_user(l).to_string_lossy().into_owned(),
        });
        let params =
            json!({"purpose": "files", "title": "Select Files", "kinds": kinds, "multiple": true, "hint": hint, "current": start, "startIn": start_in(h, key)});
        let then = Then::Files { node: node.to_owned(), port: port.to_owned(), base };
        return raise(h, Kind::FileOpen, params, by, Cont { then, remember: Some(key) });
    }
    let start = match &base {
        Some(b) if !current.trim().is_empty() && !paths::is_anchored(current.trim()) => b.join(current.trim()).to_string_lossy().into_owned(),
        _ => paths::expand_user(current.trim()).to_string_lossy().into_owned(),
    };
    let save = hint.mode == "save";
    let (kind, title) = match hint.mode.as_str() {
        "dir" | "folder" => (Kind::FolderPick, "Choose a Folder"),
        "save" => (Kind::FileSave, "Save As"),
        _ => (Kind::FileOpen, "Choose a File"),
    };
    let name = Path::new(start.trim()).file_name().and_then(|n| n.to_str()).map(str::to_owned);
    let params = json!({"purpose": "path", "title": title, "kinds": kinds, "multiple": false, "name": name, "hint": hint, "current": start, "startIn": start_in(h, key)});
    let then = Then::Path { node: node.to_owned(), port: port.to_owned(), save, base };
    raise(h, kind, params, by, Cont { then, remember: Some(key) })
}

/// Open With › Choose an App…: an app to open a value with, then `value.open` with it.
pub fn choose_app(h: &mut impl Host, params: Value, by: By) -> AskId {
    let p = json!({"purpose": "app", "title": "Open With", "kinds": [{"label": "Application", "extensions": ["app"]}], "multiple": false, "startIn": "/Applications"});
    raise(h, Kind::FileOpen, p, by, Cont::new(Then::App { params }))
}

/// A save button: say why not, or ask where (offering the node's title as the name, where the
/// last save went or else in the workflow's folder), then `run.save`.
pub fn save_result(h: &mut impl Host, params: Value, by: By) -> Option<AskId> {
    if let Some(c) = plumcraft_session::find("run.save")
        && let Err(why) = (c.enabled)(h.session())
    {
        h.say(format!("can't save: {why}"));
        return None;
    }
    let node = plumcraft_graph::NodeId(params.get("node").and_then(Value::as_str).unwrap_or_default().to_owned());
    let index = params.get("index").and_then(Value::as_i64);
    let sheet = params.get("sheet").and_then(Value::as_bool).unwrap_or(false);
    let s = h.session();
    let title = plumcraft_session::node_title(s.graph(), s.catalog(), &node);
    let stem = if sheet { format!("{title} sheet") } else { title };
    let name = plumcraft_session::save_cmds::suggested_name(s.catalog(), &stem, index);
    let base = paths::base_folder(s.path());
    let last = s.save.last.as_ref().filter(|(_, r)| r.ok).and_then(|(ask, _)| Path::new(&ask.path).parent().map(Path::to_path_buf));
    let formats: Vec<SaveFormat> = s.catalog().save_formats.clone();
    // the last save's folder, then the one a save last used in an earlier run
    let remembered = h.prefs().and_then(|p| p.dir(prefs::keys::RESULT_WRITE));
    let folder = last.or(remembered).or_else(|| base.clone()).unwrap_or_default();
    let kinds: Vec<Value> =
        formats.iter().map(|f| json!({"label": f.label, "extensions": f.extensions.iter().map(|e| e.trim_start_matches('.')).collect::<Vec<_>>()})).collect();
    let p = json!({"purpose": "result", "title": "Save Result", "kinds": kinds, "formats": formats, "name": name, "suggested": folder.join(&name),
                   "startIn": start_in(h, prefs::keys::RESULT_WRITE)});
    Some(raise(h, Kind::FileSave, p, by, Cont { then: Then::SaveResult { params, base }, remember: Some(prefs::keys::RESULT_WRITE) }))
}

/// Settings › Appearance › Import… (`import`) or Export…: a theme file.
pub fn theme_file(h: &mut impl Host, import: bool, by: By) -> AskId {
    let kinds = json!([{"label": "PlumCraft theme", "extensions": ["json"]}]);
    if import {
        raise(h, Kind::FileOpen, json!({"purpose": "theme", "title": "Import Theme", "kinds": kinds, "multiple": false}), by, Cont::new(Then::ThemeImport))
    } else {
        let name = format!("{}.plumtheme.json", h.theme_name());
        raise(h, Kind::FileSave, json!({"purpose": "theme", "title": "Export Theme", "kinds": kinds, "name": name}), by, Cont::new(Then::ThemeExport))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::catalogue::SchemaReply;

    /// A host with no canvas: a session, its asks, what it said, whether it closed.
    struct Bare {
        session: Session,
        asks: Asks<Cont>,
        said: Vec<String>,
        closed: bool,
        prefs: prefs::Store,
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
            self.said.push(text);
        }
        fn prefs(&mut self) -> Option<&mut prefs::Store> {
            Some(&mut self.prefs)
        }
        fn close(&mut self) {
            self.closed = true;
        }
    }

    fn bare() -> Bare {
        let catalog: SchemaReply = serde_json::from_str(r#"{"v": 2, "engine": {"schema_digest": "d"}, "nodes": [{"id": "t.x"}]}"#).unwrap();
        Bare { session: Session::new(catalog), asks: Asks::default(), said: Vec::new(), closed: false, prefs: prefs::Store::default() }
    }

    fn dir(name: &str) -> PathBuf {
        let d = plumcraft_scratch::root().join(format!("invoke-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn dirty(h: &mut Bare) {
        let g = plumcraft_graph::Graph::from_json(r#"{"version": 1, "nodes": {"a": {"kind": "t.x"}}, "wires": []}"#).unwrap();
        h.session
            .edit("Dirty", move |into, _| {
                *into = g;
                Ok(())
            })
            .unwrap();
        assert!(h.session.is_dirty());
    }

    fn only(h: &mut Bare) -> (AskId, Kind, Value) {
        let a: Vec<&Ask> = h.asks.open().collect();
        assert_eq!(a.len(), 1, "{:?}", h.asks.inspect());
        (a[0].id, a[0].kind, a[0].params.clone())
    }

    #[test]
    fn new_over_unsaved_changes_prompts_then_saves_through_a_file_ask_then_starts_over() {
        let d = dir("new-save");
        let mut h = bare();
        dirty(&mut h);
        invoke(&mut h, "graph.new", By::Script);
        let (id, kind, params) = only(&mut h);
        assert_eq!((kind, params["kind"].as_str(), params["then"].as_str()), (Kind::Prompt, Some("unsavedChanges"), Some("new")));
        assert_eq!(params["answers"], json!(["save", "discard", "cancel"]));
        answer(&mut h, id, &json!({"answer": "save"})).unwrap();
        let (save, kind, params) = only(&mut h);
        assert_eq!((kind, params["purpose"].as_str(), params["name"].as_str()), (Kind::FileSave, Some("workflow"), Some("untitled.json")));
        assert!(h.asks.get(save).is_some_and(|a| a.by == By::Script), "the chain stays the script's");
        let path = d.join("kept.json");
        answer(&mut h, save, &json!({"path": path})).unwrap();
        assert!(path.is_file(), "saved first");
        assert!(h.asks.is_empty());
        assert!(!h.session.is_dirty() && h.session.path().is_none(), "then a new graph");
        assert_eq!(h.prefs.dir(prefs::keys::WORKFLOW_WRITE), Some(d.clone()), "the folder is remembered");
    }

    #[test]
    fn a_cancelled_step_ends_the_chain_and_open_asks_for_a_workflow() {
        let d = dir("open");
        let mut h = bare();
        h.session.execute("graph.saveAs", &json!({"path": d.join("a.json")})).unwrap();
        dirty(&mut h);
        invoke(&mut h, "graph.open", By::Person);
        let (id, ..) = only(&mut h);
        assert!(answer_prompt(&mut h, PromptAnswer::Discard));
        assert!(h.asks.get(id).is_none());
        let (open, kind, params) = only(&mut h);
        assert_eq!((kind, params["purpose"].as_str()), (Kind::FileOpen, Some("workflow")));
        cancel(&mut h, open).unwrap();
        assert!(h.asks.is_empty() && h.session.is_dirty(), "nothing opened");
        assert!(!answer_prompt(&mut h, PromptAnswer::Cancel), "no prompt open");
        // quit over changes: discard closes
        quit(&mut h, By::Person);
        assert!(!answer_prompt(&mut h, PromptAnswer::Trust), "not an answer to this prompt");
        assert!(answer_prompt(&mut h, PromptAnswer::Discard));
        assert!(h.closed);
    }

    #[test]
    fn hostile_answers_run_nothing() {
        let mut h = bare();
        invoke(&mut h, "graph.saveAs", By::Script);
        let (id, ..) = only(&mut h);
        for bad in [json!(null), json!({"path": 1}), json!({"paths": ["/a"]}), json!({"path": "/a", "x": 1})] {
            assert!(answer(&mut h, id, &bad).is_err(), "{bad}");
        }
        assert!(h.session.path().is_none());
        assert!(answer(&mut h, id + 100, &json!({"path": "/a"})).unwrap_err().contains("no ask"));
        cancel(&mut h, id).unwrap();
        assert!(answer(&mut h, id, &json!({"path": "/a"})).unwrap_err().contains("cancelled"));
        assert!(cancel(&mut h, id).is_err());
    }

    #[test]
    fn starting_an_app_is_an_open_external_ask_unless_a_launcher_was_given() {
        let mut s = bare().session;
        let q = route_launches(&mut s).expect("routed");
        assert!(route_launches(&mut s).is_none(), "once");
        assert!(!s.docs.launches_by_default());
        // (what the hook records when the session starts an app)
        q.lock().unwrap().push(("/usr/bin/open".into(), vec!["-a".into(), "/Applications/Preview.app".into()], "/tmp/x.pdf".into()));
        let mut h = bare();
        h.session = s;
        // (the bare host has no queue of its own: raise what the hook recorded)
        for (program, args, file) in drain_launches(Some(&q)) {
            let params = json!({"path": file, "app": args.get(1), "program": program, "args": args});
            raise(&mut h, Kind::OpenExternal, params, By::Person, Cont::new(Then::Notice));
        }
        let (id, kind, params) = only(&mut h);
        assert_eq!((kind, params["app"].as_str(), params["path"].as_str()), (Kind::OpenExternal, Some("/Applications/Preview.app"), Some("/tmp/x.pdf")));
        answer(&mut h, id, &json!({"error": "no such app"})).unwrap();
        assert_eq!(h.said, ["no such app"], "a failure goes to the status bar");
        let mut given = bare().session;
        given.docs.set_launcher(std::sync::Arc::new(|_: &Path, _: &[String], _: &Path| Ok(())));
        assert!(route_launches(&mut given).is_none(), "a launcher given is kept");
    }

    #[test]
    fn a_path_ask_is_withdrawn_when_its_node_goes() {
        let mut h = bare();
        let hint = PathHint { mode: "open".into(), ..Default::default() };
        let id = browse(&mut h, "gone", "path", &hint, "", By::Person);
        tidy(&mut h);
        assert!(h.asks.get(id).is_none());
        assert!(answer(&mut h, id, &json!({"paths": ["/a"]})).unwrap_err().contains("withdrawn"));
    }
}
