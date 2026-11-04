//! Connected Components and Region Outlines (yapocis_external `label`, written plainly: a
//! union-find gives the labels its propagation converges to), Grid Segments, and Segment Average
//! (yapocis_external `averagesegments` / `applysegments`).

use plumcraft_engine::{Cx, Inputs, Outputs};
use plumcraft_proto::error::NodeError;

use super::{as_channel, size_of, table};
use crate::img::Img;
use crate::op::{config, f, img, n, one, op_error, s};

// ---------- Connected Components ----------

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while let Some(&p) = parent.get(i) {
        if p == i {
            break;
        }
        let gp = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = gp;
        }
        i = gp;
    }
    i
}

/// Join two sets under the smaller root, so a set's root is its smallest index.
fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
    if let Some(slot) = parent.get_mut(hi) {
        *slot = lo;
    }
}

/// The labels yapocis_external's propagation converges to, renumbered 0, 1, 2, ... as the research software
/// does (`np.unique`): every pixel starts as its flat index; a pixel not on an edge spreads its
/// label to its 4-neighbours; each takes the smallest it sees. So an area between edges is
/// labelled by its first pixel in scan order, and an edge pixel by the smallest of itself and the
/// areas beside it.
pub fn connected_components_of(edges: &[f32], h: usize, w: usize) -> Vec<f32> {
    let count = h * w;
    let edge: Vec<bool> = edges.iter().map(|v| *v > 0.5).collect();
    let is_edge = |k: usize| edge.get(k).copied().unwrap_or(true);
    let mut parent: Vec<usize> = (0..count).collect();
    for y in 0..h {
        for x in 0..w {
            let k = y * w + x;
            if is_edge(k) {
                continue;
            }
            if x + 1 < w && !is_edge(k + 1) {
                union(&mut parent, k, k + 1);
            }
            if y + 1 < h && !is_edge(k + w) {
                union(&mut parent, k, k + w);
            }
        }
    }
    let mut labels = vec![0usize; count];
    for k in 0..count {
        if !is_edge(k) {
            let root = find(&mut parent, k);
            if let Some(slot) = labels.get_mut(k) {
                *slot = root;
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let k = y * w + x;
            if !is_edge(k) {
                continue;
            }
            let mut best = k;
            let neighbours = [(x > 0).then(|| k - 1), (x + 1 < w).then(|| k + 1), (y > 0).then(|| k - w), (y + 1 < h).then(|| k + w)];
            for j in neighbours.into_iter().flatten() {
                if !is_edge(j)
                    && let Some(&l) = labels.get(j)
                {
                    best = best.min(l);
                }
            }
            if let Some(slot) = labels.get_mut(k) {
                *slot = best;
            }
        }
    }
    // renumber in order of value (np.unique's inverse, npexact)
    let labels: Vec<i64> = labels.iter().map(|&l| l as i64).collect();
    let (_, inverse) = npexact::order::unique_inverse_i64(&labels);
    inverse.into_iter().map(|r| r as f32).collect()
}

pub(super) fn connected_components(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let edges = img(i, "edges")?;
    let p = as_channel(&edges, "edges")?;
    let labels = connected_components_of(&p, edges.h, edges.w);
    Ok(one("labels", Img { labels: true, names: vec!["label".into()], ..Img::plane_of(edges.h, edges.w, labels) }))
}

// ---------- Region Outlines ----------

/// 1 where a pixel's label differs from one of its 4-neighbours' (yapocis_external `hollow`).
/// Labels are rounded half to even first, as `np.rint` does.
pub fn outlines_of(labels: &[f32], h: usize, w: usize) -> Vec<f32> {
    let lab: Vec<i64> = labels.iter().map(|v| v.round_ties_even() as i64).collect();
    let at = |k: usize| lab.get(k).copied().unwrap_or(0);
    let mut out = vec![0.0f32; h * w];
    for y in 0..h {
        for x in 0..w {
            let k = y * w + x;
            let s = at(k);
            let external = (x > 0 && at(k - 1) != s) || (x + 1 < w && at(k + 1) != s) || (y > 0 && at(k - w) != s) || (y + 1 < h && at(k + w) != s);
            if let Some(o) = out.get_mut(k) {
                *o = if external { 1.0 } else { 0.0 };
            }
        }
    }
    out
}

pub(super) fn region_outlines(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let labels = img(i, "labels")?;
    if labels.table || labels.c != 1 {
        return Err(config(format!("labels must be one channel, got {}", labels.describe())));
    }
    let p = as_channel(&labels, "labels")?;
    Ok(one("outlines", Img::plane_of(labels.h, labels.w, outlines_of(&p, labels.h, labels.w))))
}

// ---------- Grid Segments ----------

/// Rectangles of `tw` × `th` tiling an `h` × `w` array, as rows of n, x1, y1, x2, y2.
pub fn grid_segments_of(h: usize, w: usize, tw: usize, th: usize) -> Vec<f32> {
    let (tw, th) = (tw.max(1), th.max(1));
    let mut rows = Vec::new();
    for y in (0..h).step_by(th) {
        for x in (0..w).step_by(tw) {
            let (x2, y2) = ((x + tw).min(w) - 1, (y + th).min(h) - 1);
            rows.extend([((x2 - x + 1) * (y2 - y + 1)) as f32, x as f32, y as f32, x2 as f32, y2 as f32]);
        }
    }
    rows
}

pub(super) fn grid_segments(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let (h, w) = size_of(&img(i, "like")?);
    Img::check_size(h, w, 1).map_err(config)?;
    let tw = n(i, "tile_width", 16).clamp(1, 4096) as usize;
    let th = n(i, "tile_height", 16).clamp(1, 4096) as usize;
    let names = ["n", "x1", "y1", "x2", "y2"].iter().map(|s| (*s).to_owned()).collect();
    Ok(one("segments", table(grid_segments_of(h, w, tw, th), names)))
}

// ---------- Segment Average ----------

/// Points a segment table may visit in all (rectangles clamped to the array still count every
/// point of the rectangle, as the kernels do).
const MAX_VISITS: u128 = 1 << 28;

/// `HALF * (a + b)`, or the circular midpoint with period `maxval` (yapocis_external
/// `_pair_mean`), in float32.
fn pair_mean(a: f32, b: f32, maxval: f32) -> f32 {
    let mut m = 0.5f32 * (a + b);
    if maxval > 0.0 && (a - b).abs() > 0.5 * maxval {
        m += 0.5 * maxval;
        if m >= maxval {
            m -= maxval;
        }
    }
    m
}

/// Each segment's rectangle replaced by its mean (clamped to [0, 1 - eps)) or median (the upper
/// middle value), each two-point segment by the pair's midpoint, each one-point segment left
/// alone; coordinates are clamped to the array (yapocis_external `averagesegments` and
/// `applysegments`, run segment by segment in order). `segments` holds rows of n, x1, y1, x2,
/// y2, already truncated to integers.
pub fn segment_average_of(img: &[f32], h: usize, w: usize, segments: &[[i64; 5]], median: bool, maxval: f32) -> Result<Vec<f32>, String> {
    let visits: u128 =
        segments.iter().map(|[n, x1, y1, x2, y2]| if *n == 1 || *n == 2 { 2 } else { ((x2 - x1 + 1).max(0) as u128) * ((y2 - y1 + 1).max(0) as u128) }).sum();
    if visits > MAX_VISITS {
        return Err(format!("the segments cover {visits} points; at most {MAX_VISITS}"));
    }
    if h == 0 || w == 0 {
        return Ok(img.to_vec());
    }
    let address = |x: i64, y: i64| (y.clamp(0, h as i64 - 1) as usize) * w + x.clamp(0, w as i64 - 1) as usize;
    let get = |k: usize| img.get(k).copied().unwrap_or(0.0);
    let mut out = img.to_vec();
    let set = |out: &mut Vec<f32>, k: usize, v: f32| {
        if let Some(o) = out.get_mut(k) {
            *o = v;
        }
    };
    for &[n, x1, y1, x2, y2] in segments {
        if n == 1 {
            let a = address(x1, y1);
            set(&mut out, a, get(a));
        } else if n == 2 {
            let (a, b) = (address(x1, y1), address(x2, y2));
            let m = pair_mean(get(a), get(b), maxval);
            set(&mut out, a, m);
            set(&mut out, b, m);
        } else {
            let m = if median {
                let mut buf = Vec::new();
                for x in x1..=x2 {
                    for y in y1..=y2 {
                        buf.push(get(address(x, y)));
                    }
                }
                if buf.is_empty() {
                    continue;
                }
                // np.sort (npexact): NaN last
                let buf = npexact::order::sort_f32(&buf);
                let mid = buf.len() / 2;
                buf.get(mid).copied().unwrap_or(0.0)
            } else {
                let (mut total, mut count) = (0.0f32, 0u64);
                for x in x1..=x2 {
                    for y in y1..=y2 {
                        total += get(address(x, y));
                        count += 1;
                    }
                }
                if count == 0 {
                    continue;
                }
                let m = total / count as f32;
                let m = if m < f32::EPSILON { 0.0 } else { m };
                if m > 1.0 - f32::EPSILON { 1.0 - f32::EPSILON } else { m }
            };
            for x in x1..=x2 {
                for y in y1..=y2 {
                    set(&mut out, address(x, y), m);
                }
            }
        }
    }
    Ok(out)
}

pub(super) fn segment_average(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    if a.table {
        return Err(config("array is a table, not a picture"));
    }
    let segs = img(i, "segments")?;
    if !segs.table {
        return Err(config("segments must be a table (Grid Segments)"));
    }
    if segs.c != 5 {
        return Err(config(format!("segments must have shape (k, 5): [n, x1, y1, x2, y2] per row, got {}", segs.describe())));
    }
    let rows: Vec<[i64; 5]> = segs.data.as_chunks::<5>().0.iter().map(|r| r.map(|v| i64::from(v as i32))).collect();
    let median = match s(i, "statistic", "mean").as_str() {
        "mean" => false,
        "median" => true,
        other => return Err(config(format!("statistic: {other:?} is not mean or median"))),
    };
    let period = f(i, "period", 0.0).clamp(0.0, 1e6) as f32;
    let out = a
        .map_planes(|p, h, w| {
            let clipped: Vec<f32> = p.iter().map(|v| if v.is_nan() { *v } else { v.clamp(0.0, 1.0) }).collect();
            segment_average_of(&clipped, h, w, &rows, median, period)
        })
        .map_err(op_error)?;
    Ok(one("averaged", out))
}
