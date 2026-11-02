//! Preferences kept between runs: the theme, and the folder each kind of file dialog last used.
//! Read and write folders are remembered apart, and pictures apart from workflows, so opening a
//! picture from one place and saving results to another needs no re-navigating.
//!
//! Each application has its own file, by the name it runs under (`AppConfig.name`):
//! `~/Library/Application Support/<name>/prefs.json` on macOS and
//! `$XDG_CONFIG_HOME/<name>/prefs.json` elsewhere. `<NAME>_PREFS` (`PLUMIMPRESS_PREFS`,
//! `PLUMCRAFT_EDITOR_PREFS`) names another.
//!
//! The file is input like any other: one that is missing, too big or doesn't parse is the
//! defaults, and a failed write costs the preference and not the editor. Fields it doesn't know are
//! kept, so a newer build's survive an older one.
//!
//! Lifted from the research software's `crates/ui-egui/src/prefs.rs` (`8480c03`), where the name and file were
//! The research software's own; an [`EditorApp`](crate::EditorApp) that isn't given a file keeps them in
//! memory, so tests never touch the real one.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use plumcraft_proto::catalogue::PathHint;
use plumcraft_session::paths::home_dir;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The largest preferences file read.
const MAX_BYTES: u64 = 1024 * 1024;

/// The dialog kinds whose folders are remembered.
pub mod keys {
    /// File › Open, Insert: a workflow or bundle to read.
    pub const WORKFLOW_READ: &str = "workflow.read";
    /// Save, Save As: where a workflow is written.
    pub const WORKFLOW_WRITE: &str = "workflow.write";
    /// Export Bundle.
    pub const BUNDLE_WRITE: &str = "bundle.write";
    /// Save Result…, Contact Sheet…: where the engine writes a value.
    pub const RESULT_WRITE: &str = "result.write";
    /// A picture a path input reads.
    pub const PICTURE_READ: &str = "picture.read";
    /// A picture a path input writes.
    pub const PICTURE_WRITE: &str = "picture.write";
    /// Any other file a path input reads or writes, and a folder.
    pub const FILE_READ: &str = "file.read";
    pub const FILE_WRITE: &str = "file.write";
    pub const FOLDER: &str = "folder";
}

const PICTURE_EXTS: &[&str] = &["png", "jpg", "jpeg", "tif", "tiff", "psd", "webp", "bmp", "gif", "heic"];

/// What is remembered.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// The theme's name ([`crate::theme::by_name`]); none means the default.
    pub theme: Option<String>,
    /// The last folder by dialog kind ([`keys`]).
    pub dirs: BTreeMap<String, PathBuf>,
    /// Fields this build doesn't know, kept for the file.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Prefs {
    /// The folder remembered for `key`, if it still exists.
    pub fn dir(&self, key: &str) -> Option<&Path> {
        self.dirs.get(key).map(PathBuf::as_path).filter(|d| d.is_dir())
    }

    /// The preferences at `path`, or the defaults when there are none or they can't be read.
    pub fn load_from(path: &Path) -> Prefs {
        let Ok(file) = std::fs::File::open(path) else { return Prefs::default() };
        let mut text = String::new();
        if file.take(MAX_BYTES + 1).read_to_string(&mut text).is_err() || text.len() as u64 > MAX_BYTES {
            return Prefs::default();
        }
        serde_json::from_str(&text).unwrap_or_default()
    }

    fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        plumcraft_session::file_cmds::write_atomic(path, &text).map_err(|e| e.to_string())
    }
}

/// Where application `name`'s preferences live.
pub fn path_for(name: &str) -> Option<PathBuf> {
    let var: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect();
    if let Some(p) = std::env::var_os(format!("{var}_PREFS")).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let home = home_dir()?;
    Some(if cfg!(target_os = "macos") {
        home.join("Library/Application Support").join(name).join("prefs.json")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").filter(|p| !p.is_empty()).map_or_else(|| home.join(".config"), PathBuf::from).join(name).join("prefs.json")
    })
}

/// Where application `name` keeps the workflows it trusts: beside its preferences, so
/// `<NAME>_PREFS` moves both.
pub fn trust_path_for(name: &str) -> Option<PathBuf> {
    path_for(name).map(|p| p.with_file_name("trusted.json"))
}

/// The preferences and the file they live in. Without a file they last as long as the window.
#[derive(Debug, Clone, Default)]
pub struct Store {
    path: Option<PathBuf>,
    prefs: Prefs,
}

impl Store {
    /// The preferences at `path`.
    pub fn at(path: PathBuf) -> Self {
        Store { prefs: Prefs::load_from(&path), path: Some(path) }
    }

    /// Application `name`'s own preferences ([`path_for`]).
    pub fn for_app(name: &str) -> Self {
        path_for(name).map_or_else(Store::default, Store::at)
    }

    /// The file, when there is one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn prefs(&self) -> &Prefs {
        &self.prefs
    }

    /// The folder remembered for `key`, if it still exists.
    pub fn dir(&self, key: &str) -> Option<PathBuf> {
        self.prefs.dir(key).map(Path::to_path_buf)
    }

    /// A failed write loses the preference, and is said; it never stops the editor.
    fn save(&self) {
        if let Some(path) = &self.path
            && let Err(e) = self.prefs.save_to(path)
        {
            eprintln!("preferences not saved to {}: {e}", path.display());
        }
    }

    /// Remember the folder of `chosen` (a file's parent, or the folder itself) for `key`.
    pub fn remember(&mut self, key: &str, chosen: &Path) {
        let dir = if chosen.is_dir() { Some(chosen) } else { chosen.parent().filter(|p| !p.as_os_str().is_empty()) };
        if let Some(dir) = dir
            && self.prefs.dirs.get(key).map(PathBuf::as_path) != Some(dir)
        {
            self.prefs.dirs.insert(key.to_owned(), dir.to_path_buf());
            self.save();
        }
    }

    pub fn set_theme(&mut self, name: &str) {
        if self.prefs.theme.as_deref() != Some(name) {
            self.prefs.theme = Some(name.to_owned());
            self.save();
        }
    }
}

/// Which remembered folder a path input's dialog uses: pictures apart from other files, and
/// reading apart from writing.
pub fn dir_key(hint: &PathHint) -> &'static str {
    let picture = hint.filters.iter().flat_map(|f| &f.extensions).any(|e| PICTURE_EXTS.contains(&e.trim_start_matches('.').to_ascii_lowercase().as_str()));
    match (hint.mode.as_str(), picture) {
        ("dir", _) => keys::FOLDER,
        ("save", true) => keys::PICTURE_WRITE,
        ("save", false) => keys::FILE_WRITE,
        (_, true) => keys::PICTURE_READ,
        (_, false) => keys::FILE_READ,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::catalogue::PathFilter;
    use serde_json::json;

    fn hint(mode: &str, exts: &[&str]) -> PathHint {
        PathHint { mode: mode.into(), filters: vec![PathFilter { name: "x".into(), extensions: exts.iter().map(|e| (*e).to_owned()).collect() }] }
    }

    fn folder(name: &str) -> PathBuf {
        let d = plumcraft_scratch::dir("prefs").join(name);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn read_and_write_folders_are_kept_apart_and_come_back() {
        let dir = folder("apart");
        let (a, b) = (dir.join("in"), dir.join("out"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let file = dir.join("nested/prefs.json");
        let mut s = Store::at(file.clone());
        s.remember(keys::PICTURE_READ, &a.join("corgi.jpg"));
        s.remember(keys::PICTURE_WRITE, &b);
        s.set_theme("dark");
        let back = Store::at(file.clone());
        assert_eq!(back.prefs(), s.prefs());
        assert_eq!(back.dir(keys::PICTURE_READ), Some(a));
        assert_eq!(back.dir(keys::PICTURE_WRITE), Some(b.clone()));
        assert_eq!(back.dir(keys::WORKFLOW_READ), None);
        std::fs::remove_dir(&b).unwrap();
        assert_eq!(Store::at(file).dir(keys::PICTURE_WRITE), None, "a folder that went away is forgotten");
    }

    #[test]
    fn a_bad_file_is_the_defaults_and_never_panics() {
        let dir = folder("bad");
        let file = dir.join("prefs.json");
        assert_eq!(Prefs::load_from(&file), Prefs::default(), "none there");
        let long = "x".repeat(MAX_BYTES as usize + 10);
        for junk in ["", "{nope", "[]", "null", "7", r#"{"theme": 7}"#, r#"{"dirs": []}"#, r#"{"dirs": {"a": 3}}"#, long.as_str()] {
            std::fs::write(&file, junk).unwrap();
            assert_eq!(Prefs::load_from(&file), Prefs::default(), "{}", junk.chars().take(32).collect::<String>());
        }
        std::fs::write(&file, [0xFF, 0xFE, 0x00]).unwrap();
        assert_eq!(Prefs::load_from(&file), Prefs::default(), "not text");
        assert_eq!(Prefs::load_from(&dir), Prefs::default(), "a folder");
    }

    #[test]
    fn a_field_this_build_does_not_know_survives_a_write() {
        let dir = folder("unknown");
        let file = dir.join("prefs.json");
        std::fs::write(&file, r#"{"theme": "medium", "zoom": {"x": 2}}"#).unwrap();
        let mut s = Store::at(file.clone());
        s.set_theme("dark");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["zoom"], json!({"x": 2}));
        assert_eq!(v["theme"], "dark");
    }

    #[test]
    fn a_file_that_cannot_be_written_costs_only_the_preference() {
        let dir = folder("nowrite");
        std::fs::write(dir.join("file"), "x").unwrap();
        let mut s = Store::at(dir.join("file/prefs.json"));
        s.set_theme("dark");
        assert_eq!(s.prefs().theme.as_deref(), Some("dark"), "it still holds for this run");
    }

    #[test]
    fn each_application_has_its_own_file() {
        let (a, b) = (path_for("plumimpress").unwrap(), path_for("plumcraft-editor").unwrap());
        assert_ne!(a, b);
        assert!(a.ends_with("plumimpress/prefs.json"), "{}", a.display());
        assert!(b.ends_with("plumcraft-editor/prefs.json"), "{}", b.display());
    }

    #[test]
    fn path_inputs_pick_their_folder_by_kind() {
        assert_eq!(dir_key(&hint("open", &["png", "jpg"])), keys::PICTURE_READ);
        assert_eq!(dir_key(&hint("", &["png"])), keys::PICTURE_READ);
        assert_eq!(dir_key(&hint("files", &["jpg"])), keys::PICTURE_READ);
        assert_eq!(dir_key(&hint("save", &[".PNG"])), keys::PICTURE_WRITE);
        assert_eq!(dir_key(&hint("open", &["yaml"])), keys::FILE_READ);
        assert_eq!(dir_key(&hint("save", &[])), keys::FILE_WRITE);
        assert_eq!(dir_key(&hint("dir", &["png"])), keys::FOLDER);
    }
}
