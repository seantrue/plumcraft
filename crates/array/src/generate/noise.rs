//! Coordinates, Simplex Noise (yapocis numba `simplex.py`), Mandelbrot (yapocis numba
//! `mandelbrot.py`) and Game of Life (yapocis_external `life`, written plainly).

use npexact::random::Generator;
use plumcraft_engine::{Cx, Data, Inputs, Outputs};
use plumcraft_proto::error::NodeError;

use super::{as_channel, cancelled, seed, size_from, size_of};
use crate::img::Img;
use crate::op::{b, config, f, img, img_opt, many, n, one, s};

// ---------- Coordinates ----------

pub(super) fn coordinates(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let (h, w) = size_of(&img(i, "like")?);
    Img::check_size(h, w, 1).map_err(config)?;
    let (x, y) = coordinate_planes(h, w);
    Ok(many(vec![("x", Data::Array(Img::plane_of(h, w, x).to_array())), ("y", Data::Array(Img::plane_of(h, w, y).to_array()))]))
}

/// Each pixel's column and row.
pub fn coordinate_planes(h: usize, w: usize) -> (Vec<f32>, Vec<f32>) {
    let x = (0..h * w).map(|k| (k % w) as f32).collect();
    let y = (0..h * w).map(|k| (k / w) as f32).collect();
    (x, y)
}

// ---------- Simplex Noise ----------

/// The 2-D gradients (yapocis `GRAD2`), indexed by `hash & 11`.
const GRAD2: [[f64; 2]; 12] =
    [[1.0, 1.0], [-1.0, 1.0], [1.0, -1.0], [-1.0, -1.0], [1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0], [1.0, 1.0], [0.0, -1.0], [-1.0, 1.0], [0.0, -1.0]];

/// yapocis's `_hash_int`, in numba's 64-bit integers (wrapping, arithmetic shifts).
fn hash_int(x: i64, y: i64, z: i64, seed: i64) -> i64 {
    let mut h = seed;
    h ^= x.wrapping_mul(374_761_393);
    h ^= y.wrapping_mul(668_265_263);
    h ^= z.wrapping_mul(1_264_258_069);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

fn grad(h: i64) -> [f64; 2] {
    GRAD2.get((h & 11) as usize).copied().unwrap_or([0.0, 0.0])
}

/// Python's `a % m` for `m >= 1`.
fn wrap(a: i64, m: i64) -> i64 {
    a.rem_euclid(m.max(1))
}

/// One octave of 2-D simplex noise (yapocis `_simplex2d_single`), in float64 as numba runs it.
fn simplex2d_single(x: f64, y: f64, seed: i64, tileable: bool, tile_w: i64, tile_h: i64) -> f64 {
    const F2: f64 = 0.366_025_403_784_438_6;
    const G2: f64 = 0.211_324_865_405_187_13;
    let s = (x + y) * F2;
    let i = (x + s).floor() as i64;
    let j = (y + s).floor() as i64;
    let t = i.wrapping_add(j) as f64 * G2;
    let x0 = x - (i as f64 - t);
    let y0 = y - (j as f64 - t);
    let (i1, j1) = if x0 > y0 { (1i64, 0i64) } else { (0, 1) };
    let x1 = x0 - i1 as f64 + G2;
    let y1 = y0 - j1 as f64 + G2;
    let x2 = x0 - 1.0 + 2.0 * G2;
    let y2 = y0 - 1.0 + 2.0 * G2;
    let (ii, jj) = if tileable { (wrap(i, tile_w), wrap(j, tile_h)) } else { (i, j) };
    let corner = |dx: i64, dy: i64| if tileable { (wrap(ii + dx, tile_w), wrap(jj + dy, tile_h)) } else { (ii.wrapping_add(dx), jj.wrapping_add(dy)) };
    let contribution = |t: f64, (ci, cj): (i64, i64), px: f64, py: f64| {
        if t < 0.0 {
            return 0.0;
        }
        let t = t * t;
        let g = grad(hash_int(ci, cj, 0, seed));
        t * t * (g[0] * px + g[1] * py)
    };
    let n0 = contribution(0.5 - x0 * x0 - y0 * y0, (ii, jj), x0, y0);
    let n1 = contribution(0.5 - x1 * x1 - y1 * y1, corner(i1, j1), x1, y1);
    let n2 = contribution(0.5 - x2 * x2 - y2 * y2, corner(1, 1), x2, y2);
    70.0 * (n0 + n1 + n2)
}

/// Fractal 2-D simplex noise, 0-1, `h` × `w` (yapocis `Simplex.simplex2d`). Pixel (r, c) is
/// noise at (r / h, c / w) × frequency, as the numba kernel computes it for square sizes (its
/// own loop is only right for those).
#[allow(clippy::too_many_arguments)]
pub fn simplex2d(h: usize, w: usize, seed: i64, scale: f64, octaves: i64, persistence: f64, lacunarity: f64, tileable: bool) -> Vec<f32> {
    let mut out = vec![0.0f32; h * w];
    for (r, row) in out.chunks_mut(w.max(1)).enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            let (mut total, mut amplitude, mut frequency, mut max_value) = (0.0f64, 1.0f64, scale, 0.0f64);
            for octave in 0..octaves {
                let nx = (r as f64 / h as f64) * frequency;
                let ny = (c as f64 / w as f64) * frequency;
                let tile_w = ((w as f64 / frequency) as i64).max(1);
                let tile_h = ((h as f64 / frequency) as i64).max(1);
                let noise = simplex2d_single(nx, ny, seed.wrapping_add(octave), tileable, tile_w, tile_h);
                total += noise * amplitude;
                max_value += amplitude;
                amplitude *= persistence;
                frequency *= lacunarity;
            }
            let value = (total / max_value + 1.0) * 0.5;
            *v = value.clamp(0.0, 1.0) as f32;
        }
    }
    out
}

/// yapocis `Simplex.turbulence`: `|2 * noise - 1|` of the default fBm, in float32.
pub fn turbulence(h: usize, w: usize, seed: i64, scale: f64, octaves: i64) -> Vec<f32> {
    simplex2d(h, w, seed, scale, octaves, 0.5, 2.0, false).into_iter().map(|v| (2.0f32 * v - 1.0).abs()).collect()
}

/// yapocis `Simplex.ridged`: `(1 - turbulence)^2`, in float32.
pub fn ridged(h: usize, w: usize, seed: i64, scale: f64, octaves: i64) -> Vec<f32> {
    turbulence(h, w, seed, scale, octaves)
        .into_iter()
        .map(|t| {
            let v = 1.0f32 - t;
            v * v
        })
        .collect()
}

pub(super) fn simplex_noise(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let (h, w) = size_from(i)?;
    let seed = n(i, "seed", 0).clamp(0, super::MAX_SEED as i64);
    let cells = f(i, "cells", 4.0).clamp(0.1, 512.0);
    let octaves = n(i, "octaves", 4).clamp(1, 10);
    let out = match s(i, "kind", "smooth").as_str() {
        "smooth" => {
            simplex2d(h, w, seed, cells, octaves, f(i, "persistence", 0.5).clamp(0.05, 1.0), f(i, "lacunarity", 2.0).clamp(1.1, 4.0), b(i, "tileable", false))
        }
        "turbulence" => turbulence(h, w, seed, cells, octaves),
        "ridged" => ridged(h, w, seed, cells, octaves),
        other => return Err(config(format!("kind: {other:?} is not smooth, turbulence or ridged"))),
    };
    Ok(one("noise", Img::plane_of(h, w, out)))
}

// ---------- Mandelbrot ----------

/// `np.linspace(start, stop, num, dtype=np.float32)` with Python-float bounds (npexact):
/// computed in float64, then rounded.
pub fn linspace_f32(start: f64, stop: f64, num: usize) -> Vec<f32> {
    npexact::hist::linspace_f64_as_f32(start, stop, num)
}

/// `|z| > nextafter(2, inf)` as numpy decides it (`np.abs` of a complex64 is its hypot).
fn escaped(zr: f32, zi: f32, threshold: f32) -> bool {
    if zr.is_infinite() || zi.is_infinite() {
        return true;
    }
    let (a, b) = (f64::from(zr), f64::from(zi));
    ((a * a + b * b).sqrt() as f32) > threshold
}

/// The escape iteration of each point of `re` × `im` (yapocis numba `mandelbrot`: numpy
/// complex64 steps, the first iteration at which `|z| > 2`, 0 inside the set; a point escaping at
/// iteration 0 keeps going, as the kernel's `out == 0` test lets it). Row-major, `im.len()` rows.
pub fn mandelbrot_iterations(re: &[f32], im: &[f32], maxiter: i64, cancel: &dyn Fn() -> bool) -> Option<Vec<i16>> {
    let threshold = f32::from_bits(2.0f32.to_bits() + 1);
    let fused = npexact::random::Contraction::host_default() == npexact::random::Contraction::Fma;
    let mut out = Vec::with_capacity(re.len() * im.len());
    for &ci in im {
        if cancel() {
            return None;
        }
        for &cr in re {
            let (mut zr, mut zi) = (0.0f32, 0.0f32);
            let mut found = 0i16;
            for it in 0..maxiter {
                // z * z + c, as numpy multiplies complex64: (a·a - b·b, a·b + b·a), the
                // products contracted into fused multiply-adds where numpy's wheels are built so
                // (arm64; npexact's Contraction policy)
                let (r, m) = if fused { (zr.mul_add(zr, -(zi * zi)), zr.mul_add(zi, zi * zr)) } else { (zr * zr - zi * zi, zr * zi + zi * zr) };
                zr = r + cr;
                zi = m + ci;
                if escaped(zr, zi, threshold) {
                    found = it as i16;
                    if found != 0 {
                        break;
                    }
                }
            }
            out.push(found);
        }
    }
    Some(out)
}

/// The research software's Mandelbrot: the escape iteration / max_iterations over the view.
pub fn mandelbrot_view(center_x: f64, center_y: f64, zoom: f64, maxiter: i64, h: usize, w: usize, cancel: &dyn Fn() -> bool) -> Option<Vec<f32>> {
    let half = 1.5 / zoom;
    let re = linspace_f32(center_x - half, center_x + half, w);
    let im = linspace_f32(center_y - half * h as f64 / w as f64, center_y + half * h as f64 / w as f64, h);
    let it = mandelbrot_iterations(&re, &im, maxiter, cancel)?;
    Some(it.into_iter().map(|v| f32::from(v) / maxiter as f32).collect())
}

pub(super) fn mandelbrot(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let (h, w) = size_from(i)?;
    let maxiter = n(i, "max_iterations", 256).clamp(8, 32000);
    let zoom = f(i, "zoom", 1.0).clamp(0.01, 1e6);
    let (center_x, center_y) = (f(i, "center_x", -0.5).clamp(-4.0, 4.0), f(i, "center_y", 0.0).clamp(-4.0, 4.0));
    let out = mandelbrot_view(center_x, center_y, zoom, maxiter, h, w, &|| cx.cancelled()).ok_or_else(cancelled)?;
    Ok(one("escape", Img::plane_of(h, w, out)))
}

// ---------- Game of Life ----------

/// One generation on a torus (yapocis_external `life_step`).
pub fn life_step(state: &[u8], h: usize, w: usize) -> Vec<u8> {
    let mut out = vec![0u8; h * w];
    for y in 0..h {
        for x in 0..w {
            let mut count = 0;
            for dy in [h - 1, 0, 1] {
                for dx in [w - 1, 0, 1] {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let (ny, nx) = ((y + dy) % h, (x + dx) % w);
                    if state.get(ny * w + nx).is_some_and(|v| *v > 0) {
                        count += 1;
                    }
                }
            }
            let alive = state.get(y * w + x).is_some_and(|v| *v > 0);
            if let Some(o) = out.get_mut(y * w + x) {
                *o = u8::from((alive && (count == 2 || count == 3)) || (!alive && count == 3));
            }
        }
    }
    out
}

pub(super) fn game_of_life(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let steps = n(i, "steps", 50).clamp(0, 100_000);
    let (mut state, h, w) = if let Some(start) = img_opt(i, "start")? {
        let p = as_channel(&start, "start")?;
        (p.iter().map(|v| u8::from(*v != 0.0)).collect::<Vec<u8>>(), start.h, start.w)
    } else if let Some(like) = img_opt(i, "like")? {
        let (h, w) = size_of(&like);
        Img::check_size(h, w, 1).map_err(config)?;
        let density = f(i, "density", 0.3);
        let draws = Generator::default_rng(&[seed(i)]).random_f64(h * w);
        (draws.into_iter().map(|v| u8::from(v < density)).collect(), h, w)
    } else {
        return Err(config("connect start or like"));
    };
    Img::check_size(h, w, 1).map_err(config)?;
    for k in 0..steps {
        if cx.cancelled() {
            return Err(cancelled());
        }
        state = life_step(&state, h, w);
        cx.progress((k + 1) as f64 / steps as f64, None);
    }
    Ok(one("cells", Img::plane_of(h, w, state.into_iter().map(f32::from).collect())))
}
