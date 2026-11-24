//! B153: Add Bend Point from a wire's context menu puts the point where the press was, through the
//! path the Apple shells take: a `context` gesture on the wire, then `menu.get` with the gesture
//! reply's `context` (the shells ask for the menu themselves), then the item's command as the menu
//! runs it. For an ordinary wire, and for one routed round its faces because its source is to the
//! right of its target.

use plumcraft_editor_core::control::Shell;
use plumcraft_editor_core::document::{Document, execute};
use plumcraft_editor_core::{gesture, menu, wire_path};
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

/// The context gesture at `at`, as `ui.gesture` plans it (a context gesture runs no command).
fn context_at(d: &mut Document, at: [f64; 2]) -> Value {
    let sc = d.scene().unwrap();
    d.publish_scene(sc);
    let snap = std::sync::Arc::clone(&d.snapshot);
    let req = gesture::parse(&json!({"kind": "context", "at": at, "slop": 6})).unwrap();
    let f = gesture::Facts { graph: d.session.graph(), selection: d.selection.get(), drag: &d.drag };
    let plan = gesture::plan(&f, &snap, &req).unwrap();
    Value::Object(plan.reply)
}

/// The wire into `to`'s `text` as the scene draws it.
fn wire_scene(d: &mut Document, to: &str) -> Value {
    let sc = serde_json::to_value(d.scene().unwrap()).unwrap();
    sc["wires"].as_array().unwrap().iter().find(|w| w["to"]["node"] == to).cloned().unwrap_or(Value::Null)
}

/// A point on the wire's drawn path: the middle of its middle cubic.
fn on_path(w: &Value) -> [f64; 2] {
    let pts: Vec<[f32; 2]> = w["path"].as_array().unwrap().iter().map(|p| [p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32]).collect();
    let segs = (pts.len() - 1) / 3;
    let k = 3 * (segs / 2);
    let c = [pts[k], pts[k + 1], pts[k + 2], pts[k + 3]].map(|p| wire_path::as_point(&[p[0].round() as i32, p[1].round() as i32]));
    let p = wire_path::cubic_at(c, 0.5);
    [f64::from(p.x), f64::from(p.y)]
}

/// Right-click the wire into `to` at `at`, as the Mac and iPad do: the gesture, then `menu.get`
/// with the reply's `context`, then Add Bend Point's command. The wire's bend points after.
fn add_bend_as_the_apple_shells_do(d: &mut Document, to: &str, at: [f64; 2]) -> Vec<[i64; 2]> {
    let r = context_at(d, at);
    assert_eq!(r["context"]["kind"], "wire", "the press is on the wire: {r}");
    let facts = d.menu_facts();
    let m = menu::get(&d.session, &facts, &json!({"context": r["context"]}), None).unwrap();
    let item = m["items"].as_array().unwrap().iter().find(|i| i["label"] == "Add Bend Point").cloned().unwrap();
    execute(d, item["command"].as_str().unwrap(), &item["params"]).unwrap();
    let w = d.session.graph().wire_into(&plumcraft_graph::PortRef::new(to, "text")).unwrap();
    w.via.iter().map(|p| [i64::from(p[0]), i64::from(p[1])]).collect()
}

#[test]
fn add_bend_point_puts_the_point_where_the_press_was_on_an_ordinary_wire() {
    let mut d = doc();
    let a = upper(&mut d, [0.0, 0.0]);
    let b = upper(&mut d, [500.0, 0.0]);
    execute(&mut d, "wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}})).unwrap();
    let at = on_path(&wire_scene(&mut d, &b));
    let via = add_bend_as_the_apple_shells_do(&mut d, &b, at);
    assert_eq!(via, vec![[at[0].round() as i64, at[1].round() as i64]], "the bend point where the press was ({at:?})");
    let w = wire_scene(&mut d, &b);
    assert_eq!(w["kind"], "routed", "drawn through its bend point: {w}");
}

#[test]
fn add_bend_point_puts_the_point_where_the_press_was_on_a_wire_routed_right_to_left() {
    let mut d = doc();
    // the source to the right of the target: the plain wire is routed round the faces
    let a = upper(&mut d, [600.0, 0.0]);
    let b = upper(&mut d, [0.0, 0.0]);
    execute(&mut d, "wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}})).unwrap();
    let before = wire_scene(&mut d, &b);
    assert_eq!(before["kind"], "plain");
    let at = on_path(&before);
    let via = add_bend_as_the_apple_shells_do(&mut d, &b, at);
    assert_eq!(via, vec![[at[0].round() as i64, at[1].round() as i64]], "the bend point where the press was ({at:?})");
    // and it is not under either face, where the wire and its ring would be hidden
    for (x0, x1) in [(600.0, 860.0), (0.0, 260.0)] {
        let p = via[0];
        assert!(!(p[0] as f64 > x0 && (p[0] as f64) < x1 && p[1] > 0 && p[1] < 80), "the point {p:?} is under a face");
    }
}
