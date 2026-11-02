//! A workflow from elsewhere doesn't run until someone approves it, by any route.

use std::path::{Path, PathBuf};

use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_session::Session;
use plumcraft_session::run::Outgoing;
use serde_json::{Value, json};

fn catalog() -> SchemaReply {
    serde_json::from_value(json!({"v": 2, "nodes": [
        {"id": "t.text", "inputs": [{"name": "text", "type": "STRING", "default": "", "socket": "widget"}], "outputs": [{"name": "text", "type": "TEXT"}]},
        {"id": "t.load", "inputs": [{"name": "path", "type": "STRING", "default": "", "socket": "both", "path": {"mode": "open"}}], "outputs": [{"name": "text", "type": "TEXT"}]},
        {"id": "t.write", "flags": {"output_node": true}, "memo": {"scope": "none"}, "inputs": [
            {"name": "text", "type": "TEXT", "socket": "socket"},
            {"name": "folder", "type": "STRING", "default": "", "socket": "both", "path": {"mode": "dir"}}], "outputs": [{"name": "files", "type": "TEXT"}]}
    ]}))
    .unwrap()
}

/// A folder of this test's own: `name` keeps parallel tests off each other's trust file.
fn dir(name: &str) -> PathBuf {
    plumcraft_scratch::dir(&format!("trust-{name}"))
}

/// A stranger's workflow: reads a file, writes it into a folder.
fn stranger(at: &Path) -> PathBuf {
    let file = at.join("stranger.json");
    let g = json!({"version": 1, "nodes": {
        "a": {"kind": "t.load", "values": {"path": "/etc/secrets"}},
        "b": {"kind": "t.write", "values": {"folder": "/home/me/.config"}}},
        "wires": [{"from": {"node": "a", "port": "text"}, "to": {"node": "b", "port": "text"}}]});
    std::fs::write(&file, g.to_string()).unwrap();
    file
}

fn session(at: &Path) -> Session {
    let mut s = Session::new(catalog());
    s.connected = true;
    s.set_trust_file(at.join("trusted.json"));
    s
}

fn runs(s: &mut Session) -> usize {
    s.run.take_outbox().into_iter().filter(|o| matches!(o, Outgoing::Run(_))).count()
}

fn open(s: &mut Session, f: &Path) -> Value {
    s.execute("graph.open", &json!({"path": f, "discard": true})).unwrap()
}

#[test]
fn a_stranger_is_held_and_says_what_it_touches() {
    let at = dir("a_stranger_is_held_and_says_what_it_touches");
    let mut s = session(&at);
    open(&mut s, &stranger(&at));
    let r = s.execute("run.start", &json!({})).unwrap();
    assert!(r.get("held").is_some(), "{r}");
    assert_eq!(runs(&mut s), 0, "nothing reaches the engine");
    assert!(s.run.awaiting_review());
    let review = s.execute("graph.review", &json!({})).unwrap();
    assert_eq!(review["pending"], true);
    assert_eq!(review["reads"][0]["path"], "/etc/secrets");
    assert_eq!(review["writes"][0]["path"], "/home/me/.config");
    assert_eq!(review["actions"][0]["kind"], "t.write");
    s.execute("graph.trust", &json!({})).unwrap();
    assert_eq!(runs(&mut s), 1, "what was held goes out");
    assert!(!s.run.awaiting_review());
}

#[test]
fn auto_run_and_a_second_start_stay_held() {
    let at = dir("auto_run_and_a_second_start_stay_held");
    let mut s = session(&at);
    open(&mut s, &stranger(&at));
    s.execute("run.auto", &json!({"on": true})).unwrap();
    s.execute("run.start", &json!({})).unwrap();
    s.execute("run.start", &json!({})).unwrap();
    assert_eq!(runs(&mut s), 0);
    s.execute("graph.trust", &json!({})).unwrap();
    assert_eq!(runs(&mut s), 1, "latest wins, and only it");
}

#[test]
fn a_declined_review_sends_nothing_and_asks_again() {
    let at = dir("a_declined_review_sends_nothing_and_asks_again");
    let mut s = session(&at);
    open(&mut s, &stranger(&at));
    s.execute("run.start", &json!({})).unwrap();
    s.run.decline_review();
    assert!(!s.run.running && !s.run.awaiting_review());
    let told = s.run.take_outbox();
    assert!(!told.iter().any(|o| matches!(o, Outgoing::Run(_) | Outgoing::Cancel(_))), "the engine never heard of it");
    s.execute("run.start", &json!({})).unwrap();
    assert!(s.run.awaiting_review());
    assert_eq!(runs(&mut s), 0);
}

#[test]
fn approval_lasts_until_the_file_changes() {
    let at = dir("approval_lasts_until_the_file_changes");
    let f = stranger(&at);
    let mut s = session(&at);
    open(&mut s, &f);
    s.execute("graph.trust", &json!({})).unwrap();
    let mut again = session(&at);
    open(&mut again, &f);
    again.execute("run.start", &json!({})).unwrap();
    assert_eq!(runs(&mut again), 1, "known by its content");
    // the same file, changed by someone else
    let text = std::fs::read_to_string(&f).unwrap().replace("/home/me/.config", "/home/me/.ssh");
    std::fs::write(&f, text).unwrap();
    let mut changed = session(&at);
    open(&mut changed, &f);
    changed.execute("run.start", &json!({})).unwrap();
    assert_eq!(runs(&mut changed), 0);
}

#[test]
fn saving_keeps_trust_and_never_grants_it() {
    let at = dir("saving_keeps_trust_and_never_grants_it");
    let (mine, theirs) = (at.join("mine.json"), at.join("theirs.json"));
    let mut s = session(&at);
    let added = s.execute("node.add", &json!({"kind": "t.text"})).unwrap();
    s.execute("run.start", &json!({})).unwrap();
    assert_eq!(runs(&mut s), 1, "a graph started here runs: {added}");
    s.execute("graph.saveAs", &json!({"path": mine})).unwrap();
    let mut reopened = session(&at);
    open(&mut reopened, &mine);
    reopened.execute("run.start", &json!({})).unwrap();
    assert_eq!(runs(&mut reopened), 1, "my own file, reopened");

    let mut t = session(&at);
    open(&mut t, &stranger(&at));
    t.execute("graph.saveAs", &json!({"path": theirs})).unwrap();
    let mut laundered = session(&at);
    open(&mut laundered, &theirs);
    laundered.execute("run.start", &json!({})).unwrap();
    assert_eq!(runs(&mut laundered), 0, "saving a stranger's graph doesn't approve it");
}

#[test]
fn neither_new_and_undo_nor_insert_get_round_it() {
    let at = dir("neither_new_and_undo_nor_insert_get_round_it");
    let f = stranger(&at);
    let mut s = session(&at);
    open(&mut s, &f);
    s.execute("graph.new", &json!({"discard": true})).unwrap();
    assert!(s.execute("edit.undo", &json!({})).is_err() || s.graph().nodes.is_empty(), "no way back to it");

    let mut mine = session(&at);
    mine.execute("node.add", &json!({"kind": "t.text"})).unwrap();
    mine.execute("graph.insert", &json!({"path": f})).unwrap();
    mine.execute("run.start", &json!({})).unwrap();
    assert_eq!(runs(&mut mine), 0, "a stranger inserted into my graph");
}

#[test]
fn a_path_another_node_sets_is_said_to_be() {
    let at = dir("a_path_another_node_sets_is_said_to_be");
    let file = at.join("wired.json");
    let g = json!({"version": 1, "nodes": {
        "dest": {"kind": "t.text", "values": {"text": "/home/me/.ssh"}},
        "src": {"kind": "t.text", "values": {"text": "x"}},
        "w": {"kind": "t.write"}},
        "wires": [{"from": {"node": "dest", "port": "text"}, "to": {"node": "w", "port": "folder"}},
                  {"from": {"node": "src", "port": "text"}, "to": {"node": "w", "port": "text"}}]});
    std::fs::write(&file, g.to_string()).unwrap();
    let mut s = session(&at);
    open(&mut s, &file);
    let r = s.execute("graph.review", &json!({})).unwrap();
    assert_eq!(r["writes"][0]["path"], Value::Null);
    assert!(r["writes"][0]["setBy"].as_str().is_some(), "{r}");
}

#[test]
fn without_a_trust_file_nothing_is_held() {
    let at = dir("without_a_trust_file_nothing_is_held");
    let mut s = Session::new(catalog());
    s.connected = true;
    open(&mut s, &stranger(&at));
    s.execute("run.start", &json!({})).unwrap();
    assert_eq!(runs(&mut s), 1);
}
