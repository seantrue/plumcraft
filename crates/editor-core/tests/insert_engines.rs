//! B165: Insert Graph of a graph whose nodes are another engine's, one that isn't connected yet
//! but that the host starts on demand (decision 8C: engines start as the graph comes to use
//! them). The open graph is on the text engine alone; the file is a collapsed group of array
//! nodes with promoted ports and an output, saved as `.plum`. Its nodes, wires and group come in
//! as one undo step, and the array engine is asked for, as opening the same file would. A failed
//! insert says why where a person sees it, never nothing.

use plumcraft_editor_core::control::now;
use plumcraft_editor_core::document::Document;
use plumcraft_editor_core::engines;
use plumcraft_session::Session;
use serde_json::{Value, json};

/// A session on the text engine alone, offering every engine compiled in, picking them as the
/// graph uses them.
fn text_only() -> Session {
    let mut s = Session::new(plumcraft_text::engine().schema());
    s.connected = true;
    let choices = engines::choices();
    s.engines.offered = engines::offered(&choices);
    s.engines.offered_defs = engines::offered_defs(&choices);
    s.engines.choices = choices;
    s.engines.auto = true;
    s
}

/// The file: an array picture loaded, converted, split; a text node beside it; all of it a
/// collapsed group promoting the picture's path and putting out the first channel.
fn component() -> Value {
    json!({"version": 1, "engines": ["array", "text"],
        "nodes": {
            "n26": {"kind": "array.load_image", "pos": [0, 0], "values": {"path": "pic.png"}},
            "n27": {"kind": "array.convert_color_space", "pos": [300, 0]},
            "n28": {"kind": "array.split", "pos": [600, 0]},
            "n29": {"kind": "text.literal", "pos": [0, 300], "values": {"text": "note"}}},
        "wires": [
            {"from": {"node": "n26", "port": "rgb"}, "to": {"node": "n27", "port": "array"}},
            {"from": {"node": "n27", "port": "converted"}, "to": {"node": "n28", "port": "array"}}],
        "groups": {"g4": {"name": "condition", "nodes": ["n26", "n27", "n28", "n29"],
            "promoted": [{"port": {"node": "n26", "port": "path"}, "label": "path"}],
            "outputs": [{"from": {"node": "n28", "port": "c1"}, "name": "first"}],
            "collapsed": true, "pos": [0, 0]}},
        "next_id": 30})
}

fn file(name: &str) -> std::path::PathBuf {
    let d = plumcraft_scratch::dir("b165-insert");
    let p = d.join(name);
    std::fs::write(&p, component().to_string()).unwrap();
    p
}

#[test]
fn a_component_on_an_engine_not_yet_started_inserts_and_starts_it() {
    for name in ["condition.plum", "condition.json"] {
        let mut s = text_only();
        s.execute("node.add", &json!({"kind": "text.upper", "pos": [0, 0]})).unwrap();
        let r = s.execute("graph.insert", &json!({"path": file(name)})).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(r["nodes"].as_array().map(Vec::len), Some(4), "{name}: {r}");
        assert_eq!(r["groups"].as_array().map(Vec::len), Some(1), "{name}: {r}");
        assert_eq!(r["dropped"], json!([]), "{name}: every wire fits: {r}");
        let g = s.graph();
        assert_eq!(g.nodes.len(), 5, "{name}");
        assert_eq!(g.wires.len(), 2, "{name}");
        let group = g.groups.values().next().unwrap();
        assert_eq!((group.name.as_str(), group.nodes.len(), group.promoted.len(), group.outputs.len()), ("condition", 4, 1, 1), "{name}");
        // the engine it needs is asked for, as opening it would
        assert_eq!(r["startedEngines"], json!(["array"]), "{name}: {r}");
        // one undo step
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.graph().nodes.len(), 1, "{name}");
    }
}

#[test]
fn a_kind_no_engine_offers_is_still_refused_with_why() {
    let mut s = text_only();
    let mut g = component();
    g["nodes"]["n26"]["kind"] = json!("nowhere.at_all");
    let p = plumcraft_scratch::dir("b165-insert").join("broken.plum");
    std::fs::write(&p, g.to_string()).unwrap();
    let e = s.execute("graph.insert", &json!({"path": p})).unwrap_err().to_string();
    assert!(e.contains("nowhere.at_all"), "{e}");
    assert_eq!(s.graph().nodes.len(), 0);
}

/// A control method on the document, as the port runs it: its result, or the error as a panic.
fn ctl(d: &mut Document, method: &str, params: Value) -> Value {
    let r = now(d, method, &params);
    assert_eq!(r["ok"], true, "{method}: {r}");
    r["result"].clone()
}

/// File › Insert Graph… as a person chooses it: the file ask, answered; a refusal is said (the
/// document's last error), not dropped.
#[test]
fn insert_graph_from_the_menu_says_why_it_refused() {
    let mut d = Document::new(text_only());
    let p = plumcraft_scratch::dir("b165-insert").join("nothing.plum");
    std::fs::write(&p, r#"{"version": 1, "nodes": {}, "wires": []}"#).unwrap();
    ctl(&mut d, "ui.invoke", json!({"command": "graph.insert"}));
    let asks = ctl(&mut d, "ui.inspect", json!({}))["asks"].clone();
    let ask = asks.as_array().and_then(|a| a.first()).cloned().unwrap_or_else(|| panic!("no ask: {asks}"));
    ctl(&mut d, "ask.answer", json!({"ask": ask["ask"], "answer": {"paths": [p]}}));
    let said = ctl(&mut d, "ui.inspect", json!({}))["lastError"].clone();
    assert!(said.as_str().is_some_and(|t| t.contains("no nodes")), "the refusal is said: {said}");
    // and a good file from the menu comes in
    ctl(&mut d, "ui.invoke", json!({"command": "graph.insert"}));
    let asks = ctl(&mut d, "ui.inspect", json!({}))["asks"].clone();
    let ask = asks.as_array().and_then(|a| a.first()).cloned().unwrap();
    ctl(&mut d, "ask.answer", json!({"ask": ask["ask"], "answer": {"paths": [file("menu.plum")]}}));
    assert_eq!(d.session.graph().nodes.len(), 4);
}
