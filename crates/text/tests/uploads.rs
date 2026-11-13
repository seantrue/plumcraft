//! An engine this host didn't start gets local files only once they are allowed.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use plumcraft_host::Connection;
use plumcraft_host::http::{HttpHost, HttpOptions};
use serde_json::json;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn an_attached_engine_gets_files_only_once_they_are_allowed() {
    let dir = plumcraft_scratch::dir("uploads");
    let port_file = dir.join("port");
    let mut child = Command::new(env!("CARGO_BIN_EXE_plumcraft-text-engine"))
        .args(["--listen", "127.0.0.1:0", "--port-file"])
        .arg(&port_file)
        .env("PLUMCRAFT_ENGINE_TOKEN", TOKEN)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let addr = loop {
        match std::fs::read_to_string(&port_file) {
            Ok(a) if !a.trim().is_empty() => break a,
            _ if Instant::now() > deadline => panic!("no port"),
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let host = HttpHost::attach(&format!("http://{}", addr.trim()), TOKEN, HttpOptions { remote: true, cas: dir.join("cas") }).unwrap();

    let file = dir.join("mine.txt");
    std::fs::write(&file, "hello").unwrap();
    let path = file.display().to_string();
    let run = json!({"graph": {"graph": {"version": 1, "nodes": {
        "a": {"kind": "text.load", "values": {"path": path}},
        "gone": {"kind": "text.load", "values": {"path": dir.join("no-such-file").display().to_string()}}}}}});

    let pending = host.uploads_pending(&run);
    assert_eq!(pending, [(host.base().to_owned(), path.clone())], "only what exists, and where it goes");
    assert_eq!(host.uploads(), 0);
    host.allow_uploads(&[path]);
    assert!(host.uploads_pending(&run).is_empty());
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn an_http_engine_needs_a_token_on_loopback_too() {
    let out = Command::new(env!("CARGO_BIN_EXE_plumcraft-text-engine"))
        .args(["--listen", "127.0.0.1:0"])
        .env_remove("PLUMCRAFT_ENGINE_TOKEN")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs a token"), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn nothing_answers_without_the_token() {
    use std::io::{Read, Write};
    let dir = plumcraft_scratch::dir("uploads-health");
    let port_file = dir.join("port");
    let mut child = Command::new(env!("CARGO_BIN_EXE_plumcraft-text-engine"))
        .args(["--listen", "127.0.0.1:0", "--port-file"])
        .arg(&port_file)
        .env("PLUMCRAFT_ENGINE_TOKEN", TOKEN)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let addr = loop {
        match std::fs::read_to_string(&port_file) {
            Ok(a) if !a.trim().is_empty() => break a,
            _ if Instant::now() > deadline => panic!("no port"),
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let get = |what: &str| {
        let mut s = std::net::TcpStream::connect(addr.trim()).unwrap();
        write!(s, "{what} HTTP/1.1\r\nHost: evil.example\r\nOrigin: https://evil.example\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        let mut reply = String::new();
        s.read_to_string(&mut reply).unwrap();
        reply
    };
    for what in ["GET /v1/health", "POST /v1/rpc", "GET /v1/blobs/md5/raw/00"] {
        assert!(get(what).starts_with("HTTP/1.1 401"), "{what}");
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// D16 (spec 4.3): an engine serving a store shared by several clients (`--shared`) refuses an
/// md5 upload whatever the switch, and takes a b3 one. The suite runs once per algorithm.
#[test]
fn a_shared_remote_refuses_md5_uploads() {
    use std::io::{Read, Write};
    let dir = plumcraft_scratch::dir("uploads-shared");
    let port_file = dir.join("port");
    let mut child = Command::new(env!("CARGO_BIN_EXE_plumcraft-text-engine"))
        .args(["--listen", "127.0.0.1:0", "--shared", "--port-file"])
        .arg(&port_file)
        .env("PLUMCRAFT_ENGINE_TOKEN", TOKEN)
        .env("COMMONKIT_STORE_ROOT", dir.join("store"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let addr = loop {
        match std::fs::read_to_string(&port_file) {
            Ok(a) if !a.trim().is_empty() => break a,
            _ if Instant::now() > deadline => panic!("no port"),
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let h = plumcraft_proto::Handle::raw(b"hello");
    let mut s = std::net::TcpStream::connect(addr.trim()).unwrap();
    let url = plumcraft_engine::data::blob_url(&h);
    write!(s, "PUT {url} HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello").unwrap();
    let mut reply = String::new();
    s.read_to_string(&mut reply).unwrap();
    let _ = child.kill();
    let _ = child.wait();
    let status = reply.split_whitespace().nth(1).unwrap_or_default().to_owned();
    if h.algo() == plumcraft_proto::handle::Algo::Md5 {
        assert!(status == "403" && reply.contains("b3 uploads only"), "{reply}");
    } else {
        assert_eq!(status, "201", "{reply}");
    }
}
