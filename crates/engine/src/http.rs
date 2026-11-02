//! The engine side of the HTTP binding (mode 3, spec 16.4; plan 10.2, D2).
//!
//! | Method | Path | |
//! |---|---|---|
//! | `GET` | `/v1/health` | `{ok, engine, state}` |
//! | `POST` | `/v1/rpc` | Every operation as one JSON-RPC 2.0 request, through [`Host::dispatch`]; `events.poll` long-polls here |
//! | `POST` | `/v1/blobs/has` | `{"handles": […]}` → `{"have", "missing"}` (spec 13.1) |
//! | `PUT` | `/v1/blobs/md5/raw/<hex>` | An upload, verified against its handle before it is stored (422 `integrity` otherwise) |
//! | `GET` | `/v1/blobs/md5/{raw,nd1}/<hex>` | A blob's bytes; `Range: bytes=a-b` honoured |
//!
//! Every request carries `Authorization: Bearer <token>`, and the server won't start without one,
//! on any address: a page in a browser can reach loopback.
//! running graph never blocks a cancel. Runs outlive a dropped connection (spec 16.4).
//!
//! Not yet: SSE (`GET /v1/events`), resumable uploads, `get_many`, derivatives, TLS (use an SSH
//! tunnel or a TLS proxy).

use std::io::Read;
use std::net::SocketAddr;
use std::sync::Arc;

use plumcraft_proto::error::{ErrorCode, ProtoError};
use plumcraft_proto::handle::{Handle, Recipe};
use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::host::{Factory, Host};
use crate::rpc::{self, Frame, MAX_FRAME, Response as RpcResponse};

/// The largest upload held in memory (v1; resumable uploads are later).
pub const MAX_UPLOAD: usize = 512 << 20;

/// Where to listen and what to write the bound address to: `--listen host:port` and
/// `--port-file <path>` (port 0 picks one; the file lets a supervisor find it).
pub fn listen_args(args: &[String]) -> Option<(String, Option<String>)> {
    let get = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    Some((get("--listen")?, get("--port-file")))
}

/// `--lifeline`: exit when stdin closes. A UI that spawns an HTTP engine passes it with a pipe on
/// stdin, so the engine goes when the UI does, however the UI ends (a crash, `kill`, an exit that
/// skips destructors). An engine started by hand on a server leaves it off.
pub fn lifeline(args: &[String]) {
    if !args.iter().any(|a| a == "--lifeline") {
        return;
    }
    let _ = std::thread::Builder::new().name("plumcraft-lifeline".into()).spawn(|| {
        let mut sink = [0u8; 256];
        let mut stdin = std::io::stdin();
        while matches!(stdin.read(&mut sink), Ok(n) if n > 0) {}
        eprintln!("plumcraft engine: the UI that started this engine has gone; exiting");
        std::process::exit(0);
    });
}

/// Serve until the process is stopped.
pub fn serve_http(factory: Factory, listen: &str, token: &str, port_file: Option<&str>) -> std::io::Result<()> {
    let addr: SocketAddr = listen.parse().map_err(|e| std::io::Error::other(format!("{listen}: {e}")))?;
    // On any address: loopback is within reach of every web page the user has open.
    if token.is_empty() {
        return Err(std::io::Error::other("an HTTP engine needs a token: set PLUMCRAFT_ENGINE_TOKEN"));
    }
    let server = Server::http(addr).map_err(|e| std::io::Error::other(e.to_string()))?;
    let bound = server.server_addr().to_ip().map_or_else(|| listen.to_owned(), |a| a.to_string());
    if let Some(f) = port_file {
        let tmp = format!("{f}.tmp");
        std::fs::write(&tmp, &bound)?;
        std::fs::rename(&tmp, f)?;
    }
    eprintln!("plumcraft engine: listening on http://{bound}");
    let host = Arc::new(Host::start_proving(factory, None));
    host.serve_blob_urls();
    let token: Arc<str> = Arc::from(token);
    for req in server.incoming_requests() {
        let (host, token) = (Arc::clone(&host), Arc::clone(&token));
        let _ = std::thread::Builder::new().name("plumcraft-http".into()).spawn(move || handle(&host, &token, req));
    }
    Ok(())
}

fn handle(host: &Host, token: &str, mut req: Request) {
    let url = req.url().to_owned();
    let path = url.split('?').next().unwrap_or_default().to_owned();
    if !authorised(&req, token) {
        return respond_json(req, 401, &json!({"code": ErrorCode::Unauthorized.code(), "message": "a bearer token is required"}));
    }
    if req.method() == &Method::Get && path == "/v1/health" {
        let state = serde_json::to_value(host.state()).unwrap_or(Value::Null);
        return respond_json(req, 200, &json!({"ok": true, "state": state}));
    }
    match (req.method().clone(), path.as_str()) {
        (Method::Post, "/v1/rpc") => {
            let Some(body) = read_body(&mut req, MAX_FRAME) else {
                return respond_json(req, 413, &json!({"code": ErrorCode::TooLarge.code(), "message": "request too large"}));
            };
            let reply = match rpc::parse(&body) {
                Ok(Frame::Request(r)) => RpcResponse::new(r.id, host.dispatch(&r.method, r.params)),
                Ok(Frame::Response(_)) => RpcResponse::new(None, Err(ProtoError::new(ErrorCode::InvalidRequest, "expected a request"))),
                Err(e) => RpcResponse::new(None, Err(e)),
            };
            respond_json(req, 200, &serde_json::to_value(reply).unwrap_or(Value::Null));
        }
        (Method::Post, "/v1/blobs/has") => {
            let body = read_body(&mut req, MAX_FRAME).unwrap_or_default();
            let handles: Vec<String> =
                serde_json::from_slice::<Value>(&body).ok().and_then(|v| serde_json::from_value(v["handles"].clone()).ok()).unwrap_or_default();
            let store = host.store();
            let (have, missing): (Vec<String>, Vec<String>) =
                handles.into_iter().take(10_000).partition(|h| h.parse::<Handle>().is_ok_and(|h| store.as_ref().is_some_and(|s| s.has(&h))));
            respond_json(req, 200, &json!({"have": have, "missing": missing}));
        }
        (Method::Put, p) if p.starts_with("/v1/blobs/") => {
            let Some(handle) = handle_from_path(p).filter(|h| h.recipe() == Recipe::Raw) else {
                return respond_json(req, 400, &json!({"code": ErrorCode::BadParams.code(), "message": "uploads are md5:raw: handles"}));
            };
            let Some(store) = host.store() else {
                return respond_json(req, 503, &json!({"code": ErrorCode::Unavailable.code(), "message": "this engine has no store"}));
            };
            if store.has(&handle) {
                return respond_json(req, 200, &json!({"stored": false, "had": true}));
            }
            let Some(body) = read_body(&mut req, MAX_UPLOAD) else {
                return respond_json(
                    req,
                    413,
                    &json!({"code": ErrorCode::TooLarge.code(), "message": format!("uploads are limited to {} MiB", MAX_UPLOAD >> 20)}),
                );
            };
            if !plumcraft_store::verifies(&handle, "|u1", &[body.len() as u64], &body) {
                respond_json(req, 422, &json!({"code": ErrorCode::Integrity.code(), "message": "the bytes don't match the handle"}));
            } else if store.put_verified(&handle, "|u1", &[body.len() as u64], &body) {
                respond_json(req, 201, &json!({"stored": true}));
            } else {
                respond_json(
                    req,
                    503,
                    &json!({"code": ErrorCode::Unavailable.code(), "message": "the engine's store declined the upload (its budget, free-space floor or size limit)"}),
                );
            }
        }
        (Method::Get, p) if p.starts_with("/v1/blobs/") => {
            let blob = handle_from_path(p).and_then(|h| host.store().and_then(|s| s.read_handle(&h)));
            let Some(blob) = blob else {
                return respond_json(req, 404, &json!({"code": ErrorCode::NotFound.code(), "message": "no such blob"}));
            };
            let total = blob.bytes.len();
            let range = req.headers().iter().find(|h| h.field.equiv("Range")).and_then(|h| parse_range(h.value.as_str(), total));
            let etag = handle_from_path(p).map(|h| h.to_string()).unwrap_or_default();
            let mut headers =
                vec![header("Content-Type", "application/octet-stream"), header("ETag", &format!("\"{etag}\"")), header("Cache-Control", "immutable")];
            let (status, bytes) = match range {
                Some((a, b)) => {
                    headers.push(header("Content-Range", &format!("bytes {a}-{b}/{total}")));
                    (206, blob.bytes.get(a..=b).unwrap_or_default().to_vec())
                }
                None => (200, blob.bytes.into_vec()),
            };
            let mut resp = Response::from_data(bytes).with_status_code(status);
            for h in headers.into_iter().flatten() {
                resp.add_header(h);
            }
            let _ = req.respond(resp);
        }
        _ => respond_json(req, 404, &json!({"code": ErrorCode::UnknownOp.code(), "message": format!("no route {path}")})),
    }
}

fn authorised(req: &Request, token: &str) -> bool {
    !token.is_empty()
        && req
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .and_then(|h| h.value.as_str().strip_prefix("Bearer "))
            .is_some_and(|t| rpc::same_token(t, token))
}

/// `/v1/blobs/<algo>/<recipe>/<hex>` as a handle.
fn handle_from_path(p: &str) -> Option<Handle> {
    let rest = p.strip_prefix("/v1/blobs/")?;
    let mut parts = rest.split('/');
    let (algo, recipe, hex) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    format!("{algo}:{recipe}:{hex}").parse().ok()
}

/// `bytes=a-b`, `bytes=a-` or `bytes=-n` within `total`; anything else is the whole blob.
fn parse_range(v: &str, total: usize) -> Option<(usize, usize)> {
    let spec = v.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let last = total.checked_sub(1)?;
    let (a, b) = match (a.trim(), b.trim()) {
        ("", n) => (total.saturating_sub(n.parse().ok()?), last),
        (a, "") => (a.parse().ok()?, last),
        (a, b) => (a.parse().ok()?, b.parse::<usize>().ok()?.min(last)),
    };
    (a <= b).then_some((a, b))
}

fn read_body(req: &mut Request, max: usize) -> Option<Vec<u8>> {
    if req.body_length().is_some_and(|n| n > max) {
        return None;
    }
    let mut body = Vec::new();
    req.as_reader().take(max as u64 + 1).read_to_end(&mut body).ok()?;
    (body.len() <= max).then_some(body)
}

fn header(k: &str, v: &str) -> Option<Header> {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).ok()
}

fn respond_json(req: Request, status: u16, v: &Value) {
    let mut resp = Response::from_data(serde_json::to_vec(v).unwrap_or_default()).with_status_code(status);
    if let Some(h) = header("Content-Type", "application/json") {
        resp.add_header(h);
    }
    let _ = req.respond(resp);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-9", 100), Some((0, 9)));
        assert_eq!(parse_range("bytes=90-", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=50-500", 100), Some((50, 99)));
        assert_eq!(parse_range("bytes=9-0", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
        assert_eq!(parse_range("bytes=0-0", 0), None);
    }

    #[test]
    fn blob_paths() {
        let h = Handle::md5_raw(b"x");
        assert_eq!(handle_from_path(&crate::data::blob_url(&h)), Some(h));
        assert!(handle_from_path("/v1/blobs/md5/raw/../../etc").is_none());
        assert!(handle_from_path("/v1/blobs/md5/raw/abc/extra").is_none());
    }
}
