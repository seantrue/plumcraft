//! Generate, Regions, Draw and Frequency: each kernel against references from yapocis's numba
//! backend and yapocis_external's numba kernels (`claude-debug/generate_fixtures_gen.py` writes
//! `tests/fixtures/generate/`), every node through the executor, and hostile inputs as errors,
//! never panics.

mod common;

use common::{close, engine, error, img, ok, root, run};
use plumcraft_array::generate::{draw, fft, noise, points, regions};
use serde_json::{Value, json};

// ---------- fixtures ----------

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/generate/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// (shape, values) of a stored array; null is NaN.
fn arr(v: &Value) -> (Vec<usize>, Vec<f32>) {
    let shape = v["shape"].as_array().unwrap().iter().map(|d| d.as_u64().unwrap() as usize).collect();
    let data = v["data"].as_array().unwrap().iter().map(|x| x.as_f64().map(|f| f as f32).unwrap_or(f32::NAN)).collect();
    (shape, data)
}

fn max_err(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| if x.is_nan() && y.is_nan() { 0.0 } else { (x - y).abs() }).fold(0.0, f32::max)
}

fn never() -> bool {
    false
}

// ---------- kernels against their references ----------

#[test]
fn simplex_matches_numba() {
    // numba compiled the kernel with fastmath, but in float64 throughout fastmath's reassociation
    // never reaches the float32 result here: exact (and the port uses only IEEE + - × ÷ and
    // floor, so it is exact on every machine)
    // square cases carry "size"; the non-square tileable ones (#83) carry "height" and "width"
    for case in fixture("simplex").as_array().unwrap() {
        let dim = |k: &str| case[k].as_u64().or(case["size"].as_u64()).unwrap() as usize;
        let (h, w) = (dim("height"), dim("width"));
        let (seed, cells, octaves) = (case["seed"].as_i64().unwrap(), case["cells"].as_f64().unwrap(), case["octaves"].as_i64().unwrap());
        let out = match case["kind"].as_str().unwrap() {
            "smooth" => noise::simplex2d(
                h,
                w,
                seed,
                cells,
                octaves,
                case["persistence"].as_f64().unwrap(),
                case["lacunarity"].as_f64().unwrap(),
                case["tileable"].as_bool().unwrap(),
            ),
            "turbulence" => noise::turbulence(h, w, seed, cells, octaves),
            _ => noise::ridged(h, w, seed, cells, octaves),
        };
        let (shape, want) = arr(&case["out"]);
        assert_eq!(shape, [h, w]);
        let e = max_err(&out, &want);
        eprintln!("simplex {} {h}x{w} tileable {}: max error {e:e}", case["kind"], case["tileable"]);
        assert_eq!(e, 0.0, "simplex {} {h}x{w}", case["kind"]);
    }
}

#[test]
fn mandelbrot_matches_numba() {
    // the numba backend is numpy's complex64 steps: exact, with the fused multiply-adds numpy's
    // SIMD loop uses on arm64 and on x86-64 with FMA3 and AVX2. One golden serves both: recorded
    // again on yellowfin (x86-64), it matched the M4 Max's in every pixel (docs/BUGS.md B5)
    for case in fixture("mandelbrot").as_array().unwrap() {
        let (w, h) = (case["width"].as_u64().unwrap() as usize, case["height"].as_u64().unwrap() as usize);
        let out = noise::mandelbrot_view(
            case["center_x"].as_f64().unwrap(),
            case["center_y"].as_f64().unwrap(),
            case["zoom"].as_f64().unwrap(),
            case["max_iterations"].as_i64().unwrap(),
            h,
            w,
            &never,
        )
        .unwrap();
        let (_, want) = arr(&case["out"]);
        let wrong = out.iter().zip(&want).filter(|(a, b)| a != b).count();
        eprintln!("mandelbrot {w}×{h}: {wrong} of {} pixels differ, max error {:e}", out.len(), max_err(&out, &want));
        assert_eq!(wrong, 0);
    }
}

#[test]
fn fft_matches_numpy() {
    // numba's fft1d is numpy's pocketfft in complex64; this one runs in float64 (with libm's
    // sin/cos for twiddles), so they agree to float32 rounding: magnitudes within 1e-7 of the
    // largest (seen: 8e-9), log magnitude and the inverse within 1e-6 (seen: 2.4e-7), phases
    // within 2e-6 radians wherever the magnitude is above 1e-4 of the largest (seen: 4.8e-7; the
    // phase of a near-zero bin is noise in both).
    for case in fixture("fft").as_array().unwrap() {
        let (shape, x) = arr(&case["input"]);
        let (h, w) = (shape[0], shape[1]);
        let [m, ph, log] = fft::fft_plane(&x, h, w, &never).unwrap();
        let (_, wm) = arr(&case["magnitude"]);
        let (_, wph) = arr(&case["phase"]);
        let (_, wlog) = arr(&case["log_magnitude"]);
        let peak = wm.iter().cloned().fold(0.0, f32::max);
        let em = max_err(&m, &wm) / peak;
        let elog = max_err(&log, &wlog);
        let eph = m.iter().zip(ph.iter().zip(&wph)).filter(|(mag, _)| **mag > 1e-4 * peak).map(|(_, (a, b))| {
            let d = (a - b).abs();
            d.min((2.0 * std::f32::consts::PI - d).abs())
        });
        let eph = eph.fold(0.0, f32::max);
        let back = fft::inverse_plane(&wm, &wph, h, w, &never).unwrap();
        let (_, wback) = arr(&case["inverse"]);
        let eback = max_err(&back, &wback);
        eprintln!("fft {h}×{w}: magnitude {em:e} (relative), log {elog:e}, phase {eph:e}, inverse {eback:e}");
        assert!(em <= 1e-7 && elog <= 1e-6 && eph <= 2e-6 && eback <= 1e-6, "fft {h}×{w}");
        assert!(max_err(&back, &x) <= 2e-6, "the round trip gives the input back");
    }
}

fn canvas_of(v: &Value) -> draw::Rgba {
    let (shape, data) = arr(v);
    let (h, w) = (shape[0], shape[1]);
    let planes: Vec<Vec<f32>> = (0..3).map(|k| data.iter().skip(k).step_by(3).copied().collect()).collect();
    draw::Rgba::from_rgb(h, w, &planes)
}

fn rgb_of(c: &draw::Rgba) -> Vec<f32> {
    c.data.as_chunks::<4>().0.iter().flat_map(|p| p[..3].to_vec()).collect()
}

#[test]
fn shapes_and_composite_match_numba_graphics4() {
    // graphics4 runs with fastmath; numba's types (float64 geometry, float32 colours, float64
    // blend sums) are kept here, and the results are exact (only + - × ÷ and sqrt: the same on
    // every machine)
    let f = fixture("shapes");
    for case in f["cases"].as_array().unwrap() {
        let mut c = canvas_of(&f["canvas"]);
        let shape = case["shape"].as_str().unwrap();
        let opacity = case["opacity"].as_f64().unwrap() as f32;
        let colors: Vec<[f32; 4]> = match case.get("colors").filter(|v| !v.is_null()) {
            Some(per) => arr(per).1.chunks(3).map(|p| [p[0], p[1], p[2], opacity]).collect(),
            None => {
                let [r, g, b] = draw::hex_rgb(case["color"].as_str().unwrap()).unwrap();
                vec![[r as f32, g as f32, b as f32, opacity]; 16]
            }
        };
        let (width, aa) = (case["width"].as_f64().unwrap(), case["aa"].as_f64().unwrap());
        for (row, col) in case["rows"].as_array().unwrap().iter().zip(colors) {
            let row: Vec<f64> = row.as_array().unwrap().iter().map(|v| f64::from(v.as_f64().unwrap() as f32)).collect();
            draw::draw_shape(&mut c, shape, &row, f64::from(width as f32), f64::from(aa as f32), col);
        }
        let (_, want) = arr(&case["out"]);
        let e = max_err(&rgb_of(&c), &want);
        eprintln!("graphics4 {shape} aa {aa}: max error {e:e}");
        assert_eq!(e, 0.0, "{shape} aa {aa}");
    }
    let (bshape, block) = arr(&f["block"]);
    let (_, alpha) = arr(&f["alpha"]);
    for case in f["composite"].as_array().unwrap() {
        let mut c = canvas_of(&f["canvas"]);
        let planes: Vec<Vec<f32>> = (0..3).map(|k| block.iter().skip(k).step_by(3).copied().collect()).collect();
        let mut piece = draw::Rgba::from_rgb(bshape[0], bshape[1], &planes);
        let opacity = case["opacity"].as_f64().unwrap() as f32;
        for (px, a) in piece.data.as_chunks_mut::<4>().0.iter_mut().zip(&alpha) {
            px[3] = a.clamp(0.0, 1.0) * opacity;
        }
        draw::composite_block(&mut c, &piece, case["x"].as_i64().unwrap(), case["y"].as_i64().unwrap());
        let (_, want) = arr(&case["out"]);
        let e = max_err(&rgb_of(&c), &want);
        eprintln!("graphics4 composite: max error {e:e}");
        assert_eq!(e, 0.0, "composite");
    }
}

#[test]
fn numpy_draws_are_numpys() {
    // npexact's PCG64 and Generator: exact
    let d = fixture("draws");
    let rp = &d["random_points"];
    let out = points::random_points_of(
        rp["height"].as_u64().unwrap() as usize,
        rp["width"].as_u64().unwrap() as usize,
        rp["count"].as_u64().unwrap() as usize,
        rp["seed"].as_u64().unwrap() as u128,
        &[],
    );
    assert_eq!(out, arr(&rp["out"]).1, "random points");
    let mp = &d["mask_points"];
    let (shape, mask) = arr(&mp["mask"]);
    let out = points::mask_points_of(&mask, shape[1], mp["fraction"].as_f64().unwrap(), mp["seed"].as_u64().unwrap() as u128);
    assert_eq!(out, arr(&mp["out"]).1, "mask points");
}

#[test]
fn external_kernels_match_yapocis_external() {
    let x = fixture("external");
    // connected components and outlines: exact
    let (shape, edges) = arr(&x["components"]["edges"]);
    let labels = regions::connected_components_of(&edges, shape[0], shape[1]);
    assert_eq!(labels, arr(&x["components"]["out"]).1, "connected components");
    let (shape, lab) = arr(&x["outlines"]["labels"]);
    assert_eq!(regions::outlines_of(&lab, shape[0], shape[1]), arr(&x["outlines"]["out"]).1, "outlines");
    // segments: float32 sums in the kernel's order: exact
    for (name, median) in [("segments_mean", false), ("segments_median", true), ("segments_mean_noperiod", false)] {
        let case = &x[name];
        let (shape, image) = arr(&case["img"]);
        let clipped: Vec<f32> = image.iter().map(|v| v.clamp(0.0, 1.0)).collect();
        let segs: Vec<[i64; 5]> = case["segments"].as_array().unwrap().iter().map(|r| std::array::from_fn(|k| r[k].as_i64().unwrap())).collect();
        let out = regions::segment_average_of(&clipped, shape[0], shape[1], &segs, median, case["period"].as_f64().unwrap() as f32).unwrap();
        let want = arr(&case["out"]).1;
        eprintln!("{name}: max error {:e}", max_err(&out, &want));
        assert_eq!(out, want, "{name}");
    }
    // draw lines: hard and antialiased (float32, no fastmath): exact
    let dl = &x["draw_lines"];
    let (shape, canvas) = arr(&dl["canvas"]);
    let (h, w) = (shape[0], shape[1]);
    let (_, lines) = arr(&dl["lines"]);
    let swapped: Vec<[f32; 4]> = lines.chunks(4).map(|r| [r[1], r[0], r[3], r[2]]).collect();
    let mut hard = canvas.clone();
    for [a, b, c, d] in &swapped {
        draw::bresenham(&mut hard, h, w, *a as i32 as i64, *b as i32 as i64, *c as i32 as i64, *d as i32 as i64, dl["value"].as_f64().unwrap() as f32);
    }
    assert_eq!(hard, arr(&dl["hard"]).1, "Bresenham lines");
    let mut aa = canvas.clone();
    draw::antialiased_lines(&mut aa, h, w, &swapped);
    let want = arr(&dl["antialiased"]).1;
    eprintln!("graphicsx lines: max error {:e}", max_err(&aa, &want));
    assert_eq!(aa, want, "antialiased lines");
    // life: exact
    let (shape, start) = arr(&x["life"]["start"]);
    let mut state: Vec<u8> = start.iter().map(|v| *v as u8).collect();
    for _ in 0..x["life"]["steps"].as_u64().unwrap() {
        state = noise::life_step(&state, shape[0], shape[1]);
    }
    assert_eq!(state.iter().map(|v| f32::from(*v)).collect::<Vec<_>>(), arr(&x["life"]["out"]).1, "life");
}

// ---------- the nodes through the executor ----------

#[test]
fn generators_through_the_executor() {
    let dir = root("gen-generate");
    let e = engine(&dir);
    let d = fixture("draws");
    let nodes = json!({
        "c": {"kind": "array.constant", "values": {"width": 12, "height": 10, "channels": 3}},
        "xy": {"kind": "array.coordinates"},
        "noise": {"kind": "array.simplex_noise", "values": {"width": 20, "height": 20, "seed": 11, "cells": 3.0, "octaves": 5, "kind": "ridged"}},
        "like": {"kind": "array.simplex_noise", "values": {"seed": 1}},
        "m": {"kind": "array.mandelbrot", "values": {"width": 40, "height": 30, "max_iterations": 64}},
        "life": {"kind": "array.game_of_life", "values": {"steps": 0, "seed": 8, "density": 0.3}},
        "life2": {"kind": "array.game_of_life", "values": {"steps": 4}},
    });
    let wires = [("c", "constant", "xy", "like"), ("c", "constant", "noise", "like"), ("c", "constant", "life", "like"), ("xy", "x", "life2", "start")];
    let r = ok(run(&e, nodes, &wires, &["xy", "noise", "like", "m", "life", "life2"]));
    let x = img(&e, &r, "xy", "x");
    let y = img(&e, &r, "xy", "y");
    assert_eq!((x.h, x.w, x.data[13], y.data[13]), (10, 12, 1.0, 1.0));
    let noise = img(&e, &r, "noise", "noise");
    assert_eq!((noise.h, noise.w), (10, 12), "the size of like");
    assert!(noise.data.iter().all(|v| (0.0..=1.0).contains(v)));
    assert_eq!(noise.data, noise::ridged(10, 12, 11, 3.0, 5));
    let like = img(&e, &r, "like", "noise");
    assert_eq!((like.h, like.w), (512, 512));
    let m = img(&e, &r, "m", "escape");
    let want = fixture("mandelbrot")[0]["out"].clone();
    assert_eq!(m.data, arr(&want).1);
    // a random start with steps 0 is the start: numpy's draws (10 × 12, seed 8)
    let life = img(&e, &r, "life", "cells");
    assert_eq!(life.data, arr(&d["life_start"]["out"]).1);
    let life2 = img(&e, &r, "life2", "cells");
    assert!(life2.data.iter().all(|v| *v == 0.0 || *v == 1.0));
    // no start and no like
    let r = run(&e, json!({"l": {"kind": "array.game_of_life"}}), &[], &["l"]);
    assert!(error(&r, "l").contains("start or like"));
}

#[test]
fn regions_through_the_executor() {
    let dir = root("gen-regions");
    let e = engine(&dir);
    let nodes = json!({
        "c": {"kind": "array.constant", "values": {"width": 20, "height": 16, "value": 0.4}},
        "n": {"kind": "array.simplex_noise", "values": {"seed": 4, "cells": 3.0}},
        "pts": {"kind": "array.random_points", "values": {"count": 6, "seed": 2, "extra": "18 3"}},
        "edges": {"kind": "array.draw_lines", "values": {"style": "hard", "value": 1.0}},
        "cc": {"kind": "array.connected_components"},
        "out": {"kind": "array.region_outlines"},
        "grid": {"kind": "array.grid_segments", "values": {"tile_width": 6, "tile_height": 5}},
        "avg": {"kind": "array.segment_average"},
        "med": {"kind": "array.segment_average", "values": {"statistic": "median"}},
    });
    let wires = [
        ("c", "constant", "n", "like"),
        ("c", "constant", "pts", "like"),
        ("c", "constant", "edges", "canvas"),
        ("pts", "points", "edges", "lines"),
        ("edges", "drawing", "cc", "edges"),
        ("cc", "labels", "out", "labels"),
        ("c", "constant", "grid", "like"),
        ("n", "noise", "avg", "array"),
        ("grid", "segments", "avg", "segments"),
        ("n", "noise", "med", "array"),
        ("grid", "segments", "med", "segments"),
    ];
    let r = ok(run(&e, nodes, &wires, &["pts", "edges", "cc", "out", "grid", "avg", "med", "n"]));
    let pts = img(&e, &r, "pts", "points");
    assert!(pts.table);
    assert_eq!((pts.h, pts.c, pts.names.as_slice()), (6, 4, &["x".to_owned(), "y".into(), "c3".into(), "c4".into()][..]));
    assert_eq!(&pts.data[2..4], &[18.0, 3.0]);
    let edges = img(&e, &r, "edges", "drawing");
    let cc = common::array(&e, &r, "cc", "labels");
    assert_eq!((cc.dtype.as_str(), cc.names.as_slice()), ("<i4", &["label".to_owned()][..]));
    let cc = img(&e, &r, "cc", "labels");
    assert_eq!(cc.data, regions::connected_components_of(&edges.data, 16, 20));
    assert_eq!(cc.data[0], 0.0, "the first label is 0");
    let outl = img(&e, &r, "out", "outlines");
    assert_eq!(outl.data, regions::outlines_of(&cc.data, 16, 20));
    let grid = img(&e, &r, "grid", "segments");
    assert_eq!((grid.h, grid.c, grid.table), (4 * 4, 5, true));
    assert_eq!(&grid.data[..5], &[30.0, 0.0, 0.0, 5.0, 4.0]);
    assert_eq!(&grid.data[15..20], &[10.0, 18.0, 0.0, 19.0, 4.0], "the last tile of a row is cut short");
    let n = img(&e, &r, "n", "noise");
    let avg = img(&e, &r, "avg", "averaged");
    // the kernel's float32 sum: columns outer, rows inner
    let mean = (0..6).flat_map(|x| (0..5).map(move |y| (x, y))).fold(0.0f32, |s, (x, y)| s + n.data[y * 20 + x]) / 30.0;
    assert_eq!((avg.data[0], avg.data[4 * 20 + 5]), (mean, mean), "a tile is its mean");
    let med = img(&e, &r, "med", "averaged");
    let mut tile: Vec<f32> = (0..6).flat_map(|x| (0..5).map(move |y| (x, y))).map(|(x, y)| n.data[y * 20 + x]).collect();
    tile.sort_by(f32::total_cmp);
    assert_eq!(med.data[0], tile[15], "the upper middle of the tile");
    // a picture as segments, a table of 4 columns
    let r = run(
        &e,
        json!({"c": {"kind": "array.constant"}, "a": {"kind": "array.segment_average"}}),
        &[("c", "constant", "a", "array"), ("c", "constant", "a", "segments")],
        &["a"],
    );
    assert!(error(&r, "a").contains("table"));
    let r = run(
        &e,
        json!({"c": {"kind": "array.constant", "values": {"width": 8, "height": 8}}, "p": {"kind": "array.random_points", "values": {"count": 3, "extra": "1 2"}}, "a": {"kind": "array.segment_average"}}),
        &[("c", "constant", "a", "array"), ("c", "constant", "p", "like"), ("p", "points", "a", "segments")],
        &["a"],
    );
    assert!(error(&r, "a").contains("(k, 5)"));
}

#[test]
fn drawing_through_the_executor() {
    let dir = root("gen-draw");
    let e = engine(&dir);
    let nodes = json!({
        "grey": {"kind": "array.constant", "values": {"width": 30, "height": 20, "value": 0.25}},
        "rgb": {"kind": "array.constant", "values": {"width": 30, "height": 20, "channels": 3, "value": 0.5}},
        "rgbs": {"kind": "array.stack", "values": {"space": "RGB"}},
        "pts": {"kind": "array.random_points", "values": {"count": 5, "seed": 3, "extra": "4 7"}},
        "circles": {"kind": "array.draw_shapes", "values": {"shape": "circles", "color": "#ff0000", "opacity": 0.5}},
        "grey_lines": {"kind": "array.draw_shapes", "values": {"shape": "lines", "width": 2.0}},
        "percolor": {"kind": "array.draw_shapes", "values": {"shape": "circles"}},
        "aa": {"kind": "array.draw_lines", "values": {"style": "antialiased"}},
        "block": {"kind": "array.constant", "values": {"width": 8, "height": 6, "value": 1.0}},
        "alpha": {"kind": "array.constant", "values": {"width": 8, "height": 6, "value": 0.5}},
        "comp": {"kind": "array.composite", "values": {"x": 25, "y": -2, "opacity": 0.5}},
        "comp_rgb": {"kind": "array.composite", "values": {"x": 1, "y": 1}},
        "mp": {"kind": "array.mask_points", "values": {"fraction": 0.5, "seed": 3}},
    });
    let wires = [
        ("rgb", "constant", "rgbs", "c1"),
        ("grey", "constant", "pts", "like"),
        ("rgbs", "stack", "circles", "canvas"),
        ("pts", "points", "circles", "shapes"),
        ("grey", "constant", "grey_lines", "canvas"),
        ("pts", "points", "grey_lines", "shapes"),
        ("rgbs", "stack", "percolor", "canvas"),
        ("pts", "points", "percolor", "shapes"),
        ("pts", "points", "percolor", "colors"),
        ("grey", "constant", "aa", "canvas"),
        ("pts", "points", "aa", "lines"),
        ("grey", "constant", "comp", "canvas"),
        ("block", "constant", "comp", "block"),
        ("alpha", "constant", "comp", "alpha"),
        ("rgbs", "stack", "comp_rgb", "canvas"),
        ("block", "constant", "comp_rgb", "block"),
        ("aa", "drawing", "mp", "mask"),
    ];
    let r = ok(run(&e, nodes, &wires, &["circles", "grey_lines", "percolor", "aa", "comp", "comp_rgb", "mp", "pts"]));
    let pts = img(&e, &r, "pts", "points");
    // the same drawing made directly
    let mut want = draw::Rgba::from_rgb(20, 30, &[vec![0.5; 600], vec![0.5; 600], vec![0.5; 600]]);
    for row in pts.data.chunks(4) {
        draw::draw_shape(&mut want, "circles", &row[..3].iter().map(|v| f64::from(*v)).collect::<Vec<_>>(), 1.0, 1.0, [1.0, 0.0, 0.0, 0.5]);
    }
    let circles = img(&e, &r, "circles", "drawing");
    assert_eq!((circles.c, circles.space.as_deref()), (3, Some("RGB")));
    assert_eq!(circles.data, want.data.chunks(4).flat_map(|p| p[..3].to_vec()).collect::<Vec<_>>());
    let gl = img(&e, &r, "grey_lines", "drawing");
    assert_eq!((gl.c, gl.space.as_deref()), (1, None));
    assert!(gl.data.iter().any(|v| *v > 0.9) && gl.data.contains(&0.25), "white lines on grey");
    let pc = img(&e, &r, "percolor", "drawing");
    assert_ne!(pc.data, circles.data);
    let aa = img(&e, &r, "aa", "drawing");
    assert!(aa.data.iter().any(|v| *v > 0.5 && *v <= 1.0));
    let comp = img(&e, &r, "comp", "composite");
    // 1 at alpha 0.5 × opacity 0.5 over 0.25: 0.25 + 0.25 × 0.75; rows -2..4 land on 0..=3
    assert_eq!((comp.data[0], comp.data[29], comp.data[3 * 30 + 29], comp.data[4 * 30 + 29], comp.data[24]), (0.25, 0.4375, 0.4375, 0.25, 0.25));
    let comp_rgb = img(&e, &r, "comp_rgb", "composite");
    assert_eq!((comp_rgb.data[0], comp_rgb.data[(30 + 1) * 3]), (0.5, 1.0));
    let mp = img(&e, &r, "mp", "points");
    assert!(mp.table && mp.c == 2);
    // errors: too few columns, a colour count mismatch, a 2-channel canvas, a bad colour, a
    // Lab canvas converted
    let bad = |shape: &str, extra: &str, canvas_channels: i64, color: &str| {
        let nodes = json!({
            "c": {"kind": "array.constant", "values": {"width": 10, "height": 10, "channels": canvas_channels}},
            "p": {"kind": "array.random_points", "values": {"count": 3, "extra": extra}},
            "d": {"kind": "array.draw_shapes", "values": {"shape": shape, "color": color}},
        });
        let r = run(&e, nodes, &[("c", "constant", "p", "like"), ("c", "constant", "d", "canvas"), ("p", "points", "d", "shapes")], &["d"]);
        error(&r, "d")
    };
    assert!(bad("triangles", "1 2", 1, "#ffffff").contains("need 6 columns"));
    assert!(bad("circles", "3", 2, "#ffffff").contains("no RGB form"));
    assert!(bad("circles", "3", 1, "red").contains("#rrggbb"));
    let nodes = json!({
        "c": {"kind": "array.constant", "values": {"width": 10, "height": 10}},
        "p": {"kind": "array.random_points", "values": {"count": 3, "extra": "2"}},
        "q": {"kind": "array.random_points", "values": {"count": 4, "extra": "1"}},
        "d": {"kind": "array.draw_shapes", "values": {"shape": "circles"}},
    });
    let r = run(
        &e,
        nodes,
        &[
            ("c", "constant", "p", "like"),
            ("c", "constant", "q", "like"),
            ("c", "constant", "d", "canvas"),
            ("p", "points", "d", "shapes"),
            ("q", "points", "d", "colors"),
        ],
        &["d"],
    );
    assert!(error(&r, "d").contains("3 shapes but 4 colours"));
    let lab = json!({
        "c": {"kind": "array.constant", "values": {"width": 6, "height": 6, "channels": 3, "value": 50.0}},
        "s": {"kind": "array.stack", "values": {"space": "Lab"}},
        "p": {"kind": "array.random_points", "values": {"count": 0, "extra": "1 2"}},
        "d": {"kind": "array.draw_shapes"},
    });
    let r = ok(run(
        &e,
        lab,
        &[("c", "constant", "s", "c1"), ("c", "constant", "p", "like"), ("s", "stack", "d", "canvas"), ("p", "points", "d", "shapes")],
        &["d"],
    ));
    let d = img(&e, &r, "d", "drawing");
    assert_eq!(d.space.as_deref(), Some("RGB"), "a Lab canvas comes back as RGB, with no shapes drawn");
}

#[test]
fn frequency_through_the_executor() {
    let dir = root("gen-fft");
    let e = engine(&dir);
    let nodes = json!({
        "c": {"kind": "array.constant", "values": {"width": 24, "height": 18, "channels": 2}},
        "n": {"kind": "array.simplex_noise", "values": {"seed": 9}},
        "n2": {"kind": "array.simplex_noise", "values": {"seed": 10, "kind": "turbulence"}},
        "s": {"kind": "array.stack"},
        "f": {"kind": "array.fft"},
        "i": {"kind": "array.inverse_fft"},
    });
    let wires = [
        ("c", "constant", "n", "like"),
        ("c", "constant", "n2", "like"),
        ("n", "noise", "s", "c1"),
        ("n2", "noise", "s", "c2"),
        ("s", "stack", "f", "array"),
        ("f", "magnitude", "i", "magnitude"),
        ("f", "phase", "i", "phase"),
    ];
    let r = ok(run(&e, nodes, &wires, &["s", "f", "i"]));
    let s = img(&e, &r, "s", "stack");
    let m = img(&e, &r, "f", "magnitude");
    assert_eq!((m.h, m.w, m.c), (18, 24, 2));
    let dc = m.plane(0)[9 * 24 + 12];
    let sum: f64 = s.plane(0).iter().map(|v| f64::from(*v)).sum();
    assert!((f64::from(dc) - sum).abs() < 1e-3, "zero frequency at the centre is the sum");
    let log = img(&e, &r, "f", "log_magnitude");
    assert!(close(&log.data, &m.data.iter().map(|v| v.ln_1p()).collect::<Vec<_>>(), 1e-5));
    let back = img(&e, &r, "i", "array");
    assert!(close(&back.data, &s.data, 2e-6), "back where it started");
    // mismatched shapes
    let nodes = json!({"a": {"kind": "array.constant", "values": {"width": 8, "height": 8}}, "b": {"kind": "array.constant", "values": {"width": 9, "height": 8}}, "i": {"kind": "array.inverse_fft"}});
    let r = run(&e, nodes, &[("a", "constant", "i", "magnitude"), ("b", "constant", "i", "phase")], &["i"]);
    assert!(error(&r, "i").contains("same shape"));
}

// ---------- hostile input ----------

#[test]
fn hostile_inputs_are_errors_or_harmless() {
    let dir = root("gen-hostile");
    let e = engine(&dir);
    // out-of-range and junk parameters are clamped; unknown enums are errors
    let nodes = json!({
        "n": {"kind": "array.simplex_noise", "values": {"width": -5, "height": 1e12, "cells": -1, "octaves": 1000, "seed": -3, "persistence": "x"}},
        "k": {"kind": "array.simplex_noise", "values": {"width": 8, "height": 8, "kind": "plaid"}},
        "m": {"kind": "array.mandelbrot", "values": {"width": 8, "height": 9, "zoom": 0, "max_iterations": 1e9, "center_x": 1e300}},
        "l": {"kind": "array.game_of_life", "values": {"steps": -4}},
        "c": {"kind": "array.constant", "values": {"width": 9, "height": 7, "value": f64::NAN}},
        "p": {"kind": "array.random_points", "values": {"count": -1, "extra": "1 nan inf"}},
        "q": {"kind": "array.random_points", "values": {"extra": "one"}},
        "g": {"kind": "array.grid_segments", "values": {"tile_width": 0, "tile_height": 1e9}},
    });
    let r = run(
        &e,
        nodes,
        &[("c", "constant", "l", "start"), ("c", "constant", "p", "like"), ("c", "constant", "q", "like"), ("c", "constant", "g", "like")],
        &["n", "k", "m", "l", "p", "q", "g"],
    );
    assert!(error(&r, "k").contains("plaid"));
    assert!(error(&r, "q").contains("not a number"));
    for id in ["n", "m", "l", "p", "g"] {
        assert!(matches!(r.nodes[id].state, plumcraft_proto::event::NodeState::Done), "{id}: {:?}", r.nodes[id].error);
    }
    let n = img(&e, &r, "n", "noise");
    assert_eq!((n.h, n.w), (16384, 8), "clamped sizes");
    // kernels on NaN, Inf and junk tables: no panics
    let nan = vec![f32::NAN, f32::INFINITY, -f32::INFINITY, 0.7, 0.2, f32::NAN];
    let _ = regions::connected_components_of(&nan, 2, 3);
    let _ = regions::outlines_of(&nan, 2, 3);
    let _ = regions::outlines_of(&[], 0, 0);
    let _ = regions::connected_components_of(&[], 0, 0);
    let huge = [[5, i32::MIN as i64, i32::MIN as i64, i32::MAX as i64, i32::MAX as i64]];
    assert!(regions::segment_average_of(&nan, 2, 3, &huge, false, 1.0).is_err(), "a rectangle of 2^64 points");
    let odd = [[0, -3, -3, 8, 8], [2, 100, -100, -7, 4], [-4, 2, 2, 1, 1]];
    let _ = regions::segment_average_of(&nan, 2, 3, &odd, true, 1.0).unwrap();
    let _ = regions::segment_average_of(&nan, 2, 3, &odd, false, 0.0).unwrap();
    let mut canvas = draw::Rgba::from_rgb(4, 5, &[vec![0.5; 20], vec![0.5; 20], vec![0.5; 20]]);
    for shape in ["lines", "rects", "circles", "ellipses", "triangles", "mesh_triangles"] {
        for row in [
            [f64::NAN; 6],
            [f64::INFINITY, 0.0, 1.0, 2.0, 3.0, 4.0],
            [-1e300, 1e300, 1e300, -1e300, 0.0, 1e300],
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [2.0, 2.0, -3.0, 1e9, 2.0, 2.0],
        ] {
            draw::draw_shape(&mut canvas, shape, &row, 1e9, 1e9, [1.0, f32::NAN, 0.0, 0.5]);
            draw::draw_shape(&mut canvas, shape, &row[..2], 0.1, 0.0, [1.0, 1.0, 0.0, 0.5]);
        }
    }
    let block = draw::Rgba::from_rgb(3, 3, &[vec![1.0; 9], vec![1.0; 9], vec![1.0; 9]]);
    for (x, y) in [(i64::MIN, i64::MAX), (i64::MAX, 0), (-2, -2), (4, 3)] {
        draw::composite_block(&mut canvas, &block, x, y);
    }
    let mut plane = vec![0.0f32; 20];
    for l in [[i32::MIN as i64, i32::MAX as i64, i32::MAX as i64, i32::MIN as i64], [0, 0, 0, 0], [-5, 2, 9, 2], [3, -100000, 3, 100000]] {
        draw::bresenham(&mut plane, 4, 5, l[0], l[1], l[2], l[3], 1.0);
    }
    assert!(plane.iter().filter(|v| **v == 1.0).count() >= 9, "the clipped lines are drawn");
    draw::antialiased_lines(&mut plane, 4, 5, &[[f32::MAX, -f32::MAX, -f32::MAX, f32::MAX], [1.0, 1.0, 1.0, 1.0], [f32::NAN, 0.0, 1.0, 1.0]]);
    draw::bresenham(&mut [], 0, 0, 0, 0, 5, 5, 1.0);
    assert!(fft::fft_plane(&[], 0, 0, &never).is_some());
    let [m, _, _] = fft::fft_plane(&[f32::NAN, 1.0, f32::INFINITY], 1, 3, &never).unwrap();
    assert_eq!(m.len(), 3);
    assert_eq!(noise::life_step(&[], 0, 0), Vec::<u8>::new());
    assert!(draw::hex_rgb("#12345").is_err() && draw::hex_rgb("#gggggg").is_err() && draw::hex_rgb("ÿÿÿ").is_err());
    assert_eq!(points::mask_points_of(&nan, 0, 1.0, 0), vec![0.0, 1.0, 0.0, 3.0], "w 0 is treated as 1; Inf is on, NaN off");
    // a table where a picture belongs, and a stack where a channel belongs
    let nodes = json!({
        "c": {"kind": "array.constant", "values": {"width": 6, "height": 6, "channels": 3}},
        "p": {"kind": "array.random_points", "values": {"count": 4}},
        "f": {"kind": "array.fft"},
        "cc": {"kind": "array.connected_components"},
        "mp": {"kind": "array.mask_points"},
        "dl": {"kind": "array.draw_lines"},
        "co": {"kind": "array.composite"},
    });
    let wires = [
        ("c", "constant", "p", "like"),
        ("p", "points", "f", "array"),
        ("c", "constant", "cc", "edges"),
        ("c", "constant", "mp", "mask"),
        ("c", "constant", "dl", "canvas"),
        ("p", "points", "dl", "lines"),
        ("c", "constant", "co", "canvas"),
        ("p", "points", "co", "block"),
    ];
    let r = run(&e, nodes, &wires, &["f", "cc", "mp", "dl", "co"]);
    assert!(error(&r, "f").contains("table"));
    for id in ["cc", "mp", "dl"] {
        assert!(error(&r, id).contains("one channel"), "{id}");
    }
    assert!(error(&r, "co").contains("table"));
}
