//! A wire's path through its bend points (`Wire.via`), and hit testing on it: the geometry of
//! the egui shell's `wire_route`, moved to the core (stage N1) so every shell routes and hits a
//! wire the same way. Lifted from the research software (`crates/ui-egui/src/wire_route.rs`).
//!
//! **The curve:** a Catmull-Rom spline through the output pin, the bend points and the input
//! pin, as cubic Béziers sampled [`STEPS`] times a segment. It leaves the output and enters the
//! input horizontally, as egui-snarl's wires do.

//!
//! **Cubic chains (N2 slice 3).** Every wire the scene publishes has a `path`: a chain of cubic
//! Béziers, the start point and then three points a segment (two control points and the end),
//! which every shell draws and the core hit-tests, so what is drawn and what is hit agree on
//! every platform (`docs/shell-interface.md` 8.8, open question 7). A routed wire's chain is
//! exactly the Catmull-Rom curve above ([`routed_chain`]). A plain wire's is fitted to egui-snarl's
//! degree-5 curve ([`snarl_quintic`], ported from egui-snarl 0.12): split evenly into
//! [`PLAIN_PIECES`] and each piece replaced by the cubic with its ends and end tangents
//! ([`plain_chain`]). Two pieces strayed up to 2.2 graph units from the quintic on steep wires;
//! four stay under 0.35 at usual lengths and 0.72 on a 5,000-unit wire
//! (`claude-debug/native-n2/s3-fit-check.py`).

use crate::plain::{Point, Rect};

/// Samples per curve segment.
pub const STEPS: usize = 24;

/// egui-snarl's wire frame at its default style (`pin size × 3`, the pin `interact_size.y × 0.6`
/// of egui's 18), graph units: how far a plain wire runs level out of a pin.
pub const WIRE_FRAME: f32 = 32.4;

/// The cubics a plain wire's chain has.
pub const PLAIN_PIECES: usize = 4;

/// The biggest coordinate a chain keeps (graph units); past it, or not a number, a coordinate is
/// clamped (a hostile shell or a corrupt graph can't make a chain of infinities or NaN).
pub const MAX_COORD: f32 = 1.0e7;

/// `v` as a coordinate a chain can hold: finite, within [`MAX_COORD`] (NaN is 0).
pub fn coord(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(-MAX_COORD, MAX_COORD) }
}

/// `p` with both coordinates [`coord`]ed.
pub fn tame(p: Point) -> Point {
    Point::new(coord(p.x), coord(p.y))
}

fn normalized(v: Point) -> Point {
    let l = v.length();
    if l > 0.0 && l.is_finite() { v / l } else { Point::ZERO }
}

fn lerp(a: Point, b: Point, t: f32) -> Point {
    a + (b - a) * t
}

/// The control points of a Catmull-Rom curve through `points` (pin, bend points, pin) as a
/// cubic chain: Catmull-Rom tangents inside, horizontal ones at the two pins. Fewer than two
/// points: the points as they are.
pub fn routed_chain(points: &[Point]) -> Vec<Point> {
    let n = points.len();
    if n < 2 {
        return points.iter().copied().map(tame).collect();
    }
    let at = |i: usize| points.get(i).copied().map_or(Point::ZERO, tame);
    let tangent = |i: usize| -> Point {
        if i == 0 {
            Point::new((at(1) - at(0)).length(), 0.0)
        } else if i == n - 1 {
            Point::new((at(n - 1) - at(n - 2)).length(), 0.0)
        } else {
            (at(i + 1) - at(i - 1)) * 0.5
        }
    };
    let mut out = Vec::with_capacity((n - 1) * 3 + 1);
    out.push(at(0));
    for i in 0..n - 1 {
        let (p0, p3) = (at(i), at(i + 1));
        out.push(tame(p0 + tangent(i) / 3.0));
        out.push(tame(p3 - tangent(i + 1) / 3.0));
        out.push(p3);
    }
    out
}

/// A point on the cubic `c` at `t`.
pub fn cubic_at(c: [Point; 4], t: f32) -> Point {
    let u = 1.0 - t;
    c[0] * (u * u * u) + c[1] * (3.0 * u * u * t) + c[2] * (3.0 * u * t * t) + c[3] * (t * t * t)
}

/// The cubics of a chain (start, then three points a segment); a short tail is left out.
pub fn cubics(chain: &[Point]) -> impl Iterator<Item = [Point; 4]> + '_ {
    let n = chain.len().saturating_sub(1) / 3;
    (0..n).filter_map(move |k| match chain.get(3 * k..3 * k + 4) {
        Some(&[a, b, c, d]) => Some([a, b, c, d]),
        _ => None,
    })
}

/// A chain sampled [`STEPS`] times a segment (what a polyline-drawing shell draws).
pub fn sample_chain(chain: &[Point]) -> Vec<Point> {
    let mut out = Vec::with_capacity(chain.len() / 3 * STEPS + 1);
    if let Some(first) = chain.first() {
        out.push(*first);
    }
    for c in cubics(chain) {
        for k in 1..=STEPS {
            out.push(cubic_at(c, k as f32 / STEPS as f32));
        }
    }
    out
}

/// The smooth curve through `points` (pin, bend points, pin), sampled: Catmull-Rom tangents
/// inside, horizontal ones at the two pins ([`routed_chain`], sampled).
pub fn smooth_path(points: &[Point]) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }
    sample_chain(&routed_chain(points))
}

/// egui-snarl 0.12's degree-5 wire from an output at `from` to an input at `to`
/// (`wire_bezier_5`, with its frame downscaled to a sixth of the wire's length as snarl's default
/// style does). Its two impossible branches give the plain S-bend instead of panicking.
pub fn snarl_quintic(from: Point, to: Point) -> [Point; 6] {
    let (from, to) = (tame(from), tame(to));
    let frame_size = WIRE_FRAME.min((from - to).length() / 6.0);
    let from_2 = Point::new(from.x + frame_size, from.y);
    let to_2 = Point::new(to.x - frame_size, to.y);
    let p = Point::new;
    let between = (from_2 - to_2).length();
    if frame_size.is_nan() || frame_size <= 1e-4 {
        return [from, from_2, from_2, to_2, to_2, to];
    }
    let points = if from_2.x <= to_2.x && between >= frame_size * 2.0 {
        let middle_1 = from_2 + normalized(to_2 - from_2) * frame_size;
        let middle_2 = to_2 + normalized(from_2 - to_2) * frame_size;
        [from, from_2, middle_1, middle_2, to_2, to]
    } else if from_2.x <= to_2.x {
        let t = (between - (to_2.y - from_2.y).abs()) / frame_size.mul_add(2.0, -(to_2.y - from_2.y).abs());
        let mut middle_1 = from_2 + normalized(to_2 - from_2) * frame_size;
        let mut middle_2 = to_2 + normalized(from_2 - to_2) * frame_size;
        if from_2.y >= to_2.y + frame_size {
            let u = (from_2.y - to_2.y - frame_size) / frame_size;
            let t0_middle_1 = p((1.0 - u).mul_add(frame_size, from_2.x), frame_size.mul_add(-u, from_2.y));
            let t0_middle_2 = p(to_2.x, to_2.y + frame_size);
            middle_1 = lerp(t0_middle_1, middle_1, t);
            middle_2 = lerp(t0_middle_2, middle_2, t);
        } else if from_2.y >= to_2.y {
            let u = (from_2.y - to_2.y) / frame_size;
            let t0_middle_1 = p(u.mul_add(frame_size, from_2.x), frame_size.mul_add(1.0 - u, from_2.y));
            let t0_middle_2 = p(to_2.x, to_2.y + frame_size);
            middle_1 = lerp(t0_middle_1, middle_1, t);
            middle_2 = lerp(t0_middle_2, middle_2, t);
        } else if to_2.y >= from_2.y + frame_size {
            let u = (to_2.y - from_2.y - frame_size) / frame_size;
            let t0_middle_1 = p(from_2.x, from_2.y + frame_size);
            let t0_middle_2 = p((1.0 - u).mul_add(-frame_size, to_2.x), frame_size.mul_add(-u, to_2.y));
            middle_1 = lerp(t0_middle_1, middle_1, t);
            middle_2 = lerp(t0_middle_2, middle_2, t);
        } else {
            // to_2.y >= from_2.y (snarl's last case; the one left)
            let u = (to_2.y - from_2.y) / frame_size;
            let t0_middle_1 = p(from_2.x, from_2.y + frame_size);
            let t0_middle_2 = p(u.mul_add(-frame_size, to_2.x), frame_size.mul_add(1.0 - u, to_2.y));
            middle_1 = lerp(t0_middle_1, middle_1, t);
            middle_2 = lerp(t0_middle_2, middle_2, t);
        }
        [from, from_2, middle_1, middle_2, to_2, to]
    } else if from_2.y >= frame_size.mul_add(2.0, to_2.y) {
        [from, from_2, p(from_2.x, from_2.y - frame_size), p(to_2.x, to_2.y + frame_size), to_2, to]
    } else if from_2.y >= to_2.y + frame_size {
        let t = (from_2.y - to_2.y - frame_size) / frame_size;
        [from, from_2, p((1.0 - t).mul_add(frame_size, from_2.x), frame_size.mul_add(-t, from_2.y)), p(to_2.x, to_2.y + frame_size), to_2, to]
    } else if from_2.y >= to_2.y {
        let t = (from_2.y - to_2.y) / frame_size;
        [from, from_2, p(t.mul_add(frame_size, from_2.x), frame_size.mul_add(1.0 - t, from_2.y)), p(to_2.x, to_2.y + frame_size), to_2, to]
    } else if to_2.y >= frame_size.mul_add(2.0, from_2.y) {
        [from, from_2, p(from_2.x, from_2.y + frame_size), p(to_2.x, to_2.y - frame_size), to_2, to]
    } else if to_2.y >= from_2.y + frame_size {
        let t = (to_2.y - from_2.y - frame_size) / frame_size;
        [from, from_2, p(from_2.x, from_2.y + frame_size), p((1.0 - t).mul_add(-frame_size, to_2.x), frame_size.mul_add(-t, to_2.y)), to_2, to]
    } else {
        // to_2.y >= from_2.y, the last of snarl's cases (it has `unreachable!` after it)
        let t = (to_2.y - from_2.y) / frame_size;
        [from, from_2, p(from_2.x, from_2.y + frame_size), p(t.mul_add(-frame_size, to_2.x), frame_size.mul_add(1.0 - t, to_2.y)), to_2, to]
    };
    points.map(tame)
}

/// Split a Bézier curve of any degree at `t` (de Casteljau): the two halves' control points.
fn split<const N: usize>(q: [Point; N], t: f32) -> ([Point; N], [Point; N]) {
    let mut left = q;
    let mut right = q;
    let mut p = q;
    for k in 0..N {
        let last = N - 1 - k;
        if let (Some(l), Some(r), Some(head), Some(tail)) = (left.get_mut(k), right.get_mut(last), p.first(), p.get(last)) {
            *l = *head;
            *r = *tail;
        }
        for i in 0..last {
            if let (Some(a), Some(b)) = (p.get(i).copied(), p.get(i + 1).copied())
                && let Some(slot) = p.get_mut(i)
            {
                *slot = lerp(a, b, t);
            }
        }
    }
    (left, right)
}

/// The cubic with a degree-5 curve's ends and end tangents.
fn hermite(q: [Point; 6]) -> [Point; 3] {
    let k = 5.0 / 3.0;
    [q[0] + (q[1] - q[0]) * k, q[5] - (q[5] - q[4]) * k, q[5]]
}

/// A plain wire from an output at `from` to an input at `to`, as a chain of [`PLAIN_PIECES`]
/// cubics fitted to egui-snarl's curve ([`snarl_quintic`]).
pub fn plain_chain(from: Point, to: Point) -> Vec<Point> {
    let q = snarl_quintic(from, to);
    let mut out = Vec::with_capacity(3 * PLAIN_PIECES + 1);
    out.push(q[0]);
    let mut rest = q;
    for k in 0..PLAIN_PIECES {
        let left = PLAIN_PIECES - k;
        let piece = if left > 1 {
            let (l, r) = split(rest, 1.0 / left as f32);
            rest = r;
            l
        } else {
            rest
        };
        out.extend(hermite(piece).map(tame));
    }
    out
}

/// How far a wire routed backward keeps from the faces it leaves and enters (graph units).
pub const BACK_CLEAR: f32 = 24.0;
/// The radius of a routed backward wire's corners (graph units; less where a leg is short).
pub const BACK_CORNER: f32 = 12.0;
/// A quarter circle's cubic control distance, a radius's fraction.
const KAPPA: f32 = 0.552_284_8;

/// A wire that runs right to left (its input left of its output, N4 slice 2; the maintainer, N3
/// exit card 13: "Lay out engine could also add bend points on wires going right to left so as
/// to clear the corners of the sending and receiving nodes"): instead of egui-snarl's S-curve,
/// which cuts across both faces, it leaves its output to the right, turns, runs along a channel
/// clear of both faces, and comes into its input from the left, with rounded corners. The
/// channel is midway between the faces when one is wholly above the other, else below both or
/// above both, whichever is the shorter way. `a` and `b` are the source's and the target's faces.
/// None when the wire runs left to right (a plain wire's curve is used), or a rect isn't a
/// number. The chain is cubic, as every chain: straight legs and quarter turns, at most nine.
pub fn backward_chain(from: Point, to: Point, a: Rect, b: Rect) -> Option<Vec<Point>> {
    backward_corners(from, to, a, b).map(|c| rounded(&c, BACK_CORNER))
}

/// The corners of [`backward_chain`]'s route, from the output pin to the input pin: six points,
/// every leg horizontal or vertical (B155: the corners a person can drag).
pub fn backward_corners(from: Point, to: Point, a: Rect, b: Rect) -> Option<Vec<Point>> {
    let all = [from.x, from.y, to.x, to.y, a.min.x, a.min.y, a.max.x, a.max.y, b.min.x, b.min.y, b.max.x, b.max.y];
    if !all.iter().all(|v| v.is_finite()) || to.x >= from.x {
        return None;
    }
    let x1 = from.x.max(a.max.x) + BACK_CLEAR;
    let x2 = to.x.min(b.min.x) - BACK_CLEAR;
    let y = if b.min.y >= a.max.y {
        (a.max.y + b.min.y) / 2.0
    } else if b.max.y <= a.min.y {
        (b.max.y + a.min.y) / 2.0
    } else {
        let below = a.max.y.max(b.max.y) + BACK_CLEAR;
        let above = a.min.y.min(b.min.y) - BACK_CLEAR;
        let cost = |c: f32| (from.y - c).abs() + (to.y - c).abs();
        if cost(above) < cost(below) { above } else { below }
    };
    Some(vec![from, Point::new(x1, from.y), Point::new(x1, y), Point::new(x2, y), Point::new(x2, to.y), to])
}

/// Two coordinates within half a graph unit: a leg between points this close in x is vertical,
/// in y horizontal.
const ALIGNED: f32 = 0.5;

fn aligned(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() <= ALIGNED || (a.y - b.y).abs() <= ALIGNED
}

/// Whether a wire's own bend points make a right-angled route (B155): at least two, and each
/// next one straight across from or straight above or below the one before. Such a wire is
/// drawn with straight legs and rounded corners, not a curve through its points. No state is
/// kept for it: the points say it.
pub fn right_angled(via: &[[i32; 2]]) -> bool {
    via.len() >= 2 && via.windows(2).all(|w| aligned(as_point(&w[0]), as_point(&w[1])))
}

/// The corners of a right-angled wire (see [`right_angled`]), from its output pin to its input
/// pin: it leaves the output across to its first point's x, then up or down to it; through its
/// points; and from its last point up or down to the input's height, then across into it, so it
/// leaves and enters horizontally wherever its nodes have moved. Repeated points and points on
/// a straight leg are left out. None for a wire that isn't right-angled.
pub fn orthogonal_corners(from: Point, to: Point, via: &[[i32; 2]]) -> Option<Vec<Point>> {
    if !right_angled(via) || ![from.x, from.y, to.x, to.y].iter().all(|v| v.is_finite()) {
        return None;
    }
    let first = as_point(via.first()?);
    let last = as_point(via.last()?);
    let mut pts = vec![from, Point::new(first.x, from.y)];
    pts.extend(via.iter().map(as_point));
    pts.push(Point::new(last.x, to.y));
    pts.push(to);
    Some(simplified(&pts))
}

/// A polyline without repeated points, without points on the middle of a straight leg, and
/// without spurs (a leg that runs out and back along itself ends where it turned back).
pub fn simplified(points: &[Point]) -> Vec<Point> {
    let mut pts: Vec<Point> = Vec::with_capacity(points.len());
    for p in points.iter().copied().map(tame) {
        // what the point before ends up as may itself be on a line now: look back until not
        loop {
            if pts.last().is_some_and(|l: &Point| l.distance(p) < 1e-3) {
                break;
            }
            if pts.len() >= 2 {
                let (a, b) = (pts[pts.len() - 2], pts[pts.len() - 1]);
                let (u, v) = (b - a, p - b);
                if (u.x * v.y - u.y * v.x).abs() < 1e-3 {
                    pts.pop();
                    continue;
                }
            }
            pts.push(p);
            break;
        }
    }
    pts
}

/// The largest bend-point coordinate made here (graph units), as [`MAX_COORD`].
const MAX_VIA_AT: i64 = 10_000_000;

fn to_via(p: Point) -> [i32; 2] {
    let c = |v: f32| (v.round() as i64).clamp(-MAX_VIA_AT, MAX_VIA_AT) as i32;
    [c(p.x), c(p.y)]
}

/// Make Right-Angled (B155, `wire.square`): a wire's bend points moved so each leg is horizontal
/// or vertical. It leaves its output across; from each point to the next that isn't straight
/// across or above or below it, it goes across first, then up or down (the one choice where
/// either would do); into its input it comes up or down first, then across. Corners on a
/// straight leg and repeated points are left out, so a wire whose pins are level and whose
/// points lie on the line between them has none left (a plain wire). The result is always
/// right-angled ([`right_angled`]) or empty.
pub fn square_up(from: Point, to: Point, via: &[[i32; 2]]) -> Vec<[i32; 2]> {
    let mut pts = vec![from];
    let targets: Vec<Point> = via.iter().map(as_point).chain(std::iter::once(to)).collect();
    let n = targets.len();
    for (k, p) in targets.into_iter().enumerate() {
        let cur = pts.last().copied().unwrap_or(from);
        if !aligned(cur, p) {
            let elbow = if k + 1 == n { Point::new(cur.x, p.y) } else { Point::new(p.x, cur.y) };
            pts.push(elbow);
        }
        pts.push(p);
    }
    let s = simplified(&pts);
    let inner = if s.len() >= 2 { &s[1..s.len() - 1] } else { &[][..] };
    let out: Vec<[i32; 2]> = inner.iter().copied().map(to_via).collect();
    if out.is_empty() || right_angled(&out) { out } else { Vec::new() }
}

/// A right-angled route's bend points after a drag (B155): a corner (`segment` false, `index`
/// its bend point) moves by (`dx`, `dy`), and the points beside it that were straight across
/// from it or above or below it move with it on that axis, so every leg stays horizontal or
/// vertical; a leg (`segment` true, between bend points `index` and `index + 1`) moves across
/// itself: a horizontal one up or down, a vertical one left or right. An index out of range
/// changes nothing.
pub fn route_drag(via: &[[i32; 2]], index: usize, segment: bool, dx: f32, dy: f32) -> Vec<[i32; 2]> {
    let mut out = via.to_vec();
    let shift = |v: i32, d: f32| -> i32 { (i64::from(v) + d.round() as i64).clamp(-MAX_VIA_AT, MAX_VIA_AT) as i32 };
    if segment {
        let (Some(&a), Some(&b)) = (via.get(index), via.get(index + 1)) else { return out };
        let (horizontal, vertical) = (a[1] == b[1], a[0] == b[0]);
        for k in [index, index + 1] {
            if let Some(p) = out.get_mut(k) {
                if horizontal && !vertical {
                    p[1] = shift(p[1], dy);
                } else if vertical && !horizontal {
                    p[0] = shift(p[0], dx);
                } else {
                    *p = [shift(p[0], dx), shift(p[1], dy)];
                }
            }
        }
        return out;
    }
    let Some(&p) = via.get(index) else { return out };
    let moved = [shift(p[0], dx), shift(p[1], dy)];
    for k in [index.wrapping_sub(1), index + 1] {
        if let Some(q) = out.get_mut(k) {
            if q[0] == p[0] && q[1] != p[1] {
                q[0] = moved[0];
            } else if q[1] == p[1] && q[0] != p[0] {
                q[1] = moved[1];
            }
        }
    }
    if let Some(q) = out.get_mut(index) {
        *q = moved;
    }
    out
}

/// A polyline as a cubic chain: its legs straight, its corners rounded to radius `r` (less on a
/// short leg); points that repeat or lie on a line are left out.
pub fn rounded(points: &[Point], r: f32) -> Vec<Point> {
    let mut pts: Vec<Point> = Vec::with_capacity(points.len());
    for p in points.iter().copied().map(tame) {
        if pts.last().is_some_and(|l: &Point| l.distance(p) < 1e-3) {
            continue;
        }
        // a point on the line through the two before it is no corner
        if pts.len() >= 2 {
            let (a, b) = (pts[pts.len() - 2], pts[pts.len() - 1]);
            let (u, v) = (b - a, p - b);
            if (u.x * v.y - u.y * v.x).abs() < 1e-3 && u.x * v.x + u.y * v.y > 0.0 {
                pts.pop();
            }
        }
        pts.push(p);
    }
    let Some(&first) = pts.first() else { return Vec::new() };
    let mut out = vec![first];
    let line = |out: &mut Vec<Point>, to: Point| {
        if let Some(&at) = out.last() {
            out.push(tame(lerp(at, to, 1.0 / 3.0)));
            out.push(tame(lerp(at, to, 2.0 / 3.0)));
            out.push(tame(to));
        }
    };
    for k in 1..pts.len() {
        let p = pts[k];
        if k + 1 == pts.len() {
            line(&mut out, p);
            break;
        }
        let (prev, next) = (pts[k - 1], pts[k + 1]);
        let (din, dout) = (normalized(p - prev), normalized(next - p));
        let rk = r.min((p - prev).length() / 2.0).min((next - p).length() / 2.0).max(0.0);
        let (s, e) = (p - din * rk, p + dout * rk);
        line(&mut out, s);
        out.push(tame(s + din * (rk * KAPPA)));
        out.push(tame(e - dout * (rk * KAPPA)));
        out.push(tame(e));
    }
    out
}

/// The distance from `p` to a chain (each cubic sampled [`STEPS`] times); infinite for no chain,
/// and for a lone point its distance.
pub fn chain_distance(chain: &[Point], p: Point) -> f32 {
    if chain.len() == 1 {
        return chain.first().map_or(f32::INFINITY, |c| c.distance(p));
    }
    let mut best = f32::INFINITY;
    for c in cubics(chain) {
        let mut prev = c[0];
        for k in 1..=STEPS {
            let next = cubic_at(c, k as f32 / STEPS as f32);
            best = best.min(segment_distance(prev, next, p));
            prev = next;
        }
    }
    best
}

/// The distance from `p` to the line segment from `a` to `b`.
pub fn segment_distance(a: Point, b: Point, p: Point) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t).distance(p)
}

/// A rect holding the whole chain (its control points' bounds hold the curve); none for an empty
/// chain.
pub fn chain_bounds(chain: &[Point]) -> Option<Rect> {
    let first = *chain.first()?;
    Some(chain.iter().fold(Rect::from_min_max(first, first), |r, p| {
        Rect::from_min_max(Point::new(r.min.x.min(p.x), r.min.y.min(p.y)), Point::new(r.max.x.max(p.x), r.max.y.max(p.y)))
    }))
}

/// The point halfway along a chain (by its cubics: the middle one's middle, or the end of the
/// first half).
pub fn chain_middle(chain: &[Point]) -> Option<Point> {
    let all: Vec<[Point; 4]> = cubics(chain).collect();
    let n = all.len();
    if n == 0 {
        return chain.first().copied();
    }
    if n % 2 == 1 { all.get(n / 2).map(|c| cubic_at(*c, 0.5)) } else { all.get(n / 2).map(|c| c[0]) }
}

/// The distance from `p` to a sampled path.
pub fn path_distance(path: &[Point], p: Point) -> f32 {
    path.windows(2)
        .filter_map(|w| match w {
            [a, b] => {
                let ab = *b - *a;
                let t = if ab.length_sq() > 0.0 { ((p - *a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
                Some((*a + ab * t).distance(p))
            }
            _ => None,
        })
        .fold(f32::INFINITY, f32::min)
}

/// Where a new bend point at `q` goes among `via` (wire from `a` to `b`): before the first
/// control point after the closest stretch of the curve.
pub fn insert_index(a: Point, via: &[[i32; 2]], b: Point, q: Point) -> usize {
    let mut points = vec![a];
    points.extend(via.iter().map(as_point));
    points.push(b);
    let path = smooth_path(&points);
    // the closest sample, then which segment (STEPS samples each) it lies on
    let (best, _) = path.iter().enumerate().map(|(i, s)| (i, s.distance(q))).fold((0, f32::INFINITY), |acc, x| if x.1 < acc.1 { x } else { acc });
    (best.saturating_sub(1) / STEPS).min(via.len())
}

/// A bend point as a point.
pub fn as_point(v: &[i32; 2]) -> Point {
    Point::new(v[0] as f32, v[1] as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(Point::new(x0, y0), Point::new(x1, y1))
    }

    /// Does the chain stay out of `r` (sampled)?
    fn clear_of(chain: &[Point], r: Rect) -> bool {
        sample_chain(chain).iter().all(|p| !(p.x > r.min.x + 0.5 && p.x < r.max.x - 0.5 && p.y > r.min.y + 0.5 && p.y < r.max.y - 0.5))
    }

    #[test]
    fn a_wire_running_right_to_left_clears_both_faces() {
        // the target below and left of the source: the channel runs between them
        let (a, b) = (rect(300.0, 0.0, 500.0, 120.0), rect(0.0, 200.0, 180.0, 320.0));
        let (from, to) = (Point::new(500.0, 60.0), Point::new(0.0, 240.0));
        let c = backward_chain(from, to, a, b).unwrap();
        assert_eq!((c.len() - 1) % 3, 0, "a cubic chain");
        assert_eq!((c[0], *c.last().unwrap()), (from, to));
        assert!(clear_of(&c, a) && clear_of(&c, b), "{c:?}");
        let (lo, hi) = c.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        assert!(lo >= 60.0 - 1e-3 && hi <= 240.0 + 1e-3, "no detour above or below: {lo}..{hi}");
        // leaves to the right and enters from the left, level
        assert!(c[1].x > from.x && (c[1].y - from.y).abs() < 1e-3);
        let n = c.len();
        assert!(c[n - 2].x < to.x && (c[n - 2].y - to.y).abs() < 1e-3);
        // stacked: the target straight below the source, its input left of the output
        let (a, b) = (rect(0.0, 0.0, 200.0, 100.0), rect(0.0, 160.0, 200.0, 260.0));
        let c = backward_chain(Point::new(200.0, 50.0), Point::new(0.0, 200.0), a, b).unwrap();
        assert!(clear_of(&c, a) && clear_of(&c, b), "stacked: {c:?}");
        // a node directly behind (overlapping heights): round below or above, whichever is shorter
        let (a, b) = (rect(400.0, 0.0, 600.0, 100.0), rect(0.0, 20.0, 200.0, 140.0));
        let c = backward_chain(Point::new(600.0, 80.0), Point::new(0.0, 100.0), a, b).unwrap();
        assert!(clear_of(&c, a) && clear_of(&c, b), "behind: {c:?}");
        assert!(c.iter().any(|p| p.y >= 140.0 + BACK_CLEAR - 1e-3), "below both: {c:?}");
        let c = backward_chain(Point::new(600.0, 10.0), Point::new(0.0, 30.0), a, b).unwrap();
        assert!(c.iter().any(|p| p.y <= -BACK_CLEAR + 1e-3), "above both, nearer: {c:?}");
        // left to right is no routed wire; a NaN is none either
        assert!(backward_chain(Point::new(0.0, 0.0), Point::new(10.0, 0.0), a, b).is_none());
        assert!(backward_chain(Point::new(f32::NAN, 0.0), Point::new(-10.0, 0.0), a, b).is_none());
    }

    #[test]
    fn a_polyline_is_rounded_into_cubics() {
        let c = rounded(&[Point::new(0.0, 0.0), Point::new(50.0, 0.0), Point::new(100.0, 0.0), Point::new(100.0, 80.0)], 12.0);
        // the middle point is on a line: a leg, a corner, a leg
        assert_eq!(c.len(), 1 + 3 * 3, "{c:?}");
        assert!(rounded(&[], 5.0).is_empty());
        assert_eq!(rounded(&[Point::new(1.0, 1.0), Point::new(1.0, 1.0)], 5.0), vec![Point::new(1.0, 1.0)]);
    }

    #[test]
    fn the_curve_passes_through_its_points_and_leaves_pins_level() {
        let pts = [Point::new(0.0, 0.0), Point::new(100.0, 80.0), Point::new(200.0, 0.0)];
        let path = smooth_path(&pts);
        assert_eq!(path.len(), 2 * STEPS + 1);
        assert_eq!(path[0], pts[0]);
        assert!((path[STEPS] - pts[1]).length() < 1e-3, "through the bend point");
        assert!((path[2 * STEPS] - pts[2]).length() < 1e-3);
        // leaving the output: heading right, level at first
        let first = path[1] - path[0];
        assert!(first.x > 0.0 && first.y.abs() < first.x * 0.6, "{first:?}");
        // entering the input from the left
        let last = path[2 * STEPS] - path[2 * STEPS - 1];
        assert!(last.x > 0.0 && last.y.abs() < last.x * 0.6, "{last:?}");
        assert!(path_distance(&path, Point::new(100.0, 80.0)) < 1e-3);
        assert!(path_distance(&path, Point::new(100.0, 300.0)) > 100.0);
        assert_eq!(smooth_path(&pts[..1]).len(), 1);
        assert_eq!(path_distance(&[], Point::ZERO), f32::INFINITY);
    }

    #[test]
    fn a_new_bend_point_goes_where_it_was_clicked() {
        let (a, b) = (Point::new(0.0, 0.0), Point::new(400.0, 0.0));
        let via = [[100, 100], [300, 100]];
        assert_eq!(insert_index(a, &via, b, Point::new(40.0, 50.0)), 0, "before the first");
        assert_eq!(insert_index(a, &via, b, Point::new(200.0, 110.0)), 1, "between them");
        assert_eq!(insert_index(a, &via, b, Point::new(370.0, 40.0)), 2, "after the last");
        assert_eq!(insert_index(a, &[], b, Point::new(200.0, 0.0)), 0);
    }
}

#[cfg(test)]
mod chain_tests {
    use super::*;

    fn quintic_at(q: [Point; 6], t: f32) -> Point {
        let mut p = q;
        for k in (1..6).rev() {
            for i in 0..k {
                p[i] = lerp(p[i], p[i + 1], t);
            }
        }
        p[0]
    }

    #[test]
    fn a_plain_wire_is_four_cubics_close_to_snarls_curve() {
        for to in [
            (300.0, 100.0),
            (300.0, 0.0),
            (120.0, 300.0),
            (80.0, 20.0),
            (600.0, -400.0),
            (-300.0, 200.0),
            (-50.0, 0.0),
            (10.0, 300.0),
            (5000.0, -3000.0),
            (40.0, 400.0),
        ] {
            let (a, b) = (Point::new(0.0, 0.0), Point::new(to.0, to.1));
            let chain = plain_chain(a, b);
            assert_eq!(chain.len(), 3 * PLAIN_PIECES + 1);
            assert_eq!((chain[0], chain[3 * PLAIN_PIECES]), (a, b), "{to:?}");
            let q = snarl_quintic(a, b);
            let worst = (0..=200).map(|i| chain_distance(&chain, quintic_at(q, i as f32 / 200.0))).fold(0.0f32, f32::max);
            // backward wires bend tighter than forward ones; still well under the wire's width
            let limit = if to.0 > 2.0 * WIRE_FRAME { 0.8 } else { 1.25 };
            assert!(worst < limit, "{to:?}: {worst}");
        }
    }

    #[test]
    fn degenerate_wires_are_still_numbers() {
        let z = Point::new(5.0, 5.0);
        for (a, b) in [
            (z, z),
            (Point::new(f32::NAN, 0.0), z),
            (Point::new(f32::INFINITY, f32::NEG_INFINITY), Point::new(0.0, f32::NAN)),
            (Point::new(1e30, 1e30), Point::new(-1e30, 3.0)),
            (Point::new(0.0, 0.0), Point::new(0.0, 1e-7)),
        ] {
            let c = plain_chain(a, b);
            assert!(c.iter().all(|p| p.x.is_finite() && p.y.is_finite()), "{a:?} {b:?}: {c:?}");
            assert!(chain_distance(&c, Point::ZERO).is_finite());
            let r = routed_chain(&[a, Point::new(f32::NAN, 1.0), b]);
            assert!(r.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        }
        assert_eq!(chain_distance(&[], Point::ZERO), f32::INFINITY);
        assert!(chain_bounds(&[]).is_none());
        assert_eq!(chain_middle(&[]), None);
        assert_eq!(chain_middle(&[z]), Some(z));
    }

    #[test]
    fn a_routed_chain_is_the_smooth_curve() {
        let pts = [Point::new(0.0, 0.0), Point::new(100.0, 80.0), Point::new(200.0, 0.0)];
        let chain = routed_chain(&pts);
        assert_eq!(chain.len(), 7);
        assert_eq!((chain[0], chain[3], chain[6]), (pts[0], pts[1], pts[2]), "through the bend point");
        let sampled = sample_chain(&chain);
        assert_eq!(sampled, smooth_path(&pts));
        // splitting a curve keeps it: the halves meet on it
        let q = snarl_quintic(Point::ZERO, Point::new(300.0, 100.0));
        let (l, r) = split(q, 0.5);
        assert_eq!(l[5], r[0]);
        assert!((l[5] - quintic_at(q, 0.5)).length() < 1e-3);
    }
}
