//! The session against the real text engine's catalogue, and a run end to end through the
//! in-process host, as the editor drives it: commands in, requests out of the outbox, events
//! back in.

use std::time::{Duration, Instant};

use plumcraft_engine::host::{Host, PollReply};
use plumcraft_proto::event::NodeState;
use plumcraft_session::run::Outgoing;
use plumcraft_session::{Session, command_specs};
use serde_json::{Value, json};

fn session() -> Session {
    let mut s = Session::new(plumcraft_text::engine().schema());
    s.connected = true;
    s
}

fn add(s: &mut Session, kind: &str) -> String {
    s.execute("node.add", &json!({"kind": kind})).unwrap()["id"].as_str().unwrap().to_owned()
}

fn wire(s: &mut Session, a: &str, ap: &str, b: &str, bp: &str) {
    s.execute("wire.connect", &json!({"from": {"node": a, "port": ap}, "to": {"node": b, "port": bp}})).unwrap();
}

/// Net labels are only how wires look: drawing one makes nothing stale, and an engine finds the
/// labelled graph in its memo.
#[test]
fn a_net_label_changes_nothing_that_runs() {
    let mut s = session();
    let a = add(&mut s, "text.literal");
    let u = add(&mut s, "text.upper");
    s.execute("node.set", &json!({"node": a, "port": "text", "value": "hello"})).unwrap();
    wire(&mut s, &a, "text", &u, "text");
    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    s.execute("run.start", &json!({})).unwrap();
    pump(&mut s, &host);
    assert_eq!(s.run.nodes[&u].state, Some(NodeState::Done));
    s.execute("wire.label", &json!({"to": {"node": u, "port": "text"}, "name": "story"})).unwrap();
    s.execute("net.rename", &json!({"name": "story", "to": "tale"})).unwrap();
    assert!(s.run.nodes[&u].stale_since.is_none(), "nothing went stale");
    s.execute("run.start", &json!({})).unwrap();
    pump(&mut s, &host);
    assert_eq!(s.run.nodes[&u].state, Some(NodeState::Cached), "the same memo identity");
}

/// Frames too: an engine runs a graph with them, finds it in its memo, and they make nothing
/// stale.
#[test]
fn a_frame_changes_nothing_that_runs() {
    let mut s = session();
    let a = add(&mut s, "text.literal");
    let u = add(&mut s, "text.upper");
    s.execute("node.set", &json!({"node": a, "port": "text", "value": "hello"})).unwrap();
    wire(&mut s, &a, "text", &u, "text");
    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    s.execute("run.start", &json!({})).unwrap();
    pump(&mut s, &host);
    assert_eq!(s.run.nodes[&u].state, Some(NodeState::Done));
    s.execute("frame.add", &json!({"title": "all", "nodes": [a, u]})).unwrap();
    s.execute("frame.set", &json!({"frame": "f1", "color": "#3f8f5a"})).unwrap();
    s.execute("frame.resize", &json!({"frame": "f1", "size": [900, 900]})).unwrap();
    assert!(s.run.nodes[&u].stale_since.is_none(), "nothing went stale");
    s.execute("run.start", &json!({})).unwrap();
    pump(&mut s, &host);
    assert_eq!(s.run.nodes[&u].state, Some(NodeState::Cached), "the same memo identity");
}

/// Each run reports what the caches did, each cached node says where it came from and what it
/// would have cost, and the session adds them up.
#[test]
fn a_run_reports_its_caches() {
    let mut s = session();
    let a = add(&mut s, "text.literal");
    let u = add(&mut s, "text.upper");
    s.execute("node.set", &json!({"node": a, "port": "text", "value": "hello"})).unwrap();
    wire(&mut s, &a, "text", &u, "text");
    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    s.execute("run.start", &json!({})).unwrap();
    pump(&mut s, &host);
    let first = s.run.cache.last.clone().unwrap();
    assert_eq!((first.computed, first.hits()), (2, 0));
    s.execute("run.start", &json!({})).unwrap();
    pump(&mut s, &host);
    let second = s.run.cache.last.clone().unwrap();
    assert_eq!((second.computed, second.from_memory), (0, 2));
    assert_eq!(s.run.nodes[&u].source.as_deref(), Some("memory"));
    assert!(s.run.nodes[&u].computed_ms.is_some(), "what it would have cost");
    let r = s.execute("run.stats", &json!({})).unwrap();
    assert_eq!(r["runs"], 2);
    assert_eq!(r["session"]["computed"], 2);
    assert_eq!(r["session"]["from_memory"], 2);
    assert!(r["nodes"].as_array().unwrap().iter().any(|n| n["source"] == "memory"));
    assert_eq!(s.execute("run.cacheReport", &json!({})).unwrap(), json!({"open": true}));
    assert_eq!(s.execute("run.cacheReport", &json!({"open": false})).unwrap(), json!({"open": false}));
    assert!(s.execute("run.cacheReport", &json!({"open": "yes"})).is_err());
}

/// A slice changes what runs: it makes its node stale, with the reason.
#[test]
fn a_slice_is_a_stale_reason() {
    let graph = |slice: Value| {
        let wire = json!({"from": {"node": "a", "port": "p"}, "to": {"node": "b", "port": "text"}, "slice": slice});
        plumcraft_graph::Graph::from_json(&json!({"version": 1, "nodes": {"a": {"kind": "x"}, "b": {"kind": "x"}}, "wires": [wire]}).to_string()).unwrap()
    };
    let why = plumcraft_session::run::stale_reasons(&graph(Value::Null), &graph(json!("1:")));
    assert_eq!(why[&plumcraft_graph::NodeId::from("b")], ["input text slice changed: none → [1:]"]);
}

#[test]
fn edit_and_run_end_to_end() {
    let mut s = session();
    let a = add(&mut s, "text.literal");
    s.execute("node.set", &json!({"node": a, "port": "text", "value": "the corgi naps"})).unwrap();
    let u = add(&mut s, "text.upper");
    let w = add(&mut s, "text.words");
    wire(&mut s, &a, "text", &u, "text");
    wire(&mut s, &u, "text", &w, "text");
    s.execute("run.watch", &json!({"node": u})).unwrap();
    s.execute("run.start", &json!({})).unwrap();

    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    for out in s.run.take_outbox() {
        match out {
            Outgoing::Run(req) => {
                host.dispatch("run", serde_json::to_value(&*req).unwrap()).unwrap();
            }
            Outgoing::Cancel(id) => {
                host.dispatch("run.cancel", json!({"run_id": id})).unwrap();
            }
            Outgoing::Release(_) => {}
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut since = 0;
    while s.run.running && Instant::now() < deadline {
        let p: PollReply = serde_json::from_value(host.dispatch("events.poll", json!({"since": since, "wait_ms": 200})).unwrap()).unwrap();
        since = p.last_seq;
        for e in &p.events {
            s.run.apply_event(e, Instant::now());
        }
    }
    assert!(!s.run.running, "the reply arrived");
    for id in [&a, &u, &w] {
        assert_eq!(s.run.node(id).unwrap().state, Some(NodeState::Done), "{id}");
    }
    let d = s.run.watched[0].as_ref().expect("the watched display streamed in");
    assert_eq!(d.summary, "text, 14 characters, 1 lines");

    // An edit makes the edited node and everything downstream stale, with reasons.
    s.execute("node.set", &json!({"node": a, "port": "text", "value": "two corgis"})).unwrap();
    let stale: Vec<&str> = [&a, &u, &w].iter().filter(|id| s.run.node(id).unwrap().is_stale()).map(|s| s.as_str()).collect();
    assert_eq!(stale.len(), 3);
    assert!(s.run.node(&w).unwrap().stale_why[0].starts_with("upstream"));
}

#[test]
fn type_checks_and_failed_edits_leave_the_graph_alone() {
    let mut s = session();
    let w = add(&mut s, "text.words");
    let u = add(&mut s, "text.upper");
    let before = s.graph().clone();
    let err = s.execute("wire.connect", &json!({"from": {"node": w, "port": "count"}, "to": {"node": u, "port": "text"}})).unwrap_err();
    assert!(err.to_string().contains("type mismatch"), "{err}");
    assert_eq!(s.graph(), &before);
    assert!(!s.journal().last().unwrap().ok, "failed commands are journaled too");
    assert!(s.execute("node.add", &json!({"kind": "text.teleport"})).is_err());
    assert!(s.execute("nope.nope", &json!({})).is_err());
}

#[test]
fn runs_need_an_engine_and_a_graph() {
    let mut s = session();
    assert!(s.execute("run.start", &json!({})).unwrap_err().to_string().contains("empty"));
    add(&mut s, "text.literal");
    s.connected = false;
    assert!(s.execute("run.start", &json!({})).unwrap_err().to_string().contains("not connected"));
    assert!(s.execute("run.cancel", &json!({})).is_err(), "nothing to cancel");
}

/// Every command with adversarial parameters: errors are fine, panics are not (as the research software's
/// `panic_hunt`), and nothing leaves the graph with a problem it didn't have (a workflow opened
/// with problems reports them, and keeps them).
#[test]
fn panic_hunt() {
    let dir = plumcraft_scratch::root().join("e2-hunt");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = |name: &str, text: &str| {
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p.to_string_lossy().into_owned()
    };
    let garbage = file("garbage.json", "{\"version\": 1, \"nodes\": [}");
    let version = file("version.json", r#"{"version": 99, "nodes": {}}"#);
    let alien = file(
        "alien.json",
        r#"{"version": 1, "nodes": {"n1": {"kind": "vi.nope"}, "n2": {"kind": "text.upper"}}, "wires": [{"from": {"node": "n1", "port": "x"}, "to": {"node": "n2", "port": "text"}}]}"#,
    );
    let good = file("good.json", r#"{"version": 1, "nodes": {"n1": {"kind": "text.literal"}, "n2": {"kind": "text.slow"}}}"#);
    let grouped = file(
        "grouped.json",
        r#"{"version": 1, "nodes": {"n1": {"kind": "text.literal"}, "n2": {"kind": "text.slow"}},
            "groups": {"g1": {"name": "think", "nodes": ["n2"], "promoted": [{"port": {"node": "n2", "port": "seconds"}, "label": "seconds"}], "collapsed": true, "checkpoints": ["n2"]}}}"#,
    );
    let bad_groups = file(
        "bad-groups.json",
        r#"{"version": 1, "nodes": {"n1": {"kind": "text.literal"}}, "groups": {"g1": {"name": "x", "nodes": ["n9"], "parent": "g1"}}}"#,
    );
    let huge = dir.join("huge.json");
    std::fs::File::create(&huge).unwrap().set_len(plumcraft_session::MAX_WORKFLOW_BYTES + 1).unwrap();
    let huge = huge.to_string_lossy().into_owned();
    let folder = dir.to_string_lossy().into_owned();
    let missing = dir.join("missing.json").to_string_lossy().into_owned();
    let clip = r#"{"version": 1, "nodes": {"n1": {"kind": "text.literal", "pos": [1, 2]}, "n2": {"kind": "text.upper"}}, "wires": [{"from": {"node": "n1", "port": "text"}, "to": {"node": "n2", "port": "text"}}]}"#;
    let mut nasty = vec![
        json!({"path": garbage, "discard": true}),
        json!({"path": version, "discard": true}),
        json!({"path": huge, "discard": true}),
        json!({"path": folder, "discard": true}),
        json!({"path": missing, "discard": true}),
        json!({"path": "", "discard": true}),
        json!({"path": "\u{0}", "discard": 1}),
        json!({"path": ["a"], "discard": null}),
        json!({"path": dir.join("no/such/dir/x.json"), "discard": true}),
        json!({"path": dir.join("saved.json")}),
        json!({"text": clip, "pos": [-1e30, 1e30]}),
        json!({"text": "{\"version\": 1, \"nodes\": {\"n1\": {\"kind\": \"x\"}}}"}),
        json!({"text": "[".repeat(10_000)}),
        json!({"text": clip, "pos": "here"}),
        json!({"path": alien, "discard": true}),
        json!({"nodes": ["n1", "n2"]}),
        json!({"discard": true}),
        json!({"path": good, "discard": true}),
        json!({"text": clip}),
        json!({"path": bad_groups, "discard": true}),
        json!({"path": grouped, "discard": true}),
        json!({"path": grouped, "asGroup": "yes", "pos": [f64::MAX, 0]}),
        json!({"path": grouped, "asGroup": true, "name": "\u{0}"}),
        json!({"path": grouped, "pos": [1e30, -1e30]}),
    ];
    // group commands
    nasty.extend([
        json!({"nodes": "n1", "groups": {"g1": 1}}),
        json!({"nodes": [1, null], "groups": [null]}),
        json!({"nodes": [], "groups": ["g1", "g1", "g9"], "name": "outer"}),
        json!({"nodes": ["n1", "n2"], "groups": ["g1"]}),
        json!({"nodes": ["n1"], "name": "x".repeat(10_000)}),
        json!({"nodes": ["n1"], "name": "", "collapsed": 1}),
        json!({"group": "g1", "name": "x".repeat(10_000)}),
        json!({"group": "g1", "name": "\n"}),
        json!({"group": "g1", "port": {"node": "n2", "port": "seconds"}, "label": "seconds", "doc": 5}),
        json!({"group": "g1", "port": {"node": "n2", "port": "seconds"}, "label": "s", "doc": "d".repeat(100_000)}),
        json!({"group": "g1", "port": {"node": "n1", "port": "text"}, "label": "text"}),
        json!({"group": "g1", "port": {"node": "n2", "port": "mode.k"}, "label": "k"}),
        json!({"group": "g1", "port": "n2.seconds"}),
        json!({"group": "g1", "label": "seconds"}),
        json!({"group": "g1", "label": "seconds", "newLabel": "", "min": f64::MAX, "max": -f64::MAX}),
        json!({"group": "g1", "label": "seconds", "min": [], "doc": 7}),
        json!({"group": "g1", "label": "seconds", "min": -1e308, "max": 1e308}),
        json!({"group": "g1", "from": {"node": "n2", "port": "text"}, "name": "out"}),
        json!({"group": "g1", "from": {"node": "n2", "port": "nope"}, "name": "out"}),
        json!({"group": "g1", "from": {"node": "n1", "port": "text"}}),
        json!({"group": "g1", "name": "out"}),
        json!({"group": "g1", "collapsed": 1}),
        json!({"group": "g1", "collapsed": true, "pos": [3, 4]}),
        json!({"group": "g1", "pos": [f64::NAN, 4]}),
        json!({"group": "g9", "pos": [3, 4]}),
        json!({"group": ["g1"]}),
        json!({"group": "g1"}),
    ]);
    nasty.extend([
        Value::Null,
        json!(true),
        json!(-1),
        json!(1e308),
        json!("n1"),
        json!([]),
        json!({}),
        json!({"kind": null, "id": 3, "pos": "x"}),
        json!({"kind": "text.literal", "pos": [f64::MAX, -f64::MAX]}),
        json!({"kind": "text.literal", "pos": [1]}),
        json!({"kind": "\u{0}".repeat(1000)}),
        json!({"id": "n1", "pos": [0, 0, 0]}),
        json!({"node": "n1", "port": "text", "value": {"deep": [[[[]]]]}}),
        json!({"node": "n2", "port": "seconds", "value": "fast"}),
        json!({"from": {"node": "n1", "port": "text"}, "to": {"node": "n1", "port": "text"}}),
        json!({"from": {"node": "n1"}, "to": "n2"}),
        json!({"from": {"node": "n1", "port": "text"}, "to": {"node": "n2", "port": "text"}}),
        json!({"to": {"node": "n2", "port": "text"}}),
        json!({"to": {"node": 5, "port": []}}),
        json!({"nodes": ["n1", "n1", "n99"]}),
        json!({"nodes": "n1"}),
        json!({"nodes": [1, null]}),
        json!({"nodes": ["n2"], "offset": [1e39, 0], "keepInputs": "yes"}),
        json!({"node": "n1", "title": "x".repeat(10_000)}),
        json!({"node": "n1", "title": "\u{7}bell"}),
        json!({"nodes": ["n1"], "view": "sideways"}),
        json!({"targets": "n1", "watch": 5}),
        json!({"targets": ["n99"], "watch": [{"node": "n1"}]}),
        json!({"on": "yes"}),
        json!({"node": "n99"}),
    ]);
    // frames, and arranging nodes
    nasty.extend([
        json!({"frame": "f1", "pos": [3.0e38, -3.0e38], "size": [-1, 0]}),
        json!({"frame": ["f1"], "color": "#zzzzzz", "title": null}),
        json!({"frame": "f1", "title": "\u{202e}".repeat(500), "color": 7}),
        json!({"frame": "f1", "pos": [1, 1], "nodes": [null], "groups": "g1"}),
        json!({"frame": "f1", "pos": [1e7, 1e7], "nodes": ["n99"], "groups": ["g99"]}),
        json!({"nodes": ["n1", "n2"], "sizes": {"n1": [3.0e38, 3.0e38]}, "title": "t"}),
        json!({"nodes": ["n1", "n2"], "groups": [[]], "edge": "diagonal", "sizes": {"n1": [-1, 1e39]}}),
        json!({"nodes": ["n1", "n2"], "edge": "right", "sizes": {"n1": [3.0e38, 1], "n2": [3.0e38, 1]}}),
        json!({"nodes": ["n1", "n2", "n3"], "axis": "vertical", "sizes": []}),
        json!({"nodes": ["n1", "n2"], "groups": ["g1"], "axis": "horizontal"}),
    ]);
    let mut s = session();
    add(&mut s, "text.literal");
    add(&mut s, "text.slow");
    s.execute("group.create", &json!({"nodes": ["n2"], "name": "think"})).unwrap();
    // the problems the last workflow opened had
    let mut opened: std::collections::BTreeSet<String> = Default::default();
    for spec in command_specs() {
        for p in &nasty {
            let r = s.execute(spec.id, p);
            let _ = s.run.take_outbox();
            if spec.id == "graph.open" && r.is_ok() {
                opened = s.problems();
            }
            if let Some(e) = s.problems().into_iter().find(|e| !opened.contains(e)) {
                panic!("{} with {p} broke the graph: {e}", spec.id);
            }
        }
    }
    for id in ["edit.paste", "graph.saveAs", "group.create", "group.promote", "group.editPromoted", "graph.insert"] {
        assert!(command_specs().iter().any(|c| c.id == id), "{id}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Several engines (docs/research/15-multi-engine.md, decision E4): with a merged catalogue in
/// which two engines offer `text.upper`, placing one pins it to the first and the workflow
/// records the engine order; `node.engine` re-pins to an engine that offers the kind (not to one
/// that doesn't, nor to an unknown one), unpins with null, is an undo step and makes the node
/// stale; a kind only one engine offers isn't pinned.
#[test]
fn pins_with_several_engines() {
    let mut cat = plumcraft_text::engine().schema();
    for n in &mut cat.nodes {
        n.engines = if n.id == "text.upper" { vec!["text".into(), "py".into()] } else { vec!["text".into()] };
    }
    let mut s = Session::new(cat);
    s.engines.current = Some(plumcraft_session::EngineTarget::Choice { engine: "text".into(), mode: "inproc".into() });
    s.engines.added.push(plumcraft_session::EngineTarget::Choice { engine: "py".into(), mode: "python".into() });
    let lit = add(&mut s, "text.literal");
    let up = add(&mut s, "text.upper");
    let node = |s: &Session, id: &str| s.graph().nodes.iter().find(|(k, _)| k.0 == id).map(|(_, n)| n.clone()).unwrap();
    assert_eq!(node(&s, &lit).engine, None, "one engine offers it: not pinned");
    assert_eq!(node(&s, &up).engine.as_deref(), Some("text"), "pinned to the one that runs it now");
    assert_eq!(s.graph().engines, ["text", "py"]);
    let saved = serde_json::to_value(s.graph()).unwrap();
    assert_eq!(saved["engines"], json!(["text", "py"]));
    assert_eq!(saved["nodes"][&up]["engine"], "text");

    s.execute("node.engine", &json!({"id": up, "engine": "py"})).unwrap();
    assert_eq!(node(&s, &up).engine.as_deref(), Some("py"));
    let e = s.execute("node.engine", &json!({"id": lit, "engine": "py"})).unwrap_err().to_string();
    assert!(e.contains("doesn't offer"), "{e}");
    for bad in [json!({"id": up, "engine": 3}), json!({"id": up, "engine": ""}), json!({"id": "n99", "engine": "py"})] {
        assert!(s.execute("node.engine", &bad).is_err(), "{bad}");
    }
    s.execute("node.engine", &json!({"nodes": [up], "engine": null})).unwrap();
    assert_eq!(node(&s, &up).engine, None);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(node(&s, &up).engine.as_deref(), Some("py"), "an undo step");
    // Re-pinning makes the node stale, with the reason.
    let before = s.graph().clone();
    s.execute("node.engine", &json!({"id": up, "engine": "text"})).unwrap();
    let why = plumcraft_session::run::stale_reasons(&before, s.graph());
    assert!(why.iter().any(|(id, r)| id.0 == up && r.iter().any(|x| x.contains("engine changed (py → text)"))), "{why:?}");
}

/// Send what the session queued to `host` and feed the events back until the reply.
fn pump(s: &mut Session, host: &Host) {
    for out in s.run.take_outbox() {
        match out {
            Outgoing::Run(req) => {
                host.dispatch("run", serde_json::to_value(&*req).unwrap()).unwrap();
            }
            Outgoing::Cancel(id) => {
                host.dispatch("run.cancel", json!({"run_id": id})).unwrap();
            }
            Outgoing::Release(_) => {}
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut since = 0;
    while s.run.running && Instant::now() < deadline {
        let p: PollReply = serde_json::from_value(host.dispatch("events.poll", json!({"since": since, "wait_ms": 200})).unwrap()).unwrap();
        since = p.last_seq;
        for e in &p.events {
            s.run.apply_event(e, Instant::now());
        }
    }
    assert!(!s.run.running, "the reply arrived");
}

/// Pop-outs (docs/research/16-viewers.md): a pop-out follows its port beside the panel's watch;
/// one run brings both displays; the panel watching something else keeps the pop-out; the same
/// port twice brings the open one forward; `run.view` aims at a pop-out by id; a peek asks for a
/// port without touching the panel; closing drops the pop-out's watch; bad ids are errors.
#[test]
fn popouts_follow_their_ports_beside_the_panel() {
    let mut s = session();
    let a = add(&mut s, "text.literal");
    s.execute("node.set", &json!({"node": a, "port": "text", "value": "the corgi naps"})).unwrap();
    let u = add(&mut s, "text.upper");
    let w = add(&mut s, "text.words");
    wire(&mut s, &a, "text", &u, "text");
    wire(&mut s, &u, "text", &w, "text");
    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    s.execute("run.watch", &json!({"node": u})).unwrap();
    let p = s.execute("view.popout", &json!({"node": w})).unwrap();
    let id = p["id"].as_u64().unwrap();
    assert!(p["run_id"].is_string(), "a run asked for: {p}");
    pump(&mut s, &host);
    use plumcraft_session::run::WatchRole;
    assert_eq!(s.run.panel().and_then(|(_, d)| d).map(|d| d.summary.as_str()), Some("text, 14 characters, 1 lines"));
    let pop = s.run.entry(WatchRole::Popout(id)).and_then(|(_, d)| d).expect("the pop-out's display");
    assert_eq!(pop.value, Some(json!(3)), "{pop:?}");
    // The panel watches another port: the pop-out stays.
    s.execute("run.watch", &json!({"node": a})).unwrap();
    assert_eq!(s.run.entry(WatchRole::Popout(id)).map(|(w, _)| w.node.clone()), Some(w.clone()));
    assert_eq!(s.run.panel().map(|(x, _)| x.node.clone()), Some(a.clone()));
    // The same port again: the open one.
    assert_eq!(s.execute("view.popout", &json!({"node": w})).unwrap()["existing"], true);
    // A peek: the panel is untouched; a run brings the peeked display.
    s.execute("view.peek", &json!({"node": u})).unwrap();
    pump(&mut s, &host);
    assert!(s.run.entry(WatchRole::Peek).and_then(|(_, d)| d).is_some_and(|d| d.summary.starts_with("text")));
    assert_eq!(s.run.panel().map(|(x, _)| x.node.clone()), Some(a.clone()));
    let listed = s.execute("view.list", &Value::Null).unwrap();
    assert_eq!(listed["popouts"][0]["id"], id);
    s.execute("view.viewer", &json!({"popout": id, "viewer": "text.plain"})).unwrap();
    assert_eq!(s.views.popout(id).and_then(|o| o.viewer.as_deref()), Some("text.plain"));
    // Close: its watch goes, the others stay.
    s.execute("view.close", &json!({"id": id})).unwrap();
    assert!(s.run.entry(WatchRole::Popout(id)).is_none());
    assert!(s.run.panel().is_some() && s.run.entry(WatchRole::Peek).is_some());
    for (cmd, bad) in [
        ("view.close", json!({"id": id})),
        ("view.popout", json!({"node": "n404"})),
        ("view.viewer", json!({"popout": "x"})),
        ("view.viewer", json!({"viewer": "a b"})),
    ] {
        assert!(s.execute(cmd, &bad).is_err(), "{cmd} {bad}");
    }
}

/// Bend points (`wire.route`, lifted from the research software): one undo step, saved with the graph, and
/// nothing goes stale (the engine never sees them as a change); paste keeps them shifted with
/// the copies; Tidy straightens the wires it moves; bad lists are refused.
#[test]
fn routing_a_wire_is_one_undo_step_that_makes_nothing_stale() {
    let mut s = session();
    let a = add(&mut s, "text.literal");
    let u = add(&mut s, "text.upper");
    wire(&mut s, &a, "text", &u, "text");
    // a bend changes nothing that runs: no stale reasons
    let to = json!({"node": u, "port": "text"});
    let before = s.graph().clone();
    let r = s.execute("wire.route", &json!({"to": to, "via": [[120.4, 80], [200, 90]]})).unwrap();
    assert_eq!(r["via"], json!([[120, 80], [200, 90]]));
    assert!(plumcraft_session::run::stale_reasons(&before, s.graph()).is_empty(), "a bend isn't a change to what runs");
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.graph().wires[0].via.is_empty(), "one undo step");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.graph().wires[0].via, vec![[120, 80], [200, 90]]);
    // paste: the copy's wire bends where the copies went
    let text = s.graph().subgraph(&[a.as_str().into(), u.as_str().into()]).to_json().unwrap();
    let pasted = s.execute("edit.paste", &json!({"text": text})).unwrap();
    let nu = pasted["map"][&u].as_str().unwrap().to_owned();
    let shift = plumcraft_session::PASTE_OFFSET;
    let bent = s.graph().wires.iter().find(|w| w.to.node.0 == nu).map(|w| w.via.clone()).unwrap();
    assert_eq!(bent, vec![[120 + shift[0] as i32, 80 + shift[1] as i32], [200 + shift[0] as i32, 90 + shift[1] as i32]]);
    // straighten; bad lists refused
    s.execute("wire.route", &json!({"to": to, "via": []})).unwrap();
    assert!(s.graph().wires.iter().find(|w| w.to.node.0 == u).unwrap().via.is_empty());
    for bad in [
        json!({"to": to, "via": [[1]]}),
        json!({"to": to, "via": "x"}),
        json!({"to": to, "via": [[1e12, 0]]}),
        json!({"to": {"node": a, "port": "text"}, "via": [[0, 0]]}),
    ] {
        assert!(s.execute("wire.route", &bad).is_err(), "{bad}");
    }
    let many: Vec<Value> = (0..40).map(|i| json!([i, i])).collect();
    assert!(s.execute("wire.route", &json!({"to": to, "via": many})).is_err());
    // Tidy straightens a wire between nodes it moves
    s.execute("wire.route", &json!({"to": to, "via": [[500, 500]]})).unwrap();
    s.execute("graph.tidy", &json!({})).unwrap();
    assert!(s.graph().wires.iter().all(|w| w.via.is_empty()), "{:?}", s.graph().wires);
}

/// A graph's life: its runs carry its scope, edits keep it, and a new graph releases it, so the
/// engine can free what it kept. A graph that failed to open is no new life.
#[test]
fn runs_carry_the_graphs_life_and_a_new_graph_releases_it() {
    use plumcraft_session::run::Outgoing;
    let mut s = session();
    s.connected = true;
    let first = s.run.scope.clone();
    assert!(!first.is_empty());
    s.execute("node.add", &json!({"kind": "text.upper"})).unwrap();
    s.execute("run.start", &json!({})).unwrap();
    let out = s.run.take_outbox();
    let Some(Outgoing::Run(req)) = out.iter().find(|o| matches!(o, Outgoing::Run(_))) else { panic!("{out:?}") };
    assert_eq!(req.scope, first, "edits keep it");

    assert!(s.execute("graph.open", &json!({"path": "/no/such/file.json", "discard": true})).is_err());
    assert_eq!(s.run.scope, first);
    assert!(!s.run.take_outbox().iter().any(|o| matches!(o, Outgoing::Release(_))));

    s.execute("graph.new", &json!({"discard": true})).unwrap();
    assert_ne!(s.run.scope, first, "a new life");
    let out = s.run.take_outbox();
    assert!(matches!(out.as_slice(), [Outgoing::Release(old)] if *old == first), "{out:?}");
}

/// Removing a watched node used to leave its watch, and an engine refuses every run after that:
/// `no such node(s)`. Found by expanding a watched command in the real app.
#[test]
fn a_removed_node_takes_its_watches_with_it() {
    use plumcraft_session::run::Outgoing;
    let mut s = session();
    s.connected = true;
    let keep = s.execute("node.add", &json!({"kind": "text.upper"})).unwrap()["id"].as_str().unwrap().to_owned();
    let gone = s.execute("node.add", &json!({"kind": "text.upper"})).unwrap()["id"].as_str().unwrap().to_owned();
    s.execute("run.watch", &json!({"node": gone})).unwrap();
    let popout = s.execute("view.popout", &json!({"node": gone})).unwrap()["id"].as_u64().unwrap();
    let other = s.execute("view.popout", &json!({"node": keep})).unwrap()["id"].as_u64().unwrap();
    s.run.take_outbox();
    s.execute("node.remove", &json!({"id": gone})).unwrap();
    assert!(s.run.panel().is_none());
    assert!(s.views.popout(popout).is_none(), "its pop-out closed");
    assert!(s.views.popout(other).is_some(), "another stays");
    s.execute("run.start", &json!({})).unwrap();
    let out = s.run.take_outbox();
    let Some(Outgoing::Run(req)) = out.iter().find(|o| matches!(o, Outgoing::Run(_))) else { panic!("{out:?}") };
    assert_eq!(req.watch.iter().map(|w| w.node.as_str()).collect::<Vec<_>>(), [keep.as_str()]);
}
