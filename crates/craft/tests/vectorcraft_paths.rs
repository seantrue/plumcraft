//! What VectorCraft keeps of a set of strokes between `vc.open` and `vc.export_svg`, simplified or
//! not: PlumImpress's From SVG is built on this (note 18, section 3). Skipped without
//! `vectorcraft-cli`.

mod common;

use common::*;
use plumcraft_proto::PortValue;
use plumcraft_proto::data::{BlobMeta, Form};
use plumcraft_proto::handle::Handle;
use serde_json::json;

const STROKES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300" viewBox="0 0 400 300">
<g fill="none" stroke="#000" stroke-width="1">
<path d="M10 10 L60 12 L110 11 L160 14 L210 13"/>
<path d="M210 200 L170 180 L120 190 L70 170 L20 180"/>
<path d="M300 40 L320 40 L320 60 L300 60 Z"/>
<path d="M50 100 L51 101 L52 103 L54 104 L55 106 L57 107 L58 109 L60 110 L100 150"/>
</g></svg>"##;

#[test]
fn what_vectorcraft_keeps_of_strokes() {
    let Some(env) = Env::new("paths") else { return };
    let (h, _) = inproc(&env);
    let svg = PortValue::Blob {
        data: plumcraft_store::inline_ref(
            Handle::raw(STROKES.as_bytes()),
            BlobMeta { media: "image/svg+xml".into(), size: STROKES.len() as u64, form: Form::Bytes, aliases: Vec::new() },
            STROKES.as_bytes(),
        ),
    };
    for (id, nodes, wires) in [
        ("plain", json!({"open": {"kind": "vc.open"}, "svg": {"kind": "vc.export_svg"}}), vec![("open", "doc", "svg", "doc")]),
        (
            "simple",
            json!({"open": {"kind": "vc.open"}, "smooth": {"kind": "vc.simplify", "values": {"level": 50.0}}, "svg": {"kind": "vc.export_svg"}}),
            vec![("open", "doc", "smooth", "doc"), ("smooth", "doc", "svg", "doc")],
        ),
    ] {
        let g = graph(nodes, &wires);
        run(&h, id, g, json!({"targets": ["svg"], "memo": "off", "inputs": {"open.svg": svg}}));
        let (_, r) = collect(&h, id, 0);
        assert!(r.ok, "{:?}", r.nodes);
        let out = h.resolve(&blob(&r, "svg", "svg")).unwrap().bytes;
        let out = String::from_utf8(out.into_vec()).unwrap();
        let at = |from: &str| out.find(from).unwrap_or_else(|| panic!("{id}: no {from:?} in\n{out}"));
        // Every stroke, in order, each from where it started.
        let (first, second, square, last) =
            (at(if id == "plain" { "10,10" } else { "M10 10" }), at("210,200"), at("<rect"), at(if id == "plain" { "50,100" } else { "M50 100" }));
        assert!(first < second && second < square && square < last, "{id}: order\n{out}");
        // Open stays open; a closed square comes back a `rect`, so a reader must take any shape.
        assert_eq!(["<path", "<polyline", "<rect"].iter().map(|t| out.matches(t).count()).sum::<usize>(), 4, "{id}\n{out}");
        assert!(!out.contains('Z'), "{id}: nothing closed\n{out}");
        assert_eq!(out.contains(" C"), id == "simple", "Simplify makes cubics\n{out}");
    }
}
