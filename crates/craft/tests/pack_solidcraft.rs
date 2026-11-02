//! The SolidCraft pack: it parses, and with `solidcraft-cli` (on PATH or at `$PLUMCRAFT_SOLIDCRAFT_CLI`) the
//! engine lists every curated node and runs its demo: new → parameter → box → cut cylinder → fillet → colour → PNG, STL,
//! STEP, measure.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{Manifest, builtin, demo};
use plumcraft_proto::data::{DataRef, Form, Locator};
use serde_json::Value;

const CURATED: &[&str] = &[
    "sol.open",
    "sol.new",
    "sol.parameter",
    "sol.box",
    "sol.cylinder",
    "sol.sphere",
    "sol.fillet",
    "sol.chamfer",
    "sol.shell",
    "sol.combine",
    "sol.move",
    "sol.mirror",
    "sol.pattern",
    "sol.appearance",
    "sol.measure",
    "sol.inspect",
    "sol.export_png",
    "sol.export_stl",
    "sol.export_step",
    "sol.export_3mf",
    "sol.export_obj",
];

#[test]
fn the_solidcraft_pack_parses() {
    let m = builtin("solidcraft").unwrap().unwrap();
    assert_eq!((m.id.as_str(), m.doc.wire.as_str(), m.doc.ext.as_str()), ("solidcraft", "solidcraft.doc", "solidcraft"));
    assert_eq!(m.roots.as_ref().map(|r| (r.read.as_str(), r.write.as_str())), Some(("--allow-path", "--allow-path")));
    assert!(m.actions.close_all.is_none(), "one design per process: new and open replace it");
    let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, CURATED);
    let d: Value = serde_json::from_str(demo("solidcraft").unwrap()).unwrap();
    assert!(d["nodes"]["stl"].is_object());
}

/// The pack with its program pointed at a built solidcraft-cli, or `None` when there is none.
fn solidcraft() -> Option<Manifest> {
    let mut m = builtin("solidcraft").unwrap().unwrap();
    if m.executable().is_some() {
        return Some(m);
    }
    match std::env::var("PLUMCRAFT_SOLIDCRAFT_CLI").ok().map(PathBuf::from).filter(|p| p.is_file()) {
        Some(p) => {
            m.command[0] = p.display().to_string();
            Some(m)
        }
        None => {
            eprintln!("SKIP: solidcraft-cli is not built; {}", m.hint);
            None
        }
    }
}

/// A blob's bytes: from its path (a published blob is a commonkit `.npy` of `u1`, unwrapped
/// here), decoded from its inline base64, or read through the store (a compressed blob has a
/// `commonkit` locator only).
fn bytes(d: &DataRef) -> Vec<u8> {
    for l in &d.at {
        match l {
            Locator::Path { path, .. } => return unnpy(std::fs::read(path).unwrap()),
            Locator::Inline { b64 } => return unbase64(b64),
            _ => {}
        }
    }
    plumcraft_store::Store::new(store_root()).resolve(d, true).unwrap_or_else(|| panic!("{:?} doesn't resolve", d.handle)).bytes.into_vec()
}

fn unnpy(b: Vec<u8>) -> Vec<u8> {
    if !b.starts_with(b"\x93NUMPY") {
        return b;
    }
    let (len, at) = if b[6] == 1 { (u16::from_le_bytes([b[8], b[9]]) as usize, 10) } else { (u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize, 12) };
    b[at + len..].to_vec()
}

fn unbase64(s: &str) -> Vec<u8> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0u32);
    for c in s.bytes().filter(|c| *c != b'=' && !c.is_ascii_whitespace()) {
        let v = A.iter().position(|a| *a == c).unwrap() as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

#[test]
fn solidcraft_runs_its_demo() {
    let Some(m) = solidcraft() else { return };
    let opts = plumcraft_craft::Options { root: Some(craft_root("solidcraft")), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(&m, &opts).unwrap();
    eprintln!("SolidCraft {}: {} commands; nodes {:?}; dropped {:?}", report.server_version, report.commands.len(), report.curated, report.dropped);
    assert!(report.dropped.is_empty(), "{:?}", report.dropped);
    assert_eq!(report.curated, CURATED);
    assert_eq!(report.hatch, ["sol.run"]);
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(60));
    let h: Conn = Box::new(plumcraft_host::InProc(h));

    let g: Value = serde_json::from_str(demo("solidcraft").unwrap()).unwrap();
    let r = go(&h, "sol-demo", g.clone(), &["colour", "png", "stl", "step", "measure"], "off");
    assert!(r.ok, "{:?}", r.nodes.iter().map(|(k, n)| (k, n.state, n.error.as_ref().map(|e| e.message.clone()))).collect::<Vec<_>>());
    let doc = blob(&r, "colour", "doc");
    assert_eq!(doc.meta.form, Form::Doc { app: "solidcraft".into() });
    let png = bytes(&blob(&r, "png", "image"));
    assert!(png.starts_with(b"\x89PNG") && png.len() > 2000, "png: {} bytes", png.len());
    let (w, hgt) = (u32::from_be_bytes([png[16], png[17], png[18], png[19]]), u32::from_be_bytes([png[20], png[21], png[22], png[23]]));
    assert_eq!((w, hgt), (480, 320));
    let stl = bytes(&blob(&r, "stl", "stl"));
    // Binary STL: an 80-byte header, a u32 triangle count, 50 bytes a triangle.
    let tris = u32::from_le_bytes([stl[80], stl[81], stl[82], stl[83]]) as usize;
    assert!(tris > 12 && stl.len() == 84 + 50 * tris, "{tris} triangles in {} bytes", stl.len());
    let step = String::from_utf8(bytes(&blob(&r, "step", "step"))).unwrap();
    assert!(step.starts_with("ISO-10303-21"), "{}", &step[..step.len().min(80)]);
    // 40 × 30 × 20 less a Ø16 hole through it, less the fillets: below the box, above the box less the hole.
    let m = json_out(&r, "measure", "result");
    let vol = m.pointer("/bodies/0/volume_mm3").or_else(|| m.pointer("/total/volume_mm3")).and_then(Value::as_f64).unwrap_or_else(|| panic!("{m}"));
    let hole = std::f64::consts::PI * 64.0 * 20.0;
    assert!(vol < 24000.0 - hole && vol > 24000.0 - hole - 500.0, "volume {vol}");
    let again = go(&h, "sol-demo2", g, &["colour"], "off");
    assert_eq!(blob(&again, "colour", "doc").handle, doc.handle, "SolidCraft saves are byte-stable");
}
