//! B15 (#50): any value as bytes, a slice of them, and bytes read as an array of a dtype in a
//! shape. A raw binary file goes Load Files → To Bytes → Slice → Bytes as Array; hostile
//! settings are errors, never panics.

mod common;

use common::{array, engine, error, ok, root, run};
use plumcraft_array::channels::bytes::{parse_shape, slice_indices};
use serde_json::json;

#[test]
fn slices_follow_python() {
    let r = |s, e, st| slice_indices(10, s, e, st);
    assert_eq!(r(0, 0, 1), (0..10).collect::<Vec<u64>>(), "stop 0 is the end");
    assert_eq!(r(2, 5, 1), [2, 3, 4]);
    assert_eq!(r(-3, 0, 1), [7, 8, 9]);
    assert_eq!(r(0, -8, 1), [0, 1]);
    assert_eq!(r(1, 0, 3), [1, 4, 7]);
    assert!(r(8, 2, 1).is_empty());
    assert_eq!(r(-100, 100, 1).len(), 10, "clamped to the array");
    assert!(slice_indices(0, 0, 0, 1).is_empty());
    assert_eq!(slice_indices(3, i64::MIN, i64::MAX, i64::MAX), [0]);
}

#[test]
fn shapes_parse_with_one_free_side() {
    assert_eq!(parse_shape("-1", 12).unwrap(), [12]);
    assert_eq!(parse_shape("3, 4", 12).unwrap(), [3, 4]);
    assert_eq!(parse_shape("2 -1", 12).unwrap(), [2, 6]);
    assert_eq!(parse_shape("-1,5", 12).unwrap(), [2, 5], "the free side rounds down; the rest is extra");
    for bad in ["", "a", "-1,-1", "-2", "1.5", "0,-1", "99999999999,99999999999,99999999999"] {
        assert!(parse_shape(bad, 12).is_err(), "{bad:?}");
    }
    assert!(parse_shape(&"1,".repeat(40), 1).is_err(), "too many sides");
}

/// A 4-byte header, then six big-endian int16s (2 × 3), then 3 bytes of junk: skip the header,
/// cut the file short, and read the numbers.
#[test]
fn a_binary_file_is_cut_and_read_as_a_shaped_array() {
    let dir = root("bytes-file");
    let path = dir.join("data.bin");
    let values: [i16; 6] = [1, -2, 300, -400, 5000, -32768];
    let mut file = b"HEAD".to_vec();
    for v in values {
        file.extend_from_slice(&v.to_be_bytes());
    }
    file.extend_from_slice(b"xyz");
    std::fs::write(&path, &file).unwrap();
    let e = engine(&dir);
    let nodes = json!({
        "f": {"kind": "file.load", "values": {"file": path.to_string_lossy()}},
        "b": {"kind": "array.to_bytes"},
        "s": {"kind": "array.slice", "values": {"start": 4, "stop": -3}},
        "a": {"kind": "array.from_bytes", "values": {"dtype": "int16", "byte_order": "big", "shape": "2, -1"}},
        // the whole file with the extra dropped: 19 bytes → 9 uint16s, little-endian
        "d": {"kind": "array.from_bytes", "values": {"dtype": "uint16", "shape": "-1", "extra": "drop"}},
    });
    let r = ok(run(
        &e,
        nodes,
        &[("f", "value", "b", "value"), ("b", "bytes", "s", "array"), ("s", "array", "a", "bytes"), ("b", "bytes", "d", "bytes")],
        &["b", "s", "a", "d"],
    ));
    let bytes = array(&e, &r, "b", "bytes");
    assert_eq!((bytes.dtype.as_str(), bytes.shape.as_slice(), &bytes.bytes[..]), ("|u1", &[file.len() as u64][..], &file[..]));
    assert_eq!(array(&e, &r, "s", "array").shape, [12]);
    let a = array(&e, &r, "a", "array");
    assert_eq!((a.dtype.as_str(), a.shape.as_slice()), ("<i2", &[2u64, 3][..]));
    let got: Vec<i16> = a.bytes.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect();
    assert_eq!(got, values);
    let d = array(&e, &r, "d", "array");
    assert_eq!((d.dtype.as_str(), d.shape.as_slice()), ("<u2", &[9u64][..]));
}

/// An array's bytes go round: To Bytes, then Bytes as Array in its own type and shape.
#[test]
fn an_array_goes_round_through_its_bytes() {
    let dir = root("bytes-round");
    let e = engine(&dir);
    let nodes = json!({
        "c": {"kind": "array.constant", "values": {"width": 4, "height": 3, "value": 0.25}},
        "b": {"kind": "array.to_bytes"},
        "a": {"kind": "array.from_bytes", "values": {"dtype": "float32", "shape": "3, 4"}},
        // a band of rows of the picture itself
        "s": {"kind": "array.slice", "values": {"start": 1, "stop": 3}},
    });
    let r = ok(run(&e, nodes, &[("c", "constant", "b", "value"), ("b", "bytes", "a", "bytes"), ("c", "constant", "s", "array")], &["a", "s", "c"]));
    let c = array(&e, &r, "c", "constant");
    let a = array(&e, &r, "a", "array");
    assert_eq!((a.dtype.as_str(), a.shape.as_slice(), &a.bytes[..]), (c.dtype.as_str(), c.shape.as_slice(), &c.bytes[..]));
    let s = array(&e, &r, "s", "array");
    assert_eq!(s.shape[0], 2);
    assert_eq!(&s.shape[1..], &c.shape[1..]);
}

#[test]
fn hostile_settings_are_errors() {
    let dir = root("bytes-hostile");
    let e = engine(&dir);
    let nodes = json!({
        "c": {"kind": "array.constant", "values": {"width": 3, "height": 1, "value": 1.0}},
        "b": {"kind": "array.to_bytes"},
        "big": {"kind": "array.from_bytes", "values": {"dtype": "float64", "shape": "100"}},
        "extra": {"kind": "array.from_bytes", "values": {"dtype": "float64", "shape": "1"}},
        "dt": {"kind": "array.from_bytes", "values": {"dtype": "complex128"}},
        "order": {"kind": "array.from_bytes", "values": {"byte_order": "middle"}},
        "shape": {"kind": "array.from_bytes", "values": {"shape": "1e9 1e9"}},
        "huge": {"kind": "array.from_bytes", "values": {"shape": "4294967296, 4294967296"}},
        "s": {"kind": "array.slice", "values": {"start": "x", "stop": 1e300, "step": -5}},
        "none": {"kind": "array.to_bytes"},
    });
    let wires = [("c", "constant", "b", "value")]
        .into_iter()
        .chain(["big", "extra", "dt", "order", "shape", "huge"].map(|t| ("b", "bytes", t, "bytes")))
        .chain([("b", "bytes", "s", "array")])
        .collect::<Vec<_>>();
    let r = run(&e, nodes, &wires, &["big", "extra", "dt", "order", "shape", "huge", "s", "none"]);
    assert!(error(&r, "big").contains("needs 800 bytes"), "{}", error(&r, "big"));
    assert!(error(&r, "extra").contains("extra"));
    assert!(error(&r, "dt").contains("complex128"));
    assert!(error(&r, "order").contains("middle"));
    assert!(error(&r, "shape").contains("isn't a side"));
    assert!(!error(&r, "huge").is_empty());
    assert!(error(&r, "none").contains("wire a value") || !error(&r, "none").is_empty());
    // junk slice settings fall back to their defaults, a step of at least 1
    assert!(matches!(r.nodes["s"].state, plumcraft_proto::event::NodeState::Done), "{:?}", r.nodes["s"].error);
}
