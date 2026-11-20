//! Editing values on a face (N4 slice 2): what a tap and a drag on a row mean (`ui.gesture`'s
//! plan), `row.edit` and `row.set` with their checks and undo steps, the more-button, faces kept
//! apart when one grows, and wires that run right to left routed round their faces.

use plumcraft_editor_core::control::Shell;
use plumcraft_editor_core::document::{Document, execute};
use plumcraft_editor_core::gesture;
use plumcraft_session::Session;
use serde_json::{Value, json};

fn doc() -> Document {
    Document::new(Session::new(plumcraft_text::engine().schema()))
}

fn add(d: &mut Document, kind: &str, pos: [f32; 2]) -> String {
    execute(d, "node.add", &json!({"kind": kind, "pos": pos})).unwrap()["id"].as_str().unwrap().to_owned()
}

/// Publish what the document has (as a shell's scene.get does) and run gesture `g` as
/// `ui.gesture` would: its plan, then its commands, each as any command runs.
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
        commands.push(json!({"command": c, "params": p, "ok": r.is_ok(), "result": r.as_ref().ok(), "error": r.err()}));
    }
    out["commands"] = json!(commands);
    out["menu"] = json!(plan.menu);
    out
}

/// `text.widgets` at (0, 0) measured as a 260 × 250 face: header 28, a row a 22 down from 32,
/// each input's widget rect from x 120 to 240 (the text socket has none), the more-button.
fn widgets(d: &mut Document) -> String {
    let n = add(d, "text.widgets", [0.0, 0.0]);
    let rows: Vec<Value> = (0..9).map(|k| if k == 0 { Value::Null } else { json!([120, 32 + 22 * k + 2, 240, 32 + 22 * k + 20]) }).collect();
    let pins: Vec<f64> = (0..9).map(|k| 43.0 + 22.0 * f64::from(k)).collect();
    execute(
        d,
        "geometry.set",
        &json!({"faces": [{"node": n, "size": [260, 250], "header": 28, "pins": {"in": pins, "out": [43]}, "rows": rows,
                            "parts": {"menu": [236, 4, 254, 24], "title": [30, 4, 120, 24]}}]}),
    )
    .unwrap();
    n
}

/// The middle of input row `k`'s widget.
fn row(k: i32) -> [f64; 2] {
    [180.0, f64::from(32 + 22 * k + 11)]
}

fn value(d: &Document, node: &str, port: &str) -> Value {
    d.session.graph().node(&plumcraft_graph::NodeId::from(node)).and_then(|n| n.values.get(port).cloned()).unwrap_or(Value::Null)
}

#[test]
fn a_tap_on_a_row_opens_its_editor_and_a_typed_value_is_one_undo_step() {
    let mut d = doc();
    let n = widgets(&mut d);
    let undo = d.session.undo_label().map(str::to_owned);
    let g = gesture(&mut d, json!({"kind": "tap", "at": row(2), "slop": 2}));
    assert_eq!(g["commands"][0]["command"], "selection.set");
    assert_eq!((g["commands"][1]["command"].clone(), g["commands"][1]["params"].clone()), (json!("row.edit"), json!({"node": n, "port": "count"})));
    let e = d.editing.clone().expect("the editor is open");
    assert_eq!((e.node.as_str(), e.port.as_str(), e.face.as_str(), e.row), (n.as_str(), "count", n.as_str(), 2));
    assert_eq!(d.scene().unwrap().view.edit, Some(e), "the scene's view.edit says so");
    assert_eq!(d.session.undo_label().map(str::to_owned), undo, "opening an editor is no step");
    // a value typed: one node.set, "Set Value"
    let r = execute(&mut d, "row.set", &json!({"node": n, "port": "count", "text": " 7 "})).unwrap();
    assert_eq!(r["value"], json!(7));
    assert_eq!(value(&d, &n, "count"), json!(7));
    assert_eq!(d.session.undo_label(), Some("Set Value"));
    // the same again: nothing
    let r = execute(&mut d, "row.set", &json!({"node": n, "port": "count", "text": "7"})).unwrap();
    assert_eq!(r["unchanged"], json!(true));
    execute(&mut d, "edit.undo", &json!({})).unwrap();
    assert_eq!(value(&d, &n, "count"), json!(3), "undo takes it back");
    execute(&mut d, "edit.redo", &json!({})).unwrap();
    assert_eq!(value(&d, &n, "count"), json!(7));
    // held to the range, with the core's note; a refused edit changes nothing
    let r = execute(&mut d, "row.set", &json!({"node": n, "port": "count", "text": "12"})).unwrap();
    assert_eq!((r["value"].clone(), r["note"].clone()), (json!(10), json!("count is at most 10: set to 10")));
    let before = serde_json::to_value(d.session.graph()).unwrap();
    let e = execute(&mut d, "row.set", &json!({"node": n, "port": "count", "text": "lots"})).unwrap_err();
    assert_eq!(e, "count needs a number: \"lots\" isn't one");
    assert_eq!(serde_json::to_value(d.session.graph()).unwrap(), before, "refused: the graph is as it was");
    let e = execute(&mut d, "row.set", &json!({"node": n, "port": "mode", "value": "sideways"})).unwrap_err();
    assert!(e.contains("isn't one of its choices"), "{e}");
    // a socket-only row isn't edited on the face
    assert!(execute(&mut d, "row.edit", &json!({"node": n, "port": "text"})).unwrap_err().contains("takes a wire only"));
    assert!(execute(&mut d, "row.set", &json!({"node": n, "port": "text", "text": "x"})).is_err());
    // each kind: a choice, several, text, lines, a colour
    for (port, sent, want) in [
        ("mode", json!({"value": "title"}), json!("title")),
        ("tags", json!({"text": "a, b"}), json!("a, b")),
        ("label", json!({"text": "hi"}), json!("hi")),
        ("note", json!({"text": "one\ntwo"}), json!("one\ntwo")),
        ("tint", json!({"value": [1.0, 0.5, 0.0]}), json!("#ff8000")),
        ("amount", json!({"text": "0.25"}), json!(0.25)),
    ] {
        let mut p = sent.clone();
        p["node"] = json!(n);
        p["port"] = json!(port);
        execute(&mut d, "row.set", &p).unwrap_or_else(|e| panic!("{port}: {e}"));
        assert_eq!(value(&d, &n, port), want, "{port}");
    }
    // Escape closes the editor, leaving the value
    execute(&mut d, "row.edit", &json!({"open": false})).unwrap();
    assert!(d.editing.is_none() && d.scene().unwrap().view.edit.is_none());
}

#[test]
fn tab_goes_to_the_next_editable_row_and_a_switch_flips_on_a_tap() {
    let mut d = doc();
    let n = widgets(&mut d);
    let r = execute(&mut d, "row.edit", &json!({"node": n, "port": "label"})).unwrap();
    assert_eq!((r["port"].clone(), r["row"].clone(), r["widget"]["kind"].clone()), (json!("label"), json!(1), json!("text")));
    let r = execute(&mut d, "row.edit", &json!({"step": 1})).unwrap();
    assert_eq!(r["port"], "count");
    let r = execute(&mut d, "row.edit", &json!({"step": -1})).unwrap();
    assert_eq!(r["port"], "label");
    let r = execute(&mut d, "row.edit", &json!({"step": -1})).unwrap();
    assert_eq!(r["port"], "tint", "Shift-Tab from the first wraps to the last");
    assert!(execute(&mut d, "row.edit", &json!({"step": 2})).is_err());
    assert!(execute(&mut d, "row.edit", &json!({"node": n})).is_err());
    // a tap on a switch's row flips it: one step
    let g = gesture(&mut d, json!({"kind": "tap", "at": row(4), "slop": 2}));
    assert_eq!((g["commands"][1]["command"].clone(), g["commands"][1]["params"]["value"].clone()), (json!("row.set"), json!(false)));
    assert_eq!(value(&d, &n, "flag"), json!(false));
    assert_eq!(d.session.undo_label(), Some("Set Value"));
    let g = gesture(&mut d, json!({"kind": "tap", "at": row(4), "slop": 2}));
    assert_eq!(g["commands"][1]["params"]["value"], json!(true), "and back");
    // Shift-tap selects, and edits nothing
    let g = gesture(&mut d, json!({"kind": "tap", "at": row(4), "mods": ["shift"], "slop": 2}));
    assert_eq!(g["commands"].as_array().unwrap().len(), 1);
    // the more-button: the node's menu, and no command
    let g = gesture(&mut d, json!({"kind": "tap", "at": [245, 14], "slop": 2}));
    assert_eq!((g["context"].clone(), g["commands"].clone()), (json!({"kind": "node", "node": n}), json!([])));
    assert!(g["menu"]["context"] == json!({"kind": "node", "node": n}) || g["menu"].is_object(), "{g}");
}

#[test]
fn a_drag_on_a_numbers_row_scrubs_it_and_its_end_is_one_step() {
    let mut d = doc();
    let n = widgets(&mut d);
    let at = row(3);
    let b = gesture(&mut d, json!({"kind": "drag", "phase": "begin", "at": at, "slop": 2, "zoom": 2}));
    assert_eq!((b["drag"]["kind"].clone(), b["drag"]["row"]["port"].clone(), b["drag"]["value"].clone()), (json!("scrub"), json!("amount"), json!(0.5)));
    // 10 graph units at zoom 2 is 20 points: 0.2 at 0.01 a point (amount's two decimals)
    let m = gesture(&mut d, json!({"kind": "drag", "phase": "move", "at": [at[0] + 10.0, at[1]]}));
    assert!((m["update"]["value"].as_f64().unwrap() - 0.7).abs() < 1e-9, "{m}");
    let snap = std::sync::Arc::clone(&d.snapshot);
    let u = d.drag.get().unwrap().update(&snap, plumcraft_editor_core::plain::Point::new(at[0] as f32 + 400.0, at[1] as f32), 2.0);
    assert_eq!(u.value, Some(1.0), "held to the range");
    assert_eq!(value(&d, &n, "amount"), json!(0.5), "nothing changes until the end");
    let e = gesture(&mut d, json!({"kind": "drag", "phase": "end", "at": [at[0] + 10.0, at[1]]}));
    assert_eq!(e["commands"][0]["command"], "row.set");
    assert_eq!(value(&d, &n, "amount"), json!(0.7));
    assert_eq!(d.session.undo_label(), Some("Set Value"));
    execute(&mut d, "edit.undo", &json!({})).unwrap();
    assert_eq!(value(&d, &n, "amount"), json!(0.5));
    // a cancelled scrub sets nothing
    gesture(&mut d, json!({"kind": "drag", "phase": "begin", "at": at, "slop": 2}));
    gesture(&mut d, json!({"kind": "drag", "phase": "cancel"}));
    assert_eq!(value(&d, &n, "amount"), json!(0.5));
    // a drag on a text row still moves the face
    let b = gesture(&mut d, json!({"kind": "drag", "phase": "begin", "at": row(1), "slop": 2}));
    assert_eq!(b["drag"]["kind"], "move");
    gesture(&mut d, json!({"kind": "drag", "phase": "cancel"}));
    assert!(gesture::parse(&json!({"kind": "tap", "at": [0, 0], "zoom": 0})).is_err());
    assert!(gesture::parse(&json!({"kind": "tap", "at": [0, 0], "zoom": "near"})).is_err());
}

#[test]
fn a_face_that_grows_pushes_its_neighbour_out_of_the_way_and_back_with_no_step() {
    let mut d = doc();
    let a = add(&mut d, "text.literal", [0.0, 0.0]);
    let b = add(&mut d, "text.upper", [0.0, 200.0]);
    let c = add(&mut d, "text.lower", [400.0, 0.0]);
    let (dirty, undo) = (d.session.is_dirty(), d.session.undo_label().map(str::to_owned));
    let measure = |d: &mut Document, h: f64| {
        execute(d, "geometry.set", &json!({"faces": [{"node": a, "size": [200, h]}, {"node": b, "size": [200, 100]}, {"node": c, "size": [200, 100]}]}))
            .unwrap();
    };
    let pos = |d: &Document, id: &str| d.scene().unwrap().nodes.iter().find(|f| f.id == id).unwrap().pos;
    measure(&mut d, 120.0);
    assert_eq!((pos(&d, &b), pos(&d, &c)), ([0.0, 200.0], [400.0, 0.0]), "no overlap: nothing moves");
    // a grows past b's top: b is shown below it, with the gap; c (beside it) stays
    measure(&mut d, 300.0);
    assert_eq!(pos(&d, &b), [0.0, 316.0]);
    assert_eq!(pos(&d, &c), [400.0, 0.0]);
    assert_eq!(d.session.graph().node(&plumcraft_graph::NodeId::from(b.as_str())).unwrap().pos, [0.0, 200.0], "the graph is untouched");
    assert_eq!((d.session.is_dirty(), d.session.undo_label().map(str::to_owned)), (dirty, undo), "a face growing is no edit");
    // a grows wider, over c beside it: c is pushed right, the shorter way
    execute(&mut d, "geometry.set", &json!({"faces": [{"node": a, "size": [450, 120]}]})).unwrap();
    assert_eq!(pos(&d, &c), [466.0, 0.0]);
    // it shrinks: they come back
    measure(&mut d, 120.0);
    assert_eq!((pos(&d, &b), pos(&d, &c)), ([0.0, 200.0], [400.0, 0.0]));
    // a drag moves a pushed face from where it is shown
    measure(&mut d, 300.0);
    let e = gesture(&mut d, json!({"kind": "drag", "phase": "begin", "at": [100, 330], "slop": 2}));
    assert_eq!(e["drag"]["kind"], "move");
    let e = gesture(&mut d, json!({"kind": "drag", "phase": "end", "at": [600, 330], "slop": 2}));
    assert_eq!(e["commands"][0]["params"]["nodes"][&b], json!([500.0, 316.0]), "{e}");
}

#[test]
fn a_wire_running_right_to_left_goes_round_its_faces_in_the_scene_and_the_hit_test() {
    let mut d = doc();
    let a = add(&mut d, "text.literal", [400.0, 0.0]);
    let b = add(&mut d, "text.upper", [0.0, 40.0]);
    execute(&mut d, "wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}})).unwrap();
    execute(&mut d, "geometry.set", &json!({"faces": [{"node": a, "size": [200, 100], "pins": {"in": [50], "out": [50]}}, {"node": b, "size": [200, 100], "pins": {"in": [50], "out": [50]}}]}))
        .unwrap();
    let sc = d.scene().unwrap();
    let w = sc.wires.iter().find(|w| w.to_face == b).unwrap();
    // nine cubics: out, round, along below both faces, round, in
    assert_eq!(w.path.len(), 28, "{:?}", w.path);
    assert_eq!((w.path[0], *w.path.last().unwrap()), ([600.0, 50.0], [0.0, 90.0]));
    let low = w.path.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
    assert!(low >= 140.0 + 24.0 - 1e-3, "below both faces: {low}");
    // the hit test follows the routed shape: on its run below, a wire; where the S-curve was, nothing
    let snap = {
        d.publish_scene(sc.clone());
        std::sync::Arc::clone(&d.snapshot)
    };
    let h = snap.hit(300.0, low as f32, 4.0);
    assert_eq!(h.kind, plumcraft_editor_core::geometry::kind::WIRE);
    assert_eq!(snap.hit(300.0, 70.0, 4.0).kind, plumcraft_editor_core::geometry::kind::NONE);
    // and the paths of a face being dragged are routed too
    let mut out = Vec::new();
    let nid = snap.face_nid(&b).unwrap();
    snap.wire_paths(&[nid], 0.0, 0.0, &mut out);
    assert_eq!(f32::to_bits(out[2]), 28);
}
