//! Names edited in place (N4 slice 3): a node's title, a group's or a frame's name, a net's, a
//! named port's, a wire's slice, the net label for every use of an output, a promoted
//! parameter's label. Before, each was a menu item the shell did itself (`rename.node`,
//! `rename.frame`, …, `Item::local`), which golden rule 4 doesn't allow ("No exceptions to the
//! rule", the maintainer). Now `name.edit {target}` opens the editor, the core's state, published
//! as the scene's `view.name` with where the text goes (`rect`, graph units: the face's title,
//! the frame's tab, the group's band, the net's tag, the input's row, the wire's middle), and
//! `name.set {text}` commits it through the command that owns the name, one undo step; Escape
//! is `name.edit {open: false}`. A shell puts one native text field over `rect`, as it does for
//! a row (`row.edit`, 12.10), so typing is the platform's.

use plumcraft_graph::{FrameId, GroupId, NetKey, NodeId, PortRef};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::control::check_fields;
use crate::selection::shown_id;

/// What is named.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum NameTarget {
    /// A node's title (`node.title`; blank goes back to its type's name).
    Node { node: String },
    /// A group's name (`group.rename`).
    Group { group: String },
    /// A frame's title (`frame.set {title}`).
    Frame { frame: String },
    /// A net's name (`net.rename`).
    Net {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
    },
    /// An input made one of the graph's parameters (`graph.port.name`), or a named port's name
    /// (`graph.port.rename`).
    Port { node: String, port: String },
    /// A wire's slice (`wire.slice`; blank clears it).
    Slice { node: String, port: String },
    /// Every wire from an output drawn through one net (`output.label`).
    Label { node: String, port: String },
    /// A promoted parameter's label (`group.editPromoted {newLabel}`).
    Promoted { group: String, label: String },
}

/// The name being edited: the scene's `view.name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NameEdit {
    pub target: NameTarget,
    /// What the field starts with.
    pub text: String,
    /// Shown greyed in an empty field (a node's type name).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hint: String,
    /// What is being named, for the field's accessibility label ("Frame title").
    pub label: String,
    /// Where the field goes, graph units `[x0, y0, x1, y1]` (the shell keeps it at least legible,
    /// as a row's editor).
    pub rect: [f64; 4],
}

fn s<'a>(v: &'a Value, k: &str) -> Result<&'a str, String> {
    v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 512).ok_or_else(|| format!("{k} must be a non-empty string"))
}

/// Read `name.edit`'s target: `{node}`, `{group}`, `{frame}`, `{net, group?}`, `{port: {node,
/// port}}`, `{slice: {node, port}}` (the wire into it), `{label: {node, port}}` (an output),
/// `{promoted: {group, label}}`.
pub fn parse_target(p: &Value) -> Result<NameTarget, String> {
    let port_of = |v: &Value| -> Result<(String, String), String> {
        check_fields(v, &["node", "port"])?;
        Ok((s(v, "node")?.to_owned(), s(v, "port")?.to_owned()))
    };
    if let Some(v) = p.get("port") {
        let (node, port) = port_of(v)?;
        return Ok(NameTarget::Port { node, port });
    }
    if let Some(v) = p.get("slice") {
        let (node, port) = port_of(v)?;
        return Ok(NameTarget::Slice { node, port });
    }
    if let Some(v) = p.get("label") {
        let (node, port) = port_of(v)?;
        return Ok(NameTarget::Label { node, port });
    }
    if let Some(v) = p.get("promoted") {
        check_fields(v, &["group", "label"])?;
        return Ok(NameTarget::Promoted { group: s(v, "group")?.to_owned(), label: s(v, "label")?.to_owned() });
    }
    if p.get("net").is_some() {
        let group = match p.get("group") {
            None | Some(Value::Null) => None,
            Some(_) => Some(s(p, "group")?.to_owned()),
        };
        return Ok(NameTarget::Net { name: s(p, "net")?.to_owned(), group });
    }
    if p.get("node").is_some() {
        return Ok(NameTarget::Node { node: s(p, "node")?.to_owned() });
    }
    if p.get("frame").is_some() {
        return Ok(NameTarget::Frame { frame: s(p, "frame")?.to_owned() });
    }
    if p.get("group").is_some() {
        return Ok(NameTarget::Group { group: s(p, "group")?.to_owned() });
    }
    Err("name what to edit: node, group, frame, net (with group?), port, slice, label or promoted".into())
}

fn rect_of(r: crate::plain::Rect) -> [f64; 4] {
    crate::geometry::rect_json(r)
}

/// The editor for `target`, checked against the document: what it starts with and where it goes.
pub fn open(d: &crate::document::Document, target: NameTarget) -> Result<NameEdit, String> {
    use crate::plain::{Point, Rect};
    let session = &d.session;
    let graph = session.graph();
    let catalog = session.catalog();
    let snap = &d.snapshot;
    let face_rect = |id: &str| snap.face_by_id(id).map(|(_, r)| r);
    // a face's title: its measured title part, else the header's width less the icon and menu
    let title_rect = |id: &str| -> Option<Rect> {
        let (nid, r) = snap.face_by_id(id)?;
        if let Some(t) = snap.part_rect(nid, crate::geometry::part::TITLE) {
            return Some(Rect::from_min_max(t.min, Point::new(t.max.x.max(t.min.x + 120.0).min(r.max.x - 4.0), t.max.y)));
        }
        let h = snap.face_header_of(nid).unwrap_or(28.0);
        Some(Rect::from_min_max(Point::new(r.min.x + 28.0, r.min.y + 3.0), Point::new((r.max.x - 28.0).max(r.min.x + 60.0), r.min.y + h - 3.0)))
    };
    let row_rect = |node: &str, port: &str| -> Option<Rect> {
        let face = crate::scene::faces(&d.scene_inputs(&crate::theme::DEFAULT, None, &std::collections::HashMap::new(), &[]));
        let (fi, k) = crate::inline::find(&face, node, port)?;
        let f = face.get(fi)?;
        let (nid, r) = snap.face_by_id(&f.id)?;
        snap.row_rect(nid, i32::try_from(k).ok()?).or_else(|| {
            let y = snap.pin_at(nid, false, u32::try_from(k).ok()?)?.y;
            Some(Rect::from_min_max(Point::new(r.min.x + 8.0, y - 10.0), Point::new(r.max.x - 8.0, y + 10.0)))
        })
    };
    let fallback = || Rect::from_min_max(Point::new(0.0, 0.0), Point::new(200.0, 22.0));
    let edit = match &target {
        NameTarget::Node { node } => {
            let id = NodeId::from(node.as_str());
            let n = graph.node(&id).ok_or_else(|| format!("no node {}", shown_id(node)))?;
            let hint = plumcraft_session::display_name(catalog, &n.kind);
            NameEdit {
                text: n.title.clone().unwrap_or_default(),
                hint,
                label: "Node title".into(),
                rect: rect_of(title_rect(node).unwrap_or_else(fallback)),
                target,
            }
        }
        NameTarget::Group { group } => {
            let g = graph.group(&GroupId::from(group.as_str())).ok_or_else(|| format!("no group {}", shown_id(group)))?;
            let r = snap
                .group_rect_by_id(group)
                .map(|r| Rect::from_min_max(Point::new(r.min.x + 24.0, r.min.y + 2.0), Point::new((r.max.x - 8.0).max(r.min.x + 140.0), r.min.y + 26.0)))
                .or_else(|| title_rect(&format!("group:{group}")))
                .unwrap_or_else(fallback);
            NameEdit { text: g.name.clone(), hint: String::new(), label: "Group name".into(), rect: rect_of(r), target }
        }
        NameTarget::Frame { frame } => {
            let f = graph.frames.get(&FrameId(frame.clone())).ok_or_else(|| format!("no frame {}", shown_id(frame)))?;
            let r = snap
                .frame_by_id(frame)
                .map(|(_, tab, _)| Rect::from_min_max(Point::new(tab.min.x + 18.0, tab.min.y), Point::new(tab.max.x.max(tab.min.x + 160.0), tab.max.y)))
                .unwrap_or_else(fallback);
            NameEdit { text: f.title.clone(), hint: String::new(), label: "Frame title".into(), rect: rect_of(r), target }
        }
        NameTarget::Net { name, group } => {
            let key = NetKey { group: group.as_deref().map(GroupId::from), name: name.clone() };
            if graph.net(&key).is_none() {
                return Err(format!("no net {}", shown_id(name)));
            }
            let r = snap
                .driver_tag_rect(name, group.as_deref())
                .map(|r| Rect::from_min_max(r.min, Point::new(r.max.x.max(r.min.x + 140.0), r.max.y)))
                .unwrap_or_else(fallback);
            NameEdit { text: name.clone(), hint: String::new(), label: "Net name".into(), rect: rect_of(r), target }
        }
        NameTarget::Port { node, port } => {
            let at = PortRef::new(node.as_str(), port.as_str());
            if graph.node(&at.node).is_none() {
                return Err(format!("no node {}", shown_id(node)));
            }
            let text = graph.name_of_port(&at).map(str::to_owned).unwrap_or_default();
            if text.is_empty() && graph.wire_into(&at).is_some() {
                return Err("a wired input can't be named".into());
            }
            let r = row_rect(node, port).unwrap_or_else(|| face_rect(node).unwrap_or_else(fallback));
            NameEdit { text, hint: port.clone(), label: "Port name".into(), rect: rect_of(r), target }
        }
        NameTarget::Slice { node, port } => {
            let at = PortRef::new(node.as_str(), port.as_str());
            let w = graph.wire_into(&at).ok_or_else(|| format!("no wire into {}.{}", shown_id(node), shown_id(port)))?;
            let id = format!("{node}.{port}");
            let mid = snap.wire_by_id(&id).and_then(|w| crate::wire_path::chain_middle(&w.chain));
            let r = mid.map_or_else(fallback, |m| Rect::from_min_max(Point::new(m.x - 60.0, m.y - 11.0), Point::new(m.x + 60.0, m.y + 11.0)));
            NameEdit { text: w.slice.clone().unwrap_or_default(), hint: "1:-1".into(), label: "Slice".into(), rect: rect_of(r), target }
        }
        NameTarget::Label { node, port } => {
            let id = NodeId::from(node.as_str());
            let n = graph.node(&id).ok_or_else(|| format!("no node {}", shown_id(node)))?;
            if catalog.node(&n.kind).and_then(|d| d.output(port)).is_none() {
                return Err(format!("{} has no output {}", shown_id(node), shown_id(port)));
            }
            let r = face_rect(node).and_then(|r| {
                let (nid, _) = snap.face_by_id(node)?;
                let k = (0..snap.pin_count(nid, true)).find(|k| snap.pin_port(nid, true, *k as u32).is_some_and(|p| p.port == *port))?;
                let p = snap.pin_at(nid, true, k as u32)?;
                Some(Rect::from_min_max(Point::new(r.max.x + 8.0, p.y - 11.0), Point::new(r.max.x + 168.0, p.y + 11.0)))
            });
            NameEdit { text: port.clone(), hint: String::new(), label: "Net name".into(), rect: rect_of(r.unwrap_or_else(fallback)), target }
        }
        NameTarget::Promoted { group, label } => {
            let g = graph.group(&GroupId::from(group.as_str())).ok_or_else(|| format!("no group {}", shown_id(group)))?;
            if !g.promoted.iter().any(|p| p.label == *label) {
                return Err(format!("{} has no parameter {}", shown_id(group), shown_id(label)));
            }
            let r = snap.group_rect_by_id(group).or_else(|| face_rect(&format!("group:{group}"))).unwrap_or_else(fallback);
            let r = Rect::from_min_max(Point::new(r.min.x + 24.0, r.min.y + 2.0), Point::new(r.min.x + 220.0, r.min.y + 26.0));
            NameEdit { text: label.clone(), hint: String::new(), label: "Parameter label".into(), rect: rect_of(r), target }
        }
    };
    Ok(edit)
}

/// The command `name.set {text}` runs for `target`, and its params; none when nothing changes.
pub fn commit(d: &crate::document::Document, target: &NameTarget, text: &str) -> Result<Option<(&'static str, Value)>, String> {
    let graph = d.session.graph();
    let t = text.trim();
    Ok(match target {
        NameTarget::Node { node } => {
            let n = graph.node(&NodeId::from(node.as_str())).ok_or_else(|| format!("no node {}", shown_id(node)))?;
            (n.title.as_deref().unwrap_or("") != t).then(|| ("node.title", json!({"node": node, "title": t})))
        }
        NameTarget::Group { group } => {
            let g = graph.group(&GroupId::from(group.as_str())).ok_or_else(|| format!("no group {}", shown_id(group)))?;
            if t.is_empty() {
                return Err("a group needs a name".into());
            }
            (g.name != t).then(|| ("group.rename", json!({"group": group, "name": t})))
        }
        NameTarget::Frame { frame } => {
            let f = graph.frames.get(&FrameId(frame.clone())).ok_or_else(|| format!("no frame {}", shown_id(frame)))?;
            (f.title != t).then(|| ("frame.set", json!({"frame": frame, "title": t})))
        }
        NameTarget::Net { name, group } => {
            if t.is_empty() {
                return Err("a net needs a name".into());
            }
            (name != t).then(|| ("net.rename", json!({"name": name, "group": group, "to": t})))
        }
        NameTarget::Port { node, port } => {
            let at = PortRef::new(node.as_str(), port.as_str());
            match graph.name_of_port(&at) {
                Some(old) if t.is_empty() => Some(("graph.port.unname", json!({"name": old}))),
                Some(old) => (old != t).then(|| ("graph.port.rename", json!({"name": old, "to": t}))),
                None if t.is_empty() => None,
                None => Some(("graph.port.name", json!({"port": {"node": node, "port": port}, "name": t}))),
            }
        }
        NameTarget::Slice { node, port } => {
            let at = PortRef::new(node.as_str(), port.as_str());
            let w = graph.wire_into(&at).ok_or_else(|| format!("no wire into {}.{}", shown_id(node), shown_id(port)))?;
            let now = w.slice.as_deref().unwrap_or("");
            (now != t).then(|| ("wire.slice", json!({"to": {"node": node, "port": port}, "slice": if t.is_empty() { Value::Null } else { json!(t) }})))
        }
        NameTarget::Label { node, port } => {
            if t.is_empty() {
                return Err("a net needs a name".into());
            }
            Some(("output.label", json!({"from": {"node": node, "port": port}, "name": t})))
        }
        NameTarget::Promoted { group, label } => {
            // empty: back to the default, the inner port's own name (FR5)
            if t.is_empty() {
                let g = graph.group(&GroupId::from(group.as_str())).ok_or_else(|| format!("no group {}", shown_id(group)))?;
                let p = g.promoted.iter().find(|p| p.label == *label).ok_or_else(|| format!("no parameter {label} in {}", g.name))?;
                return Ok((label != &plumcraft_graph::default_promoted_label(&p.port))
                    .then(|| ("group.editPromoted", json!({"group": group, "label": label, "newLabel": ""}))));
            }
            (label != t).then(|| ("group.editPromoted", json!({"group": group, "label": label, "newLabel": t})))
        }
    })
}
