//! A curves value: tone curves, as the array engine's Apply Curve takes them and the `curves` value
//! editor edits them (`docs/interface.md` 7.4; `docs/shell-interface.md` 12.21). Toolkit-free and
//! engine-free: the one model the array engine computes with, the core's `curves.*` commands edit
//! with, and the shells' drawing ports are pinned to (`fixtures/curves.json`).
//!
//! **The value** is JSON, an object or the same object as a string (openrv-web's ColorCurvesData):
//! `{"master": {"points": [{"x": 0, "y": 0}, {"x": 1, "y": 1}], "enabled": true}, "red": …,
//! "green": …, "blue": …}`, points in 0..1 (a point may also be `[x, y]`). ComfyUI's older widget
//! value `{"points": [[x, y], …]}` is the master curve. A missing curve is the identity. Channel k
//! goes through the master curve, then its own (red, green, blue for the first three channels).
//!
//! **The curve through a channel's points** is cubic Hermite with each point's slope the secant
//! through its neighbours (the end segments' own at the ends, 0 where two points share an x), so
//! its slope is continuous at every point; clamped to 0..1, flat beyond the end points. One point:
//! that y everywhere. A curve that is off, empty or the identity (two points, (0, 0) and (1, 1))
//! changes nothing.
//!
//! **Editing** (what an editor's gestures do, and the `curves.*` commands): a point is added
//! between the points around its x (at most [`EDIT_MAX_POINTS`]); a moved point stays between its
//! neighbours, [`MIN_GAP`] apart; a curve keeps at least two points.

use serde_json::{Map, Value, json};

/// The channels' keys, in the order channels take them.
pub const CHANNEL_KEYS: [&str; 4] = ["master", "red", "green", "blue"];

/// The most points a curve may have when read.
pub const MAX_POINTS: usize = 4096;

/// The most points an edit leaves on a curve (an editor's limit, as the value editors have kept it).
pub const EDIT_MAX_POINTS: usize = 64;

/// The least x between neighbouring points an edit leaves.
pub const MIN_GAP: f64 = 1e-3;

/// The longest curves value read, in bytes.
pub const MAX_JSON: usize = 4 << 20;

/// One curve: its points sorted by x, and whether it is on.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub points: Vec<(f64, f64)>,
    pub enabled: bool,
}

impl Default for Curve {
    fn default() -> Self {
        identity()
    }
}

/// The identity curve: (0, 0) to (1, 1), on.
pub fn identity() -> Curve {
    Curve { points: vec![(0.0, 0.0), (1.0, 1.0)], enabled: true }
}

impl Curve {
    /// Two points, (0, 0) and (1, 1).
    pub fn is_identity(&self) -> bool {
        self.points.len() == 2 && self.points.first() == Some(&(0.0, 0.0)) && self.points.get(1) == Some(&(1.0, 1.0))
    }

    /// Does this curve change values?
    pub fn applies(&self) -> bool {
        self.enabled && !self.points.is_empty() && !self.is_identity()
    }

    /// The curve at `x` ([`evaluate`]), or `x` when it doesn't apply.
    pub fn at(&self, x: f64) -> f64 {
        if self.applies() { evaluate(&self.points, x) } else { x }
    }

    /// Add a point at (x, y), each held to 0..1, after the points at or before its x. Its index,
    /// or none when the curve has [`EDIT_MAX_POINTS`] already.
    pub fn insert(&mut self, x: f64, y: f64) -> Option<usize> {
        if self.points.len() >= EDIT_MAX_POINTS || !x.is_finite() || !y.is_finite() {
            return None;
        }
        let (x, y) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
        let i = self.points.iter().position(|p| p.0 > x).unwrap_or(self.points.len());
        self.points.insert(i, (x, y));
        Some(i)
    }

    /// Move point `i` to (x, y): y held to 0..1, x to 0..1 and between its neighbours
    /// ([`MIN_GAP`] apart; where they are closer than that, x stays). False for no such point.
    pub fn move_point(&mut self, i: usize, x: f64, y: f64) -> bool {
        let n = self.points.len();
        if i >= n || !x.is_finite() || !y.is_finite() {
            return false;
        }
        let lo = if i > 0 { self.points[i - 1].0 + MIN_GAP } else { 0.0 };
        let hi = if i + 1 < n { self.points[i + 1].0 - MIN_GAP } else { 1.0 };
        let x = if lo <= hi { x.clamp(lo, hi) } else { self.points[i].0 };
        self.points[i] = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
        true
    }

    /// Remove point `i`, unless only two are left. Whether it went.
    pub fn remove(&mut self, i: usize) -> bool {
        if self.points.len() <= 2 || i >= self.points.len() {
            return false;
        }
        self.points.remove(i);
        true
    }

    /// The nearest point to (x, y) within `reach` (in the same units, the plot's 0..1), if any.
    pub fn hit(&self, x: f64, y: f64, reach: f64) -> Option<usize> {
        self.points
            .iter()
            .enumerate()
            .map(|(i, p)| (i, ((p.0 - x).powi(2) + (p.1 - y).powi(2)).sqrt()))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }
}

/// A master curve and the red, green and blue curves.
pub type CurveSet = [Curve; 4];

/// The identity curve set.
pub fn identity_set() -> CurveSet {
    [identity(), identity(), identity(), identity()]
}

/// `json.dumps(default_curves())`: every curve the identity, as the array engine's default.
pub fn default_json() -> String {
    let one = r#"{"points": [{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 1.0}], "enabled": true}"#;
    format!(r#"{{"master": {one}, "red": {one}, "green": {one}, "blue": {one}}}"#)
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

/// A channel's points, sorted by x (stably), and whether it is on.
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

/// A curve set from its JSON text (blank: every curve the identity). Refused, with why: text
/// that isn't JSON, a value that isn't an object, a channel that isn't one, more than
/// [`MAX_POINTS`] points, a coordinate that isn't a finite number.
pub fn parse(text: &str) -> Result<CurveSet, String> {
    if text.len() > MAX_JSON {
        return Err("curves: the JSON is too long".into());
    }
    let value: Value =
        if text.trim().is_empty() { Value::Object(Map::new()) } else { serde_json::from_str(text).map_err(|e| format!("curves: not JSON: {e}"))? };
    parse_object(value)
}

/// A curve set from a node's value: the JSON as a string, or the object itself.
pub fn parse_value(value: &Value) -> Result<CurveSet, String> {
    match value {
        Value::String(s) => parse(s),
        other => parse_object(other.clone()),
    }
}

fn parse_object(value: Value) -> Result<CurveSet, String> {
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

/// The value as JSON text, every channel written: `{"blue": {"enabled", "points": [{"x", "y"}]},
/// "green", "master", "red"}` (keys in this order, as a JSON object sorted by key).
pub fn to_json(set: &CurveSet) -> String {
    let mut o = Map::new();
    for (key, c) in CHANNEL_KEYS.iter().zip(set) {
        let pts: Vec<Value> = c.points.iter().map(|p| json!({"x": p.0, "y": p.1})).collect();
        o.insert((*key).to_owned(), json!({"points": pts, "enabled": c.enabled}));
    }
    Value::Object(o).to_string()
}

/// Each point's slope: the secant through its neighbours (the end segments' own at the ends; 0
/// where they share an x).
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

/// A curve ready to evaluate many values (the array engine's whole planes).
pub struct Prepared {
    xs: Vec<f64>,
    ys: Vec<f64>,
    m: Vec<f64>,
}

impl Prepared {
    /// For `points` sorted by x, at least two.
    pub fn new(points: &[(f64, f64)]) -> Prepared {
        let xs: Vec<f64> = points.iter().map(|p| p.0).collect();
        let ys: Vec<f64> = points.iter().map(|p| p.1).collect();
        let m = slopes(&xs, &ys);
        Prepared { xs, ys, m }
    }

    /// The curve at `x` (n ≥ 2): cubic Hermite segments, clamped to 0..1, flat beyond the end
    /// points; NaN stays NaN.
    pub fn at(&self, x: f64) -> f64 {
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

/// The curve through `points` (sorted by x) at `x`: one point's y everywhere; none, `x`.
pub fn evaluate(points: &[(f64, f64)], x: f64) -> f64 {
    match points {
        [] => x,
        [(_, y)] => *y,
        _ => Prepared::new(points).at(x),
    }
}

/// Values through one curve, in place, or unchanged when it doesn't apply.
pub fn apply_curve(values: &mut [f64], c: &Curve) {
    if !c.applies() {
        return;
    }
    if let [(_, y)] = c.points.as_slice() {
        values.iter_mut().for_each(|v| *v = *y);
        return;
    }
    let prepared = Prepared::new(&c.points);
    values.iter_mut().for_each(|v| *v = prepared.at(*v));
}

/// Channel `k` (0, 1, 2; a fourth and beyond take the master only) at `x`.
pub fn apply(set: &CurveSet, k: usize, x: f64) -> f64 {
    let y = set[0].at(x);
    match set.get(k + 1) {
        Some(own) if k < 3 => own.at(y),
        _ => y,
    }
}

/// An 8-bit lookup table for channel `k`, for a preview of an 8-bit picture: entry i is the
/// curve at i/255, rounded to 8 bits.
pub fn lut8(set: &CurveSet, k: usize) -> [u8; 256] {
    let mut lut = [0u8; 256];
    for (i, v) in lut.iter_mut().enumerate() {
        let y = apply(set, k, i as f64 / 255.0) as f32;
        *v = (y.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
    lut
}

/// A preset: its name, what it does, and its four points.
pub type Preset = (&'static str, &'static str, [(f64, f64); 4]);

/// The presets, by name: S curves (more contrast) and an inverse S (less), for one curve.
pub const PRESETS: [Preset; 4] = [
    ("Light S", "A little more contrast", [(0.0, 0.0), (0.25, 0.2), (0.75, 0.8), (1.0, 1.0)]),
    ("Medium S", "More contrast", [(0.0, 0.0), (0.25, 0.15), (0.75, 0.85), (1.0, 1.0)]),
    ("Strong S", "Much more contrast", [(0.0, 0.0), (0.25, 0.08), (0.75, 0.92), (1.0, 1.0)]),
    ("Inverse S", "Less contrast", [(0.0, 0.0), (0.25, 0.33), (0.75, 0.67), (1.0, 1.0)]),
];

/// The points of preset `name`, if there is one.
pub fn preset(name: &str) -> Option<Vec<(f64, f64)>> {
    PRESETS.iter().find(|(n, ..)| *n == name).map(|(_, _, p)| p.to_vec())
}

/// How many points Equalize places.
pub const EQUALIZE_POINTS: usize = 9;

/// A histogram-equalizing curve for `hist` (bins over 0..1): `n` points at its quantiles, so
/// point k sits where k/(n-1) of the values lie below, at height k/(n-1). Several quantiles in
/// one place make one point (the last still reaching the top); a single value everywhere makes a
/// step there. `None` for an empty histogram.
pub fn equalize_points(hist: &[f64], n: usize) -> Option<Vec<(f64, f64)>> {
    let total: f64 = hist.iter().filter(|c| c.is_finite() && **c > 0.0).sum();
    if hist.is_empty() || total <= 0.0 || n < 2 {
        return None;
    }
    let bins = hist.len() as f64;
    // the cumulative fraction at each bin's right edge
    let mut cdf = Vec::with_capacity(hist.len());
    let mut acc = 0.0;
    for c in hist {
        acc += if c.is_finite() && *c > 0.0 { *c } else { 0.0 };
        cdf.push(acc / total);
    }
    // x where the cdf first reaches q, linear within the bin (q = 0: where the values start)
    let quantile = |q: f64| -> f64 {
        let i = cdf.iter().position(|v| *v > 0.0 && *v >= q - 1e-12).unwrap_or(cdf.len() - 1);
        let below = if i == 0 { 0.0 } else { cdf.get(i - 1).copied().unwrap_or(0.0) };
        let span = cdf.get(i).copied().unwrap_or(0.0) - below;
        let t = if span > 0.0 { ((q - below) / span).clamp(0.0, 1.0) } else { 0.0 };
        (i as f64 + t) / bins
    };
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(n);
    for k in 0..n {
        let q = k as f64 / (n - 1) as f64;
        let x = quantile(q).clamp(0.0, 1.0);
        match points.last_mut() {
            Some(last) if x <= last.0 + MIN_GAP => {
                if k == n - 1 {
                    last.1 = 1.0;
                }
            }
            _ => points.push((x, q)),
        }
    }
    if let [only] = points.as_slice() {
        let x = only.0.clamp(0.0, 1.0 - MIN_GAP);
        points = vec![(x, 0.0), (x + MIN_GAP, 1.0)];
    }
    (points.len() >= 2).then_some(points)
}

/// The histogram a curve tab shows, from per-channel histograms: its channel's (tab 1, 2, 3), or
/// the channels' mean for the master curve (tab 0).
pub fn tab_histogram(histograms: &[Vec<u64>], tab: usize) -> Vec<f64> {
    match tab {
        0 if !histograms.is_empty() => {
            let n = histograms.iter().map(Vec::len).max().unwrap_or(0);
            (0..n).map(|i| histograms.iter().map(|h| h.get(i).copied().unwrap_or(0) as f64).sum::<f64>() / histograms.len() as f64).collect()
        }
        k => histograms.get(k.wrapping_sub(1)).map(|h| h.iter().map(|c| *c as f64).collect()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shared vectors (`fixtures/curves.json`): each case's points and the curve at each x,
    /// which the shells' drawing ports and every copy of this maths must give.
    #[test]
    fn the_shared_vectors_hold() {
        let text = include_str!("../fixtures/curves.json");
        let v: Value = serde_json::from_str(text).unwrap();
        let cases = v["cases"].as_array().unwrap();
        assert!(cases.len() >= 8);
        for c in cases {
            let points: Vec<(f64, f64)> = c["point_bits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    let b = |k: usize| f64::from_bits(u64::from_str_radix(p[k].as_str().unwrap(), 16).unwrap());
                    (b(0), b(1))
                })
                .collect();
            for s in c["samples"].as_array().unwrap() {
                // exact by bit pattern: a JSON reader may round a decimal an ulp off
                let bits = |k: usize| f64::from_bits(u64::from_str_radix(s[k].as_str().unwrap(), 16).unwrap());
                let (x, want) = (bits(2), bits(3));
                let got = evaluate(&points, x);
                assert_eq!(got.to_bits(), want.to_bits(), "{}: at {x}: {got} against {want}", c["name"]);
            }
        }
    }

    /// Write the vectors again from this code (`PLUMCRAFT_WRITE_FIXTURES=1`): the curves and the
    /// x values are chosen here; the y values are what this code computes.
    #[test]
    fn write_the_shared_vectors() {
        if std::env::var_os("PLUMCRAFT_WRITE_FIXTURES").is_none() {
            return;
        }
        let cases: Vec<(&str, Vec<(f64, f64)>)> = vec![
            ("identity", vec![(0.0, 0.0), (1.0, 1.0)]),
            ("one point", vec![(0.4, 0.7)]),
            ("light S", PRESETS[0].2.to_vec()),
            ("strong S", PRESETS[2].2.to_vec()),
            ("inverse S", PRESETS[3].2.to_vec()),
            ("overshoot clamped", vec![(0.0, 0.0), (0.1, 0.9), (0.2, 0.1), (1.0, 1.0)]),
            ("inside the ends", vec![(0.2, 0.3), (0.5, 0.8), (0.8, 0.6)]),
            ("shared x", vec![(0.0, 0.0), (0.5, 0.2), (0.5, 0.8), (1.0, 1.0)]),
            ("five points", vec![(0.0, 0.1), (0.2, 0.15), (0.45, 0.6), (0.7, 0.65), (1.0, 0.95)]),
            ("falling", vec![(0.0, 1.0), (0.3, 0.6), (1.0, 0.0)]),
        ];
        let xs: Vec<f64> = (0..=40).map(|i| f64::from(i) / 40.0).chain([-0.25, 1.5, 0.123_456_789, 0.999_999]).collect();
        let out: Vec<Value> = cases
            .iter()
            .map(|(name, pts)| {
                let hex = |v: f64| format!("{:016x}", v.to_bits());
                let samples: Vec<Value> = xs
                    .iter()
                    .map(|&x| {
                        let y = evaluate(pts, x);
                        json!([x, y, hex(x), hex(y)])
                    })
                    .collect();
                json!({"name": name, "points": pts.iter().map(|p| json!([p.0, p.1])).collect::<Vec<_>>(),
                       "point_bits": pts.iter().map(|p| json!([hex(p.0), hex(p.1)])).collect::<Vec<_>>(), "samples": samples})
            })
            .collect();
        let doc = json!({
            "about": "Shared test vectors for the curves value (crates/proto/src/curves.rs): each case's points (x, y in 0..1, sorted by x; `point_bits` the same as IEEE 754 binary64 bit patterns in hex) and the curve at each x, `[x, y, x bits, y bits]`. Cubic Hermite with secant slopes, clamped to 0..1, flat beyond the ends; one point is its y everywhere. Every copy of this maths, and the shells' drawing ports, must give these y values exactly: compare by the bit patterns, since a JSON reader may round a decimal an ulp off. Written by `PLUMCRAFT_WRITE_FIXTURES=1 cargo test -p plumcraft-proto curves`.",
            "cases": out,
        });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/curves.json");
        std::fs::write(path, serde_json::to_string_pretty(&doc).unwrap() + "\n").unwrap();
    }

    #[test]
    fn a_value_reads_as_an_object_a_string_or_comfyuis_points() {
        let set = parse_value(&json!({"master": {"points": [[0, 0], {"x": 0.5, "y": 0.7}, [1, 1]], "enabled": true}})).unwrap();
        assert_eq!(set[0].points, vec![(0.0, 0.0), (0.5, 0.7), (1.0, 1.0)]);
        assert!(set[1].is_identity() && !set[1].applies());
        let again = parse_value(&json!(to_json(&set))).unwrap();
        assert_eq!(again, set, "to_json reads back");
        let comfy = parse(r#"{"points": [[0, 0.2], [1, 0.8]]}"#).unwrap();
        assert_eq!(comfy[0].points, vec![(0.0, 0.2), (1.0, 0.8)]);
        assert_eq!(parse("").unwrap(), identity_set());
        assert!(parse("[1]").is_err() && parse("{\"master\": 3}").is_err() && parse("nope").is_err());
        assert!(parse(r#"{"master": {"points": [{"x": "a"}]}}"#).is_err());
        assert!(to_json(&identity_set()).starts_with(r#"{"blue":"#), "keys sorted");
    }

    #[test]
    fn edits_keep_order_neighbours_and_two_points() {
        let mut c = identity();
        assert_eq!(c.insert(0.5, 0.8), Some(1));
        assert_eq!(c.insert(0.2, 1.5), Some(1), "y held to 0..1");
        assert_eq!(c.points, vec![(0.0, 0.0), (0.2, 1.0), (0.5, 0.8), (1.0, 1.0)]);
        assert!(c.move_point(1, 0.9, 0.3), "x kept below its right neighbour");
        assert_eq!(c.points[1], (0.5 - MIN_GAP, 0.3));
        assert!(!c.move_point(9, 0.1, 0.1));
        assert!(c.remove(1) && c.remove(1));
        assert!(!c.remove(0), "two points are kept");
        assert_eq!(c.hit(0.98, 0.99, 0.05), Some(1));
        assert_eq!(c.hit(0.5, 0.5, 0.05), None);
        let mut full = Curve { points: (0..EDIT_MAX_POINTS).map(|i| (i as f64 / 100.0, 0.0)).collect(), enabled: true };
        assert_eq!(full.insert(0.9, 0.9), None);
    }

    #[test]
    fn channels_take_the_master_then_their_own_and_luts_round_as_8_bit() {
        let mut set = identity_set();
        set[0].points = vec![(0.0, 1.0), (1.0, 0.0)];
        assert_eq!(apply(&set, 0, 0.25), 0.75);
        set[1].points = vec![(0.0, 0.0), (0.5, 0.0), (1.0, 1.0)];
        assert_eq!(apply(&set, 0, 1.0), 0.0, "red: master to 0, then its own");
        assert_eq!(lut8(&identity_set(), 2)[128], 128);
        set[0].enabled = false;
        assert_eq!(apply(&set, 1, 0.3), 0.3, "an off master changes nothing");
    }

    #[test]
    fn presets_and_equalize() {
        assert_eq!(preset("Medium S").unwrap()[1], (0.25, 0.15));
        assert!(preset("nope").is_none());
        let flat = vec![1.0; 256];
        let pts = equalize_points(&flat, EQUALIZE_POINTS).unwrap();
        assert_eq!(pts.first(), Some(&(0.0, 0.0)));
        assert_eq!(pts.last(), Some(&(1.0, 1.0)));
        assert!(pts.windows(2).all(|w| w[1].0 > w[0].0), "{pts:?}");
        assert!(equalize_points(&[0.0; 4], 9).is_none() && equalize_points(&[], 9).is_none());
        let one = equalize_points(&[0.0, 5.0, 0.0, 0.0], 9).unwrap();
        assert_eq!((one.first(), one.last()), (Some(&(0.25, 0.0)), Some(&(0.5, 1.0))), "values in one bin: spread across it {one:?}");
        assert_eq!(tab_histogram(&[vec![2, 4], vec![4, 0]], 0), vec![3.0, 2.0]);
        assert_eq!(tab_histogram(&[vec![2, 4], vec![4, 0]], 2), vec![4.0, 0.0]);
    }
}
