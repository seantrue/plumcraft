//! Phase 2: columns by network simplex (Gansner, Koutsofios, North, Vo, "A Technique for
//! Drawing Directed Graphs", 1993, §2): ranks that minimise Σ weight × (rank(to) − rank(from))
//! with every wire at least one column long.
//!
//! Start from the longest-path ranking, grow a tight spanning tree, then while some tree edge has
//! a negative cut value, swap it for the non-tree edge of least slack across the same cut. Each
//! connected component is ranked on its own and starts at column 0.

use crate::E;

/// The most simplex exchanges per component: a guard against cycling on degenerate input.
const MAX_EXCHANGES: usize = 10_000;
const EPS: f64 = 1e-9;

/// `edges` without the ones that close a cycle (a depth-first search's back edges, in node
/// order), so ranking always has a solution. Workflows are acyclic; hostile files may not be.
pub(crate) fn acyclic(n: usize, edges: Vec<E>) -> Vec<E> {
    let mut out_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (k, e) in edges.iter().enumerate() {
        out_of[e.from].push(k);
    }
    // 0 unvisited, 1 on the stack, 2 done
    let mut state = vec![0u8; n];
    let mut back = vec![false; edges.len()];
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        // iterative DFS: (node, next out-edge index)
        let mut stack = vec![(root, 0usize)];
        state[root] = 1;
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if let Some(&k) = out_of[v].get(*next) {
                *next += 1;
                let t = edges[k].to;
                match state[t] {
                    0 => {
                        state[t] = 1;
                        stack.push((t, 0));
                    }
                    1 => back[k] = true,
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    edges.into_iter().zip(back).filter(|(_, b)| !b).map(|(e, _)| e).collect()
}

/// Each node's column. `edges` must be acyclic ([`acyclic`]).
pub(crate) fn rank(n: usize, edges: &[E]) -> Vec<i64> {
    let mut rank = longest_path(n, edges);
    for comp in components(n, edges) {
        let ce: Vec<usize> = (0..edges.len()).filter(|&k| comp.binary_search(&edges[k].from).is_ok()).collect();
        simplex(n, &comp, &ce, edges, &mut rank);
        let min = comp.iter().map(|&v| rank[v]).min().unwrap_or(0);
        for &v in &comp {
            rank[v] -= min;
        }
    }
    rank
}

/// As early as possible: sources at 0, everything else one past its latest input.
fn longest_path(n: usize, edges: &[E]) -> Vec<i64> {
    let mut indeg = vec![0usize; n];
    let mut out_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in edges {
        indeg[e.to] += 1;
        out_of[e.from].push(e.to);
    }
    let mut rank = vec![0i64; n];
    let mut queue: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    while let Some(v) = queue.pop() {
        for &t in &out_of[v] {
            rank[t] = rank[t].max(rank[v] + 1);
            indeg[t] -= 1;
            if indeg[t] == 0 {
                queue.push(t);
            }
        }
    }
    rank
}

/// Weakly connected components, each sorted.
fn components(n: usize, edges: &[E]) -> Vec<Vec<usize>> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in edges {
        adj[e.from].push(e.to);
        adj[e.to].push(e.from);
    }
    let mut seen = vec![false; n];
    let mut out = Vec::new();
    for root in 0..n {
        if seen[root] {
            continue;
        }
        seen[root] = true;
        let mut comp = vec![root];
        let mut stack = vec![root];
        while let Some(v) = stack.pop() {
            for &t in &adj[v] {
                if !seen[t] {
                    seen[t] = true;
                    comp.push(t);
                    stack.push(t);
                }
            }
        }
        comp.sort_unstable();
        out.push(comp);
    }
    out
}

fn slack(rank: &[i64], e: &E) -> i64 {
    rank[e.to] - rank[e.from] - 1
}

/// Network simplex on one component (`comp` its nodes, `ce` its edges' indices).
fn simplex(n: usize, comp: &[usize], ce: &[usize], edges: &[E], rank: &mut [i64]) {
    if comp.len() < 2 {
        return;
    }
    let mut in_tree = vec![false; n];
    let mut tree: Vec<usize> = Vec::new();
    in_tree[comp[0]] = true;
    let mut count = 1;
    // a feasible tree: grow it along tight edges; when stuck, shift it to tighten the least slack edge
    loop {
        loop {
            let mut grew = false;
            for &k in ce {
                let e = &edges[k];
                if in_tree[e.from] != in_tree[e.to] && slack(rank, e) == 0 {
                    in_tree[e.from] = true;
                    in_tree[e.to] = true;
                    tree.push(k);
                    count += 1;
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        if count >= comp.len() {
            break;
        }
        let Some(k) = ce.iter().copied().filter(|&k| in_tree[edges[k].from] != in_tree[edges[k].to]).min_by_key(|&k| slack(rank, &edges[k])) else {
            return; // not connected after all: keep the longest-path ranks
        };
        let s = slack(rank, &edges[k]);
        let delta = if in_tree[edges[k].from] { s } else { -s };
        for &v in comp {
            if in_tree[v] {
                rank[v] += delta;
            }
        }
    }

    for _ in 0..MAX_EXCHANGES {
        // the tree edge with the most negative cut value leaves
        let mut leave: Option<(usize, f64, Vec<bool>)> = None;
        for (ti, &k) in tree.iter().enumerate() {
            let side = tail_side(n, &tree, ti, edges);
            let cut: f64 = ce
                .iter()
                .map(|&j| {
                    let e = &edges[j];
                    match (side[e.from], side[e.to]) {
                        (true, false) => e.w,
                        (false, true) => -e.w,
                        _ => 0.0,
                    }
                })
                .sum();
            if cut < -EPS && leave.as_ref().is_none_or(|(_, best, _)| cut < *best) {
                leave = Some((k, cut, side));
            }
        }
        let Some((k, _, side)) = leave else { return };
        // the non-tree edge from the head side to the tail side with the least slack enters
        let Some(j) = ce.iter().copied().filter(|&j| !tree.contains(&j) && !side[edges[j].from] && side[edges[j].to]).min_by_key(|&j| slack(rank, &edges[j]))
        else {
            return;
        };
        let s = slack(rank, &edges[j]);
        tree.retain(|&t| t != k);
        tree.push(j);
        // the head side moves right until the entering edge is tight
        for &v in comp {
            if !side[v] {
                rank[v] += s;
            }
        }
    }
}

/// With tree edge `skip` removed, which nodes are on its tail's side.
fn tail_side(n: usize, tree: &[usize], skip: usize, edges: &[E]) -> Vec<bool> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (ti, &k) in tree.iter().enumerate() {
        if ti != skip {
            adj[edges[k].from].push(edges[k].to);
            adj[edges[k].to].push(edges[k].from);
        }
    }
    let mut side = vec![false; n];
    let Some(&k) = tree.get(skip) else { return side };
    let root = edges[k].from;
    side[root] = true;
    let mut stack = vec![root];
    while let Some(v) = stack.pop() {
        for &t in &adj[v] {
            if !side[t] {
                side[t] = true;
                stack.push(t);
            }
        }
    }
    side
}
