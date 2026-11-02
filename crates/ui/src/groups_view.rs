//! Groups on the canvas (plan 10.3, E3).
//!
//! - **Collapsed groups** show as one proxy node. Its rows are the group's external inputs
//!   (ports the engine marks `external`), wired or not; then its promoted inputs (with their
//!   widgets, editing the member's value); then, member by member, each input wired from outside
//!   and each required wire-only input nothing feeds (an input with a widget has a value and an
//!   optional one has a default, so they stay inside unless promoted or wired). Its outputs are
//!   the group's declared outputs, then its external outputs, then, member by member, each
//!   output wired to the outside and, when the group has neither declared nor external outputs,
//!   each output wired nowhere. So a group shows what it needs and what it makes, and a pin
//!   keeps its place when it is wired. Wires crossing the boundary attach to the proxy, so
//!   nothing vanishes, and a wire dragged to or from a proxy's pin connects the member's port.
//! - **Open groups** get a frame around their members, with a header to collapse, enter, rename
//!   or ungroup them; dragging the header moves the group.
//! - **Moving** a group (its collapsed node, or an open group's header) moves its members and
//!   nested groups by the same offset (`group.move`), so it opens where it now is; collapsing
//!   puts its node at its members' top-left.
//! - **Entering** a group shows only its members (a view state, not an edit), with a breadcrumb
//!   to leave.
//! - Node menus carry the group actions: group or collapse the selection, promote an input,
//!   expose an output, edit a promoted parameter.
//!
//! Every change is a session command (`group.*`), so it is undoable; only entering and leaving
//! a group (what the canvas shows) is not.
//!
//! Lifted from the research software's `crates/ui-egui/src/groups_view.rs` (2026-10-27, the research software `8d96021`)
//! without plugin export, command expansion and settings bundles (VI's pack), and
//! with names that are any short text rather than Python identifiers.

use std::collections::{BTreeMap, BTreeSet};

use egui::{Pos2, Rect, Vec2};
use egui_snarl::{InPinId, NodeId as SnarlId, OutPinId, Snarl};
use plumcraft_graph::{Graph, Group, GroupId, NodeId, PortRef, Promoted, Wire};
use plumcraft_proto::catalogue::{NodeDef, Port, SchemaReply, scalars, texts};
use serde_json::json;

use crate::canvas::{Action, Canvas, CanvasNode, InputRow, RowKind, input_rows};
use crate::theme::Tokens;

/// An output of a proxy: which member output it stands for.
#[derive(Debug, Clone, PartialEq)]
pub struct ProxyOut {
    pub port: PortRef,
    pub label: String,
    pub def: Port,
}

/// What a collapsed group's proxy node stands for.
#[derive(Debug, Clone, PartialEq)]
pub struct Proxy {
    pub group: GroupId,
    pub name: String,
    /// The member node of each input row (the row's `key` is the member's port).
    pub rows: Vec<NodeId>,
    pub outputs: Vec<ProxyOut>,
}

impl Proxy {
    fn input_index(&self, inputs: &[InputRow], to: &PortRef) -> Option<usize> {
        self.rows.iter().zip(inputs).position(|(n, r)| n == &to.node && r.key == to.port)
    }

    fn output_index(&self, from: &PortRef) -> Option<usize> {
        self.outputs.iter().position(|o| &o.port == from)
    }
}

/// The graph id a proxy node goes by (never a real node id: those are `n<k>` or a file's own
/// names, and a file's can't contain `:` and be a group's).
pub fn proxy_node_id(group: &GroupId) -> NodeId {
    NodeId(format!("group:{group}"))
}

/// The node behind input row `index` of a snarl node (a proxy's row stands for a member).
pub fn row_node(node: &CanvasNode, index: usize) -> NodeId {
    node.proxy.as_ref().and_then(|p| p.rows.get(index)).cloned().unwrap_or_else(|| node.id.clone())
}

/// The graph port of an input pin.
pub fn in_port(snarl: &Snarl<CanvasNode>, pin: InPinId) -> Option<PortRef> {
    let node = snarl.get_node(pin.node)?;
    let key = node.inputs.get(pin.input)?.key.clone();
    Some(PortRef::new(row_node(node, pin.input), key))
}

/// The graph port of an output pin.
pub fn out_port(snarl: &Snarl<CanvasNode>, catalog: &SchemaReply, pin: OutPinId) -> Option<PortRef> {
    let node = snarl.get_node(pin.node)?;
    match &node.proxy {
        Some(p) => Some(p.outputs.get(pin.output)?.port.clone()),
        None => Some(PortRef::new(node.id.clone(), catalog.node(&node.kind)?.outputs.get(pin.output)?.name.clone())),
    }
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

fn port_label(p: &Port) -> String {
    match p.label.as_deref() {
        Some(l) if !l.is_empty() => l.to_owned(),
        _ => p.name.clone(),
    }
}

fn node_title(graph: &Graph, catalog: &SchemaReply, id: &NodeId) -> String {
    plumcraft_session::node_title(graph, catalog, id)
}

/// `base`, or `base 2`, `base 3`, … whichever `taken` doesn't have.
pub fn unique_name(base: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_owned();
    }
    (2u32..).map(|i| format!("{base} {i}")).find(|n| !taken.contains(n)).unwrap_or_else(|| base.to_owned())
}

/// The proxy node of collapsed group `gid`, and where it sits.
pub fn proxy_node(graph: &Graph, catalog: &SchemaReply, gid: &GroupId) -> Option<(Pos2, CanvasNode)> {
    let grp = graph.group(gid)?;
    let members = graph.group_members(gid);
    let mut inputs = Vec::new();
    let mut rows = Vec::new();
    let row_for = |at: &PortRef| -> Option<InputRow> {
        let n = graph.node(&at.node)?;
        let def = catalog.node(&n.kind)?;
        input_rows(def, &at.node, graph).into_iter().find(|r| r.key == at.port)
    };
    // Members top to bottom as they sit, so the pins keep the members' order.
    let mut ordered: Vec<&NodeId> = members.iter().collect();
    ordered.sort_by(|a, b| {
        let at = |n: &NodeId| graph.node(n).map_or([0.0, 0.0], |n| n.pos);
        let (pa, pb) = (at(a), at(b));
        pa[1].total_cmp(&pb[1]).then(pa[0].total_cmp(&pb[0])).then(a.cmp(b))
    });
    let defs: Vec<(&NodeId, &NodeDef)> = ordered.iter().filter_map(|n| Some((*n, catalog.node(&graph.node(n)?.kind)?))).collect();
    // The group's own pins (ports the engine marks external), wired or not.
    for (n, def) in &defs {
        for port in def.inputs.iter().filter(|p| p.external) {
            let at = PortRef::new((*n).clone(), port.name.clone());
            if let Some(mut row) = row_for(&at) {
                row.label = format!("{} › {}", node_title(graph, catalog, n), row.label);
                row.kind = RowKind::Plain;
                inputs.push(row);
                rows.push((*n).clone());
            }
        }
    }
    for p in &grp.promoted {
        if rows.iter().zip(&inputs).any(|(n, r)| n == &p.port.node && r.key == p.port.port) {
            continue;
        }
        if let Some(mut row) = row_for(&p.port) {
            row.label.clone_from(&p.label);
            if !p.doc.is_empty() {
                row.port.tooltip = Some(p.doc.clone());
            }
            // The parameter's own range, inside the inner port's.
            row.port.min = p.min.or(row.port.min);
            row.port.max = p.max.or(row.port.max);
            row.kind = RowKind::Plain;
            inputs.push(row);
            rows.push(p.port.node.clone());
        }
    }
    // Member by member: inputs wired from outside, and required wire-only inputs nothing feeds.
    for (n, def) in &defs {
        for mut row in input_rows(def, n, graph) {
            if rows.iter().zip(&inputs).any(|(m, r)| m == *n && r.key == row.key) {
                continue;
            }
            let open = match graph.wire_into(&PortRef::new((*n).clone(), row.key.clone())) {
                Some(w) => !members.contains(&w.from.node),
                None => !row.port.has_widget() && !row.port.optional && !matches!(row.kind, RowKind::Slot { index, .. } if index > 0),
            };
            if open {
                row.label = format!("{} › {}", node_title(graph, catalog, n), row.label);
                row.kind = RowKind::Plain;
                inputs.push(row);
                rows.push((*n).clone());
            }
        }
    }
    // (any other wire from outside, to a port the member doesn't show as a row)
    for w in graph.wires.iter().filter(|w| members.contains(&w.to.node) && !members.contains(&w.from.node)) {
        if rows.iter().zip(&inputs).any(|(n, r)| n == &w.to.node && r.key == w.to.port) {
            continue;
        }
        if let Some(mut row) = row_for(&w.to) {
            row.label = format!("{} › {}", node_title(graph, catalog, &w.to.node), row.label);
            row.kind = RowKind::Plain;
            inputs.push(row);
            rows.push(w.to.node.clone());
        }
    }
    let out_def = |at: &PortRef| -> Option<Port> { catalog.node(&graph.node(&at.node)?.kind)?.output(&at.port).cloned() };
    let mut outputs: Vec<ProxyOut> = Vec::new();
    for o in &grp.outputs {
        if let Some(def) = out_def(&o.from) {
            outputs.push(ProxyOut { port: o.from.clone(), label: o.name.clone(), def });
        }
    }
    for (n, def) in &defs {
        for port in def.outputs.iter().filter(|p| p.external) {
            let at = PortRef::new((*n).clone(), port.name.clone());
            if !outputs.iter().any(|o| o.port == at) {
                outputs.push(ProxyOut { port: at, label: node_title(graph, catalog, n), def: port.clone() });
            }
        }
    }
    // Member by member: outputs wired to the outside, and (unless the group already names what
    // it makes) outputs wired nowhere.
    let loose = outputs.is_empty();
    for (n, def) in &defs {
        for port in &def.outputs {
            let at = PortRef::new((*n).clone(), port.name.clone());
            if outputs.iter().any(|o| o.port == at) {
                continue;
            }
            let to: Vec<&NodeId> = graph.wires.iter().filter(|w| w.from == at).map(|w| &w.to.node).collect();
            if to.iter().any(|t| !members.contains(*t)) || (to.is_empty() && loose) {
                let label = format!("{} › {}", node_title(graph, catalog, n), port_label(port));
                outputs.push(ProxyOut { port: at, label, def: port.clone() });
            }
        }
    }
    let proxy = Proxy { group: gid.clone(), name: grp.name.clone(), rows, outputs };
    let node =
        CanvasNode { tucked: 0, all_fields: false, id: proxy_node_id(gid), kind: String::new(), outputs: proxy.outputs.len(), inputs, proxy: Some(proxy) };
    Some((Pos2::new(grp.pos[0], grp.pos[1]), node))
}

/// The snarl pins of graph wire `w`, given where nodes and proxies landed. `None` for a wire
/// that isn't drawn (an end outside an entered group, or both ends behind the same proxy).
pub fn wire_ends(
    snarl: &Snarl<CanvasNode>,
    catalog: &SchemaReply,
    graph: &Graph,
    vis: &Visibility,
    ids: &BTreeMap<&NodeId, SnarlId>,
    proxies: &BTreeMap<GroupId, SnarlId>,
    w: &Wire,
) -> Option<(OutPinId, InPinId)> {
    if vis.is_outside(&w.from.node) || vis.is_outside(&w.to.node) {
        return None;
    }
    let from = match vis.proxy_of(&w.from.node) {
        Some(g) => {
            let sid = *proxies.get(g)?;
            OutPinId { node: sid, output: snarl.get_node(sid)?.proxy.as_ref()?.output_index(&w.from)? }
        }
        None => {
            let def = catalog.node(&graph.node(&w.from.node)?.kind)?;
            OutPinId { node: *ids.get(&w.from.node)?, output: def.outputs.iter().position(|p| p.name == w.from.port)? }
        }
    };
    let to = match vis.proxy_of(&w.to.node) {
        Some(g) => {
            let sid = *proxies.get(g)?;
            let n = snarl.get_node(sid)?;
            InPinId { node: sid, input: n.proxy.as_ref()?.input_index(&n.inputs, &w.to)? }
        }
        None => {
            let sid = *ids.get(&w.to.node)?;
            InPinId { node: sid, input: snarl.get_node(sid)?.inputs.iter().position(|r| r.key == w.to.port)? }
        }
    };
    (from.node != to.node).then_some((from, to))
}

/// What a node menu asks of the canvas itself (not the session).
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasRequest {
    Enter(GroupId),
    Leave,
}

/// What the node menu needs to know.
pub struct MenuCtx<'a> {
    pub graph: &'a Graph,
    pub catalog: &'a SchemaReply,
    /// The selected nodes and collapsed groups.
    pub selected: &'a [NodeId],
    pub selected_groups: &'a [GroupId],
}

/// The group items of the node menu of `node` (a proxy's menu is all group items).
pub fn node_menu(ui: &mut egui::Ui, cx: &MenuCtx<'_>, node: &CanvasNode, actions: &mut Vec<Action>) {
    if let Some(p) = &node.proxy {
        proxy_menu(ui, cx, p, actions);
        return;
    }
    let graph = cx.graph;
    // Group or collapse the selection (or just this node).
    let (nodes, groups): (Vec<NodeId>, Vec<GroupId>) =
        if cx.selected.contains(&node.id) { (cx.selected.to_vec(), cx.selected_groups.to_vec()) } else { (vec![node.id.clone()], Vec::new()) };
    let count = nodes.len() + groups.len();
    let what = if count == 1 { "this node".to_owned() } else { format!("{count} selected") };
    ui.separator();
    if ui.button(format!("Group {what}")).on_hover_text("Make a group of them (rename it from its header)").clicked() {
        actions.push(("group.create", json!({"nodes": nodes, "groups": groups})));
        ui.close();
    }
    if ui.button(format!("Collapse {what} into a group")).on_hover_text("Group them and show the group as one node").clicked() {
        actions.push(("group.create", json!({"nodes": nodes, "groups": groups, "collapsed": true})));
        ui.close();
    }
    // Promote and outputs, for each group this node is in, named for the group.
    let def = cx.catalog.node(&node.kind);
    for gid in graph.enclosing_groups(&node.id) {
        let Some(grp) = graph.group(&gid) else { continue };
        ui.separator();
        ui.menu_button(format!("Promote input to {}", grp.name), |ui| {
            let labels: BTreeSet<String> = grp.promoted.iter().map(|p| p.label.clone()).collect();
            for row in &node.inputs {
                let at = PortRef::new(node.id.clone(), row.key.clone());
                match grp.promoted.iter().find(|p| p.port == at) {
                    Some(p) => {
                        if ui.button(format!("[x] {} (as {}): unpromote", row.label, p.label)).clicked() {
                            actions.push(("group.unpromote", json!({"group": gid, "label": p.label})));
                            ui.close();
                        }
                    }
                    None => {
                        let label = unique_name(row.key.rsplit('.').next().unwrap_or(&row.key), &labels);
                        let doc = row.port.tooltip.clone().unwrap_or_default();
                        if ui.button(&row.label).on_hover_text(format!("Show it on {} as {label}", grp.name)).clicked() {
                            actions.push(("group.promote", json!({"group": gid, "port": at, "label": label, "doc": doc})));
                            ui.close();
                        }
                    }
                }
            }
        });
        if let Some(def) = def {
            ui.menu_button(format!("Output of {}", grp.name), |ui| {
                let names: BTreeSet<String> = grp.outputs.iter().map(|o| o.name.clone()).collect();
                for out in &def.outputs {
                    let from = PortRef::new(node.id.clone(), out.name.clone());
                    match grp.outputs.iter().find(|o| o.from == from) {
                        Some(o) => {
                            if ui.button(format!("[x] {} (as {}): remove", port_label(out), o.name)).clicked() {
                                actions.push(("group.removeOutput", json!({"group": gid, "name": o.name})));
                                ui.close();
                            }
                        }
                        None => {
                            let name = unique_name(&out.name, &names);
                            if ui.button(port_label(out)).on_hover_text(format!("Show it as {}'s output {name}", grp.name)).clicked() {
                                actions.push(("group.output", json!({"group": gid, "from": from, "name": name})));
                                ui.close();
                            }
                        }
                    }
                }
            });
        }
        mark_item(ui, cx, &gid, grp, &node.id, def, actions);
        parameters_menu(ui, cx, &gid);
        ui.menu_button(format!("Group {}", grp.name), |ui| group_items(ui, &gid, &grp.name, actions));
    }
}

/// What the catalogue calls an export mark.
fn mark_word<'a>(def: &'a plumcraft_proto::catalogue::NodeDef, catalog: &'a SchemaReply) -> &'a str {
    Some(def.export.mark.as_str()).filter(|m| !m.is_empty()).or(catalog.text(texts::EXPORT_MARK)).unwrap_or("export mark")
}

/// The export mark on `node` in group `gid`. Only for node types that take one, or one a file
/// marked: an engine with no export shows nothing.
fn mark_item(ui: &mut egui::Ui, cx: &MenuCtx<'_>, gid: &GroupId, grp: &Group, node: &NodeId, def: Option<&NodeDef>, actions: &mut Vec<Action>) {
    let on = grp.checkpoints.contains(node);
    let Some(def) = def.filter(|d| d.export.markable || on) else { return };
    let word = mark_word(def, cx.catalog);
    let word = word.chars().next().map_or_else(String::new, |c| c.to_uppercase().chain(word.chars().skip(1)).collect());
    let label = if on { format!("[x] {word} in {}", grp.name) } else { format!("{word} in {}", grp.name) };
    if ui.button(label).on_hover_text("Marks this step for the group's export; a run ignores it").clicked() {
        actions.push(("group.checkpoint", json!({"group": gid, "node": node, "on": !on})));
        ui.close();
    }
}

/// A group's menu items: rename and ungroup. Collapse, expand and enter are its header's
/// buttons, not menu items (the research software's maintainer, T02).
fn group_items(ui: &mut egui::Ui, gid: &GroupId, name: &str, actions: &mut Vec<Action>) {
    if ui.button("Rename…").on_hover_text("Rename this group").clicked() {
        request_rename(ui.ctx(), gid, name);
        ui.close();
    }
    if ui.button("Ungroup").on_hover_text("Dissolve the group; its nodes stay").clicked() {
        actions.push(("group.ungroup", json!({"group": gid})));
        ui.close();
    }
}

fn rename_window_id() -> egui::Id {
    egui::Id::new("plumcraft-group-rename")
}

/// Open the rename window for group `gid` (a text field can't live in a menu).
pub fn request_rename(ctx: &egui::Context, gid: &GroupId, current: &str) {
    ctx.data_mut(|d| d.insert_temp(rename_window_id(), (gid.clone(), current.to_owned())));
}

/// The rename window, when one was asked for. Returns the command it sends; it closes once
/// it is sent, or on Esc or its ×.
pub fn rename_window(ctx: &egui::Context, graph: &Graph) -> Vec<Action> {
    let mut actions = Vec::new();
    let Some((gid, mut text)) = ctx.data(|d| d.get_temp::<(GroupId, String)>(rename_window_id())) else { return actions };
    let Some(grp) = graph.group(&gid) else {
        ctx.data_mut(|d| d.remove::<(GroupId, String)>(rename_window_id()));
        return actions;
    };
    let mut open = true;
    let mut done = false;
    egui::Window::new(format!("Rename group {}", grp.name))
        .id(rename_window_id().with("window"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(240.0));
            if !r.has_focus() && !r.lost_focus() && text == grp.name {
                r.request_focus();
            }
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let valid = plumcraft_graph::is_name(text.trim());
            ui.horizontal(|ui| {
                let apply = ui.add_enabled(valid, egui::Button::new("Rename")).on_disabled_hover_text("A name is 1 to 200 characters").clicked();
                if (apply || (enter && valid)) && text.trim() != grp.name {
                    actions.push(("group.rename", json!({"group": gid, "name": text.trim()})));
                }
                if apply || enter || ui.button("Cancel").clicked() {
                    done = true;
                }
            });
        });
    if done || !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|d| d.remove::<(GroupId, String)>(rename_window_id()));
    } else {
        ctx.data_mut(|d| d.insert_temp(rename_window_id(), (gid, text)));
    }
    actions
}

fn parameter_window_id() -> egui::Id {
    egui::Id::new("plumcraft-parameter-window")
}

/// "Parameters of <group> ›": one item per promoted parameter, opening its editor in a window
/// ([`parameter_window`]; a text field can't live in a menu, which closes on any click in it).
fn parameters_menu(ui: &mut egui::Ui, cx: &MenuCtx<'_>, gid: &GroupId) {
    let Some(grp) = cx.graph.group(gid) else { return };
    if grp.promoted.is_empty() {
        return;
    }
    ui.menu_button(format!("Parameters of {}", grp.name), |ui| {
        for pr in &grp.promoted {
            if ui.button(format!("{}…", pr.label)).on_hover_text("Rename, document, set its range or unpromote it").clicked() {
                ui.data_mut(|d| d.insert_temp(parameter_window_id(), (gid.clone(), pr.label.clone())));
                ui.close();
            }
        }
    });
}

/// The window for one promoted parameter, opened from "Parameters of <group> › <label>…".
/// Returns the commands it sends; it closes once one is sent, or on Esc or its ×.
pub fn parameter_window(ctx: &egui::Context, graph: &Graph, catalog: &SchemaReply) -> Vec<Action> {
    let mut actions = Vec::new();
    let Some((gid, label)) = ctx.data(|d| d.get_temp::<(GroupId, String)>(parameter_window_id())) else { return actions };
    let close = |ctx: &egui::Context| ctx.data_mut(|d| d.remove::<(GroupId, String)>(parameter_window_id()));
    // the parameter (or its group) went: nothing to edit
    let Some((grp, pr)) = graph.group(&gid).and_then(|g| g.promoted.iter().find(|p| p.label == label).map(|p| (g, p.clone()))) else {
        close(ctx);
        return actions;
    };
    let cx = MenuCtx { graph, catalog, selected: &[], selected_groups: &[] };
    let mut open = true;
    let mut done = false;
    egui::Window::new(format!("Parameter {} of {}", pr.label, grp.name))
        .id(parameter_window_id().with("window"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| done = parameter_editor(ui, &cx, &gid, &pr, &mut actions));
    if done || !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        for f in ["Name", "Doc", "Min", "Max"] {
            ctx.data_mut(|d| d.remove::<String>(parameter_key(&gid, &pr.label, f)));
        }
        close(ctx);
    }
    actions
}

fn parameter_key(gid: &GroupId, label: &str, field: &str) -> egui::Id {
    egui::Id::new(("plumcraft-param", gid.0.as_str(), label, field.to_owned()))
}

/// The fields of one promoted parameter. Typed values are kept in egui's temp data until
/// Apply (or Enter), which sends one `group.editPromoted`. True once it sent a command.
pub fn parameter_editor(ui: &mut egui::Ui, cx: &MenuCtx<'_>, gid: &GroupId, pr: &Promoted, actions: &mut Vec<Action>) -> bool {
    let mut done = false;
    let inner = cx.graph.node(&pr.port.node).and_then(|n| {
        let def = cx.catalog.node(&n.kind)?;
        plumcraft_graph::resolve_input(def, n, &pr.port.node, &pr.port.port).ok().map(|r| r.port.clone())
    });
    let numeric = inner.as_ref().is_some_and(|p| p.ty == scalars::FLOAT || p.ty == scalars::INT);
    let fmt = |v: Option<f64>| v.map(|v| v.to_string()).unwrap_or_default();
    ui.weak(format!("{} on {}", pr.port.port, node_title(cx.graph, cx.catalog, &pr.port.node)));
    let key = |field: &str| parameter_key(gid, &pr.label, field);
    let field = |ui: &mut egui::Ui, name: &str, current: String, hint: String| -> (String, bool) {
        let id = key(name);
        let mut text: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or(current);
        let r = ui.horizontal(|ui| {
            ui.add_sized([36.0, 18.0], egui::Label::new(name));
            ui.add(egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(160.0))
        });
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
        let enter = r.inner.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        (text, enter)
    };
    let (label, e1) = field(ui, "Name", pr.label.clone(), String::new());
    let (doc, e2) = field(ui, "Doc", pr.doc.clone(), "shown on hover".into());
    let (min, max, e3) = if numeric {
        let lo = fmt(inner.as_ref().and_then(|p| p.min));
        let hi = fmt(inner.as_ref().and_then(|p| p.max));
        let (min, a) = field(ui, "Min", fmt(pr.min), if lo.is_empty() { "none".into() } else { format!("{lo} (the port's)") });
        let (max, b) = field(ui, "Max", fmt(pr.max), if hi.is_empty() { "none".into() } else { format!("{hi} (the port's)") });
        (min, max, a || b)
    } else {
        (String::new(), String::new(), false)
    };
    // A blank bound clears it; anything else must parse as a number.
    let bound = |t: &str| -> Result<serde_json::Value, ()> {
        let t = t.trim();
        if t.is_empty() { Ok(serde_json::Value::Null) } else { t.parse::<f64>().ok().filter(|f| f.is_finite()).map(|f| json!(f)).ok_or(()) }
    };
    let (lo, hi) = (bound(&min), bound(&max));
    let valid = plumcraft_graph::is_name(label.trim()) && lo.is_ok() && hi.is_ok();
    ui.horizontal(|ui| {
        let apply = ui
            .add_enabled(valid, egui::Button::new("Apply"))
            .on_disabled_hover_text("The name must not be blank, and min and max numbers (or blank)")
            .clicked();
        if (apply || ((e1 || e2 || e3) && valid))
            && let (Ok(lo), Ok(hi)) = (&lo, &hi)
        {
            let mut params = json!({"group": gid, "label": pr.label, "newLabel": label.trim(), "doc": doc.trim()});
            if numeric {
                params["min"] = lo.clone();
                params["max"] = hi.clone();
            }
            actions.push(("group.editPromoted", params));
            for f in ["Name", "Doc", "Min", "Max"] {
                ui.data_mut(|d| d.remove::<String>(key(f)));
            }
            done = true;
        }
        if ui.button("Unpromote").on_hover_text(format!("{}", pr.port)).clicked() {
            actions.push(("group.unpromote", json!({"group": gid, "label": pr.label})));
            done = true;
        }
    });
    done
}

fn proxy_menu(ui: &mut egui::Ui, cx: &MenuCtx<'_>, p: &Proxy, actions: &mut Vec<Action>) {
    let Some(grp) = cx.graph.group(&p.group) else { return };
    ui.weak(format!("group {} ({}, {} nodes)", grp.name, p.group, cx.graph.group_members(&p.group).len()));
    if ui.button("Expand").on_hover_text("Show the group's nodes in place").clicked() {
        actions.push(("group.collapse", json!({"group": p.group, "collapsed": false})));
        ui.close();
    }
    group_items(ui, &p.group, &grp.name, actions);
    parameters_menu(ui, cx, &p.group);
    if !grp.outputs.is_empty() {
        ui.menu_button("Outputs", |ui| {
            for o in &grp.outputs {
                if ui.button(format!("Remove {}", o.name)).on_hover_text(format!("{}", o.from)).clicked() {
                    actions.push(("group.removeOutput", json!({"group": p.group, "name": o.name})));
                    ui.close();
                }
            }
        });
    }
    ui.separator();
    for o in &p.outputs {
        if ui.button(format!("Watch {}", o.label)).on_hover_text("Show this output in the Watched panel, and run").clicked() {
            actions.push(("run.watch", json!({"node": o.port.node, "port": o.port.port})));
            ui.close();
        }
    }
}

/// Small header badges on a member node: one of its groups' outputs, and its export mark, in the
/// catalogue's words.
pub fn header_badges(ui: &mut egui::Ui, graph: &Graph, catalog: &SchemaReply, node: &NodeId, tokens: &Tokens) {
    let mut outputs = Vec::new();
    let mut marked = None;
    for g in graph.enclosing_groups(node) {
        if let Some(grp) = graph.group(&g) {
            outputs.extend(grp.outputs.iter().filter(|o| &o.from.node == node).map(|o| format!("{} › {}", grp.name, o.name)));
            if marked.is_none() && grp.checkpoints.contains(node) {
                marked = Some(grp.name.clone());
            }
        }
    }
    if let Some(group) = marked {
        let word = catalog.text(texts::EXPORT_MARK).filter(|w| !w.is_empty()).unwrap_or("marked");
        let hover = catalog.text(texts::EXPORT_MARK_HOVER).filter(|h| !h.is_empty()).unwrap_or("marked in {group}").replace("{group}", &group);
        ui.label(egui::RichText::new(word).small().color(tokens.accent)).on_hover_text(hover);
    }
    if !outputs.is_empty() {
        ui.label(egui::RichText::new("output").small().color(tokens.accent)).on_hover_text(format!("group output: {}", outputs.join(", ")));
    }
}

/// The icon of a group: a collapsed group's node and an open group's header.
pub const GROUP_ICON: &str = "boxes";

/// The frame margin around an open group's members (graph units).
const FRAME_PAD: f32 = 14.0;
/// The height of an open group's header band, above its members (graph units).
const GROUP_HEADER: f32 = 28.0;
/// The size of a group header's icons (screen points).
const GROUP_ICON_SIZE: f32 = 15.0;
/// The icon of the button that enters a group.
pub const ENTER_ICON: &str = "pencil";
/// The width left clear for an open group's action buttons at its band's right (screen points
/// at zoom 1).
const GROUP_ACTIONS_WIDTH: f32 = 64.0;
/// The smallest band, in points on screen, that still gets its icons and title.
const MIN_HEADER: f32 = 10.0;

/// An open group's frame on the canvas, in graph coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupFrame {
    pub group: GroupId,
    pub rect: Rect,
}

/// Paint open groups' frames with snarl's background painter (graph coordinates, under the
/// nodes): a faint body, a header band and a border, in the group colour.
pub fn paint_group_frames(painter: &egui::Painter, frames: &[GroupFrame], tokens: &Tokens) {
    let radius = f32::from(tokens.radius) * 2.0;
    let tint = tokens.group;
    // outer groups first, so a nested group's frame shows over its parent's
    for f in frames.iter().rev() {
        let r = f.rect;
        painter.rect_filled(r, radius, tint.body.gamma_multiply(0.55));
        let band = Rect::from_min_size(r.min, Vec2::new(r.width(), GROUP_HEADER.min(r.height())));
        let corner = radius.clamp(0.0, 255.0) as u8;
        painter.rect_filled(band, egui::CornerRadius { nw: corner, ne: corner, sw: 0, se: 0 }, tint.header);
        painter.rect_stroke(r, radius, egui::Stroke::new(1.5, tint.header), egui::StrokeKind::Outside);
    }
}

fn enclosing_depth(graph: &Graph, gid: &GroupId) -> usize {
    let mut depth = 0usize;
    let mut at = graph.group(gid).and_then(|g| g.parent.clone());
    while let Some(p) = at {
        depth = depth.saturating_add(1);
        if depth > graph.groups.len() {
            break;
        }
        at = graph.group(&p).and_then(|g| g.parent.clone());
    }
    depth
}

impl Canvas {
    /// Show only group `g`'s nodes.
    pub fn enter_group(&mut self, g: GroupId) {
        self.focus = Some(g);
        self.invalidate();
        self.request_fit();
    }

    /// Show the whole graph again.
    pub fn leave_group(&mut self) {
        if self.focus.take().is_some() {
            self.invalidate();
            self.request_fit();
        }
    }

    pub(crate) fn apply_requests(&mut self, requests: Vec<CanvasRequest>) {
        for r in requests {
            match r {
                CanvasRequest::Enter(g) => self.enter_group(g),
                CanvasRequest::Leave => self.leave_group(),
            }
        }
    }

    /// Frames around open groups, and the breadcrumb of an entered group. Returns commands.
    pub(crate) fn group_overlay(&mut self, ctx: &egui::Context, graph: &Graph, tokens: &Tokens) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut requests = Vec::new();
        let rects: BTreeMap<NodeId, Rect> = self.node_rects_by_id().into_iter().collect();
        let t = self.to_global;
        // Deeper groups first, so the outer frame encloses the inner ones' headers.
        let mut open: Vec<(usize, GroupId, Rect)> = Vec::new();
        for (gid, grp) in &graph.groups {
            if grp.collapsed || self.focus.as_ref() == Some(gid) {
                continue;
            }
            // Its members, and the proxies of collapsed groups nested in it.
            let members = graph.group_members(gid);
            let proxies: Vec<NodeId> = graph.group_subtree(gid).iter().map(proxy_node_id).collect();
            let bb = members.iter().chain(&proxies).filter_map(|m| rects.get(m)).fold(Rect::NOTHING, |bb, r| bb.union(*r));
            if !bb.is_finite() || bb.width() <= 0.0 {
                continue;
            }
            open.push((enclosing_depth(graph, gid), gid.clone(), bb));
        }
        open.sort_by_key(|o| std::cmp::Reverse(o.0));
        let mut drawn: BTreeMap<GroupId, Rect> = BTreeMap::new();
        for (_, gid, bb) in &open {
            // Include nested frames already drawn.
            let mut r = *bb;
            for (child, cr) in &drawn {
                if graph.group(child).and_then(|c| c.parent.as_ref()) == Some(gid) {
                    r = r.union(*cr);
                }
            }
            let r = r.expand2(Vec2::new(FRAME_PAD, FRAME_PAD)).with_min_y(r.min.y - FRAME_PAD - GROUP_HEADER);
            drawn.insert(gid.clone(), r);
            let screen = Rect::from_min_max(t * r.min, t * r.max);
            let Some(grp) = graph.group(gid) else { continue };
            // The header's widgets scale with the band (as the nodes do); zoomed out until they
            // can't be read or clicked, the band is drawn bare.
            let zoom = t.scaling.min(1.6);
            if GROUP_HEADER * zoom < MIN_HEADER {
                continue;
            }
            let icon = GROUP_ICON_SIZE * zoom;
            let pad = 2.0 * zoom;
            let row = (GROUP_HEADER * zoom - icon - 2.0 * pad).max(0.0) / 2.0;
            let area = egui::Area::new(egui::Id::new(("plumcraft-group-header", gid.0.as_str())))
                .order(egui::Order::Middle)
                .fixed_pos(screen.min + Vec2::new(8.0, row));
            area.show(ctx, |ui| {
                ui.set_clip_rect(self.rect);
                // Dragging the band moves the group and everything in it (its buttons, added
                // after, sit on top; the actions at its right are left clear).
                let band = Rect::from_min_size(screen.min, Vec2::new(screen.width(), GROUP_HEADER * t.scaling));
                let band = band.with_max_x((band.max.x - GROUP_ACTIONS_WIDTH * zoom).max(band.min.x));
                let drag = ui.interact(band, egui::Id::new(("plumcraft-group-band", gid.0.as_str())), egui::Sense::drag());
                let label = format!("Move group {}", grp.name);
                drag.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
                if drag.dragged() && t.scaling > 0.0 {
                    let d = drag.drag_delta() / t.scaling;
                    if d != Vec2::ZERO {
                        actions.push(("group.move", json!({"group": gid, "pos": [grp.pos[0] + d.x, grp.pos[1] + d.y]})));
                    }
                }
                drag.on_hover_cursor(egui::CursorIcon::Grab);
                ui.spacing_mut().button_padding = Vec2::splat(pad);
                ui.spacing_mut().item_spacing *= zoom;
                ui.spacing_mut().interact_size *= zoom;
                ui.horizontal(|ui| {
                    let summary =
                        format!("{} ({} nodes, {} parameters, {} outputs)", grp.name, graph.group_members(gid).len(), grp.promoted.len(), grp.outputs.len());
                    let summary = if grp.checkpoints.is_empty() { summary } else { format!("{summary}, {} marked", grp.checkpoints.len()) };
                    // the chevron collapses it, as a node's does
                    if crate::icons::button(ui, "chevron-down", icon, tokens.text, "Collapse group").clicked() {
                        actions.push(("group.collapse", json!({"group": gid, "collapsed": true})));
                    }
                    crate::icons::icon(ui, GROUP_ICON, icon, tokens.text, "Group");
                    let name = egui::RichText::new(&grp.name).color(tokens.text).size(14.0 * zoom).strong();
                    let r = ui.add(egui::Label::new(name).selectable(false).sense(egui::Sense::click()));
                    let label = format!("group {}", grp.name);
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
                    if r.double_clicked() {
                        request_rename(ui.ctx(), gid, &grp.name);
                    }
                    r.on_hover_text(format!("{summary}\nDouble-click to rename; right-click for its menu"))
                        .context_menu(|ui| group_items(ui, gid, &grp.name, &mut actions));
                });
            });
            // its actions at the band's right
            let buttons = egui::Area::new(egui::Id::new(("plumcraft-group-actions", gid.0.as_str())))
                .order(egui::Order::Middle)
                .pivot(egui::Align2::RIGHT_TOP)
                .fixed_pos(Pos2::new(screen.max.x - 6.0, screen.min.y + row));
            buttons.show(ctx, |ui| {
                ui.set_clip_rect(self.rect);
                ui.spacing_mut().button_padding = Vec2::splat(pad);
                ui.spacing_mut().item_spacing *= zoom;
                ui.spacing_mut().interact_size *= zoom;
                ui.horizontal(|ui| {
                    // the pencil enters it, to edit only its nodes
                    if crate::icons::button(ui, ENTER_ICON, icon, tokens.text, "Enter group").clicked() {
                        requests.push(CanvasRequest::Enter(gid.clone()));
                    }
                    crate::icons::menu(ui, "ellipsis-vertical", icon, tokens.text, "Group actions", |ui| group_items(ui, gid, &grp.name, &mut actions));
                });
            });
        }
        self.group_frames = open.iter().filter_map(|(_, gid, _)| Some(GroupFrame { group: gid.clone(), rect: *drawn.get(gid)? })).collect();
        // The breadcrumb of an entered group.
        if let Some(f) = self.focus.clone() {
            if graph.group(&f).is_none() {
                requests.push(CanvasRequest::Leave);
            } else {
                let mut chain = Vec::new();
                let mut at = Some(f.clone());
                while let Some(g) = at {
                    if chain.contains(&g) {
                        break;
                    }
                    at = graph.group(&g).and_then(|grp| grp.parent.clone());
                    chain.push(g);
                }
                chain.reverse();
                egui::Area::new(egui::Id::new("plumcraft-group-breadcrumb"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(self.rect.min + Vec2::new(10.0, 8.0))
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.link("Graph").on_hover_text("Leave the group").clicked() {
                                    requests.push(CanvasRequest::Leave);
                                }
                                for g in &chain {
                                    ui.label("›");
                                    let name = graph.group(g).map_or_else(|| g.0.clone(), |grp| grp.name.clone());
                                    if g == &f {
                                        ui.strong(name);
                                    } else if ui.link(name).clicked() {
                                        requests.push(CanvasRequest::Enter(g.clone()));
                                    }
                                }
                                if ui.small_button("Leave group").clicked() {
                                    requests.push(CanvasRequest::Leave);
                                }
                            });
                        });
                    });
            }
        }
        self.apply_requests(requests);
        actions
    }
}

/// `ui.inspect`'s `groups`: the entered group, each group with what it holds, and each proxy
/// with its pins.
pub fn inspect(canvas: &Canvas, graph: &Graph) -> serde_json::Value {
    let proxies: Vec<serde_json::Value> = canvas
        .snarl
        .nodes_ids_data()
        .filter_map(|(_, n)| {
            let p = n.value.proxy.as_ref()?;
            Some(json!({
                "group": p.group,
                "inputs": n.value.inputs.iter().zip(&p.rows).map(|(r, node)| json!({"label": r.label, "node": node, "port": r.key})).collect::<Vec<_>>(),
                "outputs": p.outputs.iter().map(|o| json!({"label": o.label, "from": o.port})).collect::<Vec<_>>(),
            }))
        })
        .collect();
    let groups: Vec<serde_json::Value> = graph
        .groups
        .iter()
        .map(|(id, g)| {
            json!({"id": id, "name": g.name, "collapsed": g.collapsed, "parent": g.parent, "nodes": g.nodes, "pos": g.pos,
                   "promoted": g.promoted.iter().map(|p| json!({"label": p.label, "port": p.port, "doc": p.doc, "min": p.min, "max": p.max})).collect::<Vec<_>>(),
                   "outputs": g.outputs.iter().map(|o| json!({"name": o.name, "from": o.from})).collect::<Vec<_>>()})
        })
        .collect();
    json!({"focus": canvas.focus, "list": groups, "proxies": proxies, "selected": canvas.picked.groups()})
}

/// `ui.set {focus}`: enter a group (its id), or leave (null).
pub fn set_focus(canvas: &mut Canvas, graph: &Graph, v: &serde_json::Value) -> Result<(), String> {
    match v {
        serde_json::Value::Null => {
            canvas.leave_group();
            Ok(())
        }
        serde_json::Value::String(id) => {
            let gid = GroupId::from(id.as_str());
            if graph.group(&gid).is_none() {
                return Err(format!("no group {}", id.chars().take(64).collect::<String>()));
            }
            canvas.enter_group(gid);
            Ok(())
        }
        _ => Err("focus must be a group id or null".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> SchemaReply {
        plumcraft_text::engine().schema()
    }

    /// story -> [think -> swap] -> shout, with think's seconds promoted, collapsed.
    fn grouped() -> (Graph, SchemaReply, GroupId, [NodeId; 4]) {
        let c = catalog();
        let mut g = Graph::new();
        let story = g.add_node(c.node("text.literal").unwrap(), [0.0, 0.0]);
        let think = g.add_node(c.node("text.slow").unwrap(), [300.0, 0.0]);
        let swap = g.add_node(c.node("text.replace").unwrap(), [600.0, 0.0]);
        let shout = g.add_node(c.node("text.upper").unwrap(), [900.0, 0.0]);
        g.connect(&c, PortRef::new(story.clone(), "text"), PortRef::new(think.clone(), "text")).unwrap();
        g.connect(&c, PortRef::new(think.clone(), "text"), PortRef::new(swap.clone(), "text")).unwrap();
        g.connect(&c, PortRef::new(swap.clone(), "text"), PortRef::new(shout.clone(), "text")).unwrap();
        let gid = g.create_group("think it over", &[think.clone(), swap.clone()], &[]).unwrap();
        g.promote(&c, &gid, PortRef::new(think.clone(), "seconds"), "delay", "how long").unwrap();
        g.set_collapsed(&gid, true).unwrap();
        (g, c, gid, [story, think, swap, shout])
    }

    #[test]
    fn collapsed_groups_become_one_node_with_boundary_wires() {
        let (g, c, gid, [story, think, swap, shout]) = grouped();
        let mut canvas = Canvas::default();
        canvas.sync(1, &g, &c);
        let nodes: Vec<&CanvasNode> = canvas.snarl.nodes_ids_data().map(|(_, n)| &n.value).collect();
        assert_eq!(nodes.len(), 3, "story, the proxy, shout");
        let proxy = nodes.iter().find(|n| n.proxy.is_some()).unwrap();
        assert_eq!(proxy.id, proxy_node_id(&gid));
        let p = proxy.proxy.as_ref().unwrap();
        // the promoted widget first, then the input wired from outside
        assert_eq!(proxy.inputs[0].label, "delay");
        assert_eq!(proxy.inputs[0].port.tooltip.as_deref(), Some("how long"));
        assert_eq!(p.rows, [think.clone(), think.clone()]);
        assert_eq!(proxy.inputs[1].key, "text");
        // the output wired to the outside
        assert_eq!(p.outputs.iter().map(|o| o.port.clone()).collect::<Vec<_>>(), [PortRef::new(swap.clone(), "text")]);
        // two boundary wires drawn; the internal one (think -> swap) isn't
        assert_eq!(canvas.snarl.wires().count(), 2);
        let pin_ports: Vec<(PortRef, PortRef)> =
            canvas.snarl.wires().map(|(o, i)| (out_port(&canvas.snarl, &c, o).unwrap(), in_port(&canvas.snarl, i).unwrap())).collect();
        assert!(pin_ports.contains(&(PortRef::new(story, "text"), PortRef::new(think, "text"))));
        assert!(pin_ports.contains(&(PortRef::new(swap, "text"), PortRef::new(shout, "text"))));
    }

    /// A group with nothing wired still shows what it needs and makes: the required wire-only
    /// inputs nothing feeds and its loose outputs. Wiring one keeps its pin; naming an output
    /// hides the loose ones.
    #[test]
    fn a_collapsed_group_shows_its_open_inputs_and_loose_outputs() {
        let c = catalog();
        let mut g = Graph::new();
        let up = g.add_node(c.node("text.upper").unwrap(), [0.0, 0.0]);
        let words = g.add_node(c.node("text.words").unwrap(), [0.0, 200.0]);
        let gid = g.create_group("pair", &[up.clone(), words.clone()], &[]).unwrap();
        g.set_collapsed(&gid, true).unwrap();
        let (_, node) = proxy_node(&g, &c, &gid).unwrap();
        let p = node.proxy.as_ref().unwrap();
        assert_eq!(node.inputs.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["text", "text"]);
        assert_eq!(p.rows, [up.clone(), words.clone()]);
        assert_eq!(p.outputs.len(), 2);
        let lit = g.add_node(c.node("text.literal").unwrap(), [-400.0, 0.0]);
        g.connect(&c, PortRef::new(lit, "text"), PortRef::new(up.clone(), "text")).unwrap();
        let (_, node) = proxy_node(&g, &c, &gid).unwrap();
        assert_eq!(node.inputs.len(), 2, "wired, it keeps its one pin");
        g.add_group_output(&c, &gid, PortRef::new(words.clone(), "count"), "words").unwrap();
        let (_, node) = proxy_node(&g, &c, &gid).unwrap();
        let outs: Vec<&str> = node.proxy.as_ref().unwrap().outputs.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(outs, ["words"]);
    }

    #[test]
    fn entering_shows_only_members_and_opens_the_group() {
        let (g, c, gid, [_, think, swap, _]) = grouped();
        let mut canvas = Canvas::default();
        canvas.enter_group(gid.clone());
        canvas.sync(1, &g, &c);
        let ids: BTreeSet<NodeId> = canvas.snarl.nodes_ids_data().map(|(_, n)| n.value.id.clone()).collect();
        assert_eq!(ids, [think, swap].into());
        assert_eq!(canvas.snarl.wires().count(), 1, "only the wire inside");
        canvas.leave_group();
        canvas.sync(1, &g, &c);
        assert_eq!(canvas.snarl.nodes_ids_data().count(), 3);
    }

    #[test]
    fn nested_collapsed_groups_show_the_outermost() {
        let (mut g, _, gid, [story, _, _, shout]) = grouped();
        let outer = g.create_group("outer", std::slice::from_ref(&shout), std::slice::from_ref(&gid)).unwrap();
        g.set_collapsed(&outer, true).unwrap();
        let vis = Visibility::new(&g, None);
        assert_eq!(vis.proxies, std::slice::from_ref(&outer));
        assert!(vis.shows(&story));
        assert_eq!(vis.proxy_of(&shout), Some(&outer));
        // entering the outer group shows the inner one collapsed
        let vis = Visibility::new(&g, Some(&outer));
        assert_eq!(vis.proxies, [gid]);
        assert!(vis.shows(&shout) && vis.is_outside(&story));
    }

    #[test]
    fn unique_names() {
        let taken: BTreeSet<String> = ["text".to_owned(), "text 2".to_owned()].into();
        assert_eq!(unique_name("text", &taken), "text 3");
        assert_eq!(unique_name("find", &taken), "find");
    }
}
