//! Tone curves: impressionist's `core.curves` (what the research software's Apply Curve computes with, and
//! its curve editor draws), in the same arithmetic and order.
//!
//! A curve set is openrv-web's ColorCurvesData, as the research software stores it:
//! `{"master": {"points": [{"x": 0, "y": 0}, {"x": 1, "y": 1}], "enabled": true}, "red": …,
//! "green": …, "blue": …}`, points in 0..1 (a point may also be `[x, y]`). ComfyUI's older
//! widget value `{"points": [[x, y], …]}` is the master curve. Each channel takes the master
//! curve, then its own (red, green, blue for the first three channels).

use serde_json::{Map, Value};

pub const CHANNEL_KEYS: [&str; 4] = ["master", "red", "green", "blue"];

/// The most points a curve may have.
pub const MAX_POINTS: usize = 4096;

/// One curve: its points sorted by x, and whether it is on.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub points: Vec<(f64, f64)>,
    pub enabled: bool,
}

/// `json.dumps(default_curves())`: every curve the identity.
pub fn default_json() -> String {
    let one = r#"{"points": [{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 1.0}], "enabled": true}"#;
    format!(r#"{{"master": {one}, "red": {one}, "green": {one}, "blue": {one}}}"#)
}

fn identity() -> Curve {
    Curve { points: vec![(0.0, 0.0), (1.0, 1.0)], enabled: true }
}

/// Python's truth of a JSON value.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python's `float(v)` of a JSON value.
fn number(v: &Value) -> Result<f64, String> {
    let f = match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    f.filter(|f| f.is_finite()).ok_or_else(|| format!("{v} is not a number"))
}

/// `channel_points`: a channel's points, sorted by x (stably), and whether it is enabled.
fn channel(v: Option<&Value>, key: &str) -> Result<Curve, String> {
    let Some(v) = v.filter(|v| truthy(v)) else { return Ok(identity()) };
    let Value::Object(c) = v else { return Err(format!("curves: {key} must be an object with points")) };
    let mut points = Vec::new();
    if let Some(ps) = c.get("points").filter(|p| truthy(p)) {
        let Value::Array(ps) = ps else { return Err(format!("curves: {key}.points must be a list")) };
        if ps.len() > MAX_POINTS {
            return Err(format!("curves: {key} has {} points (at most {MAX_POINTS})", ps.len()));
        }
        for p in ps {
            match p {
                Value::Object(o) => {
                    let x = o.get("x").map_or(Ok(0.0), number).map_err(|e| format!("curves: {key}: x: {e}"))?;
                    let y = o.get("y").map_or(Ok(0.0), number).map_err(|e| format!("curves: {key}: y: {e}"))?;
                    points.push((x, y));
                }
                Value::Array(a) if a.len() >= 2 => {
                    let x = a.first().map_or(Ok(0.0), number).map_err(|e| format!("curves: {key}: {e}"))?;
                    let y = a.get(1).map_or(Ok(0.0), number).map_err(|e| format!("curves: {key}: {e}"))?;
                    points.push((x, y));
                }
                _ => {}
            }
        }
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let enabled = c.get("enabled").is_none_or(truthy);
    Ok(Curve { points, enabled })
}

/// `parse`: a curve set from the widget's JSON (blank: every curve the identity).
pub fn parse(text: &str) -> Result<[Curve; 4], String> {
    if text.len() > 4 << 20 {
        return Err("curves: the JSON is too long".into());
    }
    let value: Value =
        if text.trim().is_empty() { Value::Object(Map::new()) } else { serde_json::from_str(text).map_err(|e| format!("curves: not JSON: {e}"))? };
    let o = match value {
        Value::Null => Map::new(),
        Value::Object(o) => o,
        _ => return Err("curves: expected an object such as {\"master\": {\"points\": [...]}}".into()),
    };
    if o.contains_key("points") && !o.contains_key("master") {
        // ComfyUI's own widget: the master curve
        let mut wrapped = Map::new();
        wrapped.insert("points".into(), o.get("points").cloned().unwrap_or(Value::Null));
        wrapped.insert("enabled".into(), Value::Bool(true));
        let master = channel(Some(&Value::Object(wrapped)), "points")?;
        return Ok([master, identity(), identity(), identity()]);
    }
    Ok([channel(o.get("master"), "master")?, channel(o.get("red"), "red")?, channel(o.get("green"), "green")?, channel(o.get("blue"), "blue")?])
}

/// `_slopes`: each point's slope, the secant through its neighbours (the end segments' own at
/// the ends; 0 where they share an x).
fn slopes(xs: &[f64], ys: &[f64]) -> Vec<f64> {
    let n = xs.len();
    (0..n)
        .map(|i| {
            let lo = i.saturating_sub(1);
            let hi = (i + 1).min(n - 1);
            let dx = xs.get(hi).copied().unwrap_or(0.0) - xs.get(lo).copied().unwrap_or(0.0);
            if dx != 0.0 { (ys.get(hi).copied().unwrap_or(0.0) - ys.get(lo).copied().unwrap_or(0.0)) / dx } else { 0.0 }
        })
        .collect()
}

/// A curve ready to evaluate.
struct Prepared {
    xs: Vec<f64>,
    ys: Vec<f64>,
    m: Vec<f64>,
}

impl Prepared {
    fn new(points: &[(f64, f64)]) -> Prepared {
        let xs: Vec<f64> = points.iter().map(|p| p.0).collect();
        let ys: Vec<f64> = points.iter().map(|p| p.1).collect();
        let m = slopes(&xs, &ys);
        Prepared { xs, ys, m }
    }

    /// `evaluate` at one value (n ≥ 2): cubic Hermite segments, clipped to 0..1, flat beyond
    /// the end points; NaN stays NaN.
    fn at(&self, x: f64) -> f64 {
        let n = self.xs.len();
        let g = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(0.0);
        let found = if x.is_nan() { n } else { self.xs.partition_point(|&v| v < x) };
        let i = (found as i64 - 1).clamp(0, n as i64 - 2) as usize;
        let (x1, x2, y1, y2, m1, m2) = (g(&self.xs, i), g(&self.xs, i + 1), g(&self.ys, i), g(&self.ys, i + 1), g(&self.m, i), g(&self.m, i + 1));
        let h = x2 - x1;
        let safe = if h == 0.0 { 1.0 } else { h };
        let t = (x - x1) / safe;
        let t2 = t * t;
        let t3 = t2 * t;
        let a = (2.0 * t3 - 3.0 * t2 + 1.0) * y1;
        let b = (t3 - 2.0 * t2 + t) * h * m1;
        let c = (-2.0 * t3 + 3.0 * t2) * y2;
        let d = (t3 - t2) * h * m2;
        let mut y = a + b + c + d;
        if h == 0.0 {
            y = y2;
        }
        y = y.clamp(0.0, 1.0);
        if x <= g(&self.xs, 0) {
            y = g(&self.ys, 0);
        }
        if x >= g(&self.xs, n - 1) {
            y = g(&self.ys, n - 1);
        }
        y
    }
}

/// `curve`: values through one curve, or unchanged when it is off, empty or the identity.
fn curve(values: &mut [f64], c: &Curve) {
    let p = &c.points;
    let is_identity = p.len() == 2 && p.first() == Some(&(0.0, 0.0)) && p.get(1) == Some(&(1.0, 1.0));
    if !c.enabled || p.is_empty() || is_identity {
        return;
    }
    if let [(_, y)] = p.as_slice() {
        values.iter_mut().for_each(|v| *v = *y);
        return;
    }
    let prepared = Prepared::new(p);
    values.iter_mut().for_each(|v| *v = prepared.at(*v));
}

/// `curve_array` for channels: each on 0..1 (clipped, or mapped from its own min..max with
/// `own_range`, float32 as numpy does it), through the master curve and then channel `k`'s
/// (red, green, blue for k = 0, 1, 2), and with `own_range` back to its own min..max.
pub fn curve_planes(planes: &[Vec<f32>], curves: &[Curve; 4], own_range: bool) -> Vec<Vec<f32>> {
    let [master, red, green, blue] = curves;
    let own = [red, green, blue];
    planes
        .iter()
        .enumerate()
        .map(|(k, plane)| {
            let (unit, span) = if own_range {
                let (lo, hi) = super::kernels::nan_min_max(plane);
                let d = hi - lo;
                let d = if d == 0.0 { 1.0f32 } else { d as f32 };
                let lo = lo as f32;
                (plane.iter().map(|&v| (v - lo) / d).collect::<Vec<f32>>(), Some((lo, d)))
            } else {
                (plane.iter().map(|&v| if v.is_nan() { v } else { v.clamp(0.0, 1.0) }).collect(), None)
            };
            let mut y: Vec<f64> = unit.iter().map(|&v| f64::from(v)).collect();
            curve(&mut y, master);
            if let Some(c) = own.get(k) {
                curve(&mut y, c);
            }
            let y32 = y.iter().map(|&v| v as f32);
            match span {
                Some((lo, d)) => y32.map(|v| lo + v * d).collect(),
                None => y32.collect(),
            }
        })
        .collect()
}
