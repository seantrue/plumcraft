//! FFT and Inverse FFT. yapocis's numba `fft1d` is numpy's `np.fft.fft` / `ifft` on each line,
//! for any length; here each line is transformed in float64 (radix 2 for powers of two,
//! Bluestein's chirp-z through radix 2 otherwise), rounded to float32 between the row and column
//! passes as the complex64 lines are there.

use std::f64::consts::PI;

use plumcraft_engine::{Cx, Data, Inputs, Outputs};
use plumcraft_proto::error::NodeError;

use super::cancelled;
use crate::img::Img;
use crate::op::{config, img, many, one};

/// An in-place radix-2 transform of a power-of-two length (`sign` -1 forward, +1 backward,
/// unscaled).
fn radix2(re: &mut [f64], im: &mut [f64], sign: f64) {
    let n = re.len().min(im.len());
    if n < 2 {
        return;
    }
    // bit reversal
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let twiddles: Vec<(f64, f64)> = (0..half).map(|k| (sign * 2.0 * PI * k as f64 / len as f64).sin_cos()).map(|(s, c)| (c, s)).collect();
        for start in (0..n).step_by(len) {
            for (k, (wr, wi)) in twiddles.iter().enumerate() {
                let (a, b) = (start + k, start + k + half);
                let (Some(&br), Some(&bi)) = (re.get(b), im.get(b)) else { continue };
                let tr = br * wr - bi * wi;
                let ti = br * wi + bi * wr;
                let (Some(&ar), Some(&ai)) = (re.get(a), im.get(a)) else { continue };
                for (slot, v) in [(a, ar + tr), (b, ar - tr)] {
                    if let Some(x) = re.get_mut(slot) {
                        *x = v;
                    }
                }
                for (slot, v) in [(a, ai + ti), (b, ai - ti)] {
                    if let Some(y) = im.get_mut(slot) {
                        *y = v;
                    }
                }
            }
        }
        len <<= 1;
    }
}

/// The DFT of one line, any length (`sign` -1 forward, +1 backward, unscaled).
fn dft(re: &mut [f64], im: &mut [f64], sign: f64) {
    let n = re.len().min(im.len());
    if n < 2 {
        return;
    }
    if n.is_power_of_two() {
        radix2(re, im, sign);
        return;
    }
    // Bluestein: X_k = c_k Σ_j (x_j c_j) conj(c_{k-j}), c_k = exp(sign·iπ k²/n)
    let m = (2 * n - 1).next_power_of_two();
    let chirp: Vec<(f64, f64)> = (0..n)
        .map(|k| {
            let k2 = (k as u128 * k as u128) % (2 * n as u128);
            let (s, c) = (sign * PI * k2 as f64 / n as f64).sin_cos();
            (c, s)
        })
        .collect();
    let (mut ar, mut ai) = (vec![0.0f64; m], vec![0.0f64; m]);
    let (mut br, mut bi) = (vec![0.0f64; m], vec![0.0f64; m]);
    for (k, (c, s)) in chirp.iter().enumerate() {
        let (x, y) = (re.get(k).copied().unwrap_or(0.0), im.get(k).copied().unwrap_or(0.0));
        if let (Some(p), Some(q)) = (ar.get_mut(k), ai.get_mut(k)) {
            *p = x * c - y * s;
            *q = x * s + y * c;
        }
        for idx in [k, (m - k) % m] {
            if let (Some(p), Some(q)) = (br.get_mut(idx), bi.get_mut(idx)) {
                *p = *c;
                *q = -*s;
            }
        }
    }
    radix2(&mut ar, &mut ai, -1.0);
    radix2(&mut br, &mut bi, -1.0);
    for k in 0..m {
        let (Some(&xr), Some(&xi), Some(&yr), Some(&yi)) = (ar.get(k), ai.get(k), br.get(k), bi.get(k)) else { continue };
        if let (Some(p), Some(q)) = (ar.get_mut(k), ai.get_mut(k)) {
            *p = xr * yr - xi * yi;
            *q = xr * yi + xi * yr;
        }
    }
    radix2(&mut ar, &mut ai, 1.0);
    let scale = 1.0 / m as f64;
    for (k, (c, s)) in chirp.iter().enumerate() {
        let (x, y) = (ar.get(k).copied().unwrap_or(0.0) * scale, ai.get(k).copied().unwrap_or(0.0) * scale);
        if let (Some(p), Some(q)) = (re.get_mut(k), im.get_mut(k)) {
            *p = x * c - y * s;
            *q = x * s + y * c;
        }
    }
}

/// A complex plane, float32 as complex64 holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Complex {
    pub h: usize,
    pub w: usize,
    pub re: Vec<f32>,
    pub im: Vec<f32>,
}

/// Transform every row (`rows`) or every column, `np.fft.fft` (forward) or `ifft` (scaled by
/// 1/n), each line rounded back to float32.
fn lines(p: &mut Complex, rows: bool, forward: bool, cancel: &dyn Fn() -> bool) -> Option<()> {
    let (count, len) = if rows { (p.h, p.w) } else { (p.w, p.h) };
    let index = |line: usize, k: usize| if rows { line * p.w + k } else { k * p.w + line };
    let (mut re, mut im) = (vec![0.0f64; len], vec![0.0f64; len]);
    let sign = if forward { -1.0 } else { 1.0 };
    let scale = if forward { 1.0 } else { 1.0 / len.max(1) as f64 };
    for line in 0..count {
        if line % 64 == 0 && cancel() {
            return None;
        }
        for k in 0..len {
            let at = index(line, k);
            if let (Some(r), Some(i)) = (re.get_mut(k), im.get_mut(k)) {
                *r = f64::from(p.re.get(at).copied().unwrap_or(0.0));
                *i = f64::from(p.im.get(at).copied().unwrap_or(0.0));
            }
        }
        dft(&mut re, &mut im, sign);
        for k in 0..len {
            let at = index(line, k);
            if let (Some(r), Some(i)) = (p.re.get_mut(at), p.im.get_mut(at)) {
                *r = (re.get(k).copied().unwrap_or(0.0) * scale) as f32;
                *i = (im.get(k).copied().unwrap_or(0.0) * scale) as f32;
            }
        }
    }
    Some(())
}

/// The research software's `fft2`: rows, then columns.
pub fn fft2(p: &mut Complex, forward: bool, cancel: &dyn Fn() -> bool) -> Option<()> {
    lines(p, true, forward, cancel)?;
    lines(p, false, forward, cancel)
}

/// `np.fft.fftshift` (`inverse` false) or `ifftshift` of an `h` × `w` plane (npexact); a plane
/// whose size doesn't match comes back unchanged.
pub fn shift(v: &[f32], h: usize, w: usize, inverse: bool) -> Vec<f32> {
    let shifted = if inverse { npexact::fft::ifftshift_2d(v, h, w) } else { npexact::fft::fftshift_2d(v, h, w) };
    shifted.unwrap_or_else(|| v.to_vec())
}

/// FFT of one plane: magnitude, phase and log(1 + magnitude), zero frequency centred.
pub fn fft_plane(p: &[f32], h: usize, w: usize, cancel: &dyn Fn() -> bool) -> Option<[Vec<f32>; 3]> {
    let mut c = Complex { h, w, re: p.to_vec(), im: vec![0.0; p.len()] };
    fft2(&mut c, true, cancel)?;
    let (re, im) = (shift(&c.re, h, w, false), shift(&c.im, h, w, false));
    let magnitude: Vec<f32> = re.iter().zip(&im).map(|(a, b)| f64::from(*a).hypot(f64::from(*b)) as f32).collect();
    let phase = re.iter().zip(&im).map(|(a, b)| f64::from(*b).atan2(f64::from(*a)) as f32).collect();
    let log = magnitude.iter().map(|m| f64::from(*m).ln_1p() as f32).collect();
    Some([magnitude, phase, log])
}

/// The real part of the inverse transform of magnitude·exp(i·phase), zero frequency centred.
pub fn inverse_plane(magnitude: &[f32], phase: &[f32], h: usize, w: usize, cancel: &dyn Fn() -> bool) -> Option<Vec<f32>> {
    let re: Vec<f32> = magnitude.iter().zip(phase).map(|(m, p)| m * (f64::from(*p).cos() as f32)).collect();
    let im: Vec<f32> = magnitude.iter().zip(phase).map(|(m, p)| m * (f64::from(*p).sin() as f32)).collect();
    let mut c = Complex { h, w, re: shift(&re, h, w, true), im: shift(&im, h, w, true) };
    fft2(&mut c, false, cancel)?;
    Some(c.re)
}

pub(super) fn fft(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    if a.table {
        return Err(config("array is a table, not a picture"));
    }
    Img::check_size(a.h, a.w, a.c).map_err(config)?;
    let (mut m, mut ph, mut log) = (Vec::new(), Vec::new(), Vec::new());
    for (k, p) in a.planes().iter().enumerate() {
        let [pm, pph, plog] = fft_plane(p, a.h, a.w, &|| cx.cancelled()).ok_or_else(cancelled)?;
        m.push(pm);
        ph.push(pph);
        log.push(plog);
        cx.progress((k + 1) as f64 / a.c as f64, None);
    }
    let arr = |planes: &[Vec<f32>]| Data::Array(Img::from_planes(a.h, a.w, planes).to_array());
    Ok(many(vec![("magnitude", arr(&m)), ("phase", arr(&ph)), ("log_magnitude", arr(&log))]))
}

pub(super) fn inverse_fft(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let (m, p) = (img(i, "magnitude")?, img(i, "phase")?);
    if m.table || p.table {
        return Err(config("magnitude and phase are pictures, not tables"));
    }
    if (m.h, m.w, m.c) != (p.h, p.w, p.c) {
        return Err(config(format!("magnitude ({}) and phase ({}) must be the same shape", m.describe(), p.describe())));
    }
    Img::check_size(m.h, m.w, m.c).map_err(config)?;
    let mut out = Vec::new();
    for (k, (mp, pp)) in m.planes().iter().zip(p.planes()).enumerate() {
        out.push(inverse_plane(mp, &pp, m.h, m.w, &|| cx.cancelled()).ok_or_else(cancelled)?);
        cx.progress((k + 1) as f64 / m.c as f64, None);
    }
    Ok(one("array", Img::from_planes(m.h, m.w, &out)))
}
