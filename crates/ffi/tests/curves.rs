//! The `curves` value editor through the C ABI: the array engine's Apply Curve names it, a run
//! sends its data (channel names, histograms, the preview picture), `node.editorData` returns that,
//! and `curves.*` edit the value, one undo step each.

mod common;

use common::{Client, store_root};
use serde_json::json;

#[test]
fn apply_curve_names_the_curves_editor_and_sends_its_data() {
    let root = store_root("curves");
    let mut c = Client::open(&json!({"store_root": root, "engines": {"default": "array", "mode": "inproc"}}));
    let src = c.run("node.add", json!({"kind": "array.mandelbrot", "pos": [0, 0]}))["id"].as_str().unwrap().to_owned();
    let cur = c.run("node.add", json!({"kind": "array.apply_curve", "pos": [300, 0]}))["id"].as_str().unwrap().to_owned();
    c.run("wire.connect", json!({"from": {"node": src, "port": "escape"}, "to": {"node": cur, "port": "array"}}));
    // before a run: no data
    assert_eq!(c.run("node.editorData", json!({"node": cur}))["data"], json!(null));
    c.run("run.start", json!({}));
    let done = c.call("ui.waitRun", json!({"timeout": 25}));
    assert_eq!(done["ok"], true, "{done}");
    let d = c.run("node.editorData", json!({"node": cur}));
    assert_eq!((d["kind"].as_str(), d["port"].as_str()), (Some("curves"), Some("curves")), "{d}");
    let data = &d["data"];
    let hs = data["histograms"].as_array().expect("histograms");
    assert!(!hs.is_empty() && hs.iter().all(|h| h.as_array().is_some_and(|b| b.len() == 256)), "{data}");
    assert_eq!(data["names"].as_array().map(Vec::len), Some(hs.len()));
    assert_eq!(data["picture"]["kind"], "rgba8", "{data}");
    assert!(data["picture"]["ref"]["handle"].is_string(), "{data}");
    // an edit through the core, then Equalize from the run's histogram, each an undo step
    c.run("curves.preset", json!({"node": cur, "channel": "master", "preset": "Medium S"}));
    c.run("curves.equalize", json!({"node": cur, "channel": 0}));
    assert_eq!(c.call("ui.inspect", json!({}))["result"]["undo"], "Equalize Curve");
    c.close();
}
