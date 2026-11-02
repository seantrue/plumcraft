//! The viewer's tools as commands (`viewer.*`): Pin A and Pin B, Compare, the pixel probe, zoom
//! and the sheet. None is journaled: they don't change the graph. The state lives here so the
//! UI, the control channel and MCP all drive the same thing; the viewers read it each frame.
//!
//! A **pin** is a watch entry ([`WatchRole::PinA`], [`WatchRole::PinB`]) copied from the panel's,
//! as the research software's are: it follows its port, so a run sends its display too. The panel's value
//! stands in for a side that isn't pinned.
//!
//! **Zoom** asks for detail: a picture is sent at most [`WatchRequest::px`] on its long side, so
//! at 1:1 a 12-megapixel array wants more than the 1024 it came with. `viewer.zoom` raises `px`
//! to the array's long side (within [`MAX_DETAIL_PX`]) and a run (cached) brings the pictures
//! the engine draws itself; Fit lowers it again for the next run. An engine that sends less shows
//! what it has, and the viewer says how many array pixels a block covers.

use plumcraft_proto::data::Form;
use plumcraft_proto::display::{Display, WatchRequest};
use serde::Serialize;
use serde_json::{Value, json};

use crate::commands::{CommandSpec, opt_str_param};
use crate::run::WatchRole;
use crate::run_cmds::role_param;
use crate::view_cmds::{refresh, set_viewer_of};
use crate::{Result, Session, SessionError};

/// The longest side asked of an engine (the editor's largest texture).
pub const MAX_DETAIL_PX: u32 = 4096;
/// `WatchRequest::px` when no detail is wanted.
pub const DEFAULT_PX: u32 = 1024;
/// The largest zoom asked for, in screen pixels per array pixel.
pub const MAX_ZOOM_PIXELS: u32 = 64;

/// How A and B are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareHow {
    #[default]
    Side,
    Split,
    Difference,
}

/// |A−B| as one picture (the mean of the channels), or one per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffMode {
    #[default]
    Mean,
    Channels,
}

/// The compare view's state.
#[derive(Debug, Clone, PartialEq)]
pub struct Compare {
    pub on: bool,
    pub how: CompareHow,
    pub diff: DiffMode,
    /// Where the split's divider is, 0..1 of the picture's width.
    pub split: f32,
    /// Which of the views A and B show, when they have several.
    pub channel: usize,
}

impl Default for Compare {
    fn default() -> Self {
        Compare { on: false, how: CompareHow::default(), diff: DiffMode::default(), split: 0.5, channel: 0 }
    }
}

/// What a zoom asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Zoom {
    Fit,
    /// One array pixel per screen pixel.
    OneToOne,
    /// So many screen pixels per array pixel.
    Pixels(u32),
    In,
    Out,
}

/// A zoom for the viewer to apply where it knows the view's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ZoomAsk {
    pub to: Option<Zoom>,
    /// Centre this array pixel (row, column).
    pub at: Option<(u64, u64)>,
}

/// One viewer's probe and zoom: the panel's, or a pop-out's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Tools {
    /// The pinned probe (row, column), on every picture shown.
    pub probe: Option<(u64, u64)>,
    /// Taken by the viewer that draws.
    pub zoom: Option<ZoomAsk>,
}

/// Is either side of the compare pinned?
pub fn pinned(s: &Session) -> bool {
    s.run.index_of(WatchRole::PinA).is_some() || s.run.index_of(WatchRole::PinB).is_some()
}

/// Compare is on and has something to compare.
pub fn comparing(s: &Session) -> bool {
    s.views.compare.on && pinned(s)
}

fn has_pin(s: &Session) -> std::result::Result<(), String> {
    if pinned(s) { Ok(()) } else { Err("nothing is pinned: Pin A or Pin B first".into()) }
}

/// The panel shows a value.
fn has_value(s: &Session) -> std::result::Result<(), String> {
    if s.run.panel().and_then(|(_, d)| d).is_some() { Ok(()) } else { Err("nothing is watched yet".into()) }
}

fn has_watch(s: &Session) -> std::result::Result<(), String> {
    if s.run.watch.is_empty() { Err("nothing is watched".into()) } else { Ok(()) }
}

/// The panel's or a pop-out's value is a list.
fn has_list(s: &Session) -> std::result::Result<(), String> {
    if s.run.watched.iter().flatten().any(|d| d.items > 1) { Ok(()) } else { Err("the watched value is not a list".into()) }
}

/// A slot's role: `"a"` or `"b"`.
fn slot_param(p: &Value) -> Result<Option<WatchRole>> {
    match opt_str_param(p, "slot")? {
        None => Ok(None),
        Some("a" | "A") => Ok(Some(WatchRole::PinA)),
        Some("b" | "B") => Ok(Some(WatchRole::PinB)),
        Some(other) => Err(SessionError::Params(format!("\"slot\" is \"a\" or \"b\", not {other:?}"))),
    }
}

/// Keep the panel's value as A or B, and show the compare. A left out: A, or B when A is pinned.
fn pin(s: &mut Session, p: &Value) -> Result<Value> {
    let role = slot_param(p)?.unwrap_or(if s.run.index_of(WatchRole::PinA).is_some() { WatchRole::PinB } else { WatchRole::PinA });
    let (w, d) = s.run.panel().ok_or_else(|| SessionError::Params("nothing is watched".into()))?;
    let (w, d) = (w.clone(), d.cloned());
    let out = json!({"slot": if role == WatchRole::PinA { "a" } else { "b" }, "node": w.node, "port": w.port, "index": w.index});
    s.run.set_watch(role, Some(w));
    // what the panel shows now: there is nothing to wait for
    s.run.set_display(role, d);
    s.views.compare.on = true;
    Ok(out)
}

/// Forget A and B.
fn unpin(s: &mut Session, p: &Value) -> Result<Value> {
    match slot_param(p)? {
        Some(role) => s.run.set_watch(role, None),
        None => {
            s.run.set_watch(WatchRole::PinA, None);
            s.run.set_watch(WatchRole::PinB, None);
        }
    }
    if !pinned(s) {
        s.views.compare.on = false;
    }
    Ok(json!({"pinned": pinned(s)}))
}

fn compare(s: &mut Session, p: &Value) -> Result<Value> {
    let c = &mut s.views.compare;
    let bad = |what: &str| SessionError::Params(what.to_owned());
    let mut next = c.clone();
    if let Some(v) = p.get("on").filter(|v| !v.is_null()) {
        next.on = v.as_bool().ok_or_else(|| bad("\"on\" must be true or false"))?;
    }
    match opt_str_param(p, "how")? {
        None => {}
        Some("side") => next.how = CompareHow::Side,
        Some("split") => next.how = CompareHow::Split,
        Some("difference") => next.how = CompareHow::Difference,
        Some(other) => return Err(bad(&format!("\"how\" is side, split or difference, not {other:?}"))),
    }
    match opt_str_param(p, "diff")? {
        None => {}
        Some("mean") => next.diff = DiffMode::Mean,
        Some("channels") => next.diff = DiffMode::Channels,
        Some(other) => return Err(bad(&format!("\"diff\" is mean or channels, not {other:?}"))),
    }
    if let Some(v) = p.get("split").filter(|v| !v.is_null()) {
        let f = v.as_f64().filter(|f| f.is_finite()).ok_or_else(|| bad("\"split\" must be a number from 0 to 1"))?;
        next.split = f.clamp(0.0, 1.0) as f32;
    }
    if let Some(v) = p.get("channel").filter(|v| !v.is_null()) {
        next.channel = v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad("\"channel\" must be a whole number"))?;
    }
    *c = next;
    Ok(json!({"on": c.on, "how": c.how, "diff": c.diff, "split": c.split, "channel": c.channel}))
}

/// The tools of the panel, or of pop-out `role`.
fn tools_mut(s: &mut Session, role: WatchRole) -> Result<&mut Tools> {
    match role {
        WatchRole::Popout(id) => {
            s.views.popouts.iter_mut().find(|o| o.id == id).map(|o| &mut o.tools).ok_or_else(|| SessionError::Params(format!("no pop-out {id}")))
        }
        _ => Ok(&mut s.views.tools),
    }
}

/// A (row, column) in `name`: `[row, col]`.
fn pixel_param(p: &Value, name: &str) -> Result<Option<(u64, u64)>> {
    match p.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_array().map(Vec::as_slice) {
            Some([r, c]) => match (r.as_u64(), c.as_u64()) {
                (Some(r), Some(c)) => Ok(Some((r, c))),
                _ => Err(SessionError::Params(format!("{name:?} is [row, column], whole numbers"))),
            },
            _ => Err(SessionError::Params(format!("{name:?} is [row, column]"))),
        },
    }
}

/// Pin the probe at an array pixel, or (left out) unpin it.
fn probe(s: &mut Session, p: &Value) -> Result<Value> {
    let at = match (p.get("row").and_then(Value::as_u64), p.get("col").and_then(Value::as_u64)) {
        (Some(r), Some(c)) => Some((r, c)),
        (None, None) if p.get("row").is_none_or(Value::is_null) && p.get("col").is_none_or(Value::is_null) => None,
        _ => return Err(SessionError::Params("\"row\" and \"col\" are whole numbers, both or neither".into())),
    };
    tools_mut(s, role_param(p)?)?.probe = at;
    Ok(json!({"probe": at.map(|(r, c)| [r, c])}))
}

/// The long side of a display's array, when the picture sent can be smaller than it.
fn long_side(d: &Display) -> Option<u32> {
    match d.full.as_ref().map(|f| &f.meta.form) {
        Some(Form::Array { shape, .. }) if matches!(shape.len(), 2 | 3) => {
            let long = shape.iter().take(2).copied().max()?;
            Some(u32::try_from(long).unwrap_or(u32::MAX))
        }
        _ => None,
    }
}

/// Raise `px` on the entries `roles` to what their arrays want. True when one changed.
fn want_detail(s: &mut Session, roles: &[WatchRole]) -> bool {
    let mut raised = false;
    for role in roles {
        let long = s.run.entry(*role).and_then(|(_, d)| d).and_then(long_side);
        if let (Some(long), Some(w)) = (long, s.run.entry_mut(*role)) {
            let want = long.min(MAX_DETAIL_PX);
            if want > w.px {
                w.px = want;
                raised = true;
            }
        }
    }
    raised
}

/// Back to the size pictures are sent at, from the next run.
fn no_detail(s: &mut Session, roles: &[WatchRole]) {
    for role in roles {
        if let Some(w) = s.run.entry_mut(*role) {
            w.px = w.px.min(DEFAULT_PX);
        }
    }
}

fn zoom_param(p: &Value) -> Result<(Option<Zoom>, bool)> {
    let bad =
        || SessionError::Params(format!("\"to\" is \"fit\", \"1:1\", \"in\", \"out\", \"detail\" or screen pixels per array pixel, 1 to {MAX_ZOOM_PIXELS}"));
    match p.get("to") {
        None | Some(Value::Null) => Ok((None, false)),
        Some(Value::String(t)) => match t.as_str() {
            "fit" => Ok((Some(Zoom::Fit), false)),
            "1:1" => Ok((Some(Zoom::OneToOne), true)),
            "in" => Ok((Some(Zoom::In), true)),
            "out" => Ok((Some(Zoom::Out), false)),
            "detail" => Ok((None, true)),
            _ => Err(bad()),
        },
        Some(v) => {
            let n = v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| (1..=MAX_ZOOM_PIXELS).contains(n)).ok_or_else(bad)?;
            Ok((Some(Zoom::Pixels(n)), true))
        }
    }
}

fn zoom(s: &mut Session, p: &Value) -> Result<Value> {
    let role = role_param(p)?;
    let (to, detail) = zoom_param(p)?;
    let at = pixel_param(p, "at")?;
    if s.run.entry(role).is_none() {
        return Err(SessionError::Params("nothing is watched there".into()));
    }
    if to.is_some() || at.is_some() {
        tools_mut(s, role)?.zoom = Some(ZoomAsk { to, at });
    }
    let roles: Vec<WatchRole> = if matches!(role, WatchRole::Popout(_)) { vec![role] } else { vec![WatchRole::Panel, WatchRole::PinA, WatchRole::PinB] };
    let mut out = json!({"to": p.get("to"), "at": at.map(|(r, c)| [r, c])});
    if to == Some(Zoom::Fit) {
        no_detail(s, &roles);
    } else if detail && want_detail(s, &roles) {
        out["run_id"] = json!(refresh(s));
    }
    Ok(out)
}

/// A list's items as a labelled grid: the watch asks for their pictures, and the items viewer
/// shows them. `index` shows that item as it closes the sheet.
fn recipe(s: &mut Session, p: &Value) -> Result<Value> {
    let role = role_param(p)?;
    let recipe = s.run.entry(role).and_then(|(_, d)| d).and_then(|d| d.recipe.clone());
    Ok(json!({"recipe": recipe}))
}

fn sheet(s: &mut Session, p: &Value) -> Result<Value> {
    let role = role_param(p)?;
    let w = s.run.entry_mut(role).ok_or_else(|| SessionError::Params("nothing is watched there".into()))?;
    let on = match p.get("on") {
        None | Some(Value::Null) => !w.sheet,
        Some(v) => v.as_bool().ok_or_else(|| SessionError::Params("\"on\" must be true or false".into()))?,
    };
    let was = w.sheet;
    let index = match p.get("index") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_i64().ok_or_else(|| SessionError::Params("\"index\" must be a whole number".into()))?),
    };
    w.sheet = on;
    if let Some(i) = index {
        w.index = i;
    }
    let changed = index.is_some() || (on && !was);
    set_viewer_of(s, role, on.then(|| ITEMS.to_owned()))?;
    let mut out = json!({"on": on});
    if changed {
        out["run_id"] = json!(refresh(s));
    }
    Ok(out)
}

/// The viewer that shows a list's sheet.
const ITEMS: &str = "items";

/// Forget pins and tools (a new graph).
pub(crate) fn clear(s: &mut Session) {
    s.views.tools = Tools::default();
    s.views.compare = Compare::default();
}

/// Drop the pins whose node is gone.
pub(crate) fn prune(s: &mut Session) {
    for role in [WatchRole::PinA, WatchRole::PinB] {
        let gone = s.run.entry(role).is_some_and(|(w, _): (&WatchRequest, _)| s.graph().node(&w.node.as_str().into()).is_none());
        if gone {
            s.run.set_watch(role, None);
        }
    }
    if !pinned(s) {
        s.views.compare.on = false;
    }
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "viewer.pin",
            label: "Pin for Compare",
            menu: &[],
            shortcut: None,
            params: "{slot?: \"a\" | \"b\"} -> {slot, node, port, index}: keep the Watched panel's value as A or B and show the compare (left out: A, or B when A is pinned). A side not pinned is the panel's value",
            enabled: has_value,
            run: pin,
            journal: false,
        },
        CommandSpec {
            id: "viewer.unpin",
            label: "Unpin",
            menu: &[],
            shortcut: None,
            params: "{slot?: \"a\" | \"b\"} -> {pinned}: forget a pin, or both",
            enabled: has_pin,
            run: unpin,
            journal: false,
        },
        CommandSpec {
            id: "viewer.compare",
            label: "Compare",
            menu: &[],
            shortcut: None,
            params: "{on?, how?: \"side\" | \"split\" | \"difference\", diff?: \"mean\" | \"channels\", split?: 0..1, channel?} -> the compare's state: A and B side by side, as a split, or as their exact difference from the full arrays",
            enabled: has_pin,
            run: compare,
            journal: false,
        },
        CommandSpec {
            id: "viewer.probe",
            label: "Pin Probe",
            menu: &[],
            shortcut: None,
            params: "{row, col, popout?} | {popout?} -> {probe}: pin the pixel probe at an array pixel (or unpin it). ui.inspect's watched.view.probe reads the value there, from the array's own numbers",
            enabled: has_watch,
            run: probe,
            journal: false,
        },
        CommandSpec {
            id: "viewer.zoom",
            label: "Zoom Picture",
            menu: &[],
            shortcut: None,
            params: "{to?: \"fit\" | \"1:1\" | \"in\" | \"out\" | \"detail\" | screen pixels per array pixel (1..64), at?: [row, col], popout?} -> {to, at, run_id?}: zoom the picture, with one image pixel as one nearest-neighbour block; a run (cached) brings a picture big enough, when the array is larger than the one shown. at centres that pixel. detail asks for that picture without moving the view",
            enabled: has_watch,
            run: zoom,
            journal: false,
        },
        CommandSpec {
            id: "viewer.recipe",
            label: "Recipe",
            menu: &[],
            shortcut: None,
            params: "{popout?, copy?: bool} -> {recipe} (what made the value the panel, or pop-out popout, shows: {style?, params, settings, graph}, or null; copy puts it on the clipboard as JSON)",
            enabled: crate::commands::always,
            run: recipe,
            journal: false,
        },
        CommandSpec {
            id: "viewer.sheet",
            label: "Sheet",
            menu: &[],
            shortcut: None,
            params: "{on?, index?, popout?} (toggles when on is left out) -> {on, run_id?}: every item of the watched list as a labelled grid, or one at a time; index shows that item",
            enabled: has_list,
            run: sheet,
            journal: false,
        },
    ]
}
