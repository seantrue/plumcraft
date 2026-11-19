//! What a node shows as rows, and what a collapsed group's node offers as outputs: the pure part
//! of the canvas's node faces, which the menu model needs too (a node's Promote menu lists its
//! input rows; a collapsed group's menu lists its outputs). Moved from `plumcraft-ui`'s
//! `canvas.rs` and `groups_view.rs` in N1 slice 8, which re-export it.

use std::collections::BTreeSet;

use plumcraft_graph::{Graph, GroupId, NodeId, PortRef};
use plumcraft_proto::catalogue::{NodeDef, Port, SchemaReply};
use serde_json::Value;

/// What kind of row an input row is.
#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    Plain,
    /// A sub-port of choice port `parent`.
    Sub {
        parent: String,
    },
    /// Slot `index` (0-based) of variadic port `parent`.
    Slot {
        parent: String,
        index: usize,
    },
}

/// One input row of a node.
#[derive(Debug, Clone, PartialEq)]
pub struct InputRow {
    /// The value and wire key: `sigma`, `mode.turn`, `c2`.
    pub key: String,
    /// The label shown beside the pin.
    pub label: String,
    pub port: Port,
    pub kind: RowKind,
}

/// A port's label: the catalogue's, else its name.
pub fn port_label(p: &Port) -> String {
    match p.label.as_deref() {
        Some(l) if !l.is_empty() => l.to_owned(),
        _ => p.name.clone(),
    }
}

/// The input rows of a node of type `def` in `graph`.
pub fn input_rows(def: &NodeDef, id: &NodeId, graph: &Graph) -> Vec<InputRow> {
    let values = graph.node(id).map(|n| &n.values);
    let mut rows = Vec::new();
    for port in &def.inputs {
        if let Some(v) = &port.variadic {
            let names = plumcraft_graph::variadic_slots(port);
            let wired: Vec<bool> = names.iter().map(|n| graph.wire_into(&PortRef::new(id.clone(), n.clone())).is_some()).collect();
            let last = wired.iter().rposition(|w| *w).map_or(0, |i| i + 1);
            let count = last.saturating_add(1).max(v.min as usize).min(names.len());
            let base = port_label(port);
            for (index, name) in names.into_iter().take(count).enumerate() {
                let mut slot = port.clone();
                slot.variadic = None;
                rows.push(InputRow { label: format!("{base} {name}"), key: name, port: slot, kind: RowKind::Slot { parent: port.name.clone(), index } });
            }
            continue;
        }
        rows.push(InputRow { key: port.name.clone(), label: port_label(port), port: port.clone(), kind: RowKind::Plain });
        if let Some(options) = &port.options {
            let choice = values.and_then(|v| v.get(&port.name)).unwrap_or(&port.default);
            let key = match choice {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            for sub in options.get(&key).map(Vec::as_slice).unwrap_or_default() {
                rows.push(InputRow {
                    key: format!("{}.{}", port.name, sub.name),
                    label: format!("{} › {}", port_label(port), port_label(sub)),
                    port: sub.clone(),
                    kind: RowKind::Sub { parent: port.name.clone() },
                });
            }
        }
    }
    rows
}

/// The rows node `id` shows: [`input_rows`] without the fields its choice doesn't use, unless
/// the node shows every field. Also how many were tucked away.
pub fn shown_rows(def: &NodeDef, id: &NodeId, graph: &Graph) -> (Vec<InputRow>, usize) {
    let mut inputs = input_rows(def, id, graph);
    let Some(node) = graph.node(id) else { return (inputs, 0) };
    let tucked = plumcraft_graph::ports::tucked(def, node, |p| graph.wire_into(&PortRef::new(id.clone(), p)).is_some());
    if !node.all_fields {
        inputs.retain(|r| !(r.kind == RowKind::Plain && tucked.contains(&r.key)));
    }
    (inputs, tucked.len())
}

/// `base`, or `base 2`, `base 3`, … whichever `taken` doesn't have.
pub fn unique_name(base: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_owned();
    }
    (2u32..).map(|i| format!("{base} {i}")).find(|n| !taken.contains(n)).unwrap_or_else(|| base.to_owned())
}

/// One output of a collapsed group's node: the member's port it stands for.
#[derive(Debug, Clone, PartialEq)]
pub struct ProxyOut {
    pub port: PortRef,
    pub label: String,
    pub def: Port,
}

/// Group `gid`'s members, top to bottom as they sit (then left to right), with their types.
pub fn ordered_members<'a>(graph: &'a Graph, catalog: &'a SchemaReply, gid: &GroupId) -> Vec<(NodeId, &'a NodeDef)> {
    let members = graph.group_members(gid);
    let mut ordered: Vec<&NodeId> = members.iter().collect();
    ordered.sort_by(|a, b| {
        let at = |n: &NodeId| graph.node(n).map_or([0.0, 0.0], |n| n.pos);
        let (pa, pb) = (at(a), at(b));
        pa[1].total_cmp(&pb[1]).then(pa[0].total_cmp(&pb[0])).then(a.cmp(b))
    });
    ordered.into_iter().filter_map(|n| Some((n.clone(), catalog.node(&graph.node(n)?.kind)?))).collect()
}

/// The outputs a collapsed group's node offers: the outputs the group names, its members' ports
/// the engine marks external, then member by member the outputs wired outside and (unless the
/// group already names what it makes) those wired nowhere.
pub fn proxy_outputs(graph: &Graph, catalog: &SchemaReply, gid: &GroupId) -> Vec<ProxyOut> {
    let Some(grp) = graph.group(gid) else { return Vec::new() };
    let members = graph.group_members(gid);
    let defs = ordered_members(graph, catalog, gid);
    let title = |n: &NodeId| plumcraft_session::node_title(graph, catalog, n);
    let out_def = |at: &PortRef| -> Option<Port> { catalog.node(&graph.node(&at.node)?.kind)?.output(&at.port).cloned() };
    let mut outputs: Vec<ProxyOut> = Vec::new();
    for o in &grp.outputs {
        if let Some(def) = out_def(&o.from) {
            outputs.push(ProxyOut { port: o.from.clone(), label: o.name.clone(), def });
        }
    }
    for (n, def) in &defs {
        for port in def.outputs.iter().filter(|p| p.external) {
            let at = PortRef::new(n.clone(), port.name.clone());
            if !outputs.iter().any(|o| o.port == at) {
                outputs.push(ProxyOut { port: at, label: title(n), def: port.clone() });
            }
        }
    }
    let loose = outputs.is_empty();
    for (n, def) in &defs {
        for port in &def.outputs {
            let at = PortRef::new(n.clone(), port.name.clone());
            if outputs.iter().any(|o| o.port == at) {
                continue;
            }
            let to: Vec<&NodeId> = graph.wires.iter().filter(|w| w.from == at).map(|w| &w.to.node).collect();
            if to.iter().any(|t| !members.contains(*t)) || (to.is_empty() && loose) {
                let label = format!("{} › {}", title(n), port_label(port));
                outputs.push(ProxyOut { port: at, label, def: port.clone() });
            }
        }
    }
    outputs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_names() {
        let taken: BTreeSet<String> = ["text".to_owned(), "text 2".to_owned()].into();
        assert_eq!(unique_name("text", &taken), "text 3");
        assert_eq!(unique_name("find", &taken), "find");
    }
}
