//! PlumCraft's node editor, as a library: [`run`] with an [`AppConfig`] is the application, so
//! another can be built on it with its own name, compiled-in engines, commands and viewers
//! (PlumImpress is one). Engine manifests and crafts work there as here. `plumcraft-editor` is the
//! default configuration.
//!
//! It opens a workflow on a node canvas, connects
//! to an engine (the text engine, the audio engine with `--engine audio`, or VectorCraft through
//! the craft adapter with `--engine vectorcraft`), and lets you edit
//! and run the graph: add nodes from the palette (Tab or a
//! right-click), wire them, set their widgets, and click a node's title to watch its output.
//! Every action is a session command (plan 10.3, E1.3).
//!
//! ```text
//! plumcraft-editor [WORKFLOW.json] [--engine text|audio|array|vectorcraft|ID] [--engine-manifest FILE]... [--inproc | --process | --python | --http [--python]]
//!                  [--screenshot OUT.png [--shot-runs N]] [--control PORT [--control-token-file PATH]]
//! ```
//!
//! A workflow given opens as File › Open would (`graph.open`): it becomes the session's file,
//! so Save writes it back and its relative paths resolve against its folder. Without one, the
//! engine's demo opens, unsaved. `--engine` picks the engine (`text`, the
//! default, `audio` or `vectorcraft`); only the text engine has a Python twin, so `--python`
//! with another is an error. VectorCraft runs as a separate process unless `--inproc` (D49:
//! an adapter fault can't take the editor down), and its document values open in the
//! VectorCraft app (`doc.open`, from the craft manifest's `[gui]`). The engine is hosted as in
//! `plumcraft-view`: compiled in (mode 1, the default for text and audio), as a separate
//! process (`--process`: the Rust engine binary, `plumcraft-text-engine` or
//! `plumcraft-audio-engine` beside the editor, always, as the editor doesn't compile it in; `--python`: the Python engine under `uv run`;
//! mode 2) or over HTTP (`--http`, with `--python` for the Python engine; mode 3).
//! The flags pick the engine at startup; the Engine menu lists every engine and hosting
//! ([`choices`]) and switches while the editor runs (`engine.switch`, `engine.reconnect`,
//! `engine.attach`; `plumcraft_ui::engine_view`), keeping the graph. The editor starts them
//! through a [`Launcher`].
//!
//! A file of a kind the application opens beside workflows ([`AppConfig::openers`], say a picture
//! that holds its recipe) opens the same way, unsaved.
//!
//! **Without a window** ([`headless`]): `run FILE` computes a workflow's results and writes them as
//! Save Result does, and `nodes` lists the catalogue. `--version` and `--help` say what they say.
//!
//! Closing the window over unsaved changes (its close button, Cmd+Q, File › Quit) asks first:
//! Save, Discard or Cancel.
//!
//! `--screenshot` runs the workflow once (or `--shot-runs N` times, to see the memo's `cached`
//! nodes), saves the window after the last reply, and exits (for checking the UI without a
//! person). It falls back to drawing offscreen when the window never delivers its picture (a
//! locked screen).
//!
//! `--control PORT` serves the control channel on `127.0.0.1:PORT` (0 picks a free port; the
//! port is printed to stderr): newline-delimited JSON, authenticated by a 64-hex token read
//! from `--control-token-file` (created, mode 0600, when missing) or printed to stderr. See
//! `docs/control-protocol.md`.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod headless;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use plumcraft_automation::server;
use plumcraft_engine::host::Host;
use plumcraft_graph::Graph;
use plumcraft_host::http::{HttpHost, HttpOptions};
use plumcraft_host::manifest::{Resolved, Start};
use plumcraft_host::process::{Manifest, ProcessHost};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_session::{EngineChoice, Session};
use plumcraft_ui::link::Link;
use plumcraft_ui::{EditorApp, Shot};

/// An engine compiled into the application.
#[derive(Debug, Clone, Copy)]
pub struct PackEngine {
    /// `--engine`'s name, and what the node kinds' prefix should be.
    pub id: &'static str,
    /// Its Engine menu item.
    pub label: &'static str,
    /// Build it, on the engine's own thread.
    pub engine: fn() -> Result<plumcraft_engine::Engine, String>,
    /// Its demo workflow.
    pub demo: Option<&'static str>,
    /// Host it in a child of this same executable (mode 2), so that its crash isn't the editor's;
    /// otherwise in-process (mode 1).
    pub worker: bool,
}

/// What makes an application of this editor.
pub struct AppConfig {
    /// The program's name, for messages.
    pub name: &'static str,
    /// The window title's app name.
    pub title: &'static str,
    /// Its version, for `--version` and the About window.
    pub version: &'static str,
    /// What it was built from, name and version, for the same two; `&[]`'s worth for none. A
    /// function, since `--version` asks before any engine starts.
    pub versions: fn() -> Vec<(String, String)>,
    /// What the application's own arguments add to `--help`.
    pub help: &'static str,
    /// The app icon, a PNG.
    pub icon: &'static [u8],
    /// The engines compiled in, beside PlumCraft's.
    pub packs: Vec<PackEngine>,
    /// The engine used when none is named (the text engine when `None`).
    pub default_engine: Option<&'static str>,
    /// Commands the application adds. Their ids take the application's prefix.
    pub commands: Vec<plumcraft_session::commands::CommandSpec>,
    /// Viewers the application adds, preferred over PlumCraft's for the values they accept.
    pub viewers: Vec<plumcraft_ui::viewers::ViewerKind>,
    /// Editors the application draws, by `editor.kind`, for the nodes that name them.
    pub editors: Vec<plumcraft_ui::value_editors::EditorKind>,
    /// Files it opens as graphs beside workflows, by extension.
    pub openers: Vec<plumcraft_session::file_cmds::FileOpener>,
    /// What it does around commands and runs.
    pub hooks: plumcraft_session::commands::Hooks,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            name: "plumcraft-editor",
            title: plumcraft_ui::APP_TITLE,
            version: env!("CARGO_PKG_VERSION"),
            versions: Vec::new,
            help: "",
            icon: APP_ICON,
            packs: Vec::new(),
            default_engine: None,
            commands: Vec::new(),
            viewers: Vec::new(),
            editors: Vec::new(),
            openers: Vec::new(),
            hooks: plumcraft_session::commands::Hooks::default(),
        }
    }
}

impl PackEngine {
    fn mode(&self) -> Mode {
        if self.worker { Mode::Rust } else { Mode::InProc }
    }
}

/// The flag that makes this executable the engine process for a [`PackEngine::worker`] pack.
const WORKER_FLAG: &str = "--engine-worker";

/// Serve pack `id` to the editor that started this process, until it goes away.
fn serve_worker(id: &str, args: &[String]) -> Result<(), String> {
    let (socket, token) = plumcraft_engine::serve::endpoint_from_env(args)?;
    let i = u16::try_from(PACKS.get().map_or(0, Vec::len)).unwrap_or(u16::MAX);
    let engine = (0..i).filter_map(pack).find(|p| p.id == id).map(|p| p.engine).ok_or_else(|| format!("no engine {id:?}"))?;
    plumcraft_engine::serve::serve_unix(Box::new(engine), &socket, &token).map_err(|e| format!("the editor's socket: {e}"))
}

/// The compiled-in engines, set once by [`run`]: engines start on threads of their own.
static PACKS: std::sync::OnceLock<Vec<PackEngine>> = std::sync::OnceLock::new();

fn pack(i: u16) -> Option<&'static PackEngine> {
    PACKS.get().and_then(|p| p.get(usize::from(i)))
}

/// Which engine, hosted how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Mode 1: the Rust engine compiled in.
    InProc,
    /// Mode 2: the Rust engine binary as a separate process.
    Rust,
    /// Mode 2: the same engine written in Python, as a separate process.
    Python,
    /// Mode 3: the Rust engine over HTTP, talked to as a UI on another machine would.
    HttpRust,
    /// Mode 3: the Python engine over HTTP, likewise.
    HttpPython,
}

impl Mode {
    const ALL: [Mode; 5] = [Mode::InProc, Mode::Rust, Mode::Python, Mode::HttpRust, Mode::HttpPython];

    /// `engine.switch`'s `mode` (after the flags that pick it).
    fn id(self) -> &'static str {
        match self {
            Mode::InProc => "inproc",
            Mode::Rust => "process",
            Mode::Python => "python",
            Mode::HttpRust => "http",
            Mode::HttpPython => "http-python",
        }
    }

    fn from_id(id: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.id() == id)
    }

    /// Its Engine menu item.
    fn menu_label(self) -> &'static str {
        match self {
            Mode::InProc => "Rust · in-process (mode 1)",
            Mode::Rust => "Rust · process (mode 2)",
            Mode::Python => "Python · process (mode 2)",
            Mode::HttpRust => "Rust · HTTP (mode 3)",
            Mode::HttpPython => "Python · HTTP (mode 3)",
        }
    }

    fn python(self) -> bool {
        matches!(self, Mode::Python | Mode::HttpPython)
    }

    /// The status bar's hosting.
    fn label(self) -> &'static str {
        match self {
            Mode::InProc => "Rust · mode 1 · in-process",
            Mode::Rust => "Rust · mode 2 · process",
            Mode::Python => "Python · mode 2 · process",
            Mode::HttpRust => "Rust · mode 3 · HTTP, remote",
            Mode::HttpPython => "Python · mode 3 · HTTP, remote",
        }
    }
}

/// Which engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineKind {
    Text,
    Audio,
    /// The research software's general array nodes on the CPU (`plumcraft-array`).
    Array,
    /// The craft adapter on an artcraft app's MCP server (`plumcraft-craft`, plan 10.6 M3): a
    /// built-in pack's id (`vectorcraft`, `lightcraft`, …), from `crates/craft/packs/`.
    Craft(&'static str),
    /// An engine an engine manifest names (spec 16.6): an index into [`EXTERNALS`].
    External(u16),
    /// An engine compiled into the application: an index into [`PACKS`].
    Pack(u16),
}

/// An engine an engine manifest names.
#[derive(Debug, Clone)]
struct External {
    id: String,
    title: String,
    resolved: Resolved,
}

thread_local! {
    /// The engines manifests name. On the UI thread, where engines are chosen and started:
    /// built at startup, read again by `engine.manifests.reload`.
    static EXTERNALS: RefCell<Vec<External>> = const { RefCell::new(Vec::new()) };
}

fn external(i: u16) -> Option<External> {
    EXTERNALS.with_borrow(|e| e.get(usize::from(i)).cloned())
}

/// The folder engines install their manifests in.
fn manifest_folder() -> Option<PathBuf> {
    // home(), not $HOME: Windows sets no HOME for an app started from the desktop, so the folder
    // `register` had just written to was never read there
    plumcraft_proto::programs::home().map(|h| h.join(".config").join("plumcraft").join("engines"))
}

/// Fill [`EXTERNALS`] from `files`, then every `*.toml` in `folder`. What can't be used (it won't
/// parse, or its id is taken, by a built-in engine or an earlier manifest) is reported, and the
/// rest still load.
fn load_manifests(files: &[PathBuf], folder: Option<&Path>) -> Vec<String> {
    let mut listed: Vec<PathBuf> = files.to_vec();
    if let Some(Ok(dir)) = folder.map(std::fs::read_dir) {
        let mut found: Vec<PathBuf> = dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
        found.sort();
        listed.extend(found);
    }
    let builtin: Vec<String> = [EngineKind::Text, EngineKind::Audio, EngineKind::Array]
        .into_iter()
        .map(EngineKind::id)
        .chain(plumcraft_craft::manifest::BUILTIN.iter().map(|b| (*b).to_owned()))
        .collect();
    let (mut loaded, mut problems): (Vec<External>, Vec<String>) = (Vec::new(), Vec::new());
    for path in listed {
        match plumcraft_host::manifest::load(&path) {
            Err(e) => problems.push(e),
            Ok(r) if builtin.contains(&r.manifest.id) => problems.push(format!("{}: id {:?} is a built-in engine", path.display(), r.manifest.id)),
            Ok(r) => match loaded.iter().find(|x| x.id == r.manifest.id) {
                // the same file twice (given and installed) is one engine
                Some(x) if x.resolved.path == r.path => {}
                Some(x) => problems.push(format!("{}: id {:?} is already {}", path.display(), r.manifest.id, x.resolved.path.display())),
                None => loaded.push(External { id: r.manifest.id.clone(), title: r.manifest.title().to_owned(), resolved: r }),
            },
        }
    }
    EXTERNALS.with_borrow_mut(|e| *e = loaded);
    problems
}

/// How to get the audio engine, which is built apart from the editor: its SoundCraft crates
/// aren't on crates.io, and the interface makes it one more process (docs/guide.md).
const AUDIO_HINT: &str = "the audio engine is optional: in a PlumCraft checkout, cargo build -p plumcraft-audio, beside the editor";

/// A workflow with no nodes, for a craft whose pack has no demo.
const EMPTY_WORKFLOW: &str = r#"{"version": 1, "nodes": {}, "wires": []}"#;

impl EngineKind {
    /// Every engine: text, audio, each built-in craft pack, then those manifests name.
    fn all() -> Vec<EngineKind> {
        let externals = EXTERNALS.with_borrow(Vec::len);
        let packs = PACKS.get().map_or(0, Vec::len);
        (0..packs)
            .filter_map(|i| u16::try_from(i).ok())
            .map(EngineKind::Pack)
            .chain([EngineKind::Text, EngineKind::Audio, EngineKind::Array])
            .chain(plumcraft_craft::manifest::BUILTIN.iter().map(|id| EngineKind::Craft(id)))
            .chain((0..externals).filter_map(|i| u16::try_from(i).ok()).map(EngineKind::External))
            .collect()
    }

    /// `--engine`'s name and `engine.switch`'s `engine`.
    fn id(self) -> String {
        match self {
            EngineKind::Text => "text".into(),
            EngineKind::Audio => "audio".into(),
            EngineKind::Array => "array".into(),
            EngineKind::Craft(id) => id.into(),
            EngineKind::External(i) => external(i).map_or_else(String::new, |e| e.id),
            EngineKind::Pack(i) => pack(i).map_or_else(String::new, |p| p.id.to_owned()),
        }
    }

    fn from_id(id: &str) -> Option<EngineKind> {
        EngineKind::all().into_iter().find(|k| k.id() == id)
    }

    fn label(self) -> String {
        match self {
            EngineKind::Text => "Text".into(),
            EngineKind::Audio => "Audio".into(),
            EngineKind::Array => "Array".into(),
            EngineKind::Craft(id) => plumcraft_craft::manifest::builtin(id).and_then(Result::ok).map_or_else(|| id.to_owned(), |m| m.name),
            EngineKind::External(i) => external(i).map_or_else(String::new, |e| e.title),
            EngineKind::Pack(i) => pack(i).map_or_else(String::new, |p| p.label.to_owned()),
        }
    }

    /// Does a manifest start this engine, or is it already running?
    fn attaches(self) -> bool {
        matches!(self, EngineKind::External(i) if external(i).is_some_and(|e| matches!(e.resolved.start, Start::Attach { .. })))
    }

    fn is_craft(self) -> bool {
        matches!(self, EngineKind::Craft(_))
    }

    /// Is there an engine `self` hosted as `mode`? Only the text engine has a Python twin; the
    /// audio engine and one a manifest names are a process or HTTP, and one already running is
    /// only HTTP.
    fn hosts(self, mode: Mode) -> bool {
        match self {
            EngineKind::External(_) if self.attaches() => mode == Mode::HttpRust,
            EngineKind::External(_) | EngineKind::Audio => matches!(mode, Mode::Rust | Mode::HttpRust),
            EngineKind::Pack(i) => mode == pack(i).map_or(Mode::InProc, PackEngine::mode),
            _ => self == EngineKind::Text || !mode.python(),
        }
    }

    /// Its Engine menu item for `mode`: "Rust" doesn't describe an engine a manifest names.
    fn mode_label(self, mode: Mode) -> String {
        match (self, mode) {
            (EngineKind::External(_), Mode::HttpRust) if self.attaches() => "Running · HTTP (mode 3)".into(),
            (EngineKind::External(_), Mode::HttpRust) => "HTTP (mode 3)".into(),
            (EngineKind::External(_), _) => "Process (mode 2)".into(),
            _ => mode.menu_label().into(),
        }
    }

    /// How it is hosted without a flag: the craft adapter and the audio engine as their own
    /// processes, the others compiled in.
    fn default_mode(self) -> Mode {
        match self {
            EngineKind::External(_) if self.attaches() => Mode::HttpRust,
            EngineKind::External(_) | EngineKind::Audio => Mode::Rust,
            // An application on this library may be one file, with no adapter binary beside it.
            EngineKind::Craft(_) if PACKS.get().is_none_or(Vec::is_empty) => Mode::Rust,
            EngineKind::Pack(i) => pack(i).map_or(Mode::InProc, PackEngine::mode),
            _ => Mode::InProc,
        }
    }

    /// `--engine NAME` (text when absent); with several, `--engine a,b`, the first.
    fn from_args(args: &[String]) -> Result<Self, String> {
        Ok(Self::all_from_args(args)?.first().copied().unwrap_or(EngineKind::Text))
    }

    /// Every engine `--engine a,b,…` names, in order (text when absent); none twice.
    fn all_from_args(args: &[String]) -> Result<Vec<Self>, String> {
        if !args.iter().any(|a| a == "--engine") {
            return Ok(vec![EngineKind::Text]);
        }
        let names = || EngineKind::all().into_iter().map(EngineKind::id).collect::<Vec<_>>().join(", ");
        let Some(list) = flag_value(args, "--engine") else { return Err(format!("--engine needs a name: {}", names())) };
        let mut kinds = Vec::new();
        for name in list.split(',').map(str::trim) {
            let k = EngineKind::from_id(name).ok_or_else(|| format!("--engine {name:?}: the engines are {}", names()))?;
            if kinds.contains(&k) {
                return Err(format!("--engine names {name} twice"));
            }
            kinds.push(k);
        }
        Ok(kinds)
    }

    /// The engine compiled in (mode 1). The craft adapter starts the craft's MCP server and
    /// reads its catalogue first, which can fail (the craft not built).
    fn engine(self) -> Result<plumcraft_engine::Engine, String> {
        match self {
            EngineKind::Text => Ok(plumcraft_text::engine()),
            EngineKind::Audio => Err("the audio engine runs as its own process, plumcraft-audio-engine".into()),
            EngineKind::Array => Ok(plumcraft_array::engine()),
            EngineKind::Craft(id) => plumcraft_craft::manifest::Manifest::named(id).and_then(|m| plumcraft_craft::engine(&m)),
            EngineKind::External(_) => Err("an engine a manifest names runs as its own process".into()),
            EngineKind::Pack(i) => pack(i).ok_or_else(|| "that engine is not compiled in".to_owned()).and_then(|p| (p.engine)()),
        }
    }

    /// Its demo workflow (an empty one for a craft without a demo).
    fn demo(self) -> Result<String, String> {
        Ok(match self {
            EngineKind::Text => plumcraft_text::DEMO_WORKFLOW.into(),
            EngineKind::Audio => include_str!("../demos/audio.json").into(),
            EngineKind::Array => plumcraft_array::DEMO_WORKFLOW.into(),
            EngineKind::Craft(id) => plumcraft_craft::manifest::demo(id).unwrap_or(EMPTY_WORKFLOW).into(),
            EngineKind::External(i) => match external(i).and_then(|e| e.resolved.demo) {
                Some(p) => std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?,
                None => EMPTY_WORKFLOW.into(),
            },
            EngineKind::Pack(i) => pack(i).and_then(|p| p.demo).unwrap_or(EMPTY_WORKFLOW).into(),
        })
    }

    /// The engine binary's name, the command that builds it, and its arguments: `None` for one
    /// a manifest names.
    fn binary(self) -> Option<(&'static str, &'static str, &'static str, Vec<String>)> {
        match self {
            EngineKind::Text => Some(("text", "plumcraft-text-engine", "cargo build -p plumcraft-text", Vec::new())),
            EngineKind::Audio => Some(("audio", "plumcraft-audio-engine", AUDIO_HINT, Vec::new())),
            EngineKind::Array => Some(("array", "plumcraft-array-engine", "cargo build -p plumcraft-array", Vec::new())),
            EngineKind::Craft(id) => Some((id, "plumcraft-craft-engine", "cargo build -p plumcraft-craft", vec!["--craft".into(), id.into()])),
            EngineKind::External(_) | EngineKind::Pack(_) => None,
        }
    }

    /// How long its engine may take to be ready: the craft adapter starts the craft's process
    /// and reads its commands and catalogues first (VectorCraft: 800 commands, three catalogues).
    fn ready_wait(self) -> Duration {
        // uv may have an environment to build before an external engine starts
        if self.is_craft() || matches!(self, EngineKind::External(_) | EngineKind::Pack(_)) { Duration::from_secs(60) } else { Duration::from_secs(10) }
    }
}

/// The apps that open document values ("Open in VectorCraft"), from the craft manifests the
/// editor trusts: never from an engine's messages.
fn doc_apps() -> Vec<plumcraft_session::DocApp> {
    plumcraft_craft::manifest::BUILTIN
        .iter()
        .filter_map(|name| plumcraft_craft::manifest::builtin(name)?.ok())
        .filter_map(|m| {
            let gui = m.gui.clone()?;
            Some(plumcraft_session::DocApp {
                app: m.doc.app.clone(),
                wire: m.doc.wire.clone(),
                label: gui.label.clone().unwrap_or_else(|| m.name.clone()),
                ext: m.doc.ext.clone(),
                program: gui.executable(),
                args: gui.program().into_iter().skip(1).collect(),
                viewer: has_own_view(&m.doc.wire, &m.doc.media, &m.doc.app),
            })
        })
        .collect()
}

/// Whether one of PlumCraft's viewers, besides the document's rendition and its bytes, shows a
/// craft's documents (PdfCraft's PDFs: pages). Without one, a pop-out opens the app.
fn has_own_view(wire: &str, media: &str, app: &str) -> bool {
    use plumcraft_proto::data::{BlobMeta, DataRef, Form};
    let d = plumcraft_proto::display::Display {
        media: plumcraft_proto::catalogue::Media::Doc,
        ty: wire.to_owned(),
        full: Some(DataRef { meta: BlobMeta { media: media.to_owned(), form: Form::Doc { app: app.to_owned() }, ..Default::default() }, ..Default::default() }),
        ..Default::default()
    };
    plumcraft_ui::viewers::kinds_for(&d).iter().any(|k| !matches!(k.id, "doc" | "hex"))
}

/// The app icon (window and Dock), the 1024 px master; credits in `plums/CREDITS.md`.
const APP_ICON: &[u8] = include_bytes!("../assets/plumcraft.png");

/// Flags followed by a value.
const VALUE_FLAGS: &[&str] = &["--screenshot", "--shot-runs", "--control", "--control-token-file", "--engine", "--engine-manifest"];

/// The value after flag `name`.
fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1))
}

/// The workflow path: the first argument that is neither a flag nor a flag's value.
fn workflow_arg(args: &[String]) -> Option<PathBuf> {
    args.iter()
        .enumerate()
        .find(|(i, a)| !a.starts_with("--") && (*i == 0 || args.get(i - 1).is_none_or(|p| !VALUE_FLAGS.contains(&p.as_str()))))
        .map(|(_, a)| PathBuf::from(a))
}

/// `--control PORT` and its token: `Ok(None)` without the flag.
fn control_args(args: &[String]) -> Result<Option<(u16, String, Option<PathBuf>)>, String> {
    if !args.iter().any(|a| a == "--control") {
        return Ok(None);
    }
    let port = flag_value(args, "--control").and_then(|p| p.parse::<u16>().ok()).ok_or("--control needs a port number (0 picks a free one)")?;
    let token_file = flag_value(args, "--control-token-file").map(PathBuf::from);
    let token = server::server_token(token_file.as_deref()).map_err(|e| format!("control token: {e}"))?;
    Ok(Some((port, token, token_file)))
}

/// winit's default macOS menu quits on Cmd+Q without a close request the editor could cancel;
/// without it, Cmd+Q is a key the editor binds to `app.quit`, which asks over unsaved changes.
#[cfg(target_os = "macos")]
fn no_default_menu(builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {
    use winit::platform::macos::EventLoopBuilderExtMacOS;
    builder.with_default_menu(false);
}

#[cfg(not(target_os = "macos"))]
fn no_default_menu(_: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {}

/// Start the control server and hand its requests to `app` (errors are printed, not fatal).
fn start_control(program: &str, app: &mut EditorApp, ctx: &egui::Context, port: u16, token: String, token_file: Option<&Path>) {
    let wake_ctx = ctx.clone();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || wake_ctx.request_repaint());
    match server::start(port, token.clone(), wake) {
        Ok((rx, bound)) => {
            eprintln!("{program}: control channel on 127.0.0.1:{bound}");
            match token_file {
                Some(p) => eprintln!("{program}: control token in {}", p.display()),
                None => eprintln!("{program}: control token: {token}"),
            }
            app.control_rx = Some(rx);
        }
        Err(e) => eprintln!("{program}: {e}"),
    }
}

/// The engine and mode, checked: there is no Python audio engine.
fn choose(args: &[String]) -> Result<(EngineKind, Mode), String> {
    let kind = EngineKind::from_args(args)?;
    let (python, http) = (args.iter().any(|a| a == "--python"), args.iter().any(|a| a == "--http"));
    let mode = match (python, http, args.iter().any(|a| a == "--process"), args.iter().any(|a| a == "--inproc")) {
        (true, true, ..) => Mode::HttpPython,
        (false, true, ..) => Mode::HttpRust,
        (true, false, ..) => Mode::Python,
        (false, false, true, _) => Mode::Rust,
        (false, false, false, true) => Mode::InProc,
        _ => kind.default_mode(),
    };
    if !kind.hosts(mode) {
        if mode == Mode::InProc {
            return Err(format!("--inproc: the {} engine isn't compiled in (use --process or --http)", kind.id()));
        }
        return Err(format!("--python: there is no Python {} engine (use --process or --http for the Rust one)", kind.id()));
    }
    Ok((kind, mode))
}

/// Every engine and hosting the Engine menu offers.
fn choices() -> Vec<EngineChoice> {
    EngineKind::all()
        .into_iter()
        .flat_map(|k| Mode::ALL.into_iter().filter(move |m| k.hosts(*m)).map(move |m| (k, m)))
        .map(|(k, m)| EngineChoice { engine: k.id(), mode: m.id().into(), engine_label: k.label(), mode_label: k.mode_label(m) })
        .collect()
}

/// Where engine binary `name` is: beside the editor, or (the editor's tests run from
/// `target/<profile>/deps`) one folder up.
fn engine_binary(name: &str) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // `plumcraft-text-engine.exe` on Windows
    let name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let name = name.as_str();
    let beside = exe.with_file_name(name);
    if beside.exists() {
        return Ok(beside);
    }
    let up = exe.parent().filter(|d| d.file_name().is_some_and(|n| n == "deps")).and_then(Path::parent).map(|d| d.join(name));
    Ok(up.filter(|p| p.exists()).unwrap_or(beside))
}

/// Starts the engines (`connect`); tests point it at other binaries.
#[derive(Debug, Clone, Default)]
struct Launcher {
    /// Engine binaries to use instead of the ones beside the editor.
    binaries: Vec<(EngineKind, PathBuf)>,
    /// `--engine-manifest`'s files, read again by `engine.manifests.reload`.
    manifests: Vec<PathBuf>,
}

impl Launcher {
    /// The Engine menu's connector: `engine.switch`'s ids, checked, then [`Launcher::connect`].
    fn connector(self) -> plumcraft_ui::engine_view::Connector {
        Box::new(move |engine, mode, ctx| {
            let kind = EngineKind::from_id(engine).ok_or_else(|| format!("no engine {engine:?}"))?;
            let mode = Mode::from_id(mode).ok_or_else(|| format!("no hosting {mode:?}"))?;
            self.connect(kind, mode, ctx)
        })
    }

    /// Start engine `kind` in `mode` and link to it.
    fn connect(&self, kind: EngineKind, mode: Mode, ctx: Option<&egui::Context>) -> Result<Link, String> {
        let bin = self.binaries.iter().find(|(k, _)| *k == kind).map(|(_, p)| p.clone());
        connect_with(kind, mode, ctx, bin)
    }
}

/// Start engine `kind` in `mode` (as `plumcraft-view` does) and link to it, with the engine
/// binary `bin` (mode 2 and 3, Rust) when given.
fn connect_with(kind: EngineKind, mode: Mode, ctx: Option<&egui::Context>, bin: Option<PathBuf>) -> Result<Link, String> {
    if let EngineKind::External(i) = kind {
        let e = external(i).ok_or("that engine's manifest is gone (Engine › Reload Engine List)")?;
        return connect_external(&e, mode, kind.ready_wait(), ctx);
    }
    let manifest = match mode {
        Mode::InProc => None,
        Mode::Rust if matches!(kind, EngineKind::Pack(_)) => {
            let exe = std::env::current_exe().map_err(|e| format!("this program's path: {e}"))?;
            let command = vec![exe.display().to_string(), WORKER_FLAG.to_owned(), kind.id()];
            Some(Manifest { name: kind.id(), command, env: Vec::new(), hint: "the engine's process stopped".into(), store: None, cwd: None })
        }
        Mode::Rust | Mode::HttpRust => {
            let (name, binary, hint, args) = kind.binary().ok_or("this engine has no binary")?;
            let exe = match bin {
                Some(b) => b,
                None => engine_binary(binary)?,
            };
            if !exe.is_file() {
                return Err(format!("no engine binary at {}. Fix: {hint}", exe.display()));
            }
            let command = std::iter::once(exe.display().to_string()).chain(args).collect();
            Some(Manifest { name: name.into(), command, env: Vec::new(), hint: hint.into(), store: None, cwd: None })
        }
        Mode::Python | Mode::HttpPython if kind != EngineKind::Text => return Err(format!("there is no Python {} engine", kind.id())),
        Mode::Python | Mode::HttpPython => {
            let (command, env) = plumcraft_text::python_command();
            Some(Manifest {
                name: "text (Python)".into(),
                command,
                env,
                hint: "needs uv (cd python && uv sync), or set PLUMCRAFT_UV".into(),
                store: None,
                cwd: None,
            })
        }
    };
    let conn: Arc<dyn Connection> = if let (Some(m), true) = (&manifest, matches!(mode, Mode::HttpRust | Mode::HttpPython)) {
        Arc::new(HttpHost::spawn(m, HttpOptions { remote: true, ..Default::default() })?)
    } else if let Some(m) = manifest {
        let h = ProcessHost::start(m).map_err(|e| e.to_string())?;
        h.wait_ready(kind.ready_wait());
        Arc::new(h)
    } else {
        let h = Host::start(Box::new(move || kind.engine()), None);
        h.wait_ready(kind.ready_wait());
        Arc::new(InProc(h))
    };
    Link::start(conn, mode.label(), ctx.cloned())
}

/// Start the engine a manifest names and link to it: a process on a socket, or HTTP, spawned or
/// already running.
fn connect_external(e: &External, mode: Mode, wait: Duration, ctx: Option<&egui::Context>) -> Result<Link, String> {
    let (conn, label): (Arc<dyn Connection>, &str) = match (&e.resolved.start, mode) {
        (Start::Attach { url, token }, _) => (Arc::new(HttpHost::attach(url, token, HttpOptions::default())?), "mode 3 · HTTP, running"),
        (Start::Command(m), Mode::HttpRust) => (Arc::new(HttpHost::spawn(m, HttpOptions { remote: true, ..Default::default() })?), "mode 3 · HTTP"),
        (Start::Command(m), _) => {
            let h = ProcessHost::start(m.clone()).map_err(|e| e.to_string())?;
            h.wait_ready(wait);
            (Arc::new(h), "mode 2 · process")
        }
    };
    Link::start(conn, label, ctx.cloned())
}

/// The demos for several engines (`demos/`, docs/research/15-multi-engine.md): the engines each
/// needs, the workflow and its name. `--engine` naming exactly those engines, in any order,
/// and no workflow, opens it.
const MULTI_DEMOS: &[(&[&str], &str, &str)] = &[
    (&["array", "vectorcraft"], include_str!("../demos/trace-and-measure.json"), "Trace and measure"),
    (&["array", "audio", "text"], include_str!("../demos/chord.json"), "What a chord looks like"),
    (&["array", "gridcraft", "text"], include_str!("../demos/analytics.json"), "Analytics in GridCraft"),
];

/// The demo for exactly these engines, if there is one.
fn multi_demo(kinds: &[EngineKind]) -> Option<(&'static str, &'static str)> {
    let mut ids: Vec<String> = kinds.iter().map(|k| k.id()).collect();
    ids.sort_unstable();
    MULTI_DEMOS.iter().find(|(engines, _, _)| engines.iter().eq(ids.iter())).map(|(_, w, n)| (*w, *n))
}

/// Read a workflow (engine `kind`'s demo when no path is given), placing nodes without a
/// position, as `graph.open` reads it.
fn read_workflow(path: Option<&Path>, kind: EngineKind) -> Result<(Graph, String), String> {
    match path {
        Some(p) => {
            let graph = plumcraft_session::file_cmds::read_any(p).map_err(|e| e.to_string())?;
            Ok((graph, p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())))
        }
        None => Ok((plumcraft_session::file_cmds::parse_workflow(&kind.demo()?).map_err(|e| format!("the demo: {e}"))?, "Demo".to_owned())),
    }
}

/// The editor on `graph` (read from `path`, which it opens with `graph.open`, or the demo),
/// linked to the engine (or saying why it isn't).
#[cfg(test)]
fn editor(ctx: &egui::Context, graph: Graph, path: Option<&Path>, name: String, kind: EngineKind, mode: Mode, shot: Option<Shot>) -> EditorApp {
    editor_with(ctx, graph, path, name, kind, mode, shot, Launcher::default())
}

/// [`editor`], starting engines with `launcher` (tests point it at other binaries).
#[allow(clippy::too_many_arguments)]
fn editor_with(
    ctx: &egui::Context,
    graph: Graph,
    path: Option<&Path>,
    name: String,
    kind: EngineKind,
    mode: Mode,
    shot: Option<Shot>,
    launcher: Launcher,
) -> EditorApp {
    let link = launcher.connect(kind, mode, Some(ctx));
    // The catalogue first, so the session titles and checks the graph against it.
    let catalog = link
        .as_ref()
        .ok()
        .and_then(|l| l.conn.call("schema", serde_json::json!({})).ok())
        .and_then(|v| serde_json::from_value::<SchemaReply>(v).ok())
        .unwrap_or_default();
    let mut app = match path {
        Some(p) => {
            let mut app = EditorApp::new(Session::new(catalog));
            // its file, its folder for relative paths, its problems in the status bar
            app.run("graph.open", &serde_json::json!({"path": p}));
            app
        }
        None => EditorApp::new(Session::with_graph(catalog, graph)),
    };
    // "Open in VectorCraft" on document values (session doc.open)
    app.session.docs.apps = doc_apps();
    app.document = name;
    app.shot = shot;
    app.setup_context(ctx);
    // The Engine menu: every engine and hosting, this one checked.
    let manifests = launcher.manifests.clone();
    let app = app.with_engines(choices(), Some((kind.id(), mode.id().into())), launcher.connector()).with_reloader(Box::new(move || {
        let problems = load_manifests(&manifests, manifest_folder().as_deref());
        (choices(), problems)
    }));
    let mut app = app.with_installer(std::sync::Arc::new(|url: &str| install_engine(url, &[])));
    match link {
        Ok(link) => app.with_link(link),
        Err(e) => {
            app.last_error = Some(format!("engine: {e}"));
            app
        }
    }
}

/// The commands `uv tool install` says it installed ("Installed 2 executables: a, b").
fn installed_executables(said: &str) -> Vec<String> {
    said.lines()
        .filter_map(|l| l.trim().strip_prefix("Installed ")?.split_once(": ").map(|(_, names)| names.to_owned()))
        .flat_map(|names| names.split(", ").map(|n| n.trim().to_owned()).collect::<Vec<_>>())
        .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)))
        .collect()
}

/// The last lines of a command's output, for an error.
fn tail(out: &std::process::Output) -> String {
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(4)..].join("\n")
}

/// Engine › Install Engine…: `uv tool install --reinstall git+URL`, then `register` on each
/// command it installed, so the engine's manifest is in `~/.config/plumcraft/engines/`. `env` is
/// set on both (a test's own tool and config folders).
fn install_engine(url: &str, env: &[(&str, &std::path::Path)]) -> Result<String, String> {
    use std::process::Command;
    let uv = plumcraft_host::manifest::program("uv").ok_or("needs uv (https://docs.astral.sh/uv), or set PLUMCRAFT_UV")?;
    let run = |cmd: &mut Command| -> Result<std::process::Output, String> {
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.output().map_err(|e| e.to_string())
    };
    let out = run(Command::new(&uv).args(["tool", "install", "--reinstall"]).arg(format!("git+{url}")))?;
    if !out.status.success() {
        return Err(format!("uv tool install failed: {}", tail(&out)));
    }
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let exes = installed_executables(&said);
    if exes.is_empty() {
        return Err(format!("uv installed it but named no command: {}", tail(&out)));
    }
    let bin = run(Command::new(&uv).args(["tool", "dir", "--bin"]))?;
    let bin = std::path::PathBuf::from(String::from_utf8_lossy(&bin.stdout).trim());
    let mut registered = Vec::new();
    for exe in &exes {
        let out = run(Command::new(bin.join(exe)).arg("register"))?;
        if !out.status.success() {
            return Err(format!("installed {exe}, but `{exe} register` failed: {}", tail(&out)));
        }
        registered.push(exe.as_str());
    }
    Ok(format!("installed and registered {} from {url}", registered.join(", ")))
}

/// The engines a workflow file names (`"engines": [...]`), for a headless command given no
/// `--engine`: the first argument that isn't a flag (or a flag's value) is the file.
fn file_engines(args: &[String]) -> Option<Vec<String>> {
    let mut it = args.iter();
    let mut file = None;
    while let Some(a) = it.next() {
        if ["--out", "--node", "--format", "--engine-manifest"].contains(&a.as_str()) {
            it.next();
        } else if !a.starts_with('-') {
            file = Some(a);
            break;
        }
    }
    let text = std::fs::read_to_string(file?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let ids: Vec<String> = v.get("engines")?.as_array()?.iter().filter_map(|e| e.as_str().map(str::to_owned)).collect();
    if ids.is_empty() { None } else { Some(ids) }
}

/// `--engine-manifest`'s files, in order.
fn manifest_args(args: &[String]) -> Vec<PathBuf> {
    args.windows(2).filter(|w| w[0] == "--engine-manifest").map(|w| PathBuf::from(&w[1])).collect()
}

/// `args`, with `--engine` naming the engines the workflow names, for `run`, `check` and `tidy`
/// given no `--engine`. A manifest given (`--engine-manifest`) only says how to start one of them.
fn with_file_engines(mut args: Vec<String>) -> Vec<String> {
    if matches!(args.first().map(String::as_str), Some("run" | "check" | "tidy"))
        && !args.iter().any(|a| a == "--engine")
        && let Some(ids) = file_engines(&args[1..])
    {
        args.extend(["--engine".to_owned(), ids.join(",")]);
    }
    args
}

/// `args`, with `--engine <id>` of the first manifest given when `--engine` names none.
fn with_engine_arg(mut args: Vec<String>) -> Vec<String> {
    if !args.iter().any(|a| a == "--engine")
        && let Some(id) = manifest_args(&args).first().and_then(|f| plumcraft_host::manifest::load(f).ok()).map(|r| r.manifest.id)
    {
        args.extend(["--engine".to_owned(), id]);
    }
    args
}

/// Run the application `config` describes.
pub fn run(config: AppConfig) -> ExitCode {
    let AppConfig { name: program, title, version, versions, help, icon, packs, default_engine, commands, viewers, editors, openers, hooks } = config;
    let added = plumcraft_session::commands::register(commands)
        .and_then(|()| plumcraft_session::commands::set_hooks(hooks))
        .and_then(|()| plumcraft_ui::viewers::register(viewers))
        .and_then(|()| plumcraft_ui::value_editors::register(editors))
        .and_then(|()| plumcraft_session::file_cmds::register_openers(openers));
    if let Err(e) = added.and_then(|()| PACKS.set(packs).map_err(|_| "the application started twice".to_owned())) {
        eprintln!("{program}: {e}");
        return ExitCode::FAILURE;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == WORKER_FLAG) {
        let id = args.get(1).cloned().unwrap_or_default();
        return match serve_worker(&id, &args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{program}: {e}");
                ExitCode::FAILURE
            }
        };
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("{}", version_text(title, version, &versions()));
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{}{help}", usage(program));
        return ExitCode::SUCCESS;
    }
    let mut args = with_file_engines(args);
    if let Some(engine) = default_engine.filter(|_| !args.iter().any(|a| a == "--engine" || a == "--engine-manifest")) {
        args.extend(["--engine".to_owned(), engine.to_owned()]);
    }
    let manifests = manifest_args(&args);
    for problem in load_manifests(&manifests, manifest_folder().as_deref()) {
        eprintln!("{program}: {problem}");
    }
    let args = with_engine_arg(args);
    let (kind, mode) = match choose(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{program}: {e}");
            return ExitCode::from(2);
        }
    };
    let extras: Vec<EngineKind> = EngineKind::all_from_args(&args).map(|k| k.into_iter().skip(1).collect()).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("run") => return headless::run(program, &args[1..], kind, mode, &extras, manifests),
        Some("nodes") => return headless::nodes(program, kind, mode, &extras, manifests),
        Some("check") => return headless::check(program, &args[1..], kind, mode, &extras, manifests),
        Some("tidy") => return headless::tidy(program, &args[1..], kind, mode, &extras, manifests),
        _ => {}
    }
    let shot_runs = flag_value(&args, "--shot-runs").and_then(|n| n.parse().ok()).unwrap_or(1);
    let shot = flag_value(&args, "--screenshot").map(|p| Shot::new(PathBuf::from(p), shot_runs));
    let path = match workflow_arg(&args) {
        // a bundle unpacks beside itself, and its workflow opens from there
        Some(p) if plumcraft_session::archive::is_zip(&p) => match plumcraft_session::archive::extract_bundle(&p) {
            Ok(workflow) => Some(workflow),
            Err(e) => {
                eprintln!("{program}: {e}");
                return ExitCode::from(2);
            }
        },
        p => p,
    };
    let control = match control_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{program}: {e}");
            return ExitCode::from(2);
        }
    };
    let all_kinds: Vec<EngineKind> = std::iter::once(kind).chain(extras.iter().copied()).collect();
    let read = match (path.as_deref(), multi_demo(&all_kinds)) {
        (None, Some((demo, title))) => plumcraft_session::file_cmds::parse_workflow(demo).map(|g| (g, title.to_owned())).map_err(|e| format!("the demo: {e}")),
        (p, _) => read_workflow(p, kind),
    };
    let (graph, name) = match read {
        Ok(g) => g,
        Err(e) => {
            eprintln!("{program}: {e}");
            return ExitCode::from(2);
        }
    };
    let mut viewport = egui::ViewportBuilder::default().with_title(title).with_inner_size([1440.0, 860.0]).with_min_inner_size([720.0, 440.0]);
    // A bad icon costs the icon, not the editor.
    match eframe::icon_data::from_png_bytes(icon) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(e) => eprintln!("{program}: app icon: {e}"),
    }
    let options =
        eframe::NativeOptions { viewport, renderer: eframe::Renderer::Wgpu, event_loop_builder: Some(Box::new(no_default_menu)), ..Default::default() };
    let creator = move |cc: &eframe::CreationContext<'_>| -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
        let launcher = Launcher { manifests, ..Default::default() };
        // `--screenshot FILE` is an unattended run of a file named here: nobody to ask, and naming
        // it is the approval, as with the `run` command
        let unattended = shot.is_some();
        let mut app = editor_with(&cc.egui_ctx, graph, path.as_deref(), name, kind, mode, shot, launcher);
        app.app_title = title;
        if !unattended && let Some(file) = plumcraft_ui::prefs::trust_path_for(program) {
            app.session.set_trust_file(file);
        }
        app.session.about = about(title, version, &versions());
        // this application's own preferences, by its name
        app.use_prefs(plumcraft_ui::prefs::Store::for_app(program), &cc.egui_ctx);
        // `--engine a,b`: the others beside the first (Engine › Add Engine), hosted as the
        // first where they can be, else as they are by default. A pack's hosting is its own: its
        // application may be one executable, with no engine binaries beside it.
        for extra in extras {
            let m = if extra.hosts(mode) && !matches!(kind, EngineKind::Pack(_)) { mode } else { extra.default_mode() };
            if let Err(e) = app.try_run("engine.add", &serde_json::json!({"engine": extra.id(), "mode": m.id()})) {
                eprintln!("{program}: --engine {}: {e}", extra.id());
            }
        }
        if let Some((port, token, token_file)) = control {
            start_control(program, &mut app, &cc.egui_ctx, port, token, token_file.as_deref());
        }
        Ok(Box::new(app))
    };
    match eframe::run_native(title, options, Box::new(creator)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{program}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn about(title: &str, version: &str, parts: &[(String, String)]) -> plumcraft_session::About {
    plumcraft_session::About { title: title.to_owned(), version: version.to_owned(), parts: parts.to_vec() }
}

/// `--version`: the application, then what it was built from.
fn version_text(title: &str, version: &str, parts: &[(String, String)]) -> String {
    std::iter::once(format!("{title} {version}")).chain(parts.iter().map(|(n, v)| format!("{n} {v}"))).collect::<Vec<_>>().join("\n")
}

/// `--help`.
fn usage(program: &str) -> String {
    format!(
        "usage: {program} [WORKFLOW] [--engine ID[,ID...]] [--engine-manifest FILE]... [--inproc | --process | --python | --http]
               [--screenshot OUT.png [--shot-runs N]] [--control PORT [--control-token-file PATH]]
       {program} run FILE [--out DIR] [--node NODE[.PORT]]... [--format EXT] [--sheet] [--engine ID[,ID...]]
       {program} check FILE [--engine ID[,ID...]]
       {program} tidy FILE [--engine ID[,ID...]]
       {program} nodes [--engine ID[,ID...]]

  WORKFLOW          a workflow, a .zip bundle, or a file this application opens as one
  run FILE          no window: compute FILE's results and write each as Save Result would;
                    with no Result node, run its Write Files nodes instead
    --out DIR       where (default: the current folder; made if missing); also where the
                    Write Files nodes write when given
    --node N[.P]    this output instead of every Result node's (repeatable)
    --format EXT    a format's extension (default: the engine's first)
    --sheet         lists as one contact sheet instead of numbered files
  check FILE        no window: say what is wrong with FILE (types, wires, unwired inputs,
                    nodes nothing uses) without running it; exit 1 when anything is
  tidy FILE         no window: lay FILE out as Edit › Tidy does, and save it
  nodes             no window: list the engine's node types
    --engine        default for run, check and tidy: the engines FILE names in its engines list
  -V, --version     show the version and what this build is made of
  -h, --help        show this help
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_names_the_application_then_what_it_is_made_of() {
        let parts = [("renderer".to_owned(), "abc".to_owned()), ("kernels".to_owned(), "1".to_owned())];
        assert_eq!(version_text("Thing", "1.2", &parts), "Thing 1.2\nrenderer abc\nkernels 1");
        assert_eq!(version_text("Thing", "1.2", &[]), "Thing 1.2");
        assert!(usage("thing").starts_with("usage: thing "), "{}", usage("thing"));
        assert_eq!(about("Thing", "1.2", &parts).parts.len(), 2);
    }

    #[test]
    fn the_app_icon_decodes_square() {
        let icon = eframe::icon_data::from_png_bytes(APP_ICON).unwrap();
        assert_eq!((icon.width, icon.height), (1024, 1024));
        assert_eq!(icon.rgba.len(), 1024 * 1024 * 4);
    }

    #[test]
    fn the_demo_opens_with_every_node_placed() {
        let (g, name) = read_workflow(None, EngineKind::Text).unwrap();
        assert_eq!(name, "Demo");
        assert_eq!(g.nodes.len(), 7);
        let mut at: Vec<[i64; 2]> = g.nodes.values().map(|n| [n.pos[0] as i64, n.pos[1] as i64]).collect();
        at.sort_unstable();
        at.dedup();
        assert_eq!(at.len(), 7, "no two nodes stacked");
        assert!(read_workflow(Some(Path::new("/no/such/workflow.json")), EngineKind::Text).is_err());
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn flag_values_are_not_the_workflow() {
        let args = strings(&["--control", "0", "--control-token-file", "/tmp/t", "wf.json", "--screenshot", "x.png"]);
        assert_eq!(workflow_arg(&args), Some(PathBuf::from("wf.json")));
        assert_eq!(workflow_arg(&strings(&["--control", "9000"])), None);
        assert_eq!(flag_value(&args, "--screenshot").map(String::as_str), Some("x.png"));
    }

    #[test]
    fn control_flags_parse() {
        assert!(control_args(&strings(&["--process"])).unwrap().is_none());
        let (port, token, file) = control_args(&strings(&["--control", "0"])).unwrap().unwrap();
        assert_eq!((port, token.len(), file), (0, 64, None));
        assert!(control_args(&strings(&["--control"])).is_err());
        assert!(control_args(&strings(&["--control", "70000"])).is_err());
        let dir = plumcraft_scratch::root().join("editor-token");
        let path = dir.join("control.token");
        let _ = std::fs::remove_dir_all(&dir);
        let args = strings(&["--control", "0", "--control-token-file", path.to_str().unwrap()]);
        let (_, a, f) = control_args(&args).unwrap().unwrap();
        let (_, b, _) = control_args(&args).unwrap().unwrap();
        assert_eq!((a.as_str(), f.as_deref()), (b.as_str(), Some(path.as_path())));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_engine_is_chosen_by_flag() {
        assert_eq!(choose(&[]), Ok((EngineKind::Text, Mode::InProc)));
        assert_eq!(choose(&strings(&["--engine", "audio"])), Ok((EngineKind::Audio, Mode::Rust)));
        assert!(choose(&strings(&["--engine", "audio", "--inproc"])).unwrap_err().contains("isn't compiled in"));
        assert_eq!(choose(&strings(&["--engine", "audio", "--process"])), Ok((EngineKind::Audio, Mode::Rust)));
        assert_eq!(choose(&strings(&["--engine", "audio", "--http"])), Ok((EngineKind::Audio, Mode::HttpRust)));
        assert_eq!(choose(&strings(&["--engine", "text", "--python"])), Ok((EngineKind::Text, Mode::Python)));
        assert!(choose(&strings(&["--engine", "audio", "--python"])).unwrap_err().contains("no Python audio engine"));
        // Several: the first is the engine, the others are added beside it.
        assert_eq!(choose(&strings(&["--engine", "text,array"])), Ok((EngineKind::Text, Mode::InProc)));
        assert_eq!(EngineKind::all_from_args(&strings(&["--engine", "text, array"])), Ok(vec![EngineKind::Text, EngineKind::Array]));
        assert!(EngineKind::all_from_args(&strings(&["--engine", "text,text"])).unwrap_err().contains("twice"));
        assert!(EngineKind::all_from_args(&strings(&["--engine", "text,nope"])).unwrap_err().contains("\"nope\""));
        // The several-engine demos: by the set of engines, in any order; each parses.
        assert_eq!(multi_demo(&[EngineKind::Array, EngineKind::Craft("vectorcraft")]).map(|d| d.1), Some("Trace and measure"));
        assert_eq!(multi_demo(&[EngineKind::Text, EngineKind::Audio, EngineKind::Array]).map(|d| d.1), Some("What a chord looks like"));
        assert_eq!(multi_demo(&[EngineKind::Text, EngineKind::Array, EngineKind::Craft("gridcraft")]).map(|d| d.1), Some("Analytics in GridCraft"));
        assert!(multi_demo(&[EngineKind::Text, EngineKind::Array]).is_none());
        for (_, w, n) in MULTI_DEMOS {
            assert!(plumcraft_session::file_cmds::parse_workflow(w).is_ok(), "{n}");
        }
        assert!(choose(&strings(&["--engine", "audio", "--http", "--python"])).is_err());
        assert!(choose(&strings(&["--engine", "video"])).unwrap_err().contains("video"));
        // VectorCraft runs as its own process unless asked (D49)
        assert_eq!(choose(&strings(&["--engine", "vectorcraft"])), Ok((EngineKind::Craft("vectorcraft"), Mode::Rust)));
        assert_eq!(choose(&strings(&["--engine", "vectorcraft", "--inproc"])), Ok((EngineKind::Craft("vectorcraft"), Mode::InProc)));
        assert_eq!(choose(&strings(&["--engine", "vectorcraft", "--http"])), Ok((EngineKind::Craft("vectorcraft"), Mode::HttpRust)));
        assert!(choose(&strings(&["--engine", "vectorcraft", "--python"])).unwrap_err().contains("no Python vectorcraft engine"));
        assert_eq!(choose(&strings(&["--inproc"])), Ok((EngineKind::Text, Mode::InProc)));
        assert!(choose(&strings(&["--engine"])).is_err());
        // the engine's name is not the workflow
        assert_eq!(workflow_arg(&strings(&["--engine", "audio"])), None);
        assert_eq!(workflow_arg(&strings(&["--engine", "audio", "wf.json"])), Some(PathBuf::from("wf.json")));
        let ctx = egui::Context::default();
        assert!(connect_with(EngineKind::Audio, Mode::Python, Some(&ctx), None).is_err());
    }

    #[test]
    fn a_headless_run_takes_the_files_engines_beside_a_manifest() {
        let dir = plumcraft_scratch::dir("editor-file-engines");
        let wf = dir.join("wf.json");
        std::fs::write(&wf, r#"{"version": 1, "engines": ["duckdb", "gridcraft"], "nodes": {}, "wires": []}"#).unwrap();
        let wf = wf.display().to_string();
        let engines = |args: &[&str]| {
            let a = with_file_engines(strings(args));
            a.windows(2).find(|w| w[0] == "--engine").map(|w| w[1].clone())
        };
        assert_eq!(engines(&["run", &wf]), Some("duckdb,gridcraft".into()));
        // a manifest says how to start one of them: the file still names them all, and the
        // manifest's path before the file isn't taken for it
        assert_eq!(engines(&["run", "--engine-manifest", "duckdb.toml", &wf]), Some("duckdb,gridcraft".into()));
        assert_eq!(engines(&["check", &wf, "--engine-manifest", "duckdb.toml"]), Some("duckdb,gridcraft".into()));
        // --engine wins, and a window's arguments are left alone
        assert_eq!(engines(&["run", &wf, "--engine", "text"]), Some("text".into()));
        assert_eq!(engines(&[&wf]), None);
    }

    #[test]
    fn the_audio_demo_opens_with_every_node_placed() {
        let (g, name) = read_workflow(None, EngineKind::Audio).unwrap();
        assert_eq!(name, "Demo");
        assert_eq!(g.nodes.len(), 7);
        assert!(g.nodes.values().all(|n| n.kind.starts_with("audio.")));
        let mut at: Vec<[i64; 2]> = g.nodes.values().map(|n| [n.pos[0] as i64, n.pos[1] as i64]).collect();
        at.sort_unstable();
        at.dedup();
        assert_eq!(at.len(), 7, "no two nodes stacked");
    }

    #[test]
    fn the_vectorcraft_demo_opens_placed_and_its_app_is_offered() {
        let (g, _) = read_workflow(None, EngineKind::Craft("vectorcraft")).unwrap();
        assert_eq!(g.nodes.len(), 5);
        assert!(g.nodes.values().all(|n| n.kind.starts_with("vc.")));
        let apps = doc_apps();
        // one per built-in pack with a [gui], VectorCraft's among them
        let with_gui =
            plumcraft_craft::manifest::BUILTIN.iter().filter_map(|id| plumcraft_craft::manifest::builtin(id)?.ok()).filter(|m| m.gui.is_some()).count();
        assert_eq!(apps.len(), with_gui);
        let vc = apps.iter().find(|a| a.app == "vectorcraft").expect("VectorCraft's [gui]");
        assert_eq!((vc.wire.as_str(), vc.label.as_str(), vc.ext.as_str()), ("vectorcraft.doc", "VectorCraft", "vectorcraft"));
        // PlumCraft shows PdfCraft's documents itself (pages); every other craft's pop out in its app
        let own: Vec<&str> = apps.iter().filter(|a| a.viewer).map(|a| a.app.as_str()).collect();
        assert_eq!(own, ["pdfcraft"]);
    }

    #[test]
    fn the_engine_menu_offers_every_valid_engine_and_hosting() {
        let c = choices();
        let ids: Vec<String> = c.iter().map(|c| format!("{}/{}", c.engine, c.mode)).collect();
        let mut want: Vec<String> = [
            "text/inproc",
            "text/process",
            "text/python",
            "text/http",
            "text/http-python",
            "audio/process",
            "audio/http",
            "array/inproc",
            "array/process",
            "array/http",
        ]
        .map(String::from)
        .to_vec();
        for id in plumcraft_craft::manifest::BUILTIN {
            want.extend(["inproc", "process", "http"].map(|m| format!("{id}/{m}")));
        }
        assert_eq!(ids, want);
        assert!(ids.contains(&"vectorcraft/process".to_owned()));
        for m in Mode::ALL {
            assert_eq!(Mode::from_id(m.id()), Some(m));
        }
        for k in EngineKind::all() {
            assert_eq!(EngineKind::from_id(&k.id()), Some(k));
        }
        let ctx = egui::Context::default();
        let mut connect = Launcher::default().connector();
        assert!(connect("video", "inproc", Some(&ctx)).err().unwrap().contains("video"));
        assert!(connect("text", "carrier-pigeon", Some(&ctx)).err().unwrap().contains("carrier-pigeon"));
        assert!(connect("audio", "python", Some(&ctx)).err().unwrap().contains("no Python audio engine"));
    }

    pub(crate) mod engine_kittest {
        use std::path::PathBuf;
        use std::sync::OnceLock;
        use std::time::{Duration, Instant};

        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        use plumcraft_proto::event::NodeState;
        use serde_json::{Value, json};

        use super::super::*;

        /// The engine binaries the Rust process and HTTP modes start, built once if this test
        /// binary was built without them (`cargo test -p plumcraft-editor`).
        pub(crate) fn engine_binaries() {
            static BUILT: OnceLock<()> = OnceLock::new();
            BUILT.get_or_init(|| {
                if ["plumcraft-text-engine", "plumcraft-audio-engine", "plumcraft-craft-engine"].iter().all(|b| engine_binary(b).is_ok_and(|p| p.is_file())) {
                    return;
                }
                // <target>/<profile>/deps/<this test>
                let exe = std::env::current_exe().unwrap();
                let target = exe.parent().and_then(Path::parent).and_then(Path::parent).unwrap();
                let release = exe.parent().and_then(Path::parent).is_some_and(|p| p.ends_with("release"));
                let mut cmd = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
                cmd.args(["build", "-p", "plumcraft-text", "-p", "plumcraft-audio", "-p", "plumcraft-craft", "--bins", "--target-dir"]).arg(target);
                if release {
                    cmd.arg("--release");
                }
                let status = cmd.status().unwrap();
                assert!(status.success(), "building the engine binaries: {status}");
            });
        }

        fn harness(app: EditorApp, size: egui::Vec2) -> Harness<'static, EditorApp> {
            let mut h = Harness::builder().with_size(size).build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
            h.state().setup_context(&h.ctx.clone());
            h.run_steps(6);
            h
        }

        /// The editor on engine `kind`'s demo, hosted in-process, starting engines with `launcher`.
        fn text_editor(launcher: Launcher) -> Harness<'static, EditorApp> {
            engine_binaries();
            let (graph, name) = read_workflow(None, EngineKind::Text).unwrap();
            let ctx = egui::Context::default();
            let app = editor_with(&ctx, graph, None, name, EngineKind::Text, Mode::InProc, None, launcher);
            harness(app, egui::vec2(1500.0, 900.0))
        }

        /// Step until `done` (or 60 s: the Python engine syncs its environment on first use).
        fn until(h: &mut Harness<'static, EditorApp>, what: &str, done: impl Fn(&EditorApp) -> bool) {
            let start = Instant::now();
            while !done(h.state()) {
                assert!(start.elapsed() < Duration::from_secs(60), "timed out waiting for {what}: {:?}", h.state().last_error);
                std::thread::sleep(Duration::from_millis(10));
                h.step();
            }
            h.run_steps(3);
        }

        /// Run the graph, wait for its reply, and check every node finished as on the in-process
        /// engine: the demo's `broken` node fails on purpose, and `after` is skipped; the rest
        /// are done, or cached (a slow node, `think`, is persisted in the store, spec 14.5, so
        /// an engine with the same salt answers it from there).
        fn run_all_done(h: &mut Harness<'static, EditorApp>, what: &str) {
            assert!(h.state_mut().start_run_if_enabled().is_some(), "{what}: run.start is enabled: {:?}", h.state().last_error);
            until(h, what, |a| !a.session.run.running && a.session.run.outbox_is_empty() && a.session.run.last_reply.is_some());
            let g = h.state().session.graph();
            let run = &h.state().session.run;
            for id in g.nodes.keys() {
                let want = match id.0.as_str() {
                    "broken" => NodeState::Error,
                    "after" => NodeState::Skipped,
                    _ => NodeState::Done,
                };
                let node = run.node(&id.0).unwrap_or_else(|| panic!("{what}: {id}: no result"));
                let ok = node.state == Some(want) || (want == NodeState::Done && node.state == Some(NodeState::Cached));
                assert!(ok, "{what}: {id}: {:?} {:?}", node.state, node.error);
            }
            assert!(run.nodes.values().any(|n| n.thumb.is_some()), "{what}: results arrived");
        }

        fn switch(h: &mut Harness<'static, EditorApp>, engine: &str, mode: &str) -> Result<Value, String> {
            let ctx = h.ctx.clone();
            let r = h.state_mut().control_now(&ctx, "command.run", &json!({"command": "engine.switch", "params": {"engine": engine, "mode": mode}}));
            h.run_steps(3);
            if r["ok"] == true { Ok(r["result"].clone()) } else { Err(r["error"].as_str().unwrap_or_default().to_owned()) }
        }

        fn inspect(h: &mut Harness<'static, EditorApp>) -> Value {
            let ctx = h.ctx.clone();
            h.state_mut().control_now(&ctx, "ui.inspect", &Value::Null)["result"].clone()
        }

        #[test]
        fn switching_text_engines_runs_after_every_switch() {
            let mut h = text_editor(Launcher::default());
            h.get_by_label_contains("Rust · mode 1 · in-process");
            assert!(h.state().window_title().ends_with("(Text · Rust · in-process (mode 1))"), "{}", h.state().window_title());
            run_all_done(&mut h, "in-process");
            let nodes = h.state().session.graph().nodes.len();
            for (mode, label) in [("python", "Python · mode 2 · process"), ("http", "Rust · mode 3 · HTTP, remote"), ("inproc", "Rust · mode 1 · in-process")]
            {
                let r = switch(&mut h, "text", mode).unwrap_or_else(|e| panic!("{mode}: {e}"));
                assert_eq!((r["engine"].as_str(), r["mode"].as_str(), r["connected"].as_bool()), (Some("text"), Some(mode), Some(true)), "{r}");
                assert_eq!(r["problems"], json!([]), "{r}");
                // the old engine's results are gone, the graph stays
                assert!(h.state().session.run.nodes.is_empty() && h.state().session.run.last_reply.is_none(), "{mode}");
                assert_eq!(h.state().session.graph().nodes.len(), nodes);
                h.get_by_label_contains(label);
                assert!(h.state().window_title().contains(&format!("Text · {}", Mode::from_id(mode).unwrap().menu_label())), "{}", h.state().window_title());
                let v = inspect(&mut h);
                assert_eq!((v["engine"]["engine"].as_str(), v["engine"]["mode"].as_str()), (Some("text"), Some(mode)), "{v}");
                assert_eq!(v["engine"]["choices"].as_array().map(Vec::len), Some(10 + 3 * plumcraft_craft::manifest::BUILTIN.len()));
                assert!(v["engine"]["label"].as_str().unwrap().contains(label), "{v}");
                run_all_done(&mut h, mode);
            }
            // Reconnect starts it again, results arriving after
            let ctx = h.ctx.clone();
            let r = h.state_mut().control_now(&ctx, "command.run", &json!({"command": "engine.reconnect"}));
            assert_eq!(r["ok"], true, "{r}");
            run_all_done(&mut h, "reconnected");
        }

        fn command(h: &mut Harness<'static, EditorApp>, id: &str, params: Value) -> Result<Value, String> {
            let ctx = h.ctx.clone();
            let r = h.state_mut().control_now(&ctx, "command.run", &json!({"command": id, "params": params}));
            h.run_steps(3);
            if r["ok"] == true { Ok(r["result"].clone()) } else { Err(r["error"].as_str().unwrap_or_default().to_owned()) }
        }

        /// Engine › Pick Engines Automatically: a graph that names an engine not connected starts
        /// it on open, in-process (mode 1, the fastest), beside the text engine.
        #[test]
        fn picking_engines_automatically_starts_what_an_opened_graph_needs() {
            let mut h = text_editor(Launcher::default());
            assert_eq!(command(&mut h, "engine.auto", json!({"on": true})).unwrap()["on"], true);
            let g = json!({"version": 1, "engines": ["text", "array"], "nodes": {
                "lit": {"kind": "text.literal", "values": {"text": "corgi"}, "pos": [0, 0]},
                "twice": {"kind": "array.arithmetic", "pos": [300, 0]}}});
            let path = plumcraft_scratch::root().join("needs-array.json");
            std::fs::write(&path, g.to_string()).unwrap();
            let opened = command(&mut h, "graph.open", json!({"path": path, "discard": true})).unwrap();
            // (what comes back is the engine pump's: the array engine, added and connected)
            assert_eq!((opened["added"].clone(), opened["connected"].clone()), (json!({"engine": "array", "mode": "inproc"}), json!(true)), "{opened}");
            h.run_steps(4);
            assert!(h.state().session.catalog().node("array.arithmetic").is_some(), "the array engine is connected");
            assert_eq!(
                h.state().session.engines.added,
                [plumcraft_session::EngineTarget::Choice { engine: "array".into(), mode: "inproc".into() }],
                "hosted in-process"
            );
            assert!(h.state().session.missing_engines().is_none());
        }

        /// Engine › Add Engine (docs/research/15-multi-engine.md): the array engine beside the
        /// text engine, behind a coordinator; one merged catalogue; the text demo still runs; a
        /// graph across both (Text → Code points → Arithmetic) runs, each node on its engine;
        /// Remove goes back to the text engine's nodes, reporting the array node.
        #[test]
        fn adding_the_array_engine_runs_a_graph_across_both() {
            let mut h = text_editor(Launcher::default());
            run_all_done(&mut h, "text alone");
            let r = command(&mut h, "engine.add", json!({"engine": "array", "mode": "inproc"})).unwrap();
            assert_eq!(r["engines"], json!(["text", "array"]), "{r}");
            assert_eq!(r["connected"], true, "{r}");
            let cat = h.state().session.catalog().clone();
            assert!(cat.node("text.upper").is_some() && cat.node("array.arithmetic").is_some());
            assert_eq!(cat.node("array.arithmetic").map(|n| n.engines.clone()), Some(vec!["array".to_owned()]));
            assert!(h.state().window_title().contains("+ Array"), "{}", h.state().window_title());
            let v = inspect(&mut h);
            assert_eq!(
                v["engine"]["engines"].as_array().map(|e| e.iter().map(|x| x["id"].clone()).collect::<Vec<_>>()),
                Some(vec![json!("text"), json!("array")]),
                "{v}"
            );
            assert!(command(&mut h, "engine.add", json!({"engine": "array", "mode": "inproc"})).unwrap_err().contains("already connected"));
            run_all_done(&mut h, "the text demo through the coordinator");

            // A graph across both engines.
            let codes = cat.node("text.codes").unwrap().outputs[0].name.clone();
            let g = json!({"version": 1, "nodes": {
                "lit": {"kind": "text.literal", "values": {"text": "corgi"}, "pos": [0, 0]},
                "codes": {"kind": "text.codes", "pos": [200, 0]},
                "twice": {"kind": "array.arithmetic", "values": {"operation": "multiply", "constant": 2.0}, "pos": [400, 0]}},
                "wires": [{"from": {"node": "lit", "port": "text"}, "to": {"node": "codes", "port": "text"}},
                          {"from": {"node": "codes", "port": codes}, "to": {"node": "twice", "port": "a"}}]});
            let path = plumcraft_scratch::root().join("across.json");
            std::fs::write(&path, g.to_string()).unwrap();
            let opened = command(&mut h, "graph.open", json!({"path": path, "discard": true})).unwrap();
            assert_eq!(opened["problemCount"], 0, "{opened}");
            assert!(h.state_mut().start_run_if_enabled().is_some());
            until(&mut h, "the run across engines", |a| !a.session.run.running && a.session.run.outbox_is_empty() && a.session.run.last_reply.is_some());
            let reply = h.state().session.run.last_reply.clone().unwrap();
            assert!(reply.ok, "{reply:?}");
            for n in ["lit", "codes", "twice"] {
                let st = h.state().session.run.node(n).and_then(|x| x.state);
                assert!(matches!(st, Some(NodeState::Done | NodeState::Cached)), "{n}: {st:?}");
            }

            // Each node shows the engine that runs it.
            assert!(!h.query_all_by_label("array").collect::<Vec<_>>().is_empty(), "the Arithmetic node's badge");
            // A kind both engines offer is pinned when placed (to the first), and can be moved.
            let zip = command(&mut h, "node.add", json!({"kind": "seq.zip", "pos": [0, 300]})).unwrap()["id"].as_str().unwrap().to_owned();
            let pin = |h: &Harness<'static, EditorApp>| h.state().session.graph().nodes.iter().find(|(k, _)| k.0 == zip).and_then(|(_, n)| n.engine.clone());
            assert_eq!(pin(&h).as_deref(), Some("text"));
            assert_eq!(h.state().session.graph().engines, ["text", "array"]);
            command(&mut h, "node.engine", json!({"id": zip, "engine": "array"})).unwrap();
            assert_eq!(pin(&h).as_deref(), Some("array"));
            h.run_steps(3);
            h.get_by_label("array •");
            command(&mut h, "node.remove", json!({"nodes": [zip]})).unwrap();

            // Remove: back to the text engine's nodes; the array node is a problem, kept.
            let r = command(&mut h, "engine.remove", json!({"engine": "array"})).unwrap();
            assert_eq!(r["engines"], json!(["text"]), "{r}");
            assert!(r["problems"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().contains("array.arithmetic")), "{r}");
            assert!(h.state().session.catalog().node("array.arithmetic").is_none());
            assert_eq!(h.state().session.graph().nodes.len(), 3, "nothing deleted");
            assert!(command(&mut h, "engine.remove", json!({"engine": "array"})).is_err());
            // A switch goes back to one engine, with no coordinator.
            switch(&mut h, "text", "inproc").unwrap();
            assert!(h.state().coordinator.is_none() && h.state().session.engines.added.is_empty());
        }

        /// The Engine menu's picks (`engine.use`): another engine is added, a connected one
        /// changes its connection and keeps the others, and Clear Engines then a pick is a
        /// complete change.
        #[test]
        fn a_pick_adds_or_changes_an_engine_and_clear_starts_over() {
            let mut h = text_editor(Launcher::default());
            run_all_done(&mut h, "text alone");
            let r = command(&mut h, "engine.use", json!({"engine": "array", "mode": "inproc"})).unwrap();
            assert_eq!(r["how"], "add", "{r}");
            assert_eq!(h.state().session.engines.engine_ids(), ["text", "array"]);
            // the first engine's connection changes; the array engine comes back beside it
            assert_eq!(command(&mut h, "engine.use", json!({"engine": "text", "mode": "process"})).unwrap()["how"], "change");
            h.run_steps(3);
            assert!(h.state().session.engines.is_current("text", "process"), "{:?}", h.state().last_error);
            assert_eq!(h.state().session.engines.engine_ids(), ["text", "array"]);
            assert!(h.state().session.connected && h.state().session.catalog().node("array.arithmetic").is_some());
            assert_eq!(command(&mut h, "engine.use", json!({"engine": "array", "mode": "inproc"})).unwrap()["how"], "unchanged");
            run_all_done(&mut h, "text by process beside array");

            // Clear: no engine, the graph kept, nothing to reconnect
            let nodes = h.state().session.graph().nodes.len();
            command(&mut h, "engine.clear", json!({})).unwrap();
            assert!(!h.state().session.connected && h.state().link.is_none() && h.state().coordinator.is_none());
            assert!(h.state().session.engines.engine_ids().is_empty());
            assert_eq!(h.state().session.graph().nodes.len(), nodes);
            assert!(!h.state().enabled("engine.reconnect") && !h.state().enabled("engine.clear") && !h.state().enabled("run.start"));

            // then a pick from the menu, as a person would: the one engine there is
            h.get_by_label("Engine").click();
            h.run_steps(3);
            assert!(h.query_by_label("Add Engine ⏵").is_none(), "every pick adds; no Add Engine");
            h.get_by_label("Text ⏵").hover();
            h.run_steps(3);
            h.get_by_role_and_label(egui::accesskit::Role::RadioButton, "Rust · in-process (mode 1)").click();
            h.run_steps(3);
            assert!(h.state().session.connected, "{:?}", h.state().last_error);
            assert!(h.state().session.engines.is_current("text", "inproc"));
            assert!(h.state().session.engines.added.is_empty());
            run_all_done(&mut h, "after a clear and a pick");
        }

        #[test]
        fn a_switch_cancels_the_run_in_flight() {
            let mut h = text_editor(Launcher::default());
            assert!(h.state_mut().start_run_if_enabled().is_some());
            let ctx = h.ctx.clone();
            h.state_mut().pump_runs(&ctx);
            switch(&mut h, "text", "process").unwrap();
            assert!(!h.state().session.run.running, "the old run is cancelled");
            h.get_by_label_contains("Rust · mode 2 · process");
            run_all_done(&mut h, "after the cancelled run");
        }

        /// A manifest for the Python text engine: an engine this build doesn't know.
        fn python_manifest(path: &Path, id: &str) {
            let (command, _) = plumcraft_text::python_command();
            let words: Vec<String> = command.iter().map(|c| format!("{c:?}")).collect();
            std::fs::write(path, format!("id = {id:?}\nname = \"Text from a file\"\ncommand = [{}]\n", words.join(", "))).unwrap();
        }

        fn offered(h: &mut Harness<'static, EditorApp>, engine: &str) -> Vec<(String, String)> {
            let list = command(h, "engine.list", json!({})).unwrap();
            let choices = list["choices"].as_array().unwrap();
            choices
                .iter()
                .filter(|c| c["engine"] == engine)
                .map(|c| (c["mode"].as_str().unwrap().to_owned(), c["modeLabel"].as_str().unwrap().to_owned()))
                .collect()
        }

        #[test]
        fn a_manifest_engine_is_offered_switched_to_and_run_both_ways() {
            let m = plumcraft_scratch::dir("editor-manifest").join("pytext.toml");
            python_manifest(&m, "pytext");
            assert_eq!(load_manifests(std::slice::from_ref(&m), None), Vec::<String>::new());
            let kind = EngineKind::from_id("pytext").unwrap();
            assert_eq!((kind.default_mode(), kind.label().as_str()), (Mode::Rust, "Text from a file"));
            assert!(kind.hosts(Mode::Rust) && kind.hosts(Mode::HttpRust) && !kind.hosts(Mode::InProc) && !kind.hosts(Mode::Python));
            let mut h = text_editor(Launcher { manifests: vec![m], ..Default::default() });
            let choices = offered(&mut h, "pytext");
            assert_eq!(choices, [("process".to_owned(), "Process (mode 2)".to_owned()), ("http".to_owned(), "HTTP (mode 3)".to_owned())]);
            switch(&mut h, "pytext", "process").unwrap();
            h.get_by_label_contains("mode 2 · process");
            run_all_done(&mut h, "from a manifest, on a socket");
            switch(&mut h, "pytext", "http").unwrap();
            run_all_done(&mut h, "from a manifest, over HTTP");
        }

        /// A generated engine beside a built-in one: a graph runs across the Rust text engine and
        /// the Python engine and back, TEXT crossing each way, as a core type does.
        #[test]
        fn a_generated_engine_runs_beside_a_built_in_one() {
            let dest = plumcraft_scratch::dir("editor-beside").join("river-engine");
            let python = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python").canonicalize().unwrap();
            let uv = std::env::var("PLUMCRAFT_UV").unwrap_or_else(|_| "uv".into());
            let new = std::process::Command::new(uv)
                .args(["run", "--project"])
                .arg(python)
                .args(["plumcraft-engine", "new"])
                .arg(&dest)
                .args(["--types", "text"])
                .output()
                .unwrap();
            assert!(new.status.success(), "{}", String::from_utf8_lossy(&new.stderr));
            let m = dest.join("plumcraft-engine.toml");
            assert_eq!(load_manifests(std::slice::from_ref(&m), None), Vec::<String>::new());
            let mut h = text_editor(Launcher { manifests: vec![m], ..Default::default() });
            let r = command(&mut h, "engine.add", json!({"engine": "river", "mode": "process"})).unwrap();
            assert_eq!(r["engines"], json!(["text", "river"]), "{r}");
            let g = json!({"version": 1, "nodes": {
                "lit": {"kind": "text.literal", "values": {"text": "a corgi"}, "pos": [0, 0]},
                "loud": {"kind": "river.shout", "values": {"marks": 1}, "pos": [200, 0]},
                "rev": {"kind": "text.reverse", "pos": [400, 0]}},
                "wires": [{"from": {"node": "lit", "port": "text"}, "to": {"node": "loud", "port": "text"}},
                          {"from": {"node": "loud", "port": "text"}, "to": {"node": "rev", "port": "text"}}]});
            let path = plumcraft_scratch::root().join("beside.json");
            std::fs::write(&path, g.to_string()).unwrap();
            let opened = command(&mut h, "graph.open", json!({"path": path, "discard": true})).unwrap();
            assert_eq!(opened["problemCount"], 0, "{opened}");
            command(&mut h, "run.watch", json!({"node": "rev", "port": "text"})).unwrap();
            assert!(h.state_mut().start_run_if_enabled().is_some());
            until(&mut h, "the run across engines", |a| !a.session.run.running && a.session.run.outbox_is_empty() && a.session.run.last_reply.is_some());
            let reply = h.state().session.run.last_reply.clone().unwrap();
            assert!(reply.ok, "{reply:?}");
            let value = serde_json::to_string(&reply.watch).unwrap();
            assert!(value.contains("!IGROC A"), "out to Python and back: {value}");
        }

        #[test]
        fn reload_offers_a_manifest_installed_while_running() {
            let dir = plumcraft_scratch::dir("editor-reload");
            let m = dir.join("late.toml");
            let _ = std::fs::remove_file(&m);
            assert!(load_manifests(std::slice::from_ref(&m), None)[0].contains("late.toml"));
            let mut h = text_editor(Launcher { manifests: vec![m.clone()], ..Default::default() });
            assert_eq!(offered(&mut h, "late"), []);
            python_manifest(&m, "late");
            let r = command(&mut h, "engine.manifests.reload", json!({})).unwrap();
            assert_eq!(r["problems"], json!([]), "{r}");
            assert_eq!(offered(&mut h, "late").len(), 2);
            // A manifest that breaks says why in the status bar, and takes only itself away.
            std::fs::write(&m, "id = \"Not An Id\"").unwrap();
            let r = command(&mut h, "engine.manifests.reload", json!({})).unwrap();
            assert!(r["problems"][0].as_str().unwrap().contains("lower-case"), "{r}");
            assert!(h.state().last_error.as_ref().unwrap().contains("engine manifests"));
            assert_eq!(offered(&mut h, "late"), []);
            assert_eq!(offered(&mut h, "text").len(), 5, "the built-in engines are still there");
        }

        #[test]
        fn a_taken_id_is_refused_naming_both_and_the_rest_load() {
            let dir = plumcraft_scratch::dir("editor-collide");
            let (a, b, c, d) = (dir.join("a.toml"), dir.join("b.toml"), dir.join("c.toml"), dir.join("d.toml"));
            python_manifest(&a, "shared");
            python_manifest(&b, "shared");
            python_manifest(&c, "vectorcraft");
            python_manifest(&d, "other");
            let problems = load_manifests(&[a.clone(), b, c, d, a], None);
            assert_eq!(problems.len(), 2, "the same file twice is one engine: {problems:?}");
            assert!(problems[0].contains("b.toml") && problems[0].contains("a.toml"), "{problems:?}");
            assert!(problems[1].contains("c.toml") && problems[1].contains("built-in"), "{problems:?}");
            assert!(EngineKind::from_id("shared").is_some() && EngineKind::from_id("other").is_some());
        }

        #[test]
        fn engine_manifest_arguments_pick_the_engine() {
            let m = plumcraft_scratch::dir("editor-args").join("named.toml");
            python_manifest(&m, "named");
            let a = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
            let path = m.display().to_string();
            let args = a(&["--engine-manifest", &path, "demo.json"]);
            assert_eq!(manifest_args(&args), std::slice::from_ref(&m));
            assert_eq!(workflow_arg(&args), Some(PathBuf::from("demo.json")));
            load_manifests(std::slice::from_ref(&m), None);
            let (kind, mode) = choose(&with_engine_arg(args)).unwrap();
            assert_eq!((kind.id().as_str(), mode), ("named", Mode::Rust));
            let (kind, _) = choose(&with_engine_arg(a(&["--engine-manifest", &path, "--engine", "text"]))).unwrap();
            assert_eq!(kind, EngineKind::Text, "--engine still chooses");
            let kinds = EngineKind::all_from_args(&a(&["--engine", "text,named"])).unwrap();
            assert_eq!(kinds.len(), 2, "beside another engine, through the coordinator");
        }

        #[test]
        fn text_to_audio_keeps_the_graph_and_reports_its_problems() {
            engine_binaries();
            let mut h = text_editor(Launcher::default());
            let before = h.state().session.graph().clone();
            let r = switch(&mut h, "audio", "process").unwrap();
            assert_eq!(h.state().session.catalog().engine.name, "Audio");
            assert_eq!(h.state().session.graph(), &before, "nothing deleted");
            let problems = r["problems"].as_array().unwrap();
            assert!(problems.len() >= before.nodes.len(), "{r}");
            assert!(problems.iter().any(|p| p.as_str().unwrap().contains("unknown kind \"text.")), "{r}");
            let e = h.state().last_error.clone().unwrap();
            assert!(e.contains("Audio · Rust · process (mode 2)") && e.contains("problem(s)"), "{e}");
            h.get_by_label_contains("engine: Audio");
            h.get_by_label_contains("problem(s) with this engine");
            // back to text: no problems, and it runs
            let r = switch(&mut h, "text", "inproc").unwrap();
            assert_eq!(r["problems"], json!([]));
            assert_eq!(h.state().last_error, None);
            run_all_done(&mut h, "back on text");
        }

        #[test]
        fn a_failed_switch_says_why_and_the_editor_still_works() {
            let bogus = PathBuf::from("/no/such/dir/plumcraft-text-engine");
            let mut h = text_editor(Launcher { binaries: vec![(EngineKind::Text, bogus)], ..Default::default() });
            let e = switch(&mut h, "text", "process").unwrap_err();
            assert!(e.contains("no engine binary at /no/such/dir/plumcraft-text-engine") && e.contains("cargo build -p plumcraft-text"), "{e}");
            assert!(!h.state().session.connected && h.state().link.is_none());
            h.get_by_label_contains("engine: Text · Rust · process (mode 2), not connected");
            h.get_by_label_contains("no engine binary");
            assert!(h.state().window_title().ends_with("not connected)"), "{}", h.state().window_title());
            assert!(!h.state().enabled("run.start"));
            // the graph can still be edited, on the catalogue it had
            assert!(h.state().session.catalog().node("text.upper").is_some());
            h.state_mut().run("node.add", &json!({"kind": "text.upper", "pos": [0, 600]})).unwrap();
            h.state_mut().run("edit.undo", &Value::Null).unwrap();
            // a bogus switch is refused with the choices
            assert!(switch(&mut h, "text", "telegraph").unwrap_err().contains("text/inproc"));
            // the Engine menu still works: pick in-process from it, as a person would
            h.get_by_label("Engine").click();
            h.run_steps(3);
            // the menu's own commands come first, then the engines, each a submenu of its
            // connections; the failed one isn't in use, so it names no connection
            let reload = h.get_by_label("Reload Engine List").rect().top();
            let text = h.get_by_label("Text ⏵");
            assert!(reload < text.rect().top(), "the fixed commands are above the engines");
            assert!(h.query_by_label("Rust · in-process (mode 1)").is_none(), "connections wait in submenus");
            text.hover();
            h.run_steps(3);
            h.get_by_role_and_label(egui::accesskit::Role::RadioButton, "Rust · in-process (mode 1)").click();
            h.run_steps(3);
            assert!(h.state().session.connected, "{:?}", h.state().last_error);
            assert!(h.state().session.engines.is_current("text", "inproc"));
            run_all_done(&mut h, "after the failed switch");
        }

        #[test]
        fn a_short_window_scrolls_the_engine_list_and_keeps_its_commands() {
            engine_binaries();
            let (graph, name) = read_workflow(None, EngineKind::Text).unwrap();
            let ctx = egui::Context::default();
            let app = editor_with(&ctx, graph, None, name, EngineKind::Text, Mode::InProc, None, Launcher::default());
            let mut h = harness(app, egui::vec2(900.0, 320.0));
            h.get_by_label("Engine").click();
            h.run_steps(3);
            // the commands are all there, at the top
            for fixed in ["Reconnect", "Attach…", "Reload Engine List"] {
                assert!(h.get_by_label(fixed).rect().bottom() < 320.0, "{fixed} shows");
            }
            // every engine is in the list, but the last ones are scrolled out of sight
            let last = h.get_by_label("WordCraft ⏵").rect();
            assert!(last.top() > 320.0, "the list scrolls rather than running on: {last:?}");
        }

        #[test]
        fn attach_connects_to_a_running_http_engine() {
            let mut h = text_editor(Launcher::default());
            let dir = plumcraft_scratch::root().join("editor-attach");
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let port_file = dir.join("port");
            let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
            let mut child = std::process::Command::new(engine_binary("plumcraft-text-engine").unwrap())
                .args(["--lifeline", "--listen", "127.0.0.1:0", "--port-file"])
                .arg(&port_file)
                .env(plumcraft_engine::serve::ENV_TOKEN, token)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let start = Instant::now();
            let addr = loop {
                if let Ok(a) = std::fs::read_to_string(&port_file).map(|a| a.trim().to_owned())
                    && !a.is_empty()
                {
                    break a;
                }
                assert!(start.elapsed() < Duration::from_secs(20), "the engine never listened");
                std::thread::sleep(Duration::from_millis(20));
            };
            let url = format!("http://{addr}");
            // Engine › Attach… opens the window; fill it and press Attach
            h.state_mut().invoke("engine.attach");
            h.run_steps(3);
            h.get_by_label("Attach to an Engine");
            h.state_mut().attach_form = Some(plumcraft_ui::engine_view::AttachForm { url: url.clone(), token: "wrong".into() });
            h.run_steps(2);
            h.get_by_label("Attach").click();
            h.run_steps(3);
            // a wrong token: refused, said, the window stays for another try
            assert!(!h.state().session.connected);
            assert!(h.state().last_error.as_deref().unwrap_or_default().contains("HTTP · attached"), "{:?}", h.state().last_error);
            h.state_mut().attach_form = Some(plumcraft_ui::engine_view::AttachForm { url: url.clone(), token: token.into() });
            h.run_steps(2);
            h.get_by_label("Attach").click();
            h.run_steps(3);
            assert!(h.state().session.connected, "{:?}", h.state().last_error);
            h.get_by_label_contains(&format!("HTTP · attached · {url}"));
            let v = inspect(&mut h);
            assert_eq!((v["engine"]["mode"].as_str(), v["engine"]["url"].as_str()), (Some("attach"), Some(url.as_str())), "{v}");
            assert!(!v.to_string().contains(token), "the token stays out of ui.inspect");
            run_all_done(&mut h, "attached");
            // back to in-process: the attached engine is left running (it isn't ours)
            switch(&mut h, "text", "inproc").unwrap();
            assert!(child.try_wait().unwrap().is_none());
            drop(child.stdin.take());
            let start = Instant::now();
            while child.try_wait().unwrap().is_none() {
                assert!(start.elapsed() < Duration::from_secs(10), "the lifeline stops it");
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = std::fs::remove_dir_all(dir);
        }

        /// An offscreen picture after switching to Python (`claude-debug/editor/engine-menu.png`),
        /// with the Engine menu open. `cargo test -p plumcraft-editor -- --ignored engine_menu_picture`
        #[test]
        #[ignore = "writes a picture for a person to look at"]
        fn engine_menu_picture() {
            engine_binaries();
            let (graph, name) = read_workflow(None, EngineKind::Text).unwrap();
            let ctx = egui::Context::default();
            let app = editor_with(&ctx, graph, None, name, EngineKind::Text, Mode::InProc, None, Launcher::default());
            let mut h = Harness::builder().with_size(egui::vec2(1440.0, 860.0)).wgpu().build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
            h.state().setup_context(&h.ctx.clone());
            h.run_steps(6);
            switch(&mut h, "text", "python").unwrap();
            run_all_done(&mut h, "python");
            h.state_mut().canvas.request_fit();
            h.run_steps(6);
            h.get_by_label("Engine").click();
            h.run_steps(4);
            let img = h.render().unwrap();
            let (w, ht) = img.dimensions();
            let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, ht as usize], img.as_raw());
            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../claude-debug/editor");
            std::fs::create_dir_all(&dir).unwrap();
            plumcraft_ui::save_png(&color, &dir.join("engine-menu.png")).unwrap();
        }
    }

    mod vectorcraft_kittest {
        use std::path::{Path, PathBuf};
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};

        use egui_kittest::Harness;
        use egui_kittest::kittest::{NodeT, Queryable};
        use plumcraft_proto::display::Rendition;
        use plumcraft_proto::event::NodeState;
        use plumcraft_session::DocApps;
        use plumcraft_ui::media::MediaViewer;
        use serde_json::{Value, json};

        use super::super::*;

        type Opened = Arc<Mutex<Vec<(PathBuf, PathBuf)>>>;

        /// The corgi on a floral carpet (`$PLUMCRAFT_CORGI`, else claude-debug's copy).
        fn corgi() -> Option<PathBuf> {
            let p = std::env::var_os("PLUMCRAFT_CORGI")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../claude-debug/craft-mcp/corgi.jpg"));
            p.is_file().then(|| p.canonicalize().unwrap())
        }

        /// VectorCraft and the corgi, or a skip with why.
        fn ready(what: &str) -> Option<PathBuf> {
            let m = plumcraft_craft::manifest::Manifest::named("vectorcraft").unwrap();
            if m.executable().is_none() {
                eprintln!("{what}: skipped, VectorCraft isn't built");
                return None;
            }
            let c = corgi();
            if c.is_none() {
                eprintln!("{what}: skipped, no corgi (set PLUMCRAFT_CORGI)");
            }
            c
        }

        /// The editor as `plumcraft-editor --engine vectorcraft` opens it (mode 2, the demo), the
        /// corgi chosen in Open, and a fake VectorCraft app that only records what it was asked
        /// to open (no test opens the real app's window).
        fn vc_editor(corgi: &Path, wgpu: bool) -> (Harness<'static, EditorApp>, Opened, PathBuf) {
            super::super::tests::engine_kittest::engine_binaries();
            let (graph, name) = read_workflow(None, EngineKind::Craft("vectorcraft")).unwrap();
            let ctx = egui::Context::default();
            let mut app = editor(&ctx, graph, None, name, EngineKind::Craft("vectorcraft"), Mode::Rust, None);
            assert!(app.session.connected, "{:?}", app.last_error);
            let opened: Opened = Arc::default();
            let rec = Arc::clone(&opened);
            let root = plumcraft_scratch::root().join(format!("editor-open-{wgpu}"));
            let _ = std::fs::remove_dir_all(&root);
            let mut apps = doc_apps();
            if let Some(vc) = apps.iter_mut().find(|a| a.app == "vectorcraft") {
                vc.program = Ok(PathBuf::from("/fake/vectorcraft"));
            }
            app.session.docs = DocApps::new(apps).with_root(root.clone()).with_launcher(Arc::new(move |p: &Path, _: &[String], f: &Path| {
                rec.lock().unwrap().push((p.to_path_buf(), f.to_path_buf()));
                Ok(())
            }));
            app.run("node.set", &json!({"node": "open", "port": "file", "value": corgi})).unwrap();
            let b = Harness::builder().with_size(egui::vec2(1500.0, 950.0));
            let b = if wgpu { b.wgpu() } else { b };
            let mut h = b.build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
            h.state().setup_context(&h.ctx.clone());
            h.run_steps(6);
            (h, opened, root)
        }

        fn until(h: &mut Harness<'static, EditorApp>, what: &str, done: impl Fn(&EditorApp) -> bool) {
            let start = Instant::now();
            while !done(h.state()) {
                assert!(start.elapsed() < Duration::from_secs(120), "timed out waiting for {what}: {:?}", h.state().last_error);
                std::thread::sleep(Duration::from_millis(10));
                h.step();
            }
            h.run_steps(3);
        }

        fn idle(app: &EditorApp) -> bool {
            !app.session.run.running && app.session.run.outbox_is_empty()
        }

        fn view(h: &mut Harness<'static, EditorApp>) -> Value {
            let ctx = h.ctx.clone();
            h.state_mut().control_now(&ctx, "ui.inspect", &Value::Null)["result"].clone()
        }

        /// Watch `node.port` and wait for its viewer to say `state`.
        fn watch(h: &mut Harness<'static, EditorApp>, node: &str, port: &str, done: impl Fn(&Value) -> bool) -> Value {
            h.state_mut().run("run.watch", &json!({"node": node, "port": port})).unwrap();
            let start = Instant::now();
            loop {
                let v = view(h);
                if idle(h.state()) && done(&v["watched"]["view"]) {
                    return v;
                }
                assert!(start.elapsed() < Duration::from_secs(60), "{node}.{port}: {v}");
                std::thread::sleep(Duration::from_millis(10));
                h.step();
            }
        }

        #[test]
        fn trace_and_recolor_by_hand_with_previews_and_open_in_vectorcraft() {
            let Some(corgi) = ready("vectorcraft editor") else { return };
            let (mut h, opened, root) = vc_editor(&corgi, false);
            // the palette: the craft's nodes, grouped
            let cat = h.state().session.catalog().clone();
            assert_eq!(cat.engine.name, "VectorCraft");
            // (VectorCraft's own count grows with its builds: 145 on 2026-10-30, and 17 building blocks)
            assert!(cat.nodes.len() >= 162, "{} nodes", cat.nodes.len());
            let path = |id: &str| plumcraft_ui::palette::category_path(cat.node(id).unwrap());
            assert_eq!(path("vc.trace"), "VectorCraft › Trace");
            assert_eq!(path("vary.range2d"), "Building blocks › Vary");
            assert_eq!(path("vc.export_svg"), "VectorCraft › Export");
            assert_eq!(path("vc.run"), "VectorCraft › Advanced");
            assert!(path("vc.fx.blur.gaussian").starts_with("VectorCraft › Effect"), "{}", path("vc.fx.blur.gaussian"));
            h.get_by_label_contains("VectorCraft 0.");
            h.get_by_label_contains("mode 2 · process");

            // the chain runs; every node has a thumbnail
            assert!(h.state_mut().start_run_if_enabled().is_some());
            until(&mut h, "the chain", |a| idle(a) && a.session.run.last_reply.is_some());
            for id in ["open", "trace", "recolor", "svg", "png"] {
                let n = h.state().session.run.node(id).cloned().unwrap();
                assert_eq!(n.state, Some(NodeState::Done), "{id}: {:?}", n.error);
                assert!(matches!(n.thumb, Some(Rendition::Rgba8 { .. })), "{id}: a thumbnail: {:?}", n.thumb);
            }
            // blank widgets stay blank: only the file was set (DragValue used to clamp `colors`)
            let sets: Vec<&plumcraft_session::JournalEntry> = h.state().session.journal().iter().filter(|e| e.id == "node.set").collect();
            assert_eq!(sets.len(), 1, "{sets:?}");
            let trace = h.state().session.graph().node(&plumcraft_graph::NodeId::from("trace")).unwrap().values.clone();
            assert!(trace.get("colors").is_none_or(Value::is_null), "{trace:?}");

            // the SVG view: parsed and drawn by resvg, crisp at the size shown
            let v = watch(&mut h, "svg", "svg", |w| w["state"] == "ready");
            assert_eq!(v["watched"]["media"], "vector");
            let size = v["watched"]["view"]["size"].clone();
            assert!(size[0].as_f64().unwrap() > 800.0, "{v}");
            // the label follows the raster by a frame or two (it raced: 1 run in 6 failed here)
            let start = Instant::now();
            while h.query_by_label_contains("drawn at").is_none() && start.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(10));
                h.step();
            }
            h.get_by_label_contains("drawn at");
            // zoomed in 4×: drawn again, larger
            let before = h.state().media.svg.raster().unwrap();
            h.state_mut().media.svg.zoom.zoom_at(4.0, egui::Vec2::ZERO);
            for _ in 0..3000 {
                if h.state().media.svg.raster().is_some_and(|r| r.0 > before.0 + 200) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
                h.step();
            }
            assert!(h.state().media.svg.raster().is_some_and(|r| r.0 > before.0 + 200), "a sharper SVG: {before:?} {}", h.state().media.svg.inspect());
            assert!(h.state().media.svg.renders >= 2);

            // the image view: the PNG decoded
            let v = watch(&mut h, "png", "image", |w| w["state"] == "ready");
            assert_eq!(v["watched"]["media"], "image");
            assert!(v["watched"]["view"]["size"][0].as_u64().unwrap() > 800, "{v}");

            // the doc view: the rendition, and Open in VectorCraft
            let v = watch(&mut h, "trace", "doc", |w| w["open"]["enabled"] == true);
            assert_eq!(v["watched"]["media"], "doc");
            assert!(!v["watched"]["full"].as_bool().unwrap(), "a document is never read by the UI");
            let button = h.get_by_label("Open in VectorCraft");
            assert!(!button.accesskit_node().is_disabled());
            button.click();
            h.run_steps(3);
            let calls = opened.lock().unwrap().clone();
            assert_eq!(calls.len(), 1, "{:?}", h.state().last_error);
            let (program, copy) = &calls[0];
            assert_eq!(program, Path::new("/fake/vectorcraft"));
            assert!(copy.starts_with(root.join("trace")), "{}", copy.display());
            let doc = h.state().session.run.watched[0].as_ref().unwrap().full.clone().unwrap();
            assert_eq!(&plumcraft_proto::Handle::md5_raw(&std::fs::read(copy).unwrap()), doc.handle.as_ref().unwrap(), "the copy is the value");
            let theirs =
                |l: &plumcraft_proto::data::Locator| matches!(l, plumcraft_proto::data::Locator::Path { path, .. } if Path::new(path) == copy.as_path());
            assert!(!doc.at.iter().any(theirs), "never the store's or the CAS's file: {doc:?}");
            h.get_by_label_contains("opened a copy");

            // from a node's menu, on a node with no value at hand: watched, run (cached), opened
            let ctx = h.ctx.clone();
            let r = h.state_mut().control_now(&ctx, "command.run", &json!({"command": "doc.open", "params": {"node": "recolor"}}));
            assert_eq!(r["result"]["pending"], true, "{r}");
            until(&mut h, "the recolored document to open", |a| a.session.docs.last.is_some());
            assert!(matches!(&h.state().session.docs.last, Some(Ok(o)) if o.node == "recolor"), "{:?}", h.state().session.docs.last);
            assert_eq!(opened.lock().unwrap().len(), 2);
            assert!(h.state().session.run.nodes.values().all(|n| n.state == Some(NodeState::Cached)), "the memo answered the run");
            let v = view(&mut h);
            assert_eq!(v["docs"]["last"]["ok"], true, "{v}");
            let _ = std::fs::remove_dir_all(root);
        }

        #[test]
        fn a_long_trace_cancels_from_the_editor() {
            let Some(corgi) = ready("vectorcraft cancel") else { return };
            let big = corgi.with_file_name("corgi_big.jpg");
            if !big.is_file() {
                eprintln!("vectorcraft cancel: skipped, no corgi_big.jpg");
                return;
            }
            let (mut h, _, root) = vc_editor(&corgi, false);
            h.state_mut().run("node.set", &json!({"node": "open", "port": "file", "value": big})).unwrap();
            h.state_mut().run("node.set", &json!({"node": "trace", "port": "preset", "value": "High Fidelity Photo"})).unwrap();
            let ctx = h.ctx.clone();
            let r = h.state_mut().control_now(&ctx, "command.run", &json!({"command": "run.start", "params": {"memo": "off"}}));
            assert_eq!(r["ok"], true, "{r}");
            until(&mut h, "the trace to start", |a| {
                a.session.run.node("trace").is_some_and(|n| matches!(n.state, Some(NodeState::Started | NodeState::Progress)))
            });
            let t = Instant::now();
            h.state_mut().run("run.cancel", &Value::Null).unwrap();
            h.run_steps(3);
            assert!(!h.state().session.run.running, "cancelled at once");
            assert!(t.elapsed() < Duration::from_secs(1));
            // the engine still works: a fresh run of the small corgi
            h.state_mut().run("node.set", &json!({"node": "open", "port": "file", "value": corgi})).unwrap();
            h.state_mut().run("node.set", &json!({"node": "trace", "port": "preset", "value": "6 Colors"})).unwrap();
            assert!(h.state_mut().start_run_if_enabled().is_some());
            until(&mut h, "the run after the cancel", |a| idle(a) && a.session.run.last_reply.as_ref().is_some_and(|r| r.ok));
            let _ = std::fs::remove_dir_all(root);
        }

        /// Offscreen pictures of the three viewers (`claude-debug/editor/m3-*.png`).
        /// `cargo test -p plumcraft-editor -- --ignored vectorcraft_pictures`
        #[test]
        #[ignore = "writes pictures for a person to look at"]
        fn vectorcraft_pictures() {
            let Some(corgi) = ready("vectorcraft pictures") else { return };
            let (mut h, _, root) = vc_editor(&corgi, true);
            assert!(h.state_mut().start_run_if_enabled().is_some());
            until(&mut h, "the chain", |a| idle(a) && a.session.run.last_reply.is_some());
            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../claude-debug/editor");
            std::fs::create_dir_all(&dir).unwrap();
            for (node, port, file, state) in
                [("svg", "svg", "m3-svg.png", "ready"), ("png", "image", "m3-image.png", "ready"), ("trace", "doc", "m3-doc.png", "")]
            {
                watch(&mut h, node, port, |w| state.is_empty() || w["state"] == state);
                h.state_mut().canvas.request_fit();
                h.run_steps(8);
                let img = h.render().unwrap();
                let (w, ht) = img.dimensions();
                let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, ht as usize], img.as_raw());
                plumcraft_ui::save_png(&color, &dir.join(file)).unwrap();
            }
            let _ = std::fs::remove_dir_all(root);
        }
    }

    mod audio_kittest {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, Instant};

        use egui_kittest::Harness;
        use egui_kittest::kittest::{NodeT, Queryable};
        use plumcraft_proto::display::Rendition;
        use plumcraft_proto::event::NodeState;
        use plumcraft_ui::playback::{NoOutput, NullOutput};
        use serde_json::json;

        use super::super::*;

        /// The editor as `plumcraft-editor --engine audio` opens it (mode 2, the demo), with a
        /// silent output so no test makes a sound.
        fn audio_editor() -> (Harness<'static, EditorApp>, Arc<AtomicUsize>) {
            super::engine_kittest::engine_binaries();
            let (graph, name) = read_workflow(None, EngineKind::Audio).unwrap();
            let ctx = egui::Context::default();
            let mut app = editor(&ctx, graph, None, name, EngineKind::Audio, Mode::Rust, None);
            let starts = Arc::new(AtomicUsize::new(0));
            app.audio.player.set_output(Box::new(NullOutput { rate: 44_100, starts: Arc::clone(&starts) }));
            let mut h = Harness::builder().with_size(egui::vec2(1500.0, 900.0)).build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
            h.state().setup_context(&h.ctx.clone());
            h.run_steps(6);
            (h, starts)
        }

        /// Step until `done` (or 30 s).
        fn until(h: &mut Harness<'static, EditorApp>, what: &str, done: impl Fn(&EditorApp) -> bool) {
            let start = Instant::now();
            while !done(h.state()) {
                assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
                std::thread::sleep(Duration::from_millis(10));
                h.step();
            }
            h.run_steps(3);
        }

        fn idle(app: &EditorApp) -> bool {
            !app.session.run.running && app.session.run.outbox_is_empty()
        }

        #[test]
        fn the_audio_demo_runs_with_waveforms_and_an_audio_view() {
            let (mut h, starts) = audio_editor();
            assert!(h.state().session.connected, "{:?}", h.state().last_error);
            assert_eq!(h.state().session.catalog().engine.name, "Audio");
            h.get_by_label_contains("engine: Audio");
            assert!(h.state_mut().start_run_if_enabled().is_some());
            until(&mut h, "the demo's run", |a| idle(a) && a.session.run.last_reply.is_some());
            for id in ["tone", "fade", "gain", "verb"] {
                let n = h.state().session.run.node(id).cloned().unwrap();
                assert_eq!(n.state, Some(NodeState::Done), "{id}: {:?}", n.error);
                let Some(Rendition::Peaks { data, .. }) = &n.thumb else { panic!("{id}: no peaks thumb: {:?}", n.thumb) };
                assert!(h.state().thumbs.has_peaks(data), "{id}: the waveform is decoded and cached");
                h.get_by_label(&format!("Waveform of {id}"));
            }
            assert!(h.state().thumbs.peaks_cached() >= 4);

            // a click on a waveform watches the node's audio
            h.state_mut().run("run.watch", &json!({"node": "verb"}));
            until(&mut h, "the watched audio and its spectrogram", |a| idle(a) && a.audio.spectrogram().is_some());
            let d = h.state().session.run.watched.first().cloned().flatten().unwrap();
            assert_eq!(d.media, plumcraft_proto::catalogue::Media::Audio);
            h.get_by_label(&d.summary);
            h.get_by_label_contains("bytes from the store");
            h.get_by_label("Watched waveform");
            h.get_by_label("Spectrogram");
            let clip = h.state().audio.clip().cloned().unwrap();
            assert_eq!((clip.rate, clip.channels), (48_000, 2));
            assert!(clip.seconds() >= 2.0, "the reverb's tail: {}", clip.seconds());

            // play (silently), loop, stop
            h.get_by_label("Play").click();
            h.run_steps(3);
            assert!(h.state().audio.player.is_playing());
            assert_eq!(starts.load(Ordering::Relaxed), 1);
            assert_eq!(h.state().audio.player.buffer().map(|b| b.len() / 2), Some((clip.frames as u128 * 44_100 / 48_000) as usize), "resampled to the device");
            let ctx = h.ctx.clone();
            let r = h.state_mut().control_now(&ctx, "ui.audio", &json!({"loop": true}));
            assert_eq!(r["result"]["playback"]["loop"], true, "{r}");
            h.run_steps(2);
            h.get_by_label("Stop").click();
            h.run_steps(3);
            assert!(!h.state().audio.player.is_playing());
            let r = h.state_mut().control_now(&ctx, "ui.inspect", &serde_json::Value::Null);
            assert_eq!(r["result"]["watched"]["audio"]["clip"]["channels"], 2, "{r}");
            assert_eq!(r["result"]["watched"]["audio"]["spectrogram"]["state"], "ready");
            let r = h.state_mut().control_now(&ctx, "ui.audio", &json!({"play": "yes"}));
            assert_eq!(r["ok"], false);
        }

        #[test]
        fn a_1khz_tone_peaks_in_its_spectrogram_bin() {
            let (mut h, _) = audio_editor();
            for (port, value) in [("waveform", json!("sine")), ("frequency", json!(1000.0)), ("channels", json!(1))] {
                h.state_mut().run("node.set", &json!({"node": "tone", "port": port, "value": value})).unwrap();
            }
            h.state_mut().run("run.watch", &json!({"node": "tone"}));
            until(&mut h, "the tone's spectrogram", |a| idle(a) && a.audio.spectrogram().is_some());
            let (spec, tex) = h.state().audio.spectrogram().map(|(s, t)| (s.clone(), t.size())).unwrap();
            assert_eq!((spec.fft, spec.bins, spec.rate), (1024, 513, 48_000));
            assert_eq!(tex, [spec.frames, 513], "a texture a column per frame");
            let expect = 1000.0 / (48_000.0 / 1024.0);
            let peak = spec.peak_bin().unwrap() as f64;
            assert!((peak - expect).abs() <= 1.0, "peak bin {peak}, expected ≈ {expect:.2}");
        }

        #[test]
        fn play_without_an_output_says_so_and_does_not_panic() {
            let (mut h, _) = audio_editor();
            h.state_mut().audio.player.set_output(Box::new(NoOutput));
            h.state_mut().run("run.watch", &json!({"node": "gain"}));
            until(&mut h, "the watched audio", |a| idle(a) && a.audio.clip().is_some());
            h.get_by_label_contains("no audio output");
            assert!(h.get_by_label("Play").accesskit_node().is_disabled(), "Play is disabled");
            h.get_by_label("Play").click();
            h.run_steps(3);
            assert!(!h.state().audio.player.is_playing());
            let ctx = h.ctx.clone();
            let r = h.state_mut().control_now(&ctx, "ui.audio", &json!({"play": true}));
            assert_eq!(r["ok"], false);
            assert!(r["error"].as_str().unwrap().contains("no audio output"), "{r}");
            let r = h.state_mut().control_now(&ctx, "ui.audio", &json!({"play": false}));
            assert_eq!(r["ok"], true, "stopping is always fine: {r}");
        }
    }
}

#[cfg(test)]
mod install_tests {
    use super::*;

    #[test]
    fn the_commands_uv_installed_are_read_from_what_it_said() {
        let said = "Resolved 20 packages in 1.2s\n + duckdb-engine==0.1.0\nInstalled 1 executable: duckdb-engine\n";
        assert_eq!(installed_executables(said), ["duckdb-engine"]);
        assert_eq!(installed_executables("Installed 2 executables: a-engine, b_tool\n"), ["a-engine", "b_tool"]);
        assert!(installed_executables("Installed 1 executable: ../../evil; rm\n").is_empty(), "only plain names");
        assert!(installed_executables("nothing to say\n").is_empty());
    }

    /// The real thing: DuckDB from GitHub into a scratch tool folder and a scratch HOME, so
    /// nothing of the person's changes. Needs uv and the network:
    /// `cargo test -p plumcraft-editor -- --ignored a_public_engine_installs_from_github`
    #[test]
    #[ignore = "downloads from GitHub"]
    fn a_public_engine_installs_from_github() {
        let root = plumcraft_scratch::dir("install-github");
        let (tools, bin, home) = (root.join("tools"), root.join("bin"), root.join("home"));
        // the person's uv cache, so it needn't download everything again
        let cache = std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/uv")).unwrap();
        let env: [(&str, &std::path::Path); 4] = [("UV_TOOL_DIR", &tools), ("UV_TOOL_BIN_DIR", &bin), ("HOME", &home), ("UV_CACHE_DIR", &cache)];
        let said = install_engine("https://github.com/seantrue/plumcraft-duckdb-engine", &env).unwrap();
        assert!(said.contains("duckdb-engine"), "{said}");
        let manifest = home.join(".config/plumcraft/engines/duckdb.toml");
        let resolved = plumcraft_host::manifest::load(&manifest).unwrap();
        assert_eq!(resolved.manifest.id, "duckdb");
        assert!(resolved.demo.as_ref().is_some_and(|d| d.is_file()), "the wheel's demo: {:?}", resolved.demo);
    }
}
