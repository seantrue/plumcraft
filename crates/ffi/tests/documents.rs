//! Several documents on one core (`docs/shell-interface.md` 4.3; N1 slice 7): `document.open`,
//! `document.list`, `document.close`, a request's `doc`, one engine compiled in shared by the
//! documents (decision 11B), and `plum_open` returning before its engine has connected.

mod common;

use std::time::{Duration, Instant};

use common::{Client, store_root, take};
use plumcraft_ffi::plum_poll;
use serde_json::{Value, json};

/// What the feed brought besides replies.
#[derive(Default)]
struct Feed {
    asks: Vec<Value>,
    changed: Vec<Value>,
    events: Vec<Value>,
}

/// Send a request (to document `doc`, when given) and poll until its reply comes.
fn ask(c: &mut Client, feed: &mut Feed, doc: Option<Value>, method: &str, params: Value) -> Value {
    let mut req = json!({"method": method, "params": params});
    if let Some(d) = doc {
        req["doc"] = d;
    }
    let id = c.send(&req);
    wait(c, feed, id)
}

fn wait(c: &mut Client, feed: &mut Feed, id: u64) -> Value {
    let t0 = Instant::now();
    loop {
        poll(c, feed);
        if let Some(r) = c.replies.remove(&id) {
            return r;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "no reply to request {id}");
        c.wait_wake(Duration::from_millis(100));
    }
}

fn poll(c: &mut Client, feed: &mut Feed) {
    let bytes = take(plum_poll(c.core));
    if bytes.is_empty() {
        return;
    }
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    for r in v["replies"].as_array().unwrap() {
        c.replies.insert(r["id"].as_u64().unwrap(), r.clone());
    }
    feed.events.extend(v["events"].as_array().cloned().unwrap_or_default());
    feed.asks.extend(v.get("asks").and_then(Value::as_array).cloned().unwrap_or_default());
    feed.changed.extend(v.get("changed").and_then(Value::as_array).cloned().unwrap_or_default());
}

/// `command.run` on document `doc`, asserting it worked: its result.
fn run(c: &mut Client, feed: &mut Feed, doc: u64, command: &str, params: Value) -> Value {
    let r = ask(c, feed, Some(json!(doc)), "command.run", json!({"command": command, "params": params}));
    assert_eq!(r["ok"], true, "doc {doc} {command}: {r}");
    r["result"].clone()
}

/// literal → upper case on document `doc`, watched: the upper node's id.
fn text_graph(c: &mut Client, feed: &mut Feed, doc: u64, text: &str) -> String {
    let lit = run(c, feed, doc, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}))["id"].as_str().unwrap().to_owned();
    run(c, feed, doc, "node.set", json!({"node": lit, "port": "text", "value": text}));
    let up = run(c, feed, doc, "node.add", json!({"kind": "text.upper", "pos": [260, 0]}))["id"].as_str().unwrap().to_owned();
    run(c, feed, doc, "wire.connect", json!({"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}}));
    up
}

/// The open documents, as `document.list` lists them.
fn docs(c: &mut Client, feed: &mut Feed) -> Vec<Value> {
    let r = ask(c, feed, None, "document.list", json!({}));
    r["result"]["docs"].as_array().cloned().unwrap_or_default()
}

/// What the Watched panel of document `doc` shows, once its run is done.
fn watched_text(c: &mut Client, feed: &mut Feed, doc: u64) -> String {
    let done = ask(c, feed, Some(json!(doc)), "ui.waitRun", json!({"timeout": 20}));
    assert_eq!((done["ok"].as_bool(), done["result"]["running"].as_bool()), (Some(true), Some(false)), "doc {doc}: {done}");
    let w = ask(c, feed, Some(json!(doc)), "spike.waitRun", json!({"timeout_ms": 20_000}));
    w["result"]["watch"][0]["renditions"].as_array().into_iter().flatten().filter_map(|r| r.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("")
}

#[test]
fn two_documents_edit_undo_and_run_apart_on_one_engine() {
    let mut c = Client::open(&json!({"store_root": store_root("docs-two")}));
    let mut feed = Feed::default();
    let first = docs(&mut c, &mut feed)[0]["doc"].as_u64().unwrap();
    let r = ask(&mut c, &mut feed, None, "document.open", json!({}));
    assert_eq!(r["ok"], true, "{r}");
    let second = r["result"]["doc"].as_u64().unwrap();
    assert_ne!(first, second);
    // independent graphs
    let up1 = text_graph(&mut c, &mut feed, first, "one corgi");
    let up2 = text_graph(&mut c, &mut feed, second, "two corgis");
    run(&mut c, &mut feed, second, "node.add", json!({"kind": "text.literal", "pos": [0, 300]}));
    // (nodes, wires)
    let graph = |c: &mut Client, feed: &mut Feed, doc: u64| {
        let g = ask(c, feed, Some(json!(doc)), "graph.get", json!({}))["result"].clone();
        (g["nodes"].as_object().map_or(0, |n| n.len()), g["wires"].as_array().map_or(0, Vec::len))
    };
    assert_eq!((graph(&mut c, &mut feed, first), graph(&mut c, &mut feed, second)), ((2, 1), (3, 1)));
    // undo in one leaves the other
    run(&mut c, &mut feed, second, "edit.undo", json!({}));
    assert_eq!((graph(&mut c, &mut feed, first), graph(&mut c, &mut feed, second)), ((2, 1), (2, 1)));
    // a request without `doc` acts on the first document
    let r = ask(&mut c, &mut feed, None, "command.run", json!({"command": "edit.undo", "params": {}}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!((graph(&mut c, &mut feed, first), graph(&mut c, &mut feed, second)), ((2, 0), (2, 1)));
    run(&mut c, &mut feed, first, "edit.redo", json!({}));
    // runs: each document watches its own, on the one engine both share
    run(&mut c, &mut feed, first, "run.watch", json!({"node": up1, "port": "text"}));
    run(&mut c, &mut feed, second, "run.watch", json!({"node": up2, "port": "text"}));
    assert_eq!(watched_text(&mut c, &mut feed, first), "ONE CORGI");
    assert_eq!(watched_text(&mut c, &mut feed, second), "TWO CORGIS");
    let list = docs(&mut c, &mut feed);
    let labels: Vec<&str> = list.iter().filter_map(|d| d["engine"]["label"].as_str()).collect();
    assert_eq!(labels.len(), 2, "{list:?}");
    assert!(labels.iter().all(|l| l.contains("in-process") && l.contains("connected")), "{labels:?}");
    // every event names its document, and each document's runs come only to it
    let docs_seen: std::collections::BTreeSet<u64> = feed.events.iter().filter_map(|e| e["doc"].as_u64()).collect();
    assert_eq!(docs_seen, [first, second].into_iter().collect(), "events name their documents");
    for e in &feed.events {
        if let Some(run_id) = e["run_id"].as_str() {
            let others = feed.events.iter().filter(|o| o["run_id"].as_str() == Some(run_id)).filter(|o| o["doc"] != e["doc"]).count();
            assert_eq!(others, 0, "run {run_id} reported to two documents");
        }
    }
    // each document's state is its own: its graph's values, its undo history
    let value = |c: &mut Client, feed: &mut Feed, doc: u64| ask(c, feed, Some(json!(doc)), "graph.get", json!({}))["result"]["nodes"]["n1"].to_string();
    let (v1, v2) = (value(&mut c, &mut feed, first), value(&mut c, &mut feed, second));
    assert!(v1.contains("one corgi") && v2.contains("two corgis"), "{v1} {v2}");
    let i1 = ask(&mut c, &mut feed, Some(json!(first)), "ui.inspect", json!({}))["result"].clone();
    let i2 = ask(&mut c, &mut feed, Some(json!(second)), "ui.inspect", json!({}))["result"].clone();
    assert!(i1["dirty"] == true && i2["dirty"] == true);
    assert_eq!((i1["redo"].clone(), i2["redo"].clone()), (Value::Null, json!("Add Node")), "{i1} {i2}");
    c.close();
}

#[test]
fn an_ask_in_each_document_is_answered_apart_and_closing_one_leaves_the_other_working() {
    let dir = std::path::PathBuf::from(store_root("docs-asks"));
    let mut c = Client::open(&json!({"store_root": dir}));
    let mut feed = Feed::default();
    let first = docs(&mut c, &mut feed)[0]["doc"].as_u64().unwrap();
    let second = ask(&mut c, &mut feed, None, "document.open", json!({"demo": true}))["result"]["doc"].as_u64().unwrap();
    // the demo opened in the second once its engine was there
    let n = ask(&mut c, &mut feed, Some(json!(second)), "graph.get", json!({}))["result"]["nodes"].as_object().map_or(0, |n| n.len());
    assert!(n > 0, "the demo's nodes");
    run(&mut c, &mut feed, first, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}));
    // Save As in both: an ask each, named by its document in the feed
    let mut asks = Vec::new();
    for doc in [first, second] {
        let r = ask(&mut c, &mut feed, Some(json!(doc)), "ui.invoke", json!({"command": "graph.saveAs"}));
        assert_eq!(r["ok"], true, "{r}");
        asks.push(r["result"]["asks"][0]["ask"].as_u64().unwrap());
    }
    for (doc, id) in [(first, asks[0]), (second, asks[1])] {
        assert!(feed.asks.iter().any(|a| a["ask"] == id && a["doc"] == doc && a["kind"] == "file.save"), "doc {doc}'s ask: {:?}", feed.asks);
    }
    // each answered on its own document
    for (doc, id, name) in [(first, asks[0], "first.json"), (second, asks[1], "second.json")] {
        let path = dir.join(name);
        let r = ask(&mut c, &mut feed, Some(json!(doc)), "ask.answer", json!({"ask": id, "answer": {"path": path}}));
        assert_eq!(r["ok"], true, "{r}");
        assert!(path.is_file(), "{name}");
    }
    let saved = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    assert!(saved("first.json").contains("text.literal") && !saved("first.json").contains("text.upper"));
    assert_ne!(saved("first.json"), saved("second.json"));
    // a document with unsaved changes asks before it closes; Discard closes it
    run(&mut c, &mut feed, first, "node.add", json!({"kind": "text.literal", "pos": [0, 200]}));
    let r = ask(&mut c, &mut feed, None, "document.close", json!({"doc": first}));
    assert_eq!(r["result"]["closing"], false, "{r}");
    let prompt = r["result"]["ask"].clone();
    let r = ask(&mut c, &mut feed, Some(json!(first)), "ask.answer", json!({"ask": prompt, "answer": {"answer": "discard"}}));
    assert_eq!(r["ok"], true, "{r}");
    let t0 = Instant::now();
    while docs(&mut c, &mut feed).len() != 1 {
        assert!(t0.elapsed() < Duration::from_secs(10), "the first document didn't close");
    }
    assert!(feed.changed.iter().any(|ch| ch["doc"] == first && ch["domain"] == "document" && ch["closing"] == true), "{:?}", feed.changed);
    // the other still works, and requests naming no document now reach it
    let list = docs(&mut c, &mut feed);
    assert_eq!((list[0]["doc"].as_u64(), list[0]["first"].as_bool()), (Some(second), Some(true)));
    let up = text_graph(&mut c, &mut feed, second, "still here");
    run(&mut c, &mut feed, second, "run.watch", json!({"node": up, "port": "text"}));
    assert_eq!(watched_text(&mut c, &mut feed, second), "STILL HERE");
    let r = ask(&mut c, &mut feed, None, "graph.get", json!({}));
    assert!(r["result"]["nodes"].as_object().is_some_and(|n| n.contains_key(&up)), "{r}");
    c.close();
}

#[test]
fn unknown_and_closed_documents_and_bad_requests_are_refused() {
    let mut c = Client::open(&json!({"store_root": store_root("docs-hostile")}));
    let mut feed = Feed::default();
    let second = ask(&mut c, &mut feed, None, "document.open", json!({}))["result"]["doc"].as_u64().unwrap();
    for (doc, code) in
        [(json!(987_654), "unknown_doc"), (json!("one"), "bad_params"), (json!(-1), "bad_params"), (json!([1]), "bad_params"), (json!(1.5), "bad_params")]
    {
        let r = ask(&mut c, &mut feed, Some(doc.clone()), "graph.get", json!({}));
        assert_eq!((r["ok"].as_bool(), r["code"].as_str()), (Some(false), Some(code)), "{doc}: {r}");
    }
    for bad in [json!({"demo": 1}), json!({"path": 7}), json!({"path": ""}), json!({"demo": true, "path": "/x.json"}), json!({"what": 1})] {
        let r = ask(&mut c, &mut feed, None, "document.open", bad.clone());
        assert_eq!(r["ok"], false, "{bad}: {r}");
    }
    for bad in [json!({"doc": 987_654}), json!({"doc": second, "discard": "yes"}), json!({"doc": "x"})] {
        let r = ask(&mut c, &mut feed, None, "document.close", bad.clone());
        assert_eq!(r["ok"], false, "{bad}: {r}");
    }
    // closed, then gone
    let r = ask(&mut c, &mut feed, None, "document.close", json!({"doc": second, "discard": true}));
    assert_eq!(r["result"], json!({"closing": true}), "{r}");
    let r = ask(&mut c, &mut feed, Some(json!(second)), "ui.inspect", json!({}));
    assert_eq!(r["code"], "unknown_doc", "{r}");
    let r = ask(&mut c, &mut feed, None, "document.close", json!({"doc": second}));
    assert_eq!(r["code"], "unknown_doc", "{r}");
    // the last one closed: what names no document is refused, the core still answers
    let r = ask(&mut c, &mut feed, None, "document.close", json!({"discard": true}));
    assert_eq!(r["result"]["closing"], true, "{r}");
    let r = ask(&mut c, &mut feed, None, "command.run", json!({"command": "node.add", "params": {"kind": "text.literal"}}));
    assert_eq!((r["ok"].as_bool(), r["code"].as_str()), (Some(false), Some("unknown_doc")), "{r}");
    assert_eq!(ask(&mut c, &mut feed, None, "ping", json!({}))["result"]["pong"], true);
    assert_eq!(docs(&mut c, &mut feed), Vec::<Value>::new());
    // a new document after that works as the first did
    let third = ask(&mut c, &mut feed, None, "document.open", json!({}))["result"]["doc"].as_u64().unwrap();
    run(&mut c, &mut feed, third, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}));
    c.close();
}

/// `plum_open` returns before its engine has connected: requests sent at once are held, then
/// answered in order once it has, and the feed says when (`changed`, domain `engine`).
#[test]
fn plum_open_returns_before_the_engine_and_holds_what_comes_meanwhile() {
    for (name, config) in [
        ("in-process", json!({"store_root": store_root("docs-open-inproc")})),
        (
            "process",
            json!({"store_root": store_root("docs-open-process"), "engines": {"default": "text", "mode": "process"}, "platform": {"subprocess": true}}),
        ),
    ] {
        let t0 = Instant::now();
        let mut c = Client::open(&config);
        let opened = t0.elapsed();
        let mut feed = Feed::default();
        // sent before anything is known of the engine: held, then answered in order
        let ids: Vec<u64> = (0..3)
            .map(|i| c.send(&json!({"method": "command.run", "params": {"command": "node.add", "params": {"kind": "text.literal", "pos": [0, i * 100]}}})))
            .collect();
        let list = c.send(&json!({"method": "document.list", "params": {}}));
        let listed = wait(&mut c, &mut feed, list);
        let mut nodes = Vec::new();
        for id in ids {
            let r = wait(&mut c, &mut feed, id);
            assert_eq!(r["ok"], true, "{name}: {r}");
            nodes.push(r["result"]["id"].as_str().unwrap().to_owned());
        }
        let ready = t0.elapsed();
        assert_eq!(nodes.len(), 3);
        let t1 = Instant::now();
        while !feed.changed.iter().any(|ch| ch["domain"] == "engine" && ch["ready"] == true && ch["connected"] == true) {
            assert!(t1.elapsed() < Duration::from_secs(10), "{name}: no engine change in the feed: {:?}", feed.changed);
            poll(&mut c, &mut feed);
            c.wait_wake(Duration::from_millis(50));
        }
        eprintln!("{name}: plum_open {opened:?}; held requests answered after {ready:?}; listed at once: {}", listed["result"]);
        assert!(opened < Duration::from_millis(500), "{name}: plum_open waited: {opened:?}");
        c.close();
    }
}
