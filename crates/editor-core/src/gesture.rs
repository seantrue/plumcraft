//! What a recognised gesture means (N2 slice 4; `docs/shell-interface.md` 8.13, `ui.gesture`).
//!
//! **The platform recognises, the core decides** (`docs/research/24-native-shells.md` 3, rule 2).
//! A shell recognises a tap, a double tap, a secondary click or long press, a drag (begin, move,
//! end, cancel) or a marquee, in graph units, with the modifiers held, and sends it as
//! `ui.gesture`; the core hit-tests it against the hot calls' snapshot ([`Snapshot::hit`],
//! [`Snapshot::wire_target`]) and says what it means by running the commands every shell already
//! has (`selection.set`, `edit.move`, `wire.connect`, `run.watch`, `canvas.palette`, …). Raw
//! pointer events never cross: what a press, a hold or a flick is, is the platform's.
//!
//! **Drags are stateful.** `begin` decides what the drag is (a move of the faces under it and the
//! selection with them, a wire from a pin or a wire's end picked up off an input, a marquee, a
//! bend point, a frame, a frame's size, an open group) and keeps it in the document's
//! [`DragCell`]; `move` says where it is now (the offset, the wire's target and whether it fits,
//! the marquee's rect) and changes nothing; `end` runs what it means; `cancel` forgets it. So a
//! cancelled drag leaves the document as it was, and a drag runs at most one command that is an
//! undo step: a face drag's `edit.move` (one step, "Move 3 Nodes", since N3 slice 1, named by the
//! drag's serial so the next drag is a step of its own), a frame's or an open group's move, or a
//! wire's `wire.connect`, `wire.reroute`, `wire.disconnect` or `wire.route`. A new wire dropped
//! on nothing opens the palette there for it (`canvas.palette {wire}`, question 8), which is no
//! step until a kind is chosen (`palette.choose`). `move` is also a typed hot call on
//! the C ABI (`plum_gesture_move`), read from the same cell on the shell's thread, so a drag
//! crosses the boundary as JSON only at its two ends.
//!
//! Everything here is hostile input: a point must be two finite numbers within ±1e7, a slop is
//! clamped, an unknown kind, phase, modifier or field is refused, and an `end` or `move` with no
//! drag under way is an error, never a panic.

use std::sync::{Arc, Mutex};

use plumcraft_graph::{GroupId, NodeId, PortRef};
use serde_json::{Value, json};

use crate::control::check_fields;
use crate::geometry::{FooterTap, Hit, Snapshot, kind, part};
use crate::plain::{Point, Rect};

/// The hit distance when a gesture gives none (graph units; the egui canvas's wire hit at zoom 1).
pub const DEFAULT_SLOP: f32 = 12.0;
/// How far a coordinate may be from the origin.
pub const MAX_COORD: f64 = 1.0e7;
/// A drag that ends closer than this to where it began moved nothing (graph units).
pub const STILL: f32 = 1.0e-3;

/// The modifiers held (`mods: ["shift", "primary", "alt", "ctrl"]`; `primary` is ⌘ on Apple
/// platforms and Ctrl elsewhere, as in the menu model's shortcuts).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub primary: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl Mods {
    fn parse(v: Option<&Value>) -> Result<Mods, String> {
        let mut m = Mods::default();
        let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(m) };
        let list = v.as_array().filter(|a| a.len() <= 8).ok_or("mods must be a list of \"shift\", \"primary\", \"alt\", \"ctrl\"")?;
        for x in list {
            match x.as_str() {
                Some("shift") => m.shift = true,
                Some("primary") => m.primary = true,
                Some("alt") => m.alt = true,
                Some("ctrl") => m.ctrl = true,
                _ => return Err("mods must be a list of \"shift\", \"primary\", \"alt\", \"ctrl\"".into()),
            }
        }
        Ok(m)
    }

    /// As `ui.gesture` takes them.
    pub fn json(&self) -> Value {
        let mut v = Vec::new();
        for (on, name) in [(self.shift, "shift"), (self.primary, "primary"), (self.alt, "alt"), (self.ctrl, "ctrl")] {
            if on {
                v.push(name);
            }
        }
        json!(v)
    }
}

/// A drag's phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Begin,
    Move,
    End,
    Cancel,
}

/// One recognised gesture, in graph units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    Tap {
        at: Point,
        mods: Mods,
    },
    DoubleTap {
        at: Point,
        mods: Mods,
    },
    /// A secondary click, a Control-click, a long press.
    Context {
        at: Point,
        mods: Mods,
    },
    Drag {
        phase: Phase,
        at: Point,
        mods: Mods,
    },
    /// A marquee in one go (a Select mode, Pencil): what a drag on empty canvas ends as.
    Marquee {
        at: Point,
        to: Point,
        mods: Mods,
    },
}

/// `ui.gesture`'s params, checked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Request {
    pub gesture: Gesture,
    /// The hit distance, graph units (the shell's target size over its zoom).
    pub slop: f32,
    /// The shell's zoom, screen points a graph unit (N4 slice 2; default 1): a scrub moves a
    /// number by screen points, as egui's drag fields do, whatever the zoom.
    pub zoom: f32,
}

fn point(v: Option<&Value>, name: &str) -> Result<Point, String> {
    let a = v.and_then(Value::as_array).filter(|a| a.len() == 2).ok_or_else(|| format!("{name} must be [x, y] graph coordinates"))?;
    let n = |v: &Value| v.as_f64().filter(|f| f.is_finite() && f.abs() <= MAX_COORD).map(|f| f as f32);
    match (n(&a[0]), n(&a[1])) {
        (Some(x), Some(y)) => Ok(Point::new(x, y)),
        _ => Err(format!("{name} must be [x, y], numbers within ±1e7")),
    }
}

/// `{kind: tap | doubleTap | context | drag | marquee, phase?: begin | move | end | cancel (a
/// drag's), at: [x, y], to?: [x, y] (a marquee's other corner), mods?: [..], slop?}`.
pub fn parse(params: &Value) -> Result<Request, String> {
    check_fields(params, &["kind", "phase", "at", "to", "mods", "slop", "zoom"])?;
    let kind = params.get("kind").and_then(Value::as_str).ok_or("kind must be tap, doubleTap, context, drag or marquee")?;
    let mods = Mods::parse(params.get("mods"))?;
    let slop = match params.get("slop") {
        None | Some(Value::Null) => DEFAULT_SLOP,
        Some(v) => v.as_f64().filter(|f| f.is_finite() && *f >= 0.0).map(|f| f.min(1.0e4) as f32).ok_or("slop must be a distance in graph units, 0 or more")?,
    };
    let zoom = match params.get("zoom") {
        None | Some(Value::Null) => 1.0,
        Some(v) => v
            .as_f64()
            .filter(|f| f.is_finite() && *f > 0.0)
            .map(|f| f.clamp(0.01, 100.0) as f32)
            .ok_or("zoom must be the view's zoom, screen points a graph unit, more than 0")?,
    };
    let phase = match params.get("phase") {
        None | Some(Value::Null) => None,
        Some(v) => Some(match v.as_str() {
            Some("begin") => Phase::Begin,
            Some("move") => Phase::Move,
            Some("end") => Phase::End,
            Some("cancel") => Phase::Cancel,
            _ => return Err("phase must be begin, move, end or cancel".into()),
        }),
    };
    if phase.is_some() && kind != "drag" {
        return Err("phase is a drag's".into());
    }
    if params.get("to").is_some_and(|t| !t.is_null()) && kind != "marquee" {
        return Err("to is a marquee's".into());
    }
    let gesture = match kind {
        "tap" => Gesture::Tap { at: point(params.get("at"), "at")?, mods },
        "doubleTap" => Gesture::DoubleTap { at: point(params.get("at"), "at")?, mods },
        "context" | "longPress" => Gesture::Context { at: point(params.get("at"), "at")?, mods },
        "drag" => {
            let phase = phase.ok_or("a drag needs its phase: begin, move, end or cancel")?;
            let at = if phase == Phase::Cancel && params.get("at").is_none_or(Value::is_null) { Point::ZERO } else { point(params.get("at"), "at")? };
            Gesture::Drag { phase, at, mods }
        }
        "marquee" => Gesture::Marquee { at: point(params.get("at"), "at")?, to: point(params.get("to"), "to")?, mods },
        other => {
            return Err(format!(
                "kind {:?}: one of tap, doubleTap, context, drag, marquee (pan and zoom are the shell's)",
                other.chars().take(32).collect::<String>()
            ));
        }
    };
    Ok(Request { gesture, slop, zoom })
}

/// What a drag is, decided when it began.
#[derive(Debug, Clone, PartialEq)]
pub enum DragKind {
    /// Faces moved: nodes and collapsed groups (members with them), by one `edit.move`.
    Move {
        faces: Vec<u64>,
        nodes: Vec<NodeId>,
        groups: Vec<GroupId>,
    },
    /// A wire: from `anchor` (face nid, output?, pin), the end that stays; `picked` the input
    /// whose wire's end was picked up (a reroute), else a new wire.
    Wire {
        anchor: (u64, bool, u32),
        port: PortRef,
        picked: Option<PortRef>,
    },
    Marquee,
    /// A wire's bend point.
    Bend {
        to: PortRef,
        index: usize,
        via: Vec<[i32; 2]>,
    },
    /// A frame by its tab (`frame.move`, with what it holds).
    Frame {
        frame: String,
    },
    /// A frame's size by its handle.
    Resize {
        frame: String,
    },
    /// An open group by its band (`group.move`).
    Group {
        group: GroupId,
    },
    /// A number scrubbed by a drag on its row (N4 slice 2; egui's drag fields): its value moves
    /// with the pointer, a `row.set` at the end.
    Scrub {
        node: String,
        port: String,
        start: f64,
        widget: Box<crate::scene::Widget>,
    },
}

impl DragKind {
    /// Its code in `plum_drag`: 1 move, 2 wire, 3 marquee, 4 bend, 5 frame, 6 resize, 7 group,
    /// 8 scrub.
    pub fn code(&self) -> u32 {
        match self {
            DragKind::Move { .. } => 1,
            DragKind::Wire { .. } => 2,
            DragKind::Marquee => 3,
            DragKind::Bend { .. } => 4,
            DragKind::Frame { .. } => 5,
            DragKind::Resize { .. } => 6,
            DragKind::Group { .. } => 7,
            DragKind::Scrub { .. } => 8,
        }
    }

    pub fn word(&self) -> &'static str {
        match self {
            DragKind::Move { .. } => "move",
            DragKind::Wire { .. } => "wire",
            DragKind::Marquee => "marquee",
            DragKind::Bend { .. } => "bend",
            DragKind::Frame { .. } => "frame",
            DragKind::Resize { .. } => "resize",
            DragKind::Group { .. } => "group",
            DragKind::Scrub { .. } => "scrub",
        }
    }
}

/// A drag under way.
#[derive(Debug, Clone, PartialEq)]
pub struct Drag {
    /// Rises with every drag a document begins.
    pub serial: u64,
    /// Where it began (graph units).
    pub from: Point,
    pub mods: Mods,
    pub kind: DragKind,
    /// The shell's zoom when it began (a scrub's speed is in screen points).
    pub zoom: f32,
}

/// Where a drag is now ([`Drag::update`], `plum_gesture_move`): nothing in the document changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Update {
    pub code: u32,
    pub serial: u64,
    /// The offset from where it began.
    pub dx: f32,
    pub dy: f32,
    /// A marquee's rect, normalised.
    pub rect: Option<Rect>,
    /// A wire's target: where it would land and whether it fits.
    pub target: Hit,
    /// A scrub's value now.
    pub value: Option<f64>,
}

fn rect_of(a: Point, b: Point) -> Rect {
    Rect::from_min_max(Point::new(a.x.min(b.x), a.y.min(b.y)), Point::new(a.x.max(b.x), a.y.max(b.y)))
}

impl Drag {
    /// Where the drag is with the pointer at `at`: pure, read from `snap`.
    pub fn update(&self, snap: &Snapshot, at: Point, slop: f32) -> Update {
        let (dx, dy) = (crate::wire_path::coord(at.x - self.from.x), crate::wire_path::coord(at.y - self.from.y));
        let target = match &self.kind {
            DragKind::Wire { anchor: (face, output, pin), .. } => snap.wire_target(*face, *output, *pin, at.x, at.y, slop),
            _ => Hit { index: -1, x: at.x, y: at.y, ..Hit::default() },
        };
        let rect = matches!(self.kind, DragKind::Marquee).then(|| rect_of(self.from, at));
        // a scrub moves by screen points right (and up), as egui's drag fields
        let value = match &self.kind {
            DragKind::Scrub { start, widget, .. } => Some(crate::inline::scrub_value(widget, *start, f64::from((dx - dy) * self.zoom))),
            _ => None,
        };
        Update { code: self.kind.code(), serial: self.serial, dx, dy, rect, target, value }
    }
}

/// A document's drag under way, shared with the C ABI's hot call (`plum_gesture_move`), which
/// reads it on the shell's thread. Held only to clone or swap the `Arc`.
#[derive(Debug, Clone, Default)]
pub struct DragCell(Arc<Mutex<DragState>>);

#[derive(Debug, Default)]
struct DragState {
    drag: Option<Arc<Drag>>,
    /// The highest serial a drag has had: serials rise with every drag begun, under way or not
    /// (N2 slice 4 restarted them at 1 once a drag ended; N3 slice 1 names a drag's moves by its
    /// serial, so two drags must not share one).
    last: u64,
    /// The last tap: where, what it hit, and when ([`DOUBLE_TAP`]).
    tap: Option<(Point, Hit, std::time::Instant)>,
}

/// A double tap within this long of a tap, and as near it as the hit distance, opens what that
/// tap hit: the first tap may have changed the faces (a watch that runs grows a face, so a pin at
/// its edge moves), and a double click means what its first click was on.
pub const DOUBLE_TAP: std::time::Duration = std::time::Duration::from_millis(800);

impl DragCell {
    pub fn get(&self) -> Option<Arc<Drag>> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).drag.clone()
    }

    pub fn set(&self, d: Option<Drag>) {
        let mut st = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(d) = &d {
            st.last = st.last.max(d.serial);
        }
        st.drag = d.map(Arc::new);
    }

    /// Keep the last tap's place and hit.
    fn note_tap(&self, at: Point, h: Hit) {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).tap = Some((at, h, std::time::Instant::now()));
    }

    /// The hit of a tap within [`DOUBLE_TAP`] and `slop` of `at`, taken (a double tap uses it once).
    fn take_tap(&self, at: Point, slop: f32) -> Option<Hit> {
        let mut st = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (p, h, when) = st.tap.take()?;
        let near = (p.x - at.x).hypot(p.y - at.y) <= slop.max(4.0);
        (near && when.elapsed() <= DOUBLE_TAP).then_some(h)
    }

    /// The serial the next drag begun gets.
    pub fn next_serial(&self) -> u64 {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).last.wrapping_add(1).max(1)
    }
}

/// What a drag's moves are named by (`edit.move`'s `drag`), so they are one undo step and the
/// next drag's are another.
fn drag_key(d: &Drag) -> String {
    format!("gesture:{}", d.serial)
}

/// What the dispatcher does for a gesture: the commands to run in order, the reply's own
/// fields, and a context menu to build.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub commands: Vec<(&'static str, Value)>,
    pub reply: serde_json::Map<String, Value>,
    /// `menu.get`'s `context` for what was hit (a secondary click or long press).
    pub menu: Option<Value>,
}

/// What the gesture planner reads of the document.
pub struct Facts<'a> {
    pub graph: &'a plumcraft_graph::Graph,
    pub selection: crate::selection::Selection,
    pub drag: &'a DragCell,
}

fn port_json(p: &PortRef) -> Value {
    json!({"node": p.node.0, "port": p.port})
}

/// `selection.set` for a click on face `id` with `mods` (the egui canvas's rules: Shift adds, ⌘
/// removes, else replaces; a collapsed group's face is its group).
fn select(id: &str, mods: Mods) -> (&'static str, Value) {
    let mut p = match id.strip_prefix("group:") {
        Some(g) => json!({"groups": [g]}),
        None => json!({"nodes": [id]}),
    };
    if mods.primary {
        p["mode"] = json!("remove");
    } else if mods.shift {
        p["mode"] = json!("add");
    }
    ("selection.set", p)
}

/// A click on nothing that selects (empty canvas, a wire, a frame's body): the selection goes,
/// unless a modifier was held.
fn clear(mods: Mods, out: &mut Vec<(&'static str, Value)>) {
    if !mods.shift && !mods.primary {
        out.push(("selection.clear", json!({})));
    }
}

fn watch(port: &PortRef) -> (&'static str, Value) {
    ("run.watch", port_json(port))
}

fn hit_reply(snap: &Snapshot, h: &Hit) -> Value {
    snap.hit_json(h)
}

/// Decide what gesture `r` means, from `snap` and `f`; a drag's state is kept in `f.drag`.
pub fn plan(f: &Facts<'_>, snap: &Snapshot, r: &Request) -> Result<Plan, String> {
    let mut p = Plan::default();
    match r.gesture {
        Gesture::Tap { at, mods } => {
            let h = snap.hit(at.x, at.y, r.slop);
            f.drag.note_tap(at, h);
            p.reply.insert("hit".into(), hit_reply(snap, &h));
            // the header's more-button opens the node's menu, as a context gesture there does
            // (N4 slice 2): no command, the menu in the reply
            if h.kind == kind::NODE && h.part == part::MENU {
                if let Some(cx) = context(f, snap, &h) {
                    p.reply.insert("context".into(), cx.clone());
                    p.menu = Some(cx);
                }
            } else if let Some(g) = band_chevron(snap, &h, at) {
                // an open group's band: its chevron collapses it (N4 slice 3), selecting nothing
                p.commands.push(("group.collapse", json!({"group": g, "collapsed": true})));
            } else {
                tap(f, snap, &h, at, mods, &mut p.commands);
            }
        }
        Gesture::DoubleTap { at, .. } => {
            // what the first tap hit, when it was this double tap's ([`DOUBLE_TAP`])
            let h = f.drag.take_tap(at, r.slop).unwrap_or_else(|| snap.hit(at.x, at.y, r.slop));
            p.reply.insert("hit".into(), hit_reply(snap, &h));
            // a double tap on a row that can be edited edits it (the first tap opened it)
            match edit_row(snap, &h) {
                Some(cmd) => p.commands.push(cmd),
                None => double_tap(snap, &h, at, &mut p.commands),
            }
        }
        Gesture::Context { at, .. } => {
            let h = snap.hit(at.x, at.y, r.slop);
            p.reply.insert("hit".into(), hit_reply(snap, &h));
            match context(f, snap, &h) {
                Some(mut cx) => {
                    // a wire's menu puts Add Bend Point where the press was (N4 slice 3), in the
                    // reply's `context` too: the Apple shells ask `menu.get` with that context
                    // themselves, and without `at` the point went to a place of the core's
                    // choosing, under a face or on the wire's old line (B153)
                    if cx["kind"] == "wire" {
                        cx["at"] = json!([at.x, at.y]);
                    }
                    p.reply.insert("context".into(), cx.clone());
                    p.menu = Some(cx);
                }
                None => p.commands.push(("canvas.palette", json!({"open": true, "at": [at.x, at.y]}))),
            }
        }
        Gesture::Marquee { at, to, mods } => {
            p.commands.push(marquee(snap, rect_of(at, to), mods));
        }
        Gesture::Drag { phase, at, mods } => drag(f, snap, phase, at, mods, r.slop, r.zoom, &mut p)?,
    }
    Ok(p)
}

fn tap(f: &Facts<'_>, snap: &Snapshot, h: &Hit, at: Point, mods: Mods, out: &mut Vec<(&'static str, Value)>) {
    match h.kind {
        kind::PIN => {
            // a pin watches its value: an output's own, a wired input's source (an input with no
            // wire has only its own value, which its row shows)
            let output = h.part == part::OUTPUT;
            let Some(port) = snap.pin_port(h.node, output, h.pin) else { return };
            if output {
                out.push(watch(&port));
            } else if let Some(w) = f.graph.wire_into(&port) {
                out.push(watch(&w.from));
            }
        }
        kind::NODE => {
            let Some(id) = snap.face_id_of(h.node) else { return };
            let group = id.strip_prefix("group:").map(GroupId::from);
            match h.part {
                // the header's controls do what they are, and select nothing
                part::CHEVRON => {
                    out.push(match &group {
                        Some(g) => ("group.collapse", json!({"group": g, "collapsed": false})),
                        None => {
                            let collapsed = f.graph.node(&NodeId::from(id)).is_some_and(|n| n.view == plumcraft_graph::NodeView::Collapsed);
                            ("node.view", json!({"id": id, "view": if collapsed { "full" } else { "collapsed" }}))
                        }
                    });
                }
                part::ENTER => {
                    if let Some(g) = &group {
                        out.push(("canvas.focus", json!({"group": g})));
                    }
                }
                // a path row's Browse button (B133): the node selected, and the system's file
                // dialog asked for (`row.browse`, invoked: the core's file ask, then the value set)
                part::ROW if !mods.shift && !mods.primary && browse_row(snap, h, at).is_some() => {
                    out.push(select(id, mods));
                    if let Some(cmd) = browse_row(snap, h, at) {
                        out.push(cmd);
                    }
                }
                // a row whose value has an editor (FR1: on a collapsed group's face, a promoted
                // one): the node is selected and its editor opened, as its footer opens it
                part::ROW if !mods.shift && !mods.primary && h.kind == kind::NODE && snap.row_opens(h.node, h.index).is_some() => {
                    out.push(select(id, mods));
                    if let Some(node) = snap.row_opens(h.node, h.index) {
                        out.push(("node.editor", json!({"node": node})));
                    }
                }
                part::ROW if !mods.shift && !mods.primary && edit_row(snap, h).is_some() => {
                    // a row that can be edited: the node is selected and the row edited (N4
                    // slice 2): a switch flips, anything else opens the shell's field editor
                    out.push(select(id, mods));
                    if let Some(cmd) = edit_row(snap, h) {
                        out.push(cmd);
                    }
                }
                // the footer's Save Result… line saves the value, as its menu item does: the core
                // asks where (and the format's options), in every shell alike; Contact Sheet…,
                // offered on the same line for a list, is its right half
                part::FOOTER
                    if !mods.shift && !mods.primary && group.is_none() && matches!(snap.footer_tap(h.node, h.index), Some((FooterTap::Save { .. }, _))) =>
                {
                    out.push(select(id, mods));
                    if let Some((FooterTap::Save { port, sheet }, rect)) = snap.footer_tap(h.node, h.index) {
                        let right = rect.is_some_and(|r| at.x > (r.min.x + r.max.x) / 2.0);
                        out.push(("run.save", json!({"node": id, "port": port, "sheet": *sheet && right, "ask": true})));
                    }
                }
                _ => {
                    out.push(select(id, mods));
                    // a title, a preview or a footer's excerpt watches the node (when it has an
                    // output to watch)
                    if matches!(h.part, part::TITLE | part::PREVIEW | part::FOOTER) && group.is_none() && snap.pin_count(h.node, true) > 0 {
                        out.push(("run.watch", json!({"node": id})));
                    }
                }
            }
        }
        kind::TAG => {
            let driver = h.part == part::DRIVER;
            let Some((name, group, _)) = snap.tag_of(driver, h.item) else { return };
            let key = plumcraft_graph::NetKey { group: group.as_deref().map(GroupId::from), name: name.clone() };
            if f.selection.net.as_ref() == Some(&key) {
                out.push(("net.select", json!({})));
            } else {
                let mut p = json!({"name": name});
                if let Some(g) = group {
                    p["group"] = json!(g);
                }
                out.push(("net.select", p));
            }
        }
        kind::WIRE => {
            clear(mods, out);
            if let Some(w) = snap.wire_of(h.wire) {
                out.push(watch(&w.from));
            }
        }
        // a frame's tab is the frame's (its menu, its title): nothing under it is clicked
        kind::FRAME if h.part == part::FRAME_TAB => {}
        _ => {
            clear(mods, out);
            if h.kind == kind::NONE && f.selection.net.is_some() && !mods.shift && !mods.primary {
                out.push(("net.select", json!({})));
            }
        }
    }
}

/// A tap on a path row's Browse button (B133: [`crate::geometry::browse_rect`] of the row's
/// widget, on a row that can be edited): `row.browse`. None anywhere else, or when the shell
/// measured no widget rect for the row.
fn browse_row(snap: &Snapshot, h: &Hit, at: Point) -> Option<(&'static str, Value)> {
    if h.kind != kind::NODE || h.part != part::ROW {
        return None;
    }
    let w = snap.row_widget(h.node, h.index).filter(|w| w.editable && w.widget.path_mode.is_some())?;
    let rect = snap.row_rect(h.node, h.index)?;
    crate::geometry::browse_rect(rect).contains(at).then(|| ("row.browse", json!({"node": w.node, "port": w.key})))
}

/// What a tap on an editable row does (N4 slice 2): a switch's `row.set` to the other state, any
/// other widget's `row.edit` (the shell's field editor over it). None for a row that can't be
/// edited (wired, socket-only, inherited, read-only), or a hit that isn't a row.
fn edit_row(snap: &Snapshot, h: &Hit) -> Option<(&'static str, Value)> {
    if h.kind != kind::NODE || h.part != part::ROW {
        return None;
    }
    let w = snap.row_widget(h.node, h.index).filter(|w| w.editable)?;
    if w.widget.kind == "toggle" {
        let on = w.widget.value.as_bool().or_else(|| w.widget.default.as_bool()).unwrap_or(false);
        return Some(("row.set", json!({"node": w.node, "port": w.key, "value": !on})));
    }
    Some(("row.edit", json!({"node": w.node, "port": w.key})))
}

/// The open group whose band's chevron (its first 26 graph units) `at` is on, if any.
fn band_chevron(snap: &Snapshot, h: &Hit, at: Point) -> Option<String> {
    if h.kind != kind::GROUP {
        return None;
    }
    let (id, r) = snap.group_of(h.item)?;
    (at.x <= r.min.x + 26.0 && at.y <= r.min.y + crate::geometry::GROUP_HEADER).then(|| id.to_owned())
}

/// "Open what it is on" (the maintainer's N3 exit card 3; N4 slice 3): a face opens its
/// parameters, a collapsed group's face the group, a value (an output pin, a wired input's, the
/// preview, a text excerpt, a wire) pops out in a window, a frame's tab or an open group's band
/// edits its name, a tag goes to the net's driver, empty canvas opens the palette.
fn double_tap(snap: &Snapshot, h: &Hit, at: Point, out: &mut Vec<(&'static str, Value)>) {
    match h.kind {
        kind::NODE => {
            let Some(id) = snap.face_id_of(h.node) else { return };
            match id.strip_prefix("group:") {
                Some(g) => out.push(("canvas.focus", json!({"group": g}))),
                None if matches!(h.part, part::PREVIEW | part::FOOTER) && snap.pin_count(h.node, true) > 0 => {
                    out.push(("view.popout", json!({"node": id})));
                }
                None => out.push(("node.parameters", json!({"node": id}))),
            }
        }
        kind::PIN => {
            let output = h.part == part::OUTPUT;
            let Some(port) = snap.pin_port(h.node, output, h.pin) else { return };
            if output {
                out.push(("view.popout", port_json(&port)));
            } else if let Some(w) = snap.wire_into(h.node, h.pin) {
                out.push(("view.popout", port_json(&w.from)));
            }
        }
        kind::FRAME if h.part == part::FRAME_TAB => {
            if let Some((id, _)) = snap.frame_of(h.item) {
                out.push(("name.edit", json!({"frame": id})));
            }
        }
        kind::GROUP => {
            if let Some((id, _)) = snap.group_of(h.item) {
                out.push(("name.edit", json!({"group": id})));
            }
        }
        kind::WIRE => {
            if let Some(w) = snap.wire_of(h.wire) {
                out.push(("view.popout", port_json(&w.from)));
            }
        }
        kind::BEND => {
            if let Some(w) = snap.wire_of(h.wire) {
                let mut via = w.via.clone();
                if let Ok(k) = usize::try_from(h.index)
                    && k < via.len()
                {
                    via.remove(k);
                    out.push(("wire.route", json!({"to": port_json(&w.to), "via": via})));
                }
            }
        }
        kind::TAG => {
            // the driver's tag goes to the node that drives the net; a use's to the driver too
            if let Some(id) = snap.face_id_of(h.node).filter(|id| !id.starts_with("group:")) {
                let driver_node = if h.part == part::DRIVER { Some(id.to_owned()) } else { None };
                if let Some(n) = driver_node {
                    out.push(("canvas.reveal", json!({"node": n})));
                }
            }
        }
        kind::NONE => out.push(("canvas.palette", json!({"open": true, "at": [at.x, at.y]}))),
        _ => {}
    }
}

/// The context menu for what was hit, as `menu.get` takes it; none on empty canvas (the
/// palette opens there, as today).
fn context(f: &Facts<'_>, snap: &Snapshot, h: &Hit) -> Option<Value> {
    match h.kind {
        kind::NODE => {
            let id = snap.face_id_of(h.node)?;
            Some(match id.strip_prefix("group:") {
                Some(g) => json!({"kind": "proxy", "group": g}),
                None => json!({"kind": "node", "node": id}),
            })
        }
        kind::PIN => {
            let output = h.part == part::OUTPUT;
            let port = snap.pin_port(h.node, output, h.pin)?;
            Some(json!({"kind": if output { "output" } else { "input" }, "port": port_json(&port)}))
        }
        kind::WIRE | kind::BEND => snap.wire_of(h.wire).map(|w| json!({"kind": "wire", "to": port_json(&w.to)})),
        kind::TAG => {
            let (name, group, wire) = snap.tag_of(h.part == part::DRIVER, h.item)?;
            let mut v = json!({"kind": "net", "net": name});
            if let Some(g) = group {
                v["group"] = json!(g);
            }
            if let Some(w) = wire.and_then(|w| snap.wire_by_id(&w)) {
                v["to"] = port_json(&w.to);
            }
            Some(v)
        }
        kind::FRAME => snap.frame_of(h.item).map(|(id, _)| json!({"kind": "frame", "frame": id})),
        kind::GROUP => snap.group_of(h.item).map(|(id, _)| json!({"kind": "group", "group": id})),
        _ => {
            let _ = f;
            None
        }
    }
}

/// A marquee's `selection.set`: every face it touches; Shift adds, ⌘ takes away, else replaces.
fn marquee(snap: &Snapshot, r: Rect, mods: Mods) -> (&'static str, Value) {
    let (mut nodes, mut groups) = (Vec::new(), Vec::new());
    for (_, id) in snap.faces_touching(r) {
        match id.strip_prefix("group:") {
            Some(g) => groups.push(g.to_owned()),
            None => nodes.push(id.to_owned()),
        }
    }
    let mode = if mods.primary {
        "remove"
    } else if mods.shift {
        "add"
    } else {
        "replace"
    };
    ("selection.set", json!({"nodes": nodes, "groups": groups, "mode": mode}))
}

/// The drag a press at `h` begins.
fn begin(f: &Facts<'_>, snap: &Snapshot, h: &Hit) -> DragKind {
    match h.kind {
        kind::PIN => {
            let output = h.part == part::OUTPUT;
            if !output && let Some(w) = snap.wire_into(h.node, h.pin) {
                // a drag on a wired input picks the wire's end up (open question 17, every platform)
                return DragKind::Wire { anchor: (w.from_pin.0, true, w.from_pin.1), port: w.from, picked: Some(w.to) };
            }
            match snap.pin_port(h.node, output, h.pin) {
                Some(port) => DragKind::Wire { anchor: (h.node, output, h.pin), port, picked: None },
                None => DragKind::Marquee,
            }
        }
        kind::NODE if h.part == part::ROW && snap.row_widget(h.node, h.index).is_some_and(|w| w.editable && w.widget.kind == "number") => {
            // a drag on a number's row scrubs it (N4 slice 2: egui's drag fields; the core's
            // rule, not each shell's)
            match snap.row_widget(h.node, h.index) {
                Some(w) => DragKind::Scrub {
                    node: w.node.clone(),
                    port: w.key.clone(),
                    start: crate::inline::scrub_start(&w.widget),
                    widget: Box::new(w.widget.clone()),
                },
                None => DragKind::Marquee,
            }
        }
        kind::NODE => {
            let Some(id) = snap.face_id_of(h.node) else { return DragKind::Marquee };
            let sel = &f.selection;
            let group = id.strip_prefix("group:").map(GroupId::from);
            let in_selection = match &group {
                Some(g) => sel.groups.contains(g),
                None => sel.nodes.contains(&NodeId::from(id)),
            };
            if !in_selection {
                // a face not selected moves alone, and the selection stays as it was
                return match group {
                    Some(g) => DragKind::Move { faces: vec![h.node], nodes: Vec::new(), groups: vec![g] },
                    None => DragKind::Move { faces: vec![h.node], nodes: vec![NodeId::from(id)], groups: Vec::new() },
                };
            }
            // the whole selection: selected groups not inside another selected one, and the
            // selected nodes their groups don't carry already
            let (groups, nodes) = sel.moving(f.graph);
            let mut faces: Vec<u64> = groups.iter().filter_map(|g| snap.face_by_id(&format!("group:{}", g.0)).map(|(nid, _)| nid)).collect();
            faces.extend(nodes.iter().filter_map(|n| snap.face_by_id(&n.0).map(|(nid, _)| nid)));
            DragKind::Move { faces, nodes, groups }
        }
        kind::BEND => match (snap.wire_of(h.wire), usize::try_from(h.index)) {
            (Some(w), Ok(index)) if index < w.via.len() => DragKind::Bend { to: w.to, index, via: w.via },
            _ => DragKind::Marquee,
        },
        kind::FRAME => match (snap.frame_of(h.item), h.part) {
            (Some((id, _)), part::FRAME_TAB) => DragKind::Frame { frame: id.to_owned() },
            (Some((id, _)), part::FRAME_HANDLE) => DragKind::Resize { frame: id.to_owned() },
            _ => DragKind::Marquee,
        },
        kind::GROUP => match snap.group_of(h.item) {
            Some((id, _)) => DragKind::Group { group: GroupId::from(id) },
            None => DragKind::Marquee,
        },
        // a tag, a wire, empty canvas: a marquee (on touch the shell pans there instead, and
        // sends no drag)
        _ => DragKind::Marquee,
    }
}

fn update_json(snap: &Snapshot, u: &Update) -> Value {
    let mut v = json!({"serial": u.serial, "dx": u.dx, "dy": u.dy});
    if let Some(r) = u.rect {
        v["rect"] = json!(crate::geometry::rect_json(r));
        v["touches"] = json!(snap.faces_touching(r).into_iter().map(|(_, id)| id.to_owned()).collect::<Vec<_>>());
    }
    if u.code == 2 {
        v["target"] = snap.hit_json(&u.target);
    }
    if let Some(value) = u.value {
        v["value"] = json!(value);
    }
    v
}

#[allow(clippy::too_many_arguments)]
fn drag(f: &Facts<'_>, snap: &Snapshot, phase: Phase, at: Point, mods: Mods, slop: f32, zoom: f32, p: &mut Plan) -> Result<(), String> {
    match phase {
        Phase::Begin => {
            let h = snap.hit(at.x, at.y, slop);
            let kind = begin(f, snap, &h);
            let serial = f.drag.next_serial();
            let mut d = json!({"serial": serial, "kind": kind.word()});
            match &kind {
                DragKind::Move { faces, .. } => d["faces"] = json!(faces),
                DragKind::Scrub { node, port, start, .. } => {
                    d["row"] = json!({"node": node, "port": port, "face": snap.face_id_of(h.node), "index": h.index});
                    d["value"] = json!(start);
                }
                DragKind::Wire { anchor: (face, output, pin), picked, .. } => {
                    d["from"] = json!({"node": snap.face_id_of(*face), "nid": face, "side": if *output { "out" } else { "in" }, "pin": pin});
                    if let Some(to) = picked {
                        d["picked"] = port_json(to);
                    }
                }
                _ => {}
            }
            p.reply.insert("hit".into(), snap.hit_json(&h));
            p.reply.insert("drag".into(), d);
            f.drag.set(Some(Drag { serial, from: at, mods, kind, zoom }));
        }
        Phase::Move => {
            let d = f.drag.get().ok_or("no drag under way (begin one first)")?;
            let u = d.update(snap, at, slop);
            p.reply.insert("drag".into(), json!({"serial": d.serial, "kind": d.kind.word()}));
            p.reply.insert("update".into(), update_json(snap, &u));
        }
        Phase::Cancel => {
            let d = f.drag.get();
            f.drag.set(None);
            p.reply.insert("cancelled".into(), json!(d.is_some()));
        }
        Phase::End => {
            let d = f.drag.get().ok_or("no drag under way (begin one first)")?;
            f.drag.set(None);
            let u = d.update(snap, at, slop);
            p.reply.insert("drag".into(), json!({"serial": d.serial, "kind": d.kind.word()}));
            p.reply.insert("update".into(), update_json(snap, &u));
            end(f, snap, &d, &u, at, p);
        }
    }
    Ok(())
}

fn moved(u: &Update) -> bool {
    u.dx.abs() > STILL || u.dy.abs() > STILL
}

fn end(f: &Facts<'_>, snap: &Snapshot, d: &Drag, u: &Update, at: Point, p: &mut Plan) {
    let by = |pos: [f32; 2]| [pos[0] + u.dx, pos[1] + u.dy];
    // a face moves from where it is shown (pushed out of another's way, the scene's make_room)
    let shown = |face: &str, pos: [f32; 2]| by(snap.shown_pos(face).unwrap_or(pos));
    let graph = f.graph;
    let key = drag_key(d);
    match &d.kind {
        DragKind::Move { groups, nodes, .. } => {
            if !moved(u) {
                return;
            }
            // one command, one undo step, named for what moved (N3 slice 1)
            let groups: serde_json::Map<String, Value> =
                groups.iter().filter_map(|g| graph.group(g).map(|grp| (g.0.clone(), json!(shown(&format!("group:{}", g.0), grp.pos))))).collect();
            let nodes: serde_json::Map<String, Value> =
                nodes.iter().filter_map(|n| graph.node(n).map(|node| (n.0.clone(), json!(shown(&n.0, node.pos))))).collect();
            if !groups.is_empty() || !nodes.is_empty() {
                p.commands.push(("edit.move", json!({"nodes": nodes, "groups": groups, "drag": key})));
            }
        }
        DragKind::Wire { anchor: (_, output, _), port, picked } => {
            let t = &u.target;
            let landed = (t.kind == kind::PIN).then(|| snap.pin_port(t.node, t.part == part::OUTPUT, t.pin)).flatten();
            match (picked, landed) {
                (Some(to), Some(new_to)) if &new_to != to => p.commands.push(("wire.reroute", json!({"to": port_json(to), "newTo": port_json(&new_to)}))),
                (Some(_), Some(_)) => {}
                (Some(to), None) => {
                    // dropped on empty canvas: the wire goes (a drop near where it was, on its own
                    // input, keeps it)
                    if moved(u) {
                        p.commands.push(("wire.disconnect", json!({"to": port_json(to)})));
                    }
                }
                (None, Some(other)) => {
                    let (from, to) = if *output { (port, &other) } else { (&other, port) };
                    p.commands.push(("wire.connect", json!({"from": port_json(from), "to": port_json(to)})));
                }
                // a new wire dropped on nothing opens the palette there, offering what takes
                // the wire (question 8, N3 slice 1); a choice adds and connects in one step
                (None, None) => {
                    if moved(u) {
                        let wire = if *output { json!({"from": port_json(port)}) } else { json!({"to": port_json(port)}) };
                        p.commands.push(("canvas.palette", json!({"open": true, "at": [at.x, at.y], "wire": wire})));
                    }
                }
            }
        }
        DragKind::Marquee => {
            if moved(u)
                && let Some(r) = u.rect
            {
                p.commands.push(marquee(snap, r, d.mods));
            }
        }
        DragKind::Bend { to, index, via } => {
            if !moved(u) {
                return;
            }
            let mut via = via.clone();
            if let Some(v) = via.get_mut(*index) {
                let (dx, dy) = (u.dx.round() as i64, u.dy.round() as i64);
                let clamp = |a: i32, b: i64| i32::try_from((i64::from(a) + b).clamp(-10_000_000, 10_000_000)).unwrap_or(a);
                *v = [clamp(v[0], dx), clamp(v[1], dy)];
            }
            p.commands.push(("wire.route", json!({"to": port_json(to), "via": via})));
        }
        DragKind::Frame { frame } => {
            if let Some(fr) = graph.frames.get(&plumcraft_graph::FrameId(frame.clone()))
                && moved(u)
            {
                p.commands.push(("frame.move", json!({"frame": frame, "pos": by(fr.pos), "drag": key})));
            }
        }
        DragKind::Resize { frame } => {
            if let Some(fr) = graph.frames.get(&plumcraft_graph::FrameId(frame.clone()))
                && moved(u)
            {
                p.commands.push(("frame.resize", json!({"frame": frame, "size": [(fr.size[0] + u.dx).max(0.0), (fr.size[1] + u.dy).max(0.0)], "drag": key})));
            }
        }
        DragKind::Group { group } => {
            if let Some(g) = graph.group(group)
                && moved(u)
            {
                p.commands.push(("group.move", json!({"group": group, "pos": by(g.pos), "drag": key})));
            }
        }
        DragKind::Scrub { node, port, start, .. } => {
            // one row.set at the end, none when it came back to where it was
            if let Some(v) = u.value
                && moved(u)
                && v != *start
            {
                p.commands.push(("row.set", json!({"node": node, "port": port, "value": v})));
            }
        }
    }
}

/// `plum_gesture_move`'s reply as JSON (tests compare it with `ui.gesture`'s `move`).
pub fn update_value(snap: &Snapshot, u: &Update) -> Value {
    update_json(snap, u)
}

/// `plum_gesture_move`'s typed answer word for word, as JSON (`geometry.dragMove`, N5 slice 4):
/// `{status, kind, serial, dx, dy, target: {kind, nid, pin, fits, at}, rect?, value?}`, the
/// numbers the C ABI's struct carries (`kind` 1 to 8; a target's `kind` the hit's word), so a
/// driver over the control port checks the same as one holding the C ABI.
pub fn typed_json(u: &Update) -> Value {
    const KINDS: [&str; 8] = ["none", "node", "pin", "wire", "bend", "tag", "frame", "group"];
    let t = &u.target;
    let mut v = json!({
        "status": 0, "kind": u.code, "serial": u.serial, "dx": u.dx, "dy": u.dy,
        "target": {"kind": KINDS.get(t.kind as usize).copied().unwrap_or("?"), "nid": t.node, "pin": t.pin, "fits": t.fits, "at": [t.x, t.y]},
    });
    if let Some(r) = u.rect {
        v["rect"] = json!([r.min.x, r.min.y, r.max.x, r.max.y]);
    }
    if let Some(value) = u.value {
        v["value"] = json!(value);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_gestures_are_refused_never_panicking() {
        let bad = [
            json!(null),
            json!([]),
            json!({}),
            json!({"kind": 5}),
            json!({"kind": "pinch", "at": [0, 0]}),
            json!({"kind": "tap"}),
            json!({"kind": "tap", "at": [1]}),
            json!({"kind": "tap", "at": ["x", 1]}),
            json!({"kind": "tap", "at": [1.0e30, 1]}),
            json!({"kind": "tap", "at": [0, 0], "mods": ["hyper"]}),
            json!({"kind": "tap", "at": [0, 0], "mods": "shift"}),
            json!({"kind": "tap", "at": [0, 0], "slop": -1}),
            json!({"kind": "tap", "at": [0, 0], "phase": "begin"}),
            json!({"kind": "tap", "at": [0, 0], "to": [1, 1]}),
            json!({"kind": "tap", "at": [0, 0], "extra": 1}),
            json!({"kind": "drag", "at": [0, 0]}),
            json!({"kind": "drag", "phase": "sideways", "at": [0, 0]}),
            json!({"kind": "drag", "phase": "move"}),
            json!({"kind": "marquee", "at": [0, 0]}),
            json!({"kind": "x".repeat(100_000), "at": [0, 0]}),
        ];
        for b in bad {
            assert!(parse(&b).is_err(), "{b} was taken");
        }
        let ok = parse(&json!({"kind": "drag", "phase": "cancel"})).unwrap();
        assert!(matches!(ok.gesture, Gesture::Drag { phase: Phase::Cancel, .. }));
        let m = parse(&json!({"kind": "tap", "at": [1, 2], "mods": ["shift", "primary"], "slop": 1e9})).unwrap();
        assert_eq!(m.slop, 1.0e4);
        assert!(matches!(m.gesture, Gesture::Tap { mods: Mods { shift: true, primary: true, .. }, .. }));
    }

    #[test]
    fn a_drag_with_nothing_under_way_is_an_error_and_a_cancel_is_not() {
        let graph = plumcraft_graph::Graph::default();
        let cell = DragCell::default();
        let f = Facts { graph: &graph, selection: crate::selection::Selection::default(), drag: &cell };
        let snap = Snapshot::default();
        for phase in ["move", "end"] {
            let r = parse(&json!({"kind": "drag", "phase": phase, "at": [0, 0]})).unwrap();
            assert!(plan(&f, &snap, &r).is_err(), "{phase}");
        }
        let r = parse(&json!({"kind": "drag", "phase": "cancel"})).unwrap();
        assert_eq!(plan(&f, &snap, &r).unwrap().reply["cancelled"], json!(false));
        // a drag on an empty snapshot is a marquee, and moving it changes nothing
        let r = parse(&json!({"kind": "drag", "phase": "begin", "at": [0, 0]})).unwrap();
        let p = plan(&f, &snap, &r).unwrap();
        assert_eq!(p.reply["drag"]["kind"], "marquee");
        assert!(p.commands.is_empty());
        let r = parse(&json!({"kind": "drag", "phase": "move", "at": [50, 40]})).unwrap();
        let p = plan(&f, &snap, &r).unwrap();
        assert_eq!(p.reply["update"]["rect"], json!([0.0, 0.0, 50.0, 40.0]));
        assert!(p.commands.is_empty());
        let r = parse(&json!({"kind": "drag", "phase": "end", "at": [50, 40]})).unwrap();
        let p = plan(&f, &snap, &r).unwrap();
        assert_eq!(p.commands, vec![("selection.set", json!({"nodes": [], "groups": [], "mode": "replace"}))]);
        assert!(cell.get().is_none(), "an end forgets the drag");
        // a drag's serial rises with every drag begun, ended or not (it names a drag's moves, so
        // two drags are two undo steps; N3 slice 1)
        let begin = parse(&json!({"kind": "drag", "phase": "begin", "at": [0, 0]})).unwrap();
        let serial = |f: &Facts<'_>| plan(f, &snap, &begin).unwrap().reply["drag"]["serial"].as_u64().unwrap();
        let a = serial(&f);
        let r = parse(&json!({"kind": "drag", "phase": "cancel"})).unwrap();
        plan(&f, &snap, &r).unwrap();
        let b = serial(&f);
        assert!(b > a && a > 1, "{a} then {b}");
    }
}
