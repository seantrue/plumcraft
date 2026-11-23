//! An application's own engines and file openers, compiled into the library its shell links
//! (`docs/shell-interface.md` 4.5). PlumCraft's own apps link this library as it is; an
//! application with an engine of its own builds a static library of its own around this crate,
//! which links both, calls [`register`] from an entry of its own, and has its shell call that
//! entry before the first `plum_open`. Its shell is PlumCraft's, built around that library
//! instead of this one (the Apple kit's `PLUMCRAFT_FFI_XCFRAMEWORK`; the shell's `AppLibrary`
//! hook calls the entry).
//!
//! What is registered is what the egui editor's `AppConfig` takes for the same things: the
//! engines as [`PackEngine`]s (`plumcraft_editor_core::engines::set_packs`), the files they open
//! as workflows as [`FileOpener`]s (`plumcraft_session::file_cmds::register_openers`). Once per
//! process, and only before a core opens: a core reads the engines once, as it opens.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

pub use plumcraft_editor_core::engines::PackEngine;
use plumcraft_editor_core::engines::{EngineKind, set_packs};
pub use plumcraft_session::file_cmds::FileOpener;
use plumcraft_session::file_cmds::register_openers;

/// What an application compiles in beside PlumCraft's engines.
#[derive(Debug, Clone, Default)]
pub struct App {
    /// Its engines, each hosted in process (`worker: false`): a shell's process starts no copy
    /// of itself. Each `id` is the engine's (`--engine`, `engines.default`, `engine.switch`) and
    /// its node kinds' prefix.
    pub packs: Vec<PackEngine>,
    /// What opens other files as workflows (File › Open, `graph.open`), by extension.
    pub openers: Vec<FileOpener>,
}

/// Set once [`register`] has run (whether it took the engines or not): one try per process.
static REGISTERED: LazyLock<Mutex<bool>> = LazyLock::new(Mutex::default);
/// Set by the first `plum_open`: engines are read as a core opens.
static OPENED: AtomicBool = AtomicBool::new(false);

/// Called by `plum_open` before it opens a core.
pub(crate) fn opening() {
    // under the registration's lock, so a core never opens halfway through one
    let _registering = REGISTERED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    OPENED.store(true, Ordering::SeqCst);
}

/// Register the application's engines and file openers, before the first `plum_open`. Refused,
/// with why, once a core has opened, on a second call, for an engine hosted as a worker, for an
/// id that is empty, not a plain name, or already an engine's (`text`, `array`, `audio`, a craft's, another of the
/// application's), and for an extension already taken (a workflow's `json` or `zip`, or another
/// opener's). Nothing is registered when anything is refused.
pub fn register(app: App) -> Result<(), String> {
    let mut done = REGISTERED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if OPENED.load(Ordering::SeqCst) {
        return Err("an application's engines are registered before the first core opens".into());
    }
    if *done {
        return Err("an application's engines are registered once".into());
    }
    let mut seen: Vec<&str> = Vec::new();
    for p in &app.packs {
        let id = p.id.trim();
        if id.is_empty() || id != p.id || id.contains(['.', ',', '/']) {
            return Err(format!("the engine {:?}: an engine's id is a plain name", p.id));
        }
        if p.worker {
            return Err(format!("the engine {id:?}: a native shell hosts an application's engine in process (worker: false)"));
        }
        if seen.contains(&id) || EngineKind::from_id(id).is_some() {
            return Err(format!("the engine {id:?}: that id is taken"));
        }
        seen.push(id);
    }
    // the openers' extensions are checked as they are added; check them all first, so a refusal
    // leaves nothing half registered
    let taken = plumcraft_session::file_cmds::openers();
    let mut exts: Vec<&str> = taken.iter().flat_map(|o| o.extensions.iter().copied()).collect();
    for o in &app.openers {
        for e in o.extensions {
            if e.is_empty() || e.eq_ignore_ascii_case("json") || e.eq_ignore_ascii_case("zip") || exts.iter().any(|x| x.eq_ignore_ascii_case(e)) {
                return Err(format!("the file opener {:?}: .{e} is taken", o.label));
            }
            exts.push(e);
        }
    }
    *done = true;
    register_openers(app.openers)?;
    if !app.packs.is_empty() {
        set_packs(app.packs)?;
    }
    Ok(())
}

/// The ids of the engines the application registered, in order (none before [`register`]).
pub fn registered() -> Vec<String> {
    EngineKind::all().into_iter().filter(|k| matches!(k, EngineKind::Pack(_))).map(EngineKind::id).collect()
}
