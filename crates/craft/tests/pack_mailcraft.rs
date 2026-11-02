//! The MailCraft pack draft (`crates/craft/drafts/mailcraft.toml`, not a built-in pack while
//! MailCraft refuses agent saves, so the editor doesn't offer a broken engine): it parses and keeps MailCraft's safety rules (sample mailbox only, only
//! `--allow mailbox`, nothing outward-facing); with `mailcraft-cli` on PATH the engine lists every
//! curated node. A chain cannot run yet: MailCraft refuses to let an agent save the mailbox file,
//! so the live case checks that block (see claude-debug/craft-pack-mailcraft.md) and runs the
//! chain once saving works.

mod common;

use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{Manifest, builtin};
use plumcraft_proto::event::NodeState;
use serde_json::json;

/// The draft, or `None` (and a message) where claude-debug's untracked files aren't present.
fn draft() -> Option<Manifest> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("drafts/mailcraft.toml");
    if !p.is_file() {
        eprintln!("SKIP: no {}", p.display());
        return None;
    }
    Some(Manifest::load(&p).unwrap())
}

const CURATED: &[&str] = &[
    "mc.open",
    "mc.sample",
    "mc.folders",
    "mc.messages",
    "mc.message",
    "mc.events",
    "mc.contacts",
    "mc.tasks",
    "mc.categorize",
    "mc.flag",
    "mc.mark_read",
    "mc.move",
    "mc.export_mbox",
];

#[test]
fn the_mailcraft_pack_parses_and_is_safe() {
    assert!(builtin("mailcraft").is_none(), "MailCraft stays out of the built-in packs until it can save");
    let Some(m) = draft() else { return };
    assert_eq!((m.id.as_str(), m.doc.wire.as_str()), ("mailcraft", "mailcraft.doc"));
    // Only the built-in sample mailbox, never the owner's, and only the mailbox-file class.
    assert_eq!(m.command, ["mailcraft-cli", "mcp", "--sample", "--allow", "mailbox"]);
    assert!(!m.command.iter().any(|a| a == "--file" || a == "--connect"), "never the owner's mailbox or app");
    let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, CURATED);
    for n in &m.nodes {
        let c = n.command.as_deref().unwrap_or_default();
        for bad in ["send", "compose", "reply", "forward", "delete", "account", "rules", "junk", "sync", "options", "signature", "openAttachment"] {
            assert!(!c.to_ascii_lowercase().contains(&bad.to_ascii_lowercase()), "{}: {c}", n.id);
        }
    }
    // run_node allows exact ids only: a prefix could let a new command through.
    let r = m.run_node.as_ref().unwrap();
    assert!(r.allow.iter().all(|a| !a.ends_with('.') && a.contains('.')), "{:?}", r.allow);
}

#[test]
fn mailcraft_lists_its_nodes_and_the_save_is_refused() {
    let Some(m) = draft() else { return };
    if m.executable().is_none() {
        eprintln!("SKIP: mailcraft-cli is not on PATH; {}", m.hint);
        return;
    }
    let opts = plumcraft_craft::Options { root: Some(craft_root("mailcraft")), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(&m, &opts).unwrap();
    eprintln!("MailCraft {}: {} commands; nodes {:?}; dropped {:?}", report.server_version, report.commands.len(), report.curated, report.dropped);
    assert!(report.dropped.is_empty(), "{:?}", report.dropped);
    assert_eq!(report.curated, CURATED);
    assert_eq!(report.hatch, ["mc.run"]);
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(60));
    let h: Conn = Box::new(plumcraft_host::InProc(h));

    let g = chain(&[("s", "mc.sample", json!({})), ("f", "mc.folders", json!({}))]);
    let r = go(&h, "mc", g, &["s", "f"], "off");
    let s = &r.nodes["s"];
    if s.state == NodeState::Error {
        let why = s.error.as_ref().map(|e| e.message.clone()).unwrap_or_default();
        assert!(why.contains("owner's to open and save"), "an unexpected failure: {why}");
        eprintln!("BLOCKED: MailCraft refuses to save the mailbox for an agent: {why}");
        return;
    }
    assert!(r.ok, "{:?}", states(&r));
    let folders = json_out(&r, "f", "result");
    assert!(folders.to_string().contains("Inbox"), "{folders}");
}
