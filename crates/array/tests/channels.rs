//! Files, channels and stacks through the executor: Load Image (a file and a wired picture,
//! alpha, align), Encode Image (and back), Split → Stack round trips keeping the space, Pick
//! Channel, Constant, Frame, Crop to Multiple; and hostile inputs as errors, never panics.

mod common;

use common::{array, blob, close, engine, error, img, ok, root, run};
use serde_json::json;

/// A 6×5 RGBA PNG with a gradient and a transparent corner.
fn write_png(path: &std::path::Path) {
    let mut px = Vec::new();
    for y in 0..5u8 {
        for x in 0..6u8 {
            px.extend_from_slice(&[x * 40, y * 50, 128, if x == 0 && y == 0 { 0 } else { 255 }]);
        }
    }
    image::save_buffer(path, &px, 6, 5, image::ColorType::Rgba8).unwrap();
}

#[test]
fn load_image_reads_a_file_aligned_with_its_alpha() {
    let dir = root("load");
    let png = dir.join("p.png");
    write_png(&png);
    let e = engine(&dir);
    let r = ok(run(&e, json!({"l": {"kind": "array.load_image", "values": {"path": png.display().to_string()}}}), &[], &["l"]));
    let rgb = img(&e, &r, "l", "rgb");
    assert_eq!((rgb.h, rgb.w, rgb.c, rgb.space.as_deref()), (4, 4, 3, Some("RGB")), "aligned to 4");
    assert_eq!(rgb.names, ["red", "green", "blue"]);
    assert_eq!(&rgb.data[..3], &[0.0, 0.0, 0.0], "the transparent pixel over black");
    let px = |x: usize, y: usize| rgb.data[(y * 4 + x) * 3..(y * 4 + x) * 3 + 3].to_vec();
    assert!(close(&px(1, 2), &[40.0 / 255.0, 100.0 / 255.0, 128.0 / 255.0], 1e-6));
    let alpha = img(&e, &r, "l", "alpha");
    assert_eq!((alpha.c, alpha.data[0], alpha.data[1]), (1, 0.0, 1.0));
    let rgba = img(&e, &r, "l", "rgba");
    assert_eq!((rgba.c, &rgba.data[..4]), (4, &[0.0, 0.0, 128.0 / 255.0, 0.0][..]), "rgba: the colours under the alpha");
    // white background, no align
    let r = ok(run(
        &e,
        json!({"l": {"kind": "array.load_image", "values": {"path": png.display().to_string(), "align": false, "background": "white"}}}),
        &[],
        &["l"],
    ));
    let rgb = img(&e, &r, "l", "rgb");
    assert_eq!((rgb.h, rgb.w, &rgb.data[..3]), (5, 6, &[1.0, 1.0, 1.0][..]));
    // missing, junk, and nothing chosen
    let bad = dir.join("junk.png");
    std::fs::write(&bad, b"not a picture").unwrap();
    for path in [dir.join("missing.png"), bad, std::path::PathBuf::new()] {
        let r = run(&e, json!({"l": {"kind": "array.load_image", "values": {"path": path.display().to_string()}}}), &[], &["l"]);
        assert!(!error(&r, "l").is_empty());
    }
}

#[test]
fn encode_image_round_trips_through_load_image() {
    let dir = root("encode");
    let png = dir.join("p.png");
    write_png(&png);
    let e = engine(&dir);
    let nodes = json!({
        "l": {"kind": "array.load_image", "values": {"path": png.display().to_string(), "align": false}},
        "e": {"kind": "array.encode_image", "values": {"conversion": "round"}},
        "j": {"kind": "array.encode_image", "values": {"format": "jpeg", "quality": 90}},
    });
    let r = ok(run(&e, nodes, &[("l", "rgb", "e", "image"), ("l", "rgba", "j", "image")], &["l", "e", "j"]));
    let (d, bytes) = blob(&e, &r, "e", "image");
    assert_eq!(d.meta.media, "image/png");
    assert!(bytes.starts_with(b"\x89PNG"));
    let back = plumcraft_array::channels::decode(&bytes).unwrap();
    let rgb = img(&e, &r, "l", "rgb");
    assert!(close(&back.data, &rgb.data, 0.5 / 255.0 + 1e-6), "rounded 8-bit, read back");
    let (d, bytes) = blob(&e, &r, "j", "image");
    assert_eq!(d.meta.media, "image/jpeg");
    assert!(bytes.starts_with(&[0xff, 0xd8]));
    // a Lab picture must be converted first
    let lab = json!({"c": {"kind": "array.constant", "values": {"channels": 3, "width": 4, "height": 4}}, "s": {"kind": "array.stack", "values": {"space": "Lab"}}, "e": {"kind": "array.encode_image"}});
    let r = run(&e, lab, &[("c", "constant", "s", "c1"), ("s", "stack", "e", "image")], &["e"]);
    assert!(error(&r, "e").contains("Convert Color Space"), "a Lab picture is converted to RGB first");
}

/// Decode Image: an encoded picture back into arrays, as Load Image reads the same file (rgb
/// over the background, alpha, rgba); unwired, an error.
#[test]
fn decode_image_reads_what_encode_image_wrote() {
    let dir = root("decode");
    let png = dir.join("p.png");
    write_png(&png);
    let e = engine(&dir);
    let nodes = json!({
        "l": {"kind": "array.load_image", "values": {"path": png.display().to_string(), "align": false}},
        "e": {"kind": "array.encode_image", "values": {"conversion": "round"}},
        "d": {"kind": "array.decode_image"},
    });
    let r = ok(run(&e, nodes, &[("l", "rgba", "e", "image"), ("e", "image", "d", "image")], &["l", "d"]));
    for port in ["rgb", "alpha", "rgba"] {
        let (a, b) = (img(&e, &r, "l", port), img(&e, &r, "d", port));
        assert_eq!((b.h, b.w, b.c), (a.h, a.w, a.c), "{port}");
        assert!(close(&b.data, &a.data, 0.5 / 255.0 + 1e-6), "{port}: 8-bit, read back");
    }
    assert_eq!(img(&e, &r, "d", "rgb").space.as_deref(), Some("RGB"));
    // Unwired: the executor's error, before the node runs.
    let r = run(&e, json!({"d": {"kind": "array.decode_image"}}), &[], &["d"]);
    assert!(error(&r, "d").contains("has no value"), "{}", error(&r, "d"));
}

/// Array to SVG and Rasterize SVG, inverses: a picture into an SVG and back at its own size is
/// the same picture, 8 bits a channel (a fully transparent pixel keeps no colour); the SVG is
/// the core SVG type, with its size; Rasterize SVG scales when asked, keeping the shape.
#[test]
fn array_to_svg_and_rasterize_svg_round_trip() {
    let dir = root("svg");
    let png = dir.join("p.png");
    write_png(&png);
    let e = engine(&dir);
    let nodes = json!({
        "l": {"kind": "array.load_image", "values": {"path": png.display().to_string(), "align": false}},
        "s": {"kind": "array.array_to_svg"},
        "r": {"kind": "array.rasterize_svg"},
        "big": {"kind": "array.rasterize_svg", "values": {"width": 12}},
    });
    let wires = [("l", "rgba", "s", "image"), ("s", "svg", "r", "svg"), ("s", "svg", "big", "svg")];
    let r = ok(run(&e, nodes, &wires, &["l", "s", "r", "big"]));
    let (a, b) = (img(&e, &r, "l", "rgba"), img(&e, &r, "r", "rgba"));
    assert_eq!((b.h, b.w, b.c), (a.h, a.w, 4));
    for (pa, pb) in a.data.as_chunks::<4>().0.iter().zip(b.data.as_chunks::<4>().0) {
        let n = if pa[3] > 0.0 { 4 } else { 1 };
        assert!(close(&pb[4 - n..], &pa[4 - n..], 0.5 / 255.0 + 1e-6), "{pa:?} -> {pb:?}");
    }
    let (data, bytes) = blob(&e, &r, "s", "svg");
    assert_eq!(data.meta.media, "image/svg+xml");
    assert!(matches!(data.meta.form, plumcraft_proto::data::Form::Vector { width, height } if width == 6.0 && height == 5.0), "{:?}", data.meta.form);
    assert!(String::from_utf8_lossy(&bytes).starts_with("<svg"));
    let big = img(&e, &r, "big", "rgb");
    assert_eq!((big.w, big.h), (12, 10), "twice as wide, the shape kept");
}

#[test]
fn split_and_stack_keep_the_space_and_names() {
    let dir = root("split");
    let png = dir.join("p.png");
    write_png(&png);
    let e = engine(&dir);
    let nodes = json!({
        "l": {"kind": "array.load_image", "values": {"path": png.display().to_string()}},
        "sp": {"kind": "array.split"},
        "st": {"kind": "array.stack"},
        "named": {"kind": "array.stack", "values": {"space": "HSI"}},
        "none": {"kind": "array.stack", "values": {"space": "none"}},
        "pick": {"kind": "array.pick_channel", "values": {"channel": 2}},
    });
    let wires = [
        ("l", "rgb", "sp", "array"),
        ("sp", "c1", "st", "c1"),
        ("sp", "c2", "st", "c2"),
        ("sp", "c3", "st", "c3"),
        ("sp", "c1", "named", "c1"),
        ("sp", "c2", "named", "c2"),
        ("sp", "c3", "named", "c3"),
        ("sp", "c3", "none", "c1"),
        ("l", "rgb", "none", "c2"),
        ("l", "rgb", "pick", "array"),
    ];
    let r = ok(run(&e, nodes, &wires, &["l", "sp", "st", "named", "none", "pick"]));
    let rgb = img(&e, &r, "l", "rgb");
    let c2 = img(&e, &r, "sp", "c2");
    assert_eq!((c2.c, c2.space.as_deref(), c2.names.as_slice()), (1, Some("RGB"), &["green".to_owned()][..]));
    let st = img(&e, &r, "st", "stack");
    assert_eq!((st.space.as_deref(), &st.data), (Some("RGB"), &rgb.data), "split then stacked: the same RGB");
    let named = array(&e, &r, "named", "stack");
    assert_eq!(
        (named.space.as_deref(), named.names.as_slice()),
        (Some("HSI"), &["hue".to_owned(), "saturation".into(), "intensity".into()][..]),
        "labelled, not converted"
    );
    let none = img(&e, &r, "none", "stack");
    assert_eq!((none.c, none.space.as_deref(), none.names[0].as_str(), none.names[1].as_str()), (4, None, "blue", "red"));
    assert_eq!(img(&e, &r, "pick", "channel").data, c2.data);
    // too few channels for a space; no channels; out-of-range channel; mismatched sizes
    let r =
        run(&e, json!({"c": {"kind": "array.constant"}, "s": {"kind": "array.stack", "values": {"space": "Lab"}}}), &[("c", "constant", "s", "c1")], &["s"]);
    assert!(error(&r, "s").contains("3 channels"));
    let r = run(&e, json!({"s": {"kind": "array.stack"}}), &[], &["s"]);
    assert!(error(&r, "s").contains("at least one"));
    let r = run(
        &e,
        json!({"c": {"kind": "array.constant"}, "p": {"kind": "array.pick_channel", "values": {"channel": 3}}}),
        &[("c", "constant", "p", "array")],
        &["p"],
    );
    assert!(error(&r, "p").contains("1 channel"));
    let r = run(
        &e,
        json!({"a": {"kind": "array.constant", "values": {"width": 4, "height": 4}}, "b": {"kind": "array.constant", "values": {"width": 5, "height": 4}}, "s": {"kind": "array.stack"}}),
        &[("a", "constant", "s", "c1"), ("b", "constant", "s", "c2")],
        &["s"],
    );
    assert!(error(&r, "s").contains("4×4"));
}

#[test]
fn constant_frame_and_crop() {
    let dir = root("utils");
    let e = engine(&dir);
    let nodes = json!({
        "c": {"kind": "array.constant", "values": {"value": 0.5, "width": 7, "height": 6, "channels": 2}},
        "f": {"kind": "array.frame", "values": {"margin": 2, "margin_value": -1.0, "frame_value": 9.0}},
        "k": {"kind": "array.crop_to_multiple", "values": {"multiple": 4}},
        "like": {"kind": "array.constant", "values": {"value": 2.0}},
    });
    let r = ok(run(&e, nodes, &[("c", "constant", "f", "array"), ("c", "constant", "k", "array"), ("k", "cropped", "like", "like")], &["c", "f", "k", "like"]));
    let c = img(&e, &r, "c", "constant");
    assert_eq!((c.h, c.w, c.c), (6, 7, 2));
    let f = img(&e, &r, "f", "framed");
    let p = f.plane(0);
    assert_eq!((p[0], p[7 + 1], p[2 * 7 + 2], p[2 * 7 + 3]), (9.0, -1.0, 0.5, 0.5), "frame, margin, inside");
    let k = img(&e, &r, "k", "cropped");
    assert_eq!((k.h, k.w), (4, 4));
    let like = img(&e, &r, "like", "constant");
    assert_eq!((like.h, like.w, like.data[0]), (4, 4, 2.0));
}
