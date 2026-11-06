//! Viewers in the editor (docs/research/16-viewers.md), driven as a person would:
//!
//! - **Pop-outs** follow their port: open beside the Watched panel, updated by the next run after
//!   an edit, never modal (the canvas still takes edits while one is open), closed by
//!   `view.close` or their Close button.
//! - **The panel's tabs**: a value offers several viewers (an array: Pictures, Values and Bytes;
//!   short text, inline, only Text); `view.viewer` switches, and the panel draws the new one.
//! - **Values on wires**: hovering a wire shows its value's preview; a value not yet known is
//!   asked for (`view.peek`) without changing what the panel watches.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{add, harness, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_engine::host::Host;
use plumcraft_host::{Connection, InProc};
use plumcraft_session::run::WatchRole;
use plumcraft_ui::EditorApp;
use plumcraft_ui::link::Link;
use serde_json::{Value, json};

/// Literal → Upper → Words and Literal → Codes, on the in-process text engine.
fn editor() -> (Harness<'static, EditorApp>, [String; 4]) {
    let mut s = session();
    let a = add(&mut s, "text.literal", 20.0, 20.0);
    s.execute("node.set", &json!({"node": a, "port": "text", "value": "the corgi naps"})).unwrap();
    let u = add(&mut s, "text.upper", 420.0, 20.0);
    let w = add(&mut s, "text.words", 820.0, 20.0);
    let c = add(&mut s, "text.codes", 420.0, 320.0);
    for (from, to) in [(&a, &u), (&u, &w), (&a, &c)] {
        s.execute("wire.connect", &json!({"from": {"node": from, "port": "text"}, "to": {"node": to, "port": "text"}})).unwrap();
    }
    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    let conn: Arc<dyn Connection> = Arc::new(InProc(host));
    let app = EditorApp::new(s).with_link(Link::start(conn, "in-process", None).unwrap());
    // `PLUMCRAFT_SHOT_DIR`: render with the GPU, so a test can keep pictures (`shot`).
    let h = if std::env::var_os("PLUMCRAFT_SHOT_DIR").is_some() {
        let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).wgpu().build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
        h.run_steps(6);
        tidy(&mut h);
        h
    } else {
        harness(app)
    };
    (h, [a, u, w, c])
}

/// With `PLUMCRAFT_SHOT_DIR` set, save what the harness shows as `<dir>/<name>.png`.
fn shot(h: &mut Harness<'static, EditorApp>, name: &str) {
    let Some(dir) = std::env::var_os("PLUMCRAFT_SHOT_DIR") else { return };
    let img = h.render().expect("a GPU render");
    img.save(std::path::Path::new(&dir).join(format!("{name}.png"))).unwrap();
}

/// With `PLUMCRAFT_SHOT_DIR` set, Edit › Tidy (with the sizes the canvas measured) and fit, so
/// pictures show a tidy graph; then wait out the run it may set off.
fn tidy(h: &mut Harness<'static, EditorApp>) {
    if std::env::var_os("PLUMCRAFT_SHOT_DIR").is_none() {
        return;
    }
    h.state_mut().invoke("graph.tidy");
    h.run_steps(4);
    until(h, "the run after Tidy", idle);
}

fn cmd(h: &mut Harness<'static, EditorApp>, id: &str, p: Value) -> Value {
    let r = h.state_mut().try_run(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    h.run_steps(2);
    r
}

/// Step until `done` (10 s at most).
fn until(h: &mut Harness<'static, EditorApp>, what: &str, done: impl Fn(&EditorApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(3);
}

fn idle(a: &EditorApp) -> bool {
    !a.session.run.running && a.session.run.outbox_is_empty()
}

fn popout_summary(a: &EditorApp, id: u64) -> Option<String> {
    a.popouts.iter().find(|w| w.id == id).and_then(|w| w.pane.display()).map(|d| d.summary.clone())
}

#[test]
fn popouts_follow_their_port_and_never_block_the_editor() {
    let (mut h, [a, u, w, _]) = editor();
    cmd(&mut h, "run.watch", json!({"node": u}));
    let id = cmd(&mut h, "view.popout", json!({"node": w}))["id"].as_u64().unwrap();
    until(&mut h, "the pop-out's value", |s| idle(s) && popout_summary(s, id).is_some());
    let first = popout_summary(h.state(), id).unwrap();
    // The pop-out is drawn (an in-editor window here: the kittest has one viewport), titled.
    h.get_by_label(&format!("{w}.count"));
    // Not modal: the canvas still takes an edit while it is open; the next run updates it.
    cmd(&mut h, "node.set", json!({"node": a, "port": "text", "value": "two corgis nap in the sun"}));
    assert!(h.state().session.graph().node(&a.as_str().into()).is_some_and(|n| n.values["text"] == "two corgis nap in the sun"));
    h.state_mut().start_run_if_enabled();
    until(&mut h, "the updated value", |s| idle(s) && popout_summary(s, id).is_some_and(|x| x != first));
    let v = h.state().popouts[0].pane.display().and_then(|d| d.value.clone());
    assert_eq!(v, Some(json!(6)), "six words now");
    // The panel still shows its own port.
    assert_eq!(h.state().session.run.panel().map(|(w, _)| w.node.clone()), Some(u.clone()));
    // ui.inspect lists it; the Close button closes it.
    let ctx = h.ctx.clone();
    let insp = h.state_mut().control_now(&ctx, "ui.inspect", &Value::Null)["result"].clone();
    assert_eq!(insp["popouts"][0]["id"], id, "{}", insp["popouts"]);
    common::click(&mut h, "Close");
    assert!(h.state().popouts.is_empty() && h.state().session.views.popouts.is_empty());
    assert!(h.state().session.run.entry(WatchRole::Popout(id)).is_none());
}

#[test]
fn the_panel_offers_every_viewer_of_its_value_as_tabs() {
    let (mut h, [_, u, _, c]) = editor();
    // Short text comes inline: one viewer, no tabs.
    cmd(&mut h, "run.watch", json!({"node": u}));
    h.state_mut().start_run_if_enabled();
    until(&mut h, "the text", |s| idle(s) && s.session.run.panel().and_then(|(_, d)| d).is_some());
    assert!(h.query_by_label("Bytes").is_none());
    // An array: Pictures, Values and Bytes.
    cmd(&mut h, "run.watch", json!({"node": c}));
    h.state_mut().start_run_if_enabled();
    until(&mut h, "the array", |s| {
        idle(s) && s.session.run.panel().and_then(|(_, d)| d).is_some_and(|d| d.media == plumcraft_proto::catalogue::Media::Ndarray)
    });
    for tab in ["Pictures", "Values", "Bytes"] {
        h.get_by_label(tab);
    }
    cmd(&mut h, "view.viewer", json!({"viewer": "hex"}));
    h.run_steps(3);
    assert_eq!(h.state().panel_pane.current(), Some("hex"));
    h.get_by_label_contains("00000000  74 00 00 00 68 00 00 00");
    cmd(&mut h, "view.viewer", json!({"viewer": "array.grid"}));
    h.run_steps(4);
    assert_eq!(h.state().panel_pane.current(), Some("array.grid"));
    let state = h.state().panel_pane.inspect()["state"].clone();
    assert_eq!(state["range"], json!([32.0, 116.0]), "space to t: {state}");
    // A pop-out of the same value, in its own viewer.
    let id = cmd(&mut h, "view.popout", json!({"node": c, "viewer": "array.grid"}))["id"].as_u64().unwrap();
    until(&mut h, "the pop-out", |s| idle(s) && popout_summary(s, id).is_some());
    h.run_steps(3);
    assert_eq!(h.state().popouts[0].pane.current(), Some("array.grid"));
}

#[test]
fn hovering_a_wire_shows_its_value() {
    let (mut h, [a, u, w, _]) = editor();
    cmd(&mut h, "run.watch", json!({"node": u}));
    h.state_mut().start_run_if_enabled();
    until(&mut h, "the first run", idle);
    let style = h.ctx.global_style();
    let catalog = h.state().session.catalog().clone();
    let mids = h.state().canvas.wire_midpoints(&style, &catalog);
    // Upper → Words: its value (Upper's text) is the panel's, already known.
    let (at, ..) = mids.iter().find(|(_, f, t)| f.node.0 == u && t.node.0 == w).cloned().unwrap();
    h.hover_at(at);
    let rest = Instant::now();
    while rest.elapsed() < Duration::from_millis(400) {
        std::thread::sleep(Duration::from_millis(30));
        h.run_steps(1);
    }
    h.get_by_label_contains(&format!("{u}.text -> {w}.text"));
    h.get_by_label("click: watch · double-click: pop out");
    assert!(h.query_all_by_label_contains("THE CORGI NAPS").count() >= 3, "on the node, in the panel and in the preview");
    shot(&mut h, "wire-preview-known");
    // Literal → Upper: not watched, so peeked; the panel keeps its port.
    let (at, ..) = mids.iter().find(|(_, f, t)| f.node.0 == a && t.node.0 == u).cloned().unwrap();
    h.hover_at(at);
    until(&mut h, "the peeked value", |s| s.wires.cached(&a, "text").is_some());
    assert_eq!(h.state().session.run.panel().map(|(w, _)| w.node.clone()), Some(u.clone()));
    h.run_steps(3);
    h.get_by_label_contains(&format!("{a}.text -> {u}.text"));
    assert!(h.query_all_by_label_contains("the corgi naps").count() >= 2, "on the node and in the preview");
    shot(&mut h, "wire-preview-peeked");
}

/// A table (Letter counts) in the Watched panel through egui_extras' `TableBuilder`: the header
/// names the columns, a click on one sorts by it (descending on a second click), and the
/// values are there.
#[test]
fn tables_show_in_a_table_builder_and_sort_by_a_header_click() {
    let (mut h, [a, ..]) = editor();
    let l = cmd(&mut h, "node.add", json!({"kind": "text.letters", "pos": [420, 600]}))["id"].as_str().unwrap().to_owned();
    cmd(&mut h, "wire.connect", json!({"from": {"node": a, "port": "text"}, "to": {"node": l, "port": "text"}}));
    cmd(&mut h, "run.watch", json!({"node": l}));
    h.state_mut().start_run_if_enabled();
    until(&mut h, "the table", |s| idle(s) && s.session.run.panel().and_then(|(_, d)| d).is_some_and(|d| d.media == plumcraft_proto::catalogue::Media::Table));
    assert_eq!(h.state().panel_pane.current(), Some("table.grid"));
    h.get_by_label("letter");
    // ("count" is also Word count's output pin: the panel's is the right-most)
    h.query_all_by_label("count").max_by(|x, y| x.rect().min.x.total_cmp(&y.rect().min.x)).unwrap().click();
    h.run_steps(3);
    assert_eq!(h.state().panel_pane.inspect()["state"]["sort"], json!({"column": 1, "ascending": true}));
    h.get_by_label("count ^").click();
    h.run_steps(3);
    assert_eq!(h.state().panel_pane.inspect()["state"]["sort"], json!({"column": 1, "ascending": false}));
    tidy(&mut h);
    shot(&mut h, "table-builder");
    // Rows (Table to Rows) show in the table viewer too, by default, the first row as names.
    let r = cmd(&mut h, "node.add", json!({"kind": "seq.table_rows", "pos": [820, 600]}))["id"].as_str().unwrap().to_owned();
    cmd(&mut h, "wire.connect", json!({"from": {"node": l, "port": "table"}, "to": {"node": r, "port": "table"}}));
    cmd(&mut h, "run.watch", json!({"node": r}));
    h.state_mut().start_run_if_enabled();
    until(&mut h, "the rows", |s| idle(s) && s.session.run.panel().and_then(|(_, d)| d).is_some_and(|d| d.node == r));
    h.run_steps(3);
    assert_eq!(h.state().panel_pane.current(), Some("table.grid"), "{}", h.state().panel_pane.inspect());
    assert!(h.state().panel_pane.inspect()["viewers"].as_array().unwrap().contains(&json!("tree")), "the tree is still offered");
    h.get_by_label("letter");
    assert_eq!(h.state().panel_pane.inspect()["state"]["sort"], Value::Null, "another port starts unsorted");
    tidy(&mut h);
    shot(&mut h, "rows-as-table");
}

/// An STL file read by Load Files opens in the Mesh viewer (#38): parsed, drawn, turned by a
/// drag and fitted again by a double-click, with its bytes a tab away.
#[test]
fn an_stl_file_shows_as_a_mesh_to_turn() {
    use plumcraft_proto::mesh::{tetrahedron, write_binary_stl};
    let dir = plumcraft_scratch::dir("kittest-stl");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("tetra.stl");
    std::fs::write(&file, write_binary_stl("solid but binary", &tetrahedron())).unwrap();
    let (mut h, _) = editor();
    let load = cmd(&mut h, "node.add", json!({"kind": "file.load", "pos": [20.0, 600.0]}))["id"].as_str().unwrap().to_owned();
    cmd(&mut h, "node.set", json!({"node": load, "port": "file", "value": file.to_string_lossy()}));
    cmd(&mut h, "run.watch", json!({"node": load}));
    h.state_mut().start_run_if_enabled();
    until(&mut h, "the mesh", |s| idle(s) && s.panel_pane.current() == Some("mesh") && s.panel_pane.inspect()["state"]["rendered"].is_array());
    let state = h.state().panel_pane.inspect()["state"].clone();
    assert_eq!((state["state"].as_str(), state["triangles"].as_u64(), state["ascii"].as_bool()), (Some("ok"), Some(4), Some(false)), "{state}");
    assert_eq!(state["bounds"], json!([[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]));
    h.get_by_label("Bytes");
    h.get_by_label_contains("4 triangles · binary · 1 × 1 × 1");
    shot(&mut h, "stl-mesh");
    // a drag turns it, a double-click fits it again
    let yaw = state["camera"]["yaw"].as_f64().unwrap();
    let at = h.get_by_label_contains("4 triangles").rect().center() + egui::vec2(0.0, 150.0);
    h.drag_at(at);
    h.run_steps(1);
    h.hover_at(at + egui::vec2(60.0, 0.0));
    h.run_steps(1);
    h.drop_at(at + egui::vec2(60.0, 0.0));
    h.run_steps(3);
    let turned = h.state().panel_pane.inspect()["state"]["camera"]["yaw"].as_f64().unwrap();
    assert!((turned - yaw).abs() > 0.1, "dragged: {yaw} → {turned}");
    common::click(&mut h, "Fit");
    h.run_steps(2);
    assert_eq!(h.state().panel_pane.inspect()["state"]["camera"]["yaw"].as_f64(), Some(yaw));
}
