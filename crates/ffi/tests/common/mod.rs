//! A client of the C ABI written as a C caller would write it: a wake that only signals, a
//! poll after each wake, replies matched by request number.

#![allow(dead_code)]

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant};

use plumcraft_ffi::{PlumBuf, plum_close, plum_free, plum_open, plum_poll, plum_request};
use serde_json::{Value, json};

#[derive(Default)]
pub struct Signal {
    pub woken: Mutex<bool>,
    pub cv: Condvar,
    pub count: AtomicUsize,
}

/// Signals by number: the wake's ctx is the number, so the wake reads no pointer.
static SIGNALS: LazyLock<Mutex<HashMap<usize, Arc<Signal>>>> = LazyLock::new(Mutex::default);
static NEXT: AtomicUsize = AtomicUsize::new(1);

pub extern "C" fn wake(ctx: *mut c_void) {
    let s = SIGNALS.lock().unwrap().get(&(ctx as usize)).cloned();
    if let Some(s) = s {
        s.count.fetch_add(1, Ordering::Relaxed);
        *s.woken.lock().unwrap() = true;
        s.cv.notify_all();
    }
}

/// A buffer's bytes, copied, and the buffer freed.
pub fn take(buf: PlumBuf) -> Vec<u8> {
    if buf.ptr.is_null() {
        return Vec::new();
    }
    // SAFETY: plum_poll's buffer holds len bytes until freed, once, here.
    unsafe {
        let v = std::slice::from_raw_parts(buf.ptr, buf.len).to_vec();
        plum_free(buf);
        v
    }
}

pub struct Client {
    pub core: u64,
    pub signal: Arc<Signal>,
    pub replies: HashMap<u64, Value>,
    pub events: Vec<Value>,
    pub polls: usize,
    pub events_per_poll: Vec<usize>,
    /// The size of each poll's buffer.
    pub poll_bytes: Vec<usize>,
    /// The `changed` entries every poll brought, in order (a test that waits for one may have
    /// had it polled while waiting for a reply).
    pub changed: Vec<Value>,
}

pub fn store_root(name: &str) -> String {
    plumcraft_scratch::dir(&format!("ffi-{name}")).display().to_string()
}

impl Client {
    pub fn open(config: &Value) -> Client {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let signal = Arc::new(Signal::default());
        SIGNALS.lock().unwrap().insert(n, Arc::clone(&signal));
        let text = config.to_string();
        // SAFETY: the config is live for the call; ctx is a number the wake looks up.
        let core = unsafe { plum_open(text.as_ptr(), text.len(), Some(wake), n as *mut c_void) };
        assert_ne!(core, 0, "plum_open: {}", String::from_utf8_lossy(&take(plumcraft_ffi::plum_last_error())));
        Client { core, signal, replies: HashMap::new(), events: Vec::new(), polls: 0, events_per_poll: Vec::new(), poll_bytes: Vec::new(), changed: Vec::new() }
    }

    pub fn send(&self, req: &Value) -> u64 {
        let text = req.to_string();
        // SAFETY: the request is live for the call.
        let id = unsafe { plum_request(self.core, text.as_ptr(), text.len()) };
        assert_ne!(id, 0);
        id
    }

    /// Poll once: replies and events into the client.
    pub fn poll(&mut self) -> bool {
        let bytes = take(plum_poll(self.core));
        if bytes.is_empty() {
            return false;
        }
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        self.polls += 1;
        self.poll_bytes.push(bytes.len());
        for r in v["replies"].as_array().unwrap() {
            self.replies.insert(r["id"].as_u64().unwrap(), r.clone());
        }
        let events = v["events"].as_array().unwrap();
        if !events.is_empty() {
            self.events_per_poll.push(events.len());
        }
        self.events.extend(events.iter().cloned());
        self.changed.extend(v["changed"].as_array().into_iter().flatten().cloned());
        true
    }

    /// Wait for a wake (or `max`), as a shell's main loop would.
    pub fn wait_wake(&self, max: Duration) {
        let w = self.signal.woken.lock().unwrap();
        let (mut w, _) = self.signal.cv.wait_timeout_while(w, max, |w| !*w).unwrap();
        *w = false;
    }

    pub fn wait_reply(&mut self, id: u64, max: Duration) -> Value {
        let t0 = Instant::now();
        loop {
            self.poll();
            if let Some(r) = self.replies.remove(&id) {
                return r;
            }
            assert!(t0.elapsed() < max, "no reply to request {id} in {max:?}");
            self.wait_wake(Duration::from_millis(200));
        }
    }

    /// Send and wait for the reply.
    pub fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.send(&json!({"method": method, "params": params}));
        self.wait_reply(id, Duration::from_secs(60))
    }

    /// `command.run`, asserting it worked: its result.
    pub fn run(&mut self, command: &str, params: Value) -> Value {
        let r = self.call("command.run", json!({"command": command, "params": params}));
        assert_eq!(r["ok"], true, "{command}: {r}");
        r["result"].clone()
    }

    pub fn close(self) {
        assert_eq!(plum_close(self.core), plumcraft_ffi::PLUM_OK);
    }
}

/// The spike's graph: a 64-byte line repeated a million times (64 MB of text, published to the
/// store), upper-cased, and the demo's slow node beside it, so a run has status to report.
pub fn build_graph(c: &mut Client) -> (String, String) {
    let lit = c.run("node.add", json!({"kind": "text.literal", "pos": [0, 0]}))["id"].as_str().unwrap().to_owned();
    c.run("node.set", json!({"node": lit, "port": "text", "value": "The quick brown fox jumps over the lazy dog, then naps. 0123456\n"}));
    let rep = c.run("node.add", json!({"kind": "text.repeat", "pos": [260, 0]}))["id"].as_str().unwrap().to_owned();
    c.run("node.set", json!({"node": rep, "port": "times", "value": 1_000_000}));
    let slow = c.run("node.add", json!({"kind": "text.slow", "pos": [260, 160]}))["id"].as_str().unwrap().to_owned();
    c.run("node.set", json!({"node": slow, "port": "seconds", "value": 0.3}));
    let up = c.run("node.add", json!({"kind": "text.upper", "pos": [520, 160]}))["id"].as_str().unwrap().to_owned();
    c.run("wire.connect", json!({"from": {"node": lit, "port": "text"}, "to": {"node": rep, "port": "text"}}));
    c.run("wire.connect", json!({"from": {"node": lit, "port": "text"}, "to": {"node": slow, "port": "text"}}));
    c.run("wire.connect", json!({"from": {"node": slow, "port": "text"}, "to": {"node": up, "port": "text"}}));
    (rep, up)
}
