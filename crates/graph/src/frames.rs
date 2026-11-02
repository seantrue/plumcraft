//! Frames: titled rectangles on the canvas for arranging nodes, as Blender's frames and
//! ComfyUI's groups. They mean nothing to an engine (what is a group for is a [`crate::Group`]):
//! a frame only remembers where it is and what it says.
//!
//! A frame holds what lies inside it ([`Graph::frame_held`]), so moving a frame moves that too
//! ([`Graph::move_frame`]): the nodes whose position (top-left corner) is inside, and a group
//! whose position is. A collapsed group is one node on the canvas, so it comes with everything
//! in it; hidden in one outside, a node stays where it is.
//!
//! Lifted from the research software's `crates/graph/src/frames.rs`. Different here:
//!
//! - collapsed groups move with their frame (the research software's left the group's node behind while its
//!   hidden members went);
//! - a drag can name what it picked up at its start, so a frame dragged over a node doesn't
//!   collect it;
//! - bend points move by the difference of the rounded positions, so a drag of many small steps
//!   doesn't drift, and a move that would put one past [`crate::MAX_VIA_COORD`] is refused (the
//!   file would not read back);
//! - a file's frame may not have a negative size;
//! - what this crate doesn't know about a frame is kept, as everywhere else in a workflow.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Graph, GraphError, GroupId, NodeId, Result};

/// A frame's id in its graph (`f1`, `f2`, …).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FrameId(pub String);

impl std::fmt::Display for FrameId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for FrameId {
    fn from(s: &str) -> Self {
        FrameId(s.to_owned())
    }
}

/// A titled rectangle on the canvas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    #[serde(default)]
    pub title: String,
    /// Top-left corner, in graph coordinates.
    pub pos: [f32; 2],
    pub size: [f32; 2],
    /// A tint, `#rrggbb`; the theme's accent when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Fields this editor doesn't know, kept for the file.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// What a frame carries when it moves ([`Graph::frame_held`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Held {
    pub nodes: BTreeSet<NodeId>,
    pub groups: BTreeSet<GroupId>,
}

/// The smallest frame the editor makes, each side.
pub const MIN_FRAME_SIDE: f32 = 80.0;
/// The largest coordinate or side a frame may have: far beyond any real canvas, small enough
/// that sums of them stay finite.
pub const MAX_FRAME_COORD: f32 = 1.0e7;
/// The longest frame title kept, in characters.
pub const MAX_FRAME_TITLE: usize = 200;

fn bad(reason: impl Into<String>) -> GraphError {
    GraphError::BadFrame(reason.into())
}

fn check_pos(pos: [f32; 2]) -> Result<()> {
    if pos.iter().all(|v| v.is_finite() && v.abs() <= MAX_FRAME_COORD) {
        Ok(())
    } else {
        Err(bad(format!("position {pos:?} is not a finite point within ±{MAX_FRAME_COORD}")))
    }
}

/// A size clamped to [`MIN_FRAME_SIDE`], or an error for one that isn't a finite size. A
/// negative side is clamped too, here where a person is dragging a corner.
fn check_size(size: [f32; 2]) -> Result<[f32; 2]> {
    if size.iter().all(|v| v.is_finite() && *v <= MAX_FRAME_COORD) {
        Ok([size[0].max(MIN_FRAME_SIDE), size[1].max(MIN_FRAME_SIDE)])
    } else {
        Err(bad(format!("size {size:?} is not a finite size up to {MAX_FRAME_COORD}")))
    }
}

/// A file's size: kept as written, so it must already be one.
fn check_stored_size(size: [f32; 2]) -> Result<()> {
    if size.iter().all(|v| *v >= 0.0) { check_size(size).map(|_| ()) } else { Err(bad(format!("size {size:?} is negative"))) }
}

fn check_title(title: &str) -> Result<String> {
    let t = title.trim();
    if t.chars().count() > MAX_FRAME_TITLE || t.chars().any(char::is_control) {
        return Err(bad(format!("a frame title has at most {MAX_FRAME_TITLE} characters and no control characters")));
    }
    Ok(t.to_owned())
}

fn check_color(color: &str) -> Result<()> {
    let hex = color.strip_prefix('#').unwrap_or("");
    if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) { Ok(()) } else { Err(bad(format!("colour {color:?} is not #rrggbb"))) }
}

impl Frame {
    /// Is `p` (graph coordinates) inside the frame?
    pub fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.pos[0] && p[1] >= self.pos[1] && p[0] <= self.pos[0] + self.size[0] && p[1] <= self.pos[1] + self.size[1]
    }
}

impl Graph {
    pub fn frame(&self, id: &FrameId) -> Option<&Frame> {
        self.frames.get(id)
    }

    fn frame_mut(&mut self, id: &FrameId) -> Result<&mut Frame> {
        self.frames.get_mut(id).ok_or_else(|| GraphError::UnknownFrame(id.clone()))
    }

    /// A frame id nothing has (a file's `next_frame` is whatever the file says).
    fn fresh_frame_id(&mut self) -> FrameId {
        let mut n = self.next_frame.max(1);
        let mut id = FrameId(format!("f{n}"));
        while self.frames.contains_key(&id) {
            n = n.checked_add(1).unwrap_or(1);
            id = FrameId(format!("f{n}"));
        }
        self.next_frame = n.saturating_add(1);
        id
    }

    /// Add a frame; its id is fresh.
    pub fn add_frame(&mut self, title: &str, pos: [f32; 2], size: [f32; 2]) -> Result<FrameId> {
        let title = check_title(title)?;
        check_pos(pos)?;
        let size = check_size(size)?;
        let id = self.fresh_frame_id();
        self.frames.insert(id.clone(), Frame { title, pos, size, color: None, extra: BTreeMap::new() });
        self.touch();
        Ok(id)
    }

    pub fn remove_frame(&mut self, id: &FrameId) -> Result<Frame> {
        let f = self.frames.remove(id).ok_or_else(|| GraphError::UnknownFrame(id.clone()))?;
        self.touch();
        Ok(f)
    }

    /// Change a frame's title or colour (`Some("")` clears the colour).
    pub fn set_frame(&mut self, id: &FrameId, title: Option<&str>, color: Option<&str>) -> Result<()> {
        let title = title.map(check_title).transpose()?;
        if let Some(c) = color.filter(|c| !c.is_empty()) {
            check_color(c)?;
        }
        let f = self.frame_mut(id)?;
        if let Some(t) = title {
            f.title = t;
        }
        if let Some(c) = color {
            f.color = (!c.is_empty()).then(|| c.to_owned());
        }
        self.touch();
        Ok(())
    }

    /// What a frame holds now: the nodes inside it, and the groups whose position is. A collapsed
    /// group is one node on the canvas: inside, it brings its members and the groups in it
    /// (wherever they are); outside, its hidden members don't count.
    pub fn frame_held(&self, id: &FrameId) -> Held {
        let Some(f) = self.frames.get(id) else { return Held::default() };
        let mut held = Held { nodes: self.nodes.iter().filter(|(_, n)| f.contains(n.pos)).map(|(id, _)| id.clone()).collect(), groups: BTreeSet::new() };
        let (inside, outside): (Vec<_>, Vec<_>) = self.groups.iter().filter(|(_, g)| g.collapsed).partition(|(_, g)| f.contains(g.pos));
        for (gid, _) in outside {
            held.nodes.retain(|n| !self.group_members(gid).contains(n));
        }
        for (gid, _) in inside {
            held.nodes.extend(self.group_members(gid));
            held.groups.extend(self.group_subtree(gid));
        }
        held.groups.extend(self.groups.iter().filter(|(_, g)| f.contains(g.pos)).map(|(gid, _)| gid.clone()));
        held
    }

    /// Move a frame to `pos`, with what it holds: `held` when given (what a drag picked up when
    /// it began; ids the graph doesn't have are skipped), else what it holds now. Bend points
    /// inside it go too. Returns what moved. A bend point the move would put off the canvas
    /// refuses the move, which changes nothing.
    pub fn move_frame(&mut self, id: &FrameId, pos: [f32; 2], held: Option<&Held>) -> Result<Held> {
        check_pos(pos)?;
        let was = self.frames.get(id).cloned().ok_or_else(|| GraphError::UnknownFrame(id.clone()))?;
        let held = held.cloned().unwrap_or_else(|| self.frame_held(id));
        let delta = [pos[0] - was.pos[0], pos[1] - was.pos[1]];
        // bend points are whole units: the difference of the rounded positions adds up, over a
        // drag's many small moves, to where the frame ended (rounding each move wouldn't)
        let whole = [pos[0].round() as i32 - was.pos[0].round() as i32, pos[1].round() as i32 - was.pos[1].round() as i32];
        let shift = |v: [i32; 2]| [v[0].saturating_add(whole[0]), v[1].saturating_add(whole[1])];
        let inside = |v: &[i32; 2]| was.contains([v[0] as f32, v[1] as f32]);
        if self
            .wires
            .iter()
            .flat_map(|w| &w.via)
            .filter(|v| inside(v))
            .any(|v| shift(*v).iter().any(|c| c.unsigned_abs() > crate::MAX_VIA_COORD.unsigned_abs()))
        {
            return Err(bad(format!("moving {id} to {pos:?} would put a bend point off the canvas")));
        }
        self.frame_mut(id)?.pos = pos;
        for n in &held.nodes {
            if let Some(node) = self.nodes.get_mut(n) {
                node.pos = [node.pos[0] + delta[0], node.pos[1] + delta[1]];
            }
        }
        for g in &held.groups {
            if let Some(group) = self.groups.get_mut(g) {
                group.pos = [group.pos[0] + delta[0], group.pos[1] + delta[1]];
            }
        }
        for v in self.wires.iter_mut().flat_map(|w| &mut w.via) {
            if inside(v) {
                *v = shift(*v);
            }
        }
        self.touch();
        Ok(held)
    }

    /// Resize a frame (each side at least [`MIN_FRAME_SIDE`]). Its nodes stay where they are.
    pub fn resize_frame(&mut self, id: &FrameId, size: [f32; 2]) -> Result<()> {
        let size = check_size(size)?;
        self.frame_mut(id)?.size = size;
        self.touch();
        Ok(())
    }

    /// Put a frame at `pos` with `size`, moving nothing else (Tidy refits a frame around the
    /// nodes it moved).
    pub fn fit_frame(&mut self, id: &FrameId, pos: [f32; 2], size: [f32; 2]) -> Result<()> {
        check_pos(pos)?;
        let size = check_size(size)?;
        let f = self.frame_mut(id)?;
        f.pos = pos;
        f.size = size;
        self.touch();
        Ok(())
    }

    /// Check frames read from a file: finite, in range, with valid colours.
    pub(crate) fn check_frames(&self) -> Result<()> {
        for (id, f) in &self.frames {
            let checked = check_pos(f.pos).and_then(|()| check_stored_size(f.size)).and_then(|()| check_title(&f.title)).and_then(|_| match &f.color {
                Some(c) => check_color(c),
                None => Ok(()),
            });
            checked.map_err(|e| bad(format!("{id}: {e}")))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn graph_with_nodes() -> Graph {
        let text = json!({"version": 1, "nodes": {
            "n1": {"kind": "k", "pos": [10.0, 10.0]},
            "n2": {"kind": "k", "pos": [300.0, 50.0]},
            "n3": {"kind": "k", "pos": [900.0, 900.0]},
        }});
        Graph::from_json(&text.to_string()).unwrap()
    }

    fn pos(g: &Graph, id: &str) -> [f32; 2] {
        g.node(&NodeId::from(id)).unwrap().pos
    }

    #[test]
    fn a_frame_carries_the_nodes_inside_it() {
        let mut g = graph_with_nodes();
        let f = g.add_frame(" inputs ", [0.0, 0.0], [500.0, 300.0]).unwrap();
        assert_eq!(f, FrameId::from("f1"));
        assert_eq!(g.frame(&f).unwrap().title, "inputs");
        assert_eq!(g.frame_held(&f).nodes, [NodeId::from("n1"), NodeId::from("n2")].into());
        let moved = g.move_frame(&f, [100.0, -20.0], None).unwrap();
        assert_eq!(moved.nodes.len(), 2);
        assert_eq!(pos(&g, "n1"), [110.0, -10.0]);
        assert_eq!(pos(&g, "n2"), [400.0, 30.0]);
        assert_eq!(pos(&g, "n3"), [900.0, 900.0], "outside: left alone");
        // resizing keeps nodes where they are, and a frame never gets smaller than the minimum
        g.resize_frame(&f, [10.0, 1000.0]).unwrap();
        assert_eq!(g.frame(&f).unwrap().size, [MIN_FRAME_SIDE, 1000.0]);
        assert_eq!(pos(&g, "n3"), [900.0, 900.0]);
        // title and colour
        g.set_frame(&f, Some("sources"), Some("#336699")).unwrap();
        assert_eq!(g.frame(&f).unwrap().color.as_deref(), Some("#336699"));
        g.set_frame(&f, None, Some("")).unwrap();
        assert_eq!(g.frame(&f).unwrap().color, None);
        // ids stay fresh after a removal
        let f2 = g.add_frame("", [0.0, 0.0], [100.0, 100.0]).unwrap();
        g.remove_frame(&f).unwrap();
        let f3 = g.add_frame("", [0.0, 0.0], [100.0, 100.0]).unwrap();
        assert_ne!(f2, f3);
        assert!(g.frame(&f).is_none());
    }

    #[test]
    fn a_drag_keeps_what_it_picked_up_and_collects_nothing_on_the_way() {
        let mut g = graph_with_nodes();
        let f = g.add_frame("", [0.0, 0.0], [200.0, 200.0]).unwrap();
        let held = g.frame_held(&f);
        assert_eq!(held.nodes, [NodeId::from("n1")].into());
        // over n2 (at 300, 50) and past it, in steps, as a drag goes
        for x in [100.0, 200.0, 300.0, 400.0] {
            g.move_frame(&f, [x, 0.0], Some(&held)).unwrap();
        }
        assert_eq!(pos(&g, "n2"), [300.0, 50.0], "a node the frame passed over stays");
        assert_eq!(pos(&g, "n1"), [410.0, 10.0]);
        // without what it held, it holds what is inside now
        g.resize_frame(&f, [800.0, 1000.0]).unwrap();
        g.move_frame(&f, [0.0, 0.0], None).unwrap();
        assert_eq!(pos(&g, "n3"), [500.0, 900.0]);
        // ids it doesn't have are skipped
        let ghost = Held { nodes: [NodeId::from("n9")].into(), groups: [GroupId::from("g9")].into() };
        assert!(g.move_frame(&f, [5.0, 5.0], Some(&ghost)).is_ok());
    }

    #[test]
    fn a_collapsed_group_goes_with_its_frame_as_one_node() {
        let mut g = graph_with_nodes();
        let inner = g.create_group("inner", &[NodeId::from("n2")], &[]).unwrap();
        g.set_collapsed(&inner, true).unwrap();
        g.groups.get_mut(&inner).unwrap().pos = [100.0, 100.0];
        let f = g.add_frame("", [50.0, 50.0], [200.0, 200.0]).unwrap();
        // its member lies outside the frame, its node inside
        assert_eq!(pos(&g, "n2"), [300.0, 50.0]);
        assert_eq!(g.frame_held(&f), Held { nodes: [NodeId::from("n2")].into(), groups: [inner.clone()].into() });
        g.move_frame(&f, [150.0, 50.0], None).unwrap();
        assert_eq!(g.group(&inner).unwrap().pos, [200.0, 100.0]);
        assert_eq!(pos(&g, "n2"), [400.0, 50.0], "it opens where it now is");
        // a collapsed group's hidden member inside a frame its node is outside of stays
        let f = g.add_frame("", [350.0, 0.0], [100.0, 100.0]).unwrap();
        assert!(g.frame_held(&f).nodes.is_empty());
        g.move_frame(&f, [0.0, 0.0], None).unwrap();
        assert_eq!(pos(&g, "n2"), [400.0, 50.0]);
        // an open group's position goes with the frame, so it collapses there
        g.groups.get_mut(&inner).unwrap().collapsed = false;
        let f = g.add_frame("", [190.0, 90.0], [100.0, 100.0]).unwrap();
        g.move_frame(&f, [200.0, 90.0], None).unwrap();
        assert_eq!(g.group(&inner).unwrap().pos, [210.0, 100.0]);
        assert_eq!(pos(&g, "n2"), [400.0, 50.0], "and its members only move if they are inside");
    }

    #[test]
    fn hostile_frames_are_refused() {
        let mut g = graph_with_nodes();
        assert!(g.add_frame("x", [f32::NAN, 0.0], [100.0, 100.0]).is_err());
        assert!(g.add_frame("x", [0.0, 0.0], [f32::INFINITY, 100.0]).is_err());
        assert!(g.add_frame("x", [2.0e7, 0.0], [100.0, 100.0]).is_err());
        assert!(g.add_frame("a\nb", [0.0, 0.0], [100.0, 100.0]).is_err());
        assert!(g.add_frame(&"x".repeat(201), [0.0, 0.0], [100.0, 100.0]).is_err());
        let f = g.add_frame("x", [0.0, 0.0], [100.0, 100.0]).unwrap();
        assert!(g.set_frame(&f, None, Some("red")).is_err());
        assert!(g.move_frame(&f, [f32::MAX, 0.0], None).is_err());
        assert!(g.move_frame(&FrameId::from("f9"), [0.0, 0.0], None).is_err());
        assert!(g.remove_frame(&FrameId::from("f9")).is_err());
        assert!(g.fit_frame(&f, [2.0e7, 0.0], [100.0, 100.0]).is_err());
        assert!(g.fit_frame(&FrameId::from("f9"), [0.0, 0.0], [100.0, 100.0]).is_err());
    }

    #[test]
    fn hostile_frames_in_a_file_are_errors() {
        let open = |frames: Value| Graph::from_json(&json!({"version": 1, "nodes": {}, "frames": frames}).to_string());
        let good = json!({"title": "x", "pos": [0, 0], "size": [200, 100]});
        assert!(open(json!({"f1": good})).is_ok());
        let with = |key: &str, v: Value| {
            let mut f = good.clone();
            f[key] = v;
            json!({"f1": f})
        };
        for bad in [
            with("size", json!([1e30, 10])),
            with("size", json!([-100, 10])),
            with("size", json!([1])),
            with("size", json!("big")),
            with("pos", json!([1e39, 0])),
            with("pos", json!([2e7, 0])),
            with("pos", json!(null)),
            with("title", json!("a\nb")),
            with("title", json!("x".repeat(201))),
            with("title", json!(7)),
            with("color", json!("red")),
            with("color", json!(7)),
            json!({"f1": {"name": "no position"}}),
            json!({"f1": 7}),
            json!(["f1"]),
            json!(7),
        ] {
            assert!(open(bad.clone()).is_err(), "{bad}");
        }
        let text = |next: Value| json!({"version": 1, "nodes": {}, "next_frame": next}).to_string();
        assert!(Graph::from_json(&text(json!(-1))).is_err());
        assert!(Graph::from_json(&text(json!("x"))).is_err());
    }

    #[test]
    fn a_frame_id_is_found_whatever_the_file_says_next() {
        let text = json!({"version": 1, "nodes": {}, "next_frame": u64::MAX,
            "frames": {format!("f{}", u64::MAX): {"pos": [0, 0], "size": [100, 100]}}});
        let mut g = Graph::from_json(&text.to_string()).unwrap();
        assert_eq!(g.next_frame, u64::MAX);
        let id = g.add_frame("", [0.0, 0.0], [100.0, 100.0]).unwrap();
        assert_eq!(id, FrameId::from("f1"), "past the last number it starts again");
        assert_eq!(g.frames.len(), 2);
    }

    #[test]
    fn frames_round_trip_and_are_left_out_when_there_are_none() {
        let mut g = graph_with_nodes();
        assert!(!g.to_json().unwrap().contains("frame"));
        g.add_frame("inputs", [0.0, 0.0], [500.0, 300.0]).unwrap();
        let back = Graph::from_json(&g.to_json().unwrap()).unwrap();
        assert_eq!(back.frames, g.frames);
        assert_eq!(back.next_frame, 2);
    }

    /// As the research software writes them, with a colour and a field nobody knows yet, and a size under the
    /// editor's minimum: read and written, nothing about them changes.
    #[test]
    fn a_file_keeps_its_frames_exactly() {
        let frames = json!({"f7": {"title": " odd  title ", "pos": [-12.5, 0.25], "size": [40.0, 1234.5], "color": "#AbCdEf", "locked": {"by": "me"}},
                            "f2": {"title": "", "pos": [0.0, 0.0], "size": [80.0, 80.0]}});
        let text = json!({"version": 1, "nodes": {}, "frames": frames, "next_frame": 9});
        let back: Value = serde_json::from_str(&Graph::from_json(&text.to_string()).unwrap().to_json().unwrap()).unwrap();
        assert_eq!(back["frames"], frames);
        assert_eq!(back["next_frame"], json!(9));
    }
}
