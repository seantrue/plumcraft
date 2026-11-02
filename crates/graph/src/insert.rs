//! Inserting one graph into another (`graph.insert`: File › Insert Graph… and Insert as
//! Group…). Everything in the inserted graph comes across under fresh ids: its nodes, the wires
//! among them (with slices, bend points and net labels, a taken net name renamed), its groups (nesting,
//! promoted parameters, outputs and checkpoints kept) and its frames, all moved by one offset.
//!
//! Lifted from the research software's `crates/graph/src/insert.rs` (2026-10-27, the research software `8d96021`; plan
//! 10.3, E3).

use std::collections::BTreeMap;

use plumcraft_proto::catalogue::SchemaReply;

use crate::{FrameId, Graph, GraphError, Group, GroupId, GroupOutput, NodeId, PortRef, Promoted, Result, Wire};

/// What [`Graph::insert_graph`] added.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inserted {
    /// The inserted graph's node ids -> their new ids.
    pub nodes: BTreeMap<NodeId, NodeId>,
    /// Its group ids -> their new ids.
    pub groups: BTreeMap<GroupId, GroupId>,
    /// Its frames' new ids.
    pub frames: Vec<FrameId>,
    /// Wires that no longer type-check here, with why.
    pub dropped: Vec<(Wire, String)>,
    /// Nets renamed because their names were taken here: old -> new.
    pub nets: BTreeMap<String, String>,
}

impl Inserted {
    /// The new groups that sit at the inserted graph's top level, and the new nodes that are in
    /// none of its groups: what a wrapping group takes in.
    pub fn top_level(&self, other: &Graph) -> (Vec<NodeId>, Vec<GroupId>) {
        let nodes = self.nodes.iter().filter(|(old, _)| other.group_of(old).is_none()).map(|(_, new)| new.clone()).collect();
        let groups = self.groups.iter().filter(|(old, _)| other.groups.get(*old).is_some_and(|g| g.parent.is_none())).map(|(_, new)| new.clone()).collect();
        (nodes, groups)
    }
}

impl Graph {
    /// Add `other` (a checked graph) to this one, moved by `shift` (finite: the caller checks).
    /// A node type the catalog doesn't know refuses the whole insert; call it inside an edit so
    /// that undoes it.
    pub fn insert_graph(&mut self, catalog: &SchemaReply, other: &Graph, shift: [f32; 2]) -> Result<Inserted> {
        let moved = |p: [f32; 2]| [p[0] + shift[0], p[1] + shift[1]];
        let mut out = Inserted::default();
        for (old, node) in &other.nodes {
            if catalog.node(&node.kind).is_none() {
                return Err(GraphError::UnknownKind(node.kind.clone()));
            }
            let mut node = node.clone();
            node.pos = moved(node.pos);
            if let Some(def) = catalog.node(&node.kind) {
                crate::ports::drop_filled(def, &mut node);
            }
            out.nodes.insert(old.clone(), self.insert_node(node));
        }
        let node = |n: &NodeId| out.nodes.get(n).cloned().ok_or_else(|| GraphError::UnknownNode(n.clone()));
        let port = |p: &PortRef| -> Result<PortRef> { Ok(PortRef::new(node(&p.node)?, p.port.clone())) };
        for w in &other.wires {
            let (from, to) = (port(&w.from)?, port(&w.to)?);
            match self.connect_sliced(catalog, from, to.clone(), w.slice.clone()) {
                // bend points move with the copies
                Ok(_) if !w.via.is_empty() => {
                    let by = |c: i32, d: f32| c.saturating_add(d.round().clamp(-1e9, 1e9) as i32);
                    let shifted: Vec<[i32; 2]> = w.via.iter().map(|v| [by(v[0], shift[0]), by(v[1], shift[1])]).collect();
                    // a shift that would take them off the canvas drops them, not the wire
                    let _ = self.set_via(&to, shifted);
                }
                Ok(_) => {}
                Err(e) => out.dropped.push((w.clone(), e.to_string())),
            }
        }
        // Groups, parents before children, so each parent's new id is known.
        let mut todo: Vec<&GroupId> = other.groups.keys().collect();
        while !todo.is_empty() {
            let before = todo.len();
            let mut later = Vec::new();
            for old in todo {
                let Some(g) = other.groups.get(old) else { continue };
                let parent = match &g.parent {
                    None => None,
                    Some(p) => match out.groups.get(p) {
                        Some(new) => Some(new.clone()),
                        None => {
                            later.push(old);
                            continue;
                        }
                    },
                };
                let group = Group {
                    name: g.name.clone(),
                    nodes: g.nodes.iter().map(&node).collect::<Result<_>>()?,
                    parent,
                    promoted: g.promoted.iter().map(|p| Ok(Promoted { port: port(&p.port)?, ..p.clone() })).collect::<Result<_>>()?,
                    outputs: g.outputs.iter().map(|o| Ok(GroupOutput { from: port(&o.from)?, name: o.name.clone() })).collect::<Result<_>>()?,
                    checkpoints: g.checkpoints.iter().map(&node).collect::<Result<_>>()?,
                    collapsed: g.collapsed,
                    pos: moved(g.pos),
                    nets: BTreeMap::new(),
                    extra: BTreeMap::new(),
                };
                let id = self.next_group_id();
                self.groups.insert(id.clone(), group);
                out.groups.insert(old.clone(), id);
            }
            if later.len() == before {
                // a parent missing from the file (check_groups refuses that before we get here)
                return Err(GraphError::GroupInvariant("an inserted group's parent is missing".into()));
            }
            todo = later;
        }
        out.nets = self.adopt_nets(other, &out.nodes, &out.groups);
        for f in other.frames.values() {
            let id = self.add_frame(&f.title, moved(f.pos), f.size)?;
            // as the file has it: its colour, its fields, a size under the editor's minimum
            if let Some(frame) = self.frames.get_mut(&id) {
                *frame = crate::Frame { title: frame.title.clone(), pos: frame.pos, ..f.clone() };
            }
            out.frames.push(id);
        }
        self.touch();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::catalog;

    /// literal -> slow -> upper, literal -> words; slow and upper in "inner", nested in "outer"
    /// (with words), a promoted port and an output.
    fn source(c: &SchemaReply) -> Graph {
        let mut g = Graph::new();
        let lit = g.add_node(c.node("text.literal").unwrap(), [0.0, 0.0]);
        let words = g.add_node(c.node("text.words").unwrap(), [300.0, 0.0]);
        let slow = g.add_node(c.node("text.slow").unwrap(), [600.0, 0.0]);
        let up = g.add_node(c.node("text.upper").unwrap(), [900.0, 0.0]);
        g.connect(c, PortRef::new(lit.clone(), "text"), PortRef::new(slow.clone(), "text")).unwrap();
        g.connect(c, PortRef::new(slow.clone(), "text"), PortRef::new(up.clone(), "text")).unwrap();
        g.connect(c, PortRef::new(lit.clone(), "text"), PortRef::new(words.clone(), "text")).unwrap();
        let inner = g.create_group("inner", &[slow.clone(), up.clone()], &[]).unwrap();
        let outer = g.create_group("outer", std::slice::from_ref(&words), std::slice::from_ref(&inner)).unwrap();
        g.promote(c, &outer, PortRef::new(slow.clone(), "seconds"), "seconds", "how long").unwrap();
        g.add_group_output(c, &inner, PortRef::new(up.clone(), "text"), "shout").unwrap();
        g.groups.get_mut(&inner).unwrap().checkpoints.insert(up.clone());
        g
    }

    #[test]
    fn frames_come_across_moved_as_the_file_has_them() {
        let c = catalog();
        let mut other = source(&c);
        let f = other.add_frame("sources", [10.0, 20.0], [300.0, 200.0]).unwrap();
        other.set_frame(&f, None, Some("#336699")).unwrap();
        let frame = other.frames.get_mut(&f).unwrap();
        frame.size = [40.0, 200.0];
        frame.extra.insert("locked".into(), true.into());
        let mut g = source(&c);
        let taken = g.add_frame("here", [0.0, 0.0], [100.0, 100.0]).unwrap();
        let ins = g.insert_graph(&c, &other, [5.0, 1000.0]).unwrap();
        assert_eq!(ins.frames.len(), 1);
        assert_ne!(ins.frames[0], taken, "a fresh id");
        let back = g.frame(&ins.frames[0]).unwrap();
        assert_eq!(back, &crate::Frame { pos: [15.0, 1020.0], ..other.frames[&f].clone() });
        assert_eq!(g.frame(&taken).unwrap().title, "here");
        // a shift that would take a frame off the canvas refuses the insert
        assert!(g.insert_graph(&c, &other, [3.0e7, 0.0]).is_err());
    }

    #[test]
    fn a_graph_is_inserted_whole_under_fresh_ids() {
        let c = catalog();
        let other = source(&c);
        let mut g = source(&c);
        let before = g.clone();
        let ins = g.insert_graph(&c, &other, [0.0, 1000.0]).unwrap();
        assert!(ins.dropped.is_empty(), "{:?}", ins.dropped);
        assert_eq!((g.nodes.len(), g.wires.len(), g.groups.len()), (8, 6, 4));
        for new in ins.nodes.values() {
            assert!(!before.nodes.contains_key(new), "fresh id {new}");
        }
        let slow = &ins.nodes[&NodeId::from("n3")];
        assert_eq!(g.node(slow).unwrap().pos, [600.0, 1000.0]);
        // the groups, nested as before, with their parameters and outputs
        let inner = &g.groups[&ins.groups[&GroupId::from("g1")]];
        let outer_id = &ins.groups[&GroupId::from("g2")];
        assert_eq!(inner.parent.as_ref(), Some(outer_id));
        assert_eq!(inner.outputs[0].from, PortRef::new(ins.nodes[&NodeId::from("n4")].clone(), "text"));
        assert_eq!(g.groups[outer_id].promoted[0].doc, "how long");
        assert_eq!(g.groups[outer_id].promoted[0].port.node, *slow);
        assert_eq!(inner.checkpoints, [ins.nodes[&NodeId::from("n4")].clone()].into(), "a mark follows its node to its new id");
        g.check(&c).unwrap();
        // what a wrapping group would take: the literal (in no group) and outer (top level)
        let (nodes, groups) = ins.top_level(&other);
        assert_eq!(nodes, [ins.nodes[&NodeId::from("n1")].clone()]);
        assert_eq!(groups.as_slice(), std::slice::from_ref(outer_id));
    }

    #[test]
    fn an_unknown_node_type_refuses_the_insert() {
        let c = catalog();
        let mut other = source(&c);
        if let Some(n) = other.nodes.values_mut().next() {
            n.kind = "text.teleport".into();
        }
        let mut g = Graph::new();
        assert!(matches!(g.insert_graph(&c, &other, [0.0, 0.0]), Err(GraphError::UnknownKind(_))));
    }
}
