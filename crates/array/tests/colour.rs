//! Convert Color Space and Color Matrix through the executor: round trips through every space,
//! an array without a space read as RGB, XYZ in other working spaces, and colormath's matrices
//! agreeing with the SDK's sRGB conversion.

mod common;

use common::{close, engine, error, img, ok, root, run};
use plumcraft_array::colour::convert_planes;
use serde_json::json;

const SPACES: &[&str] = &["RGB", "HSI", "IHSL", "HSV", "HLS", "Lab", "Luv", "YCrCb", "XYZ", "xyY"];

/// In-gamut colours (away from black, where xyY is 0/0), as three planes.
fn colours() -> Vec<Vec<f32>> {
    let px: Vec<[f32; 3]> = vec![[0.9, 0.1, 0.2], [0.2, 0.7, 0.3], [0.1, 0.2, 0.8], [0.5, 0.5, 0.5], [0.95, 0.9, 0.2], [0.3, 0.6, 0.9]];
    (0..3).map(|k| px.iter().map(|p| p[k]).collect()).collect()
}

#[test]
fn every_space_round_trips_through_rgb() {
    let rgb = colours();
    for space in SPACES {
        for working in ["srgb_d65", "prophoto_d50", "adobe1998_d65"] {
            let there = convert_planes("RGB", space, working, true, &rgb).unwrap();
            let back = convert_planes(space, "RGB", working, true, &there).unwrap();
            for k in 0..3 {
                assert!(close(&back[k], &rgb[k], 3e-3), "{space} in {working}: {:?} vs {:?}", back[k], rgb[k]);
            }
        }
    }
    // colormath's sRGB D65 matrices agree with the SDK's XYZ (the same numbers, column by column)
    let ours = convert_planes("RGB", "XYZ", "srgb_d65", true, &rgb).unwrap();
    let sdk = plumcraft_engine::color::from_rgb("XYZ", &rgb).unwrap();
    for k in 0..3 {
        assert!(close(&ours[k], &sdk[k], 1e-6), "{:?} vs {:?}", ours[k], sdk[k]);
    }
    assert!(convert_planes("RGB", "CMYK", "srgb_d65", true, &rgb).is_err());
    assert!(convert_planes("RGB", "XYZ", "nowhere", true, &rgb).is_err());
}

#[test]
fn convert_through_the_executor_tags_the_result() {
    let dir = root("convert");
    let e = engine(&dir);
    let nodes = json!({
        "c": {"kind": "array.constant", "values": {"value": 0.25, "channels": 3, "width": 4, "height": 4}},
        "lab": {"kind": "array.convert_color_space", "values": {"target": "Lab"}},
        "back": {"kind": "array.convert_color_space", "values": {"target": "RGB"}},
        "m": {"kind": "array.color_matrix", "values": {"transform": "unity"}},
    });
    let r = ok(run(
        &e,
        nodes,
        &[("c", "constant", "lab", "array"), ("lab", "converted", "back", "array"), ("c", "constant", "m", "array")],
        &["c", "lab", "back", "m"],
    ));
    let lab = img(&e, &r, "lab", "converted");
    assert_eq!((lab.space.as_deref(), lab.names.as_slice()), (Some("Lab"), &["L".to_owned(), "a".into(), "b".into()][..]));
    assert!((lab.data[0] - 26.97).abs() < 0.05, "L of a 0.25 sRGB grey (linear 0.0508): {}", lab.data[0]);
    let back = img(&e, &r, "back", "converted");
    assert!(close(&back.data, &img(&e, &r, "c", "constant").data, 2e-3), "auto source: the array says Lab");
    let m = img(&e, &r, "m", "transformed");
    assert_eq!((m.space, m.data[0]), (None, 0.25), "unity, untagged");
    // one channel, a junk matrix
    let r = run(&e, json!({"c": {"kind": "array.constant"}, "x": {"kind": "array.convert_color_space"}}), &[("c", "constant", "x", "array")], &["x"]);
    assert!(error(&r, "x").contains("3 channels"));
    let r = run(
        &e,
        json!({"c": {"kind": "array.constant", "values": {"channels": 3}}, "m": {"kind": "array.color_matrix", "values": {"transform": "nope"}}}),
        &[("c", "constant", "m", "array")],
        &["m"],
    );
    assert!(!error(&r, "m").is_empty());
}
