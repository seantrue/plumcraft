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
pub mod app_hooks;
pub use plumcraft_editor_core::appearance;
pub mod array_view;
pub mod asks_view;
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
pub mod form_view;
pub mod frame_view;
pub mod graph_params_view;
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
pub mod plain;
pub mod playback;
pub mod popouts;
pub use plumcraft_editor_core::prefs;
pub mod probe;
pub mod recipe_view;
pub mod rename_view;
pub use plumcraft_editor_core::run_anim;
pub mod run_view;
pub mod settings_view;
pub mod slice_view;
pub mod status_cell;
pub mod status_msgs;
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

use plumcraft_proto::catalogue::{PathHint, SaveFormat, SchemaReply};
use plumcraft_proto::event::Event;
use plumcraft_proto::run::SaveReply;
use plumcraft_proto::session::EngineState;
use plumcraft_session::run::Outgoing;
use plumcraft_session::viewer_cmds::Tools;
use plumcraft_session::{CommandSpec, Session, command_specs};
use serde_json::{Value, json};

use crate::canvas::{Action, Canvas, RunView};
use crate::run_view::Thumbs;
use crate::theme::Tokens;
use crate::watched::Watched;

/// The Watched panel's display: the application's value for the watched node
/// ([`app_hooks`]), else the run's.
fn panel_display<'a>(
    app: &'a Option<plumcraft_proto::display::Display>,
    run: &'a plumcraft_session::RunState,
) -> Option<&'a plumcraft_proto::display::Display> {
    app.as_ref().or_else(|| run.panel().and_then(|(_, d)| d))
}

/// The window title's app name.
pub const APP_TITLE: &str = plumcraft_editor_core::control::APP_TITLE;

/// Top-level menus, in order. Commands name theirs in `CommandSpec::menu`; menus the registry
/// adds later appear after these.
const MENUS: &[&str] = &["File", "Edit", "View", "Node", "Run"];

/// The workflow file extension.
pub const WORKFLOW_EXT: &str = "json";

/// Native dialogs, replaceable in tests: how this shell answers the core's file asks
/// ([`asks_view`]). Each call blocks until it is answered, as rfd's dialogs do; a shell whose
/// pickers answer later answers the asks later instead (`docs/shell-interface.md` section 11).
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
    /// A theme file to import (Settings › Appearance › Import…).
    fn open_theme(&mut self) -> Option<PathBuf> {
        None
    }
    /// Where to export a theme file, offered `name` (`paper.plumtheme.json`).
    fn save_theme(&mut self, _name: &str) -> Option<PathBuf> {
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

    fn open_theme(&mut self) -> Option<PathBuf> {
        self.dialog().set_title("Import Theme").add_filter("PlumCraft theme", &["json"]).pick_file()
    }

    fn save_theme(&mut self, name: &str) -> Option<PathBuf> {
        self.dialog().set_title("Export Theme").add_filter("PlumCraft theme", &["json"]).set_can_create_directories(true).set_file_name(name).save_file()
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

pub use plumcraft_editor_core::asks::{AskId, By};
/// The prompts, their answers and what Discard goes on to do are the core's
/// ([`plumcraft_editor_core::invoke`]): the window draws the open one as a modal
/// ([`EditorApp::answer_prompt`]).
pub use plumcraft_editor_core::invoke::{AfterDiscard, Prompt, PromptAnswer};

use plumcraft_editor_core::control::short_path;

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
    /// The theme in use, with the user's appearance settings over it.
    pub tokens: Tokens,
    /// The tokens changed since they were last installed on the context ([`Tokens::apply`]).
    pub(crate) restyle: bool,
    /// The Settings window's own state (what isn't a setting).
    pub(crate) settings_ui: settings_view::SettingsUi,
    /// The last command error, shown in the status bar.
    pub last_error: Option<String>,
    /// A refused wire's way through (`wire.convert`'s params and the path's label), offered
    /// beside the error (docs/research/20-convert.md).
    pub convert_offer: Option<(Value, String)>,
    /// The engine connection, when one was started (the core's link: [`link::Link`] is a
    /// wrapper that starts one with an egui context).
    pub link: Option<plumcraft_editor_core::link::Link>,
    /// What the application's hooks show this frame ([`app_hooks`]).
    pub app: app_hooks::AppShown,
    /// The Watched panel's display of the application's value for the watched node, which
    /// takes the place of the run's ([`app_hooks::AppShown::display`]).
    app_display: Option<plumcraft_proto::display::Display>,
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
    /// Every value shown, read and decoded off this thread: thumbnails, renditions, the Watched
    /// panel's full value, compare, pop-outs, wire previews ([`plumcraft_media::MediaService`]).
    /// Its bytes are shared by every viewer of them, as `Fulls` shared them.
    pub media_service: plumcraft_media::MediaService,
    /// Keeps the media service reading through the current engine link.
    media_follow: plumcraft_editor_core::values::Follow,
    /// Pop-out windows ([`popouts`]).
    pub popouts: Vec<popouts::PopoutWindow>,
    /// The Parameters window's doc lines being edited, by name ([`graph_params_view`]).
    param_docs: std::collections::HashMap<String, String>,
    /// Whether one of the editor's windows (the main one or a pop-out) had focus last frame:
    /// pop-outs float above the editor only then (B19).
    popouts_float: bool,
    /// The values on wires ([`wire_view`]).
    pub wires: wire_view::WireView,
    /// Thumbnail textures.
    pub thumbs: Thumbs,
    pub dialogs: Box<dyn FileDialogs>,
    /// What is kept between runs ([`prefs`]). The application gives it a file.
    pub prefs: prefs::Store,
    /// The window title's name for a graph with no file (the demo, a new graph).
    pub document: String,
    /// The question the window is asking, when it is: the open `prompt` ask's
    /// ([`asks_view`]), drawn as a modal.
    pub prompt: Option<Prompt>,
    /// What the core is asking: file dialogs, prompts, forms, the clipboard
    /// ([`plumcraft_editor_core::asks`]).
    pub asks: plumcraft_editor_core::asks::Asks<plumcraft_editor_core::invoke::Cont>,
    /// The asks presented and waiting (a paste, a form).
    pub(crate) presented: asks_view::Presented,
    /// Where the paste being answered lands (the pointer over the canvas).
    pub(crate) paste_pos: Option<[f32; 2]>,
    /// The apps the session asked to start, until they become `open.external` asks.
    pub(crate) launches: Option<plumcraft_editor_core::invoke::Launches>,
    /// The run waiting on [`Prompt::Uploads`].
    upload_held: Option<Box<plumcraft_proto::run::RunRequest>>,
    /// Commands a shortcut fired while a text field had focus: they run on the next frame,
    /// after the field, losing focus, has committed what was typed (⌘↩ in a query runs it).
    after_typing: Vec<&'static str>,
    /// The rename window ([`rename_view`]), when open.
    pub rename: Option<rename_view::RenameDialog>,
    /// What Copy or Cut last put on the clipboard.
    pub copied: Option<String>,
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
    /// A command's form, while it is open ([`form_view`]).
    pub command_form: Option<form_view::FormDialog>,
    /// An install running in the background.
    pub installing: Option<engine_view::Installing>,
    /// Every engine call the editor makes, made off the window's thread
    /// ([`plumcraft_editor_core::worker`]): runs, cancels, releases, a switch's connect.
    pub(crate) engine_worker: plumcraft_editor_core::worker::EngineWorker,
    /// An engine switch under way ([`engine_view`]).
    pub(crate) switching: plumcraft_editor_core::switchboard::Switching,
    /// The last command run asked for an engine switch.
    pub(crate) engine_asked: bool,
    /// Drawing a frame: what a command starts on an engine goes to the worker, and the frame
    /// goes on. Outside a frame (tests, scripts, the API) a command waits for it, as before.
    pub(crate) in_frame: bool,
    /// The Attach… window's attach is under way: it closes once connected.
    pub(crate) attaching: bool,
    /// The context a new engine link wakes (set every frame).
    pub(crate) repaint: Option<egui::Context>,
    /// The context of the last control request: what its replies read and repaint.
    pub(crate) control_ctx: Option<egui::Context>,
    /// The window title's app name: [`APP_TITLE`] unless an application names itself.
    pub app_title: &'static str,
    /// The latest run, replayed for the canvas's run colours ([`run_anim`]).
    pub run_anim: run_anim::RunAnim,
}

impl EditorApp {
    pub fn new(mut session: Session) -> Self {
        // the session's app starts are asked for, and this window starts them (asks_view)
        let launches = plumcraft_editor_core::invoke::route_launches(&mut session);
        let mut canvas = Canvas::default();
        if !session.graph().nodes.is_empty() {
            canvas.request_fit();
        }
        EditorApp {
            params: params_view::ParamsPanel::default(),
            value_editors: value_editors::ValueEditors::default(),
            app_title: APP_TITLE,
            run_anim: run_anim::RunAnim::default(),
            session,
            canvas,
            tokens: theme::DEFAULT,
            restyle: false,
            settings_ui: settings_view::SettingsUi::default(),
            last_error: None,
            convert_offer: None,
            link: None,
            app: app_hooks::AppShown::default(),
            app_display: None,
            coordinator: None,
            engine: EngineState::Off,
            watched: Watched::default(),
            audio: audio::AudioView::default(),
            panel_pane: viewers::Pane::default(),
            compare: compare_view::CompareView::default(),
            media_service: plumcraft_media::MediaService::new(media_config()),
            media_follow: plumcraft_editor_core::values::Follow::default(),
            popouts: Vec::new(),
            param_docs: std::collections::HashMap::new(),
            popouts_float: false,
            wires: wire_view::WireView::default(),
            media: media::MediaViews::default(),
            thumbs: Thumbs::default(),
            dialogs: Box::new(NativeDialogs::default()),
            prefs: prefs::Store::default(),
            document: "Untitled".into(),
            prompt: None,
            asks: Default::default(),
            presented: Default::default(),
            paste_pos: None,
            launches,
            upload_held: None,
            after_typing: Vec::new(),
            rename: None,
            copied: None,
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
            command_form: None,
            installing: None,
            engine_worker: Default::default(),
            switching: Default::default(),
            engine_asked: false,
            in_frame: false,
            attaching: false,
            repaint: None,
            control_ctx: None,
        }
    }

    /// Attach an engine link: its catalogue (`schema`) becomes the session's, and its events
    /// are drained every frame. A catalogue that can't be read is the status bar's error.
    pub fn with_link(mut self, link: link::Link) -> Self {
        let link = link.into_core();
        match link.conn.call("schema", json!({})).map(serde_json::from_value::<SchemaReply>) {
            Ok(Ok(catalog)) => self.set_catalog(catalog),
            Ok(Err(e)) => self.last_error = Some(format!("the engine's catalogue: {e}")),
            Err(e) => self.last_error = Some(format!("schema: {}", e.message)),
        }
        self.engine = link.state();
        self.sync_engine();
        self.media_follow.follow(&self.media_service, Some(&link.conn));
        self.session.docs.resolver = Some(plumcraft_editor_core::values::docs_resolver(&self.media_service));
        self.link = Some(link);
        self.share_connection();
        self
    }

    /// Hand the link's connection to the session, for the application's commands
    /// ([`plumcraft_session::Session::connection`]); none without a link. Called wherever the link
    /// is set or dropped, and every frame.
    pub(crate) fn share_connection(&mut self) {
        plumcraft_editor_core::switchboard::share_connection(&mut self.session, &self.link);
    }

    /// What the session knows of the engine's state.
    pub(crate) fn sync_engine(&mut self) {
        plumcraft_editor_core::switchboard::sync_state(&mut self.session, &self.engine);
    }

    /// Replace the catalogue (the graph is kept; the canvas rebuilds).
    pub fn set_catalog(&mut self, catalog: SchemaReply) {
        let first = plumcraft_editor_core::switchboard::set_catalog(&mut self.session, catalog);
        self.catalog_changed(first);
    }

    /// The canvas after a new catalogue: rebuilt, and fitted when it is the first.
    pub(crate) fn catalog_changed(&mut self, first: bool) {
        self.canvas.invalidate();
        if first && !self.session.graph().nodes.is_empty() {
            // The nodes only get their real sizes now.
            self.canvas.request_fit();
        }
    }

    /// Switch themes (the user's appearance settings go over it), and keep the choice.
    pub fn set_theme(&mut self, ctx: &egui::Context, tokens: &Tokens) {
        self.prefs.set_theme(tokens.name);
        self.retheme();
        self.restyle_now(ctx);
    }

    /// The tokens again from the theme and the appearance settings; installed on the next frame.
    pub(crate) fn retheme(&mut self) {
        let semibold = self.tokens.semibold;
        self.tokens = Tokens::from(self.prefs.prefs().appearance.resolve(self.tokens.name));
        self.tokens.semibold = semibold;
        self.restyle = true;
    }

    /// Install the tokens, the text sizes and the zoom on the context, and redraw what caches
    /// colours.
    pub(crate) fn restyle_now(&mut self, ctx: &egui::Context) {
        self.restyle = false;
        self.tokens.apply(ctx);
        self.canvas.wire_default = self.tokens.wire;
        ctx.set_zoom_factor(self.prefs.prefs().appearance.zoom.unwrap_or(1.0));
        self.canvas.invalidate();
        self.audio.recolor(ctx, &self.tokens);
    }

    /// Keep preferences in `store`, and take the theme and the look it names. An application
    /// calls this with its own ([`prefs::Store::for_app`]).
    pub fn use_prefs(&mut self, store: prefs::Store, ctx: &egui::Context) {
        self.prefs = store;
        // The user's port labels live beside the preferences (B18): `labels.toml`, which
        // `labels.reload` reads again after an edit. The application's own table stays.
        let labels = self.prefs.path().map(|p| p.with_file_name("labels.toml"));
        let app = self.session.labels.app.clone();
        self.session.use_labels(app, labels);
        let name = self.prefs.prefs().theme.as_deref().and_then(theme::by_name).map_or(theme::DEFAULT.name, |t| t.name);
        let semibold = self.tokens.semibold;
        self.tokens = Tokens::from(self.prefs.prefs().appearance.resolve(name));
        self.tokens.semibold = semibold;
        self.restyle_now(ctx);
        // after the first graph opened: turning it on starts what that graph needs
        if self.prefs.prefs().auto_engines == Some(true) {
            self.run("engine.auto", &json!({"on": true}));
        }
        self.session.views.run_colors_off = self.prefs.prefs().run_colors == Some(false);
        self.session.views.anim_ms = self.prefs.prefs().anim_ms.filter(|ms| plumcraft_session::view_cmds::is_anim_ms(u64::from(*ms)));
    }

    /// Keep Pick Engines Automatically in the preferences, however it was set (the menu, the
    /// control channel). Unset stays unset until it is turned on.
    fn save_auto_engines(&mut self) {
        let on = self.session.engines.auto;
        if self.prefs.prefs().auto_engines.unwrap_or(false) != on {
            self.prefs.set_auto_engines(on);
        }
    }

    /// Keep the run colours and the animation speed in the preferences, however they were set.
    fn save_run_anim(&mut self) {
        let (on, ms) = (!self.session.views.run_colors_off, self.session.views.anim_ms);
        if self.prefs.prefs().run_colors.unwrap_or(true) != on || self.prefs.prefs().anim_ms != ms {
            self.prefs.set_run_anim(on, ms);
        }
    }

    /// Bring the run animation up to date with the session's run, every frame: a new run is
    /// shown as it happens, and View › Animate Last Run (`view.animateRun`) replays the last one
    /// slowly. While either shows or fades the canvas is redrawn.
    pub fn pump_run_anim(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if std::mem::take(&mut self.session.views.animate_asked) && !self.session.run.running {
            let run = &self.session.run;
            let plan = run_anim_plan(&self.session, &run.needed);
            let slow = std::time::Duration::from_millis(u64::from(self.session.views.anim_ms()));
            self.run_anim.replay(run.generation, &run.needed, plan, slow, &run.log, now);
        }
        let run = &self.session.run;
        if run.generation != self.run_anim.generation() && !run.needed.is_empty() {
            let plan = run_anim_plan(&self.session, &run.needed);
            self.run_anim.begin(run.generation, &run.needed, plan, std::time::Duration::ZERO);
        }
        if run.generation == self.run_anim.generation() {
            self.run_anim.feed(&run.log);
            if !run.running {
                self.run_anim.end(now);
            }
        }
        if (!self.session.views.run_colors_off || self.run_anim.is_replay()) && self.run_anim.is_active(now) {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }

    /// Apply a theme asked for by `view.theme`.
    fn pump_theme(&mut self, ctx: &egui::Context) {
        let Some(name) = self.session.views.theme_asked.take() else { return };
        match theme::by_name(&name) {
            Some(t) => {
                self.tokens.name = t.name;
                self.set_theme(ctx, &t);
            }
            None => self.last_error = Some(format!("no theme called {name:?}")),
        }
    }

    /// Install the theme, fonts and icon loaders on a context (call once at startup).
    pub fn setup_context(&self, ctx: &egui::Context) {
        self.tokens.apply(ctx);
        fonts::install(ctx);
        icons::ensure_loaders(ctx);
        no_window_tabbing();
    }

    /// Run a command, keeping its error for the status bar. A watch that succeeds runs the
    /// graph (when it can), so the Watched panel fills.
    pub fn run(&mut self, id: &str, params: &Value) -> Option<Value> {
        self.try_run(id, params).ok()
    }

    /// [`EditorApp::run`], returning the error too (the control channel's `command.run`).
    pub fn try_run(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let result = self.session.execute(id, params);
        // reading the messages in full leaves them (and the offer) as they are
        let keeps = id == "view.messages";
        // an offer lasts until the next command
        if !keeps {
            self.convert_offer = None;
        }
        match result {
            Ok(v) => {
                if !keeps {
                    self.last_error = None;
                }
                // the window's own: the look (crate::settings_view)
                let v = match self.window_command(id, v) {
                    Ok(v) => v,
                    Err(e) => {
                        self.last_error = Some(e.clone());
                        return Err(e);
                    }
                };
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
                if id == "viewer.recipe"
                    && params.get("copy").and_then(Value::as_bool) == Some(true)
                    && let Some(text) = v.get("recipe").filter(|r| !r.is_null()).and_then(|r| serde_json::to_string_pretty(r).ok())
                {
                    // on the clipboard next frame (the core asks; the window answers)
                    plumcraft_editor_core::invoke::copy(self, &text, "text", By::Person);
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
                // the new way); the caller gets the last, and how the pick was taken. Inside a
                // frame, what waits for an engine goes to the worker, and the outcome comes later
                // (a control request waits for it); outside one, the caller waits.
                self.engine_asked = self.session.engines.is_pending();
                let done = if !self.engine_asked {
                    None
                } else if self.in_frame {
                    let mut done = None;
                    let mut out = Vec::new();
                    self.step_engines(&mut out);
                    for r in out {
                        done = Some(r?);
                    }
                    done
                } else {
                    self.engines_now().transpose()?
                };
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
        for (id, params) in actions {
            // Open with… "Choose an App…": the core asks for the app, then runs value.open
            if id == "value.open" && params.get("choose").and_then(Value::as_bool) == Some(true) {
                plumcraft_editor_core::invoke::choose_app(self, params, By::Person);
                self.present_asks();
                continue;
            }
            // Save Result…: the core asks where, then runs run.save
            if id == "run.save" && params.get("ask").and_then(Value::as_bool) == Some(true) {
                plumcraft_editor_core::invoke::save_result(self, params, By::Person);
                self.present_asks();
                continue;
            }
            self.run(id, &params);
        }
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
                plumcraft_editor_core::invoke::raise_prompt(self, Prompt::Uploads(pending), By::Person);
                self.sync_asks();
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

    /// Run a command from a menu or shortcut, filling in what the UI knows: a command's form
    /// ([`form_view`]), file dialogs for open and save, a prompt before unsaved changes are
    /// dropped, the canvas selection for node commands. The core's invoke machine does it
    /// ([`plumcraft_editor_core::invoke`]), asking; this window answers ([`asks_view`]).
    pub fn invoke(&mut self, id: &str) {
        self.invoke_by(id, By::Person);
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
        plumcraft_editor_core::invoke::quit(self, By::Person);
        self.present_asks();
    }

    /// Answer the open prompt: Discard goes on (dropping the unsaved changes); Save saves (the
    /// dialog first for a graph with no file) and goes on once nothing is left unsaved; Cancel
    /// does nothing. False when no prompt was open, or it doesn't take that answer.
    pub fn answer_prompt(&mut self, answer: PromptAnswer) -> bool {
        let answered = plumcraft_editor_core::invoke::answer_prompt(self, answer);
        self.present_asks();
        answered
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
                plumcraft_editor_core::invoke::raise_prompt(self, Prompt::Discard { then: AfterDiscard::Quit }, By::Person);
                self.sync_asks();
            }
        } else {
            self.closing = true;
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

    /// The media service, as what the viewers read from (a handle: cheap to clone, so a frame
    /// can hold it beside `&mut self`).
    pub(crate) fn source(&self) -> plumcraft_media::MediaService {
        self.media_service.clone()
    }

    /// Whether the media service is reading or decoding a value a frame asked for (a thumbnail,
    /// the Watched panel's value, a picture): it arrives with a repaint.
    pub fn media_busy(&self) -> bool {
        self.media_service.busy()
    }

    /// Wait (at most `timeout`) until the media service has made everything asked of it: whether
    /// it came to that. For tests and scripts, never a frame.
    pub fn wait_media(&self, timeout: std::time::Duration) -> bool {
        self.media_service.wait_idle(timeout)
    }

    /// Use these limits for the media service (its cache's budget: a small machine wants less).
    /// What it holds is dropped.
    pub fn with_media(mut self, config: plumcraft_media::Config) -> Self {
        self.media_service = plumcraft_media::MediaService::new(config);
        self.media_follow = plumcraft_editor_core::values::Follow::default();
        if let Some(link) = &self.link {
            self.media_follow.follow(&self.media_service, Some(&link.conn));
            self.session.docs.resolver = Some(plumcraft_editor_core::values::docs_resolver(&self.media_service));
        }
        self
    }

    /// Give the compare view A and B while it shows; it lets go of them when it doesn't.
    fn sync_compare(&mut self, ctx: &egui::Context) {
        use plumcraft_session::viewer_cmds::comparing;
        if !comparing(&self.session) {
            self.compare.clear();
            return;
        }
        let source = self.source();
        let nan = self.tokens.error;
        let sides = compare_view::sides(&self.session);
        let state = self.session.views.compare.clone();
        if self.compare.sync(ctx, &state, sides, &mut self.thumbs, &source, nan) {
            self.rerun_pending = true;
        }
    }

    /// Engine housekeeping, every frame: the engine's state, its events into the session.
    pub fn drain_engine(&mut self) {
        self.share_connection();
        let Some(link) = &self.link else {
            self.session.connected = false;
            self.session.docs.resolver = None;
            self.media_follow.follow(&self.media_service, None);
            return;
        };
        self.engine = link.state();
        self.sync_engine();
        let Some(link) = &self.link else { return };
        // values are read through the current link, on the media service's workers; the
        // session's own reads (doc.open, value.open) share what it has read
        self.media_follow.follow(&self.media_service, Some(&link.conn));
        self.session.docs.resolver = Some(plumcraft_editor_core::values::docs_resolver(&self.media_service));
        let events = link.drain();
        let now = Instant::now();
        for e in &events {
            if self.session.run.apply_event(e, now) && matches!(e.event, Event::RunReply(_)) {
                // Reaped thumbnails were published again by this run: try them again.
                self.thumbs.forget_misses();
                self.media_service.forget_misses();
            }
        }
        if !events.is_empty() {
            // what an application's value editor will read, asked for now, off this thread
            plumcraft_editor_core::values::read_ahead(&self.media_service, &self.session.run);
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
            let review = Box::new(self.session.review());
            plumcraft_editor_core::invoke::raise_prompt(self, Prompt::Review(review), By::Person);
            self.sync_asks();
            ctx.request_repaint();
        }
        // Sent by the engine worker, in order (a cancel after the run it cancels), never here:
        // the window doesn't wait for an engine to take a run (plumcraft_editor_core::worker).
        // A refusal comes back to engine_view's pump, which fails the run if it is still the
        // latest.
        if !self.engine_worker.has_waker() && !self.offscreen {
            self.engine_worker.set_waker(Some(link::repaint_waker(ctx.clone())));
        }
        for out in self.session.run.take_outbox() {
            match out {
                Outgoing::Run(req) => {
                    let body = serde_json::to_value(&*req);
                    // files for an engine nobody here started: ask first, and send this run after
                    // (the host's own list: no call to the engine)
                    let pending = match (&self.link, &body) {
                        (Some(link), Ok(body)) => link.conn.uploads_pending(body),
                        _ => Vec::new(),
                    };
                    if !pending.is_empty() {
                        self.upload_held = Some(req);
                        plumcraft_editor_core::invoke::raise_prompt(self, Prompt::Uploads(pending), By::Person);
                        self.sync_asks();
                        ctx.request_repaint();
                        continue;
                    }
                    match (&self.link, body) {
                        (Some(link), Ok(body)) => self.engine_worker.run(std::sync::Arc::clone(&link.conn), body, req.run_id.clone(), req.generation),
                        (None, _) => self.session.run.fail("run: the engine is not connected"),
                        (_, Err(e)) => self.session.run.fail(&format!("run: {e}")),
                    }
                }
                Outgoing::Release(scope) => {
                    if let Some(link) = &self.link {
                        // an engine without the capability has nothing to release
                        self.engine_worker.call(std::sync::Arc::clone(&link.conn), "graph.release", json!({"scope": scope}));
                    }
                }
                Outgoing::Cancel(run_id) => {
                    if let Some(link) = &self.link {
                        // the run's late records are ignored by generation whatever the reply
                        self.engine_worker.cancel(std::sync::Arc::clone(&link.conn), run_id);
                    }
                }
            }
        }
        if !self.session.run.outbox_is_empty() {
            ctx.request_repaint_after(HELD_OUTBOX);
        }
        self.pump_saves(ctx);
        // a value arriving from the media service's workers repaints, and what no frame asks for
        // any more is dropped before it is read
        if !self.media_service.has_wake() && !self.offscreen {
            let repaint = ctx.clone();
            self.media_service.set_wake(Some(std::sync::Arc::new(move || repaint.request_repaint())));
        }
        self.media_service.frame();
        let source = self.source();
        let display = panel_display(&self.app_display, &self.session.run);
        if self.watched.update_from(display, &source) {
            self.rerun_pending = true;
        }
        self.audio.sync(display, self.watched.full.as_deref());
        self.sync_media(ctx);
        let offered = plumcraft_session::run_cmds::offered_views(&self.session);
        let panel_display = panel_display(&self.app_display, &self.session.run).cloned();
        self.panel_pane.chosen.clone_from(&self.session.views.panel_viewer);
        self.panel_pane.sync(ctx, panel_display.as_ref(), offered, &mut self.thumbs, &source, Default::default());
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
        self.pump_run_anim(ctx);
    }

    /// Give the watched display to the picture viewers, with its rendition's texture, and set
    /// the document view's "Open in …" (enabled, or disabled with why).
    fn sync_media(&mut self, ctx: &egui::Context) {
        let source = self.source();
        let display = panel_display(&self.app_display, &self.session.run);
        // (an array's pictures are the viewer's, below: a second texture of one would be 48 MB)
        let rendition = display.filter(|d| d.media != plumcraft_proto::catalogue::Media::Ndarray).and_then(|d| {
            d.renditions.iter().find_map(|r| match r {
                plumcraft_proto::display::Rendition::Rgba8 { data, .. } => self.thumbs.get_from(ctx, data, &source).ok(),
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
        let display = panel_display(&self.app_display, &self.session.run);
        self.media.sync(ctx, display, self.watched.full.as_ref(), rendition.as_ref());
        // An array: every picture of its view, captioned, and the views its wire type offers.
        let pictures: Vec<(String, egui::TextureHandle)> = display
            .filter(|d| d.media == plumcraft_proto::catalogue::Media::Ndarray)
            .map(|d| {
                d.renditions
                    .iter()
                    .filter_map(|r| match r {
                        plumcraft_proto::display::Rendition::Rgba8 { data, role, caption, .. } if role == "view" => {
                            self.thumbs.picture_from(ctx, data, &source).ok().map(|t| (caption.clone(), t))
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
        plumcraft_editor_core::control::document_name(&self.session, &self.document)
    }

    /// The window title: the workflow's name and a dot while it has unsaved changes.
    pub fn window_title(&self) -> String {
        plumcraft_editor_core::control::window_title(&self.session, &plumcraft_editor_core::control::Shell::view(self))
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
                        if let Some(action) = self.engine_choices_menu(ui) {
                            actions.push(action);
                        }
                    }
                    match menu {
                        "Edit" => edit_op = edit_view::edit_menu(ui, selected),
                        "View" => {
                            ui.menu_button("Animation Speed", |ui| {
                                for &ms in run_anim::SPEEDS {
                                    let mark = if self.session.views.anim_ms() == ms { "✓ " } else { "    " };
                                    let (label, hover) = if ms == 0 {
                                        ("Real Time".to_owned(), "Replay every node at its recorded timing")
                                    } else {
                                        (format!("{ms} ms per node"), "Show each node running at least this long")
                                    };
                                    if ui.button(format!("{mark}{label}")).on_hover_text(hover).clicked() {
                                        actions.push(("view.animationSpeed", json!({"ms": ms})));
                                        ui.close();
                                    }
                                }
                            });
                            let fit_button = egui::Button::new("Fit").shortcut_text("F");
                            if ui.add(fit_button).on_hover_text("Fit the view to all nodes").clicked() {
                                fit = true;
                                ui.close();
                            }
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
        plumcraft_editor_core::control::banner_text(&self.session, &self.engine)
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
        plumcraft_editor_core::switchboard::engine_label_connecting(&self.session, self.link.as_ref(), &self.engine, self.switching.busy())
    }

    /// The engine cell's main line (layout B, plumcraft #86): the engine's name and state. The
    /// link and the engine's node count are in its hover, which is the whole [`Self::engine_label`].
    pub fn engine_short_label(&self) -> String {
        plumcraft_editor_core::switchboard::engine_short_label_connecting(&self.session, self.link.as_ref(), &self.engine, self.switching.busy())
    }

    /// The engine's status line (`texts::ENGINE_STATUS`, plumcraft #86) and its tone, while the
    /// engine is connected and the text isn't empty.
    pub fn engine_status(&self) -> Option<(String, plumcraft_session::commands::Tone)> {
        plumcraft_editor_core::control::engine_status(&self.session, &self.engine)
    }

    /// The status bar, in two-line cells (layout B; the maintainer: "D17 B"): the engine over
    /// its status, the document over the graph's size, the run over its times and the caches;
    /// then Auto-run, a save in progress, a Convert offer and the messages in what is left.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        use status_cell::{engine_status_line, status_cell};
        let mut commands: Vec<(&'static str, Value)> = Vec::new();
        let msgs = self.status_messages();
        let now = Instant::now();
        let status = self.engine_status();
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
            let tokens = &self.tokens;
            let sub = status.as_ref().map(|(text, tone)| move |ui: &mut egui::Ui| engine_status_line(ui, text, *tone, tokens));
            status_cell(
                ui,
                |ui| {
                    ui.label(self.engine_short_label()).on_hover_text(self.engine_label());
                },
                sub,
            );
            ui.separator();
            if let Some(i) = &self.installing {
                ui.spinner();
                ui.label(format!("installing {}…", i.url.trim_start_matches("https://github.com/")));
                ui.separator();
            }
            // the workflow, as the window title names it (the title isn't in screenshots)
            let dot = if self.session.is_dirty() { " •" } else { "" };
            let g = self.session.graph();
            let size = format!("{} nodes, {} wires", g.nodes.len(), g.wires.len());
            status_cell(
                ui,
                |ui| {
                    let doc = ui.colored_label(self.tokens.text, format!("{}{dot}", self.document_name()));
                    match self.session.path() {
                        Some(p) => doc.on_hover_text(format!("{}{}", p.display(), if dot.is_empty() { "" } else { " (unsaved changes)" })),
                        None => doc.on_hover_text("not saved yet"),
                    };
                },
                Some(|ui: &mut egui::Ui| {
                    ui.colored_label(self.tokens.text_dim, size);
                }),
            );
            ui.separator();
            // The one hash algorithm of this process (commonkit's switch) and where it came from.
            let source = plumcraft_proto::handle::Algo::source();
            ui.colored_label(self.tokens.text_dim, status_cell::hash_label(plumcraft_proto::handle::Algo::current().as_str(), &source)).on_hover_text(format!(
                "Every handle this editor makes and reads is {} ({source}). Engines must use the same.",
                plumcraft_proto::handle::Algo::current().as_str()
            ));
            ui.separator();
            let run = &self.session.run;
            let tokens = &self.tokens;
            if run.running {
                let (done, total) = run.progress();
                let secs = run.elapsed(now).map_or(0.0, |d| d.as_secs_f64());
                let cancel = status_cell(
                    ui,
                    |ui| {
                        ui.add(egui::Spinner::new().size(12.0).color(tokens.state_running));
                        ui.label(format!("running: {done} of {total} nodes"));
                        ui.small_button("Cancel")
                            .on_hover_text("Stop this run (Esc): the engine stops between nodes or inside a long one, and the rest of its results are ignored")
                            .clicked()
                    },
                    Some(|ui: &mut egui::Ui| {
                        ui.colored_label(tokens.text_dim, format!("{secs:.1} s"));
                    }),
                );
                if cancel {
                    commands.push(("run.cancel", Value::Null));
                }
            } else if let Some(wall) = run.last_wall {
                let engine = run.last_reply.as_ref().map(|r| r.elapsed_ms).filter(|ms| *ms > 0.0);
                let detail = engine.is_some() || run.cache.last.is_some();
                let sub = detail.then_some(|ui: &mut egui::Ui| {
                    if let Some(ms) = engine {
                        ui.colored_label(tokens.text_dim, format!("engine {}", run_view::format_ms(ms)));
                    }
                    commands.extend(cache_view::status_line(ui, &run.cache, tokens, engine.is_some()));
                });
                status_cell(
                    ui,
                    |ui| {
                        ui.colored_label(tokens.text_dim, format!("last run {}", run_view::format_ms(wall.as_secs_f64() * 1e3)));
                    },
                    sub,
                );
            } else {
                ui.colored_label(self.tokens.text_dim, "not run yet");
            }
            ui.separator();
            let auto = run.auto;
            if ui.selectable_label(auto, "Auto-run").on_hover_text("Run 150 ms after every edit").clicked() {
                commands.push(("run.auto", json!({"on": !auto})));
            }
            if let Some(ask) = &self.session.save.pending {
                ui.separator();
                ui.add(egui::Spinner::new().size(12.0).color(self.tokens.state_running));
                ui.label(format!("saving {}…", short_path(&ask.path))).on_hover_text(&ask.path);
            }
            if let Some((p, label)) = &self.convert_offer {
                let tip = format!("wire.convert: place {label} between them, wired, as one undo step");
                if ui.button(format!("Insert {label}")).on_hover_text(tip).clicked() {
                    commands.push(("wire.convert", p.clone()));
                }
            }
            // the messages last, in what width is left (crate::status_msgs)
            if !msgs.is_empty() {
                ui.separator();
                if status_msgs::line(ui, &msgs, &self.tokens) && !self.session.views.messages {
                    commands.push(("view.messages", json!({"open": true})));
                }
            }
        });
        let was = self.session.views.messages;
        let mut open = was;
        status_msgs::window(ui.ctx(), &mut open, &msgs, &self.tokens);
        if was && !open {
            commands.push(("view.messages", json!({"open": false})));
        }
        for (id, p) in commands {
            self.run(id, &p);
        }
    }

    /// The status bar's messages, most serious first within each kind: the run's errors, the
    /// application's segment, a save's outcome, the last thing that went wrong.
    pub fn status_messages(&self) -> Vec<status_msgs::Msg> {
        let app = self.app.status.as_deref().map(|text| (text, self.app.status_tone));
        plumcraft_editor_core::control::status_messages(&self.session, self.last_error.as_deref(), app)
    }

    fn watched_panel(&mut self, ui: &mut egui::Ui) {
        let frame = egui::Frame::new().fill(self.tokens.panel).inner_margin(egui::Margin::same(8));
        let asks = egui::Panel::right("plumcraft-watched").resizable(true).default_size(340.0).min_size(220.0).frame(frame).show(ui, |ui| {
            let run = &self.session.run;
            let (w, panel_display) = run.panel().map_or((None, None), |(w, d)| (Some(w), self.app_display.as_ref().or(d)));
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

    /// Read what the application's hooks show this frame ([`app_hooks`]), and repaint soon
    /// while they show anything.
    fn read_app_hooks(&mut self, ctx: &egui::Context) {
        self.app = app_hooks::AppShown::read();
        self.app_display = self.session.run.panel().and_then(|(w, _)| self.app.display(w, self.session.graph(), self.session.catalog()));
        if self.app.any {
            ctx.request_repaint_after(app_hooks::REPAINT);
        }
    }

    /// The node the Watched panel shows.
    pub fn watched_node(&self) -> Option<plumcraft_graph::NodeId> {
        self.session.run.panel().map(|(w, _)| plumcraft_graph::NodeId(w.node.clone()))
    }

    /// Draw one frame into `ui` (the whole window).
    pub fn show(&mut self, ui: &mut egui::Ui) {
        // what a command starts on an engine meanwhile goes to the worker (engine_view)
        let outside = !std::mem::replace(&mut self.in_frame, true);
        self.show_frame(ui);
        if outside {
            self.in_frame = false;
        }
    }

    fn show_frame(&mut self, ui: &mut egui::Ui) {
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
        let semibold = fonts::has_semibold(&ctx);
        if semibold != self.tokens.semibold {
            self.tokens.semibold = semibold;
            self.restyle = true;
        }
        self.pump_theme(&ctx);
        if self.restyle {
            self.restyle_now(&ctx);
        }
        self.save_auto_engines();
        self.save_run_anim();
        // what the core asked of the window: the clipboard, dialogs, prompts (crate::asks_view)
        self.pump_asks();
        self.read_app_hooks(&ctx);
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
        let source = self.source();
        let watched = self.watched_node();
        let runs = RunView {
            run: &self.session.run,
            thumbs: &mut self.thumbs,
            source: &source,
            docs: &self.session.docs.apps,
            recent_apps: &self.session.docs.recent_apps,
            app_text: &self.app.texts,
            anim: (!self.session.views.run_colors_off || self.run_anim.is_replay()).then_some(&self.run_anim),
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
        self.form_window(&ctx);
        self.script_asks_window(&ctx);
        self.params_window(&ctx);
        self.value_editor_windows(&ctx);
        if self.session.views.cache_report {
            for (id, p) in cache_view::report(&ctx, &self.session.run, &self.tokens) {
                self.run(id, &p);
            }
        }
        self.prompt_modal(&ctx);
        about_view::show(&ctx, &mut self.session);
        let asks = graph_params_view::show(&ctx, &mut self.session, &mut self.param_docs);
        if let Some((name, pos)) = asks.rename {
            self.canvas.nets.edit = Some(crate::net_view::NameEdit::new(crate::net_view::NameFor::RenamePort(name.clone()), name, pos));
        }
        self.run_all(asks.actions);
        let asks = self.settings_window(&ctx);
        self.run_all(asks);
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
        // an application's editor asks again each frame: it gets the bytes once read, off this thread
        // an application's editor asks again each frame: it gets the bytes once read, off this
        // thread (read ahead as the run's reports arrived: values::read_ahead)
        let resolve = plumcraft_editor_core::values::nonblocking(&self.media_service);
        let (actions, note) = self.value_editors.show(ctx, &self.session, &resolve, &self.tokens);
        self.run_all(actions);
        if note.is_some() {
            self.last_error = note;
        }
    }

    fn browse_for_path(&mut self) {
        let asked = self.canvas.edits.take_browse().or_else(|| self.params.edits.take_browse());
        let Some(((node, port), hint, current)) = asked else { return };
        plumcraft_editor_core::invoke::browse(self, &node.0, &port, &hint, &current, By::Person);
        self.present_asks();
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

/// Turn off macOS's automatic window tabbing for this process. With it on, a window opened
/// while the editor is full screen joins it as a tab, so a pop-out took the whole screen and
/// hid the editor (docs/BUGS.md B1, #22); off, it opens as a window of its own. Off the main
/// thread (a kittest) there is no AppKit to ask, and nothing happens; elsewhere than macOS it's
/// nothing. Safe to call more than once.
pub fn no_window_tabbing() {
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        objc2_app_kit::NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
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
        "view.runColors" => Some(!session.views.run_colors_off),
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

/// What the run animation needs of the graph: each node's upstream nodes, and the engine
/// that computes it (its pin, else the first of the graph's engines that offers it, else the
/// first that does; one lane when no engine is named).
fn run_anim_plan(session: &Session, needed: &std::collections::BTreeSet<String>) -> run_anim::Plan {
    let g = session.graph();
    let catalog = session.catalog();
    let mut plan = run_anim::Plan::default();
    for w in &g.wires {
        if needed.contains(&w.to.node.0) && needed.contains(&w.from.node.0) {
            plan.upstream.entry(w.to.node.0.clone()).or_default().push(w.from.node.0.clone());
        }
    }
    for id in needed {
        let node = g.node(&plumcraft_graph::NodeId(id.clone()));
        let offered = node.and_then(|n| catalog.nodes.iter().find(|d| d.id == n.kind)).map(|d| d.engines.clone()).unwrap_or_default();
        let pin = node.and_then(|n| n.engine.clone()).filter(|p| offered.contains(p));
        let lane = pin.or_else(|| g.engines.iter().find(|e| offered.contains(e)).cloned()).or_else(|| offered.first().cloned()).unwrap_or_default();
        plan.lane.insert(id.clone(), lane);
    }
    plan
}

/// The egui editor's media service: the default's workers and queue, and a 64 MiB cache, since
/// this shell keeps every picture it shows as a texture besides ([`run_view::Thumbs`]).
/// `PLUMCRAFT_MEDIA_CACHE_MB` sets the cache (1 to 65,536).
pub fn media_config() -> plumcraft_media::Config {
    let mb = std::env::var("PLUMCRAFT_MEDIA_CACHE_MB").ok().and_then(|v| v.trim().parse::<u64>().ok()).filter(|n| (1..=65_536).contains(n)).unwrap_or(64);
    plumcraft_media::Config { budget: usize::try_from(mb << 20).unwrap_or(64 << 20), ..plumcraft_media::Config::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turning_off_window_tabbing_off_the_main_thread_does_nothing() {
        // A test thread isn't AppKit's main thread: no marker, no call, no panic. That it works
        // on the main thread is checked in the real editor (claude-debug/popout_fullscreen_probe.py:
        // full screen, then a pop-out opens as a window of its own, not a tab; B1).
        no_window_tabbing();
        no_window_tabbing();
    }

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
