//! Frame commands: add, retitle or recolour, remove, move and resize the canvas's frames
//! (titled rectangles for arranging nodes; [`plumcraft_graph::frames`]). Adding, editing and
//! removing are undo steps; moving and resizing are not, as `node.move` isn't. None of them
//! makes anything stale: an engine ignores frames (`docs/interface.md`, section 7.6 rule 8).
//!
//! Lifted from the research software's `crates/engine/src/frame_cmds.rs`. `frame.add` also takes collapsed
//! groups, and `frame.move` what a drag picked up when it began.

use plumcraft_graph::{FrameId, Held, MAX_FRAME_COORD};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, group_list_param, node_list_param, opt_str_param, pos_param, str_param};
use crate::{Result, Session, SessionError};

/// Room around the nodes a new frame encloses, and above them for its title.
const PAD: f32 = 24.0;
const HEADER: f32 = 32.0;
/// A new frame's size when it encloses no nodes.
const DEFAULT_SIZE: [f32; 2] = [480.0, 320.0];

fn has_frames(s: &Session) -> std::result::Result<(), String> {
    if s.graph().frames.is_empty() { Err("there are no frames".into()) } else { Ok(()) }
}

fn frame_param(p: &Value) -> Result<FrameId> {
    Ok(FrameId::from(str_param(p, "frame")?))
}

/// `frame.add {title?, pos?, size?, nodes?, groups?, sizes?}`: around `nodes` and collapsed
/// `groups` when given, else at `pos` with `size`.
fn frame_add(s: &mut Session, p: &Value) -> Result<Value> {
    let title = opt_str_param(p, "title")?.unwrap_or("Frame").to_owned();
    let around = crate::edit_cmds::rects(s, p, 0)?;
    let (pos, size) = if around.is_empty() {
        (pos_param(p, "pos")?.unwrap_or([0.0, 0.0]), pos_param(p, "size")?.unwrap_or(DEFAULT_SIZE))
    } else {
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        for (_, [x, y, w, h]) in &around {
            lo = [lo[0].min(*x), lo[1].min(*y)];
            hi = [hi[0].max(x + w.min(MAX_FRAME_COORD)), hi[1].max(y + h.min(MAX_FRAME_COORD))];
        }
        ([lo[0] - PAD, lo[1] - PAD - HEADER], [hi[0] - lo[0] + 2.0 * PAD, hi[1] - lo[1] + 2.0 * PAD + HEADER])
    };
    let id = s.edit("Add Frame", |g, _| Ok(g.add_frame(&title, pos, size)?))?;
    Ok(json!({"id": id, "frame": s.graph().frame(&id)}))
}

fn frame_set(s: &mut Session, p: &Value) -> Result<Value> {
    let id = frame_param(p)?;
    let title = opt_str_param(p, "title")?.map(str::to_owned);
    let color = match p.get("color") {
        None => None,
        Some(Value::Null) => Some(String::new()),
        Some(Value::String(c)) => Some(c.clone()),
        Some(_) => return Err(SessionError::Params("\"color\" must be \"#rrggbb\" or null".into())),
    };
    s.edit("Edit Frame", |g, _| Ok(g.set_frame(&id, title.as_deref(), color.as_deref())?))?;
    Ok(json!({"frame": s.graph().frame(&id)}))
}

fn frame_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let id = frame_param(p)?;
    s.edit("Remove Frame", |g, _| Ok(g.remove_frame(&id)?))?;
    Ok(Value::Null)
}

fn frame_move(s: &mut Session, p: &Value) -> Result<Value> {
    let id = frame_param(p)?;
    let pos = pos_param(p, "pos")?.ok_or_else(|| SessionError::Params("\"pos\" is required".into()))?;
    let held = (p.get("nodes").is_some() || p.get("groups").is_some())
        .then(|| {
            Ok::<_, SessionError>(Held {
                nodes: node_list_param(p, "nodes")?.into_iter().collect(),
                groups: group_list_param(p, "groups")?.into_iter().collect(),
            })
        })
        .transpose()?;
    let moved = s.graph_mut_untracked().move_frame(&id, pos, held.as_ref())?;
    s.mark_dirty();
    Ok(json!({"nodes": moved.nodes, "groups": moved.groups}))
}

fn frame_resize(s: &mut Session, p: &Value) -> Result<Value> {
    let id = frame_param(p)?;
    let size = pos_param(p, "size")?.ok_or_else(|| SessionError::Params("\"size\" is required".into()))?;
    s.graph_mut_untracked().resize_frame(&id, size)?;
    s.mark_dirty();
    Ok(json!({"frame": s.graph().frame(&id)}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "frame.add",
            label: "Add Frame",
            menu: &[],
            shortcut: None,
            params: "{title?: string (default \"Frame\"), nodes?: [node id], groups?: [collapsed group id], sizes?: {id: [w, h]}, pos?: [x, y], size?: [w, h]} -> {id, frame} (around the nodes and groups when given, else at pos; a node with no size is estimated as the canvas draws it; an undo step)",
            enabled: always,
            run: frame_add,
            journal: true,
        },
        CommandSpec {
            id: "frame.set",
            label: "Edit Frame",
            menu: &[],
            shortcut: None,
            params: "{frame: frame id, title?: string, color?: \"#rrggbb\" | null} -> {frame} (an undo step)",
            enabled: has_frames,
            run: frame_set,
            journal: true,
        },
        CommandSpec {
            id: "frame.remove",
            label: "Remove Frame",
            menu: &[],
            shortcut: None,
            params: "{frame: frame id} (its nodes stay; an undo step)",
            enabled: has_frames,
            run: frame_remove,
            journal: true,
        },
        CommandSpec {
            id: "frame.move",
            label: "Move Frame",
            menu: &[],
            shortcut: None,
            params: "{frame: frame id, pos: [x, y], nodes?: [node id], groups?: [group id]} -> {nodes: [moved], groups: [moved]} (what is inside moves with it, or the nodes and groups named: a drag names what it picked up when it began; not an undo step)",
            enabled: has_frames,
            run: frame_move,
            journal: false,
        },
        CommandSpec {
            id: "frame.resize",
            label: "Resize Frame",
            menu: &[],
            shortcut: None,
            params: "{frame: frame id, size: [w, h]} -> {frame} (each side at least 80; not an undo step)",
            enabled: has_frames,
            run: frame_resize,
            journal: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use plumcraft_graph::{Graph, GroupId, NodeId};

    use super::*;

    fn session() -> Session {
        Session::new(plumcraft_text::engine().schema())
    }

    fn add(s: &mut Session, kind: &str, pos: [f32; 2]) -> String {
        s.execute("node.add", &json!({"kind": kind, "pos": pos})).unwrap()["id"].as_str().unwrap().to_owned()
    }

    fn pos(s: &Session, n: &str) -> [f32; 2] {
        s.graph().node(&NodeId::from(n)).unwrap().pos
    }

    #[test]
    fn a_frame_around_nodes_moves_them_and_undoes() {
        let mut s = session();
        let a = add(&mut s, "text.upper", [100.0, 100.0]);
        let b = add(&mut s, "text.upper", [400.0, 200.0]);
        let far = add(&mut s, "text.upper", [2000.0, 2000.0]);
        let r = s.execute("frame.add", &json!({"title": "inputs", "nodes": [a, b], "sizes": {a.as_str(): [200, 100], b.as_str(): [200, 100]}})).unwrap();
        assert_eq!(r["frame"]["pos"], json!([100.0 - PAD, 100.0 - PAD - HEADER]));
        assert_eq!(r["frame"]["size"], json!([500.0 + 2.0 * PAD, 200.0 + 2.0 * PAD + HEADER]));
        assert_eq!(s.undo_label(), Some("Add Frame"));
        let id = r["id"].as_str().unwrap().to_owned();
        // moving carries both nodes, not the far one
        let r = s.execute("frame.move", &json!({"frame": id, "pos": [0, 0]})).unwrap();
        assert_eq!(r["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(pos(&s, &a), [100.0 - 76.0, 100.0 - 44.0]);
        assert_eq!(pos(&s, &far), [2000.0, 2000.0]);
        assert_eq!(s.undo_label(), Some("Add Frame"), "moving isn't an undo step");
        s.execute("frame.resize", &json!({"frame": id, "size": [10, 900]})).unwrap();
        s.execute("frame.set", &json!({"frame": id, "title": "sources", "color": "#204060"})).unwrap();
        assert_eq!(s.graph().frame(&FrameId::from(id.as_str())).unwrap().title, "sources");
        s.execute("edit.undo", &Value::Null).unwrap();
        assert_eq!(s.graph().frame(&FrameId::from(id.as_str())).unwrap().title, "inputs");
        s.execute("edit.redo", &Value::Null).unwrap();
        assert_eq!(s.graph().frame(&FrameId::from(id.as_str())).unwrap().color.as_deref(), Some("#204060"));
        s.execute("frame.remove", &json!({"frame": id})).unwrap();
        assert!(s.graph().frames.is_empty());
        assert_eq!(s.graph().nodes.len(), 3, "its nodes stay");
        s.execute("edit.undo", &Value::Null).unwrap();
        assert_eq!(s.graph().frames.len(), 1);
    }

    #[test]
    fn a_node_with_no_size_is_framed_as_the_canvas_would_draw_it() {
        let mut s = session();
        let a = add(&mut s, "text.upper", [0.0, 0.0]);
        let r = s.execute("frame.add", &json!({"nodes": [a]})).unwrap();
        let size = r["frame"]["size"].as_array().unwrap();
        // room for the node, not just for its corner
        assert!(size[0].as_f64().unwrap() > 200.0 && size[1].as_f64().unwrap() > 100.0, "{size:?}");
    }

    #[test]
    fn a_frame_frames_a_collapsed_group_and_moving_names_what_it_holds() {
        let mut s = session();
        let a = add(&mut s, "text.upper", [0.0, 0.0]);
        let b = add(&mut s, "text.upper", [300.0, 0.0]);
        let g = s.execute("group.create", &json!({"nodes": [b], "name": "G"})).unwrap()["id"].as_str().unwrap().to_owned();
        s.execute("group.collapse", &json!({"group": g, "collapsed": true})).unwrap();
        let r = s.execute("frame.add", &json!({"groups": [g], "sizes": {g.as_str(): [200, 80]}})).unwrap();
        assert_eq!(r["frame"]["pos"], json!([300.0 - PAD, 0.0 - PAD - HEADER]));
        assert_eq!(r["frame"]["size"], json!([200.0 + 2.0 * PAD, 80.0 + 2.0 * PAD + HEADER]));
        let id = r["id"].as_str().unwrap().to_owned();
        // named, only those; not the node it would hold now
        let r = s.execute("frame.move", &json!({"frame": id, "pos": [-500, 0], "nodes": [a], "groups": []})).unwrap();
        assert_eq!(r["nodes"], json!([a]));
        assert_eq!(pos(&s, &b), [300.0, 0.0]);
        assert_eq!(s.graph().group(&GroupId::from(g.as_str())).unwrap().pos, [300.0, 0.0]);
    }

    #[test]
    fn frames_make_nothing_stale_and_are_saved() {
        let mut s = session();
        let a = add(&mut s, "text.upper", [0.0, 0.0]);
        s.run.nodes.insert(a.clone(), Default::default());
        let id = s.execute("frame.add", &json!({"nodes": [a]})).unwrap()["id"].as_str().unwrap().to_owned();
        s.execute("frame.set", &json!({"frame": id, "title": "x"})).unwrap();
        s.execute("frame.move", &json!({"frame": id, "pos": [9, 9]})).unwrap();
        s.execute("frame.remove", &json!({"frame": id})).unwrap();
        assert!(s.run.nodes[&a].stale_since.is_none());
        assert!(s.is_dirty());
        // the moves are positions, which are saved
        let mut s = session();
        let dirty = |s: &mut Session, id: &str, p: Value| {
            s.mark_saved("x.json".into());
            s.execute(id, &p).unwrap();
            assert!(s.is_dirty(), "{id}");
        };
        let id = s.execute("frame.add", &json!({"pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
        dirty(&mut s, "frame.move", json!({"frame": id, "pos": [5, 5]}));
        dirty(&mut s, "frame.resize", json!({"frame": id, "size": [200, 200]}));
        let back = Graph::from_json(&s.graph().to_json().unwrap()).unwrap();
        assert_eq!(back.frames, s.graph().frames);
    }

    #[test]
    fn frame_commands_refuse_junk() {
        let mut s = session();
        for id in ["frame.set", "frame.remove", "frame.move", "frame.resize"] {
            assert!(s.execute(id, &json!({"frame": "f1", "pos": [0, 0], "size": [1, 1]})).is_err(), "{id} with no frames");
        }
        let r = s.execute("frame.add", &json!({"pos": [10, 10]})).unwrap();
        assert_eq!(r["frame"]["title"], "Frame");
        assert_eq!(r["frame"]["size"], json!(DEFAULT_SIZE));
        let before = s.graph().clone();
        for (id, p) in [
            ("frame.add", json!({"nodes": ["n9"]})),
            ("frame.add", json!({"groups": ["g9"]})),
            ("frame.add", json!({"pos": [1e30, 0]})),
            ("frame.add", json!({"title": "a\u{0}b"})),
            ("frame.add", json!({"title": 7})),
            ("frame.add", json!({"sizes": 7})),
            ("frame.set", json!({"frame": "f1", "color": "blue"})),
            ("frame.set", json!({"frame": "f1", "color": 3})),
            ("frame.move", json!({"frame": "f9", "pos": [0, 0]})),
            ("frame.move", json!({"frame": "f1"})),
            ("frame.move", json!({"frame": "f1", "pos": [0, 0], "nodes": 7})),
            ("frame.move", json!({"frame": "f1", "pos": [0, 0], "groups": [7]})),
            ("frame.resize", json!({"frame": "f1", "size": [1e30, 1]})),
            ("frame.remove", json!({"frame": "f2"})),
        ] {
            assert!(s.execute(id, &p).is_err(), "{id} {p}");
            assert_eq!(s.graph(), &before, "{id} {p}");
        }
    }
}
