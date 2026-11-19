//! Switching engines over the C ABI: `engine.*` commands are performed (slice 2 only recorded
//! them), the engines start on the core's engine worker, never on its core thread, and a request
//! that switches replies once it has switched, as the editor's control channel does.

mod common;

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use common::{Client, store_root};
use serde_json::{Value, json};

fn command(c: &mut Client, id: &str, params: Value) -> Value {
    c.call("command.run", json!({"command": id, "params": params}))
}

/// literal → upper case, run, and both done.
fn runs_text(c: &mut Client) {
    let lit = c.run("node.add", json!({"kind": "text.literal", "pos": [0, 0]}))["id"].as_str().unwrap().to_owned();
    c.run("node.set", json!({"node": lit, "port": "text", "value": "corgi"}));
    let up = c.run("node.add", json!({"kind": "text.upper", "pos": [260, 0]}))["id"].as_str().unwrap().to_owned();
    c.run("wire.connect", json!({"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}}));
    c.run("run.start", json!({"watch": [{"node": up, "port": "text"}]}));
    let done = c.call("spike.waitRun", json!({"timeout_ms": 30_000}));
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(done["result"]["nodes"][&up]["state"], "done", "{done}");
}

#[test]
fn engine_commands_switch_add_remove_and_clear() {
    let mut c = Client::open(&json!({"store_root": store_root("switch")}));
    // the array engine, compiled in: a platform that declared nothing may have it
    let r = command(&mut c, "engine.switch", json!({"engine": "array", "mode": "inproc"}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!((r["result"]["engine"].as_str(), r["result"]["connected"].as_bool()), (Some("array"), Some(true)), "{r}");
    c.run("node.add", json!({"kind": "array.arithmetic", "pos": [0, 0]}));
    // the text engine beside it, behind a coordinator, and out again
    let r = command(&mut c, "engine.add", json!({"engine": "text", "mode": "inproc"}));
    assert_eq!(r["result"]["engines"], json!(["array", "text"]), "{r}");
    c.run("node.add", json!({"kind": "text.upper", "pos": [0, 200]}));
    let r = command(&mut c, "engine.remove", json!({"engine": "text"}));
    assert_eq!((r["result"]["engines"].clone(), r["result"]["removed"].clone()), (json!(["array"]), json!("text")), "{r}");
    // what this platform can't start isn't offered: refused, naming what is
    let r = command(&mut c, "engine.switch", json!({"engine": "text", "mode": "process"}));
    assert_eq!(r["ok"], false, "{r}");
    assert!(r["error"].as_str().unwrap().contains("the choices: text/inproc, array/inproc"), "{r}");
    // clear: no engine, nothing runs
    let r = command(&mut c, "engine.clear", json!({}));
    assert_eq!(r["result"]["label"], "engine: not connected", "{r}");
    assert_eq!(command(&mut c, "run.start", json!({}))["ok"], false);
    // back to text, which runs
    let r = command(&mut c, "engine.switch", json!({"engine": "text", "mode": "inproc"}));
    assert_eq!(r["result"]["connected"], true, "{r}");
    c.run("graph.new", json!({"discard": true}));
    runs_text(&mut c);
    // hostile: no such engine or hosting
    assert_eq!(command(&mut c, "engine.switch", json!({"engine": "nonesuch", "mode": "inproc"}))["ok"], false);
    assert_eq!(command(&mut c, "engine.switch", json!({"engine": "text", "mode": "telegraph"}))["ok"], false);
    c.close();
}

/// The text engine's binary, built if this test binary was built without it.
fn engine_binary() {
    static BUILT: OnceLock<()> = OnceLock::new();
    BUILT.get_or_init(|| {
        if plumcraft_editor_core::engines::engine_binary("plumcraft-text-engine").is_ok_and(|p| p.is_file()) {
            return;
        }
        // <target>/<profile>, wherever this cargo put this test
        let exe = std::env::current_exe().unwrap();
        let profile = plumcraft_editor_core::engines::profile_dir(&exe).unwrap();
        let mut cmd = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        cmd.args(["build", "-p", "plumcraft-text", "--bins", "--target-dir"]).arg(profile.parent().unwrap());
        if profile.ends_with("release") {
            cmd.arg("--release");
        }
        assert!(cmd.status().unwrap().success(), "building the text engine");
    });
}

#[test]
fn starting_a_process_engine_never_holds_the_core_thread() {
    engine_binary();
    let mut c = Client::open(&json!({"store_root": store_root("switch-process"), "platform": {"subprocess": true}}));
    // plum_open returns before its own engine has connected: let that finish first (a request for
    // the document is held until it has), so what follows times this switch alone
    assert_eq!(c.call("graph.get", json!({}))["ok"], true);
    let t = Instant::now();
    let id = c.send(&json!({"method": "command.run", "params": {"command": "engine.switch", "params": {"engine": "text", "mode": "process"}}}));
    let sent = t.elapsed();
    // while the process starts, the core thread answers
    let ping = c.call("ping", json!({}));
    let pinged = t.elapsed();
    let r = c.wait_reply(id, Duration::from_secs(60));
    let switched = t.elapsed();
    assert_eq!(ping["result"]["pong"], true);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!((r["result"]["mode"].as_str(), r["result"]["connected"].as_bool()), (Some("process"), Some(true)), "{r}");
    // plum_request only queues (microseconds); what it must never wait for is the process
    // engine's start, hundreds of milliseconds, so it is held to a small part of that rather than
    // to a fixed time a loaded machine's scheduler can exceed
    assert!(sent * 4 < switched, "plum_request returns at once: {sent:?}, the switch took {switched:?}");
    assert!(pinged < switched, "the core thread answered while the engine started: ping {pinged:?}, switch {switched:?}");
    runs_text(&mut c);
    // the run went to the process: its node states came back through the new link
    assert!(c.events.iter().any(|e| e["kind"] == "node.status"));
    c.close();
}
