//! The PhotoCraft pack (`packs/photocraft.toml`): it parses, and, when `photocraft-cli` is on
//! PATH, the engine lists the curated nodes and runs New → Clouds → Hue/Saturation → Export PNG,
//! then reopens that PNG (Open's image socket) → Gaussian blur → Export JPEG, and its demo.

mod common;

use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{Manifest, builtin, demo};
use plumcraft_proto::data::Form;
use serde_json::json;

const NODES: &[&str] = &[
    "pc.open",
    "pc.new",
    "pc.inspect",
    "pc.flatten",
    "pc.image_size",
    "pc.crop",
    "pc.rotate",
    "pc.auto",
    "pc.brightness_contrast",
    "pc.levels",
    "pc.hue_saturation",
    "pc.exposure",
    "pc.black_white",
    "pc.invert",
    "pc.posterize",
    "pc.color_lookup",
    "pc.gaussian_blur",
    "pc.unsharp_mask",
    "pc.add_noise",
    "pc.oil_paint",
    "pc.clouds",
    "pc.detect_faces",
    "pc.export_png",
    "pc.export_jpg",
    "pc.export_pdf",
    "pc.export_psd",
];

fn pack() -> Manifest {
    builtin("photocraft").expect("photocraft is built in").expect("photocraft.toml parses")
}

#[test]
fn the_pack_parses() {
    let m = pack();
    assert_eq!(m.id, "photocraft");
    assert_eq!(m.doc.wire, "photocraft.doc");
    let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, NODES);
    assert!(m.gui.is_some() && m.run_node.is_some());
    let d: serde_json::Value = serde_json::from_str(demo("photocraft").expect("a demo")).unwrap();
    for n in d["nodes"].as_object().unwrap().values() {
        let kind = n["kind"].as_str().unwrap();
        assert!(NODES.contains(&kind), "the demo uses {kind}");
    }
}

/// The engine in process, or `None` when `photocraft-cli` isn't installed.
fn live(name: &str) -> Option<(Conn, plumcraft_craft::Report)> {
    let m = pack();
    if m.executable().is_none() {
        eprintln!("SKIP: photocraft-cli is not on PATH; {}", m.hint);
        return None;
    }
    let root = craft_root(name);
    let opts = plumcraft_craft::Options { root: Some(root), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(&m, &opts).unwrap();
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(60));
    Some((Box::new(plumcraft_host::InProc(h)), report))
}

#[test]
fn photocraft_lists_the_nodes_and_runs_a_chain() {
    let Some((h, report)) = live("photocraft") else { return };
    eprintln!("PhotoCraft {}: {} commands; nodes {:?}; dropped {:?}", report.server_version, report.commands.len(), report.curated, report.dropped);
    assert_eq!(report.curated, NODES, "dropped: {:?}", report.dropped);
    assert_eq!(report.hatch, ["pc.run"]);

    // New → Clouds → Hue/Saturation → Export PNG, and Inspect.
    let g = graph(
        json!({
            "n": {"kind": "pc.new", "values": {"width": 96, "height": 64}},
            "c": {"kind": "pc.clouds", "values": {"seed": 7}},
            "hs": {"kind": "pc.hue_saturation", "values": {"colorize": true, "hue": 200, "saturation": 60}},
            "png": {"kind": "pc.export_png", "values": {}},
            "i": {"kind": "pc.inspect", "values": {}}
        }),
        &[("n", "doc", "c", "doc"), ("c", "doc", "hs", "doc"), ("hs", "doc", "png", "doc"), ("hs", "doc", "i", "doc")],
    );
    let r = go(&h, "pc1", g, &["hs", "png", "i"], "off");
    assert!(r.ok, "{:?}", states(&r));
    let doc = blob(&r, "hs", "doc");
    assert_eq!(doc.meta.form, Form::Doc { app: "photocraft".into() });
    let png = blob(&r, "png", "image");
    assert_eq!(png.meta.form, Form::Image { width: 96, height: 64 });
    let info = json_out(&r, "i", "result");
    assert_eq!((info["width"].as_u64(), info["height"].as_u64()), (Some(96), Some(64)), "{info}");

    // (Not asserted: equal chains in one warm process save equal bytes. PhotoCraft numbers
    // documents and layers from a process-wide counter, so the second New's ids differ; see
    // claude-debug/craft-pack-photocraft.md.)

    // The escape hatch runs an allowlisted command and refuses others.
    let ok = go(
        &h,
        "pc2",
        chain(&[("n", "pc.new", json!({"width": 32, "height": 32})), ("r", "pc.run", json!({"command": "filter.pixelate.mosaic", "params": {"cellSize": 4}}))]),
        &["r"],
        "off",
    );
    assert!(ok.ok, "{:?}", states(&ok));
    let refused = go(
        &h,
        "pc3",
        chain(&[("n", "pc.new", json!({"width": 32, "height": 32})), ("r", "pc.run", json!({"command": "file.print", "params": {}}))]),
        &["r"],
        "off",
    );
    assert!(!refused.ok, "file.print passed the allowlist");

    // Open the PNG (the image socket) → Gaussian blur → rotate → Export JPEG.
    let g = graph(
        json!({
            "n": {"kind": "pc.new", "values": {"width": 80, "height": 40}},
            "c": {"kind": "pc.clouds", "values": {"seed": 1}},
            "png": {"kind": "pc.export_png", "values": {}},
            "o": {"kind": "pc.open", "values": {}},
            "b": {"kind": "pc.gaussian_blur", "values": {"radius": 3.0}},
            "rot": {"kind": "pc.rotate", "values": {"operation": "90cw"}},
            "jpg": {"kind": "pc.export_jpg", "values": {"quality": 80}},
            "back": {"kind": "pc.export_png", "values": {}}
        }),
        &[
            ("n", "doc", "c", "doc"),
            ("c", "doc", "png", "doc"),
            ("png", "image", "o", "image"),
            ("o", "doc", "b", "doc"),
            ("b", "doc", "rot", "doc"),
            ("rot", "doc", "jpg", "doc"),
            ("rot", "doc", "back", "doc"),
        ],
    );
    let r = go(&h, "pc4", g, &["o", "jpg", "back"], "off");
    assert!(r.ok, "{:?}", states(&r));
    let jpg = blob(&r, "jpg", "image");
    assert!(jpg.meta.size > 200, "{:?}", jpg.meta);
    assert_eq!(blob(&r, "back", "image").meta.form, Form::Image { width: 40, height: 80 }, "rotated 90°");
}

/// The generated families: every filter and adjustment a curated node doesn't run is a node
/// (`pc.<command id>`), and a few of them really change the pixels.
#[test]
fn photocraft_generates_its_filters_and_adjustments() {
    let Some((h, report)) = live("photocraft-families") else { return };
    let filters = report.generated.iter().filter(|id| id.starts_with("pc.filter.")).count();
    let adjustments = report.generated.iter().filter(|id| id.starts_with("pc.image.adjustments.")).count();
    eprintln!("PhotoCraft families: {filters} filters, {adjustments} adjustments; dropped {:?}", report.dropped);
    assert!(filters >= 110, "{filters} filter nodes");
    assert!(adjustments >= 12, "{adjustments} adjustment nodes");
    for id in
        ["pc.filter.blur.boxBlur", "pc.filter.stylize.findEdges", "pc.filter.distort.twirl", "pc.image.adjustments.threshold", "pc.image.adjustments.curves"]
    {
        assert!(report.generated.iter().any(|g| g == id), "no {id}");
    }
    // curated wins: Gaussian blur is pc.gaussian_blur only; denied commands are never nodes
    for id in ["pc.filter.blur.gaussianBlur", "pc.image.adjustments.levels", "pc.filter.lastFilter"] {
        assert!(!report.generated.iter().any(|g| g == id), "{id} generated");
    }
    assert!(report.dropped.iter().any(|(id, why)| id == "pc.filter.blur.gaussianBlur" && why.contains("pc.gaussian_blur")), "{:?}", report.dropped);

    // New → Clouds → PNG, and the same through generated filters and an adjustment: each changes
    // the pixels.
    let g = graph(
        json!({
            "n": {"kind": "pc.new", "values": {"width": 64, "height": 48}},
            "c": {"kind": "pc.clouds", "values": {"seed": 3}},
            "plain": {"kind": "pc.export_png", "values": {}},
            "box": {"kind": "pc.filter.blur.boxBlur", "values": {"radius": 6}},
            "boxed": {"kind": "pc.export_png", "values": {}},
            "edges": {"kind": "pc.filter.stylize.findEdges", "values": {}},
            "edged": {"kind": "pc.export_png", "values": {}},
            "th": {"kind": "pc.image.adjustments.threshold", "values": {"level": 128}},
            "thd": {"kind": "pc.export_png", "values": {}},
            "tw": {"kind": "pc.filter.distort.twirl", "values": {}},
            "twd": {"kind": "pc.export_png", "values": {}}
        }),
        &[
            ("n", "doc", "c", "doc"),
            ("c", "doc", "plain", "doc"),
            ("c", "doc", "box", "doc"),
            ("box", "doc", "boxed", "doc"),
            ("c", "doc", "edges", "doc"),
            ("edges", "doc", "edged", "doc"),
            ("c", "doc", "th", "doc"),
            ("th", "doc", "thd", "doc"),
            ("c", "doc", "tw", "doc"),
            ("tw", "doc", "twd", "doc"),
        ],
    );
    let r = go(&h, "pcfam", g, &["plain", "boxed", "edged", "thd", "twd"], "off");
    assert!(r.ok, "{:?}", states(&r));
    let plain = blob(&r, "plain", "image");
    for n in ["boxed", "edged", "thd", "twd"] {
        let out = blob(&r, n, "image");
        assert_eq!(out.meta.form, Form::Image { width: 64, height: 48 }, "{n}");
        assert_ne!(out.handle, plain.handle, "{n} left the pixels as they were");
    }
}

#[test]
fn photocraft_runs_its_demo() {
    let Some((h, _)) = live("photocraft-demo") else { return };
    let g: serde_json::Value = serde_json::from_str(demo("photocraft").unwrap()).unwrap();
    let targets: Vec<String> = g["nodes"].as_object().unwrap().keys().cloned().collect();
    let targets: Vec<&str> = targets.iter().map(String::as_str).collect();
    let r = go(&h, "pcdemo", g, &targets, "off");
    assert!(r.ok, "{:?}", states(&r));
    let png = blob(&r, "png", "image");
    assert!(matches!(png.meta.form, Form::Image { width, height } if width > 0 && height > 0), "{:?}", png.meta.form);
}
