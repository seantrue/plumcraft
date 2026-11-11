//! Colour spaces on channel planes, with RGB (0..1 floats) as the hub: a pure-Rust port of
//! impressionist's `virtual_impressionist/colorspaces.py` and everything it calls, so a picture
//! converts here exactly as it does in the research software. A colour space is optional everywhere: an array
//! without one is plain numbers.
//!
//! Spaces and their channels (the Python module's conventions):
//!
//! - `RGB`: red, green, blue, 0..1.
//! - `HSI`: hue, saturation, intensity, 0..1, hue a fraction of a turn. yapocis's `hsi` kernel
//!   (the GLSL "IQ" RGB->HSV), as impressionist's `core.hsi` calls it through `rgb2hsi3` /
//!   `hsi2rgb3`: the forward kernel reads the pixel as `(r, b, g)`, and the inverse's
//!   `(1, 1/3, 2/3)` phase constants undo that swap. Saturation and intensity are clamped to
//!   0..1; hue may reach 1.0 in f32.
//! - `IHSL`: Hanbury's improved HLS from yapocis_external's `ihsl` kernel: hue in turns from the
//!   acos formula, saturation `max - min`, BT.709 luminance. A grey gets hue 0.5 (the kernel's
//!   `clamp(0/0)` is -1, as on the OpenCL path the research software runs). yapocis builds that kernel with
//!   `-cl-fast-relaxed-math -cl-finite-math-only`, so the GPU's hue for greys and near-greys
//!   (a 0/0, or a catastrophic cancellation in `r^2+g^2+b^2 - (rg+rb+gb)`) depends on how its
//!   compiler contracted the sums: mostly 0.5, sometimes 0.25. The port computes the kernel's
//!   source as written; its saturation is ~0 there, so the hue carries no colour. The inverse
//!   uses the kernel's four-digit matrix, so RGB -> IHSL -> RGB is close but not exact (~2e-4).
//! - `HSV`, `HLS`: OpenCV's float conventions (`cv2.cvtColor` on float32): hue in degrees
//!   0..360, the other channels 0..1.
//! - `Lab`: OpenCV's float `RGB2Lab` with sRGB gamma and D65. OpenCV (4.x) computes the forward
//!   direction by trilinear interpolation in a 33^3 fixed-point table (`RGB2LabLUT_s16`, base
//!   2^14), so L is quantised to 100/16384 and a, b to 1/64; that table and interpolation are
//!   ported. Input is clipped to 0..1 first. The inverse is OpenCV's float formula with its
//!   1024-knot spline for the sRGB encode, clipped to 0..1.
//! - `Luv`: OpenCV's float `RGB2Luv` / `Luv2RGB`: input clipped to 0..1, sRGB through OpenCV's
//!   gamma splines, L through its cube-root spline, D65 white.
//! - `YCrCb`: OpenCV's float coefficients (0.299, 0.587, 0.114; Cr 0.713, Cb 0.564) and a 0.5
//!   delta.
//! - `XYZ`: yapocis colormath's `srgb_d65` matrices, with `linearize` (the Python default): RGB
//!   is sRGB-decoded (negatives clipped to 0) before the matrix and encoded after.
//! - `xyY`: colormath's `xyz2xyY` / `xyY2xyz` on that XYZ. As in the kernel (yapocis3 #66),
//!   black and any other exact zero sum (X+Y+Z = 0) have no chromaticity: x and y are 0 there,
//!   Y is kept, and y = 0 comes back as XYZ 0 (RGB black).
//!
//! Arithmetic is f32, as in the kernels and OpenCV; tables OpenCV builds in soft-float are built
//! here in f32/f64 and rounded the same way. [`to_rgb`] does not clip beyond what the reference
//! itself clips.

use std::f64::consts::PI;
use std::sync::OnceLock;

/// The spaces [`to_rgb`] and [`from_rgb`] know.
pub const SPACES: &[&str] = &["RGB", "HSI", "IHSL", "HSV", "HLS", "Lab", "Luv", "YCrCb", "XYZ", "xyY"];

/// Three planes in `space` -> RGB planes (not clipped); `None` for a space this doesn't know or
/// planes that aren't three of one length.
pub fn to_rgb(space: &str, planes: &[Vec<f32>]) -> Option<Vec<Vec<f32>>> {
    three(planes)?;
    let f: fn([f32; 3]) -> [f32; 3] = match space {
        "RGB" => return Some(planes.to_vec()),
        "HSI" => hsi_to_rgb,
        "IHSL" => ihsl_to_rgb,
        "HSV" => hsv_to_rgb,
        "HLS" => hls_to_rgb,
        "Lab" => lab_to_rgb,
        "Luv" => luv_to_rgb,
        "YCrCb" => ycrcb_to_rgb,
        "XYZ" => xyz_to_rgb,
        "xyY" => |p| xyz_to_rgb(xyy_to_xyz(p)),
        _ => return None,
    };
    Some(map(planes, f))
}

/// RGB planes -> the three planes of `space`; `None` as for [`to_rgb`].
pub fn from_rgb(space: &str, planes: &[Vec<f32>]) -> Option<Vec<Vec<f32>>> {
    three(planes)?;
    let f: fn([f32; 3]) -> [f32; 3] = match space {
        "RGB" => return Some(planes.to_vec()),
        "HSI" => rgb_to_hsi,
        "IHSL" => rgb_to_ihsl,
        "HSV" => rgb_to_hsv,
        "HLS" => rgb_to_hls,
        "Lab" => rgb_to_lab,
        "Luv" => rgb_to_luv,
        "YCrCb" => rgb_to_ycrcb,
        "XYZ" => rgb_to_xyz,
        "xyY" => |p| xyz_to_xyy(rgb_to_xyz(p)),
        _ => return None,
    };
    Some(map(planes, f))
}

fn three(planes: &[Vec<f32>]) -> Option<()> {
    let [a, b, c] = planes else { return None };
    (a.len() == b.len() && b.len() == c.len()).then_some(())
}

/// Apply `f` pixel by pixel to three equal-length planes (checked by [`three`]).
fn map(planes: &[Vec<f32>], f: fn([f32; 3]) -> [f32; 3]) -> Vec<Vec<f32>> {
    let [a, b, c] = planes else { return Vec::new() };
    let n = a.len();
    let mut out = vec![Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n)];
    for ((&x, &y), &z) in a.iter().zip(b).zip(c) {
        let v = f([x, y, z]);
        for (o, v) in out.iter_mut().zip(v) {
            o.push(v);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// HSI: yapocis kernels/ocl/hsi.mako, `_rgb2hsi` / `_hsi2rgb`, via rgb2hsi3 / hsi2rgb3.

/// The kernel's `eps` (1e-10), a float on the GPU.
const HSI_EPS: f32 = 1.0e-10;

fn rgb_to_hsi([r, g, b]: [f32; 3]) -> [f32; 3] {
    // rgb2hsi3 builds c = (input[i], input[i+2], input[i+1]) = (r, b, g).
    let c = [r, b, g];
    // p = c.y < c.z ? (c.zy, K.wz) : (c.yz, K.xy), K = (0, -1/3, 2/3, -1)
    let p = if c[1] < c[2] { [c[2], c[1], -1.0, 2.0 / 3.0] } else { [c[1], c[2], 0.0, -1.0 / 3.0] };
    // q = c.x < p.x ? (p.xyw, c.x) : (c.x, p.yzx)
    let q = if c[0] < p[0] { [p[0], p[1], p[3], c[0]] } else { [c[0], p[1], p[2], p[0]] };
    let d = q[0] - q[3].min(q[1]);
    let mut h = q[2] + (q[3] - q[1]) / (6.0 * d + HSI_EPS);
    if h < 0.0 {
        h = -h;
    }
    let s = d / (q[0] + HSI_EPS);
    // clamp(hsi, 0, 1 - eps): 1 - 1e-10 is 1.0 in f32.
    let top = 1.0f32 - HSI_EPS;
    [clamp(h, 0.0, top), clamp(s, 0.0, top), clamp(q[0], 0.0, top)]
}

fn hsi_to_rgb([h, s, i]: [f32; 3]) -> [f32; 3] {
    // p = |fract(h + K.xyz) * 6 - K.www|, K = (1, 1/3, 2/3, 3); c = i * mix(1, clamp(p - 1), s)
    let k = [1.0f32, 1.0 / 3.0, 2.0 / 3.0];
    k.map(|k| {
        let p = (fract(h + k) * 6.0 - 3.0).abs();
        i * mix(1.0, clamp(p - 1.0, 0.0, 1.0), s)
    })
}

/// OpenCL `clamp`: `fmin(fmax(x, lo), hi)`, so NaN becomes `lo`.
fn clamp(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi)
}

/// OpenCL `fract`: `fmin(x - floor(x), 0x1.fffffep-1f)`.
fn fract(x: f32) -> f32 {
    (x - x.floor()).min(f32::from_bits(0x3f7f_ffff))
}

/// OpenCL `mix`: `a + (b - a) * t`.
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// ---------------------------------------------------------------------------------------------
// IHSL: yapocis_external kernels/ocl/ihsl.mako, RGBToIHSL / IHSLToRGB.

fn acospi(x: f32) -> f32 {
    (f64::from(x).acos() / PI) as f32
}

fn sinpi(x: f32) -> f32 {
    (f64::from(x) * PI).sin() as f32
}

fn cospi(x: f32) -> f32 {
    (f64::from(x) * PI).cos() as f32
}

fn rgb_to_ihsl([r, g, b]: [f32; 3]) -> [f32; 3] {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let y = (0.2126f32 * r) + (0.7152f32 * g) + (0.0722f32 * b);
    let mut s = mx - mn;
    let num = r - (0.5 * g) - (0.5 * b);
    let den = (((r * r) + (g * g) + (b * b)) - ((r * g) + (r * b) + (b * g))).sqrt();
    let hp = acospi(clamp(num / den, -1.0, 1.0));
    let mut h = if b > g { 1.0 - 0.5 * hp } else { 0.5 * hp };
    if !h.is_finite() {
        h = 0.0;
        s = 0.0;
    }
    [h, s, y]
}

fn ihsl_to_rgb([h, s, y]: [f32; 3]) -> [f32; 3] {
    if s <= f32::EPSILON {
        return [y, y, y];
    }
    let (c1, c2) = if h.is_finite() {
        let hstar = h - (h * 6.0).floor() / 6.0;
        let chroma = (1.732_050_8_f32 * s) / (2.0 * sinpi(2.0 / 3.0 - 2.0 * hstar));
        (chroma * cospi(2.0 * h), -chroma * sinpi(2.0 * h))
    } else {
        (0.0, 0.0)
    };
    [y + (0.7875f32 * c1) + (0.3714f32 * c2), y + (-0.2125f32 * c1) + (-0.2059f32 * c2), y + (-0.2125f32 * c1) + (0.9488f32 * c2)]
}

// ---------------------------------------------------------------------------------------------
// HSV, HLS, YCrCb: OpenCV color_hsv / color_yuv, float paths (hrange 360).

fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let v = r.max(g).max(b);
    let vmin = r.min(g).min(b);
    let diff = v - vmin;
    let s = diff / (v.abs() + f32::EPSILON);
    let diff = 60.0 / (diff + f32::EPSILON);
    let mut h = if v == r {
        (g - b) * diff
    } else if v == g {
        (b - r) * diff + 120.0
    } else {
        (r - g) * diff + 240.0
    };
    if h < 0.0 {
        h += 360.0;
    }
    [h, s, v]
}

/// OpenCV's sector table: which of `tab[0..4]` is b, g, r in each hue sector.
const SECTORS: [[usize; 3]; 6] = [[1, 3, 0], [1, 0, 2], [3, 0, 1], [0, 2, 1], [0, 1, 3], [2, 1, 0]];

fn sector_rgb(tab: [f32; 4], sector: usize) -> [f32; 3] {
    let [ib, ig, ir] = SECTORS.get(sector).copied().unwrap_or(SECTORS[0]);
    let at = |k: usize| tab.get(k).copied().unwrap_or(0.0);
    [at(ir), at(ig), at(ib)]
}

fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    if s == 0.0 {
        return [v, v, v];
    }
    let mut h = (h * (6.0 / 360.0)) % 6.0;
    if h < 0.0 {
        h += 6.0;
    }
    let mut sector = h.floor();
    h -= sector;
    if !(0.0..6.0).contains(&sector) {
        sector = 0.0;
        h = 0.0;
    }
    let tab = [v, v * (1.0 - s), v * (1.0 - s * h), v * (1.0 - s * (1.0 - h))];
    sector_rgb(tab, sector as usize)
}

fn rgb_to_hls([r, g, b]: [f32; 3]) -> [f32; 3] {
    let vmax = r.max(g).max(b);
    let vmin = r.min(g).min(b);
    let diff = vmax - vmin;
    let l = (vmax + vmin) * 0.5;
    let (mut h, mut s) = (0.0f32, 0.0f32);
    if diff > f32::EPSILON {
        s = if l < 0.5 { diff / (vmax + vmin) } else { diff / (2.0 - vmax - vmin) };
        let diff = 60.0 / diff;
        h = if vmax == r {
            (g - b) * diff
        } else if vmax == g {
            (b - r) * diff + 120.0
        } else {
            (r - g) * diff + 240.0
        };
        if h < 0.0 {
            h += 360.0;
        }
    }
    [h, l, s]
}

fn hls_to_rgb([h, l, s]: [f32; 3]) -> [f32; 3] {
    if s == 0.0 {
        return [l, l, l];
    }
    let p2 = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p1 = 2.0 * l - p2;
    let mut h = h * (6.0 / 360.0);
    if !h.is_finite() {
        h = 0.0;
    }
    let mut h = h % 6.0;
    if h < 0.0 {
        h += 6.0;
    }
    let mut sector = h.floor();
    h -= sector;
    if !(0.0..6.0).contains(&sector) {
        sector = 0.0;
        h = 0.0;
    }
    let tab = [p2, p1, p1 + (p2 - p1) * (1.0 - h), p1 + (p2 - p1) * h];
    sector_rgb(tab, sector as usize)
}

fn rgb_to_ycrcb([r, g, b]: [f32; 3]) -> [f32; 3] {
    let y = r * 0.299 + g * 0.587 + b * 0.114;
    [y, (r - y) * 0.713 + 0.5, (b - y) * 0.564 + 0.5]
}

fn ycrcb_to_rgb([y, cr, cb]: [f32; 3]) -> [f32; 3] {
    let b = y + (cb - 0.5) * 1.773;
    let g = y + (cb - 0.5) * -0.344 + (cr - 0.5) * -0.714;
    let r = y + (cr - 0.5) * 1.403;
    [r, g, b]
}

// ---------------------------------------------------------------------------------------------
// Lab, Luv: OpenCV color_lab.cpp, float paths for sRGB (COLOR_RGB2Lab etc.), D65.

const SRGB2XYZ_D65: [f64; 9] = [0.412453, 0.357580, 0.180423, 0.212671, 0.715160, 0.072169, 0.019334, 0.119193, 0.950227];
const XYZ2SRGB_D65: [f64; 9] = [3.240479, -1.53715, -0.498535, -0.969256, 1.875991, 0.041556, 0.055648, -0.204043, 1.057311];
const D65: [f64; 3] = [0.950456, 1.0, 1.088754];

const GAMMA_TAB_SIZE: usize = 1024;
const LAB_CBRT_TAB_SIZE: usize = 1024;
const LAB_LUT_DIM: usize = 33;
const LAB_BASE: f32 = 16384.0;

/// OpenCV `applyGamma` (sRGB decode), soft-double rounded to float.
fn apply_gamma(x: f32) -> f32 {
    let x = f64::from(x);
    let v = if x <= 809.0 / 20000.0 { x / (323.0 / 25.0) } else { ((x + 11.0 / 200.0) / (1.0 + 11.0 / 200.0)).powf(2.4) };
    v as f32
}

/// OpenCV `applyInvGamma` (sRGB encode).
fn apply_inv_gamma(x: f32) -> f32 {
    let x = f64::from(x);
    let v = if x <= 7827.0 / 2_500_000.0 { x * (323.0 / 25.0) } else { x.powf(1.0 / 2.4) * (1.0 + 11.0 / 200.0) - 11.0 / 200.0 };
    v as f32
}

/// OpenCV `splineBuild` (soft-float, so plain IEEE f32 here): `f` has `n + 1` knots; the table
/// has four coefficients per interval.
fn spline_build(f: &[f32], n: usize) -> Vec<f32> {
    let mut tab = vec![0.0f32; n * 4];
    let at = |v: &[f32], i: usize| v.get(i).copied().unwrap_or(0.0);
    for i in 1..n {
        let t = (at(f, i + 1) - at(f, i) * 2.0 + at(f, i - 1)) * 3.0;
        let l = 1.0 / (4.0 - at(&tab, (i - 1) * 4));
        let m = (t - at(&tab, (i - 1) * 4 + 1)) * l;
        if let Some(cell) = tab.get_mut(i * 4..i * 4 + 2) {
            cell.copy_from_slice(&[l, m]);
        }
    }
    let mut cn = 0.0f32;
    for i in (0..n).rev() {
        let c = at(&tab, i * 4 + 1) - at(&tab, i * 4) * cn;
        let b = at(f, i + 1) - at(f, i) - (cn + c * 2.0) / 3.0;
        let d = (cn - c) / 3.0;
        if let Some(cell) = tab.get_mut(i * 4..i * 4 + 4) {
            cell.copy_from_slice(&[at(f, i), b, c, d]);
        }
        cn = c;
    }
    tab
}

/// OpenCV `splineInterpolate`.
fn spline(x: f32, tab: &[f32], n: usize) -> f32 {
    let ix = (x as i32).clamp(0, n as i32 - 1);
    let x = x - ix as f32;
    let ix = ix as usize * 4;
    match tab.get(ix..ix + 4) {
        Some(&[a, b, c, d]) => ((d * x + c) * x + b) * x + a,
        _ => 0.0,
    }
}

struct Tables {
    gamma: Vec<f32>,
    inv_gamma: Vec<f32>,
    cbrt: Vec<f32>,
    /// RGB2Labprev: (L, a, b) as i16 for every grid point, index `(p + q*33 + r*33*33)*3` with
    /// p, q, r the R, G, B steps.
    lab_lut: Vec<i16>,
}

fn lthresh() -> f32 {
    216.0f32 / 24389.0
}
fn lscale() -> f32 {
    841.0f32 / 108.0
}
fn lbias() -> f32 {
    16.0f32 / 116.0
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let scale = GAMMA_TAB_SIZE as f32;
        let f: Vec<f32> = (0..=GAMMA_TAB_SIZE).map(|i| apply_gamma(i as f32 / scale)).collect();
        let g: Vec<f32> = (0..=GAMMA_TAB_SIZE).map(|i| apply_inv_gamma(i as f32 / scale)).collect();
        let cscale = 1.0f32 / cbrt_tab_scale();
        let c: Vec<f32> = (0..=LAB_CBRT_TAB_SIZE)
            .map(|i| {
                let x = cscale * i as f32;
                if x < lthresh() { (f64::from(x) * f64::from(lscale()) + f64::from(lbias())) as f32 } else { f64::from(x).cbrt() as f32 }
            })
            .collect();
        Tables {
            gamma: spline_build(&f, GAMMA_TAB_SIZE),
            inv_gamma: spline_build(&g, GAMMA_TAB_SIZE),
            cbrt: spline_build(&c, LAB_CBRT_TAB_SIZE),
            lab_lut: lab_lut(),
        }
    })
}

fn cbrt_tab_scale() -> f32 {
    (LAB_CBRT_TAB_SIZE * 2) as f32 / 3.0
}

/// OpenCV `initLabTabs`' RGB2Labprev grid (soft-float there; IEEE f32 here).
fn lab_lut() -> Vec<i16> {
    let s: Vec<f32> = (0..9).map(|k| (SRGB2XYZ_D65[k] / D65[k / 3]) as f32).collect();
    let s = |k: usize| s.get(k).copied().unwrap_or(0.0);
    let f = |v: f32| if v > lthresh() { f64::from(v).cbrt() as f32 } else { (f64::from(v) * f64::from(lscale()) + f64::from(lbias())) as f32 };
    let dim = LAB_LUT_DIM;
    let mut lut = vec![0i16; dim * dim * dim * 3];
    let step = (dim - 1) as f32;
    for r in 0..dim {
        for q in 0..dim {
            for p in 0..dim {
                let (rr, gg, bb) = (apply_gamma(p as f32 / step), apply_gamma(q as f32 / step), apply_gamma(r as f32 / step));
                let x = rr * s(0) + gg * s(1) + bb * s(2);
                let y = rr * s(3) + gg * s(4) + bb * s(5);
                let z = rr * s(6) + gg * s(7) + bb * s(8);
                let (fx, fy, fz) = (f(x), f(y), f(z));
                let l = if y > lthresh() { 116.0 * fy - 16.0 } else { y * (24389.0f32 / 27.0) };
                let a = 500.0 * (fx - fy);
                let b = 200.0 * (fy - fz);
                let i = (p + q * dim + r * dim * dim) * 3;
                let v = [(LAB_BASE * l / 100.0), (LAB_BASE * (a + 128.0) / 256.0), (LAB_BASE * (b + 128.0) / 256.0)];
                if let Some(cell) = lut.get_mut(i..i + 3) {
                    for (c, v) in cell.iter_mut().zip(v) {
                        *c = v.round_ties_even() as i16;
                    }
                }
            }
        }
    }
    lut
}

/// OpenCV's `clip` (`std::min(std::max(v, 0), 1)`), which keeps NaN, as `clamp` does.
fn clip01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn rgb_to_lab([r, g, b]: [f32; 3]) -> [f32; 3] {
    // RGB2Lab_f, useInterpolation: cvRound(clip(v) * LAB_BASE), then trilinearInterpolate.
    let lut = &tables().lab_lut;
    let c = [r, g, b].map(|v| (clip01(v) * LAB_BASE).round_ties_even() as i32);
    let cell = c.map(|v| (v >> 9) as usize);
    let frac = c.map(|v| (v >> 5) & 15);
    let dim = LAB_LUT_DIM;
    let mut acc = [0i32; 3];
    for corner in 0..8usize {
        let d = [corner & 1, (corner >> 1) & 1, (corner >> 2) & 1];
        let mut w = 1i32;
        let mut idx = 0usize;
        let strides = [1, dim, dim * dim];
        for k in 0..3 {
            w *= if d[k] == 1 { frac[k] } else { 16 - frac[k] };
            idx += (cell[k] + d[k]).min(dim - 1) * strides[k];
        }
        for (k, a) in acc.iter_mut().enumerate() {
            *a += i32::from(lut.get(idx * 3 + k).copied().unwrap_or(0)) * w;
        }
    }
    let [il, ia, ib] = acc.map(|v| (v + (1 << 11)) >> 12);
    let l = il as f32 * (1.0 / LAB_BASE);
    let a = ia as f32 * (1.0 / LAB_BASE);
    let b = ib as f32 * (1.0 / LAB_BASE);
    [l * 100.0, a * 256.0 - 128.0, b * 256.0 - 128.0]
}

/// Lab2RGB_f / Luv2RGB_f output: clip, then the sRGB encode spline.
fn encode_spline(v: f32) -> f32 {
    spline(clip01(v) * GAMMA_TAB_SIZE as f32, &tables().inv_gamma, GAMMA_TAB_SIZE)
}

fn decode_spline(v: f32) -> f32 {
    spline(clip01(v) * GAMMA_TAB_SIZE as f32, &tables().gamma, GAMMA_TAB_SIZE)
}

fn lab_to_rgb([li, ai, bi]: [f32; 3]) -> [f32; 3] {
    let c: Vec<f32> = (0..9).map(|k| (XYZ2SRGB_D65[k] * D65[k % 3]) as f32).collect();
    let c = |k: usize| c.get(k).copied().unwrap_or(0.0);
    let (y, fy) = if li <= 8.0 {
        let y = li / 903.3;
        (y, 7.787 * y + 16.0 / 116.0)
    } else {
        let fy = (li + 16.0) / 116.0;
        (fy * fy * fy, fy)
    };
    let fthresh = 6.0f32 / 29.0;
    let cube = |f: f32| if f <= fthresh { (f - 16.0 / 116.0) / 7.787 } else { f * f * f };
    let x = cube(ai / 500.0 + fy);
    let z = cube(fy - bi / 200.0);
    let r = c(0) * x + c(1) * y + c(2) * z;
    let g = c(3) * x + c(4) * y + c(5) * z;
    let b = c(6) * x + c(7) * y + c(8) * z;
    [encode_spline(r), encode_spline(g), encode_spline(b)]
}

/// Luv's white: (u'n, v'n) times 13, as OpenCV's `un`, `vn`.
fn luv_white() -> (f32, f32) {
    let d = 1.0 / (D65[0] + D65[1] * 15.0 + D65[2] * 3.0);
    ((d * 52.0 * D65[0]) as f32, (d * 117.0 * D65[1]) as f32)
}

fn rgb_to_luv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let c = SRGB2XYZ_D65.map(|v| v as f32);
    let (un, vn) = luv_white();
    let (r, g, b) = (decode_spline(r), decode_spline(g), decode_spline(b));
    let x = r * c[0] + g * c[1] + b * c[2];
    let y = r * c[3] + g * c[4] + b * c[5];
    let z = r * c[6] + g * c[7] + b * c[8];
    let l = spline(y * cbrt_tab_scale(), &tables().cbrt, LAB_CBRT_TAB_SIZE);
    let l = 116.0 * l - 16.0;
    let d = 52.0 / (x + 15.0 * y + 3.0 * z).max(f32::EPSILON);
    let u = l * (x * d - un);
    let v = l * (2.25 * y * d - vn);
    [l, u, v]
}

fn luv_to_rgb([l, u, v]: [f32; 3]) -> [f32; 3] {
    let c = XYZ2SRGB_D65.map(|v| v as f32);
    let (un, vn) = luv_white();
    let y = if l >= 8.0 {
        let y = (l + 16.0) * (1.0 / 116.0);
        y * y * y
    } else {
        l * (1.0 / 903.3)
    };
    let up = 3.0 * (u + l * un);
    let vp = (0.25 / (v + l * vn)).clamp(-0.25, 0.25);
    let x = y * 3.0 * up * vp;
    let z = y * ((156.0 * l - up) * vp - 5.0);
    let r = x * c[0] + y * c[1] + z * c[2];
    let g = x * c[3] + y * c[4] + z * c[5];
    let b = x * c[6] + y * c[7] + z * c[8];
    [encode_spline(r), encode_spline(g), encode_spline(b)]
}

// ---------------------------------------------------------------------------------------------
// XYZ, xyY: yapocis colormath (srgb_d65) with colorspaces.py's srgb_decode / srgb_encode.

/// `srgb_d65.rgb2xyz`, row-major (X, Y, Z rows).
const RGB2XYZ: [f32; 9] = [0.4124564, 0.3575761, 0.1804375, 0.2126729, 0.7151522, 0.072175, 0.0193339, 0.119192, 0.9503041];
/// `srgb_d65.xyz2rgb`, row-major.
const XYZ2RGB: [f32; 9] = [3.2404542, -1.5371385, -0.4985314, -0.969266, 1.8760108, 0.041556, 0.0556434, -0.2040259, 1.0572252];

/// numpy `clip(v, 0, None)`: NaN stays NaN.
fn clip0(v: f32) -> f32 {
    if v < 0.0 { 0.0 } else { v }
}

fn srgb_decode(v: f32) -> f32 {
    let v = clip0(v);
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn srgb_encode(v: f32) -> f32 {
    let v = clip0(v);
    // numpy float32: the exponent 1/2.4 is rounded to f32 first.
    if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf((1.0f64 / 2.4) as f32) - 0.055 }
}

/// colormath's `transform` kernel: `dot3` of the pixel with each row.
fn matrix(m: &[f32; 9], [a, b, c]: [f32; 3]) -> [f32; 3] {
    [a * m[0] + b * m[1] + c * m[2], a * m[3] + b * m[4] + c * m[5], a * m[6] + b * m[7] + c * m[8]]
}

fn rgb_to_xyz(rgb: [f32; 3]) -> [f32; 3] {
    matrix(&RGB2XYZ, rgb.map(srgb_decode))
}

fn xyz_to_rgb(xyz: [f32; 3]) -> [f32; 3] {
    matrix(&XYZ2RGB, xyz).map(srgb_encode)
}

fn xyz_to_xyy([x, y, z]: [f32; 3]) -> [f32; 3] {
    let sum = x + y + z;
    if sum != 0.0 { [x / sum, y / sum, y] } else { [0.0, 0.0, y] }
}

fn xyy_to_xyz([x, y, big_y]: [f32; 3]) -> [f32; 3] {
    if y == 0.0 {
        return [0.0, 0.0, 0.0];
    }
    [x * big_y / y, big_y, ((1.0 - x - y) * big_y) / y]
}
