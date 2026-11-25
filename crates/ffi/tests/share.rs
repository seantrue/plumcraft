//! `platform.share` (contract 4.2, the maintainer's "Disable" for egui, 2026-11-21): a shell
//! that has no share sheet says so in its config, and the core disables Share… and a pop-out's
//! Share with its reason and refuses them when run. Unsaid, a shell shares, as every shell built
//! before the capability did (the Apple apps, the Windows app).

mod common;

use common::{Client, store_root};
use serde_json::{Value, json};

const REASON: &str = "Sharing isn't available in this editor";

fn row(rows: &Value, id: &str) -> Value {
    rows.as_array().and_then(|a| a.iter().find(|r| r["id"] == json!(id))).cloned().unwrap_or(Value::Null)
}

/// File › Share… in the menu model.
fn share_item(menu: &Value) -> Value {
    fn find(v: &Value) -> Option<Value> {
        if v.get("command") == Some(&json!("graph.share")) {
            return Some(v.clone());
        }
        match v {
            Value::Array(a) => a.iter().find_map(find),
            Value::Object(o) => o.values().find_map(find),
            _ => None,
        }
    }
    find(menu).unwrap_or(Value::Null)
}

#[test]
fn a_shell_without_a_share_sheet_has_share_disabled_with_the_cores_reason() {
    let mut cl = Client::open(&json!({"store_root": store_root("share-off"), "platform": {"share": false}}));
    let rows = cl.call("command.list", json!({}))["result"].clone();
    for id in ["value.share", "graph.share"] {
        assert_eq!(row(&rows, id)["enabled"], json!(false), "{id}");
        assert_eq!(row(&rows, id)["disabledReason"], json!(REASON), "{id}");
    }
    // saved, so Share's own rule (a file) is met: the shell's lack is what disables it
    let graph = plumcraft_scratch::dir("ffi-share-off").join("g.json");
    cl.run("graph.saveAs", json!({"path": graph}));
    let item = share_item(&cl.call("menu.get", json!({}))["result"]);
    assert_eq!(item["enabled"], json!(false), "{item}");
    assert_eq!(item["reason"], json!(REASON), "{item}");
    let r = cl.call("command.run", json!({"command": "graph.share", "params": {}}));
    assert_eq!(r["ok"], json!(false), "{r}");
    assert!(r["error"].as_str().unwrap_or_default().contains(REASON), "{r}");
    let r = cl.call("ui.invoke", json!({"command": "graph.share"}));
    assert_eq!(r["error"], json!(format!("graph.share is not available: {REASON}")), "{r}");
    cl.close();
}

#[test]
fn a_shell_that_says_nothing_shares_as_before() {
    let mut cl = Client::open(&json!({"store_root": store_root("share-on"), "platform": {"subprocess": false}}));
    let graph = plumcraft_scratch::dir("ffi-share-on").join("g.json");
    cl.run("graph.saveAs", json!({"path": graph}));
    let rows = cl.call("command.list", json!({}))["result"].clone();
    assert_eq!(row(&rows, "graph.share")["enabled"], json!(true));
    assert_eq!(cl.run("graph.share", json!({}))["file"], json!(graph));
    let item = share_item(&cl.call("menu.get", json!({}))["result"]);
    assert_eq!(item["enabled"], json!(true), "{item}");
    cl.close();
}
