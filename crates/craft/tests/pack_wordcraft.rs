//! The WordCraft pack: it parses; with `wordcraft-cli` on PATH the engine lists every curated
//! node, and the demo (new → replace → style set → watermark → PNG and PDF) and a text chain run
//! end to end. Live cases skip when the CLI is absent.

mod common;

use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{Kind, builtin};
use plumcraft_proto::data::Form;
use serde_json::{Value, json};

const CURATED: &[&str] = &[
    "wc.open",
    "wc.new",
    "wc.set_text",
    "wc.replace",
    "wc.accept_changes",
    "wc.reject_changes",
    "wc.clean",
    "wc.properties",
    "wc.paragraph_style",
    "wc.font",
    "wc.font_size",
    "wc.change_case",
    "wc.line_spacing",
    "wc.style_set",
    "wc.theme",
    "wc.watermark",
    "wc.orientation",
    "wc.paper_size",
    "wc.margins",
    "wc.columns",
    "wc.word_count",
    "wc.info",
    "wc.export_pdf",
    "wc.export_png",
    "wc.export_txt",
    "wc.export_md",
    "wc.export_html",
    "wc.export_odt",
];

#[test]
fn the_pack_parses() {
    let m = builtin("wordcraft").expect("built in").expect("parses");
    assert_eq!(m.doc.wire, "wordcraft.doc");
    assert_eq!(m.doc.ext, "docx");
    let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, CURATED);
    assert!(m.nodes.iter().any(|n| n.kind == Kind::Export && n.ext.as_deref() == Some("png")));
    assert!(m.gui.is_some() && m.roots.is_some());
    let run = m.run_node.as_ref().expect("wc.run");
    assert!(!run.allow.iter().any(|a| a.starts_with("file.save") || a.starts_with("file.print") || a == "file." || a.starts_with("mailings")));
    let demo: Value = serde_json::from_str(plumcraft_craft::manifest::demo("wordcraft").expect("a demo")).expect("demo JSON");
    for n in demo["nodes"].as_object().expect("nodes").values() {
        assert!(ids.contains(&n["kind"].as_str().unwrap_or_default()), "{n}");
    }
}

fn live() -> Option<(Conn, plumcraft_craft::Report)> {
    let m = builtin("wordcraft")?.ok()?;
    if m.executable().is_none() {
        eprintln!("SKIP: wordcraft-cli is not on PATH; {}", m.hint);
        return None;
    }
    let root = craft_root("wordcraft");
    let opts = plumcraft_craft::Options { root: Some(root), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(&m, &opts).expect("the engine builds");
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(60));
    Some((Box::new(plumcraft_host::InProc(h)), report))
}

#[test]
fn wordcraft_lists_and_runs_its_nodes() {
    let Some((h, report)) = live() else { return };
    eprintln!("WordCraft {}: {} commands; nodes {:?}; dropped {:?}", report.server_version, report.commands.len(), report.curated, report.dropped);
    assert_eq!(report.curated, CURATED, "dropped: {:?}", report.dropped);
    assert_eq!(report.hatch, ["wc.run"]);

    // The demo workflow.
    let demo: Value = serde_json::from_str(plumcraft_craft::manifest::demo("wordcraft").unwrap()).unwrap();
    let r = go(&h, "demo", demo, &["mark", "png", "pdf", "count"], "off");
    assert!(r.ok, "{:?} {:?}", states(&r), r.nodes.iter().map(|(k, n)| (k, &n.error)).collect::<Vec<_>>());
    assert_eq!(blob(&r, "mark", "doc").meta.form, Form::Doc { app: "wordcraft".into() });
    let png = blob(&r, "png", "image");
    assert!(matches!(png.meta.form, Form::Image { width, height } if width > 500 && height > 500), "{:?}", png.meta.form);
    let bytes = h.resolve(&png).expect("the PNG resolves").bytes;
    assert!(bytes.starts_with(b"\x89PNG"), "a PNG");
    let pdf = h.resolve(&blob(&r, "pdf", "pdf")).expect("the PDF resolves").bytes;
    assert!(pdf.starts_with(b"%PDF-") && pdf.len() > 1000);
    assert!(json_out(&r, "count", "words").as_u64().unwrap_or(0) > 100, "{:?}", json_out(&r, "count", "result"));

    // Text in, replaced, formatted on the whole body, out as text.
    let g = chain(&[
        ("n", "wc.new", json!({})),
        ("t", "wc.set_text", json!({"text": "Dear {{name}},\nThank you."})),
        ("r", "wc.replace", json!({"text": "{{name}}", "with": "Plum"})),
        ("f", "wc.font", json!({"name": "Georgia"})),
        ("c", "wc.change_case", json!({"mode": "upper"})),
        ("x", "wc.export_txt", json!({})),
    ]);
    let r = go(&h, "text", g, &["r", "x"], "off");
    assert!(r.ok, "{:?} {:?}", states(&r), r.nodes.iter().map(|(k, n)| (k, &n.error)).collect::<Vec<_>>());
    assert_eq!(json_out(&r, "r", "replaced"), json!(1));
    let txt = String::from_utf8(h.resolve(&blob(&r, "x", "text")).expect("the text resolves").bytes.into_vec()).unwrap();
    assert!(txt.contains("DEAR PLUM,") && txt.contains("THANK YOU."), "{txt:?}");

    // Every node that takes a document runs with its defaults on the sample.
    let m = builtin("wordcraft").unwrap().unwrap();
    let mut nodes = serde_json::Map::new();
    nodes.insert("n".into(), json!({"kind": "wc.new", "values": {"template": "sample"}}));
    let mut wires = Vec::new();
    let mut targets = Vec::new();
    for n in m.nodes.iter().filter(|n| !matches!(n.kind, Kind::Open | Kind::New)) {
        let id = n.id.replace('.', "_");
        nodes.insert(id.clone(), json!({"kind": n.id, "values": {}}));
        wires.push(json!({"from": {"node": "n", "port": "doc"}, "to": {"node": id, "port": "doc"}}));
        targets.push(id);
    }
    let g = json!({"version": 1, "nodes": nodes, "wires": wires});
    let t: Vec<&str> = targets.iter().map(String::as_str).collect();
    let r = go(&h, "all", g, &t, "off");
    let failed: Vec<_> = r.nodes.iter().filter(|(_, n)| n.error.is_some()).map(|(k, n)| (k.clone(), n.error.clone())).collect();
    assert!(r.ok && failed.is_empty(), "{failed:?}");
}
