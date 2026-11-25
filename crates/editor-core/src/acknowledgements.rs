//! Acknowledgements (FR4, research note 29): the licences of what PlumCraft is built from, and
//! of each connected engine, as the Acknowledgements window shows them (`app.acknowledgements`
//! opens it; `acknowledgements.get` is its model).
//!
//! The editor's own list is data: the Rust crates' licences made by `cargo about`
//! (`acknowledgements/rust.json`, `bash scripts/acknowledgements.sh`), and what cargo doesn't
//! know about (fonts, code adapted into PlumCraft's source, the Windows shell's packages) in
//! [`OTHER`]. An engine gives its own list under the `acknowledgements` capability; one without
//! it is shown as giving none.

use std::sync::OnceLock;

use plumcraft_proto::acknowledgements::{Acknowledgements, LicenceGroup, Package};
use plumcraft_proto::caps::known;
use plumcraft_proto::session::EngineState;
use serde_json::{Value, json};

/// The Rust crates' licences, as `cargo about` made them with `acknowledgements/about.hbs`.
const RUST: &str = include_str!("../acknowledgements/rust.json");

/// Cargo.lock's SHA-256 when [`RUST`] was made: a test fails when the lockfile has changed since.
pub const LOCK_DIGEST: &str = include_str!("../acknowledgements/cargo-lock.sha256");

/// Which shells an entry of [`OTHER`] is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shells {
    All,
    /// The egui editor.
    Egui,
    /// The Windows shell (C#, WinUI 3).
    Windows,
}

/// A licence cargo doesn't know about: its text and what is under it.
pub struct Other {
    pub licence: &'static str,
    pub name: &'static str,
    pub text: &'static str,
    /// `(name, version, url)`.
    pub packages: &'static [(&'static str, &'static str, &'static str)],
    pub shells: Shells,
}

/// What the editor is built from beyond the Rust crates.
pub const OTHER: &[Other] = &[
    Other {
        licence: "OFL-1.1",
        name: "SIL Open Font License 1.1",
        text: include_str!("../../ui/assets/fonts/OFL-Inter.txt"),
        packages: &[("Inter (font: Regular, SemiBold)", "", "https://github.com/rsms/inter")],
        shells: Shells::Egui,
    },
    Other {
        licence: "CC0-1.0",
        name: "Creative Commons Zero v1.0 Universal",
        text: include_str!("../acknowledgements/other/aileron-cc0.txt"),
        packages: &[("Aileron Regular (font, contact sheet labels)", "1.102", "https://dotcolon.net/fonts/aileron")],
        shells: Shells::All,
    },
    Other {
        licence: "MIT",
        name: "MIT License",
        text: include_str!("../acknowledgements/other/solidcraft-mit.txt"),
        packages: &[("SolidCraft (the STL reader and the mesh viewer's rasteriser, adapted)", "", "https://github.com/tangency-labs/solidcraft")],
        shells: Shells::All,
    },
    Other {
        licence: "LicenseRef-Microsoft-Windows-App-SDK",
        name: "Microsoft Software License Terms: Windows App SDK",
        text: include_str!("../acknowledgements/other/windows-app-sdk-license.txt"),
        packages: &[("Microsoft.WindowsAppSDK", "2.6.5", "https://github.com/microsoft/WindowsAppSDK")],
        shells: Shells::Windows,
    },
    Other {
        licence: "LicenseRef-Microsoft-Windows-App-SDK-ML",
        name: "Microsoft Software License Terms: Windows App SDK ML",
        text: include_str!("../acknowledgements/other/windows-app-sdk-ml-license.txt"),
        packages: &[("Microsoft.WindowsAppSDK.ML", "2.6.5", "https://github.com/microsoft/WindowsAppSDK")],
        shells: Shells::Windows,
    },
    Other {
        licence: "LicenseRef-Microsoft-Windows-App-SDK-Notices",
        name: "Third-party notices: Windows App SDK",
        text: include_str!("../acknowledgements/other/windows-app-sdk-notice.txt"),
        packages: &[("Microsoft.WindowsAppSDK", "2.6.5", "https://github.com/microsoft/WindowsAppSDK")],
        shells: Shells::Windows,
    },
    Other {
        licence: "LicenseRef-Microsoft-WinUI-Notices",
        name: "Third-party notices: WinUI",
        text: include_str!("../acknowledgements/other/windows-app-sdk-winui-notice.txt"),
        packages: &[("Microsoft.WindowsAppSDK.WinUI", "2.6.5", "https://github.com/microsoft/microsoft-ui-xaml")],
        shells: Shells::Windows,
    },
    Other {
        licence: "LicenseRef-Microsoft-Win2D",
        name: "Microsoft Software License Terms: Win2D",
        text: include_str!("../acknowledgements/other/win2d-license.txt"),
        packages: &[("Microsoft.Graphics.Win2D", "1.4.0", "https://github.com/microsoft/Win2D")],
        shells: Shells::Windows,
    },
];

fn group(o: &Other) -> LicenceGroup {
    LicenceGroup {
        licence: o.licence.into(),
        name: o.name.into(),
        text: o.text.replace("\r\n", "\n"),
        packages: o.packages.iter().map(|(n, v, u)| Package { name: (*n).into(), version: (*v).into(), url: (*u).into() }).collect(),
    }
}

/// The Rust crates' licences, read once.
fn rust() -> &'static Acknowledgements {
    static LIST: OnceLock<Acknowledgements> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut a = Acknowledgements::from_cargo_about(RUST);
        for g in &mut a.groups {
            g.text = g.text.replace("\r\n", "\n");
        }
        a
    })
}

fn first(g: &LicenceGroup) -> String {
    g.packages.first().map(|p| p.name.to_lowercase()).unwrap_or_default()
}

/// Is an entry for `shells` in `shell`'s list? `shell` is `mac`, `ios`, `windows` or `egui`;
/// any other, or none, keeps every entry.
fn keeps(shells: Shells, shell: Option<&str>) -> bool {
    match (shells, shell) {
        (Shells::All, _) | (_, None) => true,
        (Shells::Egui, Some(s)) => !matches!(s, "mac" | "ios" | "windows"),
        (Shells::Windows, Some(s)) => !matches!(s, "mac" | "ios" | "egui"),
    }
}

/// The editor's list as `shell` shows it: the Rust crates', then [`OTHER`]'s for that shell,
/// sorted by licence and then by the first package's name.
pub fn editor_for(shell: Option<&str>) -> Acknowledgements {
    let mut a = rust().clone();
    a.groups.extend(OTHER.iter().filter(|o| keeps(o.shells, shell)).map(group));
    a.groups.sort_by_cached_key(|g| (g.licence.to_lowercase(), first(g)));
    a
}

/// The editor's whole list, every shell's entries.
pub fn editor() -> Acknowledgements {
    editor_for(None)
}

/// Does `g` match the search `q` (lower case): its licence, its name, or a package's name?
fn matches(g: &LicenceGroup, q: &str) -> bool {
    q.is_empty() || g.licence.to_lowercase().contains(q) || g.name.to_lowercase().contains(q) || g.packages.iter().any(|p| p.name.to_lowercase().contains(q))
}

/// A list as the window shows it: its groups matching `q`, each with its packages and text.
fn shown(a: &Acknowledgements, q: &str) -> Value {
    let groups: Vec<&LicenceGroup> = a.groups.iter().filter(|g| matches(g, q)).collect();
    json!({
        "groups": groups,
        "packages": a.package_count(),
        "shownPackages": groups.iter().map(|g| g.packages.len()).sum::<usize>(),
        "note": a.note,
    })
}

/// One engine, as `acknowledgements.get` asks it: its id, its name, and its list, or none.
pub struct EngineAck {
    pub id: String,
    pub name: String,
    /// `None`: the engine has no `acknowledgements` capability ("no list given").
    pub list: Option<Result<Acknowledgements, String>>,
}

/// Ask an engine for its list, when it has the capability. Its answer is data from the other
/// side: read leniently and bounded ([`Acknowledgements::from_value`]).
pub fn ask(conn: &dyn plumcraft_host::Connection, state: &EngineState) -> Option<Result<Acknowledgements, String>> {
    let EngineState::Connected { caps, .. } = state else { return None };
    if !caps.has(known::ACKNOWLEDGEMENTS) {
        return None;
    }
    Some(conn.call("acknowledgements", json!({})).map(Acknowledgements::from_value).map_err(|e| e.message.chars().take(300).collect()))
}

/// `acknowledgements.get {shell?, query?}`: the Acknowledgements window's model,
/// `{open, editor: {name, list}, engines: [{id, name, given, list?, error?}], query}`. `shell`
/// (`mac`, `ios`, `windows`, `egui`) leaves out the other shells' own entries; `query` keeps the
/// licences whose id, name or a package's name holds it.
pub fn get_json(open: bool, title: &str, engines: &[EngineAck], params: &Value) -> Value {
    let q = params.get("query").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
    let shell = params.get("shell").and_then(Value::as_str);
    let engines: Vec<Value> = engines
        .iter()
        .map(|e| match &e.list {
            None => json!({"id": e.id, "name": e.name, "given": false}),
            Some(Ok(a)) => json!({"id": e.id, "name": e.name, "given": true, "list": shown(a, &q)}),
            Some(Err(why)) => json!({"id": e.id, "name": e.name, "given": false, "error": why}),
        })
        .collect();
    json!({"open": open, "editor": {"name": title, "list": shown(&editor_for(shell), &q)}, "engines": engines, "query": q})
}

/// `acknowledgements.get`'s params: `shell` and `query`, strings, and nothing else.
pub fn check_params(params: &Value) -> Result<(), String> {
    let Some(o) = params.as_object() else {
        return if params.is_null() { Ok(()) } else { Err("acknowledgements.get takes {shell?, query?}".into()) };
    };
    for (k, v) in o {
        match k.as_str() {
            "shell" | "query" if v.is_string() || v.is_null() => {}
            "shell" | "query" => return Err(format!("{k:?} must be a string")),
            other => return Err(format!("acknowledgements.get has no parameter {:?}", other.chars().take(64).collect::<String>())),
        }
    }
    Ok(())
}

/// `acknowledgements.get` for a document: its engines (each of the coordinator's, or the one
/// linked), asked in turn.
pub fn document_json(d: &crate::document::Document, params: &Value) -> Value {
    let mut engines = Vec::new();
    if let Some(c) = &d.coordinator {
        let states = c.states();
        for e in c.engines() {
            let state = states.iter().find(|(id, _)| *id == e.id).map(|(_, s)| s.clone()).unwrap_or_default();
            engines.push(EngineAck { name: e.id.clone(), list: ask(e.conn.as_ref(), &state), id: e.id });
        }
    } else if let Some(link) = &d.link {
        let info = &d.session.catalog().engine;
        let name = if info.name.is_empty() { info.id.clone() } else { info.name.clone() };
        engines.push(EngineAck { id: info.id.clone(), name, list: ask(link.conn.as_ref(), &d.engine) });
    }
    let title = if d.session.about.title.is_empty() { "PlumCraft" } else { d.session.about.title.as_str() };
    get_json(d.session.views.acknowledgements, title, &engines, params)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list was made from this Cargo.lock: run `bash scripts/acknowledgements.sh` after a
    /// change to the lockfile (it needs `cargo about`; this test needs neither it nor the network).
    #[test]
    fn the_list_was_made_from_this_lockfile() {
        let lock = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock")).unwrap();
        let now = commonkit::digest::fixed::raw(commonkit::digest::fixed::Fixed::Sha256, &lock);
        assert_eq!(
            LOCK_DIGEST.trim(),
            now,
            "Cargo.lock changed since the acknowledgements were made: run `bash scripts/acknowledgements.sh` (it needs `cargo install cargo-about --locked --features cli`) and check the licences it accepts in about.toml"
        );
    }

    #[test]
    fn the_editor_list_has_each_licence_text_with_its_packages() {
        let a = editor();
        assert!(a.package_count() > 300, "{}", a.package_count());
        let names: Vec<&str> = a.groups.iter().flat_map(|g| g.packages.iter().map(|p| p.name.as_str())).collect();
        assert!(names.contains(&"serde") && names.contains(&"egui"), "the crates are there");
        assert!(!names.iter().any(|n| n.starts_with("plumcraft")), "PlumCraft's own crates aren't");
        let mpl = a.groups.iter().find(|g| g.licence == "MPL-2.0").unwrap();
        assert!(
            mpl.packages.iter().all(|p| p.name.starts_with("symphonia") && p.url.to_lowercase().contains("github.com/pdeljanov/symphonia")),
            "MPL-2.0 is symphonia's, with its source's address (2A)"
        );
        assert!(mpl.text.contains("Mozilla Public License"));
        // decision 5A: self_cell under Apache-2.0, never the GPL
        let sc = a.groups.iter().find(|g| g.packages.iter().any(|p| p.name == "self_cell")).unwrap();
        assert_eq!(sc.licence, "Apache-2.0");
        assert!(!a.groups.iter().any(|g| g.licence.contains("GPL")));
        // the Windows shell's packages only on Windows
        let has = |s: Option<&str>, l: &str| editor_for(s).groups.iter().any(|g| g.licence == l);
        assert!(has(Some("windows"), "LicenseRef-Microsoft-Win2D") && !has(Some("mac"), "LicenseRef-Microsoft-Win2D"));
        assert!(editor_for(Some("windows")).groups.iter().all(|g| !g.text.contains('\r')));
        // search
        let v = get_json(true, "PlumCraft", &[], &json!({"query": "symphonia", "shell": "mac"}));
        let groups = v["editor"]["list"]["groups"].as_array().unwrap();
        assert!(!groups.is_empty() && groups.iter().all(|g| g["packages"].to_string().contains("symphonia")));
    }

    #[test]
    fn an_engine_without_the_capability_gives_no_list() {
        let a = Acknowledgements { groups: vec![group(&OTHER[0])], note: String::new() };
        let engines = [
            EngineAck { id: "text".into(), name: "Text".into(), list: Some(Ok(a)) },
            EngineAck { id: "old".into(), name: "Old".into(), list: None },
            EngineAck { id: "bad".into(), name: "Bad".into(), list: Some(Err("timed out".into())) },
        ];
        let v = get_json(false, "PlumCraft", &engines, &json!({}));
        assert_eq!(v["engines"][0]["given"], true);
        assert_eq!(v["engines"][0]["list"]["packages"], 1);
        assert_eq!(v["engines"][1], json!({"id": "old", "name": "Old", "given": false}));
        assert_eq!(v["engines"][2]["error"], "timed out");
    }
}
