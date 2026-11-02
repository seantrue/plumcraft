//! The array engine's catalogue: every node under Array, with a title and a description, unique
//! ids, ARRAY ports typed by the core type, every wire type declared; and the demo workflow
//! runs to the end.

mod common;

use std::collections::BTreeSet;

use plumcraft_proto::catalogue::core_types;

#[test]
fn every_node_is_described_and_typed() {
    let e = plumcraft_array::engine();
    let schema = e.schema();
    let declared: BTreeSet<&str> = schema.wire_types.iter().map(|w| w.id.as_str()).collect();
    let mut ids = BTreeSet::new();
    let arrays: Vec<_> = schema.nodes.iter().filter(|n| n.id.starts_with("array.")).collect();
    assert!(arrays.len() >= 45, "{} array nodes", arrays.len());
    for n in &arrays {
        assert!(ids.insert(n.id.clone()), "{} twice", n.id);
        assert_eq!(n.category.first().map(String::as_str), Some("Array"), "{}", n.id);
        assert!(!n.display.is_empty() && n.description.len() > 20, "{}: describe it", n.id);
        for p in n.inputs.iter().chain(&n.outputs) {
            assert!(declared.contains(p.ty.as_str()), "{}.{}: {} isn't declared", n.id, p.name, p.ty);
        }
        if std::env::var_os("PLUMCRAFT_PRINT_CATALOGUE").is_some() {
            let ports = |ps: &[plumcraft_proto::catalogue::Port]| ps.iter().map(|p| format!("{}:{}", p.name, p.ty)).collect::<Vec<_>>().join(" ");
            println!("{} [{}] in: {} | out: {}", n.id, n.category.join("/"), ports(&n.inputs), ports(&n.outputs));
        }
    }
    let array = schema.wire_types.iter().find(|w| w.id == core_types::ARRAY).unwrap();
    assert_eq!(array, &core_types::array(), "the core ARRAY type, exactly");
}

#[test]
fn the_demo_runs() {
    let dir = common::root("demo");
    let e = common::engine(&dir);
    let wf: serde_json::Value = serde_json::from_str(plumcraft_array::DEMO_WORKFLOW).unwrap();
    let nodes = wf["nodes"].clone();
    let wires: Vec<(String, String, String, String)> = wf["wires"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            (
                w["from"]["node"].as_str().unwrap().into(),
                w["from"]["port"].as_str().unwrap().into(),
                w["to"]["node"].as_str().unwrap().into(),
                w["to"]["port"].as_str().unwrap().into(),
            )
        })
        .collect();
    let wires: Vec<(&str, &str, &str, &str)> = wires.iter().map(|(a, b, c, d)| (a.as_str(), b.as_str(), c.as_str(), d.as_str())).collect();
    let targets: Vec<String> = nodes.as_object().unwrap().keys().cloned().collect();
    let targets: Vec<&str> = targets.iter().map(String::as_str).collect();
    common::ok(common::run(&e, nodes, &wires, &targets));
}
