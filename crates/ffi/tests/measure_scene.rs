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
