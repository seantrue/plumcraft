//! Which engines there are, how each can be hosted, and starting one: the kinds, the hostings
//! (modes 1 to 3), the rules joining them, engine manifests, the platform's [`Capabilities`], and
//! the [`Launcher`] that starts an engine and links to it ([`Link`], woken by a [`Waker`]).
//!
//! Moved out of the windowed `plumcraft-editor` (stage N1, slice 2), so that every shell starts
//! engines the same way: the egui editor, its windowless commands ([`crate::headless`]), and the
//! C ABI (`plumcraft-ffi`). Nothing here draws or names a toolkit.
//!
//! **What exists** is read from three places: the engines built in here (text, the array
//! engine, the audio engine as its own process, the craft adapter's packs), the engines the
//! application compiles in ([`set_packs`], `plumcraft_editor::AppConfig::packs`), and engine
//! manifests ([`load_manifests`], spec 16.6). The packs are set once per process (an engine
//! starts on a thread of its own and reads its pack there). The manifests are read on the
//! thread that chooses and starts engines (the egui shell's UI thread, a native shell's core
//! thread) and kept there, as before the move.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use plumcraft_engine::host::Host;
use plumcraft_host::http::{HttpHost, HttpOptions};
use plumcraft_host::manifest::{Resolved, Start};
use plumcraft_host::process::{Manifest, ProcessHost};
use plumcraft_host::{Connection, InProc};
use plumcraft_session::EngineChoice;

use crate::link::{Link, Waker};

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

impl PackEngine {
    fn mode(&self) -> Mode {
        if self.worker { Mode::Rust } else { Mode::InProc }
    }
}

/// The flag that makes this executable the engine process for a [`PackEngine::worker`] pack.
pub const WORKER_FLAG: &str = "--engine-worker";

/// The compiled-in engines, set once ([`set_packs`]): engines start on threads of their own.
static PACKS: std::sync::OnceLock<Vec<PackEngine>> = std::sync::OnceLock::new();

/// Name the engines the application compiles in. Once per process: a second call is refused.
pub fn set_packs(packs: Vec<PackEngine>) -> Result<(), String> {
    PACKS.set(packs).map_err(|_| "the application started twice".to_owned())
}

fn pack(i: u16) -> Option<&'static PackEngine> {
    PACKS.get().and_then(|p| p.get(usize::from(i)))
}

/// Serve pack `id` to the editor that started this process, until it goes away
/// ([`WORKER_FLAG`]).
pub fn serve_worker(id: &str, args: &[String]) -> Result<(), String> {
    let (socket, token) = plumcraft_engine::serve::endpoint_from_env(args)?;
    let i = u16::try_from(PACKS.get().map_or(0, Vec::len)).unwrap_or(u16::MAX);
    let engine = (0..i).filter_map(pack).find(|p| p.id == id).map(|p| p.engine).ok_or_else(|| format!("no engine {id:?}"))?;
    plumcraft_engine::serve::serve_unix(Box::new(engine), &socket, &token).map_err(|e| format!("the editor's socket: {e}"))
}

/// What the platform the shell runs on has (`docs/shell-interface.md` 4.2), as far as starting
/// engines goes. The shell declares it at start; a hosting that needs something missing is not
/// offered, and asking for it says why ([`Capabilities::refuses`]).
///
/// The default is a desktop's: everything, so the egui editor behaves as it always has. The
/// other platform facts of 4.2 (windows, pointer, keyboard, hover, pencil, picture size) are the
/// shell's and the scene's, for later slices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// It can start programs: engines as processes, spawned HTTP engines, the craft adapter
    /// (which starts the craft's own server), installing engines.
    pub subprocess: bool,
    /// The craft apps (VectorCraft, PhotoCraft, …) can be installed and run here.
    pub craft_apps: bool,
    /// There is a Python to run the Python engines with (`uv`).
    pub python: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Capabilities::DESKTOP
    }
}

impl Capabilities {
    /// A desktop: everything.
    pub const DESKTOP: Capabilities = Capabilities { subprocess: true, craft_apps: true, python: true };
    /// Nothing but this process: in-process engines, and engines already running elsewhere.
    pub const NONE: Capabilities = Capabilities { subprocess: false, craft_apps: false, python: false };

    /// Read `platform` from a shell's config (`docs/shell-interface.md` 4.1): what it doesn't
    /// say it has, it hasn't. Anything that isn't an object is no capabilities at all.
    pub fn declared(platform: Option<&serde_json::Value>) -> Capabilities {
        let has = |k: &str| platform.and_then(|p| p.get(k)).and_then(serde_json::Value::as_bool).unwrap_or(false);
        Capabilities { subprocess: has("subprocess"), craft_apps: has("craft_apps"), python: has("python") }
    }

    /// Why `kind` hosted as `mode` can't be started here, or `None` when it can. The reason is
    /// what a disabled Engine menu item says.
    pub fn refuses(&self, kind: EngineKind, mode: Mode) -> Option<String> {
        if kind.is_craft() && !self.craft_apps {
            return Some(format!("{}: this device has no craft apps", kind.label()));
        }
        if mode.python() && !self.python {
            return Some("this device has no Python".into());
        }
        if kind.spawns(mode) && !self.subprocess {
            return Some("this device can't start programs".into());
        }
        None
    }
}

/// Which engine, hosted how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
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
    pub const ALL: [Mode; 5] = [Mode::InProc, Mode::Rust, Mode::Python, Mode::HttpRust, Mode::HttpPython];

    /// `engine.switch`'s `mode` (after the flags that pick it).
    pub fn id(self) -> &'static str {
        match self {
            Mode::InProc => "inproc",
            Mode::Rust => "process",
            Mode::Python => "python",
            Mode::HttpRust => "http",
            Mode::HttpPython => "http-python",
        }
    }

    pub fn from_id(id: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.id() == id)
    }

    /// Its Engine menu item.
    pub fn menu_label(self) -> &'static str {
        match self {
            Mode::InProc => "Rust · in-process (mode 1)",
            Mode::Rust => "Rust · process (mode 2)",
            Mode::Python => "Python · process (mode 2)",
            Mode::HttpRust => "Rust · HTTP (mode 3)",
            Mode::HttpPython => "Python · HTTP (mode 3)",
        }
    }

    pub fn python(self) -> bool {
        matches!(self, Mode::Python | Mode::HttpPython)
    }

    /// The status bar's hosting.
    pub fn label(self) -> &'static str {
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
pub enum EngineKind {
    Text,
    Audio,
    /// The research software's general array nodes on the CPU (`plumcraft-array`).
    Array,
    /// The craft adapter on an artcraft app's MCP server (`plumcraft-craft`, plan 10.6 M3): a
    /// built-in pack's id (`vectorcraft`, `lightcraft`, …), from `crates/craft/packs/`.
    Craft(&'static str),
    /// An engine an engine manifest names (spec 16.6): an index into the manifests read
    /// ([`load_manifests`]).
    External(u16),
    /// An engine compiled into the application: an index into [`set_packs`]'s.
    Pack(u16),
}

/// An engine an engine manifest names.
#[derive(Debug, Clone)]
pub struct External {
    pub id: String,
    pub title: String,
    pub resolved: Resolved,
}

thread_local! {
    /// The engines manifests name. On the thread where engines are chosen and started: built at
    /// startup, read again by `engine.manifests.reload`.
    static EXTERNALS: RefCell<Vec<External>> = const { RefCell::new(Vec::new()) };
}

fn external(i: u16) -> Option<External> {
    EXTERNALS.with_borrow(|e| e.get(usize::from(i)).cloned())
}

/// The engine manifests this thread has read, to start their engines on another thread: the
/// engine worker ([`crate::worker`]) starts them for the thread that owns the document.
#[derive(Debug, Clone, Default)]
pub struct Manifests(Vec<External>);

impl Manifests {
    /// What this thread has read ([`load_manifests`]).
    pub fn here() -> Manifests {
        Manifests(EXTERNALS.with_borrow(Clone::clone))
    }

    /// Make them this thread's (a worker that starts engines for one document).
    pub fn install(&self) {
        EXTERNALS.with_borrow_mut(|e| e.clone_from(&self.0));
    }
}

/// The folder engines install their manifests in.
pub fn manifest_folder() -> Option<PathBuf> {
    // home(), not $HOME: Windows sets no HOME for an app started from the desktop, so the folder
    // `register` had just written to was never read there
    plumcraft_proto::programs::home().map(|h| h.join(".config").join("plumcraft").join("engines"))
}

/// Read the engine manifests in `files`, then every `*.toml` in `folder`, as the engines this
/// thread offers. What can't be used (it won't parse, or its id is taken, by a built-in engine
/// or an earlier manifest) is reported, and the rest still load.
pub fn load_manifests(files: &[PathBuf], folder: Option<&Path>) -> Vec<String> {
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
pub const EMPTY_WORKFLOW: &str = r#"{"version": 1, "nodes": {}, "wires": []}"#;

/// The value after flag `name`.
pub fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1))
}

impl EngineKind {
    /// Every engine: the application's own, text, audio, array, each built-in craft pack, then
    /// those manifests name.
    pub fn all() -> Vec<EngineKind> {
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
    pub fn id(self) -> String {
        match self {
            EngineKind::Text => "text".into(),
            EngineKind::Audio => "audio".into(),
            EngineKind::Array => "array".into(),
            EngineKind::Craft(id) => id.into(),
            EngineKind::External(i) => external(i).map_or_else(String::new, |e| e.id),
            EngineKind::Pack(i) => pack(i).map_or_else(String::new, |p| p.id.to_owned()),
        }
    }

    pub fn from_id(id: &str) -> Option<EngineKind> {
        EngineKind::all().into_iter().find(|k| k.id() == id)
    }

    pub fn label(self) -> String {
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
    pub fn attaches(self) -> bool {
        matches!(self, EngineKind::External(i) if external(i).is_some_and(|e| matches!(e.resolved.start, Start::Attach { .. })))
    }

    pub fn is_craft(self) -> bool {
        matches!(self, EngineKind::Craft(_))
    }

    /// Is there an engine `self` hosted as `mode`? Only the text engine has a Python twin; the
    /// audio engine and one a manifest names are a process or HTTP, and one already running is
    /// only HTTP.
    pub fn hosts(self, mode: Mode) -> bool {
        match self {
            EngineKind::External(_) if self.attaches() => mode == Mode::HttpRust,
            EngineKind::External(_) | EngineKind::Audio => matches!(mode, Mode::Rust | Mode::HttpRust),
            EngineKind::Pack(i) => mode == pack(i).map_or(Mode::InProc, PackEngine::mode),
            _ => self == EngineKind::Text || !mode.python(),
        }
    }

    /// Does hosting it as `mode` start a program? Everything but an engine compiled in and one
    /// already running over HTTP does; the craft adapter starts the craft's own server even
    /// compiled in.
    pub fn spawns(self, mode: Mode) -> bool {
        match mode {
            Mode::InProc => self.is_craft(),
            Mode::HttpRust => !self.attaches(),
            Mode::Rust | Mode::Python | Mode::HttpPython => true,
        }
    }

    /// Its Engine menu item for `mode`: "Rust" doesn't describe an engine a manifest names.
    pub fn mode_label(self, mode: Mode) -> String {
        match (self, mode) {
            (EngineKind::External(_), Mode::HttpRust) if self.attaches() => "Running · HTTP (mode 3)".into(),
            (EngineKind::External(_), Mode::HttpRust) => "HTTP (mode 3)".into(),
            (EngineKind::External(_), _) => "Process (mode 2)".into(),
            _ => mode.menu_label().into(),
        }
    }

    /// How it is hosted without a flag: the craft adapter and the audio engine as their own
    /// processes, the others compiled in.
    pub fn default_mode(self) -> Mode {
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
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        Ok(Self::all_from_args(args)?.first().copied().unwrap_or(EngineKind::Text))
    }

    /// Every engine `--engine a,b,…` names, in order (text when absent); none twice.
    pub fn all_from_args(args: &[String]) -> Result<Vec<Self>, String> {
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
    pub fn engine(self) -> Result<plumcraft_engine::Engine, String> {
        match self {
            EngineKind::Text => Ok(plumcraft_text::engine()),
            EngineKind::Audio => Err("the audio engine runs as its own process, plumcraft-audio-engine".into()),
            EngineKind::Array => Ok(plumcraft_array::engine()),
            EngineKind::Craft(id) => plumcraft_craft::manifest::Manifest::named(id).and_then(|m| plumcraft_craft::engine(&m)),
            EngineKind::External(_) => Err("an engine a manifest names runs as its own process".into()),
            EngineKind::Pack(i) => pack(i).ok_or_else(|| "that engine is not compiled in".to_owned()).and_then(|p| (p.engine)()),
        }
    }

    /// Its demo workflow, where the engine carries one: `None` for the audio engine, whose demo
    /// is the application's (it is built apart), and for an engine with none.
    pub fn own_demo(self) -> Result<Option<String>, String> {
        Ok(match self {
            EngineKind::Text => Some(plumcraft_text::DEMO_WORKFLOW.into()),
            EngineKind::Audio => None,
            EngineKind::Array => Some(plumcraft_array::DEMO_WORKFLOW.into()),
            EngineKind::Craft(id) => plumcraft_craft::manifest::demo(id).map(str::to_owned),
            EngineKind::External(i) => match external(i).and_then(|e| e.resolved.demo) {
                Some(p) => Some(std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?),
                None => None,
            },
            EngineKind::Pack(i) => pack(i).and_then(|p| p.demo).map(str::to_owned),
        })
    }

    /// The engine binary's name, the command that builds it, and its arguments: `None` for one
    /// a manifest names.
    pub fn binary(self) -> Option<(&'static str, &'static str, &'static str, Vec<String>)> {
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
    pub fn ready_wait(self) -> Duration {
        // uv may have an environment to build before an external engine starts
        if self.is_craft() || matches!(self, EngineKind::External(_) | EngineKind::Pack(_)) { Duration::from_secs(60) } else { Duration::from_secs(10) }
    }
}

/// The engine and mode the command line asks for (`--engine`, `--inproc`, `--process`,
/// `--python`, `--http`), checked: there is no Python audio engine. On a desktop.
pub fn choose(args: &[String]) -> Result<(EngineKind, Mode), String> {
    choose_for(args, &Capabilities::DESKTOP)
}

/// [`choose`], on a platform with `caps`: a hosting it can't start is refused with the reason.
pub fn choose_for(args: &[String], caps: &Capabilities) -> Result<(EngineKind, Mode), String> {
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
    if let Some(why) = caps.refuses(kind, mode) {
        return Err(format!("the {} engine as {}: {why}", kind.id(), mode.id()));
    }
    Ok((kind, mode))
}

/// One engine and hosting, and why it can't be started here when it can't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub choice: EngineChoice,
    /// `None`: it can be started. Otherwise what a disabled Engine menu item says.
    pub why_not: Option<String>,
}

/// Every engine and hosting there is, each with why it can't be started on a platform with
/// `caps` (shell-interface 4.2: listed, and disabled with the reason).
pub fn offers(caps: &Capabilities) -> Vec<Offer> {
    EngineKind::all()
        .into_iter()
        .flat_map(|k| Mode::ALL.into_iter().filter(move |m| k.hosts(*m)).map(move |m| (k, m)))
        .map(|(k, m)| Offer {
            choice: EngineChoice { engine: k.id(), mode: m.id().into(), engine_label: k.label(), mode_label: k.mode_label(m) },
            why_not: caps.refuses(k, m),
        })
        .collect()
}

/// Every engine and hosting the Engine menu offers on a desktop.
pub fn choices() -> Vec<EngineChoice> {
    choices_for(&Capabilities::DESKTOP)
}

/// The engines and hostings that can be started on a platform with `caps`.
pub fn choices_for(caps: &Capabilities) -> Vec<EngineChoice> {
    offers(caps).into_iter().filter(|o| o.why_not.is_none()).map(|o| o.choice).collect()
}

/// Where engine binary `name` is: beside this program, or (tests run from
/// `target/<profile>/deps`) one folder up.
pub fn engine_binary(name: &str) -> Result<PathBuf, String> {
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

/// Starts engine `engine` hosted as `mode` (an [`EngineChoice`]'s ids) and links to it, woken by
/// the waker given.
pub type Connector = Box<dyn FnMut(&str, &str, Option<Waker>) -> Result<Link, String>>;

/// Starts the engines; tests point it at other binaries.
#[derive(Debug, Clone, Default)]
pub struct Launcher {
    /// Engine binaries to use instead of the ones beside the program.
    pub binaries: Vec<(EngineKind, PathBuf)>,
    /// `--engine-manifest`'s files, read again by `engine.manifests.reload`.
    pub manifests: Vec<PathBuf>,
    /// What the platform can start ([`Capabilities::DESKTOP`] unless the shell says otherwise).
    pub caps: Capabilities,
    /// A store root for the engines it starts (the shell's `Dirs.store`): compiled-in ones
    /// publish there, processes get it as `COMMONKIT_STORE_ROOT`. `None`: the shared default.
    pub store_root: Option<PathBuf>,
}

impl Launcher {
    /// `engine.switch`'s ids, checked, then [`Launcher::connect_core`]: what a shell's Engine
    /// menu starts engines with.
    pub fn connect_ids(&self, engine: &str, mode: &str, waker: Option<Waker>) -> Result<Link, String> {
        let kind = EngineKind::from_id(engine).ok_or_else(|| format!("no engine {engine:?}"))?;
        let mode = Mode::from_id(mode).ok_or_else(|| format!("no hosting {mode:?}"))?;
        self.connect_core(kind, mode, waker)
    }

    /// [`Launcher::connect_ids`] as a [`Connector`].
    pub fn connector_core(self) -> Connector {
        Box::new(move |engine, mode, waker| self.connect_ids(engine, mode, waker))
    }

    /// The engine binary to use for `kind`, when a test gave one.
    pub fn binary(&self, kind: EngineKind) -> Option<PathBuf> {
        self.binaries.iter().find(|(k, _)| *k == kind).map(|(_, p)| p.clone())
    }

    /// The engines and hostings this launcher can start.
    pub fn choices(&self) -> Vec<EngineChoice> {
        choices_for(&self.caps)
    }

    /// Start engine `kind` in `mode` and link to it, woken by `waker` (the headless commands
    /// pass none).
    pub fn connect_core(&self, kind: EngineKind, mode: Mode, waker: Option<Waker>) -> Result<Link, String> {
        if let Some(why) = self.caps.refuses(kind, mode) {
            return Err(why);
        }
        start(kind, mode, waker, self.binary(kind), self.store_root.clone())
    }
}

/// Start engine `kind` in `mode` (as `plumcraft-view` does) and link to it, with the engine
/// binary `bin` (mode 2 and 3, Rust) when given, publishing under `store` when given.
pub fn start(kind: EngineKind, mode: Mode, waker: Option<Waker>, bin: Option<PathBuf>, store: Option<PathBuf>) -> Result<Link, String> {
    if let EngineKind::External(i) = kind {
        let e = external(i).ok_or("that engine's manifest is gone (Engine › Reload Engine List)")?;
        return connect_external(&e, mode, kind.ready_wait(), waker, store);
    }
    let manifest = match mode {
        Mode::InProc => None,
        Mode::Rust if matches!(kind, EngineKind::Pack(_)) => {
            let exe = std::env::current_exe().map_err(|e| format!("this program's path: {e}"))?;
            let command = vec![exe.display().to_string(), WORKER_FLAG.to_owned(), kind.id()];
            Some(Manifest { name: kind.id(), command, env: Vec::new(), hint: "the engine's process stopped".into(), store: store.clone(), cwd: None })
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
            Some(Manifest { name: name.into(), command, env: Vec::new(), hint: hint.into(), store: store.clone(), cwd: None })
        }
        Mode::Python | Mode::HttpPython if kind != EngineKind::Text => return Err(format!("there is no Python {} engine", kind.id())),
        Mode::Python | Mode::HttpPython => {
            let (command, env) = plumcraft_text::python_command();
            Some(Manifest {
                name: "text (Python)".into(),
                command,
                env,
                hint: "needs uv (cd python && uv sync), or set PLUMCRAFT_UV".into(),
                store: store.clone(),
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
        // without a root given, the engine keeps its own (the shared default)
        let h = Host::start(Box::new(move || kind.engine().map(|e| if store.is_some() { e.with_store(store.clone()) } else { e })), None);
        h.wait_ready(kind.ready_wait());
        Arc::new(InProc(h))
    };
    Link::start(conn, mode.label(), waker)
}

/// Start the engine a manifest names and link to it: a process on a socket, or HTTP, spawned or
/// already running.
fn connect_external(e: &External, mode: Mode, wait: Duration, waker: Option<Waker>, store: Option<PathBuf>) -> Result<Link, String> {
    let with_store = |m: &Manifest| {
        let mut m = m.clone();
        if m.store.is_none() {
            m.store.clone_from(&store);
        }
        m
    };
    let (conn, label): (Arc<dyn Connection>, &str) = match (&e.resolved.start, mode) {
        (Start::Attach { url, token }, _) => (Arc::new(HttpHost::attach(url, token, HttpOptions::default())?), "mode 3 · HTTP, running"),
        (Start::Command(m), Mode::HttpRust) => {
            (Arc::new(HttpHost::spawn(&with_store(m), HttpOptions { remote: true, ..Default::default() })?), "mode 3 · HTTP")
        }
        (Start::Command(m), _) => {
            let h = ProcessHost::start(with_store(m)).map_err(|e| e.to_string())?;
            h.wait_ready(wait);
            (Arc::new(h), "mode 2 · process")
        }
    };
    Link::start(conn, label, waker)
}

/// Attach to a running HTTP engine, as a remote UI would (values by URL, sources by upload):
/// `engine.attach`. Needs nothing started, so every platform has it.
pub fn attach(url: &str, token: &str, waker: Option<Waker>) -> Result<Link, String> {
    let host = HttpHost::attach(url, token, HttpOptions { remote: true, ..Default::default() })?;
    let conn: Arc<dyn Connection> = Arc::new(host);
    Link::start(conn, format!("HTTP · attached · {url}"), waker)
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
pub fn manifest_args(args: &[String]) -> Vec<PathBuf> {
    args.windows(2).filter(|w| w[0] == "--engine-manifest").map(|w| PathBuf::from(&w[1])).collect()
}

/// `args`, with `--engine` naming the engines the workflow names, for `run`, `check` and `tidy`
/// given no `--engine`. A manifest given (`--engine-manifest`) only says how to start one of them.
pub fn with_file_engines(mut args: Vec<String>) -> Vec<String> {
    if matches!(args.first().map(String::as_str), Some("run" | "check" | "tidy"))
        && !args.iter().any(|a| a == "--engine")
        && let Some(ids) = file_engines(&args[1..])
    {
        args.extend(["--engine".to_owned(), ids.join(",")]);
    }
    args
}

/// `args`, with `--engine <id>` of the first manifest given when `--engine` names none.
pub fn with_engine_arg(mut args: Vec<String>) -> Vec<String> {
    if !args.iter().any(|a| a == "--engine")
        && let Some(id) = manifest_args(&args).first().and_then(|f| plumcraft_host::manifest::load(f).ok()).map(|r| r.manifest.id)
    {
        args.extend(["--engine".to_owned(), id]);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every (engine, mode) id an offer list holds, split into the startable and the refused.
    fn split(offers: &[Offer]) -> (Vec<String>, Vec<(String, String)>) {
        let id = |o: &Offer| format!("{}/{}", o.choice.engine, o.choice.mode);
        let ok = offers.iter().filter(|o| o.why_not.is_none()).map(id).collect();
        let refused = offers.iter().filter_map(|o| o.why_not.clone().map(|w| (id(o), w))).collect();
        (ok, refused)
    }

    #[test]
    fn a_desktop_offers_every_hosting_as_before() {
        assert_eq!(Capabilities::default(), Capabilities::DESKTOP);
        let all = offers(&Capabilities::DESKTOP);
        assert!(all.iter().all(|o| o.why_not.is_none()));
        assert_eq!(choices(), all.into_iter().map(|o| o.choice).collect::<Vec<_>>());
        assert_eq!(Launcher::default().choices(), choices());
    }

    #[test]
    fn without_programs_only_in_process_and_running_http_engines_are_offered() {
        // a running engine a manifest names (HTTP, attached) is offered; one it would start is not
        let dir = plumcraft_scratch::dir("core-caps-manifests");
        std::fs::write(dir.join("token"), "secret\n").unwrap();
        std::fs::write(dir.join("far.toml"), "id = \"far\"\nmode = \"http\"\nurl = \"http://127.0.0.1:1\"\ntoken_file = \"token\"\n").unwrap();
        // (a program that exists, never started here)
        let exe = std::env::current_exe().unwrap();
        std::fs::write(dir.join("near.toml"), format!("id = \"near\"\ncommand = [{:?}]\n", exe.display().to_string())).unwrap();
        assert_eq!(load_manifests(&[dir.join("far.toml"), dir.join("near.toml")], None), Vec::<String>::new());

        let caps = Capabilities { subprocess: false, ..Capabilities::DESKTOP };
        let (ok, refused) = split(&offers(&caps));
        let crafts: Vec<String> = plumcraft_craft::manifest::BUILTIN.iter().map(|c| format!("{c}/inproc")).collect();
        // (the crafts are installed here, but the adapter starts the craft's server, a program)
        assert_eq!(ok, ["text/inproc", "array/inproc", "far/http"]);
        for c in &crafts {
            assert!(refused.iter().any(|(id, why)| id == c && why == "this device can't start programs"), "{c}: {refused:?}");
        }
        // every other hosting is listed, saying why
        for (id, why) in [
            ("text/process", "this device can't start programs"),
            ("text/python", "this device can't start programs"),
            ("text/http", "this device can't start programs"),
            ("text/http-python", "this device can't start programs"),
            ("audio/process", "this device can't start programs"),
            ("audio/http", "this device can't start programs"),
            ("array/process", "this device can't start programs"),
            ("near/process", "this device can't start programs"),
            ("near/http", "this device can't start programs"),
        ] {
            assert!(refused.contains(&(id.to_owned(), why.to_owned())), "{id}: {refused:?}");
        }
        assert_eq!(ok.len() + refused.len(), offers(&Capabilities::DESKTOP).len(), "nothing is hidden, only refused");
        assert_eq!(choices_for(&caps).iter().map(|c| format!("{}/{}", c.engine, c.mode)).collect::<Vec<_>>(), ok);

        // a phone or tablet (no programs, no Python, no craft apps): the reasons are the nearest
        let ios = Capabilities::NONE;
        let (ok, refused) = split(&offers(&ios));
        assert_eq!(ok, ["text/inproc", "array/inproc", "far/http"]);
        let why = |id: &str| refused.iter().find(|(i, _)| i == id).map(|(_, w)| w.clone()).unwrap();
        assert_eq!(why("text/python"), "this device has no Python");
        assert_eq!(why("text/process"), "this device can't start programs");
        assert_eq!(why("vectorcraft/inproc"), "VectorCraft: this device has no craft apps");

        // starting one is refused the same way, before anything is started
        let launcher = Launcher { caps: ios, ..Default::default() };
        assert_eq!(launcher.connect_ids("text", "process", None).err().as_deref(), Some("this device can't start programs"));
        assert_eq!(launcher.connect_ids("text", "python", None).err().as_deref(), Some("this device has no Python"));
        let args = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(choose_for(&args(&["--engine", "audio"]), &ios).unwrap_err().contains("can't start programs"));
        assert_eq!(choose_for(&args(&["--engine", "array"]), &ios), Ok((EngineKind::Array, Mode::InProc)));
        // and in-process still starts
        let link = launcher.connect_ids("text", "inproc", None).unwrap();
        assert!(link.state().is_connected(), "{:?}", link.state());
    }

    #[test]
    fn a_platform_declares_what_it_has_and_nothing_else() {
        let v = serde_json::json!({"os": "ios", "subprocess": false, "craft_apps": false, "python": true, "pencil": true});
        assert_eq!(Capabilities::declared(Some(&v)), Capabilities { python: true, ..Capabilities::NONE });
        assert_eq!(Capabilities::declared(None), Capabilities::NONE, "unsaid is absent");
        assert_eq!(Capabilities::declared(Some(&serde_json::json!([1]))), Capabilities::NONE);
        assert_eq!(Capabilities::declared(Some(&serde_json::json!({"subprocess": "yes"}))), Capabilities::NONE, "hostile values are ignored");
        let all = serde_json::json!({"subprocess": true, "craft_apps": true, "python": true});
        assert_eq!(Capabilities::declared(Some(&all)), Capabilities::DESKTOP);
    }

    #[test]
    fn an_engine_starts_publishing_where_it_is_told() {
        let root = plumcraft_scratch::dir("core-launcher-store");
        let launcher = Launcher { store_root: Some(root.clone()), ..Default::default() };
        let link = launcher.connect_core(EngineKind::Text, Mode::InProc, None).unwrap();
        assert!(link.state().is_connected());
        assert_eq!(link.label, "Rust · mode 1 · in-process");
        let r = link.conn.call("hello", serde_json::json!({"protocol": 2})).unwrap();
        let hello: plumcraft_proto::session::HelloReply = serde_json::from_value(r).unwrap();
        assert_eq!(hello.data.stores, [root.display().to_string()], "{hello:?}");
    }
}
