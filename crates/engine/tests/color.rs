//! The colour-space port (`plumcraft_engine::color`) against impressionist itself: fixtures from
//! `claude-debug/color_fixtures_gen.py`, run in the research software's Python (OpenCV 4.14, yapocis on its
//! default OpenCL backend). For every space the fixture holds `from_rgb(rgb)` and
//! `to_rgb(from_rgb(rgb))`; the inverse here is fed the fixture's forward planes, so each
//! direction is checked on its own.

use plumcraft_engine::color::{SPACES, from_rgb, to_rgb};
use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/color.json");

fn planes(v: &Value) -> Vec<Vec<f32>> {
    v.as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_f64().map_or(f32::NAN, |x| x as f32)).collect()).collect()
}

/// Per-space tolerance: |got - want| <= abs + rel * |want|, per channel; `wrap` is the hue
/// period of channel 0 (distance measured around the circle), 0 for none; channel 0 is not
/// compared where the reference's channel 1 (saturation) is below `grey`.
struct Tol {
    abs: [f32; 3],
    rel: f32,
    wrap: f32,
    grey: f32,
}

/// Forward tolerances, and why:
/// - matrix and OpenCV-formula spaces: 1e-5 relative plus a few f32 ulps absolute.
/// - HSI, IHSL: the reference runs on the GPU (OpenCL, which may contract to fma, and whose
///   acospi/sinpi/cospi and division are not correctly rounded), so a few more ulps.
/// - IHSL hue of greys and near-greys (saturation < 1e-3) is not compared: there the kernel's
///   hue is 0/0 or a catastrophic cancellation (`r^2+g^2+b^2 - (rg+rb+gb)`), and yapocis builds
///   it with `-cl-fast-relaxed-math -cl-finite-math-only`, so the GPU's answer (0.5 for most
///   exact greys, 0.25 for some, anything for near-greys) depends on how its compiler contracted
///   the sum. The port gives 0.5 for every exact grey, the kernel's NaN -> clamp -> -1 path.
///   Above that the same cancellation leaves the hue good to about 1e-7 / saturation^2 (f32
///   ulps of the sums, over the squared denominator), which the tolerance adds.
/// - Lab: the forward direction is OpenCV's fixed-point table, exact by construction; the
///   tolerance allows one quantisation step (L 100/16384, a/b 1/64) for a table entry rounded the
///   other way (OpenCV builds it in soft-float).
fn forward_tol(space: &str) -> Tol {
    match space {
        "HSI" => Tol { abs: [1e-6, 1e-6, 1e-6], rel: 1e-5, wrap: 1.0, grey: 0.0 },
        "IHSL" => Tol { abs: [2e-6, 1e-6, 1e-6], rel: 1e-5, wrap: 1.0, grey: 1e-3 },
        "HSV" => Tol { abs: [1e-4, 1e-6, 1e-6], rel: 1e-5, wrap: 360.0, grey: 0.0 },
        "HLS" => Tol { abs: [1e-4, 1e-6, 1e-6], rel: 1e-5, wrap: 360.0, grey: 0.0 },
        "Lab" => Tol { abs: [0.0062, 0.0157, 0.0157], rel: 0.0, wrap: 0.0, grey: 0.0 },
        "Luv" => Tol { abs: [1e-4, 1e-4, 1e-4], rel: 1e-5, wrap: 0.0, grey: 0.0 },
        _ => Tol { abs: [1e-6, 1e-6, 1e-6], rel: 1e-5, wrap: 0.0, grey: 0.0 },
    }
}

/// Inverse tolerances: 1e-5 relative, and absolute a few f32 ulps of the matrix terms. Near
/// black, Luv/XYZ/xyY's linear RGB is a cancellation of terms of size ~1 (ulp ~1e-7, rounded
/// differently by OpenCV's SIMD or the GPU's contracted multiply-adds) that the sRGB encode
/// then multiplies by 12.92, hence 5e-6 there.
fn inverse_tol(space: &str) -> Tol {
    match space {
        "HSI" | "IHSL" | "Lab" => Tol { abs: [2e-6; 3], rel: 1e-5, wrap: 0.0, grey: 0.0 },
        "Luv" | "XYZ" | "xyY" => Tol { abs: [5e-6; 3], rel: 1e-5, wrap: 0.0, grey: 0.0 },
        _ => Tol { abs: [1e-6; 3], rel: 1e-5, wrap: 0.0, grey: 0.0 },
    }
}

/// Largest error over all samples as a multiple of the tolerance (<= 1 passes), the largest
/// absolute error, and where it was.
fn compare(got: &[Vec<f32>], want: &[Vec<f32>], tol: &Tol) -> (f32, f32, String) {
    let (mut worst, mut max_abs, mut at) = (0.0f32, 0.0f32, String::new());
    for (c, (g, w)) in got.iter().zip(want).enumerate() {
        assert_eq!(g.len(), w.len());
        for (i, (&g, &w)) in g.iter().zip(w).enumerate() {
            if w.is_nan() || g.is_nan() {
                assert!(w.is_nan() && g.is_nan(), "channel {c} sample {i}: got {g}, want {w}");
                continue;
            }
            if c == 0 && want.get(1).is_some_and(|s| s[i] < tol.grey) {
                continue;
            }
            let mut e = (g - w).abs();
            if c == 0 && tol.wrap > 0.0 {
                e = e.min(tol.wrap - e);
            }
            let mut allowed = tol.abs[c] + tol.rel * w.abs();
            if c == 0 && tol.grey > 0.0 {
                // The hue's error grows as 1/saturation^2 (see forward_tol).
                allowed += 1e-7 / want[1][i].powi(2);
            }
            let ratio = e / allowed;
            max_abs = max_abs.max(e);
            if ratio > worst {
                worst = ratio;
                at = format!("channel {c} sample {i}: got {g}, want {w}");
            }
        }
    }
    (worst, max_abs, at)
}

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}

#[test]
fn matches_impressionist() {
    let f = fixture();
    let rgb = planes(&f["rgb"]);
    let mut failures = Vec::new();
    for &space in SPACES {
        let case = &f["spaces"][space];
        assert!(case.is_object(), "fixture has no {space}");
        let want_fwd = planes(&case["from_rgb"]);
        let want_back = planes(&case["to_rgb"]);
        let fwd = from_rgb(space, &rgb).unwrap();
        let back = to_rgb(space, &want_fwd).unwrap();
        let (wf, af, atf) = compare(&fwd, &want_fwd, &forward_tol(space));
        let (wb, ab, atb) = compare(&back, &want_back, &inverse_tol(space));
        println!("{space:6} from_rgb max abs {af:.3e} ({wf:.2} of tol)   to_rgb max abs {ab:.3e} ({wb:.2} of tol)");
        if wf > 1.0 {
            failures.push(format!("{space} from_rgb {atf}"));
        }
        if wb > 1.0 {
            failures.push(format!("{space} to_rgb {atb}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn every_space_listed_is_known() {
    let p = vec![vec![0.2f32, 0.9], vec![0.4, 0.1], vec![0.6, 0.5]];
    assert_eq!(SPACES.len(), 10);
    for &space in SPACES {
        assert!(from_rgb(space, &p).is_some(), "{space}");
        assert!(to_rgb(space, &p).is_some(), "{space}");
    }
}

#[test]
fn unknown_space_is_none() {
    let p = vec![vec![0.5f32], vec![0.5], vec![0.5]];
    for space in ["", "rgb", "CMYK", "lab", "HSL"] {
        assert!(from_rgb(space, &p).is_none(), "{space}");
        assert!(to_rgb(space, &p).is_none(), "{space}");
    }
}

#[test]
fn unequal_or_wrong_count_planes_are_none() {
    let bad = [vec![vec![0.5f32, 0.5], vec![0.5], vec![0.5, 0.5]], vec![vec![0.5f32], vec![0.5]], vec![vec![0.5f32], vec![0.5], vec![0.5], vec![0.5]], vec![]];
    for p in &bad {
        for &space in SPACES {
            assert!(from_rgb(space, p).is_none(), "{space} {p:?}");
            assert!(to_rgb(space, p).is_none(), "{space} {p:?}");
        }
    }
    // Empty planes of one length are fine.
    let empty = vec![Vec::<f32>::new(); 3];
    assert_eq!(from_rgb("Lab", &empty), Some(vec![Vec::new(); 3]));
}

/// RGB -> space -> RGB recovers in-gamut colours. Bounds per space: f32 rounding for the exact
/// inverses; Lab's forward table quantises (L to 100/16384, a and b to 1/64), Luv goes through
/// OpenCV's splines, IHSL's inverse uses the kernel's four-digit matrix, and HSI's saturation
/// is divided by intensity plus 1e-10 (which dominates for very dark colours).
#[test]
fn round_trip_recovers_in_gamut() {
    let f = fixture();
    let rgb = planes(&f["rgb"]);
    // In-gamut samples only, and not the darkest (black has no xyY chromaticity).
    let keep: Vec<usize> = (0..rgb[0].len()).filter(|&i| rgb.iter().all(|p| (0.0..=1.0).contains(&p[i])) && rgb.iter().any(|p| p[i] > 0.01)).collect();
    let rgb: Vec<Vec<f32>> = rgb.iter().map(|p| keep.iter().map(|&i| p[i]).collect()).collect();
    for &space in SPACES {
        let bound = match space {
            "Lab" => 2e-3,
            // OpenCV's YCrCb coefficients are rounded (0.713 vs 1.403, ...), not exact inverses.
            "YCrCb" => 5e-4,
            "IHSL" => 2e-3,
            "Luv" => 1e-4,
            "HSI" => 1e-5,
            // colormath's seven-digit matrices are inverses to ~1e-7, plus the f32 pow pair.
            "XYZ" | "xyY" => 5e-6,
            _ => 2e-6,
        };
        let back = to_rgb(space, &from_rgb(space, &rgb).unwrap()).unwrap();
        let mut worst = 0.0f32;
        for (b, a) in back.iter().zip(&rgb) {
            for (b, a) in b.iter().zip(a) {
                worst = worst.max((b - a).abs());
            }
        }
        println!("{space:6} round trip max abs {worst:.3e}");
        assert!(worst <= bound, "{space}: round trip off by {worst}");
    }
}
