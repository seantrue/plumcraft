//! `plumcraft --engine-manifest`, through the real binary: an engine outside this repository's
//! build, started from a manifest alone (spec 16.6). First the Python text engine, then a project
//! `plumcraft-engine new` generates, on a Unix socket and over HTTP. Needs uv, as the Python
//! conformance cases do.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn plumcraft(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plumcraft")).args(args).output().expect("plumcraft runs")
}

fn stdout(o: &Output) -> String {
    format!("{}\n--- stderr\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn python_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python").canonicalize().expect("python/ is here")
}

#[test]
fn the_python_text_engine_runs_from_a_manifest() {
    let dir = plumcraft_scratch::dir("cli-manifest-text");
    let (command, _) = plumcraft_text::python_command();
    let words: Vec<String> = command.iter().map(|c| format!("{c:?}")).collect();
    let manifest = dir.join("text.toml");
    std::fs::write(&manifest, format!("id = \"pytext\"\nname = \"Text in Python\"\ncommand = [{}]\n", words.join(", "))).unwrap();
    let workflow = dir.join("w.json");
    let graph = r#"{"nodes": {"a": {"kind": "text.literal", "values": {"text": "far away"}}, "b": {"kind": "text.upper"}},
                    "wires": [{"from": {"node": "a", "port": "text"}, "to": {"node": "b", "port": "text"}}]}"#;
    std::fs::write(&workflow, graph).unwrap();
    let m = manifest.display().to_string();
    let o = plumcraft(&["nodes", "--engine-manifest", &m]);
    assert!(o.status.success() && stdout(&o).contains("text.upper"), "{}", stdout(&o));
    let o = plumcraft(&["run", &workflow.display().to_string(), "--engine-manifest", &m, "--watch", "b.text"]);
    assert!(o.status.success() && stdout(&o).contains("| FAR AWAY"), "{}", stdout(&o));
    // No workflow, and no demo in this manifest: says so.
    let o = plumcraft(&["run", "--engine-manifest", &m]);
    assert!(!o.status.success() && stdout(&o).contains("no demo"), "{}", stdout(&o));
}

#[test]
fn a_bad_manifest_says_why() {
    let dir = plumcraft_scratch::dir("cli-manifest-bad");
    let manifest = dir.join("bad.toml");
    std::fs::write(&manifest, "id = \"Bad Id\"\ncommand = [\"x\"]\n").unwrap();
    let o = plumcraft(&["nodes", "--engine-manifest", &manifest.display().to_string()]);
    assert!(!o.status.success() && stdout(&o).contains("lower-case"), "{}", stdout(&o));
    let gone = dir.join("gone.toml").display().to_string();
    assert!(!plumcraft(&["nodes", "--engine-manifest", &gone]).status.success());
}

#[test]
fn a_generated_engine_runs_its_demo_on_a_socket_and_over_http() {
    let dest = plumcraft_scratch::dir("cli-manifest-generated").join("weather-engine");
    let python = python_dir();
    let uv = std::env::var("PLUMCRAFT_UV").unwrap_or_else(|_| "uv".into());
    let new = Command::new(&uv)
        .args(["run", "--project"])
        .arg(&python)
        .args(["plumcraft-engine", "new"])
        .arg(&dest)
        .args(["--name", "Weather", "--types", "text,svg,audio"])
        .output();
    let new = new.expect("uv runs");
    assert!(new.status.success(), "{}", stdout(&new));
    let manifest = dest.join("plumcraft-engine.toml").display().to_string();
    let o = plumcraft(&["run", "--engine-manifest", &manifest, "--watch", "loud.text"]);
    assert!(o.status.success() && stdout(&o).contains("| A CORGI NAPS!!!"), "{}", stdout(&o));
    let o = plumcraft(&["run", "--engine-manifest", &manifest, "--http", "--watch", "green.svg"]);
    assert!(o.status.success() && stdout(&o).contains("image/svg+xml"), "{}", stdout(&o));
}

fn generate(dest: &Path, types: &str) {
    let uv = std::env::var("PLUMCRAFT_UV").unwrap_or_else(|_| "uv".into());
    let new = Command::new(&uv)
        .args(["run", "--project"])
        .arg(python_dir())
        .args(["plumcraft-engine", "new"])
        .arg(dest)
        .args(["--types", types])
        .output()
        .expect("uv runs");
    assert!(new.status.success(), "{}", stdout(&new));
}

#[test]
fn conform_holds_a_generated_engine_to_every_case_on_both_bindings() {
    let dest = plumcraft_scratch::dir("cli-conform").join("river-engine");
    generate(&dest, "text,array");
    let o = plumcraft(&["conform", "--engine-manifest", &dest.join("plumcraft-engine.toml").display().to_string()]);
    let out = stdout(&o);
    assert!(o.status.success(), "{out}");
    assert!(
        out.contains("18 of 18 cases hold") && out.contains("ok    process: kill -9 mid-run recovers") && out.contains("ok    http: a wrong token is refused"),
        "{out}"
    );
}

#[test]
fn conform_fails_an_engine_that_ignores_cancel_naming_the_case() {
    let dest = plumcraft_scratch::dir("cli-conform-broken").join("deaf-engine");
    generate(&dest, "text");
    let source = dest.join("src/deaf_engine/__init__.py");
    let text = std::fs::read_to_string(&source).unwrap();
    assert!(text.contains("if cx.cancelled():"), "the generated slow node changed");
    std::fs::write(&source, text.replace("if cx.cancelled():", "if False:")).unwrap();
    let m = dest.join("plumcraft-engine.toml").display().to_string();
    let o = plumcraft(&["conform", "--engine-manifest", &m, "--modes", "process"]);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(1), "{out}");
    assert!(out.contains("FAIL  process: cancel mid-run"), "{out}");
    assert!(out.contains("ok    process: hello and schema"), "the rest still report: {out}");
}

#[test]
fn conform_says_what_it_needs() {
    let dir = plumcraft_scratch::dir("cli-conform-needs");
    assert!(stdout(&plumcraft(&["conform"])).contains("--engine-manifest"));
    let attach = dir.join("attach.toml");
    std::fs::write(&attach, "id = \"far\"\nmode = \"http\"\nurl = \"http://127.0.0.1:1\"\n").unwrap();
    let o = plumcraft(&["conform", "--engine-manifest", &attach.display().to_string()]);
    assert!(!o.status.success() && stdout(&o).contains("no command"), "{}", stdout(&o));
    let (command, _) = plumcraft_text::python_command();
    let words: Vec<String> = command.iter().map(|c| format!("{c:?}")).collect();
    let noslow = dir.join("noslow.toml");
    std::fs::write(&noslow, format!("id = \"quick\"\ncommand = [{}]\n", words.join(", "))).unwrap();
    let o = plumcraft(&["conform", "--engine-manifest", &noslow.display().to_string(), "--modes", "process"]);
    assert!(!o.status.success() && stdout(&o).contains("no node quick.slow"), "{}", stdout(&o));
    let o = plumcraft(&["conform", "--engine-manifest", &noslow.display().to_string(), "--modes", "pigeon"]);
    assert!(!o.status.success() && stdout(&o).contains("pigeon"), "{}", stdout(&o));
}
