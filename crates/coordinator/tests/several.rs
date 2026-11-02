//! Step 4 (docs/research/15-multi-engine.md, section 3.5) on real engines: the text and array
//! engines, in process over one store, behind one coordinator. A graph across them
//! (Text → Code points in the text engine, → Arithmetic in the array engine) runs as one: one
//! reply, the right numbers, every node's records under the client's run; a rerun is `cached`
//! in both engines (the bound input is keyed by content); a failure upstream skips across the
//! border; a wire whose type can't cross, an unknown kind and a cycle are graph errors; cancel
//! reaches the fragment in flight and the fragments not yet sent; `run.get` gives the reply.

use std::time::{Duration, Instant};

use plumcraft_coordinator::Coordinator;
use plumcraft_engine::host::{Host, PollReply};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::PortValue;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::{Envelope, Event, NodeState};
use plumcraft_proto::reducer::Reducer;
use plumcraft_proto::run::{Phase, RunReply, RunSnapshot};
use plumcraft_proto::session::EngineState;
use serde_json::{Value, json};

fn root() -> std::path::PathBuf {
    let r = plumcraft_scratch::root().join("coordinator");
    std::fs::create_dir_all(&r).unwrap();
    r
}

fn host(make: fn() -> plumcraft_engine::Engine) -> std::sync::Arc<dyn Connection> {
    let h = Host::start(Box::new(move || Ok(make().with_store(Some(root())))), None);
    assert!(matches!(h.wait_ready(Duration::from_secs(5)), EngineState::Connected { .. }));
    std::sync::Arc::new(InProc(h))
}

fn coordinator() -> Coordinator {
    Coordinator::new().with_engine("text", host(plumcraft_text::engine)).with_engine("array", host(plumcraft_array::engine))
}

fn graph(nodes: Value, wires: &[(&str, &str, &str, &str)]) -> Value {
    let wires: Vec<Value> = wires.iter().map(|(a, ap, b, bp)| json!({"from": {"node": a, "port": ap}, "to": {"node": b, "port": bp}})).collect();
    json!({"version": 1, "nodes": nodes, "wires": wires})
}

/// Text "corgi" → Code points (text) → Arithmetic × 2 (array).
fn across(text: &str) -> Value {
    graph(
        json!({
            "lit": {"kind": "text.literal", "values": {"text": text}},
            "codes": {"kind": "text.codes"},
            "twice": {"kind": "array.arithmetic", "values": {"operation": "multiply", "constant": 2.0}},
        }),
        &[("lit", "text", "codes", "text"), ("codes", "codes", "twice", "a")],
    )
}

fn run(c: &Coordinator, id: &str, g: Value, extra: Value) -> (Vec<Envelope>, RunReply) {
    let mut req = json!({"v": 2, "op": "run", "run_id": id, "generation": 1, "graph": {"kind": "inline", "graph": g}, "memo": "use"});
    if let (Some(r), Some(x)) = (req.as_object_mut(), extra.as_object()) {
        r.extend(x.clone());
    }
    c.call("run", req).unwrap();
    collect(c, id)
}

fn collect(c: &Coordinator, id: &str) -> (Vec<Envelope>, RunReply) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let (mut since, mut all) = (0, Vec::new());
    while Instant::now() < deadline {
        let p: PollReply = serde_json::from_value(c.call("events.poll", json!({"since": since, "wait_ms": 300, "run_id": id})).unwrap()).unwrap();
        since = p.last_seq;
        for e in p.events {
            if let Event::RunReply(r) = &e.event {
                let r = (**r).clone();
                all.push(e);
                return (all, r);
            }
            all.push(e);
        }
    }
    panic!("no reply for {id}");
}

fn codes_output_port(c: &Coordinator) -> String {
    let s: SchemaReply = serde_json::from_value(c.call("schema", json!({})).unwrap()).unwrap();
    s.node("text.codes").unwrap().outputs[0].name.clone()
}

#[test]
fn the_merged_catalogue_names_each_nodes_engine() {
    let c = coordinator();
    let s: SchemaReply = serde_json::from_value(c.call("schema", json!({})).unwrap()).unwrap();
    assert_eq!(s.node("text.upper").map(|n| n.engines.clone()), Some(vec!["text".to_owned()]));
    assert_eq!(s.node("array.arithmetic").map(|n| n.engines.clone()), Some(vec!["array".to_owned()]));
    let again = c.call("schema", json!({"digest": s.engine.schema_digest})).unwrap();
    assert_eq!(again, json!({"unchanged": true}));
}

#[test]
fn a_graph_across_two_engines_runs_as_one() {
    let c = coordinator();
    let port = codes_output_port(&c);
    let g = across("corgi");
    let g: Value = serde_json::from_str(&g.to_string().replace("\"port\":\"codes\"", &format!("\"port\":\"{port}\""))).unwrap();
    let (events, reply) = run(&c, "x1", g.clone(), json!({"targets": ["twice"], "watch": [{"node": "twice", "port": "result"}]}));
    assert!(reply.ok, "{reply:?}");
    assert_eq!(reply.order, ["lit", "codes", "twice"]);
    for n in ["lit", "codes", "twice"] {
        assert_eq!(reply.nodes[n].state, NodeState::Done, "{n}");
    }
    // The numbers: each code point doubled.
    let PortValue::Blob { data } = &reply.nodes["twice"].outputs["result"] else { panic!("{:?}", reply.nodes["twice"].outputs) };
    let b = c.resolve(data).expect("the array engine's result");
    let got: Vec<f32> = b.bytes.as_chunks::<4>().0.iter().map(|x| f32::from_le_bytes(*x)).collect();
    let want: Vec<f32> = "corgi".chars().map(|ch| 2.0 * u32::from(ch) as f32).collect();
    assert_eq!(got, want);
    assert_eq!(reply.watch.len(), 1);
    assert!(reply.watch[0].error.is_none(), "{:?}", reply.watch[0]);
    // Every node's records came under the client's run, and reduce to the reply.
    let mut r = Reducer::new("x1", 1);
    for e in &events {
        assert_eq!((e.run_id.as_deref(), e.generation), (Some("x1"), Some(1)), "{e:?}");
        if let Event::Status(rec) = &e.event {
            r.apply("x1", 1, rec);
        }
    }
    for n in ["lit", "codes", "twice"] {
        assert!(events.iter().any(|e| matches!(&e.event, Event::Status(s) if s.node_id == n && s.state == NodeState::Done)), "{n}'s done record");
    }
    r.finish(&reply);
    let snap: RunSnapshot = serde_json::from_value(c.call("run.get", json!({"run_id": "x1"})).unwrap()).unwrap();
    assert_eq!(snap.phase, Phase::Finished);
    assert_eq!(snap.reply.as_ref().map(|r| &r.nodes), Some(&reply.nodes));

    // Again: cached in both engines; the array engine keys the bound input by its content.
    let (_, again) = run(&c, "x2", g, json!({"targets": ["twice"]}));
    for n in ["lit", "codes", "twice"] {
        assert_eq!(again.nodes[n].state, NodeState::Cached, "{n}");
    }
}

#[test]
fn a_failure_upstream_skips_across_the_border() {
    let c = coordinator();
    let port = codes_output_port(&c);
    let g = graph(
        json!({
            "lit": {"kind": "text.literal", "values": {"text": "x"}},
            "bad": {"kind": "text.fail"},
            "codes": {"kind": "text.codes"},
            "twice": {"kind": "array.arithmetic"},
        }),
        &[("lit", "text", "bad", "text"), ("bad", "text", "codes", "text"), ("codes", &port, "twice", "a")],
    );
    let (events, reply) = run(&c, "f1", g, json!({}));
    assert!(!reply.ok);
    assert_eq!(reply.nodes["bad"].state, NodeState::Error);
    assert_eq!(reply.nodes["codes"].state, NodeState::Skipped);
    assert_eq!(reply.nodes["twice"].state, NodeState::Skipped, "{reply:?}");
    assert!(reply.nodes["twice"].error.as_ref().is_some_and(|e| e.kind == NodeError::SKIPPED && e.message.contains("codes")));
    assert!(events.iter().any(|e| matches!(&e.event, Event::Status(s) if s.node_id == "twice" && s.state == NodeState::Skipped)));
}

#[test]
fn graph_errors() {
    let c = coordinator();
    let lit = json!({"kind": "text.literal", "values": {"text": "x"}});
    let cases = [
        (graph(json!({"lit": lit, "twice": {"kind": "array.arithmetic"}}), &[("lit", "text", "twice", "a")]), "crosses from text to array"),
        (graph(json!({"n": {"kind": "nobody.has"}}), &[]), "no connected engine"),
        (graph(json!({"n": {"kind": "text.upper", "engine": "array"}}), &[]), "has no node"),
        (graph(json!({"a": {"kind": "text.upper"}, "b": {"kind": "text.upper"}}), &[("a", "text", "b", "text"), ("b", "text", "a", "text")]), "cycle"),
    ];
    for (i, (g, why)) in cases.into_iter().enumerate() {
        let (_, reply) = run(&c, &format!("g{i}"), g, json!({}));
        let e = reply.error.unwrap_or_else(|| panic!("case {i}: no error"));
        assert_eq!(e.kind, NodeError::GRAPH, "case {i}");
        assert!(e.message.contains(why), "case {i}: {}", e.message);
    }
}

#[test]
fn cancel_reaches_every_fragment() {
    let c = coordinator();
    let port = codes_output_port(&c);
    let g = graph(
        json!({
            "lit": {"kind": "text.literal", "values": {"text": "slow"}},
            "slow": {"kind": "text.slow", "values": {"seconds": 5.0}},
            "codes": {"kind": "text.codes"},
            "twice": {"kind": "array.arithmetic"},
        }),
        &[("lit", "text", "slow", "text"), ("slow", "text", "codes", "text"), ("codes", &port, "twice", "a")],
    );
    let t = Instant::now();
    c.call("run", json!({"v": 2, "op": "run", "run_id": "c1", "generation": 1, "graph": {"kind": "inline", "graph": g}, "memo": "off"})).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    c.call("run.cancel", json!({"run_id": "c1"})).unwrap();
    let (_, reply) = collect(&c, "c1");
    assert!(t.elapsed() < Duration::from_secs(4), "cancelled, not finished: {:?}", t.elapsed());
    assert_eq!(reply.error.as_ref().map(|e| e.kind.as_str()), Some(NodeError::CANCELLED));
    assert_eq!(reply.nodes["twice"].state, NodeState::Cancelled, "never sent: {reply:?}");
    let snap: RunSnapshot = serde_json::from_value(c.call("run.get", json!({"run_id": "c1"})).unwrap()).unwrap();
    assert_eq!(snap.phase, Phase::Cancelled);
    // A cancel for a run not seen yet applies when it comes.
    c.call("run.cancel", json!({"run_id": "c2"})).unwrap();
    let (_, early) = run(&c, "c2", across("x"), json!({}));
    assert_eq!(early.error.map(|e| e.kind), Some(NodeError::CANCELLED.to_owned()));
}
