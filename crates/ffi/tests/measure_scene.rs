//! N2 slice 2's measurements of the scene through the C ABI, as a C caller would call it: the
//! whole scene of a 500-node, 800-wire graph (`scene.get`, request to reply decoded), and after
//! one node's run state changes, `scene.get {since}`. Ignored by default; run in release:
//!
//! ```sh
//! CARGO_PROFILE_RELEASE_DEBUG=0 cargo test --release -p plumcraft-ffi --test measure_scene -- --ignored --nocapture --test-threads 1
//! ```
//!
//! Each prints one `MEASURE {json}` line.

mod common;

use std::time::{Duration, Instant};

use common::{Client, store_root};
use serde_json::{Value, json};

fn stats(mut us: Vec<f64>) -> Value {
    us.sort_by(f64::total_cmp);
    let at = |q: f64| us[((us.len() as f64 - 1.0) * q).round() as usize];
    json!({"n": us.len(), "median_us": at(0.5), "p99_us": at(0.99), "min_us": us[0], "max_us": us[us.len() - 1]})
}

/// One request, polled by hand: (the reply's result, the poll's bytes, request to decoded reply).
fn timed(c: &mut Client, req: &Value) -> (Value, usize, f64) {
    let t0 = Instant::now();
    let id = c.send(req);
    loop {
        let buf = common::take(plumcraft_ffi::plum_poll(c.core));
        if !buf.is_empty() {
            let v: Value = serde_json::from_slice(&buf).unwrap();
            if let Some(r) = v["replies"].as_array().and_then(|r| r.iter().find(|r| r["id"] == id)) {
                assert_eq!(r["ok"], true, "{r}");
                return (r["result"].clone(), buf.len(), t0.elapsed().as_secs_f64() * 1e6);
            }
        }
        c.wait_wake(Duration::from_millis(200));
    }
}

#[test]
#[ignore = "measurement"]
fn scene_json_cost() {
    for (nodes, wires) in [(500u64, 800u64), (2000, 3200)] {
        let mut c = Client::open(&json!({"store_root": store_root("m-scene2")}));
        let r = c.call("spike.scene", json!({"nodes": nodes, "wires": wires}));
        assert_eq!(r["ok"], true, "{r}");
        let req = json!({"method": "scene.get", "params": {}});
        let mut full = Vec::new();
        let mut bytes = 0;
        let mut first = 0.0;
        for i in 0..25 {
            let (v, b, us) = timed(&mut c, &req);
            assert_eq!(v["nodes"].as_array().unwrap().len() as u64, nodes);
            if i == 0 {
                first = us;
            } else {
                full.push(us);
            }
            bytes = b;
        }
        // one node's run state changes (a run of the first node alone), then what changed
        let generation = timed(&mut c, &req).0["gen"].as_u64().unwrap();
        let first_node = c.call("graph.get", json!({}))["result"]["nodes"].as_object().unwrap().keys().next().unwrap().clone();
        c.run("run.watch", json!({"node": first_node}));
        let done = c.call("ui.waitRun", json!({"timeout": 25}));
        assert_eq!(done["ok"], true, "{done}");
        let (d, dbytes, dus) = timed(&mut c, &json!({"method": "scene.get", "params": {"since": generation}}));
        let changed = d["upsert"]["nodes"].as_array().unwrap().len();
        // and again, nothing changed since: the cost of looking
        let g = d["gen"].clone();
        let mut idle = Vec::new();
        for _ in 0..25 {
            idle.push(timed(&mut c, &json!({"method": "scene.get", "params": {"since": g}})).2);
        }
        println!(
            "MEASURE {}",
            json!({"name": format!("scene.ffi.{nodes}"), "nodes": nodes, "wires": wires, "full_bytes": bytes, "first_full_us": first, "full": stats(full),
                   "after_one_run": {"faces_changed": changed, "bytes": dbytes, "us": dus}, "nothing_changed": stats(idle)})
        );
        c.close();
    }
}

/// N2 slice 3: the typed hot calls across the boundary at 500 and 2,000 nodes, every face
/// measured with `plum_geometry_set`: `plum_hit_test`, `plum_wire_target`, and
/// `plum_wire_paths` for 20 faces dragged; and a one-node delta with the geometry in the scene.
#[test]
#[ignore = "measurement"]
fn hot_calls_cost() {
    for (nodes, wires) in [(500u64, 800u64), (2000, 3200)] {
        let mut c = Client::open(&json!({"store_root": store_root("m-hot")}));
        assert_eq!(c.call("spike.scene", json!({"nodes": nodes, "wires": wires}))["ok"], true);
        let scene = c.call("scene.get", json!({}))["result"].clone();
        let faces: Vec<(u64, usize, usize)> = scene["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| (f["nid"].as_u64().unwrap(), f["inputs"].as_array().unwrap().len(), f["outputs"].as_array().unwrap().len()))
            .collect();
        let pins: Vec<f32> = (0..8).map(|k| 40.0 + 22.0 * k as f32).collect();
        let list: Vec<plumcraft_ffi::PlumFace> = faces
            .iter()
            .map(|&(face, i, o)| plumcraft_ffi::PlumFace {
                face,
                rev: 0,
                w: 200.0,
                h: 120.0,
                header: 28.0,
                inputs: pins.as_ptr(),
                n_inputs: i.min(8),
                outputs: pins.as_ptr(),
                n_outputs: o.min(8),
                rows: std::ptr::null(),
                n_rows: 0,
            })
            .collect();
        let t0 = Instant::now();
        // SAFETY: the faces and their pins are live for the call.
        assert_eq!(unsafe { plumcraft_ffi::plum_geometry_set(c.core, 0, list.as_ptr(), list.len()) }, plumcraft_ffi::PLUM_OK);
        let set_us = t0.elapsed().as_secs_f64() * 1e6;
        // wait for the snapshot with the geometry: the first face's output pin where it was
        // measured (x 200; unmeasured it is at 220)
        let first = faces[0].0;
        let t0 = Instant::now();
        loop {
            let h = plumcraft_ffi::plum_hit_test(c.core, 0, 200.0, 40.0, 1.0);
            if h.status == plumcraft_ffi::PLUM_OK && h.node == first && h.kind == 2 && h.x == 200.0 {
                break;
            }
            assert!(t0.elapsed() < Duration::from_secs(20), "no snapshot with the geometry");
            c.poll();
            c.wait_wake(Duration::from_millis(5));
        }
        let applied_us = t0.elapsed().as_secs_f64() * 1e6;
        let nids: Vec<u64> = faces.iter().map(|f| f.0).collect();
        let (mut hit, mut target, mut paths) = (vec![], vec![], vec![]);
        let mut out = vec![0.0f32; 1 << 16];
        for k in 0..400usize {
            let (x, y) = ((k % 25) as f32 * 260.0 + 100.0, (k / 25 % 80) as f32 * 160.0 + 62.0);
            let t0 = Instant::now();
            std::hint::black_box(plumcraft_ffi::plum_hit_test(c.core, 0, x, y, 12.0));
            hit.push(t0.elapsed().as_secs_f64() * 1e6);
            let t0 = Instant::now();
            std::hint::black_box(plumcraft_ffi::plum_wire_target(c.core, 0, nids[k % nids.len()], 2, 0, x, y, 24.0));
            target.push(t0.elapsed().as_secs_f64() * 1e6);
            let picked: Vec<u64> = (0..20).map(|j| nids[(k * 20 + j) % nids.len()]).collect();
            let t0 = Instant::now();
            // SAFETY: picked and out are live for the call; out's length is its cap.
            std::hint::black_box(unsafe { plumcraft_ffi::plum_wire_paths(c.core, 0, picked.as_ptr(), picked.len(), 3.0, 4.0, out.as_mut_ptr(), out.len()) });
            paths.push(t0.elapsed().as_secs_f64() * 1e6);
        }
        // a one-node delta with the geometry in the scene
        let generation = timed(&mut c, &json!({"method": "scene.get", "params": {}})).0["gen"].as_u64().unwrap();
        let first_node = c.call("graph.get", json!({}))["result"]["nodes"].as_object().unwrap().keys().next().unwrap().clone();
        c.run("run.watch", json!({"node": first_node}));
        assert_eq!(c.call("ui.waitRun", json!({"timeout": 25}))["ok"], true);
        let (d, dbytes, dus) = timed(&mut c, &json!({"method": "scene.get", "params": {"since": generation}}));
        println!(
            "MEASURE {}",
            json!({"name": format!("hot.ffi.{nodes}"), "geometry_set_call_us": set_us, "geometry_to_snapshot_us": applied_us,
                   "hit_test": stats(hit), "wire_target": stats(target), "wire_paths_20_faces": stats(paths),
                   "delta_after_one_run": {"faces": d["upsert"]["nodes"].as_array().unwrap().len(), "bytes": dbytes, "us": dus}})
        );
        c.close();
    }
}
