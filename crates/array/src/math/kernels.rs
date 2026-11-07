//! The Math group's arithmetic, one plane (or one flat array) at a time.
//!
//! The elementwise kernels are yapocis's numba kernels (`kernels/numbax/operators.py`,
//! `imageops.py`) as plain loops: float32 data, the Python-float parameters in float64 as numba
//! types them, no fused multiply-add and no reassociation (numba compiled them with
//! `fastmath`, so its results can differ in the last bit). histeq and histmatch are yapocis's
//! host code (`lib/imageops.py`, after yapocis-rs's port). Wherever they call numpy, the numpy
//! behaviour is npexact's (`linspace`, `interp`, `histogram`, `cumsum`, `searchsorted`, `min`,
//! `max`, `nanmin`, `nanmax`, `nanpercentile`, `nan_to_num`, `minimum`, `maximum`, `mod`),
//! proven there by fixtures numpy recorded on arm64 and x86-64. Nothing here panics: sizes are
//! checked by the caller and every index is read with `get`.

use npexact::elementwise::{maximum_f32, minimum_f32, mod_f32, nan_to_num_f32};
use npexact::extrema::{cumsum_i64, max_f32, min_f32, nanmax_f32, nanmin_f32};
use npexact::hist::{histogram_density_f32, histogram_range_f32, interp, linspace_f64};
use npexact::order::{nanpercentile_linear_f32, searchsorted_left_f64};
use npexact::random::Contraction;

/// The operations Arithmetic offers: the research software's four (yapocis operators) and the numpy steps it
/// also covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arith {
    Add,
    Subtract,
    Multiply,
    Divide,
    SubtractFrom,
    Minimum,
    Maximum,
}

impl Arith {
    pub const NAMES: [&'static str; 7] = ["add", "subtract", "multiply", "divide", "subtract from", "minimum", "maximum"];

    pub fn parse(name: &str) -> Option<Arith> {
        Some(match name {
            "add" => Arith::Add,
            "subtract" => Arith::Subtract,
            "multiply" => Arith::Multiply,
            "divide" => Arith::Divide,
            "subtract from" => Arith::SubtractFrom,
            "minimum" => Arith::Minimum,
            "maximum" => Arith::Maximum,
            _ => return None,
        })
    }

    /// One element. yapocis's `_op_njit`: float32 `a + b`, `a - b`, `a * b`; divide is
    /// `a / (b + 1e-20)`, which numba types as float64 (the literal is a double) and stores as
    /// float32. minimum and maximum are numpy's (npexact): NaN wins, and a tie between the
    /// signed zeros resolves as numpy's loops do on this CPU.
    pub fn apply(self, a: f32, b: f32) -> f32 {
        match self {
            Arith::Add => a + b,
            Arith::Subtract => a - b,
            Arith::Multiply => a * b,
            Arith::Divide => (f64::from(a) / (f64::from(b) + 1e-20)) as f32,
            Arith::SubtractFrom => b - a,
            Arith::Minimum => minimum_f32(a, b),
            Arith::Maximum => maximum_f32(a, b),
        }
    }
}

/// yapocis `clamp_pixels`: below `low` → low, above `high` → high (compared in float64, as
/// numba compares a float32 with a Python float); NaN stays NaN.
pub fn clamp_pixels(p: &[f32], low: f64, high: f64) -> Vec<f32> {
    p.iter()
        .map(|&v| {
            let x = f64::from(v);
            if x < low {
                low as f32
            } else if x > high {
                high as f32
            } else {
                v
            }
        })
        .collect()
}

/// yapocis `stretch`, as its device kernels compute it in float32: each value clamped to
/// `input_min..input_max`, then mapped linearly onto `low..high` (NaN passes through).
pub fn stretch(p: &[f32], input_min: f64, input_max: f64, low: f64, high: f64) -> Vec<f32> {
    let (lo, hi, low, high) = (input_min as f32, input_max as f32, low as f32, high as f32);
    p.iter()
        .map(|&v| {
            let v = if v < lo { lo } else { v };
            let v = if v > hi { hi } else { v };
            let v = (v - lo) / (hi - lo);
            low + v * (high - low)
        })
        .collect()
}

/// yapocis `binarize`: 1 where `v >= threshold`, else 0 (NaN → 0).
pub fn binarize(p: &[f32], threshold: f64) -> Vec<f32> {
    p.iter().map(|&v| if f64::from(v) >= threshold { 1.0 } else { 0.0 }).collect()
}

/// yapocis `histogram` (the numba kernel): `bins` counts over low..high, bin
/// `int((v - low) * scale)` with `scale = bins / (high - low + 1e-20)`, values outside
/// clamped into the end bins. NaN is left out (numba's `int(NaN)` is the platform's: 0 on
/// arm64, which yapocis's histeq zeroes anyway). `bins` ≥ 1.
pub fn histogram(p: &[f32], bins: usize, low: f64, high: f64) -> Vec<i64> {
    let mut counts = vec![0i64; bins];
    if bins == 0 {
        return counts;
    }
    let scale = bins as f64 / (high - low + 1e-20);
    let last = bins - 1;
    for &v in p {
        if v.is_nan() {
            continue;
        }
        let f = (f64::from(v) - low) * scale;
        // int(): truncation toward zero; `as` saturates infinities
        let b = f as i64;
        let b = if b < 0 { 0 } else { (b as u64).min(last as u64) as usize };
        if let Some(c) = counts.get_mut(b) {
            *c += 1;
        }
    }
    counts
}

/// `np.cumsum` of integer counts (int64, npexact).
fn cumsum(counts: &[i64]) -> Vec<i64> {
    cumsum_i64(counts)
}

/// yapocis `Imageops.histeq(im, nbr_bins)`: the kernel's histogram over 0..1 with bin 0 (black)
/// left out, its cumulative distribution scaled to `(bins - 1) / bins`, and each value
/// interpolated on it (numpy's `interp` over `linspace(0, 1, bins)`). A plane with nothing
/// above bin 0 (yapocis: 0/0, NaN everywhere) comes out as 0. `bins` ≥ 2.
pub fn histeq(p: &[f32], bins: usize) -> Vec<f32> {
    let bins = bins.max(2);
    let mut hist = histogram(p, bins, 0.0, 1.0);
    if let Some(h) = hist.first_mut() {
        *h = 0;
    }
    let cdf = cumsum(&hist);
    let last = cdf.last().copied().unwrap_or(0);
    if last == 0 {
        return p.iter().map(|v| if v.is_nan() { f32::NAN } else { 0.0 }).collect();
    }
    let scale = (bins - 1) as i64;
    let cdf: Vec<f64> = cdf.iter().map(|&c| (scale.saturating_mul(c) as f64 / last as f64) / bins as f64).collect();
    let xp = linspace_f64(0.0, 1.0, bins);
    let x: Vec<f64> = p.iter().map(|&v| f64::from(v)).collect();
    interp(&x, &xp, &cdf, Contraction::Plain).into_iter().map(|v| v as f32).collect()
}

/// numpy's `searchsorted(a, v, side="left")` with numpy's order (NaN after everything; npexact).
fn searchsorted_left(a: &[f64], v: f64) -> usize {
    searchsorted_left_f64(a, v)
}

/// yapocis `Imageops.histmatch(im, target_hist, nbr_bins)`: the source's cumulative
/// distribution (kernel histogram over 0..1, bin 0 left out) looked up in the target's
/// (`searchsorted`, bin 0 left out too), each value interpolated onto the target bins. Degenerate
/// histograms (nothing above bin 0) go through numpy's NaN arithmetic as yapocis's do. `bins` ≥ 2
/// and `target.len() == bins`.
pub fn histmatch(p: &[f32], target: &[f64], bins: usize) -> Vec<f32> {
    let bins = bins.max(2);
    let mut src = histogram(p, bins, 0.0, 1.0);
    if let Some(h) = src.first_mut() {
        *h = 0;
    }
    let src_cdf = cumsum(&src);
    let src_last = src_cdf.last().copied().unwrap_or(0) as f64;
    let src_cdf: Vec<f64> = src_cdf.iter().map(|&c| c as f64 / src_last).collect();
    let mut t: Vec<f64> = target.iter().copied().take(bins).collect();
    t.resize(bins, 0.0);
    if let Some(h) = t.first_mut() {
        *h = 0.0;
    }
    let mut acc = 0.0f64;
    let mut t_cdf: Vec<f64> = t
        .iter()
        .map(|&x| {
            acc += x;
            acc
        })
        .collect();
    let t_last = t_cdf.last().copied().unwrap_or(0.0);
    for v in &mut t_cdf {
        *v /= t_last;
    }
    let xp = linspace_f64(0.0, 1.0, bins);
    let mapped: Vec<f64> = src_cdf.iter().map(|&s| xp.get(searchsorted_left(&t_cdf, s).min(bins - 1)).copied().unwrap_or(1.0)).collect();
    let x: Vec<f64> = p.iter().map(|&v| f64::from(v)).collect();
    interp(&x, &xp, &mapped, Contraction::Plain).into_iter().map(|v| v as f32).collect()
}

/// `np.histogram(t, bins, (0, 1))[0]` as float64 (float32 edges, values outside left out).
pub fn target_histogram(t: &[f32], bins: usize) -> Vec<f64> {
    if bins == 0 {
        return Vec::new();
    }
    histogram_range_f32(t, bins, 0.0, 1.0).0.into_iter().map(|c| c as f64).collect()
}

/// impressionist's `core.utils.histeq(img, nbins)`: numpy's density histogram (bins over the
/// values' own min..max), its cumulative sum over its last value, and each value interpolated
/// on the left bin edges. The values must be finite (numpy refuses a non-finite range).
pub fn histeq_impressionist(p: &[f32], bins: usize) -> Result<Vec<f32>, String> {
    if p.iter().any(|v| !v.is_finite()) {
        return Err("the impressionist method needs finite values (this channel has NaN or infinity)".into());
    }
    if p.is_empty() || bins == 0 {
        return Ok(p.to_vec());
    }
    let (density, edges) = histogram_density_f32(p, bins);
    let mut acc = 0.0f64;
    let cdf: Vec<f64> = density
        .iter()
        .enumerate()
        .map(|(i, &d)| {
            acc = if i == 0 { d } else { acc + d };
            acc
        })
        .collect();
    let last = cdf.last().copied().unwrap_or(1.0);
    let cdf: Vec<f64> = cdf.iter().map(|c| c / last).collect();
    let xp: Vec<f64> = edges.iter().take(bins).map(|&e| f64::from(e)).collect();
    let x: Vec<f64> = p.iter().map(|&v| f64::from(v)).collect();
    Ok(interp(&x, &xp, &cdf, Contraction::Plain).into_iter().map(|v| v as f32).collect())
}

/// The research software's `stretched(plane)`: the finite values' min..max onto 0..1 (NaN → the min first,
/// float32 arithmetic), clipped; all zeros when nothing is finite.
pub fn stretched(p: &[f32]) -> Vec<f32> {
    let finite = p.iter().copied().filter(|v| v.is_finite());
    let (lo, hi) = finite.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
    if lo > hi {
        return vec![0.0; p.len()];
    }
    let span = f64::from(hi) - f64::from(lo);
    let d = if span == 0.0 { 1.0f32 } else { span as f32 };
    p.iter()
        .map(|&v| {
            let v = if v.is_nan() {
                lo
            } else if v == f32::INFINITY {
                f32::MAX
            } else if v == f32::NEG_INFINITY {
                f32::MIN
            } else {
                v
            };
            ((v - lo) / d).clamp(0.0, 1.0)
        })
        .collect()
}

/// numpy's `min` / `max` of float32 values (NaN propagates; npexact); `None` when empty.
fn min_max(p: &[f32]) -> Option<(f32, f32)> {
    Some((min_f32(p)?, max_f32(p)?))
}

/// `np.nan_to_num` of float32 (npexact): NaN → 0, ±inf → the largest finite float32.
pub fn nan_to_num(v: f32) -> f32 {
    nan_to_num_f32(v)
}

/// `np.nan_to_num(yapocis normalize(a))`: `a - a.min()`, then over its max, float32; a constant
/// (0/0) or any NaN gives 0.
pub fn normalize(p: &[f32]) -> Vec<f32> {
    let Some((lo, _)) = min_max(p) else { return Vec::new() };
    let shifted: Vec<f32> = p.iter().map(|&v| v - lo).collect();
    let hi = min_max(&shifted).map_or(f32::NAN, |(_, h)| h);
    shifted.iter().map(|&v| nan_to_num(v / hi)).collect()
}

/// numpy's `nanmin`/`nanmax` of float32 values as Python floats (npexact): NaN when nothing but
/// NaN, or nothing at all.
pub fn nan_min_max(p: &[f32]) -> (f64, f64) {
    let lo = nanmin_f32(p).unwrap_or(f32::NAN);
    let hi = nanmax_f32(p).unwrap_or(f32::NAN);
    (f64::from(lo), f64::from(hi))
}

/// `np.nanpercentile(a, [q1, q2])` for float32 `a` (the linear method, npexact): NaN left out;
/// NaN when nothing but NaN.
pub fn nan_percentiles(p: &[f32], qs: &[f64]) -> Vec<f64> {
    nanpercentile_linear_f32(p, qs)
}

/// numpy's float32 `np.mod(v, turn)` (npexact: fmod, moved to the divisor's sign; an exact zero
/// takes the divisor's sign), then, signed, values above half a turn less one turn.
pub fn wrap_angle(p: &[f32], turn: f64, signed: bool) -> Vec<f32> {
    let t = turn as f32;
    let half = (turn / 2.0) as f32;
    p.iter()
        .map(|&v| {
            let m = mod_f32(v, t);
            if signed && m > half { m - t } else { m }
        })
        .collect()
}

/// `a (m × k) @ b (k × n)`, both row-major, each product summed in float64 in k order and
/// stored as float32. The caller checks the sizes.
pub fn matmul(a: &[f32], b: &[f32], m: usize, k: usize, n: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; m.saturating_mul(n)];
    for i in 0..m {
        for j in 0..n {
            let mut acc = 0.0f64;
            for q in 0..k {
                let x = a.get(i * k + q).copied().unwrap_or(0.0);
                let y = b.get(q * n + j).copied().unwrap_or(0.0);
                acc += f64::from(x) * f64::from(y);
            }
            if let Some(o) = out.get_mut(i * n + j) {
                *o = acc as f32;
            }
        }
    }
    out
}
