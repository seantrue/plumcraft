//! The EffectCraft pack (`packs/effectcraft.toml`): it parses, and, when `effectcraft-cli` is on
//! PATH, the engine lists the curated nodes and runs New → Solid → Text → Effect → Set property
//! → Keyframe → Preset → Info and Export PNG, and Open (a fixture project) → Shape → Export PNG.

mod common;

use std::path::Path;
use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{Manifest, builtin, demo};
use plumcraft_proto::data::Form;
use serde_json::json;

/// The curated nodes the engine lists.
const LISTED: &[&str] = &[
    "ec.open",
    "ec.new",
    "ec.comp_settings",
    "ec.info",
    "ec.text",
    "ec.solid",
    "ec.shape",
    "ec.layer_transform",
    "ec.effect",
    "ec.preset",
    "ec.set_property",
    "ec.keyframe",
    "ec.expression",
    "ec.export_png",
];

fn pack() -> Manifest {
    builtin("effectcraft").expect("effectcraft is built in").expect("effectcraft.toml parses")
}

#[test]
fn the_pack_parses() {
    let m = pack();
    assert_eq!(m.id, "effectcraft");
    assert_eq!(m.doc.ext, "ecproj");
    let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, LISTED);
    assert!(m.exclude.is_empty());
    let d: serde_json::Value = serde_json::from_str(demo("effectcraft").expect("a demo")).unwrap();
    for n in d["nodes"].as_object().unwrap().values() {
        let kind = n["kind"].as_str().unwrap();
        assert!(LISTED.contains(&kind), "the demo uses {kind}");
    }
}

fn live(name: &str) -> Option<(Conn, plumcraft_craft::Report)> {
    let m = pack();
    if m.executable().is_none() {
        eprintln!("SKIP: effectcraft-cli is not on PATH; {}", m.hint);
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
fn effectcraft_lists_the_nodes_and_runs_a_chain() {
    let Some((h, report)) = live("effectcraft") else { return };
    eprintln!("EffectCraft {}: {} commands; nodes {:?}; dropped {:?}", report.server_version, report.commands.len(), report.curated, report.dropped);
    assert_eq!(report.curated, LISTED, "dropped: {:?}", report.dropped);
    assert_eq!(report.hatch, ["ec.run"]);

    let g = chain(&[
        ("n", "ec.new", json!({"width": 320, "height": 180, "duration": 2.0, "frameRate": 24.0})),
        ("s", "ec.solid", json!({"color": "#203060"})),
        ("t", "ec.text", json!({"text": "PlumCraft", "size": 40.0})),
        ("e", "ec.effect", json!({"effect": "ec.blur.gaussian"})),
        ("p", "ec.set_property", json!({"path": "effects/#1/blurriness", "value": 6})),
        ("k", "ec.keyframe", json!({"path": "transform/rotation", "time": 1.0, "value": 45})),
        ("f", "ec.preset", json!({"preset": "fadeIn"})),
        ("i", "ec.info", json!({})),
    ]);
    let mut g = g;
    g["nodes"]["png"] = json!({"kind": "ec.export_png", "values": {"time": 1.0}});
    g["wires"].as_array_mut().unwrap().push(json!({"from": {"node": "f", "port": "doc"}, "to": {"node": "png", "port": "doc"}}));
    let r = go(&h, "ec1", g, &["f", "i", "png"], "off");
    assert!(r.ok, "{:?}", states(&r));
    assert_eq!(blob(&r, "f", "doc").meta.form, Form::Doc { app: "effectcraft".into() });
    let info = json_out(&r, "i", "info");
    assert_eq!((info["width"].as_u64(), info["height"].as_u64()), (Some(320), Some(180)), "{info}");
    let layers = json_out(&r, "i", "layers");
    let names: Vec<&str> = layers.as_array().unwrap().iter().filter_map(|l| l["type"].as_str()).collect();
    assert_eq!(names.len(), 2, "{layers}");
    let png = blob(&r, "png", "image");
    assert_eq!(png.meta.form, Form::Image { width: 320, height: 180 });
    assert!(png.meta.size > 500, "{:?}", png.meta);

    // The same chain again: EffectCraft saves byte-stable projects.
    let a = go(&h, "ec2", chain(&[("n", "ec.new", json!({"width": 64, "height": 64})), ("t", "ec.text", json!({}))]), &["t"], "off");
    let b = go(&h, "ec3", chain(&[("n", "ec.new", json!({"width": 64, "height": 64})), ("t", "ec.text", json!({}))]), &["t"], "off");
    assert_eq!(blob(&a, "t", "doc").handle, blob(&b, "t", "doc").handle);

    // Open the fixture project → Shape → Info.
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/effectcraft-hello.ecproj");
    let g = chain(&[("o", "ec.open", json!({"file": fixture.display().to_string()})), ("s", "ec.shape", json!({"kind": "star"})), ("i", "ec.info", json!({}))]);
    let mut g = g;
    g["nodes"]["png"] = json!({"kind": "ec.export_png", "values": {}});
    g["wires"].as_array_mut().unwrap().push(json!({"from": {"node": "s", "port": "doc"}, "to": {"node": "png", "port": "doc"}}));
    let r = go(&h, "ec4", g, &["i", "png"], "off");
    assert!(r.ok, "{:?}", states(&r));
    let info = json_out(&r, "i", "info");
    assert_eq!(info["width"].as_u64(), Some(320), "{info}");
    assert_eq!(json_out(&r, "i", "layers").as_array().map(Vec::len), Some(2));
    assert_eq!(blob(&r, "png", "image").meta.form, Form::Image { width: 320, height: 180 });
}

#[test]
fn effectcraft_runs_its_demo() {
    let Some((h, _)) = live("effectcraft-demo") else { return };
    let g: serde_json::Value = serde_json::from_str(demo("effectcraft").unwrap()).unwrap();
    let targets: Vec<String> = g["nodes"].as_object().unwrap().keys().cloned().collect();
    let targets: Vec<&str> = targets.iter().map(String::as_str).collect();
    let r = go(&h, "ecdemo", g, &targets, "off");
    assert!(r.ok, "{:?}", states(&r));
    assert!(json_out(&r, "info", "layers").as_array().is_some_and(|l| l.len() == 3));
    let png = blob(&r, "png", "image");
    assert_eq!(png.meta.form, Form::Image { width: 1280, height: 720 });
    assert!(png.meta.size > 1000, "{:?}", png.meta);
}
