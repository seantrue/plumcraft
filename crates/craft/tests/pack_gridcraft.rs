//! The GridCraft pack: it parses, and with `gridcraft-cli` on PATH the engine lists every curated
//! node and runs its demo (new grades sample → sort → chart → PDF, and a read of the sorted rows).

mod common;

use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{Kind, builtin, demo};
use plumcraft_proto::data::{DataRef, Form, Locator};
use serde_json::{Value, json};

const CURATED: &[&str] = &[
    "gc.open",
    "gc.new",
    "gc.add_sheet",
    "gc.set_cell",
    "gc.write_range",
    "gc.replace",
    "gc.number_format",
    "gc.format_as_table",
    "gc.sort",
    "gc.remove_duplicates",
    "gc.subtotal",
    "gc.pivot",
    "gc.chart",
    "gc.read",
    "gc.evaluate",
    "gc.inspect",
    "gc.export_pdf",
    "gc.export_csv",
    "gc.export_html",
];

#[test]
fn the_gridcraft_pack_parses() {
    let m = builtin("gridcraft").unwrap().unwrap();
    assert_eq!((m.id.as_str(), m.doc.wire.as_str(), m.doc.ext.as_str()), ("gridcraft", "gridcraft.doc", "xlsx"));
    assert!(m.roots.is_none(), "gridcraft-cli mcp has no automation roots");
    let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, CURATED);
    assert!(m.nodes.iter().any(|n| n.kind == Kind::Export && n.ext.as_deref() == Some("pdf")));
    assert_eq!(m.run_node.as_ref().map(|r| r.id.as_str()), Some("gc.run"));
    let d: Value = serde_json::from_str(demo("gridcraft").unwrap()).unwrap();
    assert!(d["nodes"]["pdf"].is_object());
}

/// A blob's bytes: from its path (a published blob is a commonkit `.npy` of `u1`, unwrapped
/// here), or decoded from its inline base64.
fn bytes(d: &DataRef) -> Vec<u8> {
    for l in &d.at {
        match l {
            Locator::Path { path, .. } => return unnpy(std::fs::read(path).unwrap()),
            Locator::Inline { b64 } => return unbase64(b64),
            _ => {}
        }
    }
    panic!("no path or inline locator for {:?}", d.handle);
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
fn gridcraft_runs_its_demo() {
    let m = builtin("gridcraft").unwrap().unwrap();
    if m.executable().is_none() {
        eprintln!("SKIP: gridcraft-cli is not on PATH; {}", m.hint);
        return;
    }
    let opts = plumcraft_craft::Options { root: Some(craft_root("gridcraft")), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(&m, &opts).unwrap();
    eprintln!("GridCraft {}: {} commands; nodes {:?}; dropped {:?}", report.server_version, report.commands.len(), report.curated, report.dropped);
    assert!(report.dropped.is_empty(), "{:?}", report.dropped);
    assert_eq!(report.curated, CURATED);
    assert_eq!(report.hatch, ["gc.run"]);
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(60));
    let h: Conn = Box::new(plumcraft_host::InProc(h));

    let g: Value = serde_json::from_str(demo("gridcraft").unwrap()).unwrap();
    let r = go(&h, "gc-demo", g, &["chart", "pdf", "read"], "off");
    assert!(r.ok, "{:?}", states(&r));
    let doc = blob(&r, "chart", "doc");
    assert_eq!(doc.meta.form, Form::Doc { app: "gridcraft".into() });
    assert!(bytes(&doc).starts_with(b"PK"), "an xlsx is a zip");
    let pdf = bytes(&blob(&r, "pdf", "pdf"));
    assert!(pdf.starts_with(b"%PDF") && pdf.len() > 1000, "{} bytes", pdf.len());
    // Sorted by Average, highest first: Elif Yılmaz (97.25), then Amara Okafor (91.5).
    let rows = json_out(&r, "read", "values");
    assert_eq!(rows[0][0], json!("Student"));
    assert_eq!(rows[1][0], json!("Elif Yılmaz"), "{rows}");
    assert_eq!(rows[2][0], json!("Amara Okafor"), "{rows}");

    // write → formula → CSV, and the saves are byte-stable (memo-able).
    let chain_g = chain(&[
        ("n", "gc.new", json!({})),
        ("w", "gc.write_range", json!({"range": "A1", "values": [["x", "y"], [1, 2], [3, 4], ["sum", "=SUM(B2:B3)"]]})),
        ("c", "gc.export_csv", json!({})),
    ]);
    let a = go(&h, "gc-csv", chain_g.clone(), &["w", "c"], "off");
    assert!(a.ok, "{:?}", states(&a));
    let csv = String::from_utf8(bytes(&blob(&a, "c", "csv"))).unwrap();
    assert!(csv.contains("sum,6"), "{csv}");
    let b = go(&h, "gc-csv2", chain_g, &["w", "c"], "off");
    assert_eq!(blob(&a, "w", "doc").handle, blob(&b, "w", "doc").handle, "two saves of one workbook are byte-identical");
}
