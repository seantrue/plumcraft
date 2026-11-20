//! The shell conformance suite (`plumcraft_editor_core::conformance`, N2 slice 4) through the C
//! ABI, headless, as a native shell drives it: requests and replies through `plum_request` and
//! `plum_poll` after a wake, measurements reported typed (`plum_geometry_set`), and the drag's
//! move through the typed hot call (`plum_gesture_move`). A second driver runs the same cases
//! over the core's own control port, as a script (or a shell's test harness) would.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use common::{Client, store_root};
use plumcraft_editor_core::conformance::{self, Driver};
use plumcraft_ffi::{PLUM_OK, PlumFace, plum_geometry_set, plum_gesture_move};
use serde_json::{Value, json};

struct Abi(Client);

fn floats(v: &Value) -> Vec<f32> {
    v.as_array().into_iter().flatten().map(|x| x.as_f64().map_or(f32::NAN, |f| f as f32)).collect()
}

impl Driver for Abi {
    fn name(&self) -> String {
        "the C ABI, headless".into()
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.0.call(method, params)
    }
    fn reports(&self) -> bool {
        true
    }
    fn report_geometry(&mut self, faces: &Value) -> Result<(), String> {
        let scene = self.0.call("scene.get", json!({}));
        let nid = |id: &str| scene["result"]["nodes"].as_array().into_iter().flatten().find(|f| f["id"] == id).and_then(|f| f["nid"].as_u64());
        let list = faces.as_array().ok_or("faces must be a list")?;
        let pins: Vec<(Vec<f32>, Vec<f32>)> = list.iter().map(|f| (floats(&f["pins"]["in"]), floats(&f["pins"]["out"]))).collect();
        let mut typed = Vec::new();
        for (f, (ins, outs)) in list.iter().zip(&pins) {
            let id = f["node"].as_str().ok_or("a face's node must be an id")?;
            let size = floats(&f["size"]);
            typed.push(PlumFace {
                face: nid(id).ok_or_else(|| format!("no face {id} in the scene"))?,
                rev: 0,
                w: size.first().copied().unwrap_or(0.0),
                h: size.get(1).copied().unwrap_or(0.0),
                header: f["header"].as_f64().map_or(f32::NAN, |h| h as f32),
                inputs: ins.as_ptr(),
                n_inputs: ins.len(),
                outputs: outs.as_ptr(),
                n_outputs: outs.len(),
                rows: std::ptr::null(),
                n_rows: 0,
            });
        }
        // SAFETY: the faces and the pins they point at are live for the call.
        let status = unsafe { plum_geometry_set(self.0.core, 0, typed.as_ptr(), typed.len()) };
        if status != PLUM_OK {
            return Err(format!("plum_geometry_set: {status}"));
        }
        // the core has them once a request after them sees them measured (they are applied in order)
        let t0 = Instant::now();
        for f in list {
            loop {
                let r = self.0.call("geometry.locate", json!({"node": f["node"]}));
                if r["result"]["measured"] == true {
                    break;
                }
                if t0.elapsed() > Duration::from_secs(20) {
                    return Err(format!("never measured: {}", f["node"]));
                }
                self.0.wait_wake(Duration::from_millis(20));
            }
        }
        Ok(())
    }
    fn hot_move(&mut self, at: [f32; 2], slop: f32) -> Option<Value> {
        let d = plum_gesture_move(self.0.core, 0, at[0], at[1], slop);
        if d.status != PLUM_OK {
            return Some(json!({"status": d.status}));
        }
        let kinds = ["none", "node", "pin", "wire", "bend", "tag", "frame", "group"];
        let t = d.target;
        let mut v = json!({"status": d.status, "kind": d.kind, "serial": d.serial, "dx": d.dx, "dy": d.dy,
                           "target": {"kind": kinds.get(t.kind as usize).copied().unwrap_or("?"), "nid": t.node, "pin": t.pin, "fits": t.fits, "at": [t.x, t.y]}});
        if d.kind == 3 {
            v["rect"] = json!(d.rect);
        }
        Some(v)
    }
}

#[test]
fn the_suite_passes_through_the_c_abi() {
    let mut d = Abi(Client::open(&json!({"store_root": store_root("conformance")})));
    let out = conformance::run_all(&mut d).unwrap();
    let all = out.iter().all(|o| o.error.is_none());
    assert!(all, "{}", conformance::summary(&d.name(), &out));
    assert!(out.len() >= 15);
    d.0.close();
}

/// A script on the core's control port: the protocol's JSON lines, as a Swift or Kotlin shell's
/// test harness would drive its own core.
struct Port {
    stream: BufReader<TcpStream>,
    next: u64,
}

impl Port {
    fn send(&mut self, v: &Value) -> Value {
        let mut line = v.to_string();
        line.push('\n');
        self.stream.get_mut().write_all(line.as_bytes()).unwrap();
        let mut reply = String::new();
        self.stream.read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap_or_else(|e| json!({"ok": false, "error": format!("not JSON: {e}: {reply}")}))
    }
}

impl Driver for Port {
    fn name(&self) -> String {
        "the C ABI's control port".into()
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let v = json!({"id": self.next, "method": method, "params": params});
        self.send(&v)
    }
    fn reports(&self) -> bool {
        true
    }
    fn report_geometry(&mut self, faces: &Value) -> Result<(), String> {
        let r = self.call("command.run", json!({"command": "geometry.set", "params": {"faces": faces}}));
        if r["ok"] == true { Ok(()) } else { Err(format!("geometry.set: {r}")) }
    }
}

#[test]
fn the_suite_passes_over_the_control_port() {
    let dir = plumcraft_scratch::dir("ffi-conformance-port");
    let token_file = dir.join("token");
    let mut c = Client::open(&json!({"store_root": store_root("conformance-port"), "control": {"port": 0, "token_file": token_file}}));
    let port = c.call("core.control", json!({}))["result"]["port"].as_u64().expect("a control port");
    let token = std::fs::read_to_string(&token_file).unwrap().trim().to_owned();
    let stream = TcpStream::connect(("127.0.0.1", u16::try_from(port).unwrap())).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let mut d = Port { stream: BufReader::new(stream), next: 0 };
    let auth = d.send(&json!({"id": "auth", "method": "auth", "params": {"token": token}}));
    assert_eq!(auth["ok"], true, "{auth}");
    let out = conformance::run_all(&mut d).unwrap();
    assert!(out.iter().all(|o| o.error.is_none()), "{}", conformance::summary(&d.name(), &out));
    drop(d);
    c.close();
}
