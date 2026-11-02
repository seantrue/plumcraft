//! Edit menu commands: undo and redo (by graph snapshots), copy, cut and paste (the clipboard
//! holds workflow JSON, so it crosses workflows and windows), align and distribute.
//!
//! Copy only reads the graph; the UI puts its text on the system clipboard (egui's
//! `copy_text`; Cmd+V arrives as egui's Paste event with the system clipboard's text). Cut,
//! paste, align and distribute are undo steps.
//!
//! Lifted from the research software's `crates/engine/src/edit_cmds.rs` (2026-10-27, the research software `8d96021`; plan
//! 10.3, E2) without wire slices, bend points or net labels; `edit.cut` is new (the research software's UI
//! runs copy then remove, two journal entries). Align and distribute also take collapsed
//! groups, which PlumCraft's selection holds, and a node with no size is estimated as the canvas
//! draws it ([`crate::layout_cmds::geometry`]), not taken for a point.

use std::collections::BTreeMap;

use plumcraft_graph::{Graph, GraphError, GroupId, NodeId, PortRef};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, group_list_param, node_list_param, pos_param, str_param};
use crate::{Result, Session, SessionError};

fn can_undo(s: &Session) -> std::result::Result<(), String> {
    if s.can_undo() { Ok(()) } else { Err("nothing to undo".into()) }
}

fn can_redo(s: &Session) -> std::result::Result<(), String> {
    if s.can_redo() { Ok(()) } else { Err("nothing to redo".into()) }
}

fn undo(s: &mut Session, _: &Value) -> Result<Value> {
    let label = s.undo_label().map(str::to_owned);
    s.step_undo();
    Ok(json!({"undid": label, "undo": s.undo_label(), "redo": s.redo_label()}))
}

fn redo(s: &mut Session, _: &Value) -> Result<Value> {
    let label = s.redo_label().map(str::to_owned);
    s.step_redo();
    Ok(json!({"redid": label, "undo": s.undo_label(), "redo": s.redo_label()}))
}

fn non_empty(p: &Value) -> Result<Vec<NodeId>> {
    let mut nodes = node_list_param(p, "nodes")?;
    if nodes.is_empty() {
        return Err(SessionError::Params("\"nodes\" must be a non-empty list of node ids".into()));
    }
    nodes.sort();
    nodes.dedup();
    Ok(nodes)
}

/// The nodes and the wires among them as workflow JSON, with their counts.
fn copied(s: &Session, nodes: &[NodeId]) -> Result<Value> {
    if let Some(missing) = nodes.iter().find(|id| s.graph().node(id).is_none()) {
        return Err(GraphError::UnknownNode(missing.clone()).into());
    }
    let sub = s.graph().subgraph(nodes);
    let text = sub.to_json()?;
    Ok(json!({"text": text, "nodes": sub.nodes.len(), "wires": sub.wires.len()}))
}

/// `edit.copy {nodes}`: the nodes and the wires among them as workflow JSON.
fn copy(s: &mut Session, p: &Value) -> Result<Value> {
    let nodes = non_empty(p)?;
    copied(s, &nodes)
}

/// `edit.cut {nodes}`: copy, then remove the nodes (and every wire they had), in one undo step.
fn cut(s: &mut Session, p: &Value) -> Result<Value> {
    let nodes = non_empty(p)?;
    let out = copied(s, &nodes)?;
    let label = if nodes.len() == 1 { "Cut Node" } else { "Cut Nodes" };
    s.edit(label, |g, _| {
        for id in &nodes {
            g.remove_node(id)?;
        }
        Ok(())
    })?;
    Ok(out)
}

/// The default paste offset when no position is given (so a paste over its own source shows).
pub const PASTE_OFFSET: [f32; 2] = [40.0, 40.0];

/// `edit.paste {text, pos?}`: add the clipboard's nodes under fresh ids, with the wires among
/// them. With `pos`, the pasted nodes' top-left corner goes there; otherwise they sit
/// [`PASTE_OFFSET`] from where they were copied. Wires that no longer type-check are dropped
/// (reported); a node type the engine doesn't know refuses the whole paste.
fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text")?;
    if text.len() as u64 > crate::MAX_WORKFLOW_BYTES {
        return Err(SessionError::Params(format!("the clipboard holds more than {} bytes", crate::MAX_WORKFLOW_BYTES)));
    }
    let clip = Graph::from_json(text).map_err(|e| SessionError::Params(format!("the clipboard doesn't hold a PlumCraft workflow: {e}")))?;
    if clip.nodes.is_empty() {
        return Err(SessionError::Params("the clipboard holds no nodes".into()));
    }
    let pos = pos_param(p, "pos")?;
    let min = clip.nodes.values().fold([f32::INFINITY, f32::INFINITY], |m, n| [m[0].min(n.pos[0]), m[1].min(n.pos[1])]);
    let shift = match pos {
        Some(at) => [at[0] - min[0], at[1] - min[1]],
        None => PASTE_OFFSET,
    };
    if !shift[0].is_finite() || !shift[1].is_finite() || clip.nodes.values().any(|n| !(n.pos[0] + shift[0]).is_finite() || !(n.pos[1] + shift[1]).is_finite()) {
        return Err(SessionError::Params("the pasted positions are not finite".into()));
    }
    let label = if clip.nodes.len() == 1 { "Paste Node" } else { "Paste Nodes" };
    let (map, dropped) = s.edit(label, |g, catalog| {
        let mut map: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        for (old, node) in &clip.nodes {
            if catalog.node(&node.kind).is_none() {
                return Err(GraphError::UnknownKind(node.kind.clone()).into());
            }
            let mut node = node.clone();
            node.pos = [node.pos[0] + shift[0], node.pos[1] + shift[1]];
            map.insert(old.clone(), g.insert_node(node));
        }
        let mut dropped = Vec::new();
        for w in &clip.wires {
            let (Some(from), Some(to)) = (map.get(&w.from.node), map.get(&w.to.node)) else { continue };
            let (from, to) = (PortRef::new(from.clone(), w.from.port.clone()), PortRef::new(to.clone(), w.to.port.clone()));
            match g.connect_sliced(catalog, from, to.clone(), w.slice.clone()) {
                // bend points move with the copies (one that would leave the canvas is dropped)
                Ok(_) if !w.via.is_empty() => {
                    let by = |c: i32, d: f32| c.saturating_add(d.round().clamp(-1e9, 1e9) as i32);
                    let _ = g.set_via(&to, w.via.iter().map(|v| [by(v[0], shift[0]), by(v[1], shift[1])]).collect());
                }
                Ok(_) => {}
                Err(e) => dropped.push(json!({"wire": crate::graph_cmds::wire_json(w), "reason": e.to_string()})),
            }
        }
        g.adopt_nets(&clip, &map, &BTreeMap::new());
        Ok((map, dropped))
    })?;
    let map_json: serde_json::Map<String, Value> = map.iter().map(|(old, new)| (old.0.clone(), Value::from(new.0.clone()))).collect();
    Ok(json!({"nodes": map.values().collect::<Vec<_>>(), "map": map_json, "dropped": dropped}))
}

/// Sizes from the UI (`{id: [w, h]}`, a collapsed group by its group id).
fn sizes_param(p: &Value) -> Result<BTreeMap<String, [f32; 2]>> {
    let bad = || SessionError::Params("\"sizes\" must be {id: [w, h]} with finite, non-negative numbers".into());
    match p.get("sizes") {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(o)) => o
            .iter()
            .map(|(k, v)| {
                let [w, h] = v.as_array().map(Vec::as_slice).ok_or_else(bad)? else { return Err(bad()) };
                let f = |c: &Value| c.as_f64().map(|f| f as f32).filter(|f| f.is_finite() && *f >= 0.0).ok_or_else(bad);
                Ok((k.clone(), [f(w)?, f(h)?]))
            })
            .collect(),
        Some(_) => Err(bad()),
    }
}

/// Something on the canvas that arranging moves: a node, or a collapsed group, one node there.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Item {
    Node(NodeId),
    Group(GroupId),
}

/// `nodes` and `groups` with their rects (`[x, y, w, h]`), at least `at_least` of them. An
/// unknown id is an error.
pub(crate) fn rects(s: &Session, p: &Value, at_least: usize) -> Result<Vec<(Item, [f32; 4])>> {
    let mut nodes = node_list_param(p, "nodes")?;
    nodes.sort();
    nodes.dedup();
    let mut groups = group_list_param(p, "groups")?;
    groups.sort();
    groups.dedup();
    if nodes.len() + groups.len() < at_least {
        return Err(SessionError::Params(format!("select at least {at_least} nodes")));
    }
    let sizes = sizes_param(p)?;
    let mut out = Vec::new();
    for id in nodes {
        let n = s.graph().node(&id).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        let size = sizes.get(&id.0).copied();
        let [w, h] = crate::layout_cmds::geometry(s.graph(), s.catalog(), &id, size, None).map_or(plumcraft_layout::DEFAULT_SIZE, |(size, ..)| size);
        out.push((Item::Node(id), [n.pos[0], n.pos[1], w, h]));
    }
    for id in groups {
        let g = s.graph().group(&id).ok_or_else(|| GraphError::UnknownGroup(id.clone()))?;
        let [w, h] = sizes.get(&id.0).copied().unwrap_or(plumcraft_layout::DEFAULT_SIZE);
        out.push((Item::Group(id), [g.pos[0], g.pos[1], w, h]));
    }
    Ok(out)
}

/// `edit.align {nodes, groups?, edge, sizes?}`: line them up on one edge or centre line.
fn align(s: &mut Session, p: &Value) -> Result<Value> {
    let edge = str_param(p, "edge")?;
    let rs = rects(s, p, 2)?;
    let n = rs.len() as f32;
    let min = |f: &dyn Fn(&[f32; 4]) -> f32| rs.iter().map(|(_, r)| f(r)).fold(f32::INFINITY, f32::min);
    let max = |f: &dyn Fn(&[f32; 4]) -> f32| rs.iter().map(|(_, r)| f(r)).fold(f32::NEG_INFINITY, f32::max);
    let place = |f: &dyn Fn(&[f32; 4]) -> [f32; 2]| -> Vec<(Item, [f32; 2])> { rs.iter().map(|(id, r)| (id.clone(), f(r))).collect() };
    let moves = match edge {
        "left" => {
            let x = min(&|r| r[0]);
            place(&|r| [x, r[1]])
        }
        "right" => {
            let x = max(&|r| r[0] + r[2]);
            place(&|r| [x - r[2], r[1]])
        }
        "top" => {
            let y = min(&|r| r[1]);
            place(&|r| [r[0], y])
        }
        "bottom" => {
            let y = max(&|r| r[1] + r[3]);
            place(&|r| [r[0], y - r[3]])
        }
        "centerX" => {
            let x = rs.iter().map(|(_, r)| r[0] + r[2] / 2.0).sum::<f32>() / n;
            place(&|r| [x - r[2] / 2.0, r[1]])
        }
        "centerY" => {
            let y = rs.iter().map(|(_, r)| r[1] + r[3] / 2.0).sum::<f32>() / n;
            place(&|r| [r[0], y - r[3] / 2.0])
        }
        other => return Err(SessionError::Params(format!("unknown edge {other:?} (left, right, top, bottom, centerX, centerY)"))),
    };
    apply_moves(s, "Align", moves)
}

/// `edit.distribute {nodes, groups?, axis, sizes?}`: keep the outermost, and space the rest so
/// the gaps between neighbours are equal.
fn distribute(s: &mut Session, p: &Value) -> Result<Value> {
    let axis = match str_param(p, "axis")? {
        "horizontal" | "x" => 0,
        "vertical" | "y" => 1,
        other => return Err(SessionError::Params(format!("unknown axis {other:?} (horizontal, vertical)"))),
    };
    let mut rs = rects(s, p, 3)?;
    rs.sort_by(|a, b| a.1[axis].total_cmp(&b.1[axis]).then_with(|| a.0.cmp(&b.0)));
    let (Some(first), Some(last)) =
        (rs.first().map(|r| r.1), rs.iter().map(|r| r.1).max_by(|a, b| (a[axis] + a[axis + 2]).total_cmp(&(b[axis] + b[axis + 2]))))
    else {
        return Err(SessionError::Params("select at least 3 nodes".into()));
    };
    let span = (last[axis] + last[axis + 2]) - first[axis];
    let total: f32 = rs.iter().map(|(_, r)| r[axis + 2]).sum();
    let gap = (span - total) / (rs.len() - 1) as f32;
    let mut at = first[axis];
    let mut moves = Vec::new();
    for (id, r) in &rs {
        let mut pos = [r[0], r[1]];
        pos[axis] = at;
        moves.push((id.clone(), pos));
        at += r[axis + 2] + gap;
    }
    apply_moves(s, "Distribute", moves)
}

fn apply_moves(s: &mut Session, label: &str, moves: Vec<(Item, [f32; 2])>) -> Result<Value> {
    if moves.iter().any(|(_, p)| !p[0].is_finite() || !p[1].is_finite()) {
        return Err(SessionError::Params("the arrangement leaves the canvas (positions not finite)".into()));
    }
    s.edit(label, |g, _| {
        for (item, pos) in &moves {
            match item {
                Item::Node(id) => g.move_node(id, *pos)?,
                // a group's members go with it
                Item::Group(id) => g.move_group(id, *pos)?,
            }
        }
        Ok(())
    })?;
    let moved: serde_json::Map<String, Value> = moves
        .into_iter()
        .map(|(item, p)| match item {
            Item::Node(id) => (id.0, json!(p)),
            Item::Group(id) => (id.0, json!(p)),
        })
        .collect();
    Ok(json!({"moved": moved}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "edit.undo",
            label: "Undo",
            menu: &["Edit"],
            shortcut: Some("Cmd+Z"),
            params: "{} -> {undid: label, undo: label|null, redo: label|null} (node moves and views aren't undo steps)",
            enabled: can_undo,
            run: undo,
            journal: true,
        },
        CommandSpec {
            id: "edit.redo",
            label: "Redo",
            menu: &["Edit"],
            shortcut: Some("Cmd+Shift+Z"),
            params: "{} -> {redid: label, undo: label|null, redo: label|null}",
            enabled: can_redo,
            run: redo,
            journal: true,
        },
        CommandSpec {
            id: "edit.copy",
            label: "Copy",
            menu: &[],
            shortcut: None,
            params: "{nodes: [node id]} -> {text: workflow JSON of the nodes and the wires among them, nodes, wires} (changes nothing; the UI puts the text on the clipboard, Cmd+C)",
            enabled: always,
            run: copy,
            journal: false,
        },
        CommandSpec {
            id: "edit.cut",
            label: "Cut",
            menu: &[],
            shortcut: None,
            params: "{nodes: [node id]} -> {text, nodes, wires} (as edit.copy, then the nodes are removed with their wires; one undo step, Cmd+X)",
            enabled: always,
            run: cut,
            journal: true,
        },
        CommandSpec {
            id: "edit.paste",
            label: "Paste",
            menu: &[],
            shortcut: None,
            params: "{text: workflow JSON (from edit.copy, or any workflow file), pos?: [x, y] (where the pasted nodes' top-left goes; default: 40 px from where they were copied)} -> {nodes: [new id], map: {old: new}, dropped: [{wire, reason}]} (one undo step, Cmd+V)",
            enabled: always,
            run: paste,
            journal: true,
        },
        CommandSpec {
            id: "edit.align",
            label: "Align",
            menu: &[],
            shortcut: None,
            params: "{nodes: [node id], groups?: [collapsed group id] (2 or more in all), edge: left|right|top|bottom|centerX|centerY, sizes?: {id: [w, h]} (a group by its id; a node with no size is estimated as the canvas draws it)} -> {moved: {id: [x, y]}} (one undo step)",
            enabled: always,
            run: align,
            journal: true,
        },
        CommandSpec {
            id: "edit.distribute",
            label: "Distribute",
            menu: &[],
            shortcut: None,
            params: "{nodes: [node id], groups?: [collapsed group id] (3 or more in all), axis: horizontal|vertical, sizes?: {id: [w, h]}} -> {moved: {id: [x, y]}} (the outermost stay; the gaps between neighbours become equal; one undo step)",
            enabled: always,
            run: distribute,
            journal: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session::new(plumcraft_text::engine().schema())
    }

    fn add(s: &mut Session, x: f32, y: f32) -> String {
        s.execute("node.add", &json!({"kind": "text.upper", "pos": [x, y]})).unwrap()["id"].as_str().unwrap().to_owned()
    }

    fn pos(s: &Session, id: &str) -> [f32; 2] {
        s.graph().node(&NodeId::from(id)).unwrap().pos
    }

    #[test]
    fn align_and_distribute_undo() {
        let mut s = session();
        let a = add(&mut s, 0.0, 0.0);
        let b = add(&mut s, 100.0, 40.0);
        let c = add(&mut s, 500.0, 10.0);
        let sizes = json!({a.clone(): [100.0, 50.0], b.clone(): [200.0, 80.0], c.clone(): [100.0, 20.0]});
        s.execute("edit.align", &json!({"nodes": [a, b, c], "edge": "top"})).unwrap();
        assert!([&a, &b, &c].iter().all(|id| pos(&s, id)[1] == 0.0));
        assert_eq!(s.undo_label(), Some("Align"));
        s.execute("edit.undo", &Value::Null).unwrap();
        assert_eq!(pos(&s, &b), [100.0, 40.0], "undo puts them back");
        s.execute("edit.align", &json!({"nodes": [a, b, c], "edge": "right", "sizes": sizes})).unwrap();
        assert_eq!((pos(&s, &a)[0], pos(&s, &b)[0], pos(&s, &c)[0]), (500.0, 400.0, 500.0), "right edges at 600");
        s.execute("edit.undo", &Value::Null).unwrap();
        s.execute("edit.align", &json!({"nodes": [a, b, c], "edge": "bottom", "sizes": sizes})).unwrap();
        assert_eq!((pos(&s, &a)[1], pos(&s, &b)[1], pos(&s, &c)[1]), (70.0, 40.0, 100.0), "bottom edges at 120");
        s.execute("edit.undo", &Value::Null).unwrap();
        s.execute("edit.align", &json!({"nodes": [a, b, c], "edge": "left"})).unwrap();
        assert_eq!((pos(&s, &a)[0], pos(&s, &b)[0], pos(&s, &c)[0]), (0.0, 0.0, 0.0));
        s.execute("edit.undo", &Value::Null).unwrap();
        s.execute("edit.align", &json!({"nodes": [a, b], "edge": "centerX", "sizes": sizes})).unwrap();
        // centres 50 and 200 -> 125
        assert_eq!((pos(&s, &a)[0], pos(&s, &b)[0]), (75.0, 25.0));
        s.execute("edit.undo", &Value::Null).unwrap();
        s.execute("edit.align", &json!({"nodes": [a, b], "edge": "centerY", "sizes": sizes})).unwrap();
        // centres 25 and 80 -> 52.5
        assert_eq!((pos(&s, &a)[1], pos(&s, &b)[1]), (27.5, 12.5));
        s.execute("edit.undo", &Value::Null).unwrap();

        // Distribute: a at 0..100, b 100..300, c 500..600: span 600, total 400, gaps 100.
        s.execute("edit.distribute", &json!({"nodes": [c, a, b], "axis": "horizontal", "sizes": sizes})).unwrap();
        assert_eq!((pos(&s, &a)[0], pos(&s, &b)[0], pos(&s, &c)[0]), (0.0, 200.0, 500.0));
        assert_eq!(pos(&s, &b)[1], 40.0, "the other axis is kept");
        assert_eq!(s.undo_label(), Some("Distribute"));
        s.execute("edit.undo", &Value::Null).unwrap();
        assert_eq!(pos(&s, &b), [100.0, 40.0]);
        // Vertically: a 0..50, c 10..30, b 40..120: span 120, total 150, gaps -15.
        s.execute("edit.distribute", &json!({"nodes": [a, b, c], "axis": "vertical", "sizes": sizes})).unwrap();
        assert_eq!((pos(&s, &a)[1], pos(&s, &c)[1], pos(&s, &b)[1]), (0.0, 35.0, 40.0));
        s.execute("edit.undo", &Value::Null).unwrap();

        // Positions don't make anything stale.
        assert!(s.run.nodes.values().all(|n| n.stale_since.is_none()));
        // Refusals.
        let before = s.graph().clone();
        for (id, p) in [
            ("edit.align", json!({"nodes": [a], "edge": "top"})),
            ("edit.align", json!({"nodes": [a, a], "edge": "top"})),
            ("edit.align", json!({"nodes": [a, b], "edge": "middle"})),
            ("edit.distribute", json!({"nodes": [a, b], "axis": "x"})),
            ("edit.distribute", json!({"nodes": [a, b, c], "axis": "z"})),
            ("edit.align", json!({"nodes": [a, "n99"], "edge": "top"})),
            ("edit.align", json!({"nodes": [a], "groups": ["g9"], "edge": "top"})),
            ("edit.align", json!({"nodes": [a, b], "edge": "top", "sizes": {"n1": [-1, 2]}})),
            ("edit.align", json!({"nodes": [a, b], "edge": "right", "sizes": {"n1": [3.0e38, 2], "n2": [1e39, 2]}})),
            ("edit.distribute", json!({"nodes": [a, b, c], "axis": "x", "sizes": []})),
        ] {
            assert!(s.execute(id, &p).is_err(), "{id} {p}");
            assert_eq!(s.graph(), &before, "{id} {p}");
        }
    }

    #[test]
    fn a_node_with_no_size_is_not_a_point() {
        let mut s = session();
        let wide = s.execute("node.add", &json!({"kind": "text.replace", "pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
        let narrow = add(&mut s, 900.0, 0.0);
        s.execute("edit.align", &json!({"nodes": [wide, narrow], "edge": "right"})).unwrap();
        assert!(pos(&s, &wide)[0] != pos(&s, &narrow)[0], "right edges line up, not left ones");
    }

    #[test]
    fn a_collapsed_group_lines_up_with_its_members() {
        let mut s = session();
        let a = add(&mut s, 0.0, 0.0);
        let b = add(&mut s, 300.0, 500.0);
        let c = add(&mut s, 900.0, 100.0);
        let g = s.execute("group.create", &json!({"nodes": [b], "name": "G"})).unwrap()["id"].as_str().unwrap().to_owned();
        s.execute("group.collapse", &json!({"group": g, "collapsed": true})).unwrap();
        s.execute("edit.align", &json!({"nodes": [a], "groups": [g], "edge": "top", "sizes": {g.as_str(): [200, 80]}})).unwrap();
        assert_eq!(s.graph().group(&GroupId::from(g.as_str())).unwrap().pos, [300.0, 0.0]);
        assert_eq!(pos(&s, &b), [300.0, 0.0], "its member goes with it");
        assert_eq!(s.undo_label(), Some("Align"));
        s.execute("edit.undo", &Value::Null).unwrap();
        assert_eq!(pos(&s, &b), [300.0, 500.0]);
        let sizes = json!({a.clone(): [100, 10], c.clone(): [100, 10], g.clone(): [100, 10]});
        s.execute("edit.distribute", &json!({"nodes": [a, c], "groups": [g], "axis": "horizontal", "sizes": sizes})).unwrap();
        assert_eq!(s.graph().group(&GroupId::from(g.as_str())).unwrap().pos, [450.0, 500.0]);
        assert_eq!(pos(&s, &b), [450.0, 500.0]);
    }
}
