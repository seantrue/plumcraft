//! The graph model: nodes with widget values and positions, wires between named ports, type
//! checking against the engine's catalogue, and workflow JSON.
//!
//! Lifted from the research software's `research-graph` (2026-10-27, the research software `8d96021`) and generalised:
//! PlumCraft's protocol types and the catalogue's type rules (plan 10.3, E1.1). Groups came with E3 ([`groups`], [`insert`]), bend points, net labels
//! ([`nets`]), wire slices and [`frames`] later.
//!
//! **What it doesn't know, it keeps.** Fields of a workflow, node, wire or group this crate has no
//! name for land in their `extra` and are written back, so opening and saving the research software file
//! loses nothing, though nothing here acts on it.
//!
//! **Groups** ([`groups`]) are an overlay: membership, nesting, promoted ports, outputs and
//! checkpoints. The
//! engine runs the flat graph and ignores them.
//!
//! **Widget becomes socket:** a `both` port keeps its widget value when it is wired, but the value
//! is marked *inherited* (`Node::inherited`): the wired value wins, and disconnecting restores the
//! widget. Socket-only ports have no widget value, so they are never marked.
//!
//! **Type rules** ([`ports`]): the engine's catalogue decides (`SchemaReply::accepts`: the same
//! type, plus its conversions); a list feeds a list input whole or a single input item by item;
//! a single value can't feed a list input. Inputs are addressed by name, plus variadic slots
//! (`c1` …) and choice sub-ports (`mode.turn`).
//!
//! The graph records values and wires; which of wire, widget and default wins when a node runs
//! is the engine's job (spec 7.6).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod frames;
pub mod groups;
pub mod insert;
pub mod nets;
pub mod ports;

use std::collections::{BTreeMap, BTreeSet};

use plumcraft_proto::catalogue::{NodeDef, SchemaReply};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use frames::{Frame, FrameId, Held, MAX_FRAME_COORD, MAX_FRAME_TITLE, MIN_FRAME_SIDE};
pub use groups::{Group, GroupId, GroupOutput, Promoted, PromotedEdit, check_name, is_name};
pub use insert::Inserted;
pub use nets::{MAX_LABEL_OFFSET, MAX_NET_CHARS, Net, NetKey, Scope, check_net_name};
pub use plumcraft_proto::slice::{Slice, parse as parse_slice};
pub use ports::{InputKind, ResolvedInput, check_sliced_wire, check_wire, input_ports, resolve_input, variadic_slots};

/// The workflow file format this crate writes and reads.
pub const WORKFLOW_VERSION: u32 = 1;

/// Errors editing or loading a graph.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("no node {0}")]
    UnknownNode(NodeId),
    #[error("unknown node type {0:?} (not in the engine's schema)")]
    UnknownKind(String),
    #[error("node {node} ({kind}) has no {dir} port {port:?}")]
    UnknownPort { node: NodeId, kind: String, dir: &'static str, port: String },
    #[error("{0} is a widget-only port and can't be wired")]
    NotWireable(PortRef),
    #[error("type mismatch: {from} is {from_ty}, {to} is {to_ty}")]
    TypeMismatch { from: String, from_ty: String, to: String, to_ty: String },
    #[error("{to} expects a list of {ty}, but {from} gives a single value")]
    ExpectsList { from: String, to: String, ty: String },
    #[error("{at} is variadic: wire into one of its slots ({slots})")]
    VariadicPort { at: PortRef, slots: String },
    #[error("{at} is only available while {parent} is {needs} (it is {now})")]
    InactiveSubPort { at: PortRef, parent: String, needs: String, now: String },
    #[error("a node can't be wired to itself")]
    SelfLoop,
    #[error("wiring {from} to {to} would make a cycle")]
    Cycle { from: PortRef, to: PortRef },
    #[error("{0} is not wired")]
    NotWired(PortRef),
    #[error("{0} has no widget")]
    NoWidget(PortRef),
    #[error("unsupported workflow version {0} (this PlumCraft reads v{WORKFLOW_VERSION})")]
    Version(u32),
    #[error("malformed workflow JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("wire {0} -> {1} names a missing node")]
    Dangling(PortRef, PortRef),
    #[error("no nodes given")]
    NoNodes,
    #[error("no group {0}")]
    UnknownGroup(GroupId),
    #[error("group {group} already has {what} {name:?}")]
    Duplicate { what: &'static str, name: String, group: GroupId },
    #[error("group {group} has no {what} {name:?}")]
    NoSuch { what: &'static str, name: String, group: GroupId },
    #[error("node {node} is not in group {group}")]
    NotMember { group: GroupId, node: NodeId },
    #[error("node {node} ({kind}) can't be marked: the catalogue doesn't call its type markable")]
    NotMarkable { node: NodeId, kind: String },
    #[error("a group needs at least one node or group")]
    EmptyGroup,
    #[error("the nodes and groups to group must all sit in the same enclosing group")]
    MixedGroups,
    #[error("broken group: {0}")]
    GroupInvariant(String),
    #[error("{what} {name:?} must be 1 to {MAX_TITLE_CHARS} characters, not blank, without control characters")]
    BadName { what: &'static str, name: String },
    #[error("bad range for {label:?}: {reason}")]
    BadRange { label: String, reason: String },
    #[error("{0}")]
    Broken(String),
    #[error("the wire {from} -> {to} can't be sliced: {reason}")]
    BadSlice { from: String, to: String, reason: String },
    #[error("bad bend points: {0}")]
    BadVia(String),
    #[error("net label {0:?}: {1}")]
    BadNetName(String, &'static str),
    #[error("net {name:?} is driven by {by}")]
    NetDriven { name: String, by: PortRef },
    #[error("no net {0:?}")]
    UnknownNet(String),
    #[error("a label sits within ±{MAX_LABEL_OFFSET} of its pin")]
    BadLabelOffset,
    #[error("no frame {0}")]
    UnknownFrame(FrameId),
    #[error("frame: {0}")]
    BadFrame(String),
}

pub type Result<T, E = GraphError> = std::result::Result<T, E>;

/// A node's id in its graph (`n1`, `n2`, …), as the protocol names nodes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub String);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        NodeId(s.to_owned())
    }
}

/// One end of a wire: a node and one of its port names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PortRef {
    pub node: NodeId,
    pub port: String,
}

impl PortRef {
    pub fn new(node: impl Into<NodeId>, port: impl Into<String>) -> Self {
        PortRef { node: node.into(), port: port.into() }
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self {
        NodeId(s)
    }
}

impl std::fmt::Display for PortRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.node, self.port)
    }
}

/// A wire from an output to an input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wire {
    pub from: PortRef,
    pub to: PortRef,
    /// A slice of the list the wire carries, in Python syntax: `start:stop:step` (`1:-1:2`, `:-1`)
    /// or an index (`-1`). A range keeps it a list and an index takes one item (spec 7.6 rule 3).
    /// Part of what runs: a slice changes the result, and the memo key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice: Option<String>,
    /// Bend points on the canvas (graph units), from the output to the input: the wire is
    /// drawn as a smooth curve through them (lifted from the research software). Only how it looks; engines
    /// ignore it, and moving one doesn't make anything stale.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<[i32; 2]>,
    /// The net the wire is drawn through ([`nets`]): the canvas draws a label at each end
    /// instead of the whole wire, as a schematic does. Only how it looks; engines ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Where the receiving label sits, as an offset from the input pin (graph units); unset:
    /// beside the pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_at: Option<[i32; 2]>,
    /// Fields this editor doesn't know, kept for the file.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// The most bend points a wire keeps.
pub const MAX_VIA: usize = 32;
/// The largest bend point coordinate.
pub const MAX_VIA_COORD: i32 = 10_000_000;

impl Wire {
    pub fn new(from: PortRef, to: PortRef) -> Self {
        Wire { from, to, slice: None, via: Vec::new(), label: None, label_at: None, extra: BTreeMap::new() }
    }

    /// The slice, parsed (`None`: no slice; `Some(Err)`: one an engine would refuse).
    pub fn parsed_slice(&self) -> Option<std::result::Result<Slice, String>> {
        self.slice.as_deref().map(parse_slice)
    }
}

/// Bend points that fit [`MAX_VIA`] and [`MAX_VIA_COORD`].
fn check_via(via: &[[i32; 2]]) -> Result<()> {
    if via.len() > MAX_VIA {
        return Err(GraphError::BadVia(format!("at most {MAX_VIA} bend points")));
    }
    if via.iter().flatten().any(|c| c.unsigned_abs() > MAX_VIA_COORD.unsigned_abs()) {
        return Err(GraphError::BadVia(format!("bend points lie within ±{MAX_VIA_COORD}")));
    }
    Ok(())
}

/// A node instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// The `NodeDef` id.
    pub kind: String,
    /// Canvas position.
    #[serde(default)]
    pub pos: [f32; 2],
    /// Widget values by input address (sub-ports as `"<port>.<sub>"`).
    #[serde(default)]
    pub values: BTreeMap<String, Value>,
    /// Ports whose widget value is overridden by a wire. Derived from the wires on load.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub inherited: BTreeSet<String>,
    /// A title the designer gave the node, shown as written. Without one the node is titled
    /// by its type ([`Graph::node_title`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The serial that tells this node apart from others titled by the same type name, shown
    /// only when there are others ([`Graph::assign_serials`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<u64>,
    /// How the canvas shows the node: in full, without its preview, or collapsed to its title.
    #[serde(default, skip_serializing_if = "NodeView::is_full")]
    pub view: NodeView,
    /// Show the fields its choice doesn't use ([`ports::tucked`]) in full.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_fields: bool,
    /// Whether the engine persists this node's result (spec 14.5): when it was slow (`auto`),
    /// `always` or `never`. Sent with the workflow; it doesn't change what the node computes.
    #[serde(default, skip_serializing_if = "NodeCache::is_auto")]
    pub cache: NodeCache,
    /// The engine that runs the node, when the designer or the editor pinned it (spec 8.2:
    /// several engines offer its kind). Engines ignore it; a coordinator reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    /// Fields this editor doesn't know, kept for the file.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Whether the engine persists a node's result (the workflow node's `cache`, spec 14.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeCache {
    Always,
    Never,
    /// When its own work took the engine's threshold (and what an unknown value reads as).
    #[default]
    #[serde(other)]
    Auto,
}

impl NodeCache {
    pub fn is_auto(&self) -> bool {
        *self == NodeCache::Auto
    }
}

/// How the canvas shows a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeView {
    /// Pins and widgets, no preview.
    Compact,
    /// The title only.
    Collapsed,
    /// Pins, widgets and the preview (and what an unknown value in a file reads as).
    #[default]
    #[serde(other)]
    Full,
}

impl NodeView {
    pub fn is_full(&self) -> bool {
        *self == NodeView::Full
    }
}

/// The longest node title (and group, promoted label or output name) kept, in characters.
pub const MAX_TITLE_CHARS: usize = 200;

/// The longest doc a promoted parameter keeps, in characters.
pub const MAX_DOC_CHARS: usize = 4000;

/// `"pencil 12"` -> `("pencil", Some(12))`; a title without a serial is its own stem.
fn title_stem(title: &str) -> (&str, Option<u64>) {
    match title.rsplit_once(' ') {
        Some((stem, n)) if !stem.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => (stem, n.parse().ok()),
        _ => (title, None),
    }
}

/// Node ids in creation order (`n2` before `n10`).
fn id_order(id: &NodeId) -> (usize, &str) {
    (id.0.len(), id.0.as_str())
}

/// What [`Graph::set_value_checked`] changed besides the value itself (a choice change brings
/// and takes away sub-ports).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetOutcome {
    /// Wires into sub-ports the new choice no longer has.
    pub disconnected: Vec<Wire>,
    /// Widget values of sub-ports the new choice no longer has.
    pub dropped: Vec<String>,
    /// Sub-port widgets the new choice brings, at their defaults.
    pub added: Vec<String>,
    /// Promotions of sub-ports the new choice no longer has: (group, label).
    pub unpromoted: Vec<(GroupId, String)>,
}

/// A node graph, which is also the workflow file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub version: u32,
    #[serde(default)]
    pub nodes: BTreeMap<NodeId, Node>,
    #[serde(default)]
    pub wires: Vec<Wire>,
    /// The next node number to hand out.
    #[serde(default)]
    pub next_id: u64,
    /// What the file says about itself, kept as written; the editor doesn't interpret it.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub meta: serde_json::Map<String, serde_json::Value>,
    /// Groups, an overlay on the flat graph ([`groups`]); not written when there are none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub groups: BTreeMap<GroupId, Group>,
    /// The nets at the top level, by name; a group's own are in [`Group::nets`] ([`nets`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub nets: BTreeMap<String, Net>,
    /// The next group number to hand out (not written while it is the first).
    #[serde(default, skip_serializing_if = "is_first")]
    pub next_group: u64,
    /// Frames: titled rectangles for arranging nodes ([`frames`]); not written when there are
    /// none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub frames: BTreeMap<FrameId, Frame>,
    /// The next frame number to hand out (a file's is kept as written: [`Graph::add_frame`]
    /// looks for a free one).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub next_frame: u64,
    /// The engines the workflow was made with, in order (spec 8.2): a coordinator runs a node
    /// several engines offer on the first of these that does. Written by the editor while
    /// several engines are connected; not written when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub engines: Vec<String>,
    /// Fields this editor doesn't know, kept for the file.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
    /// Bumped by every edit; not saved.
    #[serde(skip)]
    revision: u64,
}

impl Default for Graph {
    fn default() -> Self {
        Graph {
            version: WORKFLOW_VERSION,
            nodes: BTreeMap::new(),
            wires: Vec::new(),
            next_id: 1,
            meta: serde_json::Map::new(),
            groups: BTreeMap::new(),
            nets: BTreeMap::new(),
            next_group: 1,
            frames: BTreeMap::new(),
            next_frame: 0,
            engines: Vec::new(),
            extra: BTreeMap::new(),
            revision: 0,
        }
    }
}

fn is_first(n: &u64) -> bool {
    *n <= 1
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn def_of<'a>(catalog: &'a SchemaReply, graph: &Graph, id: &NodeId) -> Result<&'a NodeDef> {
    let node = graph.nodes.get(id).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
    catalog.node(&node.kind).ok_or_else(|| GraphError::UnknownKind(node.kind.clone()))
}

/// The highest `<prefix><n>` number among `ids`, plus one (at least 1).
fn next_number<'a>(ids: impl Iterator<Item = &'a str>, prefix: char) -> u64 {
    ids.filter_map(|id| id.strip_prefix(prefix)?.parse::<u64>().ok()).max().unwrap_or(0).saturating_add(1)
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    /// The edit counter: compare with an earlier value to see whether anything changed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.prune_nets();
    }

    pub fn node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    fn fresh_node_id(&mut self) -> NodeId {
        let mut id = NodeId(format!("n{}", self.next_id));
        while self.nodes.contains_key(&id) {
            // (past the last number, start again: a file can hold that very id)
            self.next_id = self.next_id.checked_add(1).unwrap_or(1);
            id = NodeId(format!("n{}", self.next_id));
        }
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Add a node of type `def` at `pos`, with each widget (including the sub-ports its default
    /// choices bring) at its default.
    pub fn add_node(&mut self, def: &NodeDef, pos: [f32; 2]) -> NodeId {
        let id = self.fresh_node_id();
        let mut node = Node {
            kind: def.id.clone(),
            pos,
            values: BTreeMap::new(),
            inherited: BTreeSet::new(),
            title: None,
            serial: None,
            view: NodeView::Full,
            cache: NodeCache::Auto,
            engine: None,
            all_fields: false,
            extra: BTreeMap::new(),
        };
        node.values = input_ports(def, &node).into_iter().filter(|(_, p)| p.has_widget()).map(|(n, p)| (n, p.default.clone())).collect();
        ports::drop_filled(def, &mut node);
        self.nodes.insert(id.clone(), node);
        self.touch();
        id
    }

    /// Give serials to the nodes titled by their type (`display(kind)`) that need one: those
    /// without a serial, and each node in `fresh` (just added: a paste or a duplicate) whose
    /// serial another node with the same type name already has. A serial is one past the
    /// highest its type name has in the graph. A node in `fresh` with a designer's title that
    /// another node already has gets a serial added to that title instead.
    pub fn assign_serials(&mut self, fresh: &BTreeSet<NodeId>, display: impl Fn(&str) -> String) {
        let mut ids: Vec<NodeId> = self.nodes.keys().cloned().collect();
        ids.sort_by(|a, b| id_order(a).cmp(&id_order(b)));
        // What each node would otherwise search every other node for: a file's graph is hostile
        // input, and that search made a large one take hours. Kept up to date as titles change.
        let mut stems: BTreeMap<String, String> = BTreeMap::new();
        let mut titles: BTreeMap<String, usize> = BTreeMap::new();
        let mut highest: BTreeMap<String, u64> = BTreeMap::new();
        let mut serials: BTreeMap<String, BTreeMap<u64, usize>> = BTreeMap::new();
        for n in self.nodes.values() {
            if let Some(t) = &n.title {
                *titles.entry(t.clone()).or_default() += 1;
                if let (stem, Some(k)) = title_stem(t) {
                    let top = highest.entry(stem.to_owned()).or_default();
                    *top = k.max(*top);
                }
                continue;
            }
            let stem = stems.entry(n.kind.clone()).or_insert_with(|| display(&n.kind)).clone();
            let held = serials.entry(stem).or_default();
            if let Some(k) = n.serial {
                *held.entry(k).or_default() += 1;
            }
        }
        let mut changed = false;
        for id in ids {
            let Some(node) = self.nodes.get_mut(&id) else { continue };
            if let Some(t) = node.title.clone() {
                if !(fresh.contains(&id) && titles.get(&t).is_some_and(|n| *n > 1)) {
                    continue;
                }
                let stem = title_stem(&t).0.to_owned();
                let top = highest.entry(stem.clone()).or_default();
                *top = top.saturating_add(1);
                let title = format!("{stem} {top}");
                if let Some(n) = titles.get_mut(&t) {
                    *n -= 1;
                }
                *titles.entry(title.clone()).or_default() += 1;
                node.title = Some(title);
                changed = true;
                continue;
            }
            let stem = stems.entry(node.kind.clone()).or_insert_with(|| display(&node.kind)).clone();
            let held = serials.entry(stem).or_default();
            let needs = match node.serial {
                None => true,
                Some(k) => fresh.contains(&id) && held.get(&k).is_some_and(|n| *n > 1),
            };
            if needs {
                if let Some(k) = node.serial
                    && let Some(n) = held.get_mut(&k)
                {
                    *n -= 1;
                }
                let next = held.iter().rev().find(|(_, n)| **n > 0).map_or(0, |(k, _)| *k).saturating_add(1);
                *held.entry(next).or_default() += 1;
                node.serial = Some(next);
                changed = true;
            }
        }
        if changed {
            self.touch();
        }
    }

    /// Workflows saved while titles held their serial (`"pencil 2"`): make such a title, when
    /// it is the type name plus a number, the node's serial again.
    pub fn migrate_serial_titles(&mut self, display: impl Fn(&str) -> String) {
        for node in self.nodes.values_mut() {
            let Some(t) = &node.title else { continue };
            if let (stem, Some(k)) = title_stem(t)
                && node.serial.is_none()
                && stem == display(&node.kind)
            {
                node.serial = Some(k);
                node.title = None;
            }
        }
    }

    /// The title node `id` shows: the designer's title, else its type name, with its serial
    /// when another node is titled by the same type name.
    pub fn node_title(&self, id: &NodeId, display: impl Fn(&str) -> String) -> Option<String> {
        let node = self.nodes.get(id)?;
        if let Some(t) = &node.title {
            return Some(t.clone());
        }
        let stem = display(&node.kind);
        let shared = self.nodes.iter().any(|(o, n)| o != id && n.title.is_none() && display(&n.kind) == stem);
        Some(match node.serial {
            Some(k) if shared => format!("{stem} {k}"),
            _ => stem,
        })
    }

    /// Add a copy of `node` (from another graph, say: a paste) under a fresh id. Its wires are
    /// not copied and nothing is inherited until it is wired.
    pub fn insert_node(&mut self, mut node: Node) -> NodeId {
        let id = self.fresh_node_id();
        node.inherited.clear();
        self.nodes.insert(id.clone(), node);
        self.touch();
        id
    }

    /// The nodes `ids` (unknown ids are skipped) and the wires among them, slices included, as
    /// a graph of their own: what Copy puts on the clipboard. Groups aren't copied (as
    /// The research software's): a paste lands at the top level.
    pub fn subgraph(&self, ids: &[NodeId]) -> Graph {
        let set: BTreeSet<&NodeId> = ids.iter().filter(|id| self.nodes.contains_key(*id)).collect();
        let mut g = Graph::new();
        for id in &set {
            if let Some(n) = self.nodes.get(*id) {
                g.nodes.insert((*id).clone(), n.clone());
            }
        }
        g.wires = self.wires.iter().filter(|w| set.contains(&w.from.node) && set.contains(&w.to.node)).cloned().collect();
        // every net driven from among them, at the top level (groups aren't copied)
        let same: BTreeMap<NodeId, NodeId> = set.iter().map(|&id| (id.clone(), id.clone())).collect();
        let _ = g.adopt_nets(self, &same, &BTreeMap::new());
        g.next_id = next_number(g.nodes.keys().map(|k| k.0.as_str()), 'n');
        g
    }

    /// Remove a node, every wire touching it, and its place in groups (with the promotions and
    /// outputs that named it; a group left empty goes too).
    pub fn remove_node(&mut self, id: &NodeId) -> Result<Node> {
        let node = self.nodes.remove(id).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        let (gone, kept): (Vec<Wire>, Vec<Wire>) = std::mem::take(&mut self.wires).into_iter().partition(|w| &w.from.node == id || &w.to.node == id);
        self.wires = kept;
        for w in gone {
            if let Some(n) = self.nodes.get_mut(&w.to.node) {
                n.inherited.remove(&w.to.port);
            }
        }
        self.drop_from_groups(id);
        self.touch();
        Ok(node)
    }

    /// Copy `ids` (offset by `offset`), with the wires among them; with `keep_inputs`, also the
    /// wires coming into them from outside. Each copy joins its original's innermost group (as
    /// The research software's). Returns (original, copy) pairs.
    pub fn duplicate_nodes(&mut self, ids: &[NodeId], offset: [f32; 2], keep_inputs: bool) -> Result<Vec<(NodeId, NodeId)>> {
        if ids.is_empty() {
            return Err(GraphError::NoNodes);
        }
        let set: BTreeSet<&NodeId> = ids.iter().collect();
        for id in &set {
            if !self.nodes.contains_key(*id) {
                return Err(GraphError::UnknownNode((*id).clone()));
            }
        }
        let mut map = BTreeMap::new();
        let mut pairs = Vec::new();
        for old in set {
            let Some(mut node) = self.nodes.get(old).cloned() else { continue };
            node.pos = [node.pos[0] + offset[0], node.pos[1] + offset[1]];
            node.inherited.clear();
            let new = self.fresh_node_id();
            if let Some(g) = self.group_of(old).cloned()
                && let Some(grp) = self.groups.get_mut(&g)
            {
                grp.nodes.insert(new.clone());
            }
            self.nodes.insert(new.clone(), node);
            map.insert(old.clone(), new.clone());
            pairs.push((old.clone(), new));
        }
        let mut copies = Vec::new();
        for w in &self.wires {
            let Some(to) = map.get(&w.to.node) else { continue };
            let from = match map.get(&w.from.node) {
                Some(f) => PortRef::new(f.clone(), w.from.port.clone()),
                None if keep_inputs => w.from.clone(),
                None => continue,
            };
            // bend points move with the copies; a label stays only on a wire from outside (its
            // net's driver is unchanged)
            let shift = |v: &[i32; 2]| [v[0].saturating_add(offset[0].round() as i32), v[1].saturating_add(offset[1].round() as i32)];
            let to = PortRef::new(to.clone(), w.to.port.clone());
            copies.push(Wire { from, to, via: w.via.iter().map(shift).collect(), ..w.clone() });
        }
        for w in copies {
            if let Some(n) = self.nodes.get_mut(&w.to.node)
                && n.values.contains_key(&w.to.port)
            {
                n.inherited.insert(w.to.port.clone());
            }
            self.wires.push(w);
        }
        self.touch();
        Ok(pairs)
    }

    /// Show node `id` in full, without its preview, or collapsed.
    pub fn set_view(&mut self, id: &NodeId, view: NodeView) -> Result<()> {
        let node = self.nodes.get_mut(id).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        if node.view != view {
            node.view = view;
            self.touch();
        }
        Ok(())
    }

    /// Whether the engine persists a node's result.
    pub fn set_cache(&mut self, id: &NodeId, cache: NodeCache) -> Result<()> {
        let node = self.nodes.get_mut(id).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        if node.cache != cache {
            node.cache = cache;
            self.touch();
        }
        Ok(())
    }

    /// Pin a node to an engine (`None`: whichever offers its kind).
    pub fn set_engine(&mut self, id: &NodeId, engine: Option<String>) -> Result<()> {
        let node = self.nodes.get_mut(id).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        if node.engine != engine {
            node.engine = engine;
            self.touch();
        }
        Ok(())
    }

    /// Show every field of a node, or tuck the ones its choice doesn't use under "more fields"
    /// ([`ports::tucked`]).
    pub fn set_all_fields(&mut self, id: &NodeId, all: bool) -> Result<()> {
        let node = self.nodes.get_mut(id).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        if node.all_fields != all {
            node.all_fields = all;
            self.touch();
        }
        Ok(())
    }

    /// Drop, on every node, the values that only repeat what a filling choice gives them
    /// ([`ports::drop_filled`]): a workflow saved while choosing wrote them all, so a wired
    /// choice changed nothing. Unwired, the graph computes the same. Returns whether any went.
    pub fn drop_filled_values(&mut self, catalog: &SchemaReply) -> bool {
        let mut any = false;
        for node in self.nodes.values_mut() {
            if let Some(def) = catalog.node(&node.kind) {
                any |= !ports::drop_filled(def, node).is_empty();
            }
        }
        any
    }

    /// Move a node on the canvas.
    pub fn move_node(&mut self, id: &NodeId, pos: [f32; 2]) -> Result<()> {
        let node = self.nodes.get_mut(id).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        if node.pos != pos {
            node.pos = pos;
            self.touch();
        }
        Ok(())
    }

    /// Set an existing widget value, without the catalog. The value is kept even while the port
    /// is wired (it shows inherited). Prefer [`Graph::set_value_checked`].
    pub fn set_value(&mut self, at: &PortRef, value: Value) -> Result<()> {
        let node = self.nodes.get_mut(&at.node).ok_or_else(|| GraphError::UnknownNode(at.node.clone()))?;
        let slot = node.values.get_mut(&at.port).ok_or_else(|| GraphError::NoWidget(at.clone()))?;
        *slot = value;
        self.touch();
        Ok(())
    }

    /// Set the widget value of input address `at` (a port, a variadic slot or an active
    /// sub-port), checked against the catalog. When `at` is a choice with `options`, the
    /// sub-ports of the old choice go: their wires are disconnected, their values and promotions
    /// dropped; the new choice's sub-port widgets appear at their defaults. All of that is
    /// reported in the [`SetOutcome`].
    pub fn set_value_checked(&mut self, catalog: &SchemaReply, at: &PortRef, value: Value) -> Result<SetOutcome> {
        let def = def_of(catalog, self, &at.node)?;
        let node = self.nodes.get_mut(&at.node).ok_or_else(|| GraphError::UnknownNode(at.node.clone()))?;
        let resolved = resolve_input(def, node, &at.node, &at.port)?;
        if !resolved.port.has_widget() {
            return Err(GraphError::NoWidget(at.clone()));
        }
        let is_choice = resolved.kind == InputKind::Port && resolved.port.options.is_some();
        // a filling choice gives every port its table names the chosen value: their own values
        // go, so they follow the choice (also when it comes on a wire) and stay editable
        let filled: Vec<String> = (resolved.kind == InputKind::Port)
            .then(|| resolved.port.fills.as_ref().and_then(|f| f.get(&ports::choice_key(&value))))
            .flatten()
            .map(|table| table.keys().filter(|n| n.as_str() != at.port && def.input(n).is_some_and(|p| p.has_widget())).cloned().collect())
            .unwrap_or_default();
        node.values.insert(at.port.clone(), value);
        for n in &filled {
            node.values.remove(n);
        }
        // a value set back to what the choice gives follows the choice again
        ports::drop_filled(def, node);
        let mut out = SetOutcome::default();
        if is_choice {
            let parent = at.port.as_str();
            let after = ports::active_subports(def, node, parent);
            let stale = |name: &str| ports::is_subport_of(def, parent, name) && !after.contains(name);
            out.dropped = node.values.keys().filter(|k| stale(k)).cloned().collect();
            for k in &out.dropped {
                node.values.remove(k);
                node.inherited.remove(k);
            }
            let fresh: Vec<(String, Value)> = input_ports(def, node)
                .into_iter()
                .filter(|(n, p)| after.contains(n) && p.has_widget() && !node.values.contains_key(n))
                .map(|(n, p)| (n, p.default.clone()))
                .collect();
            for (n, v) in fresh {
                out.added.push(n.clone());
                node.values.insert(n, v);
            }
            let (gone, kept): (Vec<Wire>, Vec<Wire>) = std::mem::take(&mut self.wires).into_iter().partition(|w| w.to.node == at.node && stale(&w.to.port));
            self.wires = kept;
            if let Some(n) = self.nodes.get_mut(&at.node) {
                for w in &gone {
                    n.inherited.remove(&w.to.port);
                }
            }
            out.disconnected = gone;
            for (gid, g) in &mut self.groups {
                g.promoted.retain(|p| {
                    let keep = !(p.port.node == at.node && stale(&p.port.port));
                    if !keep {
                        out.unpromoted.push((gid.clone(), p.label.clone()));
                    }
                    keep
                });
            }
        }
        self.touch();
        Ok(out)
    }

    /// The wire into `to`, if any.
    /// Set the bend points of the wire into `to` (empty: a plain curve again).
    pub fn set_via(&mut self, to: &PortRef, via: Vec<[i32; 2]>) -> Result<()> {
        check_via(&via)?;
        let w = self.wires.iter_mut().find(|w| &w.to == to).ok_or_else(|| GraphError::NotWired(to.clone()))?;
        if w.via != via {
            w.via = via;
            self.touch();
        }
        Ok(())
    }

    pub fn wire_into(&self, to: &PortRef) -> Option<&Wire> {
        self.wires.iter().find(|w| &w.to == to)
    }

    /// Is the widget value of `at` overridden by a wire?
    pub fn is_inherited(&self, at: &PortRef) -> bool {
        self.nodes.get(&at.node).is_some_and(|n| n.inherited.contains(&at.port))
    }

    /// Does any path of wires lead from `start` to `goal`?
    fn reaches(&self, start: &NodeId, goal: &NodeId) -> bool {
        let mut seen = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(n) = stack.pop() {
            if n == goal {
                return true;
            }
            if seen.insert(n) {
                stack.extend(self.wires.iter().filter(|w| &w.from.node == n).map(|w| &w.to.node));
            }
        }
        false
    }

    /// Does output `at` carry a list? Either its port is `list: true`, or its node is mapped: a
    /// list arrives at one of its single (`list: false`) inputs, and the engine runs it once per
    /// item (unless the node takes `input_list`).
    pub fn output_is_list(&self, catalog: &SchemaReply, at: &PortRef) -> bool {
        let Ok(def) = def_of(catalog, self, &at.node) else { return false };
        def.output(&at.port).is_some_and(|p| p.list) || self.is_mapped(catalog, &at.node, &mut BTreeMap::new())
    }

    /// Is `id` run once per item of a list? Memoized over the (acyclic) upstream graph.
    fn is_mapped(&self, catalog: &SchemaReply, id: &NodeId, memo: &mut BTreeMap<NodeId, bool>) -> bool {
        if let Some(&m) = memo.get(id) {
            return m;
        }
        memo.insert(id.clone(), false); // guards against a (corrupt) cycle
        let (Some(node), Ok(def)) = (self.nodes.get(id), def_of(catalog, self, id)) else { return false };
        let mapped = !def.flags.input_list
            && self.wires.iter().filter(|w| &w.to.node == id).any(|w| {
                let single = resolve_input(def, node, id, &w.to.port).is_ok_and(|r| !r.port.list);
                // An index takes one item of the list: nothing to map over.
                let indexed = matches!(w.parsed_slice(), Some(Ok(s)) if s.is_index());
                single
                    && !indexed
                    && match def_of(catalog, self, &w.from.node) {
                        Ok(src) => src.output(&w.from.port).is_some_and(|p| p.list) || self.is_mapped(catalog, &w.from.node, memo),
                        Err(_) => false,
                    }
            });
        memo.insert(id.clone(), mapped);
        mapped
    }

    /// Wire output `from` to input `to`, checking both against the catalog (ports, variadic
    /// slots, active sub-ports, wireability, types and list-ness, cycles). An input takes one
    /// wire, so an existing wire into `to` is replaced and returned.
    pub fn connect(&mut self, catalog: &SchemaReply, from: PortRef, to: PortRef) -> Result<Option<Wire>> {
        self.connect_sliced(catalog, from, to, None)
    }

    /// [`Graph::connect`] with an optional slice on the wire, checked ([`parse_slice`]) and
    /// typed: the type rules apply to what the slice leaves ([`check_sliced_wire`]).
    pub fn connect_sliced(&mut self, catalog: &SchemaReply, from: PortRef, to: PortRef, slice: Option<String>) -> Result<Option<Wire>> {
        let from_def = def_of(catalog, self, &from.node)?;
        let to_def = def_of(catalog, self, &to.node)?;
        let out = from_def.output(&from.port).ok_or_else(|| GraphError::UnknownPort {
            node: from.node.clone(),
            kind: from_def.id.clone(),
            dir: "output",
            port: from.port.clone(),
        })?;
        let to_node = self.nodes.get(&to.node).ok_or_else(|| GraphError::UnknownNode(to.node.clone()))?;
        let inp = resolve_input(to_def, to_node, &to.node, &to.port)?.port;
        if !inp.wireable() {
            return Err(GraphError::NotWireable(to));
        }
        if from.node == to.node {
            return Err(GraphError::SelfLoop);
        }
        let parsed = match slice.as_deref().map(parse_slice).transpose() {
            Ok(p) => p,
            Err(reason) => return Err(GraphError::BadSlice { from: from.to_string(), to: to.to_string(), reason }),
        };
        check_sliced_wire(catalog, &from, out, self.output_is_list(catalog, &from), parsed.as_ref(), &to, inp)?;
        if self.reaches(&to.node, &from.node) {
            return Err(GraphError::Cycle { from, to });
        }
        let replaced = self.wires.iter().position(|w| w.to == to).map(|i| self.wires.remove(i));
        if let Some(node) = self.nodes.get_mut(&to.node)
            && node.values.contains_key(&to.port)
        {
            node.inherited.insert(to.port.clone());
        }
        self.wires.push(Wire { slice: parsed.map(|p| p.to_string()), ..Wire::new(from, to) });
        self.touch();
        Ok(replaced)
    }

    /// Set (or with `None`, clear) the slice on the wire into `to`, checked and typed as
    /// [`Graph::connect_sliced`] does, and stored in canonical form (`1:-1:2`, `-1`). Returns the
    /// previous slice. Other wires a change of mapping breaks downstream are left for
    /// [`Graph::wire_problems`] to report, as for any other edit.
    pub fn set_slice(&mut self, catalog: &SchemaReply, to: &PortRef, slice: Option<&str>) -> Result<Option<String>> {
        let at = self.wires.iter().position(|w| &w.to == to).ok_or_else(|| GraphError::NotWired(to.clone()))?;
        let from = self.wires.get(at).map(|w| w.from.clone()).ok_or_else(|| GraphError::NotWired(to.clone()))?;
        let parsed = match slice.map(parse_slice).transpose() {
            Ok(p) => p,
            Err(reason) => return Err(GraphError::BadSlice { from: from.to_string(), to: to.to_string(), reason }),
        };
        let from_def = def_of(catalog, self, &from.node)?;
        let to_def = def_of(catalog, self, &to.node)?;
        let out = from_def.output(&from.port).ok_or_else(|| GraphError::UnknownPort {
            node: from.node.clone(),
            kind: from_def.id.clone(),
            dir: "output",
            port: from.port.clone(),
        })?;
        let to_node = self.nodes.get(&to.node).ok_or_else(|| GraphError::UnknownNode(to.node.clone()))?;
        let inp = resolve_input(to_def, to_node, &to.node, &to.port)?.port;
        check_sliced_wire(catalog, &from, out, self.output_is_list(catalog, &from), parsed.as_ref(), to, inp)?;
        let new = parsed.map(|p| p.to_string());
        let Some(wire) = self.wires.get_mut(at) else { return Err(GraphError::NotWired(to.clone())) };
        let old = std::mem::replace(&mut wire.slice, new);
        if old != wire.slice {
            self.touch();
        }
        Ok(old)
    }

    /// Remove the wire into `to`; its widget value (if any) is no longer inherited.
    pub fn disconnect(&mut self, to: &PortRef) -> Result<Wire> {
        let i = self.wires.iter().position(|w| &w.to == to).ok_or_else(|| GraphError::NotWired(to.clone()))?;
        let wire = self.wires.remove(i);
        if let Some(node) = self.nodes.get_mut(&to.node) {
            node.inherited.remove(&to.port);
        }
        self.touch();
        Ok(wire)
    }

    /// Wires that no longer pass the type rules (after an upstream change stopped a node being
    /// mapped, say, or a new engine schema), each with the reason. For the UI to flag.
    pub fn wire_problems(&self, catalog: &SchemaReply) -> Vec<(Wire, String)> {
        let check = |w: &Wire| -> Result<()> {
            let from_def = def_of(catalog, self, &w.from.node)?;
            let to_def = def_of(catalog, self, &w.to.node)?;
            let out = from_def.output(&w.from.port).ok_or_else(|| GraphError::UnknownPort {
                node: w.from.node.clone(),
                kind: from_def.id.clone(),
                dir: "output",
                port: w.from.port.clone(),
            })?;
            let to_node = self.nodes.get(&w.to.node).ok_or_else(|| GraphError::UnknownNode(w.to.node.clone()))?;
            let inp = resolve_input(to_def, to_node, &w.to.node, &w.to.port)?.port;
            if !inp.wireable() {
                return Err(GraphError::NotWireable(w.to.clone()));
            }
            let parsed = match w.parsed_slice().transpose() {
                Ok(p) => p,
                Err(reason) => return Err(GraphError::BadSlice { from: w.from.to_string(), to: w.to.to_string(), reason }),
            };
            check_sliced_wire(catalog, &w.from, out, self.output_is_list(catalog, &w.from), parsed.as_ref(), &w.to, inp)
        };
        self.wires.iter().filter_map(|w| check(w).err().map(|e| (w.clone(), e.to_string()))).collect()
    }

    /// Check every invariant that must hold after an edit: every node's kind is in `catalog`,
    /// the group invariants hold (ports checked against `catalog`), and every wire passes the
    /// type rules.
    pub fn check(&self, catalog: &SchemaReply) -> Result<()> {
        if let Some((id, n)) = self.nodes.iter().find(|(_, n)| catalog.node(&n.kind).is_none()) {
            return Err(GraphError::Broken(format!("node {id}: unknown kind {:?}", n.kind)));
        }
        self.check_groups(Some(catalog))?;
        match self.wire_problems(catalog).into_iter().next() {
            Some((w, why)) => Err(GraphError::Broken(format!("wire {} -> {}: {why}", w.from, w.to))),
            None => Ok(()),
        }
    }

    /// Workflow JSON.
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Read workflow JSON: checks the version, that wires name existing nodes and the group
    /// invariants (structure only: ports need the catalog, see [`Graph::check`]), and derives
    /// `inherited` from the wires (a file can't make it disagree with them). Fields this editor
    /// doesn't know are kept and
    /// written back, and nothing acts on them. Net labels are only how wires look: a bad one is dropped, its wire kept.
    pub fn from_json(text: &str) -> Result<Graph> {
        let head: Value = serde_json::from_str(text)?;
        let version = head.get("version").and_then(Value::as_u64).unwrap_or(0);
        if version != u64::from(WORKFLOW_VERSION) {
            return Err(GraphError::Version(u32::try_from(version).unwrap_or(u32::MAX)));
        }
        let mut g: Graph = serde_json::from_value(head)?;
        for w in &g.wires {
            if !g.nodes.contains_key(&w.from.node) || !g.nodes.contains_key(&w.to.node) {
                return Err(GraphError::Dangling(w.from.clone(), w.to.clone()));
            }
            check_via(&w.via).map_err(|e| GraphError::BadVia(format!("the wire into {}: {e}", w.to)))?;
        }
        g.check_groups(None)?;
        g.check_frames()?;
        for node in g.nodes.values_mut() {
            node.inherited.clear();
        }
        for w in &g.wires {
            if let Some(node) = g.nodes.get_mut(&w.to.node)
                && node.values.contains_key(&w.to.port)
            {
                node.inherited.insert(w.to.port.clone());
            }
        }
        g.next_id = g.next_id.max(next_number(g.nodes.keys().map(|k| k.0.as_str()), 'n'));
        g.next_group = g.next_group.max(next_number(g.groups.keys().map(|k| k.0.as_str()), 'g'));
        g.prune_nets();
        Ok(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A small catalogue: text nodes like the text engine's, plus a choice port with sub-ports, a
    /// variadic port, a list output and an engine conversion (TEXT → STRING).
    pub(crate) fn catalog() -> SchemaReply {
        serde_json::from_value(json!({"v": 2,
            "compat": [{"from": "TEXT", "to": "STRING"}, {"from": "INT", "to": "FLOAT"}],
            "nodes": [
                {"id": "text.literal", "inputs": [{"name": "text", "type": "STRING", "default": "", "socket": "widget"}], "outputs": [{"name": "text", "type": "TEXT"}]},
                {"id": "text.upper", "inputs": [{"name": "text", "type": "TEXT", "socket": "socket"}], "outputs": [{"name": "text", "type": "TEXT"}]},
                {"id": "text.words", "inputs": [{"name": "text", "type": "TEXT", "socket": "socket"}], "outputs": [{"name": "count", "type": "INT"}]},
                {"id": "text.slow", "inputs": [{"name": "text", "type": "TEXT", "socket": "socket"}, {"name": "seconds", "type": "FLOAT", "default": 1.0, "min": 0.0, "max": 60.0, "socket": "both"}],
                 "outputs": [{"name": "text", "type": "TEXT"}]},
                {"id": "text.label", "inputs": [{"name": "label", "type": "STRING", "default": "x", "socket": "both"}], "outputs": [{"name": "text", "type": "TEXT"}]},
                {"id": "text.case", "inputs": [
                    {"name": "text", "type": "TEXT", "socket": "socket"},
                    {"name": "mode", "type": "ENUM", "default": "upper", "choices": ["upper", "title"],
                     "options": {"title": [{"name": "small", "type": "STRING", "default": "a,of"}]}}],
                 "outputs": [{"name": "text", "type": "TEXT"}]},
                {"id": "text.concat", "inputs": [{"name": "c", "type": "TEXT", "socket": "socket", "variadic": {"template": "c{n}", "min": 1, "max": 3}}],
                 "outputs": [{"name": "text", "type": "TEXT"}]},
                {"id": "text.split", "inputs": [{"name": "text", "type": "TEXT", "socket": "socket"}], "outputs": [{"name": "parts", "type": "TEXT", "list": true}]},
                {"id": "text.join_all", "inputs": [{"name": "parts", "type": "TEXT", "list": true, "socket": "socket"}], "outputs": [{"name": "text", "type": "TEXT"}]}
            ]}))
        .unwrap()
    }

    pub(crate) fn add(g: &mut Graph, c: &SchemaReply, kind: &str) -> NodeId {
        g.add_node(c.node(kind).unwrap(), [0.0, 0.0])
    }

    fn p(n: &NodeId, port: &str) -> PortRef {
        PortRef::new(n.clone(), port)
    }

    /// Bend points (lifted from the research software): kept within bounds, saved and refused when out of
    /// them on load, moved with pasted copies and with a group's members (not a wire from
    /// outside), and gone when the input is wired anew.
    #[test]
    fn bend_points_are_kept_bounded_and_move_with_copies_and_groups() {
        let c = catalog();
        let mut g = Graph::new();
        let (l, u, w) = (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.upper"), add(&mut g, &c, "text.words"));
        g.connect(&c, p(&l, "text"), p(&u, "text")).unwrap();
        g.connect(&c, p(&u, "text"), p(&w, "text")).unwrap();
        g.set_via(&p(&u, "text"), vec![[100, 50], [200, 60]]).unwrap();
        assert_eq!(g.wire_into(&p(&u, "text")).unwrap().via, vec![[100, 50], [200, 60]]);
        assert!(g.set_via(&p(&u, "text"), vec![[0, 0]; MAX_VIA + 1]).is_err());
        assert!(g.set_via(&p(&u, "text"), vec![[MAX_VIA_COORD + 1, 0]]).is_err());
        assert!(g.set_via(&p(&l, "text"), vec![[0, 0]]).is_err(), "not wired");
        // saved and read back; a file with too many is refused; a plain wire writes no via
        let text = g.to_json().unwrap();
        assert_eq!(Graph::from_json(&text).unwrap().wire_into(&p(&u, "text")).unwrap().via, vec![[100, 50], [200, 60]]);
        assert_eq!(text.matches("\"via\"").count(), 1);
        let mut many = g.clone();
        many.wires[0].via = vec![[1, 1]; MAX_VIA + 1];
        assert!(matches!(Graph::from_json(&many.to_json().unwrap()), Err(GraphError::BadVia(_))));
        // pasted copies bend where their nodes went
        let clip = g.subgraph(&[l.clone(), u.clone()]);
        let ins = g.insert_graph(&c, &clip, [40.0, 40.0]).unwrap();
        let nu = ins.nodes.get(&u).unwrap();
        assert_eq!(g.wire_into(&p(nu, "text")).unwrap().via, vec![[140, 90], [240, 100]]);
        // a group's move takes the bends between its members, not a wire from outside
        g.set_via(&p(&w, "text"), vec![[300, 0]]).unwrap();
        let grp = g.create_group("G", &[l.clone(), u.clone()], &[]).unwrap();
        let at = g.group(&grp).unwrap().pos;
        g.move_group(&grp, [at[0] + 10.0, at[1] + 20.0]).unwrap();
        assert_eq!(g.wire_into(&p(&u, "text")).unwrap().via, vec![[110, 70], [210, 80]], "between members");
        assert_eq!(g.wire_into(&p(&w, "text")).unwrap().via, vec![[300, 0]], "from inside to outside");
        // a new wire into the input replaces the bent one, and starts plain
        let old = g.connect(&c, p(&l, "text"), p(&w, "text")).unwrap();
        assert_eq!(old.map(|o| o.via), Some(vec![[300, 0]]));
        assert!(g.wire_into(&p(&w, "text")).unwrap().via.is_empty());
    }

    /// Wire slices (lifted from the research software): checked, canonical, typed by what they leave, carried
    /// by copies and files; an index stops a node mapping.
    #[test]
    fn slices_are_checked_typed_and_kept() {
        let c = catalog();
        let mut g = Graph::new();
        let (l, split, up, join) =
            (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.split"), add(&mut g, &c, "text.upper"), add(&mut g, &c, "text.join_all"));
        g.connect(&c, p(&l, "text"), p(&split, "text")).unwrap();
        g.connect(&c, p(&split, "parts"), p(&up, "text")).unwrap();
        g.connect(&c, p(&split, "parts"), p(&join, "parts")).unwrap();
        assert!(g.output_is_list(&c, &p(&up, "text")), "mapped over the list");
        assert_eq!(g.set_slice(&c, &p(&up, "text"), Some(" 1 : -1 ")).unwrap(), None);
        assert_eq!(g.wire_into(&p(&up, "text")).unwrap().slice.as_deref(), Some("1:-1"), "canonical");
        assert!(g.output_is_list(&c, &p(&up, "text")), "a range keeps it a list");
        g.set_slice(&c, &p(&up, "text"), Some("-1")).unwrap();
        assert!(!g.output_is_list(&c, &p(&up, "text")), "an index is one item: nothing to map");
        // the type rules apply to what the slice leaves
        assert!(matches!(g.set_slice(&c, &p(&join, "parts"), Some("0")), Err(GraphError::ExpectsList { .. })));
        assert!(matches!(g.set_slice(&c, &p(&join, "parts"), Some("x")), Err(GraphError::BadSlice { .. })));
        assert!(matches!(g.set_slice(&c, &p(&split, "text"), Some("0")), Err(GraphError::BadSlice { .. })), "a single value");
        assert!(matches!(g.set_slice(&c, &p(&l, "text"), Some("0")), Err(GraphError::NotWired(_))));
        // files, copies
        let back = Graph::from_json(&g.to_json().unwrap()).unwrap();
        assert_eq!(back.wire_into(&p(&up, "text")).unwrap().slice.as_deref(), Some("-1"));
        let pairs = g.duplicate_nodes(std::slice::from_ref(&up), [0.0, 40.0], true).unwrap();
        assert_eq!(g.wire_into(&p(&pairs[0].1, "text")).unwrap().slice.as_deref(), Some("-1"));
        let clip = g.subgraph(&[split.clone(), up.clone()]);
        let ins = g.insert_graph(&c, &clip, [0.0, 400.0]).unwrap();
        assert_eq!(g.wire_into(&p(&ins.nodes[&up], "text")).unwrap().slice.as_deref(), Some("-1"));
        // a file with a bad slice opens, and the wire is flagged
        let mut bad = g.clone();
        bad.wires.iter_mut().for_each(|w| {
            if w.to == p(&up, "text") {
                w.slice = Some("junk".into())
            }
        });
        let bad = Graph::from_json(&bad.to_json().unwrap()).unwrap();
        assert!(bad.wire_problems(&c).iter().any(|(w, why)| w.to == p(&up, "text") && why.contains("junk")));
        assert_eq!(g.set_slice(&c, &p(&up, "text"), None).unwrap().as_deref(), Some("-1"));
    }

    /// What this editor doesn't know survives opening and saving, at every level.
    #[test]
    fn unknown_fields_are_kept_through_a_file() {
        let text = r#"{"version": 1, "bookmarks": {"b1": {"name": "A"}},
            "nodes": {"a": {"kind": "text.literal", "pinned": true}, "b": {"kind": "text.upper"}},
            "wires": [{"from": {"node": "a", "port": "text"}, "to": {"node": "b", "port": "text"}, "tint": "red"}],
            "groups": {"g1": {"name": "G", "nodes": ["a"], "checkpoints": ["x"]}}}"#;
        let g = Graph::from_json(text).unwrap();
        let back: Value = serde_json::from_str(&g.to_json().unwrap()).unwrap();
        assert_eq!(back["bookmarks"], json!({"b1": {"name": "A"}}));
        assert_eq!(back["nodes"]["a"]["pinned"], json!(true));
        assert_eq!(back["wires"][0]["tint"], json!("red"));
        assert_eq!(back["groups"]["g1"]["checkpoints"], json!(["x"]));
        assert_eq!(Graph::from_json(&g.to_json().unwrap()).unwrap().extra, g.extra);
    }

    /// A file can hold the last id there is: a new one is still found, not looked for forever.
    #[test]
    fn fresh_ids_are_found_past_the_last_number() {
        let c = catalog();
        let (n, g) = (format!("n{}", u64::MAX), format!("g{}", u64::MAX));
        let text = json!({"version": 1, "nodes": {&n: {"kind": "text.upper"}}, "groups": {&g: {"name": "G", "nodes": [&n]}}});
        let mut graph = Graph::from_json(&text.to_string()).unwrap();
        assert_eq!(add(&mut graph, &c, "text.upper"), NodeId::from("n1"));
        let id = graph.create_group("H", &[NodeId::from("n1")], &[]).unwrap();
        assert_eq!(id, GroupId::from("g1"));
    }

    /// A node whose preset fills `width`, with `spare` used only by the second preset.
    fn preset_catalogue() -> SchemaReply {
        let fills = json!({"a": {"width": 1.0, "spare": 0.0}, "b": {"width": 2.0, "spare": 5.0}});
        let shows = json!({"a": ["width"], "b": ["width", "spare"]});
        let def: NodeDef = serde_json::from_value(json!({"id": "m", "inputs": [
            {"name": "preset", "type": "ENUM", "default": "a", "choices": ["a", "b"], "fills": fills, "shows": shows, "socket": "both"},
            {"name": "width", "type": "FLOAT", "default": 1.0, "socket": "both"},
            {"name": "spare", "type": "FLOAT", "default": 0.0, "socket": "both"},
        ]}))
        .unwrap();
        SchemaReply { nodes: vec![def], ..Default::default() }
    }

    #[test]
    fn filled_ports_follow_their_choice_until_set_away() {
        let c = preset_catalogue();
        let mut g = Graph::default();
        let id = g.add_node(c.node("m").unwrap(), [0.0, 0.0]);
        let def = c.node("m").unwrap();
        let width = def.input("width").unwrap();
        let value = |g: &Graph| ports::current_value(def, g.node(&id).unwrap(), "width", width);
        assert!(!g.node(&id).unwrap().values.contains_key("width"), "no value of its own");
        assert_eq!(value(&g), json!(1.0));
        g.set_value_checked(&c, &PortRef::new(id.clone(), "preset"), json!("b")).unwrap();
        assert_eq!(value(&g), json!(2.0));
        g.set_value_checked(&c, &PortRef::new(id.clone(), "width"), json!(7.0)).unwrap();
        assert_eq!(value(&g), json!(7.0), "an edit stands");
        g.set_value_checked(&c, &PortRef::new(id.clone(), "width"), json!(2.0)).unwrap();
        assert!(!g.node(&id).unwrap().values.contains_key("width"), "set back, it follows again");
        g.set_value_checked(&c, &PortRef::new(id.clone(), "width"), json!(7.0)).unwrap();
        g.set_value_checked(&c, &PortRef::new(id.clone(), "preset"), json!("a")).unwrap();
        assert_eq!(value(&g), json!(1.0), "choosing again replaces what the table names");
    }

    #[test]
    fn a_saved_value_that_repeats_the_choice_follows_it_again() {
        let c = preset_catalogue();
        let mut g = Graph::default();
        let id = g.add_node(c.node("m").unwrap(), [0.0, 0.0]);
        g.nodes.get_mut(&id).unwrap().values.insert("width".into(), json!(1));
        assert!(g.drop_filled_values(&c));
        assert!(!g.drop_filled_values(&c), "a tidy graph is left alone");
    }

    #[test]
    fn fields_a_choice_doesnt_use_are_tucked_until_they_matter() {
        let c = preset_catalogue();
        let mut g = Graph::default();
        let id = g.add_node(c.node("m").unwrap(), [0.0, 0.0]);
        let def = c.node("m").unwrap();
        let tucked = |g: &Graph, wired: &[&str]| ports::tucked(def, g.node(&id).unwrap(), |p| wired.contains(&p)).into_iter().collect::<Vec<_>>();
        assert_eq!(tucked(&g, &[]), ["spare"]);
        assert!(tucked(&g, &["spare"]).is_empty(), "a wired field shows");
        g.set_value_checked(&c, &PortRef::new(id.clone(), "preset"), json!("b")).unwrap();
        assert!(tucked(&g, &[]).is_empty(), "this choice uses it");
        g.set_value_checked(&c, &PortRef::new(id.clone(), "preset"), json!("a")).unwrap();
        g.set_value_checked(&c, &PortRef::new(id.clone(), "spare"), json!(3.0)).unwrap();
        assert!(tucked(&g, &[]).is_empty(), "set away from the choice's, it shows");
    }

    #[test]
    fn all_fields_is_saved_only_when_set() {
        let c = preset_catalogue();
        let mut g = Graph::default();
        let id = g.add_node(c.node("m").unwrap(), [0.0, 0.0]);
        assert!(!g.to_json().unwrap().contains("all_fields"));
        g.set_all_fields(&id, true).unwrap();
        assert!(Graph::from_json(&g.to_json().unwrap()).unwrap().node(&id).unwrap().all_fields);
    }

    #[test]
    fn add_wire_and_inherit() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, s) = (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.slow"));
        assert_eq!((a.0.as_str(), s.0.as_str()), ("n1", "n2"));
        assert_eq!(g.node(&s).unwrap().values["seconds"], json!(1.0), "widgets start at their defaults");
        assert!(!g.node(&s).unwrap().values.contains_key("text"), "socket-only ports have no widget");
        g.connect(&c, p(&a, "text"), p(&s, "text")).unwrap();
        let w = add(&mut g, &c, "text.words");
        g.connect(&c, p(&s, "text"), p(&w, "text")).unwrap();
        // INT → FLOAT by the engine's conversions; the widget value is kept but inherited.
        g.connect(&c, p(&w, "count"), p(&s, "seconds")).unwrap_err(); // would be a cycle
        let s2 = add(&mut g, &c, "text.slow");
        g.connect(&c, p(&w, "count"), p(&s2, "seconds")).unwrap();
        assert!(g.is_inherited(&p(&s2, "seconds")));
        g.disconnect(&p(&s2, "seconds")).unwrap();
        assert!(!g.is_inherited(&p(&s2, "seconds")));
        assert_eq!(g.node(&s2).unwrap().values["seconds"], json!(1.0), "disconnecting restores the widget");
    }

    #[test]
    fn type_rules_come_from_the_catalogue() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, w, u, l) = (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.words"), add(&mut g, &c, "text.upper"), add(&mut g, &c, "text.label"));
        assert!(matches!(g.connect(&c, p(&w, "count"), p(&u, "text")), Err(GraphError::TypeMismatch { .. })), "INT into TEXT");
        g.connect(&c, p(&a, "text"), p(&l, "label")).expect("the engine says TEXT → STRING");
        let strict: SchemaReply = serde_json::from_value(json!({"v": 2, "compat": [], "nodes": serde_json::to_value(&c.nodes).unwrap()})).unwrap();
        let mut h = Graph::new();
        let (a, l) = (add(&mut h, &strict, "text.literal"), add(&mut h, &strict, "text.label"));
        assert!(matches!(h.connect(&strict, p(&a, "text"), p(&l, "label")), Err(GraphError::TypeMismatch { .. })), "no conversions, no wire");
    }

    #[test]
    fn wires_are_refused_for_good_reasons() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, u, x) = (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.upper"), add(&mut g, &c, "text.upper"));
        assert!(matches!(g.connect(&c, p(&u, "text"), p(&u, "text")), Err(GraphError::SelfLoop)));
        assert!(matches!(g.connect(&c, p(&a, "nope"), p(&u, "text")), Err(GraphError::UnknownPort { .. })));
        assert!(matches!(g.connect(&c, p(&u, "text"), p(&a, "text")), Err(GraphError::NotWireable(_))), "the literal's text is a widget only");
        g.connect(&c, p(&a, "text"), p(&u, "text")).unwrap();
        g.connect(&c, p(&u, "text"), p(&x, "text")).unwrap();
        assert!(matches!(g.connect(&c, p(&x, "text"), p(&u, "text")), Err(GraphError::Cycle { .. })));
        // An input takes one wire: a new one replaces the old.
        let b = add(&mut g, &c, "text.literal");
        let replaced = g.connect(&c, p(&b, "text"), p(&u, "text")).unwrap();
        assert_eq!(replaced.unwrap().from, p(&a, "text"));
        assert_eq!(g.wires.iter().filter(|w| w.to == p(&u, "text")).count(), 1);
    }

    #[test]
    fn lists_feed_lists_and_map_singles() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, sp, j, u) = (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.split"), add(&mut g, &c, "text.join_all"), add(&mut g, &c, "text.upper"));
        assert!(matches!(g.connect(&c, p(&a, "text"), p(&j, "parts")), Err(GraphError::ExpectsList { .. })), "a single value can't feed a list input");
        g.connect(&c, p(&a, "text"), p(&sp, "text")).unwrap();
        g.connect(&c, p(&sp, "parts"), p(&u, "text")).unwrap();
        assert!(g.output_is_list(&c, &p(&u, "text")), "upper is mapped over the parts");
        g.connect(&c, p(&u, "text"), p(&j, "parts")).expect("a mapped node's output is a list");
    }

    #[test]
    fn choices_bring_and_take_sub_ports() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, k) = (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.case"));
        assert!(matches!(g.connect(&c, p(&a, "text"), p(&k, "mode.small")), Err(GraphError::InactiveSubPort { .. })));
        let out = g.set_value_checked(&c, &p(&k, "mode"), json!("title")).unwrap();
        assert_eq!(out.added, ["mode.small"]);
        g.connect(&c, p(&a, "text"), p(&k, "mode.small")).unwrap();
        let out = g.set_value_checked(&c, &p(&k, "mode"), json!("upper")).unwrap();
        assert_eq!(out.dropped, ["mode.small"]);
        assert_eq!(out.disconnected.len(), 1);
        assert!(g.wires.is_empty());
    }

    #[test]
    fn variadic_ports_take_wires_into_slots() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, cc) = (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.concat"));
        assert!(matches!(g.connect(&c, p(&a, "text"), p(&cc, "c")), Err(GraphError::VariadicPort { .. })));
        g.connect(&c, p(&a, "text"), p(&cc, "c2")).unwrap();
        assert!(matches!(g.connect(&c, p(&a, "text"), p(&cc, "c4")), Err(GraphError::UnknownPort { .. })), "max 3 slots");
    }

    #[test]
    fn remove_duplicate_and_subgraph() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, u, w) = (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.upper"), add(&mut g, &c, "text.words"));
        g.connect(&c, p(&a, "text"), p(&u, "text")).unwrap();
        g.connect(&c, p(&u, "text"), p(&w, "text")).unwrap();
        let pairs = g.duplicate_nodes(&[u.clone(), w.clone()], [10.0, 0.0], true).unwrap();
        assert_eq!(pairs.len(), 2);
        assert_eq!(g.wires.len(), 4, "internal wire and the input from outside are copied");
        let sub = g.subgraph(&[u.clone(), w.clone()]);
        assert_eq!((sub.nodes.len(), sub.wires.len()), (2, 1));
        let before = g.revision();
        g.remove_node(&u).unwrap();
        assert!(g.revision() > before);
        assert!(!g.wires.iter().any(|x| x.from.node == u || x.to.node == u));
        assert!(matches!(g.remove_node(&u), Err(GraphError::UnknownNode(_))));
    }

    #[test]
    fn duplicates_join_their_groups_and_copies_leave_them() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, u, w) = (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.upper"), add(&mut g, &c, "text.words"));
        g.connect(&c, p(&a, "text"), p(&u, "text")).unwrap();
        g.connect(&c, p(&u, "text"), p(&w, "text")).unwrap();
        let outer = g.create_group("outer", &[a.clone(), u.clone(), w.clone()], &[]).unwrap();
        let inner = g.create_group("inner", std::slice::from_ref(&w), &[]).unwrap();
        let pairs = g.duplicate_nodes(&[u.clone(), w.clone()], [40.0, 40.0], false).unwrap();
        let (u2, w2) = (pairs[0].1.clone(), pairs[1].1.clone());
        assert_eq!(g.group_of(&u2), Some(&outer), "each copy joins its original's innermost group");
        assert_eq!(g.group_of(&w2), Some(&inner));
        assert!(g.wires.iter().any(|x| x.from == p(&u2, "text") && x.to == p(&w2, "text")));
        g.check(&c).unwrap();
        // the clipboard has no groups: a paste lands at the top level
        let sub = g.subgraph(&[u.clone(), w.clone()]);
        assert!(sub.groups.is_empty());
        assert!(!sub.to_json().unwrap().contains("groups"));
    }

    #[test]
    fn workflow_json_round_trips_and_reads_what_engines_read() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, u) = (add(&mut g, &c, "text.literal"), add(&mut g, &c, "text.upper"));
        g.move_node(&u, [120.0, 40.0]).unwrap();
        g.connect(&c, p(&a, "text"), p(&u, "text")).unwrap();
        let back = Graph::from_json(&g.to_json().unwrap()).unwrap();
        assert_eq!(back.nodes, g.nodes);
        assert_eq!(back.wires, g.wires);
        back.check(&c).unwrap();
        // An executor's workflow file (no positions, no next_id) and the research software file's extras.
        let text = r#"{"version": 1, "nodes": {"story": {"kind": "text.literal", "values": {"text": "x"}}, "n7": {"kind": "text.upper"}},
                      "wires": [{"from": {"node": "story", "port": "text"}, "to": {"node": "n7", "port": "text"}, "via": [[1, 2]], "slice": null}],
                      "groups": {}, "frames": {}}"#;
        let mut h = Graph::from_json(text).unwrap();
        h.check(&c).unwrap();
        assert_eq!(add(&mut h, &c, "text.upper").0, "n8", "fresh ids follow the file's");
        assert!(matches!(Graph::from_json(r#"{"version": 2}"#), Err(GraphError::Version(2))));
        assert!(matches!(
            Graph::from_json(r#"{"version": 1, "nodes": {}, "wires": [{"from": {"node": "a", "port": "x"}, "to": {"node": "b", "port": "y"}}]}"#),
            Err(GraphError::Dangling(..))
        ));
        let unknown = r#"{"version": 1, "nodes": {"z": {"kind": "text.teleport"}}}"#;
        assert!(matches!(Graph::from_json(unknown).unwrap().check(&c), Err(GraphError::Broken(_))));
    }

    #[test]
    fn titles_and_serials() {
        let c = catalog();
        let mut g = Graph::new();
        let (a, b) = (add(&mut g, &c, "text.upper"), add(&mut g, &c, "text.upper"));
        let display = |k: &str| k.trim_start_matches("text.").to_owned();
        g.assign_serials(&BTreeSet::new(), display);
        assert_eq!(g.node_title(&a, display).unwrap(), "upper 1");
        assert_eq!(g.node_title(&b, display).unwrap(), "upper 2");
        g.remove_node(&b).unwrap();
        assert_eq!(g.node_title(&a, display).unwrap(), "upper", "a serial shows only when another shares the name");
    }
}
