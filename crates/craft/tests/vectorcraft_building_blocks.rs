//! VectorCraft with the SDK's building blocks (Find, Select, Load and Write Files; Vary;
//! Zip and Unzip): a folder of pictures traced with every preset asked for and written out, each
//! file named by the picture and the preset that made it. Skipped without `vectorcraft-cli` or
//! the corgi.

mod common;

use common::*;
use plumcraft_proto::PortValue;
use plumcraft_proto::run::RunReply;
use serde_json::{Value, json};

/// A list output's items: plain values, or blob handles.
fn items(r: &RunReply, node: &str, port: &str) -> Vec<Value> {
    match r.nodes.get(node).and_then(|n| n.outputs.get(port)) {
        Some(PortValue::List { items }) => items
            .iter()
            .map(|i| match i {
                PortValue::Json { value } => value.clone(),
                PortValue::Blob { data } => json!(data.handle.as_ref().map(ToString::to_string)),
                other => json!(format!("{other:?}")),
            })
            .collect(),
        other => panic!("{node}.{port} is not a list: {other:?} ({:?})", r.nodes.get(node).map(|n| (&n.state, &n.error))),
    }
}

/// A folder with the corgi twice, under two names, and a stray file.
fn pictures(env: &Env) -> std::path::PathBuf {
    let d = env.root.join("pictures");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::copy(&env.corgi, d.join("corgi.jpg")).unwrap();
    std::fs::copy(&env.corgi, d.join("dog.JPG")).unwrap();
    std::fs::write(d.join("notes.txt"), "not a picture").unwrap();
    d
}

#[test]
fn a_folder_traced_with_two_presets_is_written_out_by_name() {
    let Some(env) = Env::new("blocks-find") else { return };
    let (h, _) = inproc(&env);
    let dir = pictures(&env);
    let out = env.root.join("traced");
    let _ = std::fs::remove_dir_all(&out);
    let g = graph(
        json!({
            "find":  {"kind": "file.find", "values": {"folder": dir, "pattern": "*.jpg"}},
            "open":  {"kind": "vc.open"},
            "vary":  {"kind": "vary.choices", "values": {"values": "6 Colors, 3 Colors"}},
            "trace": {"kind": "vc.trace"},
            "svg":   {"kind": "vc.export_svg"},
            "write": {"kind": "file.write", "values": {"folder": out, "basename": "trace"}}
        }),
        &[
            ("find", "files", "open", "file"),
            ("open", "doc", "trace", "doc"),
            ("vary", "values", "trace", "preset"),
            ("trace", "doc", "svg", "doc"),
            ("svg", "svg", "write", "values"),
        ],
    );
    let r = go(h.as_ref(), "find1", g.clone(), &["write"], "use");
    assert!(r.ok, "{:?}", states(&r));
    let written: Vec<Value> = items(&r, "write", "files");
    let names: Vec<String> = written.iter().filter_map(Value::as_str).map(|p| p.rsplit('/').next().unwrap_or_default().to_owned()).collect();
    assert_eq!(names, ["trace_corgi_preset-6-Colors.svg", "trace_corgi_preset-3-Colors.svg", "trace_dog_preset-6-Colors.svg", "trace_dog_preset-3-Colors.svg"]);
    for p in written.iter().filter_map(Value::as_str) {
        let svg = std::fs::read_to_string(p).unwrap();
        assert!(svg.contains("<svg"), "{p}");
    }
    // each SVG is titled by its picture; the same picture under two names traces the same
    // paths, and the presets differ
    let read = |n: &str| std::fs::read_to_string(out.join(n)).unwrap();
    let paths = |n: &str| read(n).split_once("</title>").map(|(_, rest)| rest.to_owned()).unwrap_or_default();
    assert!(read("trace_dog_preset-6-Colors.svg").contains("<title>dog.JPG</title>"));
    assert!(paths("trace_corgi_preset-6-Colors.svg") == paths("trace_dog_preset-6-Colors.svg"), "same picture, same paths");
    assert!(paths("trace_corgi_preset-6-Colors.svg") != paths("trace_corgi_preset-3-Colors.svg"), "presets differ");
    // again: every traced value cached, the files already there unchanged
    let again = go(h.as_ref(), "find2", g, &["write"], "use");
    assert!(again.ok, "{:?}", states(&again));
    assert_eq!(again.nodes["trace"].state, plumcraft_proto::event::NodeState::Cached);
}

#[test]
fn zipped_pictures_and_presets_and_loaded_images_open_in_vectorcraft() {
    let Some(env) = Env::new("blocks-zip") else { return };
    let (h, _) = inproc(&env);
    let dir = pictures(&env);
    let files = format!("{}\n{}", dir.join("corgi.jpg").display(), dir.join("dog.JPG").display());
    // Select Files zipped with two presets: each picture with its own preset (2 traces, not 4)
    let g = graph(
        json!({
            "pick":  {"kind": "file.select", "values": {"files": files}},
            "vary":  {"kind": "vary.string", "values": {"values": "6 Colors, 3 Colors"}},
            "zip":   {"kind": "seq.zip"},
            "unzip": {"kind": "seq.unzip"},
            "load":  {"kind": "file.load"},
            "open":  {"kind": "vc.open"},
            "trace": {"kind": "vc.trace"},
            "png":   {"kind": "vc.export_png"}
        }),
        &[
            ("pick", "files", "zip", "a"),
            ("vary", "values", "zip", "b"),
            ("zip", "tuples", "unzip", "tuples"),
            ("unzip", "a", "load", "file"),
            ("load", "value", "open", "image"),
            ("open", "doc", "trace", "doc"),
            ("unzip", "b", "trace", "preset"),
            ("trace", "doc", "png", "doc"),
        ],
    );
    let r = go(h.as_ref(), "zip1", g, &["png", "zip"], "off");
    assert!(r.ok, "{:?}", states(&r));
    assert_eq!(items(&r, "zip", "tuples").len(), 2);
    let pngs = items(&r, "png", "image");
    assert_eq!(pngs.len(), 2, "one trace per pair, not every combination");
    assert_ne!(pngs[0], pngs[1], "different presets");
}
