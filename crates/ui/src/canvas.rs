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
use plumcraft_graph::{Graph, GroupId, NodeId, NodeView, PortRef};
use plumcraft_proto::catalogue::{Port, SchemaReply};
use plumcraft_proto::display::Rendition;
use plumcraft_session::RunState;
use plumcraft_session::run::NodeRun;
use serde_json::{Value, json};

use crate::menu_view;
use crate::palette::{self, PaletteIndex, PaletteOutcome, PaletteState};
use crate::run_view::{self, PinKey, PinSink, Thumbs, TrackedPin};
use crate::theme::{CLEAR, FamilyTint, Text, Tokens};
use crate::value_editors;
use crate::widgets::{self, EditBuffers};
use plumcraft_editor_core::scene::{self, Face, Footer, Paint};

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
/// How far (screen points) from a port's pin the pointer still hovers or clicks it.
pub const PORT_HIT: f32 = 9.0;
/// How thick wires are drawn (graph units).
pub const WIRE_WIDTH: f32 = 2.5;
/// The width a node's header keeps for its run status ("stale 12.34 ms" fits; longer grows).
pub const STATUS_SLOT: f32 = 88.0;

// What an input row stands for, and one input row of a node: the core's (N1 slice 8).
pub use plumcraft_editor_core::rows::{InputRow, RowKind, input_rows};

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

/// The save buttons, on a result node and in the Watched panel (the core's menu model words them).
pub use plumcraft_editor_core::menu::{CONTACT_SHEET, SAVE_HELP, SAVE_RESULT, SHEET_HELP};
/// `run.save` for `node.port`, asking where first ([`crate::EditorApp`]).
fn save_action(node: &NodeId, port: &str, sheet: bool) -> Action {
    ("run.save", json!({"node": node, "port": port, "sheet": sheet, "ask": true}))
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
    /// The wire or port under the pointer this frame (no button down, no menu or palette
    /// open), and where: the host shows its value ([`crate::wire_view`]). The first port is
    /// the output whose value it is; the second the input the wire goes into, or `None` for a
    /// port hovered on its own (an output, wired or not).
    pub hover_wire: Option<(PortRef, Option<PortRef>, Pos2)>,
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
    /// The document's selection, which the canvas draws ([`crate::edit_view`]).
    pub picked: crate::edit_view::Picked,
    /// The wires the snarl was built with, as graph ports, to see what a ⌘-drag took out
    /// ([`Canvas::reconcile_wires`]).
    pub(crate) built_wires: Vec<(PortRef, PortRef)>,
    /// The group entered (only its nodes are shown), see [`crate::groups_view`]: the document's
    /// (`canvas.focus`), as this canvas shows it (`EditorApp::follow_core`).
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
    /// Where a Shift-drag on empty canvas began (graph units) and the modifiers then: a marquee
    /// sent as `ui.gesture` when let go (`crate::edit_view`).
    pub(crate) marquee_from: Option<(Pos2, egui::Modifiers)>,
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
            marquee_from: None,
        }
    }
}

/// A command for the session, from a gesture.
pub type Action = (&'static str, Value);

/// What the canvas shows of the runs.
pub struct RunView<'a> {
    pub run: &'a RunState,
    pub thumbs: &'a mut Thumbs,
    /// Reads and decodes a thumbnail (the media service, off the UI thread: a thumbnail on its
    /// way is drawn as a placeholder; nothing without an engine).
    pub source: &'a dyn plumcraft_media::Source,
    /// The apps that open document outputs ("Open in VectorCraft", `doc.open`).
    pub docs: &'a [plumcraft_session::DocApp],
    /// Apps chosen before with Open with… (`value.open`), most recent first.
    pub recent_apps: &'a [std::path::PathBuf],
    /// The latest run's replay ([`crate::run_anim`]); `None` when run colours are off.
    pub anim: Option<&'a crate::run_anim::RunAnim>,
    /// The faces as the core's scene has them, by id (N2 slice 2): their words, colours, rows,
    /// previews and footers.
    pub faces: &'a HashMap<String, Face>,
}

// (the tests' names for what the core now words: N2 slice 2)
#[cfg(test)]
use plumcraft_editor_core::widget_text::{typed_label, typed_tip};
#[cfg(test)]
use plumcraft_proto::catalogue::NodeDef;

/// A scene colour in egui's: its own, or its token's in the theme drawn (N2 slice 4: the scene
/// names a token alone).
fn paint(p: &Paint, t: &Tokens) -> egui::Color32 {
    if let Some(c) = p.color {
        let [r, g, b, a] = c;
        return egui::Color32::from_rgba_premultiplied(r, g, b, a);
    }
    match p.token.as_deref() {
        Some("pin_hidden") => t.pin_hidden,
        Some("group") => t.group.header,
        Some("group_body") => t.group.body,
        Some(name) => t.token(name).unwrap_or(egui::Color32::TRANSPARENT),
        None => egui::Color32::TRANSPARENT,
    }
}

/// The scene's structure of `graph` for the snarl ([`Canvas::sync`]): faces, wires and nets as
/// the core builds them, with the group entered; nothing of the run, the theme or the geometry
/// (the canvas draws those from the frame's faces).
pub fn structure(graph: &Graph, catalog: &SchemaReply, focus: Option<&GroupId>) -> plumcraft_editor_core::scene::Scene {
    let run = plumcraft_session::run::RunState::default();
    let texts = HashMap::new();
    plumcraft_editor_core::scene::build(&plumcraft_editor_core::scene::Inputs {
        doc: 0,
        graph,
        catalog,
        run: &run,
        selection: plumcraft_editor_core::selection::Selection::default(),
        focus,
        watched: None,
        tokens: &plumcraft_editor_core::theme::DEFAULT,
        anim: None,
        now: std::time::Instant::now(),
        app_text: &texts,
        editors: &[],
        view: plumcraft_editor_core::scene::View::default(),
        geometry: None,
    })
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
    /// Rebuild the snarl if the session moved on since the last sync, from the core's scene of
    /// `graph` ([`structure`]): which faces, in what order and view, and which wires between
    /// which pins, drawn by snarl, routed through bend points or through a net's tags (N2 slice
    /// 3). Each face's rows are still the catalogue's ([`input_rows`]): the widgets edit through
    /// the catalogue's port, which the scene doesn't carry.
    pub fn sync(&mut self, generation: u64, graph: &Graph, catalog: &SchemaReply) {
        if self.synced == Some(generation) {
            return;
        }
        self.synced = Some(generation);
        if self.focus.as_ref().is_some_and(|f| graph.group(f).is_none()) {
            self.focus = None;
        }
        let scene = structure(graph, catalog, self.focus.as_ref());
        let mut snarl = Snarl::new();
        let mut ids: HashMap<&str, SnarlId> = HashMap::new();
        for face in &scene.nodes {
            let pos = Pos2::new(face.pos[0], face.pos[1]);
            let value = match &face.proxy {
                // a collapsed group: one node standing for its members
                Some(p) => match crate::groups_view::proxy_node(graph, catalog, &GroupId::from(p.group.as_str())) {
                    Some((_, node)) => node,
                    None => continue,
                },
                None => {
                    let id = NodeId::from(face.id.as_str());
                    let Some(node) = graph.node(&id) else { continue };
                    let def = catalog.node(&node.kind);
                    let mut inputs = def.map(|d| input_rows(d, &id, graph)).unwrap_or_default();
                    let outputs = def.map_or(0, |d| d.outputs.len());
                    let tucked =
                        def.map(|d| plumcraft_graph::ports::tucked(d, node, |p| graph.wire_into(&PortRef::new(id.clone(), p)).is_some())).unwrap_or_default();
                    if !node.all_fields {
                        inputs.retain(|r| !(r.kind == RowKind::Plain && tucked.contains(&r.key)));
                    }
                    CanvasNode { id, kind: node.kind.clone(), inputs, outputs, tucked: tucked.len(), all_fields: node.all_fields, proxy: None }
                }
            };
            // collapsed nodes stay so across rebuilds (the view is saved in the graph)
            let sid = if face.view == "collapsed" { snarl.insert_node_collapsed(pos, value) } else { snarl.insert_node(pos, value) };
            ids.insert(face.id.as_str(), sid);
        }
        // the scene's wires, between faces' pins by index; a wire with bend points isn't
        // snarl's: the canvas draws it (crate::wire_route); nor is one drawn through a net's
        // tags (crate::net_view)
        let uses: HashMap<&PortRef, [i32; 2]> = scene.nets.iter().flat_map(|n| n.uses.iter().map(|u| (&u.to, u.at))).collect();
        let mut routed = Vec::new();
        let mut labelled = Vec::new();
        for w in &scene.wires {
            let (Some(&a), Some(&b)) = (ids.get(w.from_face.as_str()), ids.get(w.to_face.as_str())) else { continue };
            let pins = |sid: SnarlId| snarl.get_node(sid).map(|n| (n.outputs, n.inputs.len()));
            let (Some((outs, _)), Some((_, ins))) = (pins(a), pins(b)) else { continue };
            if w.from_pin >= outs || w.to_pin >= ins {
                continue;
            }
            let (from, to) = (OutPinId { node: a, output: w.from_pin }, InPinId { node: b, input: w.to_pin });
            let color = crate::wire_route::wire_color(graph, catalog, &w.from, self.wire_default);
            match (w.kind.as_str(), &w.net) {
                ("net", Some(net)) => labelled.push(crate::net_view::Labelled {
                    out: from,
                    inp: to,
                    from: w.from.clone(),
                    to: w.to.clone(),
                    net: net.clone(),
                    at: uses.get(&w.to).copied().unwrap_or_else(crate::net_view::default_use_at),
                    slice: graph.wire_into(&w.to).and_then(|x| x.slice.clone()),
                    color,
                }),
                ("routed", _) => {
                    routed.push(crate::wire_route::Routed { out: from, inp: to, from: w.from.clone(), to: w.to.clone(), via: w.via.clone(), color })
                }
                _ => {
                    snarl.connect(from, to);
                }
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

    /// The selected nodes, by graph id (the document's selection:
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

    /// What the canvas measured last frame, as the core keeps it (`geometry.set`, N2 slice 3):
    /// each drawn face's size (a node's, a collapsed group's by `group:<id>`) and its pins'
    /// heights from its top, in row order (the scene's).
    pub fn measured_faces(&self) -> Vec<(String, plumcraft_editor_core::geometry::FaceGeometry)> {
        let pins = self.pins.borrow();
        let positions: HashMap<SnarlId, Pos2> = self.snarl.nodes_pos_ids().map(|(sid, pos, _)| (sid, pos)).collect();
        let mut out = Vec::with_capacity(self.node_rects.len());
        for (sid, r) in &self.node_rects {
            let Some(n) = self.snarl.get_node(*sid) else { continue };
            let y = |key: PinKey| pins.get(&key).map(|p| p.y - r.min.y);
            let inputs = (0..n.inputs.len()).map(|input| y(PinKey::In(egui_snarl::InPinId { node: *sid, input }))).collect();
            let outputs = (0..n.outputs).map(|output| y(PinKey::Out(egui_snarl::OutPinId { node: *sid, output }))).collect();
            // egui-snarl draws the frame's margin around the node's position, its pins on the
            // frame's edges: the face is the frame, offset from the position (N2 slice 4)
            let offset = positions.get(sid).map_or([0.0; 2], |p| [r.min.x - p.x, r.min.y - p.y]);
            let g = plumcraft_editor_core::geometry::FaceGeometry { size: [r.width(), r.height()], inputs, outputs, offset, ..Default::default() };
            out.push((n.id.0.clone(), g));
        }
        out
    }

    /// Fit the view to `rect` (graph units) next frame: the core's fit (`canvas.fit`'s rect, N2
    /// slice 3), the margin in it already. A face not measured yet makes it the canvas's own
    /// fit, which waits for every size.
    pub fn fit_to(&mut self, rect: [f32; 4]) {
        let r = Rect::from_min_max(egui::pos2(rect[0], rect[1]), egui::pos2(rect[2], rect[3]));
        let measured = self.snarl.nodes_ids_data().all(|(sid, _)| self.node_rects.contains_key(&sid));
        match fit_transform(&[r.shrink(32.0)], self.rect).filter(|_| measured) {
            Some(t) => self.set_transform = Some(t),
            None => self.request_fit(),
        }
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

    /// `canvas.palette` opening at a screen position (its `at` in graph coordinates).
    pub(crate) fn palette_at(&self, screen: Pos2) -> Action {
        let g = self.to_global.inverse() * screen;
        ("canvas.palette", json!({"open": true, "at": [g.x, g.y]}))
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

    /// The port whose pin is under a screen position: the output that has its value (an
    /// output itself, wired or not; a wired input's source), and the input when it was one.
    /// An input with no wire has no value but its own, which its row already shows.
    pub fn port_at(&self, graph: &Graph, catalog: &SchemaReply, screen: Pos2) -> Option<(PortRef, Option<PortRef>)> {
        let pins = self.pins.borrow();
        let mut best: Option<(f32, PinKey)> = None;
        for (key, at) in pins.iter() {
            let d = (self.to_global * *at).distance(screen);
            if d <= PORT_HIT && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, *key));
            }
        }
        drop(pins);
        match best?.1 {
            PinKey::Out(o) => Some((crate::groups_view::out_port(&self.snarl, catalog, o)?, None)),
            PinKey::In(i) => {
                let to = crate::groups_view::in_port(&self.snarl, i)?;
                let from = graph.wire_into(&to)?.from.clone();
                Some((from, Some(to)))
            }
        }
    }

    /// Where a port's pin was drawn last frame, on screen (tests and the control channel hover
    /// and click ports with this). `output` picks the side.
    pub fn pin_screen(&self, catalog: &SchemaReply, port: &PortRef, output: bool) -> Option<Pos2> {
        let pins = self.pins.borrow();
        pins.iter()
            .find(|(key, _)| match key {
                PinKey::Out(o) if output => crate::groups_view::out_port(&self.snarl, catalog, *o).as_ref() == Some(port),
                PinKey::In(i) if !output => crate::groups_view::in_port(&self.snarl, *i).as_ref() == Some(port),
                _ => false,
            })
            .map(|(_, at)| self.to_global * *at)
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

    /// Draw the canvas; returns the commands its gestures asked for. The node the Watched panel
    /// shows is outlined as the scene's faces say (`runs.faces`).
    pub fn show(&mut self, ui: &mut egui::Ui, graph: &Graph, catalog: &SchemaReply, tokens: &Tokens, runs: RunView<'_>) -> Vec<Action> {
        self.rect = ui.max_rect();
        crate::icons::ensure_loaders(ui.ctx());
        self.palette_index.refresh(catalog);
        let ctx = ui.ctx().clone();
        // Keys go to the canvas unless a text field has the keyboard (egui moves focus on Tab
        // before we see it, so "nothing focused" isn't the test).
        let nothing_focused = !ctx.text_edit_focused();
        let pointer = ctx.input(|i| i.pointer.hover_pos()).filter(|p| self.rect.contains(*p));
        // Tab or Space opens the palette at the pointer, F fits: the commands they stand for,
        // run after the canvas has drawn
        let mut keyed: Vec<Action> = Vec::new();
        if self.palette.is_none() && nothing_focused {
            let (open, fit) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) || i.consume_key(egui::Modifiers::NONE, egui::Key::Space),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::F),
                )
            });
            if open {
                keyed.push(self.palette_at(pointer.unwrap_or_else(|| self.rect.center())));
            }
            if fit {
                keyed.push(("canvas.fit", json!({})));
            }
            if !keyed.is_empty() {
                ctx.request_repaint();
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
            source: runs.source,
            docs: runs.docs,
            recent_apps: runs.recent_apps,
            pins: self.pins.clone(),
            wire_clicked: None,
            port_hovered: None,
            highlight,
            selected,
            selected_groups,
            canvas_requests: Vec::new(),
            group_frames: &self.group_frames,
            routed_paths,
            name_request: None,
            anim: runs.anim,
            now: std::time::Instant::now(),
            faces: runs.faces,
        };
        SnarlWidget::new().id(egui::Id::new(CANVAS_ID)).style(snarl_style(tokens)).show(&mut self.snarl, &mut viewer, ui);
        self.to_global = viewer.to_global;
        let wire_clicked = viewer.wire_clicked.take();
        let port_label_hovered = viewer.port_hovered.take();
        let canvas_requests = std::mem::take(&mut viewer.canvas_requests);
        if let Some(edit) = viewer.name_request.take() {
            self.nets.edit = Some(edit);
        }
        let mut actions = keyed;
        actions.extend(viewer.actions);
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
        actions.extend(canvas_requests.into_iter().map(crate::groups_view::CanvasRequest::command));
        // The selection (clicks, snarl's marquee, as commands) and its outline (crate::edit_view).
        actions.extend(self.selection_gestures(&ctx));
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
        // The port or wire under the pointer: the host previews its value. A port first (its
        // pin, or an output's label), since a pin sits on its node's edge.
        let hover = ctx.input(|i| if i.pointer.any_down() { None } else { i.pointer.hover_pos() });
        let free = |p: &Pos2| ctx.layer_id_at(*p).is_none_or(|l| l.order != egui::Order::Foreground && l.order != egui::Order::Tooltip);
        let port_hover = hover.filter(|p| self.palette.is_none() && self.wire_menu.is_none() && self.rect.contains(*p) && free(p)).and_then(|p| {
            match (self.port_at(graph, catalog, p), port_label_hovered.clone()) {
                (Some((from, to)), _) => Some((from, to, p)),
                (None, Some(from)) => Some((from, None, p)),
                (None, None) => None,
            }
        });
        self.hover_wire = port_hover.or_else(|| {
            hover
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
                .and_then(|p| self.wire_at(ui.style(), catalog, p).map(|(a, b)| (a, Some(b), p)))
        });
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
        // A left click on a port's pin or a wire watches its value: since N2 slice 4 that is the
        // core's meaning of the click, sent as a `ui.gesture` tap (crate::edit_view), which
        // hit-tests the pins and wires this canvas measured.
        // Bend points on wires (crate::wire_route).
        actions.extend(self.route_overlay(&ctx));
        // Slice labels on wires, and the slice editor (crate::slice_view).
        actions.extend(self.slice_overlay(&ctx, ui.style(), graph, catalog, tokens));
        // The application's marks on wires (crate::app_hooks), kept up to date while it has any.
        if self.mark_overlay(&ctx, ui.style(), graph, catalog, tokens) {
            ctx.request_repaint_after(crate::app_hooks::REPAINT);
        }
        // Net labels' tags, their menu and the name editor (crate::net_view).
        actions.extend(self.net_overlay(&ctx, graph, catalog, tokens));
        // what Add Bend Point would make (worked out before the menu borrows the canvas)
        let bend = self.wire_menu.as_ref().and_then(|m| self.via_with_point(graph, &m.to, m.screen_pos));
        if let Some(menu) = &mut self.wire_menu {
            let mut close = false;
            let area = egui::Area::new(egui::Id::new("plumcraft-wire-menu")).order(egui::Order::Foreground).fixed_pos(menu.screen_pos).show(&ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    // the core's menu model, with where Add Bend Point would put the point
                    let parts = menu_view::Parts { graph, catalog, run: runs.run, doc_apps: runs.docs, recent_apps: runs.recent_apps };
                    let cx = menu_view::Context::Wire { to: menu.to.clone(), bend: bend.as_ref().map(|via| json!(via)) };
                    let items = plumcraft_editor_core::menu::context(&parts, &menu_view::Facts::default(), &cx);
                    let Some(item) = menu_view::draw(ui, &items, menu_view::Style { tokens, bar: false, close: false }) else { return };
                    close = true;
                    match (menu_view::action(&item), item.id.as_str()) {
                        (Some(a), _) => actions.push(a),
                        (None, "wire.slice") => {
                            let slice = graph.wire_into(&menu.to).and_then(|w| w.slice.clone());
                            self.slice_edit = Some(crate::slice_view::SliceEdit::new(menu.from.clone(), menu.to.clone(), slice.as_deref(), menu.screen_pos));
                        }
                        (None, "output.label") => {
                            let name = crate::net_view::suggested_name(graph, &menu.from);
                            self.nets.edit = Some(crate::net_view::NameEdit::new(crate::net_view::NameFor::Output(menu.from.clone()), name, menu.screen_pos));
                        }
                        // Add Bend Point where no point could be worked out: nothing to do
                        (None, _) => {}
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
            actions.push(self.palette_at(p));
            ctx.request_repaint();
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
    source: &'a dyn plumcraft_media::Source,
    docs: &'a [plumcraft_session::DocApp],
    recent_apps: &'a [std::path::PathBuf],
    pins: PinSink,
    /// A wire right-clicked this frame.
    wire_clicked: Option<(PortRef, PortRef)>,
    /// The output whose label the pointer is over this frame (its value is previewed).
    port_hovered: Option<PortRef>,
    highlight: Option<NodeId>,
    /// The selection when the frame began (node menus act on it when their node is in it).
    selected: Vec<NodeId>,
    selected_groups: Vec<GroupId>,
    /// Enter or leave a group (view changes, not edits).
    canvas_requests: Vec<crate::groups_view::CanvasRequest>,
    /// Open groups' frames to paint under the nodes.
    group_frames: &'a [crate::groups_view::GroupFrame],
    /// Wires with bend points, to paint under the nodes ([`crate::wire_route`]).
    routed_paths: Vec<(Vec<Pos2>, egui::Color32)>,
    /// A name editor an input's menu asked for this frame (name or rename a named port).
    name_request: Option<crate::net_view::NameEdit>,
    /// The latest run's replay, and the time it is shown at.
    anim: Option<&'a crate::run_anim::RunAnim>,
    now: std::time::Instant,
    /// What each face shows (the core's scene).
    faces: &'a HashMap<String, Face>,
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

/// Is a node's thumbnail drawn faded: stale, failed, or from an earlier run than the one now
/// computing the node (B22): the core's scene decides (N2 slice 2).
pub use plumcraft_editor_core::scene::thumb_faded;

pub use plumcraft_editor_core::run_text::{OLDER_THUMB, stale_tooltip};

/// A widget row's caption: a fixed-width column, so widgets line up and never overlap it.
fn caption(ui: &mut egui::Ui, rich: egui::RichText, text: &str, tooltip: Option<&str>) -> egui::Response {
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(Vec2::new(CAPTION_WIDTH, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_width(CAPTION_WIDTH);
        rich_label(ui, rich, text, tooltip, CAPTION_WIDTH)
    })
    .inner
}

impl<'a> NodeViewer<'a> {
    /// The node's face as the core's scene has it (N2 slice 2): what the face shows.
    fn face(&self, node: &CanvasNode) -> Option<&'a Face> {
        self.faces.get(&node.id.0)
    }

    /// A pin as the scene has it: round or square, in its wire type's colour; hidden when the
    /// port can't be wired.
    fn pin_of(&self, p: &scene::Pin) -> PinInfo {
        if !p.wireable {
            return PinInfo::circle().with_fill(self.tokens.pin_hidden).with_stroke(egui::Stroke::NONE);
        }
        let pin = if p.shape == "square" { PinInfo::square() } else { PinInfo::circle() };
        pin.with_fill(paint(&p.fill, self.tokens)).with_wire_color(paint(&p.wire, self.tokens))
    }

    /// A named input's name, drawn as a net label with no wire: the name before the caption, in
    /// the wire type's colour (docs/research/23-named-ports.md 3).
    fn named_tag(ui: &mut egui::Ui, named: Option<&scene::Named>, tokens: &Tokens) {
        let Some(n) = named else { return };
        ui.label(egui::RichText::new(format!("{} →", n.name)).monospace().small().color(paint(&n.paint, tokens))).on_hover_text(&n.hover);
    }

    /// An input's right-click menu: name it, or rename or unname its name (the core's menu model).
    fn input_menu(&mut self, r: &egui::Response, at: &PortRef, _port: &Port) {
        let pos = r.rect.left_bottom();
        let mut request = None;
        // a caption only senses hover; its menu needs the secondary click
        let r = r.interact(egui::Sense::click());
        r.context_menu(|ui| {
            let parts = menu_view::Parts { graph: self.graph, catalog: self.catalog, run: self.run, doc_apps: self.docs, recent_apps: self.recent_apps };
            let items = plumcraft_editor_core::menu::context(&parts, &menu_view::Facts::default(), &menu_view::Context::Input { port: at.clone() });
            let Some(item) = menu_view::draw(ui, &items, menu_view::Style { tokens: self.tokens, bar: false, close: true }) else { return };
            let name = item.params.as_ref().and_then(|p| p.get("name")).and_then(serde_json::Value::as_str).map(str::to_owned);
            match (menu_view::action(&item), item.id.as_str(), name) {
                (Some(a), ..) => self.actions.push(a),
                (None, "rename.port", Some(name)) => {
                    request = Some(crate::net_view::NameEdit::new(crate::net_view::NameFor::RenamePort(name.clone()), name, pos))
                }
                (None, "name.port", _) => request = Some(crate::net_view::NameEdit::new(crate::net_view::NameFor::Port(at.clone()), at.port.clone(), pos)),
                _ => {}
            }
        });
        if let Some(edit) = request {
            self.name_request = Some(edit);
        }
    }

    fn track(&self, info: PinInfo, key: PinKey) -> TrackedPin {
        TrackedPin { info, key, sink: self.pins.clone() }
    }

    /// The node as the run animation shows it now; a collapsed group shows its members'
    /// busiest state (running, else waiting, else failed, skipped, done, cached).
    fn shown(&self, node: &CanvasNode) -> Option<crate::run_anim::Shown> {
        use crate::run_anim::Shown;
        let anim = self.anim?;
        let Some(p) = &node.proxy else { return anim.shown(&node.id.0, self.now) };
        let states: Vec<Shown> = self.graph.group_members(&p.group).iter().filter_map(|m| anim.shown(&m.0, self.now)).collect();
        [Shown::Running, Shown::Waiting, Shown::Failed, Shown::Skipped, Shown::Done, Shown::Cached].into_iter().find(|s| states.contains(s))
    }

    /// The node's body while a run is shown: its family colour washed with its state's
    /// ([`crate::run_anim`]); `None` at rest.
    fn run_fill(&self, node: &CanvasNode) -> Option<egui::Color32> {
        use crate::run_anim::Shown;
        let strength = self.anim?.strength(self.now);
        if strength <= 0.0 {
            return None;
        }
        let t = &self.tokens;
        let (color, amount) = match self.shown(node)? {
            // paler than the body: not lit yet
            Shown::Waiting => (t.field, 0.55),
            Shown::Running => {
                // a gentle pulse, one beat every 1.2 s
                let phase = self.anim.map_or(0.0, |a| a.phase(self.now));
                (t.state_running, 0.42 + 0.14 * (phase * std::f32::consts::TAU).sin())
            }
            Shown::Done => (t.state_done, 0.40),
            Shown::Cached => (t.state_cached, 0.24),
            Shown::Failed => (t.state_error, 0.45),
            Shown::Skipped => (t.state_skipped, 0.60),
        };
        Some(crate::theme::mix(self.tint(node).body, color, amount * strength))
    }

    fn node_run(&self, node: &CanvasNode) -> Option<&NodeRun> {
        self.run.node(&node.id.0)
    }

    /// An input row as the scene words it ([`scene::InRow`]): its source decides what is drawn.
    fn input_pin(&mut self, pin: &InPin, ui: &mut egui::Ui, snarl: &mut Snarl<CanvasNode>) -> PinInfo {
        let Some(node) = snarl.get_node(pin.id.node) else { return PinInfo::circle() };
        let Some(row) = node.inputs.get(pin.id.input).cloned() else { return PinInfo::circle() };
        let Some(face) = self.face(node).and_then(|f| f.inputs.get(pin.id.input)) else { return PinInfo::circle() };
        // a proxy's row stands for a member's port
        let node_id = crate::groups_view::row_node(node, pin.id.input);
        let port = &row.port;
        let at = PortRef::new(node_id.clone(), row.key.clone());
        let label = face.label.as_str();
        let text = face.text.as_deref().unwrap_or_default();
        let hover = face.hover.as_deref();
        match face.source.as_str() {
            "wired" => match &face.badge {
                Some(badge) => {
                    ui.horizontal(|ui| {
                        pin_label(ui, text, hover, PIN_LABEL_WIDTH + CAPTION_WIDTH);
                        ui.label(egui::RichText::new(&badge.text).small().color(paint(&badge.paint, self.tokens))).on_hover_text(&badge.hover);
                        let r = ui.small_button("Reset").on_hover_text("Reset to inherited: remove the wire, so the bundle's value applies");
                        if r.clicked() {
                            self.actions.push(("bundle.reset", json!({"node": at.node, "port": at.port})));
                        }
                    });
                }
                None => {
                    pin_label(ui, text, hover, PIN_LABEL_WIDTH + CAPTION_WIDTH);
                }
            },
            "socket" => {
                pin_label(ui, text, hover, SOCKET_LABEL_WIDTH);
            }
            "inherited" => {
                // The bundle's value goes over the widget's, so the widget gives way to it.
                let hover = hover.unwrap_or_default();
                ui.horizontal(|ui| {
                    caption(ui, egui::RichText::new(label).italics(), label, Some(hover));
                    ui.label(egui::RichText::new(text).italics()).on_hover_text(hover);
                    ui.label(egui::RichText::new(face.from.as_deref().unwrap_or_default()).small().color(self.tokens.text_dim)).on_hover_text(hover);
                });
            }
            "own" => {
                let blank = self.catalog.text(plumcraft_proto::catalogue::texts::BLANK_VALUE);
                // A value nobody changed stays quiet, and one somebody did stands out: a label's
                // own colour is already the dim one.
                let rich = if face.dim { egui::RichText::new(label) } else { egui::RichText::new(label).color(self.tokens.text) };
                let current = face.widget.as_ref().map_or(Value::Null, |w| w.value.clone());
                ui.horizontal(|ui| {
                    Self::named_tag(ui, face.named.as_ref(), self.tokens);
                    let r = caption(ui, rich, label, hover);
                    self.input_menu(&r, &at, port);
                    let key = (node_id.clone(), row.key.clone());
                    if let Some(value) = widgets::port_widget(ui, port, &key, &current, self.edits, blank) {
                        self.actions.push(("node.set", json!({"node": node_id, "port": row.key, "value": value})));
                    }
                });
            }
            _ => {
                // A widget the editor doesn't know: its value read-only, or what the
                // application's editor for it says of it.
                ui.horizontal(|ui| {
                    Self::named_tag(ui, face.named.as_ref(), self.tokens);
                    let r = pin_label(ui, text, hover, PIN_LABEL_WIDTH + CAPTION_WIDTH);
                    self.input_menu(&r, &at, port);
                });
            }
        }
        self.pin_of(&face.pin)
    }

    /// An output row as the scene words it ([`scene::OutRow`]).
    fn output_pin(&mut self, pin: &OutPin, ui: &mut egui::Ui, snarl: &mut Snarl<CanvasNode>) -> PinInfo {
        let Some(node) = snarl.get_node(pin.id.node) else { return PinInfo::circle() };
        let Some(out) = self.face(node).and_then(|f| f.outputs.get(pin.id.output)) else { return PinInfo::circle() };
        let r = pin_label(ui, &out.label, Some(&out.hover), PIN_LABEL_WIDTH).interact(egui::Sense::click());
        // Hovering the label previews the output's value, wired or not; a click watches it.
        let at = PortRef::new(out.node.as_str(), out.key.clone());
        if r.hovered() {
            self.port_hovered = Some(at.clone());
        }
        if r.clicked() {
            self.actions.push(("run.watch", json!({"node": at.node, "port": at.port})));
        }
        r.context_menu(|ui| {
            let parts = menu_view::Parts { graph: self.graph, catalog: self.catalog, run: self.run, doc_apps: self.docs, recent_apps: self.recent_apps };
            let cx = menu_view::Context::Output { port: at.clone() };
            let items = plumcraft_editor_core::menu::context(&parts, &menu_view::Facts::default(), &cx);
            if let Some(a) = menu_view::draw(ui, &items, menu_view::Style { tokens: self.tokens, bar: false, close: true }).as_ref().and_then(menu_view::action)
            {
                self.actions.push(a);
            }
        });
        self.pin_of(&out.pin)
    }

    /// The node's footer as the scene lists it ([`scene::Footer`]): more fields, a Convert's
    /// path, the application's editor, the run's progress, error, save buttons, excerpt and
    /// text, and the application's line.
    fn footer(&mut self, node: &CanvasNode, ui: &mut egui::Ui) {
        let Some(face) = self.face(node) else { return };
        let dim = self.tokens.text_dim;
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            let mut items = face.footer.iter().peekable();
            while let Some(item) = items.next_if(|i| matches!(i, Footer::Fields { .. } | Footer::Convert { .. } | Footer::Editor { .. })) {
                match item {
                    Footer::Fields { text, hover, all } => {
                        let clicked = ui.small_button(egui::RichText::new(text).color(dim)).on_hover_text(hover).clicked();
                        if clicked {
                            self.actions.push(("node.fields", json!({"node": node.id, "all": all})));
                        }
                    }
                    Footer::Convert { lines } => {
                        for l in lines {
                            let r = ui.label(egui::RichText::new(&l.text).small().color(paint(&l.paint, self.tokens)));
                            if let Some(h) = &l.hover {
                                r.on_hover_text(h);
                            }
                        }
                    }
                    Footer::Editor { label, hover } => {
                        let clicked = ui.small_button(label).on_hover_text(hover).clicked();
                        if clicked {
                            self.actions.push(("node.editor", json!({"node": node.id})));
                        }
                    }
                    _ => {}
                }
            }
            if self.run.node(&node.id.0).is_some() {
                // Stay inside the node: egui-snarl lays the footer out at the node's width, and
                // wider content spills over its left edge.
                let width = ui.available_width().max(48.0);
                ui.set_max_width(width);
                while let Some(item) = items.next_if(|i| !matches!(i, Footer::AppText { .. })) {
                    self.run_footer_item(node, item, width, ui);
                }
            }
            // what the application says on the node (crate::app_hooks)
            for item in items {
                if let Footer::AppText { text, full } = item {
                    ui.set_max_width(ui.available_width().max(48.0));
                    ui.add(egui::Label::new(egui::RichText::new(text).small().color(dim)).wrap()).on_hover_text(full);
                }
            }
        });
    }

    /// One of the run's footer items.
    fn run_footer_item(&mut self, node: &CanvasNode, item: &Footer, width: f32, ui: &mut egui::Ui) {
        let dim = self.tokens.text_dim;
        match item {
            Footer::Progress { fraction, stage } => {
                ui.add(egui::ProgressBar::new(*fraction as f32).show_percentage().desired_width(width.min(200.0)).fill(self.tokens.state_running));
                if let Some(s) = stage {
                    ui.label(egui::RichText::new(s).small().color(dim));
                }
            }
            Footer::Error { text, full, paint: p } => {
                ui.add(egui::Label::new(egui::RichText::new(text).color(paint(p, self.tokens))).wrap()).on_hover_text(full);
            }
            // A result that has run: save it with its recipe; a list of them also as one
            // labelled contact sheet.
            Footer::Save { port, sheet } => {
                ui.horizontal(|ui| {
                    if ui.small_button(SAVE_RESULT).on_hover_text(SAVE_HELP).clicked() {
                        self.actions.push(save_action(&node.id, port, false));
                    }
                    if *sheet && ui.small_button(CONTACT_SHEET).on_hover_text(SHEET_HELP).clicked() {
                        self.actions.push(save_action(&node.id, port, true));
                    }
                });
            }
            Footer::Excerpt { text, faded, hover } => {
                let color = if *faded { self.tokens.text_faint } else { dim };
                let r = ui.add(egui::Label::new(egui::RichText::new(text).monospace().small().color(color)).wrap().sense(egui::Sense::click()));
                if r.on_hover_text(hover).clicked() {
                    self.actions.push(("run.watch", json!({"node": node.id})));
                }
            }
            // what the engine says on the node
            Footer::EngineText { text, full } => {
                ui.add(egui::Label::new(egui::RichText::new(text).small().color(dim)).wrap()).on_hover_text(full);
            }
            _ => {}
        }
    }

    /// A node's colours: the scene's (its family's, or a group's).
    fn tint(&self, node: &CanvasNode) -> FamilyTint {
        match self.face(node) {
            Some(f) => FamilyTint { header: paint(&f.tint.header, self.tokens), body: paint(&f.tint.body, self.tokens) },
            None => self.tokens.family_default,
        }
    }

    /// A node's border: revealed (the canvas's own highlight), else the scene's (stale, or its
    /// run state, thicker when it is the watched node).
    fn border(&self, n: &CanvasNode) -> Option<egui::Stroke> {
        if self.highlight.as_ref() == Some(&n.id) {
            return Some(egui::Stroke::new(3.0, self.tokens.highlight));
        }
        let b = self.face(n)?.border.as_ref()?;
        Some(egui::Stroke::new(b.width, paint(&b.paint, self.tokens)))
    }

    /// A node's actions: its right-click menu, and the menu in its header, as the core's menu
    /// model has them ([`plumcraft_editor_core::menu`]; a collapsed group's is all group items).
    fn node_menu_items(&mut self, n: &CanvasNode, ui: &mut egui::Ui) {
        let cx = match &n.proxy {
            Some(p) => menu_view::Context::Proxy { group: p.group.clone() },
            None => menu_view::Context::Node { node: n.id.clone() },
        };
        let parts = menu_view::Parts { graph: self.graph, catalog: self.catalog, run: self.run, doc_apps: self.docs, recent_apps: self.recent_apps };
        let facts = menu_view::Facts {
            selected: self.selected.clone(),
            selected_groups: self.selected_groups.clone(),
            editors: value_editors::facts(),
            ..menu_view::Facts::default()
        };
        let items = plumcraft_editor_core::menu::context(&parts, &facts, &cx);
        let Some(item) = menu_view::draw(ui, &items, menu_view::Style { tokens: self.tokens, bar: false, close: true }) else { return };
        match menu_view::action(&item) {
            Some(a) => self.actions.push(a),
            // a window: a text field can't live in a menu
            None => menu_view::open_window(ui.ctx(), &item),
        }
    }

    /// The preview's size when the node shows one (the scene's: it has run, and is shown in full).
    fn shown_thumb(&self, node: &CanvasNode) -> Option<Vec2> {
        let [w, h] = self.face(node)?.preview.as_ref()?.size();
        Some(Vec2::new(w, h))
    }

    /// The node's waveform thumbnail (a `peaks` rendition) in `rect`: click to watch, hover for
    /// it larger.
    fn waveform(&mut self, node: &CanvasNode, n: &NodeRun, ui: &mut egui::Ui, rect: Rect) {
        let Some(Rendition::Peaks { data, channels, .. }) = &n.thumb else { return };
        let id = ui.id().with(("plumcraft-thumb", node.id.0.as_str()));
        let r = ui.interact(rect, id, egui::Sense::click());
        // faded and the note: the scene's preview
        let (faded, note) = match self.face(node).and_then(|f| f.preview.as_ref()) {
            Some(scene::Preview::Peaks { faded, note, .. }) => (*faded, note.clone()),
            _ => (false, None),
        };
        let label = format!("Waveform of {}", node.id);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &label));
        let channels = if *channels == 1 { "mono".to_owned() } else { format!("{channels} channels") };
        let hover = match note {
            Some(note) => format!("{channels}\n{note}\nClick to watch"),
            None => format!("{channels}\nClick to watch"),
        };
        let clicked = match self.thumbs.peaks_from(data, self.source) {
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
            // on its way (the media service): the waveform's ground, nothing written on it
            Err(run_view::ThumbMiss::Pending) => {
                ui.painter().rect_filled(rect, f32::from(self.tokens.radius), self.tokens.waveform_bg);
                r.on_hover_text(&hover).clicked()
            }
            Err(miss) => {
                let text = match miss {
                    run_view::ThumbMiss::Missing | run_view::ThumbMiss::Pending => "re-run to refresh".to_owned(),
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
        let Some(Rendition::Rgba8 { data, .. }) = &n.thumb else { return };
        // what it shows of the picture, its size on hover, faded and the note: the scene's preview
        let Some(scene::Preview::Rgba8 { summary, note, faded, hover_size, .. }) = self.face(node).and_then(|f| f.preview.as_ref()) else { return };
        let ctx = ui.ctx().clone();
        let id = ui.id().with(("plumcraft-thumb", node.id.0.as_str()));
        let clicked = match self.thumbs.get_from(&ctx, data, self.source) {
            Ok(tex) => {
                let mut image = egui::Image::new((tex.id(), rect.size())).corner_radius(f32::from(self.tokens.radius));
                if *faded {
                    image = image.tint(self.tokens.faded);
                }
                let hover = match note {
                    Some(note) => format!("{summary}\n{note}\nClick to watch"),
                    None => format!("{summary}\nClick to watch"),
                };
                // painted, not laid out: the outputs sit beside it
                image.paint_at(ui, rect);
                let r = ui.interact(rect, id, egui::Sense::click());
                let label = format!("Preview of {}", node.id);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &label));
                let big = Vec2::new(hover_size[0], hover_size[1]);
                let tid = tex.id();
                r.on_hover_ui(|ui| {
                    ui.image((tid, big));
                    ui.label(&hover);
                })
                .clicked()
            }
            // on its way (the media service): the placeholder alone, labelled as the preview it
            // becomes, for the frame or two it takes
            Err(run_view::ThumbMiss::Pending) => {
                let r = ui.interact(rect, id, egui::Sense::click());
                ui.painter().rect_filled(rect, f32::from(self.tokens.radius), self.tokens.placeholder);
                let label = format!("Preview of {}", node.id);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &label));
                r.on_hover_text(format!("{summary}\nClick to watch")).clicked()
            }
            Err(miss) => {
                let text = match miss {
                    run_view::ThumbMiss::Missing | run_view::ThumbMiss::Pending => "re-run to refresh".to_owned(),
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
        self.face(node).map(|f| f.header.title.text.clone()).unwrap_or_default()
    }

    /// The node's body in its family's colour, bordered by its run state.
    fn node_frame(&mut self, default: egui::Frame, node: SnarlId, _inputs: &[InPin], _outputs: &[OutPin], snarl: &Snarl<CanvasNode>) -> egui::Frame {
        let Some(n) = snarl.get_node(node) else { return default };
        let body = self.tint(n).body;
        let framed = default.fill(self.run_fill(n).unwrap_or(body));
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
        // what the header shows: the scene's
        let Some(face) = self.face(&n) else { return };
        let head = &face.header;
        // the icon says what the node is: its doc, its family, where it sits in the palette
        let about = crate::icons::icon(ui, &head.icon, HEADER_ICON, self.tokens.header_text, "");
        match (&n.proxy, head.about.known) {
            (Some(_), _) => {
                about.on_hover_text(format!("{}\n\n{}", head.about.name, head.about.description));
            }
            (None, true) => {
                about.on_hover_ui(|ui| node_about(ui, &head.about));
            }
            (None, false) => {
                about.on_hover_text(format!("{}\n\n{}", head.about.name, head.about.description));
            }
        }
        // the state: a spinner while running, else a dot; its place kept before any run, so the
        // header is as wide either way
        match (head.state.kind.as_str(), &head.state.paint) {
            ("spinner", Some(p)) => {
                ui.add(egui::Spinner::new().size(12.0).color(paint(p, self.tokens))).on_hover_text(head.state.hover.as_deref().unwrap_or_default());
            }
            ("dot", Some(p)) => {
                let (rect, r) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.5, paint(p, self.tokens));
                r.on_hover_text(head.state.hover.as_deref().unwrap_or_default());
            }
            _ => {
                ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
            }
        }
        // the title: a click watches the node's first output (and runs)
        let rich = egui::RichText::new(&title).color(self.tokens.header_text).font(self.tokens.font(Text::Title));
        if head.title.watch {
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
        // a member's badges: its export mark, a group's output
        for b in &head.badges {
            ui.label(egui::RichText::new(&b.text).small().color(paint(&b.paint, self.tokens))).on_hover_text(&b.hover);
        }
        // With several engines: the one that runs the node (docs/research/15-multi-engine.md).
        if let Some(e) = &head.engine {
            ui.label(egui::RichText::new(&e.text).small().color(self.tokens.text_dim)).on_hover_text(&e.hover);
        }
        // The run's status (stale, cached, the time) in a slot of its own width, kept whether
        // the node has run or not: a node is as wide before a run as after, so Tidy's spacing
        // (measured from the drawn nodes) holds when a run fills the slot in.
        let status_start = ui.cursor().min.x;
        for st in &head.status {
            let mut rich = egui::RichText::new(&st.text).small().color(paint(&st.paint, self.tokens));
            if st.strong {
                rich = rich.strong();
            }
            let r = ui.label(rich);
            if let Some(h) = &st.hover {
                r.on_hover_text(h);
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
        self.face(node).is_some_and(|f| !f.footer.is_empty())
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
        node.proxy.is_none() && self.face(node).is_some_and(|f| f.header.about.known && !f.header.about.description.is_empty())
    }

    fn show_on_hover_popup(&mut self, node: SnarlId, _inputs: &[InPin], _outputs: &[OutPin], ui: &mut egui::Ui, snarl: &mut Snarl<CanvasNode>) {
        if let Some(face) = snarl.get_node(node).and_then(|n| self.face(n)) {
            node_about(ui, &face.header.about);
        }
    }
}

/// What a node type is, for its icon's tooltip and its hover popup: its name and doc, its
/// family and what that means, where it sits in the palette, and its id (the scene's).
fn node_about(ui: &mut egui::Ui, about: &scene::About) {
    ui.set_max_width(340.0);
    ui.strong(&about.name);
    if !about.description.is_empty() {
        ui.label(&about.description);
    }
    if let Some(f) = &about.family {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(f).italics());
    }
    if let Some(path) = &about.path {
        ui.weak(path);
    }
    ui.weak(&about.id);
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
