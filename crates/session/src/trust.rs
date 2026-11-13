//! Workflows from elsewhere. A workflow is a program: Load Files reads what you can read, Write
//! Files writes what you can write, and an HTTP engine is sent the files a graph names. So a graph
//! read from a file nobody here approved doesn't run until someone has seen what it touches.
//!
//! - **What is trusted:** a graph started here, and a graph whose hash is in the application's
//!   trust file. Saving a trusted graph adds its hash, so editing and saving keeps the trust; a
//!   change made elsewhere doesn't match, and asks again. Saving an unapproved graph adds nothing.
//! - **The gate is in [`crate::run::RunState`]:** every run a session starts goes through
//!   `request`, which holds it while a review is due. A held run isn't sent, so no command can get
//!   round this. `graph.trust` approves it (the held run is dropped, not sent: nothing runs until
//!   the next run), and an agent on the control channel may call it: its
//!   token already carries the owner's authority.
//! - **Off without a trust file:** a library or test session has none. The command line's `run`
//!   and `--screenshot` have none on purpose, since naming the file there is the approval.
//!
//! The trust file is input like any other: one that is missing, too big or doesn't parse trusts
//! nothing, which costs a question and no more.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::Path;

use plumcraft_graph::{Graph, NodeId, PortRef};
use plumcraft_proto::catalogue::{MemoScope, SchemaReply};
use serde::Serialize;
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always};
use crate::{Result, Session, SessionError};

/// The largest trust file read.
const MAX_BYTES: u64 = 4 * 1024 * 1024;
/// The most hashes kept; past this the oldest in hash order go, which costs a question.
const MAX_ENTRIES: usize = 20_000;
/// The most entries of each kind a review lists.
const MAX_LISTED: usize = 200;
/// The most characters of a path or title a review shows.
const MAX_TEXT: usize = 300;

/// What identifies a graph: SHA-256 of its JSON. Empty when it can't be written, which matches
/// nothing. Not MD5: this is the one hash here that an attacker wants to collide.
pub fn graph_hash(graph: &Graph) -> String {
    let Ok(text) = graph.to_json() else { return String::new() };
    // Fixed SHA-256, not the switch (D9; hash-allowlist.txt): a security digest.
    commonkit::digest::fixed::raw(commonkit::digest::fixed::Fixed::Sha256, text.as_bytes())
}

/// The hashes in a trust file. Anything unreadable is nothing.
pub fn parse(text: &str) -> BTreeSet<String> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return BTreeSet::new() };
    let Some(list) = v.get("trusted").and_then(Value::as_array) else { return BTreeSet::new() };
    let hex = |s: &&str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
    list.iter().filter_map(Value::as_str).filter(hex).take(MAX_ENTRIES).map(str::to_owned).collect()
}

fn read(path: &Path) -> BTreeSet<String> {
    let Ok(f) = std::fs::File::open(path) else { return BTreeSet::new() };
    let mut text = String::new();
    if f.take(MAX_BYTES).read_to_string(&mut text).is_err() {
        return BTreeSet::new();
    }
    parse(&text)
}

/// Is `hash` in the trust file at `path`?
pub fn is_trusted(path: &Path, hash: &str) -> bool {
    !hash.is_empty() && read(path).contains(hash)
}

/// Add `hash` to the trust file at `path`: readable by its owner alone, replaced whole.
pub fn remember(path: &Path, hash: &str) -> std::io::Result<()> {
    if hash.is_empty() {
        return Err(std::io::Error::other("this graph can't be written, so it can't be remembered"));
    }
    let mut all = read(path);
    all.insert(hash.to_owned());
    let keep: Vec<&String> = all.iter().rev().take(MAX_ENTRIES).collect();
    let text = serde_json::to_string_pretty(&json!({"version": 1, "trusted": keep})).map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let written = (|| {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()
    })();
    if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// One thing in a graph that names a place on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Touch {
    pub node: String,
    /// The node's title, as shown on the canvas.
    pub title: String,
    pub port: String,
    /// The path as the graph has it. `None`: another node sets it, so nothing can be said here.
    pub path: Option<String>,
    /// The node that sets it, when one does.
    #[serde(rename = "setBy", skip_serializing_if = "Option::is_none")]
    pub set_by: Option<String>,
}

/// A node that does something besides computing its result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Action {
    pub node: String,
    pub title: String,
    pub kind: String,
}

/// What a graph touches: `graph.review`, and what the window asks about.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Review {
    /// Is a review due before this graph runs?
    pub pending: bool,
    pub nodes: usize,
    pub reads: Vec<Touch>,
    pub writes: Vec<Touch>,
    /// Nodes that act: an output, or a result that can't be reused.
    pub actions: Vec<Action>,
    /// The engines it uses, by the catalogue and the nodes' own say.
    pub engines: Vec<String>,
    /// Node types nothing here knows. What these do can't be said.
    pub unknown: Vec<String>,
    /// Entries left out of the lists above.
    pub more: usize,
}

/// Untrusted text as one line of bounded length: no control characters, no bidi overrides.
fn shown(text: &str) -> String {
    let hidden = |c: char| c.is_control() || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    let mut out: String = text.chars().take(MAX_TEXT).map(|c| if hidden(c) { '\u{fffd}' } else { c }).collect();
    if text.chars().count() > MAX_TEXT {
        out.push('…');
    }
    out
}

/// What `graph` touches, by the catalogue's path ports and flags. Paths are as the graph has them:
/// a relative one is relative to the workflow's folder.
pub fn review(graph: &Graph, catalog: &SchemaReply) -> Review {
    let mut r = Review { nodes: graph.nodes.len(), ..Review::default() };
    let mut engines: BTreeSet<String> = std::iter::once(shown(&catalog.engine.name)).filter(|n| !n.is_empty()).collect();
    let mut unknown = BTreeSet::new();
    for (id, node) in &graph.nodes {
        let title = title_of(graph, catalog, id);
        engines.extend(node.engine.iter().map(|e| shown(e)));
        let Some(def) = catalog.node(&node.kind) else {
            unknown.insert(shown(&node.kind));
            continue;
        };
        engines.extend(def.engines.iter().map(|e| shown(e)));
        if def.flags.output_node || def.memo.scope == MemoScope::None {
            r.actions.push(Action { node: id.0.clone(), title: title.clone(), kind: shown(&node.kind) });
        }
        for port in def.inputs.iter().filter(|p| p.path.is_some()) {
            let hint = port.path.as_ref();
            let writes = hint.is_some_and(|h| h.mode == "save" || (h.mode == "dir" && def.flags.output_node));
            let list = if writes { &mut r.writes } else { &mut r.reads };
            let wired = graph.wire_into(&PortRef::new(id.0.as_str(), port.name.as_str()));
            if let Some(w) = wired {
                let touch = Touch {
                    node: id.0.clone(),
                    title: title.clone(),
                    port: port.name.clone(),
                    path: None,
                    set_by: Some(title_of(graph, catalog, &w.from.node)),
                };
                list.push(touch);
                continue;
            }
            let value = node.values.get(&port.name).unwrap_or(&port.default);
            for path in value.as_str().unwrap_or_default().lines().filter(|l| !l.trim().is_empty()) {
                list.push(Touch { node: id.0.clone(), title: title.clone(), port: port.name.clone(), path: Some(shown(path)), set_by: None });
            }
        }
    }
    r.engines = engines.into_iter().collect();
    r.unknown = unknown.into_iter().collect();
    let over = r.reads.len().saturating_sub(MAX_LISTED) + r.writes.len().saturating_sub(MAX_LISTED) + r.actions.len().saturating_sub(MAX_LISTED);
    r.reads.truncate(MAX_LISTED);
    r.writes.truncate(MAX_LISTED);
    r.actions.truncate(MAX_LISTED);
    r.more = over;
    r
}

fn title_of(graph: &Graph, catalog: &SchemaReply, id: &NodeId) -> String {
    shown(&graph.node_title(id, |k| crate::display_name(catalog, k)).unwrap_or_else(|| id.0.clone()))
}

fn graph_review(s: &mut Session, _: &Value) -> Result<Value> {
    serde_json::to_value(s.review()).map_err(|e| SessionError::Params(e.to_string()))
}

fn graph_trust(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"trusted": true, "remembered": s.trust_graph()}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "graph.review",
            label: "Review Workflow",
            menu: &[],
            shortcut: None,
            params: "{} -> {pending, nodes, reads: [{node, title, port, path, setBy?}], writes: […], actions: [{node, title, kind}], engines, unknown, more} (what this graph touches; a path another node sets is null with setBy)",
            enabled: always,
            run: graph_review,
            journal: false,
        },
        CommandSpec {
            id: "graph.trust",
            label: "Trust Workflow",
            menu: &[],
            shortcut: None,
            params: "{} -> {trusted, remembered} (approve this graph: its hash goes in the trust file; a held run is dropped, so nothing runs until the next run)",
            enabled: always,
            run: graph_trust,
            journal: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trust_file_is_input_like_any_other() {
        let good = "a".repeat(64);
        assert_eq!(parse(&format!(r#"{{"trusted": ["{good}"]}}"#)).len(), 1);
        for bad in ["", "{", "[]", "null", r#"{"trusted": 3}"#, r#"{"trusted": ["short", 7, null]}"#, r#"{"trusted": {"a": 1}}"#] {
            assert!(parse(bad).is_empty(), "{bad}");
        }
        let text = format!(r#"{{"trusted": ["{good}"]}}"#);
        for cut in 0..text.len() {
            let _ = parse(&text[..cut]);
        }
    }

    #[test]
    fn what_a_review_shows_cannot_hide_anything() {
        assert_eq!(shown("a\nb\u{202e}c"), "a\u{fffd}b\u{fffd}c");
        assert!(shown(&"x".repeat(MAX_TEXT + 5)).ends_with('…'));
    }
}
