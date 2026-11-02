//! An application's file openers (`file_cmds::FileOpener`): a file that isn't a workflow opens as
//! one, unsaved; what's wrong with it is a refusal that leaves the open graph alone; and a graph
//! that needs an engine that isn't here says which.

use std::path::{Path, PathBuf};
use std::sync::Once;

use plumcraft_session::file_cmds::{FileOpener, MAX_WORKFLOW_BYTES, read_any, register_openers};
use plumcraft_session::{EngineChoice, EngineTarget, Session};
use serde_json::{Value, json};

/// The file holds a graph after a line of nonsense; `BAD` refuses, `BOOM` panics, `HUGE` overflows.
fn read(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let body = text.strip_prefix("PIC\n").ok_or("it holds no graph")?;
    match body.trim() {
        "BOOM" => panic!("a hostile file"),
        "HUGE" => Ok(" ".repeat(MAX_WORKFLOW_BYTES as usize + 1)),
        _ => Ok(body.to_owned()),
    }
}

fn setup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| register_openers(vec![FileOpener { extensions: &["pic", "PICT"], label: "a picture", read }]).unwrap());
}

fn dir(name: &str) -> PathBuf {
    plumcraft_scratch::dir(&format!("openers-{name}"))
}

fn session() -> Session {
    setup();
    let mut s = Session::new(plumcraft_text::engine().schema());
    s.connected = true;
    s
}

fn graph(kinds: &[&str]) -> Value {
    let nodes: serde_json::Map<String, Value> = kinds.iter().enumerate().map(|(i, k)| (format!("n{i}"), json!({"kind": k}))).collect();
    json!({"version": 1, "nodes": nodes, "wires": []})
}

fn picture(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("PIC\n{body}")).unwrap();
    path
}

#[test]
fn a_file_an_application_opens_comes_in_unsaved() {
    let d = dir("open");
    let mut s = session();
    let path = picture(&d, "a.pic", &graph(&["text.literal", "text.upper"]).to_string());
    let out = s.execute("graph.open", &json!({"path": path})).unwrap();
    assert_eq!(s.graph().nodes.len(), 2);
    assert_eq!(out["openedFrom"], json!(path.display().to_string()));
    assert_eq!(out["problemCount"], 0);
    assert!(s.path().is_none() && !s.is_dirty(), "unsaved: Save asks where");
    assert!(s.execute("graph.save", &json!({})).is_err(), "and never writes over the picture");
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("PIC\n"));
    // any case of extension, and a real file name
    let upper = picture(&d, "b.Pict", &graph(&["text.literal"]).to_string());
    assert_eq!(s.execute("graph.open", &json!({"path": upper, "discard": true})).unwrap()["nodes"], 1);
    // and what the command line reads
    assert_eq!(read_any(&path).unwrap().nodes.len(), 2);
}

#[test]
fn what_a_file_lacks_is_a_refusal_that_says_so_and_leaves_the_graph() {
    let d = dir("refuse");
    let mut s = session();
    s.execute("node.add", &json!({"kind": "text.literal"})).unwrap();
    let before = s.graph().clone();
    let cases = [
        ("no.pic", "PLAIN".to_owned(), "it holds no graph"),
        ("boom.pic", "PIC\nBOOM".to_owned(), "couldn't be read"),
        ("huge.pic", "PIC\nHUGE".to_owned(), "larger than"),
        ("junk.pic", "PIC\n{not json".to_owned(), "not a workflow"),
        ("wrong.pic", "PIC\n[1, 2, 3]".to_owned(), "can't open"),
        (
            "wire.pic",
            "PIC\n{\"version\": 1, \"nodes\": {}, \"wires\": [{\"from\": {\"node\": \"x\", \"port\": \"p\"}, \"to\": {\"node\": \"y\", \"port\": \"q\"}}]}"
                .to_owned(),
            "can't open",
        ),
    ];
    for (name, body, said) in cases {
        let path = d.join(name);
        std::fs::write(&path, body).unwrap();
        let e = s.execute("graph.open", &json!({"path": path, "discard": true})).unwrap_err().to_string();
        assert!(e.contains(said) && e.contains(name), "{name}: {e}");
        assert_eq!(s.graph(), &before, "{name}");
    }
    let gone = d.join("gone.pic");
    assert!(s.execute("graph.open", &json!({"path": gone, "discard": true})).is_err());
    let folder = d.join("folder.pic");
    std::fs::create_dir_all(&folder).unwrap();
    assert!(s.execute("graph.open", &json!({"path": folder, "discard": true})).is_err(), "never read as a file");
    assert_eq!(s.graph(), &before);
    // unsaved changes are asked about first, as any open
    let good = picture(&d, "good.pic", &graph(&["text.upper"]).to_string());
    assert!(s.execute("graph.open", &json!({"path": good})).is_err());
    assert_eq!(s.graph(), &before);
}

#[test]
fn a_workflow_stays_a_workflow() {
    setup();
    for taken in [&["new", "json"], &["new", "ZIP"], &["new", "pic"], &["new", "pict"]] {
        let e = register_openers(vec![FileOpener { extensions: taken, label: "x", read }]).unwrap_err();
        assert!(e.contains(taken[1]), "{e}");
    }
}

#[test]
fn a_graph_needing_an_engine_says_which_and_a_session_with_it_says_nothing() {
    let d = dir("engines");
    let mut s = session();
    let mut whole = graph(&["text.literal", "vc.open", "vc.simplify"]);
    whole["engines"] = json!(["text", "vectorcraft"]);
    let path = picture(&d, "painted.pic", &whole.to_string());
    let out = s.execute("graph.open", &json!({"path": path})).unwrap();
    let m = &out["missingEngines"];
    assert_eq!(m["engines"], json!(["vectorcraft"]), "the engine that's here isn't named: {out}");
    assert_eq!(m["kinds"], json!(["vc.open", "vc.simplify"]));
    let text = m["text"].as_str().unwrap();
    assert!(text.contains("vectorcraft") && text.contains("vc.open") && text.contains("can't start"), "{text}");
    assert_eq!(s.graph().nodes.len(), 3, "it opens all the same, so an engine can be added");

    // offered by the Engine menu, it says that instead
    s.engines.choices = vec![EngineChoice { engine: "vectorcraft".into(), mode: "process".into(), engine_label: "VectorCraft".into(), mode_label: "".into() }];
    let text = s.missing_engines().unwrap().text;
    assert!(text.contains("Add Engine") && !text.contains("can't start"), "{text}");

    // once it's connected nothing is missing
    let mut merged = s.catalog().clone();
    for kind in ["vc.open", "vc.simplify"] {
        let mut n = merged.nodes[0].clone();
        n.id = kind.into();
        n.engines = vec!["vectorcraft".into()];
        merged.nodes.push(n);
    }
    s.set_catalog(merged);
    assert_eq!(s.missing_engines(), None);

    // a node pinned to an engine names it, with no list in the file
    let mut pinned = graph(&["text.literal", "au.tone"]);
    pinned["nodes"]["n1"]["engine"] = json!("audio");
    let path = picture(&d, "pinned.pic", &pinned.to_string());
    let out = session().execute("graph.open", &json!({"path": path})).unwrap();
    assert_eq!(out["missingEngines"]["engines"], json!(["audio"]));

    // a file that names none says that, rather than guessing from a prefix
    let path = picture(&d, "unnamed.pic", &graph(&["vc.open"]).to_string());
    let out = session().execute("graph.open", &json!({"path": path})).unwrap();
    assert_eq!(out["missingEngines"]["engines"], json!([]));
    assert!(out["missingEngines"]["text"].as_str().unwrap().contains("doesn't say"), "{out}");

    // an added engine is here, though the catalogue doesn't say so yet
    let mut s = session();
    s.engines.added.push(EngineTarget::Choice { engine: "vectorcraft".into(), mode: "process".into() });
    let path = picture(&d, "again.pic", &whole.to_string());
    let out = s.execute("graph.open", &json!({"path": path})).unwrap();
    assert_eq!(out["missingEngines"]["engines"], json!([]), "{out}");
}

#[test]
fn a_workflow_needing_an_engine_says_which_too() {
    let d = dir("workflow");
    let mut s = session();
    let mut g = graph(&["vc.open"]);
    g["engines"] = json!(["vectorcraft"]);
    let path = d.join("w.json");
    std::fs::write(&path, g.to_string()).unwrap();
    let out = s.execute("graph.open", &json!({"path": path})).unwrap();
    assert_eq!(out["missingEngines"]["engines"], json!(["vectorcraft"]));
    assert!(out.get("openedFrom").is_none());
    assert_eq!(s.path(), Some(path.as_path()), "a workflow is its own file");
}

#[test]
fn inserting_reads_through_an_opener() {
    let d = dir("insert");
    let mut s = session();
    s.execute("node.add", &json!({"kind": "text.literal"})).unwrap();
    let path = picture(&d, "a.pic", &graph(&["text.upper", "text.lower"]).to_string());
    s.execute("graph.insert", &json!({"path": path})).unwrap();
    assert_eq!(s.graph().nodes.len(), 3);
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().next(), Some("PIC"));
    let bad = d.join("bad.pic");
    std::fs::write(&bad, "PLAIN").unwrap();
    assert!(s.execute("graph.insert", &json!({"path": bad})).is_err());
    assert_eq!(s.graph().nodes.len(), 3);
}

/// Opening is linear in what a file holds. Giving each node its serial searched every other node,
/// which took hours on a file this size.
#[test]
fn a_very_large_graph_opens_in_the_time_a_person_waits() {
    let d = dir("large");
    let n = 200_000;
    let nodes: serde_json::Map<String, Value> = (0..n).map(|i| (format!("n{i}"), json!({"kind": "text.nope"}))).collect();
    let path = d.join("large.json");
    std::fs::write(&path, json!({"version": 1, "nodes": nodes, "wires": []}).to_string()).unwrap();
    let mut s = session();
    let t = std::time::Instant::now();
    s.execute("graph.open", &json!({"path": path})).unwrap();
    assert!(t.elapsed() < std::time::Duration::from_secs(30), "{:?}", t.elapsed());
    assert_eq!(s.graph().nodes.len(), n);
}
