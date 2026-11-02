//! Frames, driven through egui_kittest: drawn from the file, moved and resized by their tab and
//! handle, retitled and recoloured from their menu, made from the selection, saved.

mod common;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use plumcraft_graph::{FrameId, NodeId};
use plumcraft_ui::EditorApp;
use serde_json::{Value, json};

use common::{add, click, click_at, harness, session};

const F1: &str = "f1";

fn frame(h: &Harness<'_, EditorApp>) -> plumcraft_graph::Frame {
    h.state().session.graph().frame(&FrameId::from(F1)).expect("the frame").clone()
}

fn pos(h: &Harness<'_, EditorApp>, n: &str) -> [f32; 2] {
    h.state().session.graph().node(&NodeId::from(n)).expect("the node").pos
}

/// Two nodes, a frame around the first, and a view at scale 1 with its origin in the canvas.
fn framed() -> (Harness<'static, EditorApp>, String, String) {
    let mut s = session();
    let inside = add(&mut s, "text.upper", 120.0, 200.0);
    let outside = add(&mut s, "text.upper", 700.0, 200.0);
    s.execute("frame.add", &json!({"title": "inputs", "pos": [100, 150], "size": [300, 250]})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    (h, inside, outside)
}

/// Press at `from`, move to `to` in `steps`, release; as a person drags.
fn drag(h: &mut Harness<'static, EditorApp>, from: egui::Pos2, to: egui::Pos2, steps: u32) {
    h.hover_at(from);
    h.run_steps(2);
    h.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.run_steps(1);
    for i in 1..=steps {
        h.hover_at(from + (to - from) * (i as f32 / steps as f32));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(2);
}

fn zoom(h: &Harness<'_, EditorApp>) -> f32 {
    h.state().canvas.to_global.scaling
}

#[test]
fn the_tab_shows_the_title_and_a_frame_with_none_is_called_frame() {
    let (mut h, ..) = framed();
    assert!(h.query_by_label("frame inputs").is_some());
    h.state_mut().session.execute("frame.set", &json!({"frame": F1, "title": ""})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("frame Frame").is_some());
    assert!(h.query_by_label("frame inputs").is_none());
}

#[test]
fn a_frame_is_painted_under_the_nodes() {
    let (mut h, ..) = framed();
    let inside_canvas = h.state().canvas.to_global * egui::pos2(390.0, 390.0);
    assert!(h.state().canvas.rect.contains(inside_canvas));
    h.state_mut().session.execute("frame.set", &json!({"frame": F1, "color": "#b54a4a"})).unwrap();
    h.run_steps(3);
    let tinted = h.render().expect("a picture");
    h.state_mut().session.execute("frame.remove", &json!({"frame": F1})).unwrap();
    h.run_steps(3);
    let plain = h.render().expect("a picture");
    let px = |img: &image::RgbaImage| {
        let ppp = img.width() as f32 / 1400.0;
        *img.get_pixel((inside_canvas.x * ppp) as u32, (inside_canvas.y * ppp) as u32)
    };
    assert_ne!(px(&tinted), px(&plain), "the frame's body shows on the canvas");
}

#[test]
fn dragging_the_tab_moves_the_frame_and_what_it_holds() {
    let (mut h, inside, outside) = framed();
    let (f0, n0, o0) = (frame(&h).pos, pos(&h, &inside), pos(&h, &outside));
    let undo = h.state().session.undo_label().map(str::to_owned);
    let at = h.get_by_label("frame inputs").rect().center();
    drag(&mut h, at, at + egui::vec2(60.0, 90.0), 6);
    let z = zoom(&h);
    let f1 = frame(&h).pos;
    let moved = [f1[0] - f0[0], f1[1] - f0[1]];
    assert!((moved[0] - 60.0 / z).abs() < 12.0 / z && (moved[1] - 90.0 / z).abs() < 12.0 / z, "moved by {moved:?}");
    assert_eq!(pos(&h, &inside), [n0[0] + moved[0], n0[1] + moved[1]], "the node inside goes with it");
    assert_eq!(pos(&h, &outside), o0, "the node outside stays");
    assert_eq!(h.state().session.undo_label().map(str::to_owned), undo, "a move isn't an undo step");
    assert!(h.state().session.is_dirty());
}

#[test]
fn a_frame_dragged_over_a_node_leaves_it_there() {
    let (mut h, inside, outside) = framed();
    let o0 = pos(&h, &outside);
    let z = zoom(&h);
    let at = h.get_by_label("frame inputs").rect().center();
    // right, past the other node (it is 580 units to the right of the frame's corner)
    drag(&mut h, at, at + egui::vec2(800.0 * z, 0.0), 24);
    assert!(frame(&h).pos[0] > o0[0], "the frame passed it: {:?}", frame(&h).pos);
    assert_eq!(pos(&h, &outside), o0, "and didn't collect it");
    assert!(pos(&h, &inside)[0] > o0[0], "what it held went with it");
}

#[test]
fn the_handle_resizes_and_nothing_moves() {
    let (mut h, inside, ..) = framed();
    let n0 = pos(&h, &inside);
    let s0 = frame(&h).size;
    let at = h.get_by_label("Resize frame inputs").rect().center();
    drag(&mut h, at, at + egui::vec2(120.0, 60.0), 6);
    let z = zoom(&h);
    let s1 = frame(&h).size;
    assert!((s1[0] - s0[0] - 120.0 / z).abs() < 12.0 / z && (s1[1] - s0[1] - 60.0 / z).abs() < 12.0 / z, "{s0:?} -> {s1:?}");
    assert_eq!(pos(&h, &inside), n0);
    // past the smallest a frame can be
    let at = h.get_by_label("Resize frame inputs").rect().center();
    drag(&mut h, at, at - egui::vec2(700.0, 700.0), 6);
    assert_eq!(frame(&h).size, [plumcraft_graph::MIN_FRAME_SIDE; 2]);
}

/// Open a menu or submenu by its label.
fn open_menu_item(h: &mut Harness<'static, EditorApp>, label: &str) {
    h.get_by_label_contains(label).click();
    h.run_steps(4);
}

/// Right-click the title: its menu, not the palette over it.
fn open_menu(h: &mut Harness<'static, EditorApp>) {
    let at = h.get_by_label("frame inputs").rect().center();
    h.hover_at(at);
    h.run_steps(2);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(3);
}

#[test]
fn the_title_opens_the_menu_and_not_the_palette() {
    let (mut h, ..) = framed();
    open_menu(&mut h);
    assert!(h.state().canvas.palette.is_none());
    assert!(h.query_by_label("Remove Frame").is_some());
    assert!(h.query_by_label("frame f1").is_some(), "the menu names it");
}

#[test]
fn the_tabs_menu_button_opens_the_same_menu() {
    let (mut h, ..) = framed();
    click(&mut h, "Frame actions");
    assert!(h.query_by_label("Remove Frame").is_some());
}

#[test]
fn remove_leaves_the_nodes_and_undoes() {
    let (mut h, inside, ..) = framed();
    open_menu(&mut h);
    click(&mut h, "Remove Frame");
    h.run_steps(3);
    assert!(h.state().session.graph().frames.is_empty());
    assert!(h.state().session.graph().node(&NodeId::from(inside.as_str())).is_some());
    assert!(h.query_by_label("frame inputs").is_none());
    h.state_mut().run("edit.undo", &Value::Null);
    h.run_steps(3);
    assert!(h.query_by_label("frame inputs").is_some());
}

#[test]
fn a_colour_from_the_menu_is_saved_on_the_frame() {
    let (mut h, ..) = framed();
    open_menu(&mut h);
    open_menu_item(&mut h, "Colour");
    click(&mut h, "■ Green");
    h.run_steps(3);
    assert_eq!(frame(&h).color.as_deref(), Some("#3f8f5a"));
    assert_eq!(h.state().session.undo_label(), Some("Edit Frame"));
    // and back to the theme's
    open_menu(&mut h);
    open_menu_item(&mut h, "Colour");
    click(&mut h, "Theme accent");
    h.run_steps(3);
    assert_eq!(frame(&h).color, None);
}

#[test]
fn rename_from_the_menu_retitles_as_one_undo_step() {
    let (mut h, ..) = framed();
    open_menu(&mut h);
    click(&mut h, "Rename…");
    h.run_steps(4);
    assert!(h.state().rename.is_some(), "the window opened");
    h.state_mut().rename.as_mut().unwrap().text = "  sources ".into();
    h.run_steps(2);
    click(&mut h, "Rename");
    h.run_steps(3);
    assert!(h.state().rename.is_none());
    assert_eq!(frame(&h).title, "sources");
    assert_eq!(h.state().session.undo_label(), Some("Edit Frame"));
    h.state_mut().run("edit.undo", &Value::Null);
    assert_eq!(frame(&h).title, "inputs");
}

#[test]
fn a_double_click_on_the_title_opens_the_rename_window() {
    let mut s = session();
    s.execute("frame.add", &json!({"title": "inputs", "pos": [100, 150], "size": [300, 250]})).unwrap();
    // 60 frames a second: kittest's default step puts two clicks too far apart for a double
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, app: &mut EditorApp| app.show(ui), EditorApp::new(s));
    h.run_steps(8);
    let at = h.get_by_label("frame inputs").rect().center();
    h.hover_at(at);
    h.run_steps(2);
    for pressed in [true, false, true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        h.step();
    }
    h.run_steps(3);
    assert!(h.state().rename.is_some());
}

#[test]
fn a_frame_removed_under_the_rename_window_closes_it() {
    let (mut h, ..) = framed();
    open_menu(&mut h);
    click(&mut h, "Rename…");
    h.run_steps(4);
    h.state_mut().run("frame.remove", &json!({"frame": F1}));
    h.run_steps(3);
    assert!(h.state().rename.is_none());
}

#[test]
fn frame_selection_encloses_what_was_drawn() {
    let (mut h, inside, outside) = framed();
    h.state_mut().session.execute("frame.remove", &json!({"frame": F1})).unwrap();
    h.state_mut().canvas.picked.set(vec![NodeId::from(inside.as_str()), NodeId::from(outside.as_str())]);
    h.run_steps(3);
    open_menu_item(&mut h, "Edit");
    open_menu_item(&mut h, "Frame Selection");
    let g = h.state().session.graph();
    let f = g.frames.values().next().expect("a frame").clone();
    for (n, r) in h.state().canvas.node_rects_by_id() {
        assert!(f.contains([r.min.x, r.min.y]) && f.contains([r.max.x, r.max.y]), "{n} {r:?} inside {f:?}");
    }
    assert_eq!(h.state().session.undo_label(), Some("Add Frame"));
}

#[test]
fn frame_selection_is_disabled_with_nothing_selected() {
    let (mut h, ..) = framed();
    open_menu_item(&mut h, "Edit");
    assert!(h.get_by_label_contains("Frame Selection").accesskit_node().is_disabled());
}

#[test]
fn ui_invoke_frames_the_selection() {
    let (mut h, inside, _) = framed();
    h.state_mut().canvas.picked.set(vec![NodeId::from(inside.as_str())]);
    h.state_mut().invoke("frame.add");
    assert_eq!(h.state().session.graph().frames.len(), 2);
    h.state_mut().canvas.picked.clear();
    h.state_mut().invoke("frame.add");
    assert_eq!(h.state().session.graph().frames.len(), 2);
    assert!(h.state().last_error.as_deref().is_some_and(|e| e.contains("select nodes")));
}

#[test]
fn a_collapsed_group_goes_along_on_the_canvas() {
    let mut s = session();
    let n = add(&mut s, "text.upper", 150.0, 220.0);
    let g = s.execute("group.create", &json!({"nodes": [n], "name": "G"})).unwrap()["id"].as_str().unwrap().to_owned();
    s.execute("group.collapse", &json!({"group": g, "collapsed": true})).unwrap();
    s.execute("frame.add", &json!({"title": "inputs", "pos": [100, 150], "size": [300, 250]})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    let was = h.state().session.graph().group(&g.as_str().into()).unwrap().pos;
    let proxy = plumcraft_ui::groups_view::proxy_node_id(&g.as_str().into());
    let drawn = |h: &Harness<'_, EditorApp>| h.state().canvas.node_rects_by_id().into_iter().find(|(id, _)| *id == proxy).expect("its node").1.min.x;
    let x0 = drawn(&h);
    let at = h.get_by_label("frame inputs").rect().center();
    drag(&mut h, at, at + egui::vec2(100.0, 0.0), 5);
    let now = h.state().session.graph().group(&g.as_str().into()).unwrap().pos;
    assert!(now[0] > was[0], "{was:?} -> {now:?}");
    h.run_steps(3);
    assert_eq!(drawn(&h) - x0, now[0] - was[0], "its node is drawn there");
    assert_eq!(pos(&h, &n)[0] - now[0], 150.0 - was[0], "and it opens there");
}

#[test]
fn fit_takes_in_the_frames() {
    let (mut h, ..) = framed();
    h.state_mut().session.execute("frame.add", &json!({"title": "far", "pos": [3000, 1500], "size": [400, 300]})).unwrap();
    h.state_mut().canvas.request_fit();
    h.run_steps(8);
    let c = &h.state().canvas;
    assert!(c.rect.contains(c.to_global * egui::pos2(3400.0, 1800.0)), "the far frame's corner is in view");
}

#[test]
fn a_click_on_the_tab_keeps_the_selection_and_watches_no_wire() {
    let (mut h, inside, outside) = framed();
    h.state_mut().canvas.picked.set(vec![NodeId::from(inside.as_str())]);
    h.run_steps(2);
    let at = h.get_by_label("frame inputs").rect().center();
    click_at(&mut h, at);
    assert_eq!(h.state().canvas.selection(), [NodeId::from(inside.as_str())]);
    // a wire running behind the tab: lay one out so that its midpoint is under the title
    let literal = h.state_mut().session.execute("node.add", &json!({"kind": "text.literal", "pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
    h.state_mut().session.execute("wire.connect", &json!({"from": {"node": literal, "port": "text"}, "to": {"node": outside, "port": "text"}})).unwrap();
    h.run_steps(4);
    let style = h.ctx.global_style();
    let (mid, ..) = h.state().canvas.wire_midpoints(&style, h.state().session.catalog())[0].clone();
    let z = zoom(&h);
    let f = frame(&h);
    let delta = (mid - at) / z;
    h.state_mut().session.execute("frame.move", &json!({"frame": F1, "pos": [f.pos[0] + delta.x, f.pos[1] + delta.y], "nodes": [], "groups": []})).unwrap();
    h.run_steps(4);
    let at = h.get_by_label("frame inputs").rect().center();
    assert!(h.state().canvas.wire_at(&style, h.state().session.catalog(), at).is_some(), "the wire is under the title");
    click_at(&mut h, at);
    assert!(h.state().session.run.watch.is_empty(), "the click was the tab's, not the wire's");
}

#[test]
fn ui_inspect_lists_the_frames() {
    let (h, inside, _) = framed();
    let state = h.state().inspect(&h.ctx);
    assert_eq!(state["frames"][0]["id"], F1);
    assert_eq!(state["frames"][0]["title"], "inputs");
    assert_eq!(state["frames"][0]["nodes"], json!([inside]));
    assert_eq!(state["rename"], Value::Null);
}
