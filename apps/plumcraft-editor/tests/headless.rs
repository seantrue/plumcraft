//! The editor without a window, through the executable: what it says of itself, its catalogue, and
//! a run that writes files or says why it can't.

use std::path::PathBuf;
use std::process::{Command, Output};

fn editor(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plumcraft-editor")).args(args).output().unwrap()
}

fn says(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn folder(name: &str) -> PathBuf {
    plumcraft_scratch::dir(&format!("headless-{name}"))
}

#[test]
fn version_and_help_say_what_they_should() {
    let o = editor(&["--version"]);
    assert!(o.status.success());
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), format!("PlumCraft {}", env!("CARGO_PKG_VERSION")));
    let o = editor(&["-h"]);
    assert!(o.status.success());
    for word in ["run FILE", "nodes", "--out", "--engine"] {
        assert!(says(&o).contains(word), "{word}: {}", says(&o));
    }
}

#[test]
fn nodes_lists_the_engine_it_is_given() {
    let o = editor(&["nodes"]);
    assert!(o.status.success(), "{}", says(&o));
    assert!(String::from_utf8_lossy(&o.stdout).lines().any(|l| l.starts_with("text.upper")), "{}", says(&o));
    let o = editor(&["nodes", "--engine", "array,text"]);
    assert!(o.status.success(), "{}", says(&o));
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(text.lines().any(|l| l.starts_with("text.upper")) && text.lines().any(|l| l.starts_with("array.")), "several engines: {text}");
    assert_eq!(editor(&["nodes", "--engine", "nonesuch"]).status.code(), Some(2));
}

#[test]
fn a_run_says_what_stops_it() {
    let d = folder("refused");
    let w = d.join("w.json");
    std::fs::write(&w, r#"{"version": 1, "nodes": {"a": {"kind": "text.literal", "values": {"text": "hi"}}}, "wires": []}"#).unwrap();
    let w = w.to_str().unwrap();
    // an engine with no way to write: the session's own reason
    let o = editor(&["run", w, "--node", "a.text", "--out", d.join("out").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", says(&o));
    assert!(says(&o).contains("doesn't save files"), "{}", says(&o));
    let o = editor(&["run", w]);
    assert_eq!(o.status.code(), Some(1), "{}", says(&o));
    assert!(says(&o).contains("no Result node"), "{}", says(&o));
    let o = editor(&["run", d.join("gone.json").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", says(&o));
    assert!(says(&o).contains("gone.json"), "{}", says(&o));
    // a file naming an engine this run hasn't got says which, and how
    let n = d.join("n.json");
    std::fs::write(&n, r#"{"version": 1, "engines": ["text", "vectorcraft"], "nodes": {"a": {"kind": "vc.open"}}, "wires": []}"#).unwrap();
    let o = editor(&["run", n.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", says(&o));
    assert!(says(&o).contains("--engine text,vectorcraft") && says(&o).contains("vc.open"), "{}", says(&o));
    assert_eq!(editor(&["run"]).status.code(), Some(2));
    assert_eq!(editor(&["run", w, "--bogus"]).status.code(), Some(2));
}
