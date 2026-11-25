//! Editing on the canvas: the selection gestures, cut, copy and paste, select all, delete, align
//! and distribute, the Edit menu's canvas items, and Esc to cancel a run.
//!
//! **The selection is the document's** (`plumcraft_editor_core::selection`, N2 slice 1), and
//! changes only through the canvas's commands (`selection.set`, `selection.clear`,
//! `selection.all`, `edit.delete`, `net.select`). This canvas holds a handle to the same
//! selection ([`Picked`]) to draw it. **Since N2 slice 4 its clicks and marquees are gestures
//! the core decides** (`ui.gesture`, `plumcraft_editor_core::gesture`), hit-tested against the
//! faces this canvas measured:
//!
//! - a click is a tap: on a node `selection.set` (Shift adds, Cmd takes away), on a pin or a
//!   wire `run.watch` of its value, on empty canvas `selection.clear`;
//! - a Shift-drag begun on empty canvas is a marquee, sent when let go (egui-snarl draws its
//!   rect; snarl's own selection is no longer read: the bridge that copied what it gained is
//!   gone);
//! - dragging a selected node (or collapsed group) moves everything selected, collapsed groups
//!   with their members (still worked out here from snarl's drag, [`Canvas::drag_moves`]: snarl
//!   owns node drags);
//! - the canvas draws the selection outline itself (snarl's own is hidden, see
//!   [`crate::canvas::snarl_style`]).
//!
//! **egui-snarl's own selection** (0.12) is private state that can't be set from outside. It
//! still grows as snarl sees clicks, and snarl still moves what it thinks is selected while a
//! node is dragged; what is moved in the graph is the document's selection
//! ([`Canvas::drag_moves`]), and the canvas is rebuilt from the graph after.
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

use std::collections::HashMap;

use egui::{Pos2, Rect};
use egui_snarl::{NodeId as SnarlId, Snarl};
use plumcraft_editor_core::selection::{Mode, Selection, SelectionCell};
use plumcraft_graph::{Graph, GroupId, NodeId, PortRef};
use plumcraft_proto::catalogue::SchemaReply;
use serde_json::{Value, json};

use crate::EditorApp;
use crate::canvas::{Action, Canvas, CanvasNode};
use crate::theme::Tokens;

/// The canvas's handle to the document's selection.
///
/// Its setters (`set`, `clear`, `set_group`) change the document's selection without a command:
/// they are for tests written before the selection moved into the core; the canvas's gestures
/// go through the core (`ui.gesture`).
#[derive(Debug, Clone, Default)]
pub struct Picked {
    cell: SelectionCell,
}

impl Picked {
    /// A handle to `cell`, the document's selection.
    pub fn on(cell: SelectionCell) -> Picked {
        Picked { cell }
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

    /// Forget nodes and groups the graph no longer has (the document does this after every
    /// command).
    pub fn retain_existing(&mut self, graph: &Graph) {
        self.cell.update(|s| s.retain_existing(graph));
    }
}

/// egui's modifiers as `ui.gesture` takes them (⌘ on the Mac, Ctrl elsewhere, is `primary`).
fn gesture_mods(mods: egui::Modifiers) -> Value {
    let mut v = Vec::new();
    if mods.shift {
        v.push("shift");
    }
    if mods.command {
        v.push("primary");
    }
    json!(v)
}

/// A click on the canvas at graph point `p`, as the gesture it is: what it means (select, watch
/// a pin's or a wire's value, clear) is the core's (`plumcraft_editor_core::gesture`), hit-tested
/// against the faces this canvas measured. `scale` is the view's zoom: the hit distance is the
/// canvas's wire hit in screen points.
fn tap_action(p: Pos2, mods: egui::Modifiers, scale: f32) -> Action {
    ("ui.gesture", json!({"kind": "tap", "at": [p.x, p.y], "mods": gesture_mods(mods), "slop": crate::canvas::WIRE_HIT / scale.max(0.05)}))
}

/// A double click on the canvas at graph point `p`: `ui.gesture {kind: "doubleTap"}`, which the
/// core decides ("open what it is on", `plumcraft_editor_core::gesture`).
fn double_tap_action(p: Pos2, mods: egui::Modifiers, scale: f32) -> Action {
    ("ui.gesture", json!({"kind": "doubleTap", "at": [p.x, p.y], "mods": gesture_mods(mods), "slop": crate::canvas::WIRE_HIT / scale.max(0.05)}))
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
    /// Selection gestures, after snarl has drawn: a Shift-drag's marquee, then this frame's
    /// click, as `ui.gesture`s the core decides (N2 slice 4: snarl's private selection is no
    /// longer read, and the canvas no longer hit-tests a click itself).
    pub(crate) fn selection_gestures(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = Vec::new();
        out.extend(self.marquee_gesture(ctx));
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
        // (a frame's tab and a net's tag sit over the canvas and take their own clicks: a click
        // there isn't a click on what is under it)
        if self.over_frame_tab(p) || self.over_net_tag(p) {
            return out;
        }
        let at = self.to_global.inverse() * p;
        // A double click is a double tap: what it opens is the core's to decide (N3's card 3,
        // "the same in every shell": empty canvas the palette, a face its parameters, a
        // collapsed group the group, a pin, a wire or a preview its value in a pop-out, a bend
        // point goes). egui counts a click within 0.6 s of the one before last as a triple, and
        // then no double: that is a double tap too. A double click in a face's field is the
        // field's (a word selected, a value typed).
        let double =
            ctx.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary) || i.pointer.button_triple_clicked(egui::PointerButton::Primary));
        if double {
            if !ctx.text_edit_focused() && !self.field_rects.iter().any(|r| r.contains(at)) {
                out.push(double_tap_action(at, mods, self.to_global.scaling));
            }
        } else {
            out.push(tap_action(at, mods, self.to_global.scaling));
        }
        // the outline follows once the gesture has run
        ctx.request_repaint();
        out
    }

    /// A Shift-drag that began on empty canvas, once let go: `ui.gesture {kind: "marquee"}` from
    /// where it began to where it ended (⌘ too takes away). egui-snarl draws the rect while it
    /// is dragged; what it selects is the core's, from the faces this canvas measured.
    fn marquee_gesture(&mut self, ctx: &egui::Context) -> Option<Action> {
        let (pressed, released, origin, at, mods) =
            ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.press_origin(), i.pointer.interact_pos(), i.modifiers));
        if pressed {
            self.marquee_from = origin
                .filter(|o| {
                    mods.shift
                        && self.palette.is_none()
                        && self.rect.contains(*o)
                        && !self.over_node(*o)
                        && ctx.layer_id_at(*o).is_none_or(|l| l.order == egui::Order::Background)
                })
                .map(|o| (self.to_global.inverse() * o, mods));
        }
        if !released {
            return None;
        }
        let (from, began) = self.marquee_from.take()?;
        let to = self.to_global.inverse() * at?;
        // a press and release in place is a click, not a marquee
        if from.distance(to) * self.to_global.scaling < 3.0 {
            return None;
        }
        let mut m = began;
        m.command |= mods.command;
        Some(("ui.gesture", json!({"kind": "marquee", "at": [from.x, from.y], "to": [to.x, to.y], "mods": gesture_mods(m)})))
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
    ///
    /// A face is moved from where it is shown, which a push may have moved from its node's
    /// position (N4 slice 4, [`crate::shown`]): a node moved is one snarl has away from where the
    /// canvas put it, and the move names the shown place plus the drag, as the core's own
    /// gestures do.
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
            .filter_map(|(sid, pos, n)| {
                let own = match &n.proxy {
                    Some(p) => graph.group(&p.group)?.pos,
                    None => graph.node(&n.id)?.pos,
                };
                let was = self.placed_at(sid).map_or(own, |p| [p.x, p.y]);
                let now = [pos.x, pos.y];
                (now != was).then(|| (n.id.clone(), n.proxy.as_ref().map(|p| p.group.clone()), now, was))
            })
            .collect();
        if moved.is_empty() {
            return Vec::new();
        }
        // One `edit.move` a frame, named by the press, so the whole drag is one undo step
        // ("Move 3 Nodes"; N3 slice 1) however many frames it takes.
        let drag = format!("egui:{}", self.press);
        let one = |nodes: serde_json::Map<String, Value>, groups: serde_json::Map<String, Value>| -> Vec<Action> {
            vec![("edit.move", json!({"nodes": nodes, "groups": groups, "drag": drag}))]
        };
        let split = |list: Vec<(NodeId, Option<GroupId>, [f32; 2])>| {
            let (mut nodes, mut groups) = (serde_json::Map::new(), serde_json::Map::new());
            for (id, g, pos) in list {
                match g {
                    Some(g) => groups.insert(g.0, json!(pos)),
                    None => nodes.insert(id.0, json!(pos)),
                };
            }
            (nodes, groups)
        };
        let dragged = self.drag_anchor.as_ref().and_then(|a| moved.iter().find(|m| &m.0 == a));
        let Some((id, group, now, was)) = dragged else {
            // Not a drag we can attribute: take snarl's positions as they are.
            let (nodes, groups) = split(moved.iter().map(|(id, g, now, _)| (id.clone(), g.clone(), *now)).collect());
            return one(nodes, groups);
        };
        let in_selection = match group {
            Some(g) => self.picked.groups().contains(g),
            None => self.picked.nodes().contains(id),
        };
        if !in_selection {
            let (nodes, groups) = split(vec![(id.clone(), group.clone(), *now)]);
            return one(nodes, groups);
        }
        let delta = [now[0] - was[0], now[1] - was[1]];
        // each from where it is shown: a face's place on the canvas, else the graph's
        let placed: HashMap<&str, [f32; 2]> =
            self.snarl.nodes_ids_data().filter_map(|(sid, n)| self.placed_at(sid).map(|p| (n.value.id.0.as_str(), [p.x, p.y]))).collect();
        let shown = |face: &str, own: [f32; 2]| placed.get(face).copied().unwrap_or(own);
        let by = |p: [f32; 2]| [p[0] + delta[0], p[1] + delta[1]];
        // The selected groups' members move with their group, not again on their own.
        let carried: std::collections::BTreeSet<NodeId> = self.picked.groups().iter().flat_map(|g| graph.group_members(g)).collect();
        let groups: serde_json::Map<String, Value> = self
            .picked
            .groups()
            .iter()
            .filter(|g| !self.picked.groups().iter().any(|outer| outer != *g && graph.group_subtree(outer).contains(*g)))
            .filter_map(|g| graph.group(g).map(|grp| (g.0.clone(), json!(by(shown(&crate::groups_view::proxy_node_id(g).0, grp.pos))))))
            .collect();
        let nodes: serde_json::Map<String, Value> = self
            .picked
            .nodes()
            .iter()
            .filter(|n| !carried.contains(*n))
            .filter_map(|n| graph.node(n).map(|g| (n.0.clone(), json!(by(shown(&n.0, g.pos))))))
            .collect();
        one(nodes, groups)
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

    /// Every node the canvas shows (members of an entered group only; a collapsed group counts
    /// as its proxy), at once (tests; the Edit menu's Select All is `selection.all`).
    pub fn select_all(&mut self) {
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
            // the core's (N2 slice 3): the selection, at the sizes this canvas reported
            EditOp::Align(edge) => {
                self.run("selection.align", &json!({"edge": edge}));
            }
            EditOp::Distribute(axis) => {
                self.run("selection.distribute", &json!({"axis": axis}));
            }
            EditOp::Frame => self.invoke("frame.add"),
        }
    }

    /// What only this window does of a canvas command, once the core has checked it and kept its
    /// part (`plumcraft_editor_core::canvas_cmds`): fit, the palette, reveal.
    pub(crate) fn canvas_command(&mut self, id: &str, v: &Value) {
        match id {
            // the core worked the rect out from what this canvas measured (N2 slice 3)
            "canvas.fit" => match v["rect"].as_array().map(|a| a.iter().filter_map(Value::as_f64).map(|f| f as f32).collect::<Vec<f32>>()) {
                Some(r) if r.len() == 4 => self.canvas.fit_to([r[0], r[1], r[2], r[3]]),
                _ => self.canvas.request_fit(),
            },
            // a node picked: the core added it (with the dropped wire connected) and closed its
            // palette (N3 slice 1)
            "palette.choose" => self.canvas.palette = None,
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
                // the kinds of engines not started, listed after the catalogue's own (N4 slice 1,
                // D3: the core's `palette.items` lists the same; choosing one starts its engine)
                let starts: Vec<plumcraft_proto::catalogue::NodeDef> =
                    plumcraft_session::engine_cmds::startable_defs(&self.doc.session).into_iter().cloned().collect();
                // opened for a dropped wire (question 8, N3 slice 1): only what takes it is offered
                let fits = self.doc.palette.as_ref().and_then(|p| p.wire.as_ref()).and_then(|w| w.end()).map(|(end, output)| {
                    let refs: Vec<&plumcraft_proto::catalogue::NodeDef> = starts.iter().collect();
                    let catalog = plumcraft_editor_core::palette::with_starts(self.doc.session.catalog(), &refs);
                    plumcraft_editor_core::palette::fitting(self.doc.session.graph(), &catalog, &end, output).into_iter().map(|(i, _)| i).collect()
                });
                self.canvas.palette_starts = starts;
                if let Some(state) = &mut self.canvas.palette {
                    state.fits = fits;
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
        // the arrows nudge the selection, 1 graph unit, 10 with Shift (N3 slice 1:
        // `selection.nudge`; a held key's nudges are one undo step, each new press another); only
        // with something selected, so the keys are left alone otherwise
        let mut nudge = [0.0f32; 2];
        if !self.doc.selection.with(|s| s.nodes.is_empty() && s.groups.is_empty()) {
            // a press that isn't a key's repeat starts a new nudge step; its repeats join it
            let arrows = [egui::Key::ArrowLeft, egui::Key::ArrowRight, egui::Key::ArrowUp, egui::Key::ArrowDown];
            let fresh = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { key, pressed: true, repeat: false, .. } if arrows.contains(key))));
            if fresh {
                self.canvas.nudge_press = self.canvas.nudge_press.wrapping_add(1);
            }
            ctx.input_mut(|i| {
                for (key, d) in [
                    (egui::Key::ArrowLeft, [-1.0, 0.0]),
                    (egui::Key::ArrowRight, [1.0, 0.0]),
                    (egui::Key::ArrowUp, [0.0, -1.0]),
                    (egui::Key::ArrowDown, [0.0, 1.0]),
                ] {
                    // Shift first: a plain key's match ignores Shift
                    for (mods, step) in [(egui::Modifiers::SHIFT, 10.0), (egui::Modifiers::NONE, 1.0)] {
                        while i.consume_key(mods, key) {
                            nudge[0] += d[0] * step;
                            nudge[1] += d[1] * step;
                        }
                    }
                }
            });
        }
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
        if nudge != [0.0, 0.0] {
            // named by the press, so a held key is one undo step however long it is held
            self.run("selection.nudge", &json!({"by": nudge, "drag": format!("key:{}", self.canvas.nudge_press)}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picked_is_the_documents_selection() {
        let mut p = Picked::default();
        p.set(vec![NodeId::from("n9")]);
        assert_eq!(p.nodes(), [NodeId::from("n9")]);
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
                face: Default::default(),
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

    /// A click is a gesture the core decides: where it was (graph units), the modifiers, and the
    /// wire hit in screen points at the view's zoom (what it selects is
    /// `plumcraft_editor_core::gesture`'s, checked by the conformance suite).
    #[test]
    fn a_click_is_a_tap_gesture() {
        let (id, p) = tap_action(Pos2::new(10.0, 20.0), egui::Modifiers::NONE, 2.0);
        assert_eq!((id, &p["kind"], &p["at"], &p["mods"]), ("ui.gesture", &json!("tap"), &json!([10.0, 20.0]), &json!([])));
        assert_eq!(p["slop"], json!(crate::canvas::WIRE_HIT / 2.0));
        assert_eq!(tap_action(Pos2::ZERO, egui::Modifiers::SHIFT, 1.0).1["mods"], json!(["shift"]));
        assert_eq!(tap_action(Pos2::ZERO, egui::Modifiers::COMMAND, 1.0).1["mods"], json!(["primary"]));
        assert!(plumcraft_editor_core::gesture::parse(&tap_action(Pos2::ZERO, egui::Modifiers::COMMAND, 0.0).1).is_ok());
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
