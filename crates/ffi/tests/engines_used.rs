//! Start only the engines a graph uses (decision 8C at N3's exit, the N3 budget stage): a
//! document opens on the engines its file uses, as far as the file and the engines compiled in
//! say; a node of another engine's kind added later starts that engine and waits for it; and
//! `engines.auto: false` keeps the old way (`engines.default` and `engines.also` in every
//! document).

mod common;

use std::time::Duration;

use common::{Client, store_root};
use serde_json::{Value, json};

/// Request `method` for document `doc`, and wait for its reply.
fn call_doc(c: &mut Client, doc: u64, method: &str, params: Value) -> Value {
    let id = c.send(&json!({"method": method, "params": params, "doc": doc}));
    c.wait_reply(id, Duration::from_secs(60))
}

fn run_doc(c: &mut Client, doc: u64, command: &str, params: Value) -> Value {
    let r = call_doc(c, doc, "command.run", json!({"command": command, "params": params}));
    assert_eq!(r["ok"], true, "{command}: {r}");
    r["result"].clone()
}

/// A workflow file of `kinds`, naming no engine (as the bundled samples and every graph saved
/// with one engine connected are).
fn workflow(dir: &str, name: &str, kinds: &[&str]) -> String {
    let nodes: serde_json::Map<String, Value> =
        kinds.iter().enumerate().map(|(i, k)| (format!("n{i}"), json!({"kind": k, "pos": [i as f64 * 300.0, 0.0]}))).collect();
    let path = std::path::Path::new(dir).join(name);
    std::fs::write(&path, json!({"version": 1, "nodes": nodes, "wires": []}).to_string()).unwrap();
    path.display().to_string()
}

/// Open `path` as a new document; its number.
fn open(c: &mut Client, path: Option<&str>) -> u64 {
    let r = c.call("document.open", path.map_or_else(|| json!({}), |p| json!({"path": p})));
    assert_eq!(r["ok"], true, "{r}");
    r["result"]["doc"].as_u64().unwrap()
}

/// The engines document `doc` has: the current one's id and the added ones' (answered once its
/// engines have connected, since requests wait for them).
fn engines(c: &mut Client, doc: u64) -> (String, Vec<String>) {
    let l = run_doc(c, doc, "engine.list", json!({}));
    let current = l["current"]["engine"].as_str().unwrap_or_default().to_owned();
    let added = l["added"].as_array().into_iter().flatten().filter_map(|a| a["engine"].as_str().map(str::to_owned)).collect();
    (current, added)
}

#[test]
fn a_document_starts_only_the_engines_its_graph_uses() {
    let root = store_root("eu-open");
    let mut c = Client::open(&json!({"store_root": root, "engines": {"default": "text", "mode": "inproc"}}));
    let text_only = workflow(&root, "text.json", &["text.literal", "text.upper"]);
    let both = workflow(&root, "both.json", &["text.literal", "array.mandelbrot", "array.encode_image"]);
    let array_only = workflow(&root, "array.json", &["array.mandelbrot"]);
    // a graph of text nodes: the text engine alone, never waiting for the array engine
    let d = open(&mut c, Some(&text_only));
    assert_eq!(engines(&mut c, d), ("text".to_owned(), Vec::<String>::new()));
    // text and array nodes, though the file names neither: both, and the graph opens whole
    let d = open(&mut c, Some(&both));
    assert_eq!(engines(&mut c, d), ("text".to_owned(), vec!["array".to_owned()]));
    let info = run_doc(&mut c, d, "graph.info", json!({}));
    assert_eq!((info["nodes"].as_u64(), info["problemCount"].as_u64()), (Some(3), Some(0)), "nothing missing: {info}");
    // array nodes alone: the array engine alone
    let d = open(&mut c, Some(&array_only));
    assert_eq!(engines(&mut c, d), ("array".to_owned(), Vec::<String>::new()));
    // an untitled graph uses nothing yet: the config's engine
    let d = open(&mut c, None);
    assert_eq!(engines(&mut c, d), ("text".to_owned(), Vec::<String>::new()));
    c.close();
}

#[test]
fn adding_a_node_of_another_engines_kind_starts_that_engine_and_waits_for_it() {
    let root = store_root("eu-add");
    let mut c = Client::open(&json!({"store_root": root, "engines": {"default": "text", "mode": "inproc"}}));
    let d = open(&mut c, None);
    run_doc(&mut c, d, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}));
    assert_eq!(engines(&mut c, d).1, Vec::<String>::new());
    // the array engine isn't there: the add starts it, waits for it, and adds the node
    let m = run_doc(&mut c, d, "node.add", json!({"kind": "array.mandelbrot", "pos": [300, 0]}));
    assert!(m["id"].as_str().is_some(), "{m}");
    assert_eq!(engines(&mut c, d), ("text".to_owned(), vec!["array".to_owned()]));
    // the next of its kinds is there at once
    run_doc(&mut c, d, "node.add", json!({"kind": "array.encode_image", "pos": [600, 0]}));
    // a kind no engine here offers is refused at once, as before
    let r = call_doc(&mut c, d, "command.run", json!({"command": "node.add", "params": {"kind": "nowhere.at_all"}}));
    assert_eq!(r["ok"], false, "{r}");
    c.close();
}

#[test]
fn the_palette_lists_an_engine_not_started_and_choosing_from_it_starts_that_engine() {
    let root = store_root("eu-palette");
    let mut c = Client::open(&json!({"store_root": root, "engines": {"default": "text", "mode": "inproc"}}));
    let d = open(&mut c, None);
    run_doc(&mut c, d, "node.add", json!({"kind": "text.literal", "pos": [0, 0]}));
    run_doc(&mut c, d, "canvas.palette", json!({"open": true, "at": [300, 0]}));
    // the array engine's kinds are listed, each saying it starts that engine; the text engine's
    // say nothing of the kind
    let all = call_doc(&mut c, d, "palette.items", json!({"query": "mandelbrot"}));
    let item = all["result"]["items"].as_array().and_then(|a| a.iter().find(|i| i["kind"] == "array.mandelbrot")).cloned();
    assert_eq!(item.as_ref().map(|i| i["starts"].clone()), Some(json!("array")), "{all}");
    let text = call_doc(&mut c, d, "palette.items", json!({"query": "literal"}));
    let lit = text["result"]["items"].as_array().and_then(|a| a.iter().find(|i| i["kind"] == "text.literal")).cloned();
    assert!(lit.is_some_and(|i| i.get("starts").is_none()), "{text}");
    // choosing one starts the engine, waits for it, and adds the node as one step
    let m = run_doc(&mut c, d, "palette.choose", json!({"kind": "array.mandelbrot"}));
    assert!(m["id"].as_str().is_some(), "{m}");
    assert_eq!(engines(&mut c, d), ("text".to_owned(), vec!["array".to_owned()]));
    let info = run_doc(&mut c, d, "graph.info", json!({}));
    assert_eq!(info["nodes"].as_u64(), Some(2), "{info}");
    // and now its kinds are the catalogue's: nothing says it starts
    run_doc(&mut c, d, "canvas.palette", json!({"open": true, "at": [600, 0]}));
    let again = call_doc(&mut c, d, "palette.items", json!({"query": "mandelbrot"}));
    assert!(again["result"]["items"].as_array().is_some_and(|a| a.iter().all(|i| i.get("starts").is_none())), "{again}");
    c.close();
    // picking engines off: nothing is listed that would be refused
    let mut c = Client::open(&json!({"store_root": store_root("eu-palette-off"), "engines": {"default": "text", "mode": "inproc", "auto": false}}));
    let d = open(&mut c, None);
    run_doc(&mut c, d, "canvas.palette", json!({"open": true, "at": [0, 0]}));
    let off = call_doc(&mut c, d, "palette.items", json!({"query": "mandelbrot"}));
    assert_eq!(off["result"]["total"], 0, "{off}");
    c.close();
}

#[test]
fn engines_auto_off_keeps_the_configs_engines_in_every_document() {
    let root = store_root("eu-off");
    let mut c = Client::open(&json!({"store_root": root, "engines": {"default": "text", "mode": "inproc", "auto": false}}));
    let both = workflow(&root, "both.json", &["text.literal", "array.mandelbrot"]);
    let d = open(&mut c, Some(&both));
    assert_eq!(engines(&mut c, d), ("text".to_owned(), Vec::<String>::new()));
    // the add is refused, and says whose kind it is
    let r = call_doc(&mut c, d, "command.run", json!({"command": "node.add", "params": {"kind": "array.mandelbrot"}}));
    assert_eq!(r["ok"], false, "{r}");
    assert!(r["error"].as_str().is_some_and(|e| e.contains("array engine")), "{r}");
    c.close();
    // and `engines.also` still starts its engines beside the first, picking or not
    let mut c = Client::open(&json!({"store_root": store_root("eu-also"), "engines": {"default": "text", "also": ["array"], "auto": false}}));
    let d = open(&mut c, None);
    assert_eq!(engines(&mut c, d), ("text".to_owned(), vec!["array".to_owned()]));
    c.close();
    // hostile `auto` is the config's error
    for bad in [json!("yes"), json!(1), json!([true])] {
        let config = json!({"engines": {"auto": bad}}).to_string();
        assert!(plumcraft_ffi::core::Config::parse(config.as_bytes()).is_err(), "{config}");
    }
}
