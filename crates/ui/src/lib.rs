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
pub mod menu_view;
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
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use plumcraft_proto::catalogue::{PathHint, SaveFormat, SchemaReply};
use plumcraft_proto::session::EngineState;
use plumcraft_session::Session;
#[cfg(test)]
use plumcraft_session::command_specs;
use plumcraft_session::viewer_cmds::Tools;
use serde_json::{Value, json};

use plumcraft_editor_core::document::{self, Document};

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

/// The application: a session and its views.
pub struct EditorApp {
    /// The open document: its session, engine link and worker, asks, media service, last error
    /// ([`plumcraft_editor_core::document`]). The editor derefs to it, so `app.doc.session`,
    /// `app.doc.link`, `app.doc.last_error` and the rest read as they did when they were the editor's own.
    pub doc: Document,
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
    /// What the application's hooks show this frame ([`app_hooks`]).
    pub app: app_hooks::AppShown,
    /// The Watched panel's display of the application's value for the watched node, which
    /// takes the place of the run's ([`app_hooks::AppShown::display`]).
    app_display: Option<plumcraft_proto::display::Display>,
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
    /// The question the window is asking, when it is: the open `prompt` ask's
    /// ([`asks_view`]), drawn as a modal.
    pub prompt: Option<Prompt>,
    /// The asks presented and waiting (a paste, a form).
    pub(crate) presented: asks_view::Presented,
    /// Where the paste being answered lands (the pointer over the canvas).
    pub(crate) paste_pos: Option<[f32; 2]>,
    /// Commands a shortcut fired while a text field had focus: they run on the next frame,
    /// after the field, losing focus, has committed what was typed (⌘↩ in a query runs it).
    after_typing: Vec<&'static str>,
    /// The rename window ([`rename_view`]), when open.
    pub rename: Option<rename_view::RenameDialog>,
    /// What Copy or Cut last put on the clipboard.
    pub copied: Option<String>,
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
    /// The Attach… window, while it is open.
    pub attach_form: Option<engine_view::AttachForm>,
    /// The Install Engine… window, while it is open.
    pub install_form: Option<engine_view::InstallForm>,
    /// A command's form, while it is open ([`form_view`]).
    pub command_form: Option<form_view::FormDialog>,
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
    /// The context new engine links repaint, for the Engine menu's connector on the worker.
    pub(crate) repaint_cell: std::sync::Arc<std::sync::Mutex<Option<egui::Context>>>,
    /// Run once the first engine has connected (or failed to) and the graph has opened, when
    /// the editor was started with [`EditorApp::start_engines`]: the application starts its
    /// control channel there, so a client sees what it saw when startup waited.
    pub on_ready: Option<OnReady>,
}

/// What runs once the editor's first engine has connected ([`EditorApp::on_ready`]).
pub type OnReady = Box<dyn FnOnce(&mut EditorApp)>;

impl std::ops::Deref for EditorApp {
    type Target = Document;
    fn deref(&self) -> &Document {
        &self.doc
    }
}

impl std::ops::DerefMut for EditorApp {
    fn deref_mut(&mut self) -> &mut Document {
        &mut self.doc
    }
}

impl EditorApp {
    pub fn new(session: Session) -> Self {
        let mut canvas = Canvas::default();
        if !session.graph().nodes.is_empty() {
            canvas.request_fit();
        }
        // the session's app starts are asked for, and this window starts them (asks_view)
        let mut doc = Document::with_media(session, plumcraft_media::MediaService::new(media_config()), true);
        // the canvas draws the document's selection (N2 slice 1)
        canvas.picked = edit_view::Picked::on(doc.selection.clone());
        // an engine compiled in connects in place (slice 3's exception: two kittests pick one
        // from the menu and look three frames later)
        doc.start_in_place = true;
        EditorApp {
            params: params_view::ParamsPanel::default(),
            value_editors: value_editors::ValueEditors::default(),
            app_title: APP_TITLE,
            run_anim: run_anim::RunAnim::default(),
            doc,
            canvas,
            tokens: theme::DEFAULT,
            restyle: false,
            settings_ui: settings_view::SettingsUi::default(),
            app: app_hooks::AppShown::default(),
            app_display: None,
            watched: Watched::default(),
            audio: audio::AudioView::default(),
            panel_pane: viewers::Pane::default(),
            compare: compare_view::CompareView::default(),
            popouts: Vec::new(),
            param_docs: std::collections::HashMap::new(),
            popouts_float: false,
            wires: wire_view::WireView::default(),
            media: media::MediaViews::default(),
            thumbs: Thumbs::default(),
            dialogs: Box::new(NativeDialogs::default()),
            prefs: prefs::Store::default(),
            prompt: None,
            presented: Default::default(),
            paste_pos: None,
            after_typing: Vec::new(),
            rename: None,
            copied: None,
            shot: None,
            title: String::new(),
            control_rx: None,
            control: control::ControlState::default(),
            offscreen: false,
            closing: false,
            close_next: false,
            attach_form: None,
            install_form: None,
            command_form: None,
            engine_asked: false,
            in_frame: false,
            attaching: false,
            repaint: None,
            control_ctx: None,
            repaint_cell: Default::default(),
            on_ready: None,
        }
    }

    /// Attach an engine link: its catalogue (`schema`) becomes the session's, and its events
    /// are drained every frame. A catalogue that can't be read is the status bar's error. It reads
    /// the catalogue here, so it is set when this returns (tests and applications read it at
    /// once); the editor's own startup doesn't wait for it ([`EditorApp::start_engines`]).
    pub fn with_link(mut self, link: link::Link) -> Self {
        if let Some(first) = self.doc.link_now(link.into_core()) {
            self.catalog_changed(first);
        }
        self
    }

    /// Repaint `ctx` when something arrives for the window (the engine's events, the worker, the
    /// media service, a save): set by the first frame, or before it by an application that begins
    /// its engine then.
    pub fn wake_with(&mut self, ctx: &egui::Context) {
        self.repaint = Some(ctx.clone());
        *self.repaint_cell.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(ctx.clone());
        if self.doc.waker().is_none() {
            self.doc.set_waker(Some(link::repaint_waker(ctx.clone())));
        }
    }

    /// Start the first engine without waiting for it (the editor's own startup): `target` is
    /// switched to on the engine worker from the first frame, the window shows it connecting
    /// meanwhile, `extras` are added after it, and `opening` opens once its catalogue is there
    /// ([`plumcraft_editor_core::document::Document::start_engines`]). [`EditorApp::on_ready`]
    /// runs then.
    pub fn start_engines(mut self, target: plumcraft_session::EngineTarget, extras: Vec<plumcraft_session::EngineTarget>, opening: document::Opening) -> Self {
        self.doc.start_engines(target, extras, opening);
        self
    }

    /// Replace the catalogue (the graph is kept; the canvas rebuilds).
    pub fn set_catalog(&mut self, catalog: SchemaReply) {
        let first = plumcraft_editor_core::switchboard::set_catalog(&mut self.doc.session, catalog);
        self.catalog_changed(first);
    }

    /// The canvas after a new catalogue: rebuilt, and fitted when it is the first.
    pub(crate) fn catalog_changed(&mut self, first: bool) {
        self.canvas.invalidate();
        if first && !self.doc.session.graph().nodes.is_empty() {
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
        let app = self.doc.session.labels.app.clone();
        self.doc.session.use_labels(app, labels);
        let name = self.prefs.prefs().theme.as_deref().and_then(theme::by_name).map_or(theme::DEFAULT.name, |t| t.name);
        let semibold = self.tokens.semibold;
        self.tokens = Tokens::from(self.prefs.prefs().appearance.resolve(name));
        self.tokens.semibold = semibold;
        self.restyle_now(ctx);
        // after the first graph opened: turning it on starts what that graph needs
        if self.prefs.prefs().auto_engines == Some(true) {
            self.run("engine.auto", &json!({"on": true}));
        }
        self.doc.session.views.run_colors_off = self.prefs.prefs().run_colors == Some(false);
        self.doc.session.views.anim_ms = self.prefs.prefs().anim_ms.filter(|ms| plumcraft_session::view_cmds::is_anim_ms(u64::from(*ms)));
    }

    /// Keep Pick Engines Automatically in the preferences, however it was set (the menu, the
    /// control channel). Unset stays unset until it is turned on.
    fn save_auto_engines(&mut self) {
        let on = self.doc.session.engines.auto;
        if self.prefs.prefs().auto_engines.unwrap_or(false) != on {
            self.prefs.set_auto_engines(on);
        }
    }

    /// Keep the run colours and the animation speed in the preferences, however they were set.
    fn save_run_anim(&mut self) {
        let (on, ms) = (!self.doc.session.views.run_colors_off, self.doc.session.views.anim_ms);
        if self.prefs.prefs().run_colors.unwrap_or(true) != on || self.prefs.prefs().anim_ms != ms {
            self.prefs.set_run_anim(on, ms);
        }
    }

    /// Bring the run animation up to date with the session's run, every frame: a new run is
    /// shown as it happens, and View › Animate Last Run (`view.animateRun`) replays the last one
    /// slowly. While either shows or fades the canvas is redrawn.
    pub fn pump_run_anim(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if std::mem::take(&mut self.doc.session.views.animate_asked) && !self.doc.session.run.running {
            let run = &self.doc.session.run;
            let plan = run_anim_plan(&self.doc.session, &run.needed);
            let slow = std::time::Duration::from_millis(u64::from(self.doc.session.views.anim_ms()));
            self.run_anim.replay(run.generation, &run.needed, plan, slow, &run.log, now);
        }
        let run = &self.doc.session.run;
        if run.generation != self.run_anim.generation() && !run.needed.is_empty() {
            let plan = run_anim_plan(&self.doc.session, &run.needed);
            self.run_anim.begin(run.generation, &run.needed, plan, std::time::Duration::ZERO);
        }
        if run.generation == self.run_anim.generation() {
            self.run_anim.feed(&run.log);
            if !run.running {
                self.run_anim.end(now);
            }
        }
        if (!self.doc.session.views.run_colors_off || self.run_anim.is_replay()) && self.run_anim.is_active(now) {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }

    /// Apply a theme asked for by `view.theme`.
    fn pump_theme(&mut self, ctx: &egui::Context) {
        let Some(name) = self.doc.session.views.theme_asked.take() else { return };
        match theme::by_name(&name) {
            Some(t) => {
                self.tokens.name = t.name;
                self.set_theme(ctx, &t);
            }
            None => self.doc.last_error = Some(format!("no theme called {name:?}")),
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

    /// [`EditorApp::run`], returning the error too (the control channel's `command.run`). The
    /// core's ([`plumcraft_editor_core::document::execute`]), with this window's look, fit,
    /// selection and Watched panel around it.
    pub fn try_run(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        document::execute(self, id, params)
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

    /// Is command `id` enabled now?
    pub fn enabled(&self, id: &str) -> bool {
        self.doc.enabled(id)
    }

    /// `run.start`, when it is enabled (an engine connected, a graph to run).
    pub fn start_run_if_enabled(&mut self) -> Option<Value> {
        document::start_run_if_enabled(self)
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
        self.doc.group_to_ungroup()
    }

    /// Show what the document holds of the canvas: the group entered (entering or leaving
    /// rebuilds and fits, [`Canvas::enter_group`]) and the net selected. After every command and
    /// at the start of every frame; the selection itself the canvas reads in place.
    pub fn follow_core(&mut self) {
        if self.canvas.focus != self.doc.focus {
            match self.doc.focus.clone() {
                Some(g) => self.canvas.enter_group(g),
                None => self.canvas.leave_group(),
            }
        }
        let net = self.doc.selection.with(|s| s.net.clone());
        if self.canvas.nets.selected != net {
            self.canvas.nets.selected = net;
        }
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
        if self.doc.session.is_dirty() {
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
        self.doc.media_service.clone()
    }

    /// Whether the media service is reading or decoding a value a frame asked for (a thumbnail,
    /// the Watched panel's value, a picture): it arrives with a repaint.
    pub fn media_busy(&self) -> bool {
        self.doc.media_service.busy()
    }

    /// Wait (at most `timeout`) until the media service has made everything asked of it: whether
    /// it came to that. For tests and scripts, never a frame.
    pub fn wait_media(&self, timeout: std::time::Duration) -> bool {
        self.doc.media_service.wait_idle(timeout)
    }

    /// Use these limits for the media service (its cache's budget: a small machine wants less).
    /// What it holds is dropped.
    pub fn with_media(mut self, config: plumcraft_media::Config) -> Self {
        self.doc.set_media(config);
        self
    }

    /// Give the compare view A and B while it shows; it lets go of them when it doesn't.
    fn sync_compare(&mut self, ctx: &egui::Context) {
        use plumcraft_session::viewer_cmds::comparing;
        if !comparing(&self.doc.session) {
            self.compare.clear();
            return;
        }
        let source = self.source();
        let nan = self.tokens.error;
        let sides = compare_view::sides(&self.doc.session);
        let state = self.doc.session.views.compare.clone();
        if self.compare.sync(ctx, &state, sides, &mut self.thumbs, &source, nan) {
            self.doc.rerun_pending = true;
        }
    }

    /// Engine housekeeping, every frame: the engine's state, its events into the session
    /// ([`plumcraft_editor_core::document::Document::drain_engine`]).
    pub fn drain_engine(&mut self) {
        let doc = &mut self.doc;
        let (_, reread) = plumcraft_session::commands::with_doc(doc.id, || doc.drain_engine());
        if reread {
            // Reaped thumbnails were published again by this run: try them again.
            self.thumbs.forget_misses();
        }
    }

    /// Run housekeeping, every frame: start a due auto-run, send what the session queued, and
    /// keep the Watched panel's value read. The core's part before and after this window's
    /// ([`plumcraft_editor_core::document::pump`], [`plumcraft_editor_core::document::after`]).
    pub fn pump_runs(&mut self, ctx: &egui::Context) {
        let t = document::pump(self);
        if t.asked {
            self.sync_asks();
            ctx.request_repaint();
        }
        if let Some(d) = t.again_in {
            ctx.request_repaint_after(d);
        }
        let source = self.source();
        let display = panel_display(&self.app_display, &self.doc.session.run);
        if self.watched.update_from(display, &source) {
            self.doc.rerun_pending = true;
        }
        self.audio.sync(display, self.watched.full.as_deref());
        self.sync_media(ctx);
        let offered = plumcraft_session::run_cmds::offered_views(&self.doc.session);
        let panel_display = panel_display(&self.app_display, &self.doc.session.run).cloned();
        self.panel_pane.chosen.clone_from(&self.doc.session.views.panel_viewer);
        self.panel_pane.sync(ctx, panel_display.as_ref(), offered, &mut self.thumbs, &source, Default::default());
        self.sync_compare(ctx);
        self.sync_popouts(ctx);
        self.cache_displays();
        if let Some(d) = document::after(self) {
            ctx.request_repaint_after(d);
        }
        self.pump_run_anim(ctx);
    }

    /// Give the watched display to the picture viewers, with its rendition's texture, and set
    /// the document view's "Open in …" (enabled, or disabled with why).
    fn sync_media(&mut self, ctx: &egui::Context) {
        let source = self.source();
        let display = panel_display(&self.app_display, &self.doc.session.run);
        // (an array's pictures are the viewer's, below: a second texture of one would be 48 MB)
        let rendition = display.filter(|d| d.media != plumcraft_proto::catalogue::Media::Ndarray).and_then(|d| {
            d.renditions.iter().find_map(|r| match r {
                plumcraft_proto::display::Rendition::Rgba8 { data, .. } => self.thumbs.get_from(ctx, data, &source).ok(),
                _ => None,
            })
        });
        // the probe and any zoom: compare's while it shows, else the panel's own picture viewer's
        let tools = self.doc.session.views.tools;
        self.doc.session.views.tools.zoom = None;
        let comparing = plumcraft_session::viewer_cmds::comparing(&self.doc.session);
        self.compare.tools.take(if comparing { tools } else { Tools { zoom: None, ..tools } });
        let to_media = if comparing { Tools { zoom: None, ..tools } } else { tools };
        self.media.image.tools.take(to_media);
        self.media.array.tools.take(to_media);
        let display = panel_display(&self.app_display, &self.doc.session.run);
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
        self.media.array.offered = plumcraft_session::run_cmds::offered_views(&self.doc.session);
        self.media.doc.open = match (display, self.doc.session.run.panel().map(|(w, _)| w)) {
            (Some(d), Some(w)) if d.media == plumcraft_proto::catalogue::Media::Doc => {
                let label = self.doc.session.docs.for_wire(&d.ty).map(|a| a.label.clone());
                label.map(|label| {
                    let blocked = plumcraft_session::doc_cmds::open_blocker(&self.doc.session, &w.node, Some(&w.port)).err();
                    let note = match &self.doc.session.docs.last {
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
        plumcraft_editor_core::control::document_name(&self.doc.session, &self.doc.document)
    }

    /// The window title: the workflow's name and a dot while it has unsaved changes.
    pub fn window_title(&self) -> String {
        plumcraft_editor_core::control::window_title(&self.doc.session, &plumcraft_editor_core::control::Shell::view(self))
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
        // the core's table, the most specific first: egui matches Shift and Alt as wildcards when
        // a shortcut doesn't name them, so Cmd+Shift+Z is Redo, not Undo; Cmd+Shift+S is Save As
        let typing = ctx.text_edit_focused();
        for b in plumcraft_editor_core::menu::shortcut_table() {
            let Some(sc) = menu_view::to_egui(&b.shortcut) else { continue };
            // a text field keeps its own undo
            if typing && b.text_field_keeps {
                continue;
            }
            // consumed even when disabled, so a disabled Redo's keys don't fall through to Undo
            if ctx.input_mut(|i| i.consume_shortcut(&sc)) && plumcraft_session::find(b.command).is_some_and(|spec| (spec.enabled)(&self.doc.session).is_ok()) {
                fired.push(b.command);
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

    /// What the core's menu model needs from this shell: the selection, the canvas's centre, the
    /// engine link, the platform, the application's value editors.
    pub fn menu_facts(&self) -> menu_view::Facts {
        let center = self.canvas.to_global.inverse() * self.canvas.rect.center();
        let sel = self.doc.selection.get();
        menu_view::Facts {
            selected: sel.nodes,
            selected_groups: sel.groups,
            center: Some([center.x, center.y]),
            live: self.doc.link.is_some(),
            caps: self.doc.caps,
            editors: value_editors::facts(),
        }
    }

    /// The menu bar, drawn from the core's menu model ([`plumcraft_editor_core::menu`]); a menu's
    /// items are built when it is open.
    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let mut clicked: Option<plumcraft_editor_core::menu::Item> = None;
        let facts = self.menu_facts();
        let style = menu_view::Style { tokens: &self.tokens, bar: true, close: true };
        let session = &self.doc.session;
        egui::MenuBar::new().ui(ui, |ui| {
            for title in plumcraft_editor_core::menu::bar_titles() {
                ui.menu_button(title, |ui| {
                    let menu = plumcraft_editor_core::menu::bar_menu(session, &facts, title);
                    if let Some(item) = menu_view::draw(ui, &menu.items, style) {
                        clicked = Some(item);
                    }
                });
            }
        });
        let Some(item) = clicked else { return };
        match (&item.command, &item.params) {
            // a registry command as its menu item: asks first what it needs
            (Some(id), None) => self.invoke(id),
            (Some(_), Some(_)) => self.run_all(menu_view::action(&item).into_iter().collect()),
            // Align ›, Distribute ›: the shell's, with the sizes it measured
            (None, _) => {
                if let Some(op) = edit_view::EditOp::from_item(&item) {
                    let ctx = ui.ctx().clone();
                    self.edit_op(&ctx, op);
                }
            }
        }
    }

    /// What's wrong with the engine and how to fix it, when something is.
    pub fn banner_text(&self) -> Option<String> {
        plumcraft_editor_core::control::banner_text(&self.doc.session, &self.doc.engine)
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
        plumcraft_editor_core::switchboard::engine_label_connecting(&self.doc.session, self.doc.link.as_ref(), &self.doc.engine, self.doc.switching.busy())
    }

    /// The engine cell's main line (layout B, plumcraft #86): the engine's name and state. The
    /// link and the engine's node count are in its hover, which is the whole [`Self::engine_label`].
    pub fn engine_short_label(&self) -> String {
        plumcraft_editor_core::switchboard::engine_short_label_connecting(
            &self.doc.session,
            self.doc.link.as_ref(),
            &self.doc.engine,
            self.doc.switching.busy(),
        )
    }

    /// The engine's status line (`texts::ENGINE_STATUS`, plumcraft #86) and its tone, while the
    /// engine is connected and the text isn't empty.
    pub fn engine_status(&self) -> Option<(String, plumcraft_session::commands::Tone)> {
        plumcraft_editor_core::control::engine_status(&self.doc.session, &self.doc.engine)
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
            let dot = if self.doc.engine.is_connected() {
                self.tokens.state_done
            } else if matches!(self.doc.engine, EngineState::Connecting { .. } | EngineState::LoadingSchema) {
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
            if let Some(i) = &self.doc.installing {
                ui.spinner();
                ui.label(format!("installing {}…", i.url.trim_start_matches("https://github.com/")));
                ui.separator();
            }
            // the workflow, as the window title names it (the title isn't in screenshots)
            let dot = if self.doc.session.is_dirty() { " •" } else { "" };
            let g = self.doc.session.graph();
            let size = format!("{} nodes, {} wires", g.nodes.len(), g.wires.len());
            status_cell(
                ui,
                |ui| {
                    let doc = ui.colored_label(self.tokens.text, format!("{}{dot}", self.document_name()));
                    match self.doc.session.path() {
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
            let run = &self.doc.session.run;
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
            if let Some(ask) = &self.doc.session.save.pending {
                ui.separator();
                ui.add(egui::Spinner::new().size(12.0).color(self.tokens.state_running));
                ui.label(format!("saving {}…", short_path(&ask.path))).on_hover_text(&ask.path);
            }
            if let Some((p, label)) = &self.doc.convert_offer {
                let tip = format!("wire.convert: place {label} between them, wired, as one undo step");
                if ui.button(format!("Insert {label}")).on_hover_text(tip).clicked() {
                    commands.push(("wire.convert", p.clone()));
                }
            }
            // the messages last, in what width is left (crate::status_msgs)
            if !msgs.is_empty() {
                ui.separator();
                if status_msgs::line(ui, &msgs, &self.tokens) && !self.doc.session.views.messages {
                    commands.push(("view.messages", json!({"open": true})));
                }
            }
        });
        let was = self.doc.session.views.messages;
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
        plumcraft_editor_core::control::status_messages(&self.doc.session, self.doc.last_error.as_deref(), app)
    }

    fn watched_panel(&mut self, ui: &mut egui::Ui) {
        let frame = egui::Frame::new().fill(self.tokens.panel).inner_margin(egui::Margin::same(8));
        let asks = egui::Panel::right("plumcraft-watched").resizable(true).default_size(340.0).min_size(220.0).frame(frame).show(ui, |ui| {
            let run = &self.doc.session.run;
            let (w, panel_display) = run.panel().map_or((None, None), |(w, d)| (Some(w), self.app_display.as_ref().or(d)));
            let note = match &self.doc.session.docs.last_value {
                Some(Ok(o)) if w.is_some_and(|w| o.node == w.node && o.port == w.port) => Some(format!("opened a copy in {}: {}", o.app, o.path.display())),
                Some(Err(e)) => Some(e.clone()),
                _ => None,
            };
            let opens = watched::Opens { recent_apps: &self.doc.session.docs.recent_apps, note };
            let chosen = self.doc.session.views.panel_viewer.clone();
            let can_save = plumcraft_session::find("run.save").map_or(Err(String::new()), |c| (c.enabled)(&self.doc.session));
            let save = panel_display.filter(|d| plumcraft_session::save_cmds::takes(self.doc.session.catalog(), d.media)).map(|_| can_save);
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
                plumcraft_session::viewer_cmds::pinned(&self.doc.session),
            )
        });
        let asks = asks.inner;
        self.run_all(asks);
        if std::mem::take(&mut self.media.doc.clicked)
            && let Some(w) = self.doc.session.run.panel().map(|(w, _)| w.clone())
        {
            self.run("doc.open", &json!({"node": w.node, "port": w.port}));
        }
    }

    /// Read what the application's hooks show this frame ([`app_hooks`]), and repaint soon
    /// while they show anything.
    fn read_app_hooks(&mut self, ctx: &egui::Context) {
        self.app = app_hooks::AppShown::read();
        self.app_display = self.doc.session.run.panel().and_then(|(w, _)| self.app.display(w, self.doc.session.graph(), self.doc.session.catalog()));
        if self.app.any {
            ctx.request_repaint_after(app_hooks::REPAINT);
        }
    }

    /// The run animation, when faces are washed with it (View › Colour Nodes While Running, or
    /// a replay).
    fn shown_anim(&self) -> Option<&run_anim::RunAnim> {
        (!self.doc.session.views.run_colors_off || self.run_anim.is_replay()).then_some(&self.run_anim)
    }

    /// The scene as this window shows it (`scene.get`): the document's, in this window's theme,
    /// with its run animation, the application's lines and its editors.
    pub fn scene_now(&self) -> plumcraft_editor_core::scene::Scene {
        let tokens = plumcraft_editor_core::theme::Tokens::from(&self.tokens);
        self.doc.build_scene(&tokens, self.shown_anim(), &self.app.texts, &value_editors::scene_editors())
    }

    /// The faces the canvas draws this frame, by id ([`plumcraft_editor_core::scene::faces`]).
    fn scene_faces(&self) -> std::collections::HashMap<String, plumcraft_editor_core::scene::Face> {
        let tokens = plumcraft_editor_core::theme::Tokens::from(&self.tokens);
        let editors = value_editors::scene_editors();
        let inputs = self.doc.scene_inputs(&tokens, self.shown_anim(), &self.app.texts, &editors);
        plumcraft_editor_core::scene::faces(&inputs).into_iter().map(|f| (f.id.clone(), f)).collect()
    }

    /// Tell the core what the canvas measured (`geometry.set`'s, in process): only what changed
    /// is kept again, so a frame that changed nothing changes nothing. A script's `geometry.set`
    /// is put right the next frame.
    pub fn report_geometry(&mut self) {
        for (id, g) in self.canvas.measured_faces() {
            self.doc.geometry.set(&id, g);
        }
    }

    /// The node the Watched panel shows.
    pub fn watched_node(&self) -> Option<plumcraft_graph::NodeId> {
        self.doc.session.run.panel().map(|(w, _)| plumcraft_graph::NodeId(w.node.clone()))
    }

    /// Draw one frame into `ui` (the whole window).
    pub fn show(&mut self, ui: &mut egui::Ui) {
        // `PLUMCRAFT_TRACE_STARTUP`: say when the first frame is drawn (launch measurements)
        static FIRST_FRAME: std::sync::Once = std::sync::Once::new();
        FIRST_FRAME.call_once(|| {
            if trace_startup() {
                plumcraft_editor_core::log::say(plumcraft_editor_core::log::Level::Info, "editor", "plumcraft-editor: first frame");
            }
        });
        // what a command starts on an engine meanwhile goes to the worker (engine_view)
        let outside = !std::mem::replace(&mut self.in_frame, true);
        // the application's hooks, called while drawing, see which document this is
        plumcraft_session::commands::with_doc(self.doc.id, || self.show_frame(ui));
        if outside {
            self.in_frame = false;
        }
    }

    fn show_frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.repaint.is_none() && !self.offscreen {
            self.wake_with(&ctx);
        }
        if !self.offscreen {
            self.handle_close(&ctx);
            self.drain_control(&ctx);
        }
        if let Some(Err(e)) = self.pump_engine() {
            self.doc.last_error = Some(e);
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
        self.follow_core();
        self.canvas.sync(self.doc.session.generation(), self.doc.session.graph(), self.doc.session.catalog());
        let frame = egui::Frame::new().fill(self.tokens.canvas);
        let source = self.source();
        // what the faces show is the core's scene (N2 slice 2)
        let faces = self.scene_faces();
        let runs = RunView {
            run: &self.doc.session.run,
            thumbs: &mut self.thumbs,
            source: &source,
            docs: &self.doc.session.docs.apps,
            recent_apps: &self.doc.session.docs.recent_apps,
            anim: (!self.doc.session.views.run_colors_off || self.run_anim.is_replay()).then_some(&self.run_anim),
            faces: &faces,
        };
        let actions = egui::CentralPanel::default()
            .frame(frame)
            .show(ui, |ui| self.canvas.show(ui, self.doc.session.graph(), self.doc.session.catalog(), &self.tokens, runs))
            .inner;
        // what the canvas measured is the core's to keep (N2 slice 3): Align, Distribute, Frame
        // Selection, Tidy, the fit, the scene's wire paths and the hit tests use it
        self.report_geometry();
        self.run_all(actions);
        self.show_wire_preview(&ctx);
        // After the canvas: where there is one window (kittests, offscreen pictures) the
        // pop-outs are in-editor windows, which must draw over the nodes.
        self.show_popouts(&ctx);
        // the group windows (a text field can't live in a menu: crate::groups_view)
        let windows = groups_view::rename_window(&ctx, self.doc.session.graph());
        let windows = windows.into_iter().chain(groups_view::parameter_window(&ctx, self.doc.session.graph(), self.doc.session.catalog())).collect();
        self.run_all(windows);
        self.rename_window(&ctx);
        self.browse_for_path();
        self.attach_window(&ctx);
        self.install_window(&ctx);
        self.form_window(&ctx);
        self.script_asks_window(&ctx);
        self.params_window(&ctx);
        self.value_editor_windows(&ctx);
        if self.doc.session.views.cache_report {
            for (id, p) in cache_view::report(&ctx, &self.doc.session.run, &self.tokens) {
                self.run(id, &p);
            }
        }
        self.prompt_modal(&ctx);
        about_view::show(&ctx, &mut self.doc.session);
        let asks = graph_params_view::show(&ctx, &mut self.doc.session, &mut self.param_docs);
        if let Some((name, pos)) = asks.rename {
            self.canvas.nets.edit = Some(crate::net_view::NameEdit::new(crate::net_view::NameFor::RenamePort(name.clone()), name, pos));
        }
        self.run_all(asks.actions);
        let asks = self.settings_window(&ctx);
        self.run_all(asks);
        self.pump_runs(&ctx);
        let title = self.window_title();
        if title != self.title {
            if trace_startup() {
                plumcraft_editor_core::log::say(plumcraft_editor_core::log::Level::Info, "editor", &format!("plumcraft-editor: title {title}"));
            }
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
        let idle = !self.doc.session.run.running && self.doc.session.run.outbox_is_empty();
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
                    plumcraft_editor_core::log::say(
                        plumcraft_editor_core::log::Level::Error,
                        "editor",
                        &format!("plumcraft-editor: {}: {e}", shot.path.display()),
                    );
                    self.closing = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
            }
        }
        if let Some(image) = image {
            match save_png(&image, &shot.path) {
                Ok(()) => println!("wrote {}", shot.path.display()),
                Err(e) => {
                    plumcraft_editor_core::log::say(
                        plumcraft_editor_core::log::Level::Error,
                        "editor",
                        &format!("plumcraft-editor: {}: {e}", shot.path.display()),
                    );
                }
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
        if !dialog.target_exists(self.doc.session.graph()) {
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
        let Some(node) = self.doc.session.views.parameters.clone() else { return };
        let t = self.canvas.to_global;
        let anchor = self.canvas.node_rects_by_id().into_iter().find(|(id, _)| *id == node).map(|(_, r)| egui::Rect::from_min_max(t * r.min, t * r.max));
        let cx = params_view::Context {
            graph: self.doc.session.graph(),
            catalog: self.doc.session.catalog(),
            run: &self.doc.session.run,
            tokens: &self.tokens,
            anchor,
        };
        let actions = self.params.show(ctx, &cx, &node);
        self.run_all(actions);
    }

    fn value_editor_windows(&mut self, ctx: &egui::Context) {
        if self.doc.session.views.editors.is_empty() && self.value_editors.is_empty() {
            return;
        }
        // an application's editor asks again each frame: it gets the bytes once read, off this thread
        // an application's editor asks again each frame: it gets the bytes once read, off this
        // thread (read ahead as the run's reports arrived: values::read_ahead)
        let resolve = plumcraft_editor_core::values::nonblocking(&self.doc.media_service);
        let (actions, note) = self.value_editors.show(ctx, &self.doc.session, &resolve, &self.tokens);
        self.run_all(actions);
        if note.is_some() {
            self.doc.last_error = note;
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

/// `PLUMCRAFT_TRACE_STARTUP` is set: say on stderr when the first frame is drawn and what the
/// title says (launch measurements, `claude-debug/native-n1/s7_startup.py`).
pub fn trace_startup() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PLUMCRAFT_TRACE_STARTUP").is_some())
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

/// Parse a shortcut in the registry's notation (`Cmd+Shift+Z`) as egui's. `Cmd` is egui's
/// COMMAND (⌘ on macOS, Ctrl elsewhere). The parsing is the core's
/// ([`plumcraft_editor_core::menu::parse_shortcut`]).
pub fn parse_shortcut(s: &str) -> Option<egui::KeyboardShortcut> {
    plumcraft_editor_core::menu::parse_shortcut(s).and_then(|sc| menu_view::to_egui(&sc))
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
        app.doc.document = "Demo".into();
        assert_eq!(app.window_title(), "Demo — PlumCraft");
    }

    #[test]
    fn the_banner_says_what_is_wrong_and_how_to_fix_it() {
        let mut app = EditorApp::new(Session::default());
        assert_eq!(app.banner_text(), None);
        app.doc.engine = EngineState::EngineDown {
            reason: "exited 1".into(),
            queues: None,
            stderr_tail: Some("Traceback".into()),
            hint: "cargo build -p plumcraft-text".into(),
        };
        let text = app.banner_text().unwrap();
        assert!(text.contains("exited 1") && text.contains("cargo build -p plumcraft-text") && text.contains("Traceback"), "{text}");
    }
}
