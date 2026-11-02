//! Workflow files with groups, nets and frames: three sample workflows, and a hand-made text
//! workflow carrying pack-only group fields (`checkpoints`) and other extras, and a net naming no
//! wire. They open with their groups when the catalogue knows every node kind; the extras are
//! ignored.

use plumcraft_graph::{FrameId, Graph, GroupId, NetKey, NodeId, PortRef};
use plumcraft_proto::catalogue::SchemaReply;
use serde_json::json;

/// The two node kinds the sample file uses, with the ports its wires and groups name.
fn demo_catalog() -> SchemaReply {
    serde_json::from_value(json!({"v": 2, "nodes": [
        {"id": "demo.load", "inputs": [
            {"name": "path", "type": "STRING", "default": "", "socket": "widget"},
            {"name": "align", "type": "BOOL", "default": true},
            {"name": "background", "type": "ENUM", "default": "black", "choices": ["black", "white"]},
            {"name": "image", "type": "IMAGE", "socket": "socket", "optional": true}],
         "outputs": [{"name": "rgb", "type": "IMAGE"}]},
        {"id": "demo.adjust", "inputs": [
            {"name": "array", "type": "IMAGE", "socket": "socket"},
            {"name": "curves", "type": "STRING", "default": "", "socket": "widget"},
            {"name": "input_range", "type": "ENUM", "default": "0-1", "choices": ["0-1", "0-255"]}],
         "outputs": [{"name": "curved", "type": "IMAGE"}]}
    ]}))
    .unwrap()
}

fn text_catalog() -> SchemaReply {
    serde_json::from_value(json!({"v": 2, "nodes": [
        {"id": "text.literal", "inputs": [{"name": "text", "type": "STRING", "default": "", "socket": "widget"}], "outputs": [{"name": "text", "type": "TEXT"}]},
        {"id": "text.slow", "inputs": [{"name": "text", "type": "TEXT", "socket": "socket"}, {"name": "seconds", "type": "FLOAT", "default": 1.0, "min": 0.0}],
         "outputs": [{"name": "text", "type": "TEXT"}]},
        {"id": "text.upper", "inputs": [{"name": "text", "type": "TEXT", "socket": "socket"}], "outputs": [{"name": "text", "type": "TEXT"}]},
        {"id": "text.words", "inputs": [{"name": "text", "type": "TEXT", "socket": "socket"}], "outputs": [{"name": "count", "type": "INT"}]}
    ]}))
    .unwrap()
}

#[test]
fn a_workflow_keeps_its_net_labels() {
    let g = Graph::from_json(include_str!("fixtures/workflow-nets.json")).unwrap();
    assert_eq!(g.nets.keys().map(String::as_str).collect::<Vec<_>>(), ["Lower", "Upper"]);
    assert_eq!(g.nets["Upper"].from, PortRef::new("n15", "out"));
    assert_eq!(g.net_wires(&NetKey::top("Lower")).count(), 2);
    let canvas = g.wire_into(&PortRef::new("n17", "base")).unwrap();
    assert_eq!(canvas.label.as_deref(), Some("Upper"));
    // saved and read back, they are as they were
    let back = Graph::from_json(&g.to_json().unwrap()).unwrap();
    assert_eq!((&back.nets, &back.wires), (&g.nets, &g.wires));
}

#[test]
fn a_workflow_keeps_its_frame_exactly() {
    let text = include_str!("fixtures/workflow-frame.json");
    let g = Graph::from_json(text).unwrap();
    let frame = g.frame(&FrameId::from("f1")).expect("its frame");
    assert_eq!(frame.title, "Sample: a framed graph");
    assert_eq!(frame.pos, [-40.0, -80.0]);
    let file: serde_json::Value = serde_json::from_str(text).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&g.to_json().unwrap()).unwrap();
    assert_eq!(saved["frames"], file["frames"]);
    assert_eq!(saved["next_frame"], file["next_frame"]);
    let mut out = Vec::new();
    lost(&file, &saved, String::new(), &mut out);
    assert!(out.is_empty(), "{out:?}");
    assert!(g.extra.is_empty(), "nothing about it is left unknown: {:?}", g.extra);
}

#[test]
fn a_workflow_opens_with_its_groups() {
    let g = Graph::from_json(include_str!("fixtures/workflow-groups.json")).unwrap();
    g.check(&demo_catalog()).unwrap();
    let grp = g.group(&GroupId::from("g1")).expect("its group");
    assert_eq!(grp.name, "LoadPrep");
    assert!(grp.collapsed);
    assert_eq!(grp.nodes.len(), 2);
    assert_eq!(grp.promoted[0].port, PortRef::new("n1", "image"));
    assert_eq!(grp.promoted[0].label, "image");
    assert!(grp.promoted[0].doc.starts_with("A picture"));
    assert_eq!(grp.outputs[0].from, PortRef::new("n2", "curved"));
    assert_eq!(g.next_group, 2);
    // saved and read back, the groups are as they were
    let back = Graph::from_json(&g.to_json().unwrap()).unwrap();
    assert_eq!(back.groups, g.groups);
    // without the node kinds the file still opens; check says what is missing
    assert!(g.check(&text_catalog()).is_err());
}

#[test]
fn pack_only_group_fields_are_kept_not_errors() {
    let text = include_str!("fixtures/text-groups-with-checkpoints.json");
    let g = Graph::from_json(text).unwrap();
    g.check(&text_catalog()).unwrap();
    let (g1, g2) = (g.group(&GroupId::from("g1")).unwrap(), g.group(&GroupId::from("g2")).unwrap());
    assert_eq!(g2.parent.as_ref(), Some(&GroupId::from("g1")));
    assert_eq!(g.group_members(&GroupId::from("g1")), [NodeId::from("n2"), NodeId::from("n3")].into());
    assert_eq!(g1.outputs[0].name, "shout");
    assert!(g1.checkpoints.contains(&NodeId::from("n3")) && g2.checkpoints.contains(&NodeId::from("n3")), "read, not only kept");
    // written back: checkpoints, read as the groups' own, and frames; a net with no wires goes;
    // bend points are kept and drawn
    let saved = g.to_json().unwrap();
    for kept in ["checkpoints", "frames"] {
        assert!(saved.contains(kept), "{kept} lost from {saved}");
    }
    assert!(!saved.contains("nets"), "{saved}");
    assert!(g.wires.iter().any(|w| !w.via.is_empty()), "bend points read");
    assert!(saved.contains("\"via\""), "and written back");
    assert_eq!(Graph::from_json(&saved).unwrap().groups, g.groups);
}

/// Paths where `saved` lacks or changes what `file` has.
fn lost(file: &serde_json::Value, saved: &serde_json::Value, at: String, out: &mut Vec<String>) {
    match (file, saved) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            for (k, v) in a {
                match b.get(k) {
                    Some(w) => lost(v, w, format!("{at}.{k}"), out),
                    // written only when not the default
                    None if v.is_null() || v == &json!({}) || v == &json!([]) || v == &json!(false) || (k == "next_group" && v == &json!(1)) => {}
                    None => out.push(format!("{at}.{k} is gone")),
                }
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) if a.len() == b.len() => {
            for (i, (v, w)) in a.iter().zip(b).enumerate() {
                lost(v, w, format!("{at}[{i}]"), out);
            }
        }
        (a, b) if a.as_f64().is_some() && a.as_f64() == b.as_f64() => {}
        (a, b) if a != b => out.push(format!("{at}: {a} became {b}")),
        _ => {}
    }
}

/// Every workflow opened and saved here keeps everything it had, slices above all.
/// `PLUMCRAFT_TEST_WORKFLOWS` (a colon-separated list of folders) widens it from the fixtures.
#[test]
fn a_file_survives_opening_and_saving() {
    let mut texts: Vec<(String, String)> = vec![
        ("nets".into(), include_str!("fixtures/workflow-nets.json").into()),
        ("frame".into(), include_str!("fixtures/workflow-frame.json").into()),
        ("groups".into(), include_str!("fixtures/workflow-groups.json").into()),
    ];
    for dir in std::env::var("PLUMCRAFT_TEST_WORKFLOWS").unwrap_or_default().split(':').filter(|d| !d.is_empty()) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            if e.path().extension().is_some_and(|x| x == "json") {
                texts.push((e.path().display().to_string(), std::fs::read_to_string(e.path()).unwrap()));
            }
        }
    }
    let (mut opened, mut slices) = (0, 0);
    for (name, text) in texts {
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        if file.get("wires").is_none() {
            continue;
        }
        let g = Graph::from_json(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let saved: serde_json::Value = serde_json::from_str(&g.to_json().unwrap()).unwrap();
        let mut out = Vec::new();
        lost(&file, &saved, String::new(), &mut out);
        assert!(out.is_empty(), "{name}: {out:#?}");
        opened += 1;
        slices += g.wires.iter().filter(|w| w.slice.is_some()).count();
    }
    println!("{opened} files, {slices} slices");
    assert!(opened >= 2);
}
