//! Draw Shapes and Composite (yapocis numba `graphics4.py`: shapes antialiased with smoothstep
//! and blended "over" an RGBA canvas), and Draw Lines (yapocis_external `draw` and `graphicsx`,
//! written plainly: Bresenham lines of one value, or 1-pixel antialiased lines).
//!
//! The arithmetic follows numba's types: coordinates, widths and smoothstep in float64; colours
//! float32, each blend summed in float64 and stored as float32. Table rows with a value that is
//! not finite are skipped (the kernels' results for them are undefined).

use plumcraft_engine::{Cx, Inputs, Outputs};
use plumcraft_proto::error::NodeError;

use super::{as_channel, pymax, pymin, rows_of};
use crate::img::Img;
use crate::op::{config, f, img, img_opt, n, one, s};

/// Each shape and the columns its rows need (the research software's `SHAPES`).
pub const SHAPES: &[(&str, &[&str])] = &[
    ("lines", &["x0", "y0", "x1", "y1"]),
    ("rects", &["x0", "y0", "x1", "y1"]),
    ("circles", &["x", "y", "radius"]),
    ("ellipses", &["x", "y", "radius x", "radius y"]),
    ("triangles", &["x1", "y1", "x2", "y2", "x3", "y3"]),
    ("mesh_triangles", &["x1", "y1", "x2", "y2", "x3", "y3"]),
];

/// An (H, W, 4) RGBA canvas, as Graphics4 draws on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba {
    pub h: usize,
    pub w: usize,
    pub data: Vec<f32>,
}

impl Rgba {
    /// RGB planes (H × W each) and an alpha of 1.
    pub fn from_rgb(h: usize, w: usize, rgb: &[Vec<f32>]) -> Rgba {
        let mut data = vec![1.0f32; h * w * 4];
        for (ch, plane) in rgb.iter().take(3).enumerate() {
            for (k, v) in plane.iter().take(h * w).enumerate() {
                if let Some(d) = data.get_mut(k * 4 + ch) {
                    *d = *v;
                }
            }
        }
        Rgba { h, w, data }
    }

    pub fn plane(&self, ch: usize) -> Vec<f32> {
        self.data.iter().skip(ch).step_by(4).copied().collect()
    }

    /// Porter-Duff "over" at column `x`, row `y` (yapocis `_blend_rgba`); outside, nothing.
    pub fn blend(&mut self, x: i64, y: i64, c: [f32; 4]) {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return;
        }
        let k = (y as usize * self.w + x as usize) * 4;
        let Some(px) = self.data.get_mut(k..k + 4) else { return };
        let [r, g, b, a] = c;
        let inv = 1.0f64 - f64::from(a);
        for (d, v) in px.iter_mut().zip([r, g, b]) {
            *d = (f64::from(v * a) + f64::from(*d) * inv) as f32;
        }
        if let Some(d) = px.get_mut(3) {
            *d = (f64::from(a) + f64::from(*d) * inv) as f32;
        }
    }
}

/// A colour with its alpha scaled by `t` (`aa_rgba[3] *= blend_t`, stored as float32).
fn faded(c: [f32; 4], t: f64) -> [f32; 4] {
    [c[0], c[1], c[2], (f64::from(c[3]) * t) as f32]
}

/// yapocis `_smoothstep`, with Python's `max`/`min`.
fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = pymax(0.0, pymin(1.0, (x - e0) / (e1 - e0)));
    t * t * (3.0 - 2.0 * t)
}

/// `int(v)`: truncated toward zero.
fn int(v: f64) -> i64 {
    v as i64
}

/// One line `width` wide with flat ends (yapocis `_draw_line_rgba`).
#[allow(clippy::too_many_arguments)]
pub fn line(img: &mut Rgba, x0: f64, y0: f64, x1: f64, y1: f64, width: f64, aa: f64, c: [f32; 4]) {
    let (wl, hl) = (img.w as f64 - 1.0, img.h as f64 - 1.0);
    let xmin = int(pymax(0.0, pymin(x0, x1) - width - aa - 1.0));
    let xmax = int(pymin(wl, pymax(x0, x1) + width + aa + 1.0));
    let ymin = int(pymax(0.0, pymin(y0, y1) - width - aa - 1.0));
    let ymax = int(pymin(hl, pymax(y0, y1) + width + aa + 1.0));
    let (vx, vy) = (x1 - x0, y1 - y0);
    let seg2 = vx * vx + vy * vy + 1e-20;
    let threshold = width * 0.5 + 0.5;
    let thr2 = threshold * threshold;
    for y in ymin..=ymax {
        for x in xmin..=xmax {
            let (wx, wy) = (x as f64 - x0, y as f64 - y0);
            let t = (wx * vx + wy * vy) / seg2;
            if !(0.0..=1.0).contains(&t) {
                continue;
            }
            let dx = x as f64 - (x0 + t * vx);
            let dy = y as f64 - (y0 + t * vy);
            let dist2 = dx * dx + dy * dy;
            if aa > 0.0 {
                let blend_t = smoothstep(threshold + aa, threshold - aa, dist2.sqrt());
                if blend_t > 0.0 {
                    img.blend(x, y, faded(c, blend_t));
                }
            } else if dist2 < thr2 {
                img.blend(x, y, c);
            }
        }
    }
}

/// A rectangle between float corners, antialiased inward (yapocis `_draw_rect_rgba`).
#[allow(clippy::too_many_arguments)]
pub fn rect(img: &mut Rgba, x0: f64, y0: f64, x1: f64, y1: f64, aa: f64, c: [f32; 4]) {
    let (xmin, xmax) = (pymin(x0, x1), pymax(x0, x1));
    let (ymin, ymax) = (pymin(y0, y1), pymax(y0, y1));
    let xs = 0i64.max(int(xmin.ceil()))..=(img.w as i64 - 1).min(int(xmax.floor()));
    let ys = 0i64.max(int(ymin.ceil()))..=(img.h as i64 - 1).min(int(ymax.floor()));
    for y in ys {
        for x in xs.clone() {
            let (fx, fy) = (x as f64, y as f64);
            let inside = xmin <= fx && fx <= xmax && ymin <= fy && fy <= ymax;
            if aa > 0.0 && inside {
                let edge = pymin(pymin(fx - xmin, xmax - fx), pymin(fy - ymin, ymax - fy));
                img.blend(x, y, faded(c, smoothstep(0.0, aa, edge)));
            } else if inside {
                img.blend(x, y, c);
            }
        }
    }
}

/// A disc (yapocis `_draw_circle_rgba`).
pub fn circle(img: &mut Rgba, cx: f64, cy: f64, r: f64, aa: f64, c: [f32; 4]) {
    let xmin = int(pymax(0.0, cx - r - aa - 1.0));
    let xmax = int(pymin(img.w as f64 - 1.0, cx + r + aa + 1.0));
    let ymin = int(pymax(0.0, cy - r - aa - 1.0));
    let ymax = int(pymin(img.h as f64 - 1.0, cy + r + aa + 1.0));
    for y in ymin..=ymax {
        let dy = y as f64 - cy;
        for x in xmin..=xmax {
            let dx = x as f64 - cx;
            let dist = (dx * dx + dy * dy).sqrt();
            if aa > 0.0 {
                let blend_t = smoothstep(r + aa, r - aa, dist);
                if blend_t > 0.0 {
                    img.blend(x, y, faded(c, blend_t));
                }
            } else if dist <= r {
                img.blend(x, y, c);
            }
        }
    }
}

/// An axis-aligned ellipse (yapocis `_draw_ellipse_rgba`).
#[allow(clippy::too_many_arguments)]
pub fn ellipse(img: &mut Rgba, cx: f64, cy: f64, rx: f64, ry: f64, aa: f64, c: [f32; 4]) {
    let aa_pad = if aa > 0.0 { aa } else { 0.0 };
    let xmin = int(pymax(0.0, cx - rx - aa_pad - 1.0));
    let xmax = int(pymin(img.w as f64 - 1.0, cx + rx + aa_pad + 1.0));
    let ymin = int(pymax(0.0, cy - ry - aa_pad - 1.0));
    let ymax = int(pymin(img.h as f64 - 1.0, cy + ry + aa_pad + 1.0));
    for y in ymin..=ymax {
        let dy = y as f64 - cy;
        for x in xmin..=xmax {
            let dx = x as f64 - cx;
            let (ex, ey) = (dx / rx, dy / ry);
            let d = (ex * ex + ey * ey).sqrt();
            if aa > 0.0 {
                let dist_to_edge = (d - 1.0) * ((rx + ry) * 0.5);
                let blend_t = smoothstep(aa, -aa, dist_to_edge);
                if blend_t > 0.0 {
                    img.blend(x, y, faded(c, blend_t));
                }
            } else if d <= 1.0 {
                img.blend(x, y, c);
            }
        }
    }
}

fn edge(ax: f64, ay: f64, bx: f64, by: f64, px: f64, py: f64) -> f64 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

/// yapocis `_tri_edge`: exactly antisymmetric in (a, b).
fn tri_edge(ax: f64, ay: f64, bx: f64, by: f64, px: f64, py: f64) -> f64 {
    let swap = bx < ax || (bx == ax && by < ay);
    let (ax, ay, bx, by) = if swap { (bx, by, ax, ay) } else { (ax, ay, bx, by) };
    let v = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
    if swap { -v } else { v }
}

fn tri_owns(dx: f64, dy: f64) -> bool {
    dy > 0.0 || (dy == 0.0 && dx > 0.0)
}

/// yapocis `_tri_inside`: the top-left fill rule.
#[allow(clippy::too_many_arguments)]
fn tri_inside(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> bool {
    let area = tri_edge(x1, y1, x2, y2, x3, y3);
    if area == 0.0 {
        return false;
    }
    let s = if area > 0.0 { 1.0 } else { -1.0 };
    let e1 = s * tri_edge(x1, y1, x2, y2, px, py);
    let e2 = s * tri_edge(x2, y2, x3, y3, px, py);
    let e3 = s * tri_edge(x3, y3, x1, y1, px, py);
    (e1 > 0.0 || (e1 == 0.0 && tri_owns(s * (x2 - x1), s * (y2 - y1))))
        && (e2 > 0.0 || (e2 == 0.0 && tri_owns(s * (x3 - x2), s * (y3 - y2))))
        && (e3 > 0.0 || (e3 == 0.0 && tri_owns(s * (x1 - x3), s * (y1 - y3))))
}

fn dist_to_segment(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let (pax, pay, bax, bay) = (px - ax, py - ay, bx - ax, by - ay);
    let dot_ba = bax * bax + bay * bay;
    if dot_ba == 0.0 {
        return (pax * pax + pay * pay).sqrt();
    }
    let h = pymax(0.0, pymin(1.0, (pax * bax + pay * bay) / dot_ba));
    let (dx, dy) = (pax - h * bax, pay - h * bay);
    (dx * dx + dy * dy).sqrt()
}

/// A triangle with inclusive edges, or a mesh triangle with the top-left rule (yapocis
/// `_draw_triangle_rgba`).
pub fn triangle(img: &mut Rgba, p: [f64; 6], aa: f64, c: [f32; 4], mesh: bool) {
    let [x0, y0, x1, y1, x2, y2] = p;
    let (lo, hi) = (|a: f64, b: f64, c: f64| a.min(b).min(c), |a: f64, b: f64, c: f64| a.max(b).max(c));
    let (xmin, xmax, ymin, ymax) = if aa > 0.0 {
        (
            int(pymax(0.0, lo(x0, x1, x2) - aa - 1.0)),
            int(pymin(img.w as f64 - 1.0, hi(x0, x1, x2) + aa + 1.0)),
            int(pymax(0.0, lo(y0, y1, y2) - aa - 1.0)),
            int(pymin(img.h as f64 - 1.0, hi(y0, y1, y2) + aa + 1.0)),
        )
    } else {
        (
            int(pymax(0.0, lo(x0, x1, x2))),
            int(pymin(img.w as f64 - 1.0, hi(x0, x1, x2))),
            int(pymax(0.0, lo(y0, y1, y2))),
            int(pymin(img.h as f64 - 1.0, hi(y0, y1, y2))),
        )
    };
    if edge(x0, y0, x1, y1, x2, y2) == 0.0 {
        return;
    }
    for y in ymin..=ymax {
        for x in xmin..=xmax {
            let (fx, fy) = (x as f64, y as f64);
            let inside = if mesh {
                // (row, column) order, as the kernels see coordinates
                tri_inside(fy, fx, y0, x0, y1, x1, y2, x2)
            } else {
                let w0 = edge(x1, y1, x2, y2, fx, fy);
                let w1 = edge(x2, y2, x0, y0, fx, fy);
                let w2 = edge(x0, y0, x1, y1, fx, fy);
                (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0)
            };
            if aa > 0.0 && inside {
                let d1 = dist_to_segment(fx, fy, x0, y0, x1, y1);
                let d2 = dist_to_segment(fx, fy, x1, y1, x2, y2);
                let d3 = dist_to_segment(fx, fy, x2, y2, x0, y0);
                img.blend(x, y, faded(c, smoothstep(0.0, aa, pymin(pymin(d1, d2), d3))));
            } else if inside {
                img.blend(x, y, c);
            }
        }
    }
}

/// Draw one row of `shape` (x = column, y = row). Rows with a value that isn't finite are
/// skipped.
pub fn draw_shape(img: &mut Rgba, shape: &str, row: &[f64], width: f64, aa: f64, c: [f32; 4]) {
    if row.iter().any(|v| !v.is_finite()) {
        return;
    }
    let v = |k: usize| row.get(k).copied().unwrap_or(0.0);
    match shape {
        "lines" => line(img, v(0), v(1), v(2), v(3), width, aa, c),
        "rects" => rect(img, v(0), v(1), v(2), v(3), aa, c),
        "circles" => circle(img, v(0), v(1), v(2), aa, c),
        "ellipses" => ellipse(img, v(0), v(1), v(2), v(3), aa, c),
        "triangles" => triangle(img, std::array::from_fn(v), aa, c, false),
        "mesh_triangles" => triangle(img, std::array::from_fn(v), aa, c, true),
        _ => {}
    }
}

/// Paste an RGBA `block` (`bh` × `bw`) with its top-left at column `x`, row `y` (yapocis
/// `composite`); what falls outside is dropped.
pub fn composite_block(img: &mut Rgba, block: &Rgba, x: i64, y: i64) {
    let (bh, bw) = (block.h as i64, block.w as i64);
    for row in 0i64.max(y)..(img.h as i64).min(y.saturating_add(bh)) {
        for col in 0i64.max(x)..(img.w as i64).min(x.saturating_add(bw)) {
            let k = (((row - y) * bw + (col - x)) * 4) as usize;
            if let Some(px) = block.data.get(k..k + 4) {
                img.blend(col, row, [px[0], px[1], px[2], px[3]]);
            }
        }
    }
}

// ---------- canvases ----------

/// A canvas as RGBA (the research software's `rgba_canvas`): one channel repeated as grey, or a colour
/// stack converted to RGB.
pub fn rgba_canvas(a: &Img, what: &str) -> Result<Rgba, String> {
    if a.table {
        return Err(format!("{what}: a table is not a picture"));
    }
    Img::check_size(a.h, a.w, 4).map_err(|e| format!("{what}: {e}"))?;
    let planes = a.planes();
    let rgb = match a.c {
        1 => vec![planes.first().cloned().unwrap_or_default(); 3],
        3 => match a.space.as_deref() {
            None | Some("RGB") => planes,
            Some(space) => plumcraft_engine::color::to_rgb(space, &planes).ok_or_else(|| format!("{what}: no RGB form for {space}"))?,
        },
        c => return Err(format!("{what}: a {c}-channel array has no RGB form (use Pick Channel)")),
    };
    Ok(Rgba::from_rgb(a.h, a.w, &rgb))
}

/// Back from RGBA (the research software's `back_from_rgba`): grey (the mean of R, G, B, keeping the
/// canvas's space and names) for a one-channel canvas, else RGB.
pub fn back_from_rgba(rgba: &Rgba, canvas: &Img) -> Img {
    if canvas.c == 1 {
        let grey = rgba.data.as_chunks::<4>().0.iter().map(|[r, g, b, _]| (r + g + b) / 3.0).collect();
        return Img { space: canvas.space.clone(), names: canvas.names.clone(), ..Img::plane_of(rgba.h, rgba.w, grey) };
    }
    let names = ["red", "green", "blue"].iter().map(|s| (*s).to_owned()).collect();
    Img { space: Some("RGB".into()), names, ..Img::from_planes(rgba.h, rgba.w, &[rgba.plane(0), rgba.plane(1), rgba.plane(2)]) }
}

/// `#rrggbb` as 0-1 floats (the research software's `hex_rgb`).
pub fn hex_rgb(color: &str) -> Result<[f64; 3], String> {
    let hex = color.trim().trim_start_matches('#');
    let part = |k: usize| hex.get(k..k + 2).and_then(|p| u8::from_str_radix(p, 16).ok()).map(|v| f64::from(v) / 255.0);
    match (hex.len(), part(0), part(2), part(4)) {
        (6, Some(r), Some(g), Some(b)) => Ok([r, g, b]),
        _ => Err(format!("color: {color:?} is not #rrggbb")),
    }
}

/// Row-major rows of `cols` values from an array's data.
fn rows(a: &Img) -> (Vec<&[f32]>, usize) {
    let (n, cols) = rows_of(a);
    if cols == 0 {
        return (Vec::new(), 0);
    }
    (a.data.chunks_exact(cols).take(n).collect(), cols)
}

pub(super) fn draw_shapes(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let canvas = img(i, "canvas")?;
    let shapes = img(i, "shapes")?;
    let shape = s(i, "shape", "lines");
    let Some((_, columns)) = SHAPES.iter().find(|(s, _)| *s == shape) else {
        return Err(config(format!("shape: {shape:?} is not one of {}", SHAPES.iter().map(|(s, _)| *s).collect::<Vec<_>>().join(", "))));
    };
    let (table, cols) = rows(&shapes);
    if cols < columns.len() {
        return Err(config(format!("{shape} need {} columns ({}), the table has {cols}", columns.len(), columns.join(" "))));
    }
    let mut image = rgba_canvas(&canvas, "canvas").map_err(config)?;
    let opacity = f(i, "opacity", 1.0).clamp(0.0, 1.0) as f32;
    let width = f64::from(f(i, "width", 1.0).clamp(0.1, 200.0) as f32);
    let aa = f64::from(f(i, "antialias", 1.0).clamp(0.0, 10.0) as f32);
    let colors: Vec<[f32; 4]> = match img_opt(i, "colors")? {
        Some(c) if !table.is_empty() => {
            let (crow, ccols) = rows(&c);
            if ccols < 3 {
                return Err(config(format!("colors: a table of r, g, b rows, got {}", c.describe())));
            }
            if crow.len() != table.len() {
                return Err(config(format!("{} shapes but {} colours", table.len(), crow.len())));
            }
            crow.iter().map(|r| [r.first().copied().unwrap_or(0.0), r.get(1).copied().unwrap_or(0.0), r.get(2).copied().unwrap_or(0.0), opacity]).collect()
        }
        _ => {
            let [r, g, b] = hex_rgb(&s(i, "color", "#ffffff")).map_err(config)?;
            vec![[r as f32, g as f32, b as f32, opacity]; table.len()]
        }
    };
    for (row, c) in table.iter().zip(colors) {
        let row: Vec<f64> = row.iter().take(columns.len()).map(|v| f64::from(*v)).collect();
        draw_shape(&mut image, &shape, &row, width, aa, c);
    }
    Ok(one("drawing", back_from_rgba(&image, &canvas)))
}

pub(super) fn composite(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let canvas = img(i, "canvas")?;
    let block = img(i, "block")?;
    let mut image = rgba_canvas(&canvas, "canvas").map_err(config)?;
    let mut piece = rgba_canvas(&block, "block").map_err(config)?;
    let opacity = f(i, "opacity", 1.0).clamp(0.0, 1.0) as f32;
    let alpha = match img_opt(i, "alpha")? {
        Some(a) => {
            let p = as_channel(&a, "alpha")?;
            if (a.h, a.w) != (block.h, block.w) {
                return Err(config(format!("alpha ({}) must be the block's size ({}×{})", a.describe(), block.h, block.w)));
            }
            p
        }
        None => vec![1.0; block.h * block.w],
    };
    for (px, a) in piece.data.as_chunks_mut::<4>().0.iter_mut().zip(alpha) {
        px[3] = (if a.is_nan() { a } else { a.clamp(0.0, 1.0) }) * opacity;
    }
    let (x, y) = (n(i, "x", 0).clamp(-16384, 16384), n(i, "y", 0).clamp(-16384, 16384));
    composite_block(&mut image, &piece, x, y);
    Ok(one("composite", back_from_rgba(&image, &canvas)))
}

// ---------- Draw Lines ----------

/// A Bresenham line from (x0, y0) to (x1, y1) in the kernel's coordinates (x = row, y =
/// column) set to `value` (yapocis_external `_draw_single_line`). Only the steps inside the
/// array are visited: the minor coordinate at step k follows from the error term in closed form.
#[allow(clippy::too_many_arguments)]
pub fn bresenham(a: &mut [f32], rows: usize, cols: usize, x0: i64, y0: i64, x1: i64, y1: i64, value: f32) {
    let steep = (y1 - y0).abs() > (x1 - x0).abs();
    let (mut x0, mut y0, mut x1, mut y1) = if steep { (y0, x0, y1, x1) } else { (x0, y0, x1, y1) };
    if x0 > x1 {
        std::mem::swap(&mut x0, &mut x1);
        std::mem::swap(&mut y0, &mut y1);
    }
    let dx = i128::from(x1 - x0);
    let dy = i128::from((y1 - y0).abs());
    let e0 = dx / 2;
    let ystep: i128 = if y0 < y1 { 1 } else { -1 };
    let major = if steep { cols } else { rows } as i64;
    let first = x0.max(0);
    let last = x1.min(major - 1);
    for x in first..=last {
        let k = i128::from(x - x0);
        // increments after k steps: the m with 0 <= e0 - k·dy + m·dx < dx
        let m = if dx == 0 { 0 } else { -((e0 - k * dy).div_euclid(dx)) };
        let y = i128::from(y0) + ystep * m;
        let (px, py) = if steep { (y, i128::from(x)) } else { (i128::from(x), y) };
        if px >= 0 && py >= 0 && px < rows as i128 && py < cols as i128 {
            let addr = (px as usize) * cols + py as usize;
            if let Some(v) = a.get_mut(addr) {
                *v = value;
            }
        }
    }
}

/// 1-pixel antialiased lines (yapocis_external `graphicsx`), in float32: a pixel within 0.5 of
/// a segment (and alongside it) becomes 1 - distance, the last such line winning. `lines` are
/// (row0, col0, row1, col1).
pub fn antialiased_lines(a: &mut [f32], rows: usize, cols: usize, lines: &[[f32; 4]]) {
    const HALF: f32 = 0.5;
    for &[sx, sy, ex, ey] in lines {
        if !(sx.is_finite() && sy.is_finite() && ex.is_finite() && ey.is_finite()) {
            continue;
        }
        let (vx, vy) = (ex - sx, ey - sy);
        let mag2 = vx * vx + vy * vy;
        if mag2 == 0.0 {
            continue;
        }
        let r0 = (f64::from(sx.min(ex)).floor() - 1.0).max(0.0) as usize;
        let r1 = (f64::from(sx.max(ex)).ceil() + 1.0).min(rows as f64 - 1.0);
        let c0 = (f64::from(sy.min(ey)).floor() - 1.0).max(0.0) as usize;
        let c1 = (f64::from(sy.max(ey)).ceil() + 1.0).min(cols as f64 - 1.0);
        if r1 < 0.0 || c1 < 0.0 {
            continue;
        }
        for r in r0..=r1 as usize {
            for c in c0..=c1 as usize {
                let (px, py) = (r as f32, c as f32);
                let u = ((px - sx) * vx + (py - sy) * vy) / mag2;
                if !(0.0..=1.0).contains(&u) {
                    continue;
                }
                let dx = px - (sx + u * vx);
                let dy = py - (sy + u * vy);
                let d = (dx * dx + dy * dy).sqrt();
                if d < HALF
                    && let Some(v) = a.get_mut(r * cols + c)
                {
                    *v = HALF + (HALF - d);
                }
            }
        }
    }
}

pub(super) fn draw_lines(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let canvas = img(i, "canvas")?;
    let mut image = as_channel(&canvas, "canvas")?;
    let lines = img(i, "lines")?;
    let (table, cols) = rows(&lines);
    if cols < 4 {
        return Err(config(format!("lines: a table of x0, y0, x1, y1, got {}", lines.describe())));
    }
    // (row, column) order, as the kernels take them
    let swapped: Vec<[f32; 4]> = table.iter().map(|r| [r[1], r[0], r[3], r[2]]).filter(|r| r.iter().all(|v| v.is_finite())).collect();
    let (h, w) = (canvas.h, canvas.w);
    match s(i, "style", "hard").as_str() {
        "hard" => {
            let value = f(i, "value", 1.0) as f32;
            for [x0, y0, x1, y1] in swapped {
                let t = |v: f32| i64::from(v as i32);
                bresenham(&mut image, h, w, t(x0), t(y0), t(x1), t(y1), value);
            }
        }
        "antialiased" => antialiased_lines(&mut image, h, w, &swapped),
        other => return Err(config(format!("style: {other:?} is not hard or antialiased"))),
    }
    Ok(one("drawing", Img::plane_of(h, w, image)))
}
