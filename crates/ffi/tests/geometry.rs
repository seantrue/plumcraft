//! The geometry through the C ABI (N2 slice 3, `docs/shell-interface.md` 8.11 and section 9):
//! a shell reports what it measured, typed (`plum_geometry_set`) or as the `geometry.set`
//! command; the scene's wires follow the measured pins; the typed hot calls (`plum_hit_test`,
//! `plum_wire_target`, `plum_wire_paths`) answer from the snapshot without waiting for the core,
//! and agree with their JSON forms (`geometry.hit`, `geometry.wireTarget`, `geometry.wirePaths`);
//! and hostile arguments get an error code, never a crash.

mod common;

use std::time::{Duration, Instant};

use common::{Client, store_root};
use plumcraft_ffi::{PLUM_ERR_ARG, PLUM_ERR_CORE, PLUM_ERR_MISSING, PLUM_OK, PlumFace, plum_geometry_set, plum_hit_test, plum_wire_paths, plum_wire_target};
use serde_json::{Value, json};

fn add(c: &mut Client, kind: &str, x: f64) -> String {
    c.run("node.add", json!({"kind": kind, "pos": [x, 0]}))["id"].as_str().unwrap().to_owned()
}

fn face<'a>(scene: &'a Value, id: &str) -> &'a Value {
    scene["nodes"].as_array().unwrap().iter().find(|f| f["id"] == id).unwrap_or_else(|| panic!("no face {id}"))
}

/// Wait (polling, as a shell would) until `ok` holds of a hit test, or fail.
fn until(c: &mut Client, what: &str, mut ok: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !ok() {
        assert!(t0.elapsed() < Duration::from_secs(20), "never: {what}");
        c.poll();
        c.wait_wake(Duration::from_millis(20));
    }
}

#[test]
fn a_shell_measures_and_the_hot_calls_answer() {
    let mut c = Client::open(&json!({"store_root": store_root("geometry")}));
    let lit = add(&mut c, "text.literal", 0.0);
    let up = add(&mut c, "text.upper", 300.0);
    c.run("wire.connect", json!({"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}}));
    // before any scene is published there is nothing to hit
    assert_eq!(plum_hit_test(c.core, 0, 10.0, 10.0, 5.0).status, PLUM_ERR_MISSING);
    let scene = c.call("scene.get", json!({}))["result"].clone();
    let (lit_nid, up_nid) = (face(&scene, &lit)["nid"].as_u64().unwrap(), face(&scene, &up)["nid"].as_u64().unwrap());
    let up_text = face(&scene, &up)["inputs"].as_array().unwrap().iter().position(|r| r["key"] == "text").unwrap();
    // measured, typed: the literal 100 × 60 with its output at 30; Upper case 120 × 80
    let out_pins = [30.0f32];
    let in_pins: Vec<f32> = (0..face(&scene, &up)["inputs"].as_array().unwrap().len()).map(|k| 20.0 + 20.0 * k as f32).collect();
    let faces = [
        PlumFace {
            face: lit_nid,
            rev: 0,
            w: 100.0,
            h: 60.0,
            header: f32::NAN,
            inputs: std::ptr::null(),
            n_inputs: 0,
            outputs: out_pins.as_ptr(),
            n_outputs: 1,
            rows: std::ptr::null(),
            n_rows: 0,
        },
        PlumFace {
            face: up_nid,
            rev: 0,
            w: 120.0,
            h: 80.0,
            header: 18.0,
            inputs: in_pins.as_ptr(),
            n_inputs: in_pins.len(),
            outputs: out_pins.as_ptr(),
            n_outputs: 1,
            rows: std::ptr::null(),
            n_rows: 0,
        },
    ];
    // SAFETY: the faces and the pins they point at are live for the call.
    assert_eq!(unsafe { plum_geometry_set(c.core, 0, faces.as_ptr(), faces.len()) }, PLUM_OK);
    // the next snapshot has it: the literal's output pin is where it was measured
    let core = c.core;
    until(&mut c, "the measured pin is hit", || {
        let h = plum_hit_test(core, 0, 101.0, 31.0, 4.0);
        h.status == PLUM_OK && h.kind == 2 && h.node == lit_nid && (h.x, h.y) == (100.0, 30.0)
    });
    // a face, its header and body; nothing; the wire between them
    let h = plum_hit_test(c.core, 0, 350.0, 10.0, 0.0);
    assert_eq!((h.kind, h.part, h.node), (1, 2, up_nid), "the header: {h:?}");
    assert_eq!(plum_hit_test(c.core, 0, 350.0, 50.0, 0.0).part, 1, "the body");
    assert_eq!(plum_hit_test(c.core, 0, 5000.0, 5000.0, 10.0).kind, 0);
    let scene = c.call("scene.get", json!({}))["result"].clone();
    let w = &scene["wires"][0];
    let end = 20.0 + 20.0 * up_text as f64;
    assert_eq!((w["path"][0].clone(), w["path"][12].clone()), (json!([100.0, 30.0]), json!([300.0, end])), "the wire runs between the measured pins: {w}");
    let mid = (w["path"][6][0].as_f64().unwrap() as f32, w["path"][6][1].as_f64().unwrap() as f32);
    let h = plum_hit_test(c.core, 0, mid.0, mid.1 + 1.0, 4.0);
    assert_eq!((h.kind, h.wire), (3, w["nid"].as_u64().unwrap()), "{h:?}");
    // the JSON form says the same
    let j = c.call("geometry.hit", json!({"at": [101.0, 31.0], "slop": 4.0}))["result"].clone();
    assert_eq!((j["kind"].clone(), j["node"].clone(), j["side"].clone()), (json!("pin"), json!(lit), json!("out")), "{j}");
    // a wire dragged from the literal's output lands on Upper case's text input, which fits
    let t = plum_wire_target(c.core, 0, lit_nid, 2, 0, 302.0, end as f32 + 1.0, 6.0);
    assert_eq!((t.status, t.kind, t.node, t.pin, t.fits, t.x, t.y), (PLUM_OK, 2, up_nid, up_text as u32, 1, 300.0, end as f32), "{t:?}");
    let j = c.call("geometry.wireTarget", json!({"from": {"node": lit, "side": "out", "pin": 0}, "at": [302.0, end + 1.0], "slop": 6.0}))["result"].clone();
    assert_eq!((j["node"].clone(), j["pin"].clone(), j["fits"].clone()), (json!(up), json!(up_text), json!(1)), "{j}");
    // wire paths for the literal dragged 10 right: size first, then the floats
    let nids = [lit_nid];
    // SAFETY: nids live for the call; a null out asks only for the size.
    let need = unsafe { plum_wire_paths(c.core, 0, nids.as_ptr(), 1, 10.0, 0.0, std::ptr::null_mut(), 0) };
    assert_eq!(need, 3 + 2 * 13, "one wire of four cubics: its nid in two words, its count, its points");
    let mut out = vec![0.0f32; need];
    // SAFETY: out holds `need` floats.
    let wrote = unsafe { plum_wire_paths(c.core, 0, nids.as_ptr(), 1, 10.0, 0.0, out.as_mut_ptr(), out.len()) };
    assert_eq!(wrote, need);
    assert_eq!((u64::from(out[0].to_bits()) | (u64::from(out[1].to_bits()) << 32), out[2].to_bits()), (w["nid"].as_u64().unwrap(), 13));
    assert_eq!((out[3], out[4]), (110.0, 30.0), "its start moved with the literal");
    let mut short = vec![-1.0f32; need - 1];
    // SAFETY: short holds need - 1 floats: too few, so nothing is written.
    assert_eq!(unsafe { plum_wire_paths(c.core, 0, nids.as_ptr(), 1, 10.0, 0.0, short.as_mut_ptr(), short.len()) }, need);
    assert!(short.iter().all(|v| *v == -1.0), "nothing written past cap");
    let j = c.call("geometry.wirePaths", json!({"nodes": [lit], "offset": [10.0, 0.0]}))["result"].clone();
    assert_eq!(j["wires"][0]["path"][0], json!([110.0, 30.0]), "{j}");
    // the command form, and a fit from the measured sizes
    let r = c.run("geometry.set", json!({"faces": [{"node": lit, "size": [90, 60], "pins": {"out": [30]}}, {"node": "n999", "size": [1, 1]}]}));
    assert_eq!(r, json!({"applied": 1, "stale": [], "unknown": ["n999"]}));
    let fit = c.run("canvas.fit", json!({}));
    assert_eq!(fit["rect"], json!([-32.0, -32.0, 452.0, 112.0]), "{fit}");
    assert_eq!(c.call("scene.get", json!({}))["result"]["view"]["fit"]["seq"], fit["seq"]);
    c.close();
}

#[test]
fn hostile_hot_calls_get_an_error_not_a_crash() {
    let mut c = Client::open(&json!({"store_root": store_root("geometry-hostile")}));
    let lit = add(&mut c, "text.literal", 0.0);
    c.call("scene.get", json!({}));
    // a core that isn't there, a document that isn't, a side that isn't
    assert_eq!(plum_hit_test(0, 0, 1.0, 1.0, 1.0).status, PLUM_ERR_CORE);
    assert_eq!(plum_hit_test(c.core, 999, 1.0, 1.0, 1.0).status, PLUM_ERR_MISSING);
    let core = c.core;
    until(&mut c, "the first scene's snapshot", || plum_hit_test(core, 0, 10.0, 10.0, 0.0).kind == 1);
    for side in [0, 3, u32::MAX] {
        assert_eq!(plum_wire_target(c.core, 0, 1, side, 0, 0.0, 0.0, 1.0).status, PLUM_ERR_ARG);
    }
    // points and slops that aren't numbers, or are past reason
    for (x, y, slop) in [(f32::NAN, 0.0, 1.0), (f32::INFINITY, 1.0, f32::NAN), (1e38, -1e38, 1e38), (0.0, 0.0, -1.0)] {
        let h = plum_hit_test(c.core, 0, x, y, slop);
        assert_eq!(h.status, PLUM_OK);
        assert!(h.x.is_finite() && h.y.is_finite(), "{h:?}");
        let t = plum_wire_target(c.core, 0, u64::MAX, 2, u32::MAX, x, y, slop);
        assert_eq!((t.status, t.kind), (PLUM_OK, 0));
    }
    // wire paths: null and absurd arguments
    // SAFETY: null pointers and zero counts are allowed; the counts below are refused before any read.
    unsafe {
        assert_eq!(plum_wire_paths(c.core, 0, std::ptr::null(), 5, 0.0, 0.0, std::ptr::null_mut(), 0), 0);
        assert_eq!(plum_wire_paths(c.core, 0, std::ptr::null(), 0, f32::NAN, f32::INFINITY, std::ptr::null_mut(), 0), 0);
        assert_eq!(plum_wire_paths(0, 0, std::ptr::null(), 0, 0.0, 0.0, std::ptr::null_mut(), 0), 0);
        let one = [1u64];
        assert_eq!(plum_wire_paths(c.core, 0, one.as_ptr(), usize::MAX, 0.0, 0.0, std::ptr::null_mut(), 0), 0, "a count past reason reads nothing");
    }
    // geometry: a null list with a count, counts past reason, a closed core
    // SAFETY: as above: refused before any read.
    unsafe {
        assert_eq!(plum_geometry_set(c.core, 0, std::ptr::null(), 3), PLUM_ERR_ARG);
        assert_eq!(plum_geometry_set(c.core, 0, std::ptr::null(), 0), PLUM_OK);
        let f = PlumFace {
            face: 1,
            rev: 0,
            w: f32::NAN,
            h: -5.0,
            header: f32::INFINITY,
            inputs: std::ptr::null(),
            n_inputs: 1 << 40,
            outputs: std::ptr::null(),
            n_outputs: 0,
            rows: std::ptr::null(),
            n_rows: 0,
        };
        assert_eq!(plum_geometry_set(c.core, 0, &f, 1), PLUM_ERR_ARG);
        let ok = PlumFace { n_inputs: 0, ..f };
        assert_eq!(plum_geometry_set(c.core, 0, &ok, 1), PLUM_OK, "NaN and negative sizes are tamed, not refused");
        assert_eq!(plum_geometry_set(c.core, 0, &ok, usize::MAX), PLUM_ERR_ARG);
        assert_eq!(plum_geometry_set(0, 0, &ok, 1), PLUM_ERR_CORE);
    }
    // the JSON forms refuse what isn't their shape
    for (m, p) in [
        ("geometry.hit", json!({})),
        ("geometry.hit", json!({"at": [1]})),
        ("geometry.hit", json!({"at": ["x", 1]})),
        ("geometry.hit", json!({"at": [1, 1], "slop": "far"})),
        ("geometry.hit", json!({"at": [1, 1], "x": 1})),
        ("geometry.wireTarget", json!({"at": [1, 1]})),
        ("geometry.wireTarget", json!({"at": [1, 1], "from": {"node": lit, "side": "up", "pin": 0}})),
        ("geometry.wireTarget", json!({"at": [1, 1], "from": {"node": "n999", "side": "out", "pin": 0}})),
        ("geometry.wirePaths", json!({"nodes": "n1"})),
        ("geometry.wirePaths", json!({"nodes": [], "offset": [1]})),
    ] {
        let r = c.call(m, p.clone());
        assert_eq!(r["ok"], false, "{m} {p}: {r}");
    }
    let r = c.call("command.run", json!({"command": "geometry.set", "params": {"faces": [{"node": lit, "size": "big"}]}}));
    assert_eq!(r["ok"], false, "{r}");
    let close = c.core;
    c.close();
    assert_eq!(plum_hit_test(close, 0, 0.0, 0.0, 1.0).status, PLUM_ERR_CORE, "after close");
}
