//! The canvas's geometry in the core (N2 slice 3; `docs/shell-interface.md` 8.11 and section 9;
//! `docs/research/24-native-shells.md` 5.2, 5.3).
//!
//! **Who measures:** the shell. A face's size depends on native text and controls, so the shell
//! lays each face out from its rows and reports what it measured with `geometry.set`
//! ([`parse_set`]): the face's size, its pins' heights from its top (inputs on its left edge,
//! outputs on its right), and optionally its header's height, its input rows' widget rects and
//! its header's parts. The core owns positions (the graph's) and keeps the sizes it was told
//! ([`Measured`]); a face it hasn't been told of is [`DEFAULT_SIZE`] with its pins estimated.
//!
//! **What the core works out from them:** wire paths ([`crate::wire_path`], published in the
//! scene), open groups' frames ([`open_group_rects`]), marks' places, the fit rect
//! ([`fit_rect`]), Align, Distribute and Frame Selection's sizes ([`Measured::sizes_for`]), and
//! the [`Snapshot`] the hot calls read: hit testing ([`Snapshot::hit`]), the wire target
//! ([`Snapshot::wire_target`]) and wire paths for faces being dragged ([`Snapshot::wire_paths`]).
//!
//! Everything is in graph units. Everything a shell sends is hostile input: numbers are checked
//! or clamped ([`crate::wire_path::coord`]), sizes are never negative, lists are bounded, and no
//! query panics on zero sizes, overlapping faces, NaN or huge coordinates.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use plumcraft_graph::{Graph, GroupId, NodeId, PortRef};
use plumcraft_proto::catalogue::SchemaReply;
use serde_json::{Value, json};

use crate::control::check_fields;
use crate::plain::{Point, Rect};
use crate::wire_path::{self, coord, tame};

/// A face the shell hasn't measured (`docs/shell-interface.md` 8.11; the egui canvas's guess).
pub const DEFAULT_SIZE: [f32; 2] = [220.0, 160.0];
/// A face's header, when the shell didn't say (the pins below it are estimated from it).
pub const DEFAULT_HEADER: f32 = 28.0;
/// An unmeasured face's row height, for estimating its pins.
pub const DEFAULT_ROW: f32 = 22.0;
/// What the fit rect leaves around the faces (the egui canvas's `fit_transform`).
pub const FIT_MARGIN: f32 = 32.0;
/// The space an open group's frame leaves around its members, and its header band's height
/// (the egui canvas's `groups_view`).
pub const GROUP_PAD: f32 = 14.0;
pub const GROUP_HEADER: f32 = 28.0;
/// A frame's tab, above its top edge, estimated (its title's width is the shell's): the most
/// it is wide, and its height.
pub const FRAME_TAB: [f32; 2] = [180.0, 22.0];
/// A frame's resize handle, inside its bottom right corner.
pub const FRAME_HANDLE: f32 = 16.0;
/// A bend point's handle radius (the egui canvas's `wire_route::HANDLE`).
pub const BEND_RADIUS: f32 = 5.0;
/// A net tag, estimated: a character's width (11-point monospace), the padding and point, and
/// its height.
pub const TAG_CHAR: f32 = 6.6;
pub const TAG_PAD: f32 = 17.0;
pub const TAG_HEIGHT: f32 = 17.0;
/// The largest size a face may have (graph units).
pub const MAX_SIZE: f32 = 1.0e6;
/// The most pins, rows or parts a face may report, and faces a `geometry.set`.
pub const MAX_PINS: usize = 4096;
pub const MAX_FACES: usize = 100_000;
/// The largest hit distance a query takes.
pub const MAX_SLOP: f32 = 1.0e4;

/// What a hit is (`plum_hit.kind`, section 9.2).
pub mod kind {
    pub const NONE: u32 = 0;
    pub const NODE: u32 = 1;
    pub const PIN: u32 = 2;
    pub const WIRE: u32 = 3;
    pub const BEND: u32 = 4;
    pub const TAG: u32 = 5;
    pub const FRAME: u32 = 6;
    pub const GROUP: u32 = 7;
}

/// Which part of a face (kind 1), a pin's side (kind 2), a tag (kind 5), a frame (kind 6) or a
/// group (kind 7) a hit is (`plum_hit.part`).
pub mod part {
    pub const BODY: u32 = 1;
    pub const HEADER: u32 = 2;
    pub const TITLE: u32 = 3;
    pub const CHEVRON: u32 = 4;
    pub const ICON: u32 = 5;
    pub const MENU: u32 = 6;
    pub const ENTER: u32 = 7;
    pub const ROW: u32 = 8;
    pub const PREVIEW: u32 = 9;
    pub const FOOTER: u32 = 10;
    /// A pin's side.
    pub const INPUT: u32 = 1;
    pub const OUTPUT: u32 = 2;
    /// A net tag: its driver's, or a use's.
    pub const DRIVER: u32 = 1;
    pub const USE: u32 = 2;
    /// A frame's.
    pub const FRAME_BODY: u32 = 1;
    pub const FRAME_TAB: u32 = 2;
    pub const FRAME_HANDLE: u32 = 3;
    /// An open group's header band.
    pub const BAND: u32 = 1;
}

/// The names of a face's parts in `geometry.set` (`parts`), and their codes.
const PART_NAMES: [(&str, u32); 6] =
    [("title", part::TITLE), ("chevron", part::CHEVRON), ("icon", part::ICON), ("menu", part::MENU), ("enter", part::ENTER), ("preview", part::PREVIEW)];

/// A named part of a face, in face coordinates (`[x0, y0, x1, y1]` from its top left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Part {
    pub part: u32,
    /// A footer item's index; -1 for the others.
    pub index: i32,
    pub rect: [f32; 4],
}

/// What the shell measured of one face (graph units, at zoom 1).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaceGeometry {
    pub size: [f32; 2],
    /// The header's height (what is header and what is body); none: [`DEFAULT_HEADER`].
    pub header: Option<f32>,
    /// Each input pin's height from the face's top, in row order; none for one not measured.
    pub inputs: Vec<Option<f32>>,
    pub outputs: Vec<Option<f32>>,
    /// Each input row's widget rect, face coordinates; none for a row without one.
    pub rows: Vec<Option<[f32; 4]>>,
    pub parts: Vec<Part>,
    /// Where the face's top left is drawn, from its position (N2 slice 4): `[0, 0]` for a shell
    /// that draws a face at its position; egui-snarl draws its frame's margin around it.
    pub offset: [f32; 2],
}

impl FaceGeometry {
    /// A face measured as `size` with its pins at `inputs` and `outputs` (tests, the egui shell).
    pub fn sized(size: [f32; 2], inputs: &[f32], outputs: &[f32]) -> FaceGeometry {
        FaceGeometry {
            size: size.map(size_value),
            inputs: inputs.iter().map(|y| Some(coord(*y))).collect(),
            outputs: outputs.iter().map(|y| Some(coord(*y))).collect(),
            ..FaceGeometry::default()
        }
    }
}

/// A size the core keeps: finite, at least 0, at most [`MAX_SIZE`].
fn size_value(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, MAX_SIZE) }
}

/// The faces the shell measured, by face id (a node's id; a collapsed group's `group:<id>`).
#[derive(Debug, Clone, Default)]
pub struct Measured {
    faces: HashMap<String, FaceGeometry>,
    generation: u64,
}

impl Measured {
    /// Rises with every change (what the scene and the snapshot are rebuilt on).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn get(&self, face: &str) -> Option<&FaceGeometry> {
        self.faces.get(face)
    }

    pub fn len(&self) -> usize {
        self.faces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// Keep what face `id` measured; did anything change?
    pub fn set(&mut self, id: &str, g: FaceGeometry) -> bool {
        let mut g = g;
        // a report with no parts (the typed `plum_geometry_set` carries none) keeps the parts a
        // `geometry.parts` gave for the same layout (N4 slice 2)
        if g.parts.is_empty()
            && let Some(old) = self.faces.get(id)
            && old.size == g.size
            && old.header == g.header
        {
            g.parts.clone_from(&old.parts);
        }
        if self.faces.get(id) == Some(&g) {
            return false;
        }
        self.faces.insert(id.to_owned(), g);
        self.generation = self.generation.wrapping_add(1);
        true
    }

    /// Face `id`'s parts (`geometry.parts`, N4 slice 2): kept with what it measured; none for a
    /// face not measured yet. Did anything change?
    pub fn set_parts(&mut self, id: &str, parts: Vec<Part>) -> Option<bool> {
        let g = self.faces.get_mut(id)?;
        if g.parts == parts {
            return Some(false);
        }
        g.parts = parts;
        self.generation = self.generation.wrapping_add(1);
        Some(true)
    }

    /// Forget the faces `keep` says no to (gone from the scene).
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        let before = self.faces.len();
        self.faces.retain(|k, _| keep(k));
        if self.faces.len() != before {
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Forget everything (a new graph).
    pub fn clear(&mut self) {
        if !self.faces.is_empty() {
            self.faces.clear();
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// The size face `id` has for the core: measured, or [`DEFAULT_SIZE`].
    pub fn size_of(&self, id: &str) -> [f32; 2] {
        self.faces.get(id).map_or(DEFAULT_SIZE, |g| g.size)
    }

    /// What Align, Distribute and Frame Selection measure (`sizes`, by node id and a collapsed
    /// group's id): the faces of `nodes` and of `groups`' proxies that the shell measured (the
    /// session estimates the others, as it does for a script).
    pub fn sizes_for(&self, nodes: &[NodeId], groups: &[GroupId]) -> BTreeMap<String, [f32; 2]> {
        let mut out = BTreeMap::new();
        for n in nodes {
            if let Some(g) = self.faces.get(&n.0) {
                out.insert(n.0.clone(), g.size);
            }
        }
        for gid in groups {
            if let Some(g) = self.faces.get(&crate::rows::proxy_node_id(gid).0) {
                out.insert(gid.0.clone(), g.size);
            }
        }
        out
    }

    /// Tidy's measurements (`graph.tidy`'s `sizes` and `pins`) for the faces in `faces`
    /// (nodes only: Tidy lays out nodes), pins by port key.
    pub fn for_tidy(&self, faces: &[crate::scene::Face]) -> (serde_json::Map<String, Value>, serde_json::Map<String, Value>) {
        let mut sizes = serde_json::Map::new();
        let mut pins = serde_json::Map::new();
        for f in faces.iter().filter(|f| f.proxy.is_none()) {
            let Some(g) = self.faces.get(&f.id) else { continue };
            sizes.insert(f.id.clone(), json!(g.size));
            let side = |rows: &mut dyn Iterator<Item = &str>, ys: &[Option<f32>]| {
                let mut m = serde_json::Map::new();
                for (k, y) in rows.zip(ys) {
                    if let Some(y) = y {
                        m.insert(k.to_owned(), json!(y));
                    }
                }
                m
            };
            let inputs = side(&mut f.inputs.iter().map(|r| r.key.as_str()), &g.inputs);
            let outputs = side(&mut f.outputs.iter().map(|r| r.key.as_str()), &g.outputs);
            pins.insert(f.id.clone(), json!({"inputs": inputs, "outputs": outputs}));
        }
        (sizes, pins)
    }
}

/// One face of a `geometry.set`, checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// The face's id (`node` in the params).
    pub face: String,
    /// The face's revision the shell measured (none: not said).
    pub rev: Option<u64>,
    pub geometry: FaceGeometry,
}

fn num(v: &Value, what: &str) -> Result<f32, String> {
    v.as_f64().map(|f| coord(f as f32)).ok_or_else(|| format!("{what} must be a number"))
}

fn rect_of(v: &Value, what: &str) -> Result<[f32; 4], String> {
    let a = v.as_array().filter(|a| a.len() == 4).ok_or_else(|| format!("{what} must be [x0, y0, x1, y1]"))?;
    let mut r = [0.0f32; 4];
    for (slot, x) in r.iter_mut().zip(a) {
        *slot = num(x, what)?;
    }
    Ok(r)
}

fn pins_of(v: Option<&Value>, what: &str) -> Result<Vec<Option<f32>>, String> {
    match v {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) if a.len() <= MAX_PINS => a.iter().map(|y| if y.is_null() { Ok(None) } else { num(y, what).map(Some) }).collect(),
        Some(Value::Array(_)) => Err(format!("{what} has more than {MAX_PINS} pins")),
        Some(_) => Err(format!("{what} must be a list of heights (null for a pin not measured)")),
    }
}

/// `geometry.set`'s params: `{faces: [{node, rev?, size: [w, h], pins?: {in?: [y | null],
/// out?: [y | null]}, header?, rows?: [[x0, y0, x1, y1] | null], parts?: {title?, chevron?,
/// icon?, menu?, enter?, preview?: rect, footer?: [rect]}}]}`. Pins and rows are in row order
/// (what a wire's `fromPin`/`toPin` index), since a collapsed group's rows can share a key.
/// Unknown fields are refused; numbers are clamped to what a chain keeps, sizes to
/// `0..=MAX_SIZE`.
pub fn parse_set(params: &Value) -> Result<Vec<Report>, String> {
    check_fields(params, &["faces"])?;
    let faces = params.get("faces").and_then(Value::as_array).ok_or("faces must be a list of faces")?;
    if faces.len() > MAX_FACES {
        return Err(format!("at most {MAX_FACES} faces a call"));
    }
    let mut out = Vec::with_capacity(faces.len());
    for (k, f) in faces.iter().enumerate() {
        let at = |what: &str| format!("faces[{k}].{what}");
        check_fields(f, &["node", "rev", "size", "pins", "header", "rows", "parts", "offset"]).map_err(|e| format!("faces[{k}]: {e}"))?;
        let face = f.get("node").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 512).ok_or_else(|| at("node must be a face's id"))?;
        let rev = match f.get("rev") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| at("rev must be the face's rev (a whole number)"))?),
        };
        let size = f.get("size").and_then(Value::as_array).filter(|a| a.len() == 2).ok_or_else(|| at("size must be [w, h]"))?;
        let size = [size_value(num(&size[0], &at("size"))?), size_value(num(&size[1], &at("size"))?)];
        let (inputs, outputs) = match f.get("pins") {
            None | Some(Value::Null) => (Vec::new(), Vec::new()),
            Some(p) => {
                check_fields(p, &["in", "out"]).map_err(|e| at(&format!("pins: {e}")))?;
                (pins_of(p.get("in"), &at("pins.in"))?, pins_of(p.get("out"), &at("pins.out"))?)
            }
        };
        let header = match f.get("header") {
            None | Some(Value::Null) => None,
            Some(v) => Some(size_value(num(v, &at("header"))?)),
        };
        let rows = match f.get("rows") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) if a.len() <= MAX_PINS => {
                a.iter().map(|r| if r.is_null() { Ok(None) } else { rect_of(r, &at("rows[]")).map(Some) }).collect::<Result<_, _>>()?
            }
            Some(_) => return Err(at(&format!("rows must be a list of at most {MAX_PINS} rects (null for a row without a widget)"))),
        };
        let offset = match f.get("offset") {
            None | Some(Value::Null) => [0.0; 2],
            Some(v) => {
                let a = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| at("offset must be [dx, dy]"))?;
                let n = |v: &Value| num(v, &at("offset")).map(|f| f.clamp(-1.0e4, 1.0e4));
                [n(&a[0])?, n(&a[1])?]
            }
        };
        let parts = parts_of(f.get("parts"), &at)?;
        out.push(Report { face: face.to_owned(), rev, geometry: FaceGeometry { size, header, inputs, outputs, rows, parts, offset } });
    }
    Ok(out)
}

/// A face's `parts` in `geometry.set` or `geometry.parts`.
fn parts_of(v: Option<&Value>, at: &dyn Fn(&str) -> String) -> Result<Vec<Part>, String> {
    let mut parts = Vec::new();
    let Some(p) = v.filter(|p| !p.is_null()) else { return Ok(parts) };
    let names: Vec<&str> = PART_NAMES.iter().map(|(n, _)| *n).chain(["footer"]).collect();
    check_fields(p, &names).map_err(|e| at(&format!("parts: {e}")))?;
    for (name, code) in PART_NAMES {
        if let Some(r) = p.get(name).filter(|r| !r.is_null()) {
            parts.push(Part { part: code, index: -1, rect: rect_of(r, &at(&format!("parts.{name}")))? });
        }
    }
    match p.get("footer") {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) if a.len() <= MAX_PINS => {
            for (i, r) in a.iter().enumerate() {
                if !r.is_null() {
                    parts.push(Part { part: part::FOOTER, index: i32::try_from(i).unwrap_or(i32::MAX), rect: rect_of(r, &at("parts.footer[]"))? });
                }
            }
        }
        Some(_) => return Err(at("parts.footer must be a list of rects")),
    }
    Ok(parts)
}

/// `geometry.parts`' params (N4 slice 2): `{faces: [{node, parts}]}`, the header's and footer's
/// parts of faces already measured, for a shell whose measurements go typed
/// (`plum_geometry_set` has no field for them). Unknown fields are refused.
pub fn parse_parts(params: &Value) -> Result<Vec<(String, Vec<Part>)>, String> {
    check_fields(params, &["faces"])?;
    let faces = params.get("faces").and_then(Value::as_array).ok_or("faces must be a list of faces")?;
    if faces.len() > MAX_FACES {
        return Err(format!("at most {MAX_FACES} faces a call"));
    }
    let mut out = Vec::with_capacity(faces.len());
    for (k, f) in faces.iter().enumerate() {
        let at = |what: &str| format!("faces[{k}].{what}");
        check_fields(f, &["node", "parts"]).map_err(|e| format!("faces[{k}]: {e}"))?;
        let face = f.get("node").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 512).ok_or_else(|| at("node must be a face's id"))?;
        out.push((face.to_owned(), parts_of(f.get("parts"), &at)?));
    }
    Ok(out)
}

/// A face's rect: its position (the graph's) and the size the core has for it.
pub fn face_rect(pos: [f32; 2], g: Option<&FaceGeometry>) -> Rect {
    let size = g.map_or(DEFAULT_SIZE, |g| g.size);
    let off = g.map_or([0.0; 2], |g| g.offset);
    let min = tame(Point::new(pos[0] + off[0], pos[1] + off[1]));
    Rect::from_min_max(min, tame(min + Point::new(size[0], size[1])))
}

/// Pin `index` of a face's inputs (`output` false) or outputs: on the face's left or right
/// edge, at the height measured, else estimated below the header.
pub fn pin_point(rect: Rect, g: Option<&FaceGeometry>, output: bool, index: usize) -> Point {
    let ys = g.map(|g| if output { &g.outputs } else { &g.inputs });
    let header = g.and_then(|g| g.header).unwrap_or(DEFAULT_HEADER);
    let y = ys.and_then(|ys| ys.get(index).copied().flatten()).unwrap_or(header + DEFAULT_ROW * (index as f32 + 0.5));
    tame(Point::new(if output { rect.max.x } else { rect.min.x }, rect.min.y + y))
}

/// A wire's path as a cubic chain (`crate::wire_path`): a plain wire fitted to egui-snarl's
/// curve, a routed one through its bend points; a wire drawn through a net has none (its tags
/// are drawn instead). A plain wire that runs right to left, with its two faces' rects in `ends`
/// (source, target), goes round them ([`wire_path::backward_chain`], N4 slice 2).
pub fn wire_chain(kind: &str, from: Point, to: Point, via: &[[i32; 2]], ends: Option<(Rect, Rect)>) -> Vec<Point> {
    if kind == "plain"
        && let Some((a, b)) = ends
        && let Some(c) = wire_path::backward_chain(from, to, a, b)
    {
        return c;
    }
    match kind {
        "net" => Vec::new(),
        "routed" if !via.is_empty() => {
            let mut points = Vec::with_capacity(via.len() + 2);
            points.push(from);
            points.extend(via.iter().map(wire_path::as_point));
            points.push(to);
            wire_path::routed_chain(&points)
        }
        _ => wire_path::plain_chain(from, to),
    }
}

/// A coordinate as the scene carries it: to a thousandth of a graph unit, as an `f64` whose
/// JSON reads back exactly (an `f32` widened prints 17 digits, which serde_json's default
/// parser can read back one bit off).
pub fn round(v: f32) -> f64 {
    (f64::from(coord(v)) * 1000.0).round() / 1000.0
}

/// A chain as the scene carries it.
pub fn chain_json(chain: &[Point]) -> Vec<[f64; 2]> {
    chain.iter().map(|p| [round(p.x), round(p.y)]).collect()
}

fn union(a: Option<Rect>, b: Rect) -> Rect {
    match a {
        None => b,
        Some(a) => Rect::from_min_max(Point::new(a.min.x.min(b.min.x), a.min.y.min(b.min.y)), Point::new(a.max.x.max(b.max.x), a.max.y.max(b.max.y))),
    }
}

fn depth_of(graph: &Graph, gid: &GroupId) -> usize {
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

/// Each open group's frame (graph units), as the egui canvas draws them: around its members'
/// and its nested collapsed groups' faces, and the frames of the open groups nested in it,
/// [`GROUP_PAD`] out and a [`GROUP_HEADER`] band on top. Collapsed groups, the group entered and
/// groups with no face shown have none. `faces` are the shown faces' rects by face id.
pub fn open_group_rects(graph: &Graph, focus: Option<&GroupId>, faces: &HashMap<&str, Rect>) -> BTreeMap<GroupId, Rect> {
    let mut open: Vec<(usize, &GroupId, Rect)> = Vec::new();
    for (gid, grp) in &graph.groups {
        if grp.collapsed || focus == Some(gid) {
            continue;
        }
        let members = graph.group_members(gid);
        let proxies: Vec<String> = graph.group_subtree(gid).iter().map(|g| crate::rows::proxy_node_id(g).0).collect();
        let bb =
            members.iter().map(|m| m.0.as_str()).chain(proxies.iter().map(String::as_str)).filter_map(|m| faces.get(m)).fold(None, |bb, r| Some(union(bb, *r)));
        if let Some(bb) = bb.filter(|b| b.width() > 0.0) {
            open.push((depth_of(graph, gid), gid, bb));
        }
    }
    // deeper groups first, so an outer frame takes in the inner ones'
    open.sort_by_key(|o| std::cmp::Reverse(o.0));
    let mut drawn: BTreeMap<GroupId, Rect> = BTreeMap::new();
    for (_, gid, bb) in open {
        let mut r = bb;
        for (child, cr) in &drawn {
            if graph.group(child).and_then(|c| c.parent.as_ref()) == Some(gid) {
                r = union(Some(r), *cr);
            }
        }
        let r = Rect::from_min_max(
            tame(Point::new(r.min.x - GROUP_PAD, r.min.y - GROUP_PAD - GROUP_HEADER)),
            tame(Point::new(r.max.x + GROUP_PAD, r.max.y + GROUP_PAD)),
        );
        drawn.insert(gid.clone(), r);
    }
    drawn
}

/// What a fit shows: every face shown and, outside an entered group, every frame, with
/// [`FIT_MARGIN`] around; none when there is nothing.
pub fn fit_rect(faces: impl Iterator<Item = Rect>, frames: impl Iterator<Item = Rect>) -> Option<[f64; 4]> {
    let bb = faces.chain(frames).fold(None, |bb, r| Some(union(bb, r)))?;
    Some([round(bb.min.x - FIT_MARGIN), round(bb.min.y - FIT_MARGIN), round(bb.max.x + FIT_MARGIN), round(bb.max.y + FIT_MARGIN)])
}

fn rect_from(r: [f32; 4]) -> Rect {
    let (a, b) = (tame(Point::new(r[0], r[1])), tame(Point::new(r[2], r[3])));
    Rect::from_min_max(Point::new(a.x.min(b.x), a.y.min(b.y)), Point::new(a.x.max(b.x), a.y.max(b.y)))
}

fn expand(r: Rect, by: f32) -> Rect {
    Rect::from_min_max(r.min - Point::new(by, by), r.max + Point::new(by, by))
}

fn offset(r: Rect, by: Point) -> Rect {
    Rect::from_min_max(tame(r.min + by), tame(r.max + by))
}

// ---------- the snapshot the hot calls read ----------

/// A pin, as the hot calls see it.
#[derive(Debug, Clone)]
struct SPin {
    /// Its height from the face's top.
    y: f32,
    /// Its type, interned in [`Snapshot::types`].
    ty: u32,
    /// An input that takes a list; an output that gives one (its port's, or its node mapped).
    list: bool,
    /// Can be wired: an input's port is wireable and not a named port.
    wireable: bool,
    /// The graph node it belongs to (a proxy's rows stand for members'), by index.
    node: u32,
    convert: bool,
    /// Its port's name on that node.
    key: String,
}

/// An input row's widget as a gesture reads it (N4 slice 2): what a tap edits and a drag scrubs.
#[derive(Debug, Clone, PartialEq)]
pub struct RowWidget {
    /// The node whose input it is (on a collapsed group's face, the member's) and its key.
    pub node: String,
    pub key: String,
    /// The row can be edited on the face ([`crate::inline::editable`]).
    pub editable: bool,
    pub widget: crate::scene::Widget,
}

#[derive(Debug, Clone)]
struct SFace {
    nid: u64,
    id: String,
    /// Where the scene shows it (its node's position, pushed out of another face's way:
    /// `crate::scene`'s `make_room`).
    pos: [f32; 2],
    /// Each input row's widget, in row order (none for a row without one).
    widgets: Vec<Option<RowWidget>>,
    rect: Rect,
    header: f32,
    inputs: Vec<SPin>,
    outputs: Vec<SPin>,
    /// Input rows' widget rects, absolute.
    rows: Vec<Option<Rect>>,
    /// Header parts, absolute.
    parts: Vec<(u32, i32, Rect)>,
    /// What a tap on each footer item does, in the footer's order (the shell's `parts.footer[]`).
    footer: Vec<FooterTap>,
}

/// What a tap on a face's footer item does (a core gesture, the same in every shell): Save
/// Result… saves (and Contact Sheet…, on the line's right half, when offered); any other line
/// watches the node, as before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FooterTap {
    Watch,
    Save { port: String, sheet: bool },
}

impl FooterTap {
    fn of(f: &crate::scene::Footer) -> FooterTap {
        match f {
            crate::scene::Footer::Save { port, sheet } => FooterTap::Save { port: port.clone(), sheet: *sheet },
            _ => FooterTap::Watch,
        }
    }
}

#[derive(Debug, Clone)]
struct SWire {
    nid: u64,
    id: String,
    /// (face index, pin index) at each end.
    from: (usize, usize),
    to: (usize, usize),
    kind: WireKind,
    via: Vec<[i32; 2]>,
    chain: Vec<Point>,
    bounds: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum WireKind {
    Plain,
    Routed,
    Net,
}

impl WireKind {
    fn word(self) -> &'static str {
        match self {
            WireKind::Plain => "plain",
            WireKind::Routed => "routed",
            WireKind::Net => "net",
        }
    }
}

#[derive(Debug, Clone)]
struct STag {
    part: u32,
    /// The net's nid and id.
    net: u64,
    net_id: String,
    /// The net's name and the group it is local to.
    net_key: (String, Option<String>),
    /// A use's wire (nid, id); the driver's: none.
    wire: Option<(u64, String)>,
    face: usize,
    rect: Rect,
}

#[derive(Debug, Clone)]
struct SBox {
    nid: u64,
    id: String,
    rect: Rect,
}

/// What the hot calls read: the published scene's faces, pins, wires, tags, frames and open
/// groups with the geometry the core has, and the type rules a wire is checked by. Immutable;
/// a new one replaces it when the scene's structure or the geometry changes, and readers on any
/// thread hold the one they took (section 9.4).
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub doc: u64,
    /// The scene generation it was made from.
    pub generation: u64,
    /// What it was made from: the session's generation and the graph's revision, the geometry's
    /// generation, the group entered (what makes it out of date).
    pub key: (u64, u64, u64, Option<String>),
    /// Faces the shell measured (0: no geometry reported, the hit test is the core's guess).
    pub measured: usize,
    faces: Vec<SFace>,
    face_at: HashMap<u64, usize>,
    face_id: HashMap<String, usize>,
    wires: Vec<SWire>,
    face_wires: Vec<Vec<usize>>,
    tags: Vec<STag>,
    frames: Vec<(SBox, Rect, Rect)>,
    groups: Vec<SBox>,
    types: Vec<String>,
    /// (output type, input type) → 1 takes it, 2 through a Convert.
    fits: HashMap<(u32, u32), u32>,
    /// Graph nodes by index, and what each feeds (cycles).
    nodes: Vec<String>,
    downstream: Vec<Vec<u32>>,
}

/// Where a document hands its latest snapshot to readers on other threads (the C ABI's hot
/// calls): swapped in by [`crate::document::Document::publish_scene`] the moment a new one is
/// made, before any reply that follows it goes out (N2 slice 4: the C ABI shared it at the end of
/// its core thread's turn, so a hot call right after a reply could read the one before). Held
/// only to clone or swap the `Arc`.
#[derive(Debug, Clone, Default)]
pub struct SnapshotCell(std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<Snapshot>>>>);

impl SnapshotCell {
    pub fn get(&self) -> Option<std::sync::Arc<Snapshot>> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    pub fn set(&self, s: std::sync::Arc<Snapshot>) {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(s);
    }
}

/// What is under a point, or where a wire may land (`plum_hit`, section 9.2 and 9.3).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Hit {
    pub kind: u32,
    pub part: u32,
    /// A row, footer item or bend point; -1 for none.
    pub index: i32,
    /// The nid of what was hit: a face (kinds 1, 2), a wire (3, 4, a use's tag), a net (a
    /// driver's tag), a frame (6), a group (7).
    pub item: u64,
    /// The face (kinds 1, 2; a tag's face).
    pub node: u64,
    /// The wire (kinds 3, 4; a use's tag).
    pub wire: u64,
    /// A pin's index on its side (kind 2).
    pub pin: u32,
    /// A wire target: 0 doesn't fit, 1 `wire.connect` takes it, 2 only through a Convert.
    pub fits: u32,
    /// A pin's point (kind 2: where a wire snaps), else the point asked.
    pub x: f32,
    pub y: f32,
}

impl Hit {
    fn none(p: Point) -> Hit {
        Hit { index: -1, x: p.x, y: p.y, ..Hit::default() }
    }
}

/// What a snapshot is made from.
pub struct SnapshotInputs<'a> {
    pub doc: u64,
    pub generation: u64,
    pub key: (u64, u64, u64, Option<String>),
    pub faces: Vec<&'a crate::scene::Face>,
    pub wires: Vec<&'a crate::scene::Wire>,
    pub nets: Vec<&'a crate::scene::Net>,
    pub frames: Vec<&'a crate::scene::Frame>,
    pub groups: Vec<&'a crate::scene::Group>,
    pub measured: &'a Measured,
    pub graph: &'a Graph,
    pub catalog: &'a SchemaReply,
}

/// The kinds `wire.connect` would refuse outright from a pin to another, past the types.
fn pair_refused(a: &SPin, b: &SPin, out_first: bool) -> bool {
    let (out, inp) = if out_first { (a, b) } else { (b, a) };
    !inp.wireable || out.node == inp.node || (inp.list && !out.list && !inp.convert && !out.convert)
}

impl Snapshot {
    /// Make the snapshot (on the thread that owns the document).
    pub fn build(i: SnapshotInputs<'_>) -> Snapshot {
        let graph = i.graph;
        let mapped = graph.mapped_nodes(i.catalog);
        let named: HashSet<&PortRef> = graph.named.iter().map(|n| &n.port).collect();
        let nodes: Vec<String> = graph.nodes.keys().map(|k| k.0.clone()).collect();
        let node_ix: HashMap<&str, u32> = graph.nodes.keys().enumerate().map(|(k, id)| (id.0.as_str(), u32::try_from(k).unwrap_or(u32::MAX))).collect();
        let mut downstream = vec![Vec::new(); nodes.len()];
        for w in &graph.wires {
            if let (Some(&a), Some(&b)) = (node_ix.get(w.from.node.0.as_str()), node_ix.get(w.to.node.0.as_str()))
                && let Some(d) = downstream.get_mut(a as usize)
            {
                d.push(b);
            }
        }
        let mut types: Vec<String> = Vec::new();
        let mut type_ix: HashMap<String, u32> = HashMap::new();
        let mut intern = |ty: &str| -> u32 {
            if let Some(&k) = type_ix.get(ty) {
                return k;
            }
            let k = u32::try_from(types.len()).unwrap_or(u32::MAX);
            types.push(ty.to_owned());
            type_ix.insert(ty.to_owned(), k);
            k
        };
        let node_of = |id: &str| node_ix.get(id).copied().unwrap_or(u32::MAX);
        let mut faces = Vec::with_capacity(i.faces.len());
        let (mut out_types, mut in_types) = (BTreeSet::new(), BTreeSet::new());
        for f in &i.faces {
            let g = i.measured.get(&f.id);
            let rect = face_rect(f.pos, g);
            let header = g.and_then(|g| g.header).unwrap_or(DEFAULT_HEADER);
            let inputs = f
                .inputs
                .iter()
                .enumerate()
                .map(|(k, r)| {
                    let ty = intern(&r.ty);
                    in_types.insert(ty);
                    let port = PortRef::new(NodeId(r.node.clone()), r.key.clone());
                    SPin {
                        y: pin_point(rect, g, false, k).y - rect.min.y,
                        ty,
                        list: r.list,
                        wireable: r.pin.wireable && !named.contains(&port),
                        node: node_of(&r.node),
                        convert: graph.is_convert(&port.node),
                        key: r.key.clone(),
                    }
                })
                .collect();
            let outputs = f
                .outputs
                .iter()
                .enumerate()
                .map(|(k, r)| {
                    let ty = intern(&r.ty);
                    out_types.insert(ty);
                    let node = NodeId(r.node.clone());
                    SPin {
                        y: pin_point(rect, g, true, k).y - rect.min.y,
                        ty,
                        list: r.list || mapped.contains(&node),
                        wireable: r.pin.wireable,
                        node: node_of(&r.node),
                        convert: graph.is_convert(&node),
                        key: r.key.clone(),
                    }
                })
                .collect();
            let at = rect.min;
            let rows = g.map(|g| g.rows.iter().map(|r| r.map(|r| offset(rect_from(r), at))).collect()).unwrap_or_default();
            let parts = g.map(|g| g.parts.iter().map(|p| (p.part, p.index, offset(rect_from(p.rect), at))).collect()).unwrap_or_default();
            let widgets = f
                .inputs
                .iter()
                .map(|r| {
                    r.widget.as_ref().map(|w| RowWidget { node: r.node.clone(), key: r.key.clone(), editable: crate::inline::editable(r), widget: w.clone() })
                })
                .collect();
            let footer = f.footer.iter().map(FooterTap::of).collect();
            faces.push(SFace { nid: f.nid, id: f.id.clone(), pos: f.pos, widgets, rect, header, inputs, outputs, rows, parts, footer });
        }
        let face_at: HashMap<u64, usize> = faces.iter().enumerate().map(|(k, f)| (f.nid, k)).collect();
        let face_id: HashMap<String, usize> = faces.iter().enumerate().map(|(k, f)| (f.id.clone(), k)).collect();
        let mut face_wires = vec![Vec::new(); faces.len()];
        let mut wires = Vec::with_capacity(i.wires.len());
        for w in &i.wires {
            let (Some(&a), Some(&b)) = (face_id.get(&w.from_face), face_id.get(&w.to_face)) else { continue };
            let kind = match w.kind.as_str() {
                "net" => WireKind::Net,
                "routed" => WireKind::Routed,
                _ => WireKind::Plain,
            };
            let chain: Vec<Point> = w.path.iter().map(|p| tame(Point::new(p[0] as f32, p[1] as f32))).collect();
            let k = wires.len();
            if let Some(v) = face_wires.get_mut(a) {
                v.push(k);
            }
            if b != a
                && let Some(v) = face_wires.get_mut(b)
            {
                v.push(k);
            }
            let bounds = wire_path::chain_bounds(&chain);
            wires.push(SWire { nid: w.nid, id: w.id.clone(), from: (a, w.from_pin), to: (b, w.to_pin), kind, via: w.via.clone(), chain, bounds });
        }
        let pin_at = |faces: &[SFace], face: usize, output: bool, pin: usize| -> Option<Point> {
            let f = faces.get(face)?;
            let p = if output { f.outputs.get(pin) } else { f.inputs.get(pin) }?;
            Some(Point::new(if output { f.rect.max.x } else { f.rect.min.x }, f.rect.min.y + p.y))
        };
        let tag_rect = |anchor: Point, text: &str, ends_at_anchor: bool| -> Rect {
            let w = TAG_CHAR * text.chars().count().min(64) as f32 + TAG_PAD;
            let x0 = if ends_at_anchor { anchor.x - w } else { anchor.x };
            Rect::from_min_max(tame(Point::new(x0, anchor.y - TAG_HEIGHT / 2.0)), tame(Point::new(x0 + w, anchor.y + TAG_HEIGHT / 2.0)))
        };
        let wire_nid: HashMap<&str, (u64, &str)> = i.wires.iter().map(|w| (w.id.as_str(), (w.nid, w.id.as_str()))).collect();
        let mut tags = Vec::new();
        for n in &i.nets {
            let name = n.key.name.as_str();
            if let Some(&face) = face_id.get(&n.driver.face)
                && let Some(pin) = pin_at(&faces, face, true, n.driver.pin)
            {
                let anchor = pin + Point::new(n.driver.at[0] as f32, n.driver.at[1] as f32);
                tags.push(STag {
                    part: part::DRIVER,
                    net: n.nid,
                    net_id: n.id.clone(),
                    net_key: (n.key.name.clone(), n.key.group.as_ref().map(|g| g.0.clone())),
                    wire: None,
                    face,
                    rect: tag_rect(anchor, if n.driver.text.is_empty() { name } else { n.driver.text.as_str() }, false),
                });
            }
            for u in &n.uses {
                let Some(&face) = face_id.get(&u.face) else { continue };
                let Some(pin) = pin_at(&faces, face, false, u.pin) else { continue };
                let anchor = pin + Point::new(u.at[0] as f32, u.at[1] as f32);
                let wid = format!("{}.{}", u.to.node, u.to.port);
                let wire = wire_nid.get(wid.as_str()).map(|(nid, id)| (*nid, (*id).to_owned()));
                tags.push(STag {
                    part: part::USE,
                    net: n.nid,
                    net_id: n.id.clone(),
                    net_key: (n.key.name.clone(), n.key.group.as_ref().map(|g| g.0.clone())),
                    wire,
                    face,
                    rect: tag_rect(anchor, name, true),
                });
            }
        }
        let frames = i
            .frames
            .iter()
            .map(|f| {
                let rect = face_rect(f.pos, Some(&FaceGeometry { size: f.size.map(size_value), ..FaceGeometry::default() }));
                let tab = Rect::from_min_max(
                    tame(Point::new(rect.min.x, rect.min.y - FRAME_TAB[1])),
                    tame(Point::new(rect.min.x + rect.width().clamp(FRAME_TAB[1], FRAME_TAB[0]), rect.min.y)),
                );
                let handle = Rect::from_min_max(tame(rect.max - Point::new(FRAME_HANDLE, FRAME_HANDLE)), rect.max);
                (SBox { nid: f.nid, id: f.id.clone(), rect }, tab, handle)
            })
            .collect();
        // open groups, deeper first (a nested band is over its parent's body)
        let mut groups: Vec<(usize, SBox)> =
            i.groups.iter().filter_map(|g| g.rect.map(|r| (g.depth, SBox { nid: g.nid, id: g.id.clone(), rect: rect_from(r.map(|v| v as f32)) }))).collect();
        groups.sort_by_key(|(d, _)| std::cmp::Reverse(*d));
        let groups = groups.into_iter().map(|(_, b)| b).collect();
        let mut fits = HashMap::new();
        for &o in &out_types {
            for &t in &in_types {
                let (Some(a), Some(b)) = (types.get(o as usize), types.get(t as usize)) else { continue };
                let f = if i.catalog.accepts(a, b) {
                    1
                } else if i.catalog.convert_paths_where(a, |x| i.catalog.accepts(x, b)).iter().any(|p| !p.is_empty()) {
                    2
                } else {
                    0
                };
                fits.insert((o, t), f);
            }
        }
        Snapshot {
            doc: i.doc,
            generation: i.generation,
            key: i.key,
            measured: i.faces.iter().filter(|f| i.measured.get(&f.id).is_some()).count(),
            faces,
            face_at,
            face_id,
            wires,
            face_wires,
            tags,
            frames,
            groups,
            types,
            fits,
            nodes,
            downstream,
        }
    }

    fn pin_point(&self, face: usize, output: bool, pin: usize) -> Option<Point> {
        let f = self.faces.get(face)?;
        let p = if output { f.outputs.get(pin) } else { f.inputs.get(pin) }?;
        Some(Point::new(if output { f.rect.max.x } else { f.rect.min.x }, f.rect.min.y + p.y))
    }

    fn slop(slop: f32) -> f32 {
        if slop.is_nan() { 0.0 } else { slop.clamp(0.0, MAX_SLOP) }
    }

    /// What is at `(x, y)` (graph units) within `slop` (graph units: the shell's target size
    /// over its zoom), in section 9.2's order: a pin (the nearest wireable one; a pin sits on
    /// its face's edge), then a face (the topmost, the last drawn: its parts, rows, header or
    /// body), a bend point, a net tag, a frame's tab or handle, a wire (the nearest), an open
    /// group's band (the innermost), a frame's body, else nothing. A point that is not a number
    /// hits nothing.
    pub fn hit(&self, x: f32, y: f32, slop: f32) -> Hit {
        let s = Self::slop(slop);
        if !(x.is_finite() && y.is_finite()) {
            return Hit::none(Point::new(coord(x), coord(y)));
        }
        let p = tame(Point::new(x, y));
        // a pin: the nearest within reach, the topmost face on a tie
        let mut best: Option<(f32, usize, bool, usize)> = None;
        for (k, f) in self.faces.iter().enumerate() {
            if !expand(f.rect, s).contains(p) {
                continue;
            }
            for (output, pins) in [(false, &f.inputs), (true, &f.outputs)] {
                for (j, pin) in pins.iter().enumerate().filter(|(_, pin)| pin.wireable) {
                    let at = Point::new(if output { f.rect.max.x } else { f.rect.min.x }, f.rect.min.y + pin.y);
                    let d = at.distance(p);
                    if d <= s && best.is_none_or(|(bd, ..)| d <= bd) {
                        best = Some((d, k, output, j));
                    }
                }
            }
        }
        if let Some((_, k, output, j)) = best
            && let (Some(f), Some(at)) = (self.faces.get(k), self.pin_point(k, output, j))
        {
            return Hit {
                kind: kind::PIN,
                part: if output { part::OUTPUT } else { part::INPUT },
                index: -1,
                item: f.nid,
                node: f.nid,
                pin: u32::try_from(j).unwrap_or(u32::MAX),
                x: at.x,
                y: at.y,
                ..Hit::default()
            };
        }
        // a face: the topmost
        if let Some(f) = self.faces.iter().rev().find(|f| f.rect.contains(p)) {
            let (part, index) = f
                .parts
                .iter()
                .find(|(_, _, r)| r.contains(p))
                .map(|(c, i, _)| (*c, *i))
                .or_else(|| f.rows.iter().position(|r| r.is_some_and(|r| r.contains(p))).map(|k| (part::ROW, i32::try_from(k).unwrap_or(i32::MAX))))
                .unwrap_or(if p.y < f.rect.min.y + f.header { (part::HEADER, -1) } else { (part::BODY, -1) });
            return Hit { kind: kind::NODE, part, index, item: f.nid, node: f.nid, x: p.x, y: p.y, ..Hit::default() };
        }
        // a bend point
        let reach = s.max(BEND_RADIUS);
        for w in self.wires.iter().rev() {
            if let Some(k) = w.via.iter().position(|v| wire_path::as_point(v).distance(p) <= reach) {
                return Hit { kind: kind::BEND, index: i32::try_from(k).unwrap_or(i32::MAX), item: w.nid, wire: w.nid, x: p.x, y: p.y, ..Hit::default() };
            }
        }
        // a net tag
        if let Some(t) = self.tags.iter().rev().find(|t| expand(t.rect, s.min(4.0)).contains(p)) {
            let (item, wire) = match &t.wire {
                Some((nid, _)) => (*nid, *nid),
                None => (t.net, 0),
            };
            let node = self.faces.get(t.face).map_or(0, |f| f.nid);
            return Hit { kind: kind::TAG, part: t.part, index: -1, item, node, wire, x: p.x, y: p.y, ..Hit::default() };
        }
        // a frame's tab or handle
        for (b, tab, handle) in self.frames.iter().rev() {
            let part = if tab.contains(p) {
                part::FRAME_TAB
            } else if handle.contains(p) {
                part::FRAME_HANDLE
            } else {
                continue;
            };
            return Hit { kind: kind::FRAME, part, index: -1, item: b.nid, x: p.x, y: p.y, ..Hit::default() };
        }
        // a wire: the nearest within reach
        let mut near: Option<(f32, &SWire)> = None;
        for w in &self.wires {
            let Some(b) = w.bounds else { continue };
            if !expand(b, s).contains(p) {
                continue;
            }
            let d = wire_path::chain_distance(&w.chain, p);
            if d <= s && near.is_none_or(|(bd, _)| d < bd) {
                near = Some((d, w));
            }
        }
        if let Some((_, w)) = near {
            return Hit { kind: kind::WIRE, index: -1, item: w.nid, wire: w.nid, x: p.x, y: p.y, ..Hit::default() };
        }
        // an open group's band, the innermost
        if let Some(g) = self.groups.iter().find(|g| Rect::from_min_size(g.rect.min, Point::new(g.rect.width(), GROUP_HEADER.min(g.rect.height()))).contains(p))
        {
            return Hit { kind: kind::GROUP, part: part::BAND, index: -1, item: g.nid, x: p.x, y: p.y, ..Hit::default() };
        }
        if let Some((b, ..)) = self.frames.iter().rev().find(|(b, ..)| b.rect.contains(p)) {
            return Hit { kind: kind::FRAME, part: part::FRAME_BODY, index: -1, item: b.nid, x: p.x, y: p.y, ..Hit::default() };
        }
        Hit::none(p)
    }

    /// Does the graph node `to` reach `from` downstream (a wire from `from` into `to` would be
    /// a cycle)?
    fn reaches(&self, to: u32, from: u32) -> bool {
        if to == from {
            return true;
        }
        let mut seen = vec![false; self.downstream.len()];
        let mut stack = vec![to];
        while let Some(n) = stack.pop() {
            if n == from {
                return true;
            }
            let Some(s) = seen.get_mut(n as usize) else { continue };
            if *s {
                continue;
            }
            *s = true;
            if let Some(next) = self.downstream.get(n as usize) {
                stack.extend(next.iter().copied());
            }
        }
        false
    }

    /// Would a wire between pin `a` (on side `a_out`) and pin `b` be taken: 1 as it is, 2 only
    /// through a Convert node, 0 not.
    fn fits(&self, a: &SPin, b: &SPin, a_out: bool) -> u32 {
        if pair_refused(a, b, a_out) {
            return 0;
        }
        let (out, inp) = if a_out { (a, b) } else { (b, a) };
        if self.reaches(inp.node, out.node) {
            return 0;
        }
        // a Convert node's own rules (what it can still turn into what) are `wire.connect`'s,
        // at the drop: a wire to or from one is shown as fitting
        if out.convert || inp.convert {
            return if out.convert && inp.convert { 0 } else { 1 };
        }
        self.fits.get(&(out.ty, inp.ty)).copied().unwrap_or(0)
    }

    /// Where a wire dragged from pin `pin` of face `face` (an output when `output`) may land at
    /// `(x, y)`: the nearest pin on the other side within `slop` that fits (`fits` 1, or 2 only
    /// through a Convert), else the nearest that doesn't (`fits` 0), else nothing. Section 9.3.
    pub fn wire_target(&self, face: u64, output: bool, pin: u32, x: f32, y: f32, slop: f32) -> Hit {
        let s = Self::slop(slop);
        let p = tame(Point::new(x, y));
        if !(x.is_finite() && y.is_finite()) {
            return Hit::none(p);
        }
        let Some(from) = self
            .face_at
            .get(&face)
            .and_then(|&k| self.faces.get(k))
            .and_then(|f| if output { f.outputs.get(pin as usize) } else { f.inputs.get(pin as usize) })
        else {
            return Hit::none(p);
        };
        let mut best: Option<(u32, f32, usize, usize)> = None;
        for (k, f) in self.faces.iter().enumerate() {
            if !expand(f.rect, s).contains(p) {
                continue;
            }
            let pins = if output { &f.inputs } else { &f.outputs };
            for (j, q) in pins.iter().enumerate().filter(|(_, q)| q.wireable) {
                let at = Point::new(if output { f.rect.min.x } else { f.rect.max.x }, f.rect.min.y + q.y);
                let d = at.distance(p);
                if d > s {
                    continue;
                }
                let fit = self.fits(from, q, output);
                // a pin that fits beats one that doesn't; then the nearer
                let better = match best {
                    None => true,
                    Some((bf, bd, ..)) => (fit > 0, -d) > (bf > 0, -bd),
                };
                if better {
                    best = Some((fit, d, k, j));
                }
            }
        }
        let Some((fits, _, k, j)) = best else { return Hit::none(p) };
        let Some((f, at)) = self.faces.get(k).zip(self.pin_point(k, !output, j)) else { return Hit::none(p) };
        Hit {
            kind: kind::PIN,
            part: if output { part::INPUT } else { part::OUTPUT },
            index: -1,
            item: f.nid,
            node: f.nid,
            pin: u32::try_from(j).unwrap_or(u32::MAX),
            fits,
            x: at.x,
            y: at.y,
            ..Hit::default()
        }
    }

    /// The paths of every wire with an end on one of `faces` (nids), as if those faces were
    /// moved by `(dx, dy)`, appended to `out` as section 9.1 lays them out: per wire its nid's
    /// low and high 32 bits and its point count (each a `u32` bit-cast to a float), then the points as
    /// `x, y`. A wire drawn through a net has no path and is left out; unknown nids are skipped.
    /// Returns how many floats it appended.
    pub fn wire_paths(&self, faces: &[u64], dx: f32, dy: f32, out: &mut Vec<f32>) -> usize {
        let start = out.len();
        let by = tame(Point::new(dx, dy));
        let moved: HashSet<usize> = faces.iter().filter_map(|n| self.face_at.get(n).copied()).collect();
        let mut done: HashSet<usize> = HashSet::new();
        for &f in &moved {
            for &k in self.face_wires.get(f).into_iter().flatten() {
                if !done.insert(k) {
                    continue;
                }
                let Some(chain) = self.moved_chain(k, &moved, by) else { continue };
                if chain.is_empty() {
                    continue;
                }
                let nid = self.wires.get(k).map_or(0, |w| w.nid);
                // the whole nid, low word first (N2 slice 4: slice 3 sent the low 32 bits only)
                out.push(f32::from_bits((nid & u64::from(u32::MAX)) as u32));
                out.push(f32::from_bits((nid >> 32) as u32));
                out.push(f32::from_bits(u32::try_from(chain.len()).unwrap_or(u32::MAX)));
                for p in chain {
                    out.push(p.x);
                    out.push(p.y);
                }
            }
        }
        out.len() - start
    }

    /// Wire `k`'s chain with the ends on `moved` faces moved by `by`.
    fn moved_chain(&self, k: usize, moved: &HashSet<usize>, by: Point) -> Option<Vec<Point>> {
        let w = self.wires.get(k)?;
        if w.kind == WireKind::Net {
            return Some(Vec::new());
        }
        let shift = |face: usize| if moved.contains(&face) { by } else { Point::ZERO };
        let from = tame(self.pin_point(w.from.0, true, w.from.1)? + shift(w.from.0));
        let to = tame(self.pin_point(w.to.0, false, w.to.1)? + shift(w.to.0));
        let rect = |face: usize| self.faces.get(face).map(|f| Rect::from_min_max(tame(f.rect.min + shift(face)), tame(f.rect.max + shift(face))));
        let ends = rect(w.from.0).zip(rect(w.to.0));
        Some(wire_chain(w.kind.word(), from, to, &w.via, ends))
    }

    /// A face's nid by id (the JSON calls name faces by id).
    pub fn face_nid(&self, id: &str) -> Option<u64> {
        self.face_id.get(id).and_then(|&k| self.faces.get(k)).map(|f| f.nid)
    }

    /// The faces the snapshot holds.
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// A hit as the JSON calls give it (`geometry.hit`, `geometry.wireTarget`): its kind and
    /// part by name, the ids and nids of what it names.
    pub fn hit_json(&self, h: &Hit) -> Value {
        let face = |nid: u64| self.face_at.get(&nid).and_then(|&k| self.faces.get(k));
        let wire = |nid: u64| self.wires.iter().find(|w| w.nid == nid);
        let mut v = json!({"at": [h.x, h.y]});
        match h.kind {
            kind::PIN => {
                v["kind"] = json!("pin");
                v["side"] = json!(if h.part == part::OUTPUT { "out" } else { "in" });
                v["node"] = json!(face(h.node).map(|f| f.id.as_str()));
                v["nid"] = json!(h.node);
                v["pin"] = json!(h.pin);
                if let Some(f) = face(h.node)
                    && let Some(q) = if h.part == part::OUTPUT { f.outputs.get(h.pin as usize) } else { f.inputs.get(h.pin as usize) }
                {
                    v["type"] = json!(self.types.get(q.ty as usize));
                }
                v["fits"] = json!(h.fits);
            }
            kind::NODE => {
                v["kind"] = json!("node");
                let names = [
                    (part::BODY, "body"),
                    (part::HEADER, "header"),
                    (part::TITLE, "title"),
                    (part::CHEVRON, "chevron"),
                    (part::ICON, "icon"),
                    (part::MENU, "menu"),
                    (part::ENTER, "enter"),
                    (part::ROW, "row"),
                    (part::PREVIEW, "preview"),
                    (part::FOOTER, "footer"),
                ];
                v["part"] = json!(names.iter().find(|(c, _)| *c == h.part).map(|(_, n)| *n));
                v["node"] = json!(face(h.node).map(|f| f.id.as_str()));
                v["nid"] = json!(h.node);
                if h.index >= 0 {
                    v["index"] = json!(h.index);
                }
            }
            kind::WIRE | kind::BEND => {
                v["kind"] = json!(if h.kind == kind::WIRE { "wire" } else { "bend" });
                v["wire"] = json!(wire(h.wire).map(|w| w.id.as_str()));
                v["nid"] = json!(h.wire);
                if h.kind == kind::BEND {
                    v["index"] = json!(h.index);
                }
            }
            kind::TAG => {
                v["kind"] = json!("tag");
                v["part"] = json!(if h.part == part::DRIVER { "driver" } else { "use" });
                let t = self
                    .tags
                    .iter()
                    .find(|t| t.part == h.part && if h.part == part::DRIVER { t.net == h.item } else { t.wire.as_ref().is_some_and(|w| w.0 == h.item) });
                v["net"] = json!(t.map(|t| t.net_id.as_str()));
                if let Some((nid, id)) = t.and_then(|t| t.wire.as_ref()) {
                    v["wire"] = json!(id);
                    v["nid"] = json!(nid);
                }
                v["node"] = json!(face(h.node).map(|f| f.id.as_str()));
            }
            kind::FRAME => {
                v["kind"] = json!("frame");
                v["part"] = json!(match h.part {
                    part::FRAME_TAB => "tab",
                    part::FRAME_HANDLE => "handle",
                    _ => "body",
                });
                v["frame"] = json!(self.frames.iter().find(|(b, ..)| b.nid == h.item).map(|(b, ..)| b.id.as_str()));
                v["nid"] = json!(h.item);
            }
            kind::GROUP => {
                v["kind"] = json!("group");
                v["part"] = json!("band");
                v["group"] = json!(self.groups.iter().find(|g| g.nid == h.item).map(|g| g.id.as_str()));
                v["nid"] = json!(h.item);
            }
            _ => v["kind"] = json!("none"),
        }
        v
    }

    /// `geometry.wirePaths`' reply: each wire's id, nid and moved path.
    pub fn wire_paths_json(&self, faces: &[u64], dx: f32, dy: f32) -> Value {
        let by = tame(Point::new(dx, dy));
        let moved: HashSet<usize> = faces.iter().filter_map(|n| self.face_at.get(n).copied()).collect();
        let mut ks: Vec<usize> = moved.iter().flat_map(|&f| self.face_wires.get(f).into_iter().flatten().copied()).collect();
        ks.sort_unstable();
        ks.dedup();
        let wires: Vec<Value> = ks
            .into_iter()
            .filter_map(|k| {
                let w = self.wires.get(k)?;
                let chain = self.moved_chain(k, &moved, by)?;
                (!chain.is_empty()).then(|| json!({"wire": w.id, "nid": w.nid, "path": chain_json(&chain)}))
            })
            .collect();
        json!({"wires": wires})
    }

    /// The graph nodes it knows, for tests.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    // ---- what a gesture reads (N2 slice 4, `crate::gesture`) ----

    fn face_by_nid(&self, nid: u64) -> Option<&SFace> {
        self.face_at.get(&nid).and_then(|&k| self.faces.get(k))
    }

    /// Face `nid`'s id (a node's, or `group:<id>` for a collapsed group's).
    pub fn face_id_of(&self, nid: u64) -> Option<&str> {
        self.face_by_nid(nid).map(|f| f.id.as_str())
    }

    /// Face `nid`'s rect (graph units), at the size the core has for it.
    pub fn face_rect_of(&self, nid: u64) -> Option<Rect> {
        self.face_by_nid(nid).map(|f| f.rect)
    }

    /// Row `row`'s widget rect on face `nid` (absolute), when the shell measured one.
    pub fn row_rect(&self, nid: u64, row: i32) -> Option<Rect> {
        let f = self.face_by_nid(nid)?;
        f.rows.get(usize::try_from(row).ok()?).copied().flatten()
    }

    /// Does a part the shell measured on face `nid` cover `p`?
    pub fn covers_part(&self, nid: u64, p: Point) -> bool {
        self.face_by_nid(nid).is_some_and(|f| f.parts.iter().any(|(_, _, r)| r.contains(p)))
    }

    /// Part `code`'s rect on face `nid` (absolute), when the shell measured it.
    pub fn part_rect(&self, nid: u64, code: u32) -> Option<Rect> {
        self.face_by_nid(nid)?.parts.iter().find(|(c, _, _)| *c == code).map(|(_, _, r)| *r)
    }

    /// What a tap on footer item `index` of face `nid` does, and that item's rect when the shell
    /// measured it (absolute).
    pub fn footer_tap(&self, nid: u64, index: i32) -> Option<(&FooterTap, Option<Rect>)> {
        let f = self.face_by_nid(nid)?;
        let t = f.footer.get(usize::try_from(index).ok()?)?;
        let r = f.parts.iter().find(|(c, k, _)| *c == part::FOOTER && *k == index).map(|(_, _, r)| *r);
        Some((t, r))
    }

    /// The rect of face `nid`'s Save Result… footer line, when it has one the shell measured.
    pub fn save_footer_rect(&self, nid: u64) -> Option<Rect> {
        let f = self.face_by_nid(nid)?;
        let k = i32::try_from(f.footer.iter().position(|t| matches!(t, FooterTap::Save { .. }))?).ok()?;
        f.parts.iter().find(|(c, i, _)| *c == part::FOOTER && *i == k).map(|(_, _, r)| *r)
    }

    /// Row `row`'s widget on face `nid`, when it has one.
    pub fn row_widget(&self, nid: u64, row: i32) -> Option<&RowWidget> {
        let f = self.face_by_nid(nid)?;
        f.widgets.get(usize::try_from(row).ok()?)?.as_ref()
    }

    /// Where face `id` is shown (the scene's `pos`: its node's position, or where it was pushed
    /// to out of another face's way).
    pub fn shown_pos(&self, id: &str) -> Option<[f32; 2]> {
        self.face_id.get(id).and_then(|&k| self.faces.get(k)).map(|f| f.pos)
    }

    /// Face `nid`'s header height (what the hit test takes as header).
    pub fn face_header_of(&self, nid: u64) -> Option<f32> {
        self.face_by_nid(nid).map(|f| f.header)
    }

    /// Was face `id` measured by the shell (else its size is the core's default)?
    pub fn is_measured(&self, measured: &Measured, id: &str) -> bool {
        measured.get(id).is_some() && self.face_id.contains_key(id)
    }

    /// The graph port pin `pin` of face `nid` stands for (an output when `output`): a collapsed
    /// group's pins stand for its members' ports.
    pub fn pin_port(&self, nid: u64, output: bool, pin: u32) -> Option<PortRef> {
        let f = self.face_by_nid(nid)?;
        let p = if output { f.outputs.get(pin as usize) } else { f.inputs.get(pin as usize) }?;
        let node = self.nodes.get(p.node as usize)?;
        Some(PortRef::new(NodeId(node.clone()), p.key.clone()))
    }

    /// Where pin `pin` of face `nid` is (graph units).
    pub fn pin_at(&self, nid: u64, output: bool, pin: u32) -> Option<Point> {
        let k = *self.face_at.get(&nid)?;
        self.pin_point(k, output, pin as usize)
    }

    /// Wire `nid`: its id, its output end's port, its input end's port, its bend points, its
    /// chain, the face nids at its ends and their pins `((face, pin), (face, pin))`.
    pub fn wire_of(&self, nid: u64) -> Option<WireInfo> {
        let w = self.wires.iter().find(|w| w.nid == nid)?;
        self.wire_info(w)
    }

    /// Wire `id` (its input end's `node.port`), as [`Snapshot::wire_of`].
    pub fn wire_by_id(&self, id: &str) -> Option<WireInfo> {
        let w = self.wires.iter().find(|w| w.id == id)?;
        self.wire_info(w)
    }

    fn wire_info(&self, w: &SWire) -> Option<WireInfo> {
        let (fa, fb) = (self.faces.get(w.from.0)?, self.faces.get(w.to.0)?);
        let from = self.pin_port(fa.nid, true, u32::try_from(w.from.1).ok()?)?;
        let to = self.pin_port(fb.nid, false, u32::try_from(w.to.1).ok()?)?;
        Some(WireInfo {
            nid: w.nid,
            id: w.id.clone(),
            from,
            to,
            via: w.via.clone(),
            chain: w.chain.clone(),
            from_pin: (fa.nid, u32::try_from(w.from.1).unwrap_or(u32::MAX)),
            to_pin: (fb.nid, u32::try_from(w.to.1).unwrap_or(u32::MAX)),
        })
    }

    /// The wire into input pin `pin` of face `nid`, if it has one drawn.
    pub fn wire_into(&self, nid: u64, pin: u32) -> Option<WireInfo> {
        let k = *self.face_at.get(&nid)?;
        let w = self.wires.iter().find(|w| w.to == (k, pin as usize))?;
        self.wire_info(w)
    }

    /// A net tag's net (name, the group it is local to) and, for a use's tag, its wire's id;
    /// `driver` picks a driver's tag, by the net's nid, else a use's, by its wire's nid.
    pub fn tag_of(&self, driver: bool, item: u64) -> Option<(String, Option<String>, Option<String>)> {
        let t = self
            .tags
            .iter()
            .find(|t| if driver { t.part == part::DRIVER && t.net == item } else { t.part == part::USE && t.wire.as_ref().is_some_and(|w| w.0 == item) })?;
        Some((t.net_key.0.clone(), t.net_key.1.clone(), t.wire.as_ref().map(|w| w.1.clone())))
    }

    /// Net `name`'s (local to `group`) driver's tag, where the hit test takes it (N4 slice 3: where
    /// its name is edited in place).
    pub fn driver_tag_rect(&self, name: &str, group: Option<&str>) -> Option<Rect> {
        self.tags.iter().find(|t| t.part == part::DRIVER && t.net_key.0 == name && t.net_key.1.as_deref() == group).map(|t| t.rect)
    }

    /// Open group `id`'s rect.
    pub fn group_rect_by_id(&self, id: &str) -> Option<Rect> {
        self.groups.iter().find(|g| g.id == id).map(|g| g.rect)
    }

    /// Frame `nid`: its id and rect.
    pub fn frame_of(&self, nid: u64) -> Option<(&str, Rect)> {
        self.frames.iter().find(|(b, ..)| b.nid == nid).map(|(b, ..)| (b.id.as_str(), b.rect))
    }

    /// Frame `id`: its rect, tab and resize handle.
    pub fn frame_by_id(&self, id: &str) -> Option<(Rect, Rect, Rect)> {
        self.frames.iter().find(|(b, ..)| b.id == id).map(|(b, tab, handle)| (b.rect, *tab, *handle))
    }

    /// Open group `nid`: its id and rect.
    pub fn group_of(&self, nid: u64) -> Option<(&str, Rect)> {
        self.groups.iter().find(|g| g.nid == nid).map(|g| (g.id.as_str(), g.rect))
    }

    /// Face `id`'s nid and rect.
    pub fn face_by_id(&self, id: &str) -> Option<(u64, Rect)> {
        self.face_id.get(id).and_then(|&k| self.faces.get(k)).map(|f| (f.nid, f.rect))
    }

    /// How many pins face `nid` has on a side.
    pub fn pin_count(&self, nid: u64, output: bool) -> usize {
        self.face_by_nid(nid).map_or(0, |f| if output { f.outputs.len() } else { f.inputs.len() })
    }

    /// The faces a rect touches (a marquee: a face it intersects, as egui-snarl's), by id, in
    /// drawing order.
    pub fn faces_touching(&self, r: Rect) -> Vec<(u64, &str)> {
        self.faces
            .iter()
            .filter(|f| f.rect.min.x <= r.max.x && f.rect.max.x >= r.min.x && f.rect.min.y <= r.max.y && f.rect.max.y >= r.min.y)
            .map(|f| (f.nid, f.id.as_str()))
            .collect()
    }

    /// A hit's JSON with no ids looked up: for a `plum_drag`'s target as JSON.
    pub fn hit_kind_name(kind: u32) -> &'static str {
        match kind {
            kind::NODE => "node",
            kind::PIN => "pin",
            kind::WIRE => "wire",
            kind::BEND => "bend",
            kind::TAG => "tag",
            kind::FRAME => "frame",
            kind::GROUP => "group",
            _ => "none",
        }
    }
}

/// A drawn wire, as a gesture needs it ([`Snapshot::wire_of`]).
#[derive(Debug, Clone, PartialEq)]
pub struct WireInfo {
    pub nid: u64,
    pub id: String,
    pub from: PortRef,
    pub to: PortRef,
    pub via: Vec<[i32; 2]>,
    pub chain: Vec<Point>,
    /// (face nid, pin index) at each end.
    pub from_pin: (u64, u32),
    pub to_pin: (u64, u32),
}

/// `{at: [x, y], slop?}` (`geometry.hit`), or with `from` (`geometry.wireTarget`).
pub fn parse_point(params: &Value, more: &[&str]) -> Result<(f32, f32, f32), String> {
    let mut fields = vec!["at", "slop"];
    fields.extend_from_slice(more);
    check_fields(params, &fields)?;
    let at = params.get("at").and_then(Value::as_array).filter(|a| a.len() == 2).ok_or("at must be [x, y] graph coordinates")?;
    let n = |v: &Value| v.as_f64().map(|f| f as f32).ok_or("at must be [x, y] numbers");
    let slop = match params.get("slop") {
        None | Some(Value::Null) => 12.0,
        Some(v) => v.as_f64().map(|f| f as f32).ok_or("slop must be a distance in graph units")?,
    };
    Ok((n(&at[0])?, n(&at[1])?, slop))
}

/// `geometry.wireTarget`'s `from: {node, side: "in" | "out", pin}`: the face's nid, the side
/// (output?), the pin's index.
pub fn parse_from(snapshot: &Snapshot, params: &Value) -> Result<(u64, bool, u32), String> {
    let from = params.get("from").ok_or("from must be {node, side, pin}")?;
    check_fields(from, &["node", "side", "pin"])?;
    let id = from.get("node").and_then(Value::as_str).ok_or("from.node must be a face's id")?;
    let nid = snapshot.face_nid(id).ok_or_else(|| format!("no face {}", crate::selection::shown_id(id)))?;
    let output = match from.get("side").and_then(Value::as_str) {
        Some("out") => true,
        Some("in") => false,
        _ => return Err("from.side must be \"in\" or \"out\"".into()),
    };
    let pin = from.get("pin").and_then(Value::as_u64).and_then(|p| u32::try_from(p).ok()).ok_or("from.pin must be the pin's index on its side")?;
    Ok((nid, output, pin))
}

/// `geometry.wirePaths`' `{nodes: [face id], offset?: [dx, dy]}`.
pub fn parse_paths(snapshot: &Snapshot, params: &Value) -> Result<(Vec<u64>, f32, f32), String> {
    check_fields(params, &["nodes", "offset"])?;
    let nodes = params.get("nodes").and_then(Value::as_array).filter(|a| a.len() <= MAX_FACES).ok_or("nodes must be a list of face ids")?;
    let nids = nodes.iter().filter_map(Value::as_str).filter_map(|id| snapshot.face_nid(id)).collect();
    let (dx, dy) = match params.get("offset") {
        None | Some(Value::Null) => (0.0, 0.0),
        Some(v) => {
            let a = v.as_array().filter(|a| a.len() == 2).ok_or("offset must be [dx, dy]")?;
            let n = |v: &Value| v.as_f64().map(|f| f as f32).ok_or("offset must be [dx, dy] numbers");
            (n(&a[0])?, n(&a[1])?)
        }
    };
    Ok((nids, dx, dy))
}

/// `geometry.locate` (N2 slice 4): where the core has something, at the geometry it has, so a
/// script aims a gesture at what any shell measured. `{node: face id, part?: "center" | "header"
/// | "body", pin?: {side: "in" | "out", index}}`, `{wire: wire id}` (a point on its path, its
/// middle), or `{frame: frame id, part?: "body" | "tab" | "handle"}` → `{at: [x, y], rect?,
/// measured?}`.
pub fn locate(snap: &Snapshot, measured: &Measured, params: &Value) -> Result<Value, String> {
    check_fields(params, &["node", "part", "pin", "row", "wire", "frame"])?;
    let pt = |p: Point| json!([round(p.x), round(p.y)]);
    let part_name = params.get("part").map(|p| p.as_str().ok_or("part must be a name")).transpose()?;
    if let Some(id) = params.get("node") {
        let id = id.as_str().ok_or("node must be a face's id")?;
        let (nid, r) = snap.face_by_id(id).ok_or_else(|| format!("no face {}", crate::selection::shown_id(id)))?;
        let header = snap.face_header_of(nid).unwrap_or(DEFAULT_HEADER).min(r.height());
        let at = if let Some(pin) = params.get("pin") {
            check_fields(pin, &["side", "index"])?;
            let output = match pin.get("side").and_then(Value::as_str) {
                Some("out") => true,
                Some("in") => false,
                _ => return Err("pin.side must be \"in\" or \"out\"".into()),
            };
            let k = pin.get("index").and_then(Value::as_u64).and_then(|k| u32::try_from(k).ok()).ok_or("pin.index must be the pin's index on its side")?;
            snap.pin_at(nid, output, k)
                .ok_or_else(|| format!("face {} has no {} pin {k}", crate::selection::shown_id(id), if output { "out" } else { "in" }))?
        } else {
            match (part_name.unwrap_or("center"), params.get("row")) {
                (_, Some(k)) => {
                    let k = k.as_u64().and_then(|k| i32::try_from(k).ok()).ok_or("row must be an input row's index")?;
                    snap.row_rect(nid, k)
                        .map(|r| r.center())
                        .ok_or_else(|| format!("face {} has no widget measured on row {k}", crate::selection::shown_id(id)))?
                }
                ("center", _) => r.center(),
                // the header's own area: its middle, or the nearest point along it that no part
                // the shell measured (a title, a button) covers
                ("header", _) => {
                    let y = r.min.y + header / 2.0;
                    let free = |x: f32| !snap.covers_part(nid, Point::new(x, y));
                    let w = r.width().max(1.0);
                    let cx = r.center().x;
                    (0..64)
                        .flat_map(|k| [cx + w * k as f32 / 128.0, cx - w * k as f32 / 128.0])
                        .find(|x| *x > r.min.x && *x < r.max.x && free(*x))
                        .map_or(Point::new(cx, y), |x| Point::new(x, y))
                }
                ("body", _) => Point::new(r.center().x, r.min.y + header + (r.height() - header) / 2.0),
                // the footer's Save Result… line, on its left quarter (Contact Sheet…, when
                // offered, is the right half): where the shell measured it
                ("save", _) => snap
                    .save_footer_rect(nid)
                    .map(|f| Point::new(f.min.x + f.width() / 4.0, f.center().y))
                    .ok_or_else(|| format!("face {} has no Save Result… line measured", crate::selection::shown_id(id)))?,
                (other, _) => match PART_NAMES.iter().find(|(n, _)| *n == other) {
                    Some((_, code)) => snap
                        .part_rect(nid, *code)
                        .map(|r| r.center())
                        .ok_or_else(|| format!("face {} has no {other} measured", crate::selection::shown_id(id)))?,
                    None => {
                        return Err(format!(
                            "part {:?}: center, header, body, save, or a part the shell measured (title, chevron, icon, menu, enter, preview)",
                            other.chars().take(32).collect::<String>()
                        ));
                    }
                },
            }
        };
        return Ok(json!({"at": pt(at), "rect": rect_json(r), "nid": nid, "measured": snap.is_measured(measured, id)}));
    }
    if let Some(id) = params.get("wire") {
        let id = id.as_str().ok_or("wire must be a wire's id")?;
        let w = snap.wire_by_id(id).ok_or_else(|| format!("no wire {} drawn", crate::selection::shown_id(id)))?;
        // a point on the chain: the start of its middle cubic
        let segments = w.chain.len().saturating_sub(1) / 3;
        let at =
            w.chain.get(3 * (segments / 2)).copied().ok_or_else(|| format!("wire {} has no path (drawn through a net)", crate::selection::shown_id(id)))?;
        return Ok(json!({"at": pt(at), "nid": w.nid}));
    }
    if let Some(id) = params.get("frame") {
        let id = id.as_str().ok_or("frame must be a frame's id")?;
        let (body, tab, handle) = snap.frame_by_id(id).ok_or_else(|| format!("no frame {}", crate::selection::shown_id(id)))?;
        let at = match part_name.unwrap_or("body") {
            "body" => body.center(),
            "tab" => tab.center(),
            "handle" => handle.center(),
            other => return Err(format!("part {:?}: body, tab or handle", other.chars().take(32).collect::<String>())),
        };
        return Ok(json!({"at": pt(at), "rect": rect_json(body)}));
    }
    Err("name a node, a wire or a frame".into())
}

/// A rect as the scene carries it.
pub fn rect_json(r: Rect) -> [f64; 4] {
    [round(r.min.x), round(r.min.y), round(r.max.x), round(r.max.y)]
}

#[cfg(test)]
mod tests {
    use plumcraft_session::Session;

    use super::*;
    use crate::control::Shell;
    use crate::document::{Document, execute};

    /// Types `x.a`, `x.b`, `x.c`: an `x.a` goes into `x.a`, into `x.b` only through the
    /// converter `t.ab`, never into `x.c`.
    fn catalog() -> SchemaReply {
        serde_json::from_value(json!({
            "wire_types": [{"id": "x.a"}, {"id": "x.b"}, {"id": "x.c"}],
            "nodes": [
                {"id": "t.src", "display": "Source", "inputs": [], "outputs": [{"name": "a", "type": "x.a"}, {"name": "list", "type": "x.a", "list": true}]},
                {"id": "t.b", "display": "Bee", "inputs": [
                    {"name": "b", "type": "x.b", "socket": "socket"}, {"name": "c", "type": "x.c", "socket": "socket"},
                    {"name": "a", "type": "x.a", "socket": "socket"}, {"name": "many", "type": "x.a", "socket": "socket", "list": true},
                    {"name": "k", "type": "FLOAT", "default": 1.0, "socket": "widget"}],
                 "outputs": [{"name": "out", "type": "x.a"}]},
                {"id": "t.ab", "display": "A to B", "inputs": [{"name": "in", "type": "x.a", "socket": "socket"}], "outputs": [{"name": "out", "type": "x.b"}],
                 "converts": [{"from": "x.a", "to": "x.b"}]}
            ]
        }))
        .unwrap()
    }

    fn add(d: &mut Document, kind: &str, pos: [f32; 2]) -> String {
        execute(d, "node.add", &json!({"kind": kind, "pos": pos})).unwrap()["id"].as_str().unwrap().to_owned()
    }

    fn wire(d: &mut Document, from: &str, out: &str, to: &str, inp: &str) {
        execute(d, "wire.connect", &json!({"from": {"node": from, "port": out}, "to": {"node": to, "port": inp}})).unwrap();
    }

    fn measure(d: &mut Document, faces: Value) -> Value {
        execute(d, "geometry.set", &json!({"faces": faces})).unwrap()
    }

    fn snap(d: &mut Document) -> std::sync::Arc<Snapshot> {
        let sc = d.scene().unwrap();
        d.publish_scene(sc);
        std::sync::Arc::clone(&d.snapshot)
    }

    /// A source at (0, 0) 100×60 with outputs at 30 and 45; a Bee at (300, 0) 120×100, header
    /// 24, inputs at 20, 40, 60, 80, 95, a widget row and a title part; its `a` wired from the
    /// source.
    fn two() -> (Document, String, String) {
        let mut d = Document::new(Session::new(catalog()));
        let src = add(&mut d, "t.src", [0.0, 0.0]);
        let b = add(&mut d, "t.b", [300.0, 0.0]);
        wire(&mut d, &src, "a", &b, "a");
        let r = measure(
            &mut d,
            json!([
                {"node": src, "size": [100, 60], "pins": {"out": [30, 45]}},
                {"node": b, "size": [120, 100], "header": 24, "pins": {"in": [20, 40, 60, 80, 95], "out": [50]},
                 "rows": [null, null, null, null, [40, 88, 110, 99]], "parts": {"title": [20, 2, 80, 20]}}
            ]),
        );
        assert_eq!(r, json!({"applied": 2, "stale": [], "unknown": []}));
        (d, src, b)
    }

    #[test]
    fn hostile_geometry_is_refused_or_tamed() {
        for bad in [
            json!(null),
            json!({"faces": 3}),
            json!({"faces": [{"size": [1, 1]}]}),
            json!({"faces": [{"node": "n1"}]}),
            json!({"faces": [{"node": "n1", "size": [1]}]}),
            json!({"faces": [{"node": "n1", "size": ["w", 1]}]}),
            json!({"faces": [{"node": "n1", "size": [1, 1], "x": 1}]}),
            json!({"faces": [{"node": "n1", "size": [1, 1], "pins": {"in": [1, "y"]}}]}),
            json!({"faces": [{"node": "n1", "size": [1, 1], "pins": {"sideways": []}}]}),
            json!({"faces": [{"node": "n1", "size": [1, 1], "rows": [[1, 2, 3]]}]}),
            json!({"faces": [{"node": "n1", "size": [1, 1], "parts": {"tail": [0, 0, 1, 1]}}]}),
            json!({"faces": [{"node": "n1", "size": [1, 1], "rev": -1}]}),
            json!({"faces": [{"node": "", "size": [1, 1]}]}),
            json!({"faces": [{"node": "n1", "size": [1, 1], "pins": {"in": vec![0; MAX_PINS + 1]}}]}),
        ] {
            assert!(parse_set(&bad).is_err(), "{bad}");
        }
        // numbers past what a chain keeps are clamped, sizes are never negative
        let r = parse_set(&json!({"faces": [{"node": "n1", "size": [-5, 1e30], "pins": {"in": [1e30, null, -1e30]}, "header": -3}]})).unwrap();
        let g = &r[0].geometry;
        assert_eq!(g.size, [0.0, MAX_SIZE]);
        assert_eq!(g.inputs, [Some(wire_path::MAX_COORD), None, Some(-wire_path::MAX_COORD)]);
        assert_eq!(g.header, Some(0.0));
        assert!(parse_set(&json!({"faces": []})).unwrap().is_empty());
    }

    #[test]
    fn geometry_set_keeps_what_it_knows_and_says_what_it_doesnt() {
        let (mut d, src, _) = two();
        let rev = snap(&mut d);
        assert_eq!(rev.measured, 2);
        let now = d.scene.face_rev(&src).unwrap();
        let r =
            measure(&mut d, json!([{"node": "n999", "size": [1, 1]}, {"node": src, "rev": now - 1, "size": [101, 60]}, {"node": "group:g9", "size": [1, 1]}]));
        assert_eq!(r, json!({"applied": 1, "stale": [src], "unknown": ["n999", "group:g9"]}), "a stale one is kept all the same");
        assert_eq!(d.geometry.size_of(&src), [101.0, 60.0]);
        assert!(d.scene.watched, "a shell that measures hears when the scene changes");
        // not an undo step; a new graph forgets it
        assert!(d.session.undo_label().is_some_and(|l| l != "Report Geometry"));
        execute(&mut d, "graph.new", &json!({"discard": true})).unwrap();
        assert!(d.geometry.is_empty());
    }

    #[test]
    fn wires_are_published_between_the_measured_pins() {
        let (mut d, src, b) = two();
        let sc = d.scene().unwrap();
        let w = sc.wires.iter().find(|w| w.from_face == src).unwrap();
        assert_eq!(w.path.len(), 3 * wire_path::PLAIN_PIECES + 1);
        assert_eq!(w.path.first(), Some(&[100.0, 30.0]), "from the source's first output, on its right edge");
        assert_eq!(w.path.last(), Some(&[300.0, 60.0]), "into the Bee's third input, on its left edge");
        // unmeasured, the default size and estimated pins
        d.geometry.clear();
        let sc = d.scene().unwrap();
        let w = sc.wires.iter().find(|w| w.to_face == b).unwrap();
        assert_eq!(w.path.first(), Some(&[f64::from(DEFAULT_SIZE[0]), f64::from(DEFAULT_HEADER + DEFAULT_ROW * 0.5)]));
    }

    #[test]
    fn a_point_hits_pins_first_then_faces_then_wires() {
        let (mut d, src, b) = two();
        let s = snap(&mut d);
        let nid = |id: &str| s.face_nid(id).unwrap();
        // a pin, snapped to its point
        let h = s.hit(102.0, 31.0, 5.0);
        assert_eq!((h.kind, h.part, h.node, h.pin, h.x, h.y), (kind::PIN, part::OUTPUT, nid(&src), 0, 100.0, 30.0));
        let h = s.hit(299.0, 59.0, 5.0);
        assert_eq!((h.kind, h.part, h.node, h.pin), (kind::PIN, part::INPUT, nid(&b), 2));
        // a face's parts, a widget row, its header, its body
        assert_eq!((s.hit(350.0, 10.0, 5.0).kind, s.hit(350.0, 10.0, 5.0).part), (kind::NODE, part::TITLE));
        assert_eq!(s.hit(390.0, 10.0, 5.0).part, part::HEADER);
        let row = s.hit(360.0, 92.0, 5.0);
        assert_eq!((row.part, row.index), (part::ROW, 4));
        assert_eq!(s.hit(360.0, 70.0, 0.0).part, part::BODY);
        // the wire, near its middle
        let w = d.scene().unwrap().wires.into_iter().find(|w| w.from_face == src).unwrap();
        let chain: Vec<Point> = w.path.iter().map(|p| Point::new(p[0] as f32, p[1] as f32)).collect();
        let mid = wire_path::chain_middle(&chain).unwrap();
        let h = s.hit(mid.x + 2.0, mid.y, 6.0);
        assert_eq!((h.kind, s.hit_json(&h)["wire"].clone()), (kind::WIRE, json!(w.id)));
        assert_eq!(s.hit(mid.x, mid.y + 40.0, 6.0).kind, kind::NONE, "out of reach");
        // nothing
        assert_eq!(s.hit(5000.0, 5000.0, 12.0), Hit::none(Point::new(5000.0, 5000.0)));
        // the JSON form names what it hit
        let j = s.hit_json(&s.hit(102.0, 31.0, 5.0));
        assert_eq!((j["kind"].clone(), j["node"].clone(), j["side"].clone(), j["type"].clone()), (json!("pin"), json!(src), json!("out"), json!("x.a")));
    }

    #[test]
    fn degenerate_and_hostile_points_never_panic() {
        let (mut d, src, b) = two();
        // a face placed over another is shown out of its way (N4 slice 2: faces never overlap;
        // until then the last drawn was on top here)
        let c = add(&mut d, "t.src", [310.0, 10.0]);
        measure(&mut d, json!([{"node": c, "size": [50, 50]}, {"node": src, "size": [0, 0]}]));
        let s = snap(&mut d);
        let under = s.hit(330.0, 30.0, 0.0);
        assert_eq!((under.kind, under.node), (kind::NODE, s.face_nid(&b).unwrap()), "{b} stays where it is");
        let shown = s.shown_pos(&c).unwrap();
        assert!(shown[1] > 10.0 || shown[0] > 310.0, "{c} pushed off {b}: {shown:?}");
        let moved = s.hit(shown[0] + 25.0, shown[1] + 25.0, 0.0);
        assert_eq!((moved.kind, moved.node), (kind::NODE, s.face_nid(&c).unwrap()));
        // a face of no size hits nothing but its corner, and no query trips on it
        assert_eq!(s.hit(0.0, 0.0, 0.0).node, s.face_nid(&src).unwrap());
        for (x, y, slop) in [
            (f32::NAN, 0.0, 5.0),
            (0.0, f32::INFINITY, 5.0),
            (f32::NEG_INFINITY, f32::NAN, f32::NAN),
            (1e30, -1e30, 1e30),
            (100.0, 30.0, f32::NAN),
            (100.0, 30.0, -4.0),
            (f32::MAX, f32::MIN, f32::INFINITY),
        ] {
            let h = s.hit(x, y, slop);
            assert!(h.x.is_finite() && h.y.is_finite(), "{x} {y} {slop}: {h:?}");
            let t = s.wire_target(s.face_nid(&src).unwrap(), true, 0, x, y, slop);
            assert!(t.x.is_finite(), "{t:?}");
            let mut out = Vec::new();
            s.wire_paths(&[s.face_nid(&src).unwrap()], x, y, &mut out);
            // each wire: its nid (two words) and count (bit-cast), then its points, every one a number
            let mut at = 0;
            while at + 3 <= out.len() {
                let count = out[at + 2].to_bits() as usize;
                let points = out.get(at + 3..at + 3 + 2 * count).expect("as many points as it says");
                assert!(points.iter().all(|v| v.is_finite()), "{x} {y}: {points:?}");
                at += 3 + 2 * count;
            }
            assert_eq!(at, out.len());
        }
        assert_eq!(s.hit(f32::NAN, 0.0, 5.0).kind, kind::NONE);
        // a face far away, sized past reason
        let far = add(&mut d, "t.src", [1e30, -1e30]);
        measure(&mut d, json!([{"node": far, "size": [1e30, 1e30], "pins": {"out": [1e30]}}]));
        let s = snap(&mut d);
        let h = s.hit(wire_path::MAX_COORD, -wire_path::MAX_COORD + 1.0, 1.0);
        assert_eq!(h.kind, kind::NODE);
        // unknown faces and pins
        assert_eq!(s.wire_target(999_999, true, 0, 0.0, 0.0, 5.0).kind, kind::NONE);
        assert_eq!(s.wire_target(s.face_nid(&src).unwrap(), true, 99, 0.0, 0.0, 5.0).kind, kind::NONE);
        let mut out = Vec::new();
        assert_eq!(s.wire_paths(&[999_999, u64::MAX], 1.0, 1.0, &mut out), 0);
    }

    #[test]
    fn bends_tags_frames_and_groups_are_hit_in_order() {
        let (mut d, src, b) = two();
        execute(&mut d, "wire.route", &json!({"to": {"node": b, "port": "a"}, "via": [[200, 200]]})).unwrap();
        let sink = add(&mut d, "t.b", [600.0, 400.0]);
        wire(&mut d, &src, "a", &sink, "a");
        execute(&mut d, "wire.label", &json!({"to": {"node": sink, "port": "a"}, "name": "pics"})).unwrap();
        execute(&mut d, "frame.add", &json!({"title": "F", "pos": [-100, 600], "size": [300, 200]})).unwrap();
        let (g1, g2) = (add(&mut d, "t.src", [1000.0, 0.0]), add(&mut d, "t.src", [1300.0, 0.0]));
        let gid = execute(&mut d, "group.create", &json!({"nodes": [g1, g2], "name": "open"})).unwrap()["id"].as_str().unwrap().to_owned();
        let s = snap(&mut d);
        // a bend point
        let h = s.hit(201.0, 199.0, 2.0);
        assert_eq!((h.kind, h.index), (kind::BEND, 0));
        // a net's tags: the driver beside the source's output, a use beside the sink's input
        let driver = s.hit(100.0 + 14.0 + 8.0, 30.0, 0.0);
        assert_eq!((driver.kind, driver.part), (kind::TAG, part::DRIVER), "{}", s.hit_json(&driver));
        let use_y = 400.0 + DEFAULT_HEADER + DEFAULT_ROW * 2.5;
        let u = s.hit(600.0 - 14.0 - 8.0, use_y, 0.0);
        assert_eq!((u.kind, u.part), (kind::TAG, part::USE));
        assert_eq!(s.hit_json(&u)["net"], json!("pics"));
        // a frame: tab, handle, body
        assert_eq!((s.hit(-90.0, 590.0, 0.0).kind, s.hit(-90.0, 590.0, 0.0).part), (kind::FRAME, part::FRAME_TAB));
        assert_eq!(s.hit(195.0, 795.0, 0.0).part, part::FRAME_HANDLE);
        assert_eq!(s.hit(50.0, 700.0, 0.0).part, part::FRAME_BODY);
        // an open group's band, above its members
        let sc = d.scene().unwrap();
        let g = sc.groups.iter().find(|g| g.id == gid).unwrap();
        let r = g.rect.expect("an open group has a frame");
        assert_eq!(r[1], f64::from(0.0 - GROUP_PAD - GROUP_HEADER));
        assert_eq!(r[2], f64::from(1300.0 + DEFAULT_SIZE[0] + GROUP_PAD));
        let h = s.hit(r[0] as f32 + 100.0, r[1] as f32 + 5.0, 0.0);
        assert_eq!((h.kind, h.part), (kind::GROUP, part::BAND));
        assert_eq!(s.hit_json(&h)["group"], json!(gid));
        // collapsed, it is a face, and has no frame
        execute(&mut d, "group.collapse", &json!({"group": gid, "collapsed": true})).unwrap();
        let sc = d.scene().unwrap();
        assert!(sc.groups.iter().find(|g| g.id == gid).unwrap().rect.is_none());
    }

    #[test]
    fn a_dragged_wire_lands_where_it_fits() {
        let (mut d, src, b) = two();
        let s = snap(&mut d);
        let (from, bee) = (s.face_nid(&src).unwrap(), s.face_nid(&b).unwrap());
        // near input `a` (x.a): it fits; near `b` (x.b): only through the converter; near `c`: no
        let t = s.wire_target(from, true, 0, 298.0, 61.0, 6.0);
        assert_eq!((t.kind, t.node, t.pin, t.fits, t.x, t.y), (kind::PIN, bee, 2, 1, 300.0, 60.0));
        assert_eq!(s.wire_target(from, true, 0, 300.0, 21.0, 6.0).fits, 2);
        assert_eq!(s.wire_target(from, true, 0, 300.0, 40.0, 6.0).fits, 0);
        // a single value can't feed a list input; a list can
        assert_eq!(s.wire_target(from, true, 0, 300.0, 80.0, 6.0).fits, 0);
        assert_eq!(s.wire_target(from, true, 1, 300.0, 80.0, 6.0).fits, 1);
        // between `c` (no) and `a` (yes), within reach of both: the one that fits, though farther
        let t = s.wire_target(from, true, 0, 300.0, 48.0, 13.0);
        assert_eq!((t.pin, t.fits), (2, 1));
        // from an input, the outputs are the targets
        let t = s.wire_target(bee, false, 2, 101.0, 31.0, 5.0);
        assert_eq!((t.kind, t.part, t.node, t.fits), (kind::PIN, part::OUTPUT, from, 1));
        // its own node, and a cycle, don't fit
        assert_eq!(s.wire_target(bee, true, 0, 300.0, 60.0, 5.0).fits, 0, "a node into itself");
        let b2 = add(&mut d, "t.b", [600.0, 0.0]);
        wire(&mut d, &b, "out", &b2, "a");
        let s = snap(&mut d);
        let t = s.wire_target(s.face_nid(&b2).unwrap(), true, 0, 300.0, 60.0, 5.0);
        assert_eq!((t.node, t.fits), (bee, 0), "a cycle");
        // nothing within reach
        assert_eq!(s.wire_target(from, true, 0, 2000.0, 2000.0, 5.0).kind, kind::NONE);
    }

    #[test]
    fn wires_follow_faces_being_dragged() {
        let (mut d, src, b) = two();
        let s = snap(&mut d);
        let mut out = Vec::new();
        let n = s.wire_paths(&[s.face_nid(&src).unwrap()], 10.0, 5.0, &mut out);
        assert_eq!(n, out.len());
        let count = out[2].to_bits() as usize;
        assert_eq!(n, 3 + 2 * count, "one wire: its nid (low and high words), its count, its points");
        assert_eq!((out[3], out[4]), (110.0, 35.0), "its start moved with the source");
        assert_eq!((out[n - 2], out[n - 1]), (300.0, 60.0), "its end stayed");
        let w = d.scene().unwrap().wires.into_iter().find(|w| w.to_face == b).unwrap();
        let published = d.scene.get(None)["wires"].as_array().unwrap().iter().find(|v| v["id"] == json!(w.id)).unwrap()["nid"].as_u64().unwrap();
        assert_eq!(u64::from(out[0].to_bits()) | (u64::from(out[1].to_bits()) << 32), published);
        // both ends moved: the same shape, shifted
        out.clear();
        s.wire_paths(&[s.face_nid(&src).unwrap(), s.face_nid(&b).unwrap()], 10.0, 5.0, &mut out);
        let shifted: Vec<[f64; 2]> = out[3..].chunks(2).map(|c| [f64::from(c[0]), f64::from(c[1])]).collect();
        let at_rest: Vec<[f64; 2]> = w.path.iter().map(|p| [p[0] + 10.0, p[1] + 5.0]).collect();
        for (a, b) in shifted.iter().zip(&at_rest) {
            assert!((a[0] - b[0]).abs() < 2e-3 && (a[1] - b[1]).abs() < 2e-3, "{a:?} {b:?}");
        }
        // the JSON form
        let j = s.wire_paths_json(&[s.face_nid(&src).unwrap()], 0.0, 0.0);
        assert_eq!(j["wires"][0]["path"], json!(w.path), "the JSON form is the scene's");
    }

    #[test]
    fn fit_align_and_frame_use_the_measured_sizes() {
        let (mut d, src, b) = two();
        let r = execute(&mut d, "canvas.fit", &json!({})).unwrap();
        assert_eq!(r, json!({"fit": true, "seq": 1, "rect": [-32.0, -32.0, 452.0, 132.0]}));
        assert_eq!(d.scene().unwrap().view.fit.unwrap().seq, 1, "the scene asks the view for it");
        assert_eq!(execute(&mut d, "canvas.fit", &json!({})).unwrap()["seq"], 2, "a request each time");
        // align right: each node's right edge where the rightmost is, by its measured width
        execute(&mut d, "selection.set", &json!({"nodes": [src, b]})).unwrap();
        execute(&mut d, "selection.align", &json!({"edge": "right"})).unwrap();
        let g = d.session.graph();
        assert_eq!(g.node(&NodeId::from(src.as_str())).unwrap().pos[0], 420.0 - 100.0);
        assert_eq!(g.node(&NodeId::from(b.as_str())).unwrap().pos[0], 300.0);
        assert!(execute(&mut d, "selection.distribute", &json!({"axis": "vertical"})).unwrap_err().contains("at least 3"));
        assert!(execute(&mut d, "selection.align", &json!({"edge": "right", "x": 1})).is_err());
        // Frame Selection, invoked: a frame around them, at the measured sizes
        crate::invoke::invoke(&mut d, "frame.add", crate::asks::By::Script);
        let g = d.session.graph();
        let f = g.frames.values().next().expect("a frame");
        assert!(f.pos[0] < 300.0 && f.pos[0] + f.size[0] >= 420.0 && f.pos[1] + f.size[1] >= 100.0, "{f:?}");
        // Tidy measures what the shell reported
        let (sizes, pins) = crate::invoke::Host::measured(&d);
        assert_eq!(sizes[&src], json!([100.0, 60.0]));
        assert_eq!(pins[&b]["inputs"]["a"], json!(60.0));
        // nothing to fit
        execute(&mut d, "graph.new", &json!({"discard": true})).unwrap();
        assert_eq!(execute(&mut d, "canvas.fit", &json!({})).unwrap()["rect"], Value::Null);
    }

    #[test]
    fn the_snapshot_is_made_again_only_when_its_inputs_change() {
        let (mut d, src, _) = two();
        let a = snap(&mut d);
        let again = snap(&mut d);
        assert!(std::sync::Arc::ptr_eq(&a, &again), "nothing changed");
        // a run state changes the scene but not the snapshot
        d.session.run.nodes.insert(src.clone(), plumcraft_session::run::NodeRun { state: Some(plumcraft_proto::event::NodeState::Done), ..Default::default() });
        assert!(std::sync::Arc::ptr_eq(&a, &snap(&mut d)));
        // geometry, an edit, the group entered: made again
        measure(&mut d, json!([{"node": src, "size": [90, 60]}]));
        let b = snap(&mut d);
        assert!(!std::sync::Arc::ptr_eq(&a, &b));
        execute(&mut d, "node.move", &json!({"id": src, "pos": [5, 5]})).unwrap();
        let c = snap(&mut d);
        assert!(!std::sync::Arc::ptr_eq(&b, &c));
        assert_eq!(c.hit(6.0, 6.0, 0.0).node, c.face_nid(&src).unwrap());
    }
}
