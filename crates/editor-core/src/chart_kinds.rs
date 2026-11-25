//! The chart kinds the maintainer named (note 26 slices 7 to 9: "Histograms. Line charts, stacked
//! line charts, column charts, stacked column charts, treemaps"), laid out by the core from a
//! table's columns and a chart spec (`plumcraft_session::chart_cmds`), into the same display list
//! a shell paints for any chart ([`crate::chart`]).
//!
//! - **histogram:** `x` a numeric column in `bins` bins (30 by default) over its range; with
//!   `color`, a bar a category in each bin, stacked.
//! - **column:** `x` a column of categories (in the order they first appear, at most
//!   [`MAX_CATEGORIES`], the rest summed as "other"), `y` a numeric column aggregated by `agg`
//!   (sum; count when there is no `y`); with `color` a bar a category, side by side or stacked.
//! - **line:** `y` one or more numeric columns against `x` (a numeric column, else the row's
//!   index), sorted by x, the rows of one x made one point by `agg` (sum by default), each reduced
//!   to the plot's width (M4); with `color`, the first `y` split
//!   by category; with `stack`, filled areas stacked on each other (values summed per x).
//! - **treemap:** `path` one to three columns of categories, each group's rectangle as large as
//!   its `size` (a numeric column, summed; the rows counted when there is none), squarified
//!   (Bruls, Huizing and van Wijk 2000), coloured by the first level, labelled where it fits.
//!
//! Every mark carries its colour (a categorical palette, [`PALETTE`]); a legend names the series.
//! Hostile input: rows that aren't numbers are left out, every count is bounded, nothing panics.

use std::collections::HashMap;

use plumcraft_media::table::{Cell, Table};
use serde_json::{Value, json};

use crate::chart::{self, CHAR_W, FONT, MAX_SIDE, MIN_SIDE, Scale, padded, round2, ticks};

/// The colours of the series, in order (ten categorical colours, readable on light and dark).
pub const PALETTE: &[&str] = &["#4e79a7", "#f28e2b", "#e15759", "#76b7b2", "#59a14f", "#edc948", "#b07aa1", "#ff9da7", "#9c755f", "#bab0ac"];
/// The most categories on an axis or in a legend (the rest are "other").
pub const MAX_CATEGORIES: usize = 60;
pub const MAX_SERIES: usize = 10;
pub const DEFAULT_BINS: usize = 30;
/// The most rows read for a chart.
pub const MAX_ROWS: usize = 5_000_000;

fn colour(k: usize) -> &'static str {
    PALETTE[k % PALETTE.len()]
}

/// A cell as a category's name.
fn category(c: Option<&Cell>) -> String {
    match c {
        None | Some(Cell::Null) => "(none)".into(),
        Some(Cell::Bool(b)) => b.to_string(),
        Some(Cell::Int(i)) => i.to_string(),
        Some(Cell::Num(f)) => crate::viewers::number(*f, false),
        Some(Cell::Text(t) | Cell::Json(t)) => t.chars().take(80).collect(),
    }
}

fn col<'a>(t: &'a Table, name: Option<&str>) -> Option<&'a plumcraft_media::table::Column> {
    let n = name?;
    t.columns.iter().find(|c| c.name == n)
}

/// The spec a table gets when none was chosen: a column chart of its first measure (a column with
/// fractions, else its first numeric column) by its first other column, else a histogram of it.
pub fn auto_spec(t: &Table) -> Value {
    // a measure rather than a count or a code: a column with fractions first, then any number
    let fractional = |c: &&plumcraft_media::table::Column| c.numeric() && c.cells.iter().any(|x| matches!(x, Cell::Num(_)));
    let num = t.columns.iter().find(fractional).or_else(|| t.columns.iter().find(|c| c.numeric())).map(|c| c.name.clone());
    let cat = t.columns.iter().find(|c| !c.numeric()).map(|c| c.name.clone());
    match (cat, num) {
        (Some(x), Some(y)) => json!({"mark": "column", "x": x, "y": [y], "agg": "sum"}),
        (None, Some(y)) => json!({"mark": "histogram", "x": y}),
        (Some(x), None) => json!({"mark": "column", "x": x, "agg": "count"}),
        (None, None) => json!({"mark": "column"}),
    }
}

/// Categories in the order they first appear, the rest past `max` as "other"; and each row's.
fn categories(c: Option<&plumcraft_media::table::Column>, rows: usize, max: usize) -> (Vec<String>, Vec<usize>) {
    let mut names: Vec<String> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut of = Vec::with_capacity(rows);
    let mut other = None;
    for r in 0..rows {
        let name = c.map_or_else(|| "all".to_owned(), |c| category(c.cells.get(r)));
        let k = match index.get(&name) {
            Some(k) => *k,
            None if names.len() < max => {
                names.push(name.clone());
                index.insert(name, names.len() - 1);
                names.len() - 1
            }
            None => *other.get_or_insert_with(|| {
                names.push("other".into());
                names.len() - 1
            }),
        };
        of.push(k);
    }
    (names, of)
}

/// The frame every chart with axes shares: the plot rectangle for y ticks this wide.
fn frame(w: f64, h: f64, ywide: f64, legend: bool) -> (f64, f64, f64, f64) {
    let left = (ywide + 14.0 + FONT + 6.0).ceil();
    let right = if legend { 130.0 } else { 16.0 };
    let (top, bottom) = (12.0, (FONT * 2.0 + 22.0).ceil());
    (left, top, (w - left - right).max(20.0), (h - top - bottom).max(20.0))
}

struct Out {
    marks: Vec<Value>,
    legend: Vec<Value>,
    kind: &'static str,
}

/// The display list for `spec` over `t`, at `size` (module docs).
pub fn layout(spec: &Value, t: &Table, size: (f64, f64)) -> Result<Value, String> {
    let (w, h) = (size.0.clamp(MIN_SIDE, MAX_SIDE), size.1.clamp(MIN_SIDE, MAX_SIDE));
    let rows = t.rows.min(MAX_ROWS);
    let mark = spec.get("mark").and_then(Value::as_str).unwrap_or("column");
    let s = |k: &str| spec.get(k).and_then(Value::as_str);
    let ys: Vec<&str> = spec.get("y").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let stack = spec.get("stack").and_then(Value::as_bool).unwrap_or(false);
    let num = |x: f64| crate::viewers::number(x, false);
    if mark == "treemap" {
        return treemap(spec, t, rows, (w, h));
    }
    // the y axis's range and how its ticks read, decided per kind below
    let mut xticks: Vec<Value> = Vec::new();
    let (xtitle, ytitle);
    let (lo, hi);
    let legend_on = s("color").is_some() || ys.len() > 1;
    let out: Out;
    let plot;
    let sy;
    match mark {
        "histogram" => {
            // x, or the first column of numbers when x isn't one
            let xc =
                col(t, s("x")).filter(|c| c.numeric()).or_else(|| t.columns.iter().find(|c| c.numeric())).ok_or("a histogram needs a column of numbers")?;
            let bins = spec.get("bins").and_then(Value::as_u64).map_or(DEFAULT_BINS, |b| b.clamp(1, 1000) as usize);
            let vals: Vec<(usize, f64)> = (0..rows).filter_map(|r| xc.cells.get(r).and_then(Cell::num).filter(|v| v.is_finite()).map(|v| (r, v))).collect();
            let (a, b) = vals.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (_, v)| (a.min(*v), b.max(*v)));
            let (a, b) = if a.is_finite() && b > a { (a, b) } else { padded(a, b, 0.0) };
            let (cats, of) = categories(col(t, s("color")), rows, MAX_SERIES);
            let mut counts = vec![vec![0u64; bins]; cats.len().max(1)];
            for (r, v) in &vals {
                let k = (((v - a) / (b - a) * bins as f64) as usize).min(bins - 1);
                if let Some(c) = counts.get_mut(of.get(*r).copied().unwrap_or(0)) {
                    c[k] += 1;
                }
            }
            let top = (0..bins).map(|k| counts.iter().map(|c| c[k]).sum::<u64>()).max().unwrap_or(0) as f64;
            (lo, hi) = (0.0, padded(0.0, top.max(1.0), 0.04).1);
            let yt = ticks(lo, hi, h - 60.0, 48.0);
            let ywide = yt.iter().map(|v| num(*v).chars().count()).max().unwrap_or(1) as f64 * FONT * CHAR_W;
            plot = frame(w, h, ywide, legend_on);
            let sx = Scale { d0: a, d1: b, p0: plot.0, p1: plot.0 + plot.2 };
            sy = Scale { d0: lo, d1: hi, p0: plot.1 + plot.3, p1: plot.1 };
            let bw = plot.2 / bins as f64;
            let mut marks = Vec::new();
            let mut base = vec![0u64; bins];
            for (ci, c) in counts.iter().enumerate() {
                let mut rects = Vec::new();
                for k in 0..bins {
                    if c[k] == 0 {
                        continue;
                    }
                    let (y0, y1) = (sy.at(base[k] as f64), sy.at((base[k] + c[k]) as f64));
                    rects.extend([round2(plot.0 + k as f64 * bw), round2(y1), round2((bw - 1.0).max(0.5)), round2(y0 - y1)]);
                    base[k] += c[k];
                }
                marks.push(json!({"kind": "rects", "color": colour(ci), "rects": rects}));
            }
            for v in ticks(a, b, plot.2, 80.0) {
                xticks.push(json!({"at": round2(sx.at(v)), "text": num(v)}));
            }
            let legend = if s("color").is_some() { cats.iter().enumerate().map(|(i, c)| json!({"text": c, "color": colour(i)})).collect() } else { Vec::new() };
            out = Out { marks, legend, kind: "histogram" };
            (xtitle, ytitle) = (xc.name.clone(), "count".to_owned());
        }
        "column" => {
            let xc = col(t, s("x"));
            let (cats, xof) = categories(xc, rows, MAX_CATEGORIES);
            let yc = ys.first().and_then(|y| col(t, Some(y))).filter(|c| c.numeric());
            let agg = if yc.is_none() { "count" } else { s("agg").unwrap_or("sum") };
            let (series, sof) = categories(col(t, s("color")), rows, MAX_SERIES);
            // [series][category]: sum, count, min, max
            let n = cats.len().max(1);
            let mut acc = vec![vec![(0.0f64, 0u64, f64::INFINITY, f64::NEG_INFINITY); n]; series.len().max(1)];
            for r in 0..rows {
                let v = match yc {
                    Some(c) => match c.cells.get(r).and_then(Cell::num).filter(|v| v.is_finite()) {
                        Some(v) => v,
                        None => continue,
                    },
                    None => 1.0,
                };
                let (si, ci) = (sof.get(r).copied().unwrap_or(0), xof.get(r).copied().unwrap_or(0));
                if let Some(a) = acc.get_mut(si).and_then(|s| s.get_mut(ci)) {
                    *a = (a.0 + v, a.1 + 1, a.2.min(v), a.3.max(v));
                }
            }
            let value = |a: &(f64, u64, f64, f64)| match agg {
                _ if a.1 == 0 => 0.0,
                "mean" => a.0 / a.1 as f64,
                "count" => a.1 as f64,
                "min" => a.2,
                "max" => a.3,
                _ => a.0,
            };
            let vals: Vec<Vec<f64>> = acc.iter().map(|s| s.iter().map(value).collect()).collect();
            let (mut a, mut b) = (0.0f64, 0.0f64);
            for c in 0..n {
                if stack {
                    let (pos, neg): (f64, f64) = vals.iter().map(|s| s[c]).fold((0.0, 0.0), |(p, q), v| if v >= 0.0 { (p + v, q) } else { (p, q + v) });
                    (a, b) = (a.min(neg), b.max(pos));
                } else {
                    for s in &vals {
                        (a, b) = (a.min(s[c]), b.max(s[c]));
                    }
                }
            }
            (lo, hi) = (a, if b > a { padded(a, b, 0.04).1 } else { a + 1.0 });
            let yt = ticks(lo, hi, h - 60.0, 48.0);
            let ywide = yt.iter().map(|v| num(*v).chars().count()).max().unwrap_or(1) as f64 * FONT * CHAR_W;
            plot = frame(w, h, ywide, legend_on);
            sy = Scale { d0: lo, d1: hi, p0: plot.1 + plot.3, p1: plot.1 };
            let band = plot.2 / n as f64;
            let groups = if stack { 1 } else { vals.len() };
            let bar = (band * 0.8 / groups as f64).max(0.5);
            let mut marks = Vec::new();
            let mut up = vec![0.0f64; n];
            let mut down = vec![0.0f64; n];
            for (si, s) in vals.iter().enumerate() {
                let mut rects = Vec::new();
                for (c, v) in s.iter().enumerate() {
                    let x = plot.0 + c as f64 * band + band * 0.1 + if stack { 0.0 } else { si as f64 * bar };
                    let (from, to) = if stack {
                        let base = if *v >= 0.0 { &mut up[c] } else { &mut down[c] };
                        let from = *base;
                        *base += v;
                        (from, *base)
                    } else {
                        (0.0, *v)
                    };
                    let (ya, yb) = (sy.at(from), sy.at(to));
                    rects.extend([round2(x), round2(ya.min(yb)), round2(bar.max(0.5)), round2((ya - yb).abs())]);
                }
                marks.push(json!({"kind": "rects", "color": colour(si), "rects": rects}));
            }
            // a category's name under its band, every k-th when they crowd
            let every = ((cats.iter().map(|c| c.chars().count().min(14)).max().unwrap_or(1) as f64 * FONT * CHAR_W + 8.0) / band).ceil().max(1.0) as usize;
            for (c, name) in cats.iter().enumerate().step_by(every) {
                let short: String = if name.chars().count() > 14 { name.chars().take(13).chain(['…']).collect() } else { name.clone() };
                xticks.push(json!({"at": round2(plot.0 + (c as f64 + 0.5) * band), "text": short}));
            }
            let legend =
                if s("color").is_some() { series.iter().enumerate().map(|(i, c)| json!({"text": c, "color": colour(i)})).collect() } else { Vec::new() };
            out = Out { marks, legend, kind: if stack { "stacked column" } else { "column" } };
            xtitle = xc.map_or_else(|| "all".into(), |c| c.name.clone());
            ytitle = yc.map_or_else(|| "count".into(), |c| if agg == "sum" { c.name.clone() } else { format!("{agg} of {}", c.name) });
        }
        "line" => {
            let xc = col(t, s("x")).filter(|c| c.numeric());
            let x_of = |r: usize| xc.map_or(Some(r as f64), |c| c.cells.get(r).and_then(Cell::num).filter(|v| v.is_finite()));
            let ycols: Vec<&plumcraft_media::table::Column> = ys.iter().filter_map(|y| col(t, Some(y))).filter(|c| c.numeric()).collect();
            if ycols.is_empty() {
                return Err("a line needs y, a column of numbers".into());
            }
            // series: each y column, or the first split by colour
            let (names, sof) = categories(col(t, s("color")), rows, MAX_SERIES);
            let split = s("color").is_some();
            let series: Vec<(String, Vec<(f64, f64)>)> = if split {
                let mut v: Vec<Vec<(f64, f64)>> = vec![Vec::new(); names.len()];
                for r in 0..rows {
                    if let (Some(x), Some(y)) = (x_of(r), ycols[0].cells.get(r).and_then(Cell::num).filter(|v| v.is_finite())) {
                        v[sof.get(r).copied().unwrap_or(0).min(names.len() - 1)].push((x, y));
                    }
                }
                names.iter().cloned().zip(v).collect()
            } else {
                ycols
                    .iter()
                    .take(MAX_SERIES)
                    .map(|c| {
                        (c.name.clone(), (0..rows).filter_map(|r| Some((x_of(r)?, c.cells.get(r).and_then(Cell::num).filter(|v| v.is_finite())?))).collect())
                    })
                    .collect()
            };
            // sorted by x, the rows of one x made one point by `agg` (sum by default: a line of
            // a table with several rows a month is the month's total, not a zigzag through them)
            let agg = s("agg").unwrap_or("sum");
            let mut series: Vec<(String, Vec<(f64, f64)>)> = series
                .into_iter()
                .map(|(n, mut p)| {
                    p.sort_by(|a, b| a.0.total_cmp(&b.0));
                    (n, aggregate(&p, agg))
                })
                .collect();
            // stacked: each series summed per x over the x of them all, missing as zero
            let xs: Vec<f64> = if stack {
                let mut xs: Vec<f64> = series.iter().flat_map(|(_, p)| p.iter().map(|q| q.0)).collect();
                xs.sort_by(f64::total_cmp);
                xs.dedup();
                xs.truncate(200_000);
                xs
            } else {
                Vec::new()
            };
            if stack {
                for (_, p) in &mut series {
                    let mut sums = vec![0.0; xs.len()];
                    for (x, y) in p.iter() {
                        if let Ok(i) = xs.binary_search_by(|v| v.total_cmp(x)) {
                            sums[i] += y;
                        }
                    }
                    *p = xs.iter().copied().zip(sums).collect();
                }
            }
            let (mut a, mut b) = (f64::INFINITY, f64::NEG_INFINITY);
            let (mut ya, mut yb) = (if stack { 0.0 } else { f64::INFINITY }, if stack { 0.0 } else { f64::NEG_INFINITY });
            let mut cum = vec![0.0; xs.len()];
            for (_, p) in &series {
                for (i, (x, y)) in p.iter().enumerate() {
                    (a, b) = (a.min(*x), b.max(*x));
                    if stack {
                        cum[i] += y;
                        (ya, yb) = (ya.min(cum[i]), yb.max(cum[i]));
                    } else {
                        (ya, yb) = (ya.min(*y), yb.max(*y));
                    }
                }
            }
            if !a.is_finite() {
                return Err("no rows with numbers to draw".into());
            }
            (lo, hi) = padded(ya, yb, 0.05);
            let (lo, hi) = if stack { (lo.max(ya.min(0.0)), hi) } else { (lo, hi) };
            let yt = ticks(lo, hi, h - 60.0, 48.0);
            let ywide = yt.iter().map(|v| num(*v).chars().count()).max().unwrap_or(1) as f64 * FONT * CHAR_W;
            plot = frame(w, h, ywide, legend_on);
            let (x0, x1) = if b > a { (a, b) } else { padded(a, b, 0.0) };
            let sx = Scale { d0: x0, d1: x1, p0: plot.0, p1: plot.0 + plot.2 };
            sy = Scale { d0: lo, d1: hi, p0: plot.1 + plot.3, p1: plot.1 };
            let mut marks = Vec::new();
            let mut below = vec![0.0; xs.len()];
            for (si, (_, p)) in series.iter().enumerate() {
                if stack {
                    // a filled band from the stack below it to its own top
                    let tops: Vec<f64> = p.iter().enumerate().map(|(i, (_, y))| below[i] + y).collect();
                    let mut xy = Vec::with_capacity(p.len() * 4);
                    for (i, (x, _)) in p.iter().enumerate() {
                        xy.extend([round2(sx.at(*x)), round2(sy.at(tops[i]))]);
                    }
                    for (i, (x, _)) in p.iter().enumerate().rev() {
                        xy.extend([round2(sx.at(*x)), round2(sy.at(below[i]))]);
                    }
                    below = tops;
                    marks.push(json!({"kind": "area", "color": colour(si), "xy": xy}));
                } else {
                    let ys: Vec<f64> = p.iter().map(|q| q.1).collect();
                    let reduced = chart::m4(&ys, plot.2.max(1.0) as usize);
                    let mut xy = Vec::with_capacity(reduced.len() * 2);
                    for (i, y) in reduced {
                        if let Some((x, _)) = p.get(i) {
                            xy.extend([round2(sx.at(*x)), round2(sy.at(y))]);
                        }
                    }
                    marks.push(json!({"kind": "line", "color": colour(si), "width": 1.5, "xy": xy}));
                }
            }
            for v in ticks(x0, x1, plot.2, 80.0) {
                xticks.push(json!({"at": round2(sx.at(v)), "text": num(v)}));
            }
            let legend = if series.len() > 1 || split {
                series.iter().enumerate().map(|(i, (n, _))| json!({"text": n, "color": colour(i)})).collect()
            } else {
                Vec::new()
            };
            out = Out { marks, legend, kind: if stack { "stacked line" } else { "line" } };
            xtitle = xc.map_or_else(|| "row".into(), |c| c.name.clone());
            ytitle = if series.len() == 1 { series[0].0.clone() } else { ycols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ") };
        }
        other => return Err(format!("mark {:?}: histogram, line, column or treemap", other.chars().take(32).collect::<String>())),
    }
    let yticks: Vec<Value> = ticks(lo, hi, plot.3, 48.0).into_iter().map(|v| json!({"at": round2(sy.at(v)), "text": num(v)})).collect();
    let grid_y: Vec<Value> = yticks.iter().map(|t| t["at"].clone()).collect();
    let mut legend = out.legend;
    for (i, l) in legend.iter_mut().enumerate() {
        l["x"] = json!(round2(plot.0 + plot.2 + 14.0));
        l["y"] = json!(round2(plot.1 + 4.0 + i as f64 * (FONT + 7.0)));
    }
    let summary = format!("{} of {ytitle} by {xtitle}, {} rows", out.kind, t.rows);
    Ok(json!({
        "size": [w, h],
        "kind": out.kind,
        "sameScale": false,
        "plot": [round2(plot.0), round2(plot.1), round2(plot.2), round2(plot.3)],
        "marks": out.marks,
        "axes": [
            {"side": "bottom", "title": xtitle, "ticks": xticks},
            {"side": "left", "title": ytitle, "ticks": yticks},
        ],
        "grid": {"x": [], "y": grid_y},
        "legend": legend,
        "font": FONT,
        "drawn": t.rows.min(MAX_ROWS),
        "rows": t.rows,
        "a11y": {"title": format!("{} of {ytitle} by {xtitle}", out.kind), "summary": summary,
                 "x": {"label": xtitle, "min": "", "max": ""}, "y": {"label": ytitle, "min": num(lo), "max": num(hi)}},
    }))
}

/// Points sorted by x with the y of equal x made one by `agg` (sum, mean, count, min, max).
fn aggregate(p: &[(f64, f64)], agg: &str) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(p.len());
    let mut i = 0;
    while i < p.len() {
        let x = p[i].0;
        let mut j = i;
        while j < p.len() && p[j].0 == x {
            j += 1;
        }
        let ys = p[i..j].iter().map(|q| q.1);
        let y = match agg {
            "mean" => ys.sum::<f64>() / (j - i) as f64,
            "count" => (j - i) as f64,
            "min" => ys.fold(f64::INFINITY, f64::min),
            "max" => ys.fold(f64::NEG_INFINITY, f64::max),
            _ => ys.sum(),
        };
        out.push((x, y));
        i = j;
    }
    out
}

/// Rectangles for `values` (each > 0) filling `r` = (x, y, w, h), squarified: each row of
/// rectangles along the shorter side kept while adding one makes its worst aspect better.
pub fn squarify(values: &[f64], r: (f64, f64, f64, f64)) -> Vec<(f64, f64, f64, f64)> {
    let total: f64 = values.iter().filter(|v| **v > 0.0).sum();
    let mut out = vec![(r.0, r.1, 0.0, 0.0); values.len()];
    if total <= 0.0 || r.2 <= 0.0 || r.3 <= 0.0 {
        return out;
    }
    let mut order: Vec<usize> = (0..values.len()).filter(|i| values[*i] > 0.0).collect();
    order.sort_by(|a, b| values[*b].total_cmp(&values[*a]));
    let scale = r.2 * r.3 / total;
    let (mut x, mut y, mut w, mut h) = r;
    let worst = |row: &[f64], side: f64| -> f64 {
        let s: f64 = row.iter().sum();
        if s <= 0.0 || side <= 0.0 {
            return f64::INFINITY;
        }
        let (mx, mn) = row.iter().fold((0.0f64, f64::INFINITY), |(a, b), v| (a.max(*v), b.min(*v)));
        (side * side * mx / (s * s)).max(s * s / (side * side * mn))
    };
    let mut i = 0;
    while i < order.len() {
        let side = w.min(h);
        let mut row: Vec<f64> = vec![values[order[i]] * scale];
        let mut j = i + 1;
        while j < order.len() {
            let mut next = row.clone();
            next.push(values[order[j]] * scale);
            if worst(&next, side) > worst(&row, side) {
                break;
            }
            row = next;
            j += 1;
        }
        let s: f64 = row.iter().sum();
        if w >= h {
            // a column at the left, as wide as the row's area over the height
            let cw = if h > 0.0 { s / h } else { 0.0 };
            let mut yy = y;
            for (k, a) in row.iter().enumerate() {
                let rh = if cw > 0.0 { a / cw } else { 0.0 };
                out[order[i + k]] = (x, yy, cw, rh);
                yy += rh;
            }
            x += cw;
            w -= cw;
        } else {
            let rh = if w > 0.0 { s / w } else { 0.0 };
            let mut xx = x;
            for (k, a) in row.iter().enumerate() {
                let rw = if rh > 0.0 { a / rh } else { 0.0 };
                out[order[i + k]] = (xx, y, rw, rh);
                xx += rw;
            }
            y += rh;
            h -= rh;
        }
        i = j;
    }
    out
}

/// A treemap of `path`'s groups sized by `size` (module docs).
fn treemap(spec: &Value, t: &Table, rows: usize, (w, h): (f64, f64)) -> Result<Value, String> {
    let path: Vec<&plumcraft_media::table::Column> = spec
        .get("path")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).filter_map(|n| col(t, Some(n))).take(3).collect())
        .unwrap_or_default();
    let path = if path.is_empty() { t.columns.iter().filter(|c| !c.numeric()).take(1).collect() } else { path };
    if path.is_empty() {
        return Err("a treemap needs path, one or more columns of categories".into());
    }
    let size = col(t, spec.get("size").and_then(Value::as_str)).filter(|c| c.numeric());
    // the groups, a tree by path: each node's total and children, in first-seen order
    #[derive(Default)]
    struct Node {
        total: f64,
        children: Vec<(String, Node)>,
    }
    let mut root = Node::default();
    for r in 0..rows {
        let v = match size {
            Some(c) => match c.cells.get(r).and_then(Cell::num).filter(|v| v.is_finite() && *v > 0.0) {
                Some(v) => v,
                None => continue,
            },
            None => 1.0,
        };
        let mut n = &mut root;
        n.total += v;
        for c in &path {
            let name = category(c.cells.get(r));
            let k = match n.children.iter().position(|(m, _)| *m == name) {
                Some(k) => k,
                None if n.children.len() < 400 => {
                    n.children.push((name, Node::default()));
                    n.children.len() - 1
                }
                None => n.children.len() - 1,
            };
            n = &mut n.children[k].1;
            n.total += v;
        }
    }
    let plot = (2.0, 2.0, w - 4.0, h - 4.0);
    let mut legend = Vec::new();
    let tops: Vec<f64> = root.children.iter().map(|(_, n)| n.total).collect();
    let top_rects = squarify(&tops, plot);
    struct Placed {
        rects: Vec<Vec<f64>>,
        labels: Vec<Value>,
    }
    // `top`: the first level's name, said under a nested group's own (its colour alone doesn't name it)
    fn place(n: &Node, r: (f64, f64, f64, f64), c: usize, (name, top): (&str, &str), out: &mut Placed, depth: usize) {
        if n.children.is_empty() || depth > 3 {
            let inset = (r.0 + 1.0, r.1 + 1.0, (r.2 - 2.0).max(0.0), (r.3 - 2.0).max(0.0));
            if let Some(v) = out.rects.get_mut(c) {
                v.extend([round2(inset.0), round2(inset.1), round2(inset.2), round2(inset.3)]);
            }
            if inset.2 >= name.chars().count().min(24) as f64 * FONT * CHAR_W + 6.0 && inset.3 >= FONT * 2.6 {
                let total = crate::viewers::number(n.total, false);
                let detail = if depth > 1 { format!("{} · {total}", top.chars().take(20).collect::<String>()) } else { total };
                out.labels
                    .push(json!({"x": round2(inset.0 + 4.0), "y": round2(inset.1 + 3.0), "text": name.chars().take(24).collect::<String>(), "detail": detail}));
            }
            return;
        }
        let kids: Vec<f64> = n.children.iter().map(|(_, k)| k.total).collect();
        for ((child_name, child), cr) in n.children.iter().zip(squarify(&kids, r)) {
            place(child, cr, c, (child_name, top), out, depth + 1);
        }
    }
    let mut placed = Placed { rects: vec![Vec::new(); PALETTE.len()], labels: Vec::new() };
    for (k, ((name, n), r)) in root.children.iter().zip(top_rects).enumerate() {
        let c = k % PALETTE.len();
        if k < MAX_SERIES {
            legend.push(json!({"text": name, "color": colour(c)}));
        }
        place(n, r, c, (name, name), &mut placed, 1);
    }
    let mut marks: Vec<Value> =
        placed.rects.into_iter().enumerate().filter(|(_, r)| !r.is_empty()).map(|(c, r)| json!({"kind": "rects", "color": colour(c), "rects": r})).collect();
    marks.push(json!({"kind": "labels", "labels": placed.labels}));
    let path_names: Vec<&str> = path.iter().map(|c| c.name.as_str()).collect();
    let what = size.map_or_else(|| "rows".to_owned(), |c| c.name.clone());
    Ok(json!({
        "size": [w, h],
        "kind": "treemap",
        "sameScale": false,
        "plot": [plot.0, plot.1, round2(plot.2), round2(plot.3)],
        "marks": marks,
        "axes": [],
        "grid": {"x": [], "y": []},
        "legend": [],
        "groups": legend,
        "font": FONT,
        "drawn": t.rows.min(MAX_ROWS),
        "rows": t.rows,
        "a11y": {"title": format!("treemap of {what} by {}", path_names.join(" / ")),
                 "summary": format!("{} groups of {what} by {}, {} in all", root.children.len(), path_names.join(" / "), crate::viewers::number(root.total, false)),
                 "x": {"label": path_names.join(" / "), "min": "", "max": ""}, "y": {"label": what, "min": "", "max": ""}},
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        Table::from_value(&json!({"columns": [
            {"name": "region", "type": "STRING", "values": ["north", "south", "north", "east", "south", "north"]},
            {"name": "product", "type": "STRING", "values": ["a", "a", "b", "a", "b", "a"]},
            {"name": "month", "type": "INT", "values": [1, 1, 2, 2, 3, 3]},
            {"name": "amount", "type": "FLOAT", "values": [10.0, 20.0, 5.0, 7.0, 3.0, 15.0]},
        ]}))
        .unwrap()
    }

    fn rects(l: &Value, m: usize) -> Vec<f64> {
        l["marks"][m]["rects"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect()
    }

    #[test]
    fn a_column_chart_sums_by_category_side_by_side_or_stacked() {
        let t = table();
        let l = layout(&json!({"mark": "column", "x": "region", "y": ["amount"]}), &t, (600.0, 400.0)).unwrap();
        assert_eq!(l["kind"], "column");
        let r = rects(&l, 0);
        assert_eq!(r.len(), 12, "three regions");
        // north 30, south 23, east 7: north the tallest
        assert!(r[3] > r[7] && r[7] > r[11]);
        assert_eq!(l["axes"][0]["ticks"].as_array().unwrap().iter().map(|t| t["text"].as_str().unwrap()).collect::<Vec<_>>(), ["north", "south", "east"]);
        let s = layout(&json!({"mark": "column", "x": "region", "y": ["amount"], "color": "product", "stack": true}), &t, (600.0, 400.0)).unwrap();
        assert_eq!(s["kind"], "stacked column");
        assert_eq!(s["marks"].as_array().unwrap().len(), 2, "a mark a product");
        assert_eq!(s["legend"].as_array().unwrap().len(), 2);
        let c = layout(&json!({"mark": "column", "x": "region"}), &t, (600.0, 400.0)).unwrap();
        assert_eq!(c["axes"][1]["title"], "count", "no y: the rows counted");
    }

    #[test]
    fn a_histogram_bins_a_numeric_column() {
        let l = layout(&json!({"mark": "histogram", "x": "amount", "bins": 4}), &table(), (600.0, 400.0)).unwrap();
        assert_eq!(l["kind"], "histogram");
        assert_eq!(rects(&l, 0).len() % 4, 0);
        let fallback = layout(&json!({"mark": "histogram", "x": "region"}), &table(), (600.0, 400.0)).unwrap();
        assert_eq!(fallback["axes"][0]["title"], "month", "x not numbers: the first column that is");
    }

    #[test]
    fn lines_and_stacked_lines() {
        let t = table();
        let l = layout(&json!({"mark": "line", "x": "month", "y": ["amount"], "color": "product"}), &t, (600.0, 400.0)).unwrap();
        assert_eq!(l["kind"], "line");
        assert_eq!(l["marks"].as_array().unwrap().len(), 2, "a line a product");
        assert_ne!(l["marks"][0]["color"], l["marks"][1]["color"]);
        let s = layout(&json!({"mark": "line", "x": "month", "y": ["amount"], "color": "product", "stack": true}), &t, (600.0, 400.0)).unwrap();
        assert_eq!(s["kind"], "stacked line");
        assert_eq!(s["marks"][0]["kind"], "area");
        // several rows of one month are the month's total: a's months 1, 2 (east), 3 → 30 at month 1
        let a = &l["marks"][0]["xy"];
        assert_eq!(a.as_array().unwrap().len(), 6, "three months, one point each");
        assert_eq!(aggregate(&[(1.0, 2.0), (1.0, 3.0), (2.0, 5.0)], "sum"), vec![(1.0, 5.0), (2.0, 5.0)]);
        assert_eq!(aggregate(&[(1.0, 2.0), (1.0, 4.0)], "mean"), vec![(1.0, 3.0)]);
        assert!(layout(&json!({"mark": "line", "y": ["region"]}), &t, (600.0, 400.0)).is_err(), "y must be numbers");
    }

    #[test]
    fn a_treemap_is_squarified_and_fills_its_area() {
        let r = squarify(&[6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0], (0.0, 0.0, 600.0, 400.0));
        let area: f64 = r.iter().map(|q| q.2 * q.3).sum();
        assert!((area - 240_000.0).abs() < 1e-6, "{area}");
        // each rectangle's share of the area is its value's share
        assert!(((r[0].2 * r[0].3) / 240_000.0 - 6.0 / 24.0).abs() < 1e-9);
        assert!(r.iter().all(|q| q.2 >= 0.0 && q.3 >= 0.0));
        let l = layout(&json!({"mark": "treemap", "path": ["region", "product"], "size": "amount"}), &table(), (600.0, 400.0)).unwrap();
        assert_eq!(l["kind"], "treemap");
        assert!(l["a11y"]["summary"].as_str().unwrap().starts_with("3 groups of amount by region / product, 60 in all"));
        assert!(squarify(&[0.0, -1.0], (0.0, 0.0, 10.0, 10.0)).iter().all(|q| q.2 == 0.0), "nothing to place");
    }

    #[test]
    fn a_table_with_no_spec_gets_one() {
        assert_eq!(auto_spec(&table())["mark"], "column");
        assert_eq!(auto_spec(&table())["x"], "region");
        assert!(layout(&json!({"mark": "pie"}), &table(), (600.0, 400.0)).is_err());
    }
}
