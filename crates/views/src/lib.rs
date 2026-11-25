//! Derived views of a value (FR6, research note 30, decided "All A"): what a person looks at
//! without adding a node. A view is a small typed value made from an array's bytes:
//!
//! - **`histogram`**: the finite values counted into `bins` bins over `lo..hi` (by default their
//!   own finite min..max), over all values or per plane along an axis: a TABLE (`centre`, `count`,
//!   and `plane` per plane), its NaN, ±inf and out-of-range counts in `meta`;
//! - **`stats`**: count, NaN, ±inf, min, max, mean, standard deviation and percentiles of the
//!   finite values, over all or per plane: a TABLE, one row a plane;
//! - **`slice`**: the 2-D plane of an N-d array along two axes at given indices of the others: an
//!   array of the same dtype.
//!
//! **Implemented twice:** this crate (the Rust engine SDK, and the editor's built-in views engine
//! for an engine that has none), and the Python SDK's `plumcraft_engine.views`, held to it by the
//! shared vectors in `crates/proto/fixtures/views.json`. The rules both follow, written down so
//! they can:
//!
//! - **Bins:** a finite `x` in `lo..=hi` goes to bin `floor(((x - lo) / (hi - lo)) * bins)` in
//!   f64, in that order of operations, the last bin taking `hi`; below `lo` or above `hi` it is
//!   counted in `below` or `above`. With no range given, `lo..hi` is the finite min..max, and
//!   `hi = lo + 1` when they are equal (the SDKs' per-channel rule). `centre` of bin `k` is
//!   `lo + (k + 0.5) * (hi - lo) / bins`.
//! - **Statistics** are of the finite values, in f64: the mean by a compensated (Neumaier) sum,
//!   the standard deviation the population's (two passes), percentiles by linear interpolation
//!   between the sorted values (numpy's default). Counts are exact; floats agree to 1e-9 relative.
//! - **Planes:** an array is in C order; plane `p` along axis `k` is every element whose index on
//!   that axis is `p`.
//!
//! Every input is from the other side (golden rule 1): sizes are checked before a byte is read,
//! every parameter is bounded, and nothing here panics.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use serde_json::{Map, Value, json};

/// The most bins a histogram has.
pub const MAX_BINS: u64 = 65_536;
/// The most planes a per-plane view has.
pub const MAX_PLANES: u64 = 65_536;
/// The most rows a derived table has (planes × bins).
pub const MAX_ROWS: u64 = 1 << 22;
/// The most percentiles asked at once.
pub const MAX_PERCENTILES: usize = 32;
/// The most axes an array has.
pub const MAX_RANK: usize = 32;
/// The most finite values sorted for exact percentiles (beyond, asking for them is refused).
pub const MAX_SORTED: u64 = 1 << 28;
/// The most elements a slice has.
pub const MAX_SLICE: u64 = 1 << 26;
/// The view kinds there are.
pub const KINDS: &[&str] = &["histogram", "stats", "slice"];

/// An array as its bytes: little-endian (or byte) numbers in C order.
#[derive(Debug, Clone, Copy)]
pub struct Array<'a> {
    pub dtype: &'a str,
    pub shape: &'a [u64],
    pub bytes: &'a [u8],
}

/// What a view made.
#[derive(Debug, Clone, PartialEq)]
pub enum Derived {
    /// A TABLE (`{"columns": [{name, type, values}]}`) and what else it says (`meta`).
    Table { table: Value, meta: Value },
    /// An array: its dtype, shape and bytes.
    Array { dtype: String, shape: Vec<u64>, bytes: Vec<u8> },
}

/// Over what a view is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Per {
    All,
    /// Each plane along this axis.
    Axis(usize),
}

/// A view, checked.
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Histogram { bins: u64, lo: Option<f64>, hi: Option<f64>, per: Per },
    Stats { per: Per, percentiles: Vec<f64> },
    Slice { axes: [usize; 2], at: Vec<u64> },
}

fn err<T>(m: impl Into<String>) -> Result<T, String> {
    Err(m.into())
}

/// One element's width and how it reads.
#[derive(Clone, Copy)]
enum Ty {
    U(usize),
    I(usize),
    F2,
    F4,
    F8,
    Bool,
}

fn ty(dtype: &str) -> Result<Ty, String> {
    Ok(match dtype {
        "|u1" => Ty::U(1),
        "|i1" => Ty::I(1),
        "|b1" => Ty::Bool,
        "<u2" => Ty::U(2),
        "<i2" => Ty::I(2),
        "<u4" => Ty::U(4),
        "<i4" => Ty::I(4),
        "<u8" => Ty::U(8),
        "<i8" => Ty::I(8),
        "<f2" => Ty::F2,
        "<f4" => Ty::F4,
        "<f8" => Ty::F8,
        other => return err(format!("an array of {:?} isn't numbers", other.chars().take(16).collect::<String>())),
    })
}

fn width(t: Ty) -> usize {
    match t {
        Ty::U(n) | Ty::I(n) => n,
        Ty::F2 => 2,
        Ty::F4 => 4,
        Ty::F8 => 8,
        Ty::Bool => 1,
    }
}

/// An IEEE half as f64.
fn half(bits: u16) -> f64 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((bits >> 10) & 0x1f);
    let frac = f64::from(bits & 0x3ff);
    match exp {
        0 => sign * frac * 2f64.powi(-24),
        31 if frac == 0.0 => sign * f64::INFINITY,
        31 => f64::NAN,
        e => sign * (1.0 + frac / 1024.0) * 2f64.powi(e - 15),
    }
}

fn read(t: Ty, b: &[u8]) -> f64 {
    let mut a = [0u8; 8];
    if let Some(d) = a.get_mut(..b.len()) {
        d.copy_from_slice(b);
    }
    match t {
        Ty::Bool => f64::from(u8::from(a[0] != 0)),
        Ty::U(1) => f64::from(a[0]),
        Ty::I(1) => f64::from(a[0] as i8),
        Ty::U(2) => f64::from(u16::from_le_bytes([a[0], a[1]])),
        Ty::I(2) => f64::from(i16::from_le_bytes([a[0], a[1]])),
        Ty::U(4) => f64::from(u32::from_le_bytes([a[0], a[1], a[2], a[3]])),
        Ty::I(4) => f64::from(i32::from_le_bytes([a[0], a[1], a[2], a[3]])),
        Ty::U(_) => u64::from_le_bytes(a) as f64,
        Ty::I(_) => i64::from_le_bytes(a) as f64,
        Ty::F2 => half(u16::from_le_bytes([a[0], a[1]])),
        Ty::F4 => f64::from(f32::from_le_bytes([a[0], a[1], a[2], a[3]])),
        Ty::F8 => f64::from_le_bytes(a),
    }
}

impl Array<'_> {
    /// The element count, its reading, and a check that the bytes hold the shape.
    fn checked(&self) -> Result<(usize, Ty), String> {
        if self.shape.is_empty() || self.shape.len() > MAX_RANK {
            return err(format!("an array of {} axes (1 to {MAX_RANK})", self.shape.len()));
        }
        let t = ty(self.dtype)?;
        let n = self.shape.iter().try_fold(1u64, |a, d| a.checked_mul(*d)).ok_or("its shape overflows")?;
        let n = usize::try_from(n).map_err(|_| "its shape overflows")?;
        let need = n.checked_mul(width(t)).ok_or("its shape overflows")?;
        if self.bytes.len() != need {
            return err(format!("{} bytes, where {:?} of {} is {need}", self.bytes.len(), self.shape, self.dtype));
        }
        Ok((n, t))
    }

    /// Each element, in C order, as f64.
    fn each(&self, t: Ty, mut f: impl FnMut(usize, f64)) {
        for (i, c) in self.bytes.chunks_exact(width(t)).enumerate() {
            f(i, read(t, c));
        }
    }

    /// The plane along `axis` each flat index is in: `(stride, count)`.
    fn plane_of(&self, axis: usize) -> Result<(usize, usize), String> {
        let count = *self.shape.get(axis).ok_or_else(|| format!("no axis {axis}: the array has {}", self.shape.len()))?;
        if count > MAX_PLANES {
            return err(format!("{count} planes along axis {axis}, more than {MAX_PLANES}"));
        }
        let stride = self.shape.iter().skip(axis + 1).try_fold(1u64, |a, d| a.checked_mul(*d)).ok_or("its shape overflows")?;
        Ok((usize::try_from(stride).map_err(|_| "its shape overflows")?, usize::try_from(count).map_err(|_| "its shape overflows")?))
    }
}

fn uint(p: &Map<String, Value>, k: &str) -> Result<Option<u64>, String> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("{k} must be a whole number")),
    }
}

fn float(p: &Map<String, Value>, k: &str) -> Result<Option<f64>, String> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|f| f.is_finite()).map(Some).ok_or_else(|| format!("{k} must be a finite number")),
    }
}

fn per(p: &Map<String, Value>) -> Result<Per, String> {
    match p.get("per") {
        None | Some(Value::Null) => Ok(Per::All),
        Some(Value::String(s)) if s == "all" => Ok(Per::All),
        Some(v) => match v.get("axis").and_then(Value::as_u64) {
            Some(a) if (a as usize) < MAX_RANK => Ok(Per::Axis(a as usize)),
            _ => err("per is \"all\" or {axis: k}"),
        },
    }
}

/// A view from its JSON (`{kind, params}`), checked and bounded. Unknown kinds and fields are
/// refused with the reason.
pub fn parse(v: &Value) -> Result<View, String> {
    let kind = v.get("kind").and_then(Value::as_str).ok_or_else(|| format!("a view has a kind: {}", KINDS.join(", ")))?;
    let empty = Map::new();
    let p = match v.get("params") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(o)) => o,
        Some(_) => return err("params is an object"),
    };
    let allowed: &[&str] = match kind {
        "histogram" => &["bins", "lo", "hi", "per"],
        "stats" => &["per", "percentiles"],
        "slice" => &["axes", "at"],
        _ => return err(format!("no view {:?}: the views are {}", kind.chars().take(32).collect::<String>(), KINDS.join(", "))),
    };
    if let Some(k) = p.keys().find(|k| !allowed.contains(&k.as_str())) {
        return err(format!("{kind} takes {}, not {:?}", allowed.join(", "), k.chars().take(32).collect::<String>()));
    }
    match kind {
        "histogram" => {
            let bins = uint(p, "bins")?.unwrap_or(256);
            if bins == 0 || bins > MAX_BINS {
                return err(format!("bins is 1 to {MAX_BINS}"));
            }
            let (lo, hi) = (float(p, "lo")?, float(p, "hi")?);
            if let (Some(l), Some(h)) = (lo, hi)
                && h <= l
            {
                return err("hi must be above lo");
            }
            Ok(View::Histogram { bins, lo, hi, per: per(p)? })
        }
        "stats" => {
            let percentiles = match p.get("percentiles") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(a)) if a.len() <= MAX_PERCENTILES => {
                    a.iter().map(|q| q.as_f64().filter(|q| (0.0..=100.0).contains(q)).ok_or("a percentile is 0 to 100")).collect::<Result<Vec<_>, _>>()?
                }
                Some(_) => return err(format!("percentiles is a list of at most {MAX_PERCENTILES} numbers")),
            };
            Ok(View::Stats { per: per(p)?, percentiles })
        }
        _ => {
            let axes = p.get("axes").and_then(Value::as_array).filter(|a| a.len() == 2).ok_or("axes is two axes")?;
            let ax = |i: usize| axes.get(i).and_then(Value::as_u64).filter(|a| (*a as usize) < MAX_RANK).map(|a| a as usize).ok_or("an axis is a whole number");
            let axes = [ax(0)?, ax(1)?];
            if axes[0] == axes[1] {
                return err("the two axes differ");
            }
            let at = match p.get("at") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(a)) if a.len() <= MAX_RANK => a.iter().map(|x| x.as_u64().unwrap_or(0)).collect(),
                Some(_) => return err("at is a list of indices"),
            };
            Ok(View::Slice { axes, at })
        }
    }
}

/// The view made from `a`.
pub fn derive(view: &View, a: &Array<'_>) -> Result<Derived, String> {
    match view {
        View::Histogram { bins, lo, hi, per } => histogram(a, *bins, *lo, *hi, *per),
        View::Stats { per, percentiles } => stats(a, *per, percentiles),
        View::Slice { axes, at } => slice(a, *axes, at),
    }
}

fn column(name: &str, ty: &str, values: Vec<Value>) -> Value {
    json!({"name": name, "type": ty, "values": values})
}

/// The finite min and max, or none.
fn range(a: &Array<'_>, t: Ty) -> Option<(f64, f64)> {
    let mut r: Option<(f64, f64)> = None;
    a.each(t, |_, x| {
        if x.is_finite() {
            r = Some(r.map_or((x, x), |(l, h)| (l.min(x), h.max(x))));
        }
    });
    r
}

fn histogram(a: &Array<'_>, bins: u64, lo: Option<f64>, hi: Option<f64>, per: Per) -> Result<Derived, String> {
    let (_, t) = a.checked()?;
    let planes = match per {
        Per::All => None,
        Per::Axis(k) => Some(a.plane_of(k)?),
    };
    let np = planes.map_or(1, |(_, c)| c);
    let rows = (np as u64).checked_mul(bins).ok_or("too many rows")?;
    if rows > MAX_ROWS {
        return err(format!("{np} planes of {bins} bins is more than {MAX_ROWS} rows"));
    }
    let (lo, hi) = match (lo, hi) {
        (Some(l), Some(h)) => (l, h),
        (l, h) => {
            let (mn, mx) = range(a, t).unwrap_or((0.0, 1.0));
            let l = l.unwrap_or(mn);
            let h = h.unwrap_or(mx);
            (l, if h > l { h } else { l + 1.0 })
        }
    };
    let nb = bins as usize;
    let mut counts = vec![0u64; np * nb];
    let (mut nan, mut posinf, mut neginf, mut below, mut above) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let span = hi - lo;
    a.each(t, |i, x| {
        if x.is_nan() {
            nan += 1;
        } else if x == f64::INFINITY {
            posinf += 1;
        } else if x == f64::NEG_INFINITY {
            neginf += 1;
        } else if x < lo {
            below += 1;
        } else if x > hi {
            above += 1;
        } else {
            let k = (((x - lo) / span) * bins as f64).floor();
            let k = if k >= bins as f64 { nb - 1 } else { k.max(0.0) as usize };
            let p = planes.map_or(0, |(stride, count)| (i / stride.max(1)) % count.max(1));
            if let Some(c) = counts.get_mut(p * nb + k) {
                *c += 1;
            }
        }
    });
    let width = span / bins as f64;
    let mut cols = Vec::new();
    if planes.is_some() {
        cols.push(column("plane", "INT", (0..np).flat_map(|p| std::iter::repeat_n(json!(p), nb)).collect()));
    }
    cols.push(column("centre", "FLOAT", (0..np).flat_map(|_| (0..nb).map(|k| json!(lo + (k as f64 + 0.5) * width))).collect()));
    cols.push(column("count", "INT", counts.iter().map(|c| json!(c)).collect()));
    let mut meta = json!({"bins": bins, "lo": lo, "hi": hi, "nan": nan, "posinf": posinf, "neginf": neginf, "below": below, "above": above});
    if let Per::Axis(k) = per {
        meta["axis"] = json!(k);
    }
    Ok(Derived::Table { table: json!({"columns": cols}), meta })
}

/// A compensated (Neumaier) sum.
#[derive(Default, Clone, Copy)]
struct Sum {
    s: f64,
    c: f64,
}

impl Sum {
    fn add(&mut self, x: f64) {
        let t = self.s + x;
        if self.s.abs() >= x.abs() {
            self.c += (self.s - t) + x;
        } else {
            self.c += (x - t) + self.s;
        }
        self.s = t;
    }
    fn total(&self) -> f64 {
        self.s + self.c
    }
}

/// A percentile of sorted values, numpy's `linear` method.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return f64::NAN;
    }
    let pos = q / 100.0 * (n - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    let t = pos - lo as f64;
    let (a, b) = (sorted.get(lo).copied().unwrap_or(f64::NAN), sorted.get(hi).copied().unwrap_or(f64::NAN));
    let d = b - a;
    if t >= 0.5 { b - d * (1.0 - t) } else { a + d * t }
}

/// A percentile's column name: `p` and the shortest number (`p50`, `p99.5`).
fn pname(q: f64) -> String {
    let s = format!("{q}");
    format!("p{}", s.strip_suffix(".0").unwrap_or(&s))
}

fn stats(a: &Array<'_>, per: Per, percentiles: &[f64]) -> Result<Derived, String> {
    let (n, t) = a.checked()?;
    let planes = match per {
        Per::All => None,
        Per::Axis(k) => Some(a.plane_of(k)?),
    };
    let np = planes.map_or(1, |(_, c)| c);
    if !percentiles.is_empty() && n as u64 > MAX_SORTED {
        return err(format!("{n} values is more than exact percentiles sort ({MAX_SORTED})"));
    }
    let plane = |i: usize| planes.map_or(0, |(stride, count)| (i / stride.max(1)) % count.max(1));
    #[derive(Clone, Copy, Default)]
    struct Acc {
        count: u64,
        nan: u64,
        posinf: u64,
        neginf: u64,
        min: f64,
        max: f64,
        sum: Sum,
    }
    let mut acc = vec![Acc { min: f64::INFINITY, max: f64::NEG_INFINITY, ..Acc::default() }; np];
    let mut kept: Vec<Vec<f64>> = if percentiles.is_empty() { Vec::new() } else { vec![Vec::new(); np] };
    a.each(t, |i, x| {
        let Some(s) = acc.get_mut(plane(i)) else { return };
        if x.is_nan() {
            s.nan += 1;
        } else if x == f64::INFINITY {
            s.posinf += 1;
        } else if x == f64::NEG_INFINITY {
            s.neginf += 1;
        } else {
            s.count += 1;
            s.min = s.min.min(x);
            s.max = s.max.max(x);
            s.sum.add(x);
            if let Some(v) = kept.get_mut(plane(i)) {
                v.push(x);
            }
        }
    });
    let means: Vec<f64> = acc.iter().map(|s| if s.count > 0 { s.sum.total() / s.count as f64 } else { f64::NAN }).collect();
    let mut sq = vec![Sum::default(); np];
    a.each(t, |i, x| {
        let p = plane(i);
        if x.is_finite()
            && let (Some(s), Some(m)) = (sq.get_mut(p), means.get(p))
        {
            let d = x - m;
            s.add(d * d);
        }
    });
    let num = |x: f64| if x.is_finite() { json!(x) } else { Value::Null };
    let mut cols = Vec::new();
    if planes.is_some() {
        cols.push(column("plane", "INT", (0..np).map(|p| json!(p)).collect()));
    }
    cols.push(column("count", "INT", acc.iter().map(|s| json!(s.count)).collect()));
    cols.push(column("nan", "INT", acc.iter().map(|s| json!(s.nan)).collect()));
    cols.push(column("posinf", "INT", acc.iter().map(|s| json!(s.posinf)).collect()));
    cols.push(column("neginf", "INT", acc.iter().map(|s| json!(s.neginf)).collect()));
    cols.push(column("min", "FLOAT", acc.iter().map(|s| num(s.min)).collect()));
    cols.push(column("max", "FLOAT", acc.iter().map(|s| num(s.max)).collect()));
    cols.push(column("mean", "FLOAT", means.iter().map(|m| num(*m)).collect()));
    cols.push(column(
        "sd",
        "FLOAT",
        acc.iter().zip(&sq).map(|(s, q)| if s.count > 0 { num((q.total() / s.count as f64).sqrt()) } else { Value::Null }).collect(),
    ));
    for v in &mut kept {
        v.sort_by(f64::total_cmp);
    }
    for q in percentiles {
        cols.push(column(&pname(*q), "FLOAT", kept.iter().map(|v| num(percentile(v, *q))).collect()));
    }
    let mut meta = json!({"values": n});
    if let Per::Axis(k) = per {
        meta["axis"] = json!(k);
    }
    Ok(Derived::Table { table: json!({"columns": cols}), meta })
}

fn slice(a: &Array<'_>, axes: [usize; 2], at: &[u64]) -> Result<Derived, String> {
    let (_, t) = a.checked()?;
    let rank = a.shape.len();
    if axes.iter().any(|x| *x >= rank) {
        return err(format!("the array has {rank} axes"));
    }
    let (ni, nj) = (a.shape.get(axes[0]).copied().unwrap_or(0), a.shape.get(axes[1]).copied().unwrap_or(0));
    let out = ni.checked_mul(nj).ok_or("too large")?;
    if out > MAX_SLICE {
        return err(format!("a slice of {ni}×{nj} is more than {MAX_SLICE} elements"));
    }
    // each axis's index: the two chosen run, the rest fixed (0 when not given, clamped)
    let fixed: Vec<u64> = (0..rank).map(|k| at.get(k).copied().unwrap_or(0).min(a.shape.get(k).copied().unwrap_or(1).saturating_sub(1))).collect();
    let mut strides = vec![1u64; rank];
    for k in (0..rank.saturating_sub(1)).rev() {
        strides[k] = strides[k + 1].saturating_mul(a.shape.get(k + 1).copied().unwrap_or(1));
    }
    let base: u64 = (0..rank).filter(|k| !axes.contains(k)).map(|k| fixed[k].saturating_mul(strides[k])).sum();
    let w = width(t) as u64;
    let mut bytes = Vec::with_capacity(usize::try_from(out.saturating_mul(w)).unwrap_or(0));
    for i in 0..ni {
        for j in 0..nj {
            let e = base + i * strides[axes[0]] + j * strides[axes[1]];
            let s = usize::try_from(e * w).map_err(|_| "too large")?;
            let b = a.bytes.get(s..s + w as usize).ok_or("the bytes end early")?;
            bytes.extend_from_slice(b);
        }
    }
    Ok(Derived::Array { dtype: a.dtype.to_owned(), shape: vec![ni, nj], bytes })
}

/// A derived view as JSON for a reply or a vector: a table and its meta, or an array's dtype,
/// shape and bytes (base64 is the caller's).
pub fn to_json(d: &Derived) -> Value {
    match d {
        Derived::Table { table, meta } => json!({"table": table, "meta": meta}),
        Derived::Array { dtype, shape, bytes } => json!({"array": {"dtype": dtype, "shape": shape, "len": bytes.len()}}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32s(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn col<'a>(d: &'a Derived, name: &str) -> &'a Vec<Value> {
        let Derived::Table { table, .. } = d else { panic!("a table") };
        table["columns"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap()["values"].as_array().unwrap()
    }

    #[test]
    fn a_histogram_over_all_values_of_a_volume() {
        let v: Vec<f32> = (0..24).map(|i| i as f32).collect();
        let b = f32s(&v);
        let a = Array { dtype: "<f4", shape: &[2, 3, 4], bytes: &b };
        let d = derive(&parse(&json!({"kind": "histogram", "params": {"bins": 4}})).unwrap(), &a).unwrap();
        assert_eq!(col(&d, "count"), &vec![json!(6), json!(6), json!(6), json!(6)]);
        let d = derive(&parse(&json!({"kind": "histogram", "params": {"bins": 2, "per": {"axis": 0}}})).unwrap(), &a).unwrap();
        assert_eq!(col(&d, "plane"), &vec![json!(0), json!(0), json!(1), json!(1)]);
        assert_eq!(col(&d, "count"), &vec![json!(12), json!(0), json!(0), json!(12)]);
    }

    #[test]
    fn nan_and_inf_are_counted_apart() {
        let b = f32s(&[1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 2.0]);
        let a = Array { dtype: "<f4", shape: &[5], bytes: &b };
        let Derived::Table { meta, .. } = derive(&parse(&json!({"kind": "histogram"})).unwrap(), &a).unwrap() else { panic!() };
        assert_eq!((meta["nan"].as_u64(), meta["posinf"].as_u64(), meta["neginf"].as_u64()), (Some(1), Some(1), Some(1)));
    }

    #[test]
    fn junk_is_refused_never_a_panic() {
        for v in [
            json!(null),
            json!({"kind": "nope"}),
            json!({"kind": "histogram", "params": {"bins": 0}}),
            json!({"kind": "histogram", "params": {"bins": 1e12}}),
            json!({"kind": "histogram", "params": {"lo": 2, "hi": 1}}),
            json!({"kind": "histogram", "params": {"x": 1}}),
            json!({"kind": "stats", "params": {"percentiles": [101]}}),
            json!({"kind": "slice", "params": {"axes": [0, 0]}}),
            json!({"kind": "slice", "params": {"axes": [0]}}),
            json!({"kind": "histogram", "params": {"per": {"axis": 99}}}),
        ] {
            assert!(parse(&v).is_err(), "{v}");
        }
        let b = [0u8; 7];
        let v = parse(&json!({"kind": "stats"})).unwrap();
        assert!(derive(&v, &Array { dtype: "<f4", shape: &[2], bytes: &b }).is_err(), "the bytes don't fill the shape");
        assert!(derive(&v, &Array { dtype: "<U4", shape: &[1], bytes: &b }).is_err());
        assert!(derive(&v, &Array { dtype: "<f4", shape: &[u64::MAX, 4], bytes: &b }).is_err());
        assert!(derive(&v, &Array { dtype: "<f4", shape: &[], bytes: &b }).is_err());
        let h = parse(&json!({"kind": "histogram", "params": {"per": {"axis": 3}}})).unwrap();
        assert!(derive(&h, &Array { dtype: "|u1", shape: &[7], bytes: &b }).is_err(), "no such axis");
    }

    #[test]
    fn a_slice_of_a_volume() {
        let b: Vec<u8> = (0..24u8).collect();
        let a = Array { dtype: "|u1", shape: &[2, 3, 4], bytes: &b };
        let d = derive(&parse(&json!({"kind": "slice", "params": {"axes": [1, 2], "at": [1]}})).unwrap(), &a).unwrap();
        assert_eq!(d, Derived::Array { dtype: "|u1".into(), shape: vec![3, 4], bytes: (12..24).collect() });
        let d = derive(&parse(&json!({"kind": "slice", "params": {"axes": [0, 1], "at": [0, 0, 2]}})).unwrap(), &a).unwrap();
        assert_eq!(d, Derived::Array { dtype: "|u1".into(), shape: vec![2, 3], bytes: vec![2, 6, 10, 14, 18, 22] });
    }
}
