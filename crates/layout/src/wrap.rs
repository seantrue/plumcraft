//! Phase 5: where to wrap a drawing far wider than tall into rows, one under another.
//!
//! The maintainer (2026-10-23): "Wrap, look for places where it's a minimal number of edges to
//! wrap." Each cut goes between two columns, chosen near where an even split would fall (within
//! half a row's width), at the boundary the fewest wires cross; ties go to the nearest. Every
//! wire that crosses a cut runs back from the end of one row to the start of the next, so a cut
//! that more than [`Options::max_cut_wires`] cross isn't made, and short graphs (fewer than
//! [`Options::min_wrap_columns`]) aren't wrapped at all. The cuts are chosen before anything is
//! placed vertically, so each row is then packed on its own ([`crate::place`]).

use crate::Options;

/// Where rows start: column indices, ascending, none at 0. Empty: one row.
///
/// `start[c]` and `end[c]` are column `c`'s extent; `height[c]` the height of its stack;
/// `crossing[c]` how many wires a cut just before column `c` would cross.
pub(crate) fn cuts(start: &[f32], end: &[f32], height: &[f32], crossing: &[usize], opts: &Options) -> Vec<usize> {
    let cols = start.len();
    if cols < opts.min_wrap_columns {
        return Vec::new();
    }
    let right = end.iter().copied().fold(0.0, f32::max);
    let tall = height.iter().copied().fold(0.0, f32::max);
    if tall <= 0.0 || right / tall <= opts.max_aspect {
        return Vec::new();
    }
    // k rows of width right/k and height ~tall: aspect right / (k² tall)
    let k = ((right / (tall * opts.max_aspect)).sqrt().ceil() as usize).clamp(2, cols);
    let half = right / k as f32 / 2.0;
    let mut out: Vec<usize> = Vec::new();
    for i in 1..k {
        let ideal = right * i as f32 / k as f32;
        let after = out.last().copied().unwrap_or(0);
        let near = |b: &usize| (start[*b] - ideal).abs();
        let cost = |b: &usize| crossing.get(*b).copied().unwrap_or(usize::MAX);
        let pick = (after + 1..cols)
            .filter(|b| near(b) <= half && cost(b) <= opts.max_cut_wires)
            .min_by(|a, b| cost(a).cmp(&cost(b)).then(near(a).total_cmp(&near(b))));
        if let Some(b) = pick {
            out.push(b);
        }
    }
    out
}
