//! The `--control` transport: a loopback TCP server speaking newline-delimited JSON, with token
//! authentication. Adapted from photocraft's `apps/photocraft/src/control_server.rs` and
//! `crates/automation/src/security.rs` (MIT OR Apache-2.0, used under MIT), by way of the research software's
//! `crates/automation/src/server.rs` (2026-10-27, the research software `8d96021`; plan 10.3, E1.4).
//!
//! Each connection's first frame must be `{"method": "auth", "params": {"token": "<64 hex>"}}`.
//! After that, each request line is queued to the UI thread as a [`ControlRequest`]; the reply
//! (with the request's `id`) is written when the UI answers, or an error after
//! [`REPLY_DEADLINE`].

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// The first request on every connection must use this method.
pub const AUTH_METHOD: &str = "auth";
/// The longest request line, including its newline.
pub const MAX_REQUEST_BYTES: usize = 1 << 20;
/// Connections served at once.
pub const MAX_CONNECTIONS: usize = 16;
/// Idle and write timeout of a connection.
pub const IO_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a new connection has to authenticate.
pub const AUTH_DEADLINE: Duration = Duration::from_secs(10);
/// How long the server waits for the UI to answer a request.
pub const REPLY_DEADLINE: Duration = Duration::from_secs(30);

const TOKEN_HEX_LEN: usize = 64;

/// One request, handed to the UI thread.
pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    /// Send `{"ok": true, "result"}` or `{"ok": false, "error"}`; the server adds the id.
    pub reply: Sender<Value>,
    /// A request still queued at this instant is answered with a timeout instead of run.
    pub deadline: Instant,
}

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Value) -> (Self, Receiver<Value>) {
        let (tx, rx) = channel();
        (ControlRequest { method: method.into(), params, reply: tx, deadline: Instant::now() + REPLY_DEADLINE }, rx)
    }

    /// Has the transport stopped waiting for this one?
    pub fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }
}

/// Errors setting up the server.
#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("{0}")]
    Token(String),
    #[error("{0}")]
    Io(String),
}

/// A fresh 256-bit token as 64 hex digits (two v4 UUIDs: 244 random bits from the OS RNG).
pub fn generate_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// Accept only 64 hexadecimal characters.
pub fn validate_token(token: &str) -> Result<(), ControlError> {
    if token.len() != TOKEN_HEX_LEN || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ControlError::Token("a control token must be exactly 64 hexadecimal characters".into()));
    }
    Ok(())
}

/// Compare tokens without an early exit on the first differing byte.
pub fn token_matches(expected: &str, supplied: &str) -> bool {
    if expected.len() != TOKEN_HEX_LEN || supplied.len() != TOKEN_HEX_LEN {
        return false;
    }
    expected.bytes().zip(supplied.bytes()).fold(0u8, |d, (a, b)| d | (a.to_ascii_lowercase() ^ b.to_ascii_lowercase())) == 0
}

/// A token file is a password: refuse one that isn't a plain file of this user's, or that anyone
/// else can reach. In a shared folder someone else may have put it there, with a token they know.
#[cfg(unix)]
fn check_private(path: &Path) -> Result<(), ControlError> {
    use std::os::unix::fs::MetadataExt;
    let say = |why: String| ControlError::Token(format!("{}: {why}", path.display()));
    let meta = std::fs::symlink_metadata(path).map_err(|e| ControlError::Io(format!("{}: {e}", path.display())))?;
    if !meta.file_type().is_file() {
        return Err(say("a token file must be a plain file, not a link".into()));
    }
    if meta.uid() != rustix::process::getuid().as_raw() {
        return Err(say("this token file belongs to someone else".into()));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(say("others can reach this token file: chmod 600 it".into()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_private(_: &Path) -> Result<(), ControlError> {
    Ok(())
}

fn read_token_file(path: &Path) -> Result<String, ControlError> {
    check_private(path)?;
    let text = std::fs::read_to_string(path).map_err(|e| ControlError::Io(format!("{}: {e}", path.display())))?;
    let token = text.trim().to_ascii_lowercase();
    validate_token(&token)?;
    Ok(token)
}

fn create_token_file(path: &Path, token: &str) -> Result<(), ControlError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| ControlError::Io(format!("{}: {e}", parent.display())))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| ControlError::Io(format!("{}: {e}", path.display())))?;
    writeln!(file, "{token}").map_err(|e| ControlError::Io(format!("{}: {e}", path.display())))
}

/// The server's token: read from `token_file` (created, mode 0600, when missing), else fresh.
pub fn server_token(token_file: Option<&Path>) -> Result<String, ControlError> {
    let Some(path) = token_file else { return Ok(generate_token()) };
    if path.exists() {
        return read_token_file(path);
    }
    let token = generate_token();
    match create_token_file(path, &token) {
        Ok(()) => Ok(token),
        // Another process won the create-new race.
        Err(_) if path.exists() => read_token_file(path),
        Err(e) => Err(e),
    }
}

/// The reply to the first frame, and whether it authenticated.
pub fn authentication_reply(line: &str, expected: &str) -> (Value, bool) {
    let Ok(req) = serde_json::from_str::<Value>(line) else {
        return (json!({"id": null, "ok": false, "error": "authentication required"}), false);
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let supplied = req.get("params").and_then(|p| p.get("token")).and_then(Value::as_str).unwrap_or("");
    if req.get("method").and_then(Value::as_str) == Some(AUTH_METHOD) && token_matches(expected, supplied) {
        (json!({"id": id, "ok": true, "result": {"authenticated": true}}), true)
    } else {
        (json!({"id": id, "ok": false, "error": "authentication required"}), false)
    }
}

/// Read one line of at most [`MAX_REQUEST_BYTES`]. `Ok(None)` at end of stream; `Err` for a line
/// too long or not UTF-8.
fn read_line(reader: &mut impl BufRead) -> std::io::Result<Option<Result<String, String>>> {
    let mut bytes = Vec::new();
    let n = reader.by_ref().take(MAX_REQUEST_BYTES as u64 + 1).read_until(b'\n', &mut bytes)?;
    if n == 0 {
        return Ok(None);
    }
    if n > MAX_REQUEST_BYTES {
        return Ok(Some(Err(format!("request exceeds {MAX_REQUEST_BYTES} bytes"))));
    }
    Ok(Some(String::from_utf8(bytes).map_err(|_| "request is not UTF-8".to_owned())))
}

/// Start the server on `127.0.0.1:port`. Requests arrive on the returned receiver; `wake` is
/// called after each is queued (egui's `request_repaint`). Returns the bound port too (useful
/// with port 0).
pub fn start(port: u16, token: String, wake: Arc<dyn Fn() + Send + Sync>) -> Result<(Receiver<ControlRequest>, u16), ControlError> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| ControlError::Io(format!("cannot listen on 127.0.0.1:{port}: {e}")))?;
    let bound = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let (tx, rx) = channel::<ControlRequest>();
    let active = Arc::new(AtomicUsize::new(0));
    let token = Arc::new(token);
    let spawned = std::thread::Builder::new().name("plumcraft-control".into()).spawn(move || {
        for mut stream in listener.incoming().flatten() {
            if active.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
                active.fetch_sub(1, Ordering::AcqRel);
                let _ = writeln!(stream, "{}", json!({"id": null, "ok": false, "error": "connection limit reached"}));
                continue;
            }
            let (tx, token, wake, active) = (tx.clone(), Arc::clone(&token), Arc::clone(&wake), Arc::clone(&active));
            let _ = std::thread::Builder::new().name("plumcraft-control-conn".into()).spawn(move || {
                serve(stream, &token, &tx, wake.as_ref(), AUTH_DEADLINE);
                active.fetch_sub(1, Ordering::AcqRel);
            });
        }
    });
    spawned.map_err(|e| ControlError::Io(format!("cannot start the control thread: {e}")))?;
    Ok((rx, bound))
}

/// How often a [`Server`]'s listener looks for a stop between connections.
const ACCEPT_POLL: Duration = Duration::from_millis(50);

/// A control server that stops when it is dropped: the listener closes and every connection is
/// shut, so its threads end. What a core that is opened and closed many times (the C ABI's)
/// starts; the windowed editor's lives as long as the process ([`start`]).
pub struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    /// Open connections, by number, so a drop can shut them.
    conns: Arc<Mutex<Vec<(u64, TcpStream)>>>,
    listener: Option<JoinHandle<()>>,
}

/// Is `ip` on this machine or its local network (loopback, a private IPv4 range, link-local, an
/// IPv6 unique-local address)? A control port bound beyond loopback serves only these peers.
pub fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80 || v6.to_ipv4_mapped().is_some_and(|v4| is_local(IpAddr::V4(v4)))
        }
    }
}

/// This machine's address on its local network: the one it would send from (no packet is
/// sent: a UDP socket is only connected), when that address is a local one ([`is_local`]) and
/// not loopback. None without a local network.
pub fn lan_address() -> Option<IpAddr> {
    let sock = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    sock.connect(("10.255.255.255", 9)).ok()?;
    let ip = sock.local_addr().ok()?.ip();
    (is_local(ip) && !ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

impl Server {
    /// Start serving on `127.0.0.1:port` (0 picks a free one); requests arrive on the returned
    /// receiver, and `wake` is called after each is queued.
    pub fn start(port: u16, token: String, wake: Arc<dyn Fn() + Send + Sync>) -> Result<(Server, Receiver<ControlRequest>), ControlError> {
        Server::start_on(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), port, token, wake)
    }

    /// Start serving on `ip:port`. Beyond loopback (N4 slice 4: a debug build on a device, so the
    /// conformance suite can drive it) a connection from a peer that isn't on the local network
    /// ([`is_local`]) is closed at once, before it is read; every connection must still
    /// authenticate first with the token. The caller decides whether a bind beyond loopback is
    /// allowed at all (the C ABI's core refuses one without an explicit debug flag and a
    /// pre-shared token).
    pub fn start_on(ip: IpAddr, port: u16, token: String, wake: Arc<dyn Fn() + Send + Sync>) -> Result<(Server, Receiver<ControlRequest>), ControlError> {
        let at = std::net::SocketAddr::new(ip, port);
        let listener = TcpListener::bind(at).map_err(|e| ControlError::Io(format!("cannot listen on {at}: {e}")))?;
        let bound = listener.local_addr().map(|a| a.port()).unwrap_or(port);
        listener.set_nonblocking(true).map_err(|e| ControlError::Io(format!("cannot listen on {ip}:{bound}: {e}")))?;
        let loopback = ip.is_loopback();
        let (tx, rx) = channel::<ControlRequest>();
        let stop = Arc::new(AtomicBool::new(false));
        let conns: Arc<Mutex<Vec<(u64, TcpStream)>>> = Arc::default();
        let (stop2, conns2, token) = (Arc::clone(&stop), Arc::clone(&conns), Arc::new(token));
        let thread = std::thread::Builder::new()
            .name("plumcraft-control".into())
            .spawn(move || {
                let (active, next) = (Arc::new(AtomicUsize::new(0)), AtomicU64::new(0));
                while !stop2.load(Ordering::Acquire) {
                    let mut stream = match listener.accept() {
                        // beyond loopback, only the local network's peers are served at all
                        Ok((s, peer)) if loopback || is_local(peer.ip()) => s,
                        Ok((s, _)) => {
                            let _ = s.shutdown(std::net::Shutdown::Both);
                            continue;
                        }
                        Err(_) => {
                            std::thread::sleep(ACCEPT_POLL);
                            continue;
                        }
                    };
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    if active.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
                        active.fetch_sub(1, Ordering::AcqRel);
                        let _ = writeln!(stream, "{}", json!({"id": null, "ok": false, "error": "connection limit reached"}));
                        continue;
                    }
                    let n = next.fetch_add(1, Ordering::Relaxed);
                    if let Ok(held) = stream.try_clone() {
                        lock(&conns2).push((n, held));
                    }
                    let (tx, token, wake, act, conns) = (tx.clone(), Arc::clone(&token), Arc::clone(&wake), Arc::clone(&active), Arc::clone(&conns2));
                    let spawned = std::thread::Builder::new().name("plumcraft-control-conn".into()).spawn(move || {
                        serve(stream, &token, &tx, wake.as_ref(), AUTH_DEADLINE);
                        lock(&conns).retain(|(i, _)| *i != n);
                        act.fetch_sub(1, Ordering::AcqRel);
                    });
                    if spawned.is_err() {
                        lock(&conns2).retain(|(i, _)| *i != n);
                        active.fetch_sub(1, Ordering::AcqRel);
                    }
                }
            })
            .map_err(|e| ControlError::Io(format!("cannot start the control thread: {e}")))?;
        Ok((Server { port: bound, stop, conns, listener: Some(thread) }, rx))
    }

    /// The port it listens on.
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Server {
    /// Stop listening (within [`ACCEPT_POLL`]) and shut every connection; their threads end
    /// on their own once their sockets are shut.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for (_, s) in lock(&self.conns).drain(..) {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
        if let Some(t) = self.listener.take() {
            let _ = t.join();
        }
    }
}

/// Lock, taking a poisoned lock as it is.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write_line(out: &mut TcpStream, v: &Value) -> bool {
    writeln!(out, "{v}").and_then(|()| out.flush()).is_ok()
}

/// Serve one connection until it closes.
pub fn serve(stream: TcpStream, token: &str, tx: &Sender<ControlRequest>, wake: &(dyn Fn() + Send + Sync), auth_deadline: Duration) {
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let Ok(read) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read);
    let mut out = stream;
    let started = Instant::now();
    let mut authenticated = false;
    loop {
        let timeout = if authenticated { IO_TIMEOUT } else { auth_deadline.saturating_sub(started.elapsed()) };
        if timeout.is_zero() || reader.get_ref().set_read_timeout(Some(timeout)).is_err() {
            break;
        }
        let line = match read_line(&mut reader) {
            Ok(None) | Err(_) => break,
            Ok(Some(Err(e))) => {
                let _ = write_line(&mut out, &json!({"id": null, "ok": false, "error": e}));
                break;
            }
            Ok(Some(Ok(l))) => l,
        };
        if line.trim().is_empty() {
            continue;
        }
        if !authenticated {
            let (reply, ok) = authentication_reply(&line, token);
            authenticated = ok;
            if !write_line(&mut out, &reply) || !ok {
                break;
            }
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => {
                let id = msg.get("id").cloned().unwrap_or(Value::Null);
                let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_owned();
                let mut r = if method == AUTH_METHOD {
                    json!({"ok": false, "error": "already authenticated"})
                } else {
                    let params = msg.get("params").cloned().unwrap_or(json!({}));
                    let (req, rx) = ControlRequest::new(method, params);
                    let deadline = req.deadline;
                    if tx.send(req).is_err() {
                        break;
                    }
                    wake();
                    rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap_or_else(
                        |_| json!({"ok": false, "error": format!("timeout: no reply within {} s (the request may still have run)", REPLY_DEADLINE.as_secs())}),
                    )
                };
                if let Some(o) = r.as_object_mut() {
                    o.insert("id".into(), id);
                }
                r
            }
            Err(e) => json!({"id": null, "ok": false, "error": format!("bad JSON: {e}")}),
        };
        if !write_line(&mut out, &reply) {
            break;
        }
    }
}

/// A token file's default place, for documentation and the CLI.
pub fn default_token_file() -> PathBuf {
    match plumcraft_session::paths::home_dir() {
        Some(home) => home.join(".config/plumcraft/control.token"),
        None => std::env::temp_dir().join("plumcraft-control.token"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn connect(port: u16) -> (TcpStream, BufReader<TcpStream>) {
        let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let r = BufReader::new(s.try_clone().unwrap());
        (s, r)
    }

    fn roundtrip(s: &mut TcpStream, r: &mut BufReader<TcpStream>, v: Value) -> Value {
        writeln!(s, "{v}").unwrap();
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[test]
    fn tokens() {
        let t = generate_token();
        assert_eq!(t.len(), 64);
        validate_token(&t).unwrap();
        assert_ne!(t, generate_token());
        assert!(validate_token("abc").is_err());
        assert!(validate_token(&"g".repeat(64)).is_err());
        assert!(token_matches(TOKEN, &TOKEN.to_uppercase()));
        assert!(!token_matches(TOKEN, &TOKEN.replace('0', "1")));
        assert!(!token_matches(TOKEN, "short"));
    }

    #[test]
    fn token_files_are_created_then_reused() {
        let dir = plumcraft_scratch::root().join("token-test");
        let path = dir.join("control.token");
        let _ = std::fs::remove_dir_all(&dir);
        let a = server_token(Some(&path)).unwrap();
        let b = server_token(Some(&path)).unwrap();
        assert_eq!(a, b);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::write(&path, "nope").unwrap();
        assert!(server_token(Some(&path)).is_err());
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(server_token(None).unwrap().len(), 64);
    }

    #[cfg(unix)]
    #[test]
    fn a_token_file_others_can_reach_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = plumcraft_scratch::dir("token-private");
        let path = dir.join("control.token");
        let token = server_token(Some(&path)).unwrap();
        for mode in [0o640, 0o604, 0o666] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            let e = server_token(Some(&path)).unwrap_err().to_string();
            assert!(e.contains("chmod 600"), "{mode:o}: {e}");
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(server_token(Some(&path)).unwrap(), token);
        let link = dir.join("link.token");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(server_token(Some(&link)).unwrap_err().to_string().contains("plain file"));
    }

    #[test]
    fn auth_first_then_requests_reach_the_ui() {
        let (rx, port) = start(0, TOKEN.into(), Arc::new(|| {})).unwrap();
        let ui = std::thread::spawn(move || {
            let req = rx.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(req.method, "ui.inspect");
            assert!(!req.expired());
            req.reply.send(json!({"ok": true, "result": {"nodes": 0}})).unwrap();
        });
        let (mut s, mut r) = connect(port);
        let denied = roundtrip(&mut s, &mut r, json!({"id": 1, "method": "ui.inspect"}));
        assert_eq!(denied["error"], "authentication required");
        // The connection is closed after a failed auth.
        let (mut s, mut r) = connect(port);
        let ok = roundtrip(&mut s, &mut r, json!({"id": "a", "method": "auth", "params": {"token": TOKEN}}));
        assert_eq!((ok["id"].as_str(), ok["ok"].as_bool()), (Some("a"), Some(true)));
        let again = roundtrip(&mut s, &mut r, json!({"id": 2, "method": "auth", "params": {"token": TOKEN}}));
        assert_eq!(again["error"], "already authenticated");
        let reply = roundtrip(&mut s, &mut r, json!({"id": 3, "method": "ui.inspect"}));
        assert_eq!(reply, json!({"id": 3, "ok": true, "result": {"nodes": 0}}));
        writeln!(s, "{{not json").unwrap();
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert!(line.contains("bad JSON"), "{line}");
        ui.join().unwrap();
    }

    #[test]
    fn a_silent_client_is_dropped_at_the_auth_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let t = Instant::now();
            serve(stream, TOKEN, &tx, &|| {}, Duration::from_millis(300));
            t.elapsed()
        });
        let _s = TcpStream::connect(addr).unwrap();
        assert!(server.join().unwrap() < Duration::from_secs(3));
        assert!(rx.try_recv().is_err());
    }
}

#[cfg(test)]
mod bind_tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn local_means_this_machine_or_its_own_network() {
        for ip in ["127.0.0.1", "10.0.0.5", "172.16.1.1", "192.168.1.20", "169.254.3.4", "::1", "fd12::1", "fe80::1", "::ffff:192.168.0.2"] {
            assert!(is_local(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "172.32.0.1", "2001:4860::8888", "::ffff:8.8.8.8", "0.0.0.0"] {
            assert!(!is_local(ip.parse().unwrap()), "{ip}");
        }
        if let Some(ip) = lan_address() {
            assert!(is_local(ip) && !ip.is_loopback());
        }
    }

    #[test]
    fn a_server_on_another_address_still_wants_the_token() {
        // IPv6 loopback: an address other than 127.0.0.1 that never leaves this machine
        let ip = IpAddr::V6(Ipv6Addr::LOCALHOST);
        let token = generate_token();
        let Ok((srv, _rx)) = Server::start_on(ip, 0, token.clone(), Arc::new(|| {})) else {
            // a host without IPv6 loopback: nothing to show here
            return;
        };
        let mut s = TcpStream::connect((ip, srv.port())).unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        writeln!(s, "{}", json!({"id": 1, "method": "auth", "params": {"token": "0".repeat(64)}})).unwrap();
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert!(line.contains("authentication required"), "{line}");
        let _ = Ipv4Addr::LOCALHOST;
    }
}
