//! Net labels: a way of drawing a wire, as a schematic does. A wire with a `label` is drawn as
//! a label at each end instead of a line across the canvas; every wire from one output labelled
//! with the same name is the same net, driven by that output ([`Net::from`]).
//!
//! Labels are only how the graph looks. The graph is the same with or without them: the run
//! request, the memo identity and staleness all read `from`/`to`.
//!
//! **Nets are local to groups.** A net lives in a [`Scope`]: the top level ([`Graph::nets`]) or
//! a group ([`crate::Group::nets`]), always the innermost one holding its driver and every use.
//! So a net drawn inside a group belongs to it, and two groups can each have a net `picture`.
//! A name may not be held twice where one scope encloses the other, so a label never means two
//! nets on one screen; nets in groups side by side may share it. Grouping moves a net into the
//! new group when the group holds all of it; ungrouping moves it out (renamed if the name is
//! taken there).
//!
//! The invariants, kept by [`Graph::prune_nets`] after every edit and on load (a file's labels
//! are hostile input, and a bad label is dropped, its wire kept):
//! - a wire's `label` names a net whose `from` is that wire's `from`; one output drives at most
//!   one net of a name;
//! - every net has at least one wire (the net goes when its last wire does), and sits in the
//!   innermost scope holding its driver and its wires' inputs;
//! - no two nets of a name sit in scopes one of which encloses the other;
//! - names are trimmed, 1 to [`MAX_NET_CHARS`] characters, without control characters;
//! - label offsets lie within ±[`MAX_LABEL_OFFSET`] of their pin.
//!
//! A label sits at an offset from its pin, so it moves with its node.
//!
//! Lifted from the research software's `crates/graph/src/nets.rs` (2026-10-31, the research software `8480c03`).

use std::collections::{BTreeMap, BTreeSet};

use plumcraft_proto::catalogue::SchemaReply;
use serde::{Deserialize, Serialize};

use crate::{Graph, GraphError, GroupId, NodeId, PortRef, Result, Wire};

/// The longest net name, in characters.
pub const MAX_NET_CHARS: usize = 40;
/// The farthest a label sits from its pin, in graph units.
pub const MAX_LABEL_OFFSET: i32 = 100_000;

/// Where a net's name holds: the top level (`None`) or a group.
pub type Scope = Option<GroupId>;

/// A net: the output that drives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Net {
    pub from: PortRef,
    /// Where the driver's label sits, as an offset from the output pin (graph units); unset:
    /// beside the pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[i32; 2]>,
}

/// Which net: its name in its scope.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NetKey {
    /// The group it is local to; `None`: the top level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Scope,
    pub name: String,
}

impl NetKey {
    pub fn new(group: Scope, name: impl Into<String>) -> Self {
        NetKey { group, name: name.into() }
    }

    /// A net at the top level.
    pub fn top(name: impl Into<String>) -> Self {
        NetKey { group: None, name: name.into() }
    }
}

impl std::fmt::Display for NetKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.group {
            None => f.write_str(&self.name),
            Some(g) => write!(f, "{} (in {g})", self.name),
        }
    }
}

/// `name`, trimmed, if it can name a net.
pub fn check_net_name(name: &str) -> Result<String> {
    let t = name.trim();
    if t.is_empty() {
        return Err(GraphError::BadNetName(name.to_owned(), "a name can't be blank"));
    }
    if t.chars().count() > MAX_NET_CHARS {
        return Err(GraphError::BadNetName(name.to_owned(), "a name has at most 40 characters"));
    }
    if t.chars().any(char::is_control) {
        return Err(GraphError::BadNetName(name.to_owned(), "a name can't contain control characters"));
    }
    Ok(t.to_owned())
}

fn check_offset(at: Option<[i32; 2]>) -> Result<()> {
    if at.is_some_and(|a| a.iter().any(|c| c.abs() > MAX_LABEL_OFFSET)) { Err(GraphError::BadLabelOffset) } else { Ok(()) }
}

fn offset_ok(at: &Option<[i32; 2]>) -> bool {
    check_offset(*at).is_ok()
}

/// `base`, or `base_2`, `base_3` … : the first name `taken` doesn't hold.
fn fresh(taken: &BTreeSet<String>, base: &str) -> String {
    if !taken.contains(base) {
        return base.to_owned();
    }
    let stem: String = base.chars().take(MAX_NET_CHARS.saturating_sub(8)).collect();
    (2u64..).map(|k| format!("{stem}_{k}")).find(|n| !taken.contains(n)).unwrap_or_default()
}

impl Graph {
    /// `scope` and the groups around it, innermost first (the top level not included; a cycle
    /// in a hostile file ends the walk).
    fn scope_chain(&self, scope: &Scope) -> Vec<GroupId> {
        let mut out: Vec<GroupId> = Vec::new();
        let mut at = scope.clone();
        while let Some(g) = at {
            let Some(grp) = self.groups.get(&g) else { break };
            if out.contains(&g) {
                break;
            }
            at = grp.parent.clone();
            out.push(g);
        }
        out
    }

    /// The scope `node` sits in: its innermost group.
    pub fn scope_of(&self, node: &NodeId) -> Scope {
        self.group_of(node).cloned()
    }

    /// Does `outer` enclose `inner` (or is it `inner`)?
    pub fn scope_encloses(&self, outer: &Scope, inner: &Scope) -> bool {
        match outer {
            None => true,
            Some(o) => self.scope_chain(inner).contains(o),
        }
    }

    /// May a name be held in both `a` and `b`? Not when one encloses the other.
    fn scopes_overlap(&self, a: &Scope, b: &Scope) -> bool {
        self.scope_encloses(a, b) || self.scope_encloses(b, a)
    }

    /// The innermost scope holding every node of `nodes` (the top level for none).
    pub fn common_scope<'a>(&self, nodes: impl IntoIterator<Item = &'a NodeId>) -> Scope {
        let mut common: Option<Vec<GroupId>> = None;
        for n in nodes {
            let mut chain = self.scope_chain(&self.scope_of(n));
            chain.reverse(); // outermost first
            common = Some(match common {
                None => chain,
                Some(c) => c.into_iter().zip(chain).take_while(|(a, b)| a == b).map(|(a, _)| a).collect(),
            });
        }
        common.and_then(|c| c.last().cloned())
    }

    /// The nets local to `scope` (`None` for a group that isn't there).
    pub fn nets_in(&self, scope: &Scope) -> Option<&BTreeMap<String, Net>> {
        match scope {
            None => Some(&self.nets),
            Some(g) => self.groups.get(g).map(|g| &g.nets),
        }
    }

    fn nets_in_mut(&mut self, scope: &Scope) -> Result<&mut BTreeMap<String, Net>> {
        match scope {
            None => Ok(&mut self.nets),
            Some(g) => self.groups.get_mut(g).map(|g| &mut g.nets).ok_or_else(|| GraphError::UnknownGroup(g.clone())),
        }
    }

    pub fn net(&self, key: &NetKey) -> Option<&Net> {
        self.nets_in(&key.group)?.get(&key.name)
    }

    /// Every net: the top level's, then each group's.
    pub fn all_nets(&self) -> impl Iterator<Item = (NetKey, &Net)> + '_ {
        let top = self.nets.iter().map(|(k, n)| (NetKey::top(k.clone()), n));
        top.chain(self.groups.iter().flat_map(|(g, grp)| grp.nets.iter().map(move |(k, n)| (NetKey::new(Some(g.clone()), k.clone()), n))))
    }

    /// Is any wire drawn through a net?
    pub fn has_nets(&self) -> bool {
        self.all_nets().next().is_some()
    }

    /// The wires drawn through net `key`.
    pub fn net_wires<'a>(&'a self, key: &'a NetKey) -> impl Iterator<Item = &'a Wire> + 'a {
        let from = self.net(key).map(|n| n.from.clone());
        self.wires.iter().filter(move |w| w.label.as_deref() == Some(key.name.as_str()) && from.as_ref() == Some(&w.from))
    }

    /// The net wire `w` is drawn through.
    pub fn net_of_wire(&self, w: &Wire) -> Option<NetKey> {
        let name = w.label.as_ref()?;
        self.all_nets().find(|(k, n)| &k.name == name && n.from == w.from).map(|(k, _)| k)
    }

    /// The net output `from` drives, if any (the first, if it drives several).
    pub fn net_of(&self, from: &PortRef) -> Option<NetKey> {
        self.all_nets().find(|(_, n)| &n.from == from).map(|(k, _)| k)
    }

    /// The driver of a net other than `driver`'s that holds `name` where `scope` would see it.
    fn name_taken(&self, scope: &Scope, name: &str, driver: &PortRef) -> Option<PortRef> {
        self.all_nets().find(|(k, n)| k.name == name && &n.from != driver && self.scopes_overlap(&k.group, scope)).map(|(_, n)| n.from.clone())
    }

    /// `base`, or `base_2`, `base_3` … : the first name a new net in `scope` can have.
    pub fn fresh_net_name(&self, scope: &Scope, base: &str) -> String {
        let taken: BTreeSet<String> = self.all_nets().filter(|(k, _)| self.scopes_overlap(&k.group, scope)).map(|(k, _)| k.name).collect();
        fresh(&taken, base)
    }

    /// Make sure output `from` drives a net `name` that can also reach the nodes `more`: in the
    /// innermost scope holding its driver, its wires and them (moved out there if it was further
    /// in). Refuses a name another output's net holds there.
    fn ensure_net(&mut self, name: &str, from: &PortRef, more: &[NodeId]) -> Result<NetKey> {
        let existing = self.all_nets().find(|(k, n)| k.name == name && &n.from == from).map(|(k, n)| (k, n.clone()));
        let mut ends: Vec<NodeId> = vec![from.node.clone()];
        ends.extend(more.iter().cloned());
        if let Some((k, _)) = &existing {
            ends.extend(self.net_wires(k).map(|w| w.to.node.clone()));
        }
        let scope = self.common_scope(&ends);
        if let Some(by) = self.name_taken(&scope, name, from) {
            return Err(GraphError::NetDriven { name: name.to_owned(), by });
        }
        let net = match existing {
            Some((k, _)) if k.group == scope => return Ok(k),
            Some((k, net)) => {
                self.nets_in_mut(&k.group)?.remove(&k.name);
                net
            }
            None => Net { from: from.clone(), at: None },
        };
        self.nets_in_mut(&scope)?.insert(name.to_owned(), net);
        Ok(NetKey::new(scope, name))
    }

    /// Put `net` in `scope` as `name`, or a fresh name if that is taken there, relabelling the
    /// wires from its driver labelled `name`. Returns the name it got.
    fn place_net(&mut self, scope: &Scope, name: &str, net: Net) -> Result<String> {
        let new = if self.name_taken(scope, name, &net.from).is_some() { self.fresh_net_name(scope, name) } else { name.to_owned() };
        if new != name {
            for w in self.wires.iter_mut().filter(|w| w.label.as_deref() == Some(name) && w.from == net.from) {
                w.label = Some(new.clone());
            }
        }
        self.nets_in_mut(scope)?.insert(new.clone(), net);
        Ok(new)
    }

    /// Draw the wire into `to` through net `name` (created on the wire's output if new, local to
    /// the innermost group holding both ends). Returns the net.
    pub fn label_wire(&mut self, to: &PortRef, name: &str) -> Result<NetKey> {
        let name = check_net_name(name)?;
        let from = self.wire_into(to).ok_or_else(|| GraphError::NotWired(to.clone()))?.from.clone();
        let key = self.ensure_net(&name, &from, std::slice::from_ref(&to.node))?;
        if let Some(w) = self.wires.iter_mut().find(|w| &w.to == to)
            && w.label.as_deref() != Some(name.as_str())
        {
            w.label = Some(name.clone());
            w.label_at = None;
        }
        self.touch();
        Ok(key)
    }

    /// Draw the wire into `to` as a plain wire again. Returns the net it was drawn through.
    pub fn unlabel_wire(&mut self, to: &PortRef) -> Result<Option<NetKey>> {
        let w = self.wire_into(to).ok_or_else(|| GraphError::NotWired(to.clone()))?;
        let old = self.net_of_wire(w);
        if let Some(w) = self.wires.iter_mut().find(|w| &w.to == to) {
            w.label = None;
            w.label_at = None;
        }
        self.touch();
        Ok(old)
    }

    /// Draw the wires from output `from` that `only` picks through net `name`. Returns the net
    /// and their inputs; refuses when it picks none.
    pub fn label_output(&mut self, from: &PortRef, name: &str, only: impl Fn(&Wire) -> bool) -> Result<(NetKey, Vec<PortRef>)> {
        let name = check_net_name(name)?;
        let tos: Vec<PortRef> = self.wires.iter().filter(|w| &w.from == from && only(w)).map(|w| w.to.clone()).collect();
        if tos.is_empty() {
            return Err(GraphError::NotWired(from.clone()));
        }
        let nodes: Vec<NodeId> = tos.iter().map(|t| t.node.clone()).collect();
        let key = self.ensure_net(&name, from, &nodes)?;
        for w in self.wires.iter_mut().filter(|w| tos.contains(&w.to)) {
            if w.label.as_deref() != Some(name.as_str()) {
                w.label = Some(name.clone());
                w.label_at = None;
            }
        }
        self.touch();
        Ok((key, tos))
    }

    /// Rename net `key` to `to`; refuses a name a net it would be seen with has.
    pub fn rename_net(&mut self, key: &NetKey, to: &str) -> Result<NetKey> {
        let to = check_net_name(to)?;
        if to == key.name {
            return Ok(key.clone());
        }
        let net = self.net(key).cloned().ok_or_else(|| GraphError::UnknownNet(key.to_string()))?;
        if let Some(by) = self.name_taken(&key.group, &to, &net.from) {
            return Err(GraphError::NetDriven { name: to, by });
        }
        if self.all_nets().any(|(k, n)| k.name == to && n.from == net.from) {
            return Err(GraphError::NetDriven { name: to, by: net.from });
        }
        let map = self.nets_in_mut(&key.group)?;
        map.remove(&key.name);
        map.insert(to.clone(), net.clone());
        for w in self.wires.iter_mut().filter(|w| w.label.as_deref() == Some(key.name.as_str()) && w.from == net.from) {
            w.label = Some(to.clone());
        }
        self.touch();
        Ok(NetKey::new(key.group.clone(), to))
    }

    /// Wire input `to` to net `key` (from its driver), drawn through the net: dropping a label
    /// on a pin connects it, as in a schematic. Checked as [`Graph::connect`]; a pin outside the
    /// net's group takes the net out to the group holding both (refused if the name is taken
    /// there). Returns the wire it replaced.
    pub fn connect_net(&mut self, catalog: &SchemaReply, key: &NetKey, to: &PortRef) -> Result<Option<Wire>> {
        let net = self.net(key).cloned().ok_or_else(|| GraphError::UnknownNet(key.to_string()))?;
        let mut ends: Vec<NodeId> = vec![net.from.node.clone(), to.node.clone()];
        ends.extend(self.net_wires(key).filter(|w| &w.to != to).map(|w| w.to.node.clone()));
        let scope = self.common_scope(&ends);
        if let Some(by) = self.name_taken(&scope, &key.name, &net.from) {
            return Err(GraphError::NetDriven { name: key.name.clone(), by });
        }
        let replaced = self.connect(catalog, net.from.clone(), to.clone())?;
        // the wire replaced may have been the net's last, and pruned it
        let now = self.ensure_net(&key.name, &net.from, std::slice::from_ref(&to.node))?;
        if let Ok(map) = self.nets_in_mut(&now.group)
            && let Some(n) = map.get_mut(&now.name)
            && n.at.is_none()
        {
            n.at = net.at;
        }
        if let Some(w) = self.wires.iter_mut().find(|w| &w.to == to) {
            w.label = Some(key.name.clone());
        }
        self.touch();
        Ok(replaced)
    }

    /// Move the driver's label of net `key` (an offset from its output pin; `None`: beside it).
    pub fn move_net_label(&mut self, key: &NetKey, at: Option<[i32; 2]>) -> Result<()> {
        check_offset(at)?;
        let net = self.nets_in_mut(&key.group).ok().and_then(|m| m.get_mut(&key.name)).ok_or_else(|| GraphError::UnknownNet(key.to_string()))?;
        if net.at != at {
            net.at = at;
            self.touch();
        }
        Ok(())
    }

    /// Move the receiving label of the wire into `to` (an offset from the input pin; `None`:
    /// beside it). The wire must be drawn through a net.
    pub fn move_wire_label(&mut self, to: &PortRef, at: Option<[i32; 2]>) -> Result<()> {
        check_offset(at)?;
        let w = self.wires.iter_mut().find(|w| &w.to == to).ok_or_else(|| GraphError::NotWired(to.clone()))?;
        if w.label.is_none() {
            return Err(GraphError::BadNetName(String::new(), "the wire isn't drawn through a net"));
        }
        if w.label_at != at {
            w.label_at = at;
            self.touch();
        }
        Ok(())
    }

    /// Bring `other`'s nets across with its nodes (a copy, a paste, an insert): `nodes` maps its
    /// node ids to the new ones, whose wires must already be here, and `groups` its group ids
    /// (a net in a group not mapped comes to the top level). A name taken where the net lands
    /// gets a suffix (`picture` -> `picture_2`). Returns the names that changed.
    pub fn adopt_nets(&mut self, other: &Graph, nodes: &BTreeMap<NodeId, NodeId>, groups: &BTreeMap<GroupId, GroupId>) -> BTreeMap<String, String> {
        let mut renamed = BTreeMap::new();
        let port = |p: &PortRef| nodes.get(&p.node).map(|n| PortRef::new(n.clone(), p.port.clone()));
        for (key, net) in other.all_nets() {
            let Some(from) = port(&net.from) else { continue };
            let scope = key.group.as_ref().and_then(|g| groups.get(g)).cloned();
            let Ok(new) = self.place_net(&scope, &key.name, Net { from: from.clone(), at: net.at }) else { continue };
            for w in other.net_wires(&key) {
                let Some(to) = port(&w.to) else { continue };
                if let Some(here) = self.wires.iter_mut().find(|h| h.to == to && h.from == from) {
                    here.label = Some(new.clone());
                    here.label_at = w.label_at;
                }
            }
            if new != key.name {
                renamed.insert(key.name.clone(), new);
            }
        }
        self.touch();
        renamed
    }

    /// Move `nets`, local to a group being dissolved, out to the scope `to` around it, each
    /// renamed where its name is taken there.
    pub(crate) fn lift_nets(&mut self, nets: BTreeMap<String, Net>, to: &Scope) {
        for (name, net) in nets {
            // a parent that isn't there: the top level
            let scope = if self.nets_in(to).is_some() { to.clone() } else { None };
            let _ = self.place_net(&scope, &name, net);
        }
    }

    /// Restore the invariants (module docs): drop labels naming no net from their output, nets
    /// with a bad name or no wires, and offsets out of range; put each net in the innermost
    /// scope holding all of it, renaming one whose name is taken there.
    pub fn prune_nets(&mut self) {
        // every net, the top level's first, each with the scope it was in
        let mut entries: Vec<(Scope, String, Net)> = std::mem::take(&mut self.nets).into_iter().map(|(k, n)| (None, k, n)).collect();
        for (g, grp) in &mut self.groups {
            entries.extend(std::mem::take(&mut grp.nets).into_iter().map(|(k, n)| (Some(g.clone()), k, n)));
        }
        let mut seen = BTreeSet::new();
        entries.retain(|(_, k, n)| check_net_name(k).is_ok_and(|t| &t == k) && !n.from.port.is_empty() && seen.insert((k.clone(), n.from.clone())));
        // where each belongs; nets without wires go; nets already there keep their names first
        let mut placed: Vec<(bool, Scope, String, Net)> = Vec::new();
        for (was, name, mut net) in entries {
            let uses: Vec<&NodeId> =
                self.wires.iter().filter(|w| w.label.as_deref() == Some(name.as_str()) && w.from == net.from).map(|w| &w.to.node).collect();
            if uses.is_empty() {
                continue;
            }
            let scope = self.common_scope(std::iter::once(&net.from.node).chain(uses));
            if !offset_ok(&net.at) {
                net.at = None;
            }
            placed.push((was != scope, scope, name, net));
        }
        placed.sort_by_key(|p| p.0);
        let mut out: Vec<(Scope, String, Net)> = Vec::new();
        let mut renames: Vec<(String, PortRef, String)> = Vec::new();
        for (_, scope, name, net) in placed {
            let taken: BTreeSet<String> = out.iter().filter(|(s, _, _)| self.scopes_overlap(s, &scope)).map(|(_, n, _)| n.clone()).collect();
            let new = fresh(&taken, &name);
            if new != name {
                renames.push((name, net.from.clone(), new.clone()));
            }
            out.push((scope, new, net));
        }
        for w in &mut self.wires {
            if let Some((_, _, new)) = renames.iter().find(|(old, from, _)| w.label.as_ref() == Some(old) && &w.from == from) {
                w.label = Some(new.clone());
            }
            let ok = w.label.as_ref().is_some_and(|l| out.iter().any(|(_, n, net)| n == l && net.from == w.from));
            if !ok {
                w.label = None;
            }
            if w.label.is_none() || !offset_ok(&w.label_at) {
                w.label_at = None;
            }
        }
        for (scope, name, net) in out {
            match &scope {
                None => {
                    self.nets.insert(name, net);
                }
                Some(g) => {
                    if let Some(grp) = self.groups.get_mut(g) {
                        grp.nets.insert(name, net);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{add, catalog};

    /// A literal feeding three slow nodes, and a word count feeding one's seconds.
    fn fan(c: &SchemaReply) -> (Graph, NodeId, Vec<NodeId>, NodeId) {
        let mut g = Graph::new();
        let img = add(&mut g, c, "text.literal");
        let grads: Vec<NodeId> = (0..3).map(|_| add(&mut g, c, "text.slow")).collect();
        for gr in &grads {
            g.connect(c, PortRef::new(img.clone(), "text"), PortRef::new(gr.clone(), "text")).unwrap();
        }
        let sc = add(&mut g, c, "text.words");
        g.connect(c, PortRef::new(sc.clone(), "count"), PortRef::new(grads[0].clone(), "seconds")).unwrap();
        (g, img, grads, sc)
    }

    fn top(name: &str) -> NetKey {
        NetKey::top(name)
    }

    fn image(n: &NodeId) -> PortRef {
        PortRef::new(n.clone(), "text")
    }

    #[test]
    fn a_label_names_a_net_driven_by_one_output() {
        let c = catalog();
        let (mut g, img, grads, sc) = fan(&c);
        assert_eq!(g.label_wire(&image(&grads[0]), "  picture ").unwrap(), NetKey::top("picture"));
        assert_eq!(g.nets["picture"].from, PortRef::new(img.clone(), "text"));
        assert_eq!(g.net_of(&PortRef::new(img.clone(), "text")), Some(NetKey::top("picture")));
        g.label_wire(&image(&grads[1]), "picture").unwrap();
        assert_eq!(g.net_wires(&top("picture")).count(), 2);
        // another output can't drive it
        let scale = PortRef::new(grads[0].clone(), "seconds");
        assert!(matches!(g.label_wire(&scale, "picture"), Err(GraphError::NetDriven { .. })));
        assert!(matches!(g.label_wire(&scale, " "), Err(GraphError::BadNetName(..))));
        assert!(matches!(g.label_wire(&scale, &"x".repeat(41)), Err(GraphError::BadNetName(..))));
        assert!(matches!(g.label_wire(&PortRef::new(sc.clone(), "nope"), "a"), Err(GraphError::NotWired(_))));
        // the net goes with its last wire
        g.unlabel_wire(&image(&grads[0])).unwrap();
        assert!(g.nets.contains_key("picture"));
        g.disconnect(&image(&grads[1])).unwrap();
        assert!(g.nets.is_empty());
    }

    #[test]
    fn rewiring_or_removing_the_driver_drops_its_labels() {
        let c = catalog();
        let (mut g, img, grads, _) = fan(&c);
        g.label_output(&PortRef::new(img.clone(), "text"), "picture", |_| true).unwrap();
        assert_eq!(g.net_wires(&top("picture")).count(), 3);
        // a wire into the same input from elsewhere is a plain wire
        let other = add(&mut g, &c, "text.literal");
        g.connect(&c, PortRef::new(other, "text"), image(&grads[2])).unwrap();
        assert_eq!(g.net_wires(&top("picture")).count(), 2);
        g.remove_node(&img).unwrap();
        assert!(g.nets.is_empty());
        assert!(g.wires.iter().all(|w| w.label.is_none()));
    }

    #[test]
    fn label_output_picks_wires_and_refuses_none() {
        let c = catalog();
        let (mut g, img, grads, _) = fan(&c);
        let rgb = PortRef::new(img.clone(), "text");
        let (_, tos) = g.label_output(&rgb, "picture", |w| w.to.node != grads[0]).unwrap();
        assert_eq!(tos.len(), 2);
        assert!(g.wire_into(&image(&grads[0])).unwrap().label.is_none());
        assert!(g.label_output(&rgb, "picture", |_| false).is_err());
        assert!(g.label_output(&PortRef::new(img, "alpha"), "x", |_| true).is_err());
    }

    #[test]
    fn rename_connect_and_move() {
        let c = catalog();
        let (mut g, img, grads, sc) = fan(&c);
        g.label_wire(&image(&grads[0]), "picture").unwrap();
        g.label_wire(&PortRef::new(grads[0].clone(), "seconds"), "scales").unwrap();
        assert!(matches!(g.rename_net(&top("picture"), "scales"), Err(GraphError::NetDriven { .. })));
        assert!(matches!(g.rename_net(&top("nope"), "x"), Err(GraphError::UnknownNet(_))));
        g.rename_net(&top("picture"), "rgb").unwrap();
        assert_eq!(g.wire_into(&image(&grads[0])).unwrap().label.as_deref(), Some("rgb"));
        // connecting over the net's only wire keeps the net
        g.connect_net(&c, &top("scales"), &PortRef::new(grads[0].clone(), "seconds")).unwrap();
        assert_eq!(g.net_wires(&top("scales")).count(), 1);
        // dropping a label on a pin wires it, type-checked
        let pencil = add(&mut g, &c, "text.slow");
        g.connect_net(&c, &top("scales"), &PortRef::new(pencil.clone(), "seconds")).unwrap();
        let w = g.wire_into(&PortRef::new(pencil.clone(), "seconds")).unwrap();
        assert_eq!((w.from.node.clone(), w.label.as_deref()), (sc, Some("scales")));
        assert!(g.connect_net(&c, &top("rgb"), &PortRef::new(pencil.clone(), "seconds")).is_err(), "text isn't a number");
        assert!(g.connect_net(&c, &top("nope"), &PortRef::new(pencil, "seconds")).is_err());
        // labels move, within bounds
        g.move_net_label(&top("rgb"), Some([30, -10])).unwrap();
        g.move_wire_label(&image(&grads[0]), Some([-90, 0])).unwrap();
        assert_eq!(g.nets["rgb"].at, Some([30, -10]));
        assert_eq!(g.wire_into(&image(&grads[0])).unwrap().label_at, Some([-90, 0]));
        assert!(matches!(g.move_net_label(&top("rgb"), Some([MAX_LABEL_OFFSET + 1, 0])), Err(GraphError::BadLabelOffset)));
        assert!(g.move_wire_label(&image(&grads[1]), Some([1, 1])).is_err(), "not labelled");
        let _ = img;
    }

    #[test]
    fn copies_and_inserts_bring_their_nets_renamed_on_a_clash() {
        let c = catalog();
        let (mut g, img, grads, _) = fan(&c);
        g.label_output(&PortRef::new(img.clone(), "text"), "picture", |_| true).unwrap();
        // a copy of the driver and two uses keeps the net among them
        let clip = g.subgraph(&[img.clone(), grads[0].clone(), grads[1].clone()]);
        assert_eq!(clip.net_wires(&top("picture")).count(), 2);
        // a copy of uses without the driver keeps no labels
        let uses = g.subgraph(&[grads[0].clone()]);
        assert!(uses.nets.is_empty());
        // inserted beside the original: the net is renamed
        let ins = g.insert_graph(&c, &clip, [0.0, 500.0]).unwrap();
        let new_img = &ins.nodes[&img];
        assert_eq!(g.nets["picture_2"].from, PortRef::new(new_img.clone(), "text"));
        assert_eq!(g.net_wires(&top("picture_2")).count(), 2);
        assert_eq!(g.net_wires(&top("picture")).count(), 3);
        // a duplicate of a use keeps the label on its wire from outside, not on copied wires
        let pairs = g.duplicate_nodes(std::slice::from_ref(&grads[2]), [0.0, 40.0], true).unwrap();
        assert_eq!(g.wire_into(&image(&pairs[0].1)).unwrap().label.as_deref(), Some("picture"));
        let pairs = g.duplicate_nodes(&[img.clone(), grads[2].clone()], [0.0, 40.0], false).unwrap();
        let copy = &pairs.iter().find(|(o, _)| o == &grads[2]).unwrap().1;
        assert!(g.wire_into(&image(copy)).unwrap().label.is_none());
    }

    #[test]
    fn a_file_keeps_good_labels_and_drops_bad_ones() {
        let c = catalog();
        let (mut g, img, grads, _) = fan(&c);
        g.label_output(&PortRef::new(img, "text"), "picture", |_| true).unwrap();
        g.move_wire_label(&image(&grads[1]), Some([-80, 4])).unwrap();
        let back = Graph::from_json(&g.to_json().unwrap()).unwrap();
        assert_eq!(back.nets, g.nets);
        assert_eq!(back.wires, g.wires);
        // hostile: a label naming another output's net, a net with no wires, a blank name,
        // an offset out of range
        let mut bad = g.clone();
        bad.wires[0].label = Some("ghost".into());
        bad.wires[1].label_at = Some([i32::MAX, 0]);
        bad.nets.insert("lonely".into(), Net { from: PortRef::new("n9", "x"), at: None });
        bad.nets.insert(" ".into(), Net { from: bad.wires[2].from.clone(), at: None });
        bad.wires[3].label = Some("picture".into()); // the count wire: not driven by the literal
        let back = Graph::from_json(&serde_json::to_string(&bad).unwrap()).unwrap();
        assert_eq!(back.nets.keys().collect::<Vec<_>>(), ["picture"]);
        assert_eq!(back.wires.len(), 4, "every wire kept");
        assert_eq!(back.wires[0].label, None);
        assert_eq!(back.wires[1].label_at, None);
        assert_eq!(back.wires[3].label, None);
    }

    #[test]
    fn fresh_names_count_up_and_stay_short() {
        let c = catalog();
        let (mut g, img, grads, _) = fan(&c);
        g.label_wire(&image(&grads[0]), "picture").unwrap();
        assert_eq!(g.fresh_net_name(&None, "picture"), "picture_2");
        assert_eq!(g.fresh_net_name(&None, "other"), "other");
        let long = "p".repeat(MAX_NET_CHARS);
        g.rename_net(&top("picture"), &long).unwrap();
        assert!(check_net_name(&g.fresh_net_name(&None, &long)).is_ok());
        let _ = img;
    }

    /// Two copies of literal -> slow, slow, each copy in its own group.
    fn two_groups(c: &SchemaReply) -> (Graph, [(GroupId, NodeId, [NodeId; 2]); 2]) {
        let mut g = Graph::new();
        let make = |g: &mut Graph, name: &str| {
            let img = add(g, c, "text.literal");
            let a = add(g, c, "text.slow");
            let b = add(g, c, "text.slow");
            for n in [&a, &b] {
                g.connect(c, PortRef::new(img.clone(), "text"), image(n)).unwrap();
            }
            let gid = g.create_group(name, &[img.clone(), a.clone(), b.clone()], &[]).unwrap();
            (gid, img, [a, b])
        };
        let one = make(&mut g, "one");
        let two = make(&mut g, "two");
        (g, [one, two])
    }

    #[test]
    fn a_net_inside_a_group_is_the_groups_own() {
        let c = catalog();
        let (mut g, [(g1, img1, uses1), (g2, img2, uses2)]) = two_groups(&c);
        let k1 = g.label_wire(&image(&uses1[0]), "picture").unwrap();
        assert_eq!(k1, NetKey::new(Some(g1.clone()), "picture"));
        assert!(g.nets.is_empty(), "not at the top level");
        assert_eq!(g.groups[&g1].nets["picture"].from, PortRef::new(img1.clone(), "text"));
        // the other group can have a net of the same name: another net
        let k2 = g.label_wire(&image(&uses2[0]), "picture").unwrap();
        assert_eq!(k2.group, Some(g2.clone()));
        assert_eq!(g.net_wires(&k1).count(), 1);
        assert_eq!(g.net_wires(&k2).count(), 1);
        assert_eq!(g.net_of_wire(g.wire_into(&image(&uses2[0])).unwrap()), Some(k2.clone()));
        // a top-level net of that name would be seen with both: refused
        let top_img = add(&mut g, &c, "text.literal");
        let top_use = add(&mut g, &c, "text.slow");
        g.connect(&c, PortRef::new(top_img.clone(), "text"), image(&top_use)).unwrap();
        assert!(matches!(g.label_wire(&image(&top_use), "picture"), Err(GraphError::NetDriven { .. })));
        assert_eq!(g.fresh_net_name(&None, "picture"), "picture_2");
        assert_eq!(g.fresh_net_name(&Some(g1.clone()), "other"), "other");
        // renaming one leaves the other
        let k1 = g.rename_net(&k1, "rgb").unwrap();
        assert_eq!(k1, NetKey::new(Some(g1.clone()), "rgb"));
        assert_eq!(g.net_wires(&k2).count(), 1);
        assert!(g.rename_net(&k2, "rgb").is_ok(), "groups side by side may share a name");
        let _ = img2;
    }

    #[test]
    fn grouping_takes_a_net_in_and_ungrouping_lets_it_out() {
        let c = catalog();
        let (mut g, img, grads, _) = fan(&c);
        let key = g.label_output(&PortRef::new(img.clone(), "text"), "picture", |w| w.to.node != grads[2]).unwrap().0;
        assert_eq!(key, top("picture"));
        // a group holding the driver and both uses: the net is its own now
        let gid = g.create_group("inner", &[img.clone(), grads[0].clone(), grads[1].clone()], &[]).unwrap();
        assert!(g.nets.is_empty());
        let inner = NetKey::new(Some(gid.clone()), "picture");
        assert_eq!(g.net_wires(&inner).count(), 2);
        // a pin outside takes it back out
        g.connect_net(&c, &inner, &image(&grads[2])).unwrap();
        assert_eq!(g.net_wires(&top("picture")).count(), 3);
        assert!(g.groups[&gid].nets.is_empty());
        g.unlabel_wire(&image(&grads[2])).unwrap();
        assert_eq!(g.net_wires(&inner).count(), 2, "and in again");
        // ungrouping: out to the top level, where the name is free
        g.ungroup(&gid).unwrap();
        assert_eq!(g.net_wires(&top("picture")).count(), 2);
    }

    #[test]
    fn ungrouping_renames_a_net_whose_name_is_taken_outside() {
        let c = catalog();
        let (mut g, [(g1, _, uses1), (g2, _, uses2)]) = two_groups(&c);
        g.label_wire(&image(&uses1[0]), "picture").unwrap();
        g.label_wire(&image(&uses2[0]), "picture").unwrap();
        // out at the top level, the first group's net would be seen with the second's
        g.ungroup(&g1).unwrap();
        assert_eq!(g.net_wires(&top("picture_2")).count(), 1);
        assert_eq!(g.wire_into(&image(&uses1[0])).unwrap().label.as_deref(), Some("picture_2"));
        g.ungroup(&g2).unwrap();
        let names: Vec<&str> = g.nets.keys().map(String::as_str).collect();
        assert_eq!(names, ["picture", "picture_2"]);
        assert_eq!(g.wire_into(&image(&uses2[0])).unwrap().label.as_deref(), Some("picture"));
    }

    #[test]
    fn copies_files_and_inserts_keep_group_nets_apart() {
        let c = catalog();
        let (mut g, [(g1, img1, uses1), (g2, img2, uses2)]) = two_groups(&c);
        g.label_wire(&image(&uses1[0]), "picture").unwrap();
        g.label_wire(&image(&uses2[0]), "picture").unwrap();
        // a file keeps them where they are
        let back = Graph::from_json(&g.to_json().unwrap()).unwrap();
        assert_eq!(back.groups, g.groups);
        assert_eq!(back.wires, g.wires);
        // inserted beside them, each group keeps its own net, names unchanged
        let ins = g.insert_graph(&c, &back, [0.0, 800.0]).unwrap();
        assert!(ins.nets.is_empty(), "{:?}", ins.nets);
        for old in [&g1, &g2] {
            assert_eq!(g.groups[&ins.groups[old]].nets.keys().collect::<Vec<_>>(), ["picture"]);
        }
        // a copy (no groups) brings both to its top level, one renamed
        let clip = g.subgraph(&[img1.clone(), uses1[0].clone(), img2.clone(), uses2[0].clone()]);
        assert_eq!(clip.nets.keys().collect::<Vec<_>>(), ["picture", "picture_2"]);
        // hostile: a group's net used outside the group moves out to where it belongs
        let mut bad = back.clone();
        let net = bad.groups.get_mut(&g1).unwrap().nets.remove("picture").unwrap();
        bad.groups.get_mut(&g2).unwrap().nets.insert("wrong".into(), net.clone());
        bad.wires.iter_mut().filter(|w| w.from == net.from && w.label.is_some()).for_each(|w| w.label = Some("wrong".into()));
        let fixed = Graph::from_json(&serde_json::to_string(&bad).unwrap()).unwrap();
        assert_eq!(fixed.groups[&g1].nets.keys().collect::<Vec<_>>(), ["wrong"]);
        assert!(fixed.groups[&g2].nets.contains_key("picture"));
    }
}
