//! The scene through the C ABI (N2 slice 2, `docs/shell-interface.md` section 8): `scene.get`
//! gives the whole scene, the feed's `changed {domain: "scene"}` says when it moved on, and
//! `scene.get {since}` gives only what changed. A run's progress is at most one scene change in
//! a poll. Hostile params are refused and the core goes on answering.

mod common;

use std::time::{Duration, Instant};

use common::{Client, store_root, take};
use plumcraft_ffi::plum_poll;
use serde_json::{Value, json};

trait Raw {
    fn send_raw(&self, bytes: &[u8]) -> u64;
}

impl Raw for Client {
    /// A request as given, bytes and all.
    fn send_raw(&self, bytes: &[u8]) -> u64 {
        // SAFETY: the bytes are live for the call.
        let id = unsafe { plumcraft_ffi::plum_request(self.core, bytes.as_ptr(), bytes.len()) };
        assert_ne!(id, 0);
        id
    }
}

fn add(c: &mut Client, kind: &str, x: f64) -> String {
    c.run("node.add", json!({"kind": kind, "pos": [x, 0]}))["id"].as_str().unwrap().to_owned()
}

/// Poll once as a shell would, keeping the replies in the client: the feed's scene changes in
/// this poll (each `{doc, domain, gen}`).
fn poll_scene(c: &mut Client) -> Vec<Value> {
    let bytes = take(plum_poll(c.core));
    if bytes.is_empty() {
        return Vec::new();
    }
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    for r in v["replies"].as_array().into_iter().flatten() {
        c.replies.insert(r["id"].as_u64().unwrap(), r.clone());
    }
    v["changed"].as_array().into_iter().flatten().filter(|x| x["domain"] == "scene").cloned().collect()
}

/// The newest scene generation the feed has named, and every poll's count of scene changes:
/// every poll after the first `scene.get` goes through here, so no change in the feed is missed.
#[derive(Default)]
struct Feed {
    seen: u64,
    most_in_a_poll: usize,
}

impl Feed {
    fn poll(&mut self, c: &mut Client) {
        let changes = poll_scene(c);
        self.most_in_a_poll = self.most_in_a_poll.max(changes.len());
        self.seen = changes.iter().filter_map(|ch| ch["gen"].as_u64()).fold(self.seen, u64::max);
    }

    /// Send a request and poll until its reply, noting the feed's scene changes.
    fn call(&mut self, c: &mut Client, method: &str, params: Value) -> Value {
        let id = c.send(&json!({"method": method, "params": params}));
        let t0 = Instant::now();
        loop {
            self.poll(c);
            if let Some(r) = c.replies.remove(&id) {
                return r;
            }
            assert!(t0.elapsed() < Duration::from_secs(60), "no reply to {method}");
            c.wait_wake(Duration::from_millis(100));
        }
    }

    fn run(&mut self, c: &mut Client, command: &str, params: Value) -> Value {
        let r = self.call(c, "command.run", json!({"command": command, "params": params}));
        assert_eq!(r["ok"], true, "{command}: {r}");
        r["result"].clone()
    }

    fn scene(&mut self, c: &mut Client, params: Value) -> Value {
        let r = self.call(c, "scene.get", params);
        assert_eq!(r["ok"], true, "{r}");
        r["result"].clone()
    }

    /// Poll until the feed has named a scene past `generation` (or `max`); the newest it named.
    fn past(&mut self, c: &mut Client, generation: u64, max: Duration) -> u64 {
        let t0 = Instant::now();
        while self.seen <= generation {
            assert!(t0.elapsed() < max, "no scene change past {generation} in {max:?}");
            c.wait_wake(Duration::from_millis(100));
            self.poll(c);
        }
        self.seen
    }
}

fn scene(c: &mut Client, params: Value) -> Value {
    let r = c.call("scene.get", params);
    assert_eq!(r["ok"], true, "{r}");
    r["result"].clone()
}

fn ids(list: &Value) -> Vec<String> {
    list.as_array().unwrap().iter().map(|i| i["id"].as_str().map_or_else(|| i.as_str().unwrap().to_owned(), str::to_owned)).collect()
}

#[test]
fn a_shell_reads_the_scene_and_then_only_what_changed() {
    let mut c = Client::open(&json!({"store_root": store_root("scene")}));
    let lit = add(&mut c, "text.literal", 0.0);
    let up = add(&mut c, "text.upper", 300.0);
    c.run("wire.connect", json!({"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}}));
    let mut f = Feed::default();

    // the whole scene
    let full = f.scene(&mut c, json!({}));
    assert_eq!((full["full"].as_bool(), full["shell"].as_u64()), (Some(true), Some(1)), "{full}");
    let g1 = full["gen"].as_u64().unwrap();
    assert!(g1 >= 1);
    assert_eq!(ids(&full["nodes"]), [lit.clone(), up.clone()]);
    assert_eq!(ids(&full["wires"]), [format!("{up}.text")]);
    let upper = &full["nodes"][1];
    assert_eq!(upper["header"]["title"]["text"], "Upper case");
    assert_eq!(upper["header"]["status"][0]["text"], "not run");
    assert_eq!(upper["inputs"][0]["source"], "socket", "{upper}");
    assert!(upper["nid"].as_u64().unwrap() > 0 && upper["rev"].as_u64().unwrap() == g1);
    let literal = &full["nodes"][0];
    assert_eq!(literal["inputs"][0]["widget"]["kind"], "text", "{literal}");
    assert_eq!(full["wires"][0]["fromFace"], json!(lit));
    // asked twice with nothing changed: the same generation
    assert_eq!(f.scene(&mut c, json!({}))["gen"].as_u64(), Some(g1));
    assert!(f.scene(&mut c, json!({"since": g1}))["upsert"]["nodes"].as_array().unwrap().is_empty());

    // one node moves: the feed says the scene changed, and the delta is that face alone
    f.run(&mut c, "node.move", json!({"id": lit, "pos": [0, 120]}));
    let g2 = f.past(&mut c, g1, Duration::from_secs(20));
    let d = f.scene(&mut c, json!({"since": g1}));
    assert_eq!(d["full"], false, "{d}");
    assert!(d["gen"].as_u64().unwrap() >= g2);
    assert_eq!(ids(&d["upsert"]["nodes"]), std::slice::from_ref(&lit), "{d}");
    assert_eq!(d["upsert"]["nodes"][0]["pos"], json!([0.0, 120.0]));
    // its wire's path moved with it (N2 slice 3: wires carry their paths), and only that wire
    assert_eq!(ids(&d["upsert"]["wires"]), [format!("{up}.text")], "{d}");
    assert_eq!(d["upsert"]["wires"][0]["path"][0][1].as_f64().map(|y| y > 120.0), Some(true), "{d}");
    assert!(d["remove"]["nodes"].as_array().unwrap().is_empty());

    // a run: its progress comes as scene changes, at most one a poll
    let before = d["gen"].as_u64().unwrap();
    f.run(&mut c, "node.set", json!({"node": lit, "port": "text", "value": "a corgi"}));
    f.run(&mut c, "run.start", json!({}));
    let done = f.call(&mut c, "ui.waitRun", json!({"timeout": 25}));
    assert_eq!(done["ok"], true, "{done}");
    let after = f.past(&mut c, before, Duration::from_secs(20));
    assert!(f.most_in_a_poll <= 1, "{} scene changes in one poll", f.most_in_a_poll);
    let d = f.scene(&mut c, json!({"since": before}));
    assert!(d["gen"].as_u64().unwrap() >= after);
    let ran = d["upsert"]["nodes"].as_array().unwrap();
    assert!(ran.iter().any(|x| x["id"] == json!(up) && matches!(x["run"]["state"].as_str(), Some("done" | "cached"))), "{d}");

    // a node removed: its face and its wire go
    f.run(&mut c, "node.remove", json!({"nodes": [up]}));
    let g = f.past(&mut c, d["gen"].as_u64().unwrap(), Duration::from_secs(20));
    let d2 = f.scene(&mut c, json!({"since": d["gen"]}));
    assert!(d2["gen"].as_u64().unwrap() >= g);
    assert_eq!(d2["remove"]["nodes"], json!([up]));
    assert_eq!(d2["remove"]["wires"], json!([format!("{up}.text")]));

    // the selection comes with every reply
    f.run(&mut c, "selection.set", json!({"nodes": [lit]}));
    assert_eq!(f.scene(&mut c, json!({"since": d2["gen"]}))["selection"]["nodes"], json!([lit]));

    // hostile params: refused, and the core goes on answering
    for bad in [json!({"since": -1}), json!({"since": "x"}), json!({"since": 1.5}), json!({"since": [1]}), json!({"what": 1}), json!([1]), json!("x"), json!(7)]
    {
        let r = f.call(&mut c, "scene.get", bad.clone());
        assert_eq!(r["ok"], false, "{bad}: {r}");
    }
    assert_eq!(f.scene(&mut c, json!({"since": u64::MAX}))["full"], true, "a generation from the future gets everything");
    assert_eq!(f.scene(&mut c, json!({"since": null}))["full"], true);
    assert!(f.most_in_a_poll <= 1, "{} scene changes in one poll", f.most_in_a_poll);
    c.close();
}

/// Everything a shell can send `scene.get`, raw: answered every time, never a panic, and the
/// core still serves the scene after.
#[test]
fn hostile_scene_requests_are_answered() {
    let mut c = Client::open(&json!({"store_root": store_root("scene-hostile")}));
    add(&mut c, "text.literal", 0.0);
    let deep = format!("{{\"method\": \"scene.get\", \"params\": {{\"since\": {}1{}}}}}", "[".repeat(200), "]".repeat(200));
    let raw: Vec<Vec<u8>> = vec![
        br#"{"method": "scene.get"}"#.to_vec(),
        br#"{"method": "scene.get", "params": null}"#.to_vec(),
        br#"{"method": "scene.get", "params": {"since": 1e30}}"#.to_vec(),
        br#"{"method": "scene.get", "params": {"since": 18446744073709551616}}"#.to_vec(),
        br#"{"method": "scene.get", "params": {"since": -0}}"#.to_vec(),
        br#"{"method": "scene.get", "params": {"since": {"gen": 1}}}"#.to_vec(),
        br#"{"method": "scene.get", "params": {"since": true}}"#.to_vec(),
        br#"{"method": "scene.get", "params": {}, "doc": 99999}"#.to_vec(),
        br#"{"method": "scene.get", "params": {}, "doc": "one"}"#.to_vec(),
        br#"{"method": "scene.get", "params": {"since": 0, "since": "x"}}"#.to_vec(),
        "{\"method\": \"scene.get\", \"params\": {\"since\": \"\u{0}\"}}".as_bytes().to_vec(),
        deep.into_bytes(),
    ];
    for b in &raw {
        let id = c.send_raw(b);
        let r = c.wait_reply(id, Duration::from_secs(10));
        assert!(r["ok"].is_boolean(), "{:?} -> {r}", String::from_utf8_lossy(b));
        if r["ok"] == false {
            assert!(r["error"].is_string(), "{r}");
        }
    }
    // a sweep of shapes in every position scene.get reads
    let shapes =
        [json!(null), json!(0), json!(-1), json!(1.5), json!(u64::MAX), json!("1"), json!([]), json!({}), json!(true), json!("\u{1F600}".repeat(1000))];
    for s in &shapes {
        for params in [json!({"since": s}), s.clone(), json!({"since": 1, "extra": s})] {
            let r = c.call("scene.get", params.clone());
            assert!(r["ok"].is_boolean(), "{params} -> {r}");
        }
    }
    assert_eq!(scene(&mut c, json!({}))["nodes"].as_array().unwrap().len(), 1);
    c.close();
}

/// A shell that fetches `scene.get {since}` on every scene change (the Swift kit's document model)
/// sees an edit's change in the feed within a display frame or so of the edit's reply, not a
/// tick (50 ms) later. N3 slice 2 found the second: a `scene.get` answered in a turn with no look
/// left the scene marked fresh, and the look after the next edit took the old generation for it.
#[test]
fn an_edit_reaches_the_feed_without_waiting_for_a_tick() {
    let mut c = Client::open(&json!({"store_root": store_root("scene-edit-feed")}));
    let mut feed = Feed::default();
    let a = add(&mut c, "text.literal", 0.0);
    let mut have = feed.scene(&mut c, json!({}))["gen"].as_u64().unwrap();
    let mut late = Vec::new();
    for i in 0..15 {
        feed.run(&mut c, "node.set", json!({"node": a, "port": "text", "value": format!("v{i}")}));
        let t0 = Instant::now();
        let seen = feed.past(&mut c, have, Duration::from_secs(5));
        late.push(t0.elapsed());
        // fetched at once, as a shell does on the change
        have = feed.scene(&mut c, json!({"since": have}))["gen"].as_u64().unwrap();
        assert!(have >= seen);
    }
    late.sort();
    let median = late[late.len() / 2];
    // a look owed is taken within SCENE_EVERY (16 ms) of the last; the bug's was a 50 ms tick
    assert!(median < Duration::from_millis(35), "an edit's scene change came {median:?} after its reply (median): {late:?}");
    c.close();
}

/// N3 slice 3: a native shell following the system's appearance names Paper or Dusk with
/// `view.theme`; `theme.get` and the scene's computed colours follow, and only the faces whose
/// colours changed come in the delta.
#[test]
fn view_theme_changes_the_scenes_theme() {
    let mut c = Client::open(&json!({"store_root": store_root("scene-theme")}));
    let lit = add(&mut c, "text.literal", 0.0);
    let paper = c.call("theme.get", json!({}));
    assert_eq!(paper["result"]["name"], "paper", "{paper}");
    let before = scene(&mut c, json!({}));
    let g = before["gen"].as_u64().unwrap();
    c.run("view.theme", json!({"name": "dusk"}));
    let dusk = c.call("theme.get", json!({}));
    assert_eq!(dusk["result"]["name"], "dusk", "{dusk}");
    assert_ne!(dusk["result"]["tokens"]["canvas"], paper["result"]["tokens"]["canvas"]);
    // the family's tint is computed from the theme, so the face changed
    let delta = scene(&mut c, json!({"since": g}));
    let faces = delta["upsert"]["nodes"].as_array().unwrap();
    assert_eq!(ids(&delta["upsert"]["nodes"]), [lit], "{delta}");
    assert_ne!(faces[0]["tint"], before["nodes"][0]["tint"]);
    // and back
    c.run("view.theme", json!({"name": "paper"}));
    assert_eq!(c.call("theme.get", json!({}))["result"]["name"], "paper");
}
