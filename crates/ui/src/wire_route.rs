//! Wires with bend points (`Wire.via`): drawn as a smooth curve through them, and edited on
//! the canvas. Lifted from the research software (`crates/ui-egui/src/wire_route.rs`).
//!
//! egui-snarl draws a wire from pin to pin and can't route one, so a wire with bend points
//! isn't given to it. The canvas draws it instead, under the nodes (through snarl's
//! background painter), and hit-tests it for clicks, hovers and the wire menu.
//!
//! **The curve:** a Catmull-Rom spline through the output pin, the bend points and the input
//! pin, drawn as cubic Béziers. It leaves the output and enters the input horizontally, as
//! snarl's wires do.
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

use crate::canvas::{Action, Canvas};
use crate::run_view::PinKey;

/// Samples per curve segment.
const STEPS: usize = 24;
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

/// The smooth curve through `points` (pin, bend points, pin), sampled: Catmull-Rom tangents
/// inside, horizontal ones at the two pins.
pub fn smooth_path(points: &[Pos2]) -> Vec<Pos2> {
    let n = points.len();
    if n < 2 {
        return points.to_vec();
    }
    let at = |i: usize| points.get(i).copied().unwrap_or(Pos2::ZERO);
    let tangent = |i: usize| -> Vec2 {
        if i == 0 {
            Vec2::new((at(1) - at(0)).length(), 0.0)
        } else if i == n - 1 {
            Vec2::new((at(n - 1) - at(n - 2)).length(), 0.0)
        } else {
            (at(i + 1) - at(i - 1)) * 0.5
        }
    };
    let mut out = Vec::with_capacity((n - 1) * STEPS + 1);
    out.push(at(0));
    for i in 0..n - 1 {
        let (p0, p3) = (at(i), at(i + 1));
        let (p1, p2) = (p0 + tangent(i) / 3.0, p3 - tangent(i + 1) / 3.0);
        for k in 1..=STEPS {
            let t = k as f32 / STEPS as f32;
            let u = 1.0 - t;
            let p = p0.to_vec2() * (u * u * u) + p1.to_vec2() * (3.0 * u * u * t) + p2.to_vec2() * (3.0 * u * t * t) + p3.to_vec2() * (t * t * t);
            out.push(p.to_pos2());
        }
    }
    out
}

/// The distance from `p` to a sampled path.
pub fn path_distance(path: &[Pos2], p: Pos2) -> f32 {
    path.windows(2)
        .filter_map(|w| match w {
            [a, b] => {
                let ab = *b - *a;
                let t = if ab.length_sq() > 0.0 { ((p - *a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
                Some((*a + ab * t).distance(p))
            }
            _ => None,
        })
        .fold(f32::INFINITY, f32::min)
}

/// Where a new bend point at `q` goes among `via` (wire from `a` to `b`): before the first
/// control point after the closest stretch of the curve.
pub fn insert_index(a: Pos2, via: &[[i32; 2]], b: Pos2, q: Pos2) -> usize {
    let mut points = vec![a];
    points.extend(via.iter().map(as_pos));
    points.push(b);
    let path = smooth_path(&points);
    // the closest sample, then which segment (STEPS samples each) it lies on
    let (best, _) = path.iter().enumerate().map(|(i, s)| (i, s.distance(q))).fold((0, f32::INFINITY), |acc, x| if x.1 < acc.1 { x } else { acc });
    (best.saturating_sub(1) / STEPS).min(via.len())
}

fn as_pos(v: &[i32; 2]) -> Pos2 {
    Pos2::new(v[0] as f32, v[1] as f32)
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

    /// The routed wire under a screen position (within a few screen points), if any.
    pub fn routed_at(&self, screen: Pos2) -> Option<(PortRef, PortRef)> {
        let scale = self.to_global.scaling.max(0.05);
        let p = self.to_global.inverse() * screen;
        self.routed_paths()
            .into_iter()
            .map(|(i, path, _)| (i, path_distance(&path, p) * scale))
            .filter(|(_, d)| *d <= crate::canvas::WIRE_HIT)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .and_then(|(i, _)| self.routed.get(i).map(|r| (r.from.clone(), r.to.clone())))
    }

    /// Routed wires' midpoints on screen, with their ports.
    pub(crate) fn routed_midpoints(&self) -> Vec<(Pos2, PortRef, PortRef)> {
        self.routed_paths()
            .into_iter()
            .filter_map(|(i, path, _)| {
                let r = self.routed.get(i)?;
                Some((self.to_global * *path.get(path.len() / 2)?, r.from.clone(), r.to.clone()))
            })
            .collect()
    }

    /// The point `t` (0 to 1) of the way along the bent wire into `to`, on screen (tests and
    /// the control channel click it).
    pub fn routed_point(&self, to: &PortRef, t: f32) -> Option<Pos2> {
        let (_, path, _) = self.routed_paths().into_iter().find(|(i, ..)| self.routed.get(*i).is_some_and(|r| &r.to == to))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_passes_through_its_points_and_leaves_pins_level() {
        let pts = [Pos2::new(0.0, 0.0), Pos2::new(100.0, 80.0), Pos2::new(200.0, 0.0)];
        let path = smooth_path(&pts);
        assert_eq!(path.len(), 2 * STEPS + 1);
        assert_eq!(path[0], pts[0]);
        assert!((path[STEPS] - pts[1]).length() < 1e-3, "through the bend point");
        assert!((path[2 * STEPS] - pts[2]).length() < 1e-3);
        // leaving the output: heading right, level at first
        let first = path[1] - path[0];
        assert!(first.x > 0.0 && first.y.abs() < first.x * 0.6, "{first:?}");
        // entering the input from the left
        let last = path[2 * STEPS] - path[2 * STEPS - 1];
        assert!(last.x > 0.0 && last.y.abs() < last.x * 0.6, "{last:?}");
        assert!(path_distance(&path, Pos2::new(100.0, 80.0)) < 1e-3);
        assert!(path_distance(&path, Pos2::new(100.0, 300.0)) > 100.0);
        assert_eq!(smooth_path(&pts[..1]).len(), 1);
        assert_eq!(path_distance(&[], Pos2::ZERO), f32::INFINITY);
    }

    #[test]
    fn a_new_bend_point_goes_where_it_was_clicked() {
        let (a, b) = (Pos2::new(0.0, 0.0), Pos2::new(400.0, 0.0));
        let via = [[100, 100], [300, 100]];
        assert_eq!(insert_index(a, &via, b, Pos2::new(40.0, 50.0)), 0, "before the first");
        assert_eq!(insert_index(a, &via, b, Pos2::new(200.0, 110.0)), 1, "between them");
        assert_eq!(insert_index(a, &via, b, Pos2::new(370.0, 40.0)), 2, "after the last");
        assert_eq!(insert_index(a, &[], b, Pos2::new(200.0, 0.0)), 0);
    }
}
