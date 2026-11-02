//! Mode 3 (spec 16.4, 13): an engine reached over HTTP, possibly on another machine.
//!
//! - **Calls** go to `POST /v1/rpc`; `events.poll` long-polls there too.
//! - **Sources** (spec 13.2): before a run is sent, every path value on a port the catalogue
//!   marks as a path is hashed (cached by path, size and mtime), asked about with
//!   `POST /v1/blobs/has`, uploaded only if missing, and rewritten to
//!   `{"source": {handle, name}}`. The engine never sees a client path. Skipped when the
//!   same-host proof says the engine reads the UI's files.
//! - **Results** (spec 13.2 steps 5 and 6): `url` locators are fetched lazily, by handle, only
//!   when something reads them; every byte is verified against the handle and stored in the
//!   UI's **durable CAS** (the UI's only write target), so a second read is local.
//! - `remote: true` skips the same-host proof, as a UI on another machine would fail it.
//!
//! Not yet: supervision and restart of a spawned HTTP engine (it is killed on drop), SSE,
//! leases and pins, resumable uploads.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use plumcraft_engine::serve::ENV_TOKEN;
use plumcraft_proto::PROTOCOL;
use plumcraft_proto::data::{DataRef, Form, Locator};
use plumcraft_proto::error::{ErrorCode, ProtoError};
use plumcraft_proto::handle::Handle;
use plumcraft_proto::session::{EngineState, Proof};
use plumcraft_store::{Blob, Store};
use serde_json::{Value, json};
use ureq::Agent;

use crate::Connection;
use crate::process::{DataSession, Manifest, kill_group};

/// The longest call: a long-poll (30 s) plus slack.
const CALL_TIMEOUT: Duration = Duration::from_secs(45);
/// `output.save` runs the graph for its node (spec 6.1: 300 s; the engine's own limit answers
/// first).
const SAVE_TIMEOUT: Duration = Duration::from_secs(310);
/// The largest reply or download read into memory.
const MAX_BODY: u64 = 512 << 20;

/// How the UI talks to this engine.
#[derive(Debug, Clone)]
pub struct HttpOptions {
    /// Behave as a UI on another machine: no same-host proof, so values come by URL and
    /// sources by upload.
    pub remote: bool,
    /// The UI's durable CAS (default `~/Library/Caches/plumcraft/cas`).
    pub cas: PathBuf,
}

impl Default for HttpOptions {
    fn default() -> Self {
        HttpOptions { remote: false, cas: default_cas() }
    }
}

/// The UI's durable CAS root.
pub fn default_cas() -> PathBuf {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    if cfg!(target_os = "macos") { home.join("Library/Caches/plumcraft/cas") } else { home.join(".cache/plumcraft/cas") }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// An engine over HTTP.
pub struct HttpHost {
    base: String,
    token: String,
    agent: Agent,
    /// The same, for what may take minutes.
    slow_agent: Agent,
    opts: HttpOptions,
    cas: Store,
    /// The UI's commonkit store: its nonce file, and same-host locators.
    local: Store,
    state: Mutex<EngineState>,
    data: Mutex<DataSession>,
    /// Node kind → its ports that take a path, with their defaults (from the catalogue).
    path_ports: Mutex<PathPorts>,
    /// Files the user let go to an engine this host didn't start.
    allowed: Mutex<BTreeSet<String>>,
    hashes: Mutex<HashMap<(PathBuf, u64, SystemTime), Handle>>,
    uploads: AtomicU64,
    next_id: AtomicU64,
    child: Mutex<Option<Child>>,
    /// The spawned engine's stdin: when this process ends, the pipe closes and the engine,
    /// started with `--lifeline`, exits.
    lifeline: Mutex<Option<std::process::ChildStdin>>,
    dir: Option<PathBuf>,
}

impl HttpHost {
    /// Attach to a running engine at `base` (`http://host:port`).
    pub fn attach(base: &str, token: &str, opts: HttpOptions) -> Result<HttpHost, String> {
        let h = HttpHost::new(base, token, opts, None, None);
        h.connect()?;
        Ok(h)
    }

    /// Start a local HTTP engine from a manifest (its command gets `--listen 127.0.0.1:0
    /// --port-file …` and the token) and attach to it. The process is killed on drop.
    pub fn spawn(m: &Manifest, opts: HttpOptions) -> Result<HttpHost, String> {
        plumcraft_scratch::sweep_once();
        let dir = crate::process::private_dir("plumcraft-http").map_err(|e| e.to_string())?;
        let port_file = dir.join("port");
        let token = crate::process::token().map_err(|e| e.to_string())?;
        let (program, args) = m.command.split_first().ok_or("the manifest has no command")?;
        let mut cmd = Command::new(program);
        cmd.args(args)
            .args(["--lifeline", "--listen", "127.0.0.1:0", "--port-file"])
            .arg(&port_file)
            .envs(m.env.iter().cloned())
            .env(ENV_TOKEN, &token)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        if let Some(dir) = &m.cwd {
            cmd.current_dir(dir);
        }
        if let Some(root) = &m.store {
            cmd.env(plumcraft_store::ROOT_ENV, root);
        }
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn().map_err(|e| format!("could not start the engine: {e}"))?;
        let lifeline = child.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(20);
        let addr = loop {
            if let Ok(a) = std::fs::read_to_string(&port_file) {
                break a;
            }
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!("the engine exited before listening ({status})"));
            }
            if Instant::now() > deadline {
                kill_group(&mut child);
                return Err("the engine didn't listen within 20 s".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let h = HttpHost::new(&format!("http://{}", addr.trim()), &token, opts, Some(child), Some(dir));
        *lock(&h.lifeline) = lifeline;
        h.connect()?;
        Ok(h)
    }

    fn new(base: &str, token: &str, opts: HttpOptions, child: Option<Child>, dir: Option<PathBuf>) -> HttpHost {
        let agent: Agent = Agent::config_builder().timeout_global(Some(CALL_TIMEOUT)).http_status_as_error(false).build().into();
        let slow_agent: Agent = Agent::config_builder().timeout_global(Some(SAVE_TIMEOUT)).http_status_as_error(false).build().into();
        HttpHost {
            base: base.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
            agent,
            slow_agent,
            cas: Store::new(&opts.cas),
            local: Store::new(plumcraft_store::default_root()),
            opts,
            state: Mutex::new(EngineState::Connecting { url: base.to_owned() }),
            data: Mutex::new(DataSession::default()),
            path_ports: Mutex::new(BTreeMap::new()),
            allowed: Mutex::new(BTreeSet::new()),
            hashes: Mutex::new(HashMap::new()),
            uploads: AtomicU64::new(0),
            next_id: AtomicU64::new(1),
            child: Mutex::new(child),
            lifeline: Mutex::new(None),
            dir,
        }
    }

    /// `hello` (with the proof unless remote), `session.confirm`, and the catalogue.
    fn connect(&self) -> Result<(), String> {
        let ours = if self.opts.remote { None } else { plumcraft_store::write_nonce(self.local.root(), "u").ok() };
        let mut hello = json!({"v": PROTOCOL, "op": "hello", "caps": ["events.poll", "data.shm", "data.path"]});
        if let Some((path, nonce)) = &ours {
            hello["proof"] = json!({"path": path, "nonce": nonce});
        }
        let reply = self.rpc("hello", hello);
        if let Some((path, _)) = &ours {
            let _ = std::fs::remove_file(path);
        }
        let reply = reply.map_err(|e| {
            let why = format!("hello failed: {}", e.message);
            *lock(&self.state) = EngineState::EngineDown { reason: why.clone(), queues: None, stderr_tail: None, hint: String::new() };
            why
        })?;
        let proof = reply.get("proof").cloned().and_then(|p| serde_json::from_value::<Proof>(p).ok()).unwrap_or_default();
        let shm = !self.opts.remote && plumcraft_store::check_nonce(&proof.path, &proof.nonce);
        let path = !self.opts.remote && proof.client_proof_read;
        let mut stores: Vec<String> = reply.get("data").and_then(|d| d.get("stores")).cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        // The work root's files are the engine's own, readable after the proof (spec 4.4).
        stores.extend(reply.get("data").and_then(|d| d.get("work_root")).and_then(Value::as_str).filter(|w| !w.is_empty()).map(str::to_owned));
        *lock(&self.data) = DataSession { shm, path, stores };
        self.rpc("session.confirm", json!({"shm": shm, "path": path})).map_err(|e| e.message)?;
        let schema = self.rpc("schema", json!({})).map_err(|e| e.message)?;
        let mut ports = BTreeMap::new();
        for n in schema["nodes"].as_array().into_iter().flatten() {
            let paths: Vec<(String, Value)> = n["inputs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["path"].is_object() && !matches!(p["path"]["mode"].as_str(), Some("save" | "dir")))
                .filter_map(|p| Some((p["name"].as_str()?.to_owned(), p["default"].clone())))
                .collect();
            if let (Some(id), false) = (n["id"].as_str(), paths.is_empty()) {
                ports.insert(id.to_owned(), paths);
            }
        }
        *lock(&self.path_ports) = ports;
        let version = reply["engine"]["version"].as_str().unwrap_or_default().to_owned();
        let caps = reply.get("caps").cloned().and_then(|c| serde_json::from_value(c).ok()).unwrap_or_default();
        let nodes = schema["nodes"].as_array().map_or(0, |n| n.len() as u64);
        *lock(&self.state) = EngineState::Connected { version, nodes, caps };
        Ok(())
    }

    /// Sources uploaded so far (tests: a rerun must upload nothing).
    pub fn uploads(&self) -> u64 {
        self.uploads.load(Ordering::SeqCst)
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn data_session(&self) -> DataSession {
        lock(&self.data).clone()
    }

    fn request(&self, method: &str, path: &str, body: Option<&[u8]>, headers: &[(&str, &str)]) -> Result<(u16, Vec<u8>), String> {
        self.request_on(&self.agent, method, path, body, headers)
    }

    fn request_on(&self, agent: &Agent, method: &str, path: &str, body: Option<&[u8]>, headers: &[(&str, &str)]) -> Result<(u16, Vec<u8>), String> {
        let url = format!("{}{path}", self.base);
        let auth = format!("Bearer {}", self.token);
        let result = if method == "GET" {
            let mut r = agent.get(&url).header("Authorization", &auth);
            for (k, v) in headers {
                r = r.header(*k, *v);
            }
            r.call()
        } else {
            let mut r = if method == "PUT" { agent.put(&url) } else { agent.post(&url) }.header("Authorization", &auth);
            for (k, v) in headers {
                r = r.header(*k, *v);
            }
            r.send(body.unwrap_or_default())
        };
        let mut resp = result.map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let bytes = resp.body_mut().with_config().limit(MAX_BODY).read_to_vec().map_err(|e| e.to_string())?;
        Ok((status, bytes))
    }

    fn rpc(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let body = serde_json::to_vec(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).unwrap_or_default();
        let agent = if method == "output.save" { &self.slow_agent } else { &self.agent };
        let (status, bytes) = self.request_on(agent, "POST", "/v1/rpc", Some(&body), &[("Content-Type", "application/json")]).map_err(|e| {
            let why = format!("the engine at {} is unreachable: {e}", self.base);
            *lock(&self.state) = EngineState::EngineDown { reason: why.clone(), queues: None, stderr_tail: None, hint: String::new() };
            ProtoError::new(ErrorCode::Unavailable, why)
        })?;
        if status == 401 {
            return Err(ProtoError::new(ErrorCode::Unauthorized, "the engine refused the token"));
        }
        let v: Value = serde_json::from_slice(&bytes).map_err(|e| ProtoError::new(ErrorCode::Parse, e.to_string()))?;
        match (v.get("result"), v.get("error")) {
            (_, Some(e)) if !e.is_null() => Err(serde_json::from_value(e.clone()).unwrap_or_else(|_| ProtoError::new(ErrorCode::Internal, e.to_string()))),
            (Some(r), _) => Ok(r.clone()),
            _ => Ok(Value::Null),
        }
    }

    /// Spec 13.2 steps 1–3: upload what the engine lacks and rewrite path values to sources.
    fn gather_sources(&self, params: &mut Value) -> Result<(), ProtoError> {
        if lock(&self.data).path {
            return Ok(()); // the engine reads our files directly
        }
        let ports = lock(&self.path_ports).clone();
        let Some(nodes) = params.pointer_mut("/graph/graph/nodes").and_then(Value::as_object_mut) else { return Ok(()) };
        let wanted = sources(nodes, &ports);
        if wanted.is_empty() {
            return Ok(());
        }
        let hashed: Vec<(String, String, PathBuf, Option<Handle>)> = wanted
            .into_iter()
            .map(|(id, port, path)| {
                let h = self.hash_file(&path);
                (id, port, path, h)
            })
            .collect();
        let handles: Vec<String> = hashed.iter().filter_map(|(.., h)| h.as_ref().map(ToString::to_string)).collect();
        let body = serde_json::to_vec(&json!({"handles": handles})).unwrap_or_default();
        let (_, bytes) = self
            .request("POST", "/v1/blobs/has", Some(&body), &[("Content-Type", "application/json")])
            .map_err(|e| ProtoError::new(ErrorCode::Unavailable, e))?;
        let missing: Vec<String> =
            serde_json::from_slice::<Value>(&bytes).ok().and_then(|v| serde_json::from_value(v["missing"].clone()).ok()).unwrap_or_default();
        for (id, port, path, handle) in hashed {
            // An unreadable file stays a path: the engine reports it, as it would locally.
            let Some(handle) = handle else { continue };
            if missing.contains(&handle.to_string()) {
                let bytes = std::fs::read(&path).map_err(|e| ProtoError::new(ErrorCode::BadParams, format!("{}: {e}", path.display())))?;
                let (status, reply) = self
                    .request("PUT", &plumcraft_engine::data::blob_url(&handle), Some(&bytes), &[("Content-Type", "application/octet-stream")])
                    .map_err(|e| ProtoError::new(ErrorCode::Unavailable, e))?;
                if !(200..300).contains(&status) {
                    return Err(ProtoError::new(
                        ErrorCode::Integrity,
                        format!("upload of {} refused ({status}): {}", path.display(), String::from_utf8_lossy(&reply)),
                    ));
                }
                self.uploads.fetch_add(1, Ordering::SeqCst);
            }
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if let Some(n) = nodes.get_mut(&id) {
                n["values"][&port] = json!({"source": {"handle": handle.to_string(), "name": name}});
            }
        }
        Ok(())
    }

    /// A file's `md5:raw:` handle, cached by path, size and mtime.
    fn hash_file(&self, path: &Path) -> Option<Handle> {
        let meta = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
        let key = (path.to_path_buf(), meta.len(), meta.modified().ok()?);
        if let Some(h) = lock(&self.hashes).get(&key) {
            return Some(h.clone());
        }
        let h = Handle::md5_raw(&std::fs::read(path).ok()?);
        lock(&self.hashes).insert(key, h.clone());
        Some(h)
    }

    /// Spec 13.2 steps 5 and 6: fetch by URL, verify, keep in the durable CAS.
    fn fetch(&self, r: &DataRef, url: &str) -> Option<Blob> {
        let handle = r.handle.as_ref()?;
        let path = if url.starts_with('/') { url.to_owned() } else { return None };
        let (status, bytes) = self.request("GET", &path, None, &[]).ok()?;
        if status != 200 {
            return None;
        }
        let (dtype, shape) = match &r.meta.form {
            Form::Array { dtype, shape, .. } => (dtype.clone(), shape.clone()),
            // Audio is a (frames, channels) array too (plan 10.4); taken as raw bytes its nd1
            // handle never verified, so remote audio never resolved.
            Form::Audio { dtype, channels, frames, .. } => (dtype.clone(), vec![*frames, u64::from(*channels)]),
            _ => ("|u1".to_owned(), vec![bytes.len() as u64]),
        };
        // A mismatch is discarded, never stored (spec 13.2 step 6). Verified bytes are the
        // value even when the cache declines them (its budget or free-space floor).
        if !plumcraft_store::verifies(handle, &dtype, &shape, &bytes) {
            return None;
        }
        let _ = self.cas.put_verified(handle, &dtype, &shape, &bytes);
        Some(Blob { dtype, shape, bytes: bytes.into() })
    }
}

/// Node kind → its ports that take a path, with their defaults.
type PathPorts = BTreeMap<String, Vec<(String, Value)>>;

/// The local files these nodes name: `(node, port, file)`. A value that is only the catalogue's
/// default is left out: a person chose nothing, and the engine wrote the catalogue, so a default
/// of `/etc/passwd` must not be how an engine asks for that file.
fn sources(nodes: &serde_json::Map<String, Value>, ports: &PathPorts) -> Vec<(String, String, PathBuf)> {
    let mut wanted = Vec::new();
    for (id, n) in nodes {
        let Some(paths) = n["kind"].as_str().and_then(|k| ports.get(k)) else { continue };
        for (port, default) in paths {
            let value = &n["values"][port];
            if value == default {
                continue;
            }
            if let Some(p) = value.as_str().filter(|p| !p.is_empty()) {
                wanted.push((id.clone(), port.clone(), PathBuf::from(p)));
            }
        }
    }
    wanted
}

impl Connection for HttpHost {
    fn uploads_pending(&self, params: &Value) -> Vec<(String, String)> {
        // An engine this host started is the user's own; one that reads our files needs nothing.
        if lock(&self.child).is_some() || lock(&self.data).path {
            return Vec::new();
        }
        let Some(nodes) = params.pointer("/graph/graph/nodes").and_then(Value::as_object) else { return Vec::new() };
        let allowed = lock(&self.allowed);
        let files: BTreeSet<String> = sources(nodes, &lock(&self.path_ports))
            .into_iter()
            .filter(|(.., p)| p.is_file())
            .map(|(.., p)| p.display().to_string())
            .filter(|p| !allowed.contains(p))
            .collect();
        files.into_iter().map(|f| (self.base.clone(), f)).collect()
    }

    fn allow_uploads(&self, files: &[String]) {
        lock(&self.allowed).extend(files.iter().cloned());
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        let mut params = params;
        if method == "run" || method == "output.save" {
            self.gather_sources(&mut params)?;
        }
        // The file is ours to write, not a path on the engine's machine (spec 13.2).
        if method == "output.save"
            && !lock(&self.data).path
            && let Some(o) = params.as_object_mut()
        {
            o.remove("path");
        }
        let out = self.rpc(method, params);
        if out.is_ok() && matches!(*lock(&self.state), EngineState::EngineDown { .. }) {
            // Reachable again: say hello afresh (the engine may have restarted).
            let _ = self.connect();
        }
        out
    }

    fn state(&self) -> EngineState {
        lock(&self.state).clone()
    }

    fn resolve(&self, r: &DataRef) -> Option<Blob> {
        let d = lock(&self.data).clone();
        if let Some(b) = self.local.resolve_in(r, d.shm, &d.stores) {
            return Some(b); // inline bytes, or the engine's files on this machine
        }
        if let Some(b) = r.handle.as_ref().and_then(|h| self.cas.read_handle(h)) {
            return Some(b);
        }
        r.at.iter().find_map(|l| match l {
            Locator::Url { url, .. } => self.fetch(r, url),
            _ => None,
        })
    }
}

impl Drop for HttpHost {
    fn drop(&mut self) {
        if let Some(c) = lock(&self.child).as_mut() {
            kill_group(c);
            let _ = c.wait();
        }
        if let Some(d) = &self.dir {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;

    #[test]
    fn a_catalogue_default_is_never_a_source() {
        let ports: PathPorts = BTreeMap::from([("x.grab".to_owned(), vec![("file".to_owned(), json!("/etc/passwd"))])]);
        let nodes = json!({
            "bait": {"kind": "x.grab", "values": {"file": "/etc/passwd"}},
            "mine": {"kind": "x.grab", "values": {"file": "/home/me/photo.jpg"}},
            "none": {"kind": "x.grab", "values": {"file": ""}},
            "other": {"kind": "x.else", "values": {"file": "/home/me/notes.txt"}}});
        let wanted = sources(nodes.as_object().unwrap(), &ports);
        assert_eq!(wanted, [("mine".to_owned(), "file".to_owned(), PathBuf::from("/home/me/photo.jpg"))]);
    }
}
