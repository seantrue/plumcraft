//! The core thread: it holds the core's documents (`docs/shell-interface.md` 4.3), each a
//! [`Document`] of the editor core (its session, engine link and worker, asks: the same document
//! the egui editor holds), takes requests from a channel, answers them into an outbox, and wakes
//! the shell once per batch (coalesced: no second wake until the shell has polled). No `unsafe`
//! here; `lib.rs` is the boundary.
//!
//! **Documents.** `plum_open` opens the first; `document.open {demo?, path?}` more, `document.list`
//! lists them, `document.close {doc, discard?}` closes one (asking first over unsaved changes). A
//! request names its document with a top-level `doc`; one that names none acts on the first still
//! open, so a shell with one document never names it. An engine compiled in is shared by the
//! documents (decision 11B: one engine, a link each; each document forwards only its own runs'
//! events, each naming its `doc`).
//!
//! **Nothing waits for an engine, not even `plum_open`.** A document's first engine connects on
//! its worker; requests for it meanwhile are held and answered in order once it has (or once it
//! failed to start), and the feed's `changed` says so (`{doc, domain: "engine", gen, connected,
//! label, ready}`).
//!
//! Threads, per core:
//! - **core** (`plumcraft-ffi-core`): the documents and every command. The only thread that calls
//!   the shell's `wake`. It never waits for an engine (`docs/shell-interface.md` 3.1 rule 6).
//! - **engine worker** (`plumcraft-engine-worker`, one per document): the editor core's engine worker, which makes
//!   every engine call the session asks for (runs, cancels, releases) and does an engine switch's
//!   waiting part (stopping the old engine, starting the new, its catalogue), in order, and hands
//!   back what came of it; the core thread applies that when woken. A request that switches
//!   engines (`command.run engine.*`) replies once the switch is done.
//! - **pump** (`plumcraft-editor-pump`): the editor core's [`link::Link`], which long-polls
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
use plumcraft_editor_core::asks::By;
use plumcraft_editor_core::control::{self, Shell};
use plumcraft_editor_core::document::{self, DocId, Document, Opening, SharedEngines, shared_starter};
use plumcraft_editor_core::engines::{Capabilities, EMPTY_WORKFLOW, EngineKind, Launcher, Mode};
use plumcraft_editor_core::geometry::{FaceGeometry, Report, Snapshot};
use plumcraft_editor_core::invoke;
use plumcraft_editor_core::link;
use plumcraft_editor_core::switchboard;
use plumcraft_graph::{Graph, PortRef};
use plumcraft_host::Connection;
use plumcraft_media::{Fetch, Made, MediaService, Want};
use plumcraft_proto::data::DataRef;
use plumcraft_proto::event::{Envelope, Event};
use plumcraft_proto::handle::Handle;
use plumcraft_session::Session;
use plumcraft_store::{Blob, Store};
use serde_json::{Value, json};

/// The shell's wake: a plain C function and its context, called with no payload.
pub type WakeFn = extern "C" fn(*mut c_void);

/// The most events kept for a shell that doesn't poll; older ones are dropped and counted.
const MAX_EVENTS: usize = 20_000;
/// How long the core sleeps between looks at its timers (`spike.waitRun` deadlines).
const TICK: Duration = Duration::from_millis(50);
/// How long a busy core goes at most without looking whether its menus changed.
const MENUS_EVERY: Duration = Duration::from_secs(1);
/// The most often a document's scene is looked at while the core is busy (a display frame): run
/// progress makes at most one scene change in the feed a frame, and a poll sees one at most.
const SCENE_EVERY: Duration = Duration::from_millis(16);
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
    /// Engines started beside it in every document (`engines.also`, N3 slice 4), each by id and
    /// hosted its own default way: one catalogue for both, as `engine.add` gives (the Mac app
    /// opens every document on the text and array engines, both compiled in).
    pub also: Vec<String>,
    /// Start only the engines a document's graph uses (`engines.auto`, default true; decision 8C at
    /// N3's exit): a document opens on the engines its file uses (`engines.default` for one that
    /// uses none, or that can't be read before it opens), and picks engines automatically
    /// (`engine.auto`) from then on, so a graph that comes to use another engine (inserted, or a
    /// node of its kind added) starts it. False: every document starts on `engines.default` and
    /// `engines.also` only, as before.
    pub auto: bool,
    /// What the platform has (`platform`, 4.2). What the config doesn't say it has, it hasn't:
    /// without `platform`, only engines compiled in and engines already running elsewhere.
    pub platform: Capabilities,
    /// The control channel (`control: {port, token_file}`, 4.1 and 5.3): serve it on
    /// `127.0.0.1:port` (0 picks a free one) when a port is given. Loopback only, always.
    pub control: Option<ControlConfig>,
    /// The media service's limits (`media: {cache_mb, workers, queue}`): a phone wants a smaller
    /// cache than the default 256 MiB.
    pub media: plumcraft_media::Config,
    /// The log's level (`log: {level}`; `PLUMCRAFT_LOG` wins); none: as it is (`info`).
    pub log_level: Option<plumcraft_editor_core::log::Level>,
    /// Where the log file goes (`dirs.logs`, or `log: {dir}`): `plumcraft.log` there, bounded.
    pub log_dir: Option<PathBuf>,
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
            also: Vec::new(),
            auto: true,
            platform: Capabilities::NONE,
            control: None,
            media: media_config(None).unwrap_or_default(),
            log_level: None,
            log_dir: None,
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
            also: also(engines.and_then(|e| e.get("also")))?,
            auto: match engines.and_then(|e| e.get("auto")) {
                None | Some(Value::Null) => true,
                Some(Value::Bool(b)) => *b,
                Some(_) => return Err("config: engines.auto must be true or false".into()),
            },
            platform: Capabilities::declared(o.get("platform")),
            control: control_config(o.get("control"))?,
            media: media_config(o.get("media"))?,
            log_level: log_level(o.get("log"))?,
            log_dir: log_dir(o)?,
        })
    }

    /// The engines to start beside `first` (`engines.also`), each hosted its default way and
    /// checked as the first is; the first itself, or one named twice, once.
    fn also_targets(&self, first: EngineKind) -> Result<Vec<plumcraft_session::EngineTarget>, String> {
        let mut out: Vec<plumcraft_session::EngineTarget> = Vec::new();
        let mut seen = vec![first];
        for id in &self.also {
            let Some(kind) = EngineKind::from_id(id) else { return Err(format!("config: engines.also: {id:?} is no engine")) };
            if seen.contains(&kind) {
                continue;
            }
            seen.push(kind);
            let mode = kind.default_mode();
            if let Some(why) = self.platform.refuses(kind, mode) {
                return Err(format!("config: engines.also: the {} engine as {}: {why}", kind.id(), mode.id()));
            }
            out.push(plumcraft_session::EngineTarget::Choice { engine: kind.id().to_owned(), mode: mode.id().to_owned() });
        }
        Ok(out)
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

/// `engines.also: [engine id]`: none when absent or null; at most 8, each a known engine.
fn also(v: Option<&Value>) -> Result<Vec<String>, String> {
    let list = match v {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(a)) if a.len() <= 8 => a,
        Some(_) => return Err("config: engines.also must be a list of at most 8 engine ids".into()),
    };
    list.iter()
        .map(|e| match e.as_str().and_then(EngineKind::from_id) {
            Some(k) => Ok(k.id().to_owned()),
            None => Err(format!("config: engines.also: {e} is no engine")),
        })
        .collect()
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

/// `log: {level}`: the log's level, if given.
fn log_level(v: Option<&Value>) -> Result<Option<plumcraft_editor_core::log::Level>, String> {
    match v.and_then(|l| l.get("level")) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            plumcraft_editor_core::log::Level::parse(s).map(Some).ok_or_else(|| "config: log.level must be error, warn, info, debug or trace".into())
        }
        Some(_) => Err("config: log.level must be error, warn, info, debug or trace".into()),
    }
}

/// `dirs.logs` (or `log: {dir}`): where the log file goes, if given.
fn log_dir(o: &serde_json::Map<String, Value>) -> Result<Option<PathBuf>, String> {
    let v = o.get("dirs").and_then(|d| d.get("logs")).or_else(|| o.get("log").and_then(|l| l.get("dir")));
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(d)) if !d.trim().is_empty() && d.len() <= 4096 && !d.contains('\0') => Ok(Some(PathBuf::from(d))),
        Some(_) => Err("config: dirs.logs must be a folder's path".into()),
    }
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
    /// What changed of a document (`{doc, domain, …}`: its engine connected, it closed, its
    /// menus or its scene changed). A document's scene is here once at most: a newer change
    /// replaces the one not yet polled.
    changed: Vec<String>,
    /// The log has entries past what the shell was told of: the newest's seq, and the worst level
    /// among them (`log.tail` reads them).
    log: Option<(u64, plumcraft_editor_core::log::Level)>,
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
    /// Every document's engine connection, following switches (a value is read through the
    /// first that has it).
    conns: Mutex<Vec<Arc<dyn Connection>>>,
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
    /// Each document's hot-call snapshot (N2 slice 3), the document's own cell, which it swaps
    /// the moment a publish makes a new one (N2 slice 4: before, the core thread shared it at
    /// the end of its turn, after the replies); the hot calls take the `Arc` and read it with
    /// every lock let go, so the core thread never waits for a reader (section 9.4).
    snapshots: Mutex<BTreeMap<DocId, plumcraft_editor_core::geometry::SnapshotCell>>,
    /// Each document's drag under way (`ui.gesture`, N2 slice 4), the document's own cell, so
    /// `plum_gesture_move` sees a drag as soon as its `begin` has been decided.
    drags: Mutex<BTreeMap<DocId, plumcraft_editor_core::gesture::DragCell>>,
}

impl Shared {
    /// Document `doc`'s snapshot (0: the first document's), none before its scene is published.
    pub fn snapshot(&self, doc: u64) -> Option<Arc<Snapshot>> {
        let s = lock(&self.snapshots);
        let cell = if doc == 0 { s.values().next() } else { s.get(&doc) };
        cell.and_then(plumcraft_editor_core::geometry::SnapshotCell::get)
    }

    /// Document `doc`'s drag under way (0: the first document's).
    pub fn drag(&self, doc: u64) -> Option<Arc<plumcraft_editor_core::gesture::Drag>> {
        let d = lock(&self.drags);
        let cell = if doc == 0 { d.values().next() } else { d.get(&doc) };
        cell.and_then(plumcraft_editor_core::gesture::DragCell::get)
    }
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
        let (replies, events, dropped, asks, asks_ended, media, changed, log) = {
            let mut o = lock(&self.outbox);
            if o.replies.is_empty()
                && o.events.is_empty()
                && o.dropped == 0
                && o.asks.is_empty()
                && o.asks_ended.is_empty()
                && o.media.is_empty()
                && o.changed.is_empty()
                && o.log.is_none()
            {
                return None;
            }
            (
                std::mem::take(&mut o.replies),
                std::mem::take(&mut o.events),
                std::mem::take(&mut o.dropped),
                std::mem::take(&mut o.asks),
                std::mem::take(&mut o.asks_ended),
                std::mem::take(&mut o.media),
                std::mem::take(&mut o.changed),
                o.log.take(),
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
        if !changed.is_empty() {
            out.extend_from_slice(b"],\"changed\":[");
            join_into(&mut out, &changed);
        }
        out.push(b']');
        if let Some((seq, worst)) = log {
            out.extend_from_slice(format!(",\"log\":{{\"seq\":{seq},\"worst\":\"{}\"}}", worst.name()).as_bytes());
        }
        out.extend_from_slice(format!(",\"dropped\":{dropped}}}").as_bytes());
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
    /// A link or the engine worker has something waiting.
    Drain,
    /// The control channel queued a request.
    Control,
    /// The media service finished something.
    Media,
    /// What the shell measured, typed (`plum_geometry_set`): faces by nid, with the revision
    /// measured (none: not said) (N2 slice 3).
    Geometry {
        doc: u64,
        faces: Vec<(u64, Option<u64>, FaceGeometry)>,
    },
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
    /// Check the config, start the core thread and open the first document, and return at once:
    /// the engine connects on the document's engine worker (an engine compiled in shared by
    /// every document), and the feed says when (`changed`, domain `engine`). Requests for a
    /// document whose engine hasn't connected yet are held and answered once it has.
    pub fn open(config: &Config, wake: Option<WakeFn>, ctx: *mut c_void) -> Result<Core, String> {
        let (kind, mode) = config.engine_and_mode()?;
        let also = config.also_targets(kind)?;
        let auto = config.auto;
        if let Some(level) = config.log_level {
            plumcraft_editor_core::log::set_level(level);
        }
        if let Some(dir) = &config.log_dir
            && let Err(e) = plumcraft_editor_core::log::set_file(Some(dir))
        {
            // the core runs without a file rather than not at all
            plumcraft_editor_core::log::warn("ffi", &e);
        }
        // the demo is part of the program: read now, so a bad one is the config's error
        let demo = if config.demo { Some(demo_graph(kind)?) } else { None };
        let launcher = Launcher { caps: config.platform, store_root: config.store_root.clone(), ..Default::default() };
        let (tx, rx) = channel();
        let drain = tx.clone();
        let waker = link::Waker::new(move || {
            let _ = drain.send(Msg::Drain);
        });
        let store = Store::new(config.store_root.clone().unwrap_or_else(plumcraft_store::default_root));
        let shared = Arc::new(Shared {
            outbox: Mutex::default(),
            waker: Waker { f: wake, ctx: ctx as usize, pending: AtomicBool::new(false), sent: AtomicU64::new(0) },
            counters: Counters::default(),
            conns: Mutex::default(),
            store,
            refs: Mutex::default(),
            lent: Mutex::default(),
            media: MediaService::new(config.media.clone()),
            slow_resolve: AtomicU64::new(0),
            snapshots: Mutex::default(),
            drags: Mutex::default(),
        });
        // values are read through the documents' engine connections (following switches), or
        // from the store by handle; only ever on the media service's workers
        let weak = Arc::downgrade(&shared);
        shared.media.set_resolver(Some(Arc::new(move |r: &DataRef| {
            let s = weak.upgrade()?;
            let delay = s.slow_resolve.load(Ordering::Relaxed);
            if delay > 0 {
                std::thread::sleep(Duration::from_millis(delay.min(60_000)));
            }
            let conns = lock(&s.conns).clone();
            conns.iter().find_map(|c| c.resolve(r)).or_else(|| r.handle.as_ref().and_then(|h| s.store.map_handle(h).or_else(|| s.store.read_handle(h))))
        })));
        let to_core = tx.clone();
        shared.media.set_wake(Some(Arc::new(move || {
            let _ = to_core.send(Msg::Media);
        })));
        let (control_server, control_rx, control_token_file) = match &config.control {
            None => (None, None, None),
            Some(c) => {
                let token = server::server_token(c.token_file.as_deref()).map_err(|e| format!("control token: {e}"))?;
                let to_core = tx.clone();
                let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                    let _ = to_core.send(Msg::Control);
                });
                let (srv, rx) = server::Server::start(c.port, token.clone(), wake).map_err(|e| e.to_string())?;
                // the token is printed on stderr, as the desktop editor's --control prints it, and
                // never kept in the log
                plumcraft_editor_core::log::redact(&token);
                let say = |line: &str| plumcraft_editor_core::log::say(plumcraft_editor_core::log::Level::Info, "ffi", line);
                say(&format!("plumcraft: control channel on 127.0.0.1:{}", srv.port()));
                match &c.token_file {
                    Some(p) => say(&format!("plumcraft: control token in {}", p.display())),
                    None => say(&format!("plumcraft: control token: {token}")),
                };
                (Some(srv), Some(rx), c.token_file.clone())
            }
        };
        let thread_shared = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("plumcraft-ffi-core".into())
            .spawn(move || {
                // a document's parts live on this thread only
                let mut worker = Worker {
                    shared: thread_shared,
                    docs: BTreeMap::new(),
                    waiters: Vec::new(),
                    kind,
                    mode,
                    also,
                    auto,
                    launcher,
                    engines: SharedEngines::default(),
                    waker,
                    control_rx,
                    control_server,
                    control_token_file,
                    log_seen: plumcraft_editor_core::log::last_seq(),
                    menus_at: Instant::now(),
                    scene_at: Instant::now(),
                };
                worker.open_doc(demo.map_or(Opening::Nothing, |g| Opening::Edit("Open Demo".into(), g)));
                worker.run(&rx);
            })
            .map_err(|e| e.to_string())?;
        let thread_id = Some(thread.thread().id());
        Ok(Core { tx, shared, next_id: AtomicU64::new(1), thread: Mutex::new(Some(thread)), thread_id })
    }

    /// Queue a request; its id, which its reply carries. `None` once the core has stopped.
    pub fn request(&self, body: Vec<u8>) -> Option<u64> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.tx.send(Msg::Request { id, body }).ok().map(|()| id)
    }

    /// Queue what the shell measured (`plum_geometry_set`) for document `doc` (0: the first).
    /// False once the core has stopped.
    pub fn geometry(&self, doc: u64, faces: Vec<(u64, Option<u64>, FaceGeometry)>) -> bool {
        self.tx.send(Msg::Geometry { doc, faces }).is_ok()
    }

    /// Stop the core thread and wait for it, unless this is the core thread itself (a shell that
    /// closes from inside its wake): then it stops on its own after this message. The thread
    /// closes its documents without waiting for their engines (each document's worker stops
    /// them), so this waits for no engine.
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

/// The workflow at `path`, read before its document opens to see which engines it uses: `None`
/// for one that isn't a workflow read whole (a bundle, an application's own file, one too big or
/// unreadable), which then opens on the config's engine and starts what it needs once open.
fn peek_workflow(path: &std::path::Path) -> Option<Graph> {
    /// Larger than any workflow; `graph.open` reads it again in full.
    const MOST: u64 = 64 << 20;
    if std::fs::metadata(path).ok()?.len() > MOST {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    Graph::from_json(&text).ok()
}

/// The engine's demo graph (an empty one for an engine without).
fn demo_graph(kind: EngineKind) -> Result<Graph, String> {
    let demo = kind.own_demo()?.unwrap_or_else(|| EMPTY_WORKFLOW.to_owned());
    Graph::from_json(&demo).map_err(|e| e.to_string())
}

/// The engine's demo graph (an empty one for an engine without), as one undo step.
fn load_demo(s: &mut Session, kind: EngineKind) -> Result<(), String> {
    let demo = demo_graph(kind)?;
    s.edit("Open Demo", |g, _| {
        *g = demo;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// A `spike.waitRun` waiting for its document's run to end.
struct Waiter {
    id: u64,
    doc: DocId,
    tag: Option<Value>,
    deadline: Instant,
}

/// A request for a document whose engine hasn't connected yet, answered once it has.
enum Held {
    Request { id: u64, tag: Option<Value>, method: String, params: Value },
    Control(ControlRequest),
}

/// The most requests held for one document while its engine connects.
const MAX_HELD: usize = 256;

/// The longest a request adding a node waits for the engine that offers its kind to connect
/// (decision 8C: engines start as the graph comes to use them).
const ENGINE_WAIT: Duration = Duration::from_secs(30);

/// The node kind a request adds (`node.add`, `node.addWired` through `command.run`), if any.
fn added_kind(h: &Held) -> Option<String> {
    let (method, params) = match h {
        Held::Request { method, params, .. } => (method.as_str(), params),
        Held::Control(req) => (req.method.as_str(), &req.params),
    };
    if !matches!(method, "command.run" | "engine.execute") {
        return None;
    }
    let command = params.get("command").and_then(Value::as_str)?;
    if !matches!(command, "node.add" | "node.addWired") {
        return None;
    }
    params.get("params")?.get("kind")?.as_str().map(str::to_owned)
}

/// A document of this core, and what waits for its engine.
struct Doc {
    doc: Document,
    /// Its first engine has connected (or failed to), and what it opens with is open.
    ready: bool,
    held: Vec<Held>,
    /// A request adding a node of this kind waits (until this instant at most) for the engine
    /// that offers it, which it asked for; the requests after it wait in `held` behind it.
    waiting: Option<(String, Instant)>,
    /// The `engine` domain's generation, for the feed's `changed`.
    generation: u64,
    /// The menu model's generation last told the shell (`changed`, domain `menus`).
    menus_sent: u64,
    /// The scene's generation last told the shell (`changed`, domain `scene`).
    scene_sent: u64,
    /// A shell has asked for the menus (`menu.get`), so it hears when they change; one that never
    /// asks costs the core nothing.
    menus_watched: bool,
    /// The document's state last told the shell (`changed`, domain `document`: title, path,
    /// dirty, undo and redo, last error; N3 slice 4), and its generation.
    told: Value,
    told_gen: u64,
}

/// The core thread's state: its documents, by id, and what they share.
struct Worker {
    shared: Arc<Shared>,
    docs: BTreeMap<DocId, Doc>,
    waiters: Vec<Waiter>,
    /// The engine the config names, and its hosting: every document starts on it.
    kind: EngineKind,
    mode: Mode,
    /// The engines started beside it in every document (`engines.also`).
    also: Vec<plumcraft_session::EngineTarget>,
    /// Start only the engines a document's graph uses (`engines.auto`).
    auto: bool,
    /// Starts the engines (on each document's worker).
    launcher: Launcher,
    /// The engines compiled in, one of each for every document (decision 11B).
    engines: SharedEngines,
    /// What new links and the workers wake: a `Drain` to this thread.
    waker: link::Waker,
    /// The control channel's requests (declared before its server, so they drop first and a
    /// connection waiting on one is answered at once).
    control_rx: Option<Receiver<ControlRequest>>,
    /// The control server; dropping it (the core closing) closes the port and its connections.
    control_server: Option<server::Server>,
    control_token_file: Option<PathBuf>,
    /// The newest log entry the shell was told of (the feed's `log`).
    log_seen: u64,
    /// When the documents' menus were last looked at (`note_menus`).
    menus_at: Instant,
    /// When the documents' scenes were last looked at (`note_scene`).
    scene_at: Instant,
}

/// A refusal with the contract's `code` (`docs/shell-interface.md` 6.1).
fn coded(code: &str, message: impl Into<String>) -> Value {
    let mut v = control::err(message);
    v["code"] = json!(code);
    v
}

impl Worker {
    /// Open a document on the config's engine, which connects on its worker; `opening` opens once
    /// it has. Its id.
    fn open_doc(&mut self, opening: Opening) -> DocId {
        let mut session = Session::new(plumcraft_proto::catalogue::SchemaReply::default());
        // engine.* commands switch among what this platform can start, and attach
        session.engines.choices = self.launcher.choices();
        session.engines.offered = plumcraft_editor_core::engines::offered(&session.engines.choices);
        session.engines.auto = self.auto;
        let (target, extras) = self.engines_for(&session, &opening);
        session.engines.current = Some(target.clone());
        session.engines.can_attach = true;
        let mut doc = Document::with_media(session, self.shared.media.clone(), false);
        doc.caps = self.launcher.caps;
        doc.set_waker(Some(self.waker.clone()));
        let launcher = self.launcher.clone();
        doc.starter = Some(shared_starter(self.engines.clone(), move |e: &str, m: &str, w: Option<link::Waker>| launcher.connect_ids(e, m, w)));
        let shared = Arc::clone(&self.shared);
        doc.set_asks_sink(Some(Box::new(move |raised, ended| shared.push_asks(raised, ended))));
        doc.start_engines(target, extras, opening);
        let id = doc.id;
        lock(&self.shared.drags).insert(id, doc.drag.clone());
        lock(&self.shared.snapshots).insert(id, doc.snapshot_out.clone());
        self.docs.insert(
            id,
            Doc {
                doc,
                ready: false,
                held: Vec::new(),
                waiting: None,
                generation: 0,
                menus_sent: 0,
                scene_sent: 0,
                menus_watched: false,
                told: Value::Null,
                told_gen: 0,
            },
        );
        id
    }

    /// The engines a new document starts on, the first and those beside it. With `engines.auto`
    /// (decision 8C at N3's exit), the engines its graph uses ([`plumcraft_session::engine_cmds::graph_engines`]
    /// over the file, read here before it opens, or the demo), those this platform can start,
    /// the config's own engine first when it is one; the config's engine alone for a graph that
    /// uses none, or a file that isn't a workflow read whole (a bundle, an application's file:
    /// picking engines automatically starts what it needs once it has opened). Then
    /// `engines.also`, always.
    fn engines_for(&self, session: &Session, opening: &Opening) -> (plumcraft_session::EngineTarget, Vec<plumcraft_session::EngineTarget>) {
        use plumcraft_session::EngineTarget;
        let first = EngineTarget::Choice { engine: self.kind.id(), mode: self.mode.id().to_owned() };
        let mut extras: Vec<EngineTarget> = Vec::new();
        let mut used: Vec<String> = Vec::new();
        if self.auto {
            let graph = match opening {
                Opening::Nothing => None,
                Opening::Graph(g) | Opening::Edit(_, g) => Some(g.clone()),
                Opening::Path(p) => peek_workflow(p),
            };
            let own = self.kind.id();
            used = graph
                .map(|g| plumcraft_session::engine_cmds::graph_engines(&g, &session.engines.offered, &[own.as_str()]))
                .unwrap_or_default()
                .into_iter()
                .filter(|e| *e == own || session.engines.best(e).is_some())
                .collect();
        }
        // the config's own engine, hosted as the config says, unless the graph uses others only
        let first = match used.first() {
            Some(e) if !used.contains(&self.kind.id()) => {
                let mode = session.engines.best(e).map_or_else(|| Mode::InProc.id().to_owned(), |c| c.mode.clone());
                EngineTarget::Choice { engine: e.clone(), mode }
            }
            _ => first,
        };
        let first_id = match &first {
            EngineTarget::Choice { engine, .. } => engine.clone(),
            EngineTarget::Attach { .. } => String::new(),
        };
        for e in used.iter().filter(|e| **e != first_id) {
            if let Some(c) = session.engines.best(e) {
                extras.push(EngineTarget::Choice { engine: e.clone(), mode: c.mode.clone() });
            }
        }
        for t in &self.also {
            let dup = matches!(t, EngineTarget::Choice { engine, .. } if *engine == first_id || extras.iter().any(|x| matches!(x, EngineTarget::Choice { engine: x, .. } if x == engine)));
            if !dup {
                extras.push(t.clone());
            }
        }
        (first, extras)
    }

    /// The document a request names (`doc`), or the first one open when it names none.
    fn target(&self, doc: Option<&Value>) -> Result<DocId, Value> {
        match doc {
            None | Some(Value::Null) => self.docs.keys().next().copied().ok_or_else(|| coded("unknown_doc", "no document is open")),
            Some(d) => {
                let id = d.as_u64().ok_or_else(|| coded("bad_params", "doc must be a document's number (document.list)"))?;
                if self.docs.contains_key(&id) { Ok(id) } else { Err(coded("unknown_doc", format!("no document {id} is open"))) }
            }
        }
    }

    fn run(mut self, rx: &Receiver<Msg>) {
        // a look at the scenes was put off (a message came less than SCENE_EVERY after the
        // last): wake for it when SCENE_EVERY is up, not at the next tick (N2 slice 3: an edit or
        // a geometry report reached the feed and the hot calls up to a tick, 50 ms, late)
        let mut scene_owed = false;
        loop {
            let wait = if scene_owed { SCENE_EVERY.saturating_sub(self.scene_at.elapsed()).max(Duration::from_millis(1)).min(TICK) } else { TICK };
            let msg = match rx.recv_timeout(wait) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            // nothing came for a tick: the core is idle
            let idle = msg.is_none();
            match msg {
                Some(Msg::Stop) => break,
                Some(Msg::Request { id, body }) => self.request(id, &body),
                Some(Msg::Media) => self.shared.push_media(),
                Some(Msg::Geometry { doc, faces }) => self.geometry(doc, faces),
                Some(Msg::Drain | Msg::Control) | None => {}
            }
            self.control_requests();
            self.turn_all();
            self.answer_waiters();
            self.note_log();
            // the menus are looked at when the core is idle (at most a tick after a change), and at
            // least every second when it never is, never in the middle of a stream of requests
            if idle || self.menus_at.elapsed() >= MENUS_EVERY {
                self.menus_at = Instant::now();
                let ids: Vec<DocId> = self.docs.keys().copied().collect();
                for id in ids {
                    self.note_menus(id);
                }
            }
            // the scenes at most once a display frame while busy (run progress coalesced here),
            // and whenever the core is idle
            if idle || self.scene_at.elapsed() >= SCENE_EVERY {
                self.scene_at = Instant::now();
                scene_owed = false;
                let ids: Vec<DocId> = self.docs.keys().copied().collect();
                for id in ids {
                    self.note_scene(id);
                }
            } else {
                scene_owed = true;
                // A publish this turn (a `scene.get`) stands for this turn only. Kept to a later
                // turn, the look after an edit there took the edit's scene to be the one already
                // published and saw nothing new, so the edit reached the feed only at the next
                // idle look, a tick (50 ms) and a build later (N3 slice 2: a shell that fetches
                // `scene.get {since}` on each change, as the Swift kit does, met it on every edit).
                for d in self.docs.values_mut() {
                    d.doc.scene.take_fresh();
                }
            }
            let o = lock(&self.shared.outbox);
            let any = !o.replies.is_empty()
                || !o.events.is_empty()
                || !o.asks.is_empty()
                || !o.asks_ended.is_empty()
                || !o.media.is_empty()
                || !o.changed.is_empty()
                || o.log.is_some();
            drop(o);
            if any {
                self.shared.waker.wake();
            }
        }
        // the documents close: runs cancelled and engines stopped on their workers, not here
        for (_, mut d) in std::mem::take(&mut self.docs) {
            for h in std::mem::take(&mut d.held) {
                self.refuse(h, "the core closed");
            }
            d.doc.close();
        }
        lock(&self.shared.conns).clear();
    }

    /// The log has entries the shell hasn't been told of: say so in the feed (`log: {seq, worst}`).
    fn note_log(&mut self) {
        use plumcraft_editor_core::log;
        let seq = log::last_seq();
        if seq <= self.log_seen {
            return;
        }
        let worst = log::worst_after(self.log_seen).unwrap_or(log::Level::Trace);
        self.log_seen = seq;
        let mut o = lock(&self.shared.outbox);
        o.log = Some(match o.log {
            Some((_, w)) => (seq, w.min(worst)),
            None => (seq, worst),
        });
    }

    /// The document's own state, noted once its engine is there: the feed says when its title,
    /// path, dirty state, undo or redo step or last error changed (`changed`, domain `document`,
    /// with them all and a generation), so a native shell keeps its window's title, edited dot and
    /// Edit menu without asking (contract 7.2's `document` domain; N3 slice 4).
    fn note_document(&mut self, id: DocId) {
        let Some(d) = self.docs.get_mut(&id) else { return };
        if !d.ready {
            return;
        }
        let s = &d.doc.session;
        let now = json!({"title": control::window_title(s, &Shell::view(&d.doc)), "document": control::document_name(s, &d.doc.document),
                         "path": s.path(), "dirty": s.is_dirty(), "undo": s.undo_label(), "redo": s.redo_label(), "lastError": d.doc.last_error});
        if now == d.told {
            return;
        }
        d.told_gen += 1;
        let mut c = now.clone();
        d.told = now;
        c["doc"] = json!(id);
        c["domain"] = json!("document");
        c["gen"] = json!(d.told_gen);
        let mut o = lock(&self.shared.outbox);
        // a change of it not yet polled is replaced (7.3: each domain once a poll, the latest);
        // `closing` changes are kept
        o.changed
            .retain(|x| serde_json::from_str::<Value>(x).map_or(true, |v| !(v["doc"] == json!(id) && v["domain"] == "document" && v.get("gen").is_some())));
        o.changed.push(c.to_string());
    }

    /// The document's menu bar, noted: the feed says when it changed (`changed`, domain `menus`),
    /// so a shell rebuilds its native menus then and not on every wake.
    fn note_menus(&mut self, id: DocId) {
        let Some(d) = self.docs.get_mut(&id) else { return };
        if !d.ready || !d.menus_watched {
            return;
        }
        let facts = control::Shell::menu_facts(&d.doc);
        let model = serde_json::to_value(plumcraft_editor_core::menu::bar(&d.doc.session, &facts)).unwrap_or(Value::Null);
        let generation = d.doc.menus.note(&model);
        if generation != d.menus_sent {
            d.menus_sent = generation;
            lock(&self.shared.outbox).changed.push(json!({"doc": id, "domain": "menus", "gen": generation}).to_string());
        }
    }

    /// The document's scene, noted once a shell has asked for it (`scene.get`): the feed says
    /// when it changed (`changed`, domain `scene`), replacing a change of it not yet polled, so a
    /// shell fetches what changed (`scene.get {since}`) at most once a poll.
    fn note_scene(&mut self, id: DocId) {
        use plumcraft_editor_core::control::Shell;
        let Some(d) = self.docs.get_mut(&id) else { return };
        if !d.ready || !d.doc.scene.watched {
            return;
        }
        // a request this turn published it (`scene.get`): not built twice a turn (N2 slice 3:
        // a 2,000-node scene took longer than SCENE_EVERY to build, so every `scene.get` was
        // built again here before its reply went out, doubling it); anything that changed since
        // is seen at the next look
        let generation = if d.doc.scene.take_fresh() {
            d.doc.scene.generation()
        } else {
            let Some(scene) = d.doc.scene() else { return };
            let g = d.doc.publish_scene(scene);
            d.doc.scene.take_fresh();
            g
        };
        if generation != d.scene_sent {
            d.scene_sent = generation;
            let mut o = lock(&self.shared.outbox);
            let mine = format!("{{\"doc\":{id},\"domain\":\"scene\"");
            o.changed.retain(|c| !c.starts_with(&mine));
            o.changed.push(json!({"doc": id, "domain": "scene", "gen": generation}).to_string());
        }
    }

    /// What the shell measured, typed (`plum_geometry_set`): faces named by nid, as the published
    /// scene numbers them (a nid it doesn't know is skipped), kept as `geometry.set` keeps them.
    fn geometry(&mut self, doc: u64, faces: Vec<(u64, Option<u64>, FaceGeometry)>) {
        let id = if doc == 0 { self.docs.keys().next().copied() } else { Some(doc) };
        let Some(d) = id.and_then(|id| self.docs.get_mut(&id)) else { return };
        let reports: Vec<Report> = {
            let ids = d.doc.scene.face_ids();
            faces.into_iter().filter_map(|(nid, rev, geometry)| ids.get(&nid).map(|face| Report { face: (*face).to_owned(), rev, geometry })).collect()
        };
        d.doc.apply_geometry(reports);
    }

    /// Every document's turn: what its engine and worker brought back, its events forwarded, what
    /// it queued sent; held requests answered once its engine is there; closed documents gone.
    fn turn_all(&mut self) {
        let ids: Vec<DocId> = self.docs.keys().copied().collect();
        for id in ids {
            let Some(d) = self.docs.get_mut(&id) else { continue };
            let t = document::turn(&mut d.doc);
            if !t.events.is_empty() {
                self.forward(id, &t.events);
            }
            let Some(d) = self.docs.get_mut(&id) else { continue };
            if t.ready || !t.outcomes.is_empty() {
                d.generation += 1;
                let changed = json!({"doc": id, "domain": "engine", "gen": d.generation, "connected": d.doc.session.connected,
                                     "label": switchboard::engine_label(&d.doc.session, d.doc.link.as_ref(), &d.doc.engine), "ready": t.ready || d.ready});
                lock(&self.shared.outbox).changed.push(changed.to_string());
            }
            if t.ready && !d.ready {
                d.ready = true;
                self.release(id);
            }
            // a request waiting for the engine of the node it adds: on once it has connected
            // (or failed to), or after ENGINE_WAIT
            if let Some(d) = self.docs.get_mut(&id)
                && let Some((kind, until)) = &d.waiting
                && (d.doc.session.catalog().node(kind).is_some() || !d.doc.engine_busy() || Instant::now() >= *until)
            {
                d.waiting = None;
                self.release(id);
            }
            if let Some(d) = self.docs.get_mut(&id) {
                control::tick(&mut d.doc);
                if d.doc.close_asked {
                    self.close_doc(id);
                    continue;
                }
            }
            self.note_document(id);
        }
        let conns: Vec<Arc<dyn Connection>> = self.docs.values().filter_map(|d| d.doc.link.as_ref().map(|l| Arc::clone(&l.conn))).collect();
        *lock(&self.shared.conns) = conns;
    }

    /// A document's engine events, to the shell as they came (each naming its document), their
    /// DataRefs remembered. A document linked to an engine others share sees their runs' events
    /// too: only its own runs' go out, with the engine's own (no run).
    fn forward(&self, id: DocId, events: &[Envelope]) {
        let Some(d) = self.docs.get(&id) else { return };
        let mut out = Vec::with_capacity(events.len());
        for e in events {
            if e.run_id.as_deref().is_some_and(|r| !d.doc.owns_run(r)) {
                continue;
            }
            let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
            if matches!(e.event, Event::RunReply(_) | Event::Status(_)) {
                let mut refs = lock(&self.shared.refs);
                collect_refs(&v, &mut refs);
            }
            if let Some(o) = v.as_object_mut() {
                o.insert("doc".into(), json!(id));
            }
            out.push(v.to_string());
        }
        self.shared.push_events(out);
    }

    /// Close document `id` now: its asks withdrawn (the feed says so), what waits on it answered,
    /// its engine stopped on its worker.
    fn close_doc(&mut self, id: DocId) {
        let Some(mut d) = self.docs.remove(&id) else { return };
        lock(&self.shared.drags).remove(&id);
        lock(&self.shared.snapshots).remove(&id);
        for h in std::mem::take(&mut d.held) {
            self.refuse(h, "the document closed");
        }
        let (gone, keep): (Vec<Waiter>, Vec<Waiter>) = std::mem::take(&mut self.waiters).into_iter().partition(|w| w.doc == id);
        self.waiters = keep;
        for w in gone {
            self.reply(w.id, w.tag.as_ref(), Err("the document closed".into()));
        }
        d.doc.close();
        lock(&self.shared.outbox).changed.push(json!({"doc": id, "domain": "document", "closing": true}).to_string());
    }

    fn refuse(&self, h: Held, why: &str) {
        match h {
            Held::Request { id, tag, .. } => self.reply(id, tag.as_ref(), Err(why.into())),
            Held::Control(req) => {
                let _ = req.reply.send(control::err(why));
            }
        }
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

    /// The control channel's requests, through the same dispatcher, for the first document (the
    /// control protocol names none), held while its engine connects.
    fn control_requests(&mut self) {
        let Some(rx) = &self.control_rx else { return };
        let requests: Vec<ControlRequest> = rx.try_iter().collect();
        for req in requests {
            if req.expired() {
                let _ = req.reply.send(control::err("timeout: the request expired before it ran"));
                continue;
            }
            let id = match self.target(None) {
                Ok(id) => id,
                Err(e) => {
                    let _ = req.reply.send(e);
                    continue;
                }
            };
            self.hold_or(id, Held::Control(req));
        }
    }

    /// Run `h` for document `id`, or hold it while its engine connects, or while an engine a
    /// request before it is waiting for connects.
    fn hold_or(&mut self, id: DocId, h: Held) {
        let Some(d) = self.docs.get_mut(&id) else { return self.refuse(h, "the document closed") };
        if !d.ready || d.waiting.is_some() || !d.held.is_empty() {
            if d.held.len() >= MAX_HELD {
                return self.refuse(h, "busy: the document's engine is still connecting");
            }
            d.held.push(h);
            return;
        }
        if self.waits_for_engine(id, &h) {
            if let Some(d) = self.docs.get_mut(&id) {
                d.held.push(h);
            }
            return;
        }
        self.replay(id, h);
    }

    /// Does `h` add a node of a kind that only an engine not connected offers, one this platform
    /// can start? Then, picking engines automatically, that engine is asked for (decision 8C: a
    /// graph that comes to use another engine starts it; N4's palette will add such kinds) and
    /// `h` waits for it: true. A kind nothing here offers runs at once, and is refused.
    fn waits_for_engine(&mut self, id: DocId, h: &Held) -> bool {
        let Some(kind) = added_kind(h) else { return false };
        let Some(d) = self.docs.get_mut(&id) else { return false };
        if plumcraft_session::engine_cmds::engine_for_kind(&d.doc.session, &kind).is_none() {
            return false;
        }
        let asked = plumcraft_session::engine_cmds::start_for_kind(&mut d.doc.session, &kind).is_some();
        if !asked && !d.doc.engine_busy() {
            return false;
        }
        d.waiting = Some((kind, Instant::now() + ENGINE_WAIT));
        true
    }

    /// Run what document `id` holds, in order, while nothing makes it wait; then a turn, so what
    /// they queued is sent.
    fn release(&mut self, id: DocId) {
        loop {
            let Some(d) = self.docs.get_mut(&id) else { return };
            if !d.ready || d.waiting.is_some() || d.held.is_empty() {
                break;
            }
            let h = d.held.remove(0);
            if self.waits_for_engine(id, &h) {
                if let Some(d) = self.docs.get_mut(&id) {
                    d.held.insert(0, h);
                }
                break;
            }
            self.replay(id, h);
        }
        if let Some(d) = self.docs.get_mut(&id) {
            let _ = document::turn(&mut d.doc);
        }
    }

    /// A request for a ready document.
    fn replay(&mut self, id: DocId, h: Held) {
        match h {
            Held::Request { id: rid, tag, method, params } => self.doc_request(id, rid, tag, &method, &params),
            Held::Control(req) => {
                if req.expired() {
                    let _ = req.reply.send(control::err("timeout: the request expired before it ran"));
                    return;
                }
                let Some(d) = self.docs.get_mut(&id) else { return };
                if req.method == "menu.get" {
                    d.menus_watched = true;
                }
                let reply = req.reply.clone();
                control::handle(
                    &mut d.doc,
                    &req.method,
                    &req.params,
                    Box::new(move |v: Value| {
                        let _ = reply.send(v);
                    }),
                );
            }
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
            "ping" => Ok(json!({"pong": true})),
            // where the control channel listens, if it does
            "core.control" => Ok(match &self.control_server {
                Some(srv) => json!({"port": srv.port(), "tokenFile": self.control_token_file}),
                None => json!({"port": null}),
            }),
            "spike.stats" => Ok(self.stats()),
            "spike.publish" => self.publish(&params),
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
            "document.open" => self.document_open(&params),
            "document.list" => Ok(self.document_list()),
            "document.close" => {
                // the document named in params (as the contract has it), or in the request
                let named = params.get("doc").or_else(|| v.get("doc"));
                match self.target(named) {
                    Ok(doc) => self.document_close(doc, &params),
                    Err(e) => return self.shared.push_reply(framed(id, tag.as_ref(), e)),
                }
            }
            // everything else acts on a document: the one named, or the first
            _ => {
                let doc = match self.target(v.get("doc")) {
                    Ok(d) => d,
                    Err(e) => return self.shared.push_reply(framed(id, tag.as_ref(), e)),
                };
                return self.hold_or(doc, Held::Request { id, tag, method: method.to_owned(), params });
            }
        };
        self.reply(id, tag.as_ref(), r);
    }

    /// A request for document `doc`, whose engine has connected (or failed to).
    fn doc_request(&mut self, doc: DocId, id: u64, tag: Option<Value>, method: &str, params: &Value) {
        let kind = self.kind;
        let Some(d) = self.docs.get_mut(&doc) else { return self.reply(id, tag.as_ref(), Err("the document closed".into())) };
        let r = match method {
            // `command.run run.start`, shorter
            "run" => document::execute(&mut d.doc, "run.start", params),
            "spike.waitRun" => {
                let ms = params.get("timeout_ms").and_then(Value::as_u64).unwrap_or(25_000).min(600_000);
                self.waiters.push(Waiter { id, doc, tag, deadline: Instant::now() + Duration::from_millis(ms) });
                return;
            }
            "spike.demo" => load_demo(&mut d.doc.session, kind).map(|()| json!({"nodes": d.doc.session.graph().nodes.len()})),
            "spike.scene" => scene(&mut d.doc.session, params),
            // every control-channel method, as the egui editor answers it: a command that
            // switches engines replies once switched, and this thread doesn't wait for it
            _ => {
                let reply = self.replier(id, tag);
                if let Some(d) = self.docs.get_mut(&doc) {
                    if method == "menu.get" {
                        d.menus_watched = true;
                    }
                    // `ui.invoke {by: "person"}`: the shell invokes for the person at it (its menu,
                    // its toolbar), so what the command asks is the shell's to show (N3 slice 4).
                    // Only a shell's own requests may say so; the control channel's are a script's
                    // and refuse the field as unknown.
                    if method == "ui.invoke" && params.get("by").is_some() {
                        let mut p = params.clone();
                        let by = p.as_object_mut().and_then(|o| o.remove("by"));
                        let person = match by.as_ref().and_then(Value::as_str) {
                            Some("person") => true,
                            Some("script") => false,
                            _ => return (reply)(control::err("by must be \"person\" or \"script\"")),
                        };
                        d.doc.invoke_by = if person { By::Person } else { By::Script };
                        control::handle(&mut d.doc, method, &p, reply);
                        d.doc.invoke_by = By::Script;
                        return;
                    }
                    control::handle(&mut d.doc, method, params, reply);
                }
                return;
            }
        };
        self.reply(id, tag.as_ref(), r);
    }

    /// `document.open {demo?, path?}`: a new document on the config's engine (one compiled in is
    /// shared), replying at once with its id; requests for it are held until its engine is there.
    fn document_open(&mut self, p: &Value) -> Result<Value, String> {
        control::check_fields(p, &["demo", "path"])?;
        let demo = match p.get("demo") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err("demo must be a boolean".into()),
        };
        let path = match p.get("path") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if !s.trim().is_empty() && s.len() <= 4096 && !s.contains('\0') => Some(PathBuf::from(s)),
            Some(_) => return Err("path must be a workflow's file path".into()),
        };
        let opening = match (path, demo) {
            (Some(_), true) => return Err("give path or demo, not both".into()),
            (Some(path), false) => Opening::Path(path),
            (None, true) => Opening::Edit("Open Demo".into(), demo_graph(self.kind)?),
            (None, false) => Opening::Nothing,
        };
        let id = self.open_doc(opening);
        let title = self.docs.get(&id).map(|d| control::window_title(&d.doc.session, &Shell::view(&d.doc))).unwrap_or_default();
        Ok(json!({"doc": id, "title": title}))
    }

    /// `document.list {}`: every open document.
    fn document_list(&self) -> Value {
        let first = self.docs.keys().next().copied();
        let docs: Vec<Value> = self
            .docs
            .iter()
            .map(|(id, d)| {
                let s = &d.doc.session;
                json!({"doc": id, "title": control::window_title(s, &Shell::view(&d.doc)), "path": s.path(), "dirty": s.is_dirty(),
                       "ready": d.ready, "first": Some(*id) == first,
                       "engine": {"connected": s.connected, "pending": d.doc.engine_busy(), "label": switchboard::engine_label(s, d.doc.link.as_ref(), &d.doc.engine)}})
            })
            .collect();
        json!({"docs": docs})
    }

    /// `document.close {doc, discard?}`: closed at once when nothing is unsaved or `discard` is
    /// true (`{closing: true}`); else the unsaved-changes prompt is asked (`{closing: false, ask}`)
    /// and the document closes once it is answered Discard, or saved.
    fn document_close(&mut self, doc: DocId, p: &Value) -> Result<Value, String> {
        control::check_fields(p, &["doc", "discard"])?;
        let discard = match p.get("discard") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err("discard must be a boolean".into()),
        };
        let Some(d) = self.docs.get_mut(&doc) else { return Err(format!("no document {doc} is open")) };
        if discard {
            d.doc.close_asked = true;
        } else {
            invoke::quit(&mut d.doc, By::Script);
            d.doc.feed_asks();
        }
        if d.doc.close_asked {
            self.close_doc(doc);
            return Ok(json!({"closing": true}));
        }
        let ask = invoke::open_prompt(&d.doc.asks).map(|(id, _)| id);
        Ok(json!({"closing": false, "ask": ask}))
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

    fn answer_waiters(&mut self) {
        if self.waiters.is_empty() {
            return;
        }
        let now = Instant::now();
        let idle = |d: Option<&Doc>| d.is_none_or(|d| !d.doc.session.run.running && d.doc.session.run.outbox_is_empty());
        let (done, waiting): (Vec<Waiter>, Vec<Waiter>) =
            std::mem::take(&mut self.waiters).into_iter().partition(|w| idle(self.docs.get(&w.doc)) || now >= w.deadline);
        self.waiters = waiting;
        for w in done {
            let r = match self.docs.get(&w.doc) {
                Some(d) if idle(Some(d)) => Ok(run_summary(&d.doc.session)),
                Some(_) => Err("timeout: the run is still going".into()),
                None => Err("the document closed".into()),
            };
            self.reply(w.id, w.tag.as_ref(), r);
        }
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

fn run_summary(s: &Session) -> Value {
    let run = &s.run;
    let nodes: BTreeMap<&str, Value> =
        run.nodes.iter().map(|(id, n)| (id.as_str(), json!({"state": n.state.map(|s| format!("{s:?}").to_lowercase()), "ms": n.ms}))).collect();
    let watch: Vec<Value> = run.watched.iter().map(|d| serde_json::to_value(d).unwrap_or(Value::Null)).collect();
    json!({"running": run.running, "runId": run.run_id, "generation": run.generation, "runError": run.run_error, "nodes": nodes, "watch": watch})
}

/// `spike.scene {nodes, wires}`: a large graph in one undo step, of text nodes that a run could
/// compute (Join nodes take two wires, Upper case one), wired from earlier nodes only.
fn scene(s: &mut Session, p: &Value) -> Result<Value, String> {
    let n = p.get("nodes").and_then(Value::as_u64).unwrap_or(500).clamp(2, 20_000) as usize;
    let want = p.get("wires").and_then(Value::as_u64).unwrap_or(800) as usize;
    let cat = s.catalog().clone();
    let def = |k: &str| cat.node(k).cloned().ok_or_else(|| format!("no node type {k}"));
    let (lit, join, upper) = (def("text.literal")?, def("text.join")?, def("text.upper")?);
    // joins take two wires each, the rest one: enough joins to reach `want`
    let inputs_wanted = want.min(2 * (n - 1));
    let joins = inputs_wanted.saturating_sub(n - 1).min(n - 1);
    s.edit("Scene", move |g, cat| {
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
    let g = s.graph();
    Ok(json!({"nodes": g.nodes.len(), "wires": g.wires.len()}))
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
