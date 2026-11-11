//! The parameters the installed apps gained after the 2026-11-06 baseline, now widgets
//! (2026-11-10): each pack's demo, with the new widgets set, runs through the engine to its app.
//! An app that refused a parameter would fail its node. Skipped for an app not on PATH.

mod common;

use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{builtin, demo};
use serde_json::{Value, json};

fn live(id: &str) -> Option<Conn> {
    let m = builtin(id).expect("built in").expect("parses");
    if m.executable().is_none() {
        eprintln!("SKIP: {id}: {}", m.hint);
        return None;
    }
    let opts = plumcraft_craft::Options { root: Some(craft_root(&format!("{id}-new-params"))), store: Some(Some(store_root())), include_excluded: false };
    let (engine, _, _) = plumcraft_craft::build(&m, &opts).unwrap();
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(60));
    Some(Box::new(plumcraft_host::InProc(h)))
}

fn demo_graph(id: &str) -> Value {
    serde_json::from_str(demo(id).expect("a demo")).unwrap()
}

fn kind_node(g: &Value, kind: &str) -> String {
    g["nodes"].as_object().unwrap().iter().find(|(_, n)| n["kind"] == kind).map(|(k, _)| k.clone()).unwrap_or_else(|| panic!("no {kind}"))
}

fn set(g: &mut Value, node: &str, values: Value) {
    let v = g["nodes"][node].as_object_mut().unwrap().entry("values").or_insert_with(|| json!({}));
    v.as_object_mut().unwrap().extend(values.as_object().unwrap().clone());
}

fn wire(g: &mut Value, a: &str, b: &str) {
    g["wires"].as_array_mut().unwrap().push(json!({"from": {"node": a, "port": "doc"}, "to": {"node": b, "port": "doc"}}));
}

/// Put `via` on the doc wire `from` → `to`.
fn between(g: &mut Value, from: &str, to: &str, via: &str) {
    g["wires"].as_array_mut().unwrap().retain(|w| !(w["from"]["node"] == from && w["to"]["node"] == to));
    wire(g, from, via);
    wire(g, via, to);
}

fn runs(h: &Conn, id: &str, g: Value, targets: &[&str]) -> plumcraft_proto::run::RunReply {
    let r = go(h.as_ref(), id, g, targets, "off");
    assert!(r.ok, "{id}: {:?}\n{:?}", states(&r), r.nodes.iter().filter_map(|(k, n)| n.error.as_ref().map(|e| (k, e))).collect::<Vec<_>>());
    r
}

#[test]
fn figcraft_png_takes_a_colour_profile() {
    let Some(h) = live("figcraft") else { return };
    let mut g = demo_graph("figcraft");
    let png = kind_node(&g, "fig.export_png");
    set(&mut g, &png, json!({"colorProfile": "displayP3"}));
    let r = runs(&h, "fig-p3", g, &[&png]);
    assert!(blob(&r, &png, "image").meta.size > 0);
}

#[test]
fn filmcraft_takes_h264_settings_display_and_pitch() {
    let Some(h) = live("filmcraft") else { return };
    let mut g = demo_graph("filmcraft");
    set(&mut g, "mp4", json!({"h264Profile": "high", "h264IntraOnly": true}));
    g["nodes"]["settings"] = json!({"kind": "fc.settings", "values": {"videoDisplay": "frames"}});
    g["nodes"]["speed"] = json!({"kind": "fc.speed", "values": {"speed": 50.0, "maintainPitch": true}});
    between(&mut g, "look", "mp4", "settings");
    between(&mut g, "settings", "mp4", "speed");
    let r = runs(&h, "fc-new", g, &["mp4"]);
    assert!(blob(&r, "mp4", "video").meta.size > 0);
}

#[test]
fn pdfcraft_takes_gray_stop_at_limit_and_html() {
    let Some(h) = live("pdfcraft") else { return };
    let mut g = demo_graph("pdfcraft");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pdfcraft-hello.pdf");
    set(&mut g, "open", json!({"file": fixture.display().to_string()}));
    g["nodes"]["add"] = json!({"kind": "pdf.add_text", "values": {"text": "GRAY HALF", "gray": 0.5}});
    g["nodes"]["find"] = json!({"kind": "pdf.find", "values": {"query": "GRAY", "stop_at_limit": true}});
    g["nodes"]["html"] = json!({"kind": "pdf.text", "values": {"format": "html"}});
    wire(&mut g, "watermark", "add");
    wire(&mut g, "add", "find");
    wire(&mut g, "add", "html");
    let r = runs(&h, "pdf-new", g, &["find", "html"]);
    let html = json_out(&r, "html", "result").to_string();
    assert!(html.contains("<") && html.contains("GRAY HALF"), "{html}");
}

#[test]
fn vectorcraft_takes_by_color_and_a_name_encoding() {
    let Some(corgi) = corgi() else { return };
    let Some(h) = live("vectorcraft") else { return };
    let mut g = demo_graph("vectorcraft");
    let (tr, op) = (kind_node(&g, "vc.trace"), kind_node(&g, "vc.open"));
    set(&mut g, &tr, json!({"byColor": true}));
    set(&mut g, &op, json!({"file": corgi.display().to_string(), "legacyNameEncoding": "macintosh"}));
    runs(&h, "vc-new", g, &[&tr]);
}
