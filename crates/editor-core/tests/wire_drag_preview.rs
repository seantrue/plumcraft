//! B158: a wire follows a drag on its bend points. A move of a bend point's drag, or of a
//! right-angled route's corner or leg, answers the wire as it would be (`update.wire`: its nid,
//! its bend points and its path), built by the scene's own path code: the path the scene holds
//! once the drag ends is the path the last move gave. A cancel changes nothing; other drags carry
//! no wire.

use plumcraft_editor_core::control::Shell;
use plumcraft_editor_core::document::{Document, execute};
use plumcraft_editor_core::gesture;
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

fn step(d: &mut Document, phase: &str, at: [f64; 2]) -> Value {
    gesture(d, json!({"kind": "drag", "phase": phase, "at": at, "slop": 4}))
}

/// The scene's wire into `to`'s `text`: its nid, bend points and path.
fn scene_wire(d: &mut Document, to: &str) -> (u64, Value, Value) {
    let sc = serde_json::to_value(d.scene().unwrap()).unwrap();
    let id = format!("{to}.text");
    let w = sc["wires"].as_array().unwrap().iter().find(|w| w["id"] == id.as_str()).unwrap().clone();
    // nids are given when a scene is published
    let sc = d.scene().unwrap();
    d.publish_scene(sc);
    let nid = d.snapshot.wire_by_id(&id).unwrap().nid;
    (nid, w["via"].clone(), w["path"].clone())
}

fn via(d: &Document, to: &str) -> Vec<[i32; 2]> {
    d.session.graph().wire_into(&PortRef::new(to, "text")).map(|w| w.via.clone()).unwrap_or_default()
}

/// Two faces left to right, the wire bent through (430, 200).
fn bent() -> (Document, String) {
    let mut d = doc();
    let a = upper(&mut d, [0.0, 0.0]);
    let b = upper(&mut d, [600.0, 0.0]);
    wire(&mut d, &a, &b);
    execute(&mut d, "wire.route", &json!({"to": {"node": b, "port": "text"}, "via": [[430, 200]]})).unwrap();
    (d, b)
}

#[test]
fn a_bend_points_move_answers_the_wire_as_it_would_be_and_the_end_gives_it_that_path() {
    let (mut d, b) = bent();
    let (nid, _, before) = scene_wire(&mut d, &b);
    let begin = step(&mut d, "begin", [430.0, 200.0]);
    assert_eq!(begin["drag"]["kind"], "bend", "{begin}");
    let m = step(&mut d, "move", [450.0, 230.0]);
    let w = &m["update"]["wire"];
    assert_eq!(w["nid"], nid, "{m}");
    assert_eq!(w["via"], json!([[450, 230]]));
    assert_ne!(w["path"], before, "the wire moves with the drag");
    // the path passes through the moved bend point
    let pts = w["path"].as_array().unwrap();
    assert!(pts.iter().any(|p| p == &json!([450.0, 230.0])), "{w}");
    // nothing changed yet
    assert_eq!(via(&d, &b), vec![[430, 200]]);
    let shown = w["path"].clone();
    let e = step(&mut d, "end", [450.0, 230.0]);
    assert_eq!(e["commands"][0]["command"], "wire.route", "{e}");
    let (_, v, after) = scene_wire(&mut d, &b);
    assert_eq!(v, json!([[450, 230]]));
    assert_eq!(after, shown, "the scene's path is the path the move showed");
}

#[test]
fn a_cancelled_bend_drag_leaves_the_wire_as_it_was() {
    let (mut d, b) = bent();
    let (_, _, before) = scene_wire(&mut d, &b);
    step(&mut d, "begin", [430.0, 200.0]);
    let m = step(&mut d, "move", [500.0, 300.0]);
    assert!(m["update"]["wire"].is_object(), "{m}");
    let c = gesture(&mut d, json!({"kind": "drag", "phase": "cancel"}));
    assert_eq!(c["cancelled"], true);
    assert_eq!(via(&d, &b), vec![[430, 200]]);
    assert_eq!(scene_wire(&mut d, &b).2, before);
}

#[test]
fn a_routes_corner_drag_answers_the_wire_as_it_would_be() {
    // the source to the right: an automatic route round the faces, corners at
    // (884, 43) (884, 104) (-24, 104) (-24, 43)
    let mut d = doc();
    let a = upper(&mut d, [600.0, 0.0]);
    let b = upper(&mut d, [0.0, 0.0]);
    wire(&mut d, &a, &b);
    let (nid, _, _) = scene_wire(&mut d, &b);
    let begin = step(&mut d, "begin", [884.0, 104.0]);
    assert_eq!(begin["drag"]["kind"], "route", "{begin}");
    let m = step(&mut d, "move", [904.0, 144.0]);
    let w = &m["update"]["wire"];
    assert_eq!(w["nid"], nid, "{m}");
    assert_eq!(w["via"], m["update"]["via"], "update.via stays (B155)");
    let shown = w["path"].clone();
    assert!(shown.as_array().is_some_and(|p| p.len() >= 4));
    step(&mut d, "end", [904.0, 144.0]);
    let (_, v, after) = scene_wire(&mut d, &b);
    assert_eq!(v, w["via"]);
    assert_eq!(after, shown, "the scene's path is the path the move showed");
}

#[test]
fn other_drags_carry_no_wire() {
    let (mut d, _) = bent();
    // a face's header: a move
    let begin = step(&mut d, "begin", [100.0, 10.0]);
    assert_eq!(begin["drag"]["kind"], "move", "{begin}");
    let m = step(&mut d, "move", [140.0, 30.0]);
    assert!(m["update"].get("wire").is_none(), "{m}");
    step(&mut d, "end", [140.0, 30.0]);
    // empty canvas: a marquee
    step(&mut d, "begin", [300.0, -300.0]);
    let m = step(&mut d, "move", [320.0, -280.0]);
    assert!(m["update"].get("wire").is_none(), "{m}");
    gesture(&mut d, json!({"kind": "drag", "phase": "cancel"}));
}
