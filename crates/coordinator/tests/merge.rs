//! Step 3 (docs/research/15-multi-engine.md, sections 3.2 to 3.4), on the real text, audio and
//! array engines' catalogues: the merge keeps every node once and names its engine, records
//! every engine that offers a kind (the building blocks are in all three), unions wire types and
//! compat rules, and warns about a wire type declared with different media; a node's engine
//! follows its pin, then the only offerer, then the workflow's preference; a wire may cross
//! engines with a scalar, a core type, or a type both declare alike.

use std::collections::BTreeSet;

use plumcraft_coordinator::merge::{self, ENGINE_ID};
use plumcraft_proto::catalogue::{Media, SchemaReply, WireType};

fn parts() -> Vec<(String, SchemaReply)> {
    vec![
        ("text".into(), plumcraft_text::engine().schema()),
        ("audio".into(), plumcraft_audio::engine().schema()),
        ("array".into(), plumcraft_array::engine().schema()),
    ]
}

#[test]
fn every_node_once_with_its_engine() {
    let parts = parts();
    let m = merge::catalogue(&parts);
    let all: BTreeSet<&str> = parts.iter().flat_map(|(_, p)| p.nodes.iter().map(|n| n.id.as_str())).collect();
    let merged: Vec<&str> = m.schema.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(merged.len(), all.len(), "each kind once");
    assert_eq!(merged.iter().copied().collect::<BTreeSet<_>>(), all);
    for n in &m.schema.nodes {
        assert_eq!(&n.engines, &m.owners[&n.id], "{}: every engine that offers it, the first first", n.id);
    }
    assert_eq!(m.schema.node("text.upper").map(|n| n.engines.clone()), Some(vec!["text".to_owned()]));
    assert_eq!(m.schema.node("array.stack").map(|n| n.engines.clone()), Some(vec!["array".to_owned()]));
    // The building blocks are in every engine.
    let shared: Vec<&String> = m.owners.iter().filter(|(_, o)| o.len() == 3).map(|(k, _)| k).collect();
    assert!(!shared.is_empty(), "{:?}", m.owners);
    assert_eq!(m.schema.engine.id, ENGINE_ID);
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
}

#[test]
fn types_compat_and_identity_are_unions() {
    let parts = parts();
    let m = merge::catalogue(&parts);
    for (id, p) in &parts {
        for w in &p.wire_types {
            assert!(m.schema.wire_type(&w.id).is_some(), "{id}'s {}", w.id);
        }
        for c in p.compat_rules() {
            assert!(m.schema.accepts(&c.from, &c.to), "{id}'s {} → {}", c.from, c.to);
        }
    }
    // The digest and the salt change when any engine's do.
    let mut changed = parts.clone();
    changed[1].1.engine.salt.push_str("+new code");
    changed[2].1.engine.schema_digest.push('x');
    let again = merge::catalogue(&changed);
    assert_ne!(again.schema.engine.salt, m.schema.engine.salt);
    assert_ne!(again.schema.engine.schema_digest, m.schema.engine.schema_digest);
    assert_eq!(merge::catalogue(&parts).schema.engine.schema_digest, m.schema.engine.schema_digest, "stable");
}

#[test]
fn a_type_declared_differently_is_a_warning_and_the_first_is_kept() {
    let mut parts = parts();
    parts[2].1.wire_types.push(WireType { id: "TABLE".into(), media: Media::Image, ..Default::default() });
    let m = merge::catalogue(&parts);
    assert_eq!(m.warnings.len(), 1, "{:?}", m.warnings);
    assert!(m.warnings[0].contains("TABLE") && m.warnings[0].contains("array"), "{}", m.warnings[0]);
    assert_eq!(m.schema.wire_type("TABLE").map(|w| w.media), Some(Media::Table));
}

#[test]
fn which_engine_runs_a_node() {
    let m = merge::catalogue(&parts());
    let shared = m.owners.iter().find(|(_, o)| o.len() == 3).map(|(k, _)| k.clone()).unwrap();
    let none: &[String] = &[];
    assert_eq!(merge::owner("text.upper", None, none, &m.owners), Ok("text".into()));
    assert_eq!(merge::owner(&shared, None, none, &m.owners), Ok("text".into()), "the first added");
    assert_eq!(merge::owner(&shared, None, &["array".into(), "audio".into()], &m.owners), Ok("array".into()), "the workflow's preference");
    assert_eq!(merge::owner(&shared, Some("audio"), &["array".into()], &m.owners), Ok("audio".into()), "a pin wins");
    assert_eq!(merge::owner(&shared, Some(""), none, &m.owners), Ok("text".into()), "an empty pin is none");
    assert!(merge::owner("text.upper", Some("array"), none, &m.owners).unwrap_err().contains("has no node"));
    assert!(merge::owner("text.upper", Some("vectorcraft"), none, &m.owners).unwrap_err().contains("not connected"));
    assert!(merge::owner("nope.nothing", None, none, &m.owners).unwrap_err().contains("no connected engine"));
}

/// A core type means one thing everywhere: every engine that declares one declares it exactly
/// as `core_types` does (colour, media, thumb, views), so a value crosses unchanged.
#[test]
fn every_engine_declares_the_core_types_exactly() {
    use plumcraft_proto::catalogue::core_types;
    let mut seen = BTreeSet::new();
    for (engine, schema) in parts() {
        for w in schema.wire_types.iter().filter(|w| core_types::is_core(&w.id)) {
            assert_eq!(Some(w), core_types::wire_type(&w.id).as_ref(), "{engine} declares {} its own way", w.id);
            seen.insert(w.id.clone());
        }
    }
    for id in ["ARRAY", "SVG", "TEXT", "TABLE", "IMAGE", "AUDIO"] {
        assert!(seen.contains(id), "no engine here declares {id}");
    }
}

#[test]
fn what_may_cross_an_engine_border() {
    let parts = parts();
    let (t, au, ar) = (&parts[0], &parts[1], &parts[2]);
    let ok = |ty: &str, a: &(String, SchemaReply), b: &(String, SchemaReply)| merge::portable(ty, (&a.0, &a.1), (&b.0, &b.1));
    assert_eq!(ok("ARRAY", t, ar), Ok(()), "a core type");
    assert_eq!(ok("SVG", t, au), Ok(()), "a core type, even between engines that don't declare it");
    assert_eq!(ok("FLOAT", ar, t), Ok(()), "a scalar");
    assert_eq!(ok("TEXT", t, ar), Ok(()), "TEXT, TABLE, IMAGE and AUDIO are core types too");
    assert_eq!(ok("AUDIO", au, t), Ok(()));
    assert_eq!(ok("TEXT", t, t), Ok(()), "inside one engine its own rules decide");
    // A type that isn't core crosses only when both engines declare it with the same media.
    let with = |part: &(String, SchemaReply), media: Media| {
        let mut p = part.clone();
        p.1.wire_types.push(WireType { id: "LAYER".into(), media, ..Default::default() });
        p
    };
    let (tl, arl, aul) = (with(t, Media::Composite), with(ar, Media::Composite), with(au, Media::Bytes));
    assert_eq!(ok("LAYER", &tl, &arl), Ok(()), "declared alike by both");
    assert!(ok("LAYER", &tl, &aul).unwrap_err().contains("is Composite in text but Bytes in audio"));
    assert!(ok("LAYER", &tl, ar).unwrap_err().contains("array has no type LAYER"));
}
