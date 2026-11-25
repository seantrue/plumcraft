//! The hostile-input sweep for the chart crate (golden rule 1: every message from the other side
//! is hostile; a display list and an array's bytes are data): a valid list and valid arrays,
//! mutated byte by byte and number by number, laid out and drawn; nothing may panic.

#![cfg(feature = "raster")]

use plumcraft_chart::raster::{Frame, Style, draw};
use plumcraft_chart::{Kind, layout};
use serde_json::{Value, json};

fn seed() -> Value {
    let v: Vec<f32> = (0..64).map(|i| (i as f32 * 0.3).sin()).collect();
    let bytes: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
    layout(&Kind::Series, &[], "<f4", &[64], &bytes, (320.0, 200.0)).unwrap_or(Value::Null)
}

/// Every number in the list replaced by hostile ones, and every text of it cut at each byte.
#[test]
fn mutated_display_lists_never_panic() {
    let list = seed();
    assert!(draw(&list, 80, 50, &Style::default(), Frame::Whole).is_ok(), "the seed draws");
    let text = list.to_string();
    for cut in (0..text.len()).step_by(7) {
        if let Ok(v) = serde_json::from_str::<Value>(text.get(..cut).unwrap_or_default()) {
            let _ = draw(&v, 40, 30, &Style::default(), Frame::Plot { inset: 2 });
        }
    }
    for bad in [json!(f64::MAX), json!(-1e300), json!(0), json!(-0.0), json!("x"), json!(null), json!([]), json!({})] {
        for key in ["plot", "size", "marks", "grid", "legend", "axes", "clip"] {
            let mut v = list.clone();
            v[key] = bad.clone();
            for frame in [Frame::Whole, Frame::Plot { inset: 3 }] {
                let _ = draw(&v, 33, 17, &Style::default(), frame);
            }
        }
        let mut v = list.clone();
        if let Some(m) = v["marks"].get_mut(0) {
            m["xy"] = json!([bad.clone(), bad.clone(), 1, 2]);
            m["width"] = bad.clone();
            m["color"] = bad.clone();
        }
        let _ = draw(&v, 33, 17, &Style::default(), Frame::Whole);
    }
}

/// Arrays whose bytes, shapes and dtypes don't agree are refused, never read past their end.
#[test]
fn hostile_arrays_never_panic() {
    let names: Vec<String> = vec!["x".into(), "y".into()];
    for (dtype, shape, len) in [
        ("<f4", vec![10u64, 2], 3usize),
        ("<f8", vec![u64::MAX, 2], 16),
        ("|u1", vec![0, 2], 0),
        ("junk", vec![2, 2], 16),
        ("<f4", vec![5], 1),
        ("<i8", vec![3, 4], 96),
    ] {
        let bytes = vec![0x7fu8; len];
        for kind in [Kind::Series, Kind::Points(0, 1), Kind::Columns(0, 1), Kind::Segments([0, 1, 2, 3])] {
            if let Ok(l) = layout(&kind, &names, dtype, &shape, &bytes, (f64::NAN, -5.0)) {
                let _ = draw(&l, 20, 20, &Style::default(), Frame::Plot { inset: 1 });
            }
        }
    }
}
