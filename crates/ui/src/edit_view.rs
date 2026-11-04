//! Editing on the canvas: the selection, cut, copy and paste, select all, delete, align and
//! distribute, the Edit menu's canvas items, and Esc to cancel a run.
//!
//! **The selection is PlumCraft's own** ([`Picked`]). egui-snarl 0.12 keeps its selection in
//! private state that can't be set from outside, and by snarl node index, which the canvas
//! rebuilds on every edit (so an index can come to mean another node). So the canvas keeps the
//! selection by graph id:
//!
//! - a click on a node selects it alone, Shift+click adds it, Cmd+click removes it, and a click
//!   on empty canvas clears the selection;
//! - Shift+drag on empty canvas is snarl's marquee: the nodes it adds join the selection;
//! - Select All, Paste and Duplicate set it;
//! - dragging a selected node (or collapsed group) moves everything selected, collapsed groups
//!   with their members;
//! - the canvas draws the selection outline itself (snarl's own is hidden, see
//!   [`crate::canvas::snarl_style`]).
//!
//! **The clipboard** holds workflow JSON (`edit.copy`), so nodes paste into another workflow, or
//! another PlumCraft. Cmd+C/X/V arrive as egui's Copy/Cut/Paste events (egui-winit reads the
//! system clipboard for Paste); the Edit menu asks the window for the same events.
//!
//! Every edit is a session command, so it is undoable.
//!
//! A collapsed group's node is selected as its group ([`Picked::groups`]); Cut, Copy and
//! Delete act on its members too.
//!
//! Lifted from the research software's `crates/ui-egui/src/edit_view.rs`. Its Cut (copy, then remove) is
//! `edit.cut`, one undo step. Align, Distribute and Frame Selection take selected collapsed groups as the one
//! node they are on the canvas.

use std::collections::BTreeMap;

use egui::{Pos2, Rect};
use egui_snarl::{NodeId as SnarlId, Snarl};
use plumcraft_graph::{Graph, GroupId, NodeId, PortRef};
use plumcraft_proto::catalogue::SchemaReply;
use serde_json::{Value, json};

use crate::EditorApp;
use crate::canvas::{Action, CANVAS_ID, Canvas, CanvasNode};
use crate::theme::Tokens;

/// What the canvas has selected, by graph id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Picked {
    nodes: Vec<NodeId>,
    groups: Vec<GroupId>,
    /// snarl's own selection when last looked at, to see what its marquee added.
    raw: Vec<SnarlId>,
}

impl Picked {
    pub fn nodes(&self) -> &[NodeId] {
        &self.nodes
    }

    /// Selected collapsed groups (their proxy nodes).
    pub fn groups(&self) -> &[GroupId] {
        &self.groups
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.groups.is_empty()
    }

    /// Select exactly `nodes`.
    pub fn set(&mut self, nodes: Vec<NodeId>) {
        self.nodes = nodes;
        self.nodes.dedup();
        self.groups.clear();
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.groups.clear();
    }

    /// Select exactly collapsed group `group` (its node).
    pub fn set_group(&mut self, group: GroupId) {
        self.nodes.clear();
        self.groups = vec![group];
    }

    pub fn contains(&self, node: &CanvasNode) -> bool {
        match &node.proxy {
            Some(p) => self.groups.contains(&p.group),
            None => self.nodes.contains(&node.id),
        }
    }

    fn add(&mut self, node: &CanvasNode) {
        match &node.proxy {
            Some(p) if !self.groups.contains(&p.group) => self.groups.push(p.group.clone()),
            None if !self.nodes.contains(&node.id) => self.nodes.push(node.id.clone()),
            _ => {}
        }
    }

    fn remove(&mut self, node: &CanvasNode) {
        match &node.proxy {
            Some(p) => self.groups.retain(|g| g != &p.group),
            None => self.nodes.retain(|n| n != &node.id),
        }
    }

    /// Take what snarl's selection gained since last time (its marquee, a Shift+click). Only
    /// additions: an index snarl still holds may mean another node since the last rebuild.
    pub fn absorb(&mut self, snarl: &Snarl<CanvasNode>, raw: Vec<SnarlId>) {
        if raw == self.raw {
            return;
        }
        let added: Vec<SnarlId> = raw.iter().filter(|s| !self.raw.contains(s)).copied().collect();
        for sid in added {
            if let Some(n) = snarl.get_node(sid) {
                self.add(n);
            }
        }
        self.raw = raw;
    }

    /// Forget nodes and groups the graph no longer has.
    pub fn retain_existing(&mut self, graph: &Graph) {
        self.nodes.retain(|n| graph.nodes.contains_key(n));
        self.groups.retain(|g| graph.group(g).is_some());
    }
}

/// The snarl's wires as graph ports.
pub(crate) fn snarl_wires(snarl: &Snarl<CanvasNode>, catalog: &SchemaReply) -> Vec<(PortRef, PortRef)> {
    snarl.wires().filter_map(|(out, inp)| Some((crate::canvas::out_port(snarl, catalog, out)?, crate::canvas::in_port(snarl, inp)?))).collect()
}

/// The topmost snarl node whose rect (graph coordinates) contains `p`.
fn node_at(canvas: &Canvas, rects: &[(SnarlId, Rect)], p: Pos2) -> Option<CanvasNode> {
    rects.iter().rev().find(|(_, r)| r.contains(p)).and_then(|(sid, _)| canvas.snarl.get_node(*sid).cloned())
}

impl Canvas {
    /// Selection gestures, after snarl has drawn: take its marquee, then this frame's click.
    pub(crate) fn update_selection(&mut self, ctx: &egui::Context, graph: &Graph) {
        self.picked.retain_existing(graph);
        let raw = egui_snarl::ui::get_selected_nodes(egui::Id::new(CANVAS_ID), ctx);
        self.picked.absorb(&self.snarl, raw);
        if self.palette.is_some() || self.wire_menu.is_some() {
            return;
        }
        let Some((p, mods)) = ctx.input(|i| if i.pointer.primary_clicked() { i.pointer.interact_pos().map(|p| (p, i.modifiers)) } else { None }) else {
            return;
        };
        // Only clicks on the canvas itself, not on menus or popups over it.
        if !self.rect.contains(p) || ctx.layer_id_at(p).is_some_and(|l| l.order != egui::Order::Background && l.order != egui::Order::Middle) {
            return;
        }
        let graph_p = self.to_global.inverse() * p;
        let rects = self.snarl_rects();
        // (a frame's tab sits over the canvas: a click there isn't a click on what is under it)
        if self.over_frame_tab(p) {
            return;
        }
        match node_at(self, &rects, graph_p) {
            Some(n) if mods.command => self.picked.remove(&n),
            Some(n) if mods.shift => self.picked.add(&n),
            Some(n) => {
                self.picked.clear();
                self.picked.add(&n);
            }
            None if mods.shift || mods.command => {}
            None => self.picked.clear(),
        }
    }

    /// Wire re-routing. egui-snarl 0.12 lets a ⌘-drag on a pin pick up its wires, but it takes
    /// them out of its own copy of the graph without telling the viewer. So once the pointer is
    /// up, a wire the snarl was built with and no longer has was picked up, and:
    ///
    /// - dropped on another input (snarl asked to connect its source there): the `wire.connect`
    ///   becomes one `wire.reroute`, one undo step that keeps the wire's slice;
    /// - replaced, when its input got a new wire (a ⌘-drag from an output to another output):
    ///   the `wire.connect` already replaces it;
    /// - otherwise dropped on empty canvas: `wire.disconnect`.
    pub(crate) fn reconcile_wires(&mut self, ctx: &egui::Context, catalog: &SchemaReply, actions: &mut Vec<Action>) {
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        let now = snarl_wires(&self.snarl, catalog);
        let missing: Vec<(PortRef, PortRef)> = self.built_wires.iter().filter(|w| !now.contains(w)).cloned().collect();
        if missing.is_empty() {
            return;
        }
        let connect_index =
            |actions: &[Action], key: &str, port: &PortRef| actions.iter().position(|(id, p)| *id == "wire.connect" && p.get(key) == Some(&json!(port)));
        for (from, to) in missing {
            if connect_index(actions, "to", &to).is_some() {
                continue;
            }
            match connect_index(actions, "from", &from) {
                Some(i) => {
                    if let Some(a) = actions.get_mut(i) {
                        let new_to = a.1.get("to").cloned().unwrap_or(Value::Null);
                        *a = ("wire.reroute", json!({"to": to, "newTo": new_to}));
                    }
                }
                None => actions.push(("wire.disconnect", json!({"to": to}))),
            }
        }
        self.built_wires = now;
        // Rebuild from the graph next frame, whatever the session makes of these.
        self.invalidate();
    }

    /// Where input `port`'s pin was drawn last frame, on screen (tests drive wire drags with it).
    pub fn input_pin_on_screen(&self, port: &PortRef) -> Option<Pos2> {
        let pins = self.pins.borrow();
        self.snarl.nodes_ids_data().find_map(|(sid, n)| {
            let i =
                (0..n.value.inputs.len()).find(|&i| crate::canvas::in_port(&self.snarl, egui_snarl::InPinId { node: sid, input: i }).as_ref() == Some(port))?;
            pins.get(&crate::run_view::PinKey::In(egui_snarl::InPinId { node: sid, input: i })).map(|p| self.to_global * *p)
        })
    }

    /// Where output `port`'s pin was drawn last frame, on screen (a collapsed group's pin when
    /// its proxy stands for the port).
    pub fn output_pin_on_screen(&self, catalog: &SchemaReply, port: &PortRef) -> Option<Pos2> {
        let pins = self.pins.borrow();
        self.snarl.nodes_ids_data().find_map(|(sid, n)| {
            let o = (0..n.value.outputs)
                .find(|&o| crate::canvas::out_port(&self.snarl, catalog, egui_snarl::OutPinId { node: sid, output: o }).as_ref() == Some(port))?;
            pins.get(&crate::run_view::PinKey::Out(egui_snarl::OutPinId { node: sid, output: o })).map(|p| self.to_global * *p)
        })
    }

    /// Last frame's node rects (graph coordinates) by snarl id, in draw order as far as known.
    fn snarl_rects(&self) -> Vec<(SnarlId, Rect)> {
        let mut v: Vec<(SnarlId, Rect)> = self.snarl.nodes_ids_data().filter_map(|(sid, _)| Some((sid, *self.node_rect(sid)?))).collect();
        v.sort_by_key(|(sid, _)| sid.0);
        v
    }

    /// Moves after a drag. snarl moves the node dragged (and its own, possibly stale,
    /// selection); PlumCraft moves the dragged node, plus the whole selection, nodes and
    /// collapsed groups alike, when the dragged node is in it. A collapsed group's proxy moved
    /// is a `group.move`, which carries its members with it.
    ///
    /// The node dragged is the one under the pointer when the button went down
    /// ([`Canvas::drag_anchor`]), not the one under the press point now: after the first frame
    /// of a drag it has left that point.
    pub(crate) fn drag_moves(&mut self, ctx: &egui::Context, graph: &Graph) -> Vec<Action> {
        if ctx.input(|i| i.pointer.primary_pressed()) {
            let origin = ctx.input(|i| i.pointer.press_origin()).map(|p| self.to_global.inverse() * p);
            let rects = self.snarl_rects();
            self.drag_anchor = origin.and_then(|p| node_at(self, &rects, p)).map(|n| n.id);
        }
        // (canvas id, the group when a proxy, where snarl has it, where the graph has it)
        type Moved = (NodeId, Option<GroupId>, [f32; 2], [f32; 2]);
        let moved: Vec<Moved> = self
            .snarl
            .nodes_pos_ids()
            .filter_map(|(_, pos, n)| {
                let was = match &n.proxy {
                    Some(p) => graph.group(&p.group)?.pos,
                    None => graph.node(&n.id)?.pos,
                };
                let now = [pos.x, pos.y];
                (now != was).then(|| (n.id.clone(), n.proxy.as_ref().map(|p| p.group.clone()), now, was))
            })
            .collect();
        if moved.is_empty() {
            return Vec::new();
        }
        let to = |id: &NodeId, group: &Option<GroupId>, pos: [f32; 2]| match group {
            Some(g) => ("group.move", json!({"group": g, "pos": pos})),
            None => ("node.move", json!({"id": id, "pos": pos})),
        };
        let dragged = self.drag_anchor.as_ref().and_then(|a| moved.iter().find(|m| &m.0 == a));
        let Some((id, group, now, was)) = dragged else {
            // Not a drag we can attribute: take snarl's positions as they are.
            return moved.iter().map(|(id, g, now, _)| to(id, g, *now)).collect();
        };
        let in_selection = match group {
            Some(g) => self.picked.groups().contains(g),
            None => self.picked.nodes().contains(id),
        };
        if !in_selection {
            return vec![to(id, group, *now)];
        }
        let delta = [now[0] - was[0], now[1] - was[1]];
        let by = |p: [f32; 2]| [p[0] + delta[0], p[1] + delta[1]];
        // The selected groups' members move with their group, not again on their own.
        let carried: std::collections::BTreeSet<NodeId> = self.picked.groups().iter().flat_map(|g| graph.group_members(g)).collect();
        let mut out: Vec<Action> = self
            .picked
            .groups()
            .iter()
            .filter(|g| !self.picked.groups().iter().any(|outer| outer != *g && graph.group_subtree(outer).contains(*g)))
            .filter_map(|g| graph.group(g).map(|grp| ("group.move", json!({"group": g, "pos": by(grp.pos)}))))
            .collect();
        out.extend(
            self.picked
                .nodes()
                .iter()
                .filter(|n| !carried.contains(*n))
                .filter_map(|n| graph.node(n).map(|g| ("node.move", json!({"id": n, "pos": by(g.pos)})))),
        );
        out
    }

    /// Outline the selected nodes (snarl's own outline is hidden: its selection may be stale).
    pub(crate) fn paint_selection(&self, ctx: &egui::Context, tokens: &Tokens) {
        if self.picked.is_empty() {
            return;
        }
        // On the canvas's own layer, right after snarl's nodes: over them, under every window
        // (a Middle layer of its own was drawn over the parameters window).
        let nodes = egui::LayerId::new(egui::Order::Background, egui::Id::new(crate::canvas::CANVAS_ID));
        let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("plumcraft-selection"));
        ctx.set_sublayer(nodes, layer);
        let painter = ctx.layer_painter(layer).with_clip_rect(self.rect);
        let t = self.to_global;
        for (sid, r) in self.snarl_rects() {
            if self.snarl.get_node(sid).is_some_and(|n| self.picked.contains(n)) {
                let screen = Rect::from_min_max(t * r.min, t * r.max).expand(4.0);
                painter.rect_stroke(screen, 7.0, egui::Stroke::new(2.0, tokens.accent), egui::StrokeKind::Outside);
            }
        }
    }

    /// The selection's sizes (graph units), by node id and a collapsed group's id: what Align,
    /// Distribute and Frame Selection measure.
    pub fn selection_sizes(&self) -> BTreeMap<String, [f32; 2]> {
        let size_of = |(sid, r): (SnarlId, Rect)| {
            let n = self.snarl.get_node(sid).filter(|n| self.picked.contains(n))?;
            Some((n.proxy.as_ref().map_or_else(|| n.id.0.clone(), |p| p.group.0.clone()), [r.width(), r.height()]))
        };
        self.snarl_rects().into_iter().filter_map(size_of).collect()
    }

    /// Every node the canvas shows (members of an entered group only; a collapsed group counts
    /// as its proxy).
    pub fn select_all(&mut self) {
        let mut p = Picked { raw: std::mem::take(&mut self.picked.raw), ..Picked::default() };
        for (_, n) in self.snarl.nodes_ids_data() {
            p.add(&n.value);
        }
        self.picked = p;
    }

    /// Select the nodes a command made (`{nodes: [ids]}`, as duplicate returns), or the
    /// collapsed group it made (`{group}`, as Insert as Group returns).
    pub fn select_result(&mut self, result: &Value, graph: &Graph) {
        if let Some(g) = result.get("group").and_then(Value::as_str).map(GroupId::from).filter(|g| graph.group(g).is_some_and(|grp| grp.collapsed)) {
            self.picked.set_group(g);
            return;
        }
        let ids: Vec<NodeId> =
            result.get("nodes").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(NodeId::from).collect()).unwrap_or_default();
        if !ids.is_empty() {
            self.picked.set(ids);
        }
    }
}

/// An Edit menu action the canvas handles (beyond the registry's commands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOp {
    Cut,
    Copy,
    Paste,
    SelectAll,
    Delete,
    /// `edit.align`'s edge.
    Align(&'static str),
    /// `edit.distribute`'s axis.
    Distribute(&'static str),
    /// A frame around the selection (`frame.add`).
    Frame,
}

/// Align menu items: (label, edge).
pub const ALIGN_EDGES: [(&str, &str); 6] = [
    ("Left Edges", "left"),
    ("Right Edges", "right"),
    ("Top Edges", "top"),
    ("Bottom Edges", "bottom"),
    ("Centres Horizontally", "centerX"),
    ("Centres Vertically", "centerY"),
];

/// The Edit menu's canvas items, for `many` things selected. Returns the one clicked.
pub fn edit_menu(ui: &mut egui::Ui, many: usize) -> Option<EditOp> {
    let has_selection = many > 0;
    let mut op = None;
    ui.separator();
    let item = |ui: &mut egui::Ui, label: &str, shortcut: &str, enabled: bool| -> bool {
        let r = ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut));
        let clicked = r.clicked();
        if clicked {
            ui.close();
        }
        clicked
    };
    let cmd = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
    if item(ui, "Cut", &format!("{cmd}X"), has_selection) {
        op = Some(EditOp::Cut);
    }
    if item(ui, "Copy", &format!("{cmd}C"), has_selection) {
        op = Some(EditOp::Copy);
    }
    if item(ui, "Paste", &format!("{cmd}V"), true) {
        op = Some(EditOp::Paste);
    }
    if item(ui, "Delete", "⌫", has_selection) {
        op = Some(EditOp::Delete);
    }
    if item(ui, "Select All", &format!("{cmd}A"), true) {
        op = Some(EditOp::SelectAll);
    }
    if item(ui, "Frame Selection", "", has_selection) {
        op = Some(EditOp::Frame);
    }
    ui.separator();
    ui.add_enabled_ui(many >= 2, |ui| {
        ui.menu_button("Align", |ui| {
            for (label, edge) in ALIGN_EDGES {
                if ui.button(label).clicked() {
                    op = Some(EditOp::Align(edge));
                    ui.close();
                }
            }
        })
    });
    ui.add_enabled_ui(many >= 3, |ui| {
        ui.menu_button("Distribute", |ui| {
            for (label, axis) in [("Horizontally", "horizontal"), ("Vertically", "vertical")] {
                if ui.button(label).clicked() {
                    op = Some(EditOp::Distribute(axis));
                    ui.close();
                }
            }
        })
    });
    op
}

impl EditorApp {
    /// The selected nodes that are still in the graph, with the members of selected collapsed
    /// groups.
    fn selected_ids(&self) -> Vec<NodeId> {
        let g = self.session.graph();
        let mut ids: Vec<NodeId> = self.canvas.picked.nodes().iter().filter(|n| g.nodes.contains_key(*n)).cloned().collect();
        for gid in self.canvas.picked.groups() {
            for m in g.group_members(gid) {
                if !ids.contains(&m) {
                    ids.push(m);
                }
            }
        }
        ids
    }

    /// Copy (or cut) the selection to the clipboard; true when something was copied.
    fn copy_selection(&mut self, ctx: &egui::Context, id: &str) -> bool {
        let nodes = self.selected_ids();
        if nodes.is_empty() {
            self.last_error = Some(format!("select nodes to {} first", if id == "edit.cut" { "cut" } else { "copy" }));
            return false;
        }
        let Some(r) = self.run(id, &json!({"nodes": nodes})) else { return false };
        let Some(text) = r.get("text").and_then(Value::as_str) else { return false };
        ctx.copy_text(text.to_owned());
        self.copied = Some(text.to_owned());
        true
    }

    /// Paste workflow JSON at the pointer (or beside where it was copied), selecting the result.
    pub fn paste_text(&mut self, ctx: &egui::Context, text: &str) {
        let pointer = ctx.input(|i| i.pointer.hover_pos()).filter(|p| self.canvas.rect.contains(*p));
        let mut params = json!({"text": text});
        if let Some(p) = pointer {
            let g = self.canvas.to_global.inverse() * p;
            params["pos"] = json!([g.x, g.y]);
        }
        if let Some(r) = self.run("edit.paste", &params) {
            self.canvas.select_result(&r, self.session.graph());
            if let Some(dropped) = r.get("dropped").and_then(Value::as_array).filter(|d| !d.is_empty()) {
                self.last_error = Some(format!("pasted; {} wire(s) no longer fit and were left out", dropped.len()));
            }
        }
    }

    /// Apply an Edit menu (or keyboard) action.
    pub fn edit_op(&mut self, ctx: &egui::Context, op: EditOp) {
        let nodes = self.selected_ids();
        match op {
            EditOp::Copy => {
                self.copy_selection(ctx, "edit.copy");
            }
            EditOp::Cut => {
                if self.copy_selection(ctx, "edit.cut") {
                    self.canvas.picked.clear();
                }
            }
            // The window reads the system clipboard and sends a Paste event (handled next frame).
            EditOp::Paste => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
            EditOp::SelectAll => self.canvas.select_all(),
            EditOp::Delete => {
                if nodes.is_empty() {
                    self.last_error = Some("select nodes to delete first".into());
                } else if self.run("node.remove", &json!({"nodes": nodes})).is_some() {
                    self.canvas.picked.clear();
                }
            }
            EditOp::Align(edge) => {
                let p = json!({"edge": edge});
                self.arrange("edit.align", p);
            }
            EditOp::Distribute(axis) => {
                let p = json!({"axis": axis});
                self.arrange("edit.distribute", p);
            }
            EditOp::Frame => self.frame_selection(),
        }
    }

    /// The selection as what arranging takes: its nodes, its collapsed groups (as the one node
    /// each is on the canvas, not their members) and the sizes they were drawn at.
    fn selection_params(&self, mut p: Value) -> Value {
        let g = self.session.graph();
        let nodes: Vec<NodeId> = self.canvas.picked.nodes().iter().filter(|n| g.nodes.contains_key(*n)).cloned().collect();
        p["nodes"] = json!(nodes);
        p["groups"] = json!(self.canvas.picked.groups());
        p["sizes"] = json!(self.canvas.selection_sizes());
        p
    }

    fn arrange(&mut self, id: &str, params: Value) {
        let p = self.selection_params(params);
        self.run(id, &p);
    }

    /// Edit › Frame Selection.
    pub(crate) fn frame_selection(&mut self) {
        if self.canvas.picked.is_empty() {
            self.last_error = Some("select nodes to frame first".into());
        } else {
            self.arrange("frame.add", json!({}));
        }
    }

    /// Keyboard and clipboard editing, each frame before the canvas draws: Cmd+C/X/V (egui's
    /// Copy/Cut/Paste events), Delete/Backspace, Cmd+A, and Esc to cancel a run in flight.
    /// Nothing happens while a text field has the keyboard or a popup or prompt is open.
    pub fn edit_input(&mut self, ctx: &egui::Context) {
        if ctx.text_edit_focused() {
            return;
        }
        let busy =
            self.canvas.palette.is_some() || self.canvas.wire_menu.is_some() || self.prompt.is_some() || self.rename.is_some() || egui::Popup::is_any_open(ctx);
        if busy {
            return;
        }
        let mut ops = Vec::new();
        let mut pastes = Vec::new();
        ctx.input(|i| {
            for e in &i.events {
                match e {
                    egui::Event::Copy => ops.push(EditOp::Copy),
                    egui::Event::Cut => ops.push(EditOp::Cut),
                    egui::Event::Paste(text) => pastes.push(text.clone()),
                    _ => {}
                }
            }
        });
        let (delete, all, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::Delete) || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::A),
                self.session.run.running && i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if delete {
            ops.push(EditOp::Delete);
        }
        if all {
            ops.push(EditOp::SelectAll);
        }
        for op in ops {
            self.edit_op(ctx, op);
        }
        for text in pastes {
            self.paste_text(ctx, &text);
        }
        if escape {
            self.run("run.cancel", &Value::Null);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str) -> CanvasNode {
        CanvasNode { tucked: 0, all_fields: false, id: NodeId::from(id), kind: "k".into(), inputs: Vec::new(), outputs: 0, proxy: None }
    }

    #[test]
    fn picked_takes_only_what_snarl_adds() {
        let mut snarl = Snarl::new();
        let a = snarl.insert_node(Pos2::ZERO, node("n1"));
        let b = snarl.insert_node(Pos2::ZERO, node("n2"));
        let mut p = Picked::default();
        p.set(vec![NodeId::from("n9")]);
        p.absorb(&snarl, vec![a]);
        assert_eq!(p.nodes(), [NodeId::from("n9"), NodeId::from("n1")]);
        // snarl dropping an index (it may mean another node by now) removes nothing
        p.absorb(&snarl, vec![b]);
        assert_eq!(p.nodes(), [NodeId::from("n9"), NodeId::from("n1"), NodeId::from("n2")]);
        p.absorb(&snarl, vec![b]);
        assert_eq!(p.nodes().len(), 3, "no change, no growth");
        p.clear();
        assert!(p.is_empty());
        let mut g = Graph::new();
        g.nodes.insert(
            NodeId::from("n1"),
            plumcraft_graph::Node {
                kind: "k".into(),
                pos: [0.0, 0.0],
                values: Default::default(),
                inherited: Default::default(),
                all_fields: false,
                title: None,
                serial: None,
                view: Default::default(),
                cache: Default::default(),
                engine: None,
                extra: Default::default(),
            },
        );
        p.set(vec![NodeId::from("n1"), NodeId::from("n7")]);
        p.retain_existing(&g);
        assert_eq!(p.nodes(), [NodeId::from("n1")]);
    }

    /// text -> upper.text, plus a second upper, synced into a canvas.
    fn wired() -> (Canvas, SchemaReply, NodeId, NodeId, NodeId) {
        let c = plumcraft_text::engine().schema();
        let mut g = Graph::new();
        let text = g.add_node(c.node("text.literal").unwrap(), [0.0, 0.0]);
        let up = g.add_node(c.node("text.upper").unwrap(), [300.0, 0.0]);
        let up2 = g.add_node(c.node("text.upper").unwrap(), [300.0, 300.0]);
        g.connect(&c, PortRef::new(text.clone(), "text"), PortRef::new(up.clone(), "text")).unwrap();
        let mut canvas = Canvas::default();
        canvas.sync(1, &g, &c);
        assert_eq!(canvas.built_wires.len(), 1);
        (canvas, c, text, up, up2)
    }

    /// What egui-snarl does on a ⌘-drag from an input pin: drop the wire from its own copy.
    fn snarl_drops_the_wire(canvas: &mut Canvas) {
        let (_, inp) = canvas.snarl.wires().next().unwrap();
        canvas.snarl.drop_inputs(inp);
    }

    #[test]
    fn a_dragged_wire_end_is_moved_or_dropped() {
        let ctx = egui::Context::default();
        let port = |n: &NodeId, p: &str| json!(PortRef::new(n.clone(), p));
        // moved to another input: one reroute, which keeps the wire's slice
        let (mut canvas, c, text, up, up2) = wired();
        snarl_drops_the_wire(&mut canvas);
        let connect = ("wire.connect", json!({"from": port(&text, "text"), "to": port(&up2, "text")}));
        let mut actions: Vec<Action> = vec![connect];
        canvas.reconcile_wires(&ctx, &c, &mut actions);
        assert_eq!(actions, vec![("wire.reroute", json!({"to": port(&up, "text"), "newTo": port(&up2, "text")}))]);
        // dropped on empty canvas: disconnected
        let (mut canvas, c, _, up, _) = wired();
        snarl_drops_the_wire(&mut canvas);
        let mut actions: Vec<Action> = Vec::new();
        canvas.reconcile_wires(&ctx, &c, &mut actions);
        assert_eq!(actions, vec![("wire.disconnect", json!({"to": port(&up, "text")}))]);
        // its input got a new wire: the connect replaces it, nothing more
        let (mut canvas, c, _, up, up2) = wired();
        snarl_drops_the_wire(&mut canvas);
        let connect = ("wire.connect", json!({"from": port(&up2, "text"), "to": port(&up, "text")}));
        let mut actions: Vec<Action> = vec![connect.clone()];
        canvas.reconcile_wires(&ctx, &c, &mut actions);
        assert_eq!(actions, vec![connect]);
        // nothing missing: nothing changes
        let (mut canvas, c, ..) = wired();
        let mut actions: Vec<Action> = Vec::new();
        canvas.reconcile_wires(&ctx, &c, &mut actions);
        assert!(actions.is_empty());
    }

    #[test]
    fn nothing_is_reconciled_while_the_drag_goes_on() {
        let (mut canvas, c, ..) = wired();
        snarl_drops_the_wire(&mut canvas);
        let ctx = egui::Context::default();
        let mut actions: Vec<Action> = Vec::new();
        let input = egui::RawInput {
            events: vec![egui::Event::PointerButton {
                pos: egui::pos2(1.0, 1.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| canvas.reconcile_wires(ui.ctx(), &c, &mut actions));
        out.textures_delta.clear();
        assert!(actions.is_empty(), "the wire is still being dragged");
        assert_eq!(canvas.built_wires.len(), 1);
    }

    #[test]
    fn select_result_reads_new_ids() {
        let mut c = Canvas::default();
        let g = Graph::new();
        c.select_result(&json!({"nodes": ["n4", "n5"]}), &g);
        assert_eq!(c.picked.nodes(), [NodeId::from("n4"), NodeId::from("n5")]);
        c.select_result(&json!({"nodes": []}), &g);
        assert_eq!(c.picked.nodes().len(), 2, "nothing new: unchanged");
    }
}
