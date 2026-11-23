//! Tone curves: impressionist's `core.curves` (what the research software's Apply Curve computes with, and
//! its curve editor draws), in the same arithmetic and order.
//!
//! A curve set is openrv-web's ColorCurvesData, as the research software stores it:
//! `{"master": {"points": [{"x": 0, "y": 0}, {"x": 1, "y": 1}], "enabled": true}, "red": …,
//! "green": …, "blue": …}`, points in 0..1 (a point may also be `[x, y]`). ComfyUI's older
//! widget value `{"points": [[x, y], …]}` is the master curve. Each channel takes the master
//! curve, then its own (red, green, blue for the first three channels).
//!
//! The model (parsing, evaluation, editing) is the shared one, `plumcraft_proto::curves`; this
//! module applies it to an array's planes.

use plumcraft_proto::curves::apply_curve as curve;
pub use plumcraft_proto::curves::{CHANNEL_KEYS, Curve, MAX_POINTS, default_json, parse};

/// `curve_array` for channels: each on 0..1 (clipped, or mapped from its own min..max with
/// `own_range`, float32 as numpy does it), through the master curve and then channel `k`'s
/// (red, green, blue for k = 0, 1, 2), and with `own_range` back to its own min..max.
pub fn curve_planes(planes: &[Vec<f32>], curves: &[Curve; 4], own_range: bool) -> Vec<Vec<f32>> {
    let [master, red, green, blue] = curves;
    let own = [red, green, blue];
    planes
        .iter()
        .enumerate()
        .map(|(k, plane)| {
            let (unit, span) = if own_range {
                let (lo, hi) = super::kernels::nan_min_max(plane);
                let d = hi - lo;
                let d = if d == 0.0 { 1.0f32 } else { d as f32 };
                let lo = lo as f32;
                (plane.iter().map(|&v| (v - lo) / d).collect::<Vec<f32>>(), Some((lo, d)))
            } else {
                (plane.iter().map(|&v| if v.is_nan() { v } else { v.clamp(0.0, 1.0) }).collect(), None)
            };
            let mut y: Vec<f64> = unit.iter().map(|&v| f64::from(v)).collect();
            curve(&mut y, master);
            if let Some(c) = own.get(k) {
                curve(&mut y, c);
            }
            let y32 = y.iter().map(|&v| v as f32);
            match span {
                Some((lo, d)) => y32.map(|v| lo + v * d).collect(),
                None => y32.collect(),
            }
        })
        .collect()
}
