//! The control channel on a core opened through the C ABI (`docs/shell-interface.md` 4.1 and
//! 5.3): the config's `control: {port, token_file}` starts the same loopback server the desktop
//! editor's `--control` starts, answered by the same dispatcher, so a probe drives an app built on
//! the C ABI as it drives the egui editor. No C function is added.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::{Client, store_root, take};
use plumcraft_ffi::plum_poll;
use serde_json::{Value, json};

/// A control-channel connection, authenticated.
struct Conn {
    out: TcpStream,
    lines: BufReader<TcpStream>,
    next: u64,
}

impl Conn {
    fn open(port: u16, token: &str) -> Conn {
        let out = TcpStream::connect(("127.0.0.1", port)).unwrap();
        out.set_read_timeout(Some(Duration::from_secs(40))).unwrap();
        let lines = BufReader::new(out.try_clone().unwrap());
        let mut c = Conn { out, lines, next: 0 };
        let r = c.call("auth", json!({"token": token}));
        assert_eq!(r["ok"], true, "{r}");
        c
    }

    fn line(&mut self, text: &str) -> Value {
        writeln!(self.out, "{text}").unwrap();
        let mut line = String::new();
        self.lines.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line:?}"))
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let r = self.line(&json!({"id": self.next, "method": method, "params": params}).to_string());
        assert_eq!(r["id"], self.next, "{r}");
        r
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        let r = self.call("command.run", json!({"command": command, "params": params}));
        assert_eq!(r["ok"], true, "{command}: {r}");
        r["result"].clone()
    }
}

/// Poll the C side until a `screenshot` ask arrives in the feed: the ask.
fn screenshot_ask(c: &mut Client) -> Value {
    let t0 = Instant::now();
    loop {
        let bytes = take(plum_poll(c.core));
        if !bytes.is_empty() {
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            for r in v["replies"].as_array().unwrap() {
                c.replies.insert(r["id"].as_u64().unwrap(), r.clone());
            }
            if let Some(a) = v.get("asks").and_then(Value::as_array).and_then(|a| a.iter().find(|a| a["kind"] == "screenshot")) {
                return a.clone();
            }
        }
        assert!(t0.elapsed() < Duration::from_secs(20), "no screenshot ask in the feed");
        c.wait_wake(Duration::from_millis(100));
    }
}

#[test]
fn a_core_opened_through_the_c_abi_is_driven_over_its_control_port() {
    let dir = PathBuf::from(store_root("control-port"));
    let token_file = dir.join("control.token");
    let mut c = Client::open(&json!({"store_root": dir, "control": {"port": 0, "token_file": token_file}}));
    let info = c.call("core.control", json!({}));
    let port = info["result"]["port"].as_u64().unwrap_or_else(|| panic!("{info}")) as u16;
    assert_eq!(info["result"]["tokenFile"], json!(token_file));
    let token = std::fs::read_to_string(&token_file).unwrap().trim().to_owned();

    // no token, no requests
    let mut stranger = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stranger.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    writeln!(stranger, "{}", json!({"id": 1, "method": "ui.inspect"})).unwrap();
    let mut line = String::new();
    BufReader::new(stranger).read_line(&mut line).unwrap();
    assert!(line.contains("authentication required"), "{line}");

    let mut s = Conn::open(port, &token);
    // build and run a graph, as a probe does the desktop editor
    let lit = s.run("node.add", json!({"kind": "text.literal", "pos": [0, 0]}))["id"].as_str().unwrap().to_owned();
    let up = s.run("node.add", json!({"kind": "text.upper", "pos": [260, 0]}))["id"].as_str().unwrap().to_owned();
    s.run("wire.connect", json!({"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}}));
    s.run("node.set", json!({"node": lit, "port": "text", "value": "a corgi naps"}));
    s.run("run.watch", json!({"node": up, "port": "text"}));
    s.run("run.start", json!({}));
    let done = s.call("ui.waitRun", json!({"timeout": 20}));
    assert_eq!((done["ok"].as_bool(), done["result"]["running"].as_bool()), (Some(true), Some(false)), "{done}");
    let rows = s.call("command.list", json!({}));
    assert!(rows["result"].as_array().unwrap().iter().any(|r| r["id"] == "run.start"), "{rows}");

    // ui.inspect: the core's parts, no geometry from a shell that reports none
    let i = s.call("ui.inspect", json!({}))["result"].clone();
    assert_eq!(i["graph"]["nodes"].as_array().map(Vec::len), Some(2), "{i}");
    // `run.watch` runs what the node needs, as the editor's command does (docs/shell-interface.md
    // 6.2), so `run.start` is the second run: done when it superseded the first, cached when the
    // first had finished (a slow machine). Either way the node was computed and is current.
    let state = i["run"]["nodes"][&up]["state"].as_str();
    assert!(matches!(state, Some("done" | "cached")), "{i}");
    assert!(i["title"].as_str().unwrap().starts_with("Untitled • — PlumCraft (Text"), "{i}");
    assert_eq!((i["asks"].clone(), i["engine"]["connected"].clone(), i.get("canvas")), (json!([]), json!(true), None), "{i}");
    // the same request through plum_request gets the same answer
    let via_c = c.call("ui.inspect", json!({}))["result"].clone();
    assert_eq!(via_c["graph"], i["graph"]);

    // a file ask raised over the port, answered over the port
    let saved = dir.join("saved.json");
    let r = s.call("ui.invoke", json!({"command": "graph.saveAs"}));
    let ask = r["result"]["asks"][0].clone();
    assert_eq!((ask["kind"].as_str(), ask["by"].as_str(), r["result"]["invoked"].as_str()), (Some("file.save"), Some("script"), Some("graph.saveAs")), "{r}");
    let r = s.call("ask.answer", json!({"ask": ask["ask"], "answer": {"path": saved}}));
    assert_eq!((r["ok"].as_bool(), r["result"]["asks"].clone()), (Some(true), json!([])), "{r}");
    assert!(std::fs::read_to_string(&saved).unwrap().contains("text.upper"));
    assert_eq!(s.call("ask.answer", json!({"ask": ask["ask"], "answer": {"path": saved}}))["error"], format!("ask {} was already answered", ask["ask"]));

    // ui.screenshot: asked of the shell, which answers through the C ABI; the port's request
    // replies with that answer
    let shot = dir.join("shot.png");
    let shot2 = shot.clone();
    let waiting = std::thread::spawn(move || {
        let mut s2 = Conn::open(port, &token);
        s2.call("ui.screenshot", json!({"path": shot2}))
    });
    let a = screenshot_ask(&mut c);
    assert_eq!((a["params"]["path"].clone(), a["by"].as_str()), (json!(shot), Some("script")), "{a}");
    let r = c.call("ask.answer", json!({"ask": a["ask"], "answer": {"path": shot, "width": 640, "height": 480}}));
    assert_eq!(r["ok"], true, "{r}");
    let replied = waiting.join().unwrap();
    assert_eq!(replied["result"], json!({"path": shot, "width": 640, "height": 480, "source": "window"}), "{replied}");
    // what only a windowed shell has
    assert_eq!(s.call("ui.set", json!({"zoom": 1}))["error"], "ui.set isn't available from this shell");
    assert_eq!(s.call("ui.audio", json!({"play": false}))["error"], "ui.audio isn't available from this shell");

    // hostile lines never panic; each gets an error and the channel goes on
    let mut methods: Vec<String> = [
        "command.run",
        "engine.execute",
        "graph.get",
        "ui.inspect",
        "ui.invoke",
        "ui.prompt",
        "ask.answer",
        "ui.set",
        "ui.audio",
        "ui.waitRun",
        "ui.screenshot",
    ]
    .iter()
    .map(|m| (*m).to_owned())
    .collect();
    methods.push("m".repeat(10_000));
    methods.push("\u{202e}\u{0}".into());
    let params = [
        json!(null),
        json!(7),
        json!("x"),
        json!([1]),
        json!({"command": 7}),
        json!({"ask": -1, "answer": {}}),
        json!({"ask": 1, "cancel": "yes"}),
        json!({"timeout": -1}),
        json!({"path": 3, "offscreen": "no"}),
        json!({"zzz": 1}),
    ];
    for m in &methods {
        for p in &params {
            s.next += 1;
            let r = s.line(&json!({"id": s.next, "method": m, "params": p}).to_string());
            assert!(r["ok"].is_boolean(), "{m} {p}: {r}");
        }
    }
    for junk in ["{", "[]", "7", "\"x\"", "{\"method\": 3}", "{\"id\": {\"deep\": [[[[[]]]]]}, \"method\": \"ui.inspect\"}"] {
        let r = s.line(junk);
        assert!(r["ok"].is_boolean(), "{junk}: {r}");
    }
    assert_eq!(s.call("ui.inspect", json!({}))["ok"], true);

    // closing the core closes the port
    c.close();
    let t0 = Instant::now();
    let closed = loop {
        let gone = match TcpStream::connect(("127.0.0.1", port)) {
            Err(_) => true,
            Ok(t) => {
                t.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut line = String::new();
                matches!(BufReader::new(t).read_line(&mut line), Ok(0) | Err(_))
            }
        };
        if gone || t0.elapsed() > Duration::from_secs(5) {
            break gone;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(closed, "the port closes with the core");
    let mut line = String::new();
    assert!(matches!(s.lines.read_line(&mut line), Ok(0) | Err(_)), "an open connection is shut too: {line:?}");
}

#[test]
fn a_bad_control_config_is_refused_and_none_means_no_port() {
    for bad in [json!({"control": {"port": "x"}}), json!({"control": {"port": 70000}}), json!({"control": 7}), json!({"control": {"port": 0, "token_file": 3}})]
    {
        let text = bad.to_string();
        // SAFETY: the config is live for the call; no wake.
        let core = unsafe { plumcraft_ffi::plum_open(text.as_ptr(), text.len(), None, std::ptr::null_mut()) };
        assert_eq!(core, 0, "{bad}");
        let why = String::from_utf8(take(plumcraft_ffi::plum_last_error())).unwrap();
        assert!(why.contains("control"), "{bad}: {why}");
    }
    for none in [json!({}), json!({"control": null}), json!({"control": {"port": null}})] {
        let mut c = Client::open(&none);
        assert_eq!(c.call("core.control", json!({}))["result"], json!({"port": null}), "{none}");
        c.close();
    }
}

/// B83: a request on the port that names a document (`doc`, as `plum_request` takes it) is
/// answered for that document; one that names none, for the first; one that names a document
/// that isn't open is refused (`unknown_doc`), never answered for another.
#[test]
fn the_control_port_honours_the_document_a_request_names() {
    let dir = PathBuf::from(store_root("control-port-doc"));
    let token_file = dir.join("control.token");
    let mut c = Client::open(&json!({"store_root": dir, "control": {"port": 0, "token_file": token_file}}));
    let port = c.call("core.control", json!({}))["result"]["port"].as_u64().unwrap() as u16;
    let token = std::fs::read_to_string(&token_file).unwrap().trim().to_owned();
    let first = c.call("document.list", json!({}))["result"]["docs"][0]["doc"].as_u64().unwrap();
    let second = c.call("document.open", json!({}))["result"]["doc"].as_u64().unwrap();
    assert_ne!(first, second);
    let mut s = Conn::open(port, &token);
    let on = |s: &mut Conn, doc: Value, method: &str, params: Value| -> Value {
        s.next += 1;
        let r = s.line(&json!({"id": s.next, "method": method, "params": params, "doc": doc}).to_string());
        assert_eq!(r["id"], s.next, "{r}");
        r
    };
    // a node added in the second document only
    let r = on(&mut s, json!(second), "command.run", json!({"command": "node.add", "params": {"kind": "text.literal", "pos": [0, 0]}}));
    assert_eq!(r["ok"], true, "{r}");
    let nodes = |r: &Value| r["result"]["graph"]["nodes"].as_array().map_or(0, Vec::len);
    assert_eq!(nodes(&on(&mut s, json!(second), "ui.inspect", json!({}))), 1, "the second document has the node");
    assert_eq!(nodes(&on(&mut s, json!(first), "ui.inspect", json!({}))), 0, "the first has none");
    assert_eq!(nodes(&on(&mut s, Value::Null, "ui.inspect", json!({}))), 0, "doc null: the first");
    assert_eq!(nodes(&s.call("ui.inspect", json!({}))), 0, "no doc field at all: the first");
    // what plum_request answers for the same document agrees
    let id = c.send(&json!({"method": "graph.get", "params": {}, "doc": second}));
    let via_c = c.wait_reply(id, Duration::from_secs(30));
    assert_eq!(via_c["result"]["nodes"].as_object().map(|n| n.len()), Some(1), "{via_c}");
    // a document that isn't open, or isn't a number, is refused
    for (bad, code) in [(json!(9999), "unknown_doc"), (json!("x"), "bad_params")] {
        let r = on(&mut s, bad.clone(), "ui.inspect", json!({}));
        assert_eq!((r["ok"].as_bool(), r["code"].as_str()), (Some(false), Some(code)), "{bad}: {r}");
    }
    // closed: refused from then on
    let r = c.call("document.close", json!({"doc": second, "discard": true}));
    assert_eq!(r["ok"], true, "{r}");
    let r = on(&mut s, json!(second), "ui.inspect", json!({}));
    assert_eq!(r["code"], "unknown_doc", "{r}");
    c.close();
}
