//! The DeckCraft pack: it parses; with `deckcraft-cli` on PATH the engine lists every curated
//! node, the demo (new → outline → replace → transition → footer → PNG, PDF, info) runs, an
//! outline exports as text, and every doc-taking node runs with its defaults on a starter
//! template. Live cases skip when the CLI is absent.

mod common;

use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{Kind, builtin};
use plumcraft_proto::data::Form;
use serde_json::{Value, json};

const CURATED: &[&str] = &[
    "dk.open",
    "dk.new",
    "dk.properties",
    "dk.add_slide",
    "dk.from_outline",
    "dk.delete_slide",
    "dk.duplicate_slide",
    "dk.move_slide",
    "dk.hide_slide",
    "dk.slide_layout",
    "dk.notes",
    "dk.transition",
    "dk.replace",
    "dk.replace_font",
    "dk.theme",
    "dk.variant",
    "dk.background",
    "dk.header_footer",
    "dk.slide_size",
    "dk.inspect",
    "dk.info",
    "dk.export_png",
    "dk.export_pdf",
    "dk.export_pptx",
    "dk.export_gif",
    "dk.export_outline",
];

#[test]
fn the_pack_parses() {
    let m = builtin("deckcraft").expect("built in").expect("parses");
    assert_eq!((m.doc.wire.as_str(), m.doc.ext.as_str()), ("deckcraft.doc", "deckcraft"));
    let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, CURATED);
    assert!(m.nodes.iter().any(|n| n.kind == Kind::Export && n.ext.as_deref() == Some("png")));
    assert!(m.gui.is_some() && m.roots.is_some() && m.actions.close_all.is_some());
    let run = m.run_node.as_ref().expect("dk.run");
    assert!(!run.allow.iter().any(|a| a.starts_with("file.save") || a.starts_with("file.export") || a.starts_with("show.") || a.starts_with("media.")));
    let demo: Value = serde_json::from_str(plumcraft_craft::manifest::demo("deckcraft").expect("a demo")).expect("demo JSON");
    for n in demo["nodes"].as_object().expect("nodes").values() {
        assert!(ids.contains(&n["kind"].as_str().unwrap_or_default()), "{n}");
    }
}

fn live() -> Option<(Conn, plumcraft_craft::Report)> {
    let m = builtin("deckcraft")?.ok()?;
    if m.executable().is_none() {
        eprintln!("SKIP: deckcraft-cli is not on PATH; {}", m.hint);
        return None;
    }
    let root = craft_root("deckcraft");
    let opts = plumcraft_craft::Options { root: Some(root), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(&m, &opts).expect("the engine builds");
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(60));
    Some((Box::new(plumcraft_host::InProc(h)), report))
}

fn errors(r: &plumcraft_proto::run::RunReply) -> Vec<(String, Option<plumcraft_proto::error::NodeError>)> {
    r.nodes.iter().filter(|(_, n)| n.error.is_some()).map(|(k, n)| (k.clone(), n.error.clone())).collect()
}

#[test]
fn deckcraft_lists_and_runs_its_nodes() {
    let Some((h, report)) = live() else { return };
    eprintln!("DeckCraft {}: {} commands; nodes {:?}; dropped {:?}", report.server_version, report.commands.len(), report.curated, report.dropped);
    assert_eq!(report.curated, CURATED, "dropped: {:?}", report.dropped);
    assert_eq!(report.hatch, ["dk.run"]);

    // The demo workflow.
    let demo: Value = serde_json::from_str(plumcraft_craft::manifest::demo("deckcraft").unwrap()).unwrap();
    let r = go(&h, "demo", demo, &["outline", "title", "footer", "png", "pdf", "info"], "off");
    assert!(r.ok, "{:?} {:?}", states(&r), errors(&r));
    assert_eq!(blob(&r, "footer", "doc").meta.form, Form::Doc { app: "deckcraft".into() });
    assert_eq!(json_out(&r, "outline", "slides"), json!(2));
    assert_eq!(json_out(&r, "title", "replaced"), json!(1));
    assert_eq!(json_out(&r, "info", "slides"), json!(3));
    let png = blob(&r, "png", "image");
    assert!(matches!(png.meta.form, Form::Image { width: 960, height: 540 }), "{:?}", png.meta.form);
    assert!(h.resolve(&png).expect("the PNG resolves").bytes.starts_with(b"\x89PNG"));
    let pdf = h.resolve(&blob(&r, "pdf", "pdf")).expect("the PDF resolves").bytes;
    assert!(pdf.starts_with(b"%PDF-") && pdf.len() > 1000);

    // The same graph again: the presentation saves byte-stable, so the handles repeat.
    let demo: Value = serde_json::from_str(plumcraft_craft::manifest::demo("deckcraft").unwrap()).unwrap();
    let again = go(&h, "demo2", demo, &["footer"], "off");
    assert_eq!(blob(&again, "footer", "doc").handle, blob(&r, "footer", "doc").handle, "DeckCraft saves are byte-stable");

    // An outline in, out as outline text.
    let g = chain(&[
        ("n", "dk.new", json!({"theme": "Paper"})),
        ("o", "dk.from_outline", json!({"text": "# Alpha\n- one\n# Beta\n- two"})),
        ("d", "dk.delete_slide", json!({"index": 0})),
        ("x", "dk.export_outline", json!({})),
    ]);
    let r = go(&h, "outline", g, &["x"], "off");
    assert!(r.ok, "{:?} {:?}", states(&r), errors(&r));
    let txt = String::from_utf8(h.resolve(&blob(&r, "x", "text")).expect("the text resolves").bytes.into_vec()).unwrap();
    assert!(txt.starts_with("Alpha") && txt.contains("Beta") && txt.contains("two"), "{txt:?}");

    // Every node that takes a document runs with its defaults on a starter template.
    let m = builtin("deckcraft").unwrap().unwrap();
    let mut nodes = serde_json::Map::new();
    nodes.insert("n".into(), json!({"kind": "dk.new", "values": {"template": "Pitch"}}));
    let mut wires = Vec::new();
    let mut targets = Vec::new();
    for n in m.nodes.iter().filter(|n| !matches!(n.kind, Kind::Open | Kind::New)) {
        let id = n.id.replace('.', "_");
        // text.replace fails when nothing matches; the default `{{name}}` is a placeholder.
        let values = if n.id == "dk.replace" { json!({"text": "the", "with": "THE"}) } else { json!({}) };
        nodes.insert(id.clone(), json!({"kind": n.id, "values": values}));
        wires.push(json!({"from": {"node": "n", "port": "doc"}, "to": {"node": id, "port": "doc"}}));
        targets.push(id);
    }
    let t: Vec<&str> = targets.iter().map(String::as_str).collect();
    let r = go(&h, "all", json!({"version": 1, "nodes": nodes, "wires": wires}), &t, "off");
    assert!(r.ok && errors(&r).is_empty(), "{:?}", errors(&r));

    // The export options: a PDF of one chosen slide (1-based) has one page, and a GIF of two
    // slides that doesn't loop.
    let g = graph(
        json!({
            "n": {"kind": "dk.new", "values": {"template": "Pitch"}},
            "every": {"kind": "dk.export_pdf", "values": {}},
            "one": {"kind": "dk.export_pdf", "values": {"slides": "2"}},
            "gif": {"kind": "dk.export_gif", "values": {"slides": "1-2", "loop": false}}
        }),
        &[("n", "doc", "every", "doc"), ("n", "doc", "one", "doc"), ("n", "doc", "gif", "doc")],
    );
    let r = go(&h, "options", g, &["every", "one", "gif"], "off");
    assert!(r.ok, "{:?} {:?}", states(&r), errors(&r));
    // Page objects: `/Type /Page` (or `/Type/Page`) not followed by `s` (the page tree).
    let pages = |node: &str| {
        let pdf = h.resolve(&blob(&r, node, "pdf")).expect("the PDF resolves").bytes;
        let mut n = 0;
        for (i, _) in pdf.windows(5).enumerate().filter(|(_, w)| w == b"/Type") {
            let rest = pdf[i + 5..].trim_ascii_start();
            if rest.starts_with(b"/Page") && rest.get(5) != Some(&b's') {
                n += 1;
            }
        }
        n
    };
    let (every, one) = (pages("every"), pages("one"));
    assert!(every > 1 && one == 1, "{every} pages, then {one}");
    let gif = h.resolve(&blob(&r, "gif", "gif")).expect("the GIF resolves").bytes;
    // One graphic control extension a frame: two slides, two frames.
    let frames = gif.windows(3).filter(|w| w == b"\x21\xf9\x04").count();
    assert!(gif.starts_with(b"GIF8") && frames == 2, "{frames} frames");
}
