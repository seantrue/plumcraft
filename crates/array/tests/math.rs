//! Math through the executor and against references: Arithmetic, Clamp, Stretch, Binarize,
//! Set Where, Normalize, Equalize, Histogram, Histogram Match, Apply Curve, Matrix Multiply
//! and Wrap Angle.
//!
//! `fixtures/math/reference.json` is written by `claude-debug/math_fixtures_gen.py` in
//! yapocis3's env with the numba backend: yapocis's numba kernels where the research software calls one,
//! numpy for the rest, impressionist's `core.curves` for Apply Curve. Every kernel is compared
//! with its reference below, with its tolerance and the reason for it; then every node runs
//! through the executor, and hostile inputs come back as errors, never panics.

mod common;

use common::{close, engine, error, img, ok, root, run};
use plumcraft_array::math::{curves, kernels};
use serde_json::{Value, json};

fn reference() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/math/reference.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// float32 values from their bit patterns (exact; serde_json's float parsing is not).
fn floats(v: &Value) -> Vec<f32> {
    v.as_array().unwrap().iter().map(|x| f32::from_bits(x.as_u64().unwrap() as u32)).collect()
}

/// The largest difference in float32 units in the last place of the expected value (0 when
/// identical; NaN matches NaN), and the largest absolute difference.
fn ulps(got: &[f32], want: &[f32]) -> (f64, f64) {
    assert_eq!(got.len(), want.len());
    let mut worst = (0.0f64, 0.0f64);
    for (g, w) in got.iter().zip(want) {
        if g.to_bits() == w.to_bits() || (g.is_nan() && w.is_nan()) || g == w {
            continue;
        }
        let ulp = f64::from((f32::from_bits(w.abs().to_bits() + 1) - w.abs()).max(f32::MIN_POSITIVE));
        let d = f64::from((g - w).abs());
        worst = (worst.0.max(d / ulp), worst.1.max(d));
    }
    worst
}

fn interleave(planes: &[Vec<f32>]) -> Vec<f32> {
    let n = planes[0].len();
    (0..n).flat_map(|i| planes.iter().map(move |p| p[i])).collect()
}

#[test]
fn kernels_match_their_references() {
    let r = reference();
    let inputs = r["inputs"].as_object().unwrap();
    let input = |k: &Value| floats(&inputs[k.as_str().unwrap()]);
    let mut report: std::collections::BTreeMap<String, (f64, f64, f64)> = Default::default();
    for c in r["cases"].as_array().unwrap() {
        let kernel = c["kernel"].as_str().unwrap();
        let ins: Vec<Vec<f32>> = c["inputs"].as_array().unwrap().iter().map(input).collect();
        let p = &c["params"];
        let pf = |k: &str| p[k].as_f64().unwrap();
        let ps = |k: &str| p[k].as_str().unwrap().to_owned();
        let want = floats(&c["expected"]);
        // (the result, the tolerance in float32 ulps, and why)
        let (got, tol): (Vec<f32>, f64) = match kernel {
            "arithmetic" => {
                let op = kernels::Arith::parse(&ps("operation")).unwrap();
                let b = ins.get(1).cloned().unwrap_or_else(|| vec![p["constant"].as_f64().unwrap() as f32; ins[0].len()]);
                let got = ins[0].iter().zip(&b).map(|(x, y)| op.apply(*x, *y)).collect();
                // float32 add/sub/mul are exact everywhere; divide is float64 then float32,
                // where numba's fastmath may divide by a reciprocal: 1 ulp
                (got, if op == kernels::Arith::Divide { 1.0 } else { 0.0 })
            }
            // comparisons and copies: exact
            "clamp" => (kernels::clamp_pixels(&ins[0], pf("low"), pf("high")), 0.0),
            "binarize" => (kernels::binarize(&ins[0], pf("threshold")), 0.0),
            "stretch" => {
                let (lo, hi) = match ps("mode").as_str() {
                    "auto" => kernels::nan_min_max(&ins[0]),
                    "percentile" => {
                        let q = kernels::nan_percentiles(&ins[0], &[pf("percent"), 100.0 - pf("percent")]);
                        (q[0], q[1])
                    }
                    _ => (pf("input_low"), pf("input_high")),
                };
                // the range is numpy's own (nanmin/nanpercentile): exact
                let bits = |k: &str| f64::from_bits(p[k].as_str().unwrap().parse().unwrap());
                assert_eq!((lo.to_bits(), hi.to_bits()), (bits("lo_bits").to_bits(), bits("hi_bits").to_bits()), "stretch range {p}");
                // clamped, then mapped, in float32 in the kernel's order, as numpy does it: exact
                (kernels::stretch(&ins[0], lo, hi, pf("low"), pf("high")), 0.0)
            }
            // float32 arithmetic in numpy's order: exact
            "normalize" if ins.len() == 1 => (kernels::normalize(&ins[0]), 0.0),
            "normalize" => (kernels::normalize(&interleave(&ins)), 0.0),
            "stretched" => (kernels::stretched(&ins[0]), 0.0),
            "equalize" => {
                let bins = p["bins"].as_u64().unwrap() as usize;
                let source = if ps("input") == "stretch" { kernels::stretched(&ins[0]) } else { ins[0].clone() };
                // histogram counts are exact; numpy's interp on arm64 macOS is compiled with
                // fused multiply-add and ours is not: its float64 may differ in the last bit,
                // which survives the cast to float32 at most as 1 ulp
                if ps("method") == "impressionist" {
                    (kernels::histeq_impressionist(&source, bins).unwrap(), 1.0)
                } else {
                    let unit: Vec<f32> = if ps("input") == "stretch" { source } else { source.iter().map(|v| v.clamp(0.0, 1.0)).collect() };
                    (kernels::histeq(&unit, bins), 1.0)
                }
            }
            "histogram" => {
                let bins = p["bins"].as_u64().unwrap() as usize;
                let (low, high) = (pf("low"), pf("high"));
                let counts = kernels::histogram(&ins[0], bins, low, high);
                let got = counts.iter().enumerate().flat_map(|(k, c)| [(low + (k as f64 + 0.5) * (high - low) / bins as f64) as f32, *c as f32]).collect();
                // integer counts and float64 centres: exact
                (got, 0.0)
            }
            "histogram_match" => {
                let bins = p["bins"].as_u64().unwrap() as usize;
                let unit: Vec<f32> = ins[1].iter().map(|v| v.clamp(0.0, 1.0)).collect();
                let hist = kernels::target_histogram(&unit, bins);
                let a: Vec<f32> = ins[0].iter().map(|v| v.clamp(0.0, 1.0)).collect();
                // np.histogram (npexact) exact; interp as Equalize: 1 ulp
                (kernels::histmatch(&a, &hist, bins), 1.0)
            }
            // numpy's float32 remainder: exact
            "wrap_angle" => (kernels::wrap_angle(&ins[0], pf("turn"), ps("range") == "signed"), 0.0),
            "set_where" => (ins[0].iter().zip(&ins[1]).map(|(x, m)| if *m == 1.0 { pf("value") as f32 } else { *x }).collect(), 0.0),
            "matmul" => {
                let (m, k, n) = (p["m"].as_u64().unwrap() as usize, 3, 2);
                let a = interleave(&ins[..3]);
                // float64 sums in k order against numpy's float64 matmul (BLAS may fuse or
                // reorder): 1 ulp after the cast
                (kernels::matmul(&a, &ins[3], m, k, n), 1.0)
            }
            "apply_curve" => {
                let set = curves::parse(&ps("curves")).unwrap();
                let planes = curves::curve_planes(&ins, &set, ps("input_range") == "own range");
                // numpy's elementwise float64 in the same order (no fusing): exact
                (if planes.len() == 1 { planes[0].clone() } else { interleave(&planes) }, 0.0)
            }
            other => panic!("unknown kernel {other}"),
        };
        let (u, abs) = ulps(&got, &want);
        assert!(u <= tol, "{kernel} {p}: {u} ulps (max abs {abs:e}), tolerance {tol}");
        let e = report.entry(kernel.to_owned()).or_insert((0.0, 0.0, tol));
        *e = (e.0.max(u), e.1.max(abs), tol);
    }
    for (k, (u, abs, tol)) in &report {
        println!("{k}: max {u} ulps, max abs {abs:e} (tolerance {tol} ulps)");
    }
    assert_eq!(report.len(), 13, "every kernel compared: {:?}", report.keys());
}

// ---------- through the executor ----------

/// An 8×6 RGB PNG with three different gradients, and the engine over a fresh store.
fn picture(name: &str) -> (plumcraft_engine::Engine, String) {
    let dir = root(&format!("math-{name}"));
    let png = dir.join("p.png");
    let mut px = Vec::new();
    for y in 0..6u32 {
        for x in 0..8u32 {
            px.extend_from_slice(&[(x * 36) as u8, (y * 50) as u8, ((x * 7 + y * 31) % 256) as u8]);
        }
    }
    image::save_buffer(&png, &px, 8, 6, image::ColorType::Rgb8).unwrap();
    (engine(&dir), png.display().to_string())
}

/// Nodes making the picture (`l.rgb`), its green channel (`g.channel`) and, from them, values
/// outside 0..1 (`wide.result` = rgb × 1.5 - 0.25), plus `nan.result`: a 4×4 of NaN, and
/// `inf.result`: a 4×4 of infinity.
fn sources(png: &str) -> (Value, Vec<(&'static str, &'static str, &'static str, &'static str)>) {
    let nodes = json!({
        "l": {"kind": "array.load_image", "values": {"path": png, "align": false}},
        "g": {"kind": "array.pick_channel", "values": {"channel": 2}},
        "x15": {"kind": "array.arithmetic", "values": {"operation": "multiply", "constant": 1.5}},
        "wide": {"kind": "array.arithmetic", "values": {"operation": "subtract", "constant": 0.25}},
        "big": {"kind": "array.constant", "values": {"value": 1e9, "width": 4, "height": 4}},
        "e18": {"kind": "array.arithmetic", "values": {"operation": "multiply"}},
        "e36": {"kind": "array.arithmetic", "values": {"operation": "multiply"}},
        "inf": {"kind": "array.arithmetic", "values": {"operation": "multiply"}},
        "nan": {"kind": "array.arithmetic", "values": {"operation": "subtract"}},
    });
    let wires = vec![
        ("l", "rgb", "g", "array"),
        ("l", "rgb", "x15", "a"),
        ("x15", "result", "wide", "a"),
        ("big", "constant", "e18", "a"),
        ("big", "constant", "e18", "b"),
        ("e18", "result", "e36", "a"),
        ("e18", "result", "e36", "b"),
        ("e36", "result", "inf", "a"),
        ("big", "constant", "inf", "b"),
        ("inf", "result", "nan", "a"),
        ("inf", "result", "nan", "b"),
    ];
    (nodes, wires)
}

/// Run `extra` nodes after the sources, with `extra_wires`, to `targets`.
fn run_with(
    e: &plumcraft_engine::Engine,
    png: &str,
    extra: Value,
    extra_wires: &[(&'static str, &'static str, &'static str, &'static str)],
    targets: &[&str],
) -> plumcraft_proto::run::RunReply {
    let (mut nodes, mut wires) = sources(png);
    for (k, v) in extra.as_object().unwrap() {
        nodes[k] = v.clone();
    }
    wires.extend_from_slice(extra_wires);
    let mut all: Vec<&str> = targets.to_vec();
    all.extend(["l", "g", "wide", "nan", "inf"]);
    run(e, nodes, &wires, &all)
}

fn per_plane(a: &plumcraft_array::img::Img, f: impl Fn(&[f32]) -> Vec<f32>) -> Vec<f32> {
    let planes: Vec<Vec<f32>> = a.planes().iter().map(|p| f(p)).collect();
    interleave(&planes)
}

#[test]
fn arithmetic_through_the_executor() {
    let (e, png) = picture("arith");
    let extra = json!({
        "mul": {"kind": "array.arithmetic", "values": {"operation": "multiply"}},
        "div": {"kind": "array.arithmetic", "values": {"operation": "divide"}},
        "from": {"kind": "array.arithmetic", "values": {"operation": "subtract from", "constant": 1.0}},
        "min": {"kind": "array.arithmetic", "values": {"operation": "minimum"}},
        "max": {"kind": "array.arithmetic", "values": {"operation": "maximum"}},
        "spread": {"kind": "array.arithmetic", "values": {"operation": "add"}},
    });
    let wires = [
        ("l", "rgb", "mul", "a"),
        ("g", "channel", "mul", "b"),
        ("l", "rgb", "div", "a"),
        ("g", "channel", "div", "b"),
        ("l", "rgb", "from", "a"),
        ("l", "rgb", "min", "a"),
        ("wide", "result", "min", "b"),
        ("l", "rgb", "max", "a"),
        ("wide", "result", "max", "b"),
        ("g", "channel", "spread", "a"),
        ("l", "rgb", "spread", "b"),
    ];
    let r = ok(run_with(&e, &png, extra, &wires, &["mul", "div", "from", "min", "max", "spread"]));
    let rgb = img(&e, &r, "l", "rgb");
    let g = img(&e, &r, "g", "channel").data;
    let wide = img(&e, &r, "wide", "result");
    assert_eq!((wide.space.as_deref(), wide.names.len()), (Some("RGB"), 3), "a's colour tags kept");
    assert_eq!(wide.data, rgb.data.iter().map(|v| v * 1.5 - 0.25).collect::<Vec<_>>());
    let mul = img(&e, &r, "mul", "result");
    let want: Vec<f32> = rgb.data.iter().enumerate().map(|(i, v)| v * g[i / 3]).collect();
    assert_eq!((mul.data, mul.space.as_deref()), (want, Some("RGB")), "one channel of b applies to every channel of a");
    let div = img(&e, &r, "div", "result").data;
    assert_eq!(div[0], 0.0);
    let i = 3 * 8 + 5; // (row 3, column 5): green 150/255
    assert_eq!(div[i * 3 + 1], (f64::from(rgb.data[i * 3 + 1]) / (f64::from(g[i]) + 1e-20)) as f32);
    assert_eq!(img(&e, &r, "from", "result").data, rgb.data.iter().map(|v| 1.0 - v).collect::<Vec<_>>(), "1 - x");
    assert_eq!(img(&e, &r, "min", "result").data, rgb.data.iter().zip(&wide.data).map(|(a, b)| a.min(*b)).collect::<Vec<_>>());
    assert_eq!(img(&e, &r, "max", "result").data, rgb.data.iter().zip(&wide.data).map(|(a, b)| a.max(*b)).collect::<Vec<_>>());
    let spread = img(&e, &r, "spread", "result");
    assert_eq!((spread.c, spread.space.as_deref()), (3, None), "a one-channel a spreads over b's channels, untagged");
    assert_eq!(spread.data, rgb.data.iter().enumerate().map(|(i, v)| g[i / 3] + v).collect::<Vec<_>>());
    // the hostile sources themselves
    assert!(img(&e, &r, "inf", "result").data.iter().all(|v| *v == f32::INFINITY));
    assert!(img(&e, &r, "nan", "result").data.iter().all(|v| v.is_nan()));
    // sizes and channel counts that don't fit; an unknown operation
    let bad = json!({
        "size": {"kind": "array.arithmetic", "values": {"operation": "add"}},
        "two": {"kind": "array.stack"},
        "count": {"kind": "array.arithmetic", "values": {"operation": "add"}},
        "op": {"kind": "array.arithmetic", "values": {"operation": "power"}},
    });
    let wires = [
        ("l", "rgb", "size", "a"),
        ("nan", "result", "size", "b"),
        ("g", "channel", "two", "c1"),
        ("g", "channel", "two", "c2"),
        ("l", "rgb", "count", "a"),
        ("two", "stack", "count", "b"),
        ("l", "rgb", "op", "a"),
    ];
    let r = run_with(&e, &png, bad, &wires, &["size", "count", "op"]);
    assert!(error(&r, "size").contains("differ in size"));
    assert!(error(&r, "count").contains("3 channels"));
    assert!(error(&r, "op").contains("operation"));
}

#[test]
fn clamp_stretch_binarize_wrap_and_set_where() {
    let (e, png) = picture("elementwise");
    let extra = json!({
        "clamp": {"kind": "array.clamp", "values": {"low": 0.1, "high": 0.9}},
        "auto": {"kind": "array.stretch"},
        "pct": {"kind": "array.stretch", "values": {"input_range": "percentile", "percent": 10.0, "low": -1.0, "high": 1.0}},
        "man": {"kind": "array.stretch", "values": {"input_range": "manual", "input_low": 0.2, "input_high": 0.8}},
        "bin": {"kind": "array.binarize", "values": {"threshold": 0.4}},
        "deg": {"kind": "array.arithmetic", "values": {"operation": "multiply", "constant": 1000.0}},
        "wrap": {"kind": "array.wrap_angle"},
        "uwrap": {"kind": "array.wrap_angle", "values": {"turn": 1.0, "range": "unsigned"}},
        "mask": {"kind": "array.binarize", "values": {"threshold": 0.5}},
        "set": {"kind": "array.set_where", "values": {"value": -3.0}},
    });
    let wires = [
        ("wide", "result", "clamp", "array"),
        ("wide", "result", "auto", "array"),
        ("wide", "result", "pct", "array"),
        ("wide", "result", "man", "array"),
        ("wide", "result", "bin", "array"),
        ("wide", "result", "deg", "a"),
        ("deg", "result", "wrap", "array"),
        ("wide", "result", "uwrap", "array"),
        ("g", "channel", "mask", "array"),
        ("l", "rgb", "set", "x"),
        ("mask", "binary", "set", "mask"),
    ];
    let r = ok(run_with(&e, &png, extra, &wires, &["clamp", "auto", "pct", "man", "bin", "deg", "wrap", "uwrap", "mask", "set"]));
    let wide = img(&e, &r, "wide", "result");
    let clamp = img(&e, &r, "clamp", "clamped");
    assert_eq!((clamp.space.as_deref(), &clamp.data), (Some("RGB"), &kernels::clamp_pixels(&wide.data, 0.1, 0.9)));
    let auto = img(&e, &r, "auto", "stretched");
    assert_eq!(auto.space.as_deref(), Some("RGB"), "0-1 space kept");
    assert_eq!(
        auto.data,
        per_plane(&wide, |p| {
            let (lo, hi) = kernels::nan_min_max(p);
            kernels::stretch(p, lo, hi, 0.0, 1.0)
        })
    );
    let blue: Vec<f32> = auto.plane(2);
    assert!(blue.contains(&0.0) && blue.iter().any(|v| (*v - 1.0).abs() < 1e-6), "each channel over its own range");
    let pct = img(&e, &r, "pct", "stretched");
    assert_eq!(
        pct.data,
        per_plane(&wide, |p| {
            let q = kernels::nan_percentiles(p, &[10.0, 90.0]);
            kernels::stretch(p, q[0], q[1], -1.0, 1.0)
        })
    );
    assert_eq!(img(&e, &r, "man", "stretched").data, kernels::stretch(&wide.data, 0.2, 0.8, 0.0, 1.0));
    let bin = img(&e, &r, "bin", "binary");
    assert_eq!((bin.space, bin.names.len(), bin.data), (None, 0, kernels::binarize(&wide.data, 0.4)), "binary values lose the colour tags");
    let deg = img(&e, &r, "deg", "result");
    let wrap = img(&e, &r, "wrap", "wrapped");
    assert_eq!(wrap.data, kernels::wrap_angle(&deg.data, 360.0, true));
    assert!(wrap.data.iter().all(|v| (-180.0..=180.0).contains(v)));
    let uwrap = img(&e, &r, "uwrap", "wrapped");
    assert!(uwrap.data.iter().all(|v| (0.0..1.0).contains(v)));
    let rgb = img(&e, &r, "l", "rgb");
    let mask = img(&e, &r, "mask", "binary").data;
    let set = img(&e, &r, "set", "result");
    assert_eq!(
        set.data,
        rgb.data.iter().enumerate().map(|(i, v)| if mask[i / 3] == 1.0 { -3.0 } else { *v }).collect::<Vec<_>>(),
        "a one-channel mask sets every channel"
    );
    assert_eq!(set.space.as_deref(), Some("RGB"));
}

#[test]
fn normalize_equalize_and_histograms() {
    let (e, png) = picture("hist");
    let extra = json!({
        "each": {"kind": "array.normalize"},
        "whole": {"kind": "array.normalize", "values": {"over": "whole array"}},
        "eq": {"kind": "array.equalize", "values": {"bins": 256}},
        "eqs": {"kind": "array.equalize", "values": {"bins": 64, "input": "stretch"}},
        "eqi": {"kind": "array.equalize", "values": {"bins": 256, "input": "as is", "method": "impressionist"}},
        "hist": {"kind": "array.histogram", "values": {"bins": 10}},
        "match": {"kind": "array.histogram_match", "values": {"bins": 32}},
    });
    let wires = [
        ("wide", "result", "each", "array"),
        ("wide", "result", "whole", "array"),
        ("wide", "result", "eq", "array"),
        ("wide", "result", "eqs", "array"),
        ("wide", "result", "eqi", "array"),
        ("g", "channel", "hist", "array"),
        ("l", "rgb", "match", "array"),
        ("g", "channel", "match", "target"),
    ];
    let r = ok(run_with(&e, &png, extra, &wires, &["each", "whole", "eq", "eqs", "eqi", "hist", "match"]));
    let wide = img(&e, &r, "wide", "result");
    let each = img(&e, &r, "each", "normalized");
    assert_eq!((each.space.as_deref(), &each.data), (Some("RGB"), &per_plane(&wide, kernels::normalize)));
    let whole = img(&e, &r, "whole", "normalized");
    assert_eq!(whole.data, kernels::normalize(&wide.data));
    assert_eq!(whole.data.iter().copied().fold(f32::MIN, f32::max), 1.0);
    let clip = |p: &[f32]| p.iter().map(|v| v.clamp(0.0, 1.0)).collect::<Vec<f32>>();
    assert_eq!(img(&e, &r, "eq", "equalized").data, per_plane(&wide, |p| kernels::histeq(&clip(p), 256)));
    assert_eq!(img(&e, &r, "eqs", "equalized").data, per_plane(&wide, |p| kernels::histeq(&kernels::stretched(p), 64)));
    assert_eq!(img(&e, &r, "eqi", "equalized").data, per_plane(&wide, |p| kernels::histeq_impressionist(p, 256).unwrap()));
    let hist = img(&e, &r, "hist", "histogram");
    assert!(hist.table);
    assert_eq!((hist.h, hist.c, hist.names.as_slice()), (10, 2, &["centre".to_owned(), "count".into()][..]));
    assert_eq!(hist.plane(1).iter().sum::<f32>(), 48.0, "every pixel counted");
    assert!(close(&hist.plane(0), &(0..10).map(|k| (k as f32 + 0.5) / 10.0).collect::<Vec<_>>(), 1e-7));
    let rgb = img(&e, &r, "l", "rgb");
    let g = img(&e, &r, "g", "channel").data;
    let target = kernels::target_histogram(&g, 32);
    assert_eq!(img(&e, &r, "match", "matched").data, per_plane(&rgb, |p| kernels::histmatch(p, &target, 32)));
    // wrong channels, a table, an empty range, too many bins
    let bad = json!({
        "stack": {"kind": "array.histogram"},
        "range": {"kind": "array.histogram", "values": {"low": 1.0, "high": 1.0}},
        "bins": {"kind": "array.histogram", "values": {"bins": 100_000_000}},
        "two": {"kind": "array.stack"},
        "count": {"kind": "array.histogram_match"},
        "table": {"kind": "array.equalize"},
        "hist": {"kind": "array.histogram", "values": {"bins": 10}},
        "few": {"kind": "array.equalize", "values": {"bins": 1}},
    });
    let wires = [
        ("l", "rgb", "stack", "array"),
        ("g", "channel", "range", "array"),
        ("g", "channel", "bins", "array"),
        ("g", "channel", "two", "c1"),
        ("g", "channel", "two", "c2"),
        ("l", "rgb", "count", "array"),
        ("two", "stack", "count", "target"),
        ("hist", "histogram", "table", "array"),
        ("g", "channel", "hist", "array"),
        ("g", "channel", "few", "array"),
    ];
    let r = run_with(&e, &png, bad, &wires, &["stack", "range", "bins", "count", "table", "few"]);
    assert!(error(&r, "stack").contains("one channel"));
    assert!(error(&r, "range").contains("above low"));
    assert!(error(&r, "bins").contains("bins"));
    assert!(error(&r, "count").contains("2 channels"));
    assert!(error(&r, "table").contains("table"));
    assert!(error(&r, "few").contains("bins"));
}

#[test]
fn apply_curve_and_matrix_multiply() {
    let (e, png) = picture("curve");
    let s_curve = r#"{"master": {"points": [{"x": 0, "y": 0}, {"x": 0.25, "y": 0.15}, {"x": 0.75, "y": 0.9}, {"x": 1, "y": 1}], "enabled": true}, "blue": {"points": [[0, 1], [1, 0]]}}"#;
    let extra = json!({
        "id": {"kind": "array.apply_curve"},
        "s": {"kind": "array.apply_curve", "values": {"curves": s_curve}},
        "own": {"kind": "array.apply_curve", "values": {"curves": s_curve, "input_range": "own range"}},
        "m33": {"kind": "array.constant", "values": {"value": 0.5, "width": 2, "height": 3}},
        "px": {"kind": "array.matrix_multiply"},
        "hist": {"kind": "array.histogram", "values": {"bins": 4}},
        "m23": {"kind": "array.constant", "values": {"value": 2.0, "width": 3, "height": 2}},
        "tab": {"kind": "array.matrix_multiply"},
    });
    let wires = [
        ("wide", "result", "id", "array"),
        ("l", "rgb", "s", "array"),
        ("wide", "result", "own", "array"),
        ("l", "rgb", "px", "a"),
        ("m33", "constant", "px", "b"),
        ("g", "channel", "hist", "array"),
        ("hist", "histogram", "tab", "a"),
        ("m23", "constant", "tab", "b"),
    ];
    let r = ok(run_with(&e, &png, extra, &wires, &["id", "s", "own", "px", "hist", "tab"]));
    let wide = img(&e, &r, "wide", "result");
    let rgb = img(&e, &r, "l", "rgb");
    let id = img(&e, &r, "id", "curved");
    assert_eq!(
        (id.space.as_deref(), &id.data),
        (Some("RGB"), &wide.data.iter().map(|v| v.clamp(0.0, 1.0)).collect::<Vec<_>>()),
        "the default curves: clipped, unchanged"
    );
    let set = curves::parse(s_curve).unwrap();
    let s = img(&e, &r, "s", "curved");
    assert_eq!(s.data, interleave(&curves::curve_planes(&rgb.planes(), &set, false)));
    let master_only = curves::parse(&s_curve.replace(r#", "blue": {"points": [[0, 1], [1, 0]]}"#, "")).unwrap();
    let mastered = curves::curve_planes(&[rgb.plane(2)], &master_only, false).remove(0);
    assert!(close(&s.plane(2), &mastered.iter().map(|v| 1.0 - v).collect::<Vec<_>>(), 1e-6), "blue: master, then blue's straight line down");
    let own = img(&e, &r, "own", "curved");
    assert_eq!(own.data, interleave(&curves::curve_planes(&wide.planes(), &set, true)));
    let lo = wide.data.iter().copied().fold(f32::MAX, f32::min);
    assert!(own.data.iter().all(|v| *v >= lo - 1e-6), "own range: back to each channel's own min..max");
    let px = img(&e, &r, "px", "product");
    assert_eq!((px.h, px.w, px.c, px.table, px.space.as_deref()), (6, 8, 2, false, None));
    assert_eq!(px.data, kernels::matmul(&rgb.data, &[0.5; 6], 48, 3, 2));
    let hist = img(&e, &r, "hist", "histogram");
    let tab = img(&e, &r, "tab", "product");
    assert_eq!((tab.h, tab.c, tab.table), (4, 3, true));
    assert_eq!(tab.data, kernels::matmul(&hist.data, &[2.0; 6], 4, 2, 3));
    // bad curves, mismatched matrices, a stack as B, a table to curve
    let bad = json!({
        "json": {"kind": "array.apply_curve", "values": {"curves": "{not json"}},
        "shape": {"kind": "array.apply_curve", "values": {"curves": "[1, 2]"}},
        "pts": {"kind": "array.apply_curve", "values": {"curves": r#"{"master": {"points": [{"x": "a", "y": 0}]}}"#}},
        "k": {"kind": "array.matrix_multiply"},
        "stackb": {"kind": "array.matrix_multiply"},
        "tcurve": {"kind": "array.apply_curve"},
    });
    let wires = [
        ("l", "rgb", "json", "array"),
        ("l", "rgb", "shape", "array"),
        ("l", "rgb", "pts", "array"),
        ("l", "rgb", "k", "a"),
        ("m23", "constant", "k", "b"),
        ("l", "rgb", "stackb", "a"),
        ("l", "rgb", "stackb", "b"),
        ("hist", "histogram", "tcurve", "array"),
        ("g", "channel", "hist", "array"),
    ];
    let mut nodes_extra = bad;
    nodes_extra["hist"] = json!({"kind": "array.histogram", "values": {"bins": 4}});
    nodes_extra["m23"] = json!({"kind": "array.constant", "values": {"value": 2.0, "width": 3, "height": 2}});
    let r = run_with(&e, &png, nodes_extra, &wires, &["json", "shape", "pts", "k", "stackb", "tcurve"]);
    assert!(error(&r, "json").contains("not JSON"));
    assert!(error(&r, "shape").contains("object"));
    assert!(error(&r, "pts").contains("not a number"));
    assert!(error(&r, "k").contains("3 columns"));
    assert!(error(&r, "stackb").contains("table K × N"));
    assert!(error(&r, "tcurve").contains("table"));
}

/// NaN and infinity into every node: a result or an error, never a panic; and parameters out
/// of range are errors.
#[test]
fn hostile_values_and_parameters() {
    let (e, png) = picture("hostile");
    let kinds: &[(&str, &str, Value)] = &[
        ("array.arithmetic", "a", json!({"operation": "divide", "constant": 0.0})),
        ("array.clamp", "array", json!({})),
        ("array.stretch", "array", json!({})),
        ("array.stretch", "array", json!({"input_range": "percentile"})),
        ("array.binarize", "array", json!({})),
        ("array.normalize", "array", json!({"over": "whole array"})),
        ("array.equalize", "array", json!({"bins": 16})),
        ("array.equalize", "array", json!({"bins": 16, "input": "stretch"})),
        ("array.equalize", "array", json!({"bins": 16, "method": "impressionist"})),
        ("array.histogram", "array", json!({})),
        ("array.apply_curve", "array", json!({"curves": r#"{"points": [[0, 0.2], [0.5, 0.9], [1, 1]]}"#})),
        ("array.apply_curve", "array", json!({"input_range": "own range"})),
        ("array.wrap_angle", "array", json!({})),
        ("array.matrix_multiply", "a", json!({})),
        ("array.histogram_match", "array", json!({})),
        ("array.set_where", "x", json!({})),
    ];
    for source in ["nan", "inf"] {
        let mut nodes = serde_json::Map::new();
        let mut wires = Vec::new();
        let ids: Vec<String> = (0..kinds.len()).map(|k| format!("n{k}")).collect();
        for (k, (kind, port, values)) in kinds.iter().enumerate() {
            nodes.insert(ids[k].clone(), json!({"kind": kind, "values": values}));
            wires.push((source, "result", ids[k].as_str(), *port));
            match *kind {
                "array.matrix_multiply" => wires.push((source, "result", ids[k].as_str(), "b")),
                "array.histogram_match" => wires.push((source, "result", ids[k].as_str(), "target")),
                "array.set_where" => wires.push((source, "result", ids[k].as_str(), "mask")),
                _ => {}
            }
        }
        let targets: Vec<&str> = ids.iter().map(String::as_str).collect();
        let (mut all, source_wires) = sources(&png);
        for (k, v) in nodes {
            all[k] = v;
        }
        let mut all_wires: Vec<(&str, &str, &str, &str)> = source_wires;
        all_wires.extend(wires);
        let r = run(&e, all, &all_wires, &targets);
        for (k, id) in ids.iter().enumerate() {
            let n = &r.nodes[id];
            let state = format!("{:?}", n.state);
            assert!(state == "Done" || state == "Error", "{source} into {}: {state} {:?}", kinds[k].0, n.error);
        }
        // what is defined is as numpy has it
        let clamp = img(&e, &r, "n1", "clamped").data;
        let bin = img(&e, &r, "n4", "binary").data;
        if source == "nan" {
            assert!(clamp.iter().all(|v| v.is_nan()) && bin.iter().all(|v| *v == 0.0));
            assert!(img(&e, &r, "n5", "normalized").data.iter().all(|v| *v == 0.0), "nan_to_num");
            assert!(error(&r, "n8").contains("finite"), "numpy refuses a NaN range");
        } else {
            assert!(clamp.iter().all(|v| *v == 1.0) && bin.iter().all(|v| *v == 1.0));
            let hist = img(&e, &r, "n9", "histogram");
            assert_eq!(hist.plane(1).last(), Some(&16.0), "infinity counts in the last bin");
        }
    }
    // parameters out of range
    let bad = json!({
        "pct": {"kind": "array.stretch", "values": {"input_range": "percentile", "percent": 60.0}},
        "mode": {"kind": "array.stretch", "values": {"input_range": "sideways"}},
        "turn": {"kind": "array.wrap_angle", "values": {"turn": 0.0}},
        "bins": {"kind": "array.histogram_match", "values": {"bins": 0}},
        "over": {"kind": "array.normalize", "values": {"over": "nothing"}},
        "eqbins": {"kind": "array.equalize", "values": {"bins": 1_000_000_000}},
        "clamp": {"kind": "array.clamp", "values": {"low": -1e12}},
    });
    let wires = [
        ("g", "channel", "pct", "array"),
        ("g", "channel", "mode", "array"),
        ("g", "channel", "turn", "array"),
        ("g", "channel", "bins", "array"),
        ("g", "channel", "bins", "target"),
        ("g", "channel", "over", "array"),
        ("g", "channel", "eqbins", "array"),
        ("g", "channel", "clamp", "array"),
    ];
    let r = run_with(&e, &png, bad, &wires, &["pct", "mode", "turn", "bins", "over", "eqbins", "clamp"]);
    for (id, word) in [("pct", "percent"), ("mode", "input_range"), ("turn", "turn"), ("bins", "bins"), ("over", "over"), ("eqbins", "bins"), ("clamp", "low")]
    {
        assert!(error(&r, id).contains(word), "{id}");
    }
}

#[test]
fn curve_json_shapes() {
    let set = curves::parse("").unwrap();
    assert!(set.iter().all(|c| c.enabled && c.points == [(0.0, 0.0), (1.0, 1.0)]));
    assert_eq!(curves::parse(&curves::default_json()).unwrap(), set);
    let comfy = curves::parse(r#"{"points": [[0.5, 0.2], [0, 0.1]]}"#).unwrap();
    assert_eq!(comfy[0].points, [(0.0, 0.1), (0.5, 0.2)], "ComfyUI's points are the master curve, sorted");
    let one = curves::parse(r#"{"master": {"points": [{"x": 0.3, "y": 0.7}]}}"#).unwrap();
    assert_eq!(curves::curve_planes(&[vec![0.1, 0.9]], &one, false), vec![vec![0.7, 0.7]], "one point: a constant");
    let off = curves::parse(r#"{"master": {"points": [{"x": 0, "y": 1}, {"x": 1, "y": 0}], "enabled": false}}"#).unwrap();
    assert_eq!(curves::curve_planes(&[vec![0.25]], &off, false), vec![vec![0.25]], "disabled");
    let many = format!(r#"{{"master": {{"points": [{}]}}}}"#, vec!["[0.5, 0.5]"; curves::MAX_POINTS + 1].join(","));
    assert!(curves::parse(&many).is_err());
    assert!(curves::parse(r#"{"red": [1, 2]}"#).is_err());
    // NaN goes through a curve as NaN; a fourth channel gets the master curve only
    let inv = curves::parse(r#"{"master": {"points": [[0, 1], [1, 0]]}, "red": {"points": [[0, 1], [1, 0]]}}"#).unwrap();
    let out = curves::curve_planes(&[vec![f32::NAN, 0.25], vec![0.25], vec![0.25], vec![0.25]], &inv, false);
    assert!(out[0][0].is_nan());
    assert_eq!((out[0][1], out[3][0]), (0.25, 0.75));
}
