//! Wires with bend points (`Wire.via`): drawn as a smooth curve through them, and edited on
//! the canvas. Lifted from the research software (`crates/ui-egui/src/wire_route.rs`).
//!
//! egui-snarl draws a wire from pin to pin and can't route one, so a wire with bend points
//! isn't given to it. The canvas draws it instead, under the nodes (through snarl's
//! background painter), and hit-tests it for clicks, hovers and the wire menu.
//!
//! **The curve:** a Catmull-Rom spline through the output pin, the bend points and the input
//! pin, drawn as cubic Béziers. It leaves the output and enters the input horizontally, as
//! snarl's wires do. Its geometry is the core's ([`plumcraft_editor_core::wire_path`], stage N1).
//!
//! **Editing:**
//! - right-click a wire › Add Bend Point (at the click);
//! - drag a bend point to move it;
//! - double-click one to remove it;
//! - right-click › Straighten removes them all.
//!
//! Each edit is one `wire.route`: one undo step, and nothing goes stale.

use egui::{Color32, Pos2, Vec2};
use egui_snarl::{InPinId, OutPinId};
use plumcraft_graph::{Graph, PortRef};
use plumcraft_proto::catalogue::SchemaReply;
use serde_json::json;

use plumcraft_editor_core::wire_path;

use crate::canvas::{Action, Canvas};
use crate::plain::{ToCore, ToEgui};
use crate::run_view::PinKey;
/// A bend point's handle radius, in screen points.
pub const HANDLE: f32 = 5.0;

/// A wire the canvas draws itself: its pins in the snarl, its ports, its bend points, its colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Routed {
    pub out: OutPinId,
    pub inp: InPinId,
    pub from: PortRef,
    pub to: PortRef,
    pub via: Vec<[i32; 2]>,
    pub color: Color32,
}

/// A bend point being dragged: the wire, the point, and the bend points as they are now.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteDrag {
    pub to: PortRef,
    pub index: usize,
    pub via: Vec<[i32; 2]>,
}

/// The colour of a wire from `from`: its output's wire type, as snarl colours pins.
pub fn wire_color(graph: &Graph, catalog: &SchemaReply, from: &PortRef, fallback: Color32) -> Color32 {
    graph
        .node(&from.node)
        .and_then(|n| catalog.node(&n.kind))
        .and_then(|d| d.output(&from.port))
        .and_then(|p| catalog.wire_type(&p.ty))
        .and_then(|w| w.color.as_deref())
        .and_then(crate::theme::parse_hex)
        .unwrap_or(fallback)
}

/// The smooth curve through `points` (pin, bend points, pin), sampled
/// ([`plumcraft_editor_core::wire_path::smooth_path`], in egui's points).
pub fn smooth_path(points: &[Pos2]) -> Vec<Pos2> {
    let core: Vec<_> = points.iter().map(|p| p.to_core()).collect();
    wire_path::smooth_path(&core).into_iter().map(ToEgui::to_egui).collect()
}

/// The distance from `p` to a sampled path.
pub fn path_distance(path: &[Pos2], p: Pos2) -> f32 {
    let core: Vec<_> = path.iter().map(|p| p.to_core()).collect();
    wire_path::path_distance(&core, p.to_core())
}

/// Where a new bend point at `q` goes among `via` (wire from `a` to `b`): before the first
/// control point after the closest stretch of the curve.
pub fn insert_index(a: Pos2, via: &[[i32; 2]], b: Pos2, q: Pos2) -> usize {
    wire_path::insert_index(a.to_core(), via, b.to_core(), q.to_core())
}

fn as_pos(v: &[i32; 2]) -> Pos2 {
    wire_path::as_point(v).to_egui()
}

impl Canvas {
    /// The bend points a routed wire shows: being dragged, or as the graph has them.
    fn shown_via<'a>(&'a self, r: &'a Routed) -> &'a [[i32; 2]] {
        match &self.route_drag {
            Some(d) if d.to == r.to => &d.via,
            _ => &r.via,
        }
    }

    /// Each routed wire's curve (graph coordinates) and colour, from where its pins were drawn.
    pub(crate) fn routed_paths(&self) -> Vec<(usize, Vec<Pos2>, Color32)> {
        let pins = self.pins.borrow();
        self.routed
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let (a, b) = (*pins.get(&PinKey::Out(r.out))?, *pins.get(&PinKey::In(r.inp))?);
                let mut points = vec![a];
                points.extend(self.shown_via(r).iter().map(as_pos));
                points.push(b);
                Some((i, smooth_path(&points), r.color))
            })
            .collect()
    }

    /// Every wire the canvas draws itself rather than snarl, with its ports, its curve (graph
    /// coordinates) and its colour: those with bend points, and those the core routes backward
    /// round the faces ([`crate::shown`], N4 slice 4).
    pub(crate) fn drawn_paths(&self) -> Vec<(PortRef, PortRef, Vec<Pos2>, Color32)> {
        let mut out: Vec<_> =
            self.routed_paths().into_iter().filter_map(|(i, path, color)| self.routed.get(i).map(|r| (r.from.clone(), r.to.clone(), path, color))).collect();
        out.extend(self.backward.iter().map(|b| (b.from.clone(), b.to.clone(), b.path.clone(), b.color)));
        out
    }

    /// The wire the canvas draws itself under a screen position (within a few screen points),
    /// if any: one with bend points, or one routed backward.
    pub fn routed_at(&self, screen: Pos2) -> Option<(PortRef, PortRef)> {
        let scale = self.to_global.scaling.max(0.05);
        let p = self.to_global.inverse() * screen;
        self.drawn_paths()
            .into_iter()
            .map(|(from, to, path, _)| (from, to, path_distance(&path, p) * scale))
            .filter(|(.., d)| *d <= crate::canvas::WIRE_HIT)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(from, to, _)| (from, to))
    }

    /// The midpoints on screen of the wires the canvas draws itself, with their ports.
    pub(crate) fn routed_midpoints(&self) -> Vec<(Pos2, PortRef, PortRef)> {
        self.drawn_paths().into_iter().filter_map(|(from, to, path, _)| Some((self.to_global * *path.get(path.len() / 2)?, from, to))).collect()
    }

    /// The point `t` (0 to 1) of the way along the wire into `to` the canvas draws itself (bent,
    /// or routed backward), on screen (tests and the control channel click it).
    pub fn routed_point(&self, to: &PortRef, t: f32) -> Option<Pos2> {
        let (.., path, _) = self.drawn_paths().into_iter().find(|(_, x, ..)| x == to)?;
        let k = ((path.len().saturating_sub(1)) as f32 * t.clamp(0.0, 1.0)).round() as usize;
        Some(self.to_global * *path.get(k)?)
    }

    /// Whether a screen position is on a bend point's handle (a click there is the handle's,
    /// not the wire's).
    pub fn on_bend_handle(&self, screen: Pos2) -> bool {
        self.bend_points().iter().any(|(_, _, p)| p.distance(screen) <= HANDLE + 3.0)
    }

    /// Each bend point's place on screen, by the wire's input (tests and the control channel).
    pub fn bend_points(&self) -> Vec<(PortRef, usize, Pos2)> {
        self.routed
            .iter()
            .flat_map(|r| self.shown_via(r).iter().enumerate().map(|(k, v)| (r.to.clone(), k, self.to_global * as_pos(v))).collect::<Vec<_>>())
            .collect()
    }

    /// The bend points the wire into `to` would have with one added at `screen`.
    pub(crate) fn via_with_point(&self, graph: &Graph, to: &PortRef, screen: Pos2) -> Option<Vec<[i32; 2]>> {
        let q = self.to_global.inverse() * screen;
        let new = [q.x.round() as i32, q.y.round() as i32];
        let via = graph.wire_into(to)?.via.clone();
        let mut out = via.clone();
        match self.routed.iter().find(|r| &r.to == to) {
            Some(r) => {
                let pins = self.pins.borrow();
                let (a, b) = (*pins.get(&PinKey::Out(r.out))?, *pins.get(&PinKey::In(r.inp))?);
                out.insert(insert_index(a, &via, b, q), new);
            }
            None => out.push(new),
        }
        Some(out)
    }

    /// Bend point handles: drag to move, double-click to remove. Also a right-click on a
    /// routed wire opens the wire menu. Returns the commands.
    pub(crate) fn route_overlay(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut actions = Vec::new();
        let t = self.to_global;
        let routed = self.routed.clone();
        for r in &routed {
            let via = self.shown_via(r).to_vec();
            for (k, v) in via.iter().enumerate() {
                let screen = t * as_pos(v);
                if !self.rect.contains(screen) {
                    continue;
                }
                let id = egui::Id::new(("plumcraft-bend", r.to.node.0.as_str(), r.to.port.as_str(), k));
                let area = egui::Area::new(id).order(egui::Order::Middle).constrain(false).fixed_pos(screen - Vec2::splat(HANDLE + 2.0));
                let resp = area
                    .show(ctx, |ui| {
                        let (resp, painter) = ui.allocate_painter(Vec2::splat(2.0 * HANDLE + 4.0), egui::Sense::click_and_drag());
                        let hot = resp.hovered() || resp.dragged();
                        painter.circle(
                            resp.rect.center(),
                            if hot { HANDLE + 1.5 } else { HANDLE },
                            r.color,
                            egui::Stroke::new(1.0, Color32::from_black_alpha(160)),
                        );
                        resp.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Drag to bend the wire; double-click to remove the point")
                    })
                    .inner;
                if resp.dragged() {
                    let d = resp.drag_delta() / t.scaling;
                    let drag = self.route_drag.get_or_insert_with(|| RouteDrag { to: r.to.clone(), index: k, via: via.clone() });
                    if let Some(p) = drag.via.get_mut(drag.index) {
                        let moved = as_pos(p) + d;
                        *p = [moved.x.round() as i32, moved.y.round() as i32];
                    }
                }
                if resp.drag_stopped()
                    && let Some(drag) = self.route_drag.take()
                {
                    actions.push(("wire.route", json!({"to": drag.to, "via": drag.via})));
                }
                // egui counts a click within 0.6 s of the one before last as a triple click and
                // then reports no double click: either removes the point.
                if resp.double_clicked() || resp.triple_clicked() {
                    let mut fewer = r.via.clone();
                    if k < fewer.len() {
                        fewer.remove(k);
                        actions.push(("wire.route", json!({"to": r.to, "via": fewer})));
                    }
                }
            }
        }
        // a right-click on a routed wire: the wire menu (snarl doesn't know these wires)
        if self.palette.is_none()
            && self.wire_menu.is_none()
            && let Some(p) = ctx.input(|i| if i.pointer.secondary_clicked() { i.pointer.interact_pos() } else { None })
            && self.rect.contains(p)
            && !self.over_node(p)
            && !self.over_net_tag(p)
            && let Some((from, to)) = self.routed_at(p)
        {
            self.wire_menu = Some(crate::canvas::WireMenu { screen_pos: p, from, to, fresh: true });
        }
        actions
    }
}
