//! The FigCraft pack: it parses, and with `figcraft-cli` on PATH the engine lists every curated
//! node and runs its demo (new → rectangle → ellipse → subtract → fill → shadow → PNG, SVG, CSS).
//!
//! FigCraft's `list_commands` answers in prose, so the catalogue comes from the JSON resource
//! `figcraft://commands` (`tools.list_resource`).

mod common;

use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{builtin, demo};
use plumcraft_proto::data::{DataRef, Form, Locator};
use serde_json::{Value, json};

const CURATED: &[&str] = &[
    "fig.open",
    "fig.new",
    "fig.frame",
    "fig.rectangle",
    "fig.ellipse",
    "fig.text",
    "fig.set",
    "fig.auto_layout",
    "fig.align",
    "fig.tidy",
    "fig.group",
    "fig.frame_selection",
    "fig.fill",
    "fig.effect",
    "fig.boolean",
    "fig.flatten",
    "fig.find_replace",
    "fig.code",
    "fig.pages",
    "fig.export_png",
    "fig.export_svg",
    "fig.export_pdf",
];

#[test]
fn the_figcraft_pack_parses() {
    let m = builtin("figcraft").unwrap().unwrap();
    assert_eq!((m.id.as_str(), m.doc.wire.as_str(), m.doc.ext.as_str()), ("figcraft", "figcraft.doc", "figcraft"));
    assert!(m.roots.is_some());
    assert_eq!(m.env.get("FIGCRAFT_NO_SYSTEM_FONTS").map(String::as_str), Some("1"), "renders are byte-stable only with the built-in fonts");
    let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, CURATED);
    assert_eq!(m.actions.export.as_ref().and_then(|e| e.params.get("ids")), Some(&serde_json::json!([])), "exports are the whole page");
    let d: Value = serde_json::from_str(demo("figcraft").unwrap()).unwrap();
    assert!(d["nodes"]["png"].is_object());
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
fn figcraft_runs_its_demo() {
    let m = builtin("figcraft").unwrap().unwrap();
    if m.executable().is_none() {
        eprintln!("SKIP: figcraft-cli is not on PATH; {}", m.hint);
        return;
    }
    let opts = plumcraft_craft::Options { root: Some(craft_root("figcraft")), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(&m, &opts).unwrap();
    eprintln!("FigCraft {}: {} commands; nodes {:?}; dropped {:?}", report.server_version, report.commands.len(), report.curated, report.dropped);
    assert!(report.dropped.is_empty(), "{:?}", report.dropped);
    assert_eq!(report.curated, CURATED);
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(60));
    let h: Conn = Box::new(plumcraft_host::InProc(h));

    let mut g: Value = serde_json::from_str(demo("figcraft").unwrap()).unwrap();
    g["nodes"]["pdf"] = json!({"kind": "fig.export_pdf", "values": {}});
    g["wires"].as_array_mut().unwrap().push(json!({"from": {"node": "shadow", "port": "doc"}, "to": {"node": "pdf", "port": "doc"}}));
    let r = go(&h, "fc-demo", g.clone(), &["shadow", "png", "svg", "pdf", "css"], "off");
    assert!(r.ok, "{:?}", states(&r));
    let doc = blob(&r, "shadow", "doc");
    assert_eq!(doc.meta.form, Form::Doc { app: "figcraft".into() });
    assert!(bytes(&doc).starts_with(b"PK"), "a .figcraft is a zip");
    let png = bytes(&blob(&r, "png", "image"));
    assert!(png.starts_with(b"\x89PNG") && png.len() > 1000, "png: {} bytes", png.len());
    // 2x of the 300 × 220 shape plus its shadow: about 624 × 464.
    let (w, hgt) = (u32::from_be_bytes([png[16], png[17], png[18], png[19]]), u32::from_be_bytes([png[20], png[21], png[22], png[23]]));
    assert!(w > 500 && hgt > 400, "{w} × {hgt}");
    let pdf = bytes(&blob(&r, "pdf", "pdf"));
    assert!(pdf.starts_with(b"%PDF") && pdf.len() > 500, "pdf: {} bytes", pdf.len());
    let svg = String::from_utf8(bytes(&blob(&r, "svg", "svg"))).unwrap();
    assert!(svg.contains("<svg") && svg.to_ascii_lowercase().contains("#7c3aed"), "{svg}");
    let css = json_out(&r, "css", "result");
    assert!(css["code"].as_str().is_some_and(|c| c.contains("box-shadow")), "{css}");
    let again = go(&h, "fc-demo2", g, &["shadow"], "off");
    assert_eq!(blob(&again, "shadow", "doc").handle, doc.handle, "FigCraft saves are byte-stable");
}
