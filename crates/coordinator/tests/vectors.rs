//! The shared graph vectors (`conformance/graphs/`, `docs/interface.md` 7.6): each a graph of
//! public building blocks and the item structure expected at named outputs. Every vector is
//! checked for form (it parses, its kinds are in the engines' catalogues, its expectations name
//! real outputs and are well formed), then run through the coordinator on the text, array and test
//! (`ports`) engines in process.
//!
//! A vector that `needs` a capability (today only `lists.nested`, nested lists, research note 27)
//! is held to its `expect` once every engine it runs on lists that capability. Until then it is
//! held to its `today` (what the rule before it gives), and it must **not** already give `expect`:
//! a vector that passes without the capability fails here, so the marker can't be left behind
//! when an engine starts nesting without saying so, and no vector is silently skipped.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use plumcraft_coordinator::Coordinator;
use plumcraft_engine::host::{Host, PollReply};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::PortValue;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::event::Event;
use plumcraft_proto::run::RunReply;
use plumcraft_proto::session::EngineState;
use serde::Deserialize;
use serde_json::{Value, json};

/// The capabilities a vector may need.
const KNOWN_NEEDS: &[&str] = &["lists.nested"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vector {
    name: String,
    about: String,
    #[serde(default)]
    needs: Vec<String>,
    graph: Value,
    expect: BTreeMap<String, Expect>,
    #[serde(default)]
    today: Option<BTreeMap<String, Expect>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    shape: Vec<u64>,
    #[serde(default)]
    values: Option<Value>,
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/graphs")
}

fn vectors() -> Vec<(String, Vector)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir()).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let file = p.file_name().unwrap().to_string_lossy().into_owned();
        let v: Vector = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap_or_else(|e| panic!("{file}: {e}"));
        out.push((file, v));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn root() -> PathBuf {
    let r = plumcraft_scratch::root().join("vectors");
    std::fs::create_dir_all(&r).unwrap();
    r
}

fn host(make: fn() -> plumcraft_engine::Engine) -> std::sync::Arc<dyn Connection> {
    let h = Host::start(Box::new(move || Ok(make().with_store(Some(root())))), None);
    assert!(matches!(h.wait_ready(Duration::from_secs(5)), EngineState::Connected { .. }));
    std::sync::Arc::new(InProc(h))
}

/// How an engine is made.
type Make = fn() -> plumcraft_engine::Engine;

/// The engines the vectors run on, by id.
fn engines() -> Vec<(&'static str, Make)> {
    vec![("text", plumcraft_text::engine as Make), ("array", plumcraft_array::engine), ("ports", plumcraft_ports::engine)]
}

fn coordinator() -> Coordinator {
    engines().into_iter().fold(Coordinator::new(), |c, (id, make)| c.with_engine(id, host(make)))
}

fn run(c: &Coordinator, id: &str, graph: &Value, targets: &[String]) -> RunReply {
    let req = json!({"v": 2, "op": "run", "run_id": id, "generation": 1, "graph": {"kind": "inline", "graph": graph}, "memo": "off", "targets": targets});
    c.call("run", req).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut since = 0;
    while Instant::now() < deadline {
        let p: PollReply = serde_json::from_value(c.call("events.poll", json!({"since": since, "wait_ms": 300, "run_id": id})).unwrap()).unwrap();
        since = p.last_seq;
        for e in p.events {
            if let Event::RunReply(r) = e.event {
                return *r;
            }
        }
    }
    panic!("{id}: no reply");
}

/// A value's item structure: a list's length, then its items' common structure (a value that
/// isn't a list has none). A ragged list says so.
fn shape(v: &PortValue) -> Result<Vec<u64>, String> {
    let PortValue::List { items, .. } = v else { return Ok(Vec::new()) };
    let mut out = vec![items.len() as u64];
    let inner: Vec<Vec<u64>> = items.iter().map(shape).collect::<Result<_, _>>()?;
    match inner.first() {
        Some(first) if inner.iter().all(|s| s == first) => out.extend(first),
        Some(_) => return Err(format!("ragged: {inner:?}")),
        None => {}
    }
    Ok(out)
}

/// A value as plain JSON: lists as arrays, json as itself.
fn plain(v: &PortValue) -> Value {
    match v {
        PortValue::List { items, .. } => Value::Array(items.iter().map(plain).collect()),
        PortValue::Json { value } => value.clone(),
        other => json!(format!("{other:?}")),
    }
}

/// The shape of an expected `values`, for its own check.
fn json_shape(v: &Value) -> Vec<u64> {
    match v {
        Value::Array(a) => {
            let mut out = vec![a.len() as u64];
            if let Some(first) = a.first() {
                out.extend(json_shape(first));
            }
            out
        }
        _ => Vec::new(),
    }
}

fn check_form(file: &str, v: &Vector, schema: &SchemaReply) {
    assert!(!v.name.trim().is_empty() && !v.about.trim().is_empty(), "{file}: a name and an about");
    for n in &v.needs {
        assert!(KNOWN_NEEDS.contains(&n.as_str()), "{file}: unknown need {n:?}");
    }
    let nodes = v.graph["nodes"].as_object().unwrap_or_else(|| panic!("{file}: graph.nodes"));
    for (id, n) in nodes {
        let kind = n["kind"].as_str().unwrap_or_else(|| panic!("{file}: {id} has no kind"));
        let def = schema.node(kind).unwrap_or_else(|| panic!("{file}: {id}: no engine here offers {kind}"));
        if let Some(e) = n["engine"].as_str() {
            assert!(def.engines.iter().any(|x| x == e), "{file}: {id}: {e} doesn't offer {kind} ({:?})", def.engines);
        }
    }
    for w in v.graph["wires"].as_array().unwrap_or_else(|| panic!("{file}: graph.wires")) {
        for end in ["from", "to"] {
            let id = w[end]["node"].as_str().unwrap_or_default();
            assert!(nodes.contains_key(id), "{file}: a wire names {id:?}");
        }
        if let Some(s) = w.get("slice").and_then(Value::as_str) {
            assert!(plumcraft_proto::slice::parse(s).is_ok(), "{file}: slice {s:?}");
        }
    }
    let mut sets = vec![("expect", &v.expect)];
    if let Some(t) = &v.today {
        sets.push(("today", t));
    }
    for (which, set) in sets {
        assert!(!set.is_empty(), "{file}: {which} is empty");
        for (at, e) in set {
            let (node, port) = at.split_once('.').unwrap_or_else(|| panic!("{file}: {which} {at:?} isn't node.port"));
            let kind = nodes.get(node).and_then(|n| n["kind"].as_str()).unwrap_or_else(|| panic!("{file}: {which} names {node:?}"));
            assert!(schema.node(kind).is_some_and(|d| d.outputs.iter().any(|p| p.name == port)), "{file}: {kind} has no output {port:?}");
            if let Some(values) = &e.values {
                assert_eq!(json_shape(values), e.shape, "{file}: {which} {at}: its values don't have its shape");
            }
        }
    }
    if !v.needs.is_empty() {
        assert!(v.today.is_some(), "{file}: a vector that needs {:?} says what the rule before it gives (`today`)", v.needs);
    }
}

fn holds(reply: &RunReply, set: &BTreeMap<String, Expect>) -> Result<(), String> {
    for (at, e) in set {
        let (node, port) = at.split_once('.').unwrap_or_default();
        let r = reply.nodes.get(node).ok_or_else(|| format!("{node} didn't run"))?;
        let v = r.outputs.get(port).ok_or_else(|| format!("{at}: no output ({:?}, {:?})", r.state, r.error))?;
        let got = shape(v)?;
        if got != e.shape {
            return Err(format!("{at}: shape {got:?}, not {:?}", e.shape));
        }
        if let Some(want) = &e.values
            && &plain(v) != want
        {
            return Err(format!("{at}: values {}, not {want}", plain(v)));
        }
    }
    Ok(())
}

#[test]
fn every_vector_is_well_formed_and_holds_on_the_rust_sdk_engines() {
    let all = vectors();
    assert!(all.len() >= 10, "the vectors of research note 27, section 7: {}", all.len());
    let c = coordinator();
    let schema: SchemaReply = serde_json::from_value(c.call("schema", json!({})).unwrap()).unwrap();
    let caps: BTreeMap<&str, plumcraft_proto::Caps> = engines().into_iter().map(|(id, make)| (id, make().caps())).collect();
    let mut failures = Vec::new();
    for (file, v) in &all {
        check_form(file, v, &schema);
        let used: Vec<&str> = v.graph["nodes"].as_object().into_iter().flatten().filter_map(|(_, n)| n["engine"].as_str()).collect();
        let ready = v.needs.iter().all(|need| used.iter().all(|e| caps.get(e).is_some_and(|c| c.has(need))));
        let targets: Vec<String> = v.expect.keys().filter_map(|k| k.split_once('.').map(|(n, _)| n.to_owned())).collect();
        let reply = run(&c, file, &v.graph, &targets);
        if ready {
            if let Err(e) = holds(&reply, &v.expect) {
                failures.push(format!("{file}: {e}"));
            }
        } else {
            let today = v.today.as_ref().unwrap_or(&v.expect);
            if let Err(e) = holds(&reply, today) {
                failures.push(format!("{file}: today's rule: {e}"));
            }
            if holds(&reply, &v.expect).is_ok() {
                failures.push(format!("{file}: already gives `expect` without {:?}: advertise the capability, or drop the marker", v.needs));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
