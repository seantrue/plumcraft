//! A wire's path through its bend points (`Wire.via`), and hit testing on it: the geometry of
//! the egui shell's `wire_route`, moved to the core (stage N1) so every shell routes and hits a
//! wire the same way. Lifted from the research software (`crates/ui-egui/src/wire_route.rs`).
//!
//! **The curve:** a Catmull-Rom spline through the output pin, the bend points and the input
//! pin, as cubic Béziers sampled [`STEPS`] times a segment. It leaves the output and enters the
//! input horizontally, as egui-snarl's wires do.

use crate::plain::Point;

/// Samples per curve segment.
pub const STEPS: usize = 24;

/// The smooth curve through `points` (pin, bend points, pin), sampled: Catmull-Rom tangents
/// inside, horizontal ones at the two pins.
pub fn smooth_path(points: &[Point]) -> Vec<Point> {
    let n = points.len();
    if n < 2 {
        return points.to_vec();
    }
    let at = |i: usize| points.get(i).copied().unwrap_or(Point::ZERO);
    let tangent = |i: usize| -> Point {
        if i == 0 {
            Point::new((at(1) - at(0)).length(), 0.0)
        } else if i == n - 1 {
            Point::new((at(n - 1) - at(n - 2)).length(), 0.0)
        } else {
            (at(i + 1) - at(i - 1)) * 0.5
        }
    };
    let mut out = Vec::with_capacity((n - 1) * STEPS + 1);
    out.push(at(0));
    for i in 0..n - 1 {
        let (p0, p3) = (at(i), at(i + 1));
        let (p1, p2) = (p0 + tangent(i) / 3.0, p3 - tangent(i + 1) / 3.0);
        for k in 1..=STEPS {
            let t = k as f32 / STEPS as f32;
            let u = 1.0 - t;
            out.push(p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t));
        }
    }
    out
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
