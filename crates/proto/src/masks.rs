//! A painted mask, for the `mask` value editor: the brush, the canvas it paints, and a stroke's
//! parameters as a command or a script gives them. Toolkit-free, so the core's commands and every
//! shell's drawing share one rasteriser; pinned by `crates/proto/fixtures/masks.json`.
//!
//! The brush maths is the research software's mask brush, unchanged (itself ported from a local-
//! adjustment brush), so a stroke paints the same everywhere:
//! - dabs a quarter of the radius apart, ending on each point;
//! - a hard core that falls off smoothly to the radius;
//! - flow that builds up to the density within one stroke;
//! - holding the brush still adds nothing.
//!
//! A [`MaskCanvas`] is an edit at the picture's own size: the painted picture (RGB, starting as the
//! picture exactly, so unpainted pixels keep their values) and the mask (0..1). As a file it is an
//! RGBA picture, alpha the mask rounded to 8 bits ([`MaskCanvas::rounded`], [`MaskCanvas::rgba`]).
//!
//! A stroke's points may carry a pen pressure (`[x, y, p]`, `p` in 0..1). It is checked and, for
//! now, not used: every dab paints at full pressure.

use serde_json::Value;

/// A point in canvas pixels.
pub type Point = [f64; 2];

/// The most points one stroke takes.
pub const MAX_STROKE_POINTS: usize = 100_000;
/// The most dabs one stroke lays (its length over the dab spacing).
pub const MAX_STROKE_DABS: f64 = 4_000_000.0;
/// The largest brush radius, in canvas pixels.
pub const MAX_RADIUS: f64 = 400.0;
/// The smallest brush radius.
pub const MIN_RADIUS: f64 = 0.5;
/// The largest canvas (pixels).
pub const MAX_PIXELS: usize = 256 << 20;
/// The longest side of a mask: a larger picture is painted at this size and its mask scaled up
/// (the maintainer, 2026-11-23, decision 3A).
pub const MAX_SIDE: usize = 2048;

/// The mask's size `(h, w)` for a picture of `h × w`: the picture's own up to [`MAX_SIDE`], else
/// scaled down to it, rounding each side (at least 1). The engine SDK's picture for an editor is
/// scaled by the same arithmetic, so the picture a shell paints on is the mask's size.
pub fn canvas_size(h: usize, w: usize) -> (usize, usize) {
    let scale = (h.max(w) as f64 / MAX_SIDE as f64).max(1.0);
    (((h as f64 / scale).round() as usize).max(1), ((w as f64 / scale).round() as usize).max(1))
}

fn dist(a: Point, b: Point) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn lerp(a: Point, b: Point, t: f64) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// Smoothstep from `e0` to `e1`.
fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Below this many pixels of movement the pointer is held still.
const STATIONARY_PX: f64 = 1e-6;

/// Distance between consecutive dabs of a brush of radius `r`: a quarter of the radius.
pub fn dab_spacing(r: f64) -> f64 {
    (r / 4.0).max(0.5)
}

/// Append the dabs from `prev` (already stamped) to `q`, at most [`dab_spacing`] apart and ending
/// on `q`. A sample that hasn't moved, or isn't finite, lays nothing. Returns whether `q` was
/// stamped (callers move `prev` on only then).
pub fn push_dabs(dabs: &mut Vec<Point>, prev: Option<Point>, q: Point, r: f64) -> bool {
    if !(q[0].is_finite() && q[1].is_finite()) {
        return false;
    }
    if let Some(prev) = prev {
        let d = dist(prev, q);
        if d.is_nan() || d <= STATIONARY_PX {
            return false;
        }
        let n = (d / dab_spacing(r)).ceil();
        let n = if n.is_finite() { n.clamp(0.0, 1_000_000.0) as usize } else { 0 };
        for k in 1..n {
            dabs.push(lerp(prev, q, k as f64 / n as f64));
        }
    }
    dabs.push(q);
    true
}

/// How much one dab paints at distance `dd` from its centre: 1 inside the hard core, falling
/// smoothly to 0 at the radius.
pub fn dab_alpha(dd: f64, r: f64, hard: f64) -> f32 {
    if dd > r {
        0.0
    } else if dd <= hard {
        1.0
    } else {
        1.0 - smooth(hard as f32, r as f32, dd as f32)
    }
}

/// A stroke's coverage `v` after another dab paints `a`: flow accumulates up to density.
pub fn accumulate(v: f32, a: f32, flow: f32, density: f32) -> f32 {
    (v + a * flow * (1.0 - v)).min(density)
}

/// A dab's radius at pen pressure `p`: a tenth of the brush at the lightest touch.
pub fn pressure_radius(r: f64, p: f32) -> f64 {
    let p = if p.is_nan() { 1.0 } else { f64::from(p.clamp(0.0, 1.0)) };
    (r * (0.1 + 0.9 * p)).max(0.5).min(r.max(0.5))
}

/// A dab's flow at pen pressure `p`.
pub fn pressure_flow(flow: f32, p: f32) -> f32 {
    let p = if p.is_nan() { 1.0 } else { p.clamp(0.0, 1.0) };
    flow * p
}

/// What a stroke does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Add to the mask.
    Mask,
    /// Take away from the mask.
    Erase,
    /// Paint a grey value into the picture (and the mask, so "painted where masked" sees it).
    Paint,
}

impl Tool {
    /// Its name in a command's params.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Mask => "mask",
            Tool::Erase => "erase",
            Tool::Paint => "paint",
        }
    }

    pub fn named(name: &str) -> Option<Tool> {
        [Tool::Mask, Tool::Erase, Tool::Paint].into_iter().find(|t| t.name() == name)
    }
}

/// A brush's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Brush {
    pub tool: Tool,
    /// Radius, in canvas pixels.
    pub radius: f64,
    /// 0..1: how much of the radius is the hard core.
    pub hardness: f64,
    /// 0..1: how much one dab adds.
    pub flow: f32,
    /// 0..1: the most one stroke reaches.
    pub density: f32,
    /// The grey [`Tool::Paint`] lays down, 0..255 (the picture's 8-bit units).
    pub value: u8,
}

impl Default for Brush {
    fn default() -> Self {
        Brush { tool: Tool::Mask, radius: 24.0, hardness: 0.5, flow: 0.6, density: 1.0, value: 0 }
    }
}

/// A mask value as an 8-bit alpha.
pub fn alpha(m: f32) -> u8 {
    (m.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// An edit: the painted picture and the mask, `w × h`.
#[derive(Debug, Clone, PartialEq)]
pub struct MaskCanvas {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<[u8; 3]>,
    pub mask: Vec<f32>,
}

/// A stroke in progress: its dabs so far, its coverage, and the canvas as it began.
#[derive(Debug, Clone)]
pub struct Stroke {
    brush: Brush,
    prev: Option<Point>,
    dabs: Vec<Point>,
    pressures: Vec<f32>,
    coverage: Vec<f32>,
    base: MaskCanvas,
}

impl Stroke {
    pub fn brush(&self) -> Brush {
        self.brush
    }
}

impl MaskCanvas {
    /// A clean canvas on `picture` (RGBA, row by row), nothing masked. A pixel the picture lacks
    /// is black. Larger than [`MAX_PIXELS`], it has no pixels, and nothing paints on it.
    pub fn new(w: usize, h: usize, picture: &[[u8; 4]]) -> MaskCanvas {
        let n = w.checked_mul(h).filter(|n| *n <= MAX_PIXELS).unwrap_or(0);
        let rgb = (0..n).map(|i| picture.get(i).map_or([0, 0, 0], |p| [p[0], p[1], p[2]])).collect();
        MaskCanvas { w, h, rgb, mask: vec![0.0; n] }
    }

    /// A clean canvas on `picture` that starts from `base`, a mask another node made (one value
    /// 0..1 a pixel, row by row; missing values are 0). Strokes add to it and take from it.
    pub fn on_base(w: usize, h: usize, picture: &[[u8; 4]], base: &[f32]) -> MaskCanvas {
        let mut c = MaskCanvas::new(w, h, picture);
        for (m, b) in c.mask.iter_mut().zip(base) {
            *m = if b.is_finite() { b.clamp(0.0, 1.0) } else { 0.0 };
        }
        c
    }

    /// Is this canvas, as its file would read back, the same as `start` would (nothing painted or
    /// masked beyond where it began)?
    pub fn is_unchanged(&self, start: &MaskCanvas) -> bool {
        self.rgb == start.rgb && self.mask.len() == start.mask.len() && self.mask.iter().zip(&start.mask).all(|(a, b)| alpha(*a) == alpha(*b))
    }

    /// An edit read back from RGBA pixels (alpha the mask), when there are `w × h` of them.
    pub fn from_rgba(w: usize, h: usize, rgba: &[[u8; 4]]) -> Option<MaskCanvas> {
        let n = w.checked_mul(h).filter(|n| *n <= MAX_PIXELS)?;
        let px = rgba.get(..n).filter(|p| p.len() == n)?;
        Some(MaskCanvas { w, h, rgb: px.iter().map(|p| [p[0], p[1], p[2]]).collect(), mask: px.iter().map(|p| f32::from(p[3]) / 255.0).collect() })
    }

    /// Is anything masked or painted away from `picture`?
    pub fn is_blank(&self, picture: &[[u8; 4]]) -> bool {
        self.mask.iter().all(|m| *m <= 0.0) && self.rgb.iter().zip(picture).all(|(a, p)| *a == [p[0], p[1], p[2]])
    }

    /// Begin a stroke with `brush`.
    pub fn begin(&self, brush: Brush) -> Stroke {
        Stroke { brush, prev: None, dabs: Vec::new(), pressures: Vec::new(), coverage: vec![0.0; self.mask.len()], base: self.clone() }
    }

    /// Stroke `s` to point `q` (canvas pixels) at pen pressure `p` (1 for a mouse or a finger);
    /// repaints the pixels its new dabs touch.
    pub fn stroke_to(&mut self, s: &mut Stroke, q: Point, p: f32) {
        let before = s.dabs.len();
        let r = s.brush.radius.max(0.5);
        if !push_dabs(&mut s.dabs, s.prev, q, r) {
            return;
        }
        let from = s.pressures.last().copied().unwrap_or(p);
        let added = s.dabs.len() - before;
        for k in 1..=added {
            s.pressures.push(from + (p - from) * k as f32 / added as f32);
        }
        s.prev = Some(q);
        for i in before..s.dabs.len() {
            let (Some(d), Some(pressure)) = (s.dabs.get(i).copied(), s.pressures.get(i).copied()) else { continue };
            let r = pressure_radius(r, pressure);
            let hard = r * s.brush.hardness.clamp(0.0, 1.0);
            let flow = pressure_flow(s.brush.flow.clamp(0.0, 1.0), pressure);
            self.dab(s, d, r, hard, flow);
        }
    }

    fn dab(&mut self, s: &mut Stroke, c: Point, r: f64, hard: f64, flow: f32) {
        let (w, h) = (self.w as f64, self.h as f64);
        let x0 = (c[0] - r).floor().max(0.0) as usize;
        let y0 = (c[1] - r).floor().max(0.0) as usize;
        let x1 = (c[0] + r).ceil().min(w - 1.0).max(0.0) as usize;
        let y1 = (c[1] + r).ceil().min(h - 1.0).max(0.0) as usize;
        if c[0] + r < 0.0 || c[1] + r < 0.0 || c[0] - r > w || c[1] - r > h {
            return;
        }
        let density = s.brush.density.clamp(0.0, 1.0);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let Some(i) = y.checked_mul(self.w).and_then(|r| r.checked_add(x)) else { continue };
                let dd = dist([x as f64 + 0.5, y as f64 + 0.5], c);
                let a = dab_alpha(dd, r, hard);
                if a <= 0.0 {
                    continue;
                }
                let (Some(v), Some(base_m), Some(base_rgb)) = (s.coverage.get_mut(i), s.base.mask.get(i), s.base.rgb.get(i)) else { continue };
                *v = accumulate(*v, a, flow, density);
                let v = *v;
                let (Some(m), Some(rgb)) = (self.mask.get_mut(i), self.rgb.get_mut(i)) else { continue };
                match s.brush.tool {
                    Tool::Mask => *m = base_m + v * (1.0 - base_m),
                    Tool::Erase => *m = base_m * (1.0 - v),
                    Tool::Paint => {
                        let g = f32::from(s.brush.value);
                        *rgb = base_rgb.map(|c| (f32::from(c) * (1.0 - v) + g * v).round().clamp(0.0, 255.0) as u8);
                        *m = base_m + v * (1.0 - base_m);
                    }
                }
            }
        }
    }

    /// Paint `stroke` (its points at full pressure: a pressure given is not used yet).
    pub fn paint(&mut self, stroke: &StrokeSpec) {
        let mut s = self.begin(stroke.brush);
        for q in &stroke.points {
            self.stroke_to(&mut s, *q, 1.0);
        }
    }

    /// The canvas as its file will read back: the mask rounded to 8 bits.
    pub fn rounded(mut self) -> MaskCanvas {
        for m in &mut self.mask {
            *m = f32::from(alpha(*m)) / 255.0;
        }
        self
    }

    /// The file's pixels: RGBA, row by row, alpha the mask rounded to 8 bits.
    pub fn rgba(&self) -> Vec<u8> {
        self.rgb.iter().zip(&self.mask).flat_map(|(c, m)| [c[0], c[1], c[2], alpha(*m)]).collect()
    }

    /// A hash of the file's pixels, in the time a pointer lift can spare: a word a pixel, each a
    /// 64-bit FNV-1a step, then a mix so the low bits depend on them all. An edit file's name
    /// carries it.
    pub fn pixel_hash(&self) -> u64 {
        let start = (0xcbf2_9ce4_8422_2325_u64 ^ self.w as u64).wrapping_mul(0x0100_0000_01b3) ^ self.h as u64;
        let h = self
            .rgb
            .iter()
            .zip(&self.mask)
            .fold(start, |h, (c, m)| (h ^ u64::from(u32::from_le_bytes([c[0], c[1], c[2], alpha(*m)]))).wrapping_mul(0x0100_0000_01b3));
        h ^ (h >> 32)
    }
}

/// A stroke as a command or a script gives it: the brush and its points (canvas pixels).
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeSpec {
    pub brush: Brush,
    pub points: Vec<Point>,
}

fn number(p: &Value, key: &str, default: f64, lo: f64, hi: f64) -> Result<f64, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_f64().filter(|f| f.is_finite()).map(|f| f.clamp(lo, hi)).ok_or_else(|| format!("{key:?} must be a number")),
    }
}

/// A stroke's params: `{points: [[x, y] | [x, y, p]], tool?: mask | erase | paint, radius?,
/// hardness?, flow?, density?, value? (paint's grey, 0..255)}`. Numbers are held to their ranges;
/// points must be finite, at most [`MAX_STROKE_POINTS`], and no longer than [`MAX_STROKE_DABS`]
/// dabs; a pressure `p`, when given, must be in 0..1 (it is not used yet).
pub fn parse_stroke(p: &Value) -> Result<StrokeSpec, String> {
    let d = Brush::default();
    let tool = match p.get("tool") {
        None | Some(Value::Null) => d.tool,
        Some(t) => t.as_str().and_then(Tool::named).ok_or("\"tool\" must be mask, erase or paint")?,
    };
    let brush = Brush {
        tool,
        radius: number(p, "radius", d.radius, MIN_RADIUS, MAX_RADIUS)?,
        hardness: number(p, "hardness", d.hardness, 0.0, 1.0)?,
        flow: number(p, "flow", f64::from(d.flow), 0.0, 1.0)? as f32,
        density: number(p, "density", f64::from(d.density), 0.0, 1.0)? as f32,
        value: number(p, "value", f64::from(d.value), 0.0, 255.0)?.round() as u8,
    };
    let bad = "\"points\" must be a non-empty list of [x, y] or [x, y, pressure 0..1], in the picture's pixels";
    let list = p.get("points").and_then(Value::as_array).filter(|l| !l.is_empty() && l.len() <= MAX_STROKE_POINTS).ok_or(bad)?;
    let mut points: Vec<Point> = Vec::with_capacity(list.len());
    for q in list {
        let coords = q.as_array().ok_or(bad)?;
        let num = |k: usize| coords.get(k).and_then(Value::as_f64).filter(|f| f.is_finite());
        let (Some(x), Some(y)) = (num(0), num(1)) else { return Err(bad.into()) };
        match coords.len() {
            2 => {}
            3 if num(2).is_some_and(|p| (0.0..=1.0).contains(&p)) => {}
            _ => return Err(bad.into()),
        }
        points.push([x, y]);
    }
    let length: f64 = points.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
    if !length.is_finite() || length / dab_spacing(brush.radius) > MAX_STROKE_DABS {
        return Err("the stroke is too long".into());
    }
    Ok(StrokeSpec { brush, points })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The vectors' picture: `w × h`, every pixel different enough to see a paint.
    pub fn picture(w: usize, h: usize) -> Vec<[u8; 4]> {
        (0..w * h).map(|i| [((i % w) * 5 % 256) as u8, ((i / w) * 7 % 256) as u8, ((i * 3) % 256) as u8, 255]).collect()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The f32 mask's bit patterns, hashed: what an exact preview must reproduce before rounding.
    fn mask_bits(c: &MaskCanvas) -> String {
        let h = c.mask.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, m| (h ^ u64::from(m.to_bits())).wrapping_mul(0x0100_0000_01b3));
        format!("{h:016x}")
    }

    /// The cases: each a list of strokes painted in turn on a clean canvas.
    fn cases() -> Vec<(&'static str, Vec<Value>)> {
        vec![
            ("one dab, the default brush", vec![json!({"points": [[24, 16]]})]),
            ("a hard line", vec![json!({"points": [[4, 8], [44, 8]], "radius": 5, "hardness": 1})]),
            ("a soft line", vec![json!({"points": [[4, 24], [44, 20]], "radius": 9, "hardness": 0})]),
            ("flow below density", vec![json!({"points": [[6, 6], [40, 26], [8, 28]], "radius": 6, "flow": 0.2, "density": 0.7})]),
            ("held still adds nothing", vec![json!({"points": [[20, 16], [20, 16], [20, 16]], "radius": 7})]),
            ("off the edges", vec![json!({"points": [[-10, -10], [60, 40]], "radius": 8})]),
            ("a tiny brush", vec![json!({"points": [[3.3, 4.7], [30.1, 9.9]], "radius": 0.5, "hardness": 0.3})]),
            (
                "mask then erase",
                vec![json!({"points": [[4, 16], [44, 16]], "radius": 10}), json!({"tool": "erase", "points": [[24, 2], [24, 30]], "radius": 5, "flow": 0.8})],
            ),
            ("paint a grey", vec![json!({"tool": "paint", "value": 200, "points": [[10, 10], [38, 22]], "radius": 6, "hardness": 0.6})]),
            (
                "two strokes overlap",
                vec![
                    json!({"points": [[4, 4], [44, 28]], "radius": 6, "flow": 0.5}),
                    json!({"points": [[44, 4], [4, 28]], "radius": 6, "flow": 0.5, "density": 0.6}),
                ],
            ),
            ("pressures given, not used", vec![json!({"points": [[6, 20, 0.2], [42, 12, 0.9]], "radius": 5})]),
        ]
    }

    const W: usize = 48;
    const H: usize = 32;

    fn painted(strokes: &[Value]) -> MaskCanvas {
        let mut c = MaskCanvas::new(W, H, &picture(W, H));
        for s in strokes {
            c.paint(&parse_stroke(s).unwrap());
        }
        c
    }

    fn fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/masks.json")
    }

    #[test]
    fn write_the_shared_vectors() {
        if std::env::var("PLUMCRAFT_WRITE_FIXTURES").is_err() {
            return;
        }
        let out: Vec<Value> = cases()
            .into_iter()
            .map(|(name, strokes)| {
                let c = painted(&strokes);
                json!({"name": name, "strokes": strokes, "mask_bits": mask_bits(&c), "pixel_hash": format!("{:016x}", c.pixel_hash()), "rgba": hex(&c.rgba())})
            })
            .collect();
        let doc = json!({
            "about": "Shared test vectors for the mask brush (crates/proto/src/masks.rs): each case's strokes, painted in turn on a clean 48 × 32 canvas over the picture whose pixel i (row by row) is [(i % 48) * 5 % 256, (i / 48) * 7 % 256, i * 3 % 256, 255]. `rgba` is the edit file's pixels (hex, row by row, alpha the mask rounded to 8 bits), `pixel_hash` the hash an edit file's name carries, and `mask_bits` an FNV-1a hash of the unrounded mask's f32 bit patterns (start 0xcbf29ce484222325, each step xor the bits then multiply by 0x100000001b3): a drawing port that previews a stroke exactly must match it. Written by `PLUMCRAFT_WRITE_FIXTURES=1 cargo test -p plumcraft-proto masks`.",
            "w": W, "h": H, "cases": out,
        });
        std::fs::write(fixture(), serde_json::to_string_pretty(&doc).unwrap() + "\n").unwrap();
    }

    #[test]
    fn the_shared_vectors_hold() {
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(fixture()).unwrap()).unwrap();
        let cases = doc["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 11);
        for c in cases {
            let strokes = c["strokes"].as_array().unwrap();
            let got = painted(strokes);
            let name = c["name"].as_str().unwrap();
            assert_eq!(hex(&got.rgba()), c["rgba"].as_str().unwrap(), "{name}: rgba");
            assert_eq!(format!("{:016x}", got.pixel_hash()), c["pixel_hash"].as_str().unwrap(), "{name}: pixel hash");
            assert_eq!(mask_bits(&got), c["mask_bits"].as_str().unwrap(), "{name}: mask bits");
        }
    }

    #[test]
    fn a_stroke_paints_as_the_brush_says() {
        let pic = picture(W, H);
        let c = painted(&[json!({"points": [[24, 16]], "radius": 4, "hardness": 1, "flow": 1})]);
        // a hard dab at full flow: the core is all masked, beyond the radius nothing
        assert_eq!(c.mask[16 * W + 24], 1.0);
        assert_eq!(c.mask[16 * W + 30], 0.0);
        assert!(!c.is_blank(&pic));
        // held still adds nothing more than the one dab
        let once = painted(&[json!({"points": [[20, 16]]})]);
        assert_eq!(painted(&[json!({"points": [[20, 16], [20, 16]]})]), once);
        // density caps a stroke; erase takes away; paint changes the picture where it masks
        let capped = painted(&[json!({"points": [[4, 16], [44, 16]], "density": 0.5, "flow": 1})]);
        assert!(capped.mask.iter().all(|m| *m <= 0.5) && capped.mask.contains(&0.5));
        let line = json!({"points": [[4, 16], [44, 16]], "radius": 8, "flow": 1});
        let masked = painted(std::slice::from_ref(&line));
        let erased = painted(&[line, json!({"tool": "erase", "points": [[4, 16], [44, 16]], "radius": 8, "hardness": 1, "flow": 1})]);
        // an erase only takes away, and its hard core takes everything
        assert!(erased.mask.iter().zip(&masked.mask).all(|(e, m)| e <= m));
        assert_eq!(erased.mask[16 * W + 24], 0.0);
        let grey = painted(&[json!({"tool": "paint", "value": 200, "points": [[24, 16]], "radius": 3, "hardness": 1, "flow": 1})]);
        assert_eq!(grey.rgb[16 * W + 24], [200, 200, 200]);
        assert_eq!(grey.rgb[0], [pic[0][0], pic[0][1], pic[0][2]]);
        // rounded is what the file reads back
        let r = capped.clone().rounded();
        assert_eq!(MaskCanvas::from_rgba(W, H, r.rgba().as_chunks::<4>().0), Some(r.clone()));
        assert_eq!(r.pixel_hash(), capped.pixel_hash());
        // a clean canvas is blank; an erase on a clean canvas leaves it blank
        assert!(MaskCanvas::new(W, H, &pic).is_blank(&pic));
        assert!(painted(&[json!({"tool": "erase", "points": [[24, 16]]})]).is_blank(&pic));
    }

    #[test]
    fn a_mask_from_elsewhere_is_the_start() {
        let pic = picture(W, H);
        let base: Vec<f32> = (0..W * H).map(|i| if i % W < W / 2 { 0.8 } else { 0.0 }).collect();
        let start = MaskCanvas::on_base(W, H, &pic, &base);
        let mut c = start.clone();
        assert!(c.is_unchanged(&start));
        c.paint(&parse_stroke(&json!({"tool": "erase", "points": [[4, 16]], "radius": 3, "hardness": 1, "flow": 1})).unwrap());
        assert_eq!(c.mask[16 * W + 4], 0.0);
        assert_eq!(c.mask[16 * W + 20], 0.8);
        assert!(!c.is_unchanged(&start));
        // the size rule: the picture's own up to 2048, else scaled down, rounding
        assert_eq!(canvas_size(1280, 960), (1280, 960));
        assert_eq!(canvas_size(4000, 6000), (1365, 2048));
        assert_eq!(canvas_size(1, 100_000), (1, 2048));
    }

    #[test]
    fn bad_strokes_are_refused() {
        for bad in [
            json!({}),
            json!({"points": []}),
            json!({"points": [[1]]}),
            json!({"points": [[1, "x"]]}),
            json!({"points": [[1, 2, 1.5]]}),
            json!({"points": [[1, 2, -0.1]]}),
            json!({"points": [[1, 2, 0.5, 9]]}),
            json!({"points": [[1, 2]], "tool": "smudge"}),
            json!({"points": [[1, 2]], "tool": 3}),
            json!({"points": [[1, 2]], "radius": "big"}),
            json!({"points": [[0, 0], [1e300, 1e300]]}),
            json!({"points": [[0, 0], [1e9, 0]], "radius": 0.5}),
        ] {
            assert!(parse_stroke(&bad).is_err(), "{bad}");
        }
        // numbers are held to their ranges
        let s = parse_stroke(&json!({"points": [[1, 2, 0.5]], "radius": 1e6, "hardness": -3, "flow": 9, "value": 999})).unwrap();
        assert_eq!((s.brush.radius, s.brush.hardness, s.brush.flow, s.brush.value), (MAX_RADIUS, 0.0, 1.0, 255));
        assert_eq!(s.points, vec![[1.0, 2.0]]);
    }

    #[test]
    fn odd_canvases_never_panic() {
        for (w, h) in [(0, 0), (1, 0), (0, 5), (1, 1), (3, 2)] {
            let mut c = MaskCanvas::new(w, h, &[]);
            c.paint(&parse_stroke(&json!({"points": [[0, 0], [5, 5], [-3, 9]], "radius": 400})).unwrap());
            let _ = (c.rgba(), c.pixel_hash(), c.clone().rounded());
        }
        assert!(MaskCanvas::from_rgba(2, 2, &[[0; 4]; 3]).is_none());
        assert!(MaskCanvas::from_rgba(usize::MAX, 2, &[]).is_none());
        let mut wide = MaskCanvas::new(usize::MAX, 3, &[]);
        wide.paint(&parse_stroke(&json!({"points": [[1, 1], [9, 2]], "radius": 4})).unwrap());
    }
}
