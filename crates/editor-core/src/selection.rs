//! The canvas's selection, as document state in the core (stage N2, slice 1;
//! `docs/shell-interface.md` 8.13 and 12.3).
//!
//! What is selected is the document's, not the shell's: the nodes, the collapsed groups (each
//! selected as the one node its proxy is on the canvas), and the net whose tags are lit
//! ([`Selection`]). A shell changes it only through the canvas commands
//! ([`crate::canvas_cmds`]: `selection.set`, `selection.clear`, `selection.all`, `net.select`),
//! so a click in the egui editor, a tap in a native shell and a script's `command.run` are one
//! path, and every shell reads the same state.
//!
//! The selection lives in a [`SelectionCell`]: the document holds it, and a shell that draws the
//! canvas may hold a handle to the same cell to read it while it draws (the egui canvas does).
//! Nodes and groups the graph no longer has are dropped after every command
//! ([`Selection::retain_existing`]).
//!
//! [`Visibility`] (what the canvas shows: an entered group's members, a collapsed group as its
//! proxy) is here because Select All selects what is shown; it moved from `plumcraft-ui`'s
//! `groups_view.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use plumcraft_graph::{Graph, GroupId, NetKey, NodeId};
use serde::Serialize;
use serde_json::{Value, json};

/// The most ids one `selection.set` may name.
pub const MAX_IDS: usize = 100_000;

/// What is selected: nodes, collapsed groups (by their group's id), and a net.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Selection {
    pub nodes: Vec<NodeId>,
    /// Selected collapsed groups (their proxy nodes).
    pub groups: Vec<GroupId>,
    /// The net selected (its tags lit, faint lines to every use); separate from the nodes:
    /// `selection.clear` leaves it, `net.select` changes it.
    pub net: Option<NetKey>,
}

/// How `selection.set` combines what it names with what is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Exactly these (a click).
    Replace,
    /// These as well (a Shift-click, a marquee).
    Add,
    /// Not these (a ⌘-click).
    Remove,
    /// Each one in if out, out if in.
    Toggle,
}

impl Mode {
    pub fn parse(v: Option<&Value>) -> Result<Mode, String> {
        match v {
            None | Some(Value::Null) => Ok(Mode::Replace),
            Some(Value::String(s)) => match s.as_str() {
                "replace" => Ok(Mode::Replace),
                "add" => Ok(Mode::Add),
                "remove" => Ok(Mode::Remove),
                "toggle" => Ok(Mode::Toggle),
                _ => Err("mode must be \"replace\", \"add\", \"remove\" or \"toggle\"".into()),
            },
            Some(_) => Err("mode must be \"replace\", \"add\", \"remove\" or \"toggle\"".into()),
        }
    }
}

impl Selection {
    /// Nothing selected but, perhaps, a net.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.groups.is_empty()
    }

    /// Select exactly `nodes` (and no group).
    pub fn set(&mut self, nodes: Vec<NodeId>) {
        self.nodes = nodes;
        self.nodes.dedup();
        self.groups.clear();
    }

    /// Select no node and no group (the net stays).
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.groups.clear();
    }

    /// Select exactly collapsed group `group`.
    pub fn set_group(&mut self, group: GroupId) {
        self.nodes.clear();
        self.groups = vec![group];
    }

    pub fn add_node(&mut self, n: NodeId) {
        if !self.nodes.contains(&n) {
            self.nodes.push(n);
        }
    }

    pub fn add_group(&mut self, g: GroupId) {
        if !self.groups.contains(&g) {
            self.groups.push(g);
        }
    }

    /// Combine `nodes` and `groups` with what is selected, as `mode` says.
    pub fn apply(&mut self, mode: Mode, nodes: Vec<NodeId>, groups: Vec<GroupId>) {
        match mode {
            Mode::Replace => {
                self.clear();
                nodes.into_iter().for_each(|n| self.add_node(n));
                groups.into_iter().for_each(|g| self.add_group(g));
            }
            Mode::Add => {
                nodes.into_iter().for_each(|n| self.add_node(n));
                groups.into_iter().for_each(|g| self.add_group(g));
            }
            Mode::Remove => {
                self.nodes.retain(|n| !nodes.contains(n));
                self.groups.retain(|g| !groups.contains(g));
            }
            Mode::Toggle => {
                for n in nodes {
                    match self.nodes.iter().position(|x| *x == n) {
                        Some(i) => {
                            self.nodes.remove(i);
                        }
                        None => self.nodes.push(n),
                    }
                }
                for g in groups {
                    match self.groups.iter().position(|x| *x == g) {
                        Some(i) => {
                            self.groups.remove(i);
                        }
                        None => self.groups.push(g),
                    }
                }
            }
        }
    }

    /// Forget nodes, groups and a net the graph no longer has. True when something went.
    pub fn retain_existing(&mut self, graph: &Graph) -> bool {
        let before = (self.nodes.len(), self.groups.len(), self.net.is_some());
        self.nodes.retain(|n| graph.nodes.contains_key(n));
        self.groups.retain(|g| graph.group(g).is_some());
        if self.net.as_ref().is_some_and(|k| graph.net(k).is_none()) {
            self.net = None;
        }
        before != (self.nodes.len(), self.groups.len(), self.net.is_some())
    }

    /// The selected nodes still in the graph, with the members of selected collapsed groups:
    /// what Cut, Copy and Delete act on.
    pub fn members(&self, graph: &Graph) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = self.nodes.iter().filter(|n| graph.nodes.contains_key(*n)).cloned().collect();
        for gid in &self.groups {
            for m in graph.group_members(gid) {
                if !ids.contains(&m) {
                    ids.push(m);
                }
            }
        }
        ids
    }

    /// The canvas commands' reply: `{nodes, groups, net}`.
    pub fn reply(&self) -> Value {
        json!({"nodes": self.nodes, "groups": self.groups, "net": self.net})
    }
}

/// The document's selection, shared with a shell that reads it while drawing. Cloning gives
/// another handle to the same selection.
#[derive(Debug, Clone, Default)]
pub struct SelectionCell(Arc<Mutex<Selection>>);

impl SelectionCell {
    fn lock(&self) -> MutexGuard<'_, Selection> {
        // a panic while it was held left it as it was: still a selection
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A copy of what is selected now.
    pub fn get(&self) -> Selection {
        self.lock().clone()
    }

    /// Read it in place.
    pub fn with<R>(&self, f: impl FnOnce(&Selection) -> R) -> R {
        f(&self.lock())
    }

    /// Change it. Only the canvas commands do, and the document's upkeep.
    pub fn update<R>(&self, f: impl FnOnce(&mut Selection) -> R) -> R {
        f(&mut self.lock())
    }

    /// Is this the same selection as `other`'s (two handles to one cell)?
    pub fn same(&self, other: &SelectionCell) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// `{nodes?, groups?}`'s ids, checked against the graph: a list of strings, each one the graph
/// has, at most [`MAX_IDS`].
pub fn ids_param(params: &Value, key: &str) -> Result<Vec<String>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => {
            if a.len() > MAX_IDS {
                return Err(format!("{key} names more than {MAX_IDS} ids"));
            }
            a.iter().map(|v| v.as_str().map(str::to_owned).ok_or_else(|| format!("{key} must be a list of ids"))).collect()
        }
        Some(_) => Err(format!("{key} must be a list of ids")),
    }
}

/// An id as an error message shows it.
pub fn shown_id(id: &str) -> String {
    id.chars().take(64).collect()
}

/// `selection.set`'s params, checked: the nodes and groups it names (each in the graph) and how.
pub fn parse_set(graph: &Graph, params: &Value) -> Result<(Vec<NodeId>, Vec<GroupId>, Mode), String> {
    crate::control::check_fields(params, &["nodes", "groups", "mode"])?;
    let mode = Mode::parse(params.get("mode"))?;
    let mut nodes = Vec::new();
    for id in ids_param(params, "nodes")? {
        let n = NodeId::from(id.as_str());
        if !graph.nodes.contains_key(&n) {
            return Err(format!("no node {}", shown_id(&id)));
        }
        nodes.push(n);
    }
    let mut groups = Vec::new();
    for id in ids_param(params, "groups")? {
        let g = GroupId::from(id.as_str());
        if graph.group(&g).is_none() {
            return Err(format!("no group {}", shown_id(&id)));
        }
        groups.push(g);
    }
    Ok((nodes, groups, mode))
}

/// What a command made, to select: the nodes it returned (`{nodes: [ids]}`, as duplicate and
/// paste return), or the collapsed group it made (`{group}`, as Insert as Group returns). None:
/// it made nothing to select.
pub fn made(result: &Value, graph: &Graph) -> Option<Value> {
    if let Some(g) = result.get("group").and_then(Value::as_str).filter(|g| graph.group(&GroupId::from(*g)).is_some_and(|grp| grp.collapsed)) {
        return Some(json!({"groups": [g]}));
    }
    let ids: Vec<&str> = result.get("nodes").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    (!ids.is_empty()).then(|| json!({"nodes": ids}))
}

/// What the canvas shows: nodes hidden in collapsed groups (by the group whose proxy stands for
/// them), nodes outside an entered group, and the proxies to draw.
#[derive(Debug, Clone, Default)]
pub struct Visibility {
    behind: BTreeMap<NodeId, GroupId>,
    outside: BTreeSet<NodeId>,
    pub proxies: Vec<GroupId>,
}

impl Visibility {
    pub fn new(graph: &Graph, focus: Option<&GroupId>) -> Self {
        let mut v = Visibility::default();
        let inside: Option<BTreeSet<NodeId>> = focus.map(|f| graph.group_members(f));
        if let Some(inside) = &inside {
            v.outside = graph.nodes.keys().filter(|n| !inside.contains(*n)).cloned().collect();
        }
        // The focused group and its ancestors are open while entered.
        let mut open_chain = BTreeSet::new();
        let mut at = focus.cloned();
        while let Some(g) = at {
            if !open_chain.insert(g.clone()) {
                break;
            }
            at = graph.group(&g).and_then(|grp| grp.parent.clone());
        }
        let collapsed = |g: &GroupId| graph.group(g).is_some_and(|grp| grp.collapsed) && !open_chain.contains(g);
        for gid in graph.groups.keys() {
            if !collapsed(gid) {
                continue;
            }
            // Only the outermost collapsed group gets a proxy.
            let mut up = graph.group(gid).and_then(|g| g.parent.clone());
            let mut covered = false;
            let mut steps = 0usize;
            while let Some(p) = up {
                steps = steps.saturating_add(1);
                if collapsed(&p) || steps > graph.groups.len() {
                    covered = true;
                    break;
                }
                up = graph.group(&p).and_then(|g| g.parent.clone());
            }
            if covered {
                continue;
            }
            let members = graph.group_members(gid);
            if members.is_empty() || inside.as_ref().is_some_and(|i| !members.iter().all(|m| i.contains(m))) {
                continue;
            }
            for m in members {
                v.behind.insert(m, gid.clone());
            }
            v.proxies.push(gid.clone());
        }
        v
    }

    /// Is `node` drawn as itself?
    pub fn shows(&self, node: &NodeId) -> bool {
        !self.behind.contains_key(node) && !self.outside.contains(node)
    }

    /// The collapsed group whose proxy stands for `node`.
    pub fn proxy_of(&self, node: &NodeId) -> Option<&GroupId> {
        self.behind.get(node)
    }

    pub fn is_outside(&self, node: &NodeId) -> bool {
        self.outside.contains(node)
    }
}

/// Select All: every node the canvas shows (an entered group's members only), and each
/// collapsed group shown as its proxy, in the order the canvas draws them.
pub fn all(graph: &Graph, focus: Option<&GroupId>) -> (Vec<NodeId>, Vec<GroupId>) {
    let vis = Visibility::new(graph, focus);
    let nodes = graph.nodes.keys().filter(|n| vis.shows(n)).cloned().collect();
    (nodes, vis.proxies)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph() -> (Graph, NodeId, NodeId, NodeId) {
        let c = plumcraft_text::engine().schema();
        let mut g = Graph::new();
        let a = g.add_node(c.node("text.literal").unwrap(), [0.0, 0.0]);
        let b = g.add_node(c.node("text.upper").unwrap(), [300.0, 0.0]);
        let d = g.add_node(c.node("text.upper").unwrap(), [600.0, 0.0]);
        (g, a, b, d)
    }

    #[test]
    fn modes_combine_as_clicks_do() {
        let (g, a, b, d) = graph();
        let mut s = Selection::default();
        let (n, gr, m) = parse_set(&g, &json!({"nodes": [a.0, b.0]})).unwrap();
        s.apply(m, n, gr);
        assert_eq!(s.nodes, [a.clone(), b.clone()]);
        s.apply(Mode::Add, vec![d.clone(), a.clone()], vec![]);
        assert_eq!(s.nodes, [a.clone(), b.clone(), d.clone()], "added once");
        s.apply(Mode::Remove, vec![b.clone()], vec![]);
        assert_eq!(s.nodes, [a.clone(), d.clone()]);
        s.apply(Mode::Toggle, vec![a.clone(), b.clone()], vec![]);
        assert_eq!(s.nodes, [d.clone(), b.clone()]);
        s.apply(Mode::Replace, vec![], vec![]);
        assert!(s.is_empty());
    }

    #[test]
    fn hostile_params_are_refused() {
        let (g, a, ..) = graph();
        for bad in [
            json!({"nodes": "n1"}),
            json!({"nodes": [1]}),
            json!({"nodes": ["nope"]}),
            json!({"groups": ["g9"]}),
            json!({"mode": "xor"}),
            json!({"mode": 3}),
            json!({"nodes": [a.0], "extra": 1}),
            json!([1, 2]),
        ] {
            assert!(parse_set(&g, &bad).is_err(), "{bad}");
        }
        let long = "x".repeat(10_000);
        let e = parse_set(&g, &json!({"nodes": [long]})).unwrap_err();
        assert!(e.len() < 100, "an unknown id is cut short: {e}");
        assert!(parse_set(&g, &Value::Null).is_ok(), "nothing named: select nothing");
    }

    #[test]
    fn gone_nodes_and_nets_are_forgotten() {
        let (mut g, a, b, _) = graph();
        let mut s = Selection { nodes: vec![a.clone(), b.clone()], groups: vec![GroupId::from("g9")], net: Some(NetKey::top("nope")) };
        g.nodes.remove(&b);
        assert!(s.retain_existing(&g));
        assert_eq!(s, Selection { nodes: vec![a], groups: vec![], net: None });
        assert!(!s.retain_existing(&g), "nothing more to drop");
    }

    #[test]
    fn select_all_takes_what_is_shown() {
        let (mut g, a, b, d) = graph();
        let gid = g.create_group("G", &[b.clone(), d.clone()], &[]).unwrap();
        assert_eq!(all(&g, None), (vec![a.clone(), b.clone(), d.clone()], vec![]));
        g.set_collapsed(&gid, true).unwrap();
        assert_eq!(all(&g, None), (vec![a.clone()], vec![gid.clone()]), "a collapsed group as its proxy");
        assert_eq!(all(&g, Some(&gid)), (vec![b.clone(), d.clone()], vec![]), "inside an entered group, its members");
        let mut s = Selection { nodes: vec![a.clone()], groups: vec![gid.clone()], net: None };
        assert_eq!(s.members(&g), [a.clone(), b.clone(), d.clone()]);
        s.set_group(gid.clone());
        assert_eq!(made(&json!({"group": gid.0}), &g), Some(json!({"groups": [gid.0]})));
        assert_eq!(made(&json!({"nodes": []}), &g), None);
    }

    #[test]
    fn handles_share_one_selection() {
        let cell = SelectionCell::default();
        let other = cell.clone();
        other.update(|s| s.set(vec![NodeId::from("n1")]));
        assert_eq!(cell.get().nodes, [NodeId::from("n1")]);
        assert!(cell.same(&other) && !cell.same(&SelectionCell::default()));
    }
}
