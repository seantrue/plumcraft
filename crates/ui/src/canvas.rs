//! The node canvas: an egui-snarl view of the session's graph.
//!
//! The snarl is never the model. It is rebuilt from `plumcraft-graph` whenever the session
//! changes, and every gesture (connect, disconnect, add, remove, move, edit a widget, watch)
//! becomes a session command, queued while the snarl is drawn and executed afterwards.
//!
//! **Rows.** A node's input rows come from its `NodeDef` and its current state: one row per
//! port; a choice port with `options` adds a row per sub-port of the selected choice (value key
//! `port.sub`); a variadic port has one row per connected slot plus one empty slot (names from
//! the slot template, `c1`, `c2`, …). Every row has a pin and a label beside it; scalar ports
//! show their widget, or `name = value (wired from <title>)` when a wire feeds them.
//!
//! **Groups** ([`crate::groups_view`]): a collapsed group is one proxy node whose pins stand for
//! its members' ports; an open group is a frame around its members; an entered group shows only
//! its members.
//!
//! Lifted from the research software's `crates/ui-egui/src/canvas.rs`, with groups (E3), and without the mask
//! editor, frames, net labels, wire routes or slices. A node whose catalogue entry says
//! `flags.result` offers Save Result… once it has run; `flags.bundle_port` marks a parameter
//! bundle ([`crate::bundle_view`]). A click on a node's title, its excerpt or a wire watches that value
//! ([`crate::EditorApp`] then runs).

use std::collections::{BTreeMap, HashMap};

use egui::emath::TSTransform;
use egui::{Pos2, Rect, Vec2};
use egui_snarl::ui::{NodeLayout, NodeLayoutKind, PinInfo, SnarlPin, SnarlViewer, SnarlWidget};
use egui_snarl::{InPin, InPinId, NodeId as SnarlId, OutPin, OutPinId, Snarl};
use plumcraft_graph::{Graph, GroupId, NodeCache, NodeId, NodeView, PortRef};
use plumcraft_proto::catalogue::{NodeDef, Port, SchemaReply, Socket, scalars};
use plumcraft_proto::data::DataRef;
use plumcraft_proto::display::Rendition;
use plumcraft_proto::event::NodeState;
use plumcraft_session::RunState;
use plumcraft_session::run::NodeRun;
use plumcraft_store::Blob;
use serde_json::{Value, json};

use crate::bundle_view::{self, Source};
use crate::palette::{self, PaletteIndex, PaletteOutcome, PaletteState};
use crate::run_view::{self, PinKey, PinSink, Thumbs, TrackedPin};
use crate::theme::{CLEAR, FamilyTint, Text, Tokens, parse_hex};
use crate::value_editors;
use crate::widgets::{self, EditBuffers};

/// The width of a widget row's caption; longer labels are truncated with an ellipsis.
pub const CAPTION_WIDTH: f32 = 96.0;
/// The widest a pin label (socket rows and outputs) may be.
pub const PIN_LABEL_WIDTH: f32 = 150.0;
/// The widest a socket-only input's label may be (it has no widget beside it).
pub const SOCKET_LABEL_WIDTH: f32 = 230.0;
/// The id of the snarl widget (its pan, zoom and selection live under it).
pub const CANVAS_ID: &str = "plumcraft-canvas";
/// How far (screen points) from a wire's curve a click still hits it.
pub const WIRE_HIT: f32 = 12.0;
/// How thick wires are drawn (graph units).
pub const WIRE_WIDTH: f32 = 2.5;
/// The width a node's header keeps for its run status ("stale 12.34 ms" fits; longer grows).
pub const STATUS_SLOT: f32 = 88.0;

/// What an input row stands for.
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

/// What a snarl node stands for.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasNode {
    pub id: NodeId,
    pub kind: String,
    pub inputs: Vec<InputRow>,
    pub outputs: usize,
    /// Fields its choice doesn't use ([`ports::tucked`]), and whether they are shown anyway
    /// (`Node::all_fields`): the footer's "more fields" row.
    pub tucked: usize,
    pub all_fields: bool,
    /// Set when this node is a collapsed group's proxy ([`crate::groups_view`]).
    pub proxy: Option<crate::groups_view::Proxy>,
}

fn port_label(p: &Port) -> String {
    match p.label.as_deref() {
        Some(l) if !l.is_empty() => l.to_owned(),
        _ => p.name.clone(),
    }
}

/// A socket's label with its wire type after it (`audio · WAV`), unless the label already
/// names the type (`audio` on an AUDIO port) or the type is a scalar a widget shows. Two ports
/// both called `audio` that can't be wired together then say why at a glance.
fn typed_label(label: &str, ty: &str) -> String {
    let norm = |s: &str| s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect::<String>();
    let scalar = matches!(ty, "FLOAT" | "INT" | "BOOL" | "STRING" | "ENUM" | "ANY" | "");
    if scalar || norm(label) == norm(ty) { label.to_owned() } else { format!("{label} · {ty}") }
}

/// A port's tooltip with its type on the first line.
fn typed_tip(ty: &str, tip: Option<&str>) -> String {
    match tip.filter(|t| !t.is_empty()) {
        Some(t) => format!("Type: {ty}\n{t}"),
        None => format!("Type: {ty}"),
    }
}

/// The save buttons, on a result node and in the Watched panel.
pub const SAVE_RESULT: &str = "Save Result…";
/// Characters of the engine's text shown on a node; the rest is on hover.
const NODE_TEXT_CHARS: usize = 120;
pub const SAVE_HELP: &str = "Save this result as a file with its recipe";
pub const CONTACT_SHEET: &str = "Contact Sheet…";
pub const SHEET_HELP: &str = "Save every item on one picture, each labelled with the value that varied, with the sweep's recipe";

/// `run.save` for `node.port`, asking where first ([`crate::EditorApp`]).
fn save_action(node: &NodeId, port: &str, sheet: bool) -> Action {
    ("run.save", json!({"node": node, "port": port, "sheet": sheet, "ask": true}))
}

/// How many items a node's thumbnail stands for.
fn thumb_items(r: &Rendition) -> u64 {
    match r {
        Rendition::Rgba8 { items, .. } => *items,
        _ => 0,
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

// ---------- pins ↔ graph ports: a proxy's pins name its members' ports ----------

pub use crate::groups_view::{in_port, out_port};

/// A right-click menu on a wire (egui-snarl reports the click; the menu is drawn here).
#[derive(Debug, Clone, PartialEq)]
pub struct WireMenu {
    pub screen_pos: Pos2,
    pub from: PortRef,
    pub to: PortRef,
    /// Don't close on the click that opened it.
    pub fresh: bool,
}

/// The snarl, how it maps to the graph, and the canvas's own state.
pub struct Canvas {
    pub snarl: Snarl<CanvasNode>,
    /// The colour of a wire whose type has none (the theme's `wire`), for the wires the canvas
    /// draws itself (bends, net labels).
    pub wire_default: egui::Color32,
    synced: Option<u64>,
    pub edits: EditBuffers,
    /// Node rects in graph coordinates, from the last frame.
    node_rects: HashMap<SnarlId, Rect>,
    /// The view transform (graph → screen) of the last frame.
    pub to_global: TSTransform,
    /// A transform to apply next frame.
    pub set_transform: Option<TSTransform>,
    /// Frames left to fit the view to the nodes; counted down only once every node has its
    /// measured size, so the fit uses real node rects.
    fit_frames: u8,
    fit_tries: u8,
    pub palette: Option<PaletteState>,
    pub palette_index: PaletteIndex,
    /// The screen rect of the canvas, last frame.
    pub rect: Rect,
    /// Where each pin was drawn last frame (graph coordinates), for hit testing wires.
    pub(crate) pins: PinSink,
    pub wire_menu: Option<WireMenu>,
    /// The wire under the pointer this frame (no button down, no menu or palette open), and
    /// where: the host shows its value ([`crate::wire_view`]).
    pub hover_wire: Option<(PortRef, PortRef, Pos2)>,
    /// Wires with bend points, which the canvas draws itself ([`crate::wire_route`]).
    pub(crate) routed: Vec<crate::wire_route::Routed>,
    /// A bend point being dragged.
    pub(crate) route_drag: Option<crate::wire_route::RouteDrag>,
    /// Wires drawn through net labels, which the canvas draws itself ([`crate::net_view`]).
    pub nets: crate::net_view::NetState,
    /// A wire slice being edited ([`crate::slice_view`]).
    pub slice_edit: Option<crate::slice_view::SliceEdit>,
    /// A node revealed, and until when (egui time) it is highlighted.
    pub highlight: Option<(NodeId, f64)>,
    /// A node to centre on next frame.
    reveal: Option<NodeId>,
    /// The selection, by graph id ([`crate::edit_view`]).
    pub picked: crate::edit_view::Picked,
    /// The wires the snarl was built with, as graph ports, to see what a ⌘-drag took out
    /// ([`Canvas::reconcile_wires`]).
    pub(crate) built_wires: Vec<(PortRef, PortRef)>,
    /// The group entered (only its nodes are shown), see [`crate::groups_view`].
    pub focus: Option<GroupId>,
    /// Open groups' frames (graph coordinates), from the last frame: painted under the nodes.
    pub(crate) group_frames: Vec<crate::groups_view::GroupFrame>,
    /// Frames' tabs and handles on screen, from the last frame ([`crate::frame_view`]).
    pub(crate) frame_tabs: Vec<Rect>,
    /// The frame a drag is moving, and what it held when the drag began.
    pub(crate) frame_drag: Option<(plumcraft_graph::FrameId, plumcraft_graph::Held)>,
    /// The node (or collapsed group's proxy, by [`crate::groups_view::proxy_node_id`]) under
    /// the pointer when the primary button went down: what a drag that follows moves
    /// ([`Canvas::drag_moves`]). Kept for the whole drag, since the node leaves the press point
    /// as it moves.
    pub(crate) drag_anchor: Option<NodeId>,
}

impl Default for Canvas {
    fn default() -> Self {
        Canvas {
            snarl: Snarl::new(),
            wire_default: crate::theme::DEFAULT.wire,
            synced: None,
            edits: EditBuffers::default(),
            node_rects: HashMap::new(),
            to_global: TSTransform::IDENTITY,
            set_transform: None,
            fit_frames: 0,
            fit_tries: 0,
            palette: None,
            palette_index: PaletteIndex::default(),
            rect: Rect::NOTHING,
            pins: PinSink::default(),
            wire_menu: None,
            hover_wire: None,
            routed: Vec::new(),
            route_drag: None,
            nets: crate::net_view::NetState::default(),
            slice_edit: None,
            highlight: None,
            reveal: None,
            picked: crate::edit_view::Picked::default(),
            built_wires: Vec::new(),
            focus: None,
            group_frames: Vec::new(),
            frame_tabs: Vec::new(),
            frame_drag: None,
            drag_anchor: None,
        }
    }
}

/// A command for the session, from a gesture.
pub type Action = (&'static str, Value);

/// What the canvas shows of the runs.
pub struct RunView<'a> {
    pub run: &'a RunState,
    pub thumbs: &'a mut Thumbs,
    /// Reads a thumbnail's bytes (the connection's `resolve`; nothing without an engine).
    pub resolve: &'a dyn Fn(&DataRef) -> Option<Blob>,
    /// The apps that open document outputs ("Open in VectorCraft", `doc.open`).
    pub docs: &'a [plumcraft_session::DocApp],
    /// Apps chosen before with Open with… (`value.open`), most recent first.
    pub recent_apps: &'a [std::path::PathBuf],
}

/// The transform that shows every rect in `nodes` inside `view`.
pub fn fit_transform(nodes: &[Rect], view: Rect) -> Option<TSTransform> {
    let bb = nodes.iter().fold(Rect::NOTHING, |bb, r| bb.union(*r));
    if !bb.is_finite() || !view.is_finite() || view.width() <= 0.0 || view.height() <= 0.0 {
        return None;
    }
    let bb = bb.expand(32.0);
    let scale = (view.width() / bb.width()).min(view.height() / bb.height()).clamp(0.2, 1.0);
    Some(TSTransform { scaling: scale, translation: view.center().to_vec2() - bb.center().to_vec2() * scale })
}

impl Canvas {
    /// Rebuild the snarl from `graph` if the session moved on since the last sync.
    pub fn sync(&mut self, generation: u64, graph: &Graph, catalog: &SchemaReply) {
        if self.synced == Some(generation) {
            return;
        }
        self.synced = Some(generation);
        if self.focus.as_ref().is_some_and(|f| graph.group(f).is_none()) {
            self.focus = None;
        }
        // Collapsed groups show as one proxy node; an entered group shows only its members.
        let vis = crate::groups_view::Visibility::new(graph, self.focus.as_ref());
        let mut snarl = Snarl::new();
        let mut ids: BTreeMap<&NodeId, SnarlId> = BTreeMap::new();
        for (id, node) in &graph.nodes {
            if !vis.shows(id) {
                continue;
            }
            let def = catalog.node(&node.kind);
            let mut inputs = def.map(|d| input_rows(d, id, graph)).unwrap_or_default();
            let outputs = def.map_or(0, |d| d.outputs.len());
            let tucked = def.map(|d| plumcraft_graph::ports::tucked(d, node, |p| graph.wire_into(&PortRef::new(id.clone(), p)).is_some())).unwrap_or_default();
            if !node.all_fields {
                inputs.retain(|r| !(r.kind == RowKind::Plain && tucked.contains(&r.key)));
            }
            let value = CanvasNode { id: id.clone(), kind: node.kind.clone(), inputs, outputs, tucked: tucked.len(), all_fields: node.all_fields, proxy: None };
            let pos = Pos2::new(node.pos[0], node.pos[1]);
            // collapsed nodes stay so across rebuilds (the view is saved in the graph)
            let sid = if node.view == NodeView::Collapsed { snarl.insert_node_collapsed(pos, value) } else { snarl.insert_node(pos, value) };
            ids.insert(id, sid);
        }
        let mut proxies = BTreeMap::new();
        for gid in &vis.proxies {
            if let Some((pos, node)) = crate::groups_view::proxy_node(graph, catalog, gid) {
                proxies.insert(gid.clone(), snarl.insert_node(pos, node));
            }
        }
        // A wire with bend points isn't snarl's: the canvas draws it (crate::wire_route); nor is
        // one drawn through a net label (crate::net_view).
        let mut routed = Vec::new();
        let mut labelled = Vec::new();
        for w in &graph.wires {
            let Some((from, to)) = crate::groups_view::wire_ends(&snarl, catalog, graph, &vis, &ids, &proxies, w) else { continue };
            let wire_default = self.wire_default;
            let color = || crate::wire_route::wire_color(graph, catalog, &w.from, wire_default);
            // a label means nothing outside its group: an end in a collapsed group is a wire
            let shown = vis.proxy_of(&w.from.node).is_none() && vis.proxy_of(&w.to.node).is_none();
            if let Some(net) = graph.net_of_wire(w).filter(|_| shown) {
                let at = w.label_at.unwrap_or_else(crate::net_view::default_use_at);
                labelled.push(crate::net_view::Labelled {
                    out: from,
                    inp: to,
                    from: w.from.clone(),
                    to: w.to.clone(),
                    net,
                    at,
                    slice: w.slice.clone(),
                    color: color(),
                });
            } else if w.via.is_empty() {
                snarl.connect(from, to);
            } else {
                routed.push(crate::wire_route::Routed { out: from, inp: to, from: w.from.clone(), to: w.to.clone(), via: w.via.clone(), color: color() });
            }
        }
        self.nets.rebuild(graph, labelled);
        // (a bend point being dragged stays so until the drag ends, whatever rebuilds the snarl)
        self.routed = routed;
        self.built_wires = crate::edit_view::snarl_wires(&snarl, catalog);
        self.snarl = snarl;
        self.pins.borrow_mut().clear();
    }

    /// Rebuild the snarl on the next frame (the catalogue changed).
    pub fn invalidate(&mut self) {
        self.synced = None;
    }

    /// Fit the view to the nodes on the next frames.
    pub fn request_fit(&mut self) {
        self.fit_frames = 3;
        self.fit_tries = 0;
    }

    /// Is a fit still to come?
    pub fn fit_pending(&self) -> bool {
        self.fit_frames > 0
    }

    /// A node's rect (graph coordinates) last frame, by snarl id.
    pub(crate) fn node_rect(&self, sid: SnarlId) -> Option<&Rect> {
        self.node_rects.get(&sid)
    }

    /// Centre the view on a node and highlight it for a moment.
    pub fn reveal(&mut self, node: &NodeId) {
        self.reveal = Some(node.clone());
    }

    /// The selected nodes, by graph id (PlumCraft keeps the selection itself:
    /// [`crate::edit_view::Picked`]).
    pub fn selection(&self) -> Vec<NodeId> {
        self.picked.nodes().to_vec()
    }

    /// The selected collapsed groups.
    pub fn selected_groups(&self) -> Vec<GroupId> {
        self.picked.groups().to_vec()
    }

    /// Last frame's node rects (graph coordinates) by graph id; proxies go by
    /// [`crate::groups_view::proxy_node_id`].
    pub fn node_rects_by_id(&self) -> Vec<(NodeId, Rect)> {
        self.node_rects.iter().filter_map(|(sid, r)| Some((self.snarl.get_node(*sid)?.id.clone(), *r))).collect()
    }

    /// What Tidy needs measured (`graph.tidy`'s `sizes` and `pins`): each drawn node's size, and
    /// its pins' heights from its top, by port. Proxies are left out (Tidy lays out nodes).
    pub fn measured_for_tidy(&self, catalog: &SchemaReply) -> (serde_json::Map<String, Value>, serde_json::Map<String, Value>) {
        let mut sizes = serde_json::Map::new();
        type Sides = (serde_json::Map<String, Value>, serde_json::Map<String, Value>);
        let mut pins: BTreeMap<String, Sides> = BTreeMap::new();
        for (sid, r) in &self.node_rects {
            if let Some(n) = self.snarl.get_node(*sid).filter(|n| n.proxy.is_none()) {
                sizes.insert(n.id.0.clone(), json!([r.width(), r.height()]));
            }
        }
        for (key, at) in self.pins.borrow().iter() {
            let (sid, port, input) = match key {
                PinKey::In(p) => (p.node, self.snarl.get_node(p.node).and_then(|n| n.inputs.get(p.input)).map(|row| row.key.clone()), true),
                PinKey::Out(p) => {
                    (p.node, self.snarl.get_node(p.node).and_then(|n| catalog.node(&n.kind)?.outputs.get(p.output)).map(|o| o.name.clone()), false)
                }
            };
            let (Some(node), Some(rect), Some(port)) = (self.snarl.get_node(sid).filter(|n| n.proxy.is_none()), self.node_rects.get(&sid), port) else {
                continue;
            };
            let side = pins.entry(node.id.0.clone()).or_default();
            let side = if input { &mut side.0 } else { &mut side.1 };
            side.insert(port, json!(at.y - rect.min.y));
        }
        let pins = pins.into_iter().map(|(id, (i, o))| (id, json!({"inputs": i, "outputs": o}))).collect();
        (sizes, pins)
    }

    /// Open the palette at a screen position.
    pub fn open_palette(&mut self, screen: Pos2) {
        let graph_pos = self.to_global.inverse() * screen;
        self.palette = Some(PaletteState::new(screen, graph_pos));
    }

    /// Is a screen position over a node (last frame)?
    pub fn over_node(&self, screen: Pos2) -> bool {
        let graph = self.to_global.inverse() * screen;
        self.node_rects.values().any(|r| r.contains(graph))
    }

    /// The graph ports of a snarl wire.
    fn wire_ports(&self, catalog: &SchemaReply, out: OutPinId, inp: InPinId) -> Option<(PortRef, PortRef)> {
        Some((out_port(&self.snarl, catalog, out)?, in_port(&self.snarl, inp)?))
    }

    /// The wire under a screen position, if any (within a few screen pixels of its curve).
    pub fn wire_at(&self, style: &egui::Style, catalog: &SchemaReply, screen: Pos2) -> Option<(PortRef, PortRef)> {
        if let Some(hit) = self.routed_at(screen) {
            return Some(hit);
        }
        let scale = self.to_global.scaling.max(0.05);
        let p = self.to_global.inverse() * screen;
        let pins = self.pins.borrow();
        let mut best: Option<(f32, OutPinId, InPinId)> = None;
        for (out, inp) in self.snarl.wires() {
            let (Some(a), Some(b)) = (pins.get(&PinKey::Out(out)), pins.get(&PinKey::In(inp))) else { continue };
            let d = run_view::wire_distance(run_view::wire_frame(style, *a, *b), *a, *b, p);
            if d * scale <= WIRE_HIT && best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, out, inp));
            }
        }
        drop(pins);
        best.and_then(|(_, out, inp)| self.wire_ports(catalog, out, inp))
    }

    /// Each drawn wire's midpoint on screen, with its graph ports (tests and the control
    /// channel click wires with these).
    pub fn wire_midpoints(&self, style: &egui::Style, catalog: &SchemaReply) -> Vec<(Pos2, PortRef, PortRef)> {
        let pins = self.pins.borrow();
        let mut out = Vec::new();
        for (o, i) in self.snarl.wires() {
            let (Some(a), Some(b)) = (pins.get(&PinKey::Out(o)), pins.get(&PinKey::In(i))) else { continue };
            if let Some((from, to)) = self.wire_ports(catalog, o, i) {
                let mid = run_view::wire_midpoint(run_view::wire_frame(style, *a, *b), *a, *b);
                out.push((self.to_global * mid, from, to));
            }
        }
        drop(pins);
        out.extend(self.routed_midpoints());
        out
    }

    /// Draw the canvas; returns the commands its gestures asked for. `watched` is the node the
    /// Watched panel shows (outlined).
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        graph: &Graph,
        catalog: &SchemaReply,
        tokens: &Tokens,
        runs: RunView<'_>,
        watched: Option<&NodeId>,
    ) -> Vec<Action> {
        self.rect = ui.max_rect();
        crate::icons::ensure_loaders(ui.ctx());
        self.palette_index.refresh(catalog);
        let ctx = ui.ctx().clone();
        // Keys go to the canvas unless a text field has the keyboard (egui moves focus on Tab
        // before we see it, so "nothing focused" isn't the test).
        let nothing_focused = !ctx.text_edit_focused();
        let pointer = ctx.input(|i| i.pointer.hover_pos()).filter(|p| self.rect.contains(*p));
        if self.palette.is_none() && nothing_focused {
            let (open, fit) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) || i.consume_key(egui::Modifiers::NONE, egui::Key::Space),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::F),
                )
            });
            if open {
                self.open_palette(pointer.unwrap_or_else(|| self.rect.center()));
            }
            if fit {
                self.request_fit();
            }
        }

        let fit_now = self.fit_frames > 0;
        let mut rects: Vec<Rect> = self
            .snarl
            .nodes_pos_ids()
            .map(|(sid, pos, _)| {
                self.node_rects.get(&sid).map_or_else(|| Rect::from_min_size(pos, Vec2::new(220.0, 160.0)), |r| Rect::from_min_size(pos, r.size()))
            })
            .collect();
        let node_count = rects.len();
        // Fit takes in the frames too (outside an entered group).
        if self.focus.is_none() {
            rects.extend(crate::frame_view::frame_rects(graph));
        }
        let mut forced = self.set_transform.take().or_else(|| if fit_now { fit_transform(&rects, self.rect) } else { None });
        if fit_now {
            self.fit_tries = self.fit_tries.saturating_add(1);
            if self.node_rects.len() >= node_count || self.fit_tries > 10 {
                self.fit_frames = self.fit_frames.saturating_sub(1);
            }
        }
        if let Some(target) = self.reveal.take() {
            let now = ctx.input(|i| i.time);
            self.highlight = Some((target.clone(), now + 2.0));
            let found = self.snarl.nodes_pos_ids().find(|(_, _, n)| n.id == target).map(|(sid, pos, _)| {
                self.node_rects.get(&sid).map_or_else(|| Rect::from_min_size(pos, Vec2::new(220.0, 160.0)), |r| Rect::from_min_size(pos, r.size()))
            });
            if let Some(r) = found
                && self.rect.is_finite()
            {
                let scale = self.to_global.scaling;
                forced = Some(TSTransform { scaling: scale, translation: self.rect.center().to_vec2() - r.center().to_vec2() * scale });
            }
            ctx.request_repaint();
        }
        let now = ctx.input(|i| i.time);
        let highlight = match &self.highlight {
            Some((id, until)) if *until > now => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                Some(id.clone())
            }
            _ => None,
        };

        let selected = self.selection();
        let selected_groups = self.selected_groups();
        let mut routed_paths: Vec<(Vec<Pos2>, egui::Color32)> = self.routed_paths().into_iter().map(|(_, path, color)| (path, color)).collect();
        routed_paths.extend(self.net_paths());
        let mut viewer = NodeViewer {
            graph,
            catalog,
            tokens,
            actions: Vec::new(),
            edits: &mut self.edits,
            node_rects: &mut self.node_rects,
            forced,
            to_global: self.to_global,
            run: runs.run,
            thumbs: runs.thumbs,
            resolve: runs.resolve,
            docs: runs.docs,
            recent_apps: runs.recent_apps,
            pins: self.pins.clone(),
            wire_clicked: None,
            highlight,
            watched: watched.cloned(),
            selected,
            selected_groups,
            canvas_requests: Vec::new(),
            group_frames: &self.group_frames,
            routed_paths,
        };
        SnarlWidget::new().id(egui::Id::new(CANVAS_ID)).style(snarl_style(tokens)).show(&mut self.snarl, &mut viewer, ui);
        self.to_global = viewer.to_global;
        let wire_clicked = viewer.wire_clicked.take();
        let canvas_requests = std::mem::take(&mut viewer.canvas_requests);
        let mut actions = viewer.actions;
        // A double-click on a node (its frame or header, not a field or button in it, which take
        // the click themselves) opens its parameters window. egui reports a click within 0.6 s of
        // the one before last as a triple click and no double, so that counts too.
        for (sid, n) in self.snarl.nodes_ids_data() {
            let frame = egui::Id::new(CANVAS_ID).with(("snarl-node", sid)).with("frame");
            if n.value.proxy.is_none() && ctx.read_response(frame).is_some_and(|r| r.double_clicked() || r.triple_clicked()) {
                actions.push(("node.parameters", json!({"node": n.value.id})));
            }
        }
        // A ⌘-drag moved or dropped wires (crate::edit_view).
        self.reconcile_wires(&ctx, catalog, &mut actions);
        let live: std::collections::HashSet<SnarlId> = self.snarl.nodes_ids_data().map(|(id, _)| id).collect();
        self.node_rects.retain(|id, _| live.contains(id));
        // Dragging a selected node (or collapsed group) moves the whole selection; a collapsed
        // group's node dragged moves the group (crate::edit_view).
        actions.extend(self.drag_moves(&ctx, graph));
        // The collapse chevron: keep the graph's view in step (expanding shows it in full).
        for info in self.snarl.nodes_info() {
            let n = &info.value;
            if n.proxy.is_some() {
                continue;
            }
            let Some(view) = graph.node(&n.id).map(|g| g.view) else { continue };
            let collapsed = view == NodeView::Collapsed;
            if info.open == collapsed {
                let to = if info.open { "full" } else { "collapsed" };
                actions.push(("node.view", json!({"id": n.id, "view": to})));
            }
        }
        // Group frames and the entered group's breadcrumb (crate::groups_view).
        actions.extend(self.group_overlay(&ctx, graph, tokens));
        // Frames' tabs and resize handles (crate::frame_view).
        actions.extend(self.frame_overlay(&ctx, graph, tokens));
        self.apply_requests(canvas_requests);
        // The selection (clicks, snarl's marquee) and its outline (crate::edit_view).
        self.update_selection(&ctx, graph);
        self.paint_selection(&ctx, tokens);

        // A right-click on a wire: a menu with Watch and Disconnect. egui-snarl reports one
        // within its own (thin) hit area; the same wider test as a left click catches the rest.
        let right = ctx.input(|i| if i.pointer.secondary_clicked() { i.pointer.interact_pos() } else { None });
        let wire_clicked = wire_clicked.or_else(|| {
            let p = right?;
            if self.palette.is_none()
                && self.wire_menu.is_none()
                && self.rect.contains(p)
                && !self.over_node(p)
                && !self.over_net_tag(p)
                && !self.over_frame_tab(p)
            {
                self.wire_at(ui.style(), catalog, p)
            } else {
                None
            }
        });
        if let Some((from, to)) = wire_clicked
            && let Some(p) = ctx.input(|i| i.pointer.interact_pos())
        {
            self.wire_menu = Some(WireMenu { screen_pos: p, from, to, fresh: true });
        }
        // The wire under the pointer: the host previews its value.
        let hover = ctx.input(|i| if i.pointer.any_down() { None } else { i.pointer.hover_pos() });
        self.hover_wire = hover
            .filter(|p| {
                self.palette.is_none()
                    && self.wire_menu.is_none()
                    && self.rect.contains(*p)
                    && !self.over_node(*p)
                    && !self.on_bend_handle(*p)
                    && !self.over_net_tag(*p)
                    && !self.over_frame_tab(*p)
            })
            .filter(|p| ctx.layer_id_at(*p).is_none_or(|l| l.order == egui::Order::Background || l.order == egui::Order::Middle))
            .and_then(|p| self.wire_at(ui.style(), catalog, p).map(|(a, b)| (a, b, p)));
        // A double click on a wire pops its value out.
        // (a click within 0.6 s of the one before last is egui's triple click, not a double)
        let double = ctx.input(|i| {
            let b = egui::PointerButton::Primary;
            if i.pointer.button_double_clicked(b) || i.pointer.button_triple_clicked(b) { i.pointer.interact_pos() } else { None }
        });
        if let Some(p) = double
            && self.wire_menu.is_none()
            && self.rect.contains(p)
            && !self.over_node(p)
            && !self.on_bend_handle(p)
            && !self.over_net_tag(p)
            && !self.over_frame_tab(p)
            && let Some((from, _)) = self.wire_at(ui.style(), catalog, p)
        {
            actions.push(("view.popout", json!({"node": from.node, "port": from.port})));
        }
        // A left click on a wire watches its value.
        let clicked = ctx.input(|i| if i.pointer.primary_clicked() { i.pointer.interact_pos() } else { None });
        if self.palette.is_none()
            && self.wire_menu.is_none()
            && let Some(p) = clicked
            && self.rect.contains(p)
            && !self.over_node(p)
            && !self.on_bend_handle(p)
            && !self.over_frame_tab(p)
            && ctx.layer_id_at(p).is_none_or(|l| l.order == egui::Order::Background || l.order == egui::Order::Middle)
            && let Some((from, _)) = self.wire_at(ui.style(), catalog, p)
        {
            actions.push(("run.watch", json!({"node": from.node, "port": from.port})));
        }
        // Bend points on wires (crate::wire_route).
        actions.extend(self.route_overlay(&ctx));
        // Slice labels on wires, and the slice editor (crate::slice_view).
        actions.extend(self.slice_overlay(&ctx, ui.style(), graph, catalog, tokens));
        // Net labels' tags, their menu and the name editor (crate::net_view).
        actions.extend(self.net_overlay(&ctx, graph, catalog, tokens));
        // what Add Bend Point would make (worked out before the menu borrows the canvas)
        let bend = self.wire_menu.as_ref().and_then(|m| self.via_with_point(graph, &m.to, m.screen_pos));
        if let Some(menu) = &mut self.wire_menu {
            let mut close = false;
            let area = egui::Area::new(egui::Id::new("plumcraft-wire-menu")).order(egui::Order::Foreground).fixed_pos(menu.screen_pos).show(&ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.weak(format!("{}.{} -> {}.{}", menu.from.node, menu.from.port, menu.to.node, menu.to.port));
                    if ui.button("Watch").on_hover_text("Show this wire's value in the Watched panel").clicked() {
                        actions.push(("run.watch", json!({"node": menu.from.node, "port": menu.from.port})));
                        close = true;
                    }
                    if ui.button("Pop Out").on_hover_text("Show this wire's value in a window of its own, updated by every run").clicked() {
                        actions.push(("view.popout", json!({"node": menu.from.node, "port": menu.from.port})));
                        close = true;
                    }
                    let slice = graph.wire_into(&menu.to).and_then(|w| w.slice.clone());
                    let label = if slice.is_some() { "Edit Slice…" } else { "Slice…" };
                    if ui.button(label).on_hover_text("Take part of the list this wire carries: start:stop:step (1:-1:2, :-1) or an index (-1)").clicked() {
                        self.slice_edit = Some(crate::slice_view::SliceEdit::new(menu.from.clone(), menu.to.clone(), slice.as_deref(), menu.screen_pos));
                        close = true;
                    }
                    if slice.is_some() && ui.button("Clear Slice").clicked() {
                        actions.push(("wire.slice", json!({"to": menu.to, "slice": null})));
                        close = true;
                    }
                    if ui.button("Add Bend Point").on_hover_text("Bend the wire here: drag the point to shape it").clicked() {
                        if let Some(via) = &bend {
                            actions.push(("wire.route", json!({"to": menu.to, "via": via})));
                        }
                        close = true;
                    }
                    if graph.wire_into(&menu.to).is_some_and(|w| !w.via.is_empty()) && ui.button("Straighten").on_hover_text("Remove the bend points").clicked()
                    {
                        actions.push(("wire.route", json!({"to": menu.to, "via": []})));
                        close = true;
                    }
                    if ui.button("Draw as Label…").on_hover_text("Draw this wire as a net label at each end, as a schematic does").clicked() {
                        let name = crate::net_view::suggested_name(graph, &menu.from);
                        self.nets.edit = Some(crate::net_view::NameEdit::new(crate::net_view::NameFor::Wire(menu.to.clone()), name, menu.screen_pos));
                        close = true;
                    }
                    if ui.button("Label All Uses…").on_hover_text("Draw every wire from this output as a net label").clicked() {
                        let name = crate::net_view::suggested_name(graph, &menu.from);
                        self.nets.edit = Some(crate::net_view::NameEdit::new(crate::net_view::NameFor::Output(menu.from.clone()), name, menu.screen_pos));
                        close = true;
                    }
                    if ui.button("Disconnect").clicked() {
                        actions.push(("wire.disconnect", json!({"to": menu.to})));
                        close = true;
                    }
                });
            });
            let outside = ctx.input(|i| i.pointer.any_click()) && !area.response.hovered() && !menu.fresh;
            let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            menu.fresh = false;
            if close || outside || escape {
                self.wire_menu = None;
            }
        }

        // Right-click on empty canvas opens the palette (on a node, snarl's node menu).
        if self.palette.is_none()
            && self.wire_menu.is_none()
            && let Some(p) = right
            && self.rect.contains(p)
            && !self.over_node(p)
            && ctx.layer_id_at(p).is_none_or(|l| l.order == egui::Order::Background)
        {
            self.open_palette(p);
        }
        if let Some(state) = &mut self.palette {
            match palette::show(&ctx, state, &self.palette_index, catalog) {
                PaletteOutcome::Open => {}
                PaletteOutcome::Closed => self.palette = None,
                PaletteOutcome::Add(kind) => {
                    actions.push(("node.add", json!({"kind": kind, "pos": [state.graph_pos.x, state.graph_pos.y]})));
                    self.palette = None;
                }
            }
        }
        actions
    }
}

/// The canvas's snarl style. Pins sit on the node's edge: with pins inside the frame, egui-snarl
/// 0.12 shifts each output row right by the pin's width (its spacing leaks into the column's
/// cursor), which clipped the labels of every output after the first.
///
/// Nodes collapse from the chevron in their header, not snarl's triangle.
///
/// snarl's own selection outline is hidden: PlumCraft keeps the selection itself and draws it
/// ([`crate::edit_view`]); snarl's may name nodes by stale indices.
///
/// The canvas and its grid are the theme's (`canvas`, `grid`), not egui's field colour.
pub fn snarl_style(tokens: &Tokens) -> egui_snarl::ui::SnarlStyle {
    let hidden = egui_snarl::ui::SelectionStyle { margin: egui::Margin::ZERO, rounding: egui::CornerRadius::ZERO, fill: CLEAR, stroke: egui::Stroke::NONE };
    egui_snarl::ui::SnarlStyle {
        pin_placement: Some(egui_snarl::ui::PinPlacement::Edge),
        select_style: Some(hidden),
        // the header draws its own collapse chevron (NodeViewer::show_header)
        collapsible: Some(false),
        header_drag_space: Some(Vec2::new(2.0, 0.0)),
        // thicker than snarl's default: easier to see, and snarl's own hover (wire ends,
        // its right-click) is as wide as the wire
        wire_width: Some(WIRE_WIDTH),
        bg_frame: Some(egui::Frame::new().fill(tokens.canvas)),
        bg_pattern_stroke: Some(egui::Stroke::new(1.0, tokens.grid)),
        ..egui_snarl::ui::SnarlStyle::new()
    }
}

/// The snarl viewer: draws nodes from the graph and the catalogue, and queues commands.
struct NodeViewer<'a> {
    graph: &'a Graph,
    catalog: &'a SchemaReply,
    tokens: &'a Tokens,
    actions: Vec<Action>,
    edits: &'a mut EditBuffers,
    node_rects: &'a mut HashMap<SnarlId, Rect>,
    forced: Option<TSTransform>,
    to_global: TSTransform,
    run: &'a RunState,
    thumbs: &'a mut Thumbs,
    resolve: &'a dyn Fn(&DataRef) -> Option<Blob>,
    docs: &'a [plumcraft_session::DocApp],
    recent_apps: &'a [std::path::PathBuf],
    pins: PinSink,
    /// A wire right-clicked this frame.
    wire_clicked: Option<(PortRef, PortRef)>,
    highlight: Option<NodeId>,
    /// The node the Watched panel shows.
    watched: Option<NodeId>,
    /// The selection when the frame began (node menus act on it when their node is in it).
    selected: Vec<NodeId>,
    selected_groups: Vec<GroupId>,
    /// Enter or leave a group (view changes, not edits).
    canvas_requests: Vec<crate::groups_view::CanvasRequest>,
    /// Open groups' frames to paint under the nodes.
    group_frames: &'a [crate::groups_view::GroupFrame],
    /// Wires with bend points, to paint under the nodes ([`crate::wire_route`]).
    routed_paths: Vec<(Vec<Pos2>, egui::Color32)>,
}

/// Space between a node's thumbnail and the outputs beside it, and what follows below.
const THUMB_GAP: f32 = 6.0;
/// The size of a node header's icons.
const HEADER_ICON: f32 = 14.0;

/// A label that never grows past `width`: truncated with an ellipsis, the full text (and the
/// port's tooltip) on hover.
fn pin_label(ui: &mut egui::Ui, text: &str, tooltip: Option<&str>, width: f32) -> egui::Response {
    rich_label(ui, egui::RichText::new(text), text, tooltip, width)
}

/// [`pin_label`] with styled text.
fn rich_label(ui: &mut egui::Ui, rich: egui::RichText, text: &str, tooltip: Option<&str>, width: f32) -> egui::Response {
    // Lay the text out at its own width (at most `width`), so a narrow column can't squeeze it
    // to nothing: the node grows to fit instead.
    let galley = egui::WidgetText::from(rich).into_galley(ui, Some(egui::TextWrapMode::Truncate), width, egui::TextStyle::Body);
    let r = ui.add(egui::Label::new(galley).selectable(false));
    match tooltip.filter(|t| !t.is_empty()) {
        Some(t) => r.on_hover_text(format!("{text}\n\n{t}")),
        None => r.on_hover_text(text),
    }
}

/// Why a node is stale: the edits since its last run that changed it or what feeds it.
pub fn stale_tooltip(run: &NodeRun) -> String {
    if run.stale_why.is_empty() {
        return "Stale: an edit since the last run changed this node or something upstream.".into();
    }
    let lines: Vec<String> = run.stale_why.iter().map(|w| format!("• {w}")).collect();
    format!("Stale since the last run:\n{}\nRun again to bring it up to date.", lines.join("\n"))
}

/// A widget row's caption: a fixed-width column, so widgets line up and never overlap it.
fn caption(ui: &mut egui::Ui, rich: egui::RichText, text: &str, tooltip: Option<&str>) -> egui::Response {
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(Vec2::new(CAPTION_WIDTH, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_width(CAPTION_WIDTH);
        rich_label(ui, rich, text, tooltip, CAPTION_WIDTH)
    })
    .inner
}

impl NodeViewer<'_> {
    fn def(&self, node: &CanvasNode) -> Option<&NodeDef> {
        self.catalog.node(&node.kind)
    }

    /// With several engines (a merged catalogue): the engine that runs the node, whether it is
    /// pinned, and the engines that offer its kind; as the coordinator decides (spec 8.2): the
    /// pin, else the first of the workflow's engines that offers it, else the first that does.
    fn engine_of(&self, node: &CanvasNode) -> Option<(String, bool, Vec<String>)> {
        if node.proxy.is_some() {
            return None;
        }
        let offered = self.def(node).map(|d| d.engines.clone()).filter(|e| !e.is_empty())?;
        let pin = self.graph.node(&node.id).and_then(|n| n.engine.clone()).filter(|p| offered.contains(p));
        let engine = pin.clone().or_else(|| self.graph.engines.iter().find(|e| offered.contains(e)).cloned()).or_else(|| offered.first().cloned())?;
        Some((engine, pin.is_some(), offered))
    }

    /// A pin in its wire type's colour (the engine's, else the theme's): round for scalars,
    /// square for everything else.
    fn pin(&self, ty: &str) -> PinInfo {
        let typed = self.catalog.wire_type(ty).and_then(|w| w.color.as_deref()).and_then(parse_hex);
        let pin = if scalars::is_scalar(ty) { PinInfo::circle() } else { PinInfo::square() };
        pin.with_fill(typed.unwrap_or(self.tokens.pin_default)).with_wire_color(typed.unwrap_or(self.tokens.wire))
    }

    fn track(&self, info: PinInfo, key: PinKey) -> TrackedPin {
        TrackedPin { info, key, sink: self.pins.clone() }
    }

    fn node_run(&self, node: &CanvasNode) -> Option<&NodeRun> {
        self.run.node(&node.id.0)
    }

    /// The node's view (full, without its preview, or collapsed).
    fn view_of(&self, node: &CanvasNode) -> NodeView {
        self.graph.node(&node.id).map(|g| g.view).unwrap_or_default()
    }

    fn input_pin(&mut self, pin: &InPin, ui: &mut egui::Ui, snarl: &mut Snarl<CanvasNode>) -> PinInfo {
        let Some(node) = snarl.get_node(pin.id.node) else { return PinInfo::circle() };
        let Some(row) = node.inputs.get(pin.id.input).cloned() else { return PinInfo::circle() };
        // a proxy's row stands for a member's port
        let node_id = crate::groups_view::row_node(node, pin.id.input);
        let port = &row.port;
        let at = PortRef::new(node_id.clone(), row.key.clone());
        let tip = port.tooltip.as_deref();
        let label = if port.list { format!("{} [ ]", row.label) } else { row.label.clone() };
        let wire = self.graph.wire_into(&at);
        let graph_node = self.graph.node(&node_id);
        let def = graph_node.and_then(|n| self.catalog.node(&n.kind));
        let current = graph_node.zip(def).map_or_else(|| port.default.clone(), |(n, d)| plumcraft_graph::ports::current_value(d, n, &row.key, port));
        let source = bundle_view::source(self.graph, self.catalog, &at, port, bundle_view::provenance(self.run, &node_id, &row.key));
        match (wire, port.socket_mode()) {
            (Some(w), Socket::Both | Socket::Widget) => {
                let text = format!("{label} = {} (wired from {})", widgets::value_text(&current), self.node_title(&w.from.node));
                if source == Source::Overridden {
                    let word = bundle_view::bundle_word(self.graph, self.catalog, &node_id);
                    ui.horizontal(|ui| {
                        pin_label(ui, &text, tip, PIN_LABEL_WIDTH + CAPTION_WIDTH);
                        self.actions.extend(bundle_view::override_badge(ui, self.tokens, &word, &at));
                    });
                } else {
                    pin_label(ui, &text, tip, PIN_LABEL_WIDTH + CAPTION_WIDTH);
                }
            }
            (_, Socket::Socket) => {
                let tip = typed_tip(&port.ty, tip);
                pin_label(ui, &typed_label(&label, &port.ty), Some(&tip), SOCKET_LABEL_WIDTH);
            }
            (None, _) if let Source::Inherited(value) = &source => {
                // The bundle's value goes over the widget's, so the widget gives way to it.
                let word = bundle_view::bundle_word(self.graph, self.catalog, &node_id);
                let shown = value.as_ref().map_or_else(|| format!("(from {word})"), widgets::value_text);
                let hover = format!(
                    "{label} comes from the {word} wired into this node; the widget's own value ({}) is not used. The node's menu › Detach uses its own values.",
                    widgets::value_text(&current)
                );
                ui.horizontal(|ui| {
                    caption(ui, egui::RichText::new(&label).italics(), &label, Some(&hover));
                    ui.label(egui::RichText::new(&shown).italics()).on_hover_text(&hover);
                    ui.label(egui::RichText::new(format!("from {word}")).small().color(self.tokens.text_dim)).on_hover_text(&hover);
                });
            }
            (None, _) if widgets::has_editor(port) => {
                let blank = self.catalog.text(plumcraft_proto::catalogue::texts::BLANK_VALUE);
                // A value nobody changed stays quiet (by the graph, or what the run used), and one
                // somebody did stands out: a label's own colour is already the dim one.
                let dim = current == port.default || bundle_view::provenance(self.run, &node_id, &row.key) == Some("default");
                let rich = if dim { egui::RichText::new(&label) } else { egui::RichText::new(&label).color(self.tokens.text) };
                let hover = dim.then(|| format!("{}{}(the default value)", tip.unwrap_or(""), if tip.is_some() { "\n\n" } else { "" }));
                ui.horizontal(|ui| {
                    caption(ui, rich, &label, hover.as_deref().or(tip));
                    let key = (node_id.clone(), row.key.clone());
                    if let Some(value) = widgets::port_widget(ui, port, &key, &current, self.edits, blank) {
                        self.actions.push(("node.set", json!({"node": node_id, "port": row.key, "value": value})));
                    }
                });
            }
            (None, _) => {
                // A widget the editor doesn't know: show its value read-only, or what the
                // application's editor for it says of it.
                let shown = match def.and_then(|d| value_editors::for_port(d, &row.key)) {
                    Some(editor) => (editor.summary)(&current),
                    None => widgets::value_text(&current),
                };
                pin_label(ui, &format!("{label}: {shown}"), tip, PIN_LABEL_WIDTH + CAPTION_WIDTH);
            }
        }
        if port.wireable() { self.pin(&port.ty) } else { PinInfo::circle().with_fill(self.tokens.pin_hidden).with_stroke(egui::Stroke::NONE) }
    }

    fn output_pin(&mut self, pin: &OutPin, ui: &mut egui::Ui, snarl: &mut Snarl<CanvasNode>) -> PinInfo {
        let Some(node) = snarl.get_node(pin.id.node) else { return PinInfo::circle() };
        let (node_id, port, label) = match node.proxy.as_ref().map(|p| p.outputs.get(pin.id.output)) {
            // A collapsed group's output stands for a member's.
            Some(Some(o)) => (o.port.node.clone(), o.def.clone(), o.label.clone()),
            Some(None) => return PinInfo::circle(),
            None => {
                let Some(port) = self.def(node).and_then(|d| d.outputs.get(pin.id.output)).cloned() else { return PinInfo::circle() };
                let label = port_label(&port);
                (node.id.clone(), port, label)
            }
        };
        let label = typed_label(&label, &port.ty);
        let label = if port.list { format!("[ ] {label}") } else { label };
        let tip = typed_tip(&port.ty, port.tooltip.as_deref());
        let r = pin_label(ui, &label, Some(&tip), PIN_LABEL_WIDTH).interact(egui::Sense::click());
        r.context_menu(|ui| {
            if ui.button(format!("Watch {}", port.name)).clicked() {
                self.actions.push(("run.watch", json!({"node": node_id, "port": port.name})));
                ui.close();
            }
        });
        self.pin(&port.ty)
    }

    /// The node's footer: its progress, its error and its text excerpt.
    fn footer(&mut self, node: &CanvasNode, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            if node.tucked > 0 {
                let (text, tip) = if node.all_fields {
                    ("Fewer fields".to_owned(), "Tuck away the fields this choice doesn't use")
                } else {
                    (format!("{} more field{}", node.tucked, if node.tucked == 1 { "" } else { "s" }), "Show the fields this choice doesn't use")
                };
                if ui.small_button(egui::RichText::new(text).color(self.tokens.text_dim)).on_hover_text(tip).clicked() {
                    self.actions.push(("node.fields", json!({"node": node.id, "all": !node.all_fields})));
                }
            }
            if self.graph.is_convert(&node.id) {
                self.convert_path(node, ui);
            }
            if let Some(kind) = self.own_editor(node)
                && ui.small_button(kind.label).on_hover_text(kind.help).clicked()
            {
                self.actions.push(("node.editor", json!({"node": node.id})));
            }
            if let Some(n) = self.run.node(&node.id.0).cloned() {
                self.footer_items(node, &n, ui);
            }
        });
    }

    /// A Convert node's path (docs/research/20-convert.md): its types and steps, or what it
    /// still needs.
    fn convert_path(&mut self, node: &CanvasNode, ui: &mut egui::Ui) {
        use plumcraft_proto::convert;
        let dim = self.tokens.text_dim;
        match self.graph.convert_path(self.catalog, &node.id) {
            Some(p) if p.is_empty() => {
                ui.label(egui::RichText::new("the types meet: it passes the value on").small().color(dim));
            }
            Some(p) => {
                let tip = format!("{}\nnode.convert picks again", convert::label(&p));
                ui.label(egui::RichText::new(convert::types(&p)).small().color(dim)).on_hover_text(tip);
                ui.label(egui::RichText::new(convert::label(&p)).small().color(dim));
                if !convert::lossless(&p) {
                    ui.label(egui::RichText::new("a step isn't lossless").small().color(self.tokens.warning));
                }
            }
            None => {
                ui.label(egui::RichText::new("wire both ends").small().color(dim));
            }
        }
    }

    /// The editor this node offers: the application has one for what it names, and the value
    /// isn't on a wire.
    fn own_editor(&self, node: &CanvasNode) -> Option<&'static value_editors::EditorKind> {
        if node.proxy.is_some() {
            return None;
        }
        let def = self.def(node)?;
        let kind = value_editors::offered(def)?;
        let port = def.editor.as_ref()?.port.clone();
        self.graph.wire_into(&PortRef::new(node.id.clone(), port)).is_none().then_some(kind)
    }

    fn footer_items(&mut self, node: &CanvasNode, n: &NodeRun, ui: &mut egui::Ui) {
        // Stay inside the node: egui-snarl lays the footer out at the node's width, and wider
        // content spills over its left edge.
        let width = ui.available_width().max(48.0);
        ui.set_max_width(width);
        if n.state == Some(NodeState::Progress) {
            let f = n.fraction.filter(|f| f.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0) as f32;
            ui.add(egui::ProgressBar::new(f).show_percentage().desired_width(width.min(200.0)).fill(self.tokens.state_running));
            if let Some(s) = &n.stage {
                ui.label(egui::RichText::new(s).small().color(self.tokens.text_dim));
            }
        }
        if let Some(e) = &n.error {
            // A skipped node isn't at fault: say why quietly; real errors in red.
            let (full, color) = match e.kind.as_str() {
                "skipped" => (format!("skipped: {}", e.message), self.tokens.text_dim),
                "" => (e.message.clone(), self.tokens.error),
                kind => (format!("{kind} error: {}", e.message), self.tokens.error),
            };
            let (short, _) = run_view::truncate_chars(&full, run_view::ERROR_CHARS);
            ui.add(egui::Label::new(egui::RichText::new(short).color(color)).wrap()).on_hover_text(&full);
        }
        // A result that has run: save it with its recipe; a list of them also as one labelled
        // contact sheet.
        if let Some(port) = self.savable_output(node, n) {
            let items = n.thumb.as_ref().map_or(0, thumb_items);
            ui.horizontal(|ui| {
                if ui.small_button(SAVE_RESULT).on_hover_text(SAVE_HELP).clicked() {
                    self.actions.push(save_action(&node.id, &port, false));
                }
                if items > 1 && ui.small_button(CONTACT_SHEET).on_hover_text(SHEET_HELP).clicked() {
                    self.actions.push(save_action(&node.id, &port, true));
                }
            });
        }
        if let Some(Rendition::Excerpt { text, .. }) = n.thumb.as_ref().filter(|_| self.view_of(node) == NodeView::Full)
            && !text.is_empty()
        {
            let color = if n.is_stale() || n.state == Some(NodeState::Error) { self.tokens.text_faint } else { self.tokens.text_dim };
            let hover = if n.is_stale() { format!("{}\nClick to watch", stale_tooltip(n)) } else { "Click to watch".to_owned() };
            let r = ui
                .add(egui::Label::new(egui::RichText::new(run_view::excerpt_lines(text)).monospace().small().color(color)).wrap().sense(egui::Sense::click()));
            if r.on_hover_text(hover).clicked() {
                self.actions.push(("run.watch", json!({"node": node.id})));
            }
        }
        // what the engine says on the node
        if let Some(text) = n.text.as_deref().filter(|t| !t.is_empty()) {
            let (short, _) = run_view::truncate_chars(text, NODE_TEXT_CHARS);
            ui.add(egui::Label::new(egui::RichText::new(short).small().color(self.tokens.text_dim)).wrap()).on_hover_text(text);
        }
    }

    /// The output a result node's "Save Result…" saves, once a run has made it and the engine
    /// has somewhere to put it.
    fn savable_output(&self, node: &CanvasNode, run: &NodeRun) -> Option<String> {
        if self.catalog.save_formats.is_empty() || node.proxy.is_some() || !matches!(run.state, Some(NodeState::Done | NodeState::Cached)) {
            return None;
        }
        self.def(node).filter(|d| d.flags.result)?.outputs.first().map(|o| o.name.clone())
    }

    /// A node's title on the canvas.
    fn node_title(&self, id: &NodeId) -> String {
        plumcraft_session::node_title(self.graph, self.catalog, id)
    }

    /// A node's colours: its family's (the engine's `NodeDef::family`), or a group's.
    fn tint(&self, node: &CanvasNode) -> FamilyTint {
        if node.proxy.is_some() {
            return self.tokens.group;
        }
        self.tokens.family_tint(self.catalog, self.def(node).map_or("", |d| d.family.as_str()))
    }

    /// A node's header icon: the engine's `NodeDef::icon`, or a group's.
    fn icon_name<'n>(&'n self, node: &CanvasNode) -> &'n str {
        if node.proxy.is_some() {
            return crate::groups_view::GROUP_ICON;
        }
        self.def(node).map(|d| d.icon.as_str()).filter(|i| !i.is_empty()).unwrap_or(crate::icons::FALLBACK)
    }

    /// A node's border: revealed, stale, or its run state (none: the theme's), thicker when
    /// it is the watched node.
    fn border(&self, n: &CanvasNode) -> Option<egui::Stroke> {
        if self.highlight.as_ref() == Some(&n.id) {
            return Some(egui::Stroke::new(3.0, self.tokens.highlight));
        }
        if n.proxy.is_some() {
            return Some(egui::Stroke::new(2.0, self.tokens.group.header));
        }
        let watched = self.watched.as_ref() == Some(&n.id);
        let width = |base: f32| if watched { base + 1.5 } else { base };
        let run = self.node_run(n);
        if run.is_some_and(NodeRun::is_stale) {
            return Some(egui::Stroke::new(width(2.0), self.tokens.stale));
        }
        match run.and_then(|r| r.state).and_then(|s| self.tokens.state_color(s).map(|c| (s, c))) {
            Some((s, c)) => Some(egui::Stroke::new(width(if s == NodeState::Error { 2.5 } else { 2.0 }), c)),
            None if watched => Some(egui::Stroke::new(2.5, self.tokens.accent)),
            None => None,
        }
    }

    /// A node's actions: its right-click menu, and the menu in its header.
    /// Expand a command into the graph it is made of (`node.expand`), when the catalogue says
    /// how. Into a group is the usual choice, so it comes first.
    fn expand_items(&mut self, n: &CanvasNode, ui: &mut egui::Ui) {
        if !plumcraft_session::expand_cmds::has_expansion(self.catalog, &n.kind) {
            return;
        }
        let name = plumcraft_session::display_name(self.catalog, &n.kind);
        // what the engine says it unrolls into, under what the item does
        let note = plumcraft_session::expand_cmds::expansion_of(self.catalog, &n.kind).map(|e| e.note).unwrap_or_default();
        let hover = |what: &str| if note.is_empty() { what.to_owned() } else { format!("{what}\n\n{note}") };
        ui.separator();
        if ui
            .button(format!("Expand into Group {name}"))
            .on_hover_text(hover("Replace this node with the nodes it is made of, grouped, with its settings promoted"))
            .clicked()
        {
            self.actions.push(("node.expand", json!({"node": n.id, "group": name})));
            ui.close();
        }
        if ui.button("Expand without a Group").on_hover_text(hover("Replace this node with the nodes it is made of")).clicked() {
            self.actions.push(("node.expand", json!({"node": n.id})));
            ui.close();
        }
    }

    fn node_menu_items(&mut self, n: &CanvasNode, ui: &mut egui::Ui) {
        if n.proxy.is_none() && ui.button("Edit Parameters…").on_hover_text("Every input of this node in a window of its own").clicked() {
            self.actions.push(("node.parameters", json!({"node": n.id})));
            ui.close();
        }
        let cx = crate::groups_view::MenuCtx { graph: self.graph, catalog: self.catalog, selected: &self.selected, selected_groups: &self.selected_groups };
        if n.proxy.is_some() {
            crate::groups_view::node_menu(ui, &cx, n, &mut self.actions);
            return;
        }
        if let Some(kind) = self.own_editor(n)
            && ui.button(kind.label).on_hover_text(kind.help).clicked()
        {
            self.actions.push(("node.editor", json!({"node": n.id})));
            ui.close();
        }
        ui.weak(format!("{} ({})", n.id, n.kind));
        let current = self.graph.node(&n.id).and_then(|g| g.title.clone()).unwrap_or_default();
        let shown = self.node_title(&n.id);
        // (a window: a text field can't live in a menu, crate::rename_view)
        if ui.button("Rename…").on_hover_text(format!("Title this node (now \"{shown}\")")).clicked() {
            let target = crate::rename_view::RenameTarget::Node(n.id.clone());
            crate::rename_view::request(ui.ctx(), target, &current, &plumcraft_session::display_name(self.catalog, &n.kind));
            ui.close();
        }
        // How the node shows: for the selection when the node is in it.
        let view = self.view_of(n);
        let targets: Vec<NodeId> = if self.selected.contains(&n.id) { self.selected.clone() } else { vec![n.id.clone()] };
        ui.horizontal(|ui| {
            ui.label("View");
            for (v, name, key, tip) in [
                (NodeView::Full, "Full", "full", "Pins, widgets and the preview"),
                (NodeView::Compact, "No preview", "compact", "Pins and widgets, without the preview"),
                (NodeView::Collapsed, "Collapsed", "collapsed", "The title only"),
            ] {
                if ui.selectable_label(view == v, name).on_hover_text(tip).clicked() {
                    self.actions.push(("node.view", json!({"nodes": targets, "view": key})));
                    ui.close();
                }
            }
        });
        // Whether the engine keeps the result across restarts (session node.cache, spec 14.5).
        let cache = self.graph.node(&n.id).map(|g| g.cache).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("Keep result");
            for (c, name, key, tip) in [
                (NodeCache::Auto, "When slow", "auto", "Kept in the store across restarts when the node took a second or more"),
                (NodeCache::Always, "Always", "always", "Always kept in the store across restarts"),
                (NodeCache::Never, "Never", "never", "Computed again after a restart"),
            ] {
                if ui.selectable_label(cache == c, name).on_hover_text(tip).clicked() {
                    self.actions.push(("node.cache", json!({"nodes": targets, "cache": key})));
                    ui.close();
                }
            }
        });
        // With several engines offering the kind: pin the nodes to one (session node.engine).
        if let Some((engine, pinned, offered)) = self.engine_of(n).filter(|(_, _, o)| o.len() > 1) {
            ui.horizontal(|ui| {
                ui.label("Run on");
                for e in &offered {
                    let on = &engine == e && pinned;
                    if ui.selectable_label(on, e).on_hover_text(format!("node.engine {{engine: {e:?}}}")).clicked() {
                        self.actions.push(("node.engine", json!({"nodes": targets, "engine": e})));
                        ui.close();
                    }
                }
                if ui.selectable_label(!pinned, "Any").on_hover_text("Unpinned: the first of the workflow's engines that offers it").clicked() {
                    self.actions.push(("node.engine", json!({"nodes": targets, "engine": null})));
                    ui.close();
                }
            });
        }
        ui.separator();
        let outputs: Vec<(String, String)> = self.def(n).map(|d| d.outputs.iter().map(|o| (o.name.clone(), o.ty.clone())).collect()).unwrap_or_default();
        for (port, _) in &outputs {
            if ui.button(format!("Watch {port}")).on_hover_text("Show this output in the Watched panel, and run").clicked() {
                self.actions.push(("run.watch", json!({"node": n.id, "port": port})));
                ui.close();
            }
        }
        if let Some(run) = self.node_run(n).cloned()
            && let Some(port) = self.savable_output(n, &run)
        {
            if ui.button(SAVE_RESULT).on_hover_text(SAVE_HELP).clicked() {
                self.actions.push(save_action(&n.id, &port, false));
                ui.close();
            }
            if run.thumb.as_ref().map_or(0, thumb_items) > 1 && ui.button(CONTACT_SHEET).on_hover_text(SHEET_HELP).clicked() {
                self.actions.push(save_action(&n.id, &port, true));
                ui.close();
            }
        }
        if let Some(a) = bundle_view::detach_button(ui, self.graph, self.catalog, &n.id) {
            self.actions.push(a);
            ui.close();
        }
        self.expand_items(n, ui);
        // A document output opens in its craft's app (session doc.open: a verified copy).
        for (port, ty) in &outputs {
            let Some(app) = self.docs.iter().find(|a| &a.wire == ty) else { continue };
            let r = ui.add_enabled(app.program.is_ok(), egui::Button::new(format!("Open {port} in {}", app.label)));
            let r = match &app.program {
                Err(e) => r.on_disabled_hover_text(format!("{} isn't installed: {e}", app.label)),
                Ok(_) => r.on_hover_text(format!("Open a copy of this document in {} (runs first when it has no value yet)", app.label)),
            };
            if r.clicked() {
                self.actions.push(("doc.open", json!({"node": n.id, "port": port})));
                ui.close();
            }
        }
        // An output the Watched panel can't show (a PDF, a video, a mesh) opens in another app
        // (session value.open: a verified copy).
        for (port, ty) in &outputs {
            let media = self.catalog.wire_type(ty).map(|w| w.media).unwrap_or_default();
            if plumcraft_session::open_cmds::has_viewer(media) || self.docs.iter().any(|a| &a.wire == ty) {
                continue;
            }
            let target = json!({"node": n.id, "port": port});
            if ui.button(format!("Open {port}")).on_hover_text("Open a copy in the default app for its type (runs first when it has no value yet)").clicked() {
                self.actions.push(("value.open", target.clone()));
                ui.close();
            }
            ui.menu_button(format!("Open {port} with"), |ui| {
                for app in self.recent_apps {
                    if ui.button(plumcraft_session::open_cmds::app_name(app)).on_hover_text(app.display().to_string()).clicked() {
                        let mut p = target.clone();
                        p["app"] = json!(app);
                        self.actions.push(("value.open", p));
                        ui.close();
                    }
                }
                if !self.recent_apps.is_empty() {
                    ui.separator();
                }
                if ui.button("Choose an App…").clicked() {
                    let mut p = target.clone();
                    p["choose"] = json!(true);
                    self.actions.push(("value.open", p));
                    ui.close();
                }
            });
        }
        if ui.button("Duplicate").clicked() {
            self.actions.push(("node.duplicate", json!({"nodes": targets})));
            ui.close();
        }
        if ui.button("Remove").clicked() {
            self.actions.push(("node.remove", json!({"nodes": [n.id]})));
            ui.close();
        }
        // Group the selection, promote an input, expose an output (crate::groups_view).
        let cx = crate::groups_view::MenuCtx { graph: self.graph, catalog: self.catalog, selected: &self.selected, selected_groups: &self.selected_groups };
        crate::groups_view::node_menu(ui, &cx, n, &mut self.actions);
    }

    /// The rgba8 thumbnail's size when the node shows one (it has run, and is shown in full).
    fn shown_thumb(&self, node: &CanvasNode) -> Option<Vec2> {
        if self.view_of(node) != NodeView::Full {
            return None;
        }
        match self.node_run(node)?.thumb.as_ref()? {
            Rendition::Rgba8 { w, h, .. } => Some(run_view::thumb_size(*w as f32, *h as f32, run_view::THUMB_PX)),
            Rendition::Peaks { .. } => Some(crate::audio::THUMB_SIZE),
            _ => None,
        }
    }

    /// The node's waveform thumbnail (a `peaks` rendition) in `rect`: click to watch, hover for
    /// it larger.
    fn waveform(&mut self, node: &CanvasNode, n: &NodeRun, ui: &mut egui::Ui, rect: Rect) {
        let Some(Rendition::Peaks { data, channels, .. }) = &n.thumb else { return };
        let id = ui.id().with(("plumcraft-thumb", node.id.0.as_str()));
        let r = ui.interact(rect, id, egui::Sense::click());
        let faded = n.is_stale() || n.state == Some(NodeState::Error);
        let label = format!("Waveform of {}", node.id);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &label));
        let channels = if *channels == 1 { "mono".to_owned() } else { format!("{channels} channels") };
        let hover = if n.is_stale() { format!("{channels}\n{}\nClick to watch", stale_tooltip(n)) } else { format!("{channels}\nClick to watch") };
        let clicked = match self.thumbs.peaks(data, self.resolve) {
            Ok(peaks) => {
                crate::audio::draw_waveform(ui.painter(), rect, &peaks, self.tokens, faded);
                let tokens = self.tokens.clone();
                r.on_hover_ui(|ui| {
                    let (big, _) = ui.allocate_exact_size(crate::audio::THUMB_HOVER_SIZE, egui::Sense::hover());
                    crate::audio::draw_waveform(ui.painter(), big, &peaks, &tokens, faded);
                    ui.label(&hover);
                })
                .clicked()
            }
            Err(miss) => {
                let text = match miss {
                    run_view::ThumbMiss::Missing => "re-run to refresh".to_owned(),
                    run_view::ThumbMiss::Shape(s) => format!("waveform unreadable: {s}"),
                };
                let p = ui.painter();
                p.rect_filled(rect, f32::from(self.tokens.radius), self.tokens.placeholder);
                p.text(rect.center(), egui::Align2::CENTER_CENTER, &text, self.tokens.font(Text::Caption), self.tokens.text_dim);
                r.on_hover_text(text).clicked()
            }
        };
        if clicked {
            self.actions.push(("run.watch", json!({"node": node.id})));
        }
    }

    /// The node's rgba8 thumbnail in `rect`: click to watch, hover for it larger.
    fn thumbnail(&mut self, node: &CanvasNode, n: &NodeRun, ui: &mut egui::Ui, rect: Rect) {
        if matches!(n.thumb, Some(Rendition::Peaks { .. })) {
            return self.waveform(node, n, ui, rect);
        }
        let Some(Rendition::Rgba8 { data, w, h, summary, caption, .. }) = &n.thumb else { return };
        let summary = if summary.is_empty() { caption.clone() } else { summary.clone() };
        let ctx = ui.ctx().clone();
        let id = ui.id().with(("plumcraft-thumb", node.id.0.as_str()));
        let clicked = match self.thumbs.get(&ctx, data, self.resolve) {
            Ok(tex) => {
                let mut image = egui::Image::new((tex.id(), rect.size())).corner_radius(f32::from(self.tokens.radius));
                if n.is_stale() || n.state == Some(NodeState::Error) {
                    image = image.tint(self.tokens.faded);
                }
                let hover = if n.is_stale() { format!("{summary}\n{}\nClick to watch", stale_tooltip(n)) } else { format!("{summary}\nClick to watch") };
                // painted, not laid out: the outputs sit beside it
                image.paint_at(ui, rect);
                let r = ui.interact(rect, id, egui::Sense::click());
                let label = format!("Preview of {}", node.id);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &label));
                let big = run_view::thumb_size(*w as f32, *h as f32, run_view::THUMB_HOVER_PX);
                let tid = tex.id();
                r.on_hover_ui(|ui| {
                    ui.image((tid, big));
                    ui.label(&hover);
                })
                .clicked()
            }
            Err(miss) => {
                let text = match miss {
                    run_view::ThumbMiss::Missing => "re-run to refresh".to_owned(),
                    run_view::ThumbMiss::Shape(s) => format!("preview unreadable: {s}"),
                };
                let r = ui.interact(rect, id, egui::Sense::click());
                let p = ui.painter();
                p.rect_filled(rect, f32::from(self.tokens.radius), self.tokens.placeholder);
                p.text(rect.center(), egui::Align2::CENTER_CENTER, &text, self.tokens.font(Text::Caption), self.tokens.text_dim);
                let label = format!("Preview of {} ({text})", node.id);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &label));
                r.on_hover_text(format!("{summary}\nThe preview is gone (reaped); the next run publishes it again.")).clicked()
            }
        };
        if clicked {
            self.actions.push(("run.watch", json!({"node": node.id})));
        }
    }
}

impl SnarlViewer<CanvasNode> for NodeViewer<'_> {
    fn title(&mut self, node: &CanvasNode) -> String {
        if let Some(p) = &node.proxy {
            return format!("Group: {}", p.name);
        }
        self.node_title(&node.id)
    }

    /// The node's body in its family's colour, bordered by its run state.
    fn node_frame(&mut self, default: egui::Frame, node: SnarlId, _inputs: &[InPin], _outputs: &[OutPin], snarl: &Snarl<CanvasNode>) -> egui::Frame {
        let Some(n) = snarl.get_node(node) else { return default };
        let framed = default.fill(self.tint(n).body);
        match self.border(n) {
            Some(stroke) => framed.stroke(stroke),
            None => framed,
        }
    }

    /// The header frame is clear: [`NodeViewer::show_header`] paints the band across the node.
    fn header_frame(&mut self, default: egui::Frame, _node: SnarlId, _inputs: &[InPin], _outputs: &[OutPin], _snarl: &Snarl<CanvasNode>) -> egui::Frame {
        default.fill(CLEAR).stroke(egui::Stroke::NONE)
    }

    /// The header: a band in the node's family colour across the node, its collapse chevron,
    /// icon, run state, title (click to watch) and timing, and its action menu at the right.
    fn show_header(&mut self, node: SnarlId, _inputs: &[InPin], _outputs: &[OutPin], ui: &mut egui::Ui, snarl: &mut Snarl<CanvasNode>) {
        let Some(n) = snarl.get_node(node).cloned() else { return };
        ui.spacing_mut().button_padding = Vec2::splat(1.0);
        // (painted once the header's height is known, under its content)
        let band = ui.painter().add(egui::Shape::Noop);
        let inner = ui.max_rect();
        let title = self.title(&n);
        let run = self.node_run(&n).cloned();
        let tint = self.tint(&n);
        let open = snarl.get_node_info(node).is_none_or(|i| i.open);
        // (PlumCraft's own collapse chevron: snarl's triangle would sit under the band; a
        // collapsed group's > expands the group, as an open group's V collapses it)
        if let Some(p) = &n.proxy {
            if crate::icons::button(ui, "chevron-right", HEADER_ICON, self.tokens.header_text, "Expand group").clicked() {
                self.actions.push(("group.collapse", json!({"group": p.group, "collapsed": false})));
            }
        } else {
            let (icon, label) = if open { ("chevron-down", "Collapse node") } else { ("chevron-right", "Expand node") };
            if crate::icons::button(ui, icon, HEADER_ICON, self.tokens.header_text, label).clicked() {
                snarl.open_node(node, !open);
            }
        }
        // the icon says what the node is: its doc, its family, where it sits in the palette
        let about = crate::icons::icon(ui, self.icon_name(&n), HEADER_ICON, self.tokens.header_text, "");
        match (&n.proxy, self.def(&n)) {
            (Some(p), _) => {
                about.on_hover_text(format!("Collapsed group {}\n\nExpand it (>) to edit its nodes in place, or enter it (pencil) to see only them", p.name));
            }
            (None, Some(def)) => {
                let def = def.clone();
                about.on_hover_ui(|ui| node_about(ui, &def, self.catalog));
            }
            (None, None) => {
                about.on_hover_text(format!("{}\n\nThe engine doesn't know this node type", n.kind));
            }
        }
        // the state: a spinner while running, else a dot; its place kept before any run, so the
        // header is as wide either way
        match run.as_ref().and_then(|r| r.state) {
            Some(state) if run_view::is_running(state) => {
                ui.add(egui::Spinner::new().size(12.0).color(self.tokens.state_running)).on_hover_text("running");
            }
            Some(state) if let Some(c) = self.tokens.state_color(state) => {
                let (rect, r) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.5, c);
                r.on_hover_text(run_view::state_name(state));
            }
            _ => {
                ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
            }
        }
        // the title: a click watches the node's first output (and runs)
        let has_outputs = n.proxy.is_none() && self.def(&n).is_some_and(|d| !d.outputs.is_empty());
        let rich = egui::RichText::new(&title).color(self.tokens.header_text).font(self.tokens.font(Text::Title));
        if has_outputs {
            let r = ui
                .add(egui::Label::new(rich).selectable(false).sense(egui::Sense::click()))
                .on_hover_text("Click to watch this node's output; double-click for its parameters");
            if r.clicked() {
                self.actions.push(("run.watch", json!({"node": n.id})));
            }
            // the title takes the click from the frame, so it answers a double click itself
            if r.double_clicked() || r.triple_clicked() {
                self.actions.push(("node.parameters", json!({"node": n.id})));
            }
        } else {
            ui.add(egui::Label::new(rich).selectable(false));
        }
        crate::groups_view::header_badges(ui, self.graph, self.catalog, &n.id, self.tokens);
        // With several engines: the one that runs the node (docs/research/15-multi-engine.md).
        if let Some((engine, pinned, offered)) = self.engine_of(&n) {
            let tip = match (pinned, offered.len()) {
                (true, _) => format!("Runs on {engine} (pinned). These offer it: {}. Change it in the node's menu (Run on)", offered.join(", ")),
                (false, 1) => format!("Runs on {engine}, the only engine that offers it"),
                (false, _) => format!("Runs on {engine}, the first of {} that offers it. Pin another in the node's menu (Run on)", offered.join(", ")),
            };
            let text = if pinned { format!("{engine} •") } else { engine };
            ui.label(egui::RichText::new(text).small().color(self.tokens.text_dim)).on_hover_text(tip);
        }
        // The run's status (stale, cached, the time) in a slot of its own width, kept whether
        // the node has run or not: a node is as wide before a run as after, so Tidy's spacing
        // (measured from the drawn nodes) holds when a run fills the slot in.
        let status_start = ui.cursor().min.x;
        if let Some(run) = &run {
            if run.is_stale() {
                ui.label(egui::RichText::new("stale").small().color(self.tokens.stale)).on_hover_text(stale_tooltip(run));
            }
            match (run.state, run.ms) {
                (Some(NodeState::Cached), fetched) => {
                    let (short, hover) = run_view::cached_text(run.computed_ms, fetched);
                    ui.label(egui::RichText::new(short).small().color(self.tokens.header_text)).on_hover_text(hover);
                }
                (Some(NodeState::Done | NodeState::Error), Some(ms)) => {
                    ui.label(egui::RichText::new(run_view::format_ms(ms)).small().color(self.tokens.header_text));
                }
                (Some(s @ (NodeState::Queued | NodeState::Skipped | NodeState::Cancelled)), _) => {
                    ui.label(egui::RichText::new(run_view::state_name(s)).small().color(self.tokens.header_text));
                }
                _ => {}
            }
        }
        let used = ui.cursor().min.x - status_start;
        if used < STATUS_SLOT {
            ui.add_space(STATUS_SLOT - used);
        }
        // room for the buttons at the node's right edge: the menu, and a collapsed group's
        // enter (the pencil) before it
        let buttons = if n.proxy.is_some() { 2.0 } else { 1.0 };
        let button = HEADER_ICON + 4.0;
        let width = buttons * (button + 2.0);
        ui.add_space(width);
        let content = ui.min_rect();
        let at = Rect::from_min_size(Pos2::new((inner.max.x - width + 2.0).max(content.min.x), content.center().y - button / 2.0), Vec2::new(width, button));
        let mut right = ui.new_child(egui::UiBuilder::new().max_rect(at).layout(egui::Layout::left_to_right(egui::Align::Center)));
        right.spacing_mut().item_spacing.x = 2.0;
        if let Some(p) = &n.proxy
            && crate::icons::button(&mut right, crate::groups_view::ENTER_ICON, HEADER_ICON, self.tokens.header_text, "Enter group").clicked()
        {
            self.canvas_requests.push(crate::groups_view::CanvasRequest::Enter(p.group.clone()));
        }
        crate::icons::menu(&mut right, "ellipsis-vertical", HEADER_ICON, self.tokens.header_text, "Node actions", |ui| self.node_menu_items(&n, ui));
        // the band: the node's width (inside its border), from its top to below the header
        let frame = egui::Frame::window(ui.style());
        let m = frame.inner_margin;
        let rect = Rect::from_min_max(
            Pos2::new(inner.min.x - f32::from(m.left), inner.min.y - f32::from(m.top)),
            Pos2::new(inner.max.x.max(content.max.x) + f32::from(m.right), content.max.y + f32::from(m.bottom)),
        );
        let r = frame.corner_radius;
        let radius = egui::CornerRadius { nw: r.nw.saturating_sub(1), ne: r.ne.saturating_sub(1), sw: 0, se: 0 };
        let radius = if open { radius } else { egui::CornerRadius { sw: radius.nw, se: radius.ne, ..radius } };
        ui.painter().set(band, egui::Shape::rect_filled(rect, radius, tint.header));
    }

    fn inputs(&mut self, node: &CanvasNode) -> usize {
        node.inputs.len()
    }

    fn outputs(&mut self, node: &CanvasNode) -> usize {
        node.outputs
    }

    /// The grid, then open groups' frames, under the nodes ([`crate::groups_view`]).
    fn draw_background(
        &mut self,
        background: Option<&egui_snarl::ui::BackgroundPattern>,
        viewport: &Rect,
        snarl_style: &egui_snarl::ui::SnarlStyle,
        style: &egui::Style,
        painter: &egui::Painter,
        _snarl: &Snarl<CanvasNode>,
    ) {
        if let Some(background) = background {
            background.draw(viewport, snarl_style, style, painter);
        }
        crate::frame_view::paint_frames(painter, self.graph, self.tokens);
        crate::groups_view::paint_group_frames(painter, self.group_frames, self.tokens);
        // wires with bend points, under the nodes as snarl's are, and as wide: the painter is in
        // graph units, so the zoom scales the stroke as it does snarl's (crate::wire_route)
        let _ = style;
        let width = WIRE_WIDTH;
        for (path, color) in &self.routed_paths {
            painter.add(egui::Shape::line(path.clone(), egui::Stroke::new(width, *color)));
        }
    }

    fn current_transform(&mut self, to_global: &mut TSTransform, _snarl: &mut Snarl<CanvasNode>) {
        if let Some(t) = self.forced.take() {
            *to_global = t;
        }
        self.to_global = *to_global;
    }

    fn final_node_rect(&mut self, node: SnarlId, rect: Rect, _ui: &mut egui::Ui, _snarl: &mut Snarl<CanvasNode>) {
        self.node_rects.insert(node, rect);
    }

    /// Inputs, then the outputs below them (right-aligned), with the thumbnail at the lower
    /// left beside the outputs: narrow nodes (the research software's maintainer, 2026-10-17).
    fn node_layout(&mut self, default: NodeLayout, _node: SnarlId, _inputs: &[InPin], _outputs: &[OutPin], _snarl: &Snarl<CanvasNode>) -> NodeLayout {
        NodeLayout { kind: NodeLayoutKind::Sandwich, ..default }
    }

    fn show_input(&mut self, pin: &InPin, ui: &mut egui::Ui, snarl: &mut Snarl<CanvasNode>) -> impl SnarlPin + 'static {
        ui.style_mut().override_font_id = Some(self.tokens.font(Text::Port));
        let info = self.input_pin(pin, ui, snarl);
        self.track(info, PinKey::In(pin.id))
    }

    fn has_body(&mut self, node: &CanvasNode) -> bool {
        node.proxy.is_none() && self.shown_thumb(node).is_some()
    }

    /// The thumbnail, at the left of the outputs (right-aligned, below the body); the body is
    /// as tall as the thumbnail needs beyond the outputs' rows.
    fn show_body(&mut self, node: SnarlId, _inputs: &[InPin], _outputs: &[OutPin], ui: &mut egui::Ui, snarl: &mut Snarl<CanvasNode>) {
        let Some(n) = snarl.get_node(node).cloned() else { return };
        let Some(run) = self.node_run(&n).cloned() else { return };
        let Some(size) = self.shown_thumb(&n) else { return };
        let top = ui.cursor().top();
        let rect = Rect::from_min_size(Pos2::new(ui.min_rect().left(), top), size);
        self.thumbnail(&n, &run, ui, rect);
        let rows = n.outputs as f32;
        let outputs_h = rows * (ui.spacing().interact_size.y + ui.spacing().item_spacing.y);
        ui.allocate_space(Vec2::new(0.0, (size.y + THUMB_GAP - outputs_h).max(0.0)));
    }

    fn show_output(&mut self, pin: &OutPin, ui: &mut egui::Ui, snarl: &mut Snarl<CanvasNode>) -> impl SnarlPin + 'static {
        ui.style_mut().override_font_id = Some(self.tokens.font(Text::Port));
        let info = self.output_pin(pin, ui, snarl);
        // room on the left of each output row for the thumbnail beside them
        if let Some(size) = snarl.get_node(pin.id.node).and_then(|n| self.shown_thumb(n)) {
            ui.add_space(size.x + THUMB_GAP);
        }
        self.track(info, PinKey::Out(pin.id))
    }

    fn has_footer(&mut self, node: &CanvasNode) -> bool {
        let full = self.view_of(node) == NodeView::Full;
        node.tucked > 0
            || self.graph.is_convert(&node.id)
            || self.own_editor(node).is_some()
            || self.node_run(node).is_some_and(|n| {
                n.error.is_some()
                    || n.state == Some(NodeState::Progress)
                    || (full && matches!(&n.thumb, Some(Rendition::Excerpt { text, .. }) if !text.is_empty()))
                    || n.text.as_deref().is_some_and(|t| !t.is_empty())
                    || self.savable_output(node, n).is_some()
            })
    }

    fn show_footer(&mut self, node: SnarlId, _inputs: &[InPin], _outputs: &[OutPin], ui: &mut egui::Ui, snarl: &mut Snarl<CanvasNode>) {
        if let Some(n) = snarl.get_node(node).cloned() {
            self.footer(&n, ui);
        }
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        let from = out_port(snarl, self.catalog, from.id);
        let to = in_port(snarl, to.id);
        if let (Some(from), Some(to)) = (from, to) {
            self.actions.push(("wire.connect", json!({"from": from, "to": to})));
        }
    }

    /// egui-snarl calls this for a right-click on a wire: open the wire menu (Watch,
    /// Disconnect) instead of disconnecting at once.
    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        let from = out_port(snarl, self.catalog, from.id);
        let to = in_port(snarl, to.id);
        if let (Some(from), Some(to)) = (from, to) {
            self.wire_clicked = Some((from, to));
        }
    }

    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<CanvasNode>) {
        if !pin.remotes.is_empty()
            && let Some(to) = in_port(snarl, pin.id)
        {
            self.actions.push(("wire.disconnect", json!({"to": to})));
        }
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<CanvasNode>) {
        for remote in &pin.remotes {
            if let Some(to) = in_port(snarl, *remote) {
                self.actions.push(("wire.disconnect", json!({"to": to})));
            }
        }
    }

    fn has_graph_menu(&mut self, _pos: Pos2, _snarl: &mut Snarl<CanvasNode>) -> bool {
        // Right-click opens the searchable palette instead (Canvas::show).
        false
    }

    fn has_node_menu(&mut self, _node: &CanvasNode) -> bool {
        true
    }

    fn show_node_menu(&mut self, node: SnarlId, _inputs: &[InPin], _outputs: &[OutPin], ui: &mut egui::Ui, snarl: &mut Snarl<CanvasNode>) {
        if let Some(n) = snarl.get_node(node).cloned() {
            self.node_menu_items(&n, ui);
        }
    }

    fn has_on_hover_popup(&mut self, node: &CanvasNode) -> bool {
        node.proxy.is_none() && self.def(node).is_some_and(|d| !d.description.is_empty())
    }

    fn show_on_hover_popup(&mut self, node: SnarlId, _inputs: &[InPin], _outputs: &[OutPin], ui: &mut egui::Ui, snarl: &mut Snarl<CanvasNode>) {
        if let Some(def) = snarl.get_node(node).and_then(|n| self.catalog.node(&n.kind)) {
            node_about(ui, def, self.catalog);
        }
    }
}

/// What a node type is, for its icon's tooltip and its hover popup: its name and doc, its
/// family and what that means, where it sits in the palette, and its id.
fn node_about(ui: &mut egui::Ui, def: &NodeDef, catalog: &SchemaReply) {
    ui.set_max_width(340.0);
    ui.strong(&def.display);
    if !def.description.is_empty() {
        ui.label(&def.description);
    }
    if let Some(f) = catalog.families.iter().find(|f| f.id == def.family) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("{}: {}", f.display, f.description)).italics());
    }
    if !def.category.is_empty() {
        ui.weak(def.category.join(" › "));
    }
    ui.weak(&def.id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_names_its_type_when_the_name_doesnt() {
        assert_eq!(typed_label("audio", "WAV"), "audio · WAV");
        assert_eq!(typed_label("audio", "AUDIO"), "audio");
        assert_eq!(typed_label("Image", "IMAGE"), "Image");
        assert_eq!(typed_label("doc", "soundcraft.session"), "doc · soundcraft.session");
        assert_eq!(typed_label("gain", "FLOAT"), "gain");
        assert_eq!(typed_tip("WAV", Some("The mix")), "Type: WAV\nThe mix");
    }

    fn catalog() -> SchemaReply {
        serde_json::from_str(
            r##"{"v": 2, "nodes": [
              {"id": "src", "outputs": [{"name": "array", "type": "ARRAY"}]},
              {"id": "stack", "inputs": [{"name": "channels", "type": "ARRAY", "socket": "socket",
                                          "variadic": {"template": "c{n}", "min": 1, "max": 4}}],
               "outputs": [{"name": "array", "type": "ARRAY"}]},
              {"id": "view", "inputs": [{"name": "mode", "type": "ENUM", "default": "auto", "choices": ["auto", "angle"],
                                         "options": {"auto": [], "angle": [{"name": "turn", "type": "FLOAT", "default": 1.0}]}}]}
            ]}"##,
        )
        .unwrap()
    }

    #[test]
    fn variadic_rows_grow_with_wires() {
        let c = catalog();
        let mut g = Graph::new();
        let s = g.add_node(c.node("stack").unwrap(), [0.0, 0.0]);
        let keys = |g: &Graph| input_rows(c.node("stack").unwrap(), &s, g).into_iter().map(|r| r.key).collect::<Vec<_>>();
        assert_eq!(keys(&g), ["c1"], "min slots, one empty");
        let a = g.add_node(c.node("src").unwrap(), [0.0, 0.0]);
        g.wires.push(plumcraft_graph::Wire::new(PortRef::new(a.clone(), "array"), PortRef::new(s.clone(), "c1")));
        assert_eq!(keys(&g), ["c1", "c2"]);
        g.wires.push(plumcraft_graph::Wire::new(PortRef::new(a.clone(), "array"), PortRef::new(s.clone(), "c4")));
        assert_eq!(keys(&g), ["c1", "c2", "c3", "c4"], "capped at max");
    }

    #[test]
    fn choice_rows_follow_the_selection() {
        let c = catalog();
        let mut g = Graph::new();
        let v = g.add_node(c.node("view").unwrap(), [0.0, 0.0]);
        let rows = |g: &Graph| input_rows(c.node("view").unwrap(), &v, g).into_iter().map(|r| r.key).collect::<Vec<_>>();
        assert_eq!(rows(&g), ["mode"]);
        g.set_value(&PortRef::new(v.clone(), "mode"), "angle".into()).unwrap();
        assert_eq!(rows(&g), ["mode", "mode.turn"]);
    }

    #[test]
    fn sync_mirrors_nodes_and_wires() {
        let catalog = plumcraft_text::engine().schema();
        let mut g = Graph::new();
        let a = g.add_node(catalog.node("text.literal").unwrap(), [10.0, 20.0]);
        let b = g.add_node(catalog.node("text.replace").unwrap(), [300.0, 20.0]);
        g.connect(&catalog, PortRef::new(a.clone(), "text"), PortRef::new(b.clone(), "text")).unwrap();
        let mut c = Canvas::default();
        c.sync(1, &g, &catalog);
        assert_eq!(c.snarl.nodes_ids_data().count(), 2);
        let wires: Vec<_> = c.snarl.wires().collect();
        assert_eq!(wires.len(), 1);
        assert_eq!((wires[0].0.output, wires[0].1.input), (0, 0));
        assert_eq!(c.built_wires, [(PortRef::new(a, "text"), PortRef::new(b, "text"))]);
        // The same generation is not rebuilt.
        c.snarl = Snarl::new();
        c.sync(1, &g, &catalog);
        assert_eq!(c.snarl.nodes_ids_data().count(), 0);
    }

    #[test]
    fn unknown_kinds_still_sync() {
        let catalog = SchemaReply::default();
        let mut g = Graph::new();
        let def = NodeDef { id: "gone".into(), ..Default::default() };
        g.add_node(&def, [0.0, 0.0]);
        let mut c = Canvas::default();
        c.sync(0, &g, &catalog);
        assert_eq!(c.snarl.nodes_ids_data().count(), 1);
    }

    #[test]
    fn fit_shows_every_node() {
        let view = Rect::from_min_size(Pos2::new(0.0, 30.0), Vec2::new(1000.0, 600.0));
        let nodes =
            [Rect::from_min_size(Pos2::new(40.0, 60.0), Vec2::new(200.0, 150.0)), Rect::from_min_size(Pos2::new(1400.0, 60.0), Vec2::new(260.0, 300.0))];
        let t = fit_transform(&nodes, view).unwrap();
        for r in nodes {
            let screen = Rect::from_min_max(t * r.min, t * r.max);
            assert!(view.contains_rect(screen), "{screen:?} in {view:?}");
        }
        assert!(t.scaling <= 1.0);
        assert!(fit_transform(&[], view).is_none());
    }
}
