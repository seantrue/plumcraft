//! Resize: virtual_impressionist's `core.resize` (the target size, alignment to 4, nearest for
//! integer arrays, `keep_range`) with Pillow's float32 (`F` mode) resampling
//! (`libImaging/Resample.c`), adapted from yapocis-rs's `lib/pil.rs`: the filter's coefficients
//! in float64 normalised per output pixel, a horizontal pass over only the rows the vertical
//! pass reads, then the vertical pass, summing in float64. The vertical sum uses fused
//! multiply-adds, as clang compiles Pillow on arm64 (`f64::mul_add` is exact IEEE fused, so the
//! result is still the same on every machine).

/// A resampling filter for float channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Bilinear,
    Lanczos,
}

impl Filter {
    fn support(self) -> f64 {
        match self {
            Filter::Bilinear => 1.0,
            Filter::Lanczos => 3.0,
        }
    }

    fn weight(self, x: f64) -> f64 {
        match self {
            Filter::Bilinear => {
                let x = x.abs();
                if x < 1.0 { 1.0 - x } else { 0.0 }
            }
            Filter::Lanczos => {
                if (-3.0..3.0).contains(&x) {
                    sinc(x) * sinc(x / 3.0)
                } else {
                    0.0
                }
            }
        }
    }
}

/// Pillow's `sinc_filter`.
fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let x = x * std::f64::consts::PI;
    x.sin() / x
}

/// One output pixel's first input pixel and weights.
type Coeffs = Vec<(usize, Vec<f64>)>;

/// Pillow's `precompute_coeffs`.
fn coeffs(in_size: usize, out_size: usize, filter: Filter) -> Coeffs {
    let scale = in_size as f64 / out_size as f64;
    let filterscale = scale.max(1.0);
    let support = filter.support() * filterscale;
    let ss = 1.0 / filterscale;
    (0..out_size)
        .map(|xx| {
            let center = (xx as f64 + 0.5) * scale;
            // (int) truncates toward zero
            let xmin = ((center - support + 0.5) as i64).max(0) as usize;
            let xmax = ((center + support + 0.5) as i64).min(in_size as i64).max(0) as usize;
            let mut k: Vec<f64> = (xmin..xmax.max(xmin)).map(|x| filter.weight((x as f64 - center + 0.5) * ss)).collect();
            let mut ww = 0.0;
            for w in &k {
                ww += w;
            }
            if ww != 0.0 {
                for w in k.iter_mut() {
                    *w /= ww;
                }
            }
            (xmin, k)
        })
        .collect()
}

/// `Image.fromarray(plane, "F").resize((width, height), filter)`: a plane (h × w) resampled to
/// out_h × out_w.
pub fn resample(p: &[f32], h: usize, w: usize, out_h: usize, out_w: usize, filter: Filter) -> Vec<f32> {
    if (h, w) == (out_h, out_w) {
        return p.to_vec();
    }
    if h == 0 || w == 0 || out_h == 0 || out_w == 0 {
        return vec![0.0; out_h * out_w];
    }
    let px = |x: usize, y: usize| f64::from(p.get(y * w + x).copied().unwrap_or(0.0));
    let hc = coeffs(w, out_w, filter);
    let vc = coeffs(h, out_h, filter);
    let first = vc.first().map(|b| b.0).unwrap_or(0);
    let last = vc.last().map(|b| b.0 + b.1.len()).unwrap_or(h);
    // the horizontal pass: rows first..last (or the plane as is)
    let (offset, temp_h, temp) = if out_w != w {
        let rows = last.saturating_sub(first);
        let mut temp = vec![0.0f32; rows * out_w];
        for y in 0..rows {
            for (xx, (xmin, k)) in hc.iter().enumerate() {
                let mut ss = 0.0f64;
                for (x, wt) in k.iter().enumerate() {
                    ss += px(x + xmin, y + first) * wt;
                }
                if let Some(t) = temp.get_mut(y * out_w + xx) {
                    *t = ss as f32;
                }
            }
        }
        (first, rows, temp)
    } else {
        (0, h, p.to_vec())
    };
    if out_h == h {
        return temp;
    }
    let tv = |x: usize, y: usize| f64::from(if y < temp_h { temp.get(y * out_w + x).copied().unwrap_or(0.0) } else { 0.0 });
    let mut out = vec![0.0f32; out_h * out_w];
    for (yy, (ymin, k)) in vc.iter().enumerate() {
        let ymin = ymin.saturating_sub(offset);
        for xx in 0..out_w {
            let mut ss = 0.0f64;
            for (y, wt) in k.iter().enumerate() {
                ss = tv(xx, y + ymin).mul_add(*wt, ss);
            }
            if let Some(o) = out.get_mut(yy * out_w + xx) {
                *o = ss as f32;
            }
        }
    }
    out
}

/// `_nearest`: output pixel centres mapped back to the input's (float64, truncated), for a
/// stack of `c` values a pixel.
pub fn nearest(data: &[f32], h: usize, w: usize, c: usize, out_h: usize, out_w: usize) -> Vec<f32> {
    let index = |i: usize, n_in: usize, n_out: usize| (((i as f64 + 0.5) * (n_in as f64 / n_out as f64)) as usize).min(n_in.saturating_sub(1));
    let rows: Vec<usize> = (0..out_h).map(|i| index(i, h, out_h)).collect();
    let cols: Vec<usize> = (0..out_w).map(|i| index(i, w, out_w)).collect();
    let mut out = Vec::with_capacity(out_h * out_w * c);
    for r in &rows {
        for col in &cols {
            for k in 0..c {
                out.push(data.get((r * w + col) * c + k).copied().unwrap_or(0.0));
            }
        }
    }
    out
}

/// `_scaled`: `side * factor` rounded half up, at least 1.
fn scaled(side: usize, factor: f64) -> usize {
    let v = side as f64 * factor + 0.5;
    if v.is_finite() && v >= 1.0 { v.min(1e9) as usize } else { 1 }
}

/// How the target size is given.
pub struct Target {
    pub by: String,
    pub dim: i64,
    pub width: i64,
    pub height: i64,
    pub scale: f64,
    pub shrink_only: bool,
}

/// `target_size`: the (height, width) an h × w array resamples to, before alignment.
pub fn target_size(h: usize, w: usize, t: &Target) -> Result<(usize, usize), String> {
    if h < 1 || w < 1 {
        return Ok((h, w));
    }
    let size = match t.by.as_str() {
        "maxdim" | "mindim" => {
            if t.dim < 1 {
                return Ok((h, w));
            }
            let dim = t.dim as usize;
            let maxdim = t.by == "maxdim";
            let side = if maxdim { h.max(w) } else { h.min(w) };
            let factor = dim as f64 / side as f64;
            if if maxdim { h >= w } else { h <= w } { (dim, scaled(w, factor)) } else { (scaled(h, factor), dim) }
        }
        "size" => {
            let (tw, th) = (t.width, t.height);
            if tw < 1 && th < 1 {
                return Ok((h, w));
            }
            if tw < 1 {
                (th as usize, scaled(w, th as f64 / h as f64))
            } else if th < 1 {
                (scaled(h, tw as f64 / w as f64), tw as usize)
            } else {
                (th as usize, tw as usize)
            }
        }
        "scale" => {
            if t.scale.is_nan() || t.scale <= 0.0 {
                return Ok((h, w));
            }
            (scaled(h, t.scale), scaled(w, t.scale))
        }
        other => return Err(format!("resize by {other:?}: expected maxdim, mindim, size or scale")),
    };
    if t.shrink_only && (size.0 > h || size.1 > w) {
        return Ok((h, w));
    }
    Ok(size)
}

/// The multiple an aligned result's sides are.
pub const ALIGNMENT: usize = 4;
