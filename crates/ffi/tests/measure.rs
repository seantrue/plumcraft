//! The N0(b) measurements from Rust, through the C ABI as a C caller would call it. Ignored by
//! default; run in release:
//!
//! ```sh
//! CARGO_PROFILE_RELEASE_DEBUG=0 cargo test --release -p plumcraft-ffi --test measure -- --ignored --nocapture --test-threads 1
//! ```
//!
//! Each prints one `MEASURE {json}` line.

mod common;

use std::time::{Duration, Instant};

use common::{Client, store_root};
use plumcraft_ffi::{PLUM_OK, plum_blob, plum_release};
use serde_json::{Value, json};

fn stats(mut us: Vec<f64>) -> Value {
    us.sort_by(f64::total_cmp);
    let at = |q: f64| us[((us.len() as f64 - 1.0) * q).round() as usize];
    json!({"n": us.len(), "median_us": at(0.5), "p99_us": at(0.99), "min_us": us[0], "max_us": us[us.len() - 1]})
}

fn report(name: &str, v: Value) {
    println!("MEASURE {}", json!({"name": name, "from": "rust", "v": v}));
}

/// Request to reply, one at a time: wait for the wake, poll, find the reply.
fn latency(c: &mut Client, req: &Value, n: usize) -> Value {
    for _ in 0..50 {
        let id = c.send(req);
        c.wait_reply(id, Duration::from_secs(10));
    }
    let mut us = Vec::with_capacity(n);
    for _ in 0..n {
        let t0 = Instant::now();
        let id = c.send(req);
        let r = c.wait_reply(id, Duration::from_secs(10));
        us.push(t0.elapsed().as_secs_f64() * 1e6);
        assert_eq!(r["ok"], true, "{r}");
    }
    stats(us)
}

#[test]
#[ignore = "measurement"]
fn request_latency() {
    let mut c = Client::open(&json!({"store_root": store_root("m-latency"), "demo": true}));
    report("latency.ping", latency(&mut c, &json!({"method": "ping"}), 5000));
    report(
        "latency.command.node.move",
        latency(&mut c, &json!({"method": "command.run", "params": {"command": "node.move", "params": {"id": "shout", "pos": [10, 10]}}}), 5000),
    );
    c.close();
}

/// `graph.get` end to end at a scene's size: request to reply parsed (serde_json, as a shell's
/// decoder would), the bytes, and the parts.
fn scene(c: &mut Client, nodes: u64, wires: u64) -> Value {
    let built = Instant::now();
    let r = c.call("spike.scene", json!({"nodes": nodes, "wires": wires}));
    let build_ms = built.elapsed().as_secs_f64() * 1e3;
    assert_eq!(r["ok"], true, "{r}");
    let mut e2e = Vec::new();
    let mut bytes = 0;
    let mut decode = Vec::new();
    let mut serialize = Vec::new();
    for i in 0..30 {
        let t0 = Instant::now();
        let id = c.send(&json!({"method": "graph.get"}));
        // poll by hand to time the decode apart
        let (raw, v, t1) = loop {
            let buf = common::take(plumcraft_ffi::plum_poll(c.core));
            if !buf.is_empty() {
                let t1 = Instant::now();
                let v: Value = serde_json::from_slice(&buf).unwrap();
                if v["replies"].as_array().is_some_and(|r| !r.is_empty()) {
                    break (buf, v, t1);
                }
            }
            c.wait_wake(Duration::from_millis(200));
        };
        let reply = &v["replies"][0];
        assert_eq!(reply["id"], id);
        let t2 = Instant::now();
        if i >= 5 {
            e2e.push((t2 - t0).as_secs_f64() * 1e6);
            decode.push((t2 - t1).as_secs_f64() * 1e6);
        }
        bytes = raw.len();
        // what the core spends serialising the graph alone
        let g: plumcraft_graph::Graph = serde_json::from_value(reply["result"].clone()).unwrap();
        let t3 = Instant::now();
        let text = serde_json::to_vec(&g).unwrap();
        if i >= 5 {
            serialize.push(t3.elapsed().as_secs_f64() * 1e6);
        }
        assert!(!text.is_empty());
    }
    // an edit on a graph this size: one command, request to reply
    let edit = latency(c, &json!({"method": "command.run", "params": {"command": "node.add", "params": {"kind": "text.upper", "pos": [0, -200]}}}), 200);
    json!({"nodes": nodes, "wires": wires, "build_ms": build_ms, "bytes": bytes, "end_to_end": stats(e2e), "decode_serde_value": stats(decode), "serialize_graph": stats(serialize), "node_add": edit})
}

#[test]
#[ignore = "measurement"]
fn graph_get_json_cost() {
    let mut c = Client::open(&json!({"store_root": store_root("m-scene")}));
    report("scene.500", scene(&mut c, 500, 800));
    report("scene.2000", scene(&mut c, 2000, 3200));
    c.close();
}

fn rss_kb() -> u64 {
    let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0)
}

/// Borrow, touch every page (one byte per 16 KiB), give back; RSS around each.
fn borrow(c: &Client, handle: &str) -> Value {
    let rss0 = rss_kb();
    let t0 = Instant::now();
    // SAFETY: the handle is live for the call.
    let b = unsafe { plum_blob(c.core, handle.as_ptr(), handle.len()) };
    let obtain_us = t0.elapsed().as_secs_f64() * 1e6;
    assert_eq!(b.status, PLUM_OK);
    let rss1 = rss_kb();
    // SAFETY: lent until released below.
    let bytes = unsafe { std::slice::from_raw_parts(b.ptr, b.len) };
    let t1 = Instant::now();
    let mut sum = 0u64;
    for i in (0..bytes.len()).step_by(16 << 10) {
        sum = sum.wrapping_add(u64::from(bytes[i]));
    }
    let touch_us = t1.elapsed().as_secs_f64() * 1e6;
    let rss2 = rss_kb();
    let t2 = Instant::now();
    // SAFETY: as above.
    let again = unsafe { plum_blob(c.core, handle.as_ptr(), handle.len()) };
    let again_us = t2.elapsed().as_secs_f64() * 1e6;
    assert_eq!(again.ptr, b.ptr);
    assert_eq!(plum_release(again.token), PLUM_OK);
    assert_eq!(plum_release(b.token), PLUM_OK);
    json!({"len": b.len, "obtain_us": obtain_us, "second_borrow_us": again_us, "same_pointer": again.ptr == b.ptr, "touch_us": touch_us, "rss_kb": [rss0, rss1, rss2], "sum": sum})
}

#[test]
#[ignore = "measurement"]
fn blob_64mb() {
    let mut c = Client::open(&json!({"store_root": store_root("m-blob")}));
    // pseudo-random bytes, published as they are: mapped
    let p = c.call("spike.publish", json!({"bytes": 64 << 20, "seed": 11}));
    let h = p["result"]["handle"].as_str().unwrap().to_owned();
    report("blob.64MiB.mapped", borrow(&c, &h));
    // the text engine's own 64 MB output (compressible, so stored deflated): resolved through the
    // connection, inflated and checked against its handle
    let (rep, _) = common::build_graph(&mut c);
    c.poll_bytes.clear();
    c.run("run.start", json!({"watch": [{"node": rep, "port": "text"}]}));
    let done = c.call("spike.waitRun", json!({"timeout_ms": 60_000}));
    report("run.64MB-text.poll-bytes", json!({"polls": c.poll_bytes.len(), "total": c.poll_bytes.iter().sum::<usize>(), "largest": c.poll_bytes.iter().max()}));
    let h = done["result"]["watch"][0]["full"]["handle"].as_str().unwrap().to_owned();
    report("blob.64MB.engine-output.resolved", borrow(&c, &h));
    c.close();
}

#[test]
#[ignore = "measurement"]
fn wakes_and_events_during_a_run() {
    for n in [100u64, 500] {
        let mut c = Client::open(&json!({"store_root": store_root(&format!("m-run-{n}"))}));
        // a fan of Upper case nodes from one literal: one wire each, nothing grows
        c.call("spike.scene", json!({"nodes": n, "wires": n - 1}));
        c.run("node.set", json!({"node": "n1", "port": "text", "value": "a corgi"}));
        let (w0, p0) = (c.signal.count.load(std::sync::atomic::Ordering::Relaxed), c.polls);
        c.events.clear();
        c.events_per_poll.clear();
        let t0 = Instant::now();
        c.run("run.start", json!({}));
        let done = c.call("spike.waitRun", json!({"timeout_ms": 120_000}));
        let secs = t0.elapsed().as_secs_f64();
        assert_eq!(done["ok"], true);
        let wakes = c.signal.count.load(std::sync::atomic::Ordering::Relaxed) - w0;
        let polls = c.polls - p0;
        let per: Vec<f64> = c.events_per_poll.iter().map(|&e| e as f64).collect();
        let events: usize = c.events_per_poll.iter().sum();
        let core_stats = c.call("spike.stats", json!({}))["result"].clone();
        report(
            &format!("run.{n}"),
            json!({"nodes": n, "seconds": secs, "events": events, "wakes": wakes, "polls": polls, "wakes_per_s": wakes as f64 / secs,
                   "events_per_poll": {"mean": events as f64 / per.len().max(1) as f64, "max": per.iter().copied().fold(0.0, f64::max)}, "core": core_stats}),
        );
        c.close();
    }
}
