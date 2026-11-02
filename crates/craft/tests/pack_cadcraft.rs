//! The CADCraft pack (`packs/cadcraft.toml`): it parses, its demo uses only its nodes, and, when
//! `cadcraft-cli` is on PATH, its curated nodes all pass the catalogue check, the demo chain
//! (new → rectangle → circle → hatch → text → polar array → SVG and PNG) exports a drawing, and
//! the fixture DXF opens, rotates and exports.

mod common;

use std::path::Path;
use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{Kind, Manifest, builtin, demo};
use plumcraft_proto::data::Form;
use plumcraft_proto::run::RunReply;
use serde_json::{Value, json};

const CURATED: &[&str] = &[
    "cad.new",
    "cad.open",
    "cad.inspect",
    "cad.entities",
    "cad.layer",
    "cad.line",
    "cad.pline",
    "cad.rectangle",
    "cad.circle",
    "cad.polygon",
    "cad.text",
    "cad.dim_linear",
    "cad.hatch",
    "cad.move",
    "cad.rotate",
    "cad.scale",
    "cad.mirror",
    "cad.array_rect",
    "cad.array_polar",
    "cad.explode",
    "cad.properties",
    "cad.overkill",
    "cad.export_svg",
    "cad.export_png",
    "cad.export_pdf",
    "cad.export_dwg",
];

fn cadcraft() -> Option<Manifest> {
    let m = builtin("cadcraft").unwrap().unwrap();
    if m.executable().is_none() {
        eprintln!("SKIP: cadcraft-cli is not on PATH; {}", m.hint);
        return None;
    }
    Some(m)
}

fn connect(m: &Manifest, name: &str) -> (Conn, plumcraft_craft::Report) {
    let opts = plumcraft_craft::Options { root: Some(craft_root(name)), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(m, &opts).unwrap();
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(60));
    (Box::new(plumcraft_host::InProc(h)), report)
}

fn failures(r: &RunReply) -> Vec<(String, String)> {
    r.nodes.iter().filter_map(|(k, n)| n.error.as_ref().map(|e| (k.clone(), format!("{}: {}", e.kind, e.message)))).collect()
}

fn fixture() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cadcraft-triangle.dxf").display().to_string()
}

#[test]
fn the_cadcraft_pack_parses_and_its_demo_uses_its_nodes() {
    let m = builtin("cadcraft").unwrap().unwrap();
    let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, CURATED);
    assert!(m.roots.is_none(), "cadcraft-cli mcp has no automation roots");
    assert!(m.nodes.iter().any(|n| n.kind == Kind::Open) && m.nodes.iter().any(|n| n.kind == Kind::New));
    // Round 2: a new drawing's warm and cold saves differ, so only queries, open and exports are pure.
    for n in &m.nodes {
        let pure = matches!(n.kind, Kind::Query | Kind::Export | Kind::Open);
        assert_eq!(n.pure, pure, "{}", n.id);
    }
    assert_eq!(m.selection.as_ref().and_then(|s| s.set_key.as_deref()), Some("handles"));
    assert_eq!(m.gui.as_ref().and_then(|g| g.env.as_deref()), Some("PLUMCRAFT_CADCRAFT_APP"));
    let d: Value = serde_json::from_str(demo("cadcraft").unwrap()).unwrap();
    for (name, n) in d["nodes"].as_object().unwrap() {
        let kind = n["kind"].as_str().unwrap();
        assert!(ids.contains(&kind), "demo node {name} is {kind}");
    }
}

#[test]
fn the_cadcraft_demo_chain_exports_svg_and_png() {
    let Some(m) = cadcraft() else { return };
    let (h, report) = connect(&m, "pack-cadcraft");
    eprintln!("CADCraft {}: nodes {:?}; hatch {:?}; dropped {:?}", report.server_version, report.curated, report.hatch, report.dropped);
    assert!(report.dropped.is_empty(), "{:?}", report.dropped);
    assert_eq!(report.curated, CURATED);
    assert_eq!(report.hatch, ["cad.run"]);
    let d: Value = serde_json::from_str(demo("cadcraft").unwrap()).unwrap();
    let r = go(&h, "cad-demo", Value::clone(&d), &["array", "svg", "png"], "use");
    assert!(r.ok, "{:?}", failures(&r));
    let doc = blob(&r, "array", "doc");
    assert_eq!(doc.meta.form, Form::Doc { app: "cadcraft".into() });
    // A document is published like any value (inline when small): read it as the UI would.
    let dxf = h.resolve(&doc).expect("the document resolves").bytes;
    assert!(String::from_utf8_lossy(&dxf).contains("HATCH"), "the DXF has the hatch");
    let svg = blob(&r, "svg", "svg");
    assert!(matches!(svg.meta.form, Form::Vector { .. }), "{:?}", svg.meta.form);
    let svg_text = String::from_utf8(h.resolve(&svg).unwrap().bytes.into_vec()).unwrap();
    assert!(svg_text.contains("<svg"), "an SVG");
    let png = blob(&r, "png", "image");
    assert!(matches!(png.meta.form, Form::Image { width, height } if width > 100 && height > 100), "{:?}", png.meta.form);
    assert!(r.nodes["png"].thumb.is_some(), "a PNG export has a thumb");

    // `new` and the transforms are memo scope none (warm and cold saves differ), so a rerun runs
    // them again; the export of an identical drawing is the same file.
    let again = go(&h, "cad-demo2", d, &["array", "svg", "png"], "use");
    assert!(again.ok, "{:?}", failures(&again));
    assert_eq!(blob(&again, "svg", "svg").handle, svg.handle);
}

#[test]
fn the_fixture_opens_rotates_and_exports() {
    let Some(m) = cadcraft() else { return };
    let (h, _) = connect(&m, "pack-cadcraft-open");
    let g = chain(&[
        ("o", "cad.open", json!({"file": fixture()})),
        ("r", "cad.rotate", json!({"base": [0, 0], "angle": 30.0})),
        ("p", "cad.export_pdf", json!({})),
    ]);
    let mut g = g;
    g["nodes"]["e"] = json!({"kind": "cad.entities", "values": {"type": "Line"}});
    g["wires"].as_array_mut().unwrap().push(json!({"from": {"node": "r", "port": "doc"}, "to": {"node": "e", "port": "doc"}}));
    let r = go(&h, "cad-open", g, &["p", "e"], "off");
    assert!(r.ok, "{:?}", failures(&r));
    assert_eq!(json_out(&r, "e", "count"), json!(3), "the fixture's three lines");
    let pdf = h.resolve(&blob(&r, "p", "pdf")).unwrap().bytes;
    assert_eq!(&pdf[0..5], b"%PDF-");
    // A missing file is a node error, not a crash.
    let bad = go(&h, "cad-bad", chain(&[("o", "cad.open", json!({"file": "/no/such/drawing.dxf"}))]), &["o"], "off");
    assert!(!bad.ok && bad.nodes["o"].error.is_some());
}

/// The other nodes and the escape hatch in one chain, then DWG and the inspect.
#[test]
fn every_cadcraft_node_runs() {
    let Some(m) = cadcraft() else { return };
    let (h, _) = connect(&m, "pack-cadcraft-all");
    let steps: Vec<(&str, &str, Value)> = vec![
        ("a", "cad.new", json!({"metric": false})),
        ("b", "cad.layer", json!({"name": "Parts", "color": "blue"})),
        ("c", "cad.line", json!({})),
        ("d", "cad.pline", json!({"width": 1.0})),
        ("e", "cad.polygon", json!({"center": [200, 0]})),
        ("f", "cad.dim_linear", json!({})),
        ("g", "cad.move", json!({"delta": [10, 10]})),
        ("h", "cad.scale", json!({"factor": 2.0})),
        ("i", "cad.mirror", json!({})),
        ("j", "cad.array_rect", json!({"rows": 1, "cols": 2, "colSpacing": 600.0})),
        ("k", "cad.explode", json!({})),
        ("l", "cad.properties", json!({"color": "green"})),
        ("m", "cad.run", json!({"command": "copy", "params": {"delta": [0, 0]}})),
        ("n", "cad.overkill", json!({})),
        ("o", "cad.export_dwg", json!({})),
    ];
    let mut g = chain(&steps);
    g["nodes"]["q"] = json!({"kind": "cad.inspect", "values": {}});
    g["wires"].as_array_mut().unwrap().push(json!({"from": {"node": "n", "port": "doc"}, "to": {"node": "q", "port": "doc"}}));
    let r = go(&h, "cad-all", g, &["n", "o", "q"], "off");
    assert!(r.ok, "{:?}", failures(&r));
    assert!(json_out(&r, "n", "deleted").as_u64().unwrap_or(0) > 0, "overkill removes the zero-offset copies");
    let dwg = h.resolve(&blob(&r, "o", "dwg")).unwrap().bytes;
    assert_eq!(&dwg[0..4], b"AC10", "a DWG");
    let s = json_out(&r, "q", "result");
    assert!(s["layers"].to_string().contains("Parts"), "{}", s["layers"]);
}

/// Round 2: selection by handle (`set_key = "handles"`). Eleven line segments take handles
/// 100..10A, so the circle drawn next is 10B; `select = "ids:10B"` moves only it.
#[test]
fn a_selection_by_handle_moves_only_that_object() {
    let Some(m) = cadcraft() else { return };
    let (h, _) = connect(&m, "pack-cadcraft-select");
    let points: Vec<Value> = (0..12).map(|i| json!([i * 10, 0])).collect();
    let mut g = chain(&[
        ("a", "cad.new", json!({})),
        ("b", "cad.line", json!({"points": points})),
        ("c", "cad.circle", json!({"center": [0, 50], "radius": 5.0})),
        ("d", "cad.move", json!({"delta": [100, 0], "select": "ids:10B"})),
        ("e", "cad.entities", json!({"type": "Circle"})),
    ]);
    g["nodes"]["f"] = json!({"kind": "cad.entities", "values": {"type": "Line", "limit": 1000}});
    g["wires"].as_array_mut().unwrap().push(json!({"from": {"node": "d", "port": "doc"}, "to": {"node": "f", "port": "doc"}}));
    let r = go(&h, "cad-select", g, &["e", "f"], "off");
    assert!(r.ok, "{:?}", failures(&r));
    let circles = json_out(&r, "e", "entities");
    assert_eq!(circles[0]["handle"], "10B", "{circles}");
    let x = circles[0]["geometry"]["center"]["x"].as_f64().unwrap_or_else(|| panic!("{circles}"));
    assert!((x - 100.0).abs() < 1e-9, "the circle moved: {circles}");
    let lines = json_out(&r, "f", "entities");
    let first_x: Vec<f64> = lines.as_array().unwrap().iter().filter_map(|l| l["geometry"]["a"]["x"].as_f64()).collect();
    assert_eq!(first_x.len(), 11);
    assert!(first_x.iter().all(|x| *x < 101.0), "the lines stayed: {first_x:?}");
}
