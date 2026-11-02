//! The MCP client (plan D40): newline-delimited JSON-RPC 2.0 over a child's stdio, on plain
//! threads, with `plumcraft-engine`'s [`rpc`] framing.
//!
//! - **One reader thread** reads every line the server writes, bounded by [`MAX_REPLY`]: a reply
//!   goes to the call waiting on its id, a `notifications/progress` to the call whose
//!   `progressToken` it names. Garbage lines, replies to ids nobody waits for, string ids and
//!   unknown notifications are skipped. A line over the bound kills the server: its stream can't
//!   be resynchronised.
//! - **A call** waits for its reply with a timeout, polling the caller's cancel flag every 20 ms.
//!   On cancel it sends `notifications/cancelled`, gives the server [`CANCEL_GRACE`] to answer,
//!   then kills it (D41): a VectorCraft trace ignores the cancel and a cancelled call is never
//!   answered, so waiting can't tell "stopped" from "still running".
//! - **End of stream** (the server exited, crashed or was killed) fails every pending call with
//!   [`CallError::Dead`], carrying the tail of its stderr.
//!
//! The transport is any reader and writer, so the tests drive a fake server over pipes.

use std::collections::{HashMap, VecDeque};
use std::io::{BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use plumcraft_engine::rpc::{self, Frame, Request};
use plumcraft_proto::error::ProtoError;
use serde_json::{Value, json};

/// The longest reply line accepted (note 14, section 3.7). VectorCraft caps its own request
/// lines at 16 MiB; replies can be larger (inline exports), but the adapter never asks for those.
pub const MAX_REPLY: usize = 64 << 20;
/// How long a cancelled call's server gets to answer before it is killed (D41).
pub const CANCEL_GRACE: Duration = Duration::from_millis(200);
/// The stderr bytes kept per server.
pub const STDERR_TAIL: usize = 64 << 10;
/// How often a waiting call checks its cancel flag.
const POLL: Duration = Duration::from_millis(20);
/// The MCP revision asked for (VectorCraft's default; SoundCraft answers it too).
pub const MCP_REVISION: &str = "2025-06-18";

/// Why a call has no result.
#[derive(Debug, Clone, PartialEq)]
pub enum CallError {
    /// No reply within the timeout; the server was killed.
    Timeout { method: String, after: Duration },
    /// The caller cancelled; the server was killed unless it answered within the grace.
    Cancelled,
    /// The server is gone: exited, crashed, killed, or sent a line over [`MAX_REPLY`].
    Dead { reason: String, stderr: String },
    /// A JSON-RPC error reply.
    Rpc(ProtoError),
    /// The request couldn't be written.
    Io(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Timeout { method, after } => write!(f, "no answer to {method} in {:.0} s", after.as_secs_f64()),
            CallError::Cancelled => write!(f, "cancelled"),
            CallError::Dead { reason, stderr } if stderr.is_empty() => write!(f, "{reason}"),
            CallError::Dead { reason, stderr } => write!(f, "{reason}; stderr: {stderr}"),
            CallError::Rpc(e) => write!(f, "{e}"),
            CallError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// One `notifications/progress`.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub progress: f64,
    pub total: Option<f64>,
    pub message: Option<String>,
}

/// What a waiting call is told.
enum Msg {
    Reply(Result<Value, ProtoError>),
    Progress(Progress),
    Dead(String),
}

/// What a call watches while it waits: the run's cancel flag and a progress sink.
pub trait Watch {
    fn cancelled(&self) -> bool;
    fn progress(&mut self, p: &Progress);
}

/// A watch that never cancels and drops progress.
pub struct NoWatch;

impl Watch for NoWatch {
    fn cancelled(&self) -> bool {
        false
    }

    fn progress(&mut self, _: &Progress) {}
}

/// A tool call's outcome: MCP tool errors are results with `isError`, not JSON-RPC errors.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub is_error: bool,
    /// The text content items, joined.
    pub text: String,
    /// The text, or else one content item, parsed as JSON (or `structuredContent`).
    pub value: Option<Value>,
}

impl ToolResult {
    fn from_result(v: &Value) -> ToolResult {
        let is_error = v.get("isError").and_then(Value::as_bool).unwrap_or(false);
        let text: String = v
            .get("content")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(|c| c.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        // A craft may add a note after the JSON as a second item, so the joined text isn't JSON.
        let item_json = || {
            let items = v.get("content").and_then(Value::as_array)?;
            items.iter().filter_map(|c| c.get("text").and_then(Value::as_str)).find_map(|s| serde_json::from_str::<Value>(s).ok())
        };
        let value = serde_json::from_str::<Value>(&text).ok().or_else(item_json).or_else(|| v.get("structuredContent").cloned());
        ToolResult { is_error, text, value }
    }
}

type Pending = Arc<Mutex<HashMap<u64, Sender<Msg>>>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A connected MCP server.
pub struct Client {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Pending,
    next: AtomicU64,
    /// Why the stream ended, once it has.
    dead: Arc<Mutex<Option<String>>>,
    child: Mutex<Option<Child>>,
    /// The child's watchdog ([`watchdog`]): kills the child's group if this process dies.
    watch: Mutex<Option<Child>>,
    stderr: Arc<Mutex<VecDeque<u8>>>,
    /// Lines the reader skipped (garbage, unknown ids): for the tests and diagnostics.
    skipped: Arc<AtomicU64>,
    /// `serverInfo` from `initialize`.
    pub server_name: String,
    pub server_version: String,
}

impl Client {
    /// Connect over a reader and writer (a child's stdout and stdin, or pipes in tests). Nothing
    /// is sent: call [`Client::initialize`].
    pub fn over(reader: Box<dyn Read + Send>, writer: Box<dyn Write + Send>) -> Client {
        Client::over_bounded(reader, writer, MAX_REPLY)
    }

    /// [`Client::over`] with another bound on reply lines (the tests use a small one).
    pub fn over_bounded(reader: Box<dyn Read + Send>, writer: Box<dyn Write + Send>, max_reply: usize) -> Client {
        let pending: Pending = Arc::default();
        let dead: Arc<Mutex<Option<String>>> = Arc::default();
        let skipped = Arc::new(AtomicU64::new(0));
        let client = Client {
            writer: Mutex::new(writer),
            pending: Arc::clone(&pending),
            next: AtomicU64::new(1),
            dead: Arc::clone(&dead),
            child: Mutex::new(None),
            watch: Mutex::new(None),
            stderr: Arc::default(),
            skipped: Arc::clone(&skipped),
            server_name: String::new(),
            server_version: String::new(),
        };
        let spawned = std::thread::Builder::new().name("plumcraft-mcp-read".into()).spawn(move || read_loop(reader, max_reply, &pending, &dead, &skipped));
        if let Err(e) = spawned {
            *lock(&client.dead) = Some(format!("no reader thread: {e}"));
        }
        client
    }

    /// Spawn `command` (program and arguments) with `env`, connected over its stdio, in its own
    /// process group (a terminal's ^C doesn't reach it; the engine owns its life). Its stderr goes
    /// to a [`STDERR_TAIL`] ring. When the engine dies the child's stdin closes, which ends an
    /// idle MCP server's read loop; a busy one (a trace) doesn't read it, so a [`watchdog`] in
    /// the child's group kills the group once this process is gone.
    pub fn spawn(command: &[String], env: &[(String, String)]) -> std::io::Result<Client> {
        Client::spawn_watched(command, env, std::process::id())
    }

    /// [`Client::spawn`] with the watchdog watching `parent` instead of this process (the tests
    /// stand in a process they can kill).
    #[doc(hidden)]
    pub fn spawn_watched(command: &[String], env: &[(String, String)], parent: u32) -> std::io::Result<Client> {
        use std::os::unix::process::CommandExt;
        let (program, args) = command.split_first().ok_or_else(|| std::io::Error::other("empty command"))?;
        let mut child = Command::new(program)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?;
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::other("the child has no stdio"));
        };
        let client = Client::over(Box::new(stdout), Box::new(stdin));
        let ring = Arc::clone(&client.stderr);
        let _ = std::thread::Builder::new().name("plumcraft-mcp-stderr".into()).spawn(move || drain_stderr(stderr, &ring));
        match watchdog(parent, child.id()) {
            Ok(w) => *lock(&client.watch) = Some(w),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::other(format!("no watchdog for the craft process: {e}")));
            }
        }
        *lock(&client.child) = Some(child);
        Ok(client)
    }

    /// Whether the child has exited (and been reaped). False when there is no child.
    #[doc(hidden)]
    pub fn has_exited(&self) -> bool {
        lock(&self.child).as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(Some(_))))
    }

    /// `initialize` then `notifications/initialized`; records `serverInfo`.
    pub fn initialize(&mut self, timeout: Duration) -> Result<Value, CallError> {
        let params =
            json!({"protocolVersion": MCP_REVISION, "capabilities": {}, "clientInfo": {"name": "plumcraft-craft", "version": env!("CARGO_PKG_VERSION")}});
        let reply = self.call("initialize", params, timeout, &mut NoWatch)?;
        let info = reply.get("serverInfo");
        let text = |k: &str| info.and_then(|i| i.get(k)).and_then(Value::as_str).unwrap_or_default().chars().take(200).collect::<String>();
        self.server_name = text("name");
        self.server_version = text("version");
        self.notify("notifications/initialized", json!({}))?;
        Ok(reply)
    }

    /// The child's process id, when there is a child.
    pub fn pid(&self) -> Option<u32> {
        lock(&self.child).as_ref().map(Child::id)
    }

    /// Lines the reader skipped so far.
    pub fn skipped(&self) -> u64 {
        self.skipped.load(Ordering::SeqCst)
    }

    /// Whether the stream has ended.
    pub fn is_dead(&self) -> bool {
        lock(&self.dead).is_some()
    }

    /// The last lines of the server's stderr.
    pub fn stderr_tail(&self) -> String {
        let bytes: Vec<u8> = lock(&self.stderr).iter().copied().collect();
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        lines[lines.len().saturating_sub(8)..].join("\n")
    }

    /// Kill the server (SIGKILL) and reap it. Every pending call fails with `Dead`.
    pub fn kill(&self) {
        if let Some(mut c) = lock(&self.child).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        stop_watch(&self.watch);
        mark_dead(&self.pending, &self.dead, "killed by the adapter");
    }

    /// Send a notification.
    pub fn notify(&self, method: &str, params: Value) -> Result<(), CallError> {
        let mut w = lock(&self.writer);
        rpc::write_frame(&mut *w, &Request::notify(method, params)).map_err(|e| CallError::Io(e.to_string()))
    }

    /// A request and its reply. `_meta.progressToken` is set, so a server that reports progress
    /// does; `watch` sees it and is polled for cancel.
    pub fn call(&self, method: &str, mut params: Value, timeout: Duration, watch: &mut dyn Watch) -> Result<Value, CallError> {
        if let Some(reason) = lock(&self.dead).clone() {
            return Err(CallError::Dead { reason, stderr: self.stderr_tail() });
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        if let Some(obj) = params.as_object_mut() {
            obj.insert("_meta".into(), json!({"progressToken": id}));
        }
        let (tx, rx) = channel();
        lock(&self.pending).insert(id, tx);
        let sent = {
            let mut w = lock(&self.writer);
            rpc::write_frame(&mut *w, &Request::call(id, method, params))
        };
        if let Err(e) = sent {
            lock(&self.pending).remove(&id);
            // A closed pipe means the server is gone; say so with its stderr.
            return Err(match lock(&self.dead).clone() {
                Some(reason) => CallError::Dead { reason, stderr: self.stderr_tail() },
                None => CallError::Io(format!("writing {method}: {e}")),
            });
        }
        let out = self.wait(id, method, &rx, timeout, watch);
        lock(&self.pending).remove(&id);
        out
    }

    fn wait(&self, id: u64, method: &str, rx: &Receiver<Msg>, timeout: Duration, watch: &mut dyn Watch) -> Result<Value, CallError> {
        let started = Instant::now();
        loop {
            if watch.cancelled() {
                return self.cancel(id, rx);
            }
            let left = timeout.saturating_sub(started.elapsed());
            if left.is_zero() {
                self.kill();
                return Err(CallError::Timeout { method: method.to_owned(), after: timeout });
            }
            match rx.recv_timeout(left.min(POLL)) {
                Ok(Msg::Reply(r)) => return r.map_err(CallError::Rpc),
                Ok(Msg::Progress(p)) => watch.progress(&p),
                Ok(Msg::Dead(reason)) => return Err(CallError::Dead { reason, stderr: self.stderr_tail() }),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(CallError::Dead { reason: "the reader stopped".into(), stderr: self.stderr_tail() });
                }
            }
        }
    }

    /// D41: tell the server, give it the grace to answer, then kill it.
    fn cancel(&self, id: u64, rx: &Receiver<Msg>) -> Result<Value, CallError> {
        let _ = self.notify("notifications/cancelled", json!({"requestId": id, "reason": "cancelled by the user"}));
        let deadline = Instant::now() + CANCEL_GRACE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                // It answered after all: the call finished; the run still ends cancelled, and
                // the process is clean, so it isn't killed.
                Ok(Msg::Reply(_)) => return Err(CallError::Cancelled),
                Ok(Msg::Progress(_)) if !left.is_zero() => {}
                _ => break,
            }
        }
        self.kill();
        Err(CallError::Cancelled)
    }

    /// `tools/call`.
    pub fn tool(&self, name: &str, arguments: Value, timeout: Duration, watch: &mut dyn Watch) -> Result<ToolResult, CallError> {
        let v = self.call("tools/call", json!({"name": name, "arguments": arguments}), timeout, watch)?;
        Ok(ToolResult::from_result(&v))
    }

    /// `ping`, answered within `timeout`.
    pub fn ping(&self, timeout: Duration) -> bool {
        self.call("ping", json!({}), timeout, &mut NoWatch).is_ok()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(mut c) = lock(&self.child).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        stop_watch(&self.watch);
    }
}

/// A watchdog for a craft process `child` (the leader of its own group): a `sh` loop in that
/// group that checks every second whether `parent` and `child` are alive, and SIGKILLs the
/// group once `parent` is gone. An engine that dies without running destructors (SIGKILL, a
/// supervisor killing the engine's group, a test harness exiting) would otherwise leave a busy
/// craft running with no parent: a cancelled or abandoned trace doesn't read its stdin. The
/// watchdog sits in the craft's group, not the engine's, so killing the engine's group doesn't
/// take it too; it exits by itself once the child is gone. Pure `std`: no `unsafe`, no libc.
fn watchdog(parent: u32, child: u32) -> std::io::Result<Child> {
    use std::os::unix::process::CommandExt;
    let script = r#"while kill -0 "$1" 2>/dev/null && kill -0 "$2" 2>/dev/null; do sleep 1; done; kill -0 "$1" 2>/dev/null || kill -9 -"$2" 2>/dev/null"#;
    Command::new("/bin/sh")
        .args(["-c", script, "plumcraft-craft-watch", &parent.to_string(), &child.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(i32::try_from(child).map_err(std::io::Error::other)?)
        .spawn()
}

fn stop_watch(watch: &Mutex<Option<Child>>) {
    if let Some(mut w) = lock(watch).take() {
        let _ = w.kill();
        let _ = w.wait();
    }
}

fn mark_dead(pending: &Pending, dead: &Mutex<Option<String>>, reason: &str) {
    lock(dead).get_or_insert_with(|| reason.to_owned());
    for (_, tx) in lock(pending).drain() {
        let _ = tx.send(Msg::Dead(reason.to_owned()));
    }
}

fn read_loop(reader: Box<dyn Read + Send>, max: usize, pending: &Pending, dead: &Mutex<Option<String>>, skipped: &AtomicU64) {
    let mut r = BufReader::new(reader);
    loop {
        let line = match rpc::read_frame(&mut r, max) {
            Ok(Some(l)) => l,
            Ok(None) => return mark_dead(pending, dead, "the server exited"),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                return mark_dead(pending, dead, &format!("the server sent a line over {max} bytes"));
            }
            Err(e) => return mark_dead(pending, dead, &format!("reading the server: {e}")),
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match rpc::parse(&line) {
            Ok(Frame::Response(resp)) => {
                let tx = resp.id.and_then(|id| lock(pending).get(&id).cloned());
                match tx {
                    Some(tx) => {
                        let _ = tx.send(Msg::Reply(resp.into_result()));
                    }
                    None => {
                        skipped.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
            Ok(Frame::Request(req)) if req.method == "notifications/progress" && req.id.is_none() => {
                let token = req.params.get("progressToken").and_then(|t| t.as_u64().or_else(|| t.as_str().and_then(|s| s.parse().ok())));
                let p = Progress {
                    progress: req.params.get("progress").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0),
                    total: req.params.get("total").and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0),
                    message: req.params.get("message").and_then(Value::as_str).map(|m| m.chars().take(200).collect()),
                };
                match token.and_then(|t| lock(pending).get(&t).cloned()) {
                    Some(tx) => {
                        let _ = tx.send(Msg::Progress(p));
                    }
                    None => {
                        skipped.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
            // Other notifications (logging, list changes) and requests from the server: not used.
            Ok(Frame::Request(_)) | Err(_) => {
                skipped.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}

fn drain_stderr(mut r: impl Read, ring: &Mutex<VecDeque<u8>>) {
    let mut buf = [0u8; 4096];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                let mut q = lock(ring);
                q.extend(buf.get(..n).unwrap_or_default());
                let over = q.len().saturating_sub(STDERR_TAIL);
                q.drain(..over);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_beside_the_json_leaves_the_json_readable() {
        let r = ToolResult::from_result(&json!({"content": [{"type": "text", "text": "[1, 2]"}, {"type": "text", "text": "Note: headless."}]}));
        assert_eq!(r.value, Some(json!([1, 2])));
        assert!(r.text.ends_with("Note: headless."));
    }
}
