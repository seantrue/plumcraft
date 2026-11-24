//! Asks over the C ABI (`docs/shell-interface.md` section 11): `ui.invoke` of a command that
//! needs the shell raises an ask, which arrives in `plum_poll`'s `asks`, and `ask.answer` carries
//! the chain on. No C function is added: both are requests.

mod common;

use std::time::{Duration, Instant};

use common::{Client, store_root, take};
use plumcraft_ffi::plum_poll;
use serde_json::{Value, json};

/// Every ask raised and ended, as the feed brought them.
#[derive(Default)]
struct Feed {
    raised: Vec<Value>,
    ended: Vec<Value>,
}

/// Send a request and poll (keeping the feed's asks) until its reply comes.
fn call(c: &mut Client, feed: &mut Feed, method: &str, params: Value) -> Value {
    let id = c.send(&json!({"method": method, "params": params}));
    let t0 = Instant::now();
    loop {
        let bytes = take(plum_poll(c.core));
        if !bytes.is_empty() {
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            for r in v["replies"].as_array().unwrap() {
                c.replies.insert(r["id"].as_u64().unwrap(), r.clone());
            }
            feed.raised.extend(v.get("asks").and_then(Value::as_array).cloned().unwrap_or_default());
            feed.ended.extend(v.get("asksEnded").and_then(Value::as_array).cloned().unwrap_or_default());
        }
        if let Some(r) = c.replies.remove(&id) {
            return r;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "no reply to {method}");
        c.wait_wake(Duration::from_millis(200));
    }
}

fn run(c: &mut Client, feed: &mut Feed, id: &str, params: Value) -> Value {
    let r = call(c, feed, "command.run", json!({"command": id, "params": params}));
    assert_eq!(r["ok"], true, "{id}: {r}");
    r["result"].clone()
}

#[test]
fn save_as_raises_a_file_ask_that_a_shell_answers_later() {
    let dir = std::path::PathBuf::from(store_root("asks-save"));
    let mut c = Client::open(&json!({"store_root": dir}));
    let mut feed = Feed::default();
    run(&mut c, &mut feed, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}));
    let r = call(&mut c, &mut feed, "ui.invoke", json!({"command": "graph.saveAs"}));
    assert_eq!(r["ok"], true, "{r}");
    let ask = &r["result"]["asks"][0];
    assert_eq!((ask["kind"].as_str(), ask["params"]["purpose"].as_str(), ask["by"].as_str()), (Some("file.save"), Some("workflow"), Some("script")), "{r}");
    let id = ask["ask"].as_u64().unwrap();
    assert!(feed.raised.iter().any(|a| a["ask"] == id && a["kind"] == "file.save"), "in the feed: {:?}", feed.raised);
    // nothing waits meanwhile: the core answers other requests
    assert_eq!(call(&mut c, &mut feed, "ping", json!({}))["result"]["pong"], true);
    // hostile answers are refused, and the ask stays open
    for bad in [
        json!({"ask": id, "answer": {"path": 7}}),
        json!({"ask": id}),
        json!({"ask": "one", "answer": {}}),
        json!({"ask": id + 50, "answer": {"path": "/x"}}),
        json!([1]),
    ] {
        let r = call(&mut c, &mut feed, "ask.answer", bad.clone());
        assert_eq!(r["ok"], false, "{bad}: {r}");
    }
    // the answer, a moment later: saved
    let path = dir.join("later.json");
    let r = call(&mut c, &mut feed, "ask.answer", json!({"ask": id, "answer": {"path": path}}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["result"]["asks"], json!([]), "{r}");
    assert!(path.is_file());
    assert!(feed.ended.contains(&json!({"ask": id, "ended": "answered"})), "{:?}", feed.ended);
    // a second answer is told why not
    let r = call(&mut c, &mut feed, "ask.answer", json!({"ask": id, "answer": {"path": path}}));
    assert!(r["error"].as_str().unwrap().contains("already answered"), "{r}");
    c.close();
}

#[test]
fn new_over_unsaved_changes_goes_through_the_prompt_a_save_and_a_path() {
    let dir = std::path::PathBuf::from(store_root("asks-new"));
    let mut c = Client::open(&json!({"store_root": dir}));
    let mut feed = Feed::default();
    run(&mut c, &mut feed, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}));
    // New over unsaved changes: the prompt
    let r = call(&mut c, &mut feed, "ui.invoke", json!({"command": "graph.new"}));
    let prompt = r["result"]["asks"][0].clone();
    assert_eq!(
        (prompt["kind"].as_str(), prompt["params"]["kind"].as_str(), prompt["params"]["then"].as_str()),
        (Some("prompt"), Some("unsavedChanges"), Some("new")),
        "{r}"
    );
    assert_eq!(prompt["params"]["answers"], json!(["save", "discard", "cancel"]));
    // Save: a graph with no file asks where
    let r = call(&mut c, &mut feed, "ask.answer", json!({"ask": prompt["ask"], "answer": {"answer": "save"}}));
    let save = r["result"]["asks"][0].clone();
    assert_eq!((save["kind"].as_str(), save["params"]["name"].as_str()), (Some("file.save"), Some("untitled.plum")), "FR2: a new graph is offered .plum: {r}");
    assert_eq!(call(&mut c, &mut feed, "graph.get", json!({}))["result"]["nodes"].as_object().map(|n| n.len()), Some(1), "nothing dropped yet");
    // the path: saved, then the new graph
    let path = dir.join("kept.plum");
    let r = call(&mut c, &mut feed, "ask.answer", json!({"ask": save["ask"], "answer": {"path": path}}));
    assert_eq!((r["ok"].as_bool(), r["result"]["asks"].clone()), (Some(true), json!([])), "{r}");
    assert!(std::fs::read_to_string(&path).unwrap().contains("text.literal"), "saved first");
    assert_eq!(call(&mut c, &mut feed, "graph.get", json!({}))["result"]["nodes"], json!({}), "then a new graph");
    let ended: Vec<&Value> = feed.ended.iter().map(|e| &e["ended"]).collect();
    assert_eq!(ended, [&json!("answered"), &json!("answered")]);
    // cancel ends a chain: New over changes again, cancelled, keeps them
    run(&mut c, &mut feed, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}));
    let r = call(&mut c, &mut feed, "ui.invoke", json!({"command": "graph.new"}));
    let id = r["result"]["asks"][0]["ask"].clone();
    let r = call(&mut c, &mut feed, "ask.answer", json!({"ask": id, "cancel": true}));
    assert_eq!(r["result"]["asks"], json!([]), "{r}");
    assert_eq!(call(&mut c, &mut feed, "graph.get", json!({}))["result"]["nodes"].as_object().map(|n| n.len()), Some(1));
    // an unknown command, or a disabled one, is an error
    assert_eq!(call(&mut c, &mut feed, "ui.invoke", json!({"command": "nope"}))["ok"], false);
    assert_eq!(call(&mut c, &mut feed, "ui.invoke", json!({"command": "graph.new", "x": 1}))["ok"], false);
    c.close();
}
