//! Mode 2 (spec 16.3): an engine in its own process, on a Unix socket in a private directory.
//!
//! - **Launch:** the host listens, spawns the engine with `PLUMCRAFT_ENGINE_SOCKET` and a 256-bit
//!   `PLUMCRAFT_ENGINE_TOKEN`, and the engine connects back and presents the token in `hello`.
//!   Stdout is ignored; stderr goes to a 64 KiB tail shown when the engine dies.
//! - **Events** arrive as pushed notifications and are re-sequenced into the host's own ring, so
//!   `events.poll` and resume survive a restart.
//! - **Supervision:** a crash (`kill -9` included) fails every run in flight with
//!   `kind: engine` (spec 12.3: exactly one outcome per request), reports `engine_down` with the
//!   stderr tail, and restarts with backoff; five crashes in a minute stop it.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{BufReader, Read};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use plumcraft_engine::rpc::{self, Frame, Request};
use plumcraft_engine::serve::{ENV_SOCKET, ENV_TOKEN};
use plumcraft_proto::PROTOCOL;
use plumcraft_proto::data::DataRef;
use plumcraft_proto::error::{ErrorCode, NodeError, ProtoError};
use plumcraft_proto::event::{Envelope, Event};
use plumcraft_proto::run::{Phase, RunReply, RunSnapshot};
use plumcraft_proto::session::{EngineState, Proof};
use plumcraft_store::{Blob, Store};
use serde_json::{Value, json};

use crate::Connection;
use crate::ring::Ring;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// `output.save` runs the graph for its node (spec 6.1: 300 s; the engine's own limit answers
/// first).
const SAVE_TIMEOUT: Duration = Duration::from_secs(310);
const STDERR_TAIL: usize = 64 << 10;
const MAX_CRASHES: usize = 5;
const CRASH_WINDOW: Duration = Duration::from_secs(60);

/// How to start an engine process.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub name: String,
    /// The program and its arguments.
    pub command: Vec<String>,
    pub env: Vec<(String, String)>,
    /// The banner hint when the engine is down.
    pub hint: String,
    /// A commonkit store root for both sides (the engine gets `COMMONKIT_STORE_ROOT`); `None`
    /// uses the shared default. Tests use a private root.
    pub store: Option<PathBuf>,
    /// Where it runs; `None` is here.
    pub cwd: Option<PathBuf>,
}

type Reply = Result<Value, ProtoError>;

struct Inner {
    manifest: Manifest,
    token: String,
    writer: Mutex<Option<UnixStream>>,
    pending: Mutex<HashMap<u64, Sender<Reply>>>,
    next_id: AtomicU64,
    ring: Ring,
    /// Runs sent and not yet answered: failed for the UI if the engine dies.
    live: Mutex<BTreeMap<String, u64>>,
    /// Replies the host synthesised, for `run.get` after a restart.
    synthesised: Mutex<BTreeMap<String, RunReply>>,
    state: Mutex<EngineState>,
    state_cv: Condvar,
    child: Mutex<Option<Child>>,
    stderr: Arc<Mutex<VecDeque<u8>>>,
    stop: AtomicBool,
    /// The UI's store, for its proof file and for reading blobs.
    store: Store,
    /// This connection's same-host verdicts and the engine's declared store roots.
    data: Mutex<DataSession>,
}

/// What the same-host proof established for the current engine process (spec 5.4).
#[derive(Debug, Clone, Default)]
pub struct DataSession {
    /// The UI read the engine's nonce: its `commonkit` and `path` locators are usable.
    pub shm: bool,
    /// The engine read the UI's nonce.
    pub path: bool,
    /// The engine's store roots (`hello.data.stores`).
    pub stores: Vec<String>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Inner {
    fn set_state(&self, s: EngineState) {
        *lock(&self.state) = s.clone();
        self.state_cv.notify_all();
        self.ring.push_event(None, Event::EngineState(s));
    }

    /// Send a request and wait for its reply.
    fn request(&self, method: &str, params: Value, timeout: Duration) -> Reply {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = channel();
        lock(&self.pending).insert(id, tx);
        let sent = {
            let mut w = lock(&self.writer);
            match w.as_mut() {
                Some(s) => rpc::write_frame(s, &Request::call(id, method, params)).is_ok(),
                None => false,
            }
        };
        if !sent {
            lock(&self.pending).remove(&id);
            return Err(ProtoError::new(ErrorCode::Unavailable, "the engine is not connected"));
        }
        let out = rx.recv_timeout(timeout).unwrap_or_else(|_| Err(ProtoError::new(ErrorCode::Unavailable, format!("{method}: no answer in {timeout:?}"))));
        lock(&self.pending).remove(&id);
        out
    }

    /// The engine is gone: every pending call and every live run gets its one outcome.
    fn fail_everything(&self, why: &str) {
        for (_, tx) in lock(&self.pending).drain() {
            let _ = tx.send(Err(ProtoError::new(ErrorCode::Unavailable, why.to_owned())));
        }
        let live: Vec<(String, u64)> = std::mem::take(&mut *lock(&self.live)).into_iter().collect();
        for (run_id, generation) in live {
            let reply = RunReply::failed(&run_id, generation, NodeError::engine(why));
            lock(&self.synthesised).insert(run_id.clone(), reply.clone());
            self.ring.push_event(Some((&run_id, generation)), Event::RunReply(Box::new(reply)));
        }
    }

    fn stderr_tail(&self) -> String {
        let b = lock(&self.stderr);
        let bytes: Vec<u8> = b.iter().copied().collect();
        let text = String::from_utf8_lossy(&bytes);
        text.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
    }
}

/// An engine process, supervised.
pub struct ProcessHost {
    inner: Arc<Inner>,
    supervisor: Option<JoinHandle<()>>,
    dir: PathBuf,
}

impl ProcessHost {
    pub fn start(manifest: Manifest) -> std::io::Result<ProcessHost> {
        let manifest_store = manifest.store.clone();
        let dir = private_dir("plumcraft")?;
        let socket = dir.join("e.sock");
        let listener = UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let inner = Arc::new(Inner {
            manifest,
            token: token()?,
            writer: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            ring: Ring::default(),
            live: Mutex::new(BTreeMap::new()),
            synthesised: Mutex::new(BTreeMap::new()),
            state: Mutex::new(EngineState::Connecting { url: format!("unix:{}", socket.display()) }),
            state_cv: Condvar::new(),
            child: Mutex::new(None),
            stderr: Arc::new(Mutex::new(VecDeque::new())),
            stop: AtomicBool::new(false),
            store: Store::new(manifest_store.unwrap_or_else(plumcraft_store::default_root)),
            data: Mutex::new(DataSession::default()),
        });
        let sup = Arc::clone(&inner);
        let supervisor = std::thread::Builder::new().name("plumcraft-supervisor".into()).spawn(move || supervise(&sup, &listener, &socket))?;
        Ok(ProcessHost { inner, supervisor: Some(supervisor), dir })
    }

    /// Wait until connected (or failed), up to `timeout`.
    pub fn wait_ready(&self, timeout: Duration) -> EngineState {
        let deadline = Instant::now() + timeout;
        let mut s = lock(&self.inner.state);
        while matches!(*s, EngineState::Connecting { .. } | EngineState::EngineDown { .. }) && !self.inner.stop.load(Ordering::SeqCst) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            s = self.inner.state_cv.wait_timeout(s, left).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
        s.clone()
    }

    /// The engine's process id, while it runs.
    pub fn pid(&self) -> Option<u32> {
        lock(&self.inner.child).as_ref().map(Child::id)
    }

    /// What the same-host proof established for the current engine process.
    pub fn data_session(&self) -> DataSession {
        lock(&self.inner.data).clone()
    }

    /// Kill the engine process with SIGKILL (tests, and a stuck engine).
    pub fn kill_engine(&self) {
        if let Some(c) = lock(&self.inner.child).as_mut() {
            kill_group(c);
        }
    }
}

impl Connection for ProcessHost {
    fn readable_roots(&self) -> Vec<String> {
        let data = self.data_session();
        if data.shm { data.stores } else { Vec::new() }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        let inner = &self.inner;
        match method {
            "events.poll" => {
                let n = |k: &str| params.get(k).and_then(Value::as_u64).unwrap_or(0);
                let p = inner.ring.poll(n("since"), n("wait_ms"), params.get("run_id").and_then(Value::as_str));
                serde_json::to_value(p).map_err(|e| ProtoError::new(ErrorCode::Internal, e.to_string()))
            }
            "run.get" => {
                let run_id = params.get("run_id").and_then(Value::as_str).unwrap_or_default().to_owned();
                if let Some(reply) = lock(&inner.synthesised).get(&run_id).cloned() {
                    let snap =
                        RunSnapshot { v: PROTOCOL, run_id, generation: reply.generation, phase: Phase::Failed, reply: Some(reply), ..Default::default() };
                    return serde_json::to_value(snap).map_err(|e| ProtoError::new(ErrorCode::Internal, e.to_string()));
                }
                inner.request(method, params, CALL_TIMEOUT)
            }
            "run" => {
                let run_id = params.get("run_id").and_then(Value::as_str).unwrap_or_default().to_owned();
                let generation = params.get("generation").and_then(Value::as_u64).unwrap_or(0);
                lock(&inner.live).insert(run_id.clone(), generation);
                let out = inner.request(method, params, CALL_TIMEOUT);
                if out.is_err() {
                    // Not accepted: it will never have a reply from the engine.
                    lock(&inner.live).remove(&run_id);
                }
                out
            }
            "output.save" => inner.request(method, params, SAVE_TIMEOUT),
            "hello" => {
                let mut p = params;
                if let Some(o) = p.as_object_mut() {
                    o.insert("token".into(), json!(inner.token));
                }
                inner.request(method, p, CALL_TIMEOUT)
            }
            _ => inner.request(method, params, CALL_TIMEOUT),
        }
    }

    fn state(&self) -> EngineState {
        lock(&self.inner.state).clone()
    }

    fn resolve(&self, r: &DataRef) -> Option<Blob> {
        let d = lock(&self.inner.data).clone();
        self.inner.store.resolve_in(r, d.shm, &d.stores)
    }
}

impl Drop for ProcessHost {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::SeqCst);
        if let Some(w) = lock(&self.inner.writer).as_mut() {
            let _ = rpc::write_frame(w, &Request::notify("shutdown", Value::Null));
            let _ = w.shutdown(std::net::Shutdown::Both);
        }
        // Shutdown: 2 s grace, then SIGKILL (spec 16.3, without SIGTERM: std has no signal API).
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let exited = lock(&self.inner.child).as_mut().is_none_or(|c| matches!(c.try_wait(), Ok(Some(_))));
            if exited {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.kill_engine();
        if let Some(s) = self.supervisor.take() {
            let _ = s.join();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn supervise(inner: &Arc<Inner>, listener: &UnixListener, socket: &Path) {
    let mut crashes: VecDeque<Instant> = VecDeque::new();
    while !inner.stop.load(Ordering::SeqCst) {
        let why = match connect_once(inner, listener, socket) {
            Ok(why) => why,
            Err(e) => format!("could not start the engine: {e}"),
        };
        if inner.stop.load(Ordering::SeqCst) {
            break;
        }
        inner.fail_everything(&why);
        inner.set_state(EngineState::EngineDown {
            reason: why.clone(),
            queues: None,
            stderr_tail: Some(inner.stderr_tail()),
            hint: inner.manifest.hint.clone(),
        });
        let now = Instant::now();
        crashes.push_back(now);
        while crashes.front().is_some_and(|t| now.duration_since(*t) > CRASH_WINDOW) {
            crashes.pop_front();
        }
        if crashes.len() >= MAX_CRASHES {
            inner.set_state(EngineState::EngineDown {
                reason: format!("{} crashes in a minute; stopped. Last: {why}", crashes.len()),
                queues: None,
                stderr_tail: Some(inner.stderr_tail()),
                hint: inner.manifest.hint.clone(),
            });
            break;
        }
        // Backoff: 100 ms, 200, 400 … up to 2 s.
        let shift = u32::try_from(crashes.len().saturating_sub(1)).unwrap_or(4).min(4);
        std::thread::sleep(Duration::from_millis(100u64 << shift).min(Duration::from_secs(2)));
        inner.set_state(EngineState::Connecting { url: format!("unix:{} (restarting)", socket.display()) });
    }
}

/// Spawn, accept, read until the connection ends. Returns why it ended.
fn connect_once(inner: &Arc<Inner>, listener: &UnixListener, socket: &Path) -> std::io::Result<String> {
    let m = &inner.manifest;
    let (program, args) = m.command.split_first().ok_or_else(|| std::io::Error::other("the manifest has no command"))?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .envs(m.env.iter().cloned())
        .env(ENV_SOCKET, socket)
        .env(ENV_TOKEN, &inner.token)
        .env(plumcraft_store::ROOT_ENV, inner.store.root())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .process_group(0); // its own group, so supervision can signal all of it
    if let Some(dir) = &m.cwd {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn()?;
    if let Some(mut err) = child.stderr.take() {
        let tail = Arc::clone(&inner.stderr);
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut t = lock(&tail);
                t.extend(buf.iter().take(n));
                while t.len() > STDERR_TAIL {
                    t.pop_front();
                }
            }
        });
    }
    *lock(&inner.child) = Some(child);

    // Accept within the connect timeout, or notice the engine died first.
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let stream = loop {
        match listener.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if let Some(status) = lock(&inner.child).as_mut().and_then(|c| c.try_wait().ok().flatten()) {
                    return Ok(format!("the engine exited before connecting ({})", describe(status)));
                }
                if Instant::now() > deadline || inner.stop.load(Ordering::SeqCst) {
                    kill(inner);
                    return Ok("the engine didn't connect within 10 s".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e),
        }
    };
    stream.set_nonblocking(false)?;
    *lock(&inner.writer) = Some(stream.try_clone()?);

    // Read on its own thread; this one says hello and then watches.
    let reader_inner = Arc::clone(inner);
    let reader = std::thread::Builder::new().name("plumcraft-reader".into()).spawn(move || read_loop(&reader_inner, stream))?;
    *lock(&inner.data) = DataSession::default();
    let ours = plumcraft_store::write_nonce(inner.store.root(), "u").ok();
    let mut hello_req = json!({"v": PROTOCOL, "op": "hello", "token": inner.token, "caps": ["events.push", "events.poll", "data.shm", "data.path"],
                               "host": {"stores": [inner.store.root().display().to_string()]}});
    if let Some((path, nonce)) = &ours {
        hello_req["proof"] = json!({"path": path, "nonce": nonce});
    }
    let hello = inner.request("hello", hello_req, CONNECT_TIMEOUT);
    if let Some((path, _)) = &ours {
        let _ = std::fs::remove_file(path); // read or not, it has done its job
    }
    match hello {
        Ok(h) => {
            // The proof, both directions (spec 5.4), then tell the engine.
            let proof = h.get("proof").cloned().and_then(|p| serde_json::from_value::<Proof>(p).ok()).unwrap_or_default();
            let shm = plumcraft_store::check_nonce(&proof.path, &proof.nonce);
            let mut stores: Vec<String> = h.get("data").and_then(|d| d.get("stores")).cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
            // The work root's files are the engine's own, readable after the proof (spec 4.4).
            stores.extend(h.get("data").and_then(|d| d.get("work_root")).and_then(Value::as_str).filter(|w| !w.is_empty()).map(str::to_owned));
            *lock(&inner.data) = DataSession { shm, path: proof.client_proof_read, stores };
            let _ = inner.request("session.confirm", json!({"shm": shm, "path": proof.client_proof_read}), CONNECT_TIMEOUT);
            let schema = inner.request("schema", json!({}), CONNECT_TIMEOUT).ok();
            let nodes = schema.as_ref().and_then(|s| s.get("nodes")).and_then(Value::as_array).map_or(0, |n| n.len() as u64);
            let version = h.get("engine").and_then(|e| e.get("version")).and_then(Value::as_str).unwrap_or_default().to_owned();
            let caps = h.get("caps").cloned().and_then(|c| serde_json::from_value(c).ok()).unwrap_or_default();
            inner.set_state(EngineState::Connected { version, nodes, caps });
        }
        Err(e) => {
            kill(inner);
            let _ = reader.join();
            return Ok(format!("hello failed: {}", e.message));
        }
    }
    let _ = reader.join();
    *lock(&inner.writer) = None;
    // The connection ended: reap the process and say how it ended.
    let status = {
        let mut c = lock(&inner.child);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut status = None;
        while let Some(child) = c.as_mut() {
            if let Ok(Some(s)) = child.try_wait() {
                status = Some(s);
                break;
            }
            if Instant::now() > deadline {
                kill_group(child);
                status = child.wait().ok();
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        *c = None;
        status
    };
    Ok(match status {
        Some(s) => format!("the engine stopped ({}); restarting", describe(s)),
        None => "the engine closed its connection; restarting".into(),
    })
}

fn kill(inner: &Inner) {
    if let Some(c) = lock(&inner.child).as_mut() {
        kill_group(c);
        let _ = c.wait();
    }
}

/// SIGKILL the engine's whole process group (spec 16.3, "Supervision: process group"): an
/// engine started through a wrapper (`uv run`, a shell) has its real process as a child, which
/// a kill of the wrapper alone would orphan, still holding the socket. std has no `killpg`, and
/// this crate allows no `unsafe`, so the group is signalled with `kill(1)`; the direct kill
/// follows in case that isn't available.
pub(crate) fn kill_group(c: &mut Child) {
    let _ = Command::new("kill").args(["-KILL", "--", &format!("-{}", c.id())]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    let _ = c.kill();
}

fn describe(s: std::process::ExitStatus) -> String {
    match (s.code(), s.signal()) {
        (Some(c), _) => format!("exit {c}"),
        (None, Some(sig)) => format!("signal {sig}"),
        _ => "unknown".into(),
    }
}

fn read_loop(inner: &Inner, stream: UnixStream) {
    let mut r = BufReader::new(stream);
    while let Ok(Some(line)) = rpc::read_frame(&mut r, rpc::MAX_FRAME) {
        match rpc::parse(&line) {
            Ok(Frame::Response(resp)) => {
                if let Some(tx) = resp.id.and_then(|id| lock(&inner.pending).remove(&id)) {
                    let _ = tx.send(resp.into_result());
                }
            }
            Ok(Frame::Request(note)) if note.method == "event" => {
                let Ok(env) = serde_json::from_value::<Envelope>(note.params) else { continue };
                if let Event::RunReply(r) = &env.event {
                    lock(&inner.live).remove(&r.run_id);
                }
                // The engine's own state is its own; the host reports engine states.
                if !matches!(env.event, Event::EngineState(_)) {
                    inner.ring.push(env);
                }
            }
            _ => {}
        }
    }
}

/// A fresh 0700 directory for the socket.
/// A new folder only this user can enter, `<prefix>-<pid>-<n>`. It must be new: one already there
/// may be someone else's, with files planted in it.
pub(crate) fn private_dir(prefix: &str) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    static N: AtomicU64 = AtomicU64::new(0);
    // the directories of hosts whose process died before dropping them
    plumcraft_scratch::sweep_once();
    let mut tries = 0;
    loop {
        let dir = std::env::temp_dir().join(format!("{prefix}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && tries < 64 => tries += 1,
            Err(e) => return Err(e),
        }
    }
}

/// 256 random bits as hex.
pub(crate) fn token() -> std::io::Result<String> {
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}
