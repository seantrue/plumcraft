//! A graph's file name (FR2, the maintainer: "change the default graph file extension from .json
//! -> .plum … Continue to accept .json as an alternate"). The one place the rule lives: the core's
//! asks, the egui editor and every native shell (through `graph.fileTypes`) read it from here.
//!
//! - A new graph, Save As and Save a Copy offer `.plum` first; `.json` stays choosable.
//! - A graph opened from a `.json` file keeps its name when saved (Save writes to its own file).
//! - Reading is by content, never by extension: a `.plum` file is the same workflow JSON, and a
//!   file of any other name a person opens is read as before (as JSON, or by an application's
//!   opener for its extension).

use std::path::Path;

use serde_json::{Value, json};

/// The extension a new graph's file gets, without its dot.
pub const DEFAULT_EXTENSION: &str = "plum";

/// The extensions a graph's file has, the default first. Both are opened as graphs; an
/// application's file opener can't take either.
pub const EXTENSIONS: &[&str] = &[DEFAULT_EXTENSION, "json"];

/// What a save dialog calls the default kind.
pub const LABEL: &str = "Graph";

/// What a save dialog calls the alternate kind, a graph written with a `.json` name.
pub const JSON_LABEL: &str = "Graph as JSON";

/// The stem of a graph's name before it has one.
pub const UNTITLED: &str = "untitled";

/// Is `ext` (without its dot, any case) a graph file's extension?
pub fn is_graph_extension(ext: &str) -> bool {
    EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

/// Does `path` have a graph file's extension?
pub fn is_graph_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(is_graph_extension)
}

/// `stem` with the default extension: "untitled.plum".
pub fn default_name(stem: &str) -> String {
    format!("{stem}.{DEFAULT_EXTENSION}")
}

/// The name Save As offers: the graph's own stem (or "untitled") with the default extension. A
/// graph opened from `x.json` is offered `x.plum`; the person may still choose `.json`.
pub fn save_as_name(current: Option<&Path>) -> String {
    default_name(current.and_then(Path::file_stem).and_then(|n| n.to_str()).unwrap_or(UNTITLED))
}

/// `name` as a graph's file name: the default extension added when it has none, kept when it
/// has one.
pub fn with_default_extension(name: &str) -> String {
    if Path::new(name).extension().is_none() { default_name(name) } else { name.to_owned() }
}

/// `name` as the new name of the graph file `from` (`graph.rename`): its own extension kept when
/// the name has none and `from`'s is a graph's (a `.json` graph stays `.json`), else as
/// [`with_default_extension`].
pub fn renamed(name: &str, from: &Path) -> String {
    match from.extension().and_then(|e| e.to_str()).filter(|e| is_graph_extension(e)) {
        Some(e) if Path::new(name).extension().is_none() => format!("{name}.{e}"),
        _ => with_default_extension(name),
    }
}

/// The kinds a save ask offers for a graph, the default first (`file.save`'s `kinds`).
pub fn save_kinds() -> Value {
    json!([{"label": LABEL, "extensions": [DEFAULT_EXTENSION]}, {"label": JSON_LABEL, "extensions": ["json"]}])
}

/// `graph.fileTypes`'s reply: what a shell names new graphs, offers in its panels and lists in
/// its browser.
pub fn types() -> Value {
    json!({"default": DEFAULT_EXTENSION, "extensions": EXTENSIONS, "label": LABEL, "untitled": UNTITLED, "kinds": save_kinds()})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn plum_is_the_default_and_json_is_still_a_graph() {
        assert_eq!(DEFAULT_EXTENSION, "plum");
        assert_eq!(EXTENSIONS, ["plum", "json"]);
        for ext in ["plum", "PLUM", "json", "Json"] {
            assert!(is_graph_extension(ext), "{ext}");
        }
        for ext in ["zip", "png", "plumtheme", ""] {
            assert!(!is_graph_extension(ext), "{ext}");
        }
        assert!(is_graph_file(Path::new("/a/b.plum")));
        assert!(is_graph_file(Path::new("/a/b.json")));
        assert!(!is_graph_file(Path::new("/a/b")));
    }

    #[test]
    fn a_new_graph_and_save_as_are_offered_plum() {
        assert_eq!(save_as_name(None), "untitled.plum");
        assert_eq!(save_as_name(Some(&PathBuf::from("/d/flow.json"))), "flow.plum", "Save As of a .json offers .plum");
        assert_eq!(save_as_name(Some(&PathBuf::from("/d/flow.plum"))), "flow.plum");
        assert_eq!(with_default_extension("flow"), "flow.plum");
        assert_eq!(with_default_extension("flow.json"), "flow.json", "a name's own extension is kept");
        assert_eq!(renamed("b", Path::new("/d/a.json")), "b.json", "a .json graph stays .json");
        assert_eq!(renamed("b", Path::new("/d/a.plum")), "b.plum");
        assert_eq!(renamed("b", Path::new("/d/a.png")), "b.plum", "a picture opened as its recipe");
        assert_eq!(renamed("b.json", Path::new("/d/a.plum")), "b.json");
        assert_eq!(save_kinds()[0]["extensions"], json!(["plum"]));
        assert_eq!(save_kinds()[1]["extensions"], json!(["json"]));
        assert_eq!(types()["default"], "plum");
    }
}
