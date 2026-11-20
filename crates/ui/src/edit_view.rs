//! Editing on the canvas: the selection gestures, cut, copy and paste, select all, delete, align
//! and distribute, the Edit menu's canvas items, and Esc to cancel a run.
//!
//! **The selection is the document's** (`plumcraft_editor_core::selection`, N2 slice 1), and
//! changes only through the canvas's commands (`selection.set`, `selection.clear`,
//! `selection.all`, `edit.delete`, `net.select`). This canvas holds a handle to the same
//! selection ([`Picked`]) to draw it, and turns its gestures into those commands:
//!
//! - a click on a node is `selection.set {nodes: [id]}` (a collapsed group's node: `{groups:
//!   [g]}`), Shift+click `mode: add`, Cmd+click `mode: remove`, and a click on empty canvas
//!   `selection.clear`;
//! - Shift+drag on empty canvas is snarl's marquee: what it adds is `selection.set {…, mode:
//!   add}`;
//! - dragging a selected node (or collapsed group) moves everything selected, collapsed groups
//!   with their members;
//! - the canvas draws the selection outline itself (snarl's own is hidden, see
//!   [`crate::canvas::snarl_style`]).
//!
//! **egui-snarl's own selection** (0.12) is private state that can't be set from outside, kept by
//! snarl node index, which the canvas rebuilds on every edit (so an index can come to mean
//! another node). The canvas never reads it as the selection: it only compares snarl's set with
//! last frame's to see what a marquee or a Shift+click added ([`Picked::gained`]), and sends that
//! as a command. Snarl's own outline is hidden, so its stale set never shows.
//!
//! **The clipboard** holds workflow JSON (`edit.copy`), so nodes paste into another workflow, or
//! another PlumCraft. Cmd+C/X/V arrive as egui's Copy/Cut/Paste events (egui-winit reads the
//! system clipboard for Paste); Cut and Copy are then `edit.cut` and `edit.copy` invoked as their
//! menu items are (the core fills in the selection and asks for the clipboard).
//!
//! Every edit is a session command, so it is undoable.
//!
//! Lifted from the research software's `crates/ui-egui/src/edit_view.rs`. Its Cut (copy, then remove) is
//! `edit.cut`, one undo step. Align, Distribute and Frame Selection take selected collapsed groups as the one
//! node they are on the canvas.

use std::collections::BTreeMap;

use egui::{Pos2, Rect};
use egui_snarl::{NodeId as SnarlId, Snarl};
use plumcraft_editor_core::selection::{Mode, Selection, SelectionCell};
use plumcraft_graph::{Graph, GroupId, NodeId, PortRef};
use plumcraft_proto::catalogue::SchemaReply;
use serde_json::{Value, json};

use crate::EditorApp;
use crate::canvas::{Action, CANVAS_ID, Canvas, CanvasNode};
use crate::theme::Tokens;

/// The canvas's handle to the document's selection, and what snarl's own selection was last
/// frame (to see what its marquee added).
///
/// Its setters (`set`, `clear`, `set_group`, `absorb`) change the document's selection without a
/// command: they are for tests written before the selection moved into the core; the canvas's
/// gestures send commands.
#[derive(Debug, Clone, Default)]
pub struct Picked {
    cell: SelectionCell,
    /// snarl's own selection when last looked at, to see what its marquee added.
    raw: Vec<SnarlId>,
}

impl Picked {
    /// A handle to `cell`, the document's selection.
    pub fn on(cell: SelectionCell) -> Picked {
        Picked { cell, raw: Vec::new() }
    }

    /// The selection this canvas draws.
    pub fn cell(&self) -> &SelectionCell {
        &self.cell
    }

    pub fn nodes(&self) -> Vec<NodeId> {
        self.cell.with(|s| s.nodes.clone())
    }

    /// Selected collapsed groups (their proxy nodes).
    pub fn groups(&self) -> Vec<GroupId> {
        self.cell.with(|s| s.groups.clone())
    }

    pub fn is_empty(&self) -> bool {
        self.cell.with(Selection::is_empty)
    }

    /// Select exactly `nodes` (tests).
    pub fn set(&mut self, nodes: Vec<NodeId>) {
        self.cell.update(|s| s.set(nodes));
    }

    /// Select nothing (tests).
    pub fn clear(&mut self) {
        self.cell.update(Selection::clear);
    }

    /// Select exactly collapsed group `group` (tests).
    pub fn set_group(&mut self, group: GroupId) {
        self.cell.update(|s| s.set_group(group));
    }

    pub fn contains(&self, node: &CanvasNode) -> bool {
        self.cell.with(|s| match &node.proxy {
            Some(p) => s.groups.contains(&p.group),
            None => s.nodes.contains(&node.id),
        })
    }

    /// What snarl's selection gained since last time (its marquee, a Shift+click), as graph
    /// nodes and collapsed groups not selected yet. Only additions: an index snarl still holds
    /// may mean another node since the last rebuild.
    pub fn gained(&mut self, snarl: &Snarl<CanvasNode>, raw: Vec<SnarlId>) -> (Vec<NodeId>, Vec<GroupId>) {
        if raw == self.raw {
            return (Vec::new(), Vec::new());
        }
        let (mut nodes, mut groups) = (Vec::new(), Vec::new());
        for sid in raw.iter().filter(|s| !self.raw.contains(s)) {
            let Some(n) = snarl.get_node(*sid) else { continue };
            if self.contains(n) {
                continue;
            }
            match &n.proxy {
                Some(p) if !groups.contains(&p.group) => groups.push(p.group.clone()),
                None if !nodes.contains(&n.id) => nodes.push(n.id.clone()),
                _ => {}
            }
        }
        self.raw = raw;
        (nodes, groups)
    }

    /// [`Picked::gained`], taken into the selection at once (tests).
    pub fn absorb(&mut self, snarl: &Snarl<CanvasNode>, raw: Vec<SnarlId>) {
        let (nodes, groups) = self.gained(snarl, raw);
        self.cell.update(|s| s.apply(Mode::Add, nodes, groups));
    }

    /// Forget nodes and groups the graph no longer has (the document does this after every
    /// command).
    pub fn retain_existing(&mut self, graph: &Graph) {
        self.cell.update(|s| s.retain_existing(graph));
    }

    /// Forget what snarl had selected (its index set means nothing after a rebuild).
    fn forget_raw(&mut self) {
        self.raw.clear();
    }
}

/// A click's selection command: on node `n` (none: empty canvas), with these modifiers.
fn click_command(n: Option<&CanvasNode>, mods: egui::Modifiers) -> Option<Action> {
    let what = |n: &CanvasNode| match &n.proxy {
        Some(p) => json!({"groups": [p.group]}),
        None => json!({"nodes": [n.id]}),
    };
    let with = |n: &CanvasNode, mode: &str| {
        let mut p = what(n);
        p["mode"] = json!(mode);
        p
    };
    match n {
        Some(n) if mods.command => Some(("selection.set", with(n, "remove"))),
        Some(n) if mods.shift => Some(("selection.set", with(n, "add"))),
        Some(n) => Some(("selection.set", what(n))),
        None if mods.shift || mods.command => None,
        None => Some(("selection.clear", json!({}))),
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
    /// Selection gestures, after snarl has drawn: its marquee, then this frame's click, as the
    /// selection commands they are (run after the canvas has drawn, like every gesture's).
    pub(crate) fn selection_gestures(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = Vec::new();
        let raw = egui_snarl::ui::get_selected_nodes(egui::Id::new(CANVAS_ID), ctx);
        let (nodes, groups) = self.picked.gained(&self.snarl, raw);
        if !nodes.is_empty() || !groups.is_empty() {
            out.push(("selection.set", json!({"nodes": nodes, "groups": groups, "mode": "add"})));
        }
        if self.palette.is_some() || self.wire_menu.is_some() {
            return out;
        }
        let Some((p, mods)) = ctx.input(|i| if i.pointer.primary_clicked() { i.pointer.interact_pos().map(|p| (p, i.modifiers)) } else { None }) else {
            return out;
        };
        // Only clicks on the canvas itself, not on menus or popups over it.
        if !self.rect.contains(p) || ctx.layer_id_at(p).is_some_and(|l| l.order != egui::Order::Background && l.order != egui::Order::Middle) {
            return out;
        }
        let graph_p = self.to_global.inverse() * p;
        let rects = self.snarl_rects();
        // (a frame's tab sits over the canvas: a click there isn't a click on what is under it)
        if self.over_frame_tab(p) {
            return out;
        }
        out.extend(click_command(node_at(self, &rects, graph_p).as_ref(), mods));
        if !out.is_empty() {
            // the outline follows once the commands have run
            ctx.request_repaint();
        }
        out
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
    /// as its proxy), at once (tests; the Edit menu's Select All is `selection.all`).
    pub fn select_all(&mut self) {
        self.picked.forget_raw();
        let mut nodes = Vec::new();
        let mut groups = Vec::new();
        for (_, n) in self.snarl.nodes_ids_data() {
            match &n.value.proxy {
                Some(p) => groups.push(p.group.clone()),
                None => nodes.push(n.value.id.clone()),
            }
        }
        self.picked.cell().update(|s| s.apply(Mode::Replace, nodes, groups));
    }

    /// Select what a command made, at once (tests; the core selects it with `selection.set`:
    /// `plumcraft_editor_core::invoke::select_made`).
    pub fn select_result(&mut self, result: &Value, graph: &Graph) {
        let Some(p) = plumcraft_editor_core::selection::made(result, graph) else { return };
        if let Ok((nodes, groups, mode)) = plumcraft_editor_core::selection::parse_set(graph, &p) {
            self.picked.cell().update(|s| s.apply(mode, nodes, groups));
        } else {
            // ids the graph doesn't have (yet): taken as they are
            let ids: Vec<NodeId> = p["nodes"].as_array().into_iter().flatten().filter_map(Value::as_str).map(NodeId::from).collect();
            if !ids.is_empty() {
                self.picked.set(ids);
            }
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

/// Align menu items: (label, edge), as the core's menu model has them.
pub use plumcraft_editor_core::menu::ALIGN_EDGES;

impl EditOp {
    /// The Edit menu's canvas item `item` (the core's menu model names it; the shell does it
    /// until N2 gives it a command).
    pub fn from_item(item: &plumcraft_editor_core::menu::Item) -> Option<EditOp> {
        let param = |key: &str, from: &[(&'static str, &'static str)]| {
            let want = item.params.as_ref()?.get(key)?.as_str()?;
            from.iter().find(|(_, v)| *v == want).map(|(_, v)| *v)
        };
        Some(match item.id.as_str() {
            "edit.cut" => EditOp::Cut,
            "edit.copy" => EditOp::Copy,
            "edit.paste" => EditOp::Paste,
            "edit.delete" => EditOp::Delete,
            "selection.all" => EditOp::SelectAll,
            "frame.add" => EditOp::Frame,
            "edit.align" => EditOp::Align(param("edge", &ALIGN_EDGES)?),
            "edit.distribute" => EditOp::Distribute(param("axis", &[("Horizontally", "horizontal"), ("Vertically", "vertical")])?),
            _ => return None,
        })
    }
}

impl EditorApp {
    /// Paste workflow JSON at the pointer (or beside where it was copied), selecting the result:
    /// the answer to Edit › Paste's `clipboard.get` ask when one waits, else the window offering
    /// the clipboard to the core (⌘V).
    pub fn paste_text(&mut self, ctx: &egui::Context, text: &str) {
        let pointer = ctx.input(|i| i.pointer.hover_pos()).filter(|p| self.canvas.rect.contains(*p));
        self.paste_pos = pointer.map(|p| {
            let g = self.canvas.to_global.inverse() * p;
            [g.x, g.y]
        });
        self.pasted(text);
        self.paste_pos = None;
    }

    /// Apply an Edit menu (or keyboard) action: its command invoked as its menu item is, or, for
    /// Align and Distribute, run with the selection and the sizes this canvas measured.
    pub fn edit_op(&mut self, _ctx: &egui::Context, op: EditOp) {
        match op {
            EditOp::Copy => self.invoke("edit.copy"),
            EditOp::Cut => self.invoke("edit.cut"),
            // The core asks for the clipboard: the window reads it and sends a Paste event (handled
            // next frame), which answers.
            EditOp::Paste => self.invoke("edit.paste"),
            EditOp::SelectAll => self.invoke("selection.all"),
            EditOp::Delete => self.invoke("edit.delete"),
            EditOp::Align(edge) => {
                let p = json!({"edge": edge});
                self.arrange("edit.align", p);
            }
            EditOp::Distribute(axis) => {
                let p = json!({"axis": axis});
                self.arrange("edit.distribute", p);
            }
            EditOp::Frame => self.invoke("frame.add"),
        }
    }

    /// The selection as what arranging takes: its nodes, its collapsed groups (as the one node
    /// each is on the canvas, not their members) and the sizes they were drawn at.
    fn selection_params(&self, mut p: Value) -> Value {
        let g = self.doc.session.graph();
        let sel = self.doc.selection.get();
        let nodes: Vec<NodeId> = sel.nodes.into_iter().filter(|n| g.nodes.contains_key(n)).collect();
        p["nodes"] = json!(nodes);
        p["groups"] = json!(sel.groups);
        p["sizes"] = json!(self.canvas.selection_sizes());
        p
    }

    fn arrange(&mut self, id: &str, params: Value) {
        let p = self.selection_params(params);
        self.run(id, &p);
    }

    /// Edit › Frame Selection (`frame.add` invoked: the core asks this window, which knows the
    /// sizes).
    pub(crate) fn frame_selection(&mut self) {
        if self.doc.selection.with(Selection::is_empty) {
            self.doc.last_error = Some("select nodes to frame first".into());
        } else {
            self.arrange("frame.add", json!({}));
        }
    }

    /// What only this window does of a canvas command, once the core has checked it and kept its
    /// part (`plumcraft_editor_core::canvas_cmds`): fit, the palette, reveal.
    pub(crate) fn canvas_command(&mut self, id: &str, v: &Value) {
        match id {
            "canvas.fit" => self.canvas.request_fit(),
            "canvas.palette" => {
                if v["open"] == false {
                    self.canvas.palette = None;
                    return;
                }
                let at = v["at"].as_array().and_then(|a| Some(egui::pos2(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32)));
                let center = if self.canvas.rect.is_finite() { self.canvas.rect.center() } else { Pos2::ZERO };
                let screen = at.map_or(center, |g| self.canvas.to_global * g);
                match v["query"].as_str() {
                    Some(q) => {
                        if self.canvas.palette.is_none() {
                            self.canvas.open_palette(screen);
                        }
                        if let Some(state) = &mut self.canvas.palette {
                            state.query = q.to_owned();
                            state.selected = 0;
                        }
                    }
                    None => self.canvas.open_palette(screen),
                }
            }
            "canvas.reveal" => {
                if let Some(n) = v["node"].as_str() {
                    self.canvas.reveal(&NodeId::from(n));
                }
            }
            _ => {}
        }
    }

    /// Keyboard and clipboard editing, each frame before the canvas draws: Cmd+C/X/V (egui's
    /// Copy/Cut/Paste events), Delete/Backspace, Cmd+A, and Esc to cancel a run in flight, each
    /// the command it stands for. Nothing happens while a text field has the keyboard or a popup
    /// or prompt is open.
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
                self.doc.session.run.running && i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
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
    fn the_canvas_draws_the_documents_selection_and_follows_its_focus() {
        let c = plumcraft_text::engine().schema();
        let mut app = EditorApp::new(plumcraft_session::Session::new(c));
        assert!(app.canvas.picked.cell().same(&app.doc.selection), "one selection");
        let a = app.try_run("node.add", &json!({"kind": "text.literal", "pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
        app.try_run("selection.set", &json!({"nodes": [a]})).unwrap();
        assert_eq!(app.canvas.selection(), [NodeId::from(a.as_str())]);
        let g = app.try_run("group.create", &json!({"nodes": [a]})).unwrap()["id"].as_str().unwrap().to_owned();
        // ui.set {focus} is canvas.focus: the document's, shown by the canvas at once
        let ctx = egui::Context::default();
        let r = app.control_now(&ctx, "ui.set", &json!({"focus": g}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!((app.doc.focus.clone(), app.canvas.focus.clone()), (Some(GroupId::from(g.as_str())), Some(GroupId::from(g.as_str()))));
        let r = app.control_now(&ctx, "ui.set", &json!({"focus": "g999"}));
        assert_eq!(r["error"], "no group g999");
        app.try_run("canvas.focus", &json!({"group": null})).unwrap();
        assert_eq!(app.canvas.focus, None);
        // what only the window does: done here
        app.try_run("canvas.palette", &json!({"query": "upp"})).unwrap();
        assert_eq!(app.canvas.palette.as_ref().map(|p| p.query.as_str()), Some("upp"));
        app.try_run("canvas.palette", &json!({"open": false})).unwrap();
        assert!(app.canvas.palette.is_none());
    }

    #[test]
    fn a_click_is_a_selection_command() {
        let n = node("n1");
        let none = egui::Modifiers::NONE;
        assert_eq!(click_command(Some(&n), none), Some(("selection.set", json!({"nodes": ["n1"]}))));
        assert_eq!(click_command(Some(&n), egui::Modifiers::SHIFT).unwrap().1["mode"], "add");
        assert_eq!(click_command(Some(&n), egui::Modifiers::COMMAND).unwrap().1["mode"], "remove");
        assert_eq!(click_command(None, none), Some(("selection.clear", json!({}))));
        assert_eq!(click_command(None, egui::Modifiers::SHIFT), None);
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
