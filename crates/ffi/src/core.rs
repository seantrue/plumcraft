//! The core thread: it owns the [`Session`] and the engine, takes requests from a channel,
//! answers them into an outbox, and wakes the shell once per batch (coalesced: no second wake
//! until the shell has polled). No `unsafe` here; `lib.rs` is the boundary.
//!
//! Threads, per core:
//! - **core** (`plumcraft-ffi-core`): the session and every command. The only thread that calls
//!   the shell's `wake`. It never waits for an engine (`docs/shell-interface.md` 3.1 rule 6).
//! - **engine worker** (`plumcraft-engine-worker`): the editor core's [`EngineWorker`], which makes
//!   every engine call the session asks for (runs, cancels, releases) and does an engine switch's
//!   waiting part (stopping the old engine, starting the new, its catalogue), in order, and hands
//!   back what came of it; the core thread applies that when woken. A request that switches
//!   engines (`command.run engine.*`) replies once the switch is done.
//! - **pump** (`plumcraft-editor-pump`): the editor core's [`Link`], which long-polls
//!   `events.poll` and holds the events; its [`link::Waker`] tells the core thread to drain them.
//! - the engine's own worker (`plumcraft-engine`), or its process, started by the editor core's
//!   [`Launcher`] as the config names it (the text engine, in-process, by default).
//!
//! - **media** (`plumcraft-media-N`, started on first use): the media service
//!   ([`plumcraft_media::MediaService`]), which reads a value through the engine (for an HTTP
//!   engine, a network fetch) and decodes it (a thumbnail, a picture, peaks), so neither
//!   `plum_blob` nor this core thread ever waits for that. `media.get` asks for one; what finishes
//!   comes out of `plum_poll`'s `media`, after a wake; `plum_blob` then lends it.
//!
//! - **control** (`plumcraft-control`, only when the config names a `control` port): the same
//!   loopback control server the desktop editor's `--control` starts, so a probe drives an app on
//!   the C ABI as it drives the egui editor. Its requests come to the core thread.
//!
//! Every control-channel method (`command.*`, `graph.get`, `ui.*`, `ask.answer`) is the editor
//! core's dispatcher ([`plumcraft_editor_core::control`]), the one the egui editor uses, whether
//! it comes through `plum_request` or the control port; this core is its [`Shell`] (no canvas,
//! no pixels: `ui.screenshot` becomes a `screenshot` ask). Beside them: `run`, `ping`,
//! `core.control` (the control port, if any) and a few `spike.*` helpers for the measurements.

use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::{JoinHandle, ThreadId};
use std::time::{Duration, Instant};

use plumcraft_automation::server::{self, ControlRequest};
use plumcraft_coordinator::Coordinator;
use plumcraft_editor_core::asks::Asks;
use plumcraft_editor_core::control::{self, Driver, Parked, Shell, View};
use plumcraft_editor_core::engines::{Capabilities, EMPTY_WORKFLOW, EngineKind, Launcher, Mode};
use plumcraft_editor_core::invoke::{self, Cont, Host};
use plumcraft_editor_core::link::{self, Link};
use plumcraft_editor_core::switchboard::{Effects, Switchboard, Switching, Work, Worked};
use plumcraft_editor_core::worker::EngineWorker;
use plumcraft_graph::{Graph, PortRef};
use plumcraft_host::Connection;
use plumcraft_media::{Fetch, Made, MediaService, Want};
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
    /// The control channel (`control: {port, token_file}`, 4.1 and 5.3): serve it on
    /// `127.0.0.1:port` (0 picks a free one) when a port is given. Loopback only, always.
    pub control: Option<ControlConfig>,
    /// The media service's limits (`media: {cache_mb, workers, queue}`): a phone wants a smaller
    /// cache than the default 256 MiB.
    pub media: plumcraft_media::Config,
}

/// Where the control channel listens, and its token's file (none: a fresh token, printed to
/// stderr as the desktop editor's `--control` prints it).
#[derive(Debug, Clone, PartialEq)]
pub struct ControlConfig {
    pub port: u16,
    pub token_file: Option<PathBuf>,
}

impl Default for Config {
    /// The text engine, compiled in, on a platform that has declared nothing.
    fn default() -> Self {
        Config {
            store_root: None,
            demo: false,
            engine: None,
            mode: None,
            platform: Capabilities::NONE,
            control: None,
            media: media_config(None).unwrap_or_default(),
        }
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
            control: control_config(o.get("control"))?,
            media: media_config(o.get("media"))?,
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

/// `control: {port, token_file}`: none when absent, null or with a null port (4.1: "Null `port`:
/// no control channel"); an error for a port that isn't one.
fn control_config(v: Option<&Value>) -> Result<Option<ControlConfig>, String> {
    let Some(o) = v.filter(|v| !v.is_null()).map(|v| v.as_object().ok_or("config: control must be {port, token_file}")).transpose()? else {
        return Ok(None);
    };
    let port = match o.get("port") {
        None | Some(Value::Null) => return Ok(None),
        Some(p) => p.as_u64().and_then(|p| u16::try_from(p).ok()).ok_or("config: control.port must be a port number (0 picks a free one)")?,
    };
    let token_file = match o.get("token_file") {
        None | Some(Value::Null) => None,
        Some(Value::String(f)) if !f.trim().is_empty() && !f.contains('\0') => Some(PathBuf::from(f)),
        Some(_) => return Err("config: control.token_file must be a file path".into()),
    };
    Ok(Some(ControlConfig { port, token_file }))
}

/// `media: {cache_mb, workers, queue}`: the media service's limits, every one optional; finished
/// requests are reported (`plum_poll`'s `media`).
fn media_config(v: Option<&Value>) -> Result<plumcraft_media::Config, String> {
    let c = plumcraft_media::Config::from_json(v).map_err(|e| format!("config: {e}"))?;
    Ok(plumcraft_media::Config { report: true, ..c })
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
    /// Asks raised, and asks ended (`{ask, ended}`), since the last poll.
    asks: Vec<String>,
    asks_ended: Vec<String>,
    /// Media requests finished (`{key, handle, want, status, w?, h?, len?, format?, reason?}`).
    media: Vec<String>,
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
    /// The engine connection, following switches (none while one is under way or failed).
    conn: Mutex<Option<Arc<dyn Connection>>>,
    /// The store the engine publishes to: the config's root, else the shared default.
    store: Store,
    /// Every DataRef seen in the engine's events, by handle: how `plum_blob` resolves one through
    /// the connection, as the editor does.
    refs: Mutex<HashMap<String, DataRef>>,
    /// Blobs lent out and still held, by handle, so a second borrow shares the first's bytes.
    lent: Mutex<HashMap<String, Weak<Blob>>>,
    /// Values read through the engine and decoded, on workers (`media.get`, `plum_blob`).
    pub media: MediaService,
    /// A delay added to every read through the engine (`spike.slowResolve`, ms): a slow engine
    /// for the tests and measurements.
    slow_resolve: AtomicU64,
}

/// What `plum_blob` lends: a store blob's bytes, or a picture or numbers the media service made.
pub enum Lease {
    Blob(Arc<Blob>),
    Picture(Arc<plumcraft_media::Rgba>),
    Bytes(Arc<Vec<u8>>),
}

impl Lease {
    pub fn bytes(&self) -> &[u8] {
        match self {
            Lease::Blob(b) => &b.bytes,
            Lease::Picture(p) => &p.pixels,
            Lease::Bytes(v) => v,
        }
    }
}

/// What a `derive:` key (`docs/shell-interface.md` 10.2) or a plain handle asks for.
pub fn parse_key(key: &str) -> Option<(String, Want)> {
    let Some(rest) = key.strip_prefix("derive:") else { return Some((key.to_owned(), Want::Bytes)) };
    let (kind, rest) = rest.split_once(':')?;
    let px = |r: &str| -> Option<(String, u32)> {
        let (h, px) = r.rsplit_once('@')?;
        let px = px.parse::<u32>().ok().filter(|p| (1..=plumcraft_media::decode::MAX_TEXTURE_SIDE).contains(p))?;
        Some((h.to_owned(), px))
    };
    let (handle, want) = match kind {
        "rgba" => (rest.to_owned(), Want::Rendition),
        "peaks" => (rest.to_owned(), Want::Peaks),
        "numbers" => (rest.to_owned(), Want::Numbers),
        "image" => px(rest).map(|(h, p)| (h, Want::Picture(p)))?,
        "svg" => px(rest).map(|(h, p)| (h, Want::Svg(p)))?,
        _ => return None,
    };
    (!handle.is_empty()).then_some((handle, want))
}

/// The key `plum_blob` lends `want` of `handle` by.
pub fn key_for(handle: &str, want: Want) -> String {
    match want {
        Want::Bytes => handle.to_owned(),
        Want::Rendition => format!("derive:rgba:{handle}"),
        Want::Peaks => format!("derive:peaks:{handle}"),
        Want::Numbers => format!("derive:numbers:{handle}"),
        Want::Picture(px) => format!("derive:image:{handle}@{px}"),
        Want::Svg(px) => format!("derive:svg:{handle}@{px}"),
    }
}

/// A fetch as `media.get` replies and `plum_poll`'s `media` reports it.
fn describe(handle: &str, want: Want, f: &Fetch<Made>) -> Value {
    let mut v = json!({"key": key_for(handle, want), "handle": handle, "want": want.name(), "status": f.status()});
    match f {
        Fetch::Ready(Made::Picture(p)) => {
            v["w"] = json!(p.w);
            v["h"] = json!(p.h);
            v["len"] = json!(p.pixels.len());
            v["format"] = json!("rgba8");
        }
        Fetch::Ready(Made::Bytes(b)) => {
            v["len"] = json!(b.bytes.len());
            v["format"] = json!("bytes");
            v["dtype"] = json!(b.dtype);
            v["shape"] = json!(b.shape);
        }
        Fetch::Ready(Made::Peaks(p)) => {
            v["len"] = json!(p.data.len() * 8);
            v["format"] = json!("f32");
            v["channels"] = json!(p.channels);
            v["bins"] = json!(p.bins);
        }
        Fetch::Failed(why) => v["reason"] = json!(why),
        Fetch::Pending | Fetch::Missing => {}
    }
    v
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

    /// The media requests that finished since the last look, described for the shell's feed.
    fn push_media(&self) {
        let done = self.media.take_finished();
        if done.is_empty() {
            return;
        }
        let items: Vec<String> = done
            .into_iter()
            .filter_map(|f| {
                let r = self.reference(&f.key).ok()?;
                Some(describe(&f.key, f.want, &self.media.get(&r, f.want)).to_string())
            })
            .collect();
        lock(&self.outbox).media.extend(items);
    }

    fn push_asks(&self, raised: Vec<Value>, ended: Vec<Value>) {
        if raised.is_empty() && ended.is_empty() {
            return;
        }
        let mut o = lock(&self.outbox);
        o.asks.extend(raised.iter().map(Value::to_string));
        o.asks_ended.extend(ended.iter().map(Value::to_string));
    }

    /// Everything since the last poll, as one JSON object:
    /// `{"replies": [...], "events": [...], "dropped": n}`, with `"asks": [...]` (asks raised:
    /// `{ask, kind, modal, by, params}`) and `"asksEnded": [{ask, ended}]` when there are some;
    /// `None` when there is nothing.
    pub fn poll(&self) -> Option<Vec<u8>> {
        // cleared first: anything pushed after this wakes again
        self.waker.pending.store(false, Ordering::Release);
        let (replies, events, dropped, asks, asks_ended, media) = {
            let mut o = lock(&self.outbox);
            if o.replies.is_empty() && o.events.is_empty() && o.dropped == 0 && o.asks.is_empty() && o.asks_ended.is_empty() && o.media.is_empty() {
                return None;
            }
            (
                std::mem::take(&mut o.replies),
                std::mem::take(&mut o.events),
                std::mem::take(&mut o.dropped),
                std::mem::take(&mut o.asks),
                std::mem::take(&mut o.asks_ended),
                std::mem::take(&mut o.media),
            )
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
        if !asks.is_empty() || !asks_ended.is_empty() {
            out.extend_from_slice(b"],\"asks\":[");
            join_into(&mut out, &asks);
            out.extend_from_slice(b"],\"asksEnded\":[");
            join_into(&mut out, &asks_ended);
        }
        if !media.is_empty() {
            out.extend_from_slice(b"],\"media\":[");
            join_into(&mut out, &media);
        }
        out.extend_from_slice(format!("],\"dropped\":{dropped}}}").as_bytes());
        Some(out)
    }

    /// The reference a key's handle names: one the engine sent, else a store handle's.
    fn reference(&self, handle: &str) -> Result<DataRef, i32> {
        if let Some(r) = lock(&self.refs).get(handle).cloned() {
            return Ok(r);
        }
        let h: Handle = handle.parse().map_err(|_| crate::PLUM_ERR_MISSING)?;
        Ok(DataRef { handle: Some(h), meta: Default::default(), at: Vec::new() })
    }

    /// What `plum_blob` lends for `key`, never waiting for an engine: bytes already lent (shared);
    /// a store blob by handle, or by the locators of a value the engine sent (a mapping or a read
    /// on this machine: no engine, no network); else what the
    /// media service has made, or `PLUM_PENDING` while it reads through the engine (it says so
    /// in the feed's `media` when done), or why not.
    pub fn blob(&self, key: &str) -> Result<Lease, i32> {
        let (handle, want) = parse_key(key).ok_or(crate::PLUM_ERR_ARG)?;
        if want == Want::Bytes {
            if let Some(b) = lock(&self.lent).get(&handle).and_then(Weak::upgrade) {
                return Ok(Lease::Blob(b));
            }
            if let Ok(h) = handle.parse::<Handle>()
                && let Some(b) = self.store.map_handle(&h).or_else(|| self.store.read_handle(&h))
            {
                return Ok(Lease::Blob(self.lend(&handle, Arc::new(b))));
            }
            // a value the engine sent, where its locators say on this machine (a commonkit key
            // under this store, a path): mapped or read here, still no engine and no network
            let known = lock(&self.refs).get(&handle).cloned();
            if let Some(b) = known.and_then(|r| self.store.resolve(&r, true)) {
                return Ok(Lease::Blob(self.lend(&handle, Arc::new(b))));
            }
        }
        let r = self.reference(&handle)?;
        match self.media.get(&r, want) {
            Fetch::Ready(Made::Bytes(b)) => Ok(if want == Want::Bytes { Lease::Blob(self.lend(&handle, b)) } else { Lease::Blob(b) }),
            Fetch::Ready(Made::Picture(p)) => Ok(Lease::Picture(p)),
            Fetch::Ready(Made::Peaks(p)) => Ok(Lease::Bytes(Arc::new(p.data.iter().flat_map(|[a, b]| [a.to_le_bytes(), b.to_le_bytes()]).flatten().collect()))),
            Fetch::Pending => Err(crate::PLUM_PENDING),
            Fetch::Missing => Err(crate::PLUM_ERR_MISSING),
            Fetch::Failed(_) => Err(crate::PLUM_ERR_UNREADABLE),
        }
    }

    /// Keep `b` as the bytes lent for `handle`, so a second borrow shares them.
    fn lend(&self, handle: &str, b: Arc<Blob>) -> Arc<Blob> {
        let mut lent = lock(&self.lent);
        lent.retain(|_, w| w.strong_count() > 0);
        lent.insert(handle.to_owned(), Arc::downgrade(&b));
        b
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
    /// The control channel queued a request.
    Control,
    /// The media service finished something.
    Media,
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
        let link = launcher.connect_core(kind, mode, Some(waker.clone()))?;
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
        // the session's app starts (`/usr/bin/open`) become open.external asks for the shell
        let launches = invoke::route_launches(&mut session);
        session.connected = true;
        session.set_connection(Some(Arc::clone(&conn)));
        // engine.* commands switch among what this platform can start, and attach
        session.engines.choices = launcher.choices();
        session.engines.current = Some(plumcraft_session::EngineTarget::Choice { engine: kind.id(), mode: mode.id().to_owned() });
        session.engines.can_attach = true;
        let state = link.state();
        if config.demo {
            load_demo(&mut session, kind)?;
        }
        let store = Store::new(config.store_root.clone().unwrap_or_else(plumcraft_store::default_root));
        let shared = Arc::new(Shared {
            outbox: Mutex::default(),
            waker: Waker { f: wake, ctx: ctx as usize, pending: AtomicBool::new(false), sent: AtomicU64::new(0) },
            counters: Counters::default(),
            conn: Mutex::new(Some(conn)),
            store,
            refs: Mutex::default(),
            lent: Mutex::default(),
            media: MediaService::new(config.media.clone()),
            slow_resolve: AtomicU64::new(0),
        });
        // values are read through the current engine connection (following switches), or from
        // the store by handle; only ever on the media service's workers
        let weak = Arc::downgrade(&shared);
        shared.media.set_resolver(Some(Arc::new(move |r: &DataRef| {
            let s = weak.upgrade()?;
            let delay = s.slow_resolve.load(Ordering::Relaxed);
            if delay > 0 {
                std::thread::sleep(Duration::from_millis(delay.min(60_000)));
            }
            let conn = lock(&s.conn).clone();
            conn.and_then(|c| c.resolve(r)).or_else(|| r.handle.as_ref().and_then(|h| s.store.map_handle(h).or_else(|| s.store.read_handle(h))))
        })));
        let to_core = tx.clone();
        shared.media.set_wake(Some(Arc::new(move || {
            let _ = to_core.send(Msg::Media);
        })));
        let engines = EngineWorker::new(Some(waker.clone()));
        let (control_server, control_rx, control_token_file) = match &config.control {
            None => (None, None, None),
            Some(c) => {
                let token = server::server_token(c.token_file.as_deref()).map_err(|e| format!("control token: {e}"))?;
                let to_core = tx.clone();
                let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                    let _ = to_core.send(Msg::Control);
                });
                let (srv, rx) = server::Server::start(c.port, token.clone(), wake).map_err(|e| e.to_string())?;
                eprintln!("plumcraft: control channel on 127.0.0.1:{}", srv.port());
                match &c.token_file {
                    Some(p) => eprintln!("plumcraft: control token in {}", p.display()),
                    None => eprintln!("plumcraft: control token: {token}"),
                }
                (Some(srv), Some(rx), c.token_file.clone())
            }
        };
        let worker = Worker {
            session,
            shared: Arc::clone(&shared),
            waiters: Vec::new(),
            link: Some(link),
            kind,
            engines,
            launcher,
            waker,
            coordinator: None,
            state,
            last_error: None,
            switching: Switching::default(),
            parked: Parked::default(),
            asks: Asks::default(),
            prefs: plumcraft_editor_core::prefs::Store::default(),
            launches,
            control_rx,
            control_server,
            control_token_file,
        };
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
    /// The engine's event pump; dropping it stops the pump and waits for it. None while a
    /// switch is under way, or after one failed.
    link: Option<Link>,
    /// Which engine it opened on (`spike.demo` opens its demo).
    kind: EngineKind,
    /// Every engine call, off this thread.
    engines: EngineWorker,
    /// Starts the engines `engine.*` asks for (on the worker).
    launcher: Launcher,
    /// What new links (and the worker) wake: a `Drain` to this thread.
    waker: link::Waker,
    /// The switchboard's parts (`plumcraft_editor_core::switchboard`).
    coordinator: Option<Arc<Coordinator>>,
    state: EngineState,
    last_error: Option<String>,
    switching: Switching,
    /// Control requests whose reply waits (a switch, a run, a screenshot asked of the shell).
    parked: Parked,
    /// What the core asks the shell (`docs/shell-interface.md` section 11).
    asks: Asks<Cont>,
    /// The folders each kind of file ask last used (in memory: the shell keeps its own).
    prefs: plumcraft_editor_core::prefs::Store,
    /// The apps the session asked to start, until they are asked of the shell.
    launches: Option<invoke::Launches>,
    /// The control channel's requests (declared before its server, so they drop first and a
    /// connection waiting on one is answered at once).
    control_rx: Option<Receiver<ControlRequest>>,
    /// The control server; dropping it (the core closing) closes the port and its connections.
    control_server: Option<server::Server>,
    control_token_file: Option<PathBuf>,
}

/// The invoke machine's view of this core: no canvas, so no selection.
impl Host for Worker {
    fn session(&self) -> &Session {
        &self.session
    }
    fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }
    fn asks(&mut self) -> &mut Asks<Cont> {
        &mut self.asks
    }
    fn run(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let r = self.session.execute(id, params).map_err(|e| e.to_string());
        self.send_outgoing();
        if let Err(e) = &r {
            self.last_error = Some(e.clone());
        }
        r
    }
    fn say(&mut self, text: String) {
        self.last_error = Some(text);
    }
    fn prefs(&mut self) -> Option<&mut plumcraft_editor_core::prefs::Store> {
        Some(&mut self.prefs)
    }
    fn take_launches(&mut self) -> Vec<(PathBuf, Vec<String>, PathBuf)> {
        invoke::drain_launches(self.launches.as_ref())
    }
}

/// The control channel's view of this core: the core's parts of `ui.inspect`, no geometry; a
/// `screenshot` ask for `ui.screenshot`; no `ui.set` or `ui.audio`.
impl Shell for Worker {
    fn view(&self) -> View<'_> {
        View {
            link: self.link.as_ref(),
            state: &self.state,
            coordinator: self.coordinator.as_ref(),
            switching: &self.switching,
            last_error: self.last_error.as_deref(),
            asks: &self.asks,
            document: control::UNTITLED,
            app_title: control::APP_TITLE,
            run_anim: None,
            prefs: Some(&self.prefs),
            theme: None,
        }
    }
    fn parked(&mut self) -> &mut Parked {
        &mut self.parked
    }
    fn clear_last_error(&mut self) {
        self.last_error = None;
    }
    fn presented(&mut self) {
        self.feed_asks();
    }
}

/// Engine switching on this core: everything that waits goes to the worker.
impl Driver for Worker {
    fn worker(&mut self) -> &mut EngineWorker {
        &mut self.engines
    }
    fn switching(&mut self) -> &mut Switching {
        &mut self.switching
    }
    fn board<R>(&mut self, f: impl FnOnce(&mut Switchboard<'_>) -> R) -> R {
        Worker::board(self, f)
    }
    fn start(&mut self, w: Work) -> Option<Worked> {
        let carry = (self.launcher.clone(), self.waker.clone());
        self.engines.switch(w, carry, |(launcher, waker): &mut (Launcher, link::Waker), engine: &str, mode: &str| {
            launcher.connect_ids(engine, mode, Some(waker.clone()))
        });
        None
    }
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
                    let events = self.link.as_ref().map(Link::drain).unwrap_or_default();
                    if !events.is_empty() {
                        self.events(events);
                    }
                    self.step_engines();
                }
                Some(Msg::Media) => self.shared.push_media(),
                Some(Msg::Control) | None => {}
            }
            self.control_requests();
            self.answer_waiters();
            control::tick(&mut self);
            self.feed_asks();
            let o = lock(&self.shared.outbox);
            let any = !o.replies.is_empty() || !o.events.is_empty() || !o.asks.is_empty() || !o.asks_ended.is_empty() || !o.media.is_empty();
            drop(o);
            if any {
                self.shared.waker.wake();
            }
        }
        // a run in flight is cancelled (the core is closing: the shell is waiting for that);
        // the engine stops when the last holder drops it
        if let (Some(run_id), Some(link)) = (self.session.run.run_id.clone().filter(|_| self.session.run.running), &self.link) {
            let _ = link.conn.call("run.cancel", json!({"run_id": run_id}));
        }
        *lock(&self.shared.conn) = None;
    }

    fn reply(&self, id: u64, tag: Option<&Value>, r: Result<Value, String>) {
        let v = match r {
            Ok(v) => control::ok(v),
            Err(e) => control::err(e),
        };
        self.shared.push_reply(framed(id, tag, v));
    }

    /// Where a dispatcher's reply goes: the outbox, as request `id`'s (with its `tag`).
    fn replier(&self, id: u64, tag: Option<Value>) -> control::Reply {
        let shared = Arc::clone(&self.shared);
        Box::new(move |v: Value| shared.push_reply(framed(id, tag.as_ref(), v)))
    }

    /// The control channel's requests, through the same dispatcher.
    fn control_requests(&mut self) {
        let Some(rx) = &self.control_rx else { return };
        let requests: Vec<ControlRequest> = rx.try_iter().collect();
        for req in requests {
            if req.expired() {
                let _ = req.reply.send(control::err("timeout: the request expired before it ran"));
                continue;
            }
            let reply = req.reply.clone();
            control::handle(
                self,
                &req.method,
                &req.params,
                Box::new(move |v: Value| {
                    let _ = reply.send(v);
                }),
            );
            self.send_outgoing();
            self.step_engines();
        }
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
            // `command.run run.start`, shorter
            "run" => {
                let r = self.session.execute("run.start", &params).map_err(|e| e.to_string());
                self.send_outgoing();
                r
            }
            "ping" => Ok(json!({"pong": true})),
            // where the control channel listens, if it does
            "core.control" => Ok(match &self.control_server {
                Some(srv) => json!({"port": srv.port(), "tokenFile": self.control_token_file}),
                None => json!({"port": null}),
            }),
            "spike.waitRun" => {
                let ms = params.get("timeout_ms").and_then(Value::as_u64).unwrap_or(25_000).min(600_000);
                self.waiters.push(Waiter { id, tag, deadline: Instant::now() + Duration::from_millis(ms) });
                return;
            }
            "spike.demo" => load_demo(&mut self.session, self.kind).map(|()| json!({"nodes": self.session.graph().nodes.len()})),
            "spike.scene" => self.scene(&params),
            "spike.publish" => self.publish(&params),
            "spike.stats" => Ok(self.stats()),
            // a value as plain data, made on the media service's workers: the reply says at once
            // whether it is ready (then plum_blob lends it by `key`) or pending (the feed's
            // `media` says when)
            "media.get" => self.media_get(&params),
            "media.stats" => Ok(self.shared.media.stats().json()),
            // every read through the engine takes this long (a slow engine, for the tests)
            "spike.slowResolve" => match params.get("ms").and_then(Value::as_u64) {
                Some(ms) => {
                    self.shared.slow_resolve.store(ms.min(60_000), Ordering::Relaxed);
                    Ok(json!({"ms": ms.min(60_000)}))
                }
                None => Err("spike.slowResolve needs {ms}".into()),
            },
            // every control-channel method, as the egui editor answers it: a command that
            // switches engines replies once switched, and this thread doesn't wait for it
            _ => {
                let reply = self.replier(id, tag);
                control::handle(self, method, &params, reply);
                self.send_outgoing();
                self.step_engines();
                return;
            }
        };
        self.reply(id, tag.as_ref(), r);
    }

    /// `media.get {handle, want?, px?}`: `want` one of `bytes` (the default), `rgba` (an engine's
    /// rgba8 rendition as a picture), `peaks`, `image` (a picture file decoded, at most `px` a
    /// side, 256 by default), `svg` (drawn `px` on its long side), `numbers` (a picture file's
    /// own numbers). Replies at once.
    fn media_get(&self, p: &Value) -> Result<Value, String> {
        let handle = p.get("handle").and_then(Value::as_str).filter(|h| !h.is_empty() && h.len() <= 512).ok_or("media.get needs {handle}")?;
        let px = match p.get("px") {
            None | Some(Value::Null) => 256,
            Some(v) => v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| (1..=plumcraft_media::decode::MAX_TEXTURE_SIDE).contains(n))
                .ok_or("px must be a whole number from 1 to 4096")?,
        };
        let want = match p.get("want").and_then(Value::as_str).unwrap_or("bytes") {
            "bytes" => Want::Bytes,
            "rgba" => Want::Rendition,
            "peaks" => Want::Peaks,
            "image" => Want::Picture(px),
            "svg" => Want::Svg(px),
            "numbers" => Want::Numbers,
            other => return Err(format!("want {other:?}: one of bytes, rgba, peaks, image, svg, numbers")),
        };
        let r = self.shared.reference(handle).map_err(|_| format!("{handle:?} is not a value this core has seen, nor a handle"))?;
        Ok(describe(handle, want, &self.shared.media.get(&r, want)))
    }

    /// What was asked and what ended, to the shell's feed.
    fn feed_asks(&mut self) {
        invoke::tidy(self);
        let (raised, ended) = self.asks.take_feed();
        self.shared.push_asks(raised, ended);
    }

    /// Hand what the session queued for the engine to the worker (`pump_runs`, without the
    /// window's prompts). A refusal comes back to [`Worker::step_engines`].
    fn send_outgoing(&mut self) {
        for out in self.session.run.take_outbox() {
            let Some(conn) = self.link.as_ref().map(|l| Arc::clone(&l.conn)) else {
                if matches!(out, Outgoing::Run(_)) {
                    self.session.run.fail("run: the engine is not connected");
                }
                continue;
            };
            match out {
                Outgoing::Run(req) => match serde_json::to_value(&*req) {
                    Ok(body) => self.engines.run(conn, body, req.run_id.clone(), req.generation),
                    Err(e) => self.session.run.fail(&format!("run: {e}")),
                },
                Outgoing::Release(scope) => self.engines.call(conn, "graph.release", json!({"scope": scope})),
                Outgoing::Cancel(run_id) => self.engines.cancel(conn, run_id),
            }
        }
    }

    /// The engine parts, lent to the core's switchboard for `f`.
    fn board<R>(&mut self, f: impl FnOnce(&mut Switchboard<'_>) -> R) -> R {
        let (mut reloader, installer, mut installing) = (None, None, None);
        let mut b = Switchboard {
            session: &mut self.session,
            link: &mut self.link,
            coordinator: &mut self.coordinator,
            state: &mut self.state,
            last_error: &mut self.last_error,
            reloader: &mut reloader,
            installer: &installer,
            installing: &mut installing,
            switching: &mut self.switching,
            waker: Some(self.waker.clone()),
            effects: Effects::default(),
        };
        f(&mut b)
    }

    /// Engine switching without waiting, as the editor core drives it for every shell
    /// ([`control::step`]); then `plum_blob` follows the connection.
    fn step_engines(&mut self) {
        let mut out = Vec::new();
        control::step(self, &mut out);
        *lock(&self.shared.conn) = self.link.as_ref().map(|l| Arc::clone(&l.conn));
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

/// A dispatcher's reply (`{ok, result | error}`) as request `id`'s, with its `tag`; a null result
/// is left out, as before.
fn framed(id: u64, tag: Option<&Value>, mut v: Value) -> String {
    if let Some(o) = v.as_object_mut() {
        if o.get("result").is_some_and(Value::is_null) {
            o.remove("result");
        }
        o.insert("id".into(), json!(id));
        if let Some(tag) = tag {
            o.insert("tag".into(), tag.clone());
        }
    }
    v.to_string()
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
