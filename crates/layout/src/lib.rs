//! Tidy: a layered layout for the node canvas. Inputs flow left to right, then top to bottom;
//! nothing is further apart than the gaps say; parameters sit in a lane just left of the pins
//! they feed. Lifted from the research software's `research-layout`; the research behind it is the research software's
//! `docs/research/node_layout_20261023.md`.
//!
//! The phases (the Sugiyama framework, as Graphviz `dot` and ELK Layered use it):
//!
//! 1. **Classify.** A *parameter* (`Node::param`: a source that only sends settings, such as a
//!    Vary node; the caller decides) leaves the ranking; everything else is the *spine*.
//! 2. **Rank** the spine into columns by network simplex ([`rank`]): minimise Σ weight × wire
//!    length, so a node sits just before what uses it, not as early as possible. A wire from an
//!    output with many uses pulls less (weight ÷ √fan-out): it is long wherever they go.
//! 3. **Order** each column ([`order`]): barycenter sweeps on pin positions (a wire leaves from
//!    its pin, not the node's middle), ties kept in the current top-to-bottom order so a re-tidy
//!    keeps the designer's map, then adjacent swaps while they remove crossings.
//! 4. **Place** ([`place`]): each column as wide as its widest node; each node packed to the
//!    top and lined up pin to pin with its heaviest input. Parameters go in a lane on the left
//!    of the column of their first use, level with the pin they feed.
//! 5. **Wrap** ([`wrap`]) a drawing far wider than tall into rows, cut between the columns that
//!    the fewest wires cross.
//!
//! **Net labels.** A wire drawn as a net label ([`Edge::labelled`]) is a tag at each end, not a
//! line: it still says which column comes first, but pulls little in ranking and takes no part
//! in crossing reduction, wrap cuts or lining nodes up. Tags take room beside their pins
//! ([`Node::margin`]): a column is as wide as its nodes with their tags, and a lane sits left
//! of the tags at its column's inputs.
//!
//! Pure and total: no workspace dependencies, and hostile input (non-finite or huge sizes, bad
//! indices, cycles) is clamped or dropped, never a panic.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod order;
mod place;
mod rank;
mod wrap;

/// The largest node side and gap taken, in graph units.
pub const MAX_SIZE: f32 = 20_000.0;
/// A node's size when the one given isn't usable.
pub const DEFAULT_SIZE: [f32; 2] = [200.0, 120.0];
/// How hard a wire drawn as a net label pulls in ranking, against a drawn one: it is no
/// longer for being long.
pub const LABEL_PULL: f64 = 0.1;

/// A node to lay out.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Width and height, graph units.
    pub size: [f32; 2],
    /// Each input pin's y, from the node's top.
    pub inputs: Vec<f32>,
    /// Each output pin's y, from the node's top.
    pub outputs: Vec<f32>,
    /// A parameter: placed in the lane beside the column it feeds, not ranked.
    pub param: bool,
    /// Where it is now: seeds the order within a column.
    pub pos: [f32; 2],
    /// Room kept clear on its left and right (graph units): net labels' tags at its input and
    /// output pins.
    pub margin: [f32; 2],
}

/// A wire, from output pin `out` of node `from` to input pin `inp` of node `to`.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub from: usize,
    pub out: usize,
    pub to: usize,
    pub inp: usize,
    /// How hard the wire pulls: kept short in ranking, and the heaviest input is lined up.
    pub weight: f32,
    /// Drawn as a net label, not a line (module docs).
    pub labelled: bool,
}

/// Gaps and the wrapping rule, graph units.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Between nodes stacked in a column (or a lane).
    pub node_gap: f32,
    /// Between a column and the next column's lane.
    pub column_gap: f32,
    /// Between a lane and its column.
    pub lane_gap: f32,
    /// Between wrapped rows.
    pub row_gap: f32,
    /// Wrap when width ÷ height is more than this.
    pub max_aspect: f32,
    /// Never wrap fewer columns than this.
    pub min_wrap_columns: usize,
    /// Only cut where at most this many wires cross (each runs back to the next row's start).
    pub max_cut_wires: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { node_gap: 24.0, column_gap: 48.0, lane_gap: 16.0, row_gap: 96.0, max_aspect: 3.0, min_wrap_columns: 8, max_cut_wires: 3 }
    }
}

/// Where everything goes.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Each node's top-left, graph units, the whole starting at (0, 0).
    pub pos: Vec<[f32; 2]>,
    /// Each node's column (a parameter: the column whose lane it is in).
    pub column: Vec<usize>,
    /// How many rows the columns were wrapped into.
    pub rows: usize,
}

fn clean(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { fallback }
}

/// A node as the phases use it: sizes and pins clamped to something drawable.
#[derive(Debug, Clone)]
pub(crate) struct N {
    pub w: f32,
    pub h: f32,
    pub inputs: Vec<f32>,
    pub outputs: Vec<f32>,
    pub param: bool,
    pub seed: [f32; 2],
    /// Tag room on the left and right.
    pub ml: f32,
    pub mr: f32,
}

impl N {
    fn new(n: &Node) -> Self {
        let w = clean(n.size[0], 1.0, MAX_SIZE, DEFAULT_SIZE[0]);
        let h = clean(n.size[1], 1.0, MAX_SIZE, DEFAULT_SIZE[1]);
        let pins = |ps: &[f32]| ps.iter().map(|&y| clean(y, 0.0, h, h / 2.0)).collect();
        N {
            w,
            h,
            inputs: pins(&n.inputs),
            outputs: pins(&n.outputs),
            param: n.param,
            seed: [clean(n.pos[0], -1e9, 1e9, 0.0), clean(n.pos[1], -1e9, 1e9, 0.0)],
            ml: clean(n.margin[0], 0.0, MAX_SIZE, 0.0),
            mr: clean(n.margin[1], 0.0, MAX_SIZE, 0.0),
        }
    }

    pub fn in_pin(&self, i: usize) -> f32 {
        self.inputs.get(i).copied().unwrap_or(self.h / 2.0)
    }

    pub fn out_pin(&self, i: usize) -> f32 {
        self.outputs.get(i).copied().unwrap_or(self.h / 2.0)
    }
}

/// A wire whose ends are in range.
#[derive(Debug, Clone, Copy)]
pub(crate) struct E {
    pub from: usize,
    pub out: usize,
    pub to: usize,
    pub inp: usize,
    pub w: f64,
    pub labelled: bool,
}

/// Lay `nodes` out. Edges naming a node out of range, or a node to itself, are ignored.
pub fn layout(nodes: &[Node], edges: &[Edge], opts: &Options) -> Layout {
    let ns: Vec<N> = nodes.iter().map(N::new).collect();
    let n = ns.len();
    let es: Vec<E> = edges
        .iter()
        .filter(|e| e.from < n && e.to < n && e.from != e.to)
        .map(|e| E { from: e.from, out: e.out, to: e.to, inp: e.inp, w: f64::from(clean(e.weight, 0.01, 1000.0, 1.0)), labelled: e.labelled })
        .collect();
    let opts = Options {
        node_gap: clean(opts.node_gap, 0.0, MAX_SIZE, 24.0),
        column_gap: clean(opts.column_gap, 0.0, MAX_SIZE, 48.0),
        lane_gap: clean(opts.lane_gap, 0.0, MAX_SIZE, 16.0),
        row_gap: clean(opts.row_gap, 0.0, MAX_SIZE, 96.0),
        max_aspect: clean(opts.max_aspect, 0.1, 1000.0, 3.0),
        min_wrap_columns: opts.min_wrap_columns.max(2),
        max_cut_wires: opts.max_cut_wires,
    };

    // 1. a parameter is a source feeding the spine; anything else marked one is spine after all
    let source: Vec<bool> = (0..n).map(|i| ns[i].param && !es.iter().any(|e| e.to == i)).collect();
    let param: Vec<bool> = (0..n).map(|i| source[i] && es.iter().any(|e| e.from == i && !source[e.to])).collect();
    let spine: Vec<usize> = (0..n).filter(|&i| !param[i]).collect();
    let mut local = vec![usize::MAX; n];
    for (k, &i) in spine.iter().enumerate() {
        local[i] = k;
    }
    let spine_edges =
        rank::acyclic(spine.len(), es.iter().filter(|e| !param[e.from] && !param[e.to]).map(|e| E { from: local[e.from], to: local[e.to], ..*e }).collect());

    // 2. columns. An output feeding many nodes is a bus: its wires are long wherever its uses
    // go, so each pulls less (by √fan-out) and a node it feeds can sit beside its own use.
    let fan = |e: &E| spine_edges.iter().filter(|f| f.from == e.from && f.out == e.out).count().max(1) as f64;
    // A wire drawn as a net label pulls less again.
    let rank_edges: Vec<E> = spine_edges.iter().map(|e| E { w: e.w / fan(e).sqrt() * if e.labelled { LABEL_PULL } else { 1.0 }, ..*e }).collect();
    let ranks = rank::rank(spine.len(), &rank_edges);
    // 3. order within columns, by the wires drawn as lines
    let drawn: Vec<E> = spine_edges.iter().filter(|e| !e.labelled).copied().collect();
    let spine_nodes: Vec<N> = spine.iter().map(|&i| ns[i].clone()).collect();
    let columns = order::order(&spine_nodes, &ranks, &drawn);
    // 4 and 5. place, parameters in their lanes, wrapped into rows
    let placed = place::place(&ns, &param, &spine, &ranks, &columns, &drawn, &es, &opts);
    let rows = placed.rows;

    let min_x = placed.pos.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let min_y = placed.pos.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let (dx, dy) = if min_x.is_finite() && min_y.is_finite() { (min_x, min_y) } else { (0.0, 0.0) };
    Layout { pos: placed.pos.iter().map(|p| [p[0] - dx, p[1] - dy]).collect(), column: placed.column, rows }
}

#[cfg(test)]
mod tests;
