use super::*;

/// A small deterministic generator (no dependency for tests).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> usize {
        (self.next() % n.max(1)) as usize
    }
}

fn node(w: f32, h: f32, ins: usize, outs: usize) -> Node {
    let row = |i: usize| 28.0 + 18.6 * (i as f32 + 0.5);
    Node { size: [w, h], inputs: (0..ins).map(row).collect(), outputs: (ins..ins + outs).map(row).collect(), param: false, pos: [0.0, 0.0], margin: [0.0, 0.0] }
}

fn edge(from: usize, to: usize) -> Edge {
    Edge { from, out: 0, to, inp: 0, weight: 1.0, labelled: false }
}

/// A random DAG: each node takes up to 3 inputs from earlier nodes.
fn random_dag(rng: &mut Lcg, n: usize) -> (Vec<Node>, Vec<Edge>) {
    let nodes: Vec<Node> = (0..n).map(|_| node(100.0 + rng.below(200) as f32, 60.0 + rng.below(160) as f32, 3, 2)).collect();
    let mut edges = Vec::new();
    for v in 1..n {
        for inp in 0..rng.below(4) {
            let u = rng.below(v as u64);
            edges.push(Edge { from: u, out: rng.below(2), to: v, inp, weight: 1.0 + rng.below(3) as f32, labelled: false });
        }
    }
    (nodes, edges)
}

fn overlaps(nodes: &[Node], l: &Layout) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for i in 0..nodes.len() {
        for j in i + 1..nodes.len() {
            let (a, b) = (l.pos[i], l.pos[j]);
            let (sa, sb) = (nodes[i].size, nodes[j].size);
            if a[0] < b[0] + sb[0] && b[0] < a[0] + sa[0] && a[1] < b[1] + sb[1] && b[1] < a[1] + sa[1] {
                out.push((i, j));
            }
        }
    }
    out
}

fn cost(edges: &[E], r: &[i64]) -> f64 {
    edges.iter().map(|e| e.w * (r[e.to] - r[e.from]) as f64).sum()
}

#[test]
fn network_simplex_ranks_are_optimal() {
    let mut rng = Lcg(7);
    for _ in 0..300 {
        let n = 2 + rng.below(5);
        let mut es = Vec::new();
        for v in 1..n {
            for _ in 0..rng.below(3) {
                es.push(E { from: rng.below(v as u64), out: 0, to: v, inp: 0, w: 1.0 + rng.below(4) as f64, labelled: false });
            }
        }
        let r = rank::rank(n, &es);
        assert!(es.iter().all(|e| r[e.to] > r[e.from]), "feasible: {r:?} {es:?}");
        // the best over every assignment of ranks 0..n
        let mut best = f64::INFINITY;
        let mut a = vec![0i64; n];
        loop {
            if es.iter().all(|e| a[e.to] > a[e.from]) {
                best = best.min(cost(&es, &a));
            }
            let mut i = 0;
            while i < n {
                a[i] += 1;
                if a[i] < n as i64 {
                    break;
                }
                a[i] = 0;
                i += 1;
            }
            if i == n {
                break;
            }
        }
        assert!((cost(&es, &r) - best).abs() < 1e-9, "cost {} vs best {best}: {r:?} {es:?}", cost(&es, &r));
    }
}

#[test]
fn a_parameter_sits_in_the_lane_beside_the_pin_it_feeds() {
    // a → b → c → d, and a parameter p feeding d's second input
    let mut nodes = vec![node(200.0, 120.0, 2, 1), node(200.0, 120.0, 2, 1), node(200.0, 120.0, 2, 1), node(200.0, 120.0, 2, 1)];
    let mut p = node(120.0, 50.0, 0, 1);
    p.outputs = vec![37.0];
    p.param = true;
    nodes.push(p);
    let edges = vec![edge(0, 1), edge(1, 2), edge(2, 3), Edge { from: 4, out: 0, to: 3, inp: 1, weight: 1.0, labelled: false }];
    let opts = Options::default();
    let l = layout(&nodes, &edges, &opts);
    assert_eq!(l.column, vec![0, 1, 2, 3, 3]);
    // right-aligned in d's lane, a lane gap from d
    assert_eq!(l.pos[4][0] + 120.0 + opts.lane_gap, l.pos[3][0]);
    // its pin level with d's second input
    assert_eq!(l.pos[4][1] + 37.0, l.pos[3][1] + nodes[3].inputs[1]);
    // and the chain straight along the top
    assert!(l.pos[..4].iter().all(|p| p[1] == 0.0), "{:?}", l.pos);
    assert!(overlaps(&nodes, &l).is_empty());
}

#[test]
fn a_source_is_ranked_beside_its_use_not_in_column_zero() {
    // a → b → c → d, and a spine source s (not a parameter) feeding only d
    let nodes: Vec<Node> = (0..5).map(|_| node(200.0, 100.0, 2, 1)).collect();
    let edges = vec![edge(0, 1), edge(1, 2), edge(2, 3), Edge { from: 4, out: 0, to: 3, inp: 1, weight: 1.0, labelled: false }];
    let l = layout(&nodes, &edges, &Options::default());
    assert_eq!(l.column[4], 2, "{:?}", l.column);
}

#[test]
fn a_node_off_a_busy_output_sits_beside_its_use() {
    // s fans out to a chain a → b → c → d and to x, which feeds only d: x goes beside d
    let nodes: Vec<Node> = (0..6).map(|_| node(200.0, 100.0, 2, 1)).collect();
    let (s, a, b, c, d, x) = (0, 1, 2, 3, 4, 5);
    let edges = vec![
        edge(s, a),
        edge(a, b),
        edge(b, c),
        edge(c, d),
        edge(s, b),
        edge(s, c),
        edge(s, x),
        Edge { from: x, out: 0, to: d, inp: 1, weight: 1.0, labelled: false },
    ];
    let l = layout(&nodes, &edges, &Options::default());
    assert_eq!(l.column[x], l.column[d] - 1, "{:?}", l.column);
}

#[test]
fn a_column_is_packed_to_the_top() {
    // one source feeding three nodes: they stack with exactly the node gap between them
    let nodes: Vec<Node> = (0..4).map(|_| node(200.0, 100.0, 1, 1)).collect();
    let edges = vec![edge(0, 1), edge(0, 2), edge(0, 3)];
    let opts = Options::default();
    let l = layout(&nodes, &edges, &opts);
    let mut ys: Vec<f32> = (1..4).map(|i| l.pos[i][1]).collect();
    ys.sort_by(f32::total_cmp);
    assert_eq!(ys, vec![0.0, 100.0 + opts.node_gap, 2.0 * (100.0 + opts.node_gap)]);
    // and the column right after the source's, one column gap on
    assert!((1..4).all(|i| l.pos[i][0] == 200.0 + opts.column_gap));
}

#[test]
fn a_source_above_others_in_its_column_stays_at_the_top() {
    // The research software's Mosaic style, second row: g has no inputs and feeds l; s is stacked under g and feeds l harder,
    // so l follows s. Lining g up with l pushed s down, l followed, and g again: a drift.
    let nodes: Vec<Node> = (0..4).map(|_| node(200.0, 100.0, 2, 1)).collect();
    let (a, g, s, l) = (0, 1, 2, 3);
    let mut seeded = nodes.clone();
    seeded[g].pos = [0.0, 0.0];
    seeded[s].pos = [0.0, 500.0];
    // g into l's top pin and s into the one under it, so g stays above s
    let edges = vec![
        edge(a, s),
        Edge { from: g, out: 0, to: l, inp: 0, weight: 1.0, labelled: false },
        Edge { from: s, out: 0, to: l, inp: 1, weight: 5.0, labelled: false },
    ];
    let opts = Options::default();
    let lay = layout(&seeded, &edges, &opts);
    assert_eq!(lay.column[g], lay.column[s]);
    assert_eq!(lay.pos[g][1], 0.0, "{:?}", lay.pos);
    assert_eq!(lay.pos[s][1], 100.0 + opts.node_gap);
}

#[test]
fn the_order_follows_the_pins() {
    // two sources wired crossed into one node's inputs: the one feeding the top pin goes on top
    let mut nodes: Vec<Node> = (0..3).map(|_| node(200.0, 100.0, 2, 1)).collect();
    nodes[0].pos = [0.0, 500.0]; // seeded below the other
    let edges =
        vec![Edge { from: 0, out: 0, to: 2, inp: 0, weight: 1.0, labelled: false }, Edge { from: 1, out: 0, to: 2, inp: 1, weight: 1.0, labelled: false }];
    let l = layout(&nodes, &edges, &Options::default());
    assert!(l.pos[0][1] < l.pos[1][1], "{:?}", l.pos);
}

#[test]
fn random_graphs_flow_left_to_right_without_overlaps_and_retidy_in_place() {
    let mut rng = Lcg(42);
    let opts = Options { max_aspect: 1000.0, ..Options::default() }; // no wrapping here
    for _ in 0..60 {
        let n = 1 + rng.below(30);
        let (mut nodes, edges) = random_dag(&mut rng, n);
        let l = layout(&nodes, &edges, &opts);
        assert_eq!(overlaps(&nodes, &l), vec![], "n={n}");
        for e in &edges {
            assert!(l.pos[e.to][0] >= l.pos[e.from][0] + nodes[e.from].size[0], "wire {} → {} runs back", e.from, e.to);
        }
        assert!(l.pos.iter().all(|p| p[0] >= 0.0 && p[1] >= 0.0 && p[0].is_finite() && p[1].is_finite()));
        // tidying a tidy graph moves nothing
        for (node, p) in nodes.iter_mut().zip(&l.pos) {
            node.pos = *p;
        }
        assert_eq!(layout(&nodes, &edges, &opts), l, "n={n}");
    }
}

#[test]
fn a_long_chain_wraps_where_the_fewest_wires_cross() {
    // 16 columns; every boundary is crossed by two wires (a chain and a skip of two) except
    // the one before node 9, crossed only by the chain
    let n = 16;
    let nodes: Vec<Node> = (0..n).map(|_| node(200.0, 100.0, 2, 1)).collect();
    let mut edges: Vec<Edge> = (1..n).map(|v| edge(v - 1, v)).collect();
    for v in (2..n).filter(|v| ![9, 10].contains(v)) {
        edges.push(Edge { from: v - 2, out: 0, to: v, inp: 1, weight: 0.1, labelled: false });
    }
    // flat and 16 columns wide (~40:1): an aspect limit of 15 makes two rows
    let l = layout(&nodes, &edges, &Options { max_aspect: 15.0, ..Options::default() });
    assert_eq!(l.rows, 2);
    assert_eq!(l.pos[9][0], 0.0, "the second row starts at node 9: {:?}", l.pos);
    assert!(l.pos[9][1] > l.pos[8][1] + 100.0);
    assert_eq!(overlaps(&nodes, &l), vec![]);
}

#[test]
fn a_graph_every_cut_of_which_crosses_many_wires_stays_one_row() {
    // 12 columns, each boundary crossed by the chain and three skips: no cut is cheap enough
    let n = 12;
    let nodes: Vec<Node> = (0..n).map(|_| node(200.0, 100.0, 4, 1)).collect();
    let mut edges: Vec<Edge> = (1..n).map(|v| edge(v - 1, v)).collect();
    for v in 2..n {
        for (k, back) in [2usize, 3, 4].into_iter().enumerate() {
            if v >= back {
                edges.push(Edge { from: v - back, out: 0, to: v, inp: k + 1, weight: 0.1, labelled: false });
            }
        }
    }
    let l = layout(&nodes, &edges, &Options { max_aspect: 2.0, ..Options::default() });
    assert_eq!(l.rows, 1);
}

#[test]
fn wrapped_random_graphs_never_overlap() {
    let mut rng = Lcg(9);
    let opts = Options { max_aspect: 1.0, min_wrap_columns: 3, max_cut_wires: usize::MAX, ..Options::default() }; // wrap eagerly
    let mut wrapped = 0;
    for _ in 0..60 {
        let n = 5 + rng.below(40);
        let (mut nodes, edges) = random_dag(&mut rng, n);
        for (i, node) in nodes.iter_mut().enumerate() {
            node.param = i % 7 == 3; // some parameters too
        }
        let l = layout(&nodes, &edges, &opts);
        wrapped += usize::from(l.rows > 1);
        assert_eq!(overlaps(&nodes, &l), vec![], "n={n} rows={}", l.rows);
    }
    assert!(wrapped > 10, "the test wraps: {wrapped}");
}

#[test]
fn hostile_input_is_clamped_or_dropped_never_a_panic() {
    let nodes = vec![
        Node {
            size: [f32::NAN, f32::INFINITY],
            inputs: vec![f32::NAN],
            outputs: vec![-5.0, 1e30],
            param: true,
            pos: [f32::NAN, 1e38],
            margin: [f32::NAN, -4.0],
        },
        Node { size: [0.0, -1.0], inputs: vec![], outputs: vec![], param: false, pos: [0.0, 0.0], margin: [0.0, 0.0] },
        node(1e9, 1e9, 1, 1),
    ];
    let edges = vec![
        Edge { from: 0, out: 9, to: 1, inp: 9, weight: f32::NAN, labelled: false },
        Edge { from: 1, out: 0, to: 2, inp: 0, weight: -3.0, labelled: false },
        Edge { from: 2, out: 0, to: 1, inp: 0, weight: 1.0, labelled: false }, // a cycle
        Edge { from: 1, out: 0, to: 1, inp: 0, weight: 1.0, labelled: false }, // a self loop
        Edge { from: 7, out: 0, to: 99, inp: 0, weight: 1.0, labelled: false },
    ];
    let l = layout(&nodes, &edges, &Options { node_gap: f32::NAN, max_aspect: -1.0, ..Options::default() });
    assert_eq!(l.pos.len(), 3);
    assert!(l.pos.iter().flatten().all(|v| v.is_finite()));
    assert_eq!(layout(&[], &[], &Options::default()).pos, Vec::<[f32; 2]>::new());
}

#[test]
fn wires_drawn_as_net_labels_dont_stop_a_wrap() {
    // the graph every cut of which crosses too many wires, with the skips drawn as labels: now
    // only the chain crosses each cut, so it wraps
    let n = 12;
    let nodes: Vec<Node> = (0..n).map(|_| node(200.0, 100.0, 4, 1)).collect();
    let mut edges: Vec<Edge> = (1..n).map(|v| edge(v - 1, v)).collect();
    for v in 2..n {
        for (k, back) in [2usize, 3, 4].into_iter().enumerate() {
            if v >= back {
                edges.push(Edge { from: v - back, out: 0, to: v, inp: k + 1, weight: 0.1, labelled: true });
            }
        }
    }
    let l = layout(&nodes, &edges, &Options { max_aspect: 2.0, ..Options::default() });
    assert!(l.rows > 1, "{:?}", l.rows);
    assert_eq!(overlaps(&nodes, &l), vec![]);
}

#[test]
fn a_labelled_wire_crosses_nothing_and_lines_nothing_up() {
    // two sources into one node, the top pin's wire drawn as a label: the order is the seed's,
    // where drawn wires would have swapped them
    let mut nodes: Vec<Node> = (0..3).map(|_| node(200.0, 100.0, 2, 1)).collect();
    nodes[0].pos = [0.0, 500.0];
    let edges =
        vec![Edge { from: 0, out: 0, to: 2, inp: 0, weight: 1.0, labelled: true }, Edge { from: 1, out: 0, to: 2, inp: 1, weight: 1.0, labelled: false }];
    let l = layout(&nodes, &edges, &Options::default());
    assert!(l.pos[1][1] < l.pos[0][1], "{:?}", l.pos);
    // still left to right
    assert!(l.pos[2][0] >= l.pos[0][0] + 200.0);
    // the target lines up with its drawn input, not the labelled one
    assert_eq!(l.pos[2][1], l.pos[1][1]);
}

#[test]
fn tags_get_room_beside_their_pins() {
    // a -> b -> c; b has a 90-wide tag at an input and a 60-wide one at an output; p, a
    // parameter, feeds b
    let mut nodes: Vec<Node> = (0..4).map(|_| node(200.0, 100.0, 2, 1)).collect();
    let (a, b, c, p) = (0, 1, 2, 3);
    nodes[b].margin = [90.0, 60.0];
    nodes[p] = Node { param: true, ..node(120.0, 60.0, 0, 1) };
    let edges = vec![edge(a, b), edge(b, c), Edge { from: p, out: 0, to: b, inp: 1, weight: 1.0, labelled: false }];
    let opts = Options::default();
    let l = layout(&nodes, &edges, &opts);
    let plain = layout(&nodes.iter().cloned().map(|n| Node { margin: [0.0, 0.0], ..n }).collect::<Vec<_>>(), &edges, &opts);
    // the lane, then the tag, then b
    assert!(l.pos[p][0] + 120.0 + opts.lane_gap + 90.0 <= l.pos[b][0] + 0.01, "{:?}", l.pos);
    assert_eq!(l.pos[b][0] - plain.pos[b][0], 90.0);
    // c past b's output tag
    assert!(l.pos[c][0] >= l.pos[b][0] + 200.0 + 60.0 + opts.column_gap - 0.01, "{:?}", l.pos);
    assert_eq!(l.pos[c][0] - plain.pos[c][0], 150.0);
}
