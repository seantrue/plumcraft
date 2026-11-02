//! Groups: an overlay on the flat graph. Nodes stay in [`Graph::nodes`]; a group records which
//! nodes belong to it, its parent group (groups nest as a tree), the inner inputs it promotes to
//! its surface, the inner outputs it exposes, which inner nodes are marked for export
//! (checkpoints), and how it is drawn (collapsed or open, where).
//!
//! Groups are an editing overlay saved in the workflow file. Engines run the flat graph and
//! ignore them (spec 7.6 rule 8): nothing here changes what a run computes.
//!
//! **Membership:** `Group::nodes` holds a group's *direct* members, and a node is a direct member
//! of at most one group (its innermost). A group's members are its direct members plus those of
//! every group nested in it ([`Graph::group_members`]); promotions, outputs and checkpoints may
//! name any member.
//!
//! **Names:** a group's name, its promoted labels and its output names are what the designer
//! reads, so any text of 1 to [`crate::MAX_TITLE_CHARS`] characters without control characters
//! will do ([`check_name`]); labels and output names are unique in their group. (the research software asks
//! for Python identifiers, because its groups become Cartooner plugins; that stays in its pack.)
//!
//! Lifted from the research software's `crates/graph/src/groups.rs` (2026-10-27, the research software `8d96021`; plan
//! 10.3, E3) without net labels. **Checkpoints** are a mark and nothing more: what a pack's export
//! makes of a marked node is the pack's, the catalogue says which node types take one
//! (`export.markable`), and a run ignores them. Being only a mark, a stale one (a file's, naming a
//! node that isn't a member) doesn't refuse the file; it is kept like anything else unknown.

use std::collections::{BTreeMap, BTreeSet};

use plumcraft_proto::catalogue::{SchemaReply, scalars};
use serde::{Deserialize, Serialize};

use crate::ports::resolve_input;
use crate::{Graph, GraphError, MAX_TITLE_CHARS, NodeId, PortRef, Result, Wire};

/// A group's id in its graph (`g1`, `g2`, …).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupId(pub String);

impl std::fmt::Display for GroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for GroupId {
    fn from(s: &str) -> Self {
        GroupId(s.to_owned())
    }
}

/// An inner input promoted to the group's surface: a collapsed group shows its widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Promoted {
    pub port: PortRef,
    /// The parameter's name on the group, unique in it.
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub doc: String,
    /// The group parameter's own range (numeric ports), inside the inner port's range. `None`
    /// keeps the inner port's bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

/// Changes to a promoted parameter ([`Graph::edit_promoted`]). `None` leaves a field alone;
/// for `min` and `max`, `Some(None)` clears the bound.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PromotedEdit {
    pub label: Option<String>,
    pub doc: Option<String>,
    pub min: Option<Option<f64>>,
    pub max: Option<Option<f64>>,
}

/// An inner output exposed as one of the group's results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupOutput {
    pub from: PortRef,
    /// The result's name, unique in the group.
    pub name: String,
}

/// A group of nodes. Its id is its key in [`Graph::groups`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    /// Direct members.
    #[serde(default)]
    pub nodes: BTreeSet<NodeId>,
    /// The enclosing group, if nested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<GroupId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub promoted: Vec<Promoted>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<GroupOutput>,
    /// Members marked for export (`export.markable` in the catalogue).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub checkpoints: BTreeSet<NodeId>,
    /// Shown as one node (its proxy) rather than its members in a frame.
    #[serde(default)]
    pub collapsed: bool,
    /// Where the collapsed group's node sits.
    #[serde(default)]
    pub pos: [f32; 2],
    /// The nets local to this group, by name ([`crate::nets`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub nets: BTreeMap<String, crate::Net>,
    /// Fields this editor doesn't know, kept for the file.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Refuse `name` unless it is 1 to [`MAX_TITLE_CHARS`] characters, not blank, without control
/// characters; `what` says what it names.
pub fn check_name(what: &'static str, name: &str) -> Result<()> {
    let ok = !name.trim().is_empty() && name.chars().count() <= MAX_TITLE_CHARS && !name.chars().any(char::is_control);
    if ok { Ok(()) } else { Err(GraphError::BadName { what, name: name.chars().take(64).collect() }) }
}

/// Is `name` a good group, label or output name ([`check_name`])?
pub fn is_name(name: &str) -> bool {
    check_name("name", name).is_ok()
}

impl Graph {
    pub fn group(&self, id: &GroupId) -> Option<&Group> {
        self.groups.get(id)
    }

    fn group_mut(&mut self, id: &GroupId) -> Result<&mut Group> {
        self.groups.get_mut(id).ok_or_else(|| GraphError::UnknownGroup(id.clone()))
    }

    /// The innermost group `node` belongs to.
    pub fn group_of(&self, node: &NodeId) -> Option<&GroupId> {
        self.groups.iter().find(|(_, g)| g.nodes.contains(node)).map(|(id, _)| id)
    }

    /// Groups nested directly in `id`.
    pub fn child_groups(&self, id: &GroupId) -> Vec<GroupId> {
        self.groups.iter().filter(|(_, g)| g.parent.as_ref() == Some(id)).map(|(k, _)| k.clone()).collect()
    }

    /// `id` and every group nested in it, at any depth.
    pub fn group_subtree(&self, id: &GroupId) -> BTreeSet<GroupId> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![id.clone()];
        while let Some(g) = stack.pop() {
            if self.groups.contains_key(&g) && seen.insert(g.clone()) {
                stack.extend(self.child_groups(&g));
            }
        }
        seen
    }

    /// Every node in `id`, directly or through nested groups.
    pub fn group_members(&self, id: &GroupId) -> BTreeSet<NodeId> {
        self.group_subtree(id).iter().filter_map(|g| self.groups.get(g)).flat_map(|g| g.nodes.iter().cloned()).collect()
    }

    /// The groups `node` is in, innermost first.
    pub fn enclosing_groups(&self, node: &NodeId) -> Vec<GroupId> {
        let mut out = Vec::new();
        let mut at = self.group_of(node).cloned();
        while let Some(g) = at {
            if out.contains(&g) {
                break;
            }
            at = self.group(&g).and_then(|grp| grp.parent.clone());
            out.push(g);
        }
        out
    }

    /// A group name not used yet: `group 1`, `group 2`, …
    pub fn fresh_group_name(&self) -> String {
        let taken: BTreeSet<&str> = self.groups.values().map(|g| g.name.as_str()).collect();
        (1u64..=u64::from(u32::MAX)).map(|i| format!("group {i}")).find(|n| !taken.contains(n.as_str())).unwrap_or_else(|| "group".to_owned())
    }

    pub(crate) fn next_group_id(&mut self) -> GroupId {
        let mut id = GroupId(format!("g{}", self.next_group));
        while self.groups.contains_key(&id) {
            // (past the last number, start again: a file can hold that very id)
            self.next_group = self.next_group.checked_add(1).unwrap_or(1);
            id = GroupId(format!("g{}", self.next_group));
        }
        self.next_group = self.next_group.saturating_add(1);
        id
    }

    /// Group `nodes` and nested `groups` under a new group called `name`. Everything picked must
    /// sit in the same enclosing group (or in none), which becomes the new group's parent.
    pub fn create_group(&mut self, name: &str, nodes: &[NodeId], groups: &[GroupId]) -> Result<GroupId> {
        check_name("group name", name)?;
        let nodes: BTreeSet<NodeId> = nodes.iter().cloned().collect();
        let groups: BTreeSet<GroupId> = groups.iter().cloned().collect();
        if nodes.is_empty() && groups.is_empty() {
            return Err(GraphError::EmptyGroup);
        }
        let mut containers = BTreeSet::new();
        for n in &nodes {
            if !self.nodes.contains_key(n) {
                return Err(GraphError::UnknownNode(n.clone()));
            }
            containers.insert(self.group_of(n).cloned());
        }
        for g in &groups {
            containers.insert(self.groups.get(g).ok_or_else(|| GraphError::UnknownGroup(g.clone()))?.parent.clone());
        }
        let mut it = containers.into_iter();
        let parent = it.next().flatten();
        if it.next().is_some() {
            return Err(GraphError::MixedGroups);
        }
        if let Some(p) = &parent
            && let Some(pg) = self.groups.get_mut(p)
        {
            pg.nodes.retain(|n| !nodes.contains(n));
        }
        // where the collapsed node goes: the top-left of what it holds
        let pos = {
            let mut ps: Vec<[f32; 2]> = nodes.iter().filter_map(|n| self.nodes.get(n)).map(|n| n.pos).collect();
            ps.extend(groups.iter().flat_map(|g| self.group_members(g)).filter_map(|n| self.nodes.get(&n).map(|n| n.pos)));
            let fold = |i: usize| ps.iter().filter_map(|p| p.get(i).copied()).fold(f32::INFINITY, f32::min);
            let p = [fold(0), fold(1)];
            if p[0].is_finite() && p[1].is_finite() { p } else { [0.0, 0.0] }
        };
        let id = self.next_group_id();
        for g in &groups {
            if let Some(child) = self.groups.get_mut(g) {
                child.parent = Some(id.clone());
            }
        }
        self.groups.insert(
            id.clone(),
            Group {
                name: name.to_owned(),
                nodes,
                parent,
                promoted: Vec::new(),
                outputs: Vec::new(),
                checkpoints: BTreeSet::new(),
                collapsed: false,
                pos,
                nets: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
        );
        self.touch();
        Ok(id)
    }

    /// Dissolve group `id`: its direct members and nested groups move to its parent. Its
    /// promotions and outputs go with it.
    pub fn ungroup(&mut self, id: &GroupId) -> Result<Group> {
        let g = self.groups.remove(id).ok_or_else(|| GraphError::UnknownGroup(id.clone()))?;
        if let Some(p) = &g.parent
            && let Some(pg) = self.groups.get_mut(p)
        {
            pg.nodes.extend(g.nodes.iter().cloned());
        }
        for child in self.groups.values_mut() {
            if child.parent.as_ref() == Some(id) {
                child.parent.clone_from(&g.parent);
            }
        }
        self.lift_nets(g.nets.clone(), &g.parent);
        self.touch();
        Ok(g)
    }

    pub fn rename_group(&mut self, id: &GroupId, name: &str) -> Result<()> {
        check_name("group name", name)?;
        let g = self.group_mut(id)?;
        if g.name != name {
            g.name = name.to_owned();
            self.touch();
        }
        Ok(())
    }

    fn require_member(&self, id: &GroupId, node: &NodeId) -> Result<()> {
        if !self.groups.contains_key(id) {
            return Err(GraphError::UnknownGroup(id.clone()));
        }
        if !self.nodes.contains_key(node) {
            return Err(GraphError::UnknownNode(node.clone()));
        }
        if self.group_members(id).contains(node) { Ok(()) } else { Err(GraphError::NotMember { group: id.clone(), node: node.clone() }) }
    }

    /// Promote inner input `port` (a port, a variadic slot or an active sub-port of a member)
    /// to the group's surface as parameter `label`.
    pub fn promote(&mut self, catalog: &SchemaReply, id: &GroupId, port: PortRef, label: &str, doc: &str) -> Result<()> {
        check_name("promoted label", label)?;
        if doc.chars().count() > crate::MAX_DOC_CHARS {
            return Err(GraphError::BadName { what: "promoted doc (too long)", name: doc.chars().take(64).collect() });
        }
        self.require_member(id, &port.node)?;
        let node = self.nodes.get(&port.node).ok_or_else(|| GraphError::UnknownNode(port.node.clone()))?;
        let def = catalog.node(&node.kind).ok_or_else(|| GraphError::UnknownKind(node.kind.clone()))?;
        resolve_input(def, node, &port.node, &port.port)?;
        let g = self.group_mut(id)?;
        if g.promoted.iter().any(|p| p.label == label) {
            return Err(GraphError::Duplicate { what: "promoted label", name: label.to_owned(), group: id.clone() });
        }
        if g.promoted.iter().any(|p| p.port == port) {
            return Err(GraphError::Duplicate { what: "promoted port", name: port.to_string(), group: id.clone() });
        }
        g.promoted.push(Promoted { port, label: label.to_owned(), doc: doc.to_owned(), min: None, max: None });
        self.touch();
        Ok(())
    }

    /// Rename promoted parameter `label`, or change its doc or range. A range must be finite,
    /// `min <= max`, on a numeric port, and inside the inner port's own range.
    pub fn edit_promoted(&mut self, catalog: &SchemaReply, id: &GroupId, label: &str, edit: &PromotedEdit) -> Result<()> {
        let g = self.group(id).ok_or_else(|| GraphError::UnknownGroup(id.clone()))?;
        let no_such = || GraphError::NoSuch { what: "promoted label", name: label.chars().take(64).collect(), group: id.clone() };
        let current = g.promoted.iter().find(|p| p.label == label).ok_or_else(no_such)?.clone();
        if let Some(new) = &edit.label
            && new != label
        {
            check_name("promoted label", new)?;
            if g.promoted.iter().any(|p| &p.label == new) {
                return Err(GraphError::Duplicate { what: "promoted label", name: new.clone(), group: id.clone() });
            }
        }
        if let Some(doc) = &edit.doc
            && doc.chars().count() > crate::MAX_DOC_CHARS
        {
            return Err(GraphError::BadName { what: "promoted doc (too long)", name: doc.chars().take(64).collect() });
        }
        let (min, max) = (edit.min.unwrap_or(current.min), edit.max.unwrap_or(current.max));
        if min.is_some() || max.is_some() {
            let bad = |reason: String| GraphError::BadRange { label: label.chars().take(64).collect(), reason };
            let node = self.nodes.get(&current.port.node).ok_or_else(|| GraphError::UnknownNode(current.port.node.clone()))?;
            let def = catalog.node(&node.kind).ok_or_else(|| GraphError::UnknownKind(node.kind.clone()))?;
            let port = resolve_input(def, node, &current.port.node, &current.port.port)?.port;
            if port.ty != scalars::FLOAT && port.ty != scalars::INT {
                return Err(bad(format!("only numbers have a range ({} is {})", current.port, port.ty)));
            }
            for (name, v) in [("min", min), ("max", max)] {
                let Some(v) = v else { continue };
                if !v.is_finite() {
                    return Err(bad(format!("{name} must be a finite number")));
                }
                if port.min.is_some_and(|m| v < m) || port.max.is_some_and(|m| v > m) {
                    let fmt = |b: Option<f64>| b.map_or_else(|| "…".to_owned(), |b| b.to_string());
                    return Err(bad(format!("{name} {v} is outside {}'s range {}..{}", current.port, fmt(port.min), fmt(port.max))));
                }
            }
            if let (Some(lo), Some(hi)) = (min, max)
                && lo > hi
            {
                return Err(bad(format!("min {lo} is above max {hi}")));
            }
        }
        let g = self.group_mut(id)?;
        let p = g.promoted.iter_mut().find(|p| p.label == label).ok_or_else(no_such)?;
        if let Some(new) = &edit.label {
            new.clone_into(&mut p.label);
        }
        if let Some(doc) = &edit.doc {
            doc.clone_into(&mut p.doc);
        }
        p.min = min;
        p.max = max;
        self.touch();
        Ok(())
    }

    /// Remove the promotion called `label`.
    pub fn unpromote(&mut self, id: &GroupId, label: &str) -> Result<Promoted> {
        let g = self.group_mut(id)?;
        let i = g.promoted.iter().position(|p| p.label == label).ok_or_else(|| GraphError::NoSuch {
            what: "promoted label",
            name: label.chars().take(64).collect(),
            group: id.clone(),
        })?;
        let p = g.promoted.remove(i);
        self.touch();
        Ok(p)
    }

    /// Expose inner output `from` as the group's result `name`.
    pub fn add_group_output(&mut self, catalog: &SchemaReply, id: &GroupId, from: PortRef, name: &str) -> Result<()> {
        check_name("output name", name)?;
        self.require_member(id, &from.node)?;
        let node = self.nodes.get(&from.node).ok_or_else(|| GraphError::UnknownNode(from.node.clone()))?;
        let def = catalog.node(&node.kind).ok_or_else(|| GraphError::UnknownKind(node.kind.clone()))?;
        if def.output(&from.port).is_none() {
            return Err(GraphError::UnknownPort { node: from.node.clone(), kind: def.id.clone(), dir: "output", port: from.port.clone() });
        }
        let g = self.group_mut(id)?;
        if g.outputs.iter().any(|o| o.name == name) {
            return Err(GraphError::Duplicate { what: "output name", name: name.to_owned(), group: id.clone() });
        }
        if g.outputs.iter().any(|o| o.from == from) {
            return Err(GraphError::Duplicate { what: "output", name: from.to_string(), group: id.clone() });
        }
        g.outputs.push(GroupOutput { from, name: name.to_owned() });
        self.touch();
        Ok(())
    }

    /// Remove the group result called `name`.
    pub fn remove_group_output(&mut self, id: &GroupId, name: &str) -> Result<GroupOutput> {
        let g = self.group_mut(id)?;
        let i = g.outputs.iter().position(|o| o.name == name).ok_or_else(|| GraphError::NoSuch {
            what: "output name",
            name: name.chars().take(64).collect(),
            group: id.clone(),
        })?;
        let o = g.outputs.remove(i);
        self.touch();
        Ok(o)
    }

    /// Mark (or unmark) member `node` of group `id` for export. Only node types the catalogue calls
    /// `export.markable` can be marked; anything can be unmarked.
    pub fn set_checkpoint(&mut self, catalog: &SchemaReply, id: &GroupId, node: &NodeId, on: bool) -> Result<()> {
        if on {
            self.require_member(id, node)?;
            let kind = self.nodes.get(node).map(|n| n.kind.clone()).unwrap_or_default();
            if !catalog.node(&kind).is_some_and(|d| d.export.markable) {
                return Err(GraphError::NotMarkable { node: node.clone(), kind });
            }
        }
        let g = self.group_mut(id)?;
        let changed = if on { g.checkpoints.insert(node.clone()) } else { g.checkpoints.remove(node) };
        if changed {
            self.touch();
        }
        Ok(())
    }

    /// The top-left of the positions of every node in group `id` (`None` when it holds none).
    pub fn group_top_left(&self, id: &GroupId) -> Option<[f32; 2]> {
        let members = self.group_members(id);
        let ps: Vec<[f32; 2]> = members.iter().filter_map(|n| self.nodes.get(n)).map(|n| n.pos).collect();
        let p = ps.iter().fold([f32::INFINITY, f32::INFINITY], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
        (p[0].is_finite() && p[1].is_finite()).then_some(p)
    }

    /// Collapse or open group `id`. Collapsing puts its node where its members' top-left now
    /// is (they may have moved while it was open), so it collapses in place.
    pub fn set_collapsed(&mut self, id: &GroupId, collapsed: bool) -> Result<()> {
        let at = if collapsed { self.group_top_left(id) } else { None };
        let g = self.group_mut(id)?;
        let mut changed = false;
        if g.collapsed != collapsed {
            g.collapsed = collapsed;
            changed = true;
        }
        if let Some(at) = at
            && g.pos != at
        {
            g.pos = at;
            changed = true;
        }
        if changed {
            self.touch();
        }
        Ok(())
    }

    /// Move group `id`'s node to `pos`, and everything in it (its members and the groups
    /// nested in it) by the same offset, so the group keeps its shape and opens where it now
    /// is. A position that isn't finite, or that would move a member to one that isn't, is
    /// refused.
    pub fn move_group(&mut self, id: &GroupId, pos: [f32; 2]) -> Result<()> {
        let was = self.group(id).ok_or_else(|| GraphError::UnknownGroup(id.clone()))?.pos;
        if was == pos {
            return Ok(());
        }
        let delta = [pos[0] - was[0], pos[1] - was[1]];
        let by = |p: [f32; 2]| [p[0] + delta[0], p[1] + delta[1]];
        let finite = |p: [f32; 2]| p[0].is_finite() && p[1].is_finite();
        let members = self.group_members(id);
        let inner: Vec<GroupId> = self.group_subtree(id).into_iter().filter(|g| g != id).collect();
        let all_finite = finite(pos)
            && finite(delta)
            && members.iter().filter_map(|n| self.nodes.get(n)).all(|n| finite(by(n.pos)))
            && inner.iter().filter_map(|g| self.groups.get(g)).all(|g| finite(by(g.pos)));
        if !all_finite {
            return Err(GraphError::Broken(format!("moving group {id} to {pos:?} would put a node off the canvas")));
        }
        // The bend points of wires between members move with them; a wire with an end outside
        // keeps its own.
        let inside = |w: &Wire| members.contains(&w.from.node) && members.contains(&w.to.node);
        let d = [delta[0].round().clamp(-1e9, 1e9) as i32, delta[1].round().clamp(-1e9, 1e9) as i32];
        let shift = |v: [i32; 2]| [v[0].saturating_add(d[0]), v[1].saturating_add(d[1])];
        if self
            .wires
            .iter()
            .filter(|w| inside(w))
            .any(|w| w.via.iter().any(|v| shift(*v).iter().any(|c| c.unsigned_abs() > crate::MAX_VIA_COORD.unsigned_abs())))
        {
            return Err(GraphError::Broken(format!("moving group {id} to {pos:?} would put a bend point off the canvas")));
        }
        for w in self.wires.iter_mut().filter(|w| members.contains(&w.from.node) && members.contains(&w.to.node)) {
            for v in &mut w.via {
                *v = shift(*v);
            }
        }
        for n in &members {
            if let Some(node) = self.nodes.get_mut(n) {
                node.pos = by(node.pos);
            }
        }
        for g in &inner {
            if let Some(grp) = self.groups.get_mut(g) {
                grp.pos = by(grp.pos);
            }
        }
        self.group_mut(id)?.pos = pos;
        self.touch();
        Ok(())
    }

    /// Forget `node` in every group (it is being removed), then drop the groups that are left
    /// holding nothing (no nodes, no nested groups): an empty group has nothing to show or edit.
    pub(crate) fn drop_from_groups(&mut self, node: &NodeId) {
        for g in self.groups.values_mut() {
            g.nodes.remove(node);
            g.promoted.retain(|p| &p.port.node != node);
            g.outputs.retain(|o| &o.from.node != node);
            g.checkpoints.remove(node);
        }
        loop {
            let empty: Vec<GroupId> = self
                .groups
                .iter()
                .filter(|(id, g)| g.nodes.is_empty() && !self.groups.values().any(|c| c.parent.as_ref() == Some(*id)))
                .map(|(id, _)| id.clone())
                .collect();
            if empty.is_empty() {
                break;
            }
            for id in empty {
                self.groups.remove(&id);
            }
        }
    }

    /// Check the group invariants. With a catalog, also check that promotions and outputs name
    /// existing ports (on nodes whose type the catalog knows; an unknown type is reported as
    /// such rather than refusing the file).
    pub fn check_groups(&self, catalog: Option<&SchemaReply>) -> Result<()> {
        let bad = |m: String| Err(GraphError::GroupInvariant(m));
        let mut owner: BTreeMap<&NodeId, &GroupId> = BTreeMap::new();
        for (id, g) in &self.groups {
            check_name("group name", &g.name)?;
            if let Some(p) = &g.parent
                && !self.groups.contains_key(p)
            {
                return bad(format!("group {id} names a missing parent {p}"));
            }
            if !g.pos.iter().all(|c| c.is_finite()) {
                return bad(format!("group {id} has no finite position"));
            }
            for n in &g.nodes {
                if !self.nodes.contains_key(n) {
                    return bad(format!("group {id} names a missing node {n}"));
                }
                if let Some(other) = owner.insert(n, id) {
                    return bad(format!("node {n} is in both {other} and {id}"));
                }
            }
            // nesting is a tree: walking up from any group ends within groups.len() steps
            let mut at = g.parent.as_ref();
            let mut steps = 0usize;
            while let Some(p) = at {
                steps = steps.saturating_add(1);
                if p == id || steps > self.groups.len() {
                    return bad(format!("group {id} is nested in itself"));
                }
                at = self.groups.get(p).and_then(|pg| pg.parent.as_ref());
            }
        }
        for (id, g) in &self.groups {
            let members = self.group_members(id);
            let mut labels = BTreeSet::new();
            let mut ports = BTreeSet::new();
            for p in &g.promoted {
                check_name("promoted label", &p.label)?;
                if !labels.insert(p.label.as_str()) {
                    return Err(GraphError::Duplicate { what: "promoted label", name: p.label.clone(), group: id.clone() });
                }
                if !ports.insert(&p.port) {
                    return Err(GraphError::Duplicate { what: "promoted port", name: p.port.to_string(), group: id.clone() });
                }
                if !members.contains(&p.port.node) {
                    return Err(GraphError::NotMember { group: id.clone(), node: p.port.node.clone() });
                }
                if p.min.is_some_and(|v| !v.is_finite()) || p.max.is_some_and(|v| !v.is_finite()) {
                    return Err(GraphError::BadRange { label: p.label.clone(), reason: "bounds must be finite".into() });
                }
                if let Some(c) = catalog
                    && let Some(node) = self.nodes.get(&p.port.node)
                    && let Some(def) = c.node(&node.kind)
                {
                    resolve_input(def, node, &p.port.node, &p.port.port)?;
                }
            }
            let mut names = BTreeSet::new();
            for o in &g.outputs {
                check_name("output name", &o.name)?;
                if !names.insert(o.name.as_str()) {
                    return Err(GraphError::Duplicate { what: "output name", name: o.name.clone(), group: id.clone() });
                }
                if !members.contains(&o.from.node) {
                    return Err(GraphError::NotMember { group: id.clone(), node: o.from.node.clone() });
                }
                if let Some(c) = catalog
                    && let Some(node) = self.nodes.get(&o.from.node)
                    && let Some(def) = c.node(&node.kind)
                    && def.output(&o.from.port).is_none()
                {
                    return Err(GraphError::UnknownPort { node: o.from.node.clone(), kind: def.id.clone(), dir: "output", port: o.from.port.clone() });
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{add, catalog};
    use serde_json::json;

    fn ids(v: &[&NodeId]) -> Vec<NodeId> {
        v.iter().map(|n| (*n).clone()).collect()
    }

    #[test]
    fn names() {
        for ok in ["pencil", "my group", "Größe", "a.b", "1x", "class"] {
            assert!(is_name(ok), "{ok}");
        }
        for bad in ["", "   ", "a\nb", "x\u{0}", "\u{7}bell"] {
            assert!(!is_name(bad), "{bad:?}");
        }
        assert!(!is_name(&"x".repeat(MAX_TITLE_CHARS + 1)));
    }

    #[test]
    fn create_nest_and_ungroup() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, b, d) = (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.upper"), add(&mut g, &c, "text.words"));
        g.move_node(&b, [100.0, 50.0]).unwrap();
        g.move_node(&a, [10.0, 70.0]).unwrap();
        let outer = g.create_group("sketchy", &ids(&[&a, &b, &d]), &[]).unwrap();
        assert_eq!(outer, GroupId::from("g1"));
        assert_eq!(g.group(&outer).unwrap().pos, [0.0, 0.0], "the top-left of its members");
        // nesting: b and d move from outer into inner
        let inner = g.create_group("inner", &ids(&[&b, &d]), &[]).unwrap();
        assert_eq!(g.group(&inner).unwrap().parent.as_ref(), Some(&outer));
        assert_eq!(g.group(&outer).unwrap().nodes, [a.clone()].into());
        assert_eq!(g.group_of(&b), Some(&inner));
        assert_eq!(g.enclosing_groups(&b), [inner.clone(), outer.clone()]);
        assert_eq!(g.group_members(&outer).len(), 3);
        // can't mix a top-level node with a nested one
        let e = add(&mut g, &c, "text.label");
        assert!(matches!(g.create_group("x", &ids(&[&e, &b]), &[]), Err(GraphError::MixedGroups)));
        // a group can wrap a sibling group
        let wrap = g.create_group("wrap", &ids(&[&a]), std::slice::from_ref(&inner)).unwrap();
        assert_eq!(g.group(&inner).unwrap().parent.as_ref(), Some(&wrap));
        assert_eq!(g.group(&wrap).unwrap().parent.as_ref(), Some(&outer));
        g.check(&c).unwrap();
        // ungroup wrap: a goes back to outer, inner's parent becomes outer
        g.ungroup(&wrap).unwrap();
        assert_eq!(g.group_of(&a), Some(&outer));
        assert_eq!(g.group(&inner).unwrap().parent.as_ref(), Some(&outer));
        g.ungroup(&outer).unwrap();
        assert_eq!(g.group(&inner).unwrap().parent, None);
        assert_eq!(g.group_of(&a), None);
        g.check(&c).unwrap();
        assert!(matches!(g.ungroup(&outer), Err(GraphError::UnknownGroup(_))));
        assert_eq!(g.fresh_group_name(), "group 1");
    }

    #[test]
    fn bad_names_and_empty_groups() {
        let c = catalog();
        let mut g = Graph::new();
        let a = add(&mut g, &c, "text.literal");
        let e = g.create_group("my\tplugin", std::slice::from_ref(&a), &[]).unwrap_err();
        assert!(matches!(e, GraphError::BadName { .. }), "{e}");
        assert!(matches!(g.create_group("ok", &[], &[]), Err(GraphError::EmptyGroup)));
        assert!(matches!(g.create_group("ok", &[NodeId::from("n9")], &[]), Err(GraphError::UnknownNode(_))));
        assert!(matches!(g.create_group("ok", &[], &[GroupId::from("g9")]), Err(GraphError::UnknownGroup(_))));
        let id = g.create_group("ok", &[a], &[]).unwrap();
        assert!(g.rename_group(&id, "").is_err());
        g.rename_group(&id, "a better name").unwrap();
        assert_eq!(g.group(&id).unwrap().name, "a better name");
    }

    #[test]
    fn promote_and_outputs() {
        let c = catalog();
        let mut g = Graph::new();
        let (slow, case, words, outside) =
            (add(&mut g, &c, "text.slow"), add(&mut g, &c, "text.case"), add(&mut g, &c, "text.words"), add(&mut g, &c, "text.label"));
        let id = g.create_group("think", &[slow.clone(), case.clone(), words.clone()], &[]).unwrap();
        g.promote(&c, &id, PortRef::new(slow.clone(), "seconds"), "delay", "how long to think").unwrap();
        assert!(matches!(g.promote(&c, &id, PortRef::new(slow.clone(), "seconds"), "other", ""), Err(GraphError::Duplicate { what: "promoted port", .. })));
        assert!(matches!(g.promote(&c, &id, PortRef::new(case.clone(), "mode"), "delay", ""), Err(GraphError::Duplicate { what: "promoted label", .. })));
        assert!(matches!(g.promote(&c, &id, PortRef::new(outside.clone(), "label"), "s", ""), Err(GraphError::NotMember { .. })));
        assert!(matches!(g.promote(&c, &id, PortRef::new(slow.clone(), "nope"), "s", ""), Err(GraphError::UnknownPort { .. })));
        assert!(g.promote(&c, &id, PortRef::new(case.clone(), "mode"), "", "").is_err());
        // a sub-port can be promoted only while it is active
        assert!(matches!(g.promote(&c, &id, PortRef::new(case.clone(), "mode.small"), "small", ""), Err(GraphError::InactiveSubPort { .. })));
        g.set_value_checked(&c, &PortRef::new(case.clone(), "mode"), json!("title")).unwrap();
        g.promote(&c, &id, PortRef::new(case.clone(), "mode.small"), "small words", "").unwrap();
        // switching away drops the promotion (reported by set_value_checked)
        let out = g.set_value_checked(&c, &PortRef::new(case.clone(), "mode"), json!("upper")).unwrap();
        assert_eq!(out.unpromoted, [(id.clone(), "small words".to_owned())]);
        g.check(&c).unwrap();

        g.add_group_output(&c, &id, PortRef::new(words.clone(), "count"), "words").unwrap();
        assert!(matches!(g.add_group_output(&c, &id, PortRef::new(case.clone(), "text"), "words"), Err(GraphError::Duplicate { .. })));
        assert!(matches!(g.add_group_output(&c, &id, PortRef::new(words.clone(), "count"), "again"), Err(GraphError::Duplicate { .. })));
        assert!(matches!(g.add_group_output(&c, &id, PortRef::new(words.clone(), "nope"), "x"), Err(GraphError::UnknownPort { dir: "output", .. })));
        g.set_collapsed(&id, true).unwrap();

        // removing a node removes it from the group and drops what referred to it
        g.remove_node(&words).unwrap();
        let grp = g.group(&id).unwrap();
        assert!(grp.outputs.is_empty() && !grp.nodes.contains(&words));
        g.remove_node(&slow).unwrap();
        assert!(g.group(&id).unwrap().promoted.is_empty());
        g.check(&c).unwrap();
        assert!(g.unpromote(&id, "delay").is_err());
        // the last member gone, the group goes too
        g.remove_node(&case).unwrap();
        assert!(g.groups.is_empty());
    }

    #[test]
    fn promoted_ranges() {
        let c = catalog();
        let mut g = Graph::new();
        let (slow, label) = (add(&mut g, &c, "text.slow"), add(&mut g, &c, "text.label"));
        let id = g.create_group("g", &[slow.clone(), label.clone()], &[]).unwrap();
        g.promote(&c, &id, PortRef::new(slow.clone(), "seconds"), "seconds", "").unwrap();
        g.promote(&c, &id, PortRef::new(label.clone(), "label"), "label", "").unwrap();
        let e = |min: Option<Option<f64>>, max: Option<Option<f64>>| PromotedEdit { min, max, ..Default::default() };
        g.edit_promoted(&c, &id, "seconds", &e(Some(Some(0.5)), Some(Some(5.0)))).unwrap();
        assert!(matches!(g.edit_promoted(&c, &id, "seconds", &e(Some(Some(-1.0)), None)), Err(GraphError::BadRange { .. })), "below the port's 0");
        assert!(matches!(g.edit_promoted(&c, &id, "seconds", &e(Some(Some(6.0)), None)), Err(GraphError::BadRange { .. })), "min above max");
        assert!(matches!(g.edit_promoted(&c, &id, "seconds", &e(Some(Some(f64::NAN)), None)), Err(GraphError::BadRange { .. })));
        assert!(matches!(g.edit_promoted(&c, &id, "label", &e(Some(Some(1.0)), None)), Err(GraphError::BadRange { .. })), "a string has no range");
        g.edit_promoted(&c, &id, "seconds", &PromotedEdit { label: Some("delay".into()), doc: Some("how long".into()), min: Some(None), ..Default::default() })
            .unwrap();
        let p = &g.group(&id).unwrap().promoted[0];
        assert_eq!((p.label.as_str(), p.doc.as_str(), p.min, p.max), ("delay", "how long", None, Some(5.0)));
        assert!(g.edit_promoted(&c, &id, "delay", &PromotedEdit { label: Some("label".into()), ..Default::default() }).is_err(), "taken");
    }

    #[test]
    fn broken_invariants_are_caught() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, b) = (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.upper"));
        let g1 = g.create_group("one", std::slice::from_ref(&a), &[]).unwrap();
        let g2 = g.create_group("two", std::slice::from_ref(&b), &[]).unwrap();
        let ok = g.clone();

        g.groups.get_mut(&g2).unwrap().nodes.insert(a.clone());
        assert!(matches!(g.check_groups(None), Err(GraphError::GroupInvariant(m)) if m.contains("both")));
        g = ok.clone();
        g.groups.get_mut(&g1).unwrap().parent = Some(g2.clone());
        g.groups.get_mut(&g2).unwrap().parent = Some(g1.clone());
        assert!(matches!(g.check_groups(None), Err(GraphError::GroupInvariant(m)) if m.contains("nested in itself")));
        g = ok.clone();
        g.groups.get_mut(&g1).unwrap().parent = Some(g1.clone());
        assert!(g.check_groups(None).is_err());
        g = ok.clone();
        g.groups.get_mut(&g1).unwrap().parent = Some(GroupId::from("g9"));
        assert!(g.check_groups(None).is_err());
        g = ok.clone();
        g.groups.get_mut(&g1).unwrap().nodes.insert(NodeId::from("n77"));
        assert!(g.check_groups(None).is_err());
        g = ok.clone();
        g.groups.get_mut(&g1).unwrap().pos = [f32::NAN, 0.0];
        assert!(g.check_groups(None).is_err());
        g = ok.clone();
        g.groups.get_mut(&g1).unwrap().promoted.push(Promoted {
            port: PortRef::new(a.clone(), "nope"),
            label: "x".into(),
            doc: String::new(),
            min: None,
            max: None,
        });
        assert!(g.check_groups(None).is_ok(), "ports are checked only with a catalog");
        assert!(g.check(&c).is_err());
        g = ok.clone();
        g.groups.get_mut(&g1).unwrap().promoted.push(Promoted {
            port: PortRef::new(b.clone(), "text"),
            label: "x".into(),
            doc: String::new(),
            min: None,
            max: None,
        });
        assert!(matches!(g.check_groups(None), Err(GraphError::NotMember { .. })));
        g = ok.clone();
        g.groups.get_mut(&g1).unwrap().name = "not\nok".into();
        assert!(matches!(g.check_groups(None), Err(GraphError::BadName { .. })));
        g = ok;
        g.groups.get_mut(&g1).unwrap().outputs.push(GroupOutput { from: PortRef::new(a.clone(), "nope"), name: "x".into() });
        assert!(g.check(&c).is_err());
    }

    #[test]
    fn workflow_json_round_trips_groups() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, b) = (add(&mut g, &c, "text.slow"), add(&mut g, &c, "text.upper"));
        let id = g.create_group("think", &[a.clone(), b.clone()], &[]).unwrap();
        g.create_group("inner", std::slice::from_ref(&b), &[]).unwrap();
        g.promote(&c, &id, PortRef::new(a.clone(), "seconds"), "seconds", "the delay").unwrap();
        g.add_group_output(&c, &id, PortRef::new(b.clone(), "text"), "shout").unwrap();
        g.set_collapsed(&id, true).unwrap();
        g.move_group(&id, [10.0, 20.0]).unwrap();
        let text = g.to_json().unwrap();
        let back = Graph::from_json(&text).unwrap();
        assert_eq!(back.groups, g.groups);
        assert_eq!(back.next_group, 3);
        back.check(&c).unwrap();

        // a file whose groups break the invariants is refused
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v["groups"]["g2"]["parent"] = json!("g2");
        assert!(Graph::from_json(&v.to_string()).is_err());
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v["groups"]["g1"]["nodes"] = json!(["n1", "n2"]);
        assert!(Graph::from_json(&v.to_string()).is_err(), "n2 is in g1 and g2");
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v["groups"]["g1"]["name"] = json!(3);
        assert!(Graph::from_json(&v.to_string()).is_err());

        // a graph without groups writes neither groups nor next_group
        let mut plain = Graph::new();
        add(&mut plain, &c, "text.upper");
        let text = plain.to_json().unwrap();
        assert!(!text.contains("groups") && !text.contains("next_group"), "{text}");
        assert_eq!(Graph::from_json(&text).unwrap().next_group, 1);
    }
}
