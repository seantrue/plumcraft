//! The media service: a value reference in, plain data out, and the asker never waits.
//!
//! A shell (or the core on its behalf) asks for a value by its [`DataRef`] and what it wants
//! made of it ([`Want`]: its bytes, an engine's rgba8 rendition as a picture, a waveform's peaks,
//! a picture file decoded and shrunk, an SVG drawn, a picture file's own numbers). [`MediaService::get`]
//! answers at once with a [`Fetch`]:
//!
//! - [`Fetch::Ready`]: here it is ([`Made`]);
//! - [`Fetch::Pending`]: it is being read and made on a worker; ask again after the next wake;
//! - [`Fetch::Missing`]: its bytes are gone (reaped; a rerun publishes them again), or there is no
//!   engine to read them through;
//! - [`Fetch::Failed`]: they were read and aren't what they claim (hostile input), with why.
//!
//! **Workers.** The work (the engine connection's `resolve`, which for an HTTP engine is a
//! network fetch, then decoding) runs on [`Config::workers`] threads (`plumcraft-media-N`),
//! started on first use. A waveform's peaks, which travel inline in their reference and were
//! never read through the engine, are made in place when at most [`Config::inline_max`]: no
//! engine, no queue.
//!
//! **Queue and cancellation.** At most [`Config::queue`] requests wait; past that a request is
//! answered `Pending` without being queued, and is queued when asked again. A shell that draws
//! in frames calls [`MediaService::frame`] once a frame and asks again each frame for what it
//! still shows; a request nobody has asked for in [`Config::keep_frames`] frames (a node
//! scrolled away, a superseded run's value) is dropped before its work starts. A new engine link
//! ([`MediaService::set_resolver`]) cancels everything queued for the old one. Work already
//! inside an engine call can't be stopped; its result is kept if it is still wanted.
//!
//! **Cache.** Results are kept up to [`Config::budget`] bytes, least recently asked for evicted
//! first; a result not yet collected is never evicted (it would be made again forever). A value's
//! bytes are also indexed weakly, so whatever else still holds them (the Watched panel, a
//! pop-out, a lease the shell holds) shares one copy, as `Fulls` did. Misses are kept until
//! [`MediaService::forget_misses`] (a run has published values again). Content handles name the
//! same bytes forever (`docs/interface.md` 4.4), so results outlive engine switches; values
//! without a handle (a path locator) don't.
//!
//! **Waking.** The [`Wake`] is called, from a worker, each time a result lands. It carries no
//! data; the shell hops to its own thread and asks again (the `Link` design: no data crosses on
//! a foreign thread).
//!
//! **Stopping.** Dropping the last clone of the service stops its workers: each ends after the
//! job it is in. Nothing joins a thread that might not end (a hung engine call ends when the
//! host's own timeout does).

use std::collections::{HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use plumcraft_proto::data::{DataRef, Locator};
use plumcraft_store::Blob;
use serde_json::{Value, json};

use crate::audio::{self, Peaks};
use crate::decode;
use crate::raster::Rgba;

/// A value's bytes through the engine link (the connection's `resolve`). It may block (an HTTP
/// fetch): the service only ever calls it on a worker, or in [`MediaService::blocking`].
pub type Resolve = Arc<dyn Fn(&DataRef) -> Option<Blob> + Send + Sync>;

/// Something arrived: ask again. Called on a worker thread, with no payload.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// What to make of a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Want {
    /// Its bytes, as the engine published them.
    Bytes,
    /// An engine's `rgba8` rendition, an (h, w, 4) `|u1` array, as a picture.
    Rendition,
    /// A `peaks` rendition: a waveform's overview.
    Peaks,
    /// A picture file (PNG, JPEG, TIFF, WebP, GIF) decoded, at most this many pixels a side.
    Picture(u32),
    /// An SVG drawn with this many pixels on its long side.
    Svg(u32),
    /// A picture file's own numbers, its bit depth kept (the probe and compare read them).
    Numbers,
}

impl Want {
    /// Its name in a derived key and the C ABI's `media.get`.
    pub fn name(self) -> String {
        match self {
            Want::Bytes => "bytes".into(),
            Want::Rendition => "rgba".into(),
            Want::Peaks => "peaks".into(),
            Want::Picture(px) => format!("image@{px}"),
            Want::Svg(px) => format!("svg@{px}"),
            Want::Numbers => "numbers".into(),
        }
    }

    /// Whether it is made from the value's full bytes (and so shares them with [`Want::Bytes`]).
    fn reads_full(self) -> bool {
        matches!(self, Want::Bytes | Want::Picture(_) | Want::Svg(_) | Want::Numbers)
    }
}

/// What was made.
#[derive(Debug, Clone)]
pub enum Made {
    /// The bytes ([`Want::Bytes`]), or a picture file's numbers ([`Want::Numbers`]).
    Bytes(Arc<Blob>),
    /// A picture ([`Want::Rendition`], [`Want::Picture`], [`Want::Svg`]).
    Picture(Arc<Rgba>),
    Peaks(Arc<Peaks>),
}

impl Made {
    /// Bytes held, as the cache counts them.
    pub fn size(&self) -> usize {
        match self {
            Made::Bytes(b) => b.bytes.len() + 64,
            Made::Picture(p) => p.bytes() + 32,
            Made::Peaks(p) => p.bytes(),
        }
    }
}

/// The answer to a request.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetch<T> {
    Ready(T),
    Pending,
    Missing,
    Failed(String),
}

impl<T> Fetch<T> {
    pub fn ready(self) -> Option<T> {
        match self {
            Fetch::Ready(t) => Some(t),
            _ => None,
        }
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, Fetch::Pending)
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Fetch<U> {
        match self {
            Fetch::Ready(t) => Fetch::Ready(f(t)),
            Fetch::Pending => Fetch::Pending,
            Fetch::Missing => Fetch::Missing,
            Fetch::Failed(e) => Fetch::Failed(e),
        }
    }

    /// The status word the C ABI and `ui.inspect` use: `ready`, `pending`, `missing`, `failed`.
    pub fn status(&self) -> &'static str {
        match self {
            Fetch::Ready(_) => "ready",
            Fetch::Pending => "pending",
            Fetch::Missing => "missing",
            Fetch::Failed(_) => "failed",
        }
    }
}

impl Fetch<Made> {
    pub fn bytes(self) -> Fetch<Arc<Blob>> {
        self.and(|m| if let Made::Bytes(b) = m { Some(b) } else { None })
    }
    pub fn picture(self) -> Fetch<Arc<Rgba>> {
        self.and(|m| if let Made::Picture(p) = m { Some(p) } else { None })
    }
    pub fn peaks(self) -> Fetch<Arc<Peaks>> {
        self.and(|m| if let Made::Peaks(p) = m { Some(p) } else { None })
    }
    fn and<U>(self, f: impl FnOnce(Made) -> Option<U>) -> Fetch<U> {
        match self {
            Fetch::Ready(m) => f(m).map_or_else(|| Fetch::Failed("made the wrong kind of thing".into()), Fetch::Ready),
            Fetch::Pending => Fetch::Pending,
            Fetch::Missing => Fetch::Missing,
            Fetch::Failed(e) => Fetch::Failed(e),
        }
    }
}

/// Anything values can be asked of: the [`MediaService`] (never waits), or [`Blocking`] (a
/// resolve function called in place, for code that may wait: tests, scripts, a command that
/// replies with what it read).
pub trait Source {
    fn fetch(&self, r: &DataRef, want: Want) -> Fetch<Made>;
}

/// The service's limits. [`Config::default`] suits a desktop; a phone wants a smaller budget.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Bytes of results kept (decoded pictures, bytes, peaks).
    pub budget: usize,
    /// Worker threads.
    pub workers: usize,
    /// The most requests waiting for a worker.
    pub queue: usize,
    /// Frames a request may go unasked before it is dropped unstarted (and a result uncollected
    /// before it may be evicted).
    pub keep_frames: u64,
    /// Inline peaks (base64 characters) made in place rather than on a worker.
    pub inline_max: usize,
    /// Keep a list of finished requests for [`MediaService::take_finished`] (the C ABI's feed).
    pub report: bool,
}

/// The default budget: 256 MiB.
pub const DEFAULT_BUDGET: usize = 256 << 20;
/// The most cache entries (misses cost no bytes, so they are counted too).
const MAX_ENTRIES: usize = 16_384;
/// The most finished requests kept for [`MediaService::take_finished`].
const MAX_FINISHED: usize = 4096;

impl Default for Config {
    fn default() -> Self {
        Config { budget: DEFAULT_BUDGET, workers: 2, queue: 512, keep_frames: 120, inline_max: 64 << 10, report: false }
    }
}

impl Config {
    /// `{cache_mb?, workers?, queue?, keep_frames?}` (the core's config `media` key); anything
    /// absent is the default. Bounded: 1 to 65,536 MiB, 1 to 16 workers, 1 to 65,536 queued.
    pub fn from_json(v: Option<&Value>) -> Result<Config, String> {
        let mut c = Config::default();
        let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(c) };
        let o = v.as_object().ok_or("media must be {cache_mb, workers, queue}")?;
        let num = |k: &str, lo: u64, hi: u64| -> Result<Option<u64>, String> {
            match o.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(x) => x.as_u64().filter(|n| (lo..=hi).contains(n)).map(Some).ok_or_else(|| format!("media.{k} must be a whole number from {lo} to {hi}")),
            }
        };
        if let Some(mb) = num("cache_mb", 1, 65_536)? {
            c.budget = usize::try_from(mb << 20).map_err(|_| "media.cache_mb is too large here")?;
        }
        if let Some(n) = num("workers", 1, 16)? {
            c.workers = n as usize;
        }
        if let Some(n) = num("queue", 1, 65_536)? {
            c.queue = n as usize;
        }
        if let Some(n) = num("keep_frames", 1, 1_000_000)? {
            c.keep_frames = n;
        }
        Ok(c)
    }
}

/// What the service has done, for `ui.inspect`, the C ABI and tests.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    /// Entries held (results, misses, requests in flight) and the bytes of results.
    pub entries: usize,
    pub bytes: usize,
    pub queued: usize,
    pub working: usize,
    /// Requests made in place (inline bytes), and finished by a worker.
    pub inline: u64,
    pub done: u64,
    /// Calls to the resolver, on workers and in [`MediaService::blocking`].
    pub resolves: u64,
    /// Requests dropped unstarted (unasked too long, or a new engine link).
    pub cancelled: u64,
    /// Results evicted for the budget.
    pub evicted: u64,
    /// Requests answered `Pending` because the queue was full.
    pub refused: u64,
    /// Worker threads started.
    pub threads: usize,
}

impl Stats {
    pub fn json(&self) -> Value {
        json!({"entries": self.entries, "bytes": self.bytes, "queued": self.queued, "working": self.working, "inline": self.inline, "done": self.done,
               "resolves": self.resolves, "cancelled": self.cancelled, "evicted": self.evicted, "refused": self.refused, "threads": self.threads})
    }
}

/// A request that finished (with [`Config::report`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    pub key: String,
    pub want: Want,
    /// `ready`, `missing` or `failed`.
    pub status: &'static str,
    pub reason: Option<String>,
}

type Key = (String, Want);

enum State {
    Queued,
    Working,
    Done(Made),
    Missing,
    Failed(String),
}

struct Entry {
    r: DataRef,
    state: State,
    /// The request counter when last asked for (least recent is evicted first).
    used: u64,
    /// The frame when last asked for.
    wanted: u64,
    /// Asked for since it finished (an uncollected result is never evicted).
    taken: bool,
    /// The engine link it was asked under.
    epoch: u64,
    size: usize,
}

struct St {
    config: Config,
    entries: HashMap<Key, Entry>,
    queue: VecDeque<Key>,
    /// Bytes held anywhere, by key: shared rather than read again.
    live: HashMap<String, Weak<Blob>>,
    bytes: usize,
    used: u64,
    frame: u64,
    epoch: u64,
    resolver: Option<Resolve>,
    wake: Option<Wake>,
    threads: usize,
    stop: bool,
    stats: Stats,
    finished: Vec<Finished>,
}

struct Inner {
    st: Mutex<St>,
    /// Work queued, a result landed, or stop.
    cv: Condvar,
}

/// Stops the workers when the last [`MediaService`] clone goes.
struct Owner(Arc<Inner>);

impl Drop for Owner {
    fn drop(&mut self) {
        let mut st = lock(&self.0.st);
        st.stop = true;
        st.queue.clear();
        st.resolver = None;
        st.wake = None;
        drop(st);
        self.0.cv.notify_all();
    }
}

/// The media service. Cheap to clone; every clone is the same service.
#[derive(Clone)]
pub struct MediaService {
    owner: Arc<Owner>,
}

impl std::fmt::Debug for MediaService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaService").field("stats", &self.stats()).finish()
    }
}

impl Default for MediaService {
    fn default() -> Self {
        MediaService::new(Config::default())
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The cache key of a data reference: its handle, else where it is.
pub fn key_of(r: &DataRef) -> String {
    r.handle.as_ref().map(ToString::to_string).unwrap_or_else(|| format!("{:?}", r.at))
}

/// The inline payload's length, if the reference carries its bytes.
fn inline_len(r: &DataRef) -> Option<usize> {
    r.at.iter().find_map(|l| if let Locator::Inline { b64 } = l { Some(b64.len()) } else { None })
}

impl MediaService {
    pub fn new(config: Config) -> MediaService {
        let st = St {
            config,
            entries: HashMap::new(),
            queue: VecDeque::new(),
            live: HashMap::new(),
            bytes: 0,
            used: 0,
            frame: 0,
            epoch: 0,
            resolver: None,
            wake: None,
            threads: 0,
            stop: false,
            stats: Stats::default(),
            finished: Vec::new(),
        };
        MediaService { owner: Arc::new(Owner(Arc::new(Inner { st: Mutex::new(st), cv: Condvar::new() }))) }
    }

    fn inner(&self) -> &Arc<Inner> {
        &self.owner.0
    }

    /// Its limits.
    pub fn config(&self) -> Config {
        lock(&self.inner().st).config.clone()
    }

    /// Change the budget (a memory warning, a setting): evicts down to it at once.
    pub fn set_budget(&self, budget: usize) {
        let mut st = lock(&self.inner().st);
        st.config.budget = budget;
        evict(&mut st);
    }

    /// How values are read: the engine link's `resolve` (none: no engine). A new link cancels
    /// what was queued under the old one and forgets misses and values without a handle; results
    /// by content handle stay.
    pub fn set_resolver(&self, resolver: Option<Resolve>) {
        let mut st = lock(&self.inner().st);
        st.resolver = resolver;
        st.epoch += 1;
        let cancelled = st.queue.len() as u64;
        st.queue.clear();
        st.stats.cancelled += cancelled;
        st.entries.retain(|(k, _), e| matches!(e.state, State::Done(_)) && !k.starts_with('['));
        st.live.retain(|k, _| !k.starts_with('['));
        recount(&mut st);
    }

    /// Whether a resolver is set.
    pub fn has_resolver(&self) -> bool {
        lock(&self.inner().st).resolver.is_some()
    }

    /// What to call when something arrives.
    pub fn set_wake(&self, wake: Option<Wake>) {
        lock(&self.inner().st).wake = wake;
    }

    /// Whether a wake is set.
    pub fn has_wake(&self) -> bool {
        lock(&self.inner().st).wake.is_some()
    }

    /// A new frame: what hasn't been asked for in [`Config::keep_frames`] frames is dropped
    /// before its work starts.
    pub fn frame(&self) {
        let mut st = lock(&self.inner().st);
        st.frame += 1;
        let (frame, keep) = (st.frame, st.config.keep_frames);
        let stale: Vec<Key> = st.queue.iter().filter(|k| st.entries.get(*k).is_none_or(|e| frame.saturating_sub(e.wanted) > keep)).cloned().collect();
        if stale.is_empty() {
            return;
        }
        st.queue.retain(|k| !stale.contains(k));
        for k in &stale {
            st.entries.remove(k);
        }
        st.stats.cancelled += stale.len() as u64;
    }

    /// Drop every request for `r` not yet started (whatever is wanted of it).
    pub fn cancel(&self, r: &DataRef) {
        let key = key_of(r);
        let mut st = lock(&self.inner().st);
        let before = st.queue.len();
        st.queue.retain(|(k, _)| *k != key);
        let n = before - st.queue.len();
        st.entries.retain(|(k, _), e| !(*k == key && matches!(e.state, State::Queued)));
        st.stats.cancelled += n as u64;
    }

    /// Forget misses (a run has published its values again) and values without a handle (a path
    /// may hold new bytes now).
    pub fn forget_misses(&self) {
        let mut st = lock(&self.inner().st);
        st.entries.retain(|(k, _), e| !matches!(e.state, State::Missing | State::Failed(_)) && !(k.starts_with('[') && matches!(e.state, State::Done(_))));
        st.live.retain(|k, _| !k.starts_with('['));
        recount(&mut st);
    }

    /// Drop every result kept (a memory warning). What others still hold stays shared.
    pub fn drop_caches(&self) {
        let mut st = lock(&self.inner().st);
        st.entries.retain(|_, e| matches!(e.state, State::Queued | State::Working));
        recount(&mut st);
    }

    /// Ask for `want` of `r`. Never waits.
    pub fn get(&self, r: &DataRef, want: Want) -> Fetch<Made> {
        let key = (key_of(r), want);
        let inner = self.inner();
        let mut st = lock(&inner.st);
        if st.stop {
            return Fetch::Missing;
        }
        st.used += 1;
        let (used, frame) = (st.used, st.frame);
        if let Some(e) = st.entries.get_mut(&key) {
            e.used = used;
            e.wanted = frame;
            return match &e.state {
                State::Done(m) => {
                    e.taken = true;
                    Fetch::Ready(m.clone())
                }
                State::Missing => Fetch::Missing,
                State::Failed(why) => Fetch::Failed(why.clone()),
                State::Queued | State::Working => Fetch::Pending,
            };
        }
        // bytes something else still holds: shared, not read again
        if want == Want::Bytes
            && let Some(b) = st.live.get(&key.0).and_then(Weak::upgrade)
        {
            return Fetch::Ready(Made::Bytes(b));
        }
        // a waveform's peaks travel in the reference itself and were never read through the
        // engine: made in place. Everything else goes through the connection, which may refuse
        // a value (a remote engine without `blob.range`), so on a worker.
        if want == Want::Peaks && inline_len(r).is_some_and(|n| n <= st.config.inline_max) {
            drop(st);
            let made = make(r, want, None, None);
            let fetch = settle(made);
            let mut st = lock(&inner.st);
            st.stats.inline += 1;
            store(&mut st, key, r, &fetch, true);
            return fetch;
        }
        if st.resolver.is_none() {
            return Fetch::Missing;
        }
        if st.queue.len() >= st.config.queue {
            st.stats.refused += 1;
            return Fetch::Pending;
        }
        let epoch = st.epoch;
        st.entries.insert(key.clone(), Entry { r: r.clone(), state: State::Queued, used, wanted: frame, taken: false, epoch, size: 0 });
        st.queue.push_back(key);
        let spawn = st.threads < st.config.workers.max(1);
        if spawn {
            st.threads += 1;
            st.stats.threads = st.threads;
        }
        let n = st.threads;
        drop(st);
        if spawn {
            let w = Arc::clone(inner);
            let started = std::thread::Builder::new().name(format!("plumcraft-media-{n}")).spawn(move || work_loop(&w));
            if started.is_err() {
                // no thread to be had: this request is made in place rather than never
                let mut st = lock(&inner.st);
                st.threads -= 1;
                st.stats.threads = st.threads;
                if st.threads == 0 {
                    drop(st);
                    return run_one(inner).unwrap_or(Fetch::Pending);
                }
            }
        }
        inner.cv.notify_all();
        Fetch::Pending
    }

    /// [`MediaService::get`]'s bytes.
    pub fn bytes(&self, r: &DataRef) -> Fetch<Arc<Blob>> {
        self.get(r, Want::Bytes).bytes()
    }

    /// An `rgba8` rendition as a picture.
    pub fn rendition(&self, r: &DataRef) -> Fetch<Arc<Rgba>> {
        self.get(r, Want::Rendition).picture()
    }

    /// A `peaks` rendition.
    pub fn peaks(&self, r: &DataRef) -> Fetch<Arc<Peaks>> {
        self.get(r, Want::Peaks).peaks()
    }

    /// `want` of `r`, made on this thread if it isn't here yet: for callers that may wait (a
    /// command whose reply carries what it read, a script). The result is cached as a worker's
    /// would be.
    pub fn blocking(&self, r: &DataRef, want: Want) -> Fetch<Made> {
        match self.get(r, want) {
            Fetch::Pending => {}
            done => return done,
        }
        let (resolver, have) = {
            let st = lock(&self.inner().st);
            (st.resolver.clone(), shared_bytes(&st, &key_of(r)))
        };
        let Some(resolver) = resolver else { return Fetch::Missing };
        let fetch = settle(make(r, want, Some(&resolver), have));
        let mut st = lock(&self.inner().st);
        st.stats.resolves += 1;
        store(&mut st, (key_of(r), want), r, &fetch, true);
        fetch
    }

    /// Wait until nothing is queued or being made, at most `timeout`. For tests and probes, never
    /// a shell's thread.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let inner = self.inner();
        let mut st = lock(&inner.st);
        loop {
            let busy = !st.queue.is_empty() || st.entries.values().any(|e| matches!(e.state, State::Working));
            if !busy {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            st = match inner.cv.wait_timeout(st, deadline - now) {
                Ok((g, _)) => g,
                Err(p) => p.into_inner().0,
            };
        }
    }

    /// Whether anything is queued or being made.
    pub fn busy(&self) -> bool {
        let st = lock(&self.inner().st);
        !st.queue.is_empty() || st.entries.values().any(|e| matches!(e.state, State::Working))
    }

    pub fn stats(&self) -> Stats {
        let st = lock(&self.inner().st);
        let mut s = st.stats.clone();
        s.entries = st.entries.len();
        s.bytes = st.bytes;
        s.queued = st.queue.len();
        s.working = st.entries.values().filter(|e| matches!(e.state, State::Working)).count();
        s
    }

    /// Requests finished since the last call (with [`Config::report`]).
    pub fn take_finished(&self) -> Vec<Finished> {
        std::mem::take(&mut lock(&self.inner().st).finished)
    }
}

impl Source for MediaService {
    fn fetch(&self, r: &DataRef, want: Want) -> Fetch<Made> {
        self.get(r, want)
    }
}

/// A resolve function called in place: [`Source`] for code that may wait, and the old
/// `resolve: &dyn Fn(&DataRef) -> Option<Blob>` signatures kept for callers outside the shell.
pub struct Blocking<'a>(pub &'a dyn Fn(&DataRef) -> Option<Blob>);

impl Source for Blocking<'_> {
    fn fetch(&self, r: &DataRef, want: Want) -> Fetch<Made> {
        settle(make_with(r, want, self.0, None))
    }
}

/// What making something came to: made, its bytes gone, or not what it claims.
enum Outcome {
    Made(Made),
    Missing,
    Failed(String),
}

fn settle(o: Outcome) -> Fetch<Made> {
    match o {
        Outcome::Made(m) => Fetch::Ready(m),
        Outcome::Missing => Fetch::Missing,
        Outcome::Failed(e) => Fetch::Failed(e),
    }
}

/// Bytes for `key` already held: a cached [`Want::Bytes`] result, or a live copy.
fn shared_bytes(st: &St, key: &str) -> Option<Arc<Blob>> {
    if let Some(Entry { state: State::Done(Made::Bytes(b)), .. }) = st.entries.get(&(key.to_owned(), Want::Bytes)) {
        return Some(Arc::clone(b));
    }
    st.live.get(key).and_then(Weak::upgrade)
}

fn make(r: &DataRef, want: Want, resolver: Option<&Resolve>, have: Option<Arc<Blob>>) -> Outcome {
    match resolver {
        Some(f) => make_with(r, want, &|d: &DataRef| f(d), have),
        None => make_with(r, want, &|_: &DataRef| None, have),
    }
}

/// Read (through `resolve`: the connection reads inline bytes itself, and may refuse a value)
/// and make `want` of `r`.
fn make_with(r: &DataRef, want: Want, resolve: &dyn Fn(&DataRef) -> Option<Blob>, have: Option<Arc<Blob>>) -> Outcome {
    let read = |r: &DataRef| -> Option<Arc<Blob>> {
        if let Some(b) = have.clone() {
            return Some(b);
        }
        resolve(r).map(Arc::new)
    };
    match want {
        Want::Bytes => read(r).map_or(Outcome::Missing, |b| Outcome::Made(Made::Bytes(b))),
        Want::Rendition => match read(r) {
            None => Outcome::Missing,
            Some(b) => decode::rendition(&b).map_or_else(Outcome::Failed, |p| Outcome::Made(Made::Picture(Arc::new(p)))),
        },
        // a waveform's peaks are decoded inline first, as the canvas always did
        Want::Peaks => match audio::decode_inline(r).or_else(|| resolve(r)) {
            None => Outcome::Missing,
            Some(b) => Peaks::from_blob(&b).map_or_else(Outcome::Failed, |p| Outcome::Made(Made::Peaks(Arc::new(p)))),
        },
        Want::Picture(px) => match read(r) {
            None => Outcome::Missing,
            Some(b) => decode::decode_picture(&b.bytes, px.clamp(1, decode::MAX_TEXTURE_SIDE))
                .map_or_else(Outcome::Failed, |p| Outcome::Made(Made::Picture(Arc::new(p)))),
        },
        Want::Numbers => match read(r) {
            None => Outcome::Missing,
            Some(b) => decode::decode_numbers(&b.bytes).map_or_else(Outcome::Failed, |n| Outcome::Made(Made::Bytes(Arc::new(n)))),
        },
        Want::Svg(px) => match read(r) {
            None => Outcome::Missing,
            #[cfg(feature = "svg")]
            Some(b) => crate::svg::rasterise(&b.bytes, px as f32).map_or_else(Outcome::Failed, |p| Outcome::Made(Made::Picture(Arc::new(p)))),
            #[cfg(not(feature = "svg"))]
            Some(_) => {
                let _ = px;
                Outcome::Failed("SVG needs the `svg` feature".into())
            }
        },
    }
}

/// Keep what a request came to. `taken`: its asker has it already.
fn store(st: &mut St, key: Key, r: &DataRef, fetch: &Fetch<Made>, taken: bool) {
    let (state, size) = match fetch {
        Fetch::Ready(m) => (State::Done(m.clone()), m.size()),
        Fetch::Missing => (State::Missing, 0),
        Fetch::Failed(e) => (State::Failed(e.clone()), 0),
        Fetch::Pending => return,
    };
    if let Fetch::Ready(Made::Bytes(b)) = fetch
        && key.1 == Want::Bytes
    {
        st.live.retain(|_, w| w.strong_count() > 0);
        st.live.insert(key.0.clone(), Arc::downgrade(b));
    }
    if st.config.report {
        if st.finished.len() >= MAX_FINISHED {
            st.finished.remove(0);
        }
        let reason = if let Fetch::Failed(e) = fetch { Some(e.clone()) } else { None };
        st.finished.push(Finished { key: key.0.clone(), want: key.1, status: fetch.status(), reason });
    }
    let (used, frame, epoch) = (st.used, st.frame, st.epoch);
    let old = st.entries.insert(key, Entry { r: r.clone(), state, used, wanted: frame, taken, epoch, size });
    st.bytes = st.bytes.saturating_sub(old.map_or(0, |e| e.size)) + size;
    evict(st);
}

/// Down to the budget, least recently asked for first; never a result its asker hasn't collected
/// unless it has gone unasked for [`Config::keep_frames`] frames. Then at most [`MAX_ENTRIES`].
fn evict(st: &mut St) {
    let (frame, keep) = (st.frame, st.config.keep_frames);
    let evictable = |e: &Entry| match &e.state {
        State::Done(_) => e.taken || frame.saturating_sub(e.wanted) > keep,
        State::Missing | State::Failed(_) => true,
        _ => false,
    };
    while st.bytes > st.config.budget {
        let victim = st.entries.iter().filter(|(_, e)| e.size > 0 && evictable(e)).min_by_key(|(_, e)| e.used).map(|(k, _)| k.clone());
        let Some(k) = victim else { break };
        if let Some(e) = st.entries.remove(&k) {
            st.bytes = st.bytes.saturating_sub(e.size);
            st.stats.evicted += 1;
        }
    }
    while st.entries.len() > MAX_ENTRIES {
        let victim = st.entries.iter().filter(|(_, e)| evictable(e)).min_by_key(|(_, e)| e.used).map(|(k, _)| k.clone());
        let Some(k) = victim else { break };
        if let Some(e) = st.entries.remove(&k) {
            st.bytes = st.bytes.saturating_sub(e.size);
        }
    }
}

fn recount(st: &mut St) {
    st.bytes = st.entries.values().map(|e| e.size).sum();
}

/// A worker: take a request, make it, keep it, wake the shell; until stopped.
fn work_loop(inner: &Arc<Inner>) {
    loop {
        match run_one(inner) {
            Some(_) => {}
            None => return,
        }
    }
}

/// Wait for one request and make it. `None` once stopped.
fn run_one(inner: &Arc<Inner>) -> Option<Fetch<Made>> {
    let mut st = lock(&inner.st);
    let (key, r, epoch, resolver, have) = loop {
        if st.stop {
            return None;
        }
        let Some(key) = st.queue.pop_front() else {
            st = match inner.cv.wait(st) {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            continue;
        };
        let (frame, keep, now_epoch) = (st.frame, st.config.keep_frames, st.epoch);
        let have = if key.1.reads_full() { shared_bytes(&st, &key.0) } else { None };
        let resolver = st.resolver.clone();
        let Some(e) = st.entries.get_mut(&key) else { continue };
        // nobody wants it any more, or it was asked under an engine link since replaced
        if e.epoch != now_epoch || frame.saturating_sub(e.wanted) > keep {
            st.entries.remove(&key);
            st.stats.cancelled += 1;
            continue;
        }
        e.state = State::Working;
        break (key, e.r.clone(), e.epoch, resolver, have);
    };
    drop(st);
    let made = catch_unwind(AssertUnwindSafe(|| make(&r, key.1, resolver.as_ref(), have))).unwrap_or_else(|_| Outcome::Failed("the decoder stopped".into()));
    let fetch = settle(made);
    let mut st = lock(&inner.st);
    st.stats.resolves += 1;
    st.stats.done += 1;
    let current = st.entries.get(&key).is_some_and(|e| e.epoch == epoch && matches!(e.state, State::Working));
    if current {
        store(&mut st, key, &r, &fetch, false);
    }
    let wake = st.wake.clone();
    drop(st);
    inner.cv.notify_all();
    if current && let Some(w) = wake {
        w();
    }
    Some(fetch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::data::{BlobMeta, Form};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn rgba_ref(name: &str, w: u64, h: u64) -> DataRef {
        let meta =
            BlobMeta { form: Form::Array { dtype: "|u1".into(), shape: vec![h, w, 4], space: None, names: Vec::new(), table: false }, ..BlobMeta::default() };
        DataRef { handle: Some(plumcraft_proto::Handle::raw(name.as_bytes())), meta, at: vec![] }
    }

    fn picture_blob(r: &DataRef) -> Option<Blob> {
        let Form::Array { shape, .. } = &r.meta.form else { return None };
        let n: u64 = shape.iter().product();
        Some(Blob { dtype: "|u1".into(), shape: shape.clone(), bytes: vec![7u8; n as usize].into() })
    }

    /// A resolver that takes `delay` and counts its calls, and on which threads.
    fn slow(delay: Duration, calls: &Arc<AtomicU64>) -> Resolve {
        let calls = Arc::clone(calls);
        Arc::new(move |r: &DataRef| {
            calls.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(delay);
            picture_blob(r)
        })
    }

    fn until_ready(m: &MediaService, r: &DataRef, want: Want) -> Fetch<Made> {
        let start = Instant::now();
        loop {
            match m.get(r, want) {
                Fetch::Pending if start.elapsed() < Duration::from_secs(10) => {
                    m.wait_idle(Duration::from_millis(50));
                }
                f => return f,
            }
        }
    }

    #[test]
    fn a_request_answers_at_once_and_arrives_on_a_worker_with_a_wake() {
        let m = MediaService::new(Config::default());
        let calls = Arc::new(AtomicU64::new(0));
        m.set_resolver(Some(slow(Duration::from_millis(200), &calls)));
        let wakes = Arc::new(AtomicU64::new(0));
        let w = Arc::clone(&wakes);
        m.set_wake(Some(Arc::new(move || {
            w.fetch_add(1, Ordering::Relaxed);
        })));
        let r = rgba_ref("a", 3, 2);
        let t = Instant::now();
        assert!(m.get(&r, Want::Rendition).is_pending());
        assert!(m.get(&r, Want::Rendition).is_pending(), "asked again: still one request");
        assert!(t.elapsed() < Duration::from_millis(50), "never waits: {:?}", t.elapsed());
        let p = until_ready(&m, &r, Want::Rendition).picture().ready().unwrap();
        assert_eq!((p.w, p.h, p.pixels.len()), (3, 2, 24));
        assert_eq!(calls.load(Ordering::Relaxed), 1, "read once");
        assert!(wakes.load(Ordering::Relaxed) >= 1, "the shell was woken");
        assert_eq!(m.stats().threads, 1);
    }

    #[test]
    fn misses_and_junk_are_answers_not_panics() {
        let m = MediaService::new(Config::default());
        let r = rgba_ref("gone", 3, 2);
        assert!(matches!(m.get(&r, Want::Rendition), Fetch::Missing), "no engine: missing");
        m.set_resolver(Some(Arc::new(|_: &DataRef| None)));
        assert!(matches!(until_ready(&m, &r, Want::Rendition), Fetch::Missing));
        m.set_resolver(Some(Arc::new(|_: &DataRef| Some(Blob { dtype: "|u1".into(), shape: vec![2, 3, 4], bytes: vec![0; 5].into() }))));
        assert!(matches!(until_ready(&m, &r, Want::Rendition), Fetch::Failed(_)), "torn bytes");
        for want in [Want::Peaks, Want::Picture(64), Want::Numbers, Want::Svg(64)] {
            assert!(matches!(until_ready(&m, &rgba_ref(&format!("{want:?}"), 3, 2), want), Fetch::Failed(_)), "{want:?}");
        }
        // a resolver that panics is a failure, and the worker lives on
        m.set_resolver(Some(Arc::new(|_: &DataRef| -> Option<Blob> { panic!("hostile") })));
        assert!(matches!(until_ready(&m, &rgba_ref("p", 1, 1), Want::Bytes), Fetch::Failed(_)));
        m.set_resolver(Some(Arc::new(picture_blob)));
        assert!(matches!(until_ready(&m, &rgba_ref("after", 1, 1), Want::Rendition), Fetch::Ready(_)));
    }

    #[test]
    fn unwanted_requests_are_dropped_unstarted_and_a_new_link_cancels_the_old() {
        let m = MediaService::new(Config { workers: 1, keep_frames: 2, ..Config::default() });
        let calls = Arc::new(AtomicU64::new(0));
        m.set_resolver(Some(slow(Duration::from_millis(150), &calls)));
        let refs: Vec<DataRef> = (0..6).map(|i| rgba_ref(&format!("s{i}"), 1, 1)).collect();
        for r in &refs {
            assert!(m.get(r, Want::Rendition).is_pending());
        }
        // scrolled away: nobody asks for them for three frames
        for _ in 0..3 {
            m.frame();
        }
        assert!(m.wait_idle(Duration::from_secs(5)));
        assert!(calls.load(Ordering::Relaxed) <= 2, "at most the one under way was made: {}", calls.load(Ordering::Relaxed));
        assert!(m.stats().cancelled >= 4, "{:?}", m.stats());
        // a new engine link: what was queued for the old one never runs
        let before = calls.load(Ordering::Relaxed);
        for r in &refs {
            let _ = m.get(r, Want::Rendition);
        }
        m.set_resolver(Some(Arc::new(picture_blob)));
        assert!(m.wait_idle(Duration::from_secs(5)));
        assert!(calls.load(Ordering::Relaxed) <= before + 1, "the old link isn't asked again");
    }

    #[test]
    fn the_queue_is_bounded() {
        let m = MediaService::new(Config { workers: 1, queue: 3, ..Config::default() });
        let calls = Arc::new(AtomicU64::new(0));
        m.set_resolver(Some(slow(Duration::from_millis(100), &calls)));
        for i in 0..20 {
            assert!(m.get(&rgba_ref(&format!("q{i}"), 1, 1), Want::Rendition).is_pending());
        }
        let s = m.stats();
        assert!(s.queued <= 3 && s.refused >= 15, "{s:?}");
        drop(m);
    }

    #[test]
    fn the_cache_keeps_to_its_budget_least_recent_first_and_never_loses_an_uncollected_result() {
        let m = MediaService::new(Config { budget: 10_000, ..Config::default() });
        m.set_resolver(Some(Arc::new(picture_blob)));
        // each picture is 40 × 10 × 4 = 1600 bytes (+32)
        let refs: Vec<DataRef> = (0..12).map(|i| rgba_ref(&format!("c{i}"), 40, 10)).collect();
        for r in &refs {
            assert!(until_ready(&m, r, Want::Rendition).picture().ready().is_some());
        }
        let s = m.stats();
        assert!(s.bytes <= 10_000 && s.evicted >= 6, "{s:?}");
        // the most recent are kept, the oldest went
        assert!(matches!(m.get(&refs[11], Want::Rendition), Fetch::Ready(_)));
        assert!(m.get(&refs[0], Want::Rendition).is_pending(), "evicted: asked again");
        // larger than the whole budget: still handed over once
        let big = rgba_ref("big", 100, 100);
        assert!(until_ready(&m, &big, Want::Rendition).picture().ready().is_some());
        m.drop_caches();
        assert_eq!(m.stats().bytes, 0);
    }

    #[test]
    fn bytes_held_elsewhere_are_shared_not_read_again() {
        let m = MediaService::new(Config { budget: 1, ..Config::default() });
        let calls = Arc::new(AtomicU64::new(0));
        m.set_resolver(Some(slow(Duration::ZERO, &calls)));
        let r = rgba_ref("shared", 64, 64);
        let held = until_ready(&m, &r, Want::Bytes).bytes().ready().unwrap();
        m.drop_caches();
        let again = m.get(&r, Want::Bytes).bytes().ready().expect("still held by someone: shared");
        assert!(Arc::ptr_eq(&held, &again));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        drop((held, again));
        assert!(m.get(&r, Want::Bytes).is_pending(), "let go of everywhere: read again");
    }

    #[test]
    fn inline_values_are_made_in_place() {
        use base64::Engine as _;
        let m = MediaService::new(Config::default());
        let v = [-0.5f32, 0.5, -0.25, 0.25];
        let bytes: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        let meta = BlobMeta {
            form: Form::Array { dtype: "<f4".into(), shape: vec![1, 1, 2, 2], space: None, names: Vec::new(), table: false },
            ..BlobMeta::default()
        };
        let r = DataRef { handle: None, meta, at: vec![Locator::Inline { b64: base64::engine::general_purpose::STANDARD.encode(&bytes) }] };
        let p = m.get(&r, Want::Peaks).peaks().ready().expect("no engine needed");
        assert_eq!(p.data, vec![[-0.5, 0.5], [-0.25, 0.25]]);
        assert_eq!(m.stats().threads, 0, "no worker started");
    }

    #[test]
    fn dropping_it_stops_its_workers_and_never_waits_for_a_hung_resolve() {
        let m = MediaService::new(Config::default());
        let calls = Arc::new(AtomicU64::new(0));
        m.set_resolver(Some(slow(Duration::from_secs(3), &calls)));
        let _ = m.get(&rgba_ref("hung", 1, 1), Want::Rendition);
        let start = Instant::now();
        while calls.load(Ordering::Relaxed) == 0 && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let t = Instant::now();
        drop(m);
        assert!(t.elapsed() < Duration::from_millis(100), "dropped at once: {:?}", t.elapsed());
    }

    #[test]
    fn the_config_reads_its_json_and_refuses_junk() {
        assert_eq!(Config::from_json(None).unwrap(), Config::default());
        let c = Config::from_json(Some(&json!({"cache_mb": 64, "workers": 1, "queue": 32}))).unwrap();
        assert_eq!((c.budget, c.workers, c.queue), (64 << 20, 1, 32));
        for bad in [json!(3), json!({"cache_mb": 0}), json!({"cache_mb": -1}), json!({"workers": 99}), json!({"queue": "x"}), json!({"cache_mb": 1.5})] {
            assert!(Config::from_json(Some(&bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn blocking_reads_in_place_and_shares_the_cache() {
        let m = MediaService::new(Config::default());
        let calls = Arc::new(AtomicU64::new(0));
        m.set_resolver(Some(slow(Duration::ZERO, &calls)));
        let r = rgba_ref("b", 2, 2);
        assert!(matches!(m.blocking(&r, Want::Rendition), Fetch::Ready(_)));
        assert!(matches!(m.get(&r, Want::Rendition), Fetch::Ready(_)), "then cached");
        let blocking = Blocking(&|d: &DataRef| picture_blob(d));
        assert!(matches!(blocking.fetch(&r, Want::Rendition), Fetch::Ready(_)));
    }
}
