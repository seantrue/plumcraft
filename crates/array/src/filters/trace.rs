//! Trace Contours: yapocis truetrace's CPU path (`yapocis.lib.truetrace`): the signed distance
//! field of a mask by Felzenszwalb's exact Euclidean distance transform (`edt_1d`, `edt_2d`,
//! `sdf_from_mask_cpu`, float64, stored as float32), the iso moved off the field's values
//! (`off_field_iso`), marching squares in float32 (`marching_squares_cpu`), and the segments
//! sorted (`sort_segments`).

/// `edt_1d`: the squared distance transform of a sampled function (Felzenszwalb & Huttenlocher).
fn edt_1d(f: &[f64]) -> Vec<f64> {
    let n = f.len();
    if n == 0 {
        return Vec::new();
    }
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let fv = |i: usize| f.get(i).copied().unwrap_or(f64::INFINITY);
    let parabola = |q: usize, p: usize| ((fv(q) + (q * q) as f64) - (fv(p) + (p * p) as f64)) / (2.0 * (q as f64 - p as f64));
    let mut k = 0usize;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        let mut s = parabola(q, v[k]);
        while k > 0 && s <= z[k] {
            k -= 1;
            s = parabola(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        if let Some(zz) = z.get_mut(k + 1) {
            *zz = f64::INFINITY;
        }
    }
    let mut k = 0usize;
    let mut g = vec![0.0f64; n];
    for (q, gq) in g.iter_mut().enumerate() {
        while z.get(k + 1).is_some_and(|zk| *zk < q as f64) {
            k += 1;
        }
        let vk = v.get(k).copied().unwrap_or(0);
        let dq = q as f64 - vk as f64;
        *gq = dq * dq + fv(vk);
    }
    g
}

/// `edt_2d`: the distance from each pixel to the nearest `true` one (1e6 when there is none:
/// sqrt of edt's 1e12 stand-in for infinity).
fn edt_2d(mask: &[bool], h: usize, w: usize) -> Vec<f64> {
    let f: Vec<f64> = mask.iter().map(|m| if *m { 0.0 } else { 1e12 }).collect();
    let mut tmp = vec![0.0f64; h * w];
    for y in 0..h {
        let row = f.get(y * w..(y + 1) * w).unwrap_or(&[]);
        for (x, v) in edt_1d(row).into_iter().enumerate() {
            if let Some(t) = tmp.get_mut(y * w + x) {
                *t = v;
            }
        }
    }
    let mut out = vec![0.0f64; h * w];
    for x in 0..w {
        let col: Vec<f64> = (0..h).map(|y| tmp.get(y * w + x).copied().unwrap_or(0.0)).collect();
        for (y, v) in edt_1d(&col).into_iter().enumerate() {
            if let Some(o) = out.get_mut(y * w + x) {
                *o = v.sqrt();
            }
        }
    }
    out
}

/// `sdf_from_mask_cpu`: positive inside (the distance to the background), negative outside.
pub fn sdf(mask: &[bool], h: usize, w: usize) -> Vec<f32> {
    let d_true = edt_2d(mask, h, w);
    let inverse: Vec<bool> = mask.iter().map(|m| !m).collect();
    let d_false = edt_2d(&inverse, h, w);
    d_false.iter().zip(&d_true).map(|(a, b)| (a - b) as f32).collect()
}

/// `off_field_iso`: `iso` raised in float32 steps of 1e-3 until no value of the field equals
/// it (bounded: a field has finitely many values).
pub fn off_field_iso(dist: &[f32], iso: f32) -> f32 {
    let mut iso = iso;
    for _ in 0..=dist.len() {
        if !dist.contains(&iso) {
            break;
        }
        iso += 1e-3f32;
    }
    iso
}

/// `marching_squares_cpu`: the segments (x0, y0, x1, y1) where the field crosses `iso`, in
/// float32 as numpy computes it (NumPy 2 keeps Python floats weak), in scan order. Stops at
/// `max_segments`, returning `None`.
pub fn marching_squares(dist: &[f32], h: usize, w: usize, iso: f32, max_segments: usize) -> Option<Vec<[f32; 4]>> {
    let mut segs: Vec<[f32; 4]> = Vec::new();
    let d = |x: usize, y: usize| dist.get(y * w + x).copied().unwrap_or(0.0);
    for y in 0..h.saturating_sub(1) {
        for x in 0..w.saturating_sub(1) {
            let v00 = d(x, y) - iso;
            let v10 = d(x + 1, y) - iso;
            let v11 = d(x + 1, y + 1) - iso;
            let v01 = d(x, y + 1) - iso;
            let mut c = 0u8;
            if v00 > 0.0 {
                c |= 1;
            }
            if v10 > 0.0 {
                c |= 2;
            }
            if v11 > 0.0 {
                c |= 4;
            }
            if v01 > 0.0 {
                c |= 8;
            }
            if c == 0 || c == 15 {
                continue;
            }
            let eps = 1e-20f32;
            let lerp = |t: f32, a: f32, b: f32| a + t * (b - a);
            let (xf, yf) = (x as f32, y as f32);
            let t0 = v00 / (v00 - v10 + eps);
            let t1 = v10 / (v10 - v11 + eps);
            let t2 = v01 / (v01 - v11 + eps);
            let t3 = v00 / (v00 - v01 + eps);
            let p = [(lerp(t0, xf, xf + 1.0), yf), (xf + 1.0, lerp(t1, yf, yf + 1.0)), (lerp(t2, xf, xf + 1.0), yf + 1.0), (xf, lerp(t3, yf, yf + 1.0))];
            let mut emit = |a: usize, b: usize| {
                if let (Some(pa), Some(pb)) = (p.get(a), p.get(b)) {
                    segs.push([pa.0, pa.1, pb.0, pb.1]);
                }
            };
            match c {
                1 | 14 => emit(3, 0),
                2 | 13 => emit(0, 1),
                3 | 12 => emit(3, 1),
                4 | 11 => emit(1, 2),
                6 | 9 => emit(0, 2),
                7 | 8 => emit(3, 2),
                _ => {
                    let det = v00 * v11 - v10 * v01;
                    if (c == 5) == (det > 0.0) {
                        emit(3, 0);
                        emit(1, 2);
                    } else {
                        emit(0, 1);
                        emit(2, 3);
                    }
                }
            }
            if segs.len() > max_segments {
                return None;
            }
        }
    }
    Some(segs)
}

/// `sort_segments`: by x0, then y0, x1, y1 (`np.lexsort((y1, x1, y0, x0))`, npexact: stable, NaN
/// last).
pub fn sort_segments(segs: &mut [[f32; 4]]) {
    let key = |k: usize| -> Vec<f32> { segs.iter().map(|s| s[k]).collect() };
    let (x0, y0, x1, y1) = (key(0), key(1), key(2), key(3));
    if let Some(order) = npexact::order::lexsort_f32(&[&y1, &x1, &y0, &x0]) {
        let sorted: Vec<[f32; 4]> = order.iter().filter_map(|&i| segs.get(i).copied()).collect();
        if sorted.len() == segs.len() {
            segs.copy_from_slice(&sorted);
        }
    }
}

/// The outline `offset` pixels outside a mask (nonzero = inside), as sorted segments; `None`
/// when there are more than `max_segments`.
pub fn trace(mask: &[f32], h: usize, w: usize, offset: f32, max_segments: usize) -> Option<Vec<[f32; 4]>> {
    let m: Vec<bool> = mask.iter().map(|v| *v != 0.0).collect();
    let field = sdf(&m, h, w);
    let iso = off_field_iso(&field, -offset);
    let mut segs = marching_squares(&field, h, w, iso, max_segments)?;
    sort_segments(&mut segs);
    Some(segs)
}
