//! Phases 4 and 5: coordinates, and wrapping into rows.
//!
//! **x:** each column is as wide as its widest node, with a lane on its left as wide as its
//! widest parameter (no lane, no space). Nodes are left-aligned in their column, so their input
//! pins line up; parameters are right-aligned in their lane, next to the pins they feed. Net
//! labels' tags count: a column keeps room on its left for the widest tag at its nodes' inputs
//! (between the lane and the nodes) and on its right for the widest at their outputs.
//!
//! **Rows:** with the columns' widths and stacked heights known, [`crate::wrap`] says where a
//! too-wide drawing breaks into rows. Each row is then placed on its own, and moved to the left
//! edge, under the row before.
//!
//! **y:** packed to the top. Column by column, left to right, each node in its order goes level
//! with the top of its heaviest input's node (in its row), or as high as the node above it
//! allows. A node with no inputs in its row lines up with its heaviest use instead, as a first
//! pass placed it, but only when it is the last in its column: anywhere else, moving down would
//! push the nodes under it, and a use that follows one of those would pull it down again (the
//! drift the research software's Mosaic style showed). A parameter goes level with the pin it feeds (the median, if it feeds
//! several in its column), stacked in its lane in that order. Each row starts at its own top.
//!
//! Tops, not pins: a node's inputs are at its top and its outputs at its bottom (in the research software and
//! PlumCraft alike), so lining pins up would sink a chain by most of a node at every step. This is the top-left one of
//! Brandes and Köpf's four alignments, not their balanced average: the maintainer asked for top
//! to bottom with no excess space, and the average drifts down.

use crate::{E, N, Options};

/// Left-to-right passes: the first packs nodes without inputs to the top, the second lines them
/// up with their uses as the first placed them.
const PASSES: usize = 2;

/// Where everything goes.
#[derive(Debug, Clone)]
pub(crate) struct Placed {
    pub pos: Vec<[f32; 2]>,
    /// Each node's column; a parameter's is the column whose lane it is in.
    pub column: Vec<usize>,
    pub rows: usize,
}

/// The heaviest edge in `cands` (then the lowest pin, then the lowest node).
fn heaviest<'a>(cands: impl Iterator<Item = &'a E>, pin: impl Fn(&E) -> (usize, usize)) -> Option<&'a E> {
    cands.min_by(|a, b| b.w.total_cmp(&a.w).then(pin(a).cmp(&pin(b))))
}

/// The height of nodes stacked with gaps.
fn stack(hs: impl Iterator<Item = f32>, gap: f32) -> f32 {
    let (sum, n) = hs.fold((0.0, 0usize), |(s, n), h| (s + h, n + 1));
    sum + gap * n.saturating_sub(1) as f32
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn place(
    nodes: &[N],
    param: &[bool],
    spine: &[usize],
    ranks: &[i64],
    columns: &[Vec<usize>],
    spine_edges: &[E],
    all: &[E],
    opts: &Options,
) -> Placed {
    let n = nodes.len();
    let cols = columns.len().max(1);
    let mut column = vec![0usize; n];
    for (k, &i) in spine.iter().enumerate() {
        column[i] = usize::try_from(ranks[k]).unwrap_or(0);
    }
    // each parameter's lane: the column of its first use
    let mut lanes: Vec<Vec<usize>> = vec![Vec::new(); cols];
    for p in (0..n).filter(|&p| param[p]) {
        let first = all.iter().filter(|e| e.from == p && !param[e.to]).map(|e| column[e.to]).min().unwrap_or(0);
        column[p] = first;
        if let Some(lane) = lanes.get_mut(first) {
            lane.push(p);
        }
    }

    // x, before wrapping
    let mut start = vec![0.0f32; cols];
    let mut end = vec![0.0f32; cols];
    let mut node_x = vec![0.0f32; cols];
    let mut lane_w = vec![0.0f32; cols];
    let mut height = vec![0.0f32; cols];
    let mut x = 0.0f32;
    for c in 0..cols {
        let col: &[usize] = columns.get(c).map_or(&[], Vec::as_slice);
        let col_w = col.iter().map(|&k| nodes[spine[k]].w + nodes[spine[k]].mr).fold(0.0, f32::max);
        let col_ml = col.iter().map(|&k| nodes[spine[k]].ml).fold(0.0, f32::max);
        lane_w[c] = lanes[c].iter().map(|&p| nodes[p].w + nodes[p].mr).fold(0.0, f32::max);
        start[c] = x;
        node_x[c] = x + if lane_w[c] > 0.0 { lane_w[c] + opts.lane_gap } else { 0.0 } + col_ml;
        end[c] = node_x[c] + col_w;
        x = end[c] + opts.column_gap;
        height[c] = stack(col.iter().map(|&k| nodes[spine[k]].h), opts.node_gap).max(stack(lanes[c].iter().map(|&p| nodes[p].h), opts.node_gap));
    }

    // rows: a cut costs the wires drawn across it (a net label's are tags at each end)
    let mut crossing = vec![0usize; cols];
    for e in all.iter().filter(|e| !e.labelled) {
        let (a, b) = (column[e.from].min(column[e.to]), column[e.from].max(column[e.to]));
        for c in crossing.iter_mut().take(b + 1).skip(a + 1) {
            *c += 1;
        }
    }
    let cuts = crate::wrap::cuts(&start, &end, &height, &crossing, opts);
    let row_of = |c: usize| cuts.iter().filter(|&&b| b <= c).count();

    // y of the spine (spine-local indices), each row on its own
    let m = spine.len();
    let row_k = |k: usize| row_of(usize::try_from(ranks[k]).unwrap_or(0));
    let mut y = vec![0.0f32; m];
    for pass in 0..PASSES {
        for col in columns {
            let mut bottom: Option<f32> = None;
            for (at, &v) in col.iter().enumerate() {
                let nv = &nodes[spine[v]];
                let pred = heaviest(spine_edges.iter().filter(|e| e.to == v && ranks[e.from] < ranks[v] && row_k(e.from) == row_k(v)), |e| (e.inp, e.from));
                let last = at + 1 == col.len();
                let want = if let Some(e) = pred {
                    y[e.from]
                } else if pass > 0
                    && last
                    && let Some(e) =
                        heaviest(spine_edges.iter().filter(|e| e.from == v && ranks[e.to] > ranks[v] && row_k(e.to) == row_k(v)), |e| (e.out, e.to))
                {
                    y[e.to]
                } else {
                    0.0
                };
                let top = bottom.map_or(0.0, |b| b + opts.node_gap);
                y[v] = want.max(top).max(0.0);
                bottom = Some(y[v] + nv.h);
            }
        }
    }

    let mut pos = vec![[0.0f32; 2]; n];
    let mut local = vec![usize::MAX; n];
    for (k, &i) in spine.iter().enumerate() {
        local[i] = k;
        pos[i] = [node_x[column[i]], y[k]];
    }

    // parameters, level with what they feed, stacked in their lane
    for (c, lane) in lanes.iter().enumerate() {
        let mut want: Vec<(usize, f32)> = lane
            .iter()
            .map(|&p| {
                let mut ys: Vec<f32> = all
                    .iter()
                    .filter(|e| e.from == p && !param[e.to] && column[e.to] == c)
                    .map(|e| y[local[e.to]] + nodes[e.to].in_pin(e.inp) - nodes[p].out_pin(e.out))
                    .collect();
                ys.sort_by(f32::total_cmp);
                (p, ys.get(ys.len() / 2).copied().unwrap_or(0.0))
            })
            .collect();
        want.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let mut bottom: Option<f32> = None;
        for (p, w) in want {
            let top = bottom.map_or(0.0, |b| b + opts.node_gap);
            let py = w.max(top).max(0.0);
            pos[p] = [start[c] + lane_w[c] - nodes[p].w - nodes[p].mr, py];
            bottom = Some(py + nodes[p].h);
        }
    }

    // each row to the left edge, under the one before
    let mut bounds = vec![0];
    bounds.extend(&cuts);
    bounds.push(cols);
    let mut top = 0.0f32;
    for w in bounds.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        let members: Vec<usize> = (0..n).filter(|&i| (lo..hi).contains(&column[i])).collect();
        let row_top = members.iter().map(|&i| pos[i][1]).fold(f32::INFINITY, f32::min);
        let row_bottom = members.iter().map(|&i| pos[i][1] + nodes[i].h).fold(f32::NEG_INFINITY, f32::max);
        if !row_top.is_finite() {
            continue;
        }
        let dx = start.get(lo).copied().unwrap_or(0.0);
        for &i in &members {
            pos[i] = [pos[i][0] - dx, pos[i][1] - row_top + top];
        }
        top += row_bottom - row_top + opts.row_gap;
    }
    Placed { pos, column, rows: cuts.len() + 1 }
}
