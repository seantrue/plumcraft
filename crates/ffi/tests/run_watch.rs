//! What a native shell needs to run and watch a document (N3 slice 4): the `document` domain in
//! the feed (title, dirty, undo and redo), engines started beside the first (`engines.also`), the
//! run animation in the scene while it shows, and `ui.invoke {by: "person"}` for a shell's own
//! menu.

mod common;

use std::time::{Duration, Instant};

use common::{Client, store_root, take};
use plumcraft_ffi::plum_poll;
use serde_json::{Value, json};

/// Everything the feed brought, kept as it came.
#[derive(Default)]
struct Feed {
    changed: Vec<Value>,
    asks: Vec<Value>,
}

fn pump(c: &mut Client, feed: &mut Feed) {
    let bytes = take(plum_poll(c.core));
    if bytes.is_empty() {
        return;
    }
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    for r in v["replies"].as_array().into_iter().flatten() {
        c.replies.insert(r["id"].as_u64().unwrap(), r.clone());
    }
    feed.changed.extend(v.get("changed").and_then(Value::as_array).cloned().unwrap_or_default());
    feed.asks.extend(v.get("asks").and_then(Value::as_array).cloned().unwrap_or_default());
}

fn call(c: &mut Client, feed: &mut Feed, method: &str, params: Value) -> Value {
    let id = c.send(&json!({"method": method, "params": params}));
    let t0 = Instant::now();
    loop {
        pump(c, feed);
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

/// Poll until `f` holds of what the feed brought, for at most a minute (a condition, not a sleep).
fn until(c: &mut Client, feed: &mut Feed, what: &str, f: impl Fn(&Feed) -> bool) {
    let t0 = Instant::now();
    while !f(feed) {
        assert!(t0.elapsed() < Duration::from_secs(60), "never: {what}; changed {:?}", feed.changed);
        c.wait_wake(Duration::from_millis(100));
        pump(c, feed);
    }
}

fn last_document(feed: &Feed) -> Option<&Value> {
    feed.changed.iter().rev().find(|c| c["domain"] == "document" && c.get("gen").is_some())
}

#[test]
fn the_feed_says_when_the_documents_title_dirty_state_and_undo_step_change() {
    let mut c = Client::open(&json!({"store_root": store_root("rw-document")}));
    let mut feed = Feed::default();
    // once the engine is there, the document's state comes once
    until(&mut c, &mut feed, "the first document state", |f| last_document(f).is_some());
    let first = last_document(&feed).unwrap().clone();
    assert_eq!((first["dirty"].as_bool(), first["undo"].clone()), (Some(false), Value::Null), "{first}");
    assert!(first["title"].as_str().is_some_and(|t| t.contains("Untitled")), "{first}");
    run(&mut c, &mut feed, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}));
    until(&mut c, &mut feed, "dirty after an edit", |f| last_document(f).is_some_and(|d| d["dirty"] == true));
    let d = last_document(&feed).unwrap().clone();
    assert_eq!(d["undo"], "Add Node", "{d}");
    assert!(d["gen"].as_u64() > first["gen"].as_u64(), "{d} after {first}");
    run(&mut c, &mut feed, "edit.undo", json!({}));
    // (the session keeps a document dirty after an undo back to where it was saved: the core's
    // rule, which the shell follows)
    until(&mut c, &mut feed, "redo after undo", |f| last_document(f).is_some_and(|d| d["undo"].is_null() && d["redo"] == "Add Node"));
    // saved: the path and the title follow
    let path = std::path::PathBuf::from(store_root("rw-document")).join("named.json");
    run(&mut c, &mut feed, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}));
    run(&mut c, &mut feed, "graph.saveAs", json!({"path": path}));
    until(&mut c, &mut feed, "saved", |f| {
        last_document(f).is_some_and(|d| d["dirty"] == false && d["path"].as_str().is_some_and(|p| p.ends_with("named.json")))
    });
    c.close();
}

/// B167: the same error said twice is two errors: the `document` domain changes each time, its
/// `errorSeq` rising, so a native shell's banner shows the second one too.
#[test]
fn the_same_error_twice_is_said_twice() {
    let mut c = Client::open(&json!({"store_root": store_root("rw-error-twice")}));
    let mut feed = Feed::default();
    until(&mut c, &mut feed, "the first document state", |f| last_document(f).is_some());
    let seq = |f: &Feed| last_document(f).and_then(|d| d["errorSeq"].as_u64()).unwrap_or(0);
    let start = seq(&feed);
    // a person's Copy with nothing selected: the status line says what to do
    let invoke = |c: &mut Client, feed: &mut Feed| call(c, feed, "ui.invoke", json!({"command": "edit.copy", "by": "person"}));
    invoke(&mut c, &mut feed);
    until(&mut c, &mut feed, "the first error", |f| seq(f) > start);
    let first = last_document(&feed).unwrap().clone();
    assert_eq!(first["lastError"], "select nodes to copy first", "{first}");
    invoke(&mut c, &mut feed);
    until(&mut c, &mut feed, "the same error again, counted", |f| seq(f) > first["errorSeq"].as_u64().unwrap_or(0));
    assert_eq!(last_document(&feed).unwrap()["lastError"], "select nodes to copy first");
    c.close();
}

#[test]
fn engines_also_puts_a_second_engines_kinds_in_every_documents_catalogue() {
    let mut c = Client::open(&json!({"store_root": store_root("rw-also"), "engines": {"default": "text", "mode": "inproc", "also": ["array"]}}));
    let mut feed = Feed::default();
    let t0 = Instant::now();
    // the second engine joins behind the coordinator a moment after the first
    loop {
        let r = call(&mut c, &mut feed, "command.run", json!({"command": "node.add", "params": {"kind": "array.mandelbrot", "pos": [0, 0]}}));
        if r["ok"] == true {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "array never joined: {r}");
        c.wait_wake(Duration::from_millis(100));
    }
    run(&mut c, &mut feed, "node.add", json!({"kind": "text.literal", "pos": [300, 0]}));
    c.close();
    // hostile `also` is the config's error
    for bad in [json!("array"), json!([7]), json!(["nope"]), Value::Array(vec![json!("array"); 9])] {
        let config = json!({"engines": {"also": bad}}).to_string();
        assert!(plumcraft_ffi::core::Config::parse(config.as_bytes()).is_err(), "{config}");
    }
}

#[test]
fn a_runs_animation_is_in_the_scene_while_it_shows_and_gone_once_faded() {
    let mut c = Client::open(&json!({"store_root": store_root("rw-anim")}));
    let mut feed = Feed::default();
    let a = run(&mut c, &mut feed, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}))["id"].as_str().unwrap().to_owned();
    let b = run(&mut c, &mut feed, "node.add", json!({"kind": "text.upper", "pos": [300, 0]}))["id"].as_str().unwrap().to_owned();
    run(&mut c, &mut feed, "wire.connect", json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}}));
    let at_rest = call(&mut c, &mut feed, "scene.get", json!({}));
    assert_eq!(at_rest["result"]["anim"]["on"], false, "no run yet: {}", at_rest["result"]["anim"]);
    run(&mut c, &mut feed, "run.start", json!({}));
    // while the run shows (and holds after it), the scene carries the animation and each face its
    // state as shown; then the colours fade and the scene is at rest again
    let t0 = Instant::now();
    let mut seen_on = false;
    loop {
        let s = call(&mut c, &mut feed, "scene.get", json!({}))["result"].clone();
        let on = s["anim"]["on"] == true;
        if on {
            seen_on = true;
            let shown: Vec<Value> = s["nodes"].as_array().unwrap().iter().map(|f| f["shown"].clone()).collect();
            assert!(
                shown.iter().all(|x| x.is_null() || ["waiting", "running", "done", "cached", "failed", "skipped"].contains(&x.as_str().unwrap_or(""))),
                "{shown:?}"
            );
        } else if seen_on {
            assert!(s["nodes"].as_array().unwrap().iter().all(|f| f["shown"].is_null()), "at rest, nothing shown: {}", s["nodes"]);
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "the animation never came and went: {}", s["anim"]);
        c.wait_wake(Duration::from_millis(50));
    }
    // View › Colour Nodes While Running off: the next run carries none
    run(&mut c, &mut feed, "view.runColors", json!({"on": false}));
    run(&mut c, &mut feed, "node.set", json!({"node": a, "port": "text", "value": "again"}));
    run(&mut c, &mut feed, "run.start", json!({}));
    let r = call(&mut c, &mut feed, "ui.waitRun", json!({"timeout": 30}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(call(&mut c, &mut feed, "scene.get", json!({}))["result"]["anim"]["on"], false);
    c.close();
}

#[test]
fn a_shells_own_invoke_raises_asks_for_the_person_and_a_script_cannot_claim_it() {
    let dir = std::path::PathBuf::from(store_root("rw-person"));
    let mut c = Client::open(&json!({"store_root": dir}));
    let mut feed = Feed::default();
    run(&mut c, &mut feed, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}));
    let r = call(&mut c, &mut feed, "ui.invoke", json!({"command": "graph.saveAs", "by": "person"}));
    assert_eq!(r["ok"], true, "{r}");
    let ask = r["result"]["asks"][0].clone();
    assert_eq!((ask["kind"].as_str(), ask["by"].as_str()), (Some("file.save"), Some("person")), "{r}");
    let r = call(&mut c, &mut feed, "ask.answer", json!({"ask": ask["ask"], "cancel": true}));
    assert_eq!(r["ok"], true, "{r}");
    // without `by`, or with `by: "script"`, a script's
    let r = call(&mut c, &mut feed, "ui.invoke", json!({"command": "graph.saveAs", "by": "script"}));
    assert_eq!(r["result"]["asks"][0]["by"], "script", "{r}");
    call(&mut c, &mut feed, "ask.answer", json!({"ask": r["result"]["asks"][0]["ask"], "cancel": true}));
    for bad in [json!("robot"), json!(1), json!(null)] {
        let r = call(&mut c, &mut feed, "ui.invoke", json!({"command": "graph.saveAs", "by": bad}));
        assert_eq!(r["ok"], false, "{bad}: {r}");
    }
    // the dispatcher itself (every control channel) knows no `by`: only the shell's own requests
    // carry it, and the C ABI takes it off before the dispatcher sees them
    let mut doc = plumcraft_editor_core::document::Document::new(plumcraft_session::Session::new(Default::default()));
    let v = plumcraft_editor_core::control::now(&mut doc, "ui.invoke", &json!({"command": "graph.saveAs", "by": "person"}));
    assert_eq!(v["ok"], false, "{v}");
    c.close();
}

/// B163, the public engines' way: a graph whose node needs a second engine, switched to the first
/// alone, has its problem said on the status line; once the second engine joins (`engine.add`),
/// the line says nothing stale. (The relink always said the problems again, so this passed before
/// the fix too; the fix, for a catalogue changed any other way, is `Document::recheck_problems`,
/// tested in the core.)
#[test]
fn the_problems_line_goes_when_the_engine_that_fixes_them_joins() {
    let root = store_root("rw-problems-line");
    let path = std::path::PathBuf::from(&root).join("needs-array.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        &path,
        json!({"version": 1, "nodes": {"t": {"kind": "text.literal", "pos": [0, 0], "values": {"text": "hi"}}, "m": {"kind": "array.mandelbrot", "pos": [0, 300], "values": {"width": 16, "height": 12}}}, "wires": []}).to_string(),
    )
    .unwrap();
    let mut c = Client::open(&json!({"store_root": root, "engines": {"default": "text", "mode": "inproc", "auto": false}}));
    let mut feed = Feed::default();
    until(&mut c, &mut feed, "the first document state", |f| last_document(f).is_some());
    let r = call(&mut c, &mut feed, "command.run", json!({"command": "graph.open", "params": {"path": path, "discard": true}}));
    assert_eq!(r["ok"], true, "{r}");
    // the text engine alone: the array node is a problem, said on the status line
    let r = call(&mut c, &mut feed, "command.run", json!({"command": "engine.switch", "params": {"engine": "text", "mode": "inproc"}}));
    assert_eq!(r["ok"], true, "{r}");
    let t1 = Instant::now();
    while t1.elapsed() < Duration::from_secs(3) {
        c.wait_wake(Duration::from_millis(100));
        pump(&mut c, &mut feed);
    }
    let i = call(&mut c, &mut feed, "ui.inspect", json!({}));
    assert!(i["result"]["lastError"].as_str().is_some_and(|e| e.contains("the graph has 1 problem(s)")), "{}", i["result"]["lastError"]);
    let r = call(&mut c, &mut feed, "command.run", json!({"command": "engine.add", "params": {"engine": "array", "mode": "inproc"}}));
    assert_eq!(r["ok"], true, "{r}");
    let t0 = Instant::now();
    loop {
        let i = call(&mut c, &mut feed, "ui.inspect", json!({}));
        if i["result"]["catalog"]["nodes"].as_u64().unwrap_or(0) > 60 {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "array never joined");
        c.wait_wake(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_millis(300));
    pump(&mut c, &mut feed);
    let i = call(&mut c, &mut feed, "ui.inspect", json!({}));
    let said = i["result"]["lastError"].as_str().unwrap_or_default().to_owned();
    assert!(!said.contains("problem"), "a stale problems line: {said}");
    c.close();
}
