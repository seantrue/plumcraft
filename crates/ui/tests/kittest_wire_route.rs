//! Wires with bend points (lifted from the research software): Add Bend Point from the wire menu, the curve
//! drawn by the canvas (not snarl), dragging a bend point, double-clicking it away, Straighten,
//! and a bent wire's value previewed on hover, as any wire's.

mod common;

use common::{add, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::PortRef;
use plumcraft_ui::EditorApp;
use serde_json::json;

fn harness() -> (Harness<'static, EditorApp>, PortRef) {
    let mut s = session();
    let a = add(&mut s, "text.literal", 0.0, 0.0);
    let u = add(&mut s, "text.upper", 500.0, 300.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": u, "port": "text"}})).unwrap();
    // 60 frames a second: kittest's default step (0.25 s) puts two clicks further apart than
    // egui's double-click time (0.3 s).
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, app: &mut EditorApp| app.show(ui), EditorApp::new(s));
    h.run_steps(6);
    (h, PortRef::new(u.as_str(), "text"))
}

fn via(h: &Harness<'_, EditorApp>, to: &PortRef) -> Vec<[i32; 2]> {
    h.state().session.graph().wire_into(to).unwrap().via.clone()
}

fn press(h: &mut Harness<'_, EditorApp>, at: egui::Pos2, button: egui::PointerButton) {
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(3);
}

fn mid(h: &Harness<'_, EditorApp>) -> egui::Pos2 {
    let style = h.ctx.global_style();
    let catalog = h.state().session.catalog().clone();
    h.state().canvas.wire_midpoints(&style, &catalog)[0].0
}

#[test]
fn bend_points_are_added_dragged_removed_and_straightened() {
    let (mut h, to) = harness();
    let at = mid(&h);
    press(&mut h, at, egui::PointerButton::Secondary);
    h.get_by_label("Add Bend Point").click();
    h.run_steps(4);
    let v = via(&h, &to);
    assert_eq!(v.len(), 1, "one bend point, at the click");
    let t = h.state().canvas.to_global;
    let graph_mid = t.inverse() * at;
    assert!((v[0][0] as f32 - graph_mid.x).abs() <= 1.0 && (v[0][1] as f32 - graph_mid.y).abs() <= 1.0, "{v:?} vs {graph_mid:?}");
    assert_eq!(h.state().session.undo_label(), Some("Route Wire"));
    // the canvas draws it now, not snarl, and still finds it under the pointer
    assert_eq!(h.state().canvas.snarl.wires().count(), 0);
    assert!(h.state().canvas.routed_at(at).is_some());
    // drag the bend point 80 down
    let handle = h.state().canvas.bend_points()[0].2;
    h.hover_at(handle);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: handle, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.run_steps(1);
    for k in 1..=5 {
        h.event(egui::Event::PointerMoved(handle + egui::vec2(0.0, 16.0 * k as f32)));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton {
        pos: handle + egui::vec2(0.0, 80.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(3);
    let moved = via(&h, &to);
    assert_eq!(moved[0][0], v[0][0]);
    assert!((moved[0][1] - v[0][1] - (80.0 / t.scaling) as i32).abs() <= 2, "{moved:?} from {v:?}");
    // a second point from the menu on the bent wire (a quarter of the way along), then a
    // double-click removes it again
    let on_wire = h.state().canvas.routed_point(&to, 0.25).unwrap();
    press(&mut h, on_wire, egui::PointerButton::Secondary);
    h.get_by_label("Add Bend Point").click();
    h.run_steps(4);
    assert_eq!(via(&h, &to).len(), 2);
    let first_place = egui::pos2(moved[0][0] as f32, moved[0][1] as f32);
    let second = h
        .state()
        .canvas
        .bend_points()
        .into_iter()
        .map(|(_, _, p)| p)
        .find(|p| (h.state().canvas.to_global.inverse() * *p).distance(first_place) > 4.0)
        .unwrap();
    h.hover_at(second);
    h.run_steps(1);
    for pressed in [true, false, true, false] {
        h.event(egui::Event::PointerButton { pos: second, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(3);
    assert_eq!(via(&h, &to), moved, "a double-click removes the point it was on");
    // the wire menu on the routed wire: Straighten
    let on_wire = mid(&h);
    press(&mut h, on_wire, egui::PointerButton::Secondary);
    assert!(h.state().canvas.wire_menu.is_some(), "the menu opens on a routed wire");
    h.get_by_label("Straighten").click();
    h.run_steps(4);
    assert!(via(&h, &to).is_empty());
    assert_eq!(h.state().session.undo_label(), Some("Straighten Wire"));
    assert_eq!(h.state().canvas.snarl.wires().count(), 1, "snarl's again");
    // undo brings the bend back
    h.state_mut().try_run("edit.undo", &json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(via(&h, &to).len(), 1);
}

/// With `PLUMCRAFT_SHOT_DIR` set: a picture of a wire bent round a node in its way.
#[test]
fn bent_wire_picture() {
    let Some(dir) = std::env::var_os("PLUMCRAFT_SHOT_DIR") else { return };
    let mut s = session();
    // Text → Upper case → Join, and Text → Join straight past Upper case
    let a = add(&mut s, "text.literal", 0.0, 120.0);
    let block = add(&mut s, "text.upper", 380.0, 100.0);
    let j = add(&mut s, "text.join", 760.0, 120.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": block, "port": "text"}})).unwrap();
    s.execute("wire.connect", &json!({"from": {"node": block, "port": "text"}, "to": {"node": j, "port": "b"}})).unwrap();
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": j, "port": "a"}})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 600.0)).wgpu().build_ui_state(|ui, app: &mut EditorApp| app.show(ui), EditorApp::new(s));
    h.run_steps(4);
    // Tidy first (the pictures show tidy graphs), then bend Text → Join under Upper case
    h.state_mut().invoke("graph.tidy");
    h.run_steps(4);
    let pu = h.state().session.graph().nodes[&block.as_str().into()].pos;
    let low = pu[1] + 150.0;
    let via = [[pu[0] - 30.0, low], [pu[0] + 230.0, low]];
    h.state_mut().try_run("wire.route", &json!({"to": {"node": j, "port": "a"}, "via": via})).unwrap();
    h.state_mut().canvas.request_fit();
    h.run_steps(8);
    h.render().unwrap().save(std::path::Path::new(&dir).join("bent-wire.png")).unwrap();
}
