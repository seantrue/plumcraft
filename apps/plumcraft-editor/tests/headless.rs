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
    // (with no --engine, run would start the engines the file names; naming one keeps them out)
    let o = editor(&["run", n.to_str().unwrap(), "--engine", "text"]);
    assert_eq!(o.status.code(), Some(1), "{}", says(&o));
    assert!(says(&o).contains("--engine text,vectorcraft") && says(&o).contains("vc.open"), "{}", says(&o));
    assert_eq!(editor(&["run"]).status.code(), Some(2));
    assert_eq!(editor(&["run", w, "--bogus"]).status.code(), Some(2));
}

/// A workflow that ends in Write Files: `run` with no Result node runs it, on the engines the file
/// names, and `--out` is where it writes.
#[test]
fn a_run_with_no_result_node_runs_its_write_files_nodes() {
    let d = folder("writers");
    let w = d.join("w.json");
    std::fs::write(
        &w,
        r#"{"version": 1, "engines": ["text"],
            "nodes": {"a": {"kind": "text.literal", "values": {"text": "hello"}},
                      "up": {"kind": "text.upper"},
                      "save": {"kind": "file.write", "values": {"folder": "elsewhere", "basename": "shout", "overwrite": true}}},
            "wires": [{"from": {"node": "a", "port": "text"}, "to": {"node": "up", "port": "text"}},
                      {"from": {"node": "up", "port": "text"}, "to": {"node": "save", "port": "values"}}]}"#,
    )
    .unwrap();
    let out = d.join("out");
    let o = editor(&["run", w.to_str().unwrap(), "--out", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", says(&o));
    assert_eq!(std::fs::read_to_string(out.join("shout.txt")).unwrap(), "HELLO");
    // without --out it writes where the node says, relative to the workflow's folder
    let o = editor(&["run", w.to_str().unwrap()]);
    assert!(o.status.success(), "{}", says(&o));
    assert_eq!(std::fs::read_to_string(d.join("elsewhere").join("shout.txt")).unwrap(), "HELLO");
}

#[test]
fn check_says_what_would_stop_a_run_and_tidy_lays_it_out() {
    let d = folder("check");
    let w = d.join("w.json");
    std::fs::write(
        &w,
        r#"{"version": 1, "engines": ["text"],
            "nodes": {"a": {"kind": "text.literal", "values": {"text": "hi"}, "pos": [0, 0]},
                      "loose": {"kind": "text.upper", "pos": [0, 0]},
                      "nope": {"kind": "text.lower", "values": {"bogus": 1}, "pos": [0, 0]},
                      "save": {"kind": "file.write", "values": {"folder": "out"}, "pos": [0, 0]}},
            "wires": [{"from": {"node": "a", "port": "text"}, "to": {"node": "save", "port": "values"}},
                      {"from": {"node": "a", "port": "text"}, "to": {"node": "nope", "port": "text"}}]}"#,
    )
    .unwrap();
    let o = editor(&["check", w.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", says(&o));
    let text = says(&o);
    assert!(text.contains(r#"node loose (text.upper): input "text" needs a wire"#), "{text}");
    assert!(text.contains(r#"no input "bogus""#), "{text}");
    assert!(text.contains("node nope: no Result or Write Files node depends on it"), "{text}");
    let good = d.join("good.json");
    std::fs::write(&good, r#"{"version": 1, "engines": ["text"], "nodes": {"a": {"kind": "text.literal", "values": {"text": "hi"}}, "b": {"kind": "file.write", "values": {"folder": "out"}}}, "wires": [{"from": {"node": "a", "port": "text"}, "to": {"node": "b", "port": "values"}}]}"#).unwrap();
    let o = editor(&["check", good.to_str().unwrap()]);
    assert!(o.status.success(), "{}", says(&o));
    let o = editor(&["tidy", good.to_str().unwrap()]);
    assert!(o.status.success(), "{}", says(&o));
    let g: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&good).unwrap()).unwrap();
    assert_ne!(g["nodes"]["a"]["pos"], g["nodes"]["b"]["pos"], "tidied apart: {g}");
    assert_eq!(g["engines"], serde_json::json!(["text"]), "the engines are kept: {g}");
}

/// A wire the types refuse says what would fix it when a converter can
/// (docs/research/20-convert.md): a WAV into a Gain needs WAV to Audio. Skips without the audio
/// engine.
#[test]
fn check_names_the_converter_a_refused_wire_needs() {
    if editor(&["nodes", "--engine", "audio"]).status.code() != Some(0) {
        eprintln!("SKIP: no audio engine");
        return;
    }
    let d = folder("check-convert");
    let w = d.join("w.json");
    std::fs::write(
        &w,
        r#"{"version": 1, "engines": ["audio"],
            "nodes": {"tone": {"kind": "audio.tone", "pos": [0, 0]},
                      "wav": {"kind": "audio.to_wav", "pos": [200, 0]},
                      "gain": {"kind": "audio.gain", "pos": [400, 0]},
                      "save": {"kind": "file.write", "values": {"folder": "out"}, "pos": [600, 0]}},
            "wires": [{"from": {"node": "tone", "port": "audio"}, "to": {"node": "wav", "port": "audio"}},
                      {"from": {"node": "wav", "port": "wav"}, "to": {"node": "gain", "port": "audio"}},
                      {"from": {"node": "gain", "port": "audio"}, "to": {"node": "save", "port": "values"}}]}"#,
    )
    .unwrap();
    let o = editor(&["check", w.to_str().unwrap()]);
    let text = says(&o);
    assert_eq!(o.status.code(), Some(1), "{text}");
    assert!(text.contains("wire wav.wav -> gain.audio") && text.contains("wire.convert would insert WAV to Audio"), "{text}");
    assert_eq!(text.matches("wire wav.wav -> gain.audio").count(), 1, "said once: {text}");
}
