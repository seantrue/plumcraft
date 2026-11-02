//! The control channel's `ui.*` handlers (`plumcraft_ui::control`), and one end-to-end run
//! through the real transport: a loopback server on an ephemeral port, a client thread speaking
//! newline-delimited JSON, and the editor stepping frames as eframe would.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use common::{add, harness, session};
use egui_kittest::Harness;
use plumcraft_automation::server;
use plumcraft_engine::host::Host;
use plumcraft_host::{Connection, InProc};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use plumcraft_ui::link::Link;
use serde_json::{Value, json};

const TOKEN: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn control(h: &mut Harness<'static, EditorApp>, method: &str, params: Value) -> Value {
    let ctx = h.ctx.clone();
    let v = h.state_mut().control_now(&ctx, method, &params);
    h.run_steps(3);
    v
}

/// Send a request through `handle_control` and step until it replies.
fn deferred(h: &mut Harness<'static, EditorApp>, method: &str, params: Value) -> Value {
    let (tx, rx) = channel();
    let ctx = h.ctx.clone();
    h.state_mut().handle_control(
        &ctx,
        method,
        &params,
        Box::new(move |v| {
            let _ = tx.send(v);
        }),
    );
    let start = Instant::now();
    loop {
        if let Ok(v) = rx.try_recv() {
            return v;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "{method}: no reply");
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn text_engine() -> Link {
    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    let conn: Arc<dyn Connection> = Arc::new(InProc(host));
    Link::start(conn, "in-process", None).unwrap()
}

#[test]
fn inspect_reports_graph_selection_and_run() {
    let mut s = session();
    let a = add(&mut s, "text.literal", 20.0, 20.0);
    let b = add(&mut s, "text.upper", 360.0, 20.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}})).unwrap();
    let mut h = harness(EditorApp::new(s));
    let r = control(&mut h, "ui.inspect", Value::Null);
    assert_eq!(r["ok"], true, "{r}");
    let v = &r["result"];
    assert_eq!(v["graph"]["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(v["graph"]["wires"], 1);
    assert!(v["graph"]["nodes"].as_array().unwrap().iter().any(|n| n["id"] == b.as_str() && n["kind"] == "text.upper" && n["title"] == "Upper case"));
    assert_eq!(v["selection"], json!([]));
    assert_eq!(v["engine"]["connected"], false);
    assert_eq!(v["run"]["running"], false);
    assert_eq!(v["watched"], Value::Null);
    assert!(v["canvas"]["zoom"].as_f64().unwrap() > 0.0);
    assert!(v["title"].as_str().unwrap().ends_with("PlumCraft"), "{v}");
    // graph.get is the workflow
    let g = control(&mut h, "graph.get", Value::Null);
    assert_eq!(g["result"]["wires"].as_array().unwrap().len(), 1);
}

#[test]
fn commands_run_through_the_app_with_their_errors() {
    let mut h = harness(EditorApp::new(session()));
    let r = control(&mut h, "command.run", json!({"command": "node.add", "params": {"kind": "text.upper"}}));
    assert_eq!(r["ok"], true, "{r}");
    let id = r["result"]["id"].as_str().unwrap().to_owned();
    assert!(h.state().session.graph().node(&id.as_str().into()).is_some());
    // engine.execute is the same method under the research software's name
    let r = control(&mut h, "engine.execute", json!({"command": "node.add", "params": {"kind": "no.such"}}));
    assert_eq!(r["ok"], false);
    assert!(h.state().last_error.is_some(), "the status bar shows the error too");
    let r = control(&mut h, "command.run", json!({"command": "run.start"}));
    assert!(r["error"].as_str().unwrap().contains("not connected"), "{r}");
    assert_eq!(control(&mut h, "command.run", json!({}))["ok"], false);
    assert_eq!(control(&mut h, "command.run", json!({"command": "node.add", "extra": 1}))["ok"], false);
    let rows = control(&mut h, "command.list", Value::Null);
    assert!(rows["result"].as_array().unwrap().iter().any(|c| c["id"] == "wire.connect"));
    assert_eq!(control(&mut h, "engine.commands", Value::Null)["result"], rows["result"]);
    assert!(control(&mut h, "ui.bogus", Value::Null)["error"].as_str().unwrap().contains("unknown method"));
}

#[test]
fn ui_set_moves_the_view_and_opens_the_palette() {
    let mut h = harness(EditorApp::new(session()));
    let r = control(&mut h, "ui.set", json!({"zoom": 0.5, "pan": [10.0, 20.0]}));
    assert_eq!(r["ok"], true, "{r}");
    let c = &control(&mut h, "ui.inspect", Value::Null)["result"]["canvas"];
    assert_eq!((c["zoom"].as_f64(), c["pan"][0].as_f64(), c["pan"][1].as_f64()), (Some(0.5), Some(10.0), Some(20.0)));
    control(&mut h, "ui.set", json!({"palette": "upper"}));
    let p = control(&mut h, "ui.inspect", Value::Null)["result"]["popups"]["palette"].clone();
    assert_eq!(p["query"], "upper");
    control(&mut h, "ui.set", json!({"palette": false}));
    assert_eq!(control(&mut h, "ui.inspect", Value::Null)["result"]["popups"]["palette"], Value::Null);
    for bad in [
        json!({"zoom": 100}),
        json!({"zoom": "x"}),
        json!({"pan": [1]}),
        json!({"pan": ["a", 1]}),
        json!({"fit": 3}),
        json!({"palette": 3}),
        json!({"focus": "g1"}),
        json!([1]),
    ] {
        assert_eq!(control(&mut h, "ui.set", bad.clone())["ok"], false, "{bad}");
    }
    assert_eq!(control(&mut h, "ui.set", json!({"fit": true}))["ok"], true);
}

#[test]
fn bad_deferred_requests_reply_at_once() {
    let mut h = harness(EditorApp::new(session()));
    for (method, params) in [
        ("ui.screenshot", json!({"path": 3})),
        ("ui.screenshot", json!({"offscreen": "yes"})),
        ("ui.screenshot", json!({"pathh": "x.png"})),
        ("ui.waitRun", json!({"timeout": -1})),
        ("ui.waitRun", json!({"timeout": "soon"})),
    ] {
        let r = deferred(&mut h, method, params.clone());
        assert_eq!(r["ok"], false, "{method} {params}: {r}");
    }
    // nothing running: a wait answers at once with the run summary
    let r = deferred(&mut h, "ui.waitRun", json!({"timeout": 1}));
    assert_eq!((r["ok"].as_bool(), r["result"]["running"].as_bool()), (Some(true), Some(false)));
    assert_eq!(h.state().control.pending(), 0);
}

/// Without anything to deliver the window's screenshot (no renderer behind this context, like
/// a locked screen), `ui.screenshot` draws offscreen after the wait.
#[test]
fn a_screenshot_the_window_never_delivers_is_drawn_offscreen() {
    let mut s = session();
    add(&mut s, "text.upper", 40.0, 40.0);
    let mut app = EditorApp::new(s);
    app.control.window_shot_wait = Duration::from_millis(200);
    let ctx = egui::Context::default();
    app.setup_context(&ctx);
    let dir = plumcraft_scratch::root().join("offscreen-test");
    let path = dir.join("shot.png");
    let (tx, rx) = channel();
    app.handle_control(
        &ctx,
        "ui.screenshot",
        &json!({"path": path}),
        Box::new(move |v| {
            let _ = tx.send(v);
        }),
    );
    let raw = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0))), ..Default::default() };
    let start = Instant::now();
    let reply = loop {
        // (nothing renders this context: its textures are dropped on purpose)
        ctx.run_ui(raw(), |ui| app.show(ui)).textures_delta.clear();
        if let Ok(v) = rx.try_recv() {
            break v;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "no reply");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["result"]["source"], "offscreen");
    assert_eq!((reply["result"]["width"].as_u64(), reply["result"]["height"].as_u64()), (Some(900), Some(600)));
    assert!(std::fs::metadata(&path).unwrap().len() > 1000);
    assert_eq!(app.control.pending(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

/// One client session on the real transport.
struct Client {
    out: TcpStream,
    input: BufReader<TcpStream>,
    next: u64,
}

impl Client {
    fn connect(port: u16) -> Client {
        let out = TcpStream::connect(("127.0.0.1", port)).unwrap();
        out.set_read_timeout(Some(Duration::from_secs(40))).unwrap();
        let input = BufReader::new(out.try_clone().unwrap());
        Client { out, input, next: 0 }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        writeln!(self.out, "{}", json!({"id": self.next, "method": method, "params": params})).unwrap();
        let mut line = String::new();
        self.input.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        assert_eq!(v["id"], json!(self.next), "{line}");
        v
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let v = self.call(method, params.clone());
        assert_eq!(v["ok"], true, "{method} {params}: {v}");
        v["result"].clone()
    }
}

#[test]
fn end_to_end_over_the_control_server() {
    let session = Session::new(plumcraft_text::engine().schema());
    let mut app = EditorApp::new(session).with_link(text_engine());
    let (rx, port) = server::start(0, TOKEN.into(), Arc::new(|| {})).unwrap();
    app.control_rx = Some(rx);
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 760.0)).wgpu().build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
    let start = Instant::now();
    while !h.state().session.connected {
        assert!(start.elapsed() < Duration::from_secs(10), "the engine never connected");
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    let dir = plumcraft_scratch::root().join("e2e-control");
    let _ = std::fs::remove_dir_all(&dir);
    let (shot, offscreen) = (dir.join("window.png"), dir.join("offscreen.png"));
    let (shot_c, offscreen_c) = (shot.clone(), offscreen.clone());

    let client = std::thread::spawn(move || {
        // A wrong token is refused and the connection closed.
        let mut bad = Client::connect(port);
        let r = bad.call("auth", json!({"token": TOKEN.replace('0', "1")}));
        assert_eq!((r["ok"].as_bool(), r["error"].as_str()), (Some(false), Some("authentication required")));
        let mut line = String::new();
        assert_eq!(bad.input.read_line(&mut line).unwrap_or(0), 0, "closed after a failed auth: {line}");

        let mut c = Client::connect(port);
        c.ok("auth", json!({"token": TOKEN}));
        let rows = c.ok("command.list", json!({}));
        let row = |id: &str| rows.as_array().unwrap().iter().find(|r| r["id"] == id).cloned().unwrap();
        assert_eq!(row("node.add")["enabled"], true);
        assert_eq!(row("run.start")["enabled"], false, "nothing to run yet");
        let lit = c.ok("command.run", json!({"command": "node.add", "params": {"kind": "text.literal", "pos": [0, 0]}}))["id"].as_str().unwrap().to_owned();
        c.ok("command.run", json!({"command": "node.set", "params": {"node": lit, "port": "text", "value": "a corgi on a floral carpet"}}));
        let up = c.ok("command.run", json!({"command": "node.add", "params": {"kind": "text.upper", "pos": [320, 0]}}))["id"].as_str().unwrap().to_owned();
        c.ok("command.run", json!({"command": "wire.connect", "params": {"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}}}));
        c.ok("command.run", json!({"command": "run.start", "params": {}}));
        let done = c.ok("ui.waitRun", json!({"timeout": 20}));
        assert_eq!(done["running"], false, "{done}");
        let v = c.ok("ui.inspect", json!({}));
        assert_eq!(v["run"]["nodes"][&up]["state"], "done", "{v}");
        assert_eq!(v["graph"]["wires"], 1);
        // Watching a node reruns the graph (the memo answers: cached) and fills the panel.
        c.ok("command.run", json!({"command": "run.watch", "params": {"node": up}}));
        c.ok("ui.waitRun", json!({"timeout": 20}));
        let v = c.ok("ui.inspect", json!({}));
        assert_eq!(v["run"]["nodes"][&up]["state"], "cached", "{v}");
        assert_eq!(v["watched"]["node"], up.as_str());
        assert!(v["watched"]["summary"].as_str().is_some_and(|s| s.contains("26 characters")), "{v}");
        let s = c.ok("ui.screenshot", json!({"path": shot_c}));
        assert_eq!(s["source"], "window", "kittest's renderer answers the window's screenshot");
        let s = c.ok("ui.screenshot", json!({"path": offscreen_c, "offscreen": true}));
        assert_eq!(s["source"], "offscreen");
        assert!(!c.call("ui.nope", json!({}))["ok"].as_bool().unwrap());
        up
    });

    let start = Instant::now();
    while !client.is_finished() {
        assert!(start.elapsed() < Duration::from_secs(90), "the end-to-end run hung");
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    let up = client.join().unwrap();
    assert_eq!(h.state().session.run.nodes[&up].state, Some(plumcraft_proto::event::NodeState::Cached));
    for p in [&shot, &offscreen] {
        let bytes = std::fs::read(p).unwrap();
        assert!(bytes.len() > 1000 && bytes.starts_with(b"\x89PNG"), "{}", p.display());
    }
    let _ = std::fs::remove_dir_all(dir);
}
