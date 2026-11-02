//! The DesignCraft pack: it parses; with `designcraft-cli` on PATH the engine lists every curated
//! node, the demo (new → frames → swatch → fill → shadow → preflight, inspect, PNG, PDF) runs
//! with byte-stable saves and non-empty exports, and every doc-taking node runs with its
//! defaults.

mod common;

use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{Kind, builtin};
use plumcraft_proto::data::Form;
use plumcraft_proto::run::RunReply;
use serde_json::{Value, json};

const CURATED: &[&str] = &[
    "dsc.open",
    "dsc.new",
    "dsc.sample",
    "dsc.file_info",
    "dsc.add_pages",
    "dsc.document_setup",
    "dsc.margins_columns",
    "dsc.text_frame",
    "dsc.shape",
    "dsc.equation",
    "dsc.replace",
    "dsc.replace_font",
    "dsc.swatch",
    "dsc.fill",
    "dsc.opacity",
    "dsc.drop_shadow",
    "dsc.corners",
    "dsc.rotate",
    "dsc.move",
    "dsc.outlines",
    "dsc.inspect",
    "dsc.preflight",
    "dsc.export_pdf",
    "dsc.export_png",
    "dsc.export_jpg",
];

#[test]
fn the_pack_parses() {
    let m = builtin("designcraft").expect("built in").expect("parses");
    assert_eq!((m.doc.wire.as_str(), m.doc.ext.as_str()), ("designcraft.doc", "designcraft"));
    let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).filter(|id| !m.exclude.contains_key(*id)).collect();
    assert_eq!(ids, CURATED);
    assert!(m.exclude.is_empty());
    assert!(m.nodes.iter().filter(|n| n.kind == Kind::Export).all(|n| n.action.is_some()));
    assert!(m.gui.is_some() && m.roots.is_some());
    let run = m.run_node.as_ref().expect("dsc.run");
    assert!(!run.allow.iter().any(|a| a.starts_with("file.") && a != "file.info" && a != "file.info.get"));
    let demo: Value = serde_json::from_str(plumcraft_craft::manifest::demo("designcraft").expect("a demo")).expect("demo JSON");
    for n in demo["nodes"].as_object().expect("nodes").values() {
        assert!(ids.contains(&n["kind"].as_str().unwrap_or_default()), "{n}");
    }
}

fn live() -> Option<(Conn, plumcraft_craft::Report)> {
    let m = builtin("designcraft")?.ok()?;
    if m.executable().is_none() {
        eprintln!("SKIP: designcraft-cli is not on PATH; {}", m.hint);
        return None;
    }
    let root = craft_root("designcraft");
    let opts = plumcraft_craft::Options { root: Some(root), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(&m, &opts).expect("the engine builds");
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(60));
    Some((Box::new(plumcraft_host::InProc(h)), report))
}

fn errors(r: &RunReply) -> Vec<(String, Option<plumcraft_proto::error::NodeError>)> {
    r.nodes.iter().filter(|(_, n)| n.error.is_some()).map(|(k, n)| (k.clone(), n.error.clone())).collect()
}

/// The document a run saved, less `created`: DesignCraft stamps a new document in whole seconds,
/// so two saves match byte for byte only within one second.
fn saved_document(h: &impl plumcraft_host::Connection, r: &RunReply) -> Value {
    let bytes = h.resolve(&blob(r, "shadow", "doc")).expect("the document resolves").bytes;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a DesignCraft document is a zip");
    let file = zip.by_name("document.json").expect("it holds document.json");
    let mut doc: Value = serde_json::from_reader(file).expect("document.json is JSON");
    doc.as_object_mut().expect("an object").remove("created");
    doc
}

#[test]
fn designcraft_lists_and_runs_its_nodes() {
    let Some((h, report)) = live() else { return };
    eprintln!("DesignCraft {}: {} commands; nodes {:?}; dropped {:?}", report.server_version, report.commands.len(), report.curated, report.dropped);
    assert_eq!(report.curated, CURATED, "dropped: {:?}", report.dropped);
    assert_eq!(report.hatch, ["dsc.run"]);

    let demo: Value = serde_json::from_str(plumcraft_craft::manifest::demo("designcraft").unwrap()).unwrap();
    let r = go(&h, "demo", demo.clone(), &["title", "shadow", "check", "inspect", "png", "pdf"], "off");
    assert!(r.ok, "{:?} {:?}", states(&r), errors(&r));
    assert_eq!(blob(&r, "shadow", "doc").meta.form, Form::Doc { app: "designcraft".into() });
    assert!(json_out(&r, "title", "id").as_u64().is_some());
    assert_eq!(json_out(&r, "check", "errors"), json!(0));
    let inspect = json_out(&r, "inspect", "result");
    let text = inspect.to_string();
    assert!(text.contains("PlumCraft Quarterly") || text.contains("Plum"), "{}", &text[..text.len().min(2000)]);
    let png = blob(&r, "png", "image");
    assert!(matches!(png.meta.form, Form::Image { width: 612, height: 792 }), "{:?}", png.meta.form);
    assert!(h.resolve(&png).expect("the PNG resolves").bytes.starts_with(b"\x89PNG"));
    let pdf = h.resolve(&blob(&r, "pdf", "pdf")).expect("the PDF resolves").bytes;
    assert!(pdf.starts_with(b"%PDF-") && pdf.len() > 1000, "{} bytes", pdf.len());
    let again = go(&h, "demo2", demo, &["shadow"], "off");
    assert_eq!(saved_document(&h, &again), saved_document(&h, &r), "DesignCraft saves the same document twice");

    // Every node that takes a document runs with its defaults on a page with a text frame and a
    // shape (so the selection nodes have something to act on).
    let m = builtin("designcraft").unwrap().unwrap();
    let mut nodes = serde_json::Map::new();
    nodes.insert("n".into(), json!({"kind": "dsc.new", "values": {}}));
    nodes.insert("t".into(), json!({"kind": "dsc.text_frame", "values": {}}));
    nodes.insert("s".into(), json!({"kind": "dsc.shape", "values": {}}));
    let mut wires = vec![
        json!({"from": {"node": "n", "port": "doc"}, "to": {"node": "t", "port": "doc"}}),
        json!({"from": {"node": "t", "port": "doc"}, "to": {"node": "s", "port": "doc"}}),
    ];
    let mut targets = Vec::new();
    for n in m.nodes.iter().filter(|n| !matches!(n.kind, Kind::Open | Kind::New) && !m.exclude.contains_key(&n.id)) {
        let id = n.id.replace('.', "_");
        nodes.insert(id.clone(), json!({"kind": n.id, "values": {}}));
        wires.push(json!({"from": {"node": "s", "port": "doc"}, "to": {"node": id, "port": "doc"}}));
        targets.push(id);
    }
    let t: Vec<&str> = targets.iter().map(String::as_str).collect();
    let r = go(&h, "all", json!({"version": 1, "nodes": nodes, "wires": wires}), &t, "off");
    assert!(r.ok && errors(&r).is_empty(), "{:?}", errors(&r));
}
