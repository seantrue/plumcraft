//! The PdfCraft pack (`packs/pdfcraft.toml`): it parses and keeps its safety exclusions, and, when
//! `pdfcraft-cli` is on PATH, the engine lists the curated nodes (read from `command_list`'s JSON
//! Schema params) and runs Open → Rotate → Watermark → Export PDF and text, twice: the second,
//! cold run closes the first document by its id and opens a new one (explicit document ids).

mod common;

use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{Kind, Manifest, builtin};
use serde_json::{Value, json};

fn pack() -> Manifest {
    builtin("pdfcraft").expect("pdfcraft is built in").expect("pdfcraft.toml parses")
}

fn fixture() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pdfcraft-hello.pdf").display().to_string()
}

#[test]
fn the_pack_parses_and_keeps_its_exclusions() {
    let m = pack();
    assert_eq!((m.doc.wire.as_str(), m.doc.ext.as_str(), m.doc.id_param.as_deref()), ("pdfcraft.doc", "pdf", Some("doc")));
    assert_eq!(m.actions.open.returns.as_deref(), Some("doc"));
    assert!(m.nodes.iter().any(|n| n.kind == Kind::Open));
    assert!(m.nodes.iter().filter(|n| n.kind == Kind::Export).all(|n| n.action.is_some()), "exports name their own action (no `format`)");
    let run = m.run_node.as_ref().expect("pdf.run");
    for banned in ["sign.", "ocr.", "tools.", "protect.", "print.", "form.prepare"] {
        assert!(!run.allow.iter().any(|a| a.starts_with(banned)), "{banned} is allowed");
        assert!(run.deny.iter().any(|d| d == banned), "{banned} is not denied");
    }
    assert!(m.nodes.iter().all(|n| n.command.as_deref().is_none_or(|c| !c.starts_with("sign.") && !c.starts_with("ocr.") && !c.starts_with("tools."))));
}

/// The engine in process, or `None` when `pdfcraft-cli` isn't installed.
fn live(name: &str) -> Option<(Conn, plumcraft_craft::Report)> {
    let m = pack();
    if m.executable().is_none() {
        eprintln!("SKIP: pdfcraft-cli is not on PATH; {}", m.hint);
        return None;
    }
    let opts = plumcraft_craft::Options { root: Some(craft_root(name)), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(&m, &opts).unwrap();
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(60));
    Some((Box::new(plumcraft_host::InProc(h)), report))
}

#[test]
fn pdfcraft_runs_a_chain_by_document_id() {
    let Some((h, report)) = live("pdfcraft") else { return };
    eprintln!("PdfCraft {}: {} commands; nodes {:?}; dropped {:?}", report.server_version, report.commands.len(), report.curated, report.dropped);
    let m = pack();
    let curated: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
    assert!(report.dropped.is_empty(), "dropped: {:?}", report.dropped);
    assert_eq!(report.curated, curated);

    let g = |text: &str| {
        graph(
            json!({
                "o": {"kind": "pdf.open", "values": {"file": fixture()}},
                "r": {"kind": "pdf.rotate", "values": {"degrees": 90}},
                "w": {"kind": "pdf.watermark", "values": {"text": text}},
                "pdf": {"kind": "pdf.export_pdf", "values": {}},
                "txt": {"kind": "pdf.export_text", "values": {}},
                "i": {"kind": "pdf.info", "values": {}}
            }),
            &[("o", "doc", "r", "doc"), ("r", "doc", "w", "doc"), ("w", "doc", "pdf", "doc"), ("w", "doc", "txt", "doc"), ("w", "doc", "i", "doc")],
        )
    };
    let first = go(&h, "pdf1", g("DRAFT"), &["pdf", "txt", "i"], "off");
    assert!(first.ok, "{:?}", states(&first));
    let pdf = blob(&first, "pdf", "pdf");
    assert!(pdf.meta.size > 500, "{:?}", pdf.meta);
    let txt = blob(&first, "txt", "text");
    assert!(txt.meta.size > 0, "{:?}", txt.meta);
    eprintln!("info: {}", json_out(&first, "i", "result"));

    // A different watermark: a cold open of the same file in the same process closes the first
    // document by its id and works on the new one's.
    let second = go(&h, "pdf2", g("COPY"), &["pdf"], "off");
    assert!(second.ok, "{:?}", states(&second));
    assert_ne!(blob(&second, "pdf", "pdf").handle, pdf.handle, "the watermark text changes the PDF");
    // And the first chain again gives the first document. Not the first bytes: PdfCraft's saves
    // depend on the clock, in whole seconds, through a few bytes of one compressed stream.
    let again = go(&h, "pdf3", g("DRAFT"), &["pdf"], "off");
    assert!(again.ok, "{:?}", states(&again));
    let (x, y) = (h.resolve(&blob(&again, "pdf", "pdf")).unwrap().bytes, h.resolve(&pdf).unwrap().bytes);
    assert_eq!(x.len(), y.len());
    let differ = x.iter().zip(y.iter()).filter(|(a, b)| a != b).count();
    assert!(differ < 16, "{differ} of {} bytes differ", x.len());
}

/// Two PDFs made into one, with a header and footer: Open takes a wired PDF, and Insert pages a
/// second one through its file socket.
#[test]
fn pdfcraft_joins_wired_pdfs_and_marks_every_page() {
    let Some((h, _)) = live("pdfcraft-join") else { return };
    let g = graph(
        json!({
            "src": {"kind": "pdf.open", "values": {"file": fixture()}},
            "a": {"kind": "pdf.export_pdf", "values": {}},
            "b": {"kind": "pdf.export_pdf", "values": {}},
            "back": {"kind": "pdf.open", "values": {}},
            "join": {"kind": "pdf.insert_pages", "values": {"at": 1}},
            "mark": {"kind": "pdf.header_footer", "values": {"header_left": "JOINED", "footer_right": "<<1 of n>>"}},
            "i": {"kind": "pdf.info", "values": {}},
            "t": {"kind": "pdf.text", "values": {}}
        }),
        &[
            ("src", "doc", "a", "doc"),
            ("src", "doc", "b", "doc"),
            ("a", "pdf", "back", "pdf"),
            ("back", "doc", "join", "doc"),
            ("b", "pdf", "join", "pdf"),
            ("join", "doc", "mark", "doc"),
            ("mark", "doc", "i", "doc"),
            ("mark", "doc", "t", "doc"),
        ],
    );
    let one = go(
        &h,
        "pdf-one",
        graph(json!({"o": {"kind": "pdf.open", "values": {"file": fixture()}}, "i": {"kind": "pdf.info", "values": {}}}), &[("o", "doc", "i", "doc")]),
        &["i"],
        "off",
    );
    assert!(one.ok, "{:?}", states(&one));
    let pages = |r: &Value| r.get("pages").and_then(Value::as_array).map(|p| p.len() as u64);
    let first = json_out(&one, "i", "result");
    let single = pages(&first).unwrap_or_else(|| panic!("no page count in {first}"));
    let r = go(&h, "pdf-join", g, &["i", "t"], "off");
    assert!(r.ok, "{:?}", states(&r));
    let info = json_out(&r, "i", "result");
    assert_eq!(pages(&info), Some(2 * single), "{info}");
    let text = json_out(&r, "t", "result").to_string();
    assert!(text.contains("JOINED"), "the header is on the pages: {text}");
}

/// Open is a converter from each value it takes (docs/research/20-convert.md): PdfCraft's from a
/// picture and from a PDF, VectorCraft's from a picture and from an SVG. So a PDF reaches text in
/// two steps, Open then Export text.
#[test]
fn open_converts_from_each_value_it_takes() {
    for (craft, from) in [("pdfcraft", vec!["IMAGE", "PDF"]), ("vectorcraft", vec!["IMAGE", "SVG"])] {
        let m = builtin(craft).unwrap().unwrap();
        if m.executable().is_none() {
            eprintln!("SKIP: {craft} is not on PATH; {}", m.hint);
            continue;
        }
        let opts = plumcraft_craft::Options { root: Some(craft_root(craft)), store: Some(Some(store_root())), include_excluded: false };
        let (engine, _, _) = plumcraft_craft::build(&m, &opts).unwrap();
        let cat = engine.schema();
        let open: Vec<(&str, &str)> = cat.converters().into_iter().filter(|c| c.kind.ends_with(".open")).map(|c| (c.from, c.input)).collect();
        let input = |t: &str| match t {
            "IMAGE" => "image",
            "PDF" => "pdf",
            _ => "svg",
        };
        let want: Vec<(&str, &str)> = from.iter().map(|t| (*t, input(t))).collect();
        assert_eq!(open, want, "{craft}");
        if craft == "pdfcraft" {
            let p = cat.convert_paths("PDF", "TXT");
            assert_eq!(p.first().map(|p| p.iter().map(|c| c.kind).collect::<Vec<_>>()), Some(vec!["pdf.open", "pdf.export_text"]), "a PDF to text");
        }
    }
}
