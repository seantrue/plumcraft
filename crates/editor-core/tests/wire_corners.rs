//! B155: a wire's right angles, through what a shell sends. A press on an automatic route's
//! corner or leg is a `wire` hit with its `part`; a drag there turns the route's corners into the
//! wire's own bend points with the one moved, in one `wire.route` (one undo step); a leg moves
//! across itself and the route stays right-angled; Make Right-Angled (`wire.square`) squares a
//! wire's own bend points; Route Automatically takes them away. With odd wires too.

use plumcraft_editor_core::control::Shell;
use plumcraft_editor_core::document::{Document, execute};
use plumcraft_editor_core::{gesture, menu, wire_path};
use plumcraft_graph::PortRef;
use plumcraft_session::Session;
use serde_json::{Value, json};

fn doc() -> Document {
    Document::new(Session::new(plumcraft_text::engine().schema()))
}

/// A `text.upper` face at `pos`, measured as a 260 × 80 face: its input and output pins 43 down.
fn upper(d: &mut Document, pos: [f32; 2]) -> String {
    let n = execute(d, "node.add", &json!({"kind": "text.upper", "pos": pos})).unwrap()["id"].as_str().unwrap().to_owned();
    execute(d, "geometry.set", &json!({"faces": [{"node": n, "size": [260, 80], "header": 28, "pins": {"in": [43], "out": [43]}}]})).unwrap();
    n
}

fn wire(d: &mut Document, a: &str, b: &str) {
    execute(d, "wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}})).unwrap();
}

/// Gesture `g` as `ui.gesture` runs it: its plan, then its commands.
fn gesture(d: &mut Document, g: Value) -> Value {
    let sc = d.scene().unwrap();
    d.publish_scene(sc);
    let snap = std::sync::Arc::clone(&d.snapshot);
    let req = gesture::parse(&g).unwrap();
    let plan = {
        let f = gesture::Facts { graph: d.session.graph(), selection: d.selection.get(), drag: &d.drag };
        gesture::plan(&f, &snap, &req).unwrap()
    };
    let mut out = Value::Object(plan.reply);
    let mut commands = Vec::new();
    for (c, p) in plan.commands {
        let r = execute(d, c, &p);
        commands.push(json!({"command": c, "params": p, "ok": r.is_ok(), "error": r.err()}));
    }
    out["commands"] = json!(commands);
    out
}

/// A drag from `at` by (`dx`, `dy`): begin, move, end. The begin's and the end's replies.
fn drag(d: &mut Document, at: [f64; 2], dx: f64, dy: f64) -> (Value, Value, Value) {
    let b = gesture(d, json!({"kind": "drag", "phase": "begin", "at": at, "slop": 4}));
    let to = [at[0] + dx, at[1] + dy];
    let m = gesture(d, json!({"kind": "drag", "phase": "move", "at": to, "slop": 4}));
    let e = gesture(d, json!({"kind": "drag", "phase": "end", "at": to, "slop": 4}));
    (b, m, e)
}

fn via(d: &Document, to: &str) -> Vec<[i32; 2]> {
    d.session.graph().wire_into(&PortRef::new(to, "text")).map(|w| w.via.clone()).unwrap_or_default()
}

/// Two faces with the source to the right: a wire routed round them automatically. Its corners
/// (output at 860, 43; input at 0, 43; the faces side by side, so the channel is below them):
/// (860, 43) (884, 43) (884, 104) (-24, 104) (-24, 43) (0, 43).
fn backward() -> (Document, String) {
    let mut d = doc();
    let a = upper(&mut d, [600.0, 0.0]);
    let b = upper(&mut d, [0.0, 0.0]);
    wire(&mut d, &a, &b);
    (d, b)
}

#[test]
fn a_press_on_an_automatic_routes_corner_or_leg_is_a_wire_hit_with_its_part() {
    let (mut d, _) = backward();
    let corner = gesture(&mut d, json!({"kind": "drag", "phase": "begin", "at": [884, 104], "slop": 4}));
    assert_eq!(corner["hit"]["kind"], "wire", "{corner}");
    assert_eq!(corner["hit"]["part"], "corner");
    assert_eq!(corner["hit"]["index"], 2);
    assert_eq!(corner["drag"]["kind"], "route");
    gesture(&mut d, json!({"kind": "drag", "phase": "cancel", "at": [884, 104]}));
    let leg = gesture(&mut d, json!({"kind": "drag", "phase": "begin", "at": [400, 104], "slop": 4}));
    assert_eq!((leg["hit"]["part"].clone(), leg["hit"]["index"].clone()), (json!("segment"), json!(2)), "{leg}");
    gesture(&mut d, json!({"kind": "drag", "phase": "cancel", "at": [400, 104]}));
    // the legs that leave and enter the pins are not taken hold of: they stay with the pins
    let pin_leg = gesture(&mut d, json!({"kind": "drag", "phase": "begin", "at": [872, 43], "slop": 4}));
    assert_ne!(pin_leg["hit"]["part"], "segment", "{pin_leg}");
}

#[test]
fn dragging_an_automatic_corner_makes_its_corners_the_wires_own_in_one_undo_step() {
    let (mut d, b) = backward();
    assert!(via(&d, &b).is_empty());
    let (_, m, e) = drag(&mut d, [884.0, 104.0], 0.0, 50.0);
    // the move says where the bend points would be, for the shell to draw
    assert_eq!(m["update"]["via"], json!([[884, 43], [884, 154], [-24, 154], [-24, 43]]), "{m}");
    assert_eq!(e["commands"].as_array().unwrap().len(), 1, "one command: {e}");
    assert_eq!(e["commands"][0]["command"], "wire.route");
    assert_eq!(via(&d, &b), vec![[884, 43], [884, 154], [-24, 154], [-24, 43]]);
    assert!(wire_path::right_angled(&via(&d, &b)));
    // the scene draws it right-angled, through its new corner
    let sc = serde_json::to_value(d.scene().unwrap()).unwrap();
    let w = sc["wires"].as_array().unwrap().iter().find(|w| w["to"]["node"] == b).unwrap().clone();
    assert_eq!(w["kind"], "routed");
    // one undo step takes it all back to the automatic route; redo brings it again
    execute(&mut d, "edit.undo", &json!({})).unwrap();
    assert!(via(&d, &b).is_empty());
    execute(&mut d, "edit.redo", &json!({})).unwrap();
    assert_eq!(via(&d, &b), vec![[884, 43], [884, 154], [-24, 154], [-24, 43]]);
}

#[test]
fn a_leg_moves_across_itself_and_the_route_stays_right_angled() {
    let (mut d, b) = backward();
    // the bottom leg, down 30: both its ends move down, nothing else
    drag(&mut d, [400.0, 104.0], 17.0, 30.0);
    assert_eq!(via(&d, &b), vec![[884, 43], [884, 134], [-24, 134], [-24, 43]]);
    // now the wire's own: its right-hand upright leg, 40 to the right (the drag's up and down is
    // not taken: a vertical leg moves sideways only)
    drag(&mut d, [884.0, 90.0], 40.0, 12.0);
    let v = via(&d, &b);
    assert_eq!(v, vec![[924, 43], [924, 134], [-24, 134], [-24, 43]]);
    assert!(wire_path::right_angled(&v));
    // each drag one undo step
    execute(&mut d, "edit.undo", &json!({})).unwrap();
    assert_eq!(via(&d, &b), vec![[884, 43], [884, 134], [-24, 134], [-24, 43]]);
    execute(&mut d, "edit.undo", &json!({})).unwrap();
    assert!(via(&d, &b).is_empty());
}

#[test]
fn make_right_angled_squares_a_wires_own_bend_points_and_route_automatically_drops_them() {
    let mut d = doc();
    let a = upper(&mut d, [0.0, 0.0]);
    let b = upper(&mut d, [500.0, 0.0]);
    wire(&mut d, &a, &b);
    execute(&mut d, "wire.route", &json!({"to": {"node": b, "port": "text"}, "via": [[380, 10], [420, 120]]})).unwrap();
    let sc = d.scene().unwrap();
    d.publish_scene(sc);
    // the menu offers both
    let facts = d.menu_facts();
    let m = menu::get(&d.session, &facts, &json!({"context": {"kind": "wire", "to": {"node": b, "port": "text"}}}), None).unwrap();
    let labels: Vec<&str> = m["items"].as_array().unwrap().iter().filter_map(|i| i["label"].as_str()).collect();
    assert!(labels.contains(&"Make Right-Angled") && labels.contains(&"Route Automatically"), "{labels:?}");
    let r = execute(&mut d, "wire.square", &json!({"to": {"node": b, "port": "text"}})).unwrap();
    // out of the output across, to each point across first then down or up, into the input
    // down or up first then across: (260, 43) (380, 43) (380, 10) (420, 10) (420, 120) (420, 43)
    // (500, 43); the leg down to 120 and back up to 43 is a spur, ended where it turns back
    assert_eq!(via(&d, &b), vec![[380, 43], [380, 10], [420, 10], [420, 43]], "{r}");
    assert!(wire_path::right_angled(&via(&d, &b)));
    // squared, the menu no longer offers it
    let m = menu::get(&d.session, &facts, &json!({"context": {"kind": "wire", "to": {"node": b, "port": "text"}}}), None).unwrap();
    assert!(!m["items"].as_array().unwrap().iter().any(|i| i["label"] == "Make Right-Angled"));
    // one undo step back to its own points
    execute(&mut d, "edit.undo", &json!({})).unwrap();
    assert_eq!(via(&d, &b), vec![[380, 10], [420, 120]]);
    execute(&mut d, "edit.redo", &json!({})).unwrap();
    // Route Automatically: the menu's item, as it runs
    let m = menu::get(&d.session, &facts, &json!({"context": {"kind": "wire", "to": {"node": b, "port": "text"}}}), None).unwrap();
    let item = m["items"].as_array().unwrap().iter().find(|i| i["label"] == "Route Automatically").cloned().unwrap();
    execute(&mut d, item["command"].as_str().unwrap(), &item["params"]).unwrap();
    assert!(via(&d, &b).is_empty());
    execute(&mut d, "edit.undo", &json!({})).unwrap();
    assert!(wire_path::right_angled(&via(&d, &b)));
}

#[test]
fn make_right_angled_on_an_automatic_route_takes_its_corners() {
    let (mut d, b) = backward();
    let sc = d.scene().unwrap();
    d.publish_scene(sc);
    execute(&mut d, "wire.square", &json!({"to": {"node": b, "port": "text"}})).unwrap();
    assert_eq!(via(&d, &b), vec![[884, 43], [884, 104], [-24, 104], [-24, 43]]);
}

#[test]
fn odd_wires_neither_panic_nor_make_a_route_that_isnt_right_angled() {
    // points on top of each other, a leg of no length, a point straight above the output
    let from = wire_path::as_point(&[260, 43]);
    let to = wire_path::as_point(&[500, 43]);
    for v in [vec![[300, 43], [300, 43]], vec![[260, 43], [260, 90], [260, 90]], vec![[300, 43]], vec![], vec![[500, 43], [260, 43]]] {
        let s = wire_path::square_up(from, to, &v);
        assert!(s.is_empty() || wire_path::right_angled(&s), "{v:?} squared to {s:?}");
        // a drag at any index, in range or not, changes nothing it shouldn't and never panics
        for k in 0..4 {
            for seg in [false, true] {
                let moved = wire_path::route_drag(&s, k, seg, 7.0, -9.0);
                assert_eq!(moved.len(), s.len());
            }
        }
        let _ = wire_path::orthogonal_corners(from, to, &v);
    }
    // huge drags stay in range
    let far = wire_path::route_drag(&[[0, 0], [0, 10]], 0, false, f32::MAX, f32::MIN);
    assert!(far.iter().all(|p| p[0].abs() <= 10_000_000 && p[1].abs() <= 10_000_000), "{far:?}");
    // a wire looping back to a node's own input is refused or routed; a drag on it never panics
    let mut d = doc();
    let a = upper(&mut d, [0.0, 0.0]);
    let looped = execute(&mut d, "wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": a, "port": "text"}}));
    if looped.is_ok() {
        for at in [[284.0, 104.0], [130.0, 104.0], [-24.0, 60.0]] {
            let _ = drag(&mut d, at, 10.0, 10.0);
        }
    }
}
