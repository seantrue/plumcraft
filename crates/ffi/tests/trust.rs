//! B185: a graph from elsewhere is held for review on the C ABI core, as in the egui editor
//! (`plumcraft_session::trust`): the native shells' core sets the trust file beside its
//! preferences (`dirs.prefs`), for every document it opens, and holds such a graph with no
//! preferences folder too (approvals then last as long as the core).

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{Client, store_root, take};
use plumcraft_ffi::plum_poll;
use serde_json::{Value, json};

#[derive(Default)]
struct Feed {
    raised: Vec<Value>,
}

fn ask(c: &mut Client, feed: &mut Feed, doc: Option<u64>, method: &str, params: Value) -> Value {
    let mut req = json!({"method": method, "params": params});
    if let Some(d) = doc {
        req["doc"] = json!(d);
    }
    let id = c.send(&req);
    let t0 = Instant::now();
    loop {
        poll(c, feed);
        if let Some(r) = c.replies.remove(&id) {
            return r;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "no reply to {method}");
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
    feed.raised.extend(v.get("asks").and_then(Value::as_array).cloned().unwrap_or_default());
}

fn run(c: &mut Client, feed: &mut Feed, doc: Option<u64>, command: &str, params: Value) -> Value {
    let r = ask(c, feed, doc, "command.run", json!({"command": command, "params": params}));
    assert_eq!(r["ok"], true, "{command}: {r}");
    r["result"].clone()
}

/// A graph somebody else wrote: a literal, upper-cased.
fn stranger(dir: &Path, name: &str) -> PathBuf {
    let file = dir.join(name);
    let g = json!({"version": 1, "nodes": {
        "a": {"kind": "text.literal", "pos": [0, 0], "values": {"text": "from elsewhere"}},
        "b": {"kind": "text.upper", "pos": [300, 0]}},
        "wires": [{"from": {"node": "a", "port": "text"}, "to": {"node": "b", "port": "text"}}]});
    std::fs::write(&file, g.to_string()).unwrap();
    file
}

/// The first document's review prompt, waiting for it a while (a document raises it on its turn).
fn review_prompt(c: &mut Client, feed: &mut Feed) -> Option<Value> {
    let t0 = Instant::now();
    loop {
        poll(c, feed);
        if let Some(a) = feed.raised.iter().find(|a| a["kind"] == "prompt" && a["params"]["kind"] == "reviewWorkflow") {
            return Some(a.clone());
        }
        if t0.elapsed() > Duration::from_secs(5) {
            return None;
        }
        c.wait_wake(Duration::from_millis(100));
    }
}

/// The first document's id.
fn first_doc(c: &mut Client, feed: &mut Feed) -> u64 {
    let r = ask(c, feed, None, "document.list", json!({}));
    r["result"]["docs"][0]["doc"].as_u64().unwrap()
}

#[test]
fn a_graph_from_elsewhere_is_held_until_approved_and_the_approval_is_kept_beside_the_preferences() {
    let root = PathBuf::from(store_root("trust-held"));
    let prefs = root.join("prefs");
    let file = stranger(&root, "stranger.plum");
    let mut c = Client::open(&json!({"store_root": root.join("store"), "dirs": {"prefs": prefs}}));
    let mut feed = Feed::default();
    let doc = first_doc(&mut c, &mut feed);
    run(&mut c, &mut feed, Some(doc), "graph.open", json!({"path": file, "discard": true}));
    let review = run(&mut c, &mut feed, Some(doc), "graph.review", json!({}));
    assert_eq!(review["pending"], true, "a graph from elsewhere waits for a review: {review}");
    let r = run(&mut c, &mut feed, Some(doc), "run.start", json!({}));
    assert!(r.get("held").is_some(), "its run is held: {r}");
    // the person is asked, with what it touches
    let prompt = review_prompt(&mut c, &mut feed).expect("a Review prompt reaches the shell");
    assert_eq!(prompt["params"]["answers"], json!(["trust", "cancel"]), "{prompt}");
    assert_eq!(prompt["params"]["detail"]["nodes"], 2, "{prompt}");
    // approved: kept beside the preferences, and the next run goes out
    let id = prompt["ask"].as_u64().unwrap();
    let a = ask(&mut c, &mut feed, Some(doc), "ask.answer", json!({"ask": id, "answer": {"answer": "trust"}}));
    assert_eq!(a["ok"], true, "{a}");
    let trusted = std::fs::read_to_string(prefs.join("trusted.json")).expect("trusted.json beside prefs.json");
    assert!(trusted.contains("\"trusted\""), "{trusted}");
    let r = run(&mut c, &mut feed, Some(doc), "run.start", json!({}));
    assert!(r.get("held").is_none(), "approved, it runs: {r}");
    // another document of the same core opening the same file trusts it at once
    let d = ask(&mut c, &mut feed, None, "document.open", json!({"path": file}));
    let other = d["result"]["doc"].as_u64().unwrap();
    let review = run(&mut c, &mut feed, Some(other), "graph.review", json!({}));
    assert_eq!(review["pending"], false, "approved once is approved: {review}");
    // and a second stranger, opened as a document of its own, is held there too
    let second = stranger(&root, "second.plum");
    std::fs::write(&second, std::fs::read_to_string(&second).unwrap().replace("from elsewhere", "another")).unwrap();
    let d = ask(&mut c, &mut feed, None, "document.open", json!({"path": second}));
    let third = d["result"]["doc"].as_u64().unwrap();
    let review = run(&mut c, &mut feed, Some(third), "graph.review", json!({}));
    assert_eq!(review["pending"], true, "a document opened later is held too: {review}");
}

#[test]
fn with_no_preferences_folder_a_graph_from_elsewhere_is_still_held() {
    if std::env::var_os("PLUMCRAFT_EDITOR_PREFS").is_some() {
        return;
    }
    let root = PathBuf::from(store_root("trust-noprefs"));
    let file = stranger(&root, "stranger.plum");
    let mut c = Client::open(&json!({"store_root": root.join("store")}));
    let mut feed = Feed::default();
    let doc = first_doc(&mut c, &mut feed);
    run(&mut c, &mut feed, Some(doc), "graph.open", json!({"path": file, "discard": true}));
    let r = run(&mut c, &mut feed, Some(doc), "run.start", json!({}));
    assert!(r.get("held").is_some(), "held with no preferences folder: {r}");
    let t = run(&mut c, &mut feed, Some(doc), "graph.trust", json!({}));
    assert_eq!(t["remembered"], false, "nowhere to keep it: {t}");
    let r = run(&mut c, &mut feed, Some(doc), "run.start", json!({}));
    assert!(r.get("held").is_none(), "approved for the core's life: {r}");
    let d = ask(&mut c, &mut feed, None, "document.open", json!({"path": file}));
    let other = d["result"]["doc"].as_u64().unwrap();
    let review = run(&mut c, &mut feed, Some(other), "graph.review", json!({}));
    assert_eq!(review["pending"], false, "another document of this core shares the approval: {review}");
}

#[test]
fn a_graph_made_here_and_saved_opens_again_with_no_review() {
    let root = PathBuf::from(store_root("trust-own"));
    let prefs = root.join("prefs");
    let mut c = Client::open(&json!({"store_root": root.join("store"), "dirs": {"prefs": prefs}}));
    let mut feed = Feed::default();
    let doc = first_doc(&mut c, &mut feed);
    run(&mut c, &mut feed, Some(doc), "node.add", json!({"kind": "text.literal", "pos": [0, 0]}));
    let file = root.join("mine.plum");
    run(&mut c, &mut feed, Some(doc), "graph.saveAs", json!({"path": file}));
    let d = ask(&mut c, &mut feed, None, "document.open", json!({"path": file}));
    let other = d["result"]["doc"].as_u64().unwrap();
    let review = run(&mut c, &mut feed, Some(other), "graph.review", json!({}));
    assert_eq!(review["pending"], false, "{review}");
}

#[test]
fn the_applications_own_graphs_and_unchanged_copies_of_them_are_not_reviewed() {
    let root = PathBuf::from(store_root("trust-bundled"));
    let bundled = root.join("bundle").join("Samples");
    std::fs::create_dir_all(&bundled).unwrap();
    let own = stranger(&bundled, "Showcase.plum");
    let copies = root.join("Documents");
    std::fs::create_dir_all(&copies).unwrap();
    let copy = copies.join("Showcase.plum");
    std::fs::copy(&own, &copy).unwrap();
    let edited = copies.join("Edited.plum");
    std::fs::write(&edited, std::fs::read_to_string(&own).unwrap().replace("from elsewhere", "changed")).unwrap();
    let renamed_changed = root.join("other").join("Showcase.plum");
    std::fs::create_dir_all(renamed_changed.parent().unwrap()).unwrap();
    std::fs::write(&renamed_changed, std::fs::read_to_string(&own).unwrap().replace("from elsewhere", "changed")).unwrap();
    let mut c = Client::open(&json!({"store_root": root.join("store"), "dirs": {"prefs": root.join("prefs"), "bundled": [bundled]}}));
    let mut feed = Feed::default();
    let doc = first_doc(&mut c, &mut feed);
    for (path, pending) in [(&own, false), (&copy, false), (&edited, true), (&renamed_changed, true)] {
        run(&mut c, &mut feed, Some(doc), "graph.open", json!({"path": path, "discard": true}));
        let review = run(&mut c, &mut feed, Some(doc), "graph.review", json!({}));
        assert_eq!(review["pending"], pending, "{}: {review}", path.display());
    }
}

/// The iPad's New Graph: the shell writes an empty graph and opens it. Nothing to review, and
/// what is added is added here, so its saves are approved and it opens again with no review.
#[test]
fn an_empty_graph_the_shell_wrote_is_not_reviewed_and_stays_trusted_as_it_grows() {
    let root = PathBuf::from(store_root("trust-empty"));
    let file = root.join("Untitled.plum");
    std::fs::write(&file, r#"{"version": 1, "nodes": {}, "wires": []}"#).unwrap();
    let mut c = Client::open(&json!({"store_root": root.join("store"), "dirs": {"prefs": root.join("prefs")}}));
    let mut feed = Feed::default();
    let doc = first_doc(&mut c, &mut feed);
    run(&mut c, &mut feed, Some(doc), "graph.open", json!({"path": file, "discard": true}));
    assert_eq!(run(&mut c, &mut feed, Some(doc), "graph.review", json!({}))["pending"], false);
    run(&mut c, &mut feed, Some(doc), "node.add", json!({"kind": "text.literal", "pos": [0, 0]}));
    let r = run(&mut c, &mut feed, Some(doc), "run.start", json!({}));
    assert!(r.get("held").is_none(), "{r}");
    run(&mut c, &mut feed, Some(doc), "graph.save", json!({}));
    let d = ask(&mut c, &mut feed, None, "document.open", json!({"path": file}));
    let other = d["result"]["doc"].as_u64().unwrap();
    assert_eq!(run(&mut c, &mut feed, Some(other), "graph.review", json!({}))["pending"], false, "saved here, approved");
}
