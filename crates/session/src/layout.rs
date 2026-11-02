//! Placing nodes a workflow file gives no position (`DEMO_WORKFLOW`, hand-written or generated
//! workflows): in columns by depth (the longest path from a source), each column in id order,
//! as `plumcraft-view` lays its cards out. Nodes with a `pos` keep it; placed nodes start to the
//! right of them.
//!
//! This works on the workflow JSON before [`plumcraft_graph::Graph::from_json`] reads it, because
//! the graph defaults a missing position to `[0, 0]` and can't tell it from a real one.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// Horizontal distance between columns, and vertical between rows (graph units).
pub const COLUMN_STEP: f32 = 320.0;
pub const ROW_STEP: f32 = 210.0;

/// Nodes in columns by depth (longest path from a source), each column in id order.
pub fn columns(workflow: &Value) -> Vec<Vec<String>> {
    let nodes: Vec<String> = workflow.get("nodes").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let wires: Vec<(String, String)> = workflow
        .get("wires")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|w| Some((w.get("from")?.get("node")?.as_str()?.to_owned(), w.get("to")?.get("node")?.as_str()?.to_owned())))
        .collect();
    let at: BTreeMap<&str, usize> = nodes.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut waiting = vec![0usize; nodes.len()];
    for (a, b) in &wires {
        if let (Some(&a), Some(&b)) = (at.get(a.as_str()), at.get(b.as_str())) {
            next[a].push(b);
            waiting[b] += 1;
        }
    }
    // Each node once its sources are done, so a file's wires cost what they number, in any order.
    // A cycle is never done: its nodes keep the depth they have when their other sources finish.
    let mut ready: Vec<usize> = (0..nodes.len()).filter(|i| waiting[*i] == 0).collect();
    let mut depths = vec![0usize; nodes.len()];
    while let Some(n) = ready.pop() {
        for &m in &next[n] {
            depths[m] = depths[m].max(depths[n] + 1);
            waiting[m] -= 1;
            if waiting[m] == 0 {
                ready.push(m);
            }
        }
    }
    let depth: BTreeMap<String, usize> = nodes.iter().cloned().zip(depths).collect();
    let mut cols: Vec<Vec<String>> = Vec::new();
    for (n, d) in depth {
        if cols.len() <= d {
            cols.resize(d + 1, Vec::new());
        }
        if let Some(c) = cols.get_mut(d) {
            c.push(n);
        }
    }
    cols
}

/// Give every node without a `pos` one, in [`columns`]. Returns how many were placed.
pub fn place_unplaced(workflow: &mut Value) -> usize {
    let placed = |n: &Value| n.get("pos").and_then(Value::as_array).is_some_and(|p| p.len() == 2 && p.iter().all(Value::is_number));
    let Some(nodes) = workflow.get("nodes").and_then(Value::as_object) else { return 0 };
    if nodes.values().all(placed) {
        return 0;
    }
    // To the right of what is placed already.
    let right = nodes
        .values()
        .filter(|n| placed(n))
        .filter_map(|n| n.get("pos")?.get(0)?.as_f64())
        .filter(|x| x.is_finite())
        .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))));
    let x0 = right.map_or(0.0, |r| r as f32 + COLUMN_STEP);
    let cols = columns(workflow);
    let mut count = 0;
    let Some(nodes) = workflow.get_mut("nodes").and_then(Value::as_object_mut) else { return 0 };
    for (c, col) in cols.iter().enumerate() {
        let mut row = 0usize;
        for id in col {
            let Some(n) = nodes.get_mut(id) else { continue };
            if placed(n) {
                continue;
            }
            if let Some(obj) = n.as_object_mut() {
                obj.insert("pos".into(), json!([x0 + c as f32 * COLUMN_STEP, row as f32 * ROW_STEP]));
                row += 1;
                count += 1;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long chain, its wires in the worst order, in the time a person waits. Relaxing them pass
    /// by pass took a pass per node.
    #[test]
    fn a_long_chain_costs_what_its_wires_number() {
        let n = 200_000;
        let nodes: serde_json::Map<String, Value> = (0..n).map(|i| (format!("n{i}"), json!({"kind": "k"}))).collect();
        let wires: Vec<Value> = (0..n - 1).rev().map(|i| json!({"from": {"node": format!("n{i}")}, "to": {"node": format!("n{}", i + 1)}})).collect();
        let w = json!({"nodes": nodes, "wires": wires});
        let t = std::time::Instant::now();
        let cols = columns(&w);
        assert_eq!(cols.len(), n, "one column a node");
        assert!(t.elapsed() < std::time::Duration::from_secs(10), "{:?}", t.elapsed());
    }

    #[test]
    fn the_demo_is_laid_out_in_columns() {
        let mut w: Value = serde_json::from_str(plumcraft_text::DEMO_WORKFLOW).unwrap();
        assert_eq!(columns(&w), [vec!["story"], vec!["broken", "think"], vec!["after", "swap"], vec!["count", "shout"]]);
        assert_eq!(place_unplaced(&mut w), 7);
        let pos = |id: &str| w["nodes"][id]["pos"].clone();
        assert_eq!(pos("story"), json!([0.0, 0.0]));
        assert_eq!(pos("think"), json!([COLUMN_STEP, ROW_STEP]));
        assert_eq!(pos("shout"), json!([3.0 * COLUMN_STEP, ROW_STEP]));
        // every node has its own place
        let mut all: Vec<String> = w["nodes"].as_object().unwrap().values().map(|n| n["pos"].to_string()).collect();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 7);
        assert_eq!(place_unplaced(&mut w), 0, "nothing left to place");
        plumcraft_graph::Graph::from_json(&w.to_string()).unwrap();
    }

    #[test]
    fn placed_nodes_keep_their_place_and_others_go_beside_them() {
        let mut w = json!({"version": 1, "nodes": {"a": {"kind": "k", "pos": [500, 40]}, "b": {"kind": "k"}}, "wires": []});
        assert_eq!(place_unplaced(&mut w), 1);
        assert_eq!(w["nodes"]["a"]["pos"], json!([500, 40]));
        assert_eq!(w["nodes"]["b"]["pos"], json!([500.0 + COLUMN_STEP, 0.0]));
    }

    #[test]
    fn hostile_workflows_dont_panic() {
        for mut w in [
            json!(null),
            json!({"nodes": 3}),
            json!({"nodes": {"a": 1, "b": {"pos": "x"}}, "wires": [{"from": 1}, {"from": {"node": "a"}, "to": {"node": "a"}}]}),
            json!({"nodes": {"a": {}, "b": {}}, "wires": [{"from": {"node": "a"}, "to": {"node": "b"}}, {"from": {"node": "b"}, "to": {"node": "a"}}]}),
        ] {
            place_unplaced(&mut w);
        }
    }
}
