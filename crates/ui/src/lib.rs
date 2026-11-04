//! The egui shell of PlumCraft's editor: menu bar (from the command registry), engine banner,
//! node canvas, Watched panel, status bar.
//!
//! The UI is thin: it reads the session, and every action it takes is a session command run by
//! id through [`plumcraft_session::Session::execute`]. Menus and shortcuts list whatever commands
//! the registry has, so new commands appear without UI changes; a few get their parameters from
//! the UI (the canvas selection), see [`EditorApp::invoke`]. Node types, widgets and ports come
//! only from the engine's catalogue.
//!
//! The engine is reached through a [`link::Link`] (any `Connection`); each frame the app drains
//! its events into the session's run state, sends what the session queued (`run`,
//! `run.cancel`), and starts a due auto-run.
//! The Engine menu switches engines while the editor runs ([`engine_view`]): the session's
//! `engine.*` commands record the request and the editor connects, with a connector the app gives it.
//!
//! Lifted from the research software's `crates/ui-egui/src/lib.rs` (2026-10-27, the research software `8d96021`; plan 10.3,
//! E1.3) without groups, export, the parameters panel, the VI viewer (the
//! Watched panel replaces it) or preferences. The control channel (E1.4, [`control`]) is drained
//! at the start of every frame and answered at [`EditorApp::frame_end`].
//!
//! **Files (E2).** File › Open, Save and Save As run `graph.open`, `graph.save` and
//! `graph.saveAs` with a path from [`FileDialogs`] (rfd's native dialogs; tests fake them). New
//! and Open over unsaved changes first ask, in an in-window prompt ([`Prompt`]) the control
//! channel can answer too (`ui.prompt`); only a "Discard" passes `discard: true`. The window
//! title is the workflow's file name, with ` •` while it has unsaved changes.
//!
//! **Saving a result.** Save Result… and Contact Sheet… (a result node, the Watched panel) ask
//! where through [`FileDialogs::save_output`], then run `run.save`. The session queues the
//! engine's `output.save`, which can take as long as the graph: a thread of its own waits for it,
//! so the window stays live, and the reply lands in the session and the status bar.
//!
//! **Quitting.** Closing the window (its close button, Cmd+Q, File › Quit: `app.quit`) over
//! unsaved changes cancels the close (egui's `ViewportCommand::CancelClose`) and asks the same
//! prompt; the window closes only on Discard, or once Save has saved. `--screenshot` closes
//! without asking (it never edits).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod about_view;
pub mod array_view;
pub mod audio;
pub mod bundle_view;
pub mod cache_view;
pub mod canvas;
pub mod compare_view;
pub mod control;
pub mod craft_ui;
pub mod diff;
pub mod edit_view;
pub mod engine_view;
pub mod fonts;
pub mod frame_view;
pub mod groups_view;
mod icon_data;
pub mod icons;
pub use plumcraft_session::layout;
pub mod link;
pub mod media;
pub mod net_view;
pub mod offscreen;
pub mod palette;
pub mod param_widgets;
pub mod params_view;
pub mod picture_tools;
pub mod playback;
pub mod popouts;
pub mod prefs;
pub mod probe;
pub mod recipe_view;
pub mod rename_view;
pub mod run_view;
pub mod slice_view;
pub mod theme;
pub mod value_editors;
pub mod viewers;
pub mod watched;
pub mod widgets;
pub mod wire_route;
pub mod wire_view;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use plumcraft_proto::catalogue::{PathHint, SaveFormat, SchemaReply, texts};
use plumcraft_proto::data::DataRef;
use plumcraft_proto::event::{Event, NodeState};
use plumcraft_proto::run::SaveReply;
use plumcraft_proto::session::EngineState;
use plumcraft_session::run::Outgoing;
use plumcraft_session::viewer_cmds::Tools;
use plumcraft_session::{CommandSpec, Session, command_specs, paths};
use plumcraft_store::Blob;
use serde_json::{Value, json};

use crate::canvas::{Action, Canvas, RunView};
use crate::link::Link;
use crate::run_view::Thumbs;
use crate::theme::Tokens;
use crate::watched::Watched;

/// The window title's app name.
pub const APP_TITLE: &str = "PlumCraft";

/// Top-level menus, in order. Commands name theirs in `CommandSpec::menu`; menus the registry
/// adds later appear after these.
const MENUS: &[&str] = &["File", "Edit", "View", "Node", "Run"];

/// The workflow file extension.
pub const WORKFLOW_EXT: &str = "json";

/// Native dialogs, replaceable in tests.
pub trait FileDialogs {
    /// A file or folder for a path input (`hint` from its port), starting near `current`.
    fn pick_path(&mut self, hint: &PathHint, current: &str) -> Option<PathBuf>;
    /// The next dialog starts here, when it has no file's own folder to start in: the folder this
    /// kind of dialog last used ([`prefs`]).
    fn start_in(&mut self, _dir: Option<PathBuf>) {}
    /// A workflow to open (File › Open).
    fn open_workflow(&mut self) -> Option<PathBuf> {
        None
    }
    /// Where to save the workflow (File › Save As, and Save of an unsaved one); `current` is
    /// its file, when it has one.
    fn save_workflow(&mut self, _current: Option<&Path>) -> Option<PathBuf> {
        None
    }
    /// Where to write a bundle (`.zip`) of the workflow at `current` (File › Export Bundle).
    fn save_bundle(&mut self, _current: Option<&Path>) -> Option<PathBuf> {
        None
    }
    /// An app to open a value with (Open with…).
    fn pick_app(&mut self) -> Option<PathBuf> {
        None
    }
    /// Several files for a `files` path input (Select Files), starting near `current`.
    fn pick_files(&mut self, _hint: &PathHint, _current: &str) -> Vec<PathBuf> {
        Vec::new()
    }
    /// Where to write a result (Save Result…, Contact Sheet…), offered `suggested` (a folder and
    /// a name) and the engine's formats.
    fn save_output(&mut self, _formats: &[SaveFormat], _suggested: &Path) -> Option<PathBuf> {
        None
    }
}

/// The name it had in E1.
pub use FileDialogs as PathDialogs;

/// rfd's native dialogs.
#[derive(Default)]
pub struct NativeDialogs {
    start: Option<PathBuf>,
}

impl NativeDialogs {
    /// A dialog starting in the remembered folder, which only the next one uses.
    fn dialog(&mut self) -> rfd::FileDialog {
        let d = rfd::FileDialog::new();
        match self.start.take() {
            Some(dir) => d.set_directory(dir),
            None => d,
        }
    }
}

impl FileDialogs for NativeDialogs {
    fn start_in(&mut self, dir: Option<PathBuf>) {
        self.start = dir;
    }

    fn pick_files(&mut self, hint: &PathHint, current: &str) -> Vec<PathBuf> {
        let mut d = self.dialog().set_title("Select Files");
        for f in &hint.filters {
            let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
            d = d.add_filter(&f.name, &exts);
        }
        if let Some(dir) =
            current.lines().next().map(|l| Path::new(l.trim())).and_then(|p| p.ancestors().find(|a| a.is_dir())).filter(|d| !d.as_os_str().is_empty())
        {
            d = d.set_directory(dir);
        }
        d.pick_files().unwrap_or_default()
    }

    fn pick_app(&mut self) -> Option<PathBuf> {
        let d = rfd::FileDialog::new().set_title("Open With");
        if cfg!(target_os = "macos") { d.set_directory("/Applications").add_filter("Application", &["app"]).pick_file() } else { d.pick_file() }
    }

    fn save_output(&mut self, formats: &[SaveFormat], suggested: &Path) -> Option<PathBuf> {
        let mut d = self.dialog().set_title("Save Result").set_can_create_directories(true);
        for f in formats {
            let exts: Vec<&str> = f.extensions.iter().map(|e| e.trim_start_matches('.')).collect();
            d = d.add_filter(&f.label, &exts);
        }
        if let Some(dir) = suggested.parent().filter(|p| p.is_dir()) {
            d = d.set_directory(dir);
        }
        if let Some(name) = suggested.file_name().and_then(|n| n.to_str()) {
            d = d.set_file_name(name);
        }
        d.save_file()
    }

    fn open_workflow(&mut self) -> Option<PathBuf> {
        // what the application opens beside workflows, in the one filter: macOS has no choice of them
        let openers = plumcraft_session::file_cmds::openers();
        let mut exts: Vec<&str> = vec![WORKFLOW_EXT, "zip"];
        exts.extend(openers.iter().flat_map(|o| o.extensions.iter().copied()));
        let name = std::iter::once("Workflow or bundle").chain(openers.iter().map(|o| o.label)).collect::<Vec<_>>().join(", ");
        self.dialog().set_title("Open Workflow").add_filter(&name, &exts).pick_file()
    }

    fn save_bundle(&mut self, current: Option<&Path>) -> Option<PathBuf> {
        let stem = current.and_then(Path::file_stem).and_then(|n| n.to_str()).unwrap_or("untitled");
        let mut d = self.dialog().set_title("Export Bundle").add_filter("Bundle", &["zip"]).set_can_create_directories(true);
        if let Some(dir) = current.and_then(Path::parent).filter(|d| d.is_dir()) {
            d = d.set_directory(dir);
        }
        d.set_file_name(format!("{stem}.zip")).save_file()
    }

    fn save_workflow(&mut self, current: Option<&Path>) -> Option<PathBuf> {
        let mut d = self.dialog().set_title("Save Workflow").add_filter("Workflow", &[WORKFLOW_EXT]).set_can_create_directories(true);
        if let Some(dir) = current.and_then(Path::parent).filter(|d| d.is_dir()) {
            d = d.set_directory(dir);
        }
        let name = current.and_then(Path::file_name).and_then(|n| n.to_str()).unwrap_or("untitled.json");
        d.set_file_name(name).save_file()
    }

    fn pick_path(&mut self, hint: &PathHint, current: &str) -> Option<PathBuf> {
        let mut d = self.dialog();
        for f in &hint.filters {
            let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
            d = d.add_filter(&f.name, &exts);
        }
        // Start in the current file's folder (or the folder itself) when the field names one
        // that exists.
        let current = Path::new(current.trim());
        let own = if current.as_os_str().is_empty() {
            None
        } else if current.is_dir() {
            Some(current.to_path_buf())
        } else {
            // the nearest folder that exists: a save path may name folders not made yet
            current.ancestors().skip(1).find(|p| p.is_dir()).map(Path::to_path_buf)
        };
        if let Some(dir) = own {
            d = d.set_directory(dir);
        }
        d = d.set_can_create_directories(true);
        match hint.mode.as_str() {
            "dir" | "folder" => d.set_title("Choose a Folder").pick_folder(),
            "save" => {
                if let Some(name) = current.file_name().and_then(|n| n.to_str()) {
                    d = d.set_file_name(name);
                }
                d.set_title("Save As").save_file()
            }
            _ => d.set_title("Choose a File").pick_file(),
        }
    }
}

/// An unattended screenshot: run the graph `runs` times, save the window after the last reply,
/// and close (`--screenshot`).
#[derive(Debug, Clone)]
pub struct Shot {
    pub path: PathBuf,
    pub runs: u64,
    started: u64,
    frames_after_reply: u32,
    /// When the window was asked for its picture.
    requested_at: Option<Instant>,
    since: Option<Instant>,
}

impl Shot {
    pub fn new(path: PathBuf, runs: u64) -> Self {
        Shot { path, runs: runs.max(1), started: 0, frames_after_reply: 0, requested_at: None, since: None }
    }
}

/// A question the window is asking (drawn as a modal; [`EditorApp::answer_prompt`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    /// The graph has unsaved changes: discard them to go on with `then`?
    Discard { then: AfterDiscard },
    /// A workflow from elsewhere is about to run, and this is what it touches.
    Review(Box<plumcraft_session::trust::Review>),
    /// A run would send these local files to an engine nobody here started: `(where to, file)`.
    Uploads(Vec<(String, String)>),
}

/// What a "Discard" in [`Prompt::Discard`] goes on to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterDiscard {
    /// `graph.new`.
    New,
    /// Ask for a workflow ([`FileDialogs::open_workflow`]), then `graph.open` it.
    Open,
    /// Close the window (quit).
    Quit,
}

/// An answer to [`Prompt::Discard`]: its buttons, and `ui.prompt`'s `answer`.
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
}

/// A path as the status bar shows it: its last two components.
fn short_path(path: &str) -> String {
    let p = Path::new(path);
    match (p.parent().and_then(Path::file_name), p.file_name()) {
        (Some(dir), Some(name)) => format!("{}/{}", dir.to_string_lossy(), name.to_string_lossy()),
        (None, Some(name)) => name.to_string_lossy().into_owned(),
        _ => path.to_owned(),
    }
}

/// How long a screenshot waits for its runs before it takes the window as it is.
pub const SHOT_TIMEOUT: Duration = Duration::from_secs(60);
/// How soon to look again at a run or save the application holds back (`Hooks::hold_runs`), whose
/// letting go wakes nothing.
const HELD_OUTBOX: Duration = Duration::from_millis(30);

/// The application: a session and its views.
pub struct EditorApp {
    pub session: Session,
    pub canvas: Canvas,
    /// The parameter editor (`node.parameters`).
    pub params: params_view::ParamsPanel,
    /// The nodes' own editors (`node.editor`).
    pub value_editors: value_editors::ValueEditors,
    pub tokens: Tokens,
    /// The last command error, shown in the status bar.
    pub last_error: Option<String>,
    /// A refused wire's way through (`wire.convert`'s params and the path's label), offered
    /// beside the error (docs/research/20-convert.md).
    pub convert_offer: Option<(Value, String)>,
    /// The engine connection, when one was started.
    pub link: Option<Link>,
    /// With engines added beside the first: the coordinator the link goes through
    /// ([`engine_view`]).
    pub coordinator: Option<std::sync::Arc<plumcraft_coordinator::Coordinator>>,
    /// The engine's state last frame.
    pub engine: EngineState,
    /// What the Watched panel has read of the watched value.
    pub watched: Watched,
    /// The Watched panel's audio view: the value decoded, its spectrogram, the player.
    pub audio: audio::AudioView,
    /// The Watched panel's picture viewers: images, SVG, documents ([`media`]).
    pub media: media::MediaViews,
    /// The Watched panel's other viewers (array values, JSON tree, PDF pages, bytes, …:
    /// [`viewers`]), shown as tabs beside its own.
    pub panel_pane: viewers::Pane,
    /// Compare: A and B ([`compare_view`]).
    pub compare: compare_view::CompareView,
    /// Full values in memory, shared by every viewer of them.
    fulls: watched::Fulls,
    /// Pop-out windows ([`popouts`]).
    pub popouts: Vec<popouts::PopoutWindow>,
    /// The values on wires ([`wire_view`]).
    pub wires: wire_view::WireView,
    /// Thumbnail textures.
    pub thumbs: Thumbs,
    pub dialogs: Box<dyn FileDialogs>,
    /// What is kept between runs ([`prefs`]). The application gives it a file.
    pub prefs: prefs::Store,
    /// The window title's name for a graph with no file (the demo, a new graph).
    pub document: String,
    /// The question the window is asking, when it is.
    pub prompt: Option<Prompt>,
    /// The run waiting on [`Prompt::Uploads`].
    upload_held: Option<Box<plumcraft_proto::run::RunRequest>>,
    /// Commands a shortcut fired while a text field had focus: they run on the next frame,
    /// after the field, losing focus, has committed what was typed (⌘↩ in a query runs it).
    after_typing: Vec<&'static str>,
    /// The rename window ([`rename_view`]), when open.
    pub rename: Option<rename_view::RenameDialog>,
    /// What Copy or Cut last put on the clipboard.
    pub copied: Option<String>,
    /// Text a command asked to put on the clipboard, until the next frame.
    copy_out: Option<String>,
    /// A rerun asked for while a run was in flight (a watched value's bytes were gone).
    rerun_pending: bool,
    /// The answer to the save in flight.
    saving: Option<Receiver<Result<SaveReply, String>>>,
    /// An unattended screenshot to take.
    pub shot: Option<Shot>,
    title: String,
    /// Control-channel requests from the transport (`--control`), drained every frame.
    pub control_rx: Option<Receiver<plumcraft_automation::server::ControlRequest>>,
    /// Control requests waiting for frames (screenshots, run waits).
    pub control: control::ControlState,
    /// Drawing offscreen ([`offscreen::render`]): control and screenshots are left alone.
    pub(crate) offscreen: bool,
    /// The window may close: unsaved changes were discarded or saved, or a screenshot is done.
    /// Once set, a close request is no longer cancelled.
    pub closing: bool,
    /// Close the window next frame (a quit answered; `invoke` has no egui context).
    close_next: bool,
    /// Reads the engine manifests again ([`EditorApp::with_reloader`]).
    pub(crate) reloader: Option<engine_view::Reloader>,
    /// Starts the engines the Engine menu offers ([`EditorApp::with_engines`]).
    pub connector: Option<engine_view::Connector>,
    /// The Attach… window, while it is open.
    pub attach_form: Option<engine_view::AttachForm>,
    /// Installs engines from GitHub ([`EditorApp::with_installer`]).
    pub(crate) installer: Option<engine_view::Installer>,
    /// The Install Engine… window, while it is open.
    pub install_form: Option<engine_view::InstallForm>,
    /// An install running in the background.
    pub installing: Option<engine_view::Installing>,
    /// The context a new engine link wakes (set every frame).
    pub(crate) repaint: Option<egui::Context>,
    /// The window title's app name: [`APP_TITLE`] unless an application names itself.
    pub app_title: &'static str,
}

impl EditorApp {
    pub fn new(session: Session) -> Self {
        let mut canvas = Canvas::default();
        if !session.graph().nodes.is_empty() {
            canvas.request_fit();
        }
        EditorApp {
            params: params_view::ParamsPanel::default(),
            value_editors: value_editors::ValueEditors::default(),
            app_title: APP_TITLE,
            session,
            canvas,
            tokens: theme::DEFAULT,
            last_error: None,
            convert_offer: None,
            link: None,
            coordinator: None,
            engine: EngineState::Off,
            watched: Watched::default(),
            audio: audio::AudioView::default(),
            panel_pane: viewers::Pane::default(),
            compare: compare_view::CompareView::default(),
            fulls: watched::Fulls::default(),
            popouts: Vec::new(),
            wires: wire_view::WireView::default(),
            media: media::MediaViews::default(),
            thumbs: Thumbs::default(),
            dialogs: Box::new(NativeDialogs::default()),
            prefs: prefs::Store::default(),
            document: "Untitled".into(),
            prompt: None,
            upload_held: None,
            after_typing: Vec::new(),
            rename: None,
            copied: None,
            copy_out: None,
            rerun_pending: false,
            saving: None,
            shot: None,
            title: String::new(),
            control_rx: None,
            control: control::ControlState::default(),
            offscreen: false,
            closing: false,
            close_next: false,
            reloader: None,
            connector: None,
            attach_form: None,
            installer: None,
            install_form: None,
            installing: None,
            repaint: None,
        }
    }

    /// Attach an engine link: its catalogue (`schema`) becomes the session's, and its events
    /// are drained every frame. A catalogue that can't be read is the status bar's error.
    pub fn with_link(mut self, link: Link) -> Self {
        match link.conn.call("schema", json!({})).map(serde_json::from_value::<SchemaReply>) {
            Ok(Ok(catalog)) => self.set_catalog(catalog),
            Ok(Err(e)) => self.last_error = Some(format!("the engine's catalogue: {e}")),
            Err(e) => self.last_error = Some(format!("schema: {}", e.message)),
        }
        self.engine = link.state();
        self.sync_engine();
        let conn = std::sync::Arc::clone(&link.conn);
        self.session.docs.resolver = Some(std::sync::Arc::new(move |r: &DataRef| conn.resolve(r).map(|b| b.bytes.to_vec())));
        self.link = Some(link);
        self
    }

    /// What the session knows of the engine's state.
    pub(crate) fn sync_engine(&mut self) {
        self.session.connected = self.engine.is_connected();
        self.session.caps = if let EngineState::Connected { caps, .. } = &self.engine { caps.clone() } else { Default::default() };
    }

    /// Replace the catalogue (the graph is kept; the canvas rebuilds).
    pub fn set_catalog(&mut self, catalog: SchemaReply) {
        let first = self.session.catalog().nodes.is_empty();
        self.session.set_catalog(catalog);
        self.canvas.invalidate();
        if first && !self.session.graph().nodes.is_empty() {
            // The nodes only get their real sizes now.
            self.canvas.request_fit();
        }
    }

    /// Switch themes.
    pub fn set_theme(&mut self, ctx: &egui::Context, tokens: Tokens) {
        self.tokens = tokens;
        self.prefs.set_theme(tokens.name);
        tokens.apply(ctx);
        self.canvas.invalidate();
        self.audio.recolor(ctx, &tokens);
    }

    /// Keep preferences in `store`, and take the theme it names. An application calls this with
    /// its own ([`prefs::Store::for_app`]).
    pub fn use_prefs(&mut self, store: prefs::Store, ctx: &egui::Context) {
        self.prefs = store;
        if let Some(t) = self.prefs.prefs().theme.as_deref().and_then(theme::by_name) {
            self.set_theme(ctx, t);
        }
        // after the first graph opened: turning it on starts what that graph needs
        if self.prefs.prefs().auto_engines == Some(true) {
            self.run("engine.auto", &json!({"on": true}));
        }
    }

    /// Ask a dialog, starting in the folder `key` last used, and remember where it ended.
    fn ask_dialog(&mut self, key: &str, ask: impl FnOnce(&mut dyn FileDialogs) -> Option<PathBuf>) -> Option<PathBuf> {
        self.dialogs.start_in(self.prefs.dir(key));
        let chosen = ask(self.dialogs.as_mut());
        if let Some(p) = &chosen {
            self.prefs.remember(key, p);
        }
        chosen
    }

    /// Keep Pick Engines Automatically in the preferences, however it was set (the menu, the
    /// control channel). Unset stays unset until it is turned on.
    fn save_auto_engines(&mut self) {
        let on = self.session.engines.auto;
        if self.prefs.prefs().auto_engines.unwrap_or(false) != on {
            self.prefs.set_auto_engines(on);
        }
    }

    /// Apply a theme asked for by `view.theme`.
    fn pump_theme(&mut self, ctx: &egui::Context) {
        let Some(name) = self.session.views.theme_asked.take() else { return };
        match theme::by_name(&name) {
            Some(t) => self.set_theme(ctx, t),
            None => self.last_error = Some(format!("no theme called {name:?}")),
        }
    }

    /// Install the theme, fonts and icon loaders on a context (call once at startup).
    pub fn setup_context(&self, ctx: &egui::Context) {
        self.tokens.apply(ctx);
        fonts::install(ctx);
        icons::ensure_loaders(ctx);
    }

    /// Run a command, keeping its error for the status bar. A watch that succeeds runs the
    /// graph (when it can), so the Watched panel fills.
    pub fn run(&mut self, id: &str, params: &Value) -> Option<Value> {
        self.try_run(id, params).ok()
    }

    /// [`EditorApp::run`], returning the error too (the control channel's `command.run`).
    pub fn try_run(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let result = self.session.execute(id, params);
        // an offer lasts until the next command
        self.convert_offer = None;
        match result {
            Ok(v) => {
                self.last_error = None;
                if id == "wire.convert"
                    && let Some(w) = v.get("warning").and_then(Value::as_str)
                {
                    self.last_error = Some(w.to_owned());
                }
                if id == "graph.new" || id == "graph.open" {
                    if id == "graph.new" {
                        self.document = "Untitled".into();
                    }
                    if let Some(n) = v.get("problemCount").and_then(Value::as_u64).filter(|n| *n > 0) {
                        let first = v.get("problems").and_then(|p| p.get(0)).and_then(Value::as_str).unwrap_or_default();
                        self.last_error = Some(format!("opened with {n} problem(s): {first}"));
                    }
                    // what's missing is the banner's, for as long as it is; that says more than
                    // the first node it left unknown
                    if v.get("missingEngines").is_some() {
                        self.last_error = None;
                    }
                    // a graph with no file of its own is still known by what it came from
                    if let Some(from) = v.get("openedFrom").and_then(Value::as_str) {
                        self.document = Path::new(from).file_name().map_or_else(|| from.to_owned(), |n| n.to_string_lossy().into_owned());
                    }
                    self.canvas.request_fit();
                    // nothing of the old graph stays selected, entered or watched
                    self.canvas.picked.clear();
                    self.canvas.leave_group();
                    self.watched.clear();
                }
                if id == "run.watch" {
                    self.watched.clear();
                    // only what this node needs: a click to look at it runs nothing downstream
                    if self.enabled("run.start") {
                        let node = v.get("node").cloned().unwrap_or(Value::Null);
                        self.run("run.start", &json!({"targets": [node]}));
                    }
                }
                if id == "bundle.detach" {
                    let unresolved: Vec<&str> = v.get("unresolved").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
                    if !unresolved.is_empty() {
                        self.last_error = Some(format!("detached, but these came from wires upstream and weren't copied: {}", unresolved.join(", ")));
                    }
                }
                if id == "viewer.recipe" && params.get("copy").and_then(Value::as_bool) == Some(true) {
                    self.copy_out = v.get("recipe").filter(|r| !r.is_null()).and_then(|r| serde_json::to_string_pretty(r).ok());
                }
                if id == "graph.bundle"
                    && let Some(n) = v.get("missing").and_then(Value::as_array).map(Vec::len).filter(|n| *n > 0)
                {
                    self.last_error = Some(format!("bundled, but {n} path(s) couldn't go in: {}", v["missing"][0]["path"].as_str().unwrap_or_default()));
                }
                if id == "node.expand"
                    && let Some(said) = expand_note(&v)
                {
                    self.last_error = Some(said);
                }
                // The session recorded an engine switch: perform it now, so the caller gets
                // its outcome (crate::engine_view). A pick can take steps (out, then in again
                // the new way); the caller gets the last, and how the pick was taken.
                let mut done = None;
                while let Some(outcome) = self.pump_engine() {
                    done = Some(outcome?);
                }
                match done {
                    Some(mut out) => {
                        if let (Some(how), Some(o)) = (v.get("how"), out.as_object_mut()) {
                            o.insert("how".into(), how.clone());
                        }
                        Ok(out)
                    }
                    None => Ok(v),
                }
            }
            Err(e) => {
                let e = e.to_string();
                self.last_error = Some(e.clone());
                if id == "wire.connect" {
                    self.convert_offer = self.offer_for(params);
                }
                Err(e)
            }
        }
    }

    /// What `wire.convert` would place for a refused `wire.connect`, if anything.
    fn offer_for(&self, params: &Value) -> Option<(Value, String)> {
        let port = |k: &str| -> Option<plumcraft_graph::PortRef> {
            let v = params.get(k)?;
            Some(plumcraft_graph::PortRef::new(plumcraft_graph::NodeId(v.get("node")?.as_str()?.to_owned()), v.get("port")?.as_str()?))
        };
        let (from, to) = (port("from")?, port("to")?);
        let best = plumcraft_session::convert::offers(&self.session, &from, &to).into_iter().next()?;
        let label = best.get("label")?.as_str()?.to_owned();
        let p = json!({"from": params["from"], "to": params["to"], "kinds": best["kinds"]});
        Some((p, label))
    }

    fn run_all(&mut self, actions: Vec<Action>) {
        for (id, mut params) in actions {
            // Open with… "Choose an App…": the host asks for the app, then runs value.open
            if id == "value.open" && params.get("choose").and_then(Value::as_bool) == Some(true) {
                let Some(app) = self.dialogs.pick_app() else { continue };
                if let Some(o) = params.as_object_mut() {
                    o.remove("choose");
                    o.insert("app".into(), Value::from(app.display().to_string()));
                }
            }
            // Save Result…: the host asks where, then runs run.save
            if id == "run.save" && params.get("ask").and_then(Value::as_bool) == Some(true) {
                self.ask_where_to_save(params);
                continue;
            }
            self.run(id, &params);
        }
    }

    /// A save button: say why not, or ask where (offering the node's title as the name, where
    /// the last save went or else in the workflow's folder), then run `run.save`.
    fn ask_where_to_save(&mut self, mut params: Value) {
        if let Some(c) = plumcraft_session::find("run.save")
            && let Err(why) = (c.enabled)(&self.session)
        {
            self.last_error = Some(format!("can't save: {why}"));
            return;
        }
        let node = plumcraft_graph::NodeId(params.get("node").and_then(Value::as_str).unwrap_or_default().to_owned());
        let index = params.get("index").and_then(Value::as_i64);
        let sheet = params.get("sheet").and_then(Value::as_bool).unwrap_or(false);
        let (catalog, title) = (self.session.catalog(), plumcraft_session::node_title(self.session.graph(), self.session.catalog(), &node));
        let stem = if sheet { format!("{title} sheet") } else { title };
        let name = plumcraft_session::save_cmds::suggested_name(catalog, &stem, index);
        let base = paths::base_folder(self.session.path());
        let last = self.session.save.last.as_ref().filter(|(_, r)| r.ok).and_then(|(ask, _)| Path::new(&ask.path).parent().map(Path::to_path_buf));
        // the last save's folder, then the one a save last used in an earlier run
        let folder = last.or_else(|| self.prefs.dir(prefs::keys::RESULT_WRITE)).or_else(|| base.clone()).unwrap_or_default();
        let formats = catalog.save_formats.clone();
        let Some(path) = self.ask_dialog(prefs::keys::RESULT_WRITE, |d| d.save_output(&formats, &folder.join(name))) else { return };
        // (a `/` typed into macOS's save panel comes back as `:`: folders)
        let path = paths::chosen_path(&path, base.as_deref(), true);
        if let Some(o) = params.as_object_mut() {
            o.remove("ask");
            o.insert("path".into(), Value::from(path));
        }
        self.run("run.save", &params);
    }

    /// Send a queued save on a thread of its own, and take its answer when it comes.
    fn pump_saves(&mut self, ctx: &egui::Context) {
        for req in self.session.save.take_outbox() {
            let Some(conn) = self.link.as_ref().map(|l| std::sync::Arc::clone(&l.conn)) else {
                self.session.save.fail("the engine is not connected");
                continue;
            };
            let pending = serde_json::to_value(&req).map(|b| conn.uploads_pending(&b)).unwrap_or_default();
            if !pending.is_empty() {
                self.session.save.fail("this save would send files to the engine: allow them, then save again");
                self.prompt = Some(Prompt::Uploads(pending));
                continue;
            }
            let (tx, rx) = channel();
            let wake = ctx.clone();
            let sent = std::thread::Builder::new().name("plumcraft-save".into()).spawn(move || {
                let answer = serde_json::to_value(&req)
                    .map_err(|e| e.to_string())
                    .and_then(|body| conn.call("output.save", body).map_err(|e| e.message))
                    .and_then(|v| serde_json::from_value::<SaveReply>(v).map_err(|e| format!("the save reply: {e}")));
                let _ = tx.send(answer);
                wake.request_repaint();
            });
            match sent {
                Ok(_) => self.saving = Some(rx),
                Err(e) => self.session.save.fail(&e.to_string()),
            }
        }
        if !self.session.save.outbox_is_empty() {
            ctx.request_repaint_after(HELD_OUTBOX);
        }
        let Some(rx) = &self.saving else { return };
        match rx.try_recv() {
            Ok(Ok(reply)) => self.session.save.apply_reply(reply),
            Ok(Err(why)) => self.session.save.fail(&why),
            Err(TryRecvError::Disconnected) => self.session.save.fail("the save stopped without an answer"),
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
        }
        self.saving = None;
    }

    /// Is command `id` enabled now?
    pub fn enabled(&self, id: &str) -> bool {
        plumcraft_session::find(id).is_some_and(|c| (c.enabled)(&self.session).is_ok())
    }

    /// `run.start`, when it is enabled (an engine connected, a graph to run).
    pub fn start_run_if_enabled(&mut self) -> Option<Value> {
        if self.enabled("run.start") { self.run("run.start", &json!({})) } else { None }
    }

    /// Run a command from a menu or shortcut, filling in what the UI knows: file dialogs for
    /// open and save, a prompt before unsaved changes are dropped, the canvas selection for
    /// node commands.
    pub fn invoke(&mut self, id: &str) {
        let nodes: Vec<String> = self.canvas.selection().into_iter().map(|n| n.0).collect();
        match id {
            "graph.new" | "graph.open" => {
                let then = if id == "graph.new" { AfterDiscard::New } else { AfterDiscard::Open };
                if self.session.is_dirty() {
                    self.prompt = Some(Prompt::Discard { then });
                } else {
                    self.go_on(then, false);
                }
            }
            "app.quit" => self.quit(),
            "engine.install" => {
                if self.install_form.is_none() {
                    self.install_form = Some(engine_view::InstallForm::default());
                }
            }
            "engine.attach" => {
                if self.attach_form.is_none() {
                    self.attach_form = Some(engine_view::AttachForm { url: "http://127.0.0.1:".into(), token: String::new() });
                }
            }
            "graph.save" if self.session.path().is_some() => {
                self.run(id, &json!({}));
            }
            "graph.save" | "graph.saveAs" => {
                let current = self.session.path().map(Path::to_path_buf);
                if let Some(path) = self.ask_dialog(prefs::keys::WORKFLOW_WRITE, |d| d.save_workflow(current.as_deref())) {
                    self.run("graph.saveAs", &json!({"path": path}));
                }
            }
            "graph.bundle" => {
                let current = self.session.path().map(Path::to_path_buf);
                if let Some(path) = self.ask_dialog(prefs::keys::BUNDLE_WRITE, |d| d.save_bundle(current.as_deref())) {
                    self.run(id, &json!({"path": path}));
                }
            }
            "group.create" => {
                let groups = self.canvas.selected_groups();
                if nodes.is_empty() && groups.is_empty() {
                    self.last_error = Some("select the nodes to group first".into());
                } else {
                    self.run(id, &json!({"nodes": nodes, "groups": groups}));
                }
            }
            "node.expand" => match nodes.as_slice() {
                [one] => {
                    self.run(id, &json!({"node": one}));
                }
                _ => self.last_error = Some("select one node to expand".into()),
            },
            "group.ungroup" => match self.group_to_ungroup() {
                Some(g) => {
                    self.run(id, &json!({"group": g}));
                }
                None => self.last_error = Some("select a group (or a node in one) to ungroup".into()),
            },
            "graph.insert" | "graph.insertGroup" => {
                if let Some(path) = self.ask_dialog(prefs::keys::WORKFLOW_READ, |d| d.open_workflow()) {
                    let center = self.canvas.to_global.inverse() * self.canvas.rect.center();
                    let pos = if self.canvas.rect.is_finite() { Some([center.x, center.y]) } else { None };
                    let mut p = json!({"path": path});
                    if let Some(pos) = pos {
                        p["pos"] = json!(pos);
                    }
                    if let Some(r) = self.run(id, &p) {
                        self.canvas.select_result(&r, self.session.graph());
                        if let Some(dropped) = r.get("dropped").and_then(Value::as_array).filter(|d| !d.is_empty()) {
                            self.last_error = Some(format!("inserted; {} wire(s) no longer fit and were left out", dropped.len()));
                        }
                    }
                }
            }
            "frame.add" => self.frame_selection(),
            "graph.tidy" => {
                // the selection when there is one to arrange, else everything; with the sizes and
                // pins the canvas measured last frame
                let (sizes, pins) = self.canvas.measured_for_tidy(self.session.catalog());
                let nodes = if nodes.len() >= 2 { nodes } else { Vec::new() };
                if self.run(id, &json!({"nodes": nodes, "sizes": sizes, "pins": pins})).is_some() {
                    self.canvas.request_fit();
                }
            }
            "node.remove" | "node.duplicate" => {
                if nodes.is_empty() {
                    self.last_error = Some("select nodes first".into());
                } else if let Some(r) = self.run(id, &json!({"nodes": nodes}))
                    && id == "node.duplicate"
                {
                    // The copies become the selection (crate::edit_view).
                    self.canvas.select_result(&r, self.session.graph());
                }
            }
            // an application's command gets what the canvas has selected, and may answer with a
            // word for the status line
            added if plumcraft_session::commands::is_added(added) => {
                let group = self.group_to_ungroup();
                if let Some(said) =
                    self.run(added, &json!({"nodes": nodes, "group": group})).and_then(|r| r.get("notice").and_then(Value::as_str).map(str::to_owned))
                {
                    self.last_error = Some(said);
                }
            }
            _ => {
                self.run(id, &Value::Null);
            }
        }
    }

    /// What Ungroup acts on: the first selected collapsed group, else the innermost group of the
    /// first selected node in one, else the group entered.
    pub fn group_to_ungroup(&self) -> Option<plumcraft_graph::GroupId> {
        let g = self.session.graph();
        self.canvas
            .selected_groups()
            .into_iter()
            .find(|id| g.group(id).is_some())
            .or_else(|| self.canvas.selection().iter().find_map(|n| g.group_of(n).cloned()))
            .or_else(|| self.canvas.focus.clone().filter(|f| g.group(f).is_some()))
    }

    /// Quit: close the window, asking first over unsaved changes.
    pub fn quit(&mut self) {
        if self.session.is_dirty() && !self.closing {
            self.prompt = Some(Prompt::Discard { then: AfterDiscard::Quit });
        } else {
            self.closing = true;
            self.close_next = true;
        }
    }

    /// Answer the open prompt: Discard goes on (dropping the unsaved changes); Save saves (the
    /// dialog first for a graph with no file) and goes on once nothing is left unsaved; Cancel
    /// does nothing. False when no prompt was open.
    pub fn answer_prompt(&mut self, answer: PromptAnswer) -> bool {
        match (self.prompt.take(), answer) {
            (None, _) => return false,
            (Some(Prompt::Discard { then }), PromptAnswer::Discard) => self.go_on(then, true),
            (Some(Prompt::Discard { then }), PromptAnswer::Save) => {
                self.invoke("graph.save");
                if !self.session.is_dirty() {
                    self.go_on(then, false);
                }
            }
            (Some(Prompt::Discard { .. }), PromptAnswer::Cancel) => {}
            (Some(Prompt::Review(_)), PromptAnswer::Trust) => {
                self.run("graph.trust", &json!({}));
            }
            (Some(Prompt::Review(_)), PromptAnswer::Cancel) => self.session.run.decline_review(),
            (Some(Prompt::Uploads(pending)), PromptAnswer::Trust) => {
                let files: Vec<String> = pending.into_iter().map(|(_, f)| f).collect();
                if let Some(link) = &self.link {
                    link.conn.allow_uploads(&files);
                }
                if let Some(req) = self.upload_held.take() {
                    self.session.run.resend(req);
                }
            }
            (Some(Prompt::Uploads(_)), PromptAnswer::Cancel) => {
                if self.upload_held.take().is_some() {
                    self.session.run.cancel();
                }
            }
            (p, _) => {
                self.prompt = p;
                return false;
            }
        }
        true
    }

    /// The window's close button, Cmd+Q from the system menu, or a quit answered: each frame,
    /// before anything else. A close asked for over unsaved changes is cancelled, and the
    /// prompt asks instead.
    fn handle_close(&mut self, ctx: &egui::Context) {
        if self.close_next {
            self.close_next = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !ctx.input(|i| i.viewport().close_requested()) || self.closing {
            return;
        }
        if self.session.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.prompt.is_none() {
                self.prompt = Some(Prompt::Discard { then: AfterDiscard::Quit });
            }
        } else {
            self.closing = true;
        }
    }

    /// New, or Open through the dialog, once unsaved changes may go.
    fn go_on(&mut self, then: AfterDiscard, discard: bool) {
        match then {
            AfterDiscard::New => {
                self.run("graph.new", &json!({"discard": discard}));
            }
            AfterDiscard::Open => {
                if let Some(path) = self.ask_dialog(prefs::keys::WORKFLOW_READ, |d| d.open_workflow()) {
                    self.run("graph.open", &json!({"path": path, "discard": discard}));
                }
            }
            AfterDiscard::Quit => {
                self.closing = true;
                self.close_next = true;
            }
        }
    }

    /// The prompt, as a modal over the window.
    fn prompt_modal(&mut self, ctx: &egui::Context) {
        let Some(prompt) = &self.prompt else { return };
        if let Prompt::Review(review) = prompt {
            let review = review.clone();
            return self.review_modal(ctx, &review);
        }
        if let Prompt::Uploads(files) = prompt {
            let files = files.clone();
            return self.uploads_modal(ctx, &files);
        }
        let text = prompt.text();
        let mut answer = None;
        let modal =
            egui::Modal::new(egui::Id::new("plumcraft-prompt")).frame(egui::Frame::popup(&ctx.global_style()).fill(self.tokens.panel)).show(ctx, |ui| {
                ui.set_max_width(360.0);
                ui.heading(egui::RichText::new("Unsaved changes").color(self.tokens.text));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(text).color(self.tokens.text));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").on_hover_text("Save the graph, then go on").clicked() {
                        answer = Some(PromptAnswer::Save);
                    }
                    if ui.button("Discard").on_hover_text("Drop the unsaved changes and go on").clicked() {
                        answer = Some(PromptAnswer::Discard);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(PromptAnswer::Cancel);
                    }
                });
            });
        if modal.should_close() && answer.is_none() {
            answer = Some(PromptAnswer::Cancel);
        }
        if let Some(a) = answer {
            self.answer_prompt(a);
        }
    }

    /// [`Prompt::Uploads`], as a modal: each file, and where it would go.
    fn uploads_modal(&mut self, ctx: &egui::Context, pending: &[(String, String)]) {
        let text = self.tokens.text;
        let mut answer = None;
        let frame = egui::Frame::popup(&ctx.global_style()).fill(self.tokens.panel);
        let modal = egui::Modal::new(egui::Id::new("plumcraft-uploads")).frame(frame).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading(egui::RichText::new("Send files to this engine?").color(text));
            ui.add_space(6.0);
            ui.label(egui::RichText::new(Prompt::Uploads(Vec::new()).text()).color(text));
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                let mut to = None;
                for (engine, file) in pending {
                    if to != Some(engine) {
                        ui.label(egui::RichText::new(format!("To {engine}")).color(text).strong());
                        to = Some(engine);
                    }
                    ui.label(egui::RichText::new(file).color(text).monospace());
                }
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Send Files").on_hover_text("Send these files, now and on later runs").clicked() {
                    answer = Some(PromptAnswer::Trust);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(PromptAnswer::Cancel);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(PromptAnswer::Cancel);
        }
        if let Some(a) = answer {
            self.answer_prompt(a);
        }
    }

    /// [`Prompt::Review`], as a modal: what the workflow reads, writes and does, and which engines
    /// it uses. A path another node sets is said to be, since nothing here can know it.
    fn review_modal(&mut self, ctx: &egui::Context, r: &plumcraft_session::trust::Review) {
        use plumcraft_session::trust::Touch;
        let (text, weak) = (self.tokens.text, self.tokens.text_dim);
        let mut answer = None;
        let frame = egui::Frame::popup(&ctx.global_style()).fill(self.tokens.panel);
        let modal = egui::Modal::new(egui::Id::new("plumcraft-review")).frame(frame).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading(egui::RichText::new("Review before running").color(text));
            ui.add_space(6.0);
            ui.label(egui::RichText::new(Prompt::Review(Box::default()).text()).color(text));
            ui.add_space(8.0);
            let touches = |ui: &mut egui::Ui, title: &str, list: &[Touch]| {
                if list.is_empty() {
                    return;
                }
                ui.label(egui::RichText::new(title).color(text).strong());
                for t in list {
                    let line = match (&t.path, &t.set_by) {
                        (Some(p), _) => format!("{}  ({})", p, t.title),
                        (None, by) => format!("set by {} ({}.{})", by.as_deref().unwrap_or("another node"), t.title, t.port),
                    };
                    ui.label(egui::RichText::new(line).color(text).monospace());
                }
                ui.add_space(6.0);
            };
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                touches(ui, "Writes", &r.writes);
                touches(ui, "Reads", &r.reads);
                if !r.actions.is_empty() {
                    ui.label(egui::RichText::new("Nodes that act").color(text).strong());
                    for a in &r.actions {
                        ui.label(egui::RichText::new(format!("{}  ({})", a.title, a.kind)).color(text).monospace());
                    }
                    ui.add_space(6.0);
                }
                if !r.unknown.is_empty() {
                    ui.label(egui::RichText::new(format!("Unknown node types: {}", r.unknown.join(", "))).color(text));
                }
                if r.more > 0 {
                    ui.label(egui::RichText::new(format!("and {} more", r.more)).color(weak));
                }
            });
            if !r.engines.is_empty() {
                ui.label(egui::RichText::new(format!("Engines: {}", r.engines.join(", "))).color(weak));
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Trust").on_hover_text("Remember this workflow as safe to run; it runs when you next run it").clicked() {
                    answer = Some(PromptAnswer::Trust);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(PromptAnswer::Cancel);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(PromptAnswer::Cancel);
        }
        if let Some(a) = answer {
            self.answer_prompt(a);
        }
    }

    /// A blob's bytes through the engine link (nothing without one).
    fn resolver(&self) -> impl Fn(&DataRef) -> Option<Blob> + 'static {
        let conn = self.link.as_ref().map(|l| std::sync::Arc::clone(&l.conn));
        move |r: &DataRef| conn.as_ref().and_then(|c| c.resolve(r))
    }

    /// A full value's bytes, shared with whatever else holds them ([`watched::Fulls`]).
    fn full_reader(&self) -> impl Fn(&DataRef) -> Option<std::sync::Arc<Blob>> + 'static {
        let (fulls, resolve) = (self.fulls.clone(), self.resolver());
        move |r: &DataRef| fulls.read(r, &resolve)
    }

    /// Give the compare view A and B while it shows; it lets go of them when it doesn't.
    fn sync_compare(&mut self, ctx: &egui::Context) {
        use plumcraft_session::viewer_cmds::comparing;
        if !comparing(&self.session) {
            self.compare.clear();
            return;
        }
        let resolve = self.resolver();
        let full = self.full_reader();
        let nan = self.tokens.error;
        let sides = compare_view::sides(&self.session);
        let state = self.session.views.compare.clone();
        if self.compare.sync(ctx, &state, sides, &mut self.thumbs, &resolve, &full, nan) {
            self.rerun_pending = true;
        }
    }

    /// Engine housekeeping, every frame: the engine's state, its events into the session.
    pub fn drain_engine(&mut self) {
        let Some(link) = &self.link else {
            self.session.connected = false;
            self.session.docs.resolver = None;
            return;
        };
        self.engine = link.state();
        self.sync_engine();
        let Some(link) = &self.link else { return };
        let conn = std::sync::Arc::clone(&link.conn);
        self.session.docs.resolver = Some(std::sync::Arc::new(move |r: &DataRef| conn.resolve(r).map(|b| b.bytes.to_vec())));
        let events = link.drain();
        let now = Instant::now();
        for e in &events {
            if self.session.run.apply_event(e, now) && matches!(e.event, Event::RunReply(_)) {
                // Reaped thumbnails were published again by this run: try them again.
                self.thumbs.forget_misses();
            }
        }
        if self.session.run.running && matches!(self.engine, EngineState::EngineDown { .. } | EngineState::Unavailable { .. }) {
            self.session.run.fail(&format!("the engine went away: {}", link::state_text(&self.engine)));
        }
    }

    /// Run housekeeping, every frame: start a due auto-run, send what the session queued, and
    /// keep the Watched panel's value read.
    pub fn pump_runs(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        match self.session.run.auto_due_in(now) {
            Some(d) if d.is_zero() => {
                // (when nothing can run now, no engine or an empty graph, it stays due and runs
                // on a later frame once it can; no repaint is asked for meanwhile)
                self.start_run_if_enabled();
            }
            Some(d) => ctx.request_repaint_after(d),
            None => {}
        }
        if self.session.run.awaiting_review() && self.prompt.is_none() {
            self.prompt = Some(Prompt::Review(Box::new(self.session.review())));
            ctx.request_repaint();
        }
        for out in self.session.run.take_outbox() {
            match out {
                Outgoing::Run(req) => {
                    // files for an engine nobody here started: ask first, and send this run after
                    let pending = match (&self.link, serde_json::to_value(&*req)) {
                        (Some(link), Ok(body)) => link.conn.uploads_pending(&body),
                        _ => Vec::new(),
                    };
                    if !pending.is_empty() {
                        self.upload_held = Some(req);
                        self.prompt = Some(Prompt::Uploads(pending));
                        ctx.request_repaint();
                        continue;
                    }
                    let sent = match (&self.link, serde_json::to_value(&*req)) {
                        (Some(link), Ok(body)) => link.conn.call("run", body).map_err(|e| e.message),
                        (None, _) => Err("the engine is not connected".into()),
                        (_, Err(e)) => Err(e.to_string()),
                    };
                    if let Err(why) = sent {
                        self.session.run.fail(&format!("run: {why}"));
                    }
                }
                Outgoing::Release(scope) => {
                    if let Some(link) = &self.link {
                        // an engine without the capability has nothing to release
                        let _ = link.conn.call("graph.release", json!({"scope": scope}));
                    }
                }
                Outgoing::Cancel(run_id) => {
                    if let Some(link) = &self.link {
                        // the run's late records are ignored by generation whatever the reply
                        let _ = link.conn.call("run.cancel", json!({"run_id": run_id}));
                    }
                }
            }
        }
        if !self.session.run.outbox_is_empty() {
            ctx.request_repaint_after(HELD_OUTBOX);
        }
        self.pump_saves(ctx);
        let resolve = self.resolver();
        let full = self.full_reader();
        let display = self.session.run.panel().and_then(|(_, d)| d);
        if self.watched.update(display, &full) {
            self.rerun_pending = true;
        }
        self.audio.sync(display, self.watched.full.as_deref());
        self.sync_media(ctx);
        let offered = plumcraft_session::run_cmds::offered_views(&self.session);
        let panel_display = self.session.run.panel().and_then(|(_, d)| d).cloned();
        self.panel_pane.chosen.clone_from(&self.session.views.panel_viewer);
        self.panel_pane.sync(ctx, panel_display.as_ref(), offered, &mut self.thumbs, &resolve, &full, Default::default());
        self.sync_compare(ctx);
        self.sync_popouts(ctx);
        self.cache_displays();
        // an "Open in …" waiting for its value (session doc.open)
        match plumcraft_session::doc_cmds::poll_doc_open(&mut self.session) {
            Some(Err(e)) => self.last_error = Some(e),
            Some(Ok(_)) => self.last_error = None,
            None => {}
        }
        // a document saved in its app: fed downstream and run (session doc_edits)
        match plumcraft_session::doc_edits::poll_doc_edits(&mut self.session, std::time::Instant::now()) {
            Some(Err(e)) => self.last_error = Some(e),
            Some(Ok(_)) => self.last_error = None,
            None => {}
        }
        if plumcraft_session::doc_edits::watching(&self.session) {
            ctx.request_repaint_after(plumcraft_session::doc_edits::POLL);
        }
        // an Open or Open with… waiting for its value (session value.open)
        match plumcraft_session::open_cmds::poll_value_open(&mut self.session) {
            Some(Err(e)) => self.last_error = Some(e),
            Some(Ok(_)) => self.last_error = None,
            None => {}
        }
        if self.rerun_pending && !self.session.run.running {
            self.rerun_pending = false;
            self.start_run_if_enabled();
        }
        if self.session.run.running {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// Give the watched display to the picture viewers, with its rendition's texture, and set
    /// the document view's "Open in …" (enabled, or disabled with why).
    fn sync_media(&mut self, ctx: &egui::Context) {
        let resolve = self.resolver();
        let display = self.session.run.panel().and_then(|(_, d)| d);
        // (an array's pictures are the viewer's, below: a second texture of one would be 48 MB)
        let rendition = display.filter(|d| d.media != plumcraft_proto::catalogue::Media::Ndarray).and_then(|d| {
            d.renditions.iter().find_map(|r| match r {
                plumcraft_proto::display::Rendition::Rgba8 { data, .. } => self.thumbs.get(ctx, data, &resolve).ok(),
                _ => None,
            })
        });
        // the probe and any zoom: compare's while it shows, else the panel's own picture viewer's
        let tools = self.session.views.tools;
        self.session.views.tools.zoom = None;
        let comparing = plumcraft_session::viewer_cmds::comparing(&self.session);
        self.compare.tools.take(if comparing { tools } else { Tools { zoom: None, ..tools } });
        let to_media = if comparing { Tools { zoom: None, ..tools } } else { tools };
        self.media.image.tools.take(to_media);
        self.media.array.tools.take(to_media);
        let display = self.session.run.panel().and_then(|(_, d)| d);
        self.media.sync(ctx, display, self.watched.full.as_ref(), rendition.as_ref());
        // An array: every picture of its view, captioned, and the views its wire type offers.
        let pictures: Vec<(String, egui::TextureHandle)> = display
            .filter(|d| d.media == plumcraft_proto::catalogue::Media::Ndarray)
            .map(|d| {
                d.renditions
                    .iter()
                    .filter_map(|r| match r {
                        plumcraft_proto::display::Rendition::Rgba8 { data, role, caption, .. } if role == "view" => {
                            self.thumbs.picture(ctx, data, &resolve).ok().map(|t| (caption.clone(), t))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let array = display.filter(|d| d.media == plumcraft_proto::catalogue::Media::Ndarray);
        self.media.array.sync(array, pictures, self.watched.full.as_ref(), self.watched.missing);
        self.media.array.offered = plumcraft_session::run_cmds::offered_views(&self.session);
        self.media.doc.open = match (display, self.session.run.panel().map(|(w, _)| w)) {
            (Some(d), Some(w)) if d.media == plumcraft_proto::catalogue::Media::Doc => {
                let label = self.session.docs.for_wire(&d.ty).map(|a| a.label.clone());
                label.map(|label| {
                    let blocked = plumcraft_session::doc_cmds::open_blocker(&self.session, &w.node, Some(&w.port)).err();
                    let note = match &self.session.docs.last {
                        Some(Ok(o)) if o.node == w.node && o.port == w.port => Some(format!("opened a copy: {}", o.path.display())),
                        Some(Err(e)) => Some(e.clone()),
                        _ => None,
                    };
                    media::OpenButton { label, blocked, note }
                })
            }
            _ => None,
        };
    }

    /// The window title's name: the workflow's file name, or [`EditorApp::document`] without one.
    pub fn document_name(&self) -> String {
        self.session.path().and_then(Path::file_name).map_or_else(|| self.document.clone(), |n| n.to_string_lossy().into_owned())
    }

    /// The window title: the workflow's name and a dot while it has unsaved changes.
    pub fn window_title(&self) -> String {
        let dot = if self.session.is_dirty() { " •" } else { "" };
        match self.engine_title() {
            Some(engine) => format!("{}{dot} — {} ({engine})", self.document_name(), self.app_title),
            None => format!("{}{dot} — {}", self.document_name(), self.app_title),
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.prompt.is_some() {
            return;
        }
        // last frame's, from a text field that has committed since
        for id in std::mem::take(&mut self.after_typing) {
            self.invoke(id);
        }
        let mut fired = Vec::new();
        // egui matches Shift and Alt as wildcards when a shortcut doesn't name them, so the most
        // specific go first: Cmd+Shift+Z is Redo, not Undo; Cmd+Shift+S is Save As, not Save.
        let mut specs: Vec<(&CommandSpec, egui::KeyboardShortcut)> =
            command_specs().iter().filter_map(|c| Some((c, c.shortcut.and_then(parse_shortcut)?))).collect();
        specs.sort_by_key(|(_, sc)| std::cmp::Reverse(u8::from(sc.modifiers.shift) + u8::from(sc.modifiers.alt)));
        // a text field keeps its own undo
        let typing = ctx.text_edit_focused();
        for (spec, sc) in specs {
            if typing && spec.id.starts_with("edit.") {
                continue;
            }
            // consumed even when disabled, so a disabled Redo's keys don't fall through to Undo
            if ctx.input_mut(|i| i.consume_shortcut(&sc)) && (spec.enabled)(&self.session).is_ok() {
                fired.push(spec.id);
            }
        }
        if typing && !fired.is_empty() {
            // the field commits when it loses focus this frame; the commands run after that
            ctx.memory_mut(|m| m.stop_text_input());
            ctx.request_repaint();
            self.after_typing = fired;
            return;
        }
        for id in fired {
            self.invoke(id);
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let mut invoked: Vec<&'static str> = Vec::new();
        let mut actions: Vec<Action> = Vec::new();
        let mut fit = false;
        let mut edit_op = None;
        let mut theme_pick: Option<Tokens> = None;
        let current_theme = self.tokens.name;
        let selected = self.canvas.picked.nodes().len() + self.canvas.picked.groups().len();
        let mut menus: Vec<&str> = MENUS.to_vec();
        for spec in command_specs() {
            if let Some(m) = spec.menu.first()
                && !menus.contains(m)
            {
                menus.push(m);
            }
        }
        egui::MenuBar::new().ui(ui, |ui| {
            for menu in menus {
                ui.menu_button(menu, |ui| {
                    command_menu(ui, &self.session, menu, 1, &mut invoked);
                    // the engines after the menu's own commands: there are many, and they scroll
                    if menu == "Engine" {
                        ui.separator();
                        let mut auto = self.session.engines.auto;
                        let tip = "Start the engines a workflow needs when it is opened or inserted, each hosted the fastest way it can be: in-process (mode 1), then a separate process, then HTTP";
                        if ui.checkbox(&mut auto, "Pick Engines Automatically").on_hover_text(tip).changed() {
                            actions.push(("engine.auto", json!({"on": auto})));
                            ui.close();
                        }
                        if let Some(action) = self.engine_choices_menu(ui) {
                            actions.push(action);
                        }
                    }
                    match menu {
                        "Edit" => edit_op = edit_view::edit_menu(ui, selected),
                        "View" => {
                            let fit_button = egui::Button::new("Fit").shortcut_text("F");
                            if ui.add(fit_button).on_hover_text("Fit the view to all nodes").clicked() {
                                fit = true;
                                ui.close();
                            }
                            ui.menu_button("Theme", |ui| {
                                for t in theme::ALL {
                                    if ui.radio(t.name == current_theme, t.label).clicked() {
                                        theme_pick = Some(*t);
                                        ui.close();
                                    }
                                }
                            });
                        }
                        "Node" => {
                            ui.menu_button("Add", |ui| {
                                if self.session.catalog().nodes.is_empty() {
                                    ui.label("No node types: the engine is not connected.");
                                } else if let Some(kind) = palette::node_menus(ui, self.session.catalog()) {
                                    let center = self.canvas.to_global.inverse() * self.canvas.rect.center();
                                    actions.push(("node.add", json!({"kind": kind, "pos": [center.x, center.y]})));
                                }
                            });
                            if ui.add(egui::Button::new("Search…").shortcut_text("Tab")).clicked() {
                                self.canvas.open_palette(self.canvas.rect.center());
                                ui.close();
                            }
                        }
                        _ => {}
                    }
                });
            }
        });
        if fit {
            self.canvas.request_fit();
        }
        if let Some(t) = theme_pick {
            self.run("view.theme", &json!({"name": t.name}));
        }
        if let Some(op) = edit_op {
            let ctx = ui.ctx().clone();
            self.edit_op(&ctx, op);
        }
        for id in invoked {
            self.invoke(id);
        }
        self.run_all(actions);
    }

    /// What's wrong with the engine and how to fix it, when something is.
    pub fn banner_text(&self) -> Option<String> {
        let with_hint = |what: String, hint: &str| if hint.is_empty() { what } else { format!("{what}. Fix: {hint}") };
        match &self.engine {
            EngineState::EngineDown { reason, hint, stderr_tail, .. } => {
                let tail = stderr_tail.as_deref().filter(|t| !t.is_empty()).map(|t| format!("\n{t}")).unwrap_or_default();
                Some(format!("{}{tail}", with_hint(format!("The engine is down: {reason}"), hint)))
            }
            EngineState::Unreachable { url, reason, hint, .. } => Some(with_hint(format!("The engine at {url} can't be reached: {reason}"), hint)),
            EngineState::Unavailable { reason } => Some(format!("The engine is up but can't run: {reason}")),
            // connected, with something to say: what this graph lacks, and the engine's own notice
            _ => {
                let known = !self.session.catalog().nodes.is_empty();
                let missing = known.then(|| self.session.missing_engines()).flatten().map(|m| m.text);
                let notice = self.session.catalog().text(texts::ENGINE_NOTICE).filter(|n| !n.is_empty()).map(str::to_owned);
                match (missing, notice) {
                    (Some(m), Some(n)) => Some(format!("{m}\n{n}")),
                    (Some(m), None) => Some(m),
                    (None, n) => n,
                }
            }
        }
    }

    fn banner(&self, ui: &mut egui::Ui) {
        if let Some(text) = self.banner_text() {
            egui::Panel::top("plumcraft-banner").frame(egui::Frame::new().fill(self.tokens.banner).inner_margin(egui::Margin::symmetric(10, 6))).show(
                ui,
                |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(text).color(self.tokens.text)).wrap());
                },
            );
        }
    }

    /// The status bar's engine label.
    pub fn engine_label(&self) -> String {
        match &self.link {
            None => match self.engine_title() {
                Some(wanted) => format!("engine: {wanted}"),
                None => "engine: not connected".into(),
            },
            Some(link) => {
                let name = &self.session.catalog().engine;
                let who = if name.name.is_empty() { String::new() } else { format!("{} {} · ", name.name, name.version) };
                format!("engine: {who}{} · {}", link.label, link::state_text(&self.engine))
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let mut commands: Vec<(&'static str, Value)> = Vec::new();
        let now = Instant::now();
        ui.horizontal(|ui| {
            let dot = if self.engine.is_connected() {
                self.tokens.state_done
            } else if matches!(self.engine, EngineState::Connecting { .. } | EngineState::LoadingSchema) {
                self.tokens.warning
            } else {
                self.tokens.error
            };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 5.0, dot);
            ui.label(self.engine_label());
            ui.separator();
            if let Some(i) = &self.installing {
                ui.spinner();
                ui.label(format!("installing {}…", i.url.trim_start_matches("https://github.com/")));
                ui.separator();
            }
            // the workflow, as the window title names it (the title isn't in screenshots)
            let dot = if self.session.is_dirty() { " •" } else { "" };
            let doc = ui.colored_label(self.tokens.text, format!("{}{dot}", self.document_name()));
            match self.session.path() {
                Some(p) => doc.on_hover_text(format!("{}{}", p.display(), if dot.is_empty() { "" } else { " (unsaved changes)" })),
                None => doc.on_hover_text("not saved yet"),
            };
            ui.separator();
            let g = self.session.graph();
            ui.colored_label(self.tokens.text_dim, format!("{} nodes, {} wires", g.nodes.len(), g.wires.len()));
            ui.separator();
            let run = &self.session.run;
            if run.running {
                let (done, total) = run.progress();
                ui.add(egui::Spinner::new().size(12.0).color(self.tokens.state_running));
                let secs = run.elapsed(now).map_or(0.0, |d| d.as_secs_f64());
                ui.label(format!("running: {done} of {total} nodes, {secs:.1} s"));
                if ui
                    .small_button("Cancel")
                    .on_hover_text("Stop this run (Esc): the engine stops between nodes or inside a long one, and the rest of its results are ignored")
                    .clicked()
                {
                    commands.push(("run.cancel", Value::Null));
                }
            } else if let Some(wall) = run.last_wall {
                let engine = run.last_reply.as_ref().map(|r| r.elapsed_ms).filter(|ms| *ms > 0.0);
                let text = match engine {
                    Some(ms) => format!("last run {} (engine {})", run_view::format_ms(wall.as_secs_f64() * 1e3), run_view::format_ms(ms)),
                    None => format!("last run {}", run_view::format_ms(wall.as_secs_f64() * 1e3)),
                };
                ui.colored_label(self.tokens.text_dim, text);
                commands.extend(cache_view::status_line(ui, &run.cache, &self.tokens));
            } else {
                ui.colored_label(self.tokens.text_dim, "not run yet");
            }
            let errors = run.errors();
            if errors > 0 {
                let names: Vec<String> = run.nodes.iter().filter(|(_, n)| n.state == Some(NodeState::Error)).map(|(id, _)| id.clone()).collect();
                ui.colored_label(self.tokens.error, if errors == 1 { "1 error".to_owned() } else { format!("{errors} errors") })
                    .on_hover_text(names.join(", "));
            }
            if let Some(e) = &run.run_error {
                ui.colored_label(self.tokens.error, e);
            }
            let auto = run.auto;
            if ui.selectable_label(auto, "Auto-run").on_hover_text("Run 150 ms after every edit").clicked() {
                commands.push(("run.auto", json!({"on": !auto})));
            }
            if let Some(ask) = &self.session.save.pending {
                ui.separator();
                ui.add(egui::Spinner::new().size(12.0).color(self.tokens.state_running));
                ui.label(format!("saving {}…", short_path(&ask.path))).on_hover_text(&ask.path);
            } else if let Some((ask, reply)) = &self.session.save.last {
                ui.separator();
                match (&reply.error, reply.ok) {
                    (None, true) => {
                        let shown = reply.paths.first().map_or(ask.path.as_str(), String::as_str);
                        let text = match (ask.sheet, reply.paths.len()) {
                            (true, _) => format!("saved a sheet of {} as {}", reply.items, short_path(shown)),
                            (false, 0 | 1) => format!("saved {}", short_path(shown)),
                            (false, n) => format!("saved {n} files as {}", short_path(&ask.path)),
                        };
                        let text = if reply.recipe { text } else { format!("{text}, without its recipe") };
                        ui.colored_label(self.tokens.text_dim, text).on_hover_text(reply.paths.join("\n"));
                    }
                    (Some(e), _) => {
                        ui.colored_label(self.tokens.error, format!("not saved: {}", e.message)).on_hover_text(&ask.path);
                    }
                    (None, false) => {
                        ui.colored_label(self.tokens.error, "not saved").on_hover_text(&ask.path);
                    }
                }
            }
            if let Some(err) = &self.last_error {
                ui.separator();
                ui.colored_label(self.tokens.error, err);
            }
            if let Some((p, label)) = &self.convert_offer {
                let tip = format!("wire.convert: place {label} between them, wired, as one undo step");
                if ui.button(format!("Insert {label}")).on_hover_text(tip).clicked() {
                    commands.push(("wire.convert", p.clone()));
                }
            }
        });
        for (id, p) in commands {
            self.run(id, &p);
        }
    }

    fn watched_panel(&mut self, ui: &mut egui::Ui) {
        let frame = egui::Frame::new().fill(self.tokens.panel).inner_margin(egui::Margin::same(8));
        let asks = egui::Panel::right("plumcraft-watched").resizable(true).default_size(340.0).min_size(220.0).frame(frame).show(ui, |ui| {
            let run = &self.session.run;
            let (w, panel_display) = run.panel().map_or((None, None), |(w, d)| (Some(w), d));
            let note = match &self.session.docs.last_value {
                Some(Ok(o)) if w.is_some_and(|w| o.node == w.node && o.port == w.port) => Some(format!("opened a copy in {}: {}", o.app, o.path.display())),
                Some(Err(e)) => Some(e.clone()),
                _ => None,
            };
            let opens = watched::Opens { recent_apps: &self.session.docs.recent_apps, note };
            let chosen = self.session.views.panel_viewer.clone();
            let can_save = plumcraft_session::find("run.save").map_or(Err(String::new()), |c| (c.enabled)(&self.session));
            let save = panel_display.filter(|d| plumcraft_session::save_cmds::takes(self.session.catalog(), d.media)).map(|_| can_save);
            self.watched.ui(
                ui,
                w,
                panel_display,
                run.running,
                &self.tokens,
                &mut self.audio,
                &mut self.media,
                &opens,
                &mut self.panel_pane,
                chosen.as_deref(),
                save,
                &mut self.compare,
                plumcraft_session::viewer_cmds::pinned(&self.session),
            )
        });
        let asks = asks.inner;
        self.run_all(asks);
        if std::mem::take(&mut self.media.doc.clicked)
            && let Some(w) = self.session.run.panel().map(|(w, _)| w.clone())
        {
            self.run("doc.open", &json!({"node": w.node, "port": w.port}));
        }
    }

    /// The node the Watched panel shows.
    pub fn watched_node(&self) -> Option<plumcraft_graph::NodeId> {
        self.session.run.panel().map(|(w, _)| plumcraft_graph::NodeId(w.node.clone()))
    }

    /// Draw one frame into `ui` (the whole window).
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.repaint.is_none() && !self.offscreen {
            self.repaint = Some(ctx.clone());
        }
        if !self.offscreen {
            self.handle_close(&ctx);
            self.drain_control(&ctx);
        }
        if let Some(Err(e)) = self.pump_engine() {
            self.last_error = Some(e);
        }
        self.poll_install();
        self.pump_theme(&ctx);
        self.save_auto_engines();
        if let Some(text) = self.copy_out.take() {
            ctx.copy_text(text.clone());
            self.copied = Some(text);
        }
        self.drain_engine();
        self.edit_input(&ctx);
        self.shortcuts(&ctx);
        self.pump_runs(&ctx);
        egui::Panel::top("plumcraft-menu").show(ui, |ui| self.menu_bar(ui));
        self.banner(ui);
        egui::Panel::bottom("plumcraft-status").show(ui, |ui| self.status_bar(ui));
        self.watched_panel(ui);
        self.canvas.sync(self.session.generation(), self.session.graph(), self.session.catalog());
        let frame = egui::Frame::new().fill(self.tokens.canvas);
        let resolve = self.resolver();
        let watched = self.watched_node();
        let runs = RunView {
            run: &self.session.run,
            thumbs: &mut self.thumbs,
            resolve: &resolve,
            docs: &self.session.docs.apps,
            recent_apps: &self.session.docs.recent_apps,
        };
        let actions = egui::CentralPanel::default()
            .frame(frame)
            .show(ui, |ui| self.canvas.show(ui, self.session.graph(), self.session.catalog(), &self.tokens, runs, watched.as_ref()))
            .inner;
        self.run_all(actions);
        self.show_wire_preview(&ctx);
        // After the canvas: where there is one window (kittests, offscreen pictures) the
        // pop-outs are in-editor windows, which must draw over the nodes.
        self.show_popouts(&ctx);
        // the group windows (a text field can't live in a menu: crate::groups_view)
        let windows = groups_view::rename_window(&ctx, self.session.graph());
        let windows = windows.into_iter().chain(groups_view::parameter_window(&ctx, self.session.graph(), self.session.catalog())).collect();
        self.run_all(windows);
        self.rename_window(&ctx);
        self.browse_for_path();
        self.attach_window(&ctx);
        self.install_window(&ctx);
        self.params_window(&ctx);
        self.value_editor_windows(&ctx);
        if self.session.views.cache_report {
            for (id, p) in cache_view::report(&ctx, &self.session.run, &self.tokens) {
                self.run(id, &p);
            }
        }
        self.prompt_modal(&ctx);
        about_view::show(&ctx, &mut self.session);
        self.pump_runs(&ctx);
        let title = self.window_title();
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        self.frame_end(&ctx);
    }

    /// The end of every frame: the control channel's waiting requests ([`control`]) and an
    /// unattended screenshot's progress.
    pub fn frame_end(&mut self, ctx: &egui::Context) {
        if self.offscreen {
            return;
        }
        control::frame_end(self, ctx);
        self.screenshot(ctx);
    }

    /// Draw the window's contents offscreen, at its size ([`offscreen::render`]).
    pub fn render_offscreen(&mut self, ctx: &egui::Context) -> Result<egui::ColorImage, String> {
        let size = ctx.content_rect().size();
        offscreen::render(self, size, ctx.pixels_per_point(), Some(ctx))
    }

    /// `--screenshot`: run the graph as many times as asked (to see the memo's `cached` nodes),
    /// then save the window after the last reply and close.
    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(mut shot) = self.shot.take() else { return };
        let since = *shot.since.get_or_insert_with(Instant::now);
        let timed_out = since.elapsed() > SHOT_TIMEOUT;
        let idle = !self.session.run.running && self.session.run.outbox_is_empty();
        if idle && shot.started < shot.runs && !timed_out {
            if self.start_run_if_enabled().is_some() {
                shot.started += 1;
            }
            ctx.request_repaint();
        } else if (idle && shot.started >= shot.runs) || timed_out {
            if shot.frames_after_reply == 0 {
                // nodes grew with their results (excerpts, errors): fit them as they are now
                self.canvas.request_fit();
            }
            shot.frames_after_reply += 1;
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if shot.requested_at.is_none() && shot.frames_after_reply >= 8 && !self.canvas.fit_pending() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            shot.requested_at = Some(Instant::now());
        }
        let mut image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                // (the control channel's screenshots carry a token; this one carries none)
                egui::Event::Screenshot { image, user_data, .. } if user_data.data.is_none() => Some(image.clone()),
                _ => None,
            })
        });
        // The window never answers while the screen is locked: draw offscreen instead.
        if image.is_none() && shot.requested_at.is_some_and(|at| at.elapsed() >= control::WINDOW_SHOT_WAIT) {
            match self.render_offscreen(ctx) {
                Ok(img) => image = Some(std::sync::Arc::new(img)),
                Err(e) => {
                    eprintln!("plumcraft-editor: {}: {e}", shot.path.display());
                    self.closing = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
            }
        }
        if let Some(image) = image {
            match save_png(&image, &shot.path) {
                Ok(()) => println!("wrote {}", shot.path.display()),
                Err(e) => eprintln!("plumcraft-editor: {}: {e}", shot.path.display()),
            }
            self.closing = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.shot = Some(shot);
    }

    /// A path widget's "…": run the file dialog and set the chosen path. Relative paths are
    /// relative to the workflow's folder (or, unsaved, the editor's), so the dialog starts there
    /// and a choice inside it is stored relative ([`plumcraft_session::paths`]).
    /// The parameter editor, when it is open on a node.
    /// The rename window: asked for by a menu's Rename… (or a double-click on a frame's title),
    /// closed once applied, or when what it renames goes.
    fn rename_window(&mut self, ctx: &egui::Context) {
        if let Some(asked) = rename_view::take_request(ctx) {
            self.rename = Some(asked);
        }
        let Some(mut dialog) = self.rename.take() else { return };
        if !dialog.target_exists(self.session.graph()) {
            return;
        }
        let (command, open) = dialog.show(ctx);
        if let Some((id, params)) = command {
            self.run(id, &params);
        }
        if open {
            self.rename = Some(dialog);
        }
    }

    fn params_window(&mut self, ctx: &egui::Context) {
        let Some(node) = self.session.views.parameters.clone() else { return };
        let t = self.canvas.to_global;
        let anchor = self.canvas.node_rects_by_id().into_iter().find(|(id, _)| *id == node).map(|(_, r)| egui::Rect::from_min_max(t * r.min, t * r.max));
        let cx = params_view::Context { graph: self.session.graph(), catalog: self.session.catalog(), run: &self.session.run, tokens: &self.tokens, anchor };
        let actions = self.params.show(ctx, &cx, &node);
        self.run_all(actions);
    }

    fn value_editor_windows(&mut self, ctx: &egui::Context) {
        if self.session.views.editors.is_empty() && self.value_editors.is_empty() {
            return;
        }
        let resolve = self.resolver();
        let (actions, note) = self.value_editors.show(ctx, &self.session, &resolve, &self.tokens);
        self.run_all(actions);
        if note.is_some() {
            self.last_error = note;
        }
    }

    fn browse_for_path(&mut self) {
        let asked = self.canvas.edits.take_browse().or_else(|| self.params.edits.take_browse());
        let Some(((node, port), hint, current)) = asked else { return };
        let base = paths::base_folder(self.session.path());
        let start = match &base {
            Some(b) if !current.trim().is_empty() && !paths::is_anchored(current.trim()) => b.join(current.trim()).to_string_lossy().into_owned(),
            _ => paths::expand_user(current.trim()).to_string_lossy().into_owned(),
        };
        if hint.is_many() {
            let start = paths::each_line(&current, |l| match &base {
                Some(b) if !paths::is_anchored(l) => b.join(l).to_string_lossy().into_owned(),
                _ => paths::expand_user(l).to_string_lossy().into_owned(),
            });
            self.dialogs.start_in(self.prefs.dir(prefs::dir_key(&hint)));
            let chosen = self.dialogs.pick_files(&hint, &start);
            if let Some(first) = chosen.first() {
                self.prefs.remember(prefs::dir_key(&hint), first);
                let value: Vec<String> = chosen.iter().map(|p| paths::chosen_path(p, base.as_deref(), false)).collect();
                self.run("node.set", &json!({"node": node, "port": port, "value": value.join("\n")}));
            }
            return;
        }
        if let Some(path) = self.ask_dialog(prefs::dir_key(&hint), |d| d.pick_path(&hint, &start)) {
            let value = paths::chosen_path(&path, base.as_deref(), hint.mode == "save");
            self.run("node.set", &json!({"node": node, "port": port, "value": value}));
        }
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

/// Save a screenshot as PNG.
pub fn save_png(image: &egui::ColorImage, path: &Path) -> Result<(), String> {
    let [w, h] = image.size;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), u32::try_from(w).map_err(|e| e.to_string())?, u32::try_from(h).map_err(|e| e.to_string())?);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&bytes).map_err(|e| e.to_string())
}

/// A toggle command's current state, for a check mark in its menu.
fn checked(session: &Session, id: &str) -> Option<bool> {
    match id {
        "run.auto" => Some(session.run.auto),
        _ => None,
    }
}

/// The menu items of `menu` at `depth` (submenus for longer menu paths). Pushes clicked ids.
fn command_menu(ui: &mut egui::Ui, session: &Session, menu: &str, depth: usize, invoked: &mut Vec<&'static str>) {
    let specs: Vec<&CommandSpec> = command_specs().iter().filter(|c| c.menu.first() == Some(&menu)).collect();
    command_items(ui, session, &specs, depth, invoked);
}

/// What an expansion couldn't carry over, for the status bar. `None` when it all came across.
fn expand_note(v: &Value) -> Option<String> {
    let list = |key: &str| v.get(key).and_then(Value::as_array).map_or(0, Vec::len);
    let mut parts = Vec::new();
    if list("dropped") > 0 {
        let why = v["dropped"][0]["reason"].as_str().unwrap_or_default();
        parts.push(format!("{} wire(s) dropped ({why})", list("dropped")));
    }
    if list("lost") > 0 {
        parts.push(format!("{} group parameter(s) or result(s) lost", list("lost")));
    }
    if list("unresolved") > 0 {
        parts.push(format!("{} setting(s) from wires upstream used the node's own value", list("unresolved")));
    }
    (!parts.is_empty()).then(|| format!("expanded, but: {}", parts.join("; ")))
}

fn command_items(ui: &mut egui::Ui, session: &Session, specs: &[&CommandSpec], depth: usize, invoked: &mut Vec<&'static str>) {
    let mut subs: Vec<&str> = Vec::new();
    for spec in specs {
        match spec.menu.get(depth) {
            None => {
                if menu_item(ui, session, spec) {
                    invoked.push(spec.id);
                }
            }
            Some(sub) if !subs.contains(sub) => subs.push(sub),
            Some(_) => {}
        }
    }
    for sub in subs {
        let inner: Vec<&CommandSpec> = specs.iter().copied().filter(|c| c.menu.get(depth) == Some(&sub)).collect();
        ui.menu_button(sub, |ui| command_items(ui, session, &inner, depth + 1, invoked));
    }
}

/// A menu item for `spec`: its label, shortcut and enabled state. True when clicked.
fn menu_item(ui: &mut egui::Ui, session: &Session, spec: &CommandSpec) -> bool {
    let reason = (spec.enabled)(session).err();
    let mut button = egui::Button::new(spec.label);
    if let Some(on) = checked(session, spec.id) {
        button = egui::Button::new(if on { format!("✓ {}", spec.label) } else { format!("    {}", spec.label) });
    }
    if let Some(sc) = spec.shortcut.and_then(parse_shortcut) {
        button = button.shortcut_text(ui.ctx().format_shortcut(&sc));
    }
    let r = ui.add_enabled(reason.is_none(), button);
    let r = match &reason {
        Some(why) => r.on_disabled_hover_text(why),
        None => r,
    };
    let clicked = r.clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// Parse a shortcut in the registry's notation (`Cmd+Shift+Z`). `Cmd` is egui's COMMAND (⌘ on
/// macOS, Ctrl elsewhere).
pub fn parse_shortcut(s: &str) -> Option<egui::KeyboardShortcut> {
    let mut mods = egui::Modifiers::NONE;
    let mut key = None;
    for part in s.split('+') {
        match part {
            "Cmd" => mods |= egui::Modifiers::COMMAND,
            "Ctrl" => mods |= egui::Modifiers::CTRL,
            "Shift" => mods |= egui::Modifiers::SHIFT,
            "Alt" | "Opt" => mods |= egui::Modifiers::ALT,
            k if key.is_none() => key = Some(egui::Key::from_name(k)?),
            _ => return None,
        }
    }
    Some(egui::KeyboardShortcut::new(mods, key?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_parse() {
        let sc = parse_shortcut("Cmd+Shift+Z").unwrap();
        assert_eq!(sc.logical_key, egui::Key::Z);
        assert!(sc.modifiers.command && sc.modifiers.shift);
        assert!(parse_shortcut("Cmd+N").is_some());
        for bad in ["", "Cmd+", "Cmd+Nope", "Z+Y", "Hyper+Z"] {
            assert!(parse_shortcut(bad).is_none(), "{bad}");
        }
        // every shortcut the registry names is one the UI can bind
        for spec in command_specs() {
            if let Some(s) = spec.shortcut {
                assert!(parse_shortcut(s).is_some(), "{}: {s}", spec.id);
            }
        }
    }

    #[test]
    fn title_names_the_workflow() {
        let mut app = EditorApp::new(Session::default());
        assert_eq!(app.window_title(), "Untitled — PlumCraft");
        app.document = "Demo".into();
        assert_eq!(app.window_title(), "Demo — PlumCraft");
    }

    #[test]
    fn the_banner_says_what_is_wrong_and_how_to_fix_it() {
        let mut app = EditorApp::new(Session::default());
        assert_eq!(app.banner_text(), None);
        app.engine = EngineState::EngineDown {
            reason: "exited 1".into(),
            queues: None,
            stderr_tail: Some("Traceback".into()),
            hint: "cargo build -p plumcraft-text".into(),
        };
        let text = app.banner_text().unwrap();
        assert!(text.contains("exited 1") && text.contains("cargo build -p plumcraft-text") && text.contains("Traceback"), "{text}");
    }
}
