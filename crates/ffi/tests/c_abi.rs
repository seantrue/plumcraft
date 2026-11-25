//! The C ABI, called as a C caller would: open, list, build, run, status events, a blob, and
//! everything hostile a shell could send.

mod common;

use std::time::Duration;

use common::{Client, build_graph, store_root, take};
use plumcraft_ffi::*;
use serde_json::{Value, json};

fn kinds(c: &Client) -> Vec<String> {
    c.events.iter().filter_map(|e| e["kind"].as_str().map(str::to_owned)).collect()
}

#[test]
fn a_shell_lists_builds_runs_and_borrows_a_blob() {
    let mut c = Client::open(&json!({"store_root": store_root("flow")}));
    let list = c.call("command.list", json!({}));
    assert!(list["result"].as_array().unwrap().iter().any(|r| r["id"] == "node.add" && r["enabled"] == true), "{list}");

    let (rep, up) = build_graph(&mut c);
    let graph = c.call("graph.get", json!({}));
    assert_eq!(graph["result"]["nodes"].as_object().unwrap().len(), 4);
    assert_eq!(graph["result"]["wires"].as_array().unwrap().len(), 3);

    let started = c.run("run.start", json!({"watch": [{"node": rep, "port": "text"}, {"node": up, "port": "text"}]}));
    assert!(started["run_id"].is_string(), "{started}");
    let done = c.call("spike.waitRun", json!({"timeout_ms": 30_000}));
    assert_eq!(done["ok"], true, "{done}");
    for id in [&rep, &up] {
        assert_eq!(done["result"]["nodes"][id]["state"], "done", "{done}");
    }
    // node status arrived through plum_poll, and the run's reply
    let k = kinds(&c);
    assert!(k.iter().any(|k| k == "node.status"), "{k:?}");
    assert!(k.iter().any(|k| k == "run.reply"), "{k:?}");
    assert!(c.signal.count.load(std::sync::atomic::Ordering::Relaxed) > 0, "never woken");

    // the 64 MB value is a store blob: borrow it, twice, and see the same bytes
    let handle = done["result"]["watch"][0]["full"]["handle"]
        .as_str()
        .unwrap_or_else(|| panic!("the repeat's value has no handle: {}", done["result"]["watch"][0]))
        .to_owned();
    // SAFETY: the handle is live for the call.
    let a = unsafe { plum_blob(c.core, handle.as_ptr(), handle.len()) };
    assert_eq!(a.status, PLUM_OK);
    assert_eq!(a.len, 64_000_000);
    // SAFETY: lent until released below.
    let bytes = unsafe { std::slice::from_raw_parts(a.ptr, a.len) };
    assert!(bytes.starts_with(b"The quick brown fox"));
    // SAFETY: as above.
    let b = unsafe { plum_blob(c.core, handle.as_ptr(), handle.len()) };
    assert_eq!(b.ptr, a.ptr, "a second borrow shares the first's bytes");
    assert_ne!(a.token, b.token);
    assert_eq!(plum_release(a.token), PLUM_OK);
    assert_eq!(plum_release(a.token), PLUM_ERR_MISSING, "released twice");

    // a blob published raw is mapped, not read
    let p = c.call("spike.publish", json!({"bytes": 1 << 20, "seed": 7}));
    let h = p["result"]["handle"].as_str().unwrap().to_owned();
    // SAFETY: as above.
    let m = unsafe { plum_blob(c.core, h.as_ptr(), h.len()) };
    assert_eq!((m.status, m.len), (PLUM_OK, 1 << 20));

    c.close();
    // lent bytes outlive the core
    // SAFETY: b is still lent: valid until released.
    let still = unsafe { std::slice::from_raw_parts(b.ptr, b.len) };
    assert!(still.ends_with(b"0123456\n"));
    assert_eq!(plum_release(b.token), PLUM_OK);
    assert_eq!(plum_release(m.token), PLUM_OK);
}

#[test]
fn the_demo_graph_runs_with_its_failing_branch() {
    let mut c = Client::open(&json!({"store_root": store_root("demo"), "demo": true}));
    let r = c.call("run", json!({}));
    assert_eq!(r["ok"], true, "{r}");
    let done = c.call("spike.waitRun", json!({"timeout_ms": 30_000}));
    let nodes = &done["result"]["nodes"];
    assert_eq!(nodes["shout"]["state"], "done", "{done}");
    assert_eq!(nodes["broken"]["state"], "error", "{done}");
    let states: Vec<&str> = c.events.iter().filter(|e| e["kind"] == "node.status").filter_map(|e| e["body"]["state"].as_str()).collect();
    assert!(states.contains(&"started") && states.contains(&"done"), "{states:?}");
    c.close();
}

#[test]
fn hostile_input_never_panics() {
    // null and empty configs open with defaults; a bad config is refused with a reason
    // SAFETY: null pointers are allowed and never read.
    let core = unsafe { plum_open(std::ptr::null(), 0, None, std::ptr::null_mut()) };
    assert_ne!(core, 0);
    assert_eq!(plum_close(core), PLUM_OK);
    for bad in [&b"not json"[..], b"[1,2]", b"\xff\xfe", br#"{"engine": "vectorcraft"}"#] {
        // SAFETY: live for the call.
        let core = unsafe { plum_open(bad.as_ptr(), bad.len(), None, std::ptr::null_mut()) };
        assert_eq!(core, 0, "{:?}", String::from_utf8_lossy(bad));
        assert!(!take(plum_last_error()).is_empty());
    }

    let mut c = Client::open(&json!({"store_root": store_root("hostile")}));
    // null, empty, non-UTF-8, non-JSON, no method, unknown method, bad params: each answered
    let bad: [&[u8]; 8] = [
        b"",
        b"\xff\xfe\xfd",
        b"{not json",
        b"[]",
        br#"{"id": 1}"#,
        br#"{"method": "no.such", "params": {}}"#,
        br#"{"method": "command.run", "params": {"command": 7}}"#,
        br#"{"method": "command.run", "params": {"command": "node.add", "params": {"kind": "\u0000"}}}"#,
    ];
    for b in bad {
        // SAFETY: live for the call.
        let id = unsafe { plum_request(c.core, b.as_ptr(), b.len()) };
        assert_ne!(id, 0);
        let r = c.wait_reply(id, Duration::from_secs(10));
        assert_eq!(r["ok"], false, "{:?} -> {r}", String::from_utf8_lossy(b));
        assert!(r["error"].is_string());
    }
    // SAFETY: a null pointer with a length is read as empty.
    let id = unsafe { plum_request(c.core, std::ptr::null(), 99) };
    assert_eq!(c.wait_reply(id, Duration::from_secs(10))["ok"], false);
    // the caller's id comes back as the tag
    let id = c.send(&json!({"id": "mine", "method": "ping"}));
    let r = c.wait_reply(id, Duration::from_secs(10));
    assert_eq!((r["ok"].clone(), r["tag"].clone()), (json!(true), json!("mine")));

    // blobs: nonsense handles, null, non-UTF-8, unknown core
    for h in [&b""[..], b"\xff", b"md5:raw:nothex", b"md5:raw:00000000000000000000000000000000", b"b3:raw:0000"] {
        // SAFETY: live for the call.
        let b = unsafe { plum_blob(c.core, h.as_ptr(), h.len()) };
        assert_ne!(b.status, PLUM_OK, "{:?}", String::from_utf8_lossy(h));
        assert!(b.ptr.is_null() && b.token == 0);
    }
    // SAFETY: null is allowed.
    assert_eq!(unsafe { plum_blob(c.core, std::ptr::null(), 5) }.status, PLUM_ERR_ARG);
    // SAFETY: live for the call.
    assert_eq!(unsafe { plum_blob(987_654, b"x".as_ptr(), 1) }.status, PLUM_ERR_CORE);
    assert_eq!(plum_release(0), PLUM_ERR_MISSING);
    assert_eq!(plum_release(u64::MAX), PLUM_ERR_MISSING);
    // SAFETY: an empty buffer is ignored.
    unsafe { plum_free(PlumBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 }) };

    // unknown cores, double close, calls after close
    let core = c.core;
    c.close();
    assert_eq!(plum_close(core), PLUM_ERR_CORE, "second close");
    assert_eq!(plum_close(0), PLUM_ERR_CORE);
    // SAFETY: live for the call.
    assert_eq!(unsafe { plum_request(core, b"{}".as_ptr(), 2) }, 0, "request after close");
    assert!(plum_poll(core).ptr.is_null(), "poll after close");
    // SAFETY: live for the call.
    assert_eq!(unsafe { plum_blob(core, b"x".as_ptr(), 1) }.status, PLUM_ERR_CORE);
    assert_eq!(plum_abi_version(), PLUM_ABI_VERSION);
}

#[test]
fn requests_from_many_threads_while_the_core_runs() {
    let mut c = Client::open(&json!({"store_root": store_root("threads"), "demo": true}));
    assert_eq!(c.call("run", json!({}))["ok"], true);
    let core = c.core;
    let threads: Vec<_> = (0..8)
        .map(|t| {
            std::thread::spawn(move || {
                let mut ids = Vec::new();
                for i in 0..200 {
                    let req = match i % 4 {
                        0 => json!({"method": "ping", "id": [t, i]}),
                        1 => json!({"method": "graph.get"}),
                        2 => json!({"method": "command.run", "params": {"command": "node.move", "params": {"id": "shout", "pos": [i, 0]}}}),
                        _ => json!({"method": "command.run", "params": {"command": "edit.undo"}}),
                    };
                    let text = req.to_string();
                    // SAFETY: live for the call.
                    let id = unsafe { plum_request(core, text.as_ptr(), text.len()) };
                    assert_ne!(id, 0);
                    ids.push(id);
                }
                ids
            })
        })
        .collect();
    let mut ids: Vec<u64> = threads.into_iter().flat_map(|t| t.join().unwrap()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 1600, "request numbers are unique");
    for id in ids {
        let r = c.wait_reply(id, Duration::from_secs(60));
        assert!(r.get("ok").is_some(), "{r}");
    }
    let done = c.call("spike.waitRun", json!({"timeout_ms": 30_000}));
    assert_eq!(done["ok"], true, "{done}");
    // every reply came once; coalesced wakes: never more wakes than polls plus one
    let wakes = c.signal.count.load(std::sync::atomic::Ordering::Relaxed);
    assert!(wakes <= c.polls + 1, "wakes {wakes}, polls {}", c.polls);
    c.close();
}

#[test]
fn two_cores_in_one_process() {
    let mut a = Client::open(&json!({"store_root": store_root("two-a"), "demo": true}));
    let mut b = Client::open(&json!({"store_root": store_root("two-b"), "demo": true}));
    assert_ne!(a.core, b.core);
    // an edit in one is not in the other
    a.run("node.add", json!({"kind": "text.upper"}));
    let ga = a.call("graph.get", json!({}));
    let gb = b.call("graph.get", json!({}));
    assert_eq!(ga["result"]["nodes"].as_object().unwrap().len(), 8);
    assert_eq!(gb["result"]["nodes"].as_object().unwrap().len(), 7);
    // both run at once, each with its own events
    assert_eq!(a.call("run", json!({}))["ok"], true);
    assert_eq!(b.call("run", json!({}))["ok"], true);
    let (da, db) = (a.call("spike.waitRun", json!({})), b.call("spike.waitRun", json!({})));
    assert_eq!(da["result"]["nodes"]["shout"]["state"], "done", "{da}");
    assert_eq!(db["result"]["nodes"]["shout"]["state"], "done", "{db}");
    let run = |c: &Client| -> Vec<Value> { c.events.iter().filter_map(|e| e.get("run_id").cloned()).collect() };
    let (ra, rb) = (run(&a), run(&b));
    assert!(!ra.is_empty() && !rb.is_empty());
    assert!(ra.iter().all(|r| !rb.contains(r)), "a run's events reached the other core");
    a.close();
    // the other goes on
    assert_eq!(b.call("ping", json!({}))["ok"], true);
    b.close();
}

#[test]
fn closing_from_a_busy_core_does_not_hang() {
    let mut c = Client::open(&json!({"store_root": store_root("close-busy"), "demo": true}));
    assert_eq!(c.call("run", json!({}))["ok"], true);
    // a run in flight (the demo's slow node takes 2 s), requests queued: close at once
    for _ in 0..100 {
        c.send(&json!({"method": "graph.get"}));
    }
    let t0 = std::time::Instant::now();
    c.close();
    assert!(t0.elapsed() < Duration::from_secs(5), "close took {:?}", t0.elapsed());
}
