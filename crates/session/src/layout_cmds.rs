//! `graph.tidy`: lay the graph (or the selected nodes) out with [`plumcraft_layout`]: left to
//! right, packed to the top, parameters in a lane beside the pins they feed, long chains wrapped
//! where the fewest wires cross. Lifted from the research software's `graph.tidy` (its research note is
//! The research software's `docs/research/node_layout_20261023.md`).
//!
//! **Engine-neutral.** the research software knew its picture type; PlumCraft knows only the scalars
//! (golden rule 3). A *parameter* is a node with nothing wired in whose wired outputs are all
//! scalars (a Vary node, a literal): it goes in a lane beside what it feeds. A wire carrying
//! anything else (a document, an image, a list of files) is the data spine and pulls harder.
//!
//! **Sizes and pins.** The canvas passes the node sizes and pin positions it measured; without
//! them (the control channel's `command.run`, a test) they are estimated as the canvas draws a
//! node: a header, one row per input from the top, and the outputs at the bottom.
//!
//! **Net labels.** A wire drawn through a net drops out of crossing minimisation and the wrap's
//! cut count, each node keeps room for its tags, and a tag dragged away goes back to its pin.
//!
//! One undo step. What moves: the nodes laid out; a frame holding only nodes that moved, refitted
//! around them; and a group's position (where it is drawn collapsed), to its members' top-left
//! when all of them moved. The laid-out nodes keep the top-left corner they had, so the view
//! doesn't jump.

use std::collections::{BTreeMap, BTreeSet};

use plumcraft_graph::{Graph, NodeId, PortRef};
use plumcraft_layout::{Edge, Layout, Node, Options};
use plumcraft_proto::catalogue::{NodeDef, SchemaReply, Socket, scalars};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, node_list_param};
use crate::{Result, Session, SessionError};

/// The canvas's node header, then one row per port, and the padding under the last row; the last
/// output pin's centre sits `OUT_BOTTOM` above the node's bottom (graph units, measured on the
/// editor's canvas: `crates/ui/tests/kittest_tidy.rs` holds the estimate to it).
const HEADER: f32 = 31.0;
const ROW: f32 = 21.0;
const PADDING: f32 = 2.0;
const OUT_BOTTOM: f32 = 14.5;
/// A frame's margin around what it holds: sides and bottom, and the top (its title bar).
const FRAME_SIDE: f32 = 40.0;
const FRAME_TOP: f32 = 80.0;
/// Widths: a node with a text or path field, with a dropdown, with only number fields and
/// checkboxes, and with no widgets (at least; a longer title widens it).
const WIDE: f32 = 266.0;
const CHOICE: f32 = 246.0;
const NUMBER: f32 = 170.0;
const NARROW: f32 = 160.0;
/// A header's width without its title (the canvas's `STATUS_SLOT` included).
const HEADER_BASE: f32 = 198.0;
/// The most pins a node may report (more are ignored).
const MAX_PINS: usize = 512;
/// A net tag: the advance of its 11-point monospace text, its padding and point, and the stub
/// between it and its pin, graph units (`crates/ui/src/net_view.rs`).
const TAG_ADVANCE: f32 = 6.62;
const TAG_PAD: f32 = 17.0;
const TAG_STUB: f32 = 14.0;

/// The width of a net tag showing `text`, graph units, as the canvas draws it at zoom 1
/// (`net_view::drawn_tag_width` measures it; a test holds the two together).
pub fn tag_width(text: &str) -> f32 {
    TAG_ADVANCE * text.chars().count() as f32 + TAG_PAD
}

/// The room each node keeps for net tags: `[left, right]`, the widest tag at its inputs and at
/// its outputs, each with its stub.
fn tag_margins(g: &Graph) -> BTreeMap<NodeId, [f32; 2]> {
    let mut out: BTreeMap<NodeId, [f32; 2]> = BTreeMap::new();
    for w in g.wires.iter().filter(|w| w.label.is_some()) {
        let Some(net) = g.net_of_wire(w) else { continue };
        let m = out.entry(w.to.node.clone()).or_default();
        let text = match &w.slice {
            Some(s) => format!("{} [{s}]", net.name),
            None => net.name.clone(),
        };
        m[0] = m[0].max(tag_width(&text) + TAG_STUB);
        let uses = g.net_wires(&net).count();
        let m = out.entry(w.from.node.clone()).or_default();
        m[1] = m[1].max(tag_width(&format!("{} ×{uses}", net.name)) + TAG_STUB);
    }
    out
}

/// The input rows the canvas draws for a node of type `def` (`canvas::input_rows`): each input,
/// a variadic input's shown slots, a choice's sub-ports. Their keys, in order.
fn input_keys(def: &NodeDef, id: &NodeId, g: &Graph) -> Vec<String> {
    let mut keys = Vec::new();
    for port in &def.inputs {
        if let Some(v) = &port.variadic {
            let names = plumcraft_graph::variadic_slots(port);
            let wired: Vec<bool> = names.iter().map(|n| g.wire_into(&PortRef::new(id.clone(), n.clone())).is_some()).collect();
            let last = wired.iter().rposition(|w| *w).map_or(0, |i| i + 1);
            let count = last.saturating_add(1).max(v.min as usize).min(names.len());
            keys.extend(names.into_iter().take(count));
            continue;
        }
        keys.push(port.name.clone());
        if let Some(options) = &port.options {
            let choice = g.node(id).and_then(|n| n.values.get(&port.name)).unwrap_or(&port.default);
            let key = plumcraft_graph::ports::choice_key(choice);
            for sub in options.get(&key).map(Vec::as_slice).unwrap_or_default() {
                keys.push(format!("{}.{}", port.name, sub.name));
            }
        }
    }
    keys
}

/// A size for a node nobody measured: header, input rows, output rows; as wide as its widest
/// widget, or its title. A preview (drawn once a node has run) isn't counted: the editor passes
/// what it measured.
fn estimate(def: &NodeDef, title: &str, inputs: usize) -> [f32; 2] {
    let widget = |p: &plumcraft_proto::catalogue::Port| match p.ty.as_str() {
        _ if p.socket_mode() == Socket::Socket => NARROW,
        scalars::STRING => WIDE,
        scalars::ENUM => CHOICE,
        _ => NUMBER,
    };
    // The header: chevron, icon, state dot, the title, the run status's slot and the menu
    // (the canvas keeps the dot's and the status's room before a run, so this holds either way).
    let header = HEADER_BASE + 7.0 * title.chars().count() as f32;
    let w = def.inputs.iter().map(widget).fold(header.max(NARROW), f32::max);
    [w, HEADER + ROW * (inputs + def.outputs.len()) as f32 + PADDING]
}

/// A parameter: nothing wired in, and only scalars wired out (a Vary node, a literal).
fn is_param(def: &NodeDef, id: &NodeId, g: &Graph) -> bool {
    let out_ty = |port: &str| def.outputs.iter().find(|p| p.name == port).map(|p| p.ty.as_str());
    !g.wires.iter().any(|w| &w.to.node == id) && g.wires.iter().filter(|w| &w.from.node == id).all(|w| out_ty(&w.from.port).is_some_and(scalars::is_scalar))
}

/// `{id: [w, h]}` from the params; junk entries are skipped (the layout clamps the rest).
fn sizes_param(p: &Value) -> Result<BTreeMap<NodeId, [f32; 2]>> {
    match p.get("sizes") {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(m)) => Ok(m
            .iter()
            .filter_map(|(k, v)| {
                let a = v.as_array()?;
                let w = a.first()?.as_f64()? as f32;
                let h = a.get(1)?.as_f64()? as f32;
                (w.is_finite() && h.is_finite()).then(|| (NodeId(k.clone()), [w, h]))
            })
            .collect()),
        Some(_) => Err(SessionError::Params("\"sizes\" must be {node id: [w, h]}".into())),
    }
}

/// Measured pins: each node's input and output pin heights from its top, by port.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Pins {
    pub inputs: BTreeMap<String, f32>,
    pub outputs: BTreeMap<String, f32>,
}

/// `{id: {inputs: {key: y}, outputs: {port: y}}}` from the params; junk entries are skipped.
fn pins_param(p: &Value) -> Result<BTreeMap<NodeId, Pins>> {
    let side = |v: Option<&Value>| -> BTreeMap<String, f32> {
        v.and_then(Value::as_object)
            .into_iter()
            .flatten()
            .take(MAX_PINS)
            .filter_map(|(k, y)| Some((k.clone(), y.as_f64().map(|y| y as f32).filter(|y| y.is_finite())?)))
            .collect()
    };
    match p.get("pins") {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(m)) => Ok(m
            .iter()
            .filter(|(_, v)| v.is_object())
            .map(|(k, v)| (NodeId(k.clone()), Pins { inputs: side(v.get("inputs")), outputs: side(v.get("outputs")) }))
            .collect()),
        Some(_) => Err(SessionError::Params("\"pins\" must be {node id: {inputs: {port: y}, outputs: {port: y}}}".into())),
    }
}

/// Pins in drawing order: each port's key and its height from the node's top.
pub type PinRows = Vec<(String, f32)>;

/// A node's size and its input and output pins (key, height from its top), in the order the
/// canvas draws them: what was measured, else estimated as the canvas would draw it. `None` for
/// a node not in the graph.
pub fn geometry(g: &Graph, catalog: &SchemaReply, id: &NodeId, size: Option<[f32; 2]>, pins: Option<&Pins>) -> Option<([f32; 2], PinRows, PinRows)> {
    let n = g.node(id)?;
    let def = catalog.node(&n.kind);
    let keys = def.map(|d| input_keys(d, id, g)).unwrap_or_default();
    let outs: Vec<String> = def.map(|d| d.outputs.iter().map(|p| p.name.clone()).collect()).unwrap_or_default();
    let title = g.node_title(id, |k| crate::display_name(catalog, k)).unwrap_or_default();
    let size = size.or_else(|| def.map(|d| estimate(d, &title, keys.len()))).unwrap_or(plumcraft_layout::DEFAULT_SIZE);
    let input_y = |i: usize, k: &String| pins.and_then(|m| m.inputs.get(k).copied()).unwrap_or(HEADER + ROW * (i as f32 + 0.5));
    // outputs sit at the bottom
    let last = outs.len().saturating_sub(1);
    let output_y = |i: usize, k: &String| pins.and_then(|m| m.outputs.get(k).copied()).unwrap_or(size[1] - OUT_BOTTOM - ROW * last.saturating_sub(i) as f32);
    let ins = keys.iter().enumerate().map(|(i, k)| (k.clone(), input_y(i, k))).collect();
    let outs = outs.iter().enumerate().map(|(i, k)| (k.clone(), output_y(i, k))).collect();
    Some((size, ins, outs))
}

/// Lay out `ids` (all nodes when empty) in `g`. Returns the layout and the ids in its order.
pub fn tidy(g: &mut Graph, catalog: &SchemaReply, ids: &[NodeId], sizes: &BTreeMap<NodeId, [f32; 2]>, pins: &BTreeMap<NodeId, Pins>) -> (Vec<NodeId>, Layout) {
    let chosen: Vec<NodeId> = if ids.is_empty() {
        g.nodes.keys().cloned().collect()
    } else {
        ids.iter().filter(|id| g.nodes.contains_key(*id)).cloned().collect::<BTreeSet<_>>().into_iter().collect()
    };
    let index: BTreeMap<&NodeId, usize> = chosen.iter().enumerate().map(|(i, id)| (id, i)).collect();
    let mut inputs: Vec<Vec<String>> = Vec::new();
    let mut outputs: Vec<Vec<String>> = Vec::new();
    let mut nodes: Vec<Node> = Vec::new();
    let margins = tag_margins(g);
    for id in &chosen {
        let Some(n) = g.node(id) else { continue };
        let Some((size, ins, outs)) = geometry(g, catalog, id, sizes.get(id).copied(), pins.get(id)) else { continue };
        nodes.push(Node {
            size,
            inputs: ins.iter().map(|(_, y)| *y).collect(),
            outputs: outs.iter().map(|(_, y)| *y).collect(),
            param: catalog.node(&n.kind).is_some_and(|d| is_param(d, id, g)),
            pos: n.pos,
            margin: margins.get(id).copied().unwrap_or_default(),
        });
        inputs.push(ins.into_iter().map(|(k, _)| k).collect());
        outputs.push(outs.into_iter().map(|(k, _)| k).collect());
    }
    let edges: Vec<Edge> = g
        .wires
        .iter()
        .filter_map(|w| {
            let (&from, &to) = (index.get(&w.from.node)?, index.get(&w.to.node)?);
            let out = outputs.get(from)?.iter().position(|k| *k == w.from.port).unwrap_or(0);
            let inp = inputs.get(to)?.iter().position(|k| *k == w.to.port).unwrap_or(0);
            let from_def = catalog.node(&g.node(&w.from.node)?.kind)?;
            // the data spine (anything but a scalar) pulls hardest
            let weight = if from_def.outputs.iter().find(|p| p.name == w.from.port).is_some_and(|p| !scalars::is_scalar(&p.ty)) { 2.0 } else { 1.0 };
            Some(Edge { from, out, to, inp, weight, labelled: w.label.is_some() })
        })
        .collect();
    let layout = plumcraft_layout::layout(&nodes, &edges, &Options::default());

    // keep the top-left corner the nodes had
    let corner = |ps: &mut dyn Iterator<Item = [f32; 2]>| ps.fold([f32::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
    let old = corner(&mut nodes.iter().map(|n| n.pos));
    // whole units, each rounded on its own, so tidying a tidy graph lands on the same numbers
    let origin = if old[0].is_finite() && old[1].is_finite() { [old[0].round(), old[1].round()] } else { [0.0, 0.0] };
    // frames holding only nodes that move get refitted around them
    let held: Vec<(plumcraft_graph::FrameId, BTreeSet<NodeId>)> = g
        .frames
        .keys()
        .map(|f| (f.clone(), g.frame_held(f).nodes))
        .filter(|(_, held)| !held.is_empty() && held.iter().all(|n| index.contains_key(n)))
        .collect();
    for (i, id) in chosen.iter().enumerate() {
        if let (Some(p), Some(node)) = (layout.pos.get(i), g.nodes.get_mut(id)) {
            node.pos = [origin[0] + p[0].round(), origin[1] + p[1].round()];
        }
    }
    let moving: BTreeSet<&NodeId> = chosen.iter().collect();
    // Bend points were drawn around the old places: a wire between nodes Tidy moved goes back
    // to a plain curve.
    for w in &mut g.wires {
        if moving.contains(&w.from.node) && moving.contains(&w.to.node) {
            w.via.clear();
        }
    }
    // tags back beside their pins, where room was kept for them
    let tags: Vec<PortRef> = g.wires.iter().filter(|w| w.label_at.is_some() && moving.contains(&w.to.node)).map(|w| w.to.clone()).collect();
    for to in tags {
        let _ = g.move_wire_label(&to, None);
    }
    let drivers: Vec<_> = g.all_nets().filter(|(_, n)| n.at.is_some() && moving.contains(&n.from.node)).map(|(k, _)| k).collect();
    for key in drivers {
        let _ = g.move_net_label(&key, None);
    }
    let size_of = |id: &NodeId| index.get(id).and_then(|&i| nodes.get(i)).map_or(plumcraft_layout::DEFAULT_SIZE, |n| n.size);
    for (f, held) in held {
        let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for id in &held {
            if let Some(n) = g.node(id) {
                let s = size_of(id);
                lo = [lo[0].min(n.pos[0]), lo[1].min(n.pos[1])];
                hi = [hi[0].max(n.pos[0] + s[0]), hi[1].max(n.pos[1] + s[1])];
            }
        }
        // (a fit that won't do, off the canvas, leaves the frame where it is)
        let _ = g.fit_frame(&f, [lo[0] - FRAME_SIDE, lo[1] - FRAME_TOP], [hi[0] - lo[0] + 2.0 * FRAME_SIDE, hi[1] - lo[1] + FRAME_TOP + FRAME_SIDE]);
    }
    let members: Vec<(plumcraft_graph::GroupId, BTreeSet<NodeId>)> = g.groups.keys().map(|gid| (gid.clone(), g.group_members(gid))).collect();
    for (gid, ms) in members {
        if !ms.is_empty() && ms.iter().all(|n| moving.contains(n)) {
            let top_left = corner(&mut ms.iter().filter_map(|n| g.node(n)).map(|n| n.pos));
            if let Some(group) = g.groups.get_mut(&gid)
                && top_left[0].is_finite()
            {
                group.pos = top_left;
            }
        }
    }
    (chosen, layout)
}

fn graph_tidy(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = if p.get("nodes").is_some_and(|v| !v.is_null()) { node_list_param(p, "nodes")? } else { Vec::new() };
    let sizes = sizes_param(p)?;
    let pins = pins_param(p)?;
    let (chosen, layout) = s.edit("Tidy", |g, catalog| Ok(tidy(g, catalog, &ids, &sizes, &pins)))?;
    let columns = layout.column.iter().max().map_or(0, |c| c + 1);
    Ok(json!({"nodes": chosen.len(), "columns": columns, "rows": layout.rows}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "graph.tidy",
        label: "Tidy",
        menu: &["Edit"],
        shortcut: Some("Cmd+Shift+L"),
        params: "{nodes?: [node id] (default: all), sizes?: {node id: [w, h]}, pins?: {node id: {inputs: {port: y}, outputs: {port: y}}} (measured, y from the node's top; missing ones are estimated)} -> {nodes, columns, rows} (lays them out left to right, packed to the top, parameters beside the pins they feed, long chains wrapped where the fewest wires cross; their top-left corner stays; one undo step)",
        enabled: always,
        run: graph_tidy,
        journal: true,
    }]
}

#[cfg(test)]
mod tests {
    use plumcraft_graph::{Graph, NodeId};
    use serde_json::json;

    use crate::Session;

    fn demo() -> Session {
        let graph = crate::file_cmds::parse_workflow(plumcraft_text::DEMO_WORKFLOW).unwrap();
        Session::with_graph(plumcraft_text::engine().schema(), graph)
    }

    fn x(s: &Session, id: &str) -> f32 {
        s.graph().nodes[&NodeId::from(id)].pos[0]
    }

    fn corner(g: &Graph) -> [f32; 2] {
        g.nodes.values().fold([f32::INFINITY; 2], |a, n| [a[0].min(n.pos[0]), a[1].min(n.pos[1])])
    }

    #[test]
    fn the_demo_tidies_left_to_right_and_undo_puts_it_back() {
        let mut s = demo();
        let before = s.graph().clone();
        let r = s.execute("graph.tidy", &json!({})).unwrap();
        assert_eq!(r["nodes"], 7);
        assert!(x(&s, "think") < x(&s, "swap") && x(&s, "swap") < x(&s, "shout"));
        assert!(x(&s, "broken") < x(&s, "after"));
        assert!(x(&s, "story") < x(&s, "think"), "the literal is in a lane before what it feeds");
        assert_eq!(corner(s.graph()), corner(&before));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.graph().nodes, before.nodes);
    }

    #[test]
    fn tags_get_room_and_go_back_to_their_pins() {
        let mut s = demo();
        let (from, to) = s.graph().wires.iter().map(|w| (w.from.clone(), w.to.clone())).next().unwrap();
        s.execute("output.label", &json!({"from": from, "name": "story"})).unwrap();
        s.execute("wire.labelAt", &json!({"to": to, "at": [-300, 40]})).unwrap();
        s.execute("net.move", &json!({"name": "story", "at": [200, 0]})).unwrap();
        s.execute("graph.tidy", &json!({})).unwrap();
        let g = s.graph().clone();
        assert!(g.wires.iter().all(|w| w.label_at.is_none()), "tags back beside their pins");
        assert_eq!(g.nets["story"].at, None);
        s.execute("graph.tidy", &json!({})).unwrap();
        assert_eq!(s.graph().nodes, g.nodes, "a labelled tidy graph stays put");
        assert_eq!(s.graph().wires, g.wires);
    }

    #[test]
    fn tidying_a_tidy_graph_moves_nothing() {
        let mut s = demo();
        s.execute("graph.tidy", &json!({})).unwrap();
        let once = s.graph().clone();
        s.execute("graph.tidy", &json!({})).unwrap();
        assert_eq!(s.graph().nodes, once.nodes);
    }

    #[test]
    fn measured_sizes_and_pins_are_used() {
        let mut s = demo();
        let big = [500.0, 300.0];
        s.execute(
            "graph.tidy",
            &json!({"sizes": {"think": big, "swap": "junk"}, "pins": {"think": {"inputs": {"text": 250.0}, "outputs": {"text": 20.0}}, "x": 3}}),
        )
        .unwrap();
        // the next column starts past the 500-wide node
        assert!(x(&s, "swap") >= x(&s, "think") + big[0], "{} vs {}", x(&s, "swap"), x(&s, "think") + big[0]);
    }

    #[test]
    fn a_frame_holding_what_moved_is_refitted_and_another_stays() {
        let mut s = demo();
        let ids: Vec<NodeId> = s.graph().nodes.keys().cloned().collect();
        // around everything, with a mess of a size: Tidy moves all of it, so the frame follows
        let around =
            s.execute("frame.add", &json!({"pos": [-5000, -5000], "size": [20000, 20000], "title": "all"})).unwrap()["id"].as_str().unwrap().to_owned();
        // around nothing: left as it is
        let empty = s.execute("frame.add", &json!({"pos": [-9000, -9000], "size": [100, 100]})).unwrap()["id"].as_str().unwrap().to_owned();
        s.execute("graph.tidy", &json!({"sizes": {}})).unwrap();
        let f = |s: &Session, id: &str| s.graph().frame(&plumcraft_graph::FrameId::from(id)).unwrap().clone();
        let fitted = f(&s, &around);
        assert!(fitted.size[0] < 20000.0 && fitted.size[1] < 20000.0, "{fitted:?}");
        assert_eq!(fitted.title, "all");
        assert!(ids.iter().all(|id| fitted.contains(s.graph().nodes[id].pos)), "it still holds every node");
        let corner = corner(s.graph());
        assert_eq!(fitted.pos, [corner[0] - super::FRAME_SIDE, corner[1] - super::FRAME_TOP]);
        assert_eq!(f(&s, &empty).pos, [-9000.0, -9000.0]);
        // tidying again changes nothing, frames included
        let once = s.graph().clone();
        s.execute("graph.tidy", &json!({})).unwrap();
        assert_eq!(s.graph().frames, once.frames);
        // undone, the frame is back too
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(f(&s, &around).size, [20000.0, 20000.0]);
        // a frame holding a node a selection's Tidy leaves alone stays
        let two = [ids[0].clone(), ids[1].clone()];
        s.execute("graph.tidy", &json!({"nodes": two})).unwrap();
        assert_eq!(f(&s, &around).size, [20000.0, 20000.0]);
    }

    #[test]
    fn a_group_moves_with_its_members() {
        let mut s = demo();
        s.execute("group.create", &json!({"nodes": ["shout", "count"], "name": "ends"})).unwrap();
        s.execute("graph.tidy", &json!({})).unwrap();
        let g = s.graph();
        let (_, group) = g.groups.iter().next().unwrap();
        let top_left = group.nodes.iter().map(|n| g.nodes[n].pos).fold([f32::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
        assert_eq!(group.pos, top_left);
    }

    #[test]
    fn tidy_rejects_junk_params_and_skips_unknown_ids() {
        let mut s = demo();
        assert!(s.execute("graph.tidy", &json!({"sizes": [1, 2]})).is_err());
        assert!(s.execute("graph.tidy", &json!({"pins": "no"})).is_err());
        assert!(s.execute("graph.tidy", &json!({"nodes": "story"})).is_err());
        let r = s.execute("graph.tidy", &json!({"nodes": ["nope", "story", "story"]})).unwrap();
        assert_eq!(r["nodes"], 1);
        // hostile numbers are clamped, never a panic
        let r = s.execute("graph.tidy", &json!({"sizes": {"story": [1e30, -5]}, "pins": {"story": {"inputs": {"text": -1e30}}}})).unwrap();
        assert_eq!(r["nodes"], 7);
    }
}
