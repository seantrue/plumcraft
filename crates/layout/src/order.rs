//! Phase 3: the order of nodes within each column, to cross as few wires as possible.
//!
//! A wire longer than one column is threaded through a dummy node in each column it passes, so
//! it takes part (as Sugiyama layouts do). Each end of a wire sits at its pin: the node's place
//! in the column plus the pin's height as a fraction of the node's, so two wires into one node
//! cross or not as they would on the canvas (ELK's fixed port order).
//!
//! **Stable.** The result depends only on the current top-to-bottom order of each column (the
//! seed), never on coordinates: dummies start on the straight line between their wire's ends.
//! From the seed, sweep down and up by barycenter, then swap neighbours while that removes
//! crossings; take the new order only if it crosses strictly fewer wires than the seed, and
//! repeat from it. So the designer's order is kept when it is as good, and tidying a tidy graph
//! changes nothing.

use crate::{E, N};

/// Barycenter sweeps (down and up alternate).
const SWEEPS: usize = 24;
/// Transpose passes at most.
const TRANSPOSES: usize = 32;
/// Improve-and-retry rounds at most.
const ROUNDS: usize = 8;

/// A wire's dummy in one column: on the wire from `from` to `to` (spine nodes), `t` of the way.
#[derive(Debug, Clone, Copy)]
struct Dummy {
    from: usize,
    to: usize,
    t: f64,
}

/// A wire between adjacent columns: from item `a` (at pin fraction `fa`) to item `b` (`fb`).
#[derive(Debug, Clone, Copy)]
struct Link {
    a: usize,
    fa: f64,
    b: usize,
    fb: f64,
}

struct Layered {
    /// Items `0..real` are the spine nodes; the rest are dummies, `dummies[item - real]`.
    real: usize,
    dummies: Vec<Dummy>,
    column: Vec<usize>,
    /// `links[c]`: the wires between column `c` and `c + 1`.
    links: Vec<Vec<Link>>,
    cols: usize,
}

fn frac(y: f32, h: f32) -> f64 {
    f64::from((y / h).clamp(0.0, 0.999))
}

/// Each column's spine nodes, top to bottom. `ranks` gives each node's column.
pub(crate) fn order(nodes: &[N], ranks: &[i64], edges: &[E]) -> Vec<Vec<usize>> {
    let g = layered(nodes, ranks, edges);
    // the seed: each column in its current top-to-bottom order
    let mut seed: Vec<Vec<usize>> = vec![Vec::new(); g.cols];
    for v in 0..g.real {
        seed[g.column[v]].push(v);
    }
    for col in &mut seed {
        col.sort_by(|&a, &b| {
            let (sa, sb) = (nodes[a].seed, nodes[b].seed);
            sa[1].total_cmp(&sb[1]).then(sa[0].total_cmp(&sb[0])).then(a.cmp(&b))
        });
    }
    let mut cost = crossings_of(&g, &initial(&g, &seed));
    for _ in 0..ROUNDS {
        let better = improve(&g, initial(&g, &seed));
        let real: Vec<Vec<usize>> = better.into_iter().map(|col| col.into_iter().filter(|&v| v < g.real).collect()).collect();
        let c = crossings_of(&g, &initial(&g, &real));
        if c >= cost {
            break;
        }
        cost = c;
        seed = real;
    }
    seed
}

fn layered(nodes: &[N], ranks: &[i64], edges: &[E]) -> Layered {
    let cols = ranks.iter().copied().max().map_or(0, |m| usize::try_from(m).unwrap_or(0) + 1);
    let col = |v: usize| usize::try_from(ranks[v]).unwrap_or(0);
    let real = nodes.len();
    let mut column: Vec<usize> = (0..real).map(col).collect();
    let mut dummies = Vec::new();
    let mut links: Vec<Vec<Link>> = vec![Vec::new(); cols.saturating_sub(1)];
    for e in edges {
        let (cu, cv) = (col(e.from), col(e.to));
        if cv <= cu {
            continue;
        }
        let (nu, nv) = (&nodes[e.from], &nodes[e.to]);
        let mut prev = e.from;
        let mut fprev = frac(nu.out_pin(e.out), nu.h);
        for c in cu + 1..cv {
            dummies.push(Dummy { from: e.from, to: e.to, t: (c - cu) as f64 / (cv - cu) as f64 });
            column.push(c);
            let d = real + dummies.len() - 1;
            links[c - 1].push(Link { a: prev, fa: fprev, b: d, fb: 0.5 });
            prev = d;
            fprev = 0.5;
        }
        links[cv - 1].push(Link { a: prev, fa: fprev, b: e.to, fb: frac(nv.in_pin(e.inp), nv.h) });
    }
    Layered { real, dummies, column, links, cols }
}

/// The columns with `real`'s order and each dummy on the straight line between its wire's ends
/// (by their place in their columns, as fractions of the column).
fn initial(g: &Layered, real: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut key = vec![0.0f64; g.real + g.dummies.len()];
    for col in real {
        for (i, &v) in col.iter().enumerate() {
            key[v] = (i as f64 + 0.5) / col.len() as f64;
        }
    }
    for (k, d) in g.dummies.iter().enumerate() {
        key[g.real + k] = key[d.from] + d.t * (key[d.to] - key[d.from]);
    }
    let mut layers: Vec<Vec<usize>> = real.to_vec();
    for k in 0..g.dummies.len() {
        let item = g.real + k;
        if let Some(layer) = layers.get_mut(g.column[item]) {
            layer.push(item);
        }
    }
    for layer in &mut layers {
        // stable: real nodes keep their order among themselves
        layer.sort_by(|&a, &b| key[a].total_cmp(&key[b]).then((a >= g.real).cmp(&(b >= g.real))).then(a.cmp(&b)));
    }
    layers
}

fn positions(g: &Layered, layers: &[Vec<usize>]) -> Vec<usize> {
    let mut pos = vec![0usize; g.real + g.dummies.len()];
    for layer in layers {
        for (i, &v) in layer.iter().enumerate() {
            pos[v] = i;
        }
    }
    pos
}

fn crossings_of(g: &Layered, layers: &[Vec<usize>]) -> usize {
    let pos = positions(g, layers);
    g.links.iter().map(|l| crossings(l, &pos)).sum()
}

/// Barycenter sweeps keeping the best order seen, then transposes.
fn improve(g: &Layered, mut layers: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
    let cols = g.cols;
    let mut pos = positions(g, &layers);
    let mut best = layers.clone();
    let mut best_cross = crossings_of(g, &layers);
    for sweep in 0..SWEEPS {
        let down = sweep % 2 == 0;
        let order: Vec<usize> = if down { (1..cols).collect() } else { (0..cols.saturating_sub(1)).rev().collect() };
        for c in order {
            let mut bary: Vec<(usize, f64)> = layers[c]
                .iter()
                .map(|&v| {
                    let ends: Vec<f64> = if down {
                        g.links[c - 1].iter().filter(|l| l.b == v).map(|l| pos[l.a] as f64 + l.fa).collect()
                    } else {
                        g.links[c].iter().filter(|l| l.a == v).map(|l| pos[l.b] as f64 + l.fb).collect()
                    };
                    let own = pos[v] as f64 + 0.5;
                    (v, if ends.is_empty() { own } else { ends.iter().sum::<f64>() / ends.len() as f64 })
                })
                .collect();
            bary.sort_by(|a, b| a.1.total_cmp(&b.1));
            layers[c] = bary.into_iter().map(|(v, _)| v).collect();
            for (i, &v) in layers[c].iter().enumerate() {
                pos[v] = i;
            }
        }
        let cross = crossings_of(g, &layers);
        if cross < best_cross {
            best_cross = cross;
            best = layers.clone();
        }
    }
    layers = best;
    pos = positions(g, &layers);

    // swap neighbours while it removes crossings with the columns on either side
    for _ in 0..TRANSPOSES {
        let mut improved = false;
        for (c, layer) in layers.iter_mut().enumerate() {
            for i in 0..layer.len().saturating_sub(1) {
                let local = |pos: &[usize]| {
                    let left = if c > 0 { crossings(&g.links[c - 1], pos) } else { 0 };
                    left + g.links.get(c).map_or(0, |l| crossings(l, pos))
                };
                let before = local(&pos);
                let (a, b) = (layer[i], layer[i + 1]);
                pos[a] = i + 1;
                pos[b] = i;
                if local(&pos) < before {
                    layer.swap(i, i + 1);
                    improved = true;
                } else {
                    pos[a] = i;
                    pos[b] = i + 1;
                }
            }
        }
        if !improved {
            break;
        }
    }
    layers
}

/// Pairs of wires between two adjacent columns that cross.
fn crossings(links: &[Link], pos: &[usize]) -> usize {
    let ends: Vec<(f64, f64)> = links.iter().map(|l| (pos[l.a] as f64 + l.fa, pos[l.b] as f64 + l.fb)).collect();
    let mut n = 0;
    for (i, a) in ends.iter().enumerate() {
        for b in ends.iter().skip(i + 1) {
            if (a.0 - b.0) * (a.1 - b.1) < 0.0 {
                n += 1;
            }
        }
    }
    n
}
