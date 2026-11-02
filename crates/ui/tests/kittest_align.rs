//! Align and Distribute, driven through egui_kittest: the Edit menu's submenus on the canvas's
//! selection, measured as drawn.

mod common;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use plumcraft_graph::NodeId;
use plumcraft_ui::EditorApp;
use serde_json::{Value, json};

use common::{add, harness, session};

/// Nodes of different sizes, selected.
fn three() -> (Harness<'static, EditorApp>, [String; 3]) {
    let mut s = session();
    let a = add(&mut s, "text.upper", 60.0, 80.0);
    let b = add(&mut s, "test.knobs", 420.0, 230.0);
    let c = add(&mut s, "text.replace", 900.0, 130.0);
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    let ids = [a, b, c];
    h.state_mut().canvas.picked.set(ids.iter().map(|n| n.as_str().into()).collect());
    h.run_steps(3);
    (h, ids)
}

/// Edit › `menu` › `item`.
fn choose(h: &mut Harness<'static, EditorApp>, menu: &str, item: &str) {
    h.get_by_label("Edit").click();
    h.run_steps(3);
    h.get_by_label_contains(menu).click();
    h.run_steps(4);
    h.get_by_label(item).click();
    h.run_steps(4);
}

/// Each node's rect as drawn, in the order given.
fn rects(h: &Harness<'_, EditorApp>, ids: &[String]) -> Vec<egui::Rect> {
    let drawn = h.state().canvas.node_rects_by_id();
    ids.iter().map(|id| drawn.iter().find(|(n, _)| n.0 == *id).expect("drawn").1).collect()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.51
}

#[test]
fn each_edge_lines_up_as_the_nodes_are_drawn() {
    type Edge = fn(&egui::Rect) -> f32;
    let check: [(&str, Edge); 6] = [
        ("Left Edges", |r| r.left()),
        ("Right Edges", |r| r.right()),
        ("Top Edges", |r| r.top()),
        ("Bottom Edges", |r| r.bottom()),
        ("Centres Horizontally", |r| r.center().x),
        ("Centres Vertically", |r| r.center().y),
    ];
    for (item, f) in check {
        let (mut h, ids) = three();
        let before = h.state().session.graph().clone();
        choose(&mut h, "Align", item);
        let rs = rects(&h, &ids);
        assert!(near(f(&rs[0]), f(&rs[1])) && near(f(&rs[1]), f(&rs[2])), "{item}: {rs:?}");
        assert_eq!(h.state().session.undo_label(), Some("Align"), "{item}");
        h.state_mut().run("edit.undo", &Value::Null);
        assert_eq!(h.state().session.graph().nodes, before.nodes, "{item}");
    }
}

#[test]
fn distribute_makes_the_gaps_equal() {
    for (item, axis) in [("Horizontally", 0), ("Vertically", 1)] {
        let (mut h, ids) = three();
        choose(&mut h, "Distribute", item);
        let mut rs = rects(&h, &ids);
        rs.sort_by(|a, b| a.min[axis].total_cmp(&b.min[axis]));
        let gap = |a: &egui::Rect, b: &egui::Rect| b.min[axis] - a.max[axis];
        let (g1, g2) = (gap(&rs[0], &rs[1]), gap(&rs[1], &rs[2]));
        assert!((g1 - g2).abs() < 1.0, "{item}: {g1} and {g2}");
        assert_eq!(h.state().session.undo_label(), Some("Distribute"), "{item}");
    }
}

#[test]
fn the_menus_need_enough_selected() {
    let (mut h, ids) = three();
    let disabled = |h: &mut Harness<'static, EditorApp>, label: &str| {
        h.get_by_label("Edit").click();
        h.run_steps(3);
        let off = h.get_by_label_contains(label).accesskit_node().is_disabled();
        h.get_by_label("Edit").click();
        h.run_steps(3);
        off
    };
    assert!(!disabled(&mut h, "Align") && !disabled(&mut h, "Distribute"));
    h.state_mut().canvas.picked.set(vec![ids[0].as_str().into(), ids[1].as_str().into()]);
    h.run_steps(2);
    assert!(!disabled(&mut h, "Align") && disabled(&mut h, "Distribute"), "two");
    h.state_mut().canvas.picked.set(vec![ids[0].as_str().into()]);
    h.run_steps(2);
    assert!(disabled(&mut h, "Align"), "one");
}

#[test]
fn a_collapsed_group_lines_up_as_its_node() {
    let mut s = session();
    let a = add(&mut s, "text.upper", 60.0, 80.0);
    let b = add(&mut s, "text.upper", 500.0, 300.0);
    let g = s.execute("group.create", &json!({"nodes": [b], "name": "G"})).unwrap()["id"].as_str().unwrap().to_owned();
    s.execute("group.collapse", &json!({"group": g, "collapsed": true})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    let picked: Vec<NodeId> = vec![a.as_str().into()];
    h.state_mut().canvas.picked.set(picked);
    let group = plumcraft_ui::groups_view::proxy_node_id(&g.as_str().into());
    // Shift+click its node, as a person adds it to the selection
    let at = h.state().canvas.node_rects_by_id().into_iter().find(|(n, _)| *n == group).unwrap().1;
    let at = common::on_screen(&h, at).center_top() + egui::vec2(0.0, 10.0);
    h.hover_at(at);
    h.run_steps(2);
    for pressed in [true, false] {
        let m = egui::Modifiers::SHIFT;
        h.event_modifiers(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: m }, m);
        h.run_steps(1);
    }
    h.run_steps(2);
    assert_eq!((h.state().canvas.picked.nodes().len(), h.state().canvas.picked.groups().len()), (1, 1), "both are selected");
    choose(&mut h, "Align", "Bottom Edges");
    let drawn = h.state().canvas.node_rects_by_id();
    let bottom = |id: &NodeId| drawn.iter().find(|(n, _)| n == id).unwrap().1.bottom();
    // (a proxy's frame draws a unit further from its position than a node's)
    assert!((bottom(&a.as_str().into()) - bottom(&group)).abs() <= 1.0, "{drawn:?}");
    let g = h.state().session.graph();
    assert_eq!(g.node(&b.as_str().into()).unwrap().pos, g.groups.values().next().unwrap().pos, "its member goes with it");
}
