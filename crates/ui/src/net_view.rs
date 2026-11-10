//! Net labels on the canvas (`plumcraft_graph::nets`), lifted from the research software
//! (`crates/ui-egui/src/net_view.rs`, 2026-10-31, `8480c03`): a wire drawn through a net is not given
//! to egui-snarl. Instead the canvas draws, as a schematic does:
//! - at each input, a tag naming the net, pointing into the pin on a short stub;
//! - at the driving output, one tag with the net's name and how many wires it feeds
//!   ("picture ×3").
//!
//! Tags take their wire's colour. **Gestures** on a tag:
//! - click selects the net: every tag of it is highlighted, with a faint line from the driver
//!   to each use (hovering shows the lines too); Esc or a click on empty canvas clears it;
//! - double-click on a use goes to the driver; on the driver, to each use in turn;
//! - drag moves the tag (`wire.labelAt` / `net.move`, not undo steps);
//! - drag it onto an input pin instead and let go: the pin is wired from the net, drawn
//!   through it (`net.connect`, type-checked as any wire; the pin is ringed while the tag is
//!   over it, in the highlight colour when it fits and the error colour when it doesn't);
//! - right-click: Go to Driver, Watch, Pop Out, Rename Net…, Draw as Wire.
//!
//! A plain wire's menu adds Label All Uses…, which opens the name editor and draws every wire
//! from that output through the net. (`wire.label`, one wire alone, stays a command.)
//!
//! **Groups.** A net local to a group is drawn while the group is open. A labelled wire with
//! an end in a collapsed group is drawn as an ordinary wire to the group's pin: the label means
//! nothing outside its group.

use egui::{Color32, Pos2, Rect, Vec2};
use egui_snarl::{InPinId, OutPinId};
use plumcraft_graph::{Graph, NetKey, PortRef};
use plumcraft_proto::catalogue::SchemaReply;
use serde_json::{Value, json};

use crate::canvas::{Action, Canvas};
use crate::run_view::PinKey;
use crate::theme::Tokens;

/// How far a tag sits from its pin when it hasn't been moved (graph units).
pub const STUB: i32 = 14;
/// How near a pin (screen points) a dragged tag's pointer drops on it.
pub const DROP_RADIUS: f32 = 12.0;

/// A wire drawn through a net: its pins in the snarl, its ports and its receiving tag.
#[derive(Debug, Clone, PartialEq)]
pub struct Labelled {
    pub out: OutPinId,
    pub inp: InPinId,
    pub from: PortRef,
    pub to: PortRef,
    pub net: NetKey,
    /// The tag's offset from the input pin.
    pub at: [i32; 2],
    pub slice: Option<String>,
    pub color: Color32,
}

/// A net's driving tag, at its output pin.
#[derive(Debug, Clone, PartialEq)]
pub struct Driver {
    pub out: OutPinId,
    pub from: PortRef,
    pub net: NetKey,
    pub at: [i32; 2],
    pub uses: usize,
    pub color: Color32,
}

/// Which tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Tag {
    /// The receiving tag of the wire into this input.
    Use(PortRef),
    /// The net's driving tag.
    Driver(NetKey),
}

/// A tag being dragged, and its offset now.
#[derive(Debug, Clone, PartialEq)]
pub struct TagDrag {
    pub tag: Tag,
    pub at: [i32; 2],
    /// The input pin under the pointer, and whether the net can feed it: let go there and the
    /// pin is wired from the net.
    pub over: Option<(PortRef, bool)>,
}

/// The menu on a tag.
#[derive(Debug, Clone, PartialEq)]
pub struct NetMenu {
    pub tag: Tag,
    pub net: NetKey,
    pub screen_pos: Pos2,
    pub fresh: bool,
}

/// What the name editor will do.
#[derive(Debug, Clone, PartialEq)]
pub enum NameFor {
    /// Draw every wire from this output through it.
    Output(PortRef),
    /// Rename this net.
    Rename(NetKey),
}

/// The name editor.
#[derive(Debug, Clone, PartialEq)]
pub struct NameEdit {
    pub what: NameFor,
    pub text: String,
    pub screen_pos: Pos2,
    pub focus: bool,
    pub fresh: bool,
}

impl NameEdit {
    pub fn new(what: NameFor, text: String, screen_pos: Pos2) -> Self {
        NameEdit { what, text, screen_pos, focus: true, fresh: true }
    }
}

/// The canvas's net label state.
#[derive(Debug, Clone, Default)]
pub struct NetState {
    pub(crate) labelled: Vec<Labelled>,
    pub(crate) drivers: Vec<Driver>,
    /// The selected net.
    pub selected: Option<NetKey>,
    /// The net whose tag the pointer was over last frame.
    pub(crate) hovered: Option<NetKey>,
    pub(crate) drag: Option<TagDrag>,
    pub menu: Option<NetMenu>,
    pub edit: Option<NameEdit>,
    /// Which use a driver's double-click goes to next.
    cycle: usize,
    /// Where each tag was drawn last frame (screen).
    pub(crate) rects: Vec<(Tag, Rect)>,
}

impl NetState {
    /// Rebuild from the graph: `labelled` as the snarl was built (the canvas's sync), each
    /// net's driver from its first drawn use.
    pub(crate) fn rebuild(&mut self, graph: &Graph, labelled: Vec<Labelled>) {
        let mut drivers: Vec<Driver> = Vec::new();
        for l in &labelled {
            match drivers.iter_mut().find(|d| d.net == l.net) {
                Some(d) => d.uses += 1,
                None => {
                    let at = graph.net(&l.net).and_then(|n| n.at).unwrap_or([STUB, 0]);
                    drivers.push(Driver { out: l.out, from: l.from.clone(), net: l.net.clone(), at, uses: 1, color: l.color });
                }
            }
        }
        if self.selected.as_ref().is_some_and(|s| graph.net(s).is_none()) {
            self.selected = None;
        }
        self.labelled = labelled;
        self.drivers = drivers;
    }

    fn shown_at(&self, tag: &Tag, at: [i32; 2]) -> [i32; 2] {
        match &self.drag {
            Some(d) if &d.tag == tag => d.at,
            _ => at,
        }
    }

    /// Is `net` drawn highlighted?
    fn lit(&self, net: &NetKey) -> bool {
        self.selected.as_ref() == Some(net) || self.hovered.as_ref() == Some(net)
    }

    /// The tag rect drawn last frame (tests).
    pub fn tag_rect(&self, tag: &Tag) -> Option<Rect> {
        self.rects.iter().find(|(t, _)| t == tag).map(|(_, r)| *r)
    }
}

/// The receiving tag's offset when it hasn't been moved.
pub fn default_use_at() -> [i32; 2] {
    [-STUB, 0]
}

fn plus(p: Pos2, at: [i32; 2]) -> Pos2 {
    p + Vec2::new(at[0] as f32, at[1] as f32)
}

/// What a name editor for a wire from `from` starts with: the output's net, else its port.
pub fn suggested_name(graph: &Graph, from: &PortRef) -> String {
    graph.net_of(from).map_or_else(|| from.port.clone(), |k| k.name)
}

/// A net in a command's params: `{name, group}`, with `extra` merged in.
fn key_params(net: &NetKey, extra: Value) -> Value {
    let mut p = json!({"name": net.name, "group": net.group});
    if let (Some(m), Value::Object(e)) = (p.as_object_mut(), extra) {
        m.extend(e);
    }
    p
}

/// How a net is named to the designer: its name, and the group it is local to.
fn describe(graph: &Graph, net: &NetKey) -> String {
    match net.group.as_ref().and_then(|g| graph.group(g)) {
        Some(g) => format!("{} (in group {})", net.name, g.name),
        None => net.name.clone(),
    }
}

/// The width of a tag showing `text`, graph units, as [`draw_tag`] draws it at zoom 1 (the
/// font is 11-point monospace). Tidy keeps room for tags with this estimate
/// (`plumcraft_session::layout_cmds::tag_width`); a test holds the two together.
pub fn drawn_tag_width(ctx: &egui::Context, text: &str) -> f32 {
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), egui::FontId::monospace(11.0), Color32::WHITE));
    galley.size().x + 10.0 + 7.0
}

/// A tag's outline: a box with a point on its right, `tip` at the point.
fn tag_shape(body: Rect, tip: f32) -> Vec<Pos2> {
    vec![body.left_top(), body.right_top(), Pos2::new(tip, body.center().y), body.right_bottom(), body.left_bottom()]
}

/// Draw one tag in an area at `anchor` (screen): a use's tag ends at the anchor (its point
/// toward the pin), a driver's starts there. Returns the response.
/// Tags scale with the canvas, as the nodes' text does (`zoom`: screen points per graph unit).
#[allow(clippy::too_many_arguments)]
fn draw_tag(
    ctx: &egui::Context,
    id: egui::Id,
    anchor: Pos2,
    ends_at_anchor: bool,
    text: &str,
    color: Color32,
    lit: bool,
    zoom: f32,
    tokens: &Tokens,
) -> egui::Response {
    let pivot = if ends_at_anchor { egui::Align2::RIGHT_CENTER } else { egui::Align2::LEFT_CENTER };
    let k = zoom.clamp(0.25, 2.0);
    egui::Area::new(id)
        .order(egui::Order::Middle)
        .constrain(false)
        .fixed_pos(anchor)
        .pivot(pivot)
        .show(ctx, |ui| {
            let font = egui::FontId::monospace(11.0 * k);
            let galley = ui.painter().layout_no_wrap(text.to_owned(), font, tokens.text);
            let point = 7.0 * k;
            let size = galley.size() + Vec2::new(10.0 * k + point, 4.0 * k);
            let (resp, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
            let r = resp.rect;
            let body = Rect::from_min_max(r.min, Pos2::new(r.max.x - point, r.max.y));
            let fill = if lit { tokens.highlight.gamma_multiply(0.35) } else { tokens.panel };
            let stroke = egui::Stroke::new(if lit { 2.0 } else { 1.5 }, if lit { tokens.highlight } else { color });
            painter.add(egui::Shape::convex_polygon(tag_shape(body, r.max.x), fill, stroke));
            painter.galley(body.left_center() + Vec2::new(5.0 * k, -galley.size().y / 2.0), galley, tokens.text);
            resp
        })
        .inner
}

impl Canvas {
    /// The stubs from tags to pins, and the faint lines of a lit net, in graph coordinates,
    /// for the background (under the nodes, as wires are).
    pub(crate) fn net_paths(&self) -> Vec<(Vec<Pos2>, Color32)> {
        let pins = self.pins.borrow();
        let mut out = Vec::new();
        for l in &self.nets.labelled {
            let Some(&pin) = pins.get(&PinKey::In(l.inp)) else { continue };
            let anchor = plus(pin, self.nets.shown_at(&Tag::Use(l.to.clone()), l.at));
            out.push((crate::wire_route::smooth_path(&[anchor, pin]), l.color));
            if self.nets.lit(&l.net)
                && let Some(d) = self.nets.drivers.iter().find(|d| d.net == l.net)
                && let Some(&src) = pins.get(&PinKey::Out(d.out))
            {
                let start = plus(src, self.nets.shown_at(&Tag::Driver(d.net.clone()), d.at));
                out.push((crate::wire_route::smooth_path(&[start, anchor]), l.color.gamma_multiply(0.3)));
            }
        }
        for d in &self.nets.drivers {
            let Some(&pin) = pins.get(&PinKey::Out(d.out)) else { continue };
            let anchor = plus(pin, self.nets.shown_at(&Tag::Driver(d.net.clone()), d.at));
            out.push((crate::wire_route::smooth_path(&[pin, anchor]), d.color));
        }
        out
    }

    /// Is the pointer on a net tag (last frame)? Wire clicks and the palette leave it alone.
    pub(crate) fn over_net_tag(&self, screen: Pos2) -> bool {
        self.nets.rects.iter().any(|(_, r)| r.contains(screen))
    }

    /// The snarl pin drawn for input `to`, if it is drawn.
    fn in_pin_of(&self, to: &PortRef) -> Option<InPinId> {
        let pins = self.pins.borrow();
        pins.keys().find_map(|k| match k {
            PinKey::In(pin) if crate::groups_view::in_port(&self.snarl, *pin).as_ref() == Some(to) => Some(*pin),
            _ => None,
        })
    }

    /// The input pin a tag of `net` dragged to `screen` would drop on, and whether the net can
    /// feed it: the nearest within [`DROP_RADIUS`], not the tag's own pin.
    fn drop_target(&self, graph: &Graph, catalog: &SchemaReply, net: &NetKey, tag: &Tag, screen: Pos2) -> Option<(PortRef, bool)> {
        let t = self.to_global;
        let pins = self.pins.borrow();
        let near = pins
            .iter()
            .filter_map(|(k, p)| match k {
                PinKey::In(pin) => Some((*pin, (t * *p).distance(screen))),
                PinKey::Out(_) => None,
            })
            .filter(|(_, d)| *d <= DROP_RADIUS)
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        let to = crate::groups_view::in_port(&self.snarl, near.0)?;
        if matches!(tag, Tag::Use(own) if own == &to) {
            return None;
        }
        let ok = graph.clone().connect_net(catalog, net, &to).is_ok();
        Some((to, ok))
    }

    /// Go to the next node a double-click on `tag` names.
    fn jump(&mut self, graph: &Graph, tag: &Tag) {
        match tag {
            Tag::Use(to) => {
                if let Some(w) = graph.wire_into(to) {
                    self.reveal(&w.from.node);
                }
            }
            Tag::Driver(net) => {
                let uses: Vec<&PortRef> = graph.net_wires(net).map(|w| &w.to).collect();
                if !uses.is_empty() {
                    let k = self.nets.cycle % uses.len();
                    self.nets.cycle = self.nets.cycle.wrapping_add(1);
                    let node = uses[k].node.clone();
                    self.reveal(&node);
                }
            }
        }
    }

    /// Net tags, their menu and the name editor. Returns the commands.
    pub(crate) fn net_overlay(&mut self, ctx: &egui::Context, graph: &Graph, catalog: &SchemaReply, tokens: &Tokens) -> Vec<Action> {
        let mut actions = Vec::new();
        let t = self.to_global;
        let pins = self.pins.borrow().clone();
        let mut rects = Vec::new();
        let mut hovered = None;
        let mut events: Vec<(Tag, NetKey, egui::Response)> = Vec::new();
        for l in &self.nets.labelled {
            let Some(&pin) = pins.get(&PinKey::In(l.inp)) else { continue };
            let tag = Tag::Use(l.to.clone());
            let anchor = t * plus(pin, self.nets.shown_at(&tag, l.at));
            if !self.rect.contains(anchor) {
                continue;
            }
            let text = match &l.slice {
                Some(s) => format!("{} {}", l.net.name, crate::slice_view::label_text(s)),
                None => l.net.name.clone(),
            };
            let id = egui::Id::new(("plumcraft-net-use", l.to.node.0.as_str(), l.to.port.as_str()));
            let resp = draw_tag(ctx, id, anchor, true, &text, l.color, self.nets.lit(&l.net), t.scaling, tokens);
            let resp = resp.on_hover_text(format!(
                "Net {}: from {}.\nClick to select the net; double-click to go to its driver; drag to move, or onto an input pin to wire it from the net; right-click for more.",
                describe(graph, &l.net),
                l.from
            ));
            events.push((tag, l.net.clone(), resp));
        }
        for d in &self.nets.drivers {
            let Some(&pin) = pins.get(&PinKey::Out(d.out)) else { continue };
            let tag = Tag::Driver(d.net.clone());
            let anchor = t * plus(pin, self.nets.shown_at(&tag, d.at));
            if !self.rect.contains(anchor) {
                continue;
            }
            let id = egui::Id::new(("plumcraft-net-driver", d.net.group.as_ref().map_or("", |g| g.0.as_str()), d.net.name.as_str()));
            let resp = draw_tag(ctx, id, anchor, false, &format!("{} ×{}", d.net.name, d.uses), d.color, self.nets.lit(&d.net), t.scaling, tokens);
            let resp = resp.on_hover_text(format!(
                "Net {}: {} feeds {} input{}.\nClick to select the net; double-click to visit each use; drag to move, or onto an input pin to wire it from the net; right-click for more.",
                describe(graph, &d.net),
                d.from,
                d.uses,
                if d.uses == 1 { "" } else { "s" }
            ));
            events.push((tag, d.net.clone(), resp));
        }
        for (tag, net, resp) in events {
            rects.push((tag.clone(), resp.rect));
            if resp.hovered() || resp.dragged() {
                hovered = Some(net.clone());
            }
            if resp.clicked() {
                self.nets.selected = if self.nets.selected.as_ref() == Some(&net) { None } else { Some(net.clone()) };
            }
            if resp.double_clicked() {
                self.jump(graph, &tag);
            }
            if resp.dragged() {
                let d = resp.drag_delta() / t.scaling;
                let start = match &tag {
                    Tag::Use(to) => self.nets.labelled.iter().find(|l| &l.to == to).map(|l| l.at),
                    Tag::Driver(n) => self.nets.drivers.iter().find(|x| &x.net == n).map(|x| x.at),
                };
                if let Some(start) = start {
                    let over = resp.interact_pointer_pos().and_then(|p| self.drop_target(graph, catalog, &net, &tag, p));
                    let drag = self.nets.drag.get_or_insert_with(|| TagDrag { tag: tag.clone(), at: start, over: None });
                    if drag.tag == tag {
                        let moved = plus(Pos2::ZERO, drag.at) + d;
                        drag.at = [moved.x.round() as i32, moved.y.round() as i32];
                        drag.over = over;
                    }
                }
            }
            if resp.drag_stopped()
                && let Some(drag) = self.nets.drag.take()
            {
                // let go on a pin: wire it from the net (the engine refuses one that doesn't
                // fit, as it refuses any wire), and the tag stays where it was
                actions.push(match (drag.over, drag.tag) {
                    (Some((to, _)), _) => ("net.connect", key_params(&net, json!({"to": to}))),
                    (None, Tag::Use(to)) => ("wire.labelAt", json!({"to": to, "at": drag.at})),
                    (None, Tag::Driver(key)) => ("net.move", key_params(&key, json!({"at": drag.at}))),
                });
            }
            if resp.secondary_clicked()
                && let Some(p) = resp.interact_pointer_pos()
            {
                self.nets.menu = Some(NetMenu { tag, net, screen_pos: p, fresh: true });
            }
        }
        self.nets.rects = rects;
        self.nets.hovered = hovered;
        // the pin a dragged tag would drop on
        if let Some((to, ok)) = self.nets.drag.as_ref().and_then(|d| d.over.clone())
            && let Some(pin) = self.in_pin_of(&to)
            && let Some(&at) = pins.get(&PinKey::In(pin))
        {
            let color = if ok { tokens.highlight } else { tokens.error };
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("plumcraft-net-drop")));
            painter.circle_stroke(t * at, 9.0, egui::Stroke::new(2.5, color));
        }

        // Esc, or a click on empty canvas, clears the selection.
        let click = ctx.input(|i| if i.pointer.primary_clicked() { i.pointer.interact_pos() } else { None });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.nets.menu.is_none() && self.nets.edit.is_none() {
            self.nets.selected = None;
        }
        if let Some(p) = click
            && self.rect.contains(p)
            && !self.over_node(p)
            && !self.over_net_tag(p)
            && ctx.layer_id_at(p).is_none_or(|l| l.order == egui::Order::Background)
        {
            self.nets.selected = None;
        }

        actions.extend(self.net_menu(ctx, graph));
        actions.extend(self.name_editor(ctx, tokens));
        actions
    }

    fn net_menu(&mut self, ctx: &egui::Context, graph: &Graph) -> Vec<Action> {
        let mut actions = Vec::new();
        let Some(menu) = self.nets.menu.clone() else { return actions };
        let mut close = false;
        let mut jump = false;
        let area = egui::Area::new(egui::Id::new("plumcraft-net-menu")).order(egui::Order::Foreground).fixed_pos(menu.screen_pos).show(ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                let from = graph.net(&menu.net).map(|n| n.from.to_string()).unwrap_or_default();
                ui.weak(format!("Net {}, from {from}", describe(graph, &menu.net)));
                let go = if matches!(menu.tag, Tag::Use(_)) { "Go to Driver" } else { "Go to Next Use" };
                if ui.button(go).clicked() {
                    jump = true;
                    close = true;
                }
                if let Some(net) = graph.net(&menu.net) {
                    if ui.button("Watch").on_hover_text("Show this net's value in the Watched panel").clicked() {
                        actions.push(("run.watch", json!({"node": net.from.node, "port": net.from.port})));
                        close = true;
                    }
                    if ui.button("Pop Out").on_hover_text("Show this net's value in a window of its own, updated by every run").clicked() {
                        actions.push(("view.popout", json!({"node": net.from.node, "port": net.from.port})));
                        close = true;
                    }
                }
                if ui.button("Select Net").on_hover_text("Highlight every label of this net").clicked() {
                    self.nets.selected = Some(menu.net.clone());
                    close = true;
                }
                if ui.button("Rename Net…").clicked() {
                    self.nets.edit = Some(NameEdit::new(NameFor::Rename(menu.net.clone()), menu.net.name.clone(), menu.screen_pos));
                    close = true;
                }
                if let Tag::Use(to) = &menu.tag
                    && ui.button("Draw as Wire").on_hover_text("Draw this connection as a wire again").clicked()
                {
                    actions.push(("wire.unlabel", json!({"to": to})));
                    close = true;
                }
                if !matches!(menu.tag, Tag::Use(_)) || graph.net_wires(&menu.net).count() > 1 {
                    let net = menu.net.clone();
                    if ui.button("Draw All as Wires").on_hover_text("Draw every connection of this net as a wire again").clicked() {
                        for w in graph.net_wires(&net) {
                            actions.push(("wire.unlabel", json!({"to": w.to})));
                        }
                        close = true;
                    }
                }
            });
        });
        if jump {
            self.jump(graph, &menu.tag);
        }
        let outside = ctx.input(|i| i.pointer.any_click()) && !area.response.hovered() && !menu.fresh;
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if let Some(m) = &mut self.nets.menu {
            m.fresh = false;
        }
        if close || outside || escape {
            self.nets.menu = None;
        }
        actions
    }

    fn name_editor(&mut self, ctx: &egui::Context, tokens: &Tokens) -> Vec<Action> {
        let mut actions = Vec::new();
        let Some(edit) = &mut self.nets.edit else { return actions };
        let checked = plumcraft_graph::check_net_name(&edit.text);
        let mut close = false;
        let area = egui::Area::new(egui::Id::new("plumcraft-net-name")).order(egui::Order::Foreground).fixed_pos(edit.screen_pos).show(ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_max_width(300.0);
                ui.weak(match &edit.what {
                    NameFor::Output(from) => format!("Label every wire from {from}"),
                    NameFor::Rename(net) => format!("Rename net {}", net.name),
                });
                let r = ui.add(egui::TextEdit::singleline(&mut edit.text).hint_text("net name").desired_width(180.0).font(egui::TextStyle::Monospace));
                if edit.focus {
                    r.request_focus();
                    edit.focus = false;
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if let Err(e) = &checked {
                    ui.colored_label(tokens.error, e.to_string());
                }
                let mut ok = false;
                ui.horizontal(|ui| {
                    ok = ui.add_enabled(checked.is_ok(), egui::Button::new("OK")).clicked();
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
                if let (true, Ok(name)) = (ok || enter, &checked) {
                    actions.push(match &edit.what {
                        NameFor::Output(from) => ("output.label", json!({"from": from, "name": name})),
                        NameFor::Rename(old) => ("net.rename", key_params(old, json!({"to": name}))),
                    });
                    close = true;
                }
            });
        });
        let outside = ctx.input(|i| i.pointer.any_click()) && !area.response.hovered() && !edit.fresh;
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        edit.fresh = false;
        if close || outside || escape {
            self.nets.edit = None;
        }
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidy_keeps_as_much_room_as_a_tag_takes() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let mut widths = Vec::new();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx();
            widths.clear();
            for text in ["rgb", "picture", "picture ×3", "picture [1:-1]", &"p".repeat(plumcraft_graph::MAX_NET_CHARS)] {
                widths.push((text.to_owned(), drawn_tag_width(ctx, text), plumcraft_session::layout_cmds::tag_width(text)));
            }
        });
        out.textures_delta.clear();
        assert_eq!(widths.len(), 5);
        for (text, drawn, estimate) in widths {
            assert!((drawn - estimate).abs() <= 0.05 * drawn, "{text:?}: drawn {drawn}, Tidy keeps {estimate}");
        }
    }

    #[test]
    fn a_tag_points_right_from_its_box() {
        let body = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(40.0, 10.0));
        let s = tag_shape(body, 47.0);
        assert_eq!(s.len(), 5);
        assert_eq!(s[2], Pos2::new(47.0, 5.0));
    }
}
