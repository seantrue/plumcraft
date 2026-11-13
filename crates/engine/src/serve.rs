//! The engine side of the local socket binding (mode 2, spec 16.3): an engine process connects
//! back to the UI's socket (a Unix socket, or loopback TCP on Windows: [`crate::local`]), presents its token in `hello`, and serves every operation through an
//! in-process [`Host`]. Each request runs on its own thread, so nothing waits behind a run; every
//! event is pushed as a `{"method": "event"}` notification. The process exits when the socket
//! closes, so a UI that crashes leaves no orphan.
//!
//! Stdout is never the data channel (some engines print on import); diagnostics go to stderr.

use std::io::BufReader;
use std::sync::{Arc, Mutex};

use plumcraft_proto::error::{ErrorCode, ProtoError};
use serde_json::Value;

use crate::host::{Factory, Host};
use crate::local::{Endpoint, Stream};
use crate::register::Registration;
use crate::rpc::{self, Frame, Request, Response};

/// The environment variables the UI sets when it spawns an engine.
pub const ENV_SOCKET: &str = "PLUMCRAFT_ENGINE_SOCKET";
pub const ENV_TOKEN: &str = "PLUMCRAFT_ENGINE_TOKEN";

/// The whole of an engine binary's `main`: `register`, `unregister` and `--help`
/// ([`crate::register`]), then mode 3 with `--listen host:port [--port-file f] [--lifeline]`,
/// else mode 2 (`--connect unix:<path>`, `--connect tcp:127.0.0.1:<port>` or the environment). `program` names it in messages.
pub fn main(program: &str, factory: Factory, registration: &Registration) -> std::process::ExitCode {
    use std::process::ExitCode;
    let args: Vec<String> = std::env::args().collect();
    if let Some(done) = crate::register::handle(registration, program, &args) {
        return done;
    }
    let served = if let Some((listen, port_file)) = crate::http::listen_args(&args) {
        let token = std::env::var(ENV_TOKEN).unwrap_or_default();
        crate::http::lifeline(&args);
        crate::http::serve_http(factory, &listen, &token, port_file.as_deref(), crate::http::shared(&args))
    } else {
        match endpoint_from_env(&args) {
            Ok((socket, token)) => serve_unix(factory, &socket, &token),
            Err(e) => {
                eprintln!("{program}: {e}");
                return ExitCode::from(2);
            }
        }
    };
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{program}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Where to connect and the token to present: `--connect <locator>` or the environment. The
/// locator is `unix:<path>`, `tcp:127.0.0.1:<port>` or a bare socket path ([`Endpoint::parse`]).
pub fn endpoint_from_env(args: &[String]) -> Result<(String, String), String> {
    let from_arg =
        args.iter().position(|a| a == "--connect").and_then(|i| args.get(i + 1)).filter(|s| s.starts_with("unix:") || s.starts_with("tcp:")).cloned();
    let socket = from_arg
        .or_else(|| std::env::var(ENV_SOCKET).ok())
        .ok_or("no socket: pass --connect unix:<path> (or tcp:127.0.0.1:<port>) or set PLUMCRAFT_ENGINE_SOCKET")?;
    let token = std::env::var(ENV_TOKEN).unwrap_or_default();
    Ok((socket, token))
}

/// Serve the endpoint a locator names ([`Endpoint::parse`]) until the UI closes it. The name is
/// from when the binding was Unix sockets only; [`serve_local`] takes an [`Endpoint`].
pub fn serve_unix(factory: Factory, socket: &str, token: &str) -> std::io::Result<()> {
    let at = Endpoint::parse(socket).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    serve_local(factory, &at, token)
}

/// Serve until the UI closes the connection.
pub fn serve_local(factory: Factory, at: &Endpoint, token: &str) -> std::io::Result<()> {
    let stream = at.connect()?;
    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    let host = Arc::new(Host::start_proving(factory, None));

    // Push every event as a notification, in order.
    let push_host = Arc::clone(&host);
    let push_writer = Arc::clone(&writer);
    std::thread::Builder::new().name("plumcraft-push".into()).spawn(move || {
        let mut since = 0;
        loop {
            let p = push_host.poll(since, 1000, None);
            since = p.last_seq;
            for e in p.events {
                let note = Request::notify("event", serde_json::to_value(&e).unwrap_or(Value::Null));
                let mut w = push_writer.lock().unwrap_or_else(|e| e.into_inner());
                if rpc::write_frame(&mut *w, &note).is_err() {
                    return;
                }
            }
        }
    })?;

    let mut reader = BufReader::new(stream);
    let mut authorised = token.is_empty();
    loop {
        let line = match rpc::read_frame(&mut reader, rpc::MAX_FRAME) {
            Ok(Some(l)) => l,
            Ok(None) | Err(_) => return Ok(()), // the UI went away, or sent garbage: stop
        };
        let req = match rpc::parse(&line) {
            Ok(Frame::Request(r)) => r,
            Ok(Frame::Response(_)) => continue,
            Err(e) => {
                reply(&writer, Response::new(None, Err(e)));
                continue;
            }
        };
        if req.method == "hello" {
            let presented = req.params.get("token").and_then(Value::as_str).unwrap_or_default();
            authorised = token.is_empty() || rpc::same_token(presented, token);
        }
        if !authorised {
            if req.id.is_some() {
                reply(&writer, Response::new(req.id, Err(ProtoError::new(ErrorCode::Unauthorized, "hello with the engine's token first"))));
            }
            continue;
        }
        if req.method == "shutdown" {
            return Ok(());
        }
        let host = Arc::clone(&host);
        let writer = Arc::clone(&writer);
        // Each request on its own thread: a long-poll never blocks a cancel.
        let _ = std::thread::Builder::new().name("plumcraft-req".into()).spawn(move || {
            let outcome = host.dispatch(&req.method, req.params);
            if req.id.is_some() {
                reply(&writer, Response::new(req.id, outcome));
            }
        });
    }
}

fn reply(writer: &Mutex<Stream>, r: Response) {
    let mut w = writer.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = rpc::write_frame(&mut *w, &r) {
        eprintln!("plumcraft engine: reply failed: {e}");
    }
}
