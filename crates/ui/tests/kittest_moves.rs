//! Moving things on the canvas (P04 results, 2026-10-27): dragging any node of a pasted or
//! duplicated set moves the whole set; a selected collapsed group moves with a dragged
//! selection; insert-as-group arrives collapsed and moves; a collapsed group moved then
//! expanded shows its members where it now is.

mod common;

use common::{add, harness, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::{GroupId, NodeId};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use serde_json::json;

fn pos(h: &Harness<'static, EditorApp>, id: &str) -> [f32; 2] {
    h.state().session.graph().node(&NodeId::from(id)).unwrap().pos
}

/// A point on a node's header (left of its title), on screen.
fn header(h: &Harness<'static, EditorApp>, id: &str) -> egui::Pos2 {
    let r = h.state().canvas.node_rects_by_id().into_iter().find(|(n, _)| n.0 == id).map(|(_, r)| r).expect("drawn");
    common::on_screen(h, egui::Rect::from_min_size(r.min + egui::vec2(r.width() / 2.0, 2.0), egui::vec2(2.0, 2.0))).center()
}

/// Press at `from`, move by `by` in a few steps, release.
fn drag(h: &mut Harness<'static, EditorApp>, from: egui::Pos2, by: egui::Vec2) {
    h.hover_at(from);
    h.run_steps(2);
    h.drag_at(from);
    h.run_steps(2);
    for i in 1..=4 {
        h.hover_at(from + by * (i as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(from + by);
    h.run_steps(4);
}

#[test]
fn dragging_one_pasted_node_moves_every_pasted_node() {
    let mut s = session();
    let a = add(&mut s, "text.literal", 20.0, 20.0);
    let b = add(&mut s, "text.upper", 400.0, 20.0);
    let mut h = harness(EditorApp::new(s));
    h.state_mut().canvas.picked.set(vec![a.as_str().into(), b.as_str().into()]);
    h.event(egui::Event::Copy);
    h.run_steps(2);
    let text = h.state().copied.clone().unwrap();
    h.event(egui::Event::Paste(text));
    h.run_steps(4);
    let new: Vec<String> = h.state().canvas.picked.nodes().iter().map(|n| n.0.clone()).collect();
    assert_eq!(new.len(), 2);
    let before: Vec<[f32; 2]> = new.iter().map(|n| pos(&h, n)).collect();
    let (a0, b0) = (pos(&h, &a), pos(&h, &b));
    // drag the second copy: both copies move by the same offset
    let from = header(&h, &new[1]);
    drag(&mut h, from, egui::vec2(60.0, 120.0));
    moved_by(&h, &new, &before, egui::vec2(60.0, 120.0));
    assert_eq!((pos(&h, &a), pos(&h, &b)), (a0, b0), "the originals stay");
    // undo takes the paste back (moves aren't undo steps), redo brings it again
    assert_eq!(h.state().session.undo_label(), Some("Paste Nodes"));
    h.state_mut().run("edit.undo", &json!({}));
    h.run_steps(3);
    assert_eq!(h.state().session.graph().nodes.len(), 2);
    h.state_mut().run("edit.redo", &json!({}));
    assert_eq!(h.state().session.graph().nodes.len(), 4);
}

/// Every node in `ids` moved by `by` screen points since `before`.
fn moved_by(h: &Harness<'static, EditorApp>, ids: &[String], before: &[[f32; 2]], by: egui::Vec2) {
    let scale = h.state().canvas.to_global.scaling;
    let after: Vec<[f32; 2]> = ids.iter().map(|n| pos(h, n)).collect();
    for (b4, af) in before.iter().zip(&after) {
        assert!((af[0] - b4[0] - by.x / scale).abs() < 1.0 && (af[1] - b4[1] - by.y / scale).abs() < 1.0, "{ids:?}: {before:?} -> {after:?}");
    }
}

fn ids(h: &Harness<'static, EditorApp>) -> Vec<String> {
    h.state().canvas.picked.nodes().iter().map(|n| n.0.clone()).collect()
}

#[test]
fn dragging_one_duplicated_node_moves_every_copy() {
    let mut s = session();
    let a = add(&mut s, "text.literal", 20.0, 20.0);
    let b = add(&mut s, "text.upper", 400.0, 20.0);
    let mut h = harness(EditorApp::new(s));
    h.state_mut().canvas.picked.set(vec![a.as_str().into(), b.as_str().into()]);
    h.state_mut().invoke("node.duplicate");
    h.run_steps(4);
    let new = ids(&h);
    assert_eq!(new.len(), 2);
    assert!(!new.contains(&a) && !new.contains(&b), "the copies are selected");
    let before: Vec<[f32; 2]> = new.iter().map(|n| pos(&h, n)).collect();
    let from = header(&h, &new[0]);
    drag(&mut h, from, egui::vec2(-40.0, 150.0));
    moved_by(&h, &new, &before, egui::vec2(-40.0, 150.0));
}

/// story -> slow -> upper, and a spare literal.
fn chain() -> (Session, [String; 4]) {
    let mut s = session();
    let story = add(&mut s, "text.literal", 0.0, 0.0);
    let slow = add(&mut s, "text.slow", 320.0, 0.0);
    let up = add(&mut s, "text.upper", 640.0, 0.0);
    let spare = add(&mut s, "text.literal", 0.0, 300.0);
    s.execute("wire.connect", &json!({"from": {"node": story, "port": "text"}, "to": {"node": slow, "port": "text"}})).unwrap();
    s.execute("wire.connect", &json!({"from": {"node": slow, "port": "text"}, "to": {"node": up, "port": "text"}})).unwrap();
    (s, [story, slow, up, spare])
}

fn group(h: &Harness<'static, EditorApp>, g: &str) -> plumcraft_graph::Group {
    h.state().session.graph().group(&GroupId::from(g)).cloned().unwrap()
}

#[test]
fn a_selected_collapsed_group_moves_with_the_dragged_selection() {
    let (s, [story, slow, up, spare]) = chain();
    let mut h = harness(EditorApp::new(s));
    h.state_mut().run("group.create", &json!({"nodes": [slow, up], "name": "think", "collapsed": true}));
    h.run_steps(4);
    // select the spare literal and the group (a Shift+click on its node)
    h.state_mut().canvas.picked.set(vec![spare.as_str().into()]);
    let proxy = header(&h, "group:g1");
    h.hover_at(proxy);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event_modifiers(
            egui::Event::PointerButton { pos: proxy, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::SHIFT },
            egui::Modifiers::SHIFT,
        );
        h.run_steps(1);
    }
    h.run_steps(2);
    assert_eq!(h.state().canvas.picked.groups(), [GroupId::from("g1")]);
    assert_eq!(h.state().canvas.picked.nodes(), [NodeId::from(spare.as_str())]);
    let members = [slow.clone(), up.clone(), spare.clone()];
    let before: Vec<[f32; 2]> = members.iter().map(|n| pos(&h, n)).collect();
    let (g0, story0) = (group(&h, "g1").pos, pos(&h, &story));
    // drag the spare: the group (and so its members) goes along
    let from = header(&h, &spare);
    drag(&mut h, from, egui::vec2(80.0, 40.0));
    moved_by(&h, &members, &before, egui::vec2(80.0, 40.0));
    let scale = h.state().canvas.to_global.scaling;
    let g1 = group(&h, "g1").pos;
    assert!((g1[0] - g0[0] - 80.0 / scale).abs() < 1.0 && (g1[1] - g0[1] - 40.0 / scale).abs() < 1.0, "{g0:?} -> {g1:?}");
    assert_eq!(pos(&h, &story), story0, "unselected stays");
    // and dragging the group's node moves the spare with it
    let before: Vec<[f32; 2]> = members.iter().map(|n| pos(&h, n)).collect();
    let from = header(&h, "group:g1");
    drag(&mut h, from, egui::vec2(-30.0, 60.0));
    moved_by(&h, &members, &before, egui::vec2(-30.0, 60.0));
}

/// A collapsed group moved, then expanded: its members sit where it now is, as they sat in it.
fn expands_where_moved(h: &mut Harness<'static, EditorApp>, g: &str) {
    assert!(group(h, g).collapsed);
    let members: Vec<String> = h.state().session.graph().group_members(&GroupId::from(g)).iter().map(|n| n.0.clone()).collect();
    let before: Vec<[f32; 2]> = members.iter().map(|n| pos(h, n)).collect();
    let g0 = group(h, g).pos;
    let from = header(h, &format!("group:{g}"));
    drag(h, from, egui::vec2(150.0, 200.0));
    moved_by(h, &members, &before, egui::vec2(150.0, 200.0));
    let g1 = group(h, g).pos;
    assert_ne!(g0, g1);
    // its node at its members' top-left still
    let tl = members.iter().map(|n| pos(h, n)).fold([f32::INFINITY, f32::INFINITY], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
    assert_eq!(tl, g1);
    common::click_in_node(h, "Expand group");
    h.run_steps(4);
    assert!(!group(h, g).collapsed);
    let shown: Vec<NodeId> = h.state().canvas.node_rects_by_id().into_iter().map(|(n, _)| n).collect();
    for m in &members {
        assert!(shown.contains(&NodeId::from(m.as_str())), "{m} shown");
        let r = h.state().canvas.node_rects_by_id().into_iter().find(|(n, _)| n.0 == *m).unwrap().1;
        // (a node's rect includes its frame's margin)
        let p = pos(h, m);
        assert!((p[0] - r.min.x - 7.0).abs() < 1.0 && (p[1] - r.min.y - 7.0).abs() < 1.0, "{m} drawn where the graph has it: {p:?}, {r:?}");
    }
}

#[test]
fn cmd_g_group_collapsed_moved_and_expanded_keeps_its_shape() {
    let (s, [_, slow, up, _]) = chain();
    let mut h = harness(EditorApp::new(s));
    h.state_mut().canvas.picked.set(vec![slow.as_str().into(), up.as_str().into()]);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::G);
    h.run_steps(4);
    let at = h.get_by_label("Collapse group").rect().center();
    common::click_at(&mut h, at);
    h.run_steps(4);
    expands_where_moved(&mut h, "g1");
}

#[test]
fn an_open_group_moves_by_its_header() {
    let (s, [_, slow, up, _]) = chain();
    let mut h = harness(EditorApp::new(s));
    h.state_mut().run("group.create", &json!({"nodes": [slow, up], "name": "think"}));
    h.run_steps(4);
    h.state_mut().canvas.picked.clear();
    let members = [slow.clone(), up.clone()];
    let before: Vec<[f32; 2]> = members.iter().map(|n| pos(&h, n)).collect();
    let band = h.get_by_label("Move group think").rect();
    let from = egui::pos2(band.max.x - 6.0, band.center().y);
    drag(&mut h, from, egui::vec2(50.0, 90.0));
    moved_by(&h, &members, &before, egui::vec2(50.0, 90.0));
    // and a member dragged moves (alone: nothing is selected)
    let before = pos(&h, &up);
    let from = header(&h, &up);
    drag(&mut h, from, egui::vec2(20.0, 20.0));
    moved_by(&h, std::slice::from_ref(&up), &[before], egui::vec2(20.0, 20.0));
}

struct TempDir(std::path::PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Insert as Group: it arrives as one collapsed node, selected; it moves, and expands where it
/// was moved to. Undo takes the insert back.
#[test]
fn insert_as_group_arrives_collapsed_moves_and_expands_where_moved() {
    let dir = TempDir(plumcraft_scratch::root().join("moves"));
    std::fs::create_dir_all(&dir.0).unwrap();
    let file = dir.0.join("sketch.json");
    let (mut other, _) = chain();
    other.execute("graph.saveAs", &json!({"path": file})).unwrap();
    let mut s = session();
    add(&mut s, "text.literal", 0.0, 0.0);
    let mut app = EditorApp::new(s);
    struct Pick(std::path::PathBuf);
    impl plumcraft_ui::FileDialogs for Pick {
        fn pick_path(&mut self, _: &plumcraft_proto::catalogue::PathHint, _: &str) -> Option<std::path::PathBuf> {
            None
        }
        fn open_workflow(&mut self) -> Option<std::path::PathBuf> {
            Some(self.0.clone())
        }
        fn save_workflow(&mut self, _: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
            None
        }
    }
    app.dialogs = Box::new(Pick(file.clone()));
    let mut h = harness(app);
    h.state_mut().invoke("graph.insertGroup");
    h.run_steps(4);
    let g = group(&h, "g1");
    assert_eq!(g.name, "sketch");
    assert!(g.collapsed, "arrives collapsed");
    assert_eq!(h.state().canvas.picked.groups(), [GroupId::from("g1")], "the group is selected");
    assert_eq!(h.state().canvas.snarl.nodes_ids_data().count(), 2, "the literal and the group's node");
    expands_where_moved(&mut h, "g1");
    assert_eq!(h.state().session.undo_label(), Some("Expand Group"));
    h.state_mut().run("edit.undo", &json!({}));
    h.state_mut().run("edit.undo", &json!({}));
    assert_eq!(h.state().session.graph().nodes.len(), 1, "the insert undone");
}
