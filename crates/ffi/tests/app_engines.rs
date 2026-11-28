//! An application's own engine compiled in (`plumcraft_ffi::app::register`,
//! `docs/shell-interface.md` 4.5): registered before the first core opens, it is offered in
//! process like the built-in engines, a document can start on it, its graph runs, and the files
//! its opener takes open as workflows. The public test engine (`crates/ports`) stands in for an
//! application's engine. One test, since registration is once per process and before any core.

mod common;

use std::path::Path;
use std::time::Duration;

use common::{Client, store_root};
use plumcraft_ffi::app::{App, FileOpener, PackEngine, register, registered};
use serde_json::{Value, json};

fn ports() -> Result<plumcraft_engine::Engine, String> {
    Ok(plumcraft_ports::engine())
}

fn pack(id: &'static str, worker: bool) -> PackEngine {
    PackEngine { id, label: "Ports (an application's engine)", engine: ports, demo: None, worker }
}

/// A `.ptxt` file opens as a graph that traces its text: Traced, seeded with the file's first
/// line, into Result.
fn read_ptxt(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let seed = text.lines().next().unwrap_or_default();
    Ok(json!({"version": 1, "nodes": {
        "t": {"kind": "ports.traced", "values": {"seed": seed}, "pos": [0, 0]},
        "r": {"kind": "ports.result", "pos": [300, 0]},
    }, "wires": [{"from": {"node": "t", "port": "text"}, "to": {"node": "r", "port": "text"}}]})
    .to_string())
}

const PTXT: FileOpener = FileOpener { extensions: &["ptxt"], label: "Traced Text", read: read_ptxt };

#[test]
fn an_applications_engine_registered_before_the_core_opens_is_offered_in_process_and_runs() {
    // refused, and nothing registered: a worker (a shell's process starts no copy of itself), an
    // id an engine already has, a workflow's extension
    assert!(register(App { packs: vec![pack("ports", true)], openers: vec![] }).unwrap_err().contains("in process"));
    assert!(register(App { packs: vec![pack("text", false)], openers: vec![] }).unwrap_err().contains("taken"));
    assert!(register(App { packs: vec![pack("a.b", false)], openers: vec![] }).unwrap_err().contains("plain name"));
    let json_opener = FileOpener { extensions: &["JSON"], label: "Not a workflow", read: read_ptxt };
    assert!(register(App { packs: vec![pack("ports", false)], openers: vec![json_opener] }).unwrap_err().contains(".JSON is taken"));
    assert!(registered().is_empty());
    assert!(plumcraft_session::file_cmds::openers().is_empty());

    // taken; and only once
    register(App { packs: vec![pack("ports", false)], openers: vec![PTXT] }).unwrap();
    assert_eq!(registered(), vec!["ports".to_owned()]);
    assert!(register(App { packs: vec![], openers: vec![] }).unwrap_err().contains("once"));

    // a platform that starts no programs (the iPad's): the engine is offered, in process
    let root = store_root("app-engines");
    let platform = json!({"os": "ios", "subprocess": false, "craft_apps": false, "python": false});
    let mut c = Client::open(&json!({"store_root": root, "platform": platform, "engines": {"default": "ports", "mode": "inproc"}}));
    assert!(register(App::default()).unwrap_err().contains("before the first core opens"));
    let list = c.run("engine.list", json!({}));
    let offered: Vec<&Value> = list["choices"].as_array().unwrap().iter().filter(|o| o["engine"] == "ports").collect();
    assert_eq!(offered.len(), 1, "{list}");
    assert_eq!((offered[0]["mode"].as_str(), offered[0]["engineLabel"].as_str()), (Some("inproc"), Some("Ports (an application's engine)")));
    assert_eq!(list["current"]["engine"], "ports", "{list}");

    // its opener: a .ptxt file opens as its graph, unsaved, and runs on the engine
    let file = Path::new(&root).join("seed.ptxt");
    std::fs::write(&file, "plum and pear\n").unwrap();
    let opened = c.run("graph.open", json!({"path": file.display().to_string(), "discard": true}));
    assert_eq!(opened["nodes"].as_u64(), Some(2), "{opened}");
    let info = c.run("graph.info", json!({}));
    assert_eq!(info["problemCount"].as_u64(), Some(0), "{info}");
    c.run("run.watch", json!({"node": "r", "port": "text"}));
    // a file from elsewhere, as every graph read from a file is (B185): held until approved
    let held = c.run("run.start", json!({}));
    assert!(held.get("held").is_some(), "{held}");
    c.run("graph.trust", json!({}));
    c.run("run.start", json!({}));
    let done = c.call("ui.waitRun", json!({"timeout": 25}));
    assert_eq!(done["ok"], true, "{done}");
    let run = &done["result"];
    assert_eq!(run["errors"].as_u64().unwrap_or(0), 0, "{run}");
    let watched = poll_watched(&mut c);
    // Traced's output, through Result: computed by the engine, not the seed echoed
    assert!(watched.contains("traced plum and pear"), "the result is the engine's: {watched}");
    c.close();
}

/// The watched value's text, as `ui.inspect` gives it, once the run has published it.
fn poll_watched(c: &mut Client) -> String {
    let t0 = std::time::Instant::now();
    loop {
        let i = c.call("ui.inspect", json!({}));
        let text = i["result"]["watched"].to_string();
        if text.contains("traced plum") || t0.elapsed() > Duration::from_secs(20) {
            return text;
        }
        c.wait_wake(Duration::from_millis(100));
    }
}
