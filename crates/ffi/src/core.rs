//! The core thread: it owns the [`Session`] and the engine, takes requests from a channel,
//! answers them into an outbox, and wakes the shell once per batch (coalesced: no second wake
//! until the shell has polled). No `unsafe` here; `lib.rs` is the boundary.
//!
//! Threads, per core:
//! - **core** (`plumcraft-ffi-core`): the session, every command, every engine call but the
//!   long poll. The only thread that calls the shell's `wake`.
//! - **pump** (`plumcraft-editor-pump`): the editor core's [`Link`], which long-polls
//!   `events.poll` and holds the events; its [`link::Waker`] tells the core thread to drain them.
//! - the engine's own worker (`plumcraft-engine`), or its process, started by the editor core's
//!   [`Launcher`] as the config names it (the text engine, in-process, by default).
//!
//! Spike scope (N0(b)): the methods are the control channel's session methods (`command.run`,
//! `command.list`, `graph.get`, through [`plumcraft_automation::dispatch`]) plus a few `spike.*`
//! helpers for the measurements. `plumcraft-editor-core` (N1) replaces this file.

use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::{JoinHandle, ThreadId};
use std::time::{Duration, Instant};

use plumcraft_automation::{ControlReply, ControlRequest};
use plumcraft_editor_core::engines::{Capabilities, EMPTY_WORKFLOW, EngineKind, Launcher, Mode};
use plumcraft_editor_core::link::{self, Link};
use plumcraft_graph::{Graph, PortRef};
use plumcraft_host::Connection;
use plumcraft_proto::data::DataRef;
use plumcraft_proto::event::{Envelope, Event};
use plumcraft_proto::handle::Handle;
use plumcraft_proto::session::EngineState;
use plumcraft_session::Session;
use plumcraft_session::run::Outgoing;
use plumcraft_store::{Blob, Store};
use serde_json::{Value, json};

/// The shell's wake: a plain C function and its context, called with no payload.
pub type WakeFn = extern "C" fn(*mut c_void);

/// The most events kept for a shell that doesn't poll; older ones are dropped and counted.
const MAX_EVENTS: usize = 20_000;
/// How long the core sleeps between looks at its timers (`spike.waitRun` deadlines).
const TICK: Duration = Duration::from_millis(50);
/// The largest buffer `spike.publish` makes.
const MAX_PUBLISH: u64 = 256 << 20;

/// Lock, taking a poisoned lock as it is (a panic elsewhere must not stop the shell).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What `plum_open` takes, as JSON. Every field is optional.
#[derive(Debug)]
pub struct Config {
    /// Where the engine publishes blobs (the shell's `Dirs.store`); the commonkit default without.
    pub store_root: Option<PathBuf>,
    /// Open the engine's demo graph.
    pub demo: bool,
    /// The engine (`engines.default` in `docs/shell-interface.md` 4.1, or the spike's `engine`);
    /// the text engine when absent.
    pub engine: Option<String>,
    /// Its hosting (`engines.mode`, or `mode`); the engine's default when absent.
    pub mode: Option<String>,
    /// What the platform has (`platform`, 4.2). What the config doesn't say it has, it hasn't:
    /// without `platform`, only engines compiled in and engines already running elsewhere.
    pub platform: Capabilities,
}

impl Default for Config {
    /// The text engine, compiled in, on a platform that has declared nothing.
    fn default() -> Self {
        Config { store_root: None, demo: false, engine: None, mode: None, platform: Capabilities::NONE }
    }
}

impl Config {
    pub fn parse(bytes: &[u8]) -> Result<Config, String> {
        if bytes.is_empty() {
            return Ok(Config::default());
        }
        let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("config: bad JSON: {e}"))?;
        let Some(o) = v.as_object() else { return Err("config: not a JSON object".into()) };
        let engines = o.get("engines").and_then(Value::as_object);
        let named = |key: &str, short: &str| engines.and_then(|e| e.get(key)).or_else(|| o.get(short)).and_then(Value::as_str).map(str::to_owned);
        Ok(Config {
            store_root: o.get("store_root").and_then(Value::as_str).map(PathBuf::from),
            demo: o.get("demo").and_then(Value::as_bool).unwrap_or(false),
            engine: named("default", "engine"),
            mode: named("mode", "mode"),
            platform: Capabilities::declared(o.get("platform")),
        })
    }

    /// The engine and hosting asked for, checked against what exists and what the platform can
    /// start.
    fn engine_and_mode(&self) -> Result<(EngineKind, Mode), String> {
        let kind = match &self.engine {
            Some(id) => EngineKind::from_id(id).ok_or_else(|| format!("config: engine {id:?}: there is no such engine"))?,
            None => EngineKind::Text,
        };
        let mode = match &self.mode {
            Some(m) => Mode::from_id(m).ok_or_else(|| format!("config: mode {m:?}: one of inproc, process, python, http, http-python"))?,
            None => kind.default_mode(),
        };
        if !kind.hosts(mode) {
            return Err(format!("config: the {} engine can't be hosted as {}", kind.id(), mode.id()));
        }
        if let Some(why) = self.platform.refuses(kind, mode) {
            return Err(format!("config: the {} engine as {}: {why}", kind.id(), mode.id()));
        }
        Ok((kind, mode))
    }
}

/// The shell's wake, called only by the core thread.
struct Waker {
    f: Option<WakeFn>,
    /// The shell's context pointer, kept as an address: the core never reads through it.
    ctx: usize,
    /// A wake is outstanding: set when one is sent, cleared by the shell's poll.
    pending: AtomicBool,
    sent: AtomicU64,
}

impl Waker {
    fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            self.sent.fetch_add(1, Ordering::Relaxed);
            if let Some(f) = self.f {
                f(self.ctx as *mut c_void);
            }
        }
    }
}

/// Replies and events waiting for the shell, each already JSON.
#[derive(Default)]
struct Outbox {
    replies: Vec<String>,
    events: Vec<String>,
    dropped: u64,
}

/// Counters for the measurements (`spike.stats`).
#[derive(Default)]
struct Counters {
    polls: AtomicU64,
    polls_with_events: AtomicU64,
    events: AtomicU64,
    max_events_per_poll: AtomicU64,
}

/// What the shell's threads and the core thread share.
pub struct Shared {
    outbox: Mutex<Outbox>,
    waker: Waker,
    counters: Counters,
    conn: Arc<dyn Connection>,
    /// The store the engine publishes to: the config's root, else the shared default.
    store: Store,
    /// Every DataRef seen in the engine's events, by handle: how `plum_blob` resolves one through
    /// the connection, as the editor does.
    refs: Mutex<HashMap<String, DataRef>>,
    /// Blobs lent out and still held, by handle, so a second borrow shares the first's bytes.
    lent: Mutex<HashMap<String, Weak<Blob>>>,
}

impl Shared {
    fn push_reply(&self, reply: String) {
        lock(&self.outbox).replies.push(reply);
    }

    fn push_events(&self, events: impl IntoIterator<Item = String>) {
        let mut o = lock(&self.outbox);
        o.events.extend(events);
        let excess = o.events.len().saturating_sub(MAX_EVENTS);
        if excess > 0 {
            o.events.drain(..excess);
            o.dropped += excess as u64;
        }
    }

    /// Everything since the last poll, as one JSON object:
    /// `{"replies": [...], "events": [...], "dropped": n}`; `None` when there is nothing.
    pub fn poll(&self) -> Option<Vec<u8>> {
        // cleared first: anything pushed after this wakes again
        self.waker.pending.store(false, Ordering::Release);
        let (replies, events, dropped) = {
            let mut o = lock(&self.outbox);
            if o.replies.is_empty() && o.events.is_empty() && o.dropped == 0 {
                return None;
            }
            (std::mem::take(&mut o.replies), std::mem::take(&mut o.events), std::mem::take(&mut o.dropped))
        };
        let c = &self.counters;
        c.polls.fetch_add(1, Ordering::Relaxed);
        if !events.is_empty() {
            c.polls_with_events.fetch_add(1, Ordering::Relaxed);
            c.events.fetch_add(events.len() as u64, Ordering::Relaxed);
            c.max_events_per_poll.fetch_max(events.len() as u64, Ordering::Relaxed);
        }
        let size = 48 + replies.iter().chain(&events).map(|s| s.len() + 1).sum::<usize>();
        let mut out = Vec::with_capacity(size);
        out.extend_from_slice(b"{\"replies\":[");
        join_into(&mut out, &replies);
        out.extend_from_slice(b"],\"events\":[");
        join_into(&mut out, &events);
        out.extend_from_slice(format!("],\"dropped\":{dropped}}}").as_bytes());
        Some(out)
    }

    /// A store blob's bytes, held: by a DataRef the engine sent (through the connection, checked
    /// against the handle), else the plain segment mapped, else read from the store. A blob
    /// already lent is shared, not read again.
    pub fn blob(&self, handle: &str) -> Result<Arc<Blob>, String> {
        if let Some(b) = lock(&self.lent).get(handle).and_then(Weak::upgrade) {
            return Ok(b);
        }
        let known = lock(&self.refs).get(handle).cloned();
        let blob = match known {
            Some(r) => self.conn.resolve(&r),
            None => {
                let h: Handle = handle.parse().map_err(|e| format!("not a handle: {e}"))?;
                self.store.map_handle(&h).or_else(|| self.store.read_handle(&h))
            }
        };
        let blob = Arc::new(blob.ok_or_else(|| format!("no bytes for {handle}"))?);
        let mut lent = lock(&self.lent);
        lent.retain(|_, w| w.strong_count() > 0);
        lent.insert(handle.to_owned(), Arc::downgrade(&blob));
        Ok(blob)
    }
}

fn join_into(out: &mut Vec<u8>, items: &[String]) {
    for (i, s) in items.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(s.as_bytes());
    }
}

/// What the core thread is sent.
enum Msg {
    Request {
        id: u64,
        body: Vec<u8>,
    },
    /// The link has events waiting.
    Drain,
    Stop,
}

/// One core: its threads and the way to reach them.
pub struct Core {
    tx: Sender<Msg>,
    pub shared: Arc<Shared>,
    next_id: AtomicU64,
    thread: Mutex<Option<JoinHandle<()>>>,
    thread_id: Option<ThreadId>,
}

impl Core {
    /// Start the engine, the session, the pump and the core thread.
    pub fn open(config: &Config, wake: Option<WakeFn>, ctx: *mut c_void) -> Result<Core, String> {
        let (kind, mode) = config.engine_and_mode()?;
        let launcher = Launcher { caps: config.platform, store_root: config.store_root.clone(), ..Default::default() };
        let (tx, rx) = channel();
        let drain = tx.clone();
        let waker = link::Waker::new(move || {
            let _ = drain.send(Msg::Drain);
        });
        let link = launcher.connect_core(kind, mode, Some(waker))?;
        match link.state() {
            EngineState::Connected { .. } => {}
            other => return Err(format!("the {} engine didn't start: {other:?}", kind.id())),
        }
        let conn = Arc::clone(&link.conn);
        let catalog = conn
            .call("schema", json!({}))
            .map_err(|e| e.message)
            .and_then(|v| serde_json::from_value(v).map_err(|e| format!("the engine's catalogue: {e}")))?;
        let mut session = Session::new(catalog);
        session.connected = true;
        session.set_connection(Some(Arc::clone(&conn)));
        if config.demo {
            load_demo(&mut session, kind)?;
        }
        let store = Store::new(config.store_root.clone().unwrap_or_else(plumcraft_store::default_root));
        let shared = Arc::new(Shared {
            outbox: Mutex::default(),
            waker: Waker { f: wake, ctx: ctx as usize, pending: AtomicBool::new(false), sent: AtomicU64::new(0) },
            counters: Counters::default(),
            conn,
            store,
            refs: Mutex::default(),
            lent: Mutex::default(),
        });
        let worker = Worker { session, shared: Arc::clone(&shared), waiters: Vec::new(), link, kind };
        let thread = std::thread::Builder::new().name("plumcraft-ffi-core".into()).spawn(move || worker.run(&rx)).map_err(|e| e.to_string())?;
        let thread_id = Some(thread.thread().id());
        Ok(Core { tx, shared, next_id: AtomicU64::new(1), thread: Mutex::new(Some(thread)), thread_id })
    }

    /// Queue a request; its id, which its reply carries. `None` once the core has stopped.
    pub fn request(&self, body: Vec<u8>) -> Option<u64> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.tx.send(Msg::Request { id, body }).ok().map(|()| id)
    }

    /// Stop the core thread and wait for it, unless this is the core thread itself (a shell that
    /// closes from inside its wake): then it stops on its own after this message.
    pub fn close(&self) {
        let _ = self.tx.send(Msg::Stop);
        if self.thread_id == Some(std::thread::current().id()) {
            return;
        }
        if let Some(t) = lock(&self.thread).take() {
            let _ = t.join();
        }
    }
}

/// The engine's demo graph (an empty one for an engine without), as one undo step.
fn load_demo(s: &mut Session, kind: EngineKind) -> Result<(), String> {
    let demo = kind.own_demo()?.unwrap_or_else(|| EMPTY_WORKFLOW.to_owned());
    let demo = Graph::from_json(&demo).map_err(|e| e.to_string())?;
    s.edit("Open Demo", |g, _| {
        *g = demo;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// A `spike.waitRun` waiting for the run to end.
struct Waiter {
    id: u64,
    tag: Option<Value>,
    deadline: Instant,
}

/// The core thread's state.
struct Worker {
    session: Session,
    shared: Arc<Shared>,
    waiters: Vec<Waiter>,
    /// The engine's event pump; dropping it stops the pump and waits for it.
    link: Link,
    /// Which engine it is (`spike.demo` opens its demo).
    kind: EngineKind,
}

impl Worker {
    fn run(mut self, rx: &Receiver<Msg>) {
        loop {
            let msg = match rx.recv_timeout(TICK) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            match msg {
                Some(Msg::Stop) => break,
                Some(Msg::Request { id, body }) => self.request(id, &body),
                Some(Msg::Drain) => {
                    let events = self.link.drain();
                    if !events.is_empty() {
                        self.events(events);
                    }
                }
                None => {}
            }
            self.answer_waiters();
            if !lock(&self.shared.outbox).replies.is_empty() || !lock(&self.shared.outbox).events.is_empty() {
                self.shared.waker.wake();
            }
        }
        // a run in flight is cancelled; the engine stops when the last holder drops it
        if let Some(run_id) = self.session.run.run_id.clone().filter(|_| self.session.run.running) {
            let _ = self.shared.conn.call("run.cancel", json!({"run_id": run_id}));
        }
    }

    fn reply(&self, id: u64, tag: Option<&Value>, r: Result<Value, String>) {
        let reply = match r {
            Ok(v) => ControlReply::ok(id, v),
            Err(e) => ControlReply::err(id, e),
        };
        let mut v = serde_json::to_value(&reply).unwrap_or(Value::Null);
        if let (Some(tag), Some(o)) = (tag, v.as_object_mut()) {
            o.insert("tag".into(), tag.clone());
        }
        self.shared.push_reply(v.to_string());
    }

    fn request(&mut self, id: u64, body: &[u8]) {
        let v: Value = match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(e) => return self.reply(id, None, Err(format!("bad JSON: {e}"))),
        };
        let tag = v.get("id").cloned();
        let Some(method) = v.get("method").and_then(Value::as_str) else {
            return self.reply(id, tag.as_ref(), Err("a request needs a method".into()));
        };
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        let r = match method {
            "command.run" | "engine.execute" | "command.list" | "engine.commands" | "graph.get" => {
                let r = plumcraft_automation::dispatch(&mut self.session, &ControlRequest { id, method: method.to_owned(), params });
                self.send_outgoing();
                if r.ok { Ok(r.result) } else { Err(r.error.unwrap_or_default()) }
            }
            // `command.run run.start`, shorter
            "run" => {
                let r = self.session.execute("run.start", &params).map_err(|e| e.to_string());
                self.send_outgoing();
                r
            }
            "ping" => Ok(json!({"pong": true})),
            "spike.waitRun" => {
                let ms = params.get("timeout_ms").and_then(Value::as_u64).unwrap_or(25_000).min(600_000);
                self.waiters.push(Waiter { id, tag, deadline: Instant::now() + Duration::from_millis(ms) });
                return;
            }
            "spike.demo" => load_demo(&mut self.session, self.kind).map(|()| json!({"nodes": self.session.graph().nodes.len()})),
            "spike.scene" => self.scene(&params),
            "spike.publish" => self.publish(&params),
            "spike.stats" => Ok(self.stats()),
            other => Err(format!("unknown method {:?}", other.chars().take(64).collect::<String>())),
        };
        self.reply(id, tag.as_ref(), r);
    }

    /// Send what the session queued for the engine (`pump_runs`, without the window's prompts).
    fn send_outgoing(&mut self) {
        for out in self.session.run.take_outbox() {
            match out {
                Outgoing::Run(req) => {
                    let sent =
                        serde_json::to_value(&*req).map_err(|e| e.to_string()).and_then(|body| self.shared.conn.call("run", body).map_err(|e| e.message));
                    if let Err(why) = sent {
                        self.session.run.fail(&format!("run: {why}"));
                    }
                }
                Outgoing::Release(scope) => {
                    let _ = self.shared.conn.call("graph.release", json!({"scope": scope}));
                }
                Outgoing::Cancel(run_id) => {
                    let _ = self.shared.conn.call("run.cancel", json!({"run_id": run_id}));
                }
            }
        }
    }

    /// The engine's events: into the session's run state, their DataRefs remembered, and on to
    /// the shell as they came.
    fn events(&mut self, events: Vec<Envelope>) {
        let now = Instant::now();
        let mut out = Vec::with_capacity(events.len());
        for e in &events {
            self.session.run.apply_event(e, now);
            let v = serde_json::to_value(e).unwrap_or(Value::Null);
            if matches!(e.event, Event::RunReply(_) | Event::Status(_)) {
                let mut refs = lock(&self.shared.refs);
                collect_refs(&v, &mut refs);
            }
            out.push(v.to_string());
        }
        self.shared.push_events(out);
        // a reply may have queued work (a rerun the watch asked for)
        self.send_outgoing();
    }

    fn answer_waiters(&mut self) {
        if self.waiters.is_empty() {
            return;
        }
        let now = Instant::now();
        let idle = !self.session.run.running && self.session.run.outbox_is_empty();
        let (done, waiting): (Vec<Waiter>, Vec<Waiter>) = std::mem::take(&mut self.waiters).into_iter().partition(|w| idle || now >= w.deadline);
        self.waiters = waiting;
        for w in done {
            let r = if idle { Ok(self.run_summary()) } else { Err("timeout: the run is still going".into()) };
            self.reply(w.id, w.tag.as_ref(), r);
        }
    }

    fn run_summary(&self) -> Value {
        let run = &self.session.run;
        let nodes: BTreeMap<&str, Value> =
            run.nodes.iter().map(|(id, n)| (id.as_str(), json!({"state": n.state.map(|s| format!("{s:?}").to_lowercase()), "ms": n.ms}))).collect();
        let watch: Vec<Value> = run.watched.iter().map(|d| serde_json::to_value(d).unwrap_or(Value::Null)).collect();
        json!({"running": run.running, "runId": run.run_id, "generation": run.generation, "runError": run.run_error, "nodes": nodes, "watch": watch})
    }

    /// `spike.scene {nodes, wires}`: a large graph in one undo step, of text nodes that a run
    /// could compute (Join nodes take two wires, Upper case one), wired from earlier nodes only.
    fn scene(&mut self, p: &Value) -> Result<Value, String> {
        let n = p.get("nodes").and_then(Value::as_u64).unwrap_or(500).clamp(2, 20_000) as usize;
        let want = p.get("wires").and_then(Value::as_u64).unwrap_or(800) as usize;
        let cat = self.session.catalog().clone();
        let def = |k: &str| cat.node(k).cloned().ok_or_else(|| format!("no node type {k}"));
        let (lit, join, upper) = (def("text.literal")?, def("text.join")?, def("text.upper")?);
        // joins take two wires each, the rest one: enough joins to reach `want`
        let inputs_wanted = want.min(2 * (n - 1));
        let joins = inputs_wanted.saturating_sub(n - 1).min(n - 1);
        self.session
            .edit("Scene", move |g, cat| {
                *g = Graph::new();
                let mut ids = Vec::with_capacity(n);
                let mut wires = 0;
                for i in 0..n {
                    let pos = [(i % 25) as f32 * 260.0, (i / 25) as f32 * 160.0];
                    let kind = if i == 0 {
                        &lit
                    } else if i <= joins {
                        &join
                    } else {
                        &upper
                    };
                    let id = g.add_node(kind, pos);
                    if i > 0 {
                        let ports: &[&str] = if i <= joins { &["a", "b"] } else { &["text"] };
                        for (k, port) in ports.iter().enumerate() {
                            if wires >= want {
                                break;
                            }
                            let from: &plumcraft_graph::NodeId = &ids[(i * 7 + k * 13) % i];
                            g.connect(cat, PortRef::new(from.clone(), "text"), PortRef::new(id.clone(), *port))?;
                            wires += 1;
                        }
                    }
                    ids.push(id);
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        let g = self.session.graph();
        Ok(json!({"nodes": g.nodes.len(), "wires": g.wires.len()}))
    }

    /// `spike.publish {bytes, seed?}`: that many pseudo-random bytes (so the store keeps them as
    /// they are, not compressed) published to the engine's store: `{handle}`.
    fn publish(&self, p: &Value) -> Result<Value, String> {
        let n = p.get("bytes").and_then(Value::as_u64).unwrap_or(64 << 20).min(MAX_PUBLISH) as usize;
        let mut x = p.get("seed").and_then(Value::as_u64).unwrap_or(0x9e37_79b9_7f4a_7c15) | 1;
        let mut bytes = Vec::with_capacity(n + 8);
        while bytes.len() < n {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        bytes.truncate(n);
        let store: Store = self.shared.store.clone();
        let (handle, _) = store.publish_raw(&bytes).ok_or("the store declined the bytes")?;
        Ok(json!({"handle": handle.to_string(), "bytes": n}))
    }

    fn stats(&self) -> Value {
        let c = &self.shared.counters;
        let get = |a: &AtomicU64| a.load(Ordering::Relaxed);
        json!({
            "wakes": get(&self.shared.waker.sent),
            "polls": get(&c.polls),
            "pollsWithEvents": get(&c.polls_with_events),
            "events": get(&c.events),
            "maxEventsPerPoll": get(&c.max_events_per_poll),
        })
    }
}

/// Every DataRef in an event, by handle (`{"handle": …, "meta": …, "at": […]}` anywhere in it).
fn collect_refs(v: &Value, into: &mut HashMap<String, DataRef>) {
    match v {
        Value::Object(o) => {
            if let (Some(h), Some(_)) = (o.get("handle").and_then(Value::as_str), o.get("at"))
                && let Ok(r) = serde_json::from_value::<DataRef>(v.clone())
            {
                into.insert(h.to_owned(), r);
            }
            for c in o.values() {
                collect_refs(c, into);
            }
        }
        Value::Array(a) => a.iter().for_each(|c| collect_refs(c, into)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(json: &str) -> Result<Config, String> {
        Config::parse(json.as_bytes())
    }

    #[test]
    fn the_config_names_an_engine_and_its_hosting_on_the_platform_declared() {
        // as docs/shell-interface.md 4.1 writes it, and as the spike did
        let c = config(r#"{"engines": {"default": "array", "mode": "inproc"}, "platform": {"subprocess": false}}"#).unwrap();
        assert_eq!(c.engine_and_mode(), Ok((EngineKind::Array, Mode::InProc)));
        assert_eq!(config(r#"{"engine": "text"}"#).unwrap().engine_and_mode(), Ok((EngineKind::Text, Mode::InProc)));
        assert_eq!(config("").unwrap().engine_and_mode(), Ok((EngineKind::Text, Mode::InProc)), "the text engine by default");
        // what the platform can't start is refused with the reason, before anything starts
        let e = config(r#"{"engines": {"default": "text", "mode": "process"}}"#).unwrap().engine_and_mode().unwrap_err();
        assert!(e.contains("can't start programs"), "{e}");
        let e = config(r#"{"engine": "vectorcraft"}"#).unwrap().engine_and_mode().unwrap_err();
        assert!(e.contains("no craft apps"), "{e}");
        let desktop = r#"{"engine": "text", "mode": "python", "platform": {"subprocess": true, "python": false}}"#;
        assert!(config(desktop).unwrap().engine_and_mode().unwrap_err().contains("no Python"));
        // what doesn't exist, or can't be hosted so
        assert!(config(r#"{"engine": "nonesuch"}"#).unwrap().engine_and_mode().unwrap_err().contains("nonesuch"));
        assert!(config(r#"{"engine": "text", "mode": "pigeon"}"#).unwrap().engine_and_mode().unwrap_err().contains("pigeon"));
        assert!(config(r#"{"engine": "array", "mode": "python"}"#).unwrap().engine_and_mode().unwrap_err().contains("can't be hosted"));
        // hostile shapes are ignored, not fatal
        let c = config(r#"{"engines": 7, "engine": ["text"], "platform": "everything"}"#).unwrap();
        assert_eq!((c.engine_and_mode(), c.platform), (Ok((EngineKind::Text, Mode::InProc)), Capabilities::NONE));
    }

    #[test]
    fn a_core_opens_on_the_array_engine_named_in_its_config() {
        let root = plumcraft_scratch::dir("ffi-core-array");
        let c = config(&format!(r#"{{"engines": {{"default": "array"}}, "store_root": {:?}, "demo": true}}"#, root.display().to_string())).unwrap();
        let core = Core::open(&c, None, std::ptr::null_mut()).unwrap();
        core.close();
    }
}
