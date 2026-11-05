//! The MCP client against a fake server over pipes (note 14, M2.1): framing, progress, and
//! every hostile reply: garbage JSON, oversized frames, a server that never answers, one that
//! dies mid-call, wrong and string ids. Plus the process side: a child that never answers is
//! killed at the timeout, and a missing binary is an error, not a panic.

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use plumcraft_craft::mcp::{CANCEL_GRACE, CallError, Client, NoWatch, Progress, Watch};
use serde_json::{Value, json};

/// What the fake does with a request: a list of lines to write (with `{id}` replaced), or
/// nothing (never answer), or close the stream.
#[derive(Clone)]
enum Act {
    Lines(Vec<String>),
    Silent,
    Die,
}

/// A fake MCP server on a thread: `script` maps a method (or `tools/call:<tool>`) to an act.
/// Every message it receives goes to the returned channel.
fn fake(script: fn(&str, &Value) -> Act) -> (Client, Receiver<Value>) {
    let (to_server_r, to_server_w) = std::io::pipe().unwrap();
    let (from_server_r, mut from_server_w) = std::io::pipe().unwrap();
    let (seen_tx, seen_rx): (Sender<Value>, Receiver<Value>) = channel();
    std::thread::spawn(move || {
        let mut lines = BufReader::new(to_server_r);
        let mut line = String::new();
        loop {
            line.clear();
            if lines.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            let _ = seen_tx.send(msg.clone());
            let Some(id) = msg.get("id").cloned() else { continue };
            let method = msg["method"].as_str().unwrap_or_default().to_owned();
            let key = if method == "tools/call" { format!("tools/call:{}", msg["params"]["name"].as_str().unwrap_or_default()) } else { method };
            match script(&key, &msg) {
                Act::Lines(ls) => {
                    for l in ls {
                        let l = l.replace("{id}", &id.to_string());
                        if from_server_w.write_all(l.as_bytes()).and_then(|()| from_server_w.write_all(b"\n")).is_err() {
                            return;
                        }
                    }
                }
                Act::Silent => {}
                Act::Die => return,
            }
        }
    });
    (Client::over_bounded(Box::new(from_server_r), Box::new(to_server_w), 4096), seen_rx)
}

fn reply(result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": "ID", "result": result}).to_string().replace("\"ID\"", "{id}")
}

fn tool_text(text: &str, is_error: bool) -> String {
    reply(json!({"content": [{"type": "text", "text": text}], "isError": is_error}))
}

fn standard(key: &str, _: &Value) -> Act {
    match key {
        "initialize" => Act::Lines(vec![reply(json!({"serverInfo": {"name": "fakecraft", "version": "9.9"}, "capabilities": {"tools": {}}}))]),
        "ping" => Act::Lines(vec![reply(json!({}))]),
        "tools/call:echo" => Act::Lines(vec![tool_text(r#"{"ok": true, "n": 3}"#, false)]),
        "tools/call:fail" => Act::Lines(vec![tool_text("command_run: invalid parameters for `x`: unknown parameter `y`", true)]),
        "tools/call:plain" => Act::Lines(vec![tool_text("not json", false)]),
        "tools/call:progress" => Act::Lines(vec![
            json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progressToken": "TOKEN", "progress": 1, "total": 4, "message": "a quarter"}})
                .to_string()
                .replace("\"TOKEN\"", "{id}"),
            json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progressToken": 999_999, "progress": 1}}).to_string(),
            reply(json!({"content": [{"type": "text", "text": "{}"}]})),
        ]),
        // Garbage, a reply to an id nobody asked, a string id, a notification, then the reply.
        "tools/call:noisy" => Act::Lines(vec![
            "this is not json".into(),
            "{\"half\": ".into(),
            "[1, 2, 3]".into(),
            json!({"jsonrpc": "2.0", "id": 424_242, "result": {}}).to_string(),
            json!({"jsonrpc": "2.0", "id": "a-string", "result": {}}).to_string(),
            json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "info"}}).to_string(),
            json!({"jsonrpc": "2.0", "id": 1, "method": "roots/list"}).to_string(),
            String::new(),
            tool_text(r#"{"made_it": true}"#, false),
        ]),
        "tools/call:rpc_error" => Act::Lines(vec![
            json!({"jsonrpc": "2.0", "id": "ID", "error": {"code": -32603, "message": "internal error (please report this bug)", "data": {"busy": {"x": 1}}}})
                .to_string()
                .replace("\"ID\"", "{id}"),
        ]),
        "tools/call:huge" => Act::Lines(vec![format!("{{\"pad\": \"{}\"}}", "x".repeat(10_000))]),
        "tools/call:silent" => Act::Silent,
        "tools/call:die" => Act::Die,
        _ => Act::Lines(vec![json!({"jsonrpc": "2.0", "id": "ID", "error": {"code": -32601, "message": "no such method"}}).to_string().replace("\"ID\"", "{id}")]),
    }
}

fn connected() -> (Client, Receiver<Value>) {
    let (mut c, seen) = fake(standard);
    c.initialize(Duration::from_secs(5)).unwrap();
    (c, seen)
}

const T: Duration = Duration::from_secs(5);

#[test]
fn initialize_and_tools() {
    let (c, seen) = connected();
    assert_eq!((c.server_name.as_str(), c.server_version.as_str()), ("fakecraft", "9.9"));
    let init = seen.recv_timeout(T).unwrap();
    assert_eq!(init["params"]["protocolVersion"], "2025-06-18");
    assert_eq!(seen.recv_timeout(T).unwrap()["method"], "notifications/initialized");
    let ok = c.tool("echo", json!({}), T, &mut NoWatch).unwrap();
    assert!(!ok.is_error);
    assert_eq!(ok.value, Some(json!({"ok": true, "n": 3})));
    let bad = c.tool("fail", json!({}), T, &mut NoWatch).unwrap();
    assert!(bad.is_error && bad.text.contains("unknown parameter"));
    let plain = c.tool("plain", json!({}), T, &mut NoWatch).unwrap();
    assert_eq!((plain.value, plain.text.as_str()), (None, "not json"));
    assert!(c.ping(T));
}

struct Recorder(Vec<Progress>);

impl Watch for Recorder {
    fn cancelled(&self) -> bool {
        false
    }

    fn progress(&mut self, p: &Progress) {
        self.0.push(p.clone());
    }
}

#[test]
fn progress_reaches_its_call_only() {
    let (c, _) = connected();
    let mut w = Recorder(Vec::new());
    c.tool("progress", json!({}), T, &mut w).unwrap();
    assert_eq!(w.0, [Progress { progress: 1.0, total: Some(4.0), message: Some("a quarter".into()) }]);
    assert_eq!(c.skipped(), 1, "the progress for an unknown token is skipped");
}

#[test]
fn garbage_and_wrong_ids_are_skipped() {
    let (c, _) = connected();
    let r = c.tool("noisy", json!({}), T, &mut NoWatch).unwrap();
    assert_eq!(r.value, Some(json!({"made_it": true})));
    assert!(c.skipped() >= 6, "{}", c.skipped());
    // Still in step afterwards.
    assert_eq!(c.tool("echo", json!({}), T, &mut NoWatch).unwrap().value, Some(json!({"ok": true, "n": 3})));
}

#[test]
fn rpc_errors_are_errors() {
    let (c, _) = connected();
    match c.tool("rpc_error", json!({}), T, &mut NoWatch) {
        Err(CallError::Rpc(e)) => assert_eq!(e.code.code(), -32603),
        other => panic!("{other:?}"),
    }
    assert!(matches!(c.call("no/such", json!({}), T, &mut NoWatch), Err(CallError::Rpc(_))));
}

#[test]
fn an_oversized_line_kills_the_stream() {
    let (c, _) = connected();
    match c.tool("huge", json!({}), T, &mut NoWatch) {
        Err(CallError::Dead { reason, .. }) => assert!(reason.contains("over 4096 bytes"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(c.is_dead());
    assert!(matches!(c.tool("echo", json!({}), T, &mut NoWatch), Err(CallError::Dead { .. })), "a dead client fails at once");
}

#[test]
fn a_server_that_never_answers_times_out() {
    let (c, _) = connected();
    let t = Instant::now();
    match c.tool("silent", json!({}), Duration::from_millis(300), &mut NoWatch) {
        Err(CallError::Timeout { after, .. }) => assert_eq!(after, Duration::from_millis(300)),
        other => panic!("{other:?}"),
    }
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    assert!(c.is_dead(), "a timed-out server is killed, never reused");
}

#[test]
fn a_server_that_dies_mid_call() {
    let (c, _) = connected();
    let t = Instant::now();
    match c.tool("die", json!({}), T, &mut NoWatch) {
        Err(CallError::Dead { reason, .. }) => assert!(reason.contains("exited"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(t.elapsed() < Duration::from_secs(2));
}

struct CancelAfter(Instant);

impl Watch for CancelAfter {
    fn cancelled(&self) -> bool {
        Instant::now() >= self.0
    }

    fn progress(&mut self, _: &Progress) {}
}

#[test]
fn cancel_notifies_then_kills_within_the_grace() {
    let (c, seen) = connected();
    let t = Instant::now();
    let r = c.tool("silent", json!({}), T, &mut CancelAfter(Instant::now() + Duration::from_millis(100)));
    assert_eq!(r, Err(CallError::Cancelled));
    let took = t.elapsed();
    assert!(took >= Duration::from_millis(100) + CANCEL_GRACE && took < Duration::from_millis(100) + CANCEL_GRACE + Duration::from_millis(300), "{took:?}");
    let call = std::iter::from_fn(|| seen.recv_timeout(T).ok()).find(|m| m["method"] == "tools/call").unwrap();
    let note = seen.recv_timeout(T).unwrap();
    assert_eq!(note["method"], "notifications/cancelled");
    assert_eq!(note["params"]["requestId"], call["id"]);
    assert!(c.is_dead(), "D41: a cancelled server is killed");
}

#[test]
fn a_child_that_never_answers_is_killed() {
    let mut c = Client::spawn(&sleeper(30), &[]).unwrap();
    let pid = c.pid().unwrap();
    let t = Instant::now();
    assert!(matches!(c.initialize(Duration::from_millis(300)), Err(CallError::Timeout { .. })));
    assert!(t.elapsed() < Duration::from_secs(3));
    assert!(!alive(pid), "the child {pid} was killed and reaped");
}

#[test]
fn a_child_that_exits_is_dead_with_its_stderr() {
    let script = if cfg!(windows) { "echo boom on stderr 1>&2 & exit 3" } else { "echo boom on stderr >&2; exit 3" };
    let mut c = Client::spawn(&shell(script), &[]).unwrap();
    match c.initialize(T) {
        Err(CallError::Dead { stderr, .. } | CallError::Io(stderr)) => {
            // The stderr ring fills on its own thread; give it a moment.
            std::thread::sleep(Duration::from_millis(100));
            assert!(stderr.contains("boom") || c.stderr_tail().contains("boom"), "{stderr:?} / {:?}", c.stderr_tail());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_missing_binary_is_an_error() {
    assert!(Client::spawn(&["/no/such/craft-cli".into(), "mcp".into()], &[]).is_err());
    assert!(Client::spawn(&[], &[]).is_err());
}

/// A craft busy with work (here one that ignores its stdin) outlived an engine that died
/// without running destructors: found 2026-10-27, a `vectorcraft-cli` left at full CPU with
/// parent 1 after a test process died. The watchdog kills the craft's group once the engine
/// is gone. The engine is stood in for by a `sleep` the test kills.
#[test]
fn a_craft_dies_with_its_engine() {
    let mut engine = command(&sleeper(30)).spawn().unwrap();
    let busy = if cfg!(windows) {
        vec!["powershell".into(), "-NoProfile".into(), "-Command".into(), "while ($true) { Start-Sleep -Seconds 1 }".into()]
    } else {
        shell("trap '' TERM; while :; do sleep 1; done")
    };
    let c = Client::spawn_watched(&busy, &[], engine.id()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(!c.has_exited(), "the craft runs while its engine does");
    engine.kill().unwrap();
    engine.wait().unwrap();
    let t = Instant::now();
    while !c.has_exited() {
        assert!(t.elapsed() < Duration::from_secs(5), "the craft outlived its engine");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The watchdog goes with its craft: dropping the client leaves no `plumcraft-craft-watch`.
#[test]
fn the_watchdog_goes_with_its_craft() {
    let c = Client::spawn(&sleeper(30), &[]).unwrap();
    let pid = c.pid().unwrap();
    drop(c);
    let watching = if cfg!(windows) {
        // the Windows watchdog is a PowerShell loop naming the craft's pid
        let q = format!(
            "Get-CimInstance Win32_Process -Filter \"Name = 'powershell.exe'\" | Where-Object {{ $_.ProcessId -ne $PID -and $_.CommandLine -like '*-Id {pid} *' }} | ForEach-Object {{ $_.ProcessId }}"
        );
        std::process::Command::new("powershell").args(["-NoProfile", "-Command", &q]).output().unwrap()
    } else {
        std::process::Command::new("pgrep").args(["-f", &format!("plumcraft-craft-watch {} {pid}", std::process::id())]).output().unwrap()
    };
    assert!(watching.stdout.iter().all(u8::is_ascii_whitespace), "a watchdog was left: {}", String::from_utf8_lossy(&watching.stdout));
}

/// A command that sleeps `secs` seconds.
fn sleeper(secs: u32) -> Vec<String> {
    if cfg!(windows) {
        vec!["powershell".into(), "-NoProfile".into(), "-Command".into(), format!("Start-Sleep -Seconds {secs}")]
    } else {
        vec!["/bin/sleep".into(), secs.to_string()]
    }
}

/// `script` run by the platform's shell.
fn shell(script: &str) -> Vec<String> {
    if cfg!(windows) { vec!["cmd".into(), "/C".into(), script.into()] } else { vec!["/bin/sh".into(), "-c".into(), script.into()] }
}

fn command(argv: &[String]) -> std::process::Command {
    let mut c = std::process::Command::new(&argv[0]);
    c.args(&argv[1..]);
    c
}

/// Whether process `pid` still runs.
fn alive(pid: u32) -> bool {
    if cfg!(windows) {
        let out = std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
    } else {
        std::process::Command::new("kill").args(["-0", &pid.to_string()]).status().unwrap().success()
    }
}
