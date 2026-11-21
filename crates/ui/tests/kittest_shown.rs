//! The egui canvas draws what the core's scene shows (N4 slice 4, item 6): faces pushed out of
//! each other's way where the scene shows them, frames grown round members pushed, and a wire
//! that runs right to left along the core's route round the faces, hit where it is drawn, by
//! the canvas and by the core alike.

mod common;

use common::{add, harness, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::{NodeId, PortRef};
use plumcraft_ui::EditorApp;
use serde_json::{Value, json};

fn face_pos(h: &Harness<'static, EditorApp>, id: &str) -> [f32; 2] {
    h.state().scene_now().nodes.iter().find(|f| f.id == id).map(|f| f.pos).expect("a face")
}

fn graph_pos(h: &Harness<'static, EditorApp>, id: &str) -> [f32; 2] {
    h.state().session.graph().node(&NodeId::from(id)).unwrap().pos
}

/// Where snarl has node `id` (its position, graph units).
fn snarl_pos(h: &Harness<'static, EditorApp>, id: &str) -> [f32; 2] {
    h.state().canvas.snarl.nodes_pos_ids().find(|(_, _, n)| n.id.0 == id).map(|(_, p, _)| [p.x, p.y]).expect("in the snarl")
}

fn rect_of(h: &Harness<'static, EditorApp>, id: &str) -> egui::Rect {
    h.state().canvas.node_rects_by_id().into_iter().find(|(n, _)| n.0 == id).map(|(_, r)| r).expect("drawn")
}

/// Two literals, the second on top of the first: the scene pushes it out of the way.
fn overlapping() -> (Harness<'static, EditorApp>, String, String) {
    let mut s = session();
    let a = add(&mut s, "text.literal", 0.0, 0.0);
    let b = add(&mut s, "text.literal", 40.0, 40.0);
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    (h, a, b)
}

#[test]
fn a_pushed_face_is_drawn_where_the_scene_shows_it() {
    let (h, a, b) = overlapping();
    let shown = face_pos(&h, &b);
    assert_eq!(graph_pos(&h, &b), [40.0, 40.0], "the push is the view's, not the graph's");
    assert_ne!(shown, [40.0, 40.0], "the scene pushes the lower face out of the way");
    // egui draws it there: snarl has it at the scene's place, its rect follows
    assert_eq!(snarl_pos(&h, &b), shown);
    assert_eq!(snarl_pos(&h, &a), face_pos(&h, &a));
    let (ra, rb) = (rect_of(&h, &a), rect_of(&h, &b));
    assert!(!ra.intersects(rb), "drawn apart: {ra:?} {rb:?}");
    let g = h.state().doc.geometry.get(&b).cloned().expect("measured");
    assert!((rb.min.x - (shown[0] + g.offset[0])).abs() < 0.5 && (rb.min.y - (shown[1] + g.offset[1])).abs() < 0.5, "{rb:?} at {shown:?}");
    // drawing it pushed is no edit
    assert_eq!(graph_pos(&h, &b), [40.0, 40.0]);
    assert_ne!(h.state().session.undo_label(), Some("Move Node"));
}

#[test]
fn a_frame_round_a_pushed_member_is_drawn_grown() {
    let (mut h, a, b) = overlapping();
    // a small frame holding both nodes (by their positions), which the push moves one out of
    let r = h.state_mut().session.execute("frame.add", &json!({"title": "box", "pos": [-20.0, -20.0], "size": [120.0, 120.0]})).unwrap();
    let id = r["id"].as_str().unwrap().to_owned();
    h.run_steps(6);
    let held = h.state().session.graph().frame_held(&plumcraft_graph::FrameId(id.clone()));
    assert!(held.nodes.contains(&NodeId::from(a.as_str())) && held.nodes.contains(&NodeId::from(b.as_str())), "{held:?}");
    let own = h.state().session.graph().frames.get(&plumcraft_graph::FrameId(id.clone())).unwrap().size;
    let scene = h.state().scene_now();
    let grown = scene.frames.iter().find(|f| f.id == id).map(|f| f.size).unwrap();
    assert!(grown[0] > own[0] || grown[1] > own[1], "the scene grows the frame round the pushed member: {own:?} -> {grown:?}");
    // egui draws (and fits, and handles) it at the grown size
    assert_eq!(h.state().canvas.frame_size(&id, own), grown);
    let rb = rect_of(&h, &b);
    let frame = egui::Rect::from_min_size(egui::pos2(-20.0, -20.0), egui::vec2(grown[0], grown[1]));
    assert!(frame.contains_rect(rb), "the pushed member is inside the frame drawn: {rb:?} in {frame:?}");
    // the resize handle sits at the grown corner, on screen
    let handle = h.get_by_label("Resize frame box").rect();
    let corner = h.state().canvas.to_global * frame.max;
    assert!(handle.max.distance(corner) < 6.0, "handle {handle:?} at {corner:?}");
}

#[test]
fn a_backward_wire_is_drawn_along_the_core_route_and_hit_where_drawn() {
    let mut s = session();
    let lit = add(&mut s, "text.literal", 700.0, 0.0);
    let up = add(&mut s, "text.upper", 0.0, 320.0);
    s.execute("wire.connect", &json!({"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    let to = PortRef::new(up.as_str(), "text");
    // snarl doesn't draw it (its own curve would cut across both faces); the canvas does
    assert_eq!(h.state().canvas.snarl.wires().count(), 0, "not snarl's");
    let drawn = h.state().canvas.backward_wires().to_vec();
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].to, to);
    // along the scene's path, which the core hit-tests
    let scene = h.state().scene_now();
    let wire = scene.wires.iter().find(|w| w.to == to).unwrap();
    assert!(plumcraft_ui::shown::runs_backward(&wire.path));
    assert_eq!(drawn[0].path, plumcraft_ui::shown::sampled(&wire.path));
    // it goes round the faces: no point of it inside either
    let (rl, ru) = (rect_of(&h, &lit), rect_of(&h, &up));
    let inside = drawn[0].path.iter().filter(|p| rl.shrink(1.0).contains(**p) || ru.shrink(1.0).contains(**p)).count();
    assert_eq!(inside, 0, "a point of the route inside a face");
    // a point along the route: the canvas hits it, and so does the core
    let ctx = h.ctx.clone();
    for t in [0.2, 0.5, 0.8] {
        let screen = h.state().canvas.routed_point(&to, t).unwrap();
        assert_eq!(h.state().canvas.routed_at(screen).map(|(_, x)| x), Some(to.clone()), "the canvas hits it at {t}");
        let g = h.state().canvas.to_global.inverse() * screen;
        let hit: Value = h.state_mut().control_now(&ctx, "geometry.hit", &json!({"at": [g.x, g.y], "slop": 6.0}));
        let hit = hit.get("result").cloned().unwrap_or(hit);
        assert_eq!(hit["kind"], json!("wire"), "the core hits it at {t}: {hit}");
        assert_eq!(hit["wire"], json!(format!("{up}.text")));
    }
    // a right-click there opens the wire's menu
    let at = h.state().canvas.routed_point(&to, 0.5).unwrap();
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(2);
    assert_eq!(h.state().canvas.wire_menu.as_ref().map(|m| m.to.clone()), Some(to.clone()));
}

#[test]
fn a_wire_moved_forward_is_snarls_again() {
    let mut s = session();
    let lit = add(&mut s, "text.literal", 700.0, 0.0);
    let up = add(&mut s, "text.upper", 0.0, 320.0);
    s.execute("wire.connect", &json!({"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    assert_eq!(h.state().canvas.backward_wires().len(), 1);
    h.state_mut().run("edit.move", &json!({"nodes": {up.as_str(): [1200.0, 320.0]}})).unwrap();
    h.run_steps(4);
    assert!(h.state().canvas.backward_wires().is_empty());
    assert_eq!(h.state().canvas.snarl.wires().count(), 1, "snarl's again");
    // and nothing took it for a wire dropped
    assert!(h.state().session.graph().wire_into(&PortRef::new(up.as_str(), "text")).is_some());
}
