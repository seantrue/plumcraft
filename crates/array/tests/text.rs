//! Draw Text through the executor: Inter is always there; text lands where the anchors say, in
//! its colour, on one-channel and colour canvases; a font file by path; bad fonts and options are
//! errors. With a build made with CRAFT_FONTS_DIR, the craft-fonts faces draw Japanese.

mod common;

use common::{engine, error, img, ok, root, run};
use serde_json::json;

fn canvas(c: i64) -> serde_json::Value {
    json!({"kind": "array.constant", "values": {"value": 0.0, "channels": c, "width": 200, "height": 80}})
}

/// The bounding box of pixels above 0.5 in channel 0: (x0, y0, x1, y1).
fn ink(a: &plumcraft_array::img::Img) -> Option<(usize, usize, usize, usize)> {
    let p = a.plane(0);
    let mut b: Option<(usize, usize, usize, usize)> = None;
    for (k, v) in p.iter().enumerate() {
        if *v > 0.5 {
            let (x, y) = (k % a.w, k / a.w);
            b = Some(b.map_or((x, y, x, y), |(x0, y0, x1, y1)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y))));
        }
    }
    b
}

#[test]
fn text_lands_where_the_anchors_say() {
    let dir = root("text");
    let e = engine(&dir);
    let draw = |values: serde_json::Value, c: i64| {
        let r = ok(run(&e, json!({"c": canvas(c), "t": {"kind": "array.draw_text", "values": values}}), &[("c", "constant", "t", "canvas")], &["t"]));
        img(&e, &r, "t", "drawing")
    };
    let a = draw(json!({"text": "Hello", "x": 10, "y": 40, "size": 24}), 1);
    assert_eq!((a.c, a.h, a.w), (1, 80, 200), "one channel in, one out");
    let (x0, y0, x1, y1) = ink(&a).expect("ink");
    assert!((9..=14).contains(&x0) && x1 > 60 && x1 < 120, "{x0}..{x1}");
    assert!(y0 >= 24 && y1 <= 56 && y0 < 40 && y1 > 40, "vertically centred on 40: {y0}..{y1}");
    // centred both ways
    let c = draw(json!({"text": "Hi", "x": 0, "y": 0, "size": 30, "h_align": "C", "v_align": "C"}), 3);
    let (x0, y0, x1, y1) = ink(&c).expect("ink");
    assert!(((x0 + x1) as i64 - 200).abs() <= 8 && ((y0 + y1) as i64 - 80).abs() <= 12, "{x0}..{x1}, {y0}..{y1}");
    assert_eq!(c.space.as_deref(), Some("RGB"));
    // colour: red text leaves green and blue dark
    let red = draw(json!({"text": "Red", "x": 10, "y": 40, "size": 40, "color": "#ff0000"}), 3);
    let (r, g) = (red.plane(0), red.plane(1));
    assert!(r.iter().any(|v| *v > 0.9) && g.iter().all(|v| *v < 1e-6));
    // right-anchored, negative x
    let right = draw(json!({"text": "End", "x": -10, "y": 40, "size": 24, "h_align": "R"}), 1);
    let (_, _, x1, _) = ink(&right).expect("ink");
    assert!((185..=191).contains(&x1), "right edge 10 px in: {x1}");
    // rotated 90°: taller than wide
    let up = draw(json!({"text": "Tall text", "x": 0, "y": 0, "size": 16, "h_align": "C", "v_align": "C", "rotation": 90.0}), 1);
    let (x0, y0, x1, y1) = ink(&up).expect("ink");
    assert!(y1 - y0 > x1 - x0, "{x0}..{x1} × {y0}..{y1}");
    // empty text: the canvas unchanged
    assert!(ink(&draw(json!({"text": ""}), 1)).is_none());
}

#[test]
fn fonts_by_name_and_by_file_and_bad_ones() {
    let dir = root("text-fonts");
    let e = engine(&dir);
    let names = plumcraft_array::generate::text::font_names();
    assert_eq!(names.first().map(String::as_str), Some("Inter"));
    let inter = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/assets/fonts/Inter-Regular.ttf");
    let junk = dir.join("junk.ttf");
    std::fs::write(&junk, b"not a font").unwrap();
    let go = |values: serde_json::Value| {
        run(&e, json!({"c": canvas(1), "t": {"kind": "array.draw_text", "values": values}}), &[("c", "constant", "t", "canvas")], &["t"])
    };
    ok(go(json!({"text": "file", "font_file": inter})));
    assert!(error(&go(json!({"font_file": junk.display().to_string()})), "t").contains("font"));
    assert!(!error(&go(json!({"font_file": dir.join("missing.ttf").display().to_string()})), "t").is_empty());
    assert!(error(&go(json!({"font": "Comic Sans"})), "t").contains("Inter"));
    assert!(!error(&go(json!({"color": "red"})), "t").is_empty());
    // Japanese, when this build embedded craft-fonts (CRAFT_FONTS_DIR)
    if let Some(jp) = names.iter().find(|n| n.starts_with("BIZ UDPGothic")) {
        let r = ok(go(json!({"text": "日本語", "font": jp, "size": 40})));
        assert!(ink(&img(&e, &r, "t", "drawing")).is_some(), "{jp} draws Japanese");
        // Inter has no kana: nothing drawn, not an error
        let r = ok(go(json!({"text": "日本語", "font": "Inter", "size": 40})));
        assert!(ink(&img(&e, &r, "t", "drawing")).is_none());
    }
}
