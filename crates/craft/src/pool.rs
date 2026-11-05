//! The process pool (note 14, 3.7; D41, D46, D49).
//!
//! - Every process is `<craft> mcp <read-root CAS> <write-root WORK>`: hermetic and headless
//!   (roots imply headless, so the adapter can never drive the maintainer's open app).
//! - A node leases a process, preferring one whose active document is the node's input (warm:
//!   no reopen). Up to `pool.max` run at once; more wait.
//! - A process idle for more than `pool.idle_s` is retired, down to one. One idle for more than
//!   a second is pinged (2 s) before reuse; one that doesn't answer is killed.
//! - A lease whose call failed in a way that leaves the process unknown (killed, timed out,
//!   crashed) is discarded, never returned. Five crashes in a minute stop the pool: every node
//!   then fails at once with the manifest's hint.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use plumcraft_proto::Handle;
use plumcraft_proto::error::NodeError;

use crate::mcp::Client;

/// Crashes within [`CRASH_WINDOW`] that stop the pool.
pub const MAX_CRASHES: usize = 5;
pub const CRASH_WINDOW: Duration = Duration::from_secs(60);
/// A process idle this long is pinged before reuse.
const PING_AFTER: Duration = Duration::from_secs(1);
const PING_TIMEOUT: Duration = Duration::from_secs(2);
const INIT_TIMEOUT: Duration = Duration::from_secs(20);

/// How to start one process.
#[derive(Debug, Clone)]
pub struct Launch {
    pub name: String,
    /// The program, its arguments, and the root flags.
    pub command: Vec<String>,
    pub env: Vec<(String, String)>,
    pub hint: String,
    pub max: usize,
    pub idle: Duration,
}

impl Launch {
    pub fn new(m: &crate::manifest::Manifest, read_root: &std::path::Path, write_root: &std::path::Path) -> Launch {
        let mut command = m.program();
        if let Some(r) = &m.roots {
            command.extend([r.read.clone(), read_root.display().to_string(), r.write.clone(), write_root.display().to_string()]);
        }
        Launch {
            name: m.name.clone(),
            command,
            env: m.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            hint: m.hint.clone(),
            max: m.pool.max.max(1),
            idle: Duration::from_secs(m.pool.idle_s),
        }
    }
}

/// One craft process and what it holds.
pub struct Proc {
    pub client: Client,
    /// The document the process holds as its active one, exactly as saved under this handle.
    /// `None` after anything that may have changed it without a save.
    pub active: Option<Handle>,
    /// Nodes run on the active document since its history was cleared.
    pub since_clear: u32,
    /// Nothing has been run on it since it started: it holds no documents.
    pub fresh: bool,
    /// The id of the document it has open, for a craft that names documents by id
    /// (`doc.id_param`).
    pub doc_id: Option<serde_json::Value>,
    last_used: Instant,
}

pub struct Pool {
    launch: Launch,
    idle: Mutex<Vec<Proc>>,
    cv: Condvar,
    live: AtomicUsize,
    crashes: Mutex<VecDeque<Instant>>,
    stopped: Mutex<Option<String>>,
    spawned: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Pool {
    pub fn new(launch: Launch) -> Pool {
        Pool {
            launch,
            idle: Mutex::new(Vec::new()),
            cv: Condvar::new(),
            live: AtomicUsize::new(0),
            crashes: Mutex::new(VecDeque::new()),
            stopped: Mutex::new(None),
            spawned: AtomicU64::new(0),
        }
    }

    pub fn launch(&self) -> &Launch {
        &self.launch
    }

    /// Processes started so far (respawns included).
    pub fn spawned(&self) -> u64 {
        self.spawned.load(Ordering::SeqCst)
    }

    /// Processes alive now (idle or leased).
    pub fn live(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }

    /// The pids of the idle processes (for the crash tests).
    pub fn idle_pids(&self) -> Vec<u32> {
        lock(&self.idle).iter().filter_map(|p| p.client.pid()).collect()
    }

    /// Start a process: spawn and `initialize`.
    pub fn spawn(&self) -> Result<Proc, NodeError> {
        let mut client = Client::spawn(&self.launch.command, &self.launch.env).map_err(|e| {
            let prog = self.launch.command.first().map(String::as_str).unwrap_or_default();
            engine_error(format!("{} can't be started ({prog}: {e}); {}", self.launch.name, self.launch.hint))
        })?;
        self.spawned.fetch_add(1, Ordering::SeqCst);
        client.initialize(INIT_TIMEOUT).map_err(|e| engine_error(format!("{} didn't initialize: {e}; {}", self.launch.name, self.launch.hint)))?;
        Ok(Proc { client, active: None, since_clear: 0, fresh: true, doc_id: None, last_used: Instant::now() })
    }

    /// A process, warm for `want` when one is. Waits while `max` are leased, until `deadline`.
    pub fn lease(&self, want: Option<&Handle>, deadline: Instant) -> Result<Lease<'_>, NodeError> {
        if let Some(why) = lock(&self.stopped).clone() {
            return Err(engine_error(why));
        }
        loop {
            let picked = {
                let mut idle = lock(&self.idle);
                self.retire(&mut idle);
                let warm = want.and_then(|h| idle.iter().position(|p| p.active.as_ref() == Some(h)));
                match warm.or_else(|| idle.iter().position(|p| p.active.is_none())).or(if idle.is_empty() { None } else { Some(0) }) {
                    Some(i) => Some(idle.swap_remove(i)),
                    None if self.live.load(Ordering::SeqCst) < self.launch.max => {
                        self.live.fetch_add(1, Ordering::SeqCst);
                        None
                    }
                    None => {
                        let left = deadline.saturating_duration_since(Instant::now());
                        if left.is_zero() {
                            return Err(engine_error(format!("every {} process stayed busy", self.launch.name)));
                        }
                        let _ = self.cv.wait_timeout(idle, left.min(Duration::from_millis(200)));
                        continue;
                    }
                }
            };
            match picked {
                Some(p) => {
                    // A process idle for a while may have died or wedged (R6): ping it first.
                    if p.client.is_dead() || (p.last_used.elapsed() > PING_AFTER && !p.client.ping(PING_TIMEOUT)) {
                        drop(p);
                        self.live.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    return Ok(Lease { pool: self, proc: Some(p), discard: false });
                }
                None => match self.spawn() {
                    Ok(p) => return Ok(Lease { pool: self, proc: Some(p), discard: false }),
                    Err(e) => {
                        self.live.fetch_sub(1, Ordering::SeqCst);
                        return Err(e);
                    }
                },
            }
        }
    }

    /// Retire processes idle longer than `idle`, keeping one.
    fn retire(&self, idle: &mut Vec<Proc>) {
        while idle.len() > 1 {
            let Some(i) = idle.iter().position(|p| p.last_used.elapsed() > self.launch.idle) else { break };
            drop(idle.swap_remove(i));
            self.live.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Count a crash; the fifth in a minute stops the pool.
    pub fn crashed(&self) {
        let mut c = lock(&self.crashes);
        let now = Instant::now();
        c.push_back(now);
        while c.front().is_some_and(|t| now.duration_since(*t) > CRASH_WINDOW) {
            c.pop_front();
        }
        if c.len() >= MAX_CRASHES {
            *lock(&self.stopped) =
                Some(format!("{} crashed {MAX_CRASHES} times in a minute; the adapter stopped starting it. {}", self.launch.name, self.launch.hint));
        }
    }

    /// Kill every idle process (the leased ones die when their lease ends).
    pub fn shutdown(&self) {
        let mut idle = lock(&self.idle);
        let n = idle.len();
        idle.clear();
        self.live.fetch_sub(n, Ordering::SeqCst);
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A leased process. Returned to the pool when dropped, unless discarded.
pub struct Lease<'a> {
    pool: &'a Pool,
    proc: Option<Proc>,
    discard: bool,
}

impl Lease<'_> {
    /// The process can't be trusted: kill it when the lease ends.
    pub fn discard(&mut self) {
        self.discard = true;
        if let Some(p) = self.proc.as_mut() {
            p.active = None;
        }
    }

    pub fn proc(&mut self) -> Option<&mut Proc> {
        self.proc.as_mut()
    }

    /// Replace the process with a fresh one (a craft that can't close every document, or whose
    /// close failed). On failure the lease holds no process and the error says why.
    pub fn respawn(&mut self) -> Result<(), NodeError> {
        if let Some(old) = self.proc.take() {
            old.client.kill();
        }
        match self.pool.spawn() {
            Ok(p) => {
                self.proc = Some(p);
                Ok(())
            }
            Err(e) => {
                self.pool.live.fetch_sub(1, Ordering::SeqCst);
                self.pool.cv.notify_all();
                Err(e)
            }
        }
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let Some(mut p) = self.proc.take() else { return };
        if self.discard || p.client.is_dead() {
            p.client.kill();
            drop(p);
            self.pool.live.fetch_sub(1, Ordering::SeqCst);
        } else {
            p.last_used = Instant::now();
            lock(&self.pool.idle).push(p);
        }
        self.pool.cv.notify_all();
    }
}

pub fn engine_error(message: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::ENGINE, message)
}

/// Where the adapter keeps its CAS when nothing else is said: `$PLUMCRAFT_CRAFT_ROOT/<id>`, else
/// the user's cache folder.
pub fn default_root(id: &str) -> PathBuf {
    if let Some(r) = std::env::var_os("PLUMCRAFT_CRAFT_ROOT").filter(|r| !r.is_empty()) {
        return PathBuf::from(r).join(id);
    }
    let home = plumcraft_proto::programs::home().unwrap_or_else(std::env::temp_dir);
    let base = if cfg!(target_os = "macos") { home.join("Library/Caches") } else { home.join(".cache") };
    base.join("plumcraft/craft").join(id)
}
