//! Charts laid out by the core, painted by every shell (note 26, the maintainer's decision 1A:
//! "the core lays charts out, each shell paints them"; slice 5). The core reads the value, picks
//! the marks, scales, nice ticks and their text (the core's number text), and lays them into a
//! **display list** in the chart's own points for the size the shell asks: the plot rectangle,
//! the marks, the axes with their ticks and titles, grid lines, and an accessibility outline. A
//! shell draws the list with its own 2D drawing and text; it computes nothing.
//!
//! Slice 5 plots what the egui editor's Plot viewer did, and the histogram B152 found: an (N, k)
//! table array whose columns name points (`x`, `y`), segments (`x0 y0 x1 y1` or `x1 y1 x2 y2`), a
//! histogram (`centre`, `count`: columns), or any (N, 2) table (points). Positions whose x and y
//! spans agree within [`SAME_SCALE`] are drawn to one scale, y down, as positions on a picture
//! are. Past [`MAX_MARKS`] rows the marks are an even sample (counted, said in `drawn`).
//!
//! Hostile input: every size is checked, NaN and infinite rows are left out, nothing panics.

use serde_json::{Value, json};

use crate::dtype::Dtype;

/// The most marks drawn (the rest are an even sample of the rows).
pub const MAX_MARKS: usize = 50_000;
/// When x and y spans are within this factor, positions are drawn to one scale, y down.
pub const SAME_SCALE: f64 = 20.0;
/// The size a chart is laid out at when the shell asks for none, and the bounds a shell may ask.
pub const DEFAULT_SIZE: (f64, f64) = (640.0, 400.0);
pub const MIN_SIDE: f64 = 120.0;
pub const MAX_SIDE: f64 = 8192.0;
/// The font size the layout assumes for tick text, and the share of it a character takes.
pub const FONT: f64 = 11.0;
pub(crate) const CHAR_W: f64 = 0.62;

/// What a table array is plotted as.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// Columns (x, y).
    Points(usize, usize),
    /// Columns (x0, y0, x1, y1).
    Segments([usize; 4]),
    /// A histogram: (centre, count) as columns.
    Columns(usize, usize),
    /// A 1-D array: its values against their index, a line (note 26 slice 6).
    Series,
}

/// What a table with these column names and this many columns plots as; `None`: no plot.
pub fn kind_of(names: &[String], columns: usize) -> Option<Kind> {
    let find = |n: &str| names.iter().position(|m| m == n);
    if names.len() == columns {
        for set in [["x0", "y0", "x1", "y1"], ["x1", "y1", "x2", "y2"]] {
            if let Some(c) = set.iter().map(|n| find(n)).collect::<Option<Vec<_>>>()
                && let [a, b, cc, d] = c[..]
            {
                return Some(Kind::Segments([a, b, cc, d]));
            }
        }
        if let (Some(x), Some(y)) = (find("x"), find("y")) {
            return Some(Kind::Points(x, y));
        }
        if let (Some(c), Some(n)) = (find("centre"), find("count")) {
            return Some(Kind::Columns(c, n));
        }
    }
    (columns == 2).then_some(Kind::Points(0, 1))
}

/// What a value's full reference says it plots as (a table array of two dimensions).
pub fn plotted(form: &plumcraft_proto::data::Form) -> Option<Kind> {
    match form {
        plumcraft_proto::data::Form::Array { shape, names, table: true, .. } => match shape.as_slice() {
            [_, k] => kind_of(names, usize::try_from(*k).ok()?),
            _ => None,
        },
        // a series: the egui editor's plot of a 1-D array, which has no picture
        plumcraft_proto::data::Form::Array { shape, table: false, .. } if shape.len() == 1 && shape[0] > 0 => Some(Kind::Series),
        _ => None,
    }
}

/// Nice tick values from `lo` to `hi`, about one every `px_per` points over `len` points: steps
/// of 1, 2 or 5 times a power of ten, at most 12 ticks.
pub fn ticks(lo: f64, hi: f64, len: f64, px_per: f64) -> Vec<f64> {
    if !(lo.is_finite() && hi.is_finite()) || hi <= lo || len <= 0.0 {
        return if lo.is_finite() { vec![lo] } else { Vec::new() };
    }
    let want = (len / px_per).clamp(2.0, 12.0);
    let raw = (hi - lo) / want;
    let p = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * p).find(|s| *s >= raw).unwrap_or(10.0 * p);
    let first = (lo / step).ceil();
    let mut out = Vec::new();
    let mut k = first;
    while out.len() < 13 {
        let v = k * step;
        if v > hi + step * 1e-9 {
            break;
        }
        // -0 and float dust read as clean numbers
        out.push(if v.abs() < step * 1e-9 { 0.0 } else { (v / step).round() * step });
        k += 1.0;
    }
    out
}

/// A linear scale from data to points.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Scale {
    pub(crate) d0: f64,
    pub(crate) d1: f64,
    pub(crate) p0: f64,
    pub(crate) p1: f64,
}

impl Scale {
    pub(crate) fn at(&self, v: f64) -> f64 {
        if self.d1 == self.d0 { (self.p0 + self.p1) / 2.0 } else { self.p0 + (v - self.d0) / (self.d1 - self.d0) * (self.p1 - self.p0) }
    }
}

/// A span with room around it, never empty.
pub(crate) fn padded(lo: f64, hi: f64, pad: f64) -> (f64, f64) {
    if !(lo.is_finite() && hi.is_finite()) {
        return (0.0, 1.0);
    }
    if hi <= lo {
        let d = if lo == 0.0 { 1.0 } else { lo.abs() * 0.1 };
        return (lo - d, hi + d);
    }
    let d = (hi - lo) * pad;
    (lo - d, hi + d)
}

pub(crate) fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// The rows of a table array, as numbers by column (each row's finite values only), sampled to
/// at most [`MAX_MARKS`] rows: `(rows read, rows in all)`.
fn rows(dtype: Dtype, bytes: &[u8], rows: usize, columns: usize, cols: &[usize]) -> (Vec<Vec<f64>>, usize) {
    let get = |r: usize, c: usize| dtype.read(bytes, r.checked_mul(columns)?.checked_add(c)?).filter(|v| v.is_finite());
    let step = rows.div_ceil(MAX_MARKS).max(1);
    let out = (0..rows).step_by(step).filter_map(|r| cols.iter().map(|c| get(r, *c)).collect::<Option<Vec<f64>>>()).collect();
    (out, rows)
}

/// A chart's display list for a table array's bytes, laid out at `size` points (module docs).
/// The M4 reduction of a series to at most four points a pixel column (Jugel et al. 2014: the
/// first, lowest, highest and last value of each column, in their order), so a line drawn
/// through them rasterises as the whole series would: `(index, value)` pairs, finite values only.
pub fn m4(values: &[f64], columns: usize) -> Vec<(usize, f64)> {
    let n = values.len();
    if n <= columns.saturating_mul(4) || columns == 0 {
        return values.iter().enumerate().filter(|(_, v)| v.is_finite()).map(|(i, v)| (i, *v)).collect();
    }
    let mut out = Vec::with_capacity(columns * 4);
    for c in 0..columns {
        let (a, b) = (c * n / columns, ((c + 1) * n / columns).min(n));
        let mut pick: Vec<(usize, f64)> = Vec::with_capacity(4);
        let (mut lo, mut hi, mut first, mut last) = (None::<(usize, f64)>, None::<(usize, f64)>, None, None);
        for (i, v) in values.iter().enumerate().take(b).skip(a) {
            if !v.is_finite() {
                continue;
            }
            first.get_or_insert((i, *v));
            last = Some((i, *v));
            if lo.is_none_or(|(_, l)| *v < l) {
                lo = Some((i, *v));
            }
            if hi.is_none_or(|(_, h)| *v > h) {
                hi = Some((i, *v));
            }
        }
        for p in [first, lo, hi, last].into_iter().flatten() {
            if !pick.iter().any(|q| q.0 == p.0) {
                pick.push(p);
            }
        }
        pick.sort_by_key(|p| p.0);
        out.extend(pick);
    }
    out
}

/// A 1-D array's line: its values against their index, reduced to the plot's width (module docs).
fn layout_series(dt: Dtype, n: usize, bytes: &[u8], size: (f64, f64)) -> Result<Value, String> {
    if n.checked_mul(dt.size()).is_none_or(|need| need > bytes.len()) {
        return Err(format!("{} bytes is too few for {n} values", bytes.len()));
    }
    let (w, h) = (size.0.clamp(MIN_SIDE, MAX_SIDE), size.1.clamp(MIN_SIDE, MAX_SIDE));
    let values: Vec<f64> = (0..n).map(|i| dt.read(bytes, i).unwrap_or(f64::NAN)).collect();
    let (y0, y1) = values.iter().filter(|v| v.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    let finite = y0.is_finite();
    let (y0p, y1p) = padded(y0, y1, 0.05);
    let int = dt.integer();
    let text = |v: f64| crate::viewers::number(v, int);
    let idx = |v: f64| crate::viewers::number(v, true);
    let yt_probe = ticks(y0p, y1p, h - 60.0, 48.0);
    let ywide = yt_probe.iter().map(|v| text(*v).chars().count()).max().unwrap_or(1) as f64 * FONT * CHAR_W;
    let left = (ywide + 14.0 + FONT + 6.0).ceil();
    let (top, right, bottom) = (12.0, 16.0, (FONT * 2.0 + 22.0).ceil());
    let plot = (left, top, (w - left - right).max(20.0), (h - top - bottom).max(20.0));
    let (x0, x1) = (0.0, (n.max(2) - 1) as f64);
    let sx = Scale { d0: x0, d1: x1, p0: plot.0, p1: plot.0 + plot.2 };
    let sy = Scale { d0: y0p, d1: y1p, p0: plot.1 + plot.3, p1: plot.1 };
    let reduced = m4(&values, plot.2.max(1.0) as usize);
    let mut xy = Vec::with_capacity(reduced.len() * 2);
    for (i, v) in &reduced {
        xy.extend([round2(sx.at(*i as f64)), round2(sy.at(*v))]);
    }
    let xt: Vec<f64> = ticks(x0, x1, plot.2, 80.0).into_iter().filter(|t| t.fract() == 0.0).collect();
    let yt = ticks(y0p, y1p, plot.3, 48.0);
    let drawn = values.iter().filter(|v| v.is_finite()).count();
    let summary = if finite { format!("{n} values by index; from {} to {}", text(y0), text(y1)) } else { format!("{n} values, none of them a number") };
    Ok(json!({
        "size": [w, h],
        "kind": "line",
        "sameScale": false,
        "plot": [round2(plot.0), round2(plot.1), round2(plot.2), round2(plot.3)],
        "marks": [{"kind": "line", "width": 1.5, "xy": xy}],
        "axes": [
            {"side": "bottom", "title": "index", "ticks": xt.iter().map(|v| json!({"at": round2(sx.at(*v)), "text": idx(*v)})).collect::<Vec<_>>()},
            {"side": "left", "title": "value", "ticks": yt.iter().map(|v| json!({"at": round2(sy.at(*v)), "text": text(*v)})).collect::<Vec<_>>()},
        ],
        "grid": {"x": xt.iter().map(|v| round2(sx.at(*v))).collect::<Vec<_>>(), "y": yt.iter().map(|v| round2(sy.at(*v))).collect::<Vec<_>>()},
        "font": FONT,
        "drawn": drawn,
        "rows": n,
        "points": reduced.len(),
        "a11y": {"title": "line of value against index", "summary": summary,
                 "x": {"label": "index", "min": idx(x0), "max": idx(x1)}, "y": {"label": "value", "min": text(y0p), "max": text(y1p)}},
    }))
}

pub fn layout(kind: &Kind, names: &[String], dtype: &str, shape: &[u64], bytes: &[u8], size: (f64, f64)) -> Result<Value, String> {
    let dt = Dtype::parse(dtype).ok_or_else(|| format!("a chart can't read dtype {dtype}"))?;
    if *kind == Kind::Series {
        let [n] = *shape else { return Err("a series is a 1-D array".into()) };
        return layout_series(dt, usize::try_from(n).map_err(|_| "too many values")?, bytes, size);
    }
    let [n, k] = *shape else { return Err("a chart needs a table of two dimensions".into()) };
    let (n, k) = (usize::try_from(n).map_err(|_| "too many rows")?, usize::try_from(k).map_err(|_| "too many columns")?);
    if n.checked_mul(k).and_then(|c| c.checked_mul(dt.size())).is_none_or(|need| need > bytes.len()) {
        return Err(format!("{} bytes is too few for {n}×{k} {dtype}", bytes.len()));
    }
    let (w, h) = (size.0.clamp(MIN_SIDE, MAX_SIDE), size.1.clamp(MIN_SIDE, MAX_SIDE));
    let cols: Vec<usize> = match kind {
        Kind::Points(x, y) | Kind::Columns(x, y) => vec![*x, *y],
        Kind::Segments(c) => c.to_vec(),
        Kind::Series => return Err("a series is laid out as a line".into()),
    };
    if cols.iter().any(|c| *c >= k) {
        return Err("a plotted column is past the table's last".into());
    }
    let (data, total) = rows(dt, bytes, n, k, &cols);
    let name = |c: usize| names.get(c).cloned().unwrap_or_else(|| format!("c{}", c + 1));
    // the spans: x from the even columns, y from the odd (a histogram's counts from zero)
    let span = |idx: &[usize]| {
        data.iter().flat_map(|r| idx.iter().filter_map(|i| r.get(*i).copied())).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)))
    };
    let (xi, yi): (Vec<usize>, Vec<usize>) = match kind {
        Kind::Segments(_) => (vec![0, 2], vec![1, 3]),
        _ => (vec![0], vec![1]),
    };
    let (mut x0, mut x1) = span(&xi);
    let (mut y0, mut y1) = span(&yi);
    let columns = matches!(kind, Kind::Columns(..));
    // a histogram's bars: from zero, a bin's width apart
    let bin = if columns && data.len() > 1 { (x1 - x0) / (data.len() - 1) as f64 } else { 0.0 };
    if columns {
        y0 = y0.min(0.0);
        x0 -= bin / 2.0;
        x1 += bin / 2.0;
    }
    let same = !columns && {
        let (sx, sy) = (x1 - x0, y1 - y0);
        sx.is_finite() && sy.is_finite() && sx <= sy * SAME_SCALE && sy <= sx * SAME_SCALE
    };
    (x0, x1) = if columns { (x0, x1) } else { padded(x0, x1, 0.04) };
    (y0, y1) = if columns { (y0, padded(y0, y1, 0.04).1) } else { padded(y0, y1, 0.04) };
    // margins: the y ticks' widest text, an x tick row and the axis titles
    let int = dt.integer();
    let text = |v: f64| crate::viewers::number(v, int);
    let yt_probe = ticks(y0, y1, h - 60.0, 48.0);
    let ywide = yt_probe.iter().map(|v| text(*v).chars().count()).max().unwrap_or(1) as f64 * FONT * CHAR_W;
    let left = (ywide + 14.0 + FONT + 6.0).ceil();
    let (top, right, bottom) = (12.0, 16.0, (FONT * 2.0 + 22.0).ceil());
    let mut plot = (left, top, (w - left - right).max(20.0), (h - top - bottom).max(20.0));
    // one scale for positions: the plot narrowed to the data's aspect, centred
    if same {
        let ppu = (plot.2 / (x1 - x0)).min(plot.3 / (y1 - y0));
        let (pw, ph) = (ppu * (x1 - x0), ppu * (y1 - y0));
        plot = (plot.0 + (plot.2 - pw) / 2.0, plot.1 + (plot.3 - ph) / 2.0, pw, ph);
    }
    let sx = Scale { d0: x0, d1: x1, p0: plot.0, p1: plot.0 + plot.2 };
    // y up for quantities; down for positions on a picture
    let sy = if same { Scale { d0: y0, d1: y1, p0: plot.1, p1: plot.1 + plot.3 } } else { Scale { d0: y0, d1: y1, p0: plot.1 + plot.3, p1: plot.1 } };
    let xt = ticks(x0, x1, plot.2, 80.0);
    let yt = ticks(y0, y1, plot.3, 48.0);
    let mut xy: Vec<f64> = Vec::new();
    let mark = match kind {
        // laid out by layout_series, never here
        Kind::Series => json!({"kind": "none"}),
        Kind::Points(..) => {
            for r in &data {
                if let [x, y] = r[..] {
                    xy.extend([round2(sx.at(x)), round2(sy.at(y))]);
                }
            }
            json!({"kind": "points", "r": if data.len() > 5000 { 1.0 } else { 2.0 }, "xy": xy})
        }
        Kind::Segments(_) => {
            for r in &data {
                if let [a, b, c, d] = r[..] {
                    xy.extend([round2(sx.at(a)), round2(sy.at(b)), round2(sx.at(c)), round2(sy.at(d))]);
                }
            }
            json!({"kind": "segments", "width": 1.0, "xy": xy})
        }
        Kind::Columns(..) => {
            // rects x, y, w, h: a bar a bin wide, from zero to the count
            let bw = (sx.at(x0 + bin) - sx.at(x0)).abs().max(0.5);
            let base = sy.at(0f64.max(y0));
            for r in &data {
                if let [c, v] = r[..] {
                    let (top, x) = (sy.at(v), sx.at(c) - bw / 2.0);
                    xy.extend([round2(x), round2(top.min(base)), round2(bw * 0.9), round2((base - top).abs())]);
                }
            }
            json!({"kind": "rects", "rects": xy})
        }
    };
    let tick = |s: &Scale, v: f64| json!({"at": round2(s.at(v)), "text": text(v)});
    let (xname, yname) = match kind {
        Kind::Points(x, y) | Kind::Columns(x, y) => (name(*x), name(*y)),
        Kind::Segments(c) => (format!("{}, {}", name(c[0]), name(c[2])), format!("{}, {}", name(c[1]), name(c[3]))),
        Kind::Series => ("index".into(), "value".into()),
    };
    let what = match kind {
        Kind::Points(..) => "points",
        Kind::Segments(_) => "segments",
        Kind::Columns(..) => "columns",
        Kind::Series => "line",
    };
    let drawn = data.len();
    let summary = format!(
        "{drawn} {what} of {total} rows; {xname} from {} to {}, {yname} from {} to {}",
        text(span(&xi).0),
        text(span(&xi).1),
        text(span(&yi).0),
        text(span(&yi).1)
    );
    Ok(json!({
        "size": [w, h],
        "kind": what,
        "sameScale": same,
        "plot": [round2(plot.0), round2(plot.1), round2(plot.2), round2(plot.3)],
        "marks": [mark],
        "axes": [
            {"side": "bottom", "title": xname, "ticks": xt.iter().map(|v| tick(&sx, *v)).collect::<Vec<_>>()},
            {"side": "left", "title": yname, "ticks": yt.iter().map(|v| tick(&sy, *v)).collect::<Vec<_>>()},
        ],
        "grid": {"x": xt.iter().map(|v| round2(sx.at(*v))).collect::<Vec<_>>(), "y": yt.iter().map(|v| round2(sy.at(*v))).collect::<Vec<_>>()},
        "font": FONT,
        "drawn": drawn,
        "rows": total,
        "a11y": {"title": format!("{what} of {yname} against {xname}"), "summary": summary,
                 "x": {"label": xname, "min": text(x0), "max": text(x1)}, "y": {"label": yname, "min": text(y0), "max": text(y1)}},
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32s(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }
    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn what_a_table_plots_as() {
        assert_eq!(kind_of(&names(&["x0", "y0", "x1", "y1"]), 4), Some(Kind::Segments([0, 1, 2, 3])));
        assert_eq!(kind_of(&names(&["id", "x1", "y1", "x2", "y2"]), 5), Some(Kind::Segments([1, 2, 3, 4])));
        assert_eq!(kind_of(&names(&["r", "y", "x"]), 3), Some(Kind::Points(2, 1)));
        assert_eq!(kind_of(&names(&["centre", "count"]), 2), Some(Kind::Columns(0, 1)));
        assert_eq!(kind_of(&[], 2), Some(Kind::Points(0, 1)));
        assert_eq!(kind_of(&[], 3), None);
        assert_eq!(kind_of(&names(&["x", "y"]), 3), None, "names that don't fit");
    }

    #[test]
    fn nice_ticks() {
        let t = ticks(0.0, 1.0, 400.0, 80.0);
        assert_eq!(t.len(), 6);
        assert!((t[0] - 0.0).abs() < 1e-12 && (t[5] - 1.0).abs() < 1e-12 && (t[1] - 0.2).abs() < 1e-12);
        assert_eq!(ticks(-3.0, 7.0, 500.0, 100.0), vec![-2.0, 0.0, 2.0, 4.0, 6.0]);
        assert_eq!(ticks(5.0, 5.0, 100.0, 50.0), vec![5.0]);
        assert!(ticks(f64::NAN, 1.0, 100.0, 50.0).is_empty());
        assert!(ticks(0.0, 1e300, 1e6, 1.0).len() <= 13, "at most 13 ticks");
    }

    /// B152's histogram as columns: a bar a bin, from zero to its count, the y axis from zero.
    #[test]
    fn a_histogram_is_columns_from_zero() {
        let v: Vec<f32> = (0..4).flat_map(|i| [i as f32 * 0.25 + 0.125, [10.0, 40.0, 20.0, 0.0][i]]).collect();
        let c = layout(&Kind::Columns(0, 1), &names(&["centre", "count"]), "<f4", &[4, 2], &f32s(&v), (400.0, 300.0)).unwrap();
        assert_eq!(c["kind"], "columns");
        let rects = c["marks"][0]["rects"].as_array().unwrap();
        assert_eq!(rects.len(), 16, "four bars, x y w h each");
        let plot = c["plot"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
        let bottom = plot[1] + plot[3];
        // every bar stands on the plot's bottom (y from zero); the tallest is the 40
        for b in rects.chunks(4) {
            let (y, hgt) = (b[1].as_f64().unwrap(), b[3].as_f64().unwrap());
            assert!((y + hgt - bottom).abs() < 0.02, "{y} + {hgt} vs {bottom}");
        }
        let tallest = rects.chunks(4).map(|b| b[3].as_f64().unwrap()).fold(0.0, f64::max);
        assert!((rects[7].as_f64().unwrap() - tallest).abs() < 1e-9, "the second bar is the tallest");
        assert_eq!(c["axes"][1]["ticks"][0]["text"], "0", "counts from zero");
        assert_eq!(c["axes"][0]["title"], "centre");
        assert!(c["a11y"]["summary"].as_str().unwrap().starts_with("4 columns of 4 rows"));
    }

    /// Positions of the same order are drawn to one scale, y down; two kinds of quantity are not.
    #[test]
    fn positions_share_a_scale_y_down() {
        let p = layout(&Kind::Points(0, 1), &names(&["x", "y"]), "<f4", &[2, 2], &f32s(&[0.0, 0.0, 100.0, 80.0]), (400.0, 400.0)).unwrap();
        assert_eq!(p["sameScale"], true);
        let xy: Vec<f64> = p["marks"][0]["xy"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert!(xy[1] < xy[3], "y = 0 is above y = 80: y runs down");
        assert!(((xy[2] - xy[0]) / (xy[3] - xy[1]) - 100.0 / 80.0).abs() < 0.01, "one scale");
        let q = layout(&Kind::Points(0, 1), &[], "<f4", &[2, 2], &f32s(&[0.0, 0.0, 10000.0, 1.0]), (400.0, 400.0)).unwrap();
        assert_eq!(q["sameScale"], false);
    }

    /// Note 26 slice 6: a 1-D array is a line by index; past four points a pixel column it is
    /// reduced (M4) to each column's first, lowest, highest and last value, keeping every extreme.
    #[test]
    fn a_series_is_a_line_reduced_without_losing_its_extremes() {
        let v: Vec<f64> = (0..10_000)
            .map(|i| {
                if i == 5003 {
                    9.0
                } else if i == 7001 {
                    -4.0
                } else {
                    (i as f64 * 0.01).sin()
                }
            })
            .collect();
        let r = m4(&v, 100);
        assert!(r.len() <= 400);
        assert!(r.iter().any(|p| p.0 == 5003 && p.1 == 9.0) && r.iter().any(|p| p.0 == 7001 && p.1 == -4.0), "the spikes kept");
        assert!(r.windows(2).all(|w| w[0].0 < w[1].0), "in order");
        assert_eq!(m4(&[1.0, f64::NAN, 3.0], 100), vec![(0, 1.0), (2, 3.0)], "short: every finite value");
        let bytes: Vec<u8> = v.iter().flat_map(|x| (*x as f32).to_le_bytes()).collect();
        let l = layout(&Kind::Series, &[], "<f4", &[10_000], &bytes, (640.0, 400.0)).unwrap();
        assert_eq!((l["kind"].as_str(), l["marks"][0]["kind"].as_str()), (Some("line"), Some("line")));
        assert!(l["points"].as_u64().unwrap() < 4 * 640);
        assert_eq!(l["axes"][0]["title"], "index");
        assert!(layout(&Kind::Series, &[], "<f4", &[10], &[0; 8], (640.0, 400.0)).is_err(), "too few bytes");
        let none = layout(&Kind::Series, &[], "<f4", &[2], &f32s(&[f32::NAN, f32::NAN]), (640.0, 400.0)).unwrap();
        assert!(none["a11y"]["summary"].as_str().unwrap().contains("none of them a number"));
        let form = |shape: Vec<u64>, table: bool| plumcraft_proto::data::Form::Array { dtype: "<f4".into(), shape, space: None, names: vec![], table };
        assert_eq!(plotted(&form(vec![5], false)), Some(Kind::Series));
        assert_eq!(plotted(&form(vec![0], false)), None);
        assert_eq!(plotted(&form(vec![4, 4], false)), None, "a picture is not a series");
    }

    #[test]
    fn hostile_tables_are_refused_or_skipped() {
        assert!(layout(&Kind::Points(0, 1), &[], "<f4", &[9, 2], &f32s(&[0.0; 4]), (400.0, 300.0)).is_err(), "too few bytes");
        assert!(layout(&Kind::Points(0, 1), &[], "<c8", &[1, 2], &[0; 16], (400.0, 300.0)).is_err());
        assert!(layout(&Kind::Points(0, 5), &[], "<f4", &[1, 2], &f32s(&[0.0; 2]), (400.0, 300.0)).is_err());
        assert!(layout(&Kind::Points(0, 1), &[], "<f4", &[u64::MAX, 2], &[], (400.0, 300.0)).is_err());
        let nan = layout(&Kind::Points(0, 1), &[], "<f4", &[3, 2], &f32s(&[0.0, 1.0, f32::NAN, 2.0, 4.0, f32::INFINITY]), (1e9, -5.0)).unwrap();
        assert_eq!(nan["drawn"], 1, "NaN and infinite rows left out");
        assert_eq!(nan["size"], json!([MAX_SIDE, MIN_SIDE]), "the size held to its bounds");
        let many: Vec<f32> = (0..(MAX_MARKS * 3) as u32).flat_map(|i| [i as f32, (i % 7) as f32]).collect();
        let big = layout(&Kind::Points(0, 1), &[], "<f4", &[(MAX_MARKS * 3) as u64, 2], &f32s(&many), (640.0, 400.0)).unwrap();
        assert!(big["drawn"].as_u64().unwrap() <= MAX_MARKS as u64);
        assert_eq!(big["rows"].as_u64(), Some((MAX_MARKS * 3) as u64));
    }
}
