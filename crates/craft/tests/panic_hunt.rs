//! The panic hunt (AGENTS.md golden rule 1; spec 19 case 17) and run-twice purity (note 14,
//! M2.6 and R3), over every listed VectorCraft node, on a small traced corgi (the corgi at a
//! quarter size, so the slow live effects stay quick).
//!
//! - **Hunt:** every widget of every node gets the same hostile value at once (null, wrong types,
//!   huge and negative numbers, NaN as text, empty and huge strings, lists, objects), and `select`
//!   gets hostile selectors. Each node must end `done` or with a `config`, `op` or
//!   `missing_source` error: never a panic, and never `engine` (which would mean VectorCraft
//!   crashed or hung on a value the adapter let through).
//! - **Run twice:** every node marked pure gives the same output handles twice, memo off.

mod common;

use std::collections::BTreeMap;

use common::*;
use plumcraft_engine::exec::{self, Memo};
use plumcraft_engine::{CancelToken, Emit, Engine};
use plumcraft_proto::PortValue;
use plumcraft_proto::catalogue::{MemoScope, NodeDef};
use plumcraft_proto::event::{NodeState, StatusRecord};
use plumcraft_proto::run::{RunReply, RunRequest};
use serde_json::{Value, json};

struct Sink;

impl Emit for Sink {
    fn status(&mut self, _: StatusRecord) {}
}

fn engine(env: &Env) -> Engine {
    let mut m = env.manifest.clone();
    m.timeout_s = 60;
    plumcraft_craft::build(&m, &env.options()).unwrap().0
}

fn exec_run(e: &Engine, memo: &mut Memo, id: &str, g: Value, target: &str, mode: &str) -> RunReply {
    let plane = plumcraft_engine::data::Plane { store: e.store(), shm: true, urls: false, max_inline: 64 << 10 };
    let req: RunRequest = serde_json::from_value(json!({"v": 2, "op": "run", "run_id": id, "generation": 1,
        "graph": {"kind": "inline", "graph": g}, "targets": [target], "memo": mode}))
    .unwrap();
    exec::run(e, &req, &mut Sink, &CancelToken::default(), "hunt", memo, &plane)
}

/// The small base document: corgi → PNG at 0.25 → open → trace (6 Colors), then `node`.
fn on_base(env: &Env, def: &NodeDef, mut values: Value) -> Value {
    if def.inputs.iter().any(|p| p.name == "file")
        && let Some(v) = values.as_object_mut()
    {
        v.entry("file").or_insert_with(|| json!(env.corgi_str()));
    }
    let takes_doc = def.inputs.iter().any(|p| p.name == "doc");
    let how = env.manifest.smoke.get(&def.id).cloned().unwrap_or_default();
    if let Some(v) = values.as_object_mut() {
        for (k, x) in how.values {
            v.entry(k).or_insert(x);
        }
    }
    let mut nodes = json!({
        "a": {"kind": "vc.open", "values": {"file": env.corgi_str()}},
        "b": {"kind": "vc.export_png", "values": {"scale": 0.25}},
        "c": {"kind": "vc.open"},
        "d": {"kind": "vc.trace", "values": {"preset": "6 Colors"}},
        "n": {"kind": def.id, "values": values}
    });
    let mut wires: Vec<(String, String)> = vec![("a".into(), "b".into()), ("c".into(), "d".into())];
    let mut last = if how.raster { "c".to_owned() } else { "d".to_owned() };
    for (i, b) in how.before.iter().enumerate() {
        let id = format!("p{i}");
        nodes[&id] = json!({"kind": b, "values": {}});
        wires.push((last.clone(), id.clone()));
        last = id;
    }
    if takes_doc {
        wires.push((last, "n".into()));
    } else if let Some(o) = nodes.as_object_mut() {
        o.retain(|k, _| k == "n");
        wires.clear();
    }
    let mut w: Vec<(&str, &str, &str, &str)> = wires.iter().map(|(a, b)| (a.as_str(), "doc", b.as_str(), "doc")).collect();
    if takes_doc {
        w.push(("b", "image", "c", "image"));
    }
    graph(nodes, &w)
}

fn hostile() -> Vec<Value> {
    vec![
        Value::Null,
        json!("not a number"),
        json!("NaN"),
        json!(-1),
        json!(0),
        json!(1e300),
        json!(-1e300),
        json!(i64::MAX),
        json!(""),
        json!("x".repeat(100_000)),
        json!([]),
        json!([1, "two", null]),
        json!({}),
        json!({"evil": {"deeper": [1, 2, 3]}}),
        json!(true),
    ]
}

fn hostile_selects() -> Vec<Value> {
    [
        "",
        "nothing-like-a-selector",
        "ids:",
        "ids:-1",
        "ids:99999999999",
        "ids:1,a",
        "color:",
        "color:not-a-colour",
        "kind:",
        "kind:Hologram",
        "name:",
        "json:",
        "json:[]",
        "json:{\"kind\": 7}",
        "json:{\"nope\": 1}",
    ]
    .iter()
    .map(|s| json!(s))
    .chain([json!(5), json!(["all"]), json!("x".repeat(70_000))])
    .collect()
}

fn ok_kind(r: &RunReply) -> Result<(), String> {
    match r.nodes.get("n").map(|n| (n.state, n.error.clone())) {
        Some((NodeState::Done | NodeState::Cached, _)) => Ok(()),
        Some((NodeState::Error, Some(e))) if ["config", "op", "missing_source"].contains(&e.kind.as_str()) => Ok(()),
        other => Err(format!("{other:?}")),
    }
}

#[test]
fn every_node_survives_hostile_values() {
    let Some(env) = Env::new("hunt") else { return };
    let e = engine(&env);
    let mut memo = Memo::default();
    let mut bad: BTreeMap<String, String> = BTreeMap::new();
    let mut runs = 0;
    let t = std::time::Instant::now();
    for def in e.schema().nodes {
        let widgets: Vec<String> = def.inputs.iter().filter(|p| p.has_widget() && p.name != "select" && p.name != "file").map(|p| p.name.clone()).collect();
        for (i, v) in hostile().into_iter().enumerate() {
            let values: serde_json::Map<String, Value> = widgets.iter().map(|w| (w.clone(), v.clone())).collect();
            let r = exec_run(&e, &mut memo, &format!("{}-{i}", def.id), on_base(&env, &def, Value::Object(values)), "n", "use");
            runs += 1;
            if let Err(why) = ok_kind(&r) {
                bad.insert(format!("{} = {}", def.id, v.to_string().chars().take(40).collect::<String>()), why);
            }
        }
        if def.inputs.iter().any(|p| p.name == "select") {
            for (i, s) in hostile_selects().into_iter().enumerate() {
                let r = exec_run(&e, &mut memo, &format!("{}-s{i}", def.id), on_base(&env, &def, json!({"select": s})), "n", "use");
                runs += 1;
                if let Err(why) = ok_kind(&r) {
                    bad.insert(format!("{} select={}", def.id, s.to_string().chars().take(40).collect::<String>()), why);
                }
            }
        }
    }
    // Hostile files for the open node.
    for f in ["/etc/hosts", "/dev/zero", "/no/such/file.jpg", "", "/", "relative.jpg", "../../etc/passwd"] {
        let def = e.schema().nodes.into_iter().find(|n| n.id == "vc.open").unwrap();
        let r = exec_run(&e, &mut memo, "open-hostile", graph(json!({"n": {"kind": def.id, "values": {"file": f}}}), &[]), "n", "off");
        runs += 1;
        if let Err(why) = ok_kind(&r) {
            bad.insert(format!("vc.open file={f}"), why);
        }
    }
    eprintln!("panic hunt: {runs} runs in {:?}; {} not config/op/done", t.elapsed(), bad.len());
    for (k, v) in &bad {
        eprintln!("  {k}: {v}");
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// R3: purity is asserted by the manifest; every node it marks pure must give the same outputs
/// twice. Nodes of scope `none` (PDF, `vc.run`) are left out.
#[test]
fn every_pure_node_gives_the_same_bytes_twice() {
    let Some(env) = Env::new("twice") else { return };
    let e = engine(&env);
    let mut differ = Vec::new();
    let mut checked = 0;
    // the SDK's building blocks have their own tests (plumcraft-engine tests/vary.rs)
    for def in e.schema().nodes.into_iter().filter(|d| d.memo.scope != MemoScope::None && d.category.first().is_none_or(|c| c != "Building blocks")) {
        let handles = |id: &str| {
            let r = exec_run(&e, &mut Memo::default(), id, on_base(&env, &def, json!({})), "n", "off");
            let n = r.nodes.get("n").cloned();
            assert!(
                n.as_ref().is_some_and(|n| n.state == NodeState::Done),
                "{}: {:?}",
                def.id,
                r.nodes.iter().map(|(k, n)| (k.clone(), n.state, n.error.clone())).collect::<Vec<_>>()
            );
            r.nodes["n"]
                .outputs
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        match v {
                            PortValue::Blob { data } => data.handle.as_ref().map(ToString::to_string).unwrap_or_default(),
                            other => format!("{other:?}"),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>()
        };
        let (a, b) = (handles(&format!("{}-1", def.id)), handles(&format!("{}-2", def.id)));
        checked += 1;
        if a != b {
            differ.push(def.id.clone());
        }
    }
    eprintln!("run twice: {checked} pure nodes, {} differ: {differ:?}", differ.len());
    assert!(differ.is_empty(), "{differ:?}");
}
