//! The menu model and the log through the C ABI (N1 slice 8, `docs/shell-interface.md` 12 and
//! 15): `menu.get` gives the menu bar with a generation that moves when what it holds changes
//! (the feed says so, `changed` domain `menus`), an item's enabled state follows the document,
//! invoking an item is `ui.invoke` or `command.run` with its command and params, context menus
//! name what they act on, and the Engine menu lists what a platform can't start, disabled with
//! the reason. `log.tail` reads the core's log, `log.write` adds the shell's own lines, the feed
//! says when there are new ones, hostile input is bounded, and the control token never enters it.

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::{Client, store_root, take};
use plumcraft_ffi::plum_poll;
use serde_json::{Value, json};

/// Every item under `items`, depth first.
fn walk(items: &Value, out: &mut Vec<Value>) {
    for i in items.as_array().into_iter().flatten() {
        out.push(i.clone());
        walk(&i["items"], out);
    }
}

fn find(items: &Value, pred: impl Fn(&Value) -> bool) -> Option<Value> {
    let mut all = Vec::new();
    walk(items, &mut all);
    all.into_iter().find(|i| pred(i))
}

fn ok(r: &Value) -> Value {
    assert_eq!(r["ok"], true, "{r}");
    r["result"].clone()
}

/// What the feed said changed, and the log notes, while waiting for `id`'s reply.
#[derive(Default)]
struct Feed {
    changed: Vec<Value>,
    log: Vec<Value>,
}

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
            feed.changed.extend(v.get("changed").and_then(Value::as_array).cloned().unwrap_or_default());
            if let Some(l) = v.get("log") {
                feed.log.push(l.clone());
            }
        }
        if let Some(r) = c.replies.remove(&id) {
            return r;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "no reply to {method}");
        c.wait_wake(Duration::from_millis(100));
    }
}

/// Poll until the feed says the menus changed past `gen`.
fn menus_change(c: &mut Client, feed: &mut Feed, past: u64) -> u64 {
    let t0 = Instant::now();
    loop {
        if let Some(g) = feed.changed.iter().filter(|ch| ch["domain"] == "menus").filter_map(|ch| ch["gen"].as_u64()).max().filter(|g| *g > past) {
            return g;
        }
        call(c, feed, "ping", json!({}));
        assert!(t0.elapsed() < Duration::from_secs(20), "no menus change past {past}: {:?}", feed.changed);
        c.wait_wake(Duration::from_millis(60));
    }
}

#[test]
fn the_menu_bar_follows_the_document_and_invoking_an_item_runs_its_command() {
    let mut c = Client::open(&json!({"store_root": store_root("menus-bar"), "platform": {"subprocess": true, "python": true, "craft_apps": true}}));
    let mut feed = Feed::default();
    let bar = ok(&call(&mut c, &mut feed, "menu.get", json!({})));
    let g0 = bar["gen"].as_u64().expect("a generation");
    let titles: Vec<&str> = bar["menus"].as_array().unwrap().iter().filter_map(|m| m["label"].as_str()).collect();
    assert_eq!(&titles[..5], ["File", "Edit", "View", "Node", "Run"], "{titles:?}");
    // undo: disabled on a fresh document, with why, and the platform's own place for it
    let undo = find(&bar["menus"], |i| i["id"] == "edit.undo").unwrap();
    assert_eq!((undo["enabled"].as_bool(), undo["role"].as_str()), (Some(false), Some("undo")), "{undo}");
    assert!(undo["reason"].is_string(), "{undo}");
    assert_eq!(undo["shortcut"], json!({"key": "Z", "mods": ["primary"]}));
    let quit = find(&bar["menus"], |i| i["id"] == "app.quit").unwrap();
    assert_eq!(quit["role"], "quit");
    // the same model asked again: the same generation
    assert_eq!(ok(&call(&mut c, &mut feed, "menu.get", json!({})))["gen"].as_u64(), Some(g0));

    // an edit: undo is enabled, and the feed says the menus changed
    // (polled here, so the feed's `changed` is seen)
    let added = ok(&call(&mut c, &mut feed, "command.run", json!({"command": "node.add", "params": {"kind": "text.literal", "pos": [0, 0]}})));
    let id = added["id"].as_str().unwrap().to_owned();
    let g1 = menus_change(&mut c, &mut feed, g0);
    let bar = ok(&call(&mut c, &mut feed, "menu.get", json!({})));
    assert!(bar["gen"].as_u64().unwrap() >= g1);
    let undo = find(&bar["menus"], |i| i["id"] == "edit.undo").unwrap();
    assert_eq!(undo["enabled"], true, "{undo}");

    // invoking an item: ui.invoke with its command (no params)
    let r = call(&mut c, &mut feed, "ui.invoke", json!({"command": undo["command"]}));
    ok(&r);
    let graph = ok(&call(&mut c, &mut feed, "graph.get", json!({})));
    assert_eq!(graph["nodes"].as_object().map(|n| n.len()), Some(0), "undone: {graph}");

    // an item with params: command.run with them; its check mark follows
    let speed = find(&bar["menus"], |i| i["id"] == "view.animationSpeed" && i["params"] == json!({"ms": 0})).unwrap();
    assert_eq!(speed["checked"], false);
    ok(&call(&mut c, &mut feed, "command.run", json!({"command": speed["command"], "params": speed["params"]})));
    let bar = ok(&call(&mut c, &mut feed, "menu.get", json!({})));
    assert_eq!(find(&bar["menus"], |i| i["id"] == "view.animationSpeed" && i["params"] == json!({"ms": 0})).unwrap()["checked"], true);

    // a context menu names what it acts on; its item runs as given
    ok(&call(&mut c, &mut feed, "ui.invoke", json!({"command": "edit.redo"})));
    let menu = ok(&call(&mut c, &mut feed, "menu.get", json!({"context": {"kind": "node", "node": id}})));
    assert_eq!(menu["context"], "node");
    let dup = find(&menu["items"], |i| i["id"] == "node.duplicate").unwrap();
    ok(&call(&mut c, &mut feed, "command.run", json!({"command": dup["command"], "params": dup["params"]})));
    let graph = ok(&call(&mut c, &mut feed, "graph.get", json!({})));
    assert_eq!(graph["nodes"].as_object().map(|n| n.len()), Some(2), "duplicated: {graph}");

    // hostile requests are refused, and the core goes on
    for bad in [
        json!({"context": "node"}),
        json!({"context": {"kind": "nope"}}),
        json!({"context": {"kind": "node"}}),
        json!({"context": {"kind": "wire", "to": {"node": 3}}}),
        json!({"context": {"kind": "node", "node": "x".repeat(100_000)}}),
        json!({"extra": true}),
        json!([1, 2]),
    ] {
        let r = call(&mut c, &mut feed, "menu.get", bad.clone());
        assert_eq!(r["ok"], false, "{bad}: {r}");
    }
    // a node that isn't there has an empty menu
    assert_eq!(ok(&call(&mut c, &mut feed, "menu.get", json!({"context": {"kind": "node", "node": "n404"}})))["items"], json!([]));
    ok(&call(&mut c, &mut feed, "menu.get", json!({})));
    c.close();
}

#[test]
fn a_platform_that_cant_start_programs_lists_process_engines_disabled_with_why() {
    // no `platform`: nothing but engines compiled in and engines already running. Engines picked
    // by hand (`engines.auto: false`): the menu lists each hosting
    let mut c = Client::open(&json!({"store_root": store_root("menus-engines"), "engines": {"auto": false}}));
    let mut feed = Feed::default();
    let bar = ok(&call(&mut c, &mut feed, "menu.get", json!({})));
    let engine = bar["menus"].as_array().unwrap().iter().find(|m| m["label"] == "Engine").cloned().unwrap();
    let process = find(&engine["items"], |i| i["params"] == json!({"engine": "text", "mode": "process"})).expect("the process hosting is listed");
    assert_eq!(process["enabled"], false, "{process}");
    assert_eq!(process["reason"], "this device can't start programs");
    let inproc = find(&engine["items"], |i| i["params"] == json!({"engine": "text", "mode": "inproc"})).unwrap();
    assert_eq!(inproc["enabled"], true, "{inproc}");
    assert_eq!(inproc["checked"], true, "in use: {inproc}");
    c.close();
    // picked by the graph (the default since the budget stage, as the egui editor's Pick Engines
    // Automatically): one item an engine, hosted the fastest way the platform can; one it can't
    // start at all says why
    let mut c = Client::open(&json!({"store_root": store_root("menus-engines-auto")}));
    let mut feed = Feed::default();
    let bar = ok(&call(&mut c, &mut feed, "menu.get", json!({})));
    let engine = bar["menus"].as_array().unwrap().iter().find(|m| m["label"] == "Engine").cloned().unwrap();
    let text = find(&engine["items"], |i| i["params"] == json!({"engine": "text"})).expect("the text engine is listed");
    assert_eq!((text["enabled"].clone(), text["checked"].clone()), (json!(true), json!(true)), "{text}");
    let array = find(&engine["items"], |i| i["params"] == json!({"engine": "array"})).expect("the array engine is listed");
    assert_eq!((array["enabled"].clone(), array["checked"].clone()), (json!(true), json!(false)), "{array}");
    let audio = find(&engine["items"], |i| i["params"] == json!({"engine": "audio"})).expect("the audio engine is listed");
    assert_eq!(audio["enabled"], false, "a process only: {audio}");
    assert_eq!(audio["reason"], "this device can't start programs");
    c.close();
}

#[test]
fn the_log_holds_both_sides_is_bounded_and_never_holds_the_token() {
    let dir = PathBuf::from(store_root("log"));
    let logs = dir.join("logs");
    let token_file = dir.join("control.token");
    let mut c = Client::open(&json!({"store_root": dir, "control": {"port": 0, "token_file": token_file}, "dirs": {"logs": logs}, "log": {"level": "debug"}}));
    let mut feed = Feed::default();
    let token = std::fs::read_to_string(&token_file).unwrap().trim().to_owned();
    assert!(token.len() >= 8);
    // a second core whose token is made up and printed: the log has the line, not the token
    let mut d = Client::open(&json!({"store_root": store_root("log-2"), "control": {"port": 0}}));

    let all = ok(&call(&mut c, &mut feed, "log.tail", json!({"limit": 5000})));
    let entries = all["entries"].as_array().unwrap();
    assert!(entries.iter().any(|e| e["source"] == "ffi" && e["message"].as_str().unwrap().contains("control channel on 127.0.0.1:")), "{all}");
    let printed = entries.iter().find(|e| e["message"].as_str().unwrap().starts_with("plumcraft: control token: ")).expect("the made-up token's line");
    assert!(printed["message"].as_str().unwrap().ends_with("‹redacted›"), "{printed}");

    // the shell's own line, in the same log, under `shell`, with its document; the feed says so
    let w = ok(&call(&mut c, &mut feed, "log.write", json!({"level": "warn", "message": "the shell went to the background"})));
    let seq = w["seq"].as_u64().unwrap();
    let t = ok(&call(&mut c, &mut feed, "log.tail", json!({"after": seq - 1, "source": "shell"})));
    assert_eq!(t["entries"][0]["message"], "the shell went to the background");
    assert_eq!(t["entries"][0]["level"], "warn");
    assert!(t["entries"][0]["doc"].is_u64());
    assert!(t["entries"][0]["time"].as_f64().unwrap() > 1.7e9);
    let t0 = Instant::now();
    while !feed.log.iter().any(|l| l["seq"].as_u64() >= Some(seq)) {
        call(&mut c, &mut feed, "ping", json!({}));
        assert!(t0.elapsed() < Duration::from_secs(10), "no log note in the feed: {:?}", feed.log);
    }

    // hostile: a huge message with control characters, a flood, bad params
    let huge = format!("\u{1b}[31mred\u{0}{}", "z".repeat(1 << 20));
    let w = ok(&call(&mut c, &mut feed, "log.write", json!({"level": "error", "message": huge})));
    let e = ok(&call(&mut c, &mut feed, "log.tail", json!({"after": w["seq"].as_u64().unwrap() - 1, "limit": 1})))["entries"][0].clone();
    let m = e["message"].as_str().unwrap();
    assert!(m.chars().count() <= plumcraft_editor_core::log::MESSAGE_MAX + 1 && !m.chars().any(char::is_control), "{}", &m[..60]);
    for i in 0..300 {
        let id = c.send(&json!({"method": "log.write", "params": {"level": "info", "message": format!("flood {i} {token}")}}));
        let _ = id;
    }
    for bad in [
        json!({"level": "loud", "message": "x"}),
        json!({"level": "info"}),
        json!({"level": "info", "message": 3}),
        json!({"level": "info", "message": "x", "source": "core"}),
    ] {
        assert_eq!(call(&mut c, &mut feed, "log.write", bad.clone())["ok"], false, "{bad}");
    }
    for bad in [json!({"limit": 0}), json!({"limit": 1_000_000}), json!({"after": "x"}), json!({"level": 3}), json!({"what": 1})] {
        assert_eq!(call(&mut c, &mut feed, "log.tail", bad.clone())["ok"], false, "{bad}");
    }

    // nowhere in the ring or the file is the token, even where a shell wrote it
    let all = ok(&call(&mut c, &mut feed, "log.tail", json!({"limit": 5000})));
    let text = all.to_string();
    assert!(!text.contains(&token), "the token is in the log");
    assert!(text.contains("flood 299"), "the flood arrived");
    let file = std::fs::read_to_string(logs.join("plumcraft.log")).unwrap();
    assert!(file.contains("the shell went to the background"));
    assert!(!file.contains(&token), "the token is in the log file");
    // and over the control channel, the same methods
    d.call("ping", json!({}));
    d.close();
    c.close();
}
