//! Filters, edges and geometry, against references and through the executor: Gaussian Blur,
//! Median 3×3, Convolve, Custom Convolve, Perona–Malik, Gradient, Sobel Magnitude, Zero
//! Crossings, Segment Edges, Trace Contours, Rotate and Resize.
//!
//! `fixtures/filters/*.npy` are written by `claude-debug/filters_fixtures_gen.py` with
//! impressionist's venv (yapocis3 editable, numba backend, Pillow 12.3.0): yapocis's numba
//! kernels where the research software calls one, its truetrace CPU path, virtual_impressionist's `flow` and
//! `core.resize`, and float32 numpy replicas of the two yapocis_external GPU kernels (Sobel,
//! Rotate). Each kernel is compared with its reference with a stated tolerance; then each node
//! runs through the executor, and hostile inputs come back as errors, never panics.

mod common;

use common::{close, engine, error, img, ok, root, run};
use plumcraft_array::filters::{conv_kernels, custom_weights, kernels, ladder, resize, trace};
use serde_json::{Value, json};

// ---------- fixtures ----------

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/filters").join(name)
}

/// A little-endian float32 `.npy` (C order): its shape and values.
fn npy(name: &str) -> (Vec<usize>, Vec<f32>) {
    let bytes = std::fs::read(fixture_path(&format!("{name}.npy"))).unwrap();
    assert_eq!(&bytes[..6], b"\x93NUMPY");
    let (len, start) = if bytes[6] == 1 {
        (u16::from_le_bytes([bytes[8], bytes[9]]) as usize, 10)
    } else {
        (u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize, 12)
    };
    let header = std::str::from_utf8(&bytes[start..start + len]).unwrap();
    assert!(header.contains("'<f4'") && header.contains("'fortran_order': False"), "{name}: {header}");
    let shape_text = header.split("'shape': (").nth(1).unwrap().split(')').next().unwrap();
    let shape: Vec<usize> = shape_text.split(',').map(str::trim).filter(|t| !t.is_empty()).map(|t| t.parse().unwrap()).collect();
    let values: Vec<f32> = bytes[start + len..].as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    assert_eq!(values.len(), shape.iter().product::<usize>(), "{name}");
    (shape, values)
}

/// The largest absolute difference (NaN matches NaN; a NaN against a number is infinite).
fn max_err(got: &[f32], want: &[f32]) -> f64 {
    assert_eq!(got.len(), want.len());
    got.iter()
        .zip(want)
        .map(|(g, w)| {
            if g.to_bits() == w.to_bits() || (g.is_nan() && w.is_nan()) {
                0.0
            } else if g.is_nan() || w.is_nan() {
                f64::INFINITY
            } else {
                f64::from((g - w).abs())
            }
        })
        .fold(0.0, f64::max)
}

/// Compare and report (`cargo test -- --nocapture` prints the table the port's report quotes).
fn check(what: &str, got: &[f32], want: &[f32], tol: f64) {
    let e = max_err(got, want);
    eprintln!("{what}: max |error| {e:e} (tolerance {tol:e})");
    assert!(e <= tol, "{what}: max error {e} over {tol}");
}

fn input() -> (usize, usize, Vec<f32>) {
    let (s, a) = npy("input");
    (s[0], s[1], a)
}

// ---------- kernels against references ----------

/// numba compiled `_conv1d_h_njit` with fastmath: its float32 accumulation may be contracted
/// into fused multiply-adds, so expect a float32 ulp or two on values up to ~1 (2.5e-7; 1.2e-7
/// seen). The 0 frame is exact.
#[test]
fn gaussian_blur_matches_numba_convolvesep() {
    let (h, w, a) = input();
    for scale in [0usize, 1, 3] {
        let (_, want) = npy(&format!("gauss_{scale}"));
        let got = kernels::gauss_image(&a, h, w, ladder::LADDER[scale]);
        check(&format!("gauss scale {scale}"), &got, &want, 2.5e-7);
        let r = ladder::LADDER[scale].len() / 2;
        for y in 0..h {
            for x in 0..w {
                if y < r || x < r || y + r >= h || x + r >= w {
                    assert_eq!(got[y * w + x], 0.0, "frame at {x},{y}");
                }
            }
        }
    }
    assert_eq!(ladder::LADDER.iter().map(|l| l.len()).collect::<Vec<_>>(), [9, 13, 19, 27, 39, 59, 87, 127]);
}

/// `_median3x3_njit` is compiled without fastmath and only compares: exact.
#[test]
fn median_matches_numba() {
    let (h, w, a) = input();
    for it in [1usize, 3] {
        let (_, want) = npy(&format!("median_{it}"));
        check(&format!("median {it}"), &kernels::median(&a, h, w, it), &want, 0.0);
    }
    assert_eq!(kernels::median(&a, h, w, 0), a, "0 iterations: a copy");
}

/// Kernel weights are exact (float64 generators, npexact's pairwise sums, stored as float32).
/// `_conv1d_njit` was compiled with fastmath (its float64 sum may be reassociated): 1e-6 on
/// values up to ~1 (savgol and deriv sums cancel).
#[test]
fn convolve_matches_numba_and_its_weights_are_yapocis_s() {
    let weights: Value = serde_json::from_str(&std::fs::read_to_string(fixture_path("convolve_weights.json")).unwrap()).unwrap();
    let names: Vec<String> = weights["names"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect();
    assert_eq!(conv_kernels::names(), names, "the research software's kernel names, in its order");
    for name in &names {
        let want: Vec<f32> = weights["weights"][name].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
        assert_eq!(conv_kernels::kernel(name).unwrap(), want, "{name}");
    }
    assert!(conv_kernels::kernel("binomial4").is_none() && conv_kernels::kernel("nope").is_none());
    let (h, w, a) = input();
    for (name, dir) in [("binomial5", "both"), ("deriv1_3", "horizontal"), ("hamming9", "vertical"), ("savgol7", "both")] {
        let (_, want) = npy(&format!("conv_{name}_{dir}"));
        let k = conv_kernels::kernel(&name.replace('_', "")).unwrap();
        check(&format!("convolve {name} {dir}"), &kernels::convolve_rows(&a, h, w, &k, dir), &want, 1e-6);
    }
    let k = custom_weights("1, -2, 4", true).unwrap();
    check("custom 1,-2,4 normalised, both", &kernels::convolve_rows(&a, h, w, &k, "both"), &npy("custom_both").1, 1e-6);
    let k = custom_weights("1 0 -1 0 2", false).unwrap();
    check("custom 1 0 -1 0 2 raw, vertical", &kernels::convolve_rows(&a, h, w, &k, "vertical"), &npy("custom_raw_vertical").1, 1e-6);
}

/// `_pm_step2d_njit` was compiled with fastmath (float64 conductances, possibly contracted),
/// and iterating carries the differences along: 2e-6 on 0-1 values (2e-4 on the 0-300 "as is"
/// values, the same relative error).
#[test]
fn perona_malik_matches_flow_and_anisodiff() {
    let (h, w, a) = input();
    check("perona-malik map to 0-1, 8 its", &kernels::perona_malik(&a, h, w, 8, 3.0, 0.2, None), &npy("pm_map").1, 2e-6);
    let mask = npy("pm_edges_mask").1;
    check("perona-malik with edges, 20 its", &kernels::perona_malik(&a, h, w, 20, 5.0, 0.25, Some((&mask, 0.5))), &npy("pm_edges").1, 2e-6);
    let big: Vec<f32> = a.iter().map(|v| v * 300.0).collect();
    check("perona-malik as is (0-300), 5 its", &kernels::anisodiff_n(&big, h, w, 5, 3.0, 0.2), &npy("pm_asis").1, 2e-4);
}

/// `_gradient_njit` (fastmath): the magnitude is a float32 sqrt of float32 sums that fastmath
/// may contract (2.5e-7, an ulp or two; 6e-8 seen); the angle comes from atan2f, whose last
/// bits depend on the math library (1e-6 of π; exact here).
#[test]
fn gradient_matches_numba() {
    let (h, w, a) = input();
    for reach in [1usize, 3] {
        let (g, t) = kernels::gradient(&a, h, w, reach);
        check(&format!("gradient magnitude reach {reach}"), &g, &npy(&format!("grad_mag_{reach}")).1, 2.5e-7);
        check(&format!("gradient angle reach {reach}"), &t, &npy(&format!("grad_theta_{reach}")).1, 1e-6);
    }
}

/// A numpy float32 replica of the kernel's left-to-right arithmetic: exact.
#[test]
fn sobel_matches_the_replica() {
    let (h, w, a) = input();
    check("sobel", &kernels::sobel(&a, h, w), &npy("sobel").1, 0.0);
}

/// yapocis's numba zcs is numpy (float32 deltas, threshold cast to float32): exact.
#[test]
fn zero_crossings_match_numba() {
    let (h, w, _) = input();
    let z = npy("zc_input").1;
    check("zcs threshold 1e-5", &kernels::zcs(&z, h, w, (10.0f64 / 1e6) as f32), &npy("zcs_10").1, 0.0);
    check("zcs threshold 0", &kernels::zcs(&z, h, w, 0.0), &npy("zcs_0").1, 0.0);
}

/// flow.segment_edges compares in float64: exact.
#[test]
fn segment_edges_match_flow() {
    let (h, w, _) = input();
    check("segment edges, labels, frame", &kernels::segment_edges(&npy("seg_labels").1, h, w, 1, 0.0, true), &npy("seg_edges").1, 0.0);
    check("segment edges, colour, tolerance 0.02", &kernels::segment_edges(&npy("seg_colour").1, h, w, 3, 0.02, false), &npy("seg_colour_edges").1, 0.0);
}

/// truetrace's CPU path is float64 then float32 arithmetic with no fastmath: the distance
/// field and every segment end are exact.
#[test]
fn trace_contours_match_truetrace_cpu() {
    let (h, w, _) = input();
    let mask = npy("trace_mask").1;
    let m: Vec<bool> = mask.iter().map(|v| *v != 0.0).collect();
    check("trace signed distance field", &trace::sdf(&m, h, w), &npy("trace_sdf").1, 0.0);
    for offset in [0.0f32, 1.5] {
        let segs = trace::trace(&mask, h, w, offset, 1 << 20).unwrap();
        let (shape, want) = npy(&format!("trace_{offset:.1}"));
        assert_eq!(segs.len(), shape[0], "offset {offset}: segment count");
        let got: Vec<f32> = segs.into_iter().flatten().collect();
        check(&format!("trace offset {offset}"), &got, &want, 0.0);
    }
}

/// A numpy float32 replica of rotateImage: exact.
#[test]
fn rotate_matches_the_replica() {
    let (h, w, a) = input();
    let (fh, fw) = kernels::rotated_size(h, w, 15.0);
    let (oh, ow) = (fh as usize, fw as usize);
    let (shape, want) = npy("rotate_15");
    assert_eq!((oh, ow), (shape[0], shape[1]));
    check("rotate 15 expand", &kernels::rotate(&a, h, w, oh, ow, 15.0, 0.0), &want, 0.0);
    check("rotate -30 crop, background -1", &kernels::rotate(&a, h, w, h, w, -30.0, -1.0), &npy("rotate_m30").1, 0.0);
}

/// core.resize with Pillow 12.3.0's float resampling (arm64, where clang fuses the vertical
/// pass's multiply-adds, as the port does): exact.
#[test]
fn resample_matches_pillow() {
    let (h, w, a) = input();
    let t =
        |by: &str, dim: i64, width: i64, height: i64, scale: f64, shrink_only: bool| resize::Target { by: by.into(), dim, width, height, scale, shrink_only };
    // maxdim 20 of a stack, lanczos, keep_range, no align
    let (_, stack) = npy("resize_stack");
    let (oh, ow) = resize::target_size(h, w, &t("maxdim", 20, 0, 0, 1.0, true)).unwrap();
    let (shape, want) = npy("resize_lanczos_maxdim20");
    assert_eq!((oh, ow), (shape[0], shape[1]));
    let planes: Vec<Vec<f32>> = (0..3)
        .map(|k| {
            let p: Vec<f32> = stack.iter().skip(k).step_by(3).copied().collect();
            let mut out = resize::resample(&p, h, w, oh, ow, resize::Filter::Lanczos);
            let (lo, hi) = p.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(l, u), v| (l.min(*v), u.max(*v)));
            out.iter_mut().for_each(|v| *v = v.clamp(lo, hi));
            out
        })
        .collect();
    let got: Vec<f32> = (0..oh * ow).flat_map(|i| planes.iter().map(move |p| p[i])).collect();
    check("resize lanczos maxdim 20 (stack, keep_range)", &got, &want, 0.0);
    // scale 0.5 bilinear, aligned: 20 × 24
    let (shape, want) = npy("resize_bilinear_scale05");
    assert_eq!(resize::target_size(h, w, &t("scale", 0, 0, 0, 0.5, true)).unwrap(), (20, 24));
    let mut got = resize::resample(&a, h, w, shape[0], shape[1], resize::Filter::Bilinear);
    let (lo, hi) = a.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(l, u), v| (l.min(*v), u.max(*v)));
    got.iter_mut().for_each(|v| *v = v.clamp(lo, hi));
    check("resize bilinear scale 0.5", &got, &want, 0.0);
    // enlarged 1.7 × lanczos, no keep_range
    let (oh, ow) = resize::target_size(h, w, &t("scale", 0, 0, 0, 1.7, false)).unwrap();
    let (shape, want) = npy("resize_lanczos_up");
    assert_eq!((oh, ow), (shape[0], shape[1]));
    check("resize lanczos up 1.7", &resize::resample(&a, h, w, oh, ow, resize::Filter::Lanczos), &want, 0.0);
    // nearest, by width 30
    let (oh, ow) = resize::target_size(h, w, &t("size", 0, 30, 0, 1.0, true)).unwrap();
    let (shape, want) = npy("resize_nearest_size");
    assert_eq!((oh, ow), (shape[0], shape[1]));
    check("resize nearest width 30", &resize::nearest(&a, h, w, 1, oh, ow), &want, 0.0);
    // labels (nearest), 20 × 17
    let (_, labels) = npy("seg_labels");
    let (shape, want) = npy("resize_labels");
    check("resize labels", &resize::nearest(&labels, h, w, 1, shape[0], shape[1]), &want, 0.0);
    assert_eq!(resize::target_size(h, w, &t("mindim", 13, 0, 0, 1.0, true)).unwrap(), (13, 16));
}

// ---------- hostile input at the kernels ----------

#[test]
fn kernels_take_hostile_input_without_panicking() {
    let nan = [f32::NAN, f32::INFINITY, -f32::INFINITY, 0.0, 1.0, f32::NAN, 2.0, -3.0, 1e30];
    for (h, w) in [(0, 0), (0, 3), (3, 0), (1, 1), (1, 9), (9, 1), (3, 3)] {
        let p: Vec<f32> = nan.iter().copied().cycle().take(h * w).collect();
        for l in ladder::LADDER {
            assert_eq!(kernels::gauss_image(&p, h, w, l).len(), h * w);
        }
        assert_eq!(kernels::median(&p, h, w, 2).len(), h * w);
        for k in [vec![1.0f32], vec![0.25, 0.5, 0.25], vec![1.0; 127]] {
            assert_eq!(kernels::convolve_rows(&p, h, w, &k, "both").len(), h * w);
        }
        assert_eq!(kernels::perona_malik(&p, h, w, 3, 3.0, 0.25, Some((&p, 0.5))).len(), h * w);
        assert_eq!(kernels::anisodiff_n(&p, h, w, 3, 0.01, 0.25).len(), h * w);
        let (g, t) = kernels::gradient(&p, h, w, 64);
        assert_eq!((g.len(), t.len()), (h * w, h * w));
        let (g, t) = kernels::gradient(&p, h, w, 1);
        assert!(g.iter().chain(&t).all(|v| !v.is_nan()), "gradient NaN results are 0");
        assert_eq!(kernels::sobel(&p, h, w).len(), h * w);
        assert_eq!(kernels::zcs(&p, h, w, 0.0).len(), h * w);
        assert_eq!(kernels::segment_edges(&p, h, w, 1, 0.0, true).len(), h * w);
        // a stack whose data is short of its shape
        assert_eq!(kernels::segment_edges(&p, h, w, 3, 0.0, true).len(), h * w);
        assert_eq!(kernels::rotate(&p, h, w, 5, 4, 33.0, 0.5).len(), 20);
        assert!(trace::trace(&p, h, w, 100.0, 1 << 20).is_some());
        for f in [resize::Filter::Lanczos, resize::Filter::Bilinear] {
            assert_eq!(resize::resample(&p, h, w, 7, 2, f).len(), 14);
        }
        assert_eq!(resize::nearest(&p, h, w, 1, 2, 7).len(), 14);
    }
    // a checkerboard has the most segments: the cap stops it
    let board: Vec<f32> = (0..64 * 64).map(|i| ((i / 64 + i % 64) % 2) as f32).collect();
    assert!(trace::trace(&board, 64, 64, 0.0, 100).is_none());
    // a kernel shorter than its weights' length on a flat array keeps reflecting
    assert_eq!(kernels::conv1d(&[1.0, 2.0], &[0.2; 9]).len(), 2);
    let r = kernels::rotated_size(16384, 16384, 45.0);
    assert!(r.0 > 16384.0, "a rotated maximum array is too large (the node refuses it)");
    for bad in ["", "1, 2", "a b c", "1, inf, 2", "nan"] {
        assert!(custom_weights(bad, true).is_err(), "{bad:?}");
    }
    assert_eq!(custom_weights("1 -1 0", true).unwrap(), [1.0, -1.0, 0.0], "a zero sum is not normalised");
    let t = resize::Target { by: "diagonal".into(), dim: 10, width: 0, height: 0, scale: 1.0, shrink_only: true };
    assert!(resize::target_size(10, 10, &t).is_err());
}

// ---------- through the executor ----------

/// The fixture input as an 8-bit grey PNG, and the engine with it loaded (no align).
fn picture_graph(name: &str) -> (std::path::PathBuf, plumcraft_engine::Engine, Value) {
    let dir = root(name);
    let (h, w, a) = input();
    let px: Vec<u8> = a
        .iter()
        .flat_map(|v| {
            let g = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            [g, (255 - g) / 2, g / 3]
        })
        .collect();
    let png = dir.join("in.png");
    image::save_buffer(&png, &px, w as u32, h as u32, image::ColorType::Rgb8).unwrap();
    let e = engine(&dir);
    let nodes = json!({
        "l": {"kind": "array.load_image", "values": {"path": png.display().to_string(), "align": false}},
        "g": {"kind": "array.pick_channel", "values": {"channel": 1}},
    });
    (dir, e, nodes)
}

fn with(mut nodes: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().unwrap() {
        nodes[k] = v.clone();
    }
    nodes
}

#[test]
fn every_node_runs_through_the_executor_as_its_kernel() {
    let (_dir, e, base) = picture_graph("exec");
    let nodes = with(
        base,
        json!({
            "gb": {"kind": "array.gaussian_blur", "values": {"scale": 0}},
            "gbs": {"kind": "array.gaussian_blur", "values": {"scale": 0}},
            "md": {"kind": "array.median_3x3", "values": {"iterations": 2}},
            "cv": {"kind": "array.convolve", "values": {"kernel": "hann9", "direction": "vertical"}},
            "cc": {"kind": "array.custom_convolve", "values": {"weights": "1 2 1", "normalize": true, "direction": "horizontal"}},
            "pm": {"kind": "array.perona_malik", "values": {"iterations": 4}},
            "se": {"kind": "array.segment_edges", "values": {"tolerance": 0.1, "frame": false}},
            "pme": {"kind": "array.perona_malik", "values": {"iterations": 4, "boost": 0.4}},
            "gr": {"kind": "array.gradient", "values": {"reach": 2, "angle_unit": "radians"}},
            "grs": {"kind": "array.gradient"},
            "sb": {"kind": "array.sobel_magnitude"},
            "zc": {"kind": "array.zero_crossings", "values": {"threshold": 0.0}},
            "tc": {"kind": "array.trace_contours", "values": {"offset": 0.5}},
            "rt": {"kind": "array.rotate", "values": {"degrees": 90.0}},
            "rs": {"kind": "array.resize", "values": {"by": "scale", "scale": 0.5, "resample": "bilinear"}},
        }),
    );
    let wires = [
        ("l", "rgb", "g", "array"),
        ("g", "channel", "gb", "array"),
        ("l", "rgb", "gbs", "array"),
        ("g", "channel", "md", "array"),
        ("g", "channel", "cv", "array"),
        ("g", "channel", "cc", "array"),
        ("g", "channel", "pm", "array"),
        ("l", "rgb", "se", "segments"),
        ("g", "channel", "pme", "array"),
        ("se", "edges", "pme", "edges"),
        ("g", "channel", "gr", "array"),
        ("l", "rgb", "grs", "array"),
        ("g", "channel", "sb", "array"),
        ("gb", "blurred", "zc", "array"),
        ("g", "channel", "tc", "mask"),
        ("l", "rgb", "rt", "array"),
        ("l", "rgb", "rs", "array"),
    ];
    let targets = ["l", "g", "gb", "gbs", "md", "cv", "cc", "pm", "se", "pme", "gr", "grs", "sb", "zc", "tc", "rt", "rs"];
    let r = ok(run(&e, nodes, &wires, &targets));
    let p = img(&e, &r, "g", "channel");
    let (h, w, a) = (p.h, p.w, p.data.clone());
    let rgb = img(&e, &r, "l", "rgb");
    let gb = img(&e, &r, "gb", "blurred");
    assert_eq!((gb.h, gb.w, gb.c, &gb.data), (h, w, 1, &kernels::gauss_image(&a, h, w, ladder::LADDER[0])));
    let gbs = img(&e, &r, "gbs", "blurred");
    assert_eq!((gbs.c, gbs.space.as_deref(), gbs.names.len()), (3, Some("RGB"), 3), "a blurred stack keeps its space and names");
    assert_eq!(gbs.plane(0), gb.data);
    assert_eq!(img(&e, &r, "md", "filtered").data, kernels::median(&a, h, w, 2));
    assert_eq!(img(&e, &r, "cv", "convolved").data, kernels::convolve_rows(&a, h, w, &conv_kernels::kernel("hann9").unwrap(), "vertical"));
    assert_eq!(img(&e, &r, "cc", "convolved").data, kernels::convolve_rows(&a, h, w, &[0.25, 0.5, 0.25], "horizontal"));
    assert_eq!(img(&e, &r, "pm", "diffused").data, kernels::perona_malik(&a, h, w, 4, 3.0, 0.2, None));
    let se = img(&e, &r, "se", "edges");
    assert_eq!(se.data, kernels::segment_edges(&rgb.data, h, w, 3, 0.1, false));
    assert_eq!(img(&e, &r, "pme", "diffused").data, kernels::perona_malik(&a, h, w, 4, 3.0, 0.2, Some((&se.data, 0.4))));
    let (g, t) = kernels::gradient(&a, h, w, 2);
    let gm = img(&e, &r, "gr", "magnitude");
    assert_eq!(gm.data, g);
    assert!(close(&img(&e, &r, "gr", "angle").data, &t.iter().map(|v| v * std::f32::consts::PI).collect::<Vec<_>>(), 0.0));
    let grs = img(&e, &r, "grs", "angle");
    assert_eq!((grs.c, grs.space.as_deref(), grs.names.len()), (3, None, 0), "a gradient is not a colour");
    assert!(grs.data.iter().all(|v| (-180.0..=180.0).contains(v)), "degrees");
    assert_eq!(img(&e, &r, "sb", "magnitude").data, kernels::sobel(&a, h, w));
    assert_eq!(img(&e, &r, "zc", "crossings").data, kernels::zcs(&gb.data, h, w, 0.0));
    let tc = common::array(&e, &r, "tc", "segments");
    assert!(tc.table && tc.names == ["x0", "y0", "x1", "y1"]);
    let segs = trace::trace(&a, h, w, 0.5, 1 << 20).unwrap();
    assert_eq!(tc.shape, [segs.len() as u64, 4]);
    assert_eq!(img(&e, &r, "tc", "segments").data, segs.into_iter().flatten().collect::<Vec<_>>());
    let rt = img(&e, &r, "rt", "rotated");
    assert_eq!((rt.h, rt.w, rt.c, rt.space.as_deref()), (w, h, 3, Some("RGB")), "90° expanded: the sides swap");
    let rs = img(&e, &r, "rs", "resized");
    assert_eq!((rs.h, rs.w, rs.c, rs.space.as_deref()), (20, 24, 3, Some("RGB")));
}

#[test]
fn resize_aligns_and_an_outline_can_be_empty() {
    let dir = root("resize");
    let e = engine(&dir);
    let nodes = json!({
        "c": {"kind": "array.constant", "values": {"value": 0.25, "width": 30, "height": 23, "channels": 2}},
        "same": {"kind": "array.resize", "values": {"by": "maxdim", "dim": 1600}},
        "tiny": {"kind": "array.constant", "values": {"width": 3, "height": 2}},
        "tiny_r": {"kind": "array.resize", "values": {"by": "scale", "scale": 1.0}},
        "up": {"kind": "array.resize", "values": {"by": "size", "target_width": 50, "shrink_only": false, "align": false, "resample": "nearest"}},
        "m": {"kind": "array.constant", "values": {"value": 1.0, "width": 30, "height": 23}},
        "tc": {"kind": "array.trace_contours"},
    });
    let r = ok(run(
        &e,
        nodes,
        &[("c", "constant", "same", "array"), ("tiny", "constant", "tiny_r", "array"), ("c", "constant", "up", "array"), ("m", "constant", "tc", "mask")],
        &["same", "tiny_r", "up", "tc"],
    ));
    let same = img(&e, &r, "same", "resized");
    assert_eq!((same.h, same.w, same.c), (20, 28, 2), "kept its size: cropped to multiples of 4");
    let tiny = img(&e, &r, "tiny_r", "resized");
    assert_eq!((tiny.h, tiny.w), (2, 3), "smaller than the alignment: kept");
    let up = img(&e, &r, "up", "resized");
    assert_eq!((up.h, up.w, up.data[0]), (38, 50, 0.25));
    // an all-inside mask has no background, so no crossing: an empty (0, 4) table
    let tc = common::array(&e, &r, "tc", "segments");
    assert_eq!(tc.shape, [0, 4], "an empty table");
}

#[test]
fn hostile_inputs_are_errors() {
    let (_dir, e, base) = picture_graph("hostile");
    let err = |extra: Value, wires: &[(&str, &str, &str, &str)], target: &str| {
        let r = run(&e, with(base.clone(), extra), wires, &[target]);
        error(&r, target)
    };
    // a table into a picture filter, and an empty table
    let m = json!({"z": {"kind": "array.constant", "values": {"value": 0.0, "width": 8, "height": 8}}, "tc": {"kind": "array.trace_contours"}, "gb": {"kind": "array.gaussian_blur"}, "rs": {"kind": "array.resize"}});
    assert!(err(m.clone(), &[("z", "constant", "tc", "mask"), ("tc", "segments", "gb", "array")], "gb").contains("table"));
    assert!(err(m, &[("z", "constant", "tc", "mask"), ("tc", "segments", "rs", "array")], "rs").contains("table"));
    // wrong channel counts
    assert!(err(json!({"tc": {"kind": "array.trace_contours"}}), &[("l", "rgb", "tc", "mask")], "tc").contains("one channel"));
    let pm = json!({"pm": {"kind": "array.perona_malik"}});
    assert!(err(pm.clone(), &[("l", "rgb", "g", "array"), ("g", "channel", "pm", "array"), ("l", "rgb", "pm", "edges")], "pm").contains("one channel"));
    let pm_small = json!({"pm": {"kind": "array.perona_malik"}, "c": {"kind": "array.constant", "values": {"width": 5, "height": 5}}});
    assert!(err(pm_small, &[("l", "rgb", "g", "array"), ("g", "channel", "pm", "array"), ("c", "constant", "pm", "edges")], "pm").contains("differ"));
    let as_is = json!({"pm": {"kind": "array.perona_malik", "values": {"input": "as is"}}});
    assert!(err(as_is, &[("l", "rgb", "g", "array"), ("g", "channel", "pm", "array"), ("g", "channel", "pm", "edges")], "pm").contains("map to 0-1"));
    // bad parameters
    for weights in ["1, 2", "", "x"] {
        let cc = json!({"cc": {"kind": "array.custom_convolve", "values": {"weights": weights}}});
        assert!(err(cc, &[("l", "rgb", "cc", "array")], "cc").contains("weights"));
    }
    let cv = json!({"cv": {"kind": "array.convolve", "values": {"kernel": "gauss999"}}});
    assert!(err(cv, &[("l", "rgb", "cv", "array")], "cv").contains("kernel"));
    let cv = json!({"cv": {"kind": "array.convolve", "values": {"direction": "diagonal"}}});
    assert!(err(cv, &[("l", "rgb", "cv", "array")], "cv").contains("direction"));
    let gr = json!({"gr": {"kind": "array.gradient", "values": {"angle_unit": "gradians"}}});
    assert!(err(gr, &[("l", "rgb", "gr", "array")], "gr").contains("angle_unit"));
    let rs = json!({"rs": {"kind": "array.resize", "values": {"resample": "cubic"}}});
    assert!(err(rs, &[("l", "rgb", "rs", "array")], "rs").contains("resample"));
    let rs = json!({"rs": {"kind": "array.resize", "values": {"by": "area"}}});
    assert!(err(rs, &[("l", "rgb", "rs", "array")], "rs").contains("maxdim"));
    // too large a result
    let big = json!({"c": {"kind": "array.constant", "values": {"width": 1100, "height": 1100}}, "rs": {"kind": "array.resize", "values": {"by": "scale", "scale": 16.0, "shrink_only": false}}});
    assert!(err(big, &[("c", "constant", "rs", "array")], "rs").contains("too large"));
    // nothing wired
    let unwired = err(json!({"gb": {"kind": "array.gaussian_blur"}}), &[], "gb");
    assert!(unwired.contains("array"), "{unwired}");
    // out-of-range numbers are clamped, never fatal
    let wild = json!({
        "gb": {"kind": "array.gaussian_blur", "values": {"scale": 99}},
        "md": {"kind": "array.median_3x3", "values": {"iterations": -5}},
        "gr": {"kind": "array.gradient", "values": {"reach": 100000}},
        "pm": {"kind": "array.perona_malik", "values": {"iterations": 3, "kappa": 0.0, "step": 9.0, "boost": 5.0}},
        "rt": {"kind": "array.rotate", "values": {"degrees": 1e12, "background": 1e300}},
        "zc": {"kind": "array.zero_crossings", "values": {"threshold": -1.0}},
    });
    let wires = [
        ("l", "rgb", "g", "array"),
        ("g", "channel", "gb", "array"),
        ("g", "channel", "md", "array"),
        ("g", "channel", "gr", "array"),
        ("g", "channel", "pm", "array"),
        ("g", "channel", "rt", "array"),
        ("g", "channel", "zc", "array"),
    ];
    let r = ok(run(&e, with(base.clone(), wild), &wires, &["gb", "md", "gr", "pm", "rt", "zc"]));
    let gb = img(&e, &r, "gb", "blurred");
    assert!(gb.data.iter().all(|v| *v == 0.0), "scale 7's 63 px frame covers a 40 × 48 array");
    assert!(img(&e, &r, "gr", "magnitude").data.iter().all(|v| *v == 0.0), "reach 64 leaves nothing inside");
}
