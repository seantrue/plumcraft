//! What the core's scene shows that the graph doesn't (N4 slice 4): the canvas draws it as the
//! scene has it, so what egui draws is what the core hit-tests (`ui.gesture`'s taps, the
//! geometry snapshot's wires and faces).
//!
//! - **Faces pushed apart** (`scene::make_room`): a face's `pos` in the scene is where it is
//!   shown, which differs from its node's position when it was pushed out of another's way.
//!   egui-snarl keeps each node's position itself (`Node::pos`, public), so the canvas puts every
//!   node where the scene shows it, each frame, and remembers where it put it
//!   ([`Canvas::shown_at`]): a drag is what snarl moved a node from there, and a move names the
//!   shown place plus the drag ([`Canvas::drag_moves`]), as the core's own gestures do. A node
//!   being dragged is left where snarl has it.
//! - **Frames grown** round the members the push moved (`scene::grown_frame`): drawn, fitted and
//!   handled at the scene's size ([`crate::frame_view`]).
//! - **Wires routed backward** (`wire_path::backward_chain`): egui-snarl 0.12 draws every wire it
//!   holds itself, from pin to pin, as its own curve (`SnarlWidget::show` calls its private
//!   `draw_wire` for each of `Snarl::wires`; a viewer can choose only a pin's `wire_color` and
//!   `wire_style`, none of which is the core's route). So a wire that runs right to left is
//!   taken out of the snarl, as a wire with bend points is ([`crate::wire_route`]), and the canvas
//!   draws the scene's `path` under the nodes and hit-tests the same curve. Whether a wire runs
//!   backward changes as faces move without the graph changing (a drag, a push), so the canvas
//!   moves wires in and out of the snarl every frame, keeping [`Canvas::built_wires`] in step so
//!   that a wire it took out isn't read as one a ⌘-drag picked up.

use std::collections::{HashMap, HashSet};

use egui::{Color32, Pos2};
use egui_snarl::{InPinId, OutPinId};
use plumcraft_editor_core::plain::Point;
use plumcraft_editor_core::scene::{Face, Frame, Wire};
use plumcraft_editor_core::wire_path;
use plumcraft_graph::{NodeId, PortRef};

use crate::canvas::Canvas;
use crate::plain::ToEgui;

/// A wire between two faces' pins with neither bend points nor a net: snarl's to draw, unless it
/// runs backward.
#[derive(Debug, Clone, PartialEq)]
pub struct Plain {
    pub out: OutPinId,
    pub inp: InPinId,
    pub from: PortRef,
    pub to: PortRef,
    pub color: Color32,
}

/// A plain wire the canvas draws itself along the core's route round the faces: its ends, its
/// colour, and its path sampled (graph coordinates).
#[derive(Debug, Clone, PartialEq)]
pub struct Backward {
    pub from: PortRef,
    pub to: PortRef,
    pub color: Color32,
    pub path: Vec<Pos2>,
}

/// What the canvas follows of the core's scene this frame.
#[derive(Clone, Copy)]
pub struct SceneShown<'a> {
    /// The faces, by id, at the places they are shown.
    pub faces: &'a HashMap<String, Face>,
    /// The frames, at the sizes they are shown.
    pub frames: &'a [Frame],
    /// The wires, with their paths.
    pub wires: &'a [Wire],
}

/// Does a wire's path run right to left (its input left of its output)? The core routes such a
/// plain wire round both faces ([`wire_path::backward_chain`]).
pub fn runs_backward(path: &[[f64; 2]]) -> bool {
    match (path.first(), path.last()) {
        (Some(a), Some(b)) if path.len() > 1 => b[0] < a[0],
        _ => false,
    }
}

/// A scene path (a cubic chain) sampled as egui points.
pub fn sampled(path: &[[f64; 2]]) -> Vec<Pos2> {
    let chain: Vec<Point> = path.iter().map(|p| Point::new(p[0] as f32, p[1] as f32)).collect();
    wire_path::sample_chain(&chain).into_iter().map(ToEgui::to_egui).collect()
}

impl Canvas {
    /// Follow the scene before snarl draws: faces where they are shown, frames at their shown
    /// sizes, backward wires out of the snarl and along the core's route.
    pub(crate) fn follow_scene(&mut self, ctx: &egui::Context, shown: SceneShown<'_>) {
        let down = ctx.input(|i| i.pointer.any_down());
        // a node being dragged (and the selection with it) is where snarl has it
        let node_drag = down && self.drag_anchor.is_some();
        let moving: HashSet<NodeId> = match self.drag_anchor.as_ref().filter(|_| node_drag) {
            None => HashSet::new(),
            Some(anchor) => {
                let mut set: HashSet<NodeId> = HashSet::from([anchor.clone()]);
                let picked_groups = self.picked.groups().to_vec();
                let anchor_picked = self.picked.nodes().contains(anchor)
                    || self.snarl.nodes().any(|n| &n.id == anchor && n.proxy.as_ref().is_some_and(|p| picked_groups.contains(&p.group)));
                if anchor_picked {
                    set.extend(self.picked.nodes().iter().cloned());
                    set.extend(self.snarl.nodes().filter(|n| n.proxy.as_ref().is_some_and(|p| picked_groups.contains(&p.group))).map(|n| n.id.clone()));
                }
                set
            }
        };
        let ids: Vec<_> = self.snarl.node_ids().map(|(sid, n)| (sid, n.id.clone())).collect();
        for (sid, id) in ids {
            if moving.contains(&id) {
                continue;
            }
            let Some(face) = shown.faces.get(&id.0) else { continue };
            let target = Pos2::new(face.pos[0], face.pos[1]);
            let Some(info) = self.snarl.get_node_info_mut(sid) else { continue };
            // (a node snarl moved and the move not yet made is left for the move)
            let placed = self.shown_at.get(&sid).copied().unwrap_or(info.pos);
            if info.pos == placed && info.pos != target {
                info.pos = target;
            }
            if info.pos == target {
                self.shown_at.insert(sid, target);
            }
        }
        self.frame_sizes = shown.frames.iter().map(|f| (f.id.clone(), f.size)).collect();

        // wires: a plain wire that runs backward is the canvas's; it changes over only while no
        // button is down or a node is dragged (not while a ⌘-drag carries a wire)
        let can_change = !down || node_drag;
        let paths: HashMap<&PortRef, &Vec<[f64; 2]>> = shown.wires.iter().filter(|w| w.kind == "plain").map(|w| (&w.to, &w.path)).collect();
        let mut backward = Vec::new();
        for p in self.plain.clone() {
            let path = paths.get(&p.to).copied();
            let was = self.backward_out.contains(&p.to);
            let now = if can_change { path.is_some_and(|x| runs_backward(x)) } else { was };
            if now && !was {
                if self.snarl.disconnect(p.out, p.inp) {
                    self.built_wires.retain(|w| !(w.0 == p.from && w.1 == p.to));
                }
                self.backward_out.insert(p.to.clone());
            } else if !now && was {
                self.snarl.connect(p.out, p.inp);
                if !self.built_wires.iter().any(|w| w.0 == p.from && w.1 == p.to) {
                    self.built_wires.push((p.from.clone(), p.to.clone()));
                }
                self.backward_out.remove(&p.to);
            }
            if now {
                backward.push(Backward { from: p.from, to: p.to, color: p.color, path: path.map(|x| sampled(x)).unwrap_or_default() });
            }
        }
        self.backward = backward;
    }

    /// Where the canvas put a node last (the scene's place for its face), by snarl id: what a
    /// drag moved it from.
    pub(crate) fn placed_at(&self, sid: egui_snarl::NodeId) -> Option<Pos2> {
        self.shown_at.get(&sid).copied()
    }

    /// The wires the canvas draws along the core's route round the faces (tests).
    pub fn backward_wires(&self) -> &[Backward] {
        &self.backward
    }

    /// A frame's size as shown (grown round members pushed), else its own.
    pub fn frame_size(&self, id: &str, own: [f32; 2]) -> [f32; 2] {
        self.frame_sizes.get(id).copied().unwrap_or(own)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backward_is_right_to_left() {
        assert!(runs_backward(&[[100.0, 0.0], [50.0, 0.0], [20.0, 0.0], [0.0, 10.0]]));
        assert!(!runs_backward(&[[0.0, 0.0], [10.0, 0.0], [20.0, 0.0], [30.0, 10.0]]));
        assert!(!runs_backward(&[[0.0, 0.0]]));
        assert!(!runs_backward(&[]));
    }

    #[test]
    fn sampled_follows_the_chain() {
        let path = [[0.0, 0.0], [10.0, 0.0], [20.0, 0.0], [30.0, 0.0]];
        let s = sampled(&path);
        assert_eq!(s.len(), wire_path::STEPS + 1);
        assert_eq!(s.first().copied(), Some(Pos2::new(0.0, 0.0)));
        assert!(s.last().is_some_and(|p| (p.x - 30.0).abs() < 1e-3));
    }
}
