//! The canvas's commands: the selection, the group entered, fit, the palette, reveal, and
//! Delete (stage N2, slice 1; `docs/shell-interface.md` 12.3, rule 4 "everything is a command").
//!
//! Until this slice these were the egui editor's own code. Each is now a command with an id, a
//! label, `enabled` (with a reason) and a run, listed by `command.list`, run by `command.run`,
//! `ui.invoke`, the C ABI and the menu model, through [`crate::document::execute`] like every
//! other command. They are not the session's: they change no graph (Delete runs `node.remove`,
//! which does) and are not journaled; and they leave the status bar's last error and the convert
//! offer as they were, as a click on the canvas did.
//!
//! **Where each runs.** The selection (`selection.*`, `net.select`) and the group entered
//! (`canvas.focus`) are document state, kept in the core ([`crate::selection`],
//! [`crate::document::Document::focus`]). Fit, the palette and reveal are the view's, which only
//! a shell that draws the canvas has ([`CanvasCommand::shell`]): the core checks their params and
//! hands them on through [`crate::document::Holder::shell_command`], the hook every shell-side
//! command already goes through; a shell with no canvas (the C ABI's core today) refuses them
//! with "isn't available from this shell", as it refuses `ui.set`. Every command, core state or
//! not, reaches `shell_command`, so a shell redraws what follows the core.

use plumcraft_graph::{GroupId, NetKey, NodeId};
use plumcraft_session::Session;
use serde_json::{Value, json};

use crate::control::check_fields;
use crate::document::Holder;
use crate::selection::{self, shown_id};

/// One canvas command.
#[derive(Debug, Clone, Copy)]
pub struct CanvasCommand {
    pub id: &'static str,
    pub label: &'static str,
    /// Where the menu model places it (informational in `command.list`, as a registry entry's).
    pub menu: &'static [&'static str],
    /// The key the menu model shows for it, in the registry's notation.
    pub shortcut: Option<&'static str>,
    /// Its parameters and reply, for people and agents.
    pub params: &'static str,
    pub enabled: fn(&Session) -> Result<(), String>,
    /// Only a shell that draws the canvas carries it out (fit, the palette, reveal).
    pub shell: bool,
}

fn always(_: &Session) -> Result<(), String> {
    Ok(())
}

/// Share… needs the graph's file.
fn has_file(s: &Session) -> Result<(), String> {
    if s.path().is_some() { Ok(()) } else { Err("save the graph first: Share sends its file".into()) }
}

/// Why Share… and a pop-out's Share are disabled in a shell with no share sheet
/// ([`crate::document::Document::share`] false, the config's `platform.share: false`).
pub const SHARE_UNAVAILABLE: &str = "Sharing isn't available in this editor";

/// The share commands, which need a shell that answers the `share` ask.
const SHARE_COMMANDS: &[&str] = &["value.share", "graph.share"];

/// Why this shell can't carry out `id` at all, beside the command's own rule: a share command on
/// a shell with no share sheet. The core's answer, so every shell shows it the same way.
pub fn shell_refuses(facts: &crate::menu::Facts, id: &str) -> Option<&'static str> {
    (facts.no_share && SHARE_COMMANDS.contains(&id)).then_some(SHARE_UNAVAILABLE)
}

/// `run.start`'s own rule (an engine connected, a graph to run), for Run Selected; that something
/// is selected is checked when it runs, and by the menu model's item ([`run_selected_reason`]).
fn can_run(s: &Session) -> Result<(), String> {
    plumcraft_session::find("run.start").map_or(Ok(()), |c| (c.enabled)(s))
}

/// Why Run Selected can't run now, if it can't: `run.start`'s reason, or nothing selected.
pub fn run_selected_reason(s: &Session, selected: usize) -> Option<String> {
    if let Err(why) = can_run(s) {
        return Some(why);
    }
    (selected == 0).then(|| "select nodes to run what they need".to_owned())
}

/// A pop-out is open (the value commands act on one).
fn has_popout(s: &Session) -> Result<(), String> {
    if s.views.popouts.is_empty() { Err("no pop-out is open".into()) } else { Ok(()) }
}

const COMMANDS: &[CanvasCommand] = &[
    CanvasCommand {
        id: "value.copy",
        label: "Copy Value",
        menu: &[],
        shortcut: None,
        params: "{popout, pointer?} -> {text, kind} (N4 slice 5: a pop-out's value as text: a scalar's text, a text whole, JSON pretty or the member at pointer, a table as tab-separated rows; invoked, raises the clipboard.set ask with it)",
        enabled: has_popout,
        shell: false,
    },
    CanvasCommand {
        id: "value.export",
        label: "Export Value",
        menu: &[],
        shortcut: None,
        params: "{popout} -> {file, name, bytes} (N4 slice 5: a pop-out's value written to a file of its own under the editor's cache, to drag out: a picture's own file, SVG, text, JSON, a table as CSV, a sound as a float WAV, an array as .npy)",
        enabled: has_popout,
        shell: false,
    },
    CanvasCommand {
        id: "value.share",
        label: "Share Value…",
        menu: &[],
        shortcut: None,
        params: "{popout} -> {file, subject} (N4 slice 5: value.export's file; invoked, raises the share ask with it)",
        enabled: has_popout,
        shell: false,
    },
    CanvasCommand {
        id: "value.saveAs",
        label: "Save Value As…",
        menu: &[],
        shortcut: None,
        params: "{popout, path} -> run.save's reply (N4 slice 5: the engine writes the pop-out's value, its recipe included, by the extension; invoked, raises run.save's file.save ask, purpose result)",
        enabled: has_popout,
        shell: false,
    },
    CanvasCommand {
        id: "selection.set",
        label: "Select",
        menu: &[],
        shortcut: None,
        params: "{nodes?: [node id], groups?: [collapsed group id], mode?: replace|add|remove|toggle (default replace)} -> {nodes, groups, net} (a click replaces, Shift adds, ⌘ removes; a marquee adds)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "selection.clear",
        label: "Select None",
        menu: &[],
        shortcut: None,
        params: "{} -> {nodes, groups, net} (no node or group selected; the net selected stays: net.select)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "selection.all",
        label: "Select All",
        menu: &["Edit"],
        shortcut: Some("Cmd+A"),
        params: "{} -> {nodes, groups, net} (every node shown: an entered group's members only, a collapsed group as its proxy)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "net.select",
        label: "Select Net",
        menu: &[],
        shortcut: None,
        params: "{name?: net name (none or null: no net), group?: group id the net is local to} -> {nodes, groups, net}",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "edit.delete",
        label: "Delete",
        menu: &["Edit"],
        shortcut: Some("Backspace"),
        params: "{} -> node.remove's reply (the selected nodes and the members of selected collapsed groups; one undo step; the selection is cleared)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "canvas.focus",
        label: "Enter Group",
        menu: &[],
        shortcut: None,
        params: "{group: group id | null (leave)} -> {focus: group id | null} (only the group's members are shown and selectable)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "canvas.fit",
        label: "Fit",
        menu: &["View"],
        shortcut: Some("F"),
        params: "{} -> {fit: true, seq, rect: [x0, y0, x1, y1] | null} (the core works out the rect, every face shown and the frames outside an entered group with a margin, from the geometry it has; the scene's view.fit asks the shell's view to show it)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "canvas.zoom",
        label: "Zoom",
        menu: &[],
        shortcut: None,
        params: "{by: factor 0.1 to 10} | {to: zoom 0.1 to 4} -> {seq, by?, to?} (Zoom In, Zoom Out and Actual Size, N5 slice 3: Ctrl+= by 1.25, Ctrl+- by 0.8, Ctrl+0 to 1; the view is the shell's, and the scene's view.zoom asks it to zoom about its centre once per seq)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "canvas.palette",
        label: "Search…",
        menu: &["Node"],
        shortcut: Some("Tab"),
        params: "{open?: bool (default true), at?: [x, y] graph coordinates (default the view's centre), query?: string, wire?: {from: {node, port}} | {to: {node, port}} (a new wire dropped on empty canvas: only kinds that take it are offered)} -> {open, at, query, wire?, offered?} (the node palette, the core's state since N3 slice 1: the scene's view.palette; palette.items lists what it offers, palette.choose adds one there; open false closes it, leaving nothing behind)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "palette.choose",
        label: "Add from the Palette",
        menu: &[],
        shortcut: None,
        params: "{kind: node type, port?: the port to connect (default the first that fits), at?: [x, y] (default where the palette opened)} -> node.add's or node.addWired's reply (one undo step: the node, with the palette's dropped wire connected when it has one); the palette closes",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "selection.nudge",
        label: "Nudge",
        menu: &[],
        shortcut: None,
        params: "{by: [dx, dy] graph units, each within ±10000, drag?: names the key press, as edit.move's} -> edit.move's reply (the selected nodes and collapsed groups moved by it; the arrow keys, 1 unit, 10 with Shift; nudges of the same selection naming the same drag, or none less than a second apart, are one undo step)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "run.selected",
        label: "Run Selected",
        menu: &["Run"],
        shortcut: None,
        params: "{} -> run.start's reply (run.start {targets} with the selected nodes and the members of selected collapsed groups: they and what they need upstream run; the Watched panel's watches as run.start's default). Refused when nothing is selected",
        enabled: can_run,
        shell: false,
    },
    CanvasCommand {
        id: "canvas.reveal",
        label: "Reveal",
        menu: &[],
        shortcut: None,
        params: "{node: node id} | {net: net name, group?} (Go to Next Use: each time the next of the net's uses) -> {node, seq, rect} (the core's view state since N4 slice 3, the scene's view.reveal: the shell centres its view on the face once per seq and highlights it for a moment)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "geometry.set",
        label: "Report Geometry",
        menu: &[],
        shortcut: None,
        params: "{faces: [{node: face id, rev?, size: [w, h], pins?: {in?: [y | null], out?: [y | null]}, header?, rows?: [[x0, y0, x1, y1] | null], parts?: {title?, chevron?, icon?, menu?, enter?, preview?: rect, footer?: [rect]}}]} -> {applied, stale: [face id], unknown: [face id]} (what the shell measured, graph units at zoom 1, pins and rows in row order; the core owns positions)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "geometry.parts",
        label: "Report Face Parts",
        menu: &[],
        shortcut: None,
        params: "{faces: [{node: face id, parts: {title?, chevron?, icon?, menu?, enter?, preview?: rect, footer?: [rect]}}]} -> {applied, unknown: [face id]} (the header's and footer's parts of faces already measured, for a shell that measures with the typed plum_geometry_set, which has no field for them; kept until the face's size changes)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "row.edit",
        label: "Edit Value",
        menu: &[],
        shortcut: None,
        params: "{node?, port?: an input row to edit, open?: bool (default true; false closes the editor), step?: 1 | -1 (Tab, Shift-Tab: the next or previous editable row, from the one open or the one named)} -> {open, node, port, face, row, label, widget} (the field editor's row, the core's state: the scene's view.edit; the shell puts its native control over it. Refused for a row that can't be edited, with why)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "row.set",
        label: "Set Value",
        menu: &[],
        shortcut: None,
        params: "{node, port, text?: what the field holds | value?: a value (a switch's, a choice's, a colour [r, g, b] in 0..1, a scrub's number) | reset: true (the default)} -> {node, port, value, note?, unchanged?, set?: node.set's reply} (any input of the node, shown on its face or not; checked by the row's widget: a number parsed, made whole for an INT and held to its range with a note, a choice one of its own and not a disabled one, a colour in the port's format; then one node.set, one undo step, or none when the value is what the port holds. Refused, changing nothing, with why)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "canvas.selectMode",
        label: "Select Mode",
        menu: &["View"],
        shortcut: None,
        params: "{on?: bool (default: toggle)} -> {on} (N4 slice 3: while on, a one-finger drag on empty canvas draws a marquee rather than panning, on a shell recognising touch; the scene's view.select)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "graph.share",
        label: "Share…",
        menu: &["File"],
        shortcut: None,
        params: "{} -> {file, subject} (N4 slice 3: invoked, raises the share ask with the graph's file for the platform's share sheet; refused until the graph has a file)",
        enabled: has_file,
        shell: false,
    },
    CanvasCommand {
        id: "name.edit",
        label: "Rename",
        menu: &[],
        shortcut: None,
        params: "{node} | {group} | {frame} | {net, group?} | {port: {node, port}} | {slice: {node, port}} | {label: {node, port}} | {promoted: {group, label}}, or {open: false} -> {open, target, text, hint, label, rect} (N4 slice 3: a name edited in place, the core's state: the scene's view.name; the shell puts a text field over rect, graph units; name.set commits it)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "name.set",
        label: "Set Name",
        menu: &[],
        shortcut: None,
        params: "{text} -> {target, ran?: command, result?, unchanged?} (the open name editor's text, committed through the command that owns the name: node.title, group.rename, frame.set, net.rename, graph.port.name or rename or unname, wire.slice, output.label, group.editPromoted; one undo step, none when nothing changes; the editor closes. Refused, the editor staying open, with why)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "selection.align",
        label: "Align",
        menu: &["Edit"],
        shortcut: None,
        params: "{edge: left|right|top|bottom|centerX|centerY} -> edit.align's reply (the selected nodes and collapsed groups, two or more, at the sizes the shell measured; one undo step)",
        enabled: always,
        shell: false,
    },
    CanvasCommand {
        id: "selection.distribute",
        label: "Distribute",
        menu: &["Edit"],
        shortcut: None,
        params: "{axis: horizontal|vertical} -> edit.distribute's reply (the selected nodes and collapsed groups, three or more, at the sizes the shell measured; one undo step)",
        enabled: always,
        shell: false,
    },
];

/// Every canvas command, in `command.list`'s order.
pub fn commands() -> &'static [CanvasCommand] {
    COMMANDS
}

pub fn find(id: &str) -> Option<&'static CanvasCommand> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// Is `id` a canvas command only a shell with a canvas carries out?
pub fn is_shells(id: &str) -> bool {
    find(id).is_some_and(|c| c.shell)
}

/// A `command.list` row, the same shape as a registry command's.
pub fn list_entry(s: &Session, facts: &crate::menu::Facts, c: &CanvasCommand) -> Value {
    let reason = shell_refuses(facts, c.id).map(str::to_owned).or_else(|| (c.enabled)(s).err());
    json!({"id": c.id, "label": c.label, "menu": c.menu, "shortcut": c.shortcut, "params": c.params, "enabled": reason.is_none(), "disabledReason": reason})
}

/// `{group}` of `canvas.focus` (and `ui.set {focus}`): a group the graph has, or null.
pub fn focus_param(session: &Session, v: &Value) -> Result<Option<GroupId>, String> {
    match v {
        Value::Null => Ok(None),
        Value::String(id) => {
            let gid = GroupId::from(id.as_str());
            if session.graph().group(&gid).is_none() {
                return Err(format!("no group {}", shown_id(id)));
            }
            Ok(Some(gid))
        }
        _ => Err("focus must be a group id or null".into()),
    }
}

fn net_param(session: &Session, p: &Value) -> Result<Option<NetKey>, String> {
    check_fields(p, &["name", "group"])?;
    let name = match p.get("name") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(s)) => s.clone(),
        Some(_) => return Err("name must be a net's name or null".into()),
    };
    let group = match p.get("group") {
        None | Some(Value::Null) => None,
        Some(Value::String(g)) => Some(GroupId::from(g.as_str())),
        Some(_) => return Err("group must be a group id or null".into()),
    };
    let key = NetKey::new(group, name);
    if session.graph().net(&key).is_none() {
        return Err(format!("no net {}", shown_id(&key.name)));
    }
    Ok(Some(key))
}

/// `canvas.palette`'s `wire`: `{from: {node, port}}` (an output the graph's node has) or `{to:
/// {node, port}}` (an input address it has).
fn wire_param(session: &Session, v: &Value) -> Result<Option<crate::scene::PaletteWire>, String> {
    if v.is_null() {
        return Ok(None);
    }
    let shape = "wire must be {from: {node, port}} or {to: {node, port}}";
    check_fields(v, &["from", "to"]).map_err(|_| shape.to_owned())?;
    let port = |v: &Value| -> Result<crate::scene::PortAt, String> {
        check_fields(v, &["node", "port"]).map_err(|_| shape.to_owned())?;
        let s = |k: &str| v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 256).map(str::to_owned).ok_or(shape);
        Ok(crate::scene::PortAt { node: s("node")?, port: s("port")? })
    };
    let w = match (v.get("from").filter(|x| !x.is_null()), v.get("to").filter(|x| !x.is_null())) {
        (Some(f), None) => crate::scene::PaletteWire { from: Some(port(f)?), to: None },
        (None, Some(t)) => crate::scene::PaletteWire { from: None, to: Some(port(t)?) },
        _ => return Err(shape.into()),
    };
    let (end, output) = w.end().ok_or(shape)?;
    let graph = session.graph();
    let node = graph.node(&end.node).ok_or_else(|| format!("no node {}", shown_id(&end.node.0)))?;
    let def = session.catalog().node(&node.kind).ok_or_else(|| format!("{} is of a kind no engine has", shown_id(&end.node.0)))?;
    let has = if output { def.output(&end.port).is_some() } else { plumcraft_graph::resolve_input(def, node, &end.node, &end.port).is_ok() };
    if !has {
        return Err(format!("{} has no {} {}", shown_id(&end.node.0), if output { "output" } else { "input" }, shown_id(&end.port)));
    }
    Ok(Some(w))
}

/// `[x, y]` within ±1e7, or none.
fn at_param(p: &Value) -> Result<Option<[f64; 2]>, String> {
    match p.get("at") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let a = v.as_array().filter(|a| a.len() == 2).ok_or("at must be [x, y]")?;
            let c = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|f| f.is_finite() && f.abs() <= 1.0e7).ok_or("at must be [x, y] numbers");
            Ok(Some([c(a.first())?, c(a.get(1))?]))
        }
    }
}

fn palette_param(session: &Session, p: &Value) -> Result<(Value, Option<crate::scene::Palette>), String> {
    check_fields(p, &["open", "at", "query", "wire"])?;
    let open = match p.get("open") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("open must be a boolean".into()),
    };
    let at = match p.get("at") {
        None | Some(Value::Null) => Value::Null,
        Some(v) => {
            let a = v.as_array().filter(|a| a.len() == 2).ok_or("at must be [x, y]")?;
            let c = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|f| f.is_finite() && f.abs() <= 1.0e7).ok_or("at must be [x, y] numbers");
            json!([c(a.first())?, c(a.get(1))?])
        }
    };
    let query = match p.get("query") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(q)) => json!(q.chars().take(256).collect::<String>()),
        Some(_) => return Err("query must be a string".into()),
    };
    let wire = wire_param(session, p.get("wire").unwrap_or(&Value::Null))?;
    let mut reply = json!({"open": open, "at": at, "query": query});
    if !open {
        return Ok((reply, None));
    }
    if let Some(w) = &wire {
        reply["wire"] = serde_json::to_value(w).unwrap_or(Value::Null);
        if let Some((end, output)) = w.end() {
            let starts = plumcraft_session::engine_cmds::startable_defs(session);
            let catalog = crate::palette::with_starts(session.catalog(), &starts);
            reply["offered"] = json!(crate::palette::fitting(session.graph(), &catalog, &end, output).len());
        }
    }
    let state = crate::scene::Palette { at: at_param(p)?, query: query.as_str().map(str::to_owned), wire };
    Ok((reply, Some(state)))
}

/// `{node, port}` of a row command.
fn row_param(p: &Value) -> Result<(String, String), String> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 512).map(str::to_owned);
    match (s("node"), s("port")) {
        (Some(n), Some(k)) => Ok((n, k)),
        _ => Err("name the row: node (a node id) and port (its input's key)".into()),
    }
}

/// `row.edit`: open the field editor on a row, move it (Tab), or close it.
fn row_edit(d: &mut crate::document::Document, params: &Value) -> Result<Value, String> {
    check_fields(params, &["node", "port", "open", "step"])?;
    let open = match params.get("open") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("open must be a boolean".into()),
    };
    let step = match params.get("step") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_i64() {
            Some(1) => Some(1i64),
            Some(-1) => Some(-1),
            _ => return Err("step must be 1 (the next row, Tab) or -1 (the previous, Shift-Tab)".into()),
        },
    };
    if !open {
        if step.is_some() {
            return Err("step moves an open editor: leave out open: false".into());
        }
        d.editing = None;
        return Ok(json!({"open": false}));
    }
    let named = match (params.get("node"), params.get("port")) {
        (None, None) => None,
        _ => Some(row_param(params)?),
    };
    let texts = std::collections::HashMap::new();
    let faces = crate::scene::faces(&d.scene_inputs(&crate::theme::DEFAULT, None, &texts, &[]));
    let target = match step {
        Some(by) => {
            let order = crate::inline::tab_order(&faces);
            if order.is_empty() {
                return Err("no row shown can be edited".into());
            }
            let from = named.clone().or_else(|| d.editing.as_ref().map(|e| (e.node.clone(), e.port.clone())));
            let at = from.and_then(|(n, p)| order.iter().position(|e| e.node == n && e.port == p));
            let n = order.len() as i64;
            let k = match at {
                Some(k) => (k as i64 + by).rem_euclid(n),
                None if by > 0 => 0,
                None => n - 1,
            };
            order.get(usize::try_from(k).unwrap_or(0)).cloned().ok_or("no row to go to")?
        }
        None => {
            let (node, port) = named.ok_or("name the row to edit: node and port")?;
            let (fi, k) = crate::inline::find(&faces, &node, &port).ok_or_else(|| format!("{} has no input {} shown", shown_id(&node), shown_id(&port)))?;
            let (f, r) = (&faces[fi], &faces[fi].inputs[k]);
            if !crate::inline::editable(r) {
                return Err(crate::inline::why_not(r));
            }
            crate::inline::RowEdit { node, port, face: f.id.clone(), row: k }
        }
    };
    let (label, widget) = crate::inline::find(&faces, &target.node, &target.port)
        .and_then(|(fi, k)| faces.get(fi).and_then(|f| f.inputs.get(k)))
        .map(|r| (r.label.clone(), r.widget.clone()))
        .unwrap_or_default();
    d.editing = Some(target.clone());
    Ok(json!({"open": true, "node": target.node, "port": target.port, "face": target.face, "row": target.row, "label": label, "widget": widget}))
}

/// Reveal `node`: the view state a shell follows (its face, or its collapsed group's, where the
/// core has it), the next `seq`.
fn reveal(d: &mut crate::document::Document, node: &str) -> Value {
    let graph = d.session.graph();
    let id = NodeId::from(node);
    let face = graph
        .groups
        .iter()
        .find(|(g, grp)| grp.collapsed && graph.group_members(g).contains(&id))
        .map_or_else(|| node.to_owned(), |(g, _)| format!("group:{g}"));
    let rect = d.snapshot.face_by_id(&face).map_or([0.0, 0.0, 220.0, 160.0], |(_, r)| crate::geometry::rect_json(r));
    let seq = d.reveal.as_ref().map_or(0, |r| r.seq).wrapping_add(1);
    d.reveal = Some(crate::scene::Reveal { seq, node: node.to_owned(), rect });
    json!({"node": node, "seq": seq, "rect": rect})
}

/// Run canvas command `c` (see the module docs): its core part, then the shell's.
pub fn execute<H: Holder>(h: &mut H, c: &CanvasCommand, params: &Value) -> Result<Value, String> {
    if !h.doc().share && SHARE_COMMANDS.contains(&c.id) {
        return Err(format!("{} is not available: {SHARE_UNAVAILABLE}", c.id));
    }
    (c.enabled)(h.session()).map_err(|why| format!("{} is not available: {why}", c.id))?;
    let v = core_part(h, c.id, params)?;
    h.shell_command(c.id, v)
}

fn core_part<H: Holder>(h: &mut H, id: &str, params: &Value) -> Result<Value, String> {
    let d = h.doc_mut();
    match id {
        "selection.set" => {
            let (nodes, groups, mode) = selection::parse_set(d.session.graph(), params)?;
            Ok(d.selection.update(|s| {
                s.apply(mode, nodes, groups);
                s.reply()
            }))
        }
        "selection.clear" => {
            check_fields(params, &[])?;
            Ok(d.selection.update(|s| {
                s.clear();
                s.reply()
            }))
        }
        "selection.all" => {
            check_fields(params, &[])?;
            let (nodes, groups) = selection::all(d.session.graph(), d.focus.as_ref());
            Ok(d.selection.update(|s| {
                s.clear();
                nodes.into_iter().for_each(|n| s.add_node(n));
                groups.into_iter().for_each(|g| s.add_group(g));
                s.reply()
            }))
        }
        "net.select" => {
            let key = net_param(&d.session, params)?;
            Ok(d.selection.update(|s| {
                s.net = key;
                s.reply()
            }))
        }
        "edit.delete" => {
            check_fields(params, &[])?;
            let nodes = d.selection.with(|s| s.members(d.session.graph()));
            if nodes.is_empty() {
                return Err("select nodes to delete first".into());
            }
            let r = h.run("node.remove", &json!({"nodes": nodes}))?;
            h.doc_mut().selection.update(selection::Selection::clear);
            Ok(r)
        }
        "canvas.focus" => {
            check_fields(params, &["group"])?;
            let focus = focus_param(&d.session, params.get("group").unwrap_or(&Value::Null))?;
            d.focus = focus;
            Ok(json!({"focus": d.focus}))
        }
        "canvas.fit" => {
            check_fields(params, &[])?;
            Ok(d.ask_fit())
        }
        "canvas.zoom" => {
            check_fields(params, &["by", "to"])?;
            let num = |k: &str, lo: f64, hi: f64| -> Result<Option<f64>, String> {
                match params.get(k) {
                    None | Some(Value::Null) => Ok(None),
                    Some(v) => v.as_f64().filter(|x| (lo..=hi).contains(x)).map(Some).ok_or_else(|| format!("{k} must be a number from {lo} to {hi}")),
                }
            };
            let (by, to) = (num("by", 0.1, 10.0)?, num("to", 0.1, 4.0)?);
            if by.is_some() == to.is_some() {
                return Err("canvas.zoom takes by (a factor) or to (a zoom), one of them".into());
            }
            let seq = d.zoom.as_ref().map_or(0, |z| z.seq).wrapping_add(1);
            d.zoom = Some(crate::scene::Zoom { seq, by, to });
            let mut out = json!({"seq": seq});
            if let Some(b) = by {
                out["by"] = json!(b);
            }
            if let Some(t) = to {
                out["to"] = json!(t);
            }
            Ok(out)
        }
        "geometry.set" => {
            let reports = crate::geometry::parse_set(params)?;
            Ok(d.apply_geometry(reports))
        }
        "geometry.parts" => {
            let faces = crate::geometry::parse_parts(params)?;
            let (mut applied, mut unknown) = (0u64, Vec::new());
            for (face, parts) in faces {
                match d.geometry.set_parts(&face, parts) {
                    Some(_) => applied += 1,
                    None if unknown.len() < 64 => unknown.push(face),
                    None => {}
                }
            }
            Ok(json!({"applied": applied, "unknown": unknown}))
        }
        "row.edit" => row_edit(d, params),
        "row.set" => {
            check_fields(params, &["node", "port", "text", "value", "reset"])?;
            let (node, port) = row_param(params)?;
            // reset: the default (N4 slice 3, the inspector's ↺): what nobody setting it gives
            let reset = match params.get("reset") {
                None | Some(Value::Null) | Some(Value::Bool(false)) => false,
                Some(Value::Bool(true)) => true,
                Some(_) => return Err("reset must be true or false".into()),
            };
            if reset && (params.get("text").is_some() || params.get("value").is_some()) {
                return Err("reset sets the default: leave out text and value".into());
            }
            let input = if reset {
                let graph = d.session.graph();
                let n = graph.node(&NodeId::from(node.as_str())).ok_or_else(|| format!("no node {}", shown_id(&node)))?;
                let def = d.session.catalog().node(&n.kind).ok_or_else(|| format!("{} is of a kind no engine has", shown_id(&node)))?;
                let row = crate::rows::input_rows(def, &NodeId::from(node.as_str()), graph)
                    .into_iter()
                    .find(|r| r.key == port)
                    .ok_or_else(|| format!("{} has no input {}", shown_id(&node), shown_id(&port)))?;
                let default = def.filled_default(&port, |k| n.values.get(k).filter(|_| k != port)).unwrap_or(&row.port.default).clone();
                crate::inline::Input::parse(&json!({"value": default}))?
            } else {
                crate::inline::Input::parse(params)?
            };
            let texts = std::collections::HashMap::new();
            // any input of the node, shown on its face or not (N4 slice 3: the parameters inspector)
            let nid = NodeId::from(node.as_str());
            if d.session.graph().node(&nid).is_none() {
                return Err(format!("no node {}", shown_id(&node)));
            }
            let rows = crate::scene::param_rows(&d.scene_inputs(&crate::theme::DEFAULT, None, &texts, &[]), &nid);
            let row = &rows.iter().find(|(r, _)| r.key == port).ok_or_else(|| format!("{} has no input {}", shown_id(&node), shown_id(&port)))?.0;
            if !crate::inline::editable(row) {
                return Err(crate::inline::why_not(row));
            }
            let Some(w) = &row.widget else { return Err(crate::inline::why_not(row)) };
            let label = if row.label.is_empty() { port.clone() } else { row.label.clone() };
            let parsed = crate::inline::parse(w, &label, &input)?;
            let mut out = json!({"node": node, "port": port, "value": parsed.value});
            if let Some(n) = &parsed.note {
                out["note"] = json!(n);
            }
            // what the port holds already: no step
            if parsed.value == w.value && !(w.blank && !parsed.value.is_null()) {
                out["unchanged"] = json!(true);
                return Ok(out);
            }
            let r = h.run("node.set", &json!({"node": node, "port": port, "value": parsed.value}))?;
            out["set"] = r;
            Ok(out)
        }
        "selection.align" | "selection.distribute" => {
            let (key, at_least, command) = if id == "selection.align" { ("edge", 2, "edit.align") } else { ("axis", 3, "edit.distribute") };
            check_fields(params, &[key])?;
            let how = params.get(key).and_then(Value::as_str).ok_or_else(|| format!("{key} must be a string"))?.to_owned();
            let graph = d.session.graph();
            let sel = d.selection.get();
            let nodes: Vec<NodeId> = sel.nodes.into_iter().filter(|n| graph.nodes.contains_key(n)).collect();
            let groups = sel.groups;
            if nodes.len() + groups.len() < at_least {
                let what = if id == "selection.align" { "align" } else { "distribute" };
                return Err(format!("select at least {at_least} nodes to {what}"));
            }
            let sizes = d.geometry.sizes_for(&nodes, &groups);
            let mut p = json!({"nodes": nodes, "groups": groups, "sizes": sizes});
            p[key] = json!(how);
            h.run(command, &p)
        }
        "canvas.palette" => {
            let (reply, state) = palette_param(&d.session, params)?;
            d.palette = state;
            Ok(reply)
        }
        "palette.choose" => {
            check_fields(params, &["kind", "port", "at"])?;
            let kind = params.get("kind").and_then(Value::as_str).filter(|k| k.len() <= 256).ok_or("kind must be a node type")?.to_owned();
            let port = match params.get("port") {
                None | Some(Value::Null) => None,
                Some(Value::String(p)) if p.len() <= 256 => Some(p.clone()),
                Some(_) => return Err("port must be a port's name".into()),
            };
            let palette = d.palette.clone();
            let at = at_param(params)?.or_else(|| palette.as_ref().and_then(|p| p.at)).ok_or("the palette has no place to add at: give at")?;
            let pos = json!([at[0], at[1]]);
            let r = match palette.as_ref().and_then(|p| p.wire.as_ref()).and_then(crate::scene::PaletteWire::end) {
                Some((end, output)) => {
                    let mut p = json!({"kind": kind, "pos": pos});
                    p[if output { "from" } else { "to" }] = json!({"node": end.node, "port": end.port});
                    if let Some(port) = port {
                        p["port"] = json!(port);
                    }
                    h.run("node.addWired", &p)?
                }
                None => {
                    if port.is_some() {
                        return Err("port is for a palette opened for a dropped wire".into());
                    }
                    h.run("node.add", &json!({"kind": kind, "pos": pos}))?
                }
            };
            h.doc_mut().palette = None;
            Ok(r)
        }
        "run.selected" => {
            check_fields(params, &[])?;
            let graph = d.session.graph();
            let targets = d.selection.with(|s| s.members(graph));
            if targets.is_empty() {
                return Err("select nodes to run what they need".into());
            }
            let targets: Vec<&str> = targets.iter().map(|n| n.0.as_str()).collect();
            h.run("run.start", &json!({"targets": targets}))
        }
        "selection.nudge" => {
            check_fields(params, &["by", "drag"])?;
            let by = params.get("by").and_then(Value::as_array).filter(|a| a.len() == 2).ok_or("by must be [dx, dy]")?;
            let c = |v: &Value| v.as_f64().filter(|f| f.is_finite() && f.abs() <= 1.0e4).map(|f| f as f32).ok_or("by must be [dx, dy], each within ±10000");
            let (dx, dy) = (c(&by[0])?, c(&by[1])?);
            let graph = d.session.graph();
            let (groups, nodes) = d.selection.with(|s| s.moving(graph));
            if groups.is_empty() && nodes.is_empty() {
                return Err("select nodes to move first".into());
            }
            let place = |p: [f32; 2]| json!([p[0] + dx, p[1] + dy]);
            let groups: serde_json::Map<String, Value> = groups.iter().filter_map(|g| graph.group(g).map(|grp| (g.0.clone(), place(grp.pos)))).collect();
            let nodes: serde_json::Map<String, Value> = nodes.iter().filter_map(|n| graph.node(n).map(|node| (n.0.clone(), place(node.pos)))).collect();
            let mut p = json!({"nodes": nodes, "groups": groups});
            // a shell that knows a key is held names its repeats (a Mac's key repeat), so they
            // are one step however slowly they come; else one second apart at most
            if let Some(drag) = params.get("drag").filter(|v| !v.is_null()) {
                p["drag"] = drag.clone();
            }
            h.run("edit.move", &p)
        }
        "canvas.reveal" => {
            check_fields(params, &["node", "net", "group"])?;
            // a net's tag (N4 slice 3): Go to Next Use, each time the next of its uses
            if params.get("net").is_some() {
                let key = net_param(&d.session, &json!({"name": params.get("net"), "group": params.get("group")}))?.ok_or("net must be a net's name")?;
                let tos: Vec<String> = d.session.graph().net_wires(&key).map(|w| w.to.node.0.clone()).collect();
                if tos.is_empty() {
                    return Err(format!("net {} has no uses", shown_id(&key.name)));
                }
                let k = d.reveal_next.entry(crate::scene::net_id(&key)).or_insert(0);
                let node = tos[*k % tos.len()].clone();
                *k = (*k + 1) % tos.len();
                return Ok(reveal(d, &node));
            }
            let node = params.get("node").and_then(Value::as_str).ok_or("node must be a node id")?.to_owned();
            if !d.session.graph().nodes.contains_key(&NodeId::from(node.as_str())) {
                return Err(format!("no node {}", shown_id(&node)));
            }
            Ok(reveal(d, &node))
        }
        "canvas.selectMode" => {
            check_fields(params, &["on"])?;
            let on = match params.get("on") {
                None | Some(Value::Null) => !d.select_mode,
                Some(Value::Bool(b)) => *b,
                Some(_) => return Err("on must be a boolean".into()),
            };
            d.select_mode = on;
            Ok(json!({"on": on}))
        }
        "value.copy" => crate::export::copy_text(&d.session, &d.media_service, params),
        "value.export" => crate::export::export(&d.session, &d.media_service, params),
        "value.share" => {
            let r = crate::export::export(&d.session, &d.media_service, params)?;
            Ok(json!({"file": r["file"], "subject": r["name"]}))
        }
        "value.saveAs" => {
            check_fields(params, &["popout", "path"])?;
            let id = params.get("popout").and_then(Value::as_u64).ok_or("popout must be a pop-out's id")?;
            let path = params.get("path").and_then(Value::as_str).ok_or("path: where to save (invoke Save Value As… to ask)")?;
            let (w, _) = d.session.run.entry(plumcraft_session::run::WatchRole::Popout(id)).ok_or_else(|| format!("no pop-out {id}"))?;
            let p = json!({"node": w.node, "port": w.port, "index": w.index, "path": path});
            d.session.execute("run.save", &p).map_err(|e| e.to_string())
        }
        "graph.share" => {
            check_fields(params, &[])?;
            let path = d.session.path().ok_or("save the graph first: Share sends its file")?;
            Ok(json!({"file": path, "subject": d.document}))
        }
        "name.edit" => {
            if params.get("open").is_some_and(|v| v == &json!(false)) {
                check_fields(params, &["open"])?;
                d.naming = None;
                return Ok(json!({"open": false}));
            }
            check_fields(params, &["open", "node", "group", "frame", "net", "port", "slice", "label", "promoted"])?;
            if params.get("open").is_some_and(|v| !v.is_boolean()) {
                return Err("open must be a boolean".into());
            }
            let target = crate::naming::parse_target(params)?;
            let edit = crate::naming::open(d, target)?;
            let mut out = serde_json::to_value(&edit).unwrap_or(Value::Null);
            out["open"] = json!(true);
            d.naming = Some(edit);
            Ok(out)
        }
        "name.set" => {
            check_fields(params, &["text"])?;
            let text = params.get("text").and_then(Value::as_str).filter(|t| t.len() <= 4096).ok_or("text must be a string")?;
            if text.chars().any(char::is_control) {
                return Err("a name has no control characters (a new line, a tab)".into());
            }
            let edit = d.naming.clone().ok_or("no name is being edited: name.edit first")?;
            let ran = crate::naming::commit(d, &edit.target, text)?;
            let mut out = json!({"target": edit.target});
            match ran {
                None => {
                    out["unchanged"] = json!(true);
                }
                Some((command, p)) => {
                    let r = h.run(command, &p)?;
                    out["ran"] = json!(command);
                    out["result"] = r;
                }
            }
            h.doc_mut().naming = None;
            Ok(out)
        }
        other => Err(format!("unknown command {:?}", shown_id(other))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asks::Kind;
    use crate::document::{Document, execute};
    use crate::invoke::{self, Host};

    fn doc() -> (Document, String, String) {
        let mut d = Document::new(Session::new(plumcraft_text::engine().schema()));
        let a = execute(&mut d, "node.add", &json!({"kind": "text.literal", "pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
        let b = execute(&mut d, "node.add", &json!({"kind": "text.upper", "pos": [300, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
        (d, a, b)
    }

    #[test]
    fn canvas_commands_change_the_documents_selection_and_nothing_else() {
        let (mut d, a, b) = doc();
        d.last_error = Some("an earlier error".into());
        let undo = d.session.undo_label().map(str::to_owned);
        let r = execute(&mut d, "selection.set", &json!({"nodes": [a], "mode": "add"})).unwrap();
        assert_eq!(r, json!({"nodes": [a], "groups": [], "net": null}));
        assert_eq!(d.last_error.as_deref(), Some("an earlier error"), "a click leaves the status bar as it was");
        assert_eq!(d.session.undo_label().map(str::to_owned), undo, "not an undo step");
        execute(&mut d, "selection.all", &Value::Null).unwrap();
        assert_eq!(Host::selection(&d).0, [a.clone(), b.clone()]);
        assert!(execute(&mut d, "selection.all", &json!({"x": 1})).is_err());
        execute(&mut d, "selection.clear", &json!({})).unwrap();
        assert!(d.selection.get().is_empty());
        // a new graph keeps nothing selected or entered
        execute(&mut d, "selection.set", &json!({"nodes": [b]})).unwrap();
        let gid = execute(&mut d, "group.create", &json!({"nodes": [b]})).unwrap()["id"].as_str().unwrap().to_owned();
        execute(&mut d, "canvas.focus", &json!({"group": gid})).unwrap();
        assert_eq!(d.group_to_ungroup().map(|g| g.0), Some(gid));
        execute(&mut d, "graph.new", &json!({"discard": true})).unwrap();
        assert!(d.selection.get().is_empty() && d.focus.is_none());
    }

    #[test]
    fn share_and_select_mode_are_the_cores_and_share_raises_the_share_ask() {
        let (mut d, _a, _b) = doc();
        // Select mode: a toggle, the scene's view.select
        assert_eq!(execute(&mut d, "canvas.selectMode", &json!({})).unwrap(), json!({"on": true}));
        assert!(d.build_scene(&crate::theme::DEFAULT, None, &std::collections::HashMap::new(), &[]).view.select);
        assert_eq!(execute(&mut d, "canvas.selectMode", &json!({})).unwrap(), json!({"on": false}));
        assert!(execute(&mut d, "canvas.selectMode", &json!({"on": 1})).is_err());
        // Share: refused until there is a file, then the share ask with it
        invoke::invoke(&mut d, "graph.share", crate::asks::By::Script);
        assert!(d.asks.first(Kind::Share).is_none());
        assert!(d.last_error.as_deref().is_some_and(|e| e.contains("save the graph first")), "{:?}", d.last_error);
        let dir = plumcraft_scratch::dir("share");
        let path = dir.join("shared.json");
        execute(&mut d, "graph.saveAs", &json!({"path": path})).unwrap();
        invoke::invoke(&mut d, "graph.share", crate::asks::By::Person);
        let ask = d.asks.first(Kind::Share).expect("a share ask").clone();
        assert_eq!(ask.params["file"], json!(path));
        assert_eq!(ask.by, crate::asks::By::Person);
    }

    #[test]
    fn a_shell_without_a_share_sheet_has_share_refused_with_the_cores_reason() {
        let (mut d, _a, _b) = doc();
        let path = plumcraft_scratch::dir("share-off").join("shared.json");
        execute(&mut d, "graph.saveAs", &json!({"path": path})).unwrap();
        d.share = false;
        let e = execute(&mut d, "graph.share", &json!({})).unwrap_err();
        assert_eq!(e, format!("graph.share is not available: {SHARE_UNAVAILABLE}"));
        invoke::invoke(&mut d, "graph.share", crate::asks::By::Person);
        assert!(d.asks.first(Kind::Share).is_none(), "no share ask is raised");
        let facts = crate::control::Shell::menu_facts(&d);
        let item = crate::menu::bar_menu(&d.session, &facts, "File");
        let share = item.walk().into_iter().find(|i| i.command.as_deref() == Some("graph.share")).cloned().expect("File › Share…");
        assert!(!share.enabled);
        assert_eq!(share.reason.as_deref(), Some(SHARE_UNAVAILABLE));
        // a shell that shares (the default) is as before
        d.share = true;
        assert_eq!(execute(&mut d, "graph.share", &json!({})).unwrap()["file"], json!(path));
    }

    #[test]
    fn names_are_edited_in_place_through_the_core() {
        let (mut d, a, _b) = doc();
        let r = execute(&mut d, "name.edit", &json!({"node": a})).unwrap();
        assert_eq!(r["open"], json!(true));
        assert_eq!(r["label"], json!("Node title"));
        assert!(d.naming.is_some());
        let r = execute(&mut d, "name.set", &json!({"text": "shout"})).unwrap();
        assert_eq!(r["ran"], json!("node.title"));
        assert_eq!(d.session.graph().node(&NodeId::from(a.as_str())).unwrap().title.as_deref(), Some("shout"));
        assert!(d.naming.is_none());
        // the same again: no step
        execute(&mut d, "name.edit", &json!({"node": a})).unwrap();
        assert_eq!(execute(&mut d, "name.set", &json!({"text": "shout"})).unwrap()["unchanged"], json!(true));
        // hostile: nothing open, a target that isn't there, control characters, unknown fields
        assert!(execute(&mut d, "name.set", &json!({"text": "x"})).is_err());
        assert!(execute(&mut d, "name.edit", &json!({"node": "gone"})).is_err());
        assert!(execute(&mut d, "name.edit", &json!({"what": 1})).is_err());
        assert!(execute(&mut d, "name.edit", &json!({})).is_err());
        assert!(execute(&mut d, "name.edit", &json!({"frame": 7})).is_err());
        execute(&mut d, "name.edit", &json!({"node": a})).unwrap();
        assert!(execute(&mut d, "name.set", &json!({"text": "a\nb"})).is_err());
        assert!(d.naming.is_some(), "a refusal leaves the editor open");
        assert_eq!(execute(&mut d, "name.edit", &json!({"open": false})).unwrap(), json!({"open": false}));
        assert!(d.naming.is_none());
    }

    #[test]
    fn invoked_edit_items_fill_in_the_selection() {
        let (mut d, a, b) = doc();
        // nothing selected: the status bar says what to do
        invoke::invoke(&mut d, "edit.copy", crate::asks::By::Script);
        assert_eq!(d.last_error.as_deref(), Some("select nodes to copy first"));
        invoke::invoke(&mut d, "edit.delete", crate::asks::By::Script);
        assert_eq!(d.last_error.as_deref(), Some("select nodes to delete first"));
        // Copy: the selection as workflow JSON, asked onto the clipboard
        execute(&mut d, "selection.set", &json!({"nodes": [a]})).unwrap();
        invoke::invoke(&mut d, "edit.copy", crate::asks::By::Script);
        let ask = d.asks.first(Kind::ClipboardSet).expect("a clipboard.set ask").clone();
        assert!(ask.params["text"].as_str().unwrap().contains("text.literal"), "{:?}", ask.params);
        // Paste asks for the clipboard
        invoke::invoke(&mut d, "edit.paste", crate::asks::By::Script);
        assert!(d.asks.first(Kind::ClipboardGet).is_some());
        // Duplicate selects the copies
        execute(&mut d, "selection.set", &json!({"nodes": [a, b]})).unwrap();
        invoke::invoke(&mut d, "node.duplicate", crate::asks::By::Script);
        let sel = d.selection.get().nodes;
        assert_eq!(sel.len(), 2);
        assert!(!sel.iter().any(|n| n.0 == a || n.0 == b), "the copies are selected: {sel:?}");
        // Delete removes them, and the selection goes
        invoke::invoke(&mut d, "edit.delete", crate::asks::By::Script);
        assert_eq!(d.session.graph().nodes.len(), 2);
        assert!(d.selection.get().is_empty());
    }

    #[test]
    fn a_net_is_selected_by_name_and_forgotten_with_its_net() {
        let (mut d, ..) = doc();
        let r = execute(&mut d, "net.select", &json!({"name": "nope"}));
        assert_eq!(r.unwrap_err(), "no net nope");
        assert!(execute(&mut d, "net.select", &json!({"name": 3})).is_err());
        assert_eq!(execute(&mut d, "net.select", &json!({})).unwrap()["net"], Value::Null);
    }

    /// A new graph's fit waits for its faces to be measured (N2 slice 4): the geometry report
    /// that measures the last one asks for it, once.
    #[test]
    fn a_fit_owed_is_asked_once_every_face_is_measured() {
        let (mut d, a, b) = doc();
        let texts = std::collections::HashMap::new();
        let scene = d.build_scene(&crate::theme::DEFAULT, None, &texts, &[]);
        d.publish_scene(scene);
        d.fit_owed = true;
        let one = |id: &str| crate::geometry::parse_set(&json!({"faces": [{"node": id, "size": [100, 50]}]})).unwrap();
        d.apply_geometry(one(&a));
        assert!(d.fit.is_none() && d.fit_owed, "one face of two measured: no fit yet");
        d.apply_geometry(one(&b));
        let fit = d.fit.clone().expect("asked once both are measured");
        assert_eq!((fit.seq, fit.rect), (1, [-32.0, -32.0, 432.0, 82.0]));
        assert!(!d.fit_owed);
        d.apply_geometry(one(&b));
        assert_eq!(d.fit.as_ref().map(|f| f.seq), Some(1), "paid once");
        // graph.new owes nothing (nothing to show)
        execute(&mut d, "graph.new", &json!({"discard": true})).unwrap();
        assert!(!d.fit_owed);
    }

    #[test]
    fn the_views_commands_are_checked_before_the_shell_is_asked() {
        let (mut d, a, _) = doc();
        for bad in [json!({"open": "yes"}), json!({"at": [1]}), json!({"at": ["x", 2]}), json!({"query": 5}), json!({"zoom": 1})] {
            let e = execute(&mut d, "canvas.palette", &bad).unwrap_err();
            assert!(!e.contains("isn't available"), "{bad}: checked first: {e}");
        }
        assert!(execute(&mut d, "canvas.reveal", &json!({"node": "n999"})).unwrap_err().starts_with("no node"));
        // since N4 slice 3 the core keeps it (the scene's view.reveal), as it keeps fit
        let r = execute(&mut d, "canvas.reveal", &json!({"node": a})).unwrap();
        assert_eq!((r["node"].as_str(), r["seq"].as_u64()), (Some(a.as_str()), Some(1)));
        assert_eq!(d.build_scene(&crate::theme::DEFAULT, None, &std::collections::HashMap::new(), &[]).view.reveal.map(|r| r.seq), Some(1));
        assert_eq!(execute(&mut d, "canvas.reveal", &json!({"node": a})).unwrap()["seq"], json!(2));
    }

    #[test]
    fn zoom_in_out_and_actual_size_are_commands_whose_scene_asks_the_view() {
        let (mut d, _, _) = doc();
        let undo = d.session.undo_label().map(str::to_owned);
        for bad in [json!({}), json!({"by": 1.25, "to": 1}), json!({"by": 0}), json!({"by": 11}), json!({"to": 5}), json!({"by": "in"}), json!({"step": 1})] {
            assert!(execute(&mut d, "canvas.zoom", &bad).is_err(), "{bad}");
        }
        assert!(d.zoom.is_none(), "a refusal asks nothing");
        let view = |d: &mut Document| d.build_scene(&crate::theme::DEFAULT, None, &std::collections::HashMap::new(), &[]).view.zoom;
        assert_eq!(view(&mut d), None);
        assert_eq!(execute(&mut d, "canvas.zoom", &json!({"by": 1.25})).unwrap(), json!({"seq": 1, "by": 1.25}));
        assert_eq!(view(&mut d), Some(crate::scene::Zoom { seq: 1, by: Some(1.25), to: None }));
        assert_eq!(execute(&mut d, "canvas.zoom", &json!({"to": 1})).unwrap(), json!({"seq": 2, "to": 1.0}));
        assert_eq!(view(&mut d), Some(crate::scene::Zoom { seq: 2, by: None, to: Some(1.0) }));
        assert_eq!(d.session.undo_label().map(str::to_owned), undo, "the view's, not an undo step");
    }
}
