//! N1 slice 4: the core asks, the window answers (`docs/shell-interface.md` section 11).
//!
//! - A person's chain through the window, as before: New over unsaved changes, Save in the
//!   prompt, the (fake) save dialog, then the new graph.
//! - A script's chain over the control channel: `ui.invoke` replies at once with the ask open
//!   (no dialog is shown), and `ask.answer` answers it; hostile answers are refused.
//! - An answer that switches engines replies once switched, and no frame waits for that.
//! - Edit › Paste asks for the clipboard; the window's Paste event answers.

mod common;

use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{add, click, harness, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_proto::catalogue::PathHint;
use plumcraft_ui::{AfterDiscard, EditorApp, FileDialogs, Prompt};
use serde_json::{Value, json};

/// The save dialog's answer, and how often it was shown.
#[derive(Default)]
struct Answers {
    save: Option<PathBuf>,
    shown: usize,
}

struct Fake(Arc<Mutex<Answers>>);

impl FileDialogs for Fake {
    fn pick_path(&mut self, _: &PathHint, _: &str) -> Option<PathBuf> {
        None
    }
    fn save_workflow(&mut self, _current: Option<&Path>) -> Option<PathBuf> {
        let mut a = self.0.lock().unwrap();
        a.shown += 1;
        a.save.clone()
    }
}

fn folder(name: &str) -> PathBuf {
    let d = plumcraft_scratch::root().join(format!("asks-ui-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn editor() -> (Harness<'static, EditorApp>, Arc<Mutex<Answers>>) {
    let mut s = session();
    add(&mut s, "text.literal", 20.0, 20.0);
    let answers = Arc::new(Mutex::new(Answers::default()));
    let mut app = EditorApp::new(s);
    app.dialogs = Box::new(Fake(Arc::clone(&answers)));
    (harness(app), answers)
}

fn control(h: &mut Harness<'static, EditorApp>, method: &str, params: Value) -> Value {
    let ctx = h.ctx.clone();
    h.state_mut().control_now(&ctx, method, &params)
}

#[test]
fn new_over_unsaved_changes_saves_through_the_dialog_as_a_person_would() {
    let dir = folder("person");
    let (mut h, answers) = editor();
    answers.lock().unwrap().save = Some(dir.join("kept.json"));
    h.get_by_label("File").click();
    h.run_steps(2);
    h.get_by_label_contains("New Graph").click();
    h.run_steps(3);
    assert_eq!(h.state().prompt, Some(Prompt::Discard { then: AfterDiscard::New }));
    let asks = h.state().inspect(&h.ctx)["asks"].clone();
    assert_eq!((asks[0]["kind"].as_str(), asks[0]["by"].as_str(), asks[0]["params"]["then"].as_str()), (Some("prompt"), Some("person"), Some("new")), "{asks}");
    h.get_by_label("Unsaved changes");
    click(&mut h, "Save");
    h.run_steps(3);
    // the dialog came up at once, as rfd's does, and the chain went on
    assert_eq!(answers.lock().unwrap().shown, 1);
    assert!(dir.join("kept.json").is_file(), "saved first");
    assert!(h.state().prompt.is_none());
    h.get_by_label("0 nodes, 0 wires");
    assert_eq!(h.state().window_title(), "Untitled — PlumCraft");
    assert_eq!(h.state().inspect(&h.ctx)["asks"], json!([]));
    assert_eq!(h.state().prefs.prefs().dirs.get("workflow.write").map(PathBuf::from), Some(dir), "the folder is remembered");
}

#[test]
fn a_script_answers_the_file_ask_that_ui_invoke_raises() {
    let dir = folder("script");
    let (mut h, answers) = editor();
    // Save As: the reply comes at once, with the ask; no dialog was shown
    let r = control(&mut h, "ui.invoke", json!({"command": "graph.saveAs"}));
    assert_eq!(r["ok"], true, "{r}");
    let ask = r["result"]["asks"][0].clone();
    assert_eq!((ask["kind"].as_str(), ask["by"].as_str(), ask["params"]["purpose"].as_str()), (Some("file.save"), Some("script"), Some("workflow")), "{r}");
    assert_eq!(ask["params"]["name"], "untitled.json");
    assert_eq!(answers.lock().unwrap().shown, 0, "the script answers, not the dialog");
    h.run_steps(3);
    h.get_by_label("Waiting for a script");
    // hostile answers: refused, and the ask stays
    let id = ask["ask"].as_u64().unwrap();
    for bad in [
        json!({"ask": id}),
        json!({"ask": id, "answer": {"path": 3}}),
        json!({"ask": id, "answer": {"path": "/a.json"}, "cancel": true}),
        json!({"ask": id, "answer": {"path": "/a.json", "evil": 1}}),
        json!({"ask": "x", "answer": {}}),
        json!({"ask": id + 99, "answer": {"path": "/a.json"}}),
        json!({"ask": id, "cancel": "yes"}),
        json!({"ask": id, "answer": {"path": "/a.json"}, "more": 1}),
    ] {
        let r = control(&mut h, "ask.answer", bad.clone());
        assert_eq!(r["ok"], false, "{bad}: {r}");
    }
    assert_eq!(h.state().asks.len(), 1);
    // the answer: saved, and the title follows
    let path = dir.join("scripted.json");
    let r = control(&mut h, "ask.answer", json!({"ask": id, "answer": {"path": path}}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!((r["result"]["asks"].clone(), r["result"]["title"].as_str()), (json!([]), Some("scripted.json — PlumCraft")), "{r}");
    assert!(path.is_file());
    assert!(control(&mut h, "ask.answer", json!({"ask": id, "answer": {"path": path}}))["error"].as_str().unwrap().contains("already answered"));
    h.run_steps(2);
    assert!(h.query_by_label("Waiting for a script").is_none());

    // a script's whole chain: New over changes, Save, a path, done; nothing shown natively
    h.state_mut().run("node.add", &json!({"kind": "text.upper"}));
    let r = control(&mut h, "ui.invoke", json!({"command": "graph.new"}));
    let prompt = r["result"]["asks"][0]["ask"].clone();
    assert_eq!(r["result"]["prompt"]["kind"], "unsavedChanges", "ui.inspect's prompt is kept: {r}");
    let r = control(&mut h, "ask.answer", json!({"ask": prompt, "answer": {"answer": "save"}}));
    assert_eq!(r["result"]["prompt"], Value::Null, "{r}");
    // (the graph has its file now: Save saves to it, and New goes on)
    assert_eq!(r["result"]["asks"], json!([]), "{r}");
    assert_eq!(r["result"]["graph"]["nodes"], json!([]), "{r}");
    assert!(std::fs::read_to_string(&path).unwrap().contains("text.upper"));
    // over a graph without a file, the chain has a file ask in it
    h.state_mut().run("node.add", &json!({"kind": "text.upper"}));
    let r = control(&mut h, "ui.invoke", json!({"command": "app.quit"}));
    let prompt = r["result"]["asks"][0]["ask"].clone();
    let r = control(&mut h, "ask.answer", json!({"ask": prompt, "answer": {"answer": "save"}}));
    let save = r["result"]["asks"][0].clone();
    assert_eq!((save["kind"].as_str(), save["by"].as_str()), (Some("file.save"), Some("script")), "{r}");
    // the script cancels: still open, nothing quits
    let r = control(&mut h, "ask.answer", json!({"ask": save["ask"], "cancel": true}));
    assert_eq!((r["result"]["closing"].as_bool(), r["result"]["dirty"].as_bool()), (Some(false), Some(true)), "{r}");
    assert_eq!(answers.lock().unwrap().shown, 0);
    // ui.prompt still answers a prompt (a convenience over ask.answer)
    control(&mut h, "ui.invoke", json!({"command": "graph.new"}));
    let r = control(&mut h, "ui.prompt", json!({"answer": "discard"}));
    assert_eq!((r["result"]["asks"].clone(), r["result"]["dirty"].as_bool()), (json!([]), Some(false)), "{r}");
}

/// Send a control request as the transport does, and step frames until its reply.
fn deferred(h: &mut Harness<'static, EditorApp>, method: &str, params: Value) -> (Value, Duration) {
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
    let (start, mut longest) = (Instant::now(), Duration::ZERO);
    loop {
        if let Ok(v) = rx.try_recv() {
            return (v, longest);
        }
        assert!(start.elapsed() < Duration::from_secs(30), "{method}: no reply");
        let t = Instant::now();
        h.step();
        longest = longest.max(t.elapsed());
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn an_answer_that_switches_engines_replies_once_switched_and_no_frame_waits() {
    use plumcraft_session::commands::{CommandForm, FieldKind, FormField, register_forms};
    let field = |key, label| FormField { key, label, hint: "", kind: FieldKind::Line, required: true };
    register_forms(vec![CommandForm {
        command: "engine.switch",
        title: "Switch Engine",
        intro: "",
        fields: vec![field("engine", "Engine"), field("mode", "Mode")],
        submit: "Switch",
        prefill: None,
    }])
    .unwrap();
    let text = || -> Arc<dyn plumcraft_host::Connection> {
        let host = plumcraft_engine::host::Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
        host.wait_ready(Duration::from_secs(10));
        Arc::new(plumcraft_host::InProc(host))
    };
    let slow = Duration::from_millis(500);
    let connector: plumcraft_ui::engine_view::Connector = Box::new(move |_engine, mode, ctx| {
        if mode != "inproc" {
            std::thread::sleep(slow);
        }
        plumcraft_ui::link::Link::start(text(), format!("fake {mode}"), ctx.cloned())
    });
    let conn = text();
    let s = plumcraft_session::Session::new(serde_json::from_value(conn.call("schema", json!({})).unwrap()).unwrap());
    let app = EditorApp::new(s).with_link(plumcraft_ui::link::Link::start(conn, "in-process", None).unwrap()).with_engines(
        plumcraft_editor_core::engines::choices(),
        Some(("text".into(), "inproc".into())),
        connector,
    );
    let mut h = harness(app);
    // the form is an ask; the reply comes at once
    let (r, _) = deferred(&mut h, "ui.invoke", json!({"command": "engine.switch"}));
    let ask = r["result"]["asks"][0].clone();
    assert_eq!((ask["kind"].as_str(), ask["params"]["command"].as_str()), (Some("form"), Some("engine.switch")), "{r}");
    assert!(h.state().command_form.is_some(), "the window shows it too");
    // a missing field: refused by the form, which stays open saying why
    let (r, _) = deferred(&mut h, "ask.answer", json!({"ask": ask["ask"], "answer": {"values": {"engine": "text"}}}));
    assert_eq!(r["result"]["asks"][0]["params"]["error"], "Mode is needed", "{r}");
    // the answer switches: the reply waits for the switch, the frames don't
    let (r, longest) = deferred(&mut h, "ask.answer", json!({"ask": ask["ask"], "answer": {"values": {"engine": "text", "mode": "process"}}}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!((r["result"]["engine"]["pending"].as_bool(), r["result"]["asks"].clone()), (Some(false), json!([])), "{r}");
    assert!(r["result"]["engine"]["label"].as_str().unwrap_or_default().contains("process"), "{r}");
    assert!(longest < slow / 2, "a frame waited for the engine: {longest:?}");
    assert!(h.state().command_form.is_none(), "the window closed with its ask");
}

#[test]
fn edit_paste_asks_for_the_clipboard_and_the_paste_event_answers() {
    let (mut h, _) = editor();
    let a = h.state().session.graph().nodes.keys().next().unwrap().clone();
    h.state_mut().canvas.picked.set(vec![a.clone()]);
    h.event(egui::Event::Copy);
    h.run_steps(2);
    let text = h.state().copied.clone().expect("copied through a clipboard.set ask");
    assert!(h.state().asks.is_empty(), "answered at once");
    let ctx = h.ctx.clone();
    h.state_mut().edit_op(&ctx, plumcraft_ui::edit_view::EditOp::Paste);
    let asks = h.state().inspect(&h.ctx)["asks"].clone();
    assert_eq!(asks[0]["kind"], "clipboard.get", "{asks}");
    h.event(egui::Event::Paste(text));
    h.run_steps(3);
    h.get_by_label("2 nodes, 0 wires");
    assert!(h.state().asks.is_empty());
}
