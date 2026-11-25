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

/// Is `line` a copyright notice: it starts with "Copyright" (not "copyright notice", "copyright
/// holders" as a wrapped sentence goes on), "(c)" and a year, "©" or "All rights reserved", after
/// any comment marks?
pub fn is_notice(line: &str) -> bool {
    let t = line.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '#' | '/' | '*' | '-' | '>')).to_lowercase();
    // "(c) 2020 Someone", never a list item such as Apache-2.0's "(c) You must retain, …"
    if let Some(rest) = t.strip_prefix("(c)") {
        return rest.trim_start().starts_with(|c: char| c.is_ascii_digit());
    }
    if t.starts_with('©') || t.starts_with("all rights reserved") {
        return true;
    }
    match t.strip_prefix("copyright") {
        Some(rest) => {
            let next: String = rest.trim_start().chars().take_while(|c| c.is_alphanumeric()).collect();
            !rest.starts_with(|c: char| c.is_alphanumeric())
                && !matches!(
                    next.as_str(),
                    "notice"
                        | "notices"
                        | "holder"
                        | "holders"
                        | "owner"
                        | "owners"
                        | "and"
                        | "law"
                        | "laws"
                        | "license"
                        | "licence"
                        | "statement"
                        | "statements"
                )
        }
        None => false,
    }
}

/// Where Apache-2.0's terms end: after it comes the appendix (how to apply it), where a package's
/// own notice lines are.
const APACHE_END: &str = "END OF TERMS AND CONDITIONS";

/// A licence text's substance, for comparing two: its copyright lines (and, for Apache-2.0, the
/// appendix) taken out, a title line at the start dropped, in lower case without quotes, the
/// whitespace and line wrapping one space, without a final full stop.
fn substance(text: &str) -> String {
    let head = text.find(APACHE_END).map_or(text, |i| &text[..i]);
    let mut lines: Vec<&str> = head.lines().filter(|l| !is_notice(l)).collect();
    // a title ("MIT License", "The MIT License (MIT)", "Apache License, Version 2.0, January 2004"):
    // short lines with "licen" in them before the first sentence
    while let Some(first) = lines.iter().position(|l| !l.trim().is_empty()) {
        let l = lines[first].trim().to_lowercase();
        let title = l.chars().count() <= 60 && (l.contains("licen") || l.starts_with("version ") || l.starts_with("http")) && !l.ends_with('.');
        if !title {
            break;
        }
        lines.remove(first);
    }
    let joined = lines.join(" ").to_lowercase().replace(['"', '\'', '`', '\u{201c}', '\u{201d}', '\u{2018}', '\u{2019}'], "");
    joined.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches('.').to_owned()
}

/// A package under a licence row, with its own notice lines.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RowPackage {
    pub name: String,
    pub version: String,
    pub url: String,
    /// Its copyright lines (and, under Apache-2.0, what its appendix adds), every one kept.
    pub notices: Vec<String>,
}

/// One text under a licence row: the licence's text once, or a variant, with whose it is.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RowText {
    pub text: String,
    /// `name version` of each package it is for.
    pub packages: Vec<String>,
    /// The licence's own text (the most packages'), shown without copyright lines; a variant
    /// (a changed licence, an added exception) is shown whole, under its packages.
    pub standard: bool,
}

/// One licence in a list: every package under it, and its text once, with any variants.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LicenceRow {
    pub licence: String,
    pub name: String,
    pub packages: Vec<RowPackage>,
    pub texts: Vec<RowText>,
}

/// A list as rows, one per licence id (the maintainer, on seeing a list of many "Apache-2.0: …"
/// rows: "coalesce license variations"). Texts whose substance is the same ([`substance`]) are
/// one; the copyright lines taken out of them stay with their packages, so none is lost.
pub fn coalesce(a: &Acknowledgements) -> Vec<LicenceRow> {
    let mut ids: Vec<&str> = Vec::new();
    for g in &a.groups {
        if !ids.contains(&g.licence.as_str()) {
            ids.push(&g.licence);
        }
    }
    let mut rows: Vec<LicenceRow> = ids
        .into_iter()
        .map(|id| {
            let groups: Vec<&LicenceGroup> = a.groups.iter().filter(|g| g.licence == id).collect();
            // Apache-2.0's appendix: lines most of its texts share are its boilerplate; the rest are notices
            let tails: Vec<Vec<String>> = groups
                .iter()
                .map(|g| {
                    g.text
                        .find(APACHE_END)
                        .map(|i| g.text[i + APACHE_END.len()..].lines().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()).collect())
                        .unwrap_or_default()
                })
                .collect();
            let common = |line: &str| tails.iter().filter(|t| t.iter().any(|l| l == line)).count() * 2 > tails.len().max(1);
            let notices_of = |k: usize| -> Vec<String> {
                let g = groups[k];
                let head = g.text.find(APACHE_END).map_or(g.text.as_str(), |i| &g.text[..i]);
                let mut n: Vec<String> = head.lines().filter(|l| is_notice(l)).map(|l| l.trim().to_owned()).collect();
                n.extend(tails[k].iter().filter(|l| is_notice(l) || !common(l)).cloned());
                n.dedup();
                n
            };
            let subs: Vec<String> = groups.iter().map(|g| substance(&g.text)).collect();
            // the standard text: the substance the most packages share
            let weight = |s: &str| groups.iter().zip(&subs).filter(|(_, t)| *t == s).map(|(g, _)| g.packages.len()).sum::<usize>();
            let standard = subs.iter().max_by_key(|s| weight(s)).cloned().unwrap_or_default();
            let mut packages: Vec<RowPackage> = Vec::new();
            let mut texts: Vec<RowText> = Vec::new();
            for (k, g) in groups.iter().enumerate() {
                let notices = notices_of(k);
                for p in &g.packages {
                    match packages.iter_mut().find(|x| x.name == p.name && x.version == p.version) {
                        Some(x) => {
                            for n in &notices {
                                if !x.notices.contains(n) {
                                    x.notices.push(n.clone());
                                }
                            }
                        }
                        None => packages.push(RowPackage { name: p.name.clone(), version: p.version.clone(), url: p.url.clone(), notices: notices.clone() }),
                    }
                }
                let who: Vec<String> =
                    g.packages.iter().map(|p| if p.version.is_empty() { p.name.clone() } else { format!("{} {}", p.name, p.version) }).collect();
                let is_standard = subs[k] == standard;
                match texts.iter_mut().find(|t| t.standard && is_standard) {
                    Some(t) => t.packages.extend(who),
                    None if is_standard => {
                        let head = g.text.find(APACHE_END).map_or(g.text.clone(), |i| g.text[..i + APACHE_END.len()].to_owned());
                        let body: Vec<&str> = head.lines().filter(|l| !is_notice(l)).collect();
                        texts.push(RowText { text: body.join("\n").trim().to_owned(), packages: who, standard: true });
                    }
                    None => texts.push(RowText { text: g.text.clone(), packages: who, standard: false }),
                }
            }
            texts.sort_by_key(|t| !t.standard);
            packages.sort_by_key(|p| (p.name.to_lowercase(), p.version.clone()));
            let name = groups.iter().map(|g| g.name.as_str()).find(|n| !n.is_empty()).unwrap_or("").to_owned();
            LicenceRow { licence: id.to_owned(), name, packages, texts }
        })
        .collect();
    rows.sort_by_key(|r| r.licence.to_lowercase());
    rows
}

/// Does `r` match the search `q` (lower case): its licence, its name, or a package's name?
fn matches(r: &LicenceRow, q: &str) -> bool {
    q.is_empty() || r.licence.to_lowercase().contains(q) || r.name.to_lowercase().contains(q) || r.packages.iter().any(|p| p.name.to_lowercase().contains(q))
}

/// A list as the window shows it: a row per licence matching `q`, each with its packages (and
/// their notices) and its text once; a search by a package's name keeps only those packages.
fn shown(a: &Acknowledgements, q: &str) -> Value {
    let rows = coalesce(a);
    let all: usize = rows.iter().map(|r| r.packages.len()).sum();
    // a search by a package's name keeps only those packages in their rows, with the texts they
    // are under; one by a licence keeps its row whole
    let rows: Vec<LicenceRow> = rows
        .into_iter()
        .filter(|r| matches(r, q))
        .map(|mut r| {
            if !q.is_empty() && !r.licence.to_lowercase().contains(q) && !r.name.to_lowercase().contains(q) {
                r.packages.retain(|p| p.name.to_lowercase().contains(q));
                let named = |who: &String| r.packages.iter().any(|p| who == &p.name || who.starts_with(&format!("{} ", p.name)));
                let keep: Vec<bool> = r.texts.iter().map(|t| t.packages.iter().any(named)).collect();
                let mut k = keep.into_iter();
                r.texts.retain(|_| k.next().unwrap_or(false));
            }
            r
        })
        .collect();
    json!({
        "rows": rows,
        "packages": all,
        "shownPackages": rows.iter().map(|r| r.packages.len()).sum::<usize>(),
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
/// `{open, editor: {name, list}, engines: [{id, name, given, list?, error?}], query}`, each list
/// `{rows: [LicenceRow], packages, shownPackages, note}` ([`coalesce`]: an engine's list is
/// coalesced here too, its data on the wire unchanged). `shell`
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
        // search: the licences a package's name matches, one row each
        let v = get_json(true, "PlumCraft", &[], &json!({"query": "symphonia", "shell": "mac"}));
        let rows = v["editor"]["list"]["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["licence"], "MPL-2.0");
    }

    /// "coalesce license variations": a row per licence id, and no copyright line of the source
    /// list missing from it, each with its own package.
    #[test]
    fn the_coalesced_list_has_a_row_per_licence_and_every_copyright_line() {
        for shell in [None, Some("mac"), Some("windows"), Some("egui")] {
            let a = editor_for(shell);
            let rows = coalesce(&a);
            let mut ids: Vec<&str> = a.groups.iter().map(|g| g.licence.as_str()).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(rows.len(), ids.len(), "{shell:?}");
            let mut lines = 0;
            for g in &a.groups {
                let row = rows.iter().find(|r| r.licence == g.licence).unwrap();
                for line in g.text.lines().filter(|l| is_notice(l)) {
                    lines += 1;
                    for p in &g.packages {
                        let rp = row.packages.iter().find(|x| x.name == p.name && x.version == p.version).unwrap();
                        assert!(rp.notices.iter().any(|n| n == line.trim()), "{} {}: {line:?} lost", p.name, p.version);
                    }
                }
            }
            assert!(lines > 150, "{lines} copyright lines checked");
            // every package is in its row
            for g in &a.groups {
                let row = rows.iter().find(|r| r.licence == g.licence).unwrap();
                assert!(g.packages.iter().all(|p| row.packages.iter().any(|x| x.name == p.name && x.version == p.version)));
            }
        }
        let rows = coalesce(&editor_for(Some("mac")));
        let mit = rows.iter().find(|r| r.licence == "MIT").unwrap();
        assert!(mit.texts[0].standard && mit.texts[0].packages.len() > 100, "most MIT texts are one: {}", mit.texts[0].packages.len());
        assert!(!mit.texts[0].text.lines().any(is_notice), "the standard text has no copyright line");
        let apache = rows.iter().find(|r| r.licence == "Apache-2.0").unwrap();
        assert_eq!(apache.texts.len(), 1, "Apache-2.0's texts are one");
        assert!(apache.texts[0].text.contains("(c) You must retain") && apache.texts[0].text.ends_with("END OF TERMS AND CONDITIONS"));
        let zopfli = apache.packages.iter().find(|p| p.name == "zopfli").unwrap();
        assert!(zopfli.notices.iter().any(|n| n == "Copyright 2011 Google Inc."), "{:?}", zopfli.notices);
    }

    #[test]
    fn a_text_is_the_same_whatever_its_copyright_wrapping_and_title() {
        assert!(is_notice("Copyright (c) 2016 Someone"));
        assert!(is_notice("  (c) 2020 Someone"));
        assert!(is_notice("// Copyright 2011 Google Inc."));
        assert!(!is_notice("The above copyright notice and this permission notice shall be included"));
        assert!(!is_notice("COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,"));
        assert!(!is_notice("      (c) You must retain, in the Source form of any Derivative Works"));
        let a = "MIT License\n\nCopyright (c) 2016 A\n\nPermission is hereby granted, free of charge,\nto any person.";
        let b = "The MIT License (MIT)\nCopyright 2020 B\nCopyright 2021 C\nPermission is hereby granted,\nfree of charge, to any person";
        assert_eq!(substance(a), substance(b));
        let c = "Copyright 2020 B\nPermission is hereby granted, free of charge, to any person, except for use in rockets.";
        assert_ne!(substance(a), substance(c), "a changed licence is its own");
        let list = Acknowledgements {
            groups: vec![
                LicenceGroup {
                    licence: "MIT".into(),
                    text: a.into(),
                    packages: vec![Package { name: "a".into(), version: "1".into(), ..Default::default() }],
                    ..Default::default()
                },
                LicenceGroup {
                    licence: "MIT".into(),
                    text: b.into(),
                    packages: vec![Package { name: "b".into(), version: "2".into(), ..Default::default() }],
                    ..Default::default()
                },
                LicenceGroup {
                    licence: "MIT".into(),
                    text: c.into(),
                    packages: vec![Package { name: "c".into(), version: "3".into(), ..Default::default() }],
                    ..Default::default()
                },
            ],
            note: String::new(),
        };
        let rows = coalesce(&list);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].texts.len(), 2);
        assert_eq!(rows[0].texts[0].packages, vec!["a 1", "b 2"]);
        assert!(rows[0].texts[1].text.contains("rockets") && !rows[0].texts[1].standard);
        assert_eq!(rows[0].packages[1].notices, vec!["Copyright 2020 B", "Copyright 2021 C"]);
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
        assert_eq!(v["engines"][0]["list"]["rows"][0]["licence"], "OFL-1.1");
        assert_eq!(v["engines"][1], json!({"id": "old", "name": "Old", "given": false}));
        assert_eq!(v["engines"][2]["error"], "timed out");
    }
}
