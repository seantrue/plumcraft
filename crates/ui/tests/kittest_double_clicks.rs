//! A double click on the canvas is the core's double tap (N3's card 3, "the same in every shell";
//! D7 of the egui audit): egui recognises it and sends `ui.gesture {kind: "doubleTap"}`, and the
//! core decides what it opens: empty canvas the palette there, a face its parameters, a
//! collapsed group the group, an output pin or a wire its value in a pop-out.

mod common;

use common::{add, on_screen, session, shot};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::PortRef;
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use serde_json::json;

/// 60 frames a second: kittest's default step puts two clicks too far apart for a double.
fn quick(s: Session) -> Harness<'static, EditorApp> {
    let b = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).with_step_dt(1.0 / 60.0);
    let mut h = if common::shots_dir().is_some() {
        b.wgpu().build_ui_state(|ui, app: &mut EditorApp| app.show(ui), EditorApp::new(s))
    } else {
        b.build_ui_state(|ui, app: &mut EditorApp| app.show(ui), EditorApp::new(s))
    };
    h.run_steps(8);
    h
}

fn double_click_at(h: &mut Harness<'static, EditorApp>, at: egui::Pos2) {
    h.hover_at(at);
    h.run_steps(2);
    for pressed in [true, false, true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        h.step();
    }
    h.run_steps(4);
}

fn popped_out(h: &Harness<'static, EditorApp>) -> Vec<PortRef> {
    let s = &h.state().session;
    s.views
        .popouts
        .iter()
        .filter_map(|o| s.run.entry(plumcraft_session::run::WatchRole::Popout(o.id)).map(|(w, _)| PortRef::new(w.node.as_str(), w.port.as_str())))
        .collect()
}

#[test]
fn a_double_click_on_empty_canvas_opens_the_palette_there() {
    let mut s = session();
    add(&mut s, "text.literal", 40.0, 40.0);
    let mut h = quick(s);
    let at = egui::pos2(700.0, 600.0);
    let g = h.state().canvas.to_global.inverse() * at;
    double_click_at(&mut h, at);
    shot(&mut h, "d7-double-click-empty-canvas");
    let palette = h.state().doc.palette.clone().expect("the core's palette is open");
    let pat = palette.at.expect("at a place");
    assert!((pat[0] as f32 - g.x).abs() < 0.5 && (pat[1] as f32 - g.y).abs() < 0.5, "{pat:?} vs {g:?}");
    assert!(h.state().canvas.palette.is_some(), "egui draws it");
}

#[test]
fn a_double_click_on_an_output_pin_pops_its_value_out() {
    let mut s = session();
    let lit = add(&mut s, "text.literal", 40.0, 40.0);
    let mut h = quick(s);
    let port = PortRef::new(lit.as_str(), "text");
    let catalog = h.state().session.catalog().clone();
    let at = h.state().canvas.pin_screen(&catalog, &port, true).expect("the pin is drawn");
    double_click_at(&mut h, at);
    shot(&mut h, "d7-double-click-output-pin");
    assert_eq!(popped_out(&h), [port]);
    assert!(h.state().session.views.parameters.is_none(), "a pin is not the face");
}

#[test]
fn a_double_click_on_a_wire_pops_its_value_out() {
    let mut s = session();
    let lit = add(&mut s, "text.literal", 40.0, 40.0);
    let up = add(&mut s, "text.upper", 520.0, 40.0);
    s.execute("wire.connect", &json!({"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}})).unwrap();
    let mut h = quick(s);
    let style = h.ctx.global_style();
    let catalog = h.state().session.catalog().clone();
    let (at, from, _) = h.state().canvas.wire_midpoints(&style, &catalog).into_iter().next().expect("a wire drawn");
    double_click_at(&mut h, at);
    assert_eq!(popped_out(&h), [from]);
}

#[test]
fn a_double_click_on_a_collapsed_group_enters_it() {
    let mut s = session();
    let a = add(&mut s, "text.upper", 40.0, 40.0);
    let g = s.execute("group.create", &json!({"nodes": [a], "name": "G"})).unwrap()["id"].as_str().unwrap().to_owned();
    s.execute("group.collapse", &json!({"group": g, "collapsed": true})).unwrap();
    let mut h = quick(s);
    let (_, rect) = h.state().canvas.node_rects_by_id().into_iter().find(|(n, _)| n.0.starts_with("group:")).expect("the group's face");
    let r = on_screen(&h, rect);
    double_click_at(&mut h, egui::pos2(r.right() - 4.0, r.bottom() - 4.0));
    assert_eq!(h.state().doc.focus.as_ref().map(|f| f.0.as_str()), Some(g.as_str()), "the group is entered");
}

#[test]
fn a_double_click_on_a_frames_tab_or_a_groups_band_edits_its_name_through_the_core() {
    let mut s = session();
    let a = add(&mut s, "text.upper", 40.0, 120.0);
    let b = add(&mut s, "text.lower", 400.0, 120.0);
    let _ = a;
    s.execute("frame.add", &json!({"title": "inputs", "pos": [0, 300], "size": [300, 200]})).unwrap();
    s.execute("group.create", &json!({"nodes": [b], "name": "G"})).unwrap();
    let mut h = quick(s);
    let tab = h.get_by_label("frame inputs").rect().center();
    double_click_at(&mut h, tab);
    assert!(
        h.state().doc.naming.as_ref().is_some_and(|n| matches!(n.target, plumcraft_editor_core::naming::NameTarget::Frame { .. })),
        "{:?}",
        h.state().doc.naming
    );
    h.state_mut().run("name.edit", &json!({"open": false}));
    h.run_steps(4);
    let band = h.get_by_label("group G").rect();
    double_click_at(&mut h, band.center());
    assert!(
        h.state().doc.naming.as_ref().is_some_and(|n| matches!(n.target, plumcraft_editor_core::naming::NameTarget::Group { .. })),
        "{:?}",
        h.state().doc.naming
    );
    h.run_steps(4);
    h.get_by_label("Rename “G”");
}

/// A double click on an output pin whose first click watches the value: the run that follows
/// grows the face (its status widens the header), so the pin moves out from under the pointer
/// before the second click. The double tap opens what its first tap hit (the core's
/// `gesture::DOUBLE_TAP`), so it pops the value out rather than opening the node's parameters.
#[test]
fn a_double_click_on_a_pin_that_moves_with_the_first_click_still_pops_out() {
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    let mut s = session();
    let lit = add(&mut s, "text.literal", 40.0, 40.0);
    s.execute("node.set", &json!({"node": lit, "port": "text", "value": "a corgi naps"})).unwrap();
    let up = add(&mut s, "text.upper", 420.0, 40.0);
    s.execute("wire.connect", &json!({"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}})).unwrap();
    let host = plumcraft_engine::host::Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    let conn: Arc<dyn plumcraft_host::Connection> = Arc::new(plumcraft_host::InProc(host));
    let app = EditorApp::new(s).with_link(plumcraft_ui::link::Link::start(conn, "in-process", None).unwrap());
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
    // the application's fonts, whose run status ("cached · 0.00 ms") is wider than the header keeps
    h.state().setup_context(&h.ctx.clone());
    h.run_steps(8);
    h.state_mut().start_run_if_enabled();
    let deadline = Instant::now() + Duration::from_secs(10);
    while (h.state().session.run.running || !h.state().session.run.outbox_is_empty()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        h.run_steps(1);
    }
    h.run_steps(6);
    let port = PortRef::new(up.as_str(), "text");
    let catalog = h.state().session.catalog().clone();
    let at = h.state().canvas.pin_screen(&catalog, &port, true).expect("the pin is drawn");
    let width = |h: &Harness<'static, EditorApp>| h.state().canvas.node_rects_by_id().into_iter().find(|(n, _)| n.0 == up).map(|(_, r)| r.width()).unwrap();
    let before = width(&h);
    double_click_at(&mut h, at);
    assert!(width(&h) > before + 2.0, "the first click's run grew the face: {before} then {}", width(&h));
    assert_eq!(popped_out(&h), [port]);
    assert!(h.state().session.views.parameters.is_none(), "not the face's parameters");
}
