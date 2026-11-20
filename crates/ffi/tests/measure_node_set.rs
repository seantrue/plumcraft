//! N3 slice 5's measurement of a one-node change through the C ABI, as the Mac app makes it: at
//! 500 and 2,000 nodes (`spike.scene`), on the text engine alone and on text with array beside it
//! (`engines.also`), `command.run node.set` on the scene's one text.literal face; request to
//! reply, then to the feed's `changed` scene, then `scene.get {since}` request to reply. Ignored
//! by default; run in release:
//!
//! ```sh
//! CARGO_PROFILE_RELEASE_DEBUG=0 cargo test --release -p plumcraft-ffi --test measure_node_set -- --ignored --nocapture --test-threads 1
//! ```
//!
//! Each prints one `MEASURE {json}` line. `PLUMCRAFT_MEASURE_LOOP=<seconds>` measures only the
//! 2,000-node case (text alone; text and array with `PLUMCRAFT_MEASURE_ALSO=1`), its paced rounds
//! for about that long, for a sampling profiler to attach to (`sample <pid>`);
//! `PLUMCRAFT_MEASURE_LOOP_TIGHT=1` instead sets and fetches the delta back to back first.

mod common;

use std::time::{Duration, Instant};

use common::{Client, store_root};
use serde_json::{Value, json};

fn stats(mut us: Vec<f64>) -> Value {
    us.sort_by(f64::total_cmp);
    let at = |q: f64| us[((us.len() as f64 - 1.0) * q).round() as usize];
    json!({"n": us.len(), "median_us": at(0.5), "p90_us": at(0.9), "min_us": us[0], "max_us": us[us.len() - 1]})
}

/// One request, polled by hand: (the reply's result, request to decoded reply in µs, whether a
/// `changed` scene came in the same poll).
fn timed(c: &mut Client, req: &Value) -> (Value, f64, bool) {
    let t0 = Instant::now();
    let id = c.send(req);
    loop {
        let buf = common::take(plumcraft_ffi::plum_poll(c.core));
        if !buf.is_empty() {
            let v: Value = serde_json::from_slice(&buf).unwrap();
            if let Some(r) = v["replies"].as_array().and_then(|r| r.iter().find(|r| r["id"] == id)) {
                assert_eq!(r["ok"], true, "{r}");
                let scene = scene_changed(&v);
                return (r["result"].clone(), t0.elapsed().as_secs_f64() * 1e6, scene);
            }
        }
        c.wait_wake(Duration::from_millis(200));
    }
}

fn scene_changed(v: &Value) -> bool {
    v["changed"].as_array().is_some_and(|c| c.iter().any(|c| c.as_str().map_or(c["domain"] == "scene", |s| s.contains("\"scene\""))))
}

/// Polls until the feed says the scene changed: µs waited.
fn wait_scene(c: &mut Client) -> f64 {
    let t0 = Instant::now();
    loop {
        let buf = common::take(plumcraft_ffi::plum_poll(c.core));
        if !buf.is_empty() && scene_changed(&serde_json::from_slice(&buf).unwrap()) {
            return t0.elapsed().as_secs_f64() * 1e6;
        }
        assert!(t0.elapsed() < Duration::from_secs(5), "no changed scene");
        c.wait_wake(Duration::from_millis(5));
    }
}

fn one_case(nodes: u64, wires: u64, also: bool, rounds: usize, loop_for: Option<Duration>) -> Value {
    let engines = if also { json!({"default": "text", "also": ["array"]}) } else { json!({"default": "text"}) };
    let mut c = Client::open(&json!({"store_root": store_root("m-nodeset"), "engines": engines}));
    // the document's engines connected (a document request is held until they are), apart from
    // the edit
    let t0 = Instant::now();
    assert_eq!(c.call("graph.get", json!({}))["ok"], true);
    let ready_ms = t0.elapsed().as_secs_f64() * 1e3;
    let t0 = Instant::now();
    let r = c.call("spike.scene", json!({"nodes": nodes, "wires": wires}));
    let scene_ms = t0.elapsed().as_secs_f64() * 1e3;
    assert_eq!(r["ok"], true, "{r}");
    let kinds = c.call("graph.get", json!({}))["result"]["nodes"].clone();
    let lit = kinds.as_object().unwrap().iter().find(|(_, n)| n["kind"] == "text.literal").map(|(k, _)| k.clone()).unwrap();
    // the scene is watched, as the app watches it
    let mut generation = timed(&mut c, &json!({"method": "scene.get", "params": {}})).0["gen"].as_u64().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    while c.poll() {}
    let set =
        |k: usize| json!({"method": "command.run", "params": {"command": "node.set", "params": {"node": lit, "port": "text", "value": format!("hello {k}")}}});
    if let Some(d) = loop_for {
        let t0 = Instant::now();
        let mut k = 0;
        while t0.elapsed() < d {
            timed(&mut c, &set(k));
            k += 1;
            let g = timed(&mut c, &json!({"method": "scene.get", "params": {"since": generation}})).0["gen"].as_u64().unwrap();
            generation = g;
        }
    }
    let (mut reply, mut feed, mut delta, mut total, mut idle_reply) = (vec![], vec![], vec![], vec![], vec![]);
    let mut with_reply = 0;
    let mut upserts = 0;
    for k in 0..rounds {
        // the core quiet for longer than its scene throttle, as between a person's edits
        std::thread::sleep(Duration::from_millis(40));
        while c.poll() {}
        let t0 = Instant::now();
        let (_, us, scene_too) = timed(&mut c, &set(k));
        reply.push(us);
        let fed = if scene_too {
            with_reply += 1;
            0.0
        } else {
            wait_scene(&mut c)
        };
        feed.push(fed);
        let (d, dus, _) = timed(&mut c, &json!({"method": "scene.get", "params": {"since": generation}}));
        delta.push(dus);
        total.push(t0.elapsed().as_secs_f64() * 1e6);
        upserts = d["upsert"]["nodes"].as_array().map_or(0, Vec::len);
        generation = d["gen"].as_u64().unwrap();
        // a request that changes nothing, for the turn's fixed cost at this size
        std::thread::sleep(Duration::from_millis(40));
        while c.poll() {}
        idle_reply.push(timed(&mut c, &json!({"method": "ping"})).1);
    }
    c.close();
    json!({"name": format!("node_set.ffi.{nodes}{}", if also { ".text+array" } else { ".text" }), "ready_ms": ready_ms, "spike_scene_ms": scene_ms,
           "node_set_reply": stats(reply), "reply_to_feed": stats(feed), "feed_in_reply_poll": with_reply,
           "scene_since_reply": stats(delta), "faces_upserted": upserts, "set_to_delta_total": stats(total), "ping_reply": stats(idle_reply)})
}

#[test]
#[ignore = "measurement"]
fn node_set_cost() {
    let loop_for = std::env::var("PLUMCRAFT_MEASURE_LOOP").ok().and_then(|s| s.parse::<u64>().ok()).map(Duration::from_secs);
    if let Some(d) = loop_for {
        let also = std::env::var("PLUMCRAFT_MEASURE_ALSO").is_ok_and(|v| v == "1");
        let tight = std::env::var("PLUMCRAFT_MEASURE_LOOP_TIGHT").is_ok_and(|v| v == "1");
        let v = if tight { one_case(2000, 3200, also, 5, Some(d)) } else { one_case(2000, 3200, also, (d.as_secs() * 9) as usize, None) };
        println!("MEASURE {v}");
        return;
    }
    for also in [false, true] {
        for (nodes, wires) in [(500u64, 800u64), (2000, 3200)] {
            println!("MEASURE {}", one_case(nodes, wires, also, 30, None));
        }
    }
}
