//! The filters' arithmetic on one plane (H × W values, C order), ported from yapocis's numba
//! kernels (`yapocis3/src/yapocis/kernels/numbax`) as plain loops: no fast-math, so no
//! reassociation and no fused multiply-adds, and every machine gets the same bits. The types
//! are numba's: where a kernel accumulates in float64 (a Python float meeting a float32), so
//! does the port. Every index is checked (`get`), so no input can make a kernel panic.

/// A value of a plane, or 0 outside it (never reached on valid indices).
#[inline]
fn at(p: &[f32], i: usize) -> f32 {
    p.get(i).copied().unwrap_or(0.0)
}

/// `p` (h × w) transposed to w × h.
pub fn transpose(p: &[f32], h: usize, w: usize) -> Vec<f32> {
    let mut out = vec![0.0; h * w];
    for y in 0..h {
        for x in 0..w {
            if let Some(o) = out.get_mut(x * h + y) {
                *o = at(p, y * w + x);
            }
        }
    }
    out
}

// ---------- convolvesep (Gaussian Blur) ----------

/// `_conv1d_h_njit`: a 1-D kernel stepping a row at a time over the flattened plane, in
/// float32; a flat index within `r` rows of either end is 0 (strictly inside, as numba
/// tests `r * w < i < n - r * w`).
fn conv_rows_apart(p: &[f32], h: usize, w: usize, taps: &[f32]) -> Vec<f32> {
    let n = h * w;
    let r = taps.len() / 2;
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        if r * w < i && i + r * w < n {
            let mut acc = 0.0f32;
            for (t, k) in taps.iter().enumerate() {
                // i - r*w + t*w: in range by the test above
                acc += at(p, i - r * w + t * w) * *k;
            }
            *o = acc;
        }
    }
    out
}

/// yapocis `Gaussian.gauss_image` on the numba backend: `gauss<n>_h` (down the columns) then
/// `gauss<n>_v` (along the rows, the same kernel on the transpose). Both passes zero their
/// edges, so the result has a 0 frame half the kernel wide.
pub fn gauss_image(p: &[f32], h: usize, w: usize, weights: &[f64]) -> Vec<f32> {
    let taps: Vec<f32> = weights.iter().map(|v| *v as f32).collect();
    let b = conv_rows_apart(p, h, w, &taps);
    let bt = transpose(&b, h, w);
    let ot = conv_rows_apart(&bt, w, h, &taps);
    transpose(&ot, w, h)
}

// ---------- median3x3 ----------

/// `_median3x3_njit`: the median of each interior pixel's 3 × 3 neighbourhood by insertion
/// sort (NaN compares false, as there); border pixels are copied.
pub fn median3x3(p: &[f32], h: usize, w: usize) -> Vec<f32> {
    let mut out = p.to_vec();
    if h < 3 || w < 3 {
        return out;
    }
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let mut tmp = [0.0f32; 9];
            let mut k = 0;
            for yy in y - 1..=y + 1 {
                for xx in x - 1..=x + 1 {
                    if let Some(t) = tmp.get_mut(k) {
                        *t = at(p, yy * w + xx);
                    }
                    k += 1;
                }
            }
            for i in 1..9 {
                let v = tmp[i];
                let mut j = i;
                while j > 0 && tmp[j - 1] > v {
                    tmp[j] = tmp[j - 1];
                    j -= 1;
                }
                tmp[j] = v;
            }
            if let Some(o) = out.get_mut(y * w + x) {
                *o = tmp[4];
            }
        }
    }
    out
}

/// `median3x3fast`: `iterations` passes (0 = a copy).
pub fn median(p: &[f32], h: usize, w: usize, iterations: usize) -> Vec<f32> {
    let mut a = p.to_vec();
    for _ in 0..iterations {
        a = median3x3(&a, h, w);
    }
    a
}

// ---------- convolve (Convolve, Custom Convolve) ----------

/// `_reflect_index`, repeated until the index lands inside (numba reflects once and reads
/// past the ends when the kernel is longer than the array; here it keeps reflecting).
fn reflect(i: i64, n: i64) -> usize {
    let mut i = i;
    for _ in 0..64 {
        if i < 0 {
            i = -i - 1;
        } else if i >= n {
            i = 2 * n - i - 1;
        } else {
            return i as usize;
        }
    }
    i.clamp(0, (n - 1).max(0)) as usize
}

/// `_conv1d_njit` over a flattened array: reflect-padded ("symmetric": index -1 reads 0), the
/// products in float32 and their sum in float64, stored as float32.
pub fn conv1d(a: &[f32], k: &[f32]) -> Vec<f32> {
    let n = a.len() as i64;
    let r = (k.len() / 2) as i64;
    let mut out = vec![0.0f32; a.len()];
    for (i, o) in out.iter_mut().enumerate() {
        let mut s = 0.0f64;
        for (t, kt) in k.iter().enumerate() {
            let ai = reflect(i as i64 + t as i64 - r, n);
            s += f64::from(at(a, ai) * *kt);
        }
        *o = s as f32;
    }
    out
}

/// The research software's `convolve_rows`: the kernel along rows (horizontal: the flattened plane), columns
/// (vertical: the flattened transpose), or rows then columns. It runs over the flattened
/// array, so it carries on from one row's end into the next row's start.
pub fn convolve_rows(p: &[f32], h: usize, w: usize, k: &[f32], direction: &str) -> Vec<f32> {
    let mut plane = p.to_vec();
    if direction == "horizontal" || direction == "both" {
        plane = conv1d(&plane, k);
    }
    if direction == "vertical" || direction == "both" {
        let t = conv1d(&transpose(&plane, h, w), k);
        plane = transpose(&t, w, h);
    }
    plane
}

// ---------- anisodiff (Perona–Malik) ----------

/// `_pm_step2d_njit`: one Perona–Malik step. Differences in float32, the conductance and the
/// sum in float64 (`kappa` and `step` are Python floats), the border copied.
pub fn pm_step(p: &[f32], h: usize, w: usize, kappa: f64, step: f64) -> Vec<f32> {
    let mut out = p.to_vec();
    if h < 3 || w < 3 {
        return out;
    }
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let c = at(p, y * w + x);
            let n = at(p, (y + 1) * w + x);
            let s = at(p, (y - 1) * w + x);
            let e = at(p, y * w + x - 1);
            let wv = at(p, y * w + x + 1);
            let mut acc = 0.0f64;
            for d in [n - c, s - c, e - c, wv - c] {
                let t = f64::from(d) / kappa;
                acc += f64::from(d) * (1.0 / (1.0 + t * t));
            }
            if let Some(o) = out.get_mut(y * w + x) {
                *o = (f64::from(c) + acc * step) as f32;
            }
        }
    }
    out
}

/// The largest value, NaN when there is one (numpy's `max`).
pub fn nan_max(p: &[f32]) -> f32 {
    let mut m = f32::NEG_INFINITY;
    for v in p {
        if v.is_nan() {
            return f32::NAN;
        }
        if *v > m {
            m = *v;
        }
    }
    m
}

/// The smallest value, NaN when there is one (numpy's `min`).
pub fn nan_min(p: &[f32]) -> f32 {
    let mut m = f32::INFINITY;
    for v in p {
        if v.is_nan() {
            return f32::NAN;
        }
        if *v < m {
            m = *v;
        }
    }
    m
}

/// yapocis `Anisodiff.anisodiff_n` (`filterImageN`) of one plane: scaled by 256 unless its
/// maximum is above 1, `n` steps, divided by 256 (whether or not it was scaled, as there).
pub fn anisodiff_n(p: &[f32], h: usize, w: usize, n: usize, kappa: f64, step: f64) -> Vec<f32> {
    let mut a: Vec<f32> = if nan_max(p) > 1.0 { p.to_vec() } else { p.iter().map(|v| v * 256.0).collect() };
    for _ in 0..n {
        a = pm_step(&a, h, w, kappa, step);
    }
    a.iter().map(|v| v / 256.0).collect()
}

/// virtual_impressionist's `flow.perona_malik`: the plane mapped to 0-1 (float32, as numpy
/// computes with Python-float scalars), optionally raised into a ridge `boost` high on `edges`
/// (> 0.5) with the content and kappa scaled by 1 - boost, diffused, the ridge removed, and
/// mapped back. `edges` is the edge plane and the boost.
pub fn perona_malik(p: &[f32], h: usize, w: usize, iterations: usize, kappa: f64, step: f64, edges: Option<(&[f32], f64)>) -> Vec<f32> {
    let (lo, hi) = (f64::from(nan_min(p)), f64::from(nan_max(p)));
    let span = if hi - lo == 0.0 { 1.0 } else { hi - lo };
    let (lo32, span32) = (lo as f32, span as f32);
    let unit: Vec<f32> = p.iter().map(|v| (v - lo32) / span32).collect();
    let result = match edges {
        Some((edges, boost)) if boost > 0.0 => {
            let keep = (1.0 - boost) as f32;
            let b32 = boost as f32;
            let mask: Vec<f32> = (0..unit.len()).map(|i| if at(edges, i) > 0.5 { 1.0 } else { 0.0 }).collect();
            let ridged: Vec<f32> = unit.iter().zip(&mask).map(|(u, m)| u * keep + b32 * m).collect();
            let out = anisodiff_n(&ridged, h, w, iterations, kappa * (1.0 - boost), step);
            out.iter().zip(&mask).map(|(r, m)| (r - b32 * m) / keep).collect()
        }
        _ => anisodiff_n(&unit, h, w, iterations, kappa, step),
    };
    result.iter().map(|v| v * span32 + lo32).collect()
}

// ---------- gradient ----------

/// `_gradient_njit`: central differences `reach` apart (float32), magnitude by `sqrt` and
/// angle by `atan2` in float32, the angle in multiples of π (float64: -atan2/π + 0.5, wrapped
/// into -1..1, -1 mapped to 1); 0 within `reach` of the border, and NaN results are 0.
pub fn gradient(p: &[f32], h: usize, w: usize, reach: usize) -> (Vec<f32>, Vec<f32>) {
    let mut grad = vec![0.0f32; h * w];
    let mut theta = vec![0.0f32; h * w];
    for y in reach..h.saturating_sub(reach) {
        for x in reach..w.saturating_sub(reach) {
            let gx = at(p, y * w + x + reach) - at(p, y * w + x - reach);
            let gy = at(p, (y + reach) * w + x) - at(p, (y - reach) * w + x);
            let g = (gx * gx + gy * gy).sqrt();
            let mut th = if gx == 0.0 && gy == 0.0 { 0.0f64 } else { (-f64::from(gy.atan2(gx)) / std::f64::consts::PI) + 0.5 };
            if th > 1.0 {
                th -= 2.0;
            } else if th < -1.0 {
                th += 2.0;
            }
            let i = y * w + x;
            if let Some(o) = grad.get_mut(i) {
                *o = if g.is_nan() { 0.0 } else { g };
            }
            if let Some(o) = theta.get_mut(i) {
                *o = if th.is_nan() {
                    0.0
                } else if th < -0.9999995 {
                    1.0
                } else {
                    th as f32
                };
            }
        }
    }
    (grad, theta)
}

// ---------- zcs ----------

/// `np.sign` of a float32: -1, 0 or 1, NaN for NaN.
fn sign(v: f32) -> f32 {
    if v.is_nan() {
        f32::NAN
    } else if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// yapocis `zcs1` (numba backend, numpy): 1 where the squared differences across the pixel,
/// (below - above)² + (right - left)² in float32, exceed `threshold` (a float32 under NumPy 2's
/// rules) and the sign differs between opposite neighbours; the plane's edges repeat.
pub fn zcs(p: &[f32], h: usize, w: usize, threshold: f32) -> Vec<f32> {
    let mut out = vec![0.0f32; h * w];
    for y in 0..h {
        for x in 0..w {
            let below = at(p, (y + 1).min(h - 1) * w + x);
            let above = at(p, y.saturating_sub(1) * w + x);
            let left = at(p, y * w + x.saturating_sub(1));
            let right = at(p, y * w + (x + 1).min(w - 1));
            let (dv, dh) = (below - above, right - left);
            let delta = dv * dv + dh * dh;
            let crosses = sign(below) != sign(above) || sign(left) != sign(right);
            if delta > threshold
                && crosses
                && let Some(o) = out.get_mut(y * w + x)
            {
                *o = 1.0;
            }
        }
    }
    out
}

// ---------- Sobel Magnitude ----------

/// yapocis_external's `gradient_magnitude`: unnormalised 3 × 3 Sobel, neighbours clamped at
/// the border, in float32 evaluated left to right.
pub fn sobel(p: &[f32], h: usize, w: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; h * w];
    for y in 0..h {
        let (ym, yp) = (y.saturating_sub(1), (y + 1).min(h - 1));
        for x in 0..w {
            let (xm, xp) = (x.saturating_sub(1), (x + 1).min(w - 1));
            let v = |xx: usize, yy: usize| at(p, yy * w + xx);
            let (p00, p10, p20) = (v(xm, ym), v(x, ym), v(xp, ym));
            let (p01, p21) = (v(xm, y), v(xp, y));
            let (p02, p12, p22) = (v(xm, yp), v(x, yp), v(xp, yp));
            let gx = -p00 + p20 - 2.0 * p01 + 2.0 * p21 - p02 + p22;
            let gy = -p00 - 2.0 * p10 - p20 + p02 + 2.0 * p12 + p22;
            if let Some(o) = out.get_mut(y * w + x) {
                *o = (gx * gx + gy * gy).sqrt();
            }
        }
    }
    out
}

// ---------- Segment Edges ----------

/// virtual_impressionist's `flow.segment_edges` of a stack (C values a pixel): 1 where a
/// pixel differs by more than `tolerance` in any channel from the pixel to its right or below
/// (float64), and on the border with `frame`.
pub fn segment_edges(data: &[f32], h: usize, w: usize, c: usize, tolerance: f64, frame: bool) -> Vec<f32> {
    let mut out = vec![0.0f32; h * w];
    let differs = |i: usize, j: usize| (0..c).any(|k| (f64::from(at(data, i * c + k)) - f64::from(at(data, j * c + k))).abs() > tolerance);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let edge = (x + 1 < w && differs(i, i + 1)) || (y + 1 < h && differs(i, i + w));
            let border = frame && (y == 0 || x == 0 || y + 1 == h || x + 1 == w);
            if (edge || border)
                && let Some(o) = out.get_mut(i)
            {
                *o = 1.0;
            }
        }
    }
    out
}

// ---------- Rotate ----------

/// The size a rotated h × w array grows to with `expand` (the research software's Rotate).
pub fn rotated_size(h: usize, w: usize, degrees: f64) -> (f64, f64) {
    let t = degrees.to_radians();
    let (hf, wf) = (h as f64, w as f64);
    (((hf * t.cos()).abs() + (wf * t.sin()).abs() - 1e-6).ceil(), ((wf * t.cos()).abs() + (hf * t.sin()).abs() - 1e-6).ceil())
}

/// yapocis_external's `rotateImage`: each destination pixel's offset from the destination
/// centre (integer halves) is rotated (float32) and added to the source centre, and the
/// nearest source pixel copied; pixels that map outside keep `background`.
pub fn rotate(p: &[f32], h: usize, w: usize, out_h: usize, out_w: usize, degrees: f64, background: f32) -> Vec<f32> {
    let t = degrees.to_radians();
    let (s, c) = (t.sin() as f32, t.cos() as f32);
    let mut out = vec![background; out_h * out_w];
    let (dh2, dw2) = ((out_h / 2) as i64, (out_w / 2) as i64);
    let (sh2, sw2) = ((h / 2) as f32, (w / 2) as f32);
    for r in 0..out_h {
        for col in 0..out_w {
            let x = (col as i64 - dw2) as f32;
            let y = (r as i64 - dh2) as f32;
            let xpos = c * x - s * y + sw2;
            let ypos = s * x + c * y + sh2;
            let sc = (xpos + 0.5).floor() as i64;
            let sr = (ypos + 0.5).floor() as i64;
            if sr >= 0
                && (sr as usize) < h
                && sc >= 0
                && (sc as usize) < w
                && let Some(o) = out.get_mut(r * out_w + col)
            {
                *o = at(p, sr as usize * w + sc as usize);
            }
        }
    }
    out
}
