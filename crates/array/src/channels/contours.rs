//! Contours to SVG: a picture as a real vector drawing, like a topographic map. Each brightness
//! level becomes one filled shape of smooth closed curves, stacked from dark to bright, so the
//! drawing has as many objects as levels, each editable in a vector app, rather than a mosaic of
//! fragments or an embedded bitmap (Array to SVG). Rasterize SVG draws it back.
//!
//! **Contours:** marching squares on the brightest channel, the grid padded with nothing so
//! every curve closes, and interpolated between pixels so edges aren't stepped. Each cell's
//! segments are directed with the inside on the left, so they chain into loops; a hole is a loop
//! the other way round, and the even-odd fill rule cuts it out.
//!
//! **Curves:** loops are simplified (Douglas-Peucker) and written as quadratic Béziers through
//! the midpoints of what is left, so a few points make a smooth curve.
//!
//! **Colour:** each level is filled with a horizontal gradient of the picture's mean colour in
//! that band, column by column, so a level can change hue across the picture.

use std::collections::HashMap;
use std::fmt::Write;

use plumcraft_engine::{Blob, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::catalogue::core_types;
use plumcraft_proto::error::NodeError;

use crate::img::Img;
use crate::op::{Op, array_in, config, f, float_in, img, int_in, n, out};

/// The largest picture traced, a side.
const MAX_SIDE: usize = 8192;
/// Colour stops across a level's gradient.
const STOPS: usize = 32;

pub fn nodes() -> Vec<Box<dyn Node>> {
    vec![Box::new(Op {
        id: "array.contour_svg",
        display: "Contours to SVG",
        group: "Arrays",
        description: "A picture as a vector drawing: each brightness level one filled shape of smooth closed curves, coloured from the picture, like a topographic map. Rasterize SVG draws it back.",
        inputs: vec![
            array_in("image", "The picture: RGB, or one channel, 0-1"),
            int_in("levels", 12, 1.0, 64.0, "How many brightness levels, so how many shapes"),
            float_in("detail", 0.6, 0.0, 16.0, "How far a curve may stray from the contour, in pixels; larger is simpler"),
            float_in("min_area", 16.0, 0.0, 1e6, "Drop loops smaller than this, in square pixels"),
        ],
        outputs: vec![out("svg", core_types::SVG, "The drawing")],
        run: contour_svg,
    })]
}

type Point = [f32; 2];

/// One crossing of a grid edge: `(x, y, 0)` runs right from a grid point, `(x, y, 1)` down.
type Edge = (u32, u32, u8);

/// The closed loops of `field >= level`, as points in pixels. The grid has a ring of `outside`
/// cells round it, so nothing runs off the picture.
fn loops(field: &[f32], w: usize, h: usize, level: f32) -> Vec<Vec<Point>> {
    let at = |x: usize, y: usize| -> f32 {
        if x == 0 || y == 0 || x > w || y > h { f32::NEG_INFINITY } else { field.get((y - 1) * w + (x - 1)).copied().unwrap_or(f32::NEG_INFINITY) }
    };
    let cross = |a: f32, b: f32| -> f32 { if !a.is_finite() || !b.is_finite() { 0.5 } else { ((level - a) / (b - a)).clamp(0.0, 1.0) } };
    // A pixel's centre is at +0.5, less the ring.
    let point = |e: Edge| -> Point {
        let (x, y) = (e.0 as usize, e.1 as usize);
        if e.2 == 0 {
            [x as f32 - 0.5 + cross(at(x, y), at(x + 1, y)), y as f32 - 0.5]
        } else {
            [x as f32 - 0.5, y as f32 - 0.5 + cross(at(x, y), at(x, y + 1))]
        }
    };
    let mut next: HashMap<Edge, Edge> = HashMap::new();
    for y in 0..=h {
        for x in 0..=w {
            let inside = |x: usize, y: usize| at(x, y) >= level;
            let (a, b, c, d) = (inside(x, y), inside(x + 1, y), inside(x + 1, y + 1), inside(x, y + 1));
            let case = usize::from(a) | usize::from(b) << 1 | usize::from(c) << 2 | usize::from(d) << 3;
            if case == 0 || case == 15 {
                continue;
            }
            let (ux, uy) = (x as u32, y as u32);
            let (top, right, bottom, left): (Edge, Edge, Edge, Edge) = ((ux, uy, 0), (ux + 1, uy, 1), (ux, uy + 1, 0), (ux, uy, 1));
            // Directed with the inside on the left, so each crossing has one way in and one out.
            let segments: &[(Edge, Edge)] = &match case {
                1 => [(left, top), (left, left)],
                2 => [(top, right), (top, top)],
                3 => [(left, right), (left, left)],
                4 => [(right, bottom), (right, right)],
                5 => [(left, top), (right, bottom)],
                6 => [(top, bottom), (top, top)],
                7 => [(left, bottom), (left, left)],
                8 => [(bottom, left), (bottom, bottom)],
                9 => [(bottom, top), (bottom, bottom)],
                10 => [(top, right), (bottom, left)],
                11 => [(bottom, right), (bottom, bottom)],
                12 => [(right, left), (right, right)],
                13 => [(right, top), (right, right)],
                _ => [(top, left), (top, top)],
            };
            for (from, to) in segments {
                if from != to {
                    next.insert(*from, *to);
                }
            }
        }
    }
    let mut out = Vec::new();
    while let Some(&start) = next.keys().next() {
        let (mut loop_points, mut e) = (Vec::new(), start);
        while let Some(to) = next.remove(&e) {
            loop_points.push(point(e));
            e = to;
        }
        out.push(loop_points);
    }
    out
}

fn area(p: &[Point]) -> f32 {
    let mut sum = 0.0;
    for (a, b) in p.iter().zip(p.iter().cycle().skip(1)) {
        sum += a[0] * b[1] - b[0] * a[1];
    }
    (sum / 2.0).abs()
}

/// How far `p` is from the segment `a` to `b`.
fn distance(p: Point, a: Point, b: Point) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx * dx + dy * dy;
    let t = if len > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len).clamp(0.0, 1.0) } else { 0.0 };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

/// A closed loop with the points that matter (Douglas-Peucker from two far-apart points).
fn simplify(p: &[Point], tolerance: f32) -> Vec<Point> {
    let n = p.len();
    if n < 8 || tolerance <= 0.0 {
        return p.to_vec();
    }
    let mut keep = vec![false; n];
    let half = n / 2;
    if let Some(k) = keep.get_mut(0) {
        *k = true;
    }
    if let Some(k) = keep.get_mut(half) {
        *k = true;
    }
    let mut stack = vec![(0usize, half), (half, n)];
    while let Some((a, b)) = stack.pop() {
        let (Some(&pa), Some(&pb)) = (p.get(a), p.get(b % n)) else { continue };
        let far = (a + 1..b).filter_map(|i| Some((i, distance(*p.get(i)?, pa, pb)))).fold((a, 0.0f32), |m, x| if x.1 > m.1 { x } else { m });
        if far.1 > tolerance {
            if let Some(k) = keep.get_mut(far.0) {
                *k = true;
            }
            stack.push((a, far.0));
            stack.push((far.0, b));
        }
    }
    p.iter().zip(&keep).filter(|(_, k)| **k).map(|(q, _)| *q).collect()
}

/// A closed loop as quadratic Béziers through the midpoints of its points.
fn curve(out: &mut String, p: &[Point]) {
    let mid = |a: Point, b: Point| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let (Some(&last), Some(&first)) = (p.last(), p.first()) else { return };
    let start = mid(last, first);
    let _ = write!(out, "M{:.1} {:.1}", start[0], start[1]);
    for (a, b) in p.iter().zip(p.iter().cycle().skip(1)) {
        let m = mid(*a, *b);
        let _ = write!(out, "Q{:.1} {:.1} {:.1} {:.1}", a[0], a[1], m[0], m[1]);
    }
    out.push('Z');
}

/// The mean colour of the pixels at or above `level`, in `STOPS` columns, each empty one taking
/// its nearest neighbour's.
fn stops(image: &Img, field: &[f32], level: f32, base: [f32; 3]) -> Vec<[f32; 3]> {
    let (w, h) = (image.w, image.h);
    let mut sum = vec![([0.0f32; 3], 0u32); STOPS];
    for y in 0..h {
        for x in 0..w {
            if field.get(y * w + x).is_some_and(|v| *v >= level)
                && let Some((s, count)) = sum.get_mut(x * STOPS / w)
            {
                for (k, slot) in s.iter_mut().enumerate() {
                    let c = if image.c >= 3 { k } else { 0 };
                    *slot += image.data.get((y * w + x) * image.c + c).copied().unwrap_or(0.0);
                }
                *count += 1;
            }
        }
    }
    let known = |(s, c): &([f32; 3], u32)| (*c > 0).then(|| s.map(|v| v / *c as f32));
    let colours: Vec<Option<[f32; 3]>> = sum.iter().map(known).collect();
    (0..STOPS)
        .map(|i| {
            let by_distance = (0..STOPS).flat_map(|d| [i.checked_sub(d), Some(i + d)]).flatten();
            by_distance.filter_map(|k| colours.get(k).copied().flatten()).next().unwrap_or(base)
        })
        .collect()
}

fn hex(c: [f32; 3]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

fn contour_svg(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "image")?;
    if a.table || a.labels {
        return Err(config("image: a table or labels is not a picture"));
    }
    if a.c == 3 && a.space.as_deref().is_some_and(|s| s != "RGB") {
        return Err(config(format!("image: {}: convert to RGB first (Convert Color Space)", a.describe())));
    }
    if a.c != 1 && a.c != 3 {
        return Err(config(format!("image: 1 or 3 channels, not {}", a.c)));
    }
    if a.w == 0 || a.h == 0 || a.w > MAX_SIDE || a.h > MAX_SIDE {
        return Err(config(format!("image: {}×{}: at most {MAX_SIDE} a side", a.w, a.h)));
    }
    let levels = n(i, "levels", 12).clamp(1, 64) as usize;
    let tolerance = f(i, "detail", 0.6) as f32;
    let min_area = f(i, "min_area", 16.0) as f32;
    let field: Vec<f32> = a.data.chunks_exact(a.c).map(|p| p.iter().copied().filter(|v| v.is_finite()).fold(0.0, f32::max)).collect();
    let (lo, hi) = field.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |m, v| (m.0.min(*v), m.1.max(*v)));
    if hi <= lo {
        return Err(config("image: every pixel is the same, so there are no contours"));
    }
    let background = |k: usize| {
        let (mut sum, mut count) = (0.0, 0u32);
        for (p, v) in a.data.chunks_exact(a.c).zip(&field) {
            if *v < lo + (hi - lo) / (levels as f32 + 1.0) {
                sum += p.get(if a.c >= 3 { k } else { 0 }).copied().unwrap_or(0.0);
                count += 1;
            }
        }
        if count > 0 { sum / count as f32 } else { 0.0 }
    };
    let base = [background(0), background(1), background(2)];
    let (mut defs, mut body) = (String::new(), String::new());
    for k in 0..levels {
        let level = lo + (hi - lo) * (k + 1) as f32 / (levels + 1) as f32;
        let mut d = String::new();
        for p in loops(&field, a.w, a.h, level).iter().filter(|p| area(p) >= min_area) {
            curve(&mut d, &simplify(p, tolerance));
        }
        if d.is_empty() {
            continue;
        }
        let _ = write!(defs, "<linearGradient id=\"level{k}\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"0\" x2=\"{}\" y2=\"0\">", a.w);
        for (s, c) in stops(&a, &field, level, base).iter().enumerate() {
            let _ = write!(defs, "<stop offset=\"{:.3}\" stop-color=\"{}\"/>", (s as f32 + 0.5) / STOPS as f32, hex(*c));
        }
        defs.push_str("</linearGradient>\n");
        let _ = writeln!(body, "<path id=\"contour{k}\" fill=\"url(#level{k})\" fill-rule=\"evenodd\" d=\"{d}\"/>");
    }
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\n<defs>\n{defs}</defs>\n<rect width=\"{w}\" height=\"{h}\" fill=\"{bg}\"/>\n{body}</svg>\n",
        w = a.w,
        h = a.h,
        bg = hex(base)
    );
    Ok(crate::op::many(vec![("svg", Data::Blob(Box::new(Blob::svg(svg.into_bytes())?)))]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bright disc with a dark hole on black.
    fn ring(w: usize, h: usize) -> Vec<f32> {
        let mut v = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let r = ((x as f32 - 32.0).powi(2) + (y as f32 - 32.0).powi(2)).sqrt();
                v[y * w + x] = if (10.0..24.0).contains(&r) { 1.0 } else { 0.0 };
            }
        }
        v
    }

    #[test]
    fn a_ring_has_an_outer_loop_and_a_hole_and_both_close() {
        let v = ring(64, 64);
        let l = loops(&v, 64, 64, 0.5);
        assert_eq!(l.len(), 2, "outside and hole");
        let mut areas: Vec<f32> = l.iter().map(|p| area(p)).collect();
        areas.sort_by(f32::total_cmp);
        let circle = |r: f32| std::f32::consts::PI * r * r;
        assert!((areas[0] - circle(10.0)).abs() < 0.15 * circle(10.0), "{areas:?}");
        assert!((areas[1] - circle(24.0)).abs() < 0.1 * circle(24.0), "{areas:?}");
        for p in &l {
            let (a, b) = (p[0], *p.last().unwrap());
            assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 2.0, "closed");
        }
    }

    #[test]
    fn a_shape_touching_the_edge_still_closes() {
        let l = loops(&[1.0; 16], 4, 4, 0.5);
        assert_eq!(l.len(), 1);
        assert!(area(&l[0]) > 9.0, "{}", area(&l[0]));
    }

    #[test]
    fn the_saddle_case_keeps_two_shapes_apart_or_joined_but_never_breaks() {
        let v = [1.0, 0.0, 0.0, 1.0];
        let l = loops(&v, 2, 2, 0.5);
        assert!(!l.is_empty() && l.iter().all(|p| p.len() >= 3));
    }

    #[test]
    fn simplifying_keeps_the_shape_with_far_fewer_points() {
        let l = loops(&ring(64, 64), 64, 64, 0.5);
        let big = l.iter().max_by_key(|p| p.len()).unwrap();
        let few = simplify(big, 0.6);
        assert!(few.len() < big.len() / 3, "{} from {}", few.len(), big.len());
        assert!((area(&few) - area(big)).abs() < 0.1 * area(big));
    }

    #[test]
    fn colours_read_as_hex_and_empty_columns_borrow() {
        assert_eq!(hex([1.0, 0.0, 0.5]), "#ff0080");
        let image = Img::filled(2, 4, 3, 1.0);
        let c = stops(&image, &[1.0; 8], 0.5, [0.0; 3]);
        assert_eq!((c.len(), c[0]), (STOPS, [1.0; 3]));
    }
}
