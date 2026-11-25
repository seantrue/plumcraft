//! Run Selected is a core command (N4 slice 1; the maintainer: "Do it. No exceptions to the
//! rule."): `run.selected` runs the selected nodes and what they need upstream, is refused with
//! the reason when nothing is selected, and is the Run menu's item, enabled by the selection.

mod common;

use std::time::Duration;

use common::{Client, store_root};
use serde_json::{Value, json};

fn call_doc(c: &mut Client, doc: u64, method: &str, params: Value) -> Value {
    let id = c.send(&json!({"method": method, "params": params, "doc": doc}));
    c.wait_reply(id, Duration::from_secs(60))
}

fn run_doc(c: &mut Client, doc: u64, command: &str, params: Value) -> Value {
    let r = call_doc(c, doc, "command.run", json!({"command": command, "params": params}));
    assert_eq!(r["ok"], true, "{command}: {r}");
    r["result"].clone()
}

/// The Run menu's Run Selected item.
fn item(c: &mut Client, doc: u64) -> Value {
    let m = call_doc(c, doc, "menu.get", json!({}));
    let menus = m["result"]["menus"].as_array().cloned().unwrap_or_default();
    let run = menus.iter().find(|x| x["label"] == "Run").cloned().unwrap_or(Value::Null);
    run["items"].as_array().and_then(|a| a.iter().find(|i| i["command"] == "run.selected").cloned()).unwrap_or(Value::Null)
}

#[test]
fn run_selected_runs_the_selection_and_says_why_it_cannot() {
    let root = store_root("run-selected");
    let mut c = Client::open(&json!({"store_root": root, "engines": {"default": "text", "mode": "inproc"}}));
    let r = c.call("document.open", json!({}));
    let d = r["result"]["doc"].as_u64().unwrap();
    let a = run_doc(&mut c, d, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}))["id"].as_str().unwrap().to_owned();
    let b = run_doc(&mut c, d, "node.add", json!({"kind": "text.upper", "pos": [300, 0]}))["id"].as_str().unwrap().to_owned();
    run_doc(&mut c, d, "wire.connect", json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}}));
    // listed, and in the Run menu after Run, disabled with the reason while nothing is selected
    let list = call_doc(&mut c, d, "command.list", json!({}));
    assert!(list["result"].as_array().is_some_and(|l| l.iter().any(|r| r["id"] == "run.selected" && r["label"] == "Run Selected")), "{list}");
    let it = item(&mut c, d);
    assert_eq!(it["enabled"], false, "{it}");
    assert!(it["reason"].as_str().is_some_and(|r| r.contains("select")), "{it}");
    let r = call_doc(&mut c, d, "command.run", json!({"command": "run.selected", "params": {}}));
    assert_eq!(r["ok"], false, "{r}");
    assert!(r["error"].as_str().is_some_and(|e| e.contains("select nodes")), "{r}");
    // selected: enabled, and it runs
    run_doc(&mut c, d, "selection.set", json!({"nodes": [b]}));
    assert_eq!(item(&mut c, d)["enabled"], true);
    let run = run_doc(&mut c, d, "run.selected", json!({}));
    assert!(run.get("run_id").is_some(), "{run}");
    // hostile params are refused
    let r = call_doc(&mut c, d, "command.run", json!({"command": "run.selected", "params": {"targets": [a]}}));
    assert_eq!(r["ok"], false, "{r}");
    c.close();
}
