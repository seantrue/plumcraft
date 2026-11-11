//! Every built-in pack (each `packs/<id>.toml`): it parses, its id is its file name, its demo is a
//! workflow, and nothing outward-facing or destructive is a node or passes its `run_node`
//! allowlist (sending, deleting, printing, signing, accounts, downloads, settings).

use plumcraft_craft::manifest::{BUILTIN, builtin, demo};

/// Words that mark a command as outward-facing or destructive beyond the document: sending,
/// deleting files or for good, printing, signing, accounts, downloads, the app's own settings. A
/// command id containing one is never a node's command, and is never allowed by `run_node` unless
/// its `deny` names it. (Deleting a slide or a layer inside the document is an edit, not this.)
const DENIED: &[&str] = &[
    "send",
    "deletepermanently",
    "deleteforgood",
    "emptyrecentlydeleted",
    "emptytrash",
    "file.delete",
    "print",
    "sign.",
    "sign_",
    "signature",
    "account",
    "download",
    "upload",
    "publish",
    "keychain",
    "addtophotos",
    "preferences",
    "app.settings",
    "app.options",
    // code execution bypasses the allowlist ("superscript" is a format, so not bare "script")
    "script.",
    "run_script",
    "runscript",
    "js_",
    "js.",
    "control_call",
    "run_shell",
    "shell.exec",
    "shell.run",
    // launches a browser or another app
    "link_open",
    "link.open",
    "editexternal",
    "open_external",
    "openexternal",
];

/// The denied word `id` contains, starting at a word boundary (the start, or after `.`, `_`,
/// `-`, `/`, or a lower-to-upper case change), so "design.theme" is not "sign." and
/// "photo.deletePermanently" is "deletepermanently".
fn denied_word(id: &str) -> Option<&'static str> {
    let lower = id.to_ascii_lowercase();
    let bytes = id.as_bytes();
    let boundary =
        |i: usize| i == 0 || matches!(bytes[i - 1], b'.' | b'_' | b'-' | b'/') || (bytes[i - 1].is_ascii_lowercase() && bytes[i].is_ascii_uppercase());
    DENIED.iter().copied().find(|w| lower.match_indices(w).any(|(i, _)| boundary(i)))
}

#[test]
fn denied_words_match_at_word_boundaries() {
    assert_eq!(denied_word("mail.send"), Some("send"));
    assert_eq!(denied_word("photo.deletePermanently"), Some("deletepermanently"));
    assert_eq!(denied_word("cmd_photo_deletePermanently"), Some("deletepermanently"));
    assert_eq!(denied_word("export.addToPhotos"), Some("addtophotos"));
    assert_eq!(denied_word("sign_document"), Some("sign_"));
    assert_eq!(denied_word("design.theme"), None);
    assert_eq!(denied_word("design.styleSet"), None);
    assert_eq!(denied_word("solid.shell"), None);
    assert_eq!(denied_word("slide.delete"), None);
    assert_eq!(denied_word("format.superscript"), None);
}

#[test]
fn every_built_in_pack_parses_with_its_file_name_as_id() {
    assert!(BUILTIN.contains(&"vectorcraft") && BUILTIN.contains(&"soundcraft"), "{BUILTIN:?}");
    for id in BUILTIN {
        let m = builtin(id).unwrap().unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(m.id, *id, "packs/{id}.toml has id {}", m.id);
        assert!(!m.name.is_empty(), "{id}");
        let labels = m.label_table();
        assert!(labels.skipped.is_empty(), "{id}: [labels] skipped {:?}", labels.skipped);
        if let Some(d) = demo(id) {
            let v: serde_json::Value = serde_json::from_str(d).unwrap_or_else(|e| panic!("{id}-demo.json: {e}"));
            assert!(v["nodes"].is_object(), "{id}-demo.json has no nodes");
        }
    }
}

#[test]
fn no_pack_exposes_outward_facing_or_destructive_commands() {
    for id in BUILTIN {
        let m = builtin(id).unwrap().unwrap();
        for n in &m.nodes {
            // its command, and its own action's tool or command (an export node's)
            let a = n.action.as_ref();
            for c in n.command.iter().chain(a.and_then(|a| a.command.as_ref())).chain(a.and_then(|a| a.tool.as_ref())) {
                assert!(denied_word(c).is_none(), "{id}: node {} runs {c} ({:?})", n.id, denied_word(c));
            }
        }
        // and the pack's shared actions
        let acts = &m.actions;
        for a in [Some(&acts.open), Some(&acts.save), acts.close_all.as_ref(), acts.clear_history.as_ref(), acts.export.as_ref()].into_iter().flatten() {
            for c in a.command.iter().chain(a.tool.iter()) {
                assert!(denied_word(c).is_none(), "{id}: an action runs {c} ({:?})", denied_word(c));
            }
        }
        if let Some(r) = &m.run_node {
            for a in &r.allow {
                // an allowed prefix may cover denied commands only when `deny` lists them; an
                // allowed id or prefix that itself names a denied verb is never right
                assert!(denied_word(a).is_none(), "{id}: run_node allows {a:?} ({:?})", denied_word(a));
            }
        }
        // a generated family is a node per matching command: the same rule as an allowlist
        for g in &m.generated_commands {
            for c in &g.commands {
                assert!(denied_word(c).is_none(), "{id}: generate_commands matches {c:?} ({:?})", denied_word(c));
            }
            assert!(!g.id_prefix.is_empty() && g.id_prefix.ends_with('.'), "{id}: generate_commands id_prefix {:?}", g.id_prefix);
        }
    }
}

/// The families the packs generate: PhotoCraft's filters and adjustments, EffectCraft's layer
/// commands; never the selection, and each exclusion says why.
#[test]
fn the_generated_families_are_declared() {
    let blocks = |id: &str| builtin(id).unwrap().unwrap().generated_commands;
    let pc: Vec<Vec<String>> = blocks("photocraft").into_iter().map(|g| g.commands).collect();
    assert_eq!(pc, [vec!["filter.".to_owned()], vec!["image.adjustments.".to_owned()]]);
    let ec = blocks("effectcraft");
    assert_eq!(ec.len(), 1);
    assert!(ec[0].deny.iter().any(|d| d == "layer.select"), "{:?}", ec[0].deny);
    for id in BUILTIN {
        for (node, why) in &builtin(id).unwrap().unwrap().exclude {
            assert!(why.len() > 10, "{id}: {node} is excluded without a reason");
        }
    }
}

#[test]
fn the_drafts_parse_and_are_safe_but_are_not_built_in() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("drafts");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "toml") {
            continue;
        }
        n += 1;
        let m = plumcraft_craft::manifest::Manifest::load(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        assert!(builtin(&m.id).is_none(), "{} is a draft and a built-in pack", m.id);
        for node in &m.nodes {
            if let Some(c) = &node.command {
                assert!(denied_word(c).is_none(), "draft {}: node {} runs {c}", m.id, node.id);
            }
        }
        for a in m.run_node.iter().flat_map(|r| &r.allow) {
            assert!(denied_word(a).is_none(), "draft {}: run_node allows {a:?}", m.id);
        }
    }
    assert!(n >= 2, "lightcraft and mailcraft drafts");
}

/// Node ids are unique across packs, and each pack has its own id prefix: a workflow saved with
/// one craft's engine never means another craft's node (FigCraft and FilmCraft both used `fc.`).
#[test]
fn node_ids_and_prefixes_are_unique_across_packs() {
    let mut owner: std::collections::BTreeMap<String, &str> = std::collections::BTreeMap::new();
    let mut prefixes: std::collections::BTreeMap<String, &str> = std::collections::BTreeMap::new();
    for id in BUILTIN {
        let m = builtin(id).unwrap().unwrap();
        let ids = m
            .nodes
            .iter()
            .map(|n| n.id.clone())
            .chain(m.run_node.iter().map(|r| r.id.clone()))
            .chain(m.generated.iter().map(|g| g.prefix.clone()))
            // a family's prefix once (PhotoCraft's two blocks share theirs)
            .chain(m.generated_commands.iter().map(|g| format!("{}*", g.id_prefix)).collect::<std::collections::BTreeSet<_>>());
        let mut mine = std::collections::BTreeSet::new();
        for nid in ids {
            if let Some(other) = owner.insert(nid.clone(), id) {
                panic!("{nid} is in {other} and {id}");
            }
            if let Some((p, _)) = nid.split_once('.') {
                mine.insert(p.to_owned());
            }
        }
        for p in mine {
            if let Some(other) = prefixes.insert(p.clone(), id) {
                panic!("prefix {p}. is used by {other} and {id}");
            }
        }
    }
}
