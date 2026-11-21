//! The pop-out's bar (the audit's third gap): Copy, Save As…, Share… and Pin as A or B, each the
//! core's command for that pop-out, dispatched by id (`value.copy`, `value.saveAs`, `value.share`
//! invoked for the person; `viewer.pin {slot, popout}`), none of them carried out by egui. egui
//! has no share sheet, so it tells the core so and the core disables Share with its reason (the
//! maintainer, 2026-11-21: "Disable").

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{add, session, shot};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use plumcraft_engine::host::Host;
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::catalogue::SaveFormat;
use plumcraft_session::run::WatchRole;
use plumcraft_ui::link::Link;
use plumcraft_ui::{EditorApp, FileDialogs, popouts};
use serde_json::json;

#[derive(Default, Clone)]
struct Seen {
    saves: Arc<Mutex<Vec<PathBuf>>>,
}

/// Dialogs that record what they were asked: the save dialog is cancelled.
struct Fake(Seen);

impl FileDialogs for Fake {
    fn pick_path(&mut self, _: &plumcraft_proto::catalogue::PathHint, _: &str) -> Option<PathBuf> {
        None
    }
    fn save_output(&mut self, _formats: &[SaveFormat], suggested: &Path) -> Option<PathBuf> {
        self.0.saves.lock().unwrap().push(suggested.to_path_buf());
        None
    }
}

/// Literal ("the corgi naps") → Upper, run, Upper's text popped out.
fn rig() -> (Harness<'static, EditorApp>, Seen, u64) {
    let mut s = session();
    let a = add(&mut s, "text.literal", 20.0, 20.0);
    s.execute("node.set", &json!({"node": a, "port": "text", "value": "the corgi naps"})).unwrap();
    let u = add(&mut s, "text.upper", 420.0, 20.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": u, "port": "text"}})).unwrap();
    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    let conn: Arc<dyn Connection> = Arc::new(InProc(host));
    let seen = Seen::default();
    let mut app = EditorApp::new(s).with_link(Link::start(conn, "in-process", None).unwrap());
    app.dialogs = Box::new(Fake(seen.clone()));
    let mut h = common::harness_pictured(app, egui::vec2(1400.0, 800.0));
    let r = h.state_mut().try_run("view.popout", &json!({"node": u, "port": "text"})).unwrap();
    let id = r["id"].as_u64().expect("a pop-out id");
    h.state_mut().start_run_if_enabled();
    until(&mut h, "the value in the pop-out", |a| a.session.run.entry(WatchRole::Popout(id)).is_some_and(|(_, d)| d.is_some()) && !a.session.run.running);
    (h, seen, id)
}

fn until(h: &mut Harness<'static, EditorApp>, what: &str, done: impl Fn(&EditorApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(3);
}

fn click(h: &mut Harness<'static, EditorApp>, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(4);
}

#[test]
fn the_bar_offers_every_action_the_apple_apps_viewers_have() {
    let (mut h, ..) = rig();
    shot(&mut h, "popout-bar");
    for label in [popouts::COPY, popouts::SAVE_AS, popouts::SHARE, popouts::PIN_A, popouts::PIN_B] {
        h.get_by_label(label);
    }
}

#[test]
fn copy_puts_the_value_on_the_clipboard_through_the_core() {
    let (mut h, ..) = rig();
    click(&mut h, popouts::COPY);
    // the core's value.copy made the text and raised clipboard.set, which egui answered
    assert_eq!(h.state().copied.as_deref(), Some("THE CORGI NAPS"));
}

#[test]
fn save_as_goes_through_the_cores_save() {
    let (mut h, seen, _) = rig();
    click(&mut h, popouts::SAVE_AS);
    // value.saveAs is run.save's: the text engine writes no files (no `output.save`), so the core
    // says why and asks nothing; an engine that writes gets the save dialog (kittest_save)
    assert!(seen.saves.lock().unwrap().is_empty());
    let said = h.state().last_error.clone().unwrap_or_default();
    assert!(said.starts_with("can't save:"), "{said}");
}

#[test]
fn share_is_disabled_because_the_core_says_this_editor_cant_share() {
    let (mut h, _, id) = rig();
    let reason = plumcraft_editor_core::canvas_cmds::SHARE_UNAVAILABLE;
    // the bar's Share is drawn disabled, and a click on it does nothing
    let share = h.get_by_label(popouts::SHARE);
    assert!(share.accesskit_node().is_disabled(), "Share is disabled in egui");
    click(&mut h, popouts::SHARE);
    assert!(h.state().doc.asks.first(plumcraft_editor_core::asks::Kind::Share).is_none(), "no share ask");
    // the reason is the core's: command.list, the File menu and the toolbar say it
    let ctx = h.ctx.clone();
    let rows = h.state_mut().control_now(&ctx, "command.list", &json!({}));
    let row = |id: &str| rows["result"].as_array().unwrap().iter().find(|r| r["id"] == json!(id)).cloned().unwrap();
    for cmd in ["value.share", "graph.share"] {
        assert_eq!(row(cmd)["enabled"], json!(false), "{cmd}");
        assert_eq!(row(cmd)["disabledReason"], json!(reason), "{cmd}");
    }
    let menu = h.state_mut().control_now(&ctx, "menu.get", &json!({}));
    let text = menu.to_string();
    assert!(text.contains(reason), "File › Share… is disabled with the core's reason");
    // a script invoking value.share is refused with that reason, and nothing is shared
    let r = h.state_mut().control_now(&ctx, "ui.invoke", &json!({"command": "value.share", "params": {"popout": id}}));
    assert_eq!(r["ok"], json!(false), "{r}");
    assert_eq!(r["error"], json!(format!("value.share is not available: {reason}")), "{r}");
    // and run directly, as the C ABI's command.run would
    let r = h.state_mut().try_run("value.share", &json!({"popout": id}));
    assert_eq!(r.unwrap_err(), format!("value.share is not available: {reason}"));
    assert!(h.state().doc.asks.first(plumcraft_editor_core::asks::Kind::Share).is_none());
}

#[test]
fn pin_as_a_and_b_compare_the_pop_outs_value() {
    let (mut h, _, id) = rig();
    click(&mut h, popouts::PIN_A);
    let pinned = |h: &Harness<'static, EditorApp>, r: WatchRole| h.state().session.run.entry(r).map(|(w, _)| (w.node.clone(), w.port.clone()));
    let popped = pinned(&h, WatchRole::Popout(id));
    assert!(popped.is_some());
    assert_eq!(pinned(&h, WatchRole::PinA), popped);
    assert!(h.state().session.views.compare.on, "the compare shows");
    click(&mut h, popouts::PIN_B);
    assert_eq!(pinned(&h, WatchRole::PinB), popped);
}
