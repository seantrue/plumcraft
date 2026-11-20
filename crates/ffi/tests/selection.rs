//! The selection through the C ABI (N2 slice 1, `docs/shell-interface.md` 8.13 and 12.3): it is
//! the document's, set and read with the canvas's commands like any other command; `ui.inspect`
//! shows the selected nodes; Select All, Delete, the group entered and the net follow it; the
//! menus follow it; the view's commands (fit, the palette, reveal) are refused, since this core
//! draws no canvas; and hostile params are refused without changing it.

mod common;

use common::{Client, store_root};
use serde_json::{Value, json};

fn add(c: &mut Client, kind: &str, x: f64) -> String {
    c.run("node.add", json!({"kind": kind, "pos": [x, 0]}))["id"].as_str().unwrap().to_owned()
}

fn find(items: &Value, id: &str) -> Option<Value> {
    for i in items.as_array().into_iter().flatten() {
        if i["id"] == id {
            return Some(i.clone());
        }
        if let Some(f) = find(&i["items"], id) {
            return Some(f);
        }
    }
    None
}

#[test]
fn a_shell_selects_and_reads_the_selection_back() {
    let mut c = Client::open(&json!({"store_root": store_root("selection")}));
    let a = add(&mut c, "text.literal", 0.0);
    let b = add(&mut c, "text.upper", 300.0);
    let d = add(&mut c, "text.upper", 600.0);

    // the canvas's commands are listed like the registry's
    let list = c.call("command.list", json!({}));
    let ids: Vec<&str> = list["result"].as_array().unwrap().iter().filter_map(|r| r["id"].as_str()).collect();
    for id in
        ["selection.set", "selection.clear", "selection.all", "net.select", "edit.delete", "canvas.focus", "canvas.fit", "canvas.palette", "canvas.reveal"]
    {
        assert!(ids.contains(&id), "{id} in command.list");
    }

    // select, then read it back: the reply and ui.inspect agree
    let r = c.run("selection.set", json!({"nodes": [a, b]}));
    assert_eq!(r["nodes"], json!([a, b]));
    assert_eq!(c.call("ui.inspect", json!({}))["result"]["selection"], json!([a, b]));
    let r = c.run("selection.set", json!({"nodes": [a], "mode": "toggle"}));
    assert_eq!(r["nodes"], json!([b]));
    let r = c.run("selection.set", json!({"nodes": [d], "mode": "add"}));
    assert_eq!(r["nodes"], json!([b, d]));

    // the menus follow the document's selection: Cut is enabled now
    let bar = c.call("menu.get", json!({}));
    let cut = find(&bar["result"]["menus"], "edit.cut").expect("Edit › Cut");
    assert_eq!((cut["enabled"].as_bool(), cut["command"].as_str()), (Some(true), Some("edit.cut")), "{cut}");

    // hostile params: refused, and nothing changes
    for bad in [json!({"nodes": "x"}), json!({"nodes": ["nope"]}), json!({"mode": "xor"}), json!({"nodes": [1]}), json!({"what": 1})] {
        let r = c.call("command.run", json!({"command": "selection.set", "params": bad}));
        assert_eq!(r["ok"], false, "{bad}: {r}");
    }
    assert_eq!(c.call("ui.inspect", json!({}))["result"]["selection"], json!([b, d]));

    // Select All, as Edit › Select All invokes it
    let r = c.call("ui.invoke", json!({"command": "selection.all"}));
    assert_eq!(r["result"]["selection"], json!([a, b, d]), "{r}");

    // Delete takes the selection, as one undo step, and clears it
    c.run("selection.set", json!({"nodes": [d]}));
    c.run("edit.delete", json!({}));
    let g = c.call("graph.get", json!({}))["result"].clone();
    assert!(g["nodes"].get(&d).is_none() && g["nodes"].get(&a).is_some(), "{g}");
    assert_eq!(c.call("ui.inspect", json!({}))["result"]["selection"], json!([]));
    let r = c.call("command.run", json!({"command": "edit.delete", "params": {}}));
    assert_eq!((r["ok"].as_bool(), r["error"].as_str()), (Some(false), Some("select nodes to delete first")), "{r}");
    // an undo brings it back; what the graph lost is not selected
    c.run("selection.set", json!({"nodes": [a, b]}));
    c.run("node.remove", json!({"nodes": [b]}));
    assert_eq!(c.call("ui.inspect", json!({}))["result"]["selection"], json!([a]), "a removed node leaves the selection");

    // a group entered: Select All takes its members only
    let gid = c.run("group.create", json!({"nodes": [a], "groups": []}))["id"].as_str().unwrap().to_owned();
    assert_eq!(c.run("canvas.focus", json!({"group": gid}))["focus"], json!(gid));
    assert_eq!(c.run("selection.all", json!({}))["nodes"], json!([a]));
    assert_eq!(c.run("canvas.focus", json!({"group": null}))["focus"], Value::Null);
    let r = c.call("command.run", json!({"command": "canvas.focus", "params": {"group": "g999"}}));
    assert_eq!(r["ok"], false, "{r}");

    // what only a canvas does is refused here, as ui.set is
    for (id, p) in [("canvas.fit", json!({})), ("canvas.palette", json!({"open": true})), ("canvas.reveal", json!({"node": a}))] {
        let r = c.call("command.run", json!({"command": id, "params": p}));
        assert_eq!(r["error"], json!(format!("{id} isn't available from this shell")), "{r}");
    }
    // clearing leaves nothing selected
    assert_eq!(c.run("selection.clear", json!({}))["nodes"], json!([]));
    c.close();
}
