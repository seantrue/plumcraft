//! The in-process host (mode 1, spec 16.1): the engine lives on a worker thread built by a
//! factory; everything off the run path (spec 5.5) is answered at once from a lock-protected run
//! table: `hello`, `schema`, `run.cancel`, `run.get`, `events.poll`. A running graph never
//! blocks them. `output.save` is the one request a caller waits for: it takes its turn on the
//! worker, behind any run, and answers when the files are written.
//!
//! The host assigns every envelope's `seq` in one place, keeps a ring for resume and polling,
//! keeps tombstones for cancels that arrive before their run, and catches a panicking node:
//! the run fails with `kind: engine`, the engine is rebuilt from the factory, and the host
//! carries on (spec 16.1, "Panics").
//!
//! [`Host::dispatch`] is the JSON face of the same operations, which the socket and HTTP
//! bindings carry; the conformance tests drive the host through it to prove every message
//! survives the wire.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use plumcraft_proto::PROTOCOL;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::error::{ErrorCode, NodeError, ProtoError};
use plumcraft_proto::event::{Envelope, Event, NodeState, Resync, StatusRecord};
use plumcraft_proto::run::{NodeSnapshot, Phase, RunAck, RunReply, RunRequest, RunSnapshot, SaveReply, SaveRequest};
use plumcraft_proto::session::{EngineState, Hello, HelloReply, Proof, SessionConfirm};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::data::Plane;
use crate::exec::Memo;
use crate::{CancelToken, Emit, Engine, EngineCore, exec, now_ts};

/// Builds the engine on the worker thread; called again to rebuild after a caught panic.
pub type Factory = Box<dyn FnMut() -> Result<Engine, String> + Send>;

/// Envelopes kept for resume and polling.
const RING: usize = 4096;
/// Finished runs kept for `run.get`.
const KEEP_RUNS: usize = 16;
/// Panics within this window that stop the rebuilds.
const MAX_PANICS: usize = 5;
const PANIC_WINDOW: Duration = Duration::from_secs(60);
/// How long `hello` waits for the engine to be built.
const HELLO_WAIT: Duration = Duration::from_secs(8);
/// The longest `events.poll` wait.
const MAX_WAIT_MS: u64 = 30_000;
/// The longest a save may take, its wait behind a run included (spec 6.1).
const SAVE_WAIT: Duration = Duration::from_secs(300);

/// The reply to `events.poll`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PollReply {
    pub events: Vec<Envelope>,
    pub last_seq: u64,
    /// `since` is older than the ring: call `run.get` for each live run.
    pub gap: bool,
}

struct RunEntry {
    generation: u64,
    phase: Phase,
    nodes: BTreeMap<String, NodeSnapshot>,
    reply: Option<RunReply>,
    cancel: CancelToken,
    last_seq: u64,
}

struct Table {
    seq: u64,
    ring: VecDeque<Envelope>,
    runs: BTreeMap<String, RunEntry>,
    finished: VecDeque<String>,
    tombstones: BTreeSet<String>,
    schema: Option<SchemaReply>,
    engine: Option<Arc<Engine>>,
    state: EngineState,
    queued: usize,
    /// The same-host verdicts (spec 5.4): the UI reads our files (`shm`), we read the UI's
    /// (`path`). True from the start in-process; set by `session.confirm` on a socket.
    same_host: SessionConfirm,
    /// The nonce file this engine wrote for the UI to read, removed once the UI confirms.
    nonce: Option<String>,
    /// In-process: no proof files, both verdicts hold.
    in_process: bool,
    /// The HTTP front serves blobs by handle: without the proof, values go by `url` locator.
    urls: bool,
}

struct Shared {
    table: Mutex<Table>,
    cv: Condvar,
    wake: Option<Box<dyn Fn() + Send + Sync>>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Table> {
        self.table.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Append an envelope (the one place `seq` is assigned), update the run table, and wake.
    fn push(&self, t: &mut Table, run: Option<(&str, u64)>, event: Event) {
        t.seq += 1;
        let seq = t.seq;
        if let Some((run_id, _)) = run
            && let Some(entry) = t.runs.get_mut(run_id)
        {
            entry.last_seq = seq;
            if let Event::Status(rec) = &event {
                apply_snapshot(entry, rec);
            }
        }
        t.ring.push_back(Envelope::new(seq, now_ts(), run, event));
        while t.ring.len() > RING {
            t.ring.pop_front();
        }
        self.cv.notify_all();
        if let Some(w) = &self.wake {
            w();
        }
    }

    fn finish(&self, reply: RunReply) {
        let mut t = self.lock();
        let run = (reply.run_id.clone(), reply.generation);
        if let Some(entry) = t.runs.get_mut(&run.0) {
            entry.phase = match &reply.error {
                Some(e) if e.kind == NodeError::CANCELLED => Phase::Cancelled,
                Some(_) => Phase::Failed,
                None => Phase::Finished,
            };
            entry.reply = Some(reply.clone());
        }
        t.finished.push_back(run.0.clone());
        while t.finished.len() > KEEP_RUNS {
            if let Some(old) = t.finished.pop_front() {
                t.runs.remove(&old);
            }
        }
        t.queued = t.queued.saturating_sub(1);
        self.push(&mut t, Some((&run.0, run.1)), Event::RunReply(Box::new(reply)));
    }
}

fn apply_snapshot(entry: &mut RunEntry, rec: &StatusRecord) {
    if entry.phase == Phase::Accepted {
        entry.phase = Phase::Running;
    }
    let n = entry.nodes.entry(rec.node_id.clone()).or_default();
    if rec.state == NodeState::Preview {
        if rec.thumb.is_some() {
            n.thumb = rec.thumb.clone();
        }
        return;
    }
    if n.state.is_terminal() {
        return;
    }
    n.state = rec.state;
    n.fraction = rec.fraction.or(n.fraction);
    n.stage = rec.stage.clone().or(n.stage.take());
    n.ms = rec.ms.or(n.ms);
    n.error = rec.error.clone().or(n.error.take());
}

/// The executor's [`Emit`]: straight into the run table.
struct TableEmit<'a> {
    shared: &'a Shared,
    run_id: String,
    generation: u64,
}

impl Emit for TableEmit<'_> {
    fn status(&mut self, rec: StatusRecord) {
        let mut t = self.shared.lock();
        self.shared.push(&mut t, Some((&self.run_id, self.generation)), Event::Status(Box::new(rec)));
    }
}

struct Job {
    req: RunRequest,
    cancel: CancelToken,
}

/// What the worker does, in order.
enum Work {
    Run(Box<Job>),
    Release(Vec<String>),
    ReleaseScope(String),
    Save(Box<SaveRequest>, Sender<SaveReply>),
}

/// An engine hosted in this process.
pub struct Host {
    shared: Arc<Shared>,
    jobs: Option<Sender<Work>>,
    worker: Option<JoinHandle<()>>,
    session: String,
}

impl Host {
    /// Start the worker, which builds the engine from `factory`. `wake` is called after every
    /// event (the push path: the UI repaints and drains).
    pub fn start(factory: Factory, wake: Option<Box<dyn Fn() + Send + Sync>>) -> Host {
        Host::start_with(factory, wake, true)
    }

    /// Start a host for an engine in its own process (a socket or HTTP front): blobs go through
    /// the shared store only after the UI proves it reads this engine's files (spec 5.4).
    pub fn start_proving(factory: Factory, wake: Option<Box<dyn Fn() + Send + Sync>>) -> Host {
        Host::start_with(factory, wake, false)
    }

    fn start_with(factory: Factory, wake: Option<Box<dyn Fn() + Send + Sync>>, in_process: bool) -> Host {
        let shared = Arc::new(Shared {
            table: Mutex::new(Table {
                seq: 0,
                ring: VecDeque::new(),
                runs: BTreeMap::new(),
                finished: VecDeque::new(),
                tombstones: BTreeSet::new(),
                schema: None,
                engine: None,
                state: EngineState::Connecting { url: "inproc".into() },
                queued: 0,
                same_host: SessionConfirm { shm: in_process, path: in_process },
                nonce: None,
                in_process,
                urls: false,
            }),
            cv: Condvar::new(),
            wake,
        });
        let (tx, rx) = channel::<Work>();
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new().name("plumcraft-engine".into()).spawn(move || worker_loop(&worker_shared, factory, &rx)).ok();
        let session = format!("s-{}", std::process::id());
        Host { shared, jobs: Some(tx), worker, session }
    }

    /// Wait until the engine is built (or failed to build), up to `timeout`.
    pub fn wait_ready(&self, timeout: Duration) -> EngineState {
        let deadline = Instant::now() + timeout;
        let mut t = self.shared.lock();
        while matches!(t.state, EngineState::Connecting { .. }) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            t = self.shared.cv.wait_timeout(t, left).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
        t.state.clone()
    }

    pub fn state(&self) -> EngineState {
        self.shared.lock().state.clone()
    }

    /// The engine's store, once it is built.
    pub fn store(&self) -> Option<plumcraft_store::Store> {
        self.shared.lock().engine.as_ref().and_then(|e| e.store().cloned())
    }

    /// The engine's store root, once it is built.
    pub fn work_root(&self) -> Option<std::path::PathBuf> {
        self.shared.lock().engine.as_ref().and_then(|e| e.work_root().map(std::path::Path::to_path_buf))
    }

    pub fn store_root(&self) -> Option<std::path::PathBuf> {
        self.shared.lock().engine.as_ref().and_then(|e| e.store().map(|s| s.root().to_path_buf()))
    }

    pub fn hello(&self, req: &Hello) -> Result<HelloReply, ProtoError> {
        // An engine that takes a while to build (one that starts a child and reads its
        // catalogue) answers `hello` once it is built, not `unavailable` meanwhile.
        self.wait_ready(HELLO_WAIT);
        let (engine, in_process) = {
            let t = self.shared.lock();
            (t.engine.clone().ok_or_else(|| ProtoError::new(ErrorCode::Unavailable, "the engine isn't ready"))?, t.in_process)
        };
        let mut reply = engine.hello(req, &self.session)?;
        reply.caps.insert("data.shm");
        reply.caps.insert("data.path");
        if let Some(store) = engine.store() {
            reply.data.stores = vec![store.root().display().to_string()];
        }
        if let Some(w) = engine.work_root() {
            reply.data.work_root = w.display().to_string();
        }
        if in_process {
            return Ok(reply); // same process: both directions hold without files (spec 5.4)
        }
        // Read the UI's nonce file; write ours for the UI to read. The UI then sends
        // `session.confirm` with both verdicts; until then nothing is published by locator.
        let client_proof_read = req.proof.as_ref().is_some_and(|p| plumcraft_store::check_nonce(&p.path, &p.nonce));
        let ours = engine.store().and_then(|s| plumcraft_store::write_nonce(s.root(), "e").ok());
        // A repeated hello replaces the previous nonce: remove the old file now.
        let old = std::mem::replace(&mut self.shared.lock().nonce, ours.as_ref().map(|(p, _)| p.clone()));
        if let Some(p) = old {
            let _ = std::fs::remove_file(p);
        }
        reply.proof = Some(match ours {
            Some((path, nonce)) => Proof { path, nonce, client_proof_read },
            None => Proof { client_proof_read, ..Default::default() },
        });
        Ok(reply)
    }

    /// The HTTP front calls this: it serves `/v1/blobs/…`, so a session without the proof gets
    /// `url` locators instead of `unavailable` (spec 13.2).
    pub fn serve_blob_urls(&self) {
        self.shared.lock().urls = true;
    }

    /// `session.confirm`: the UI's same-host verdicts (spec 5.4).
    pub fn confirm(&self, c: SessionConfirm) {
        let mut t = self.shared.lock();
        if !t.in_process {
            t.same_host = c;
        }
        // The UI has read (or failed to read) our nonce: it has done its job (spec 5.4).
        if let Some(p) = t.nonce.take() {
            let _ = std::fs::remove_file(p);
        }
    }

    pub fn schema(&self, digest: Option<&str>) -> Result<Value, ProtoError> {
        let t = self.shared.lock();
        let s = t.schema.as_ref().ok_or_else(|| ProtoError::new(ErrorCode::Unavailable, "the engine isn't ready"))?;
        if digest.is_some_and(|d| !d.is_empty() && d == s.engine.schema_digest) {
            return Ok(serde_json::json!({"unchanged": true}));
        }
        serde_json::to_value(s).map_err(|e| ProtoError::new(ErrorCode::Internal, e.to_string()))
    }

    /// Accept a run at once; its result arrives as a `run.reply` event and from `run.get`.
    pub fn run(&self, req: RunRequest) -> Result<RunAck, ProtoError> {
        if req.run_id.is_empty() {
            return Err(ProtoError::new(ErrorCode::BadParams, "run_id is required"));
        }
        let cancel = CancelToken::default();
        let queued_behind = {
            let mut t = self.shared.lock();
            if t.runs.contains_key(&req.run_id) {
                return Err(ProtoError::new(ErrorCode::BadParams, "run_id already used"));
            }
            if t.tombstones.remove(&req.run_id) {
                cancel.cancel();
            }
            let last_seq = t.seq;
            t.runs.insert(
                req.run_id.clone(),
                RunEntry { generation: req.generation, phase: Phase::Accepted, nodes: BTreeMap::new(), reply: None, cancel: cancel.clone(), last_seq },
            );
            t.queued += 1;
            t.queued.saturating_sub(1)
        };
        let run_id = req.run_id.clone();
        let sent = self.jobs.as_ref().is_some_and(|j| j.send(Work::Run(Box::new(Job { req: req.clone(), cancel }))).is_ok());
        if !sent {
            self.shared.finish(RunReply::failed(&req.run_id, req.generation, NodeError::engine("the engine has stopped")));
        }
        Ok(RunAck { accepted: true, run_id, queued_behind: u32::try_from(queued_behind).unwrap_or(u32::MAX) })
    }

    /// `output.save`, waiting for the worker. Nothing is lost if the worker goes first: its
    /// sender drops, and the caller hears why.
    pub fn save(&self, req: SaveRequest) -> SaveReply {
        let (tx, rx) = channel();
        let sent = self.jobs.as_ref().is_some_and(|j| j.send(Work::Save(Box::new(req), tx)).is_ok());
        if !sent {
            return SaveReply::failed(NodeError::engine("the engine has stopped"));
        }
        rx.recv_timeout(SAVE_WAIT).unwrap_or_else(|e| {
            SaveReply::failed(NodeError::engine(match e {
                RecvTimeoutError::Timeout => format!("no answer in {} s", SAVE_WAIT.as_secs()),
                RecvTimeoutError::Disconnected => "the engine has stopped".to_owned(),
            }))
        })
    }

    /// Cancel at once, off the run path. Idempotent; a cancel for a run not yet seen leaves a
    /// tombstone so that run ends without computing (spec 11).
    pub fn cancel(&self, run_id: &str) {
        let mut t = self.shared.lock();
        match t.runs.get(run_id) {
            Some(entry) => entry.cancel.cancel(),
            None => {
                t.tombstones.insert(run_id.to_owned());
            }
        }
    }

    /// `run.get`: the resync path, answered from the table.
    pub fn get(&self, run_id: &str) -> Result<RunSnapshot, ProtoError> {
        let t = self.shared.lock();
        let e = t.runs.get(run_id).ok_or_else(|| ProtoError::new(ErrorCode::NotFound, format!("no run {run_id}")))?;
        Ok(RunSnapshot {
            v: PROTOCOL,
            run_id: run_id.into(),
            generation: e.generation,
            phase: e.phase,
            last_seq: e.last_seq,
            nodes: e.nodes.clone(),
            reply: e.reply.clone(),
            ..Default::default()
        })
    }

    /// `events.poll`: the events after `since`, waiting up to `wait_ms` for the first.
    pub fn poll(&self, since: u64, wait_ms: u64, run_id: Option<&str>) -> PollReply {
        let deadline = Instant::now() + Duration::from_millis(wait_ms.min(MAX_WAIT_MS));
        let mut t = self.shared.lock();
        loop {
            let gap = t.ring.front().is_some_and(|e| e.seq > since.saturating_add(1));
            let events: Vec<Envelope> = t.ring.iter().filter(|e| e.seq > since && run_id.is_none_or(|r| e.run_id.as_deref() == Some(r))).cloned().collect();
            let left = deadline.saturating_duration_since(Instant::now());
            if !events.is_empty() || gap || left.is_zero() {
                let mut events = events;
                if gap {
                    events.insert(
                        0,
                        Envelope::new(t.seq, now_ts(), None, Event::Resync(Resync { run_id: None, reason: "events older than the ring were dropped".into() })),
                    );
                }
                return PollReply { events, last_seq: t.seq, gap };
            }
            t = self.shared.cv.wait_timeout(t, left).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
    }

    /// The JSON face of every operation (spec 6.1), as the socket and HTTP bindings call it.
    pub fn dispatch(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, ProtoError> {
            serde_json::from_value(v).map_err(|e| ProtoError::new(ErrorCode::BadParams, e.to_string()))
        }
        fn json<T: Serialize>(t: &T) -> Result<Value, ProtoError> {
            serde_json::to_value(t).map_err(|e| ProtoError::new(ErrorCode::Internal, e.to_string()))
        }
        let s = |k: &str| params.get(k).and_then(Value::as_str).map(str::to_owned);
        let n = |k: &str| params.get(k).and_then(Value::as_u64).unwrap_or(0);
        match method {
            "hello" => json(&self.hello(&parse(params.clone())?)?),
            "session.confirm" => {
                self.confirm(parse(params.clone())?);
                Ok(serde_json::json!({}))
            }
            "ping" => Ok(serde_json::json!({"v": PROTOCOL, "pong": now_ts()})),
            "schema" => self.schema(s("digest").as_deref()),
            "run" => json(&self.run(parse(params.clone())?)?),
            "run.cancel" => {
                self.cancel(&s("run_id").unwrap_or_default());
                Ok(serde_json::json!({}))
            }
            "run.get" => json(&self.get(&s("run_id").unwrap_or_default())?),
            "output.save" => json(&self.save(parse(params.clone())?)),
            "values.release" => {
                let ids: Vec<String> = parse(params.get("ids").cloned().unwrap_or(Value::Null)).unwrap_or_default();
                if let Some(j) = &self.jobs {
                    let _ = j.send(Work::Release(ids));
                }
                Ok(serde_json::json!({}))
            }
            "graph.release" => {
                if let (Some(j), Some(scope)) = (&self.jobs, s("scope").filter(|s| !s.is_empty())) {
                    let _ = j.send(Work::ReleaseScope(scope));
                }
                Ok(serde_json::json!({}))
            }
            "events.poll" => json(&self.poll(n("since"), n("wait_ms"), s("run_id").as_deref())),
            other if crate::is_pack_op(other) => self.pack_op(other, params),
            other => Err(ProtoError::new(ErrorCode::UnknownOp, format!("unknown op {:?}", other.chars().take(64).collect::<String>()))),
        }
    }

    /// A pack operation (spec 15), on this thread. A handler that panics is the engine's error,
    /// not the host's end.
    fn pack_op(&self, name: &str, params: Value) -> Result<Value, ProtoError> {
        let engine = self.shared.lock().engine.clone().ok_or_else(|| ProtoError::new(ErrorCode::Unavailable, "the engine isn't ready"))?;
        let Some(op) = engine.pack_op(name).cloned() else {
            return Err(ProtoError::new(ErrorCode::UnknownOp, format!("unknown op {:?}", name.chars().take(64).collect::<String>())));
        };
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| op(params)))
            .unwrap_or_else(|_| Err(ProtoError::new(ErrorCode::Internal, format!("the pack operation {name} panicked"))))
    }

    /// Stop the worker after the run in flight, cancelling it.
    pub fn shutdown(&mut self) {
        {
            let t = self.shared.lock();
            for e in t.runs.values() {
                e.cancel.cancel();
            }
        }
        self.jobs = None;
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn set_state(shared: &Shared, state: EngineState) {
    let mut t = shared.lock();
    t.state = state.clone();
    shared.push(&mut t, None, Event::EngineState(state));
}

/// Build the engine and any core of its own; publish its schema to the table. `None` when
/// either fails.
fn build(shared: &Shared, factory: &mut Factory) -> Option<Option<Box<dyn EngineCore>>> {
    let built = factory().and_then(|engine| {
        let state = EngineState::Connected { version: engine.info().version.clone(), nodes: engine.schema().nodes.len() as u64, caps: engine.caps().clone() };
        let core = engine.core().map(|make| make()).transpose()?;
        {
            let mut t = shared.lock();
            t.schema = Some(engine.schema());
            t.engine = Some(Arc::new(engine));
        }
        set_state(shared, state);
        Ok(core)
    });
    match built {
        Ok(core) => Some(core),
        Err(reason) => {
            set_state(shared, EngineState::EngineDown { reason, queues: None, stderr_tail: None, hint: String::new() });
            None
        }
    }
}

/// What a caught panic said.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "a node panicked".into())
}

/// The engine that panicked goes: its state may be inconsistent (spec 16.1). Down, then
/// restarting, so a client that hears of the failure already knows to wait for `connected`.
fn going_down(shared: &Shared, what: &str) {
    shared.lock().engine = None;
    set_state(shared, EngineState::EngineDown { reason: what.to_owned(), queues: None, stderr_tail: None, hint: String::new() });
    set_state(shared, EngineState::Connecting { url: "inproc (restarting)".into() });
}

/// Build a new engine after a panic. `None` once too many have come too fast, or the build fails.
fn rebuild(shared: &Shared, factory: &mut Factory, panics: &mut VecDeque<Instant>) -> Option<Option<Box<dyn EngineCore>>> {
    let now = Instant::now();
    panics.push_back(now);
    while panics.front().is_some_and(|t| now.duration_since(*t) > PANIC_WINDOW) {
        panics.pop_front();
    }
    let rebuilt = if panics.len() >= MAX_PANICS { None } else { build(shared, factory) };
    if rebuilt.is_none() {
        let reason = format!("{} crashes in a minute; stopped", panics.len());
        set_state(shared, EngineState::EngineDown { reason, queues: None, stderr_tail: None, hint: String::new() });
    }
    rebuilt
}

/// How the worker publishes for `engine`.
fn plane_of<'a>(shared: &Shared, engine: &'a Engine) -> Plane<'a> {
    let (shm, urls) = {
        let t = shared.lock();
        (t.same_host.shm, t.urls)
    };
    Plane {
        store: engine.store(),
        shm,
        urls: urls && !shm,
        max_inline: usize::try_from(engine.limits().max_inline).unwrap_or(usize::MAX),
        algo: plumcraft_proto::handle::Algo::Md5,
    }
}

fn worker_loop(shared: &Shared, mut factory: Factory, jobs: &Receiver<Work>) {
    let Some(mut core) = build(shared, &mut factory) else { return };
    let publisher = format!("inproc@{}", std::process::id());
    let mut panics: VecDeque<Instant> = VecDeque::new();
    let mut memo = Memo::default();
    while let Ok(work) = jobs.recv() {
        let job = match work {
            Work::Run(job) => *job,
            Work::Release(ids) => {
                if let Some(c) = core.as_mut() {
                    c.release(&ids);
                }
                continue;
            }
            Work::ReleaseScope(scope) => {
                if let Some(c) = core.as_mut() {
                    c.release_scope(&scope);
                }
                continue;
            }
            Work::Save(req, reply) => {
                let Some(engine) = shared.lock().engine.clone() else {
                    let _ = reply.send(SaveReply::failed(NodeError::engine("the engine has stopped")));
                    continue;
                };
                let plane = plane_of(shared, &engine);
                let result = catch_unwind(AssertUnwindSafe(|| match core.as_mut() {
                    Some(c) => c.save(*req, &plane, &CancelToken::default()),
                    None => SaveReply::unsupported(),
                }));
                let failure = match result {
                    Ok(answer) => {
                        let _ = reply.send(answer);
                        continue;
                    }
                    Err(payload) => panic_text(payload.as_ref()),
                };
                drop(engine);
                memo = Memo::default();
                going_down(shared, &failure);
                let _ = reply.send(SaveReply::failed(NodeError::engine(format!("the engine crashed ({failure}); restarting"))));
                match rebuild(shared, &mut factory, &mut panics) {
                    Some(c) => core = c,
                    None => break,
                }
                continue;
            }
        };
        // The table keeps its handle, so hello and schema answer during the run; the lock is
        // never held while a graph runs.
        let engine = shared.lock().engine.clone();
        let Some(engine) = engine else {
            shared.finish(RunReply::failed(&job.req.run_id, job.req.generation, NodeError::engine("the engine has stopped")));
            continue;
        };
        let plane = Plane { algo: job.req.handle_algo(), ..plane_of(shared, &engine) };
        let mut emit = TableEmit { shared, run_id: job.req.run_id.clone(), generation: job.req.generation };
        let result = catch_unwind(AssertUnwindSafe(|| match core.as_mut() {
            Some(c) => c.run(job.req.clone(), &plane, &mut emit, &job.cancel),
            None => exec::run(&engine, &job.req, &mut emit, &job.cancel, &publisher, &mut memo, &plane),
        }));
        match result {
            Ok(reply) => shared.finish(reply),
            Err(payload) => {
                let what = panic_text(payload.as_ref());
                drop(engine);
                memo = Memo::default(); // results from an engine that panicked aren't trusted
                going_down(shared, &what);
                shared.finish(RunReply::failed(&job.req.run_id, job.req.generation, NodeError::engine(format!("the engine crashed ({what}); restarting"))));
                match rebuild(shared, &mut factory, &mut panics) {
                    Some(c) => core = c,
                    None => break,
                }
            }
        }
    }
    // Anything still queued fails rather than hanging (spec 12.3); a save's sender just drops.
    while let Ok(work) = jobs.try_recv() {
        let Work::Run(job) = work else { continue };
        let job = *job;
        shared.finish(RunReply::failed(&job.req.run_id, job.req.generation, NodeError::engine("the engine has stopped")));
    }
}

#[cfg(test)]
mod core_tests {
    use std::rc::Rc;

    use plumcraft_proto::catalogue::{NodeDef, SchemaReply};
    use plumcraft_proto::session::EngineInfo;

    use super::*;

    /// Not `Send`, as a real core isn't: it can only live on the worker thread.
    struct Counting(Rc<std::cell::Cell<u32>>);

    impl EngineCore for Counting {
        fn run(&mut self, req: RunRequest, _: &crate::data::Plane<'_>, _: &mut dyn Emit, _: &CancelToken) -> RunReply {
            self.0.set(self.0.get() + 1);
            assert!(req.run_id != "panic", "asked to");
            RunReply { run_id: req.run_id, generation: req.generation, ok: true, elapsed_ms: f64::from(self.0.get()), ..Default::default() }
        }

        fn release_scope(&mut self, _: &str) {
            self.0.set(self.0.get() + 10);
        }

        fn save(&mut self, req: SaveRequest, _: &crate::data::Plane<'_>, _: &CancelToken) -> SaveReply {
            self.0.set(self.0.get() + 1);
            assert!(req.node != "panic", "asked to");
            SaveReply { ok: true, paths: vec![req.path.unwrap_or_default()], recipe: true, items: u64::from(self.0.get()), ..Default::default() }
        }
    }

    fn host() -> Host {
        Host::start(
            Box::new(|| {
                let catalogue = SchemaReply { nodes: vec![NodeDef { id: "c.one".into(), ..Default::default() }], ..Default::default() };
                let info = EngineInfo { id: "c".into(), name: "Core".into(), ..Default::default() };
                Ok(Engine::new(info).with_store(None).with_core(catalogue, Box::new(|| Ok(Box::new(Counting(Rc::default()))))))
            }),
            None,
        )
    }

    fn reply(h: &Host, id: &str) -> RunReply {
        h.run(RunRequest::new(id, 1, serde_json::json!({"nodes": {}, "wires": []}))).unwrap();
        for _ in 0..500 {
            if let Some(r) = h.get(id).unwrap().reply {
                return r;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("no reply for {id}");
    }

    #[test]
    fn a_core_runs_graphs_and_answers_its_own_catalogue() {
        let h = host();
        assert!(matches!(h.wait_ready(Duration::from_secs(5)), EngineState::Connected { nodes: 1, .. }));
        assert_eq!(reply(&h, "a").elapsed_ms, 1.0);
        assert_eq!(reply(&h, "b").elapsed_ms, 2.0, "the same core, with its state, runs the next graph");
        let schema = h.schema(None).unwrap();
        assert_eq!(schema["engine"]["id"], "c");
        assert_eq!(schema["nodes"][0]["id"], "c.one");
    }

    #[test]
    fn a_hostile_release_is_harmless() {
        let h = host();
        h.wait_ready(Duration::from_secs(5));
        for junk in [serde_json::json!({"ids": 7}), serde_json::json!(null), serde_json::json!({"ids": [1, null]})] {
            assert!(h.dispatch("values.release", junk).is_ok());
        }
        assert_eq!(reply(&h, "a").elapsed_ms, 1.0, "the worker carries on");
    }

    /// The same worker runs and releases, in the order they were sent.
    #[test]
    fn a_graph_release_reaches_the_core_after_the_run_before_it() {
        let h = host();
        h.wait_ready(Duration::from_secs(5));
        assert_eq!(reply(&h, "a").elapsed_ms, 1.0);
        h.dispatch("graph.release", serde_json::json!({"scope": "g1"})).unwrap();
        assert_eq!(reply(&h, "b").elapsed_ms, 12.0, "the core counted one run, one release (10) and this run");
        for junk in [serde_json::json!(null), serde_json::json!({"scope": 7}), serde_json::json!({"scope": ""}), serde_json::json!([])] {
            assert!(h.dispatch("graph.release", junk).is_ok());
        }
        assert_eq!(reply(&h, "c").elapsed_ms, 13.0, "and nothing that names no scope reaches it");
    }

    fn save_of(node: &str) -> serde_json::Value {
        serde_json::json!({"graph": {"kind": "inline", "graph": {"nodes": {}, "wires": []}}, "node": node, "port": "out", "path": "/x/a.png"})
    }

    fn saved(h: &Host, node: &str) -> SaveReply {
        serde_json::from_value(h.dispatch("output.save", save_of(node)).unwrap()).unwrap()
    }

    #[test]
    fn a_save_takes_its_turn_on_the_core_and_answers() {
        let h = host();
        h.wait_ready(Duration::from_secs(5));
        reply(&h, "a");
        let r = saved(&h, "n");
        assert!(r.ok && r.recipe, "{r:?}");
        assert_eq!((r.paths.as_slice(), r.items), (["/x/a.png".to_owned()].as_slice(), 2), "the same core, after the run");
        assert_eq!(reply(&h, "b").elapsed_ms, 3.0, "and runs carry on");
        let bad = h.dispatch("output.save", serde_json::json!({"index": "two"}));
        assert_eq!(bad.unwrap_err().code, ErrorCode::BadParams, "hostile params are an error, not a panic");
    }

    #[test]
    fn a_core_that_panics_in_a_save_is_rebuilt_and_the_save_says_so() {
        let h = host();
        h.wait_ready(Duration::from_secs(5));
        reply(&h, "a");
        let r = saved(&h, "panic");
        assert!(!r.ok);
        assert!(r.error.as_ref().unwrap().message.contains("crashed"), "{r:?}");
        h.wait_ready(Duration::from_secs(5));
        assert_eq!(reply(&h, "c").elapsed_ms, 1.0, "a new core, its state gone");
    }

    #[test]
    fn an_engine_with_no_core_of_its_own_says_it_cannot_save() {
        let info = EngineInfo { id: "plain".into(), name: "Plain".into(), ..Default::default() };
        let h = Host::start(Box::new(move || Ok(Engine::new(info.clone()).with_store(None))), None);
        h.wait_ready(Duration::from_secs(5));
        let r = saved(&h, "n");
        assert!(!r.ok && r.error.unwrap().message.contains("not supported"));
    }

    #[test]
    fn a_stopped_host_answers_a_save() {
        let mut h = host();
        h.wait_ready(Duration::from_secs(5));
        h.shutdown();
        let r = h.save(SaveRequest::default());
        assert!(!r.ok && r.error.unwrap().message.contains("stopped"));
    }

    #[test]
    fn a_core_that_panics_is_rebuilt() {
        let h = host();
        h.wait_ready(Duration::from_secs(5));
        reply(&h, "a");
        assert!(reply(&h, "panic").error.is_some());
        h.wait_ready(Duration::from_secs(5));
        assert_eq!(reply(&h, "c").elapsed_ms, 1.0, "a new core, its state gone");
    }
}
