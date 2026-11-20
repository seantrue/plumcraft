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

const COMMANDS: &[CanvasCommand] = &[
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
        id: "canvas.palette",
        label: "Search…",
        menu: &["Node"],
        shortcut: Some("Tab"),
        params: "{open?: bool (default true), at?: [x, y] graph coordinates (default the view's centre), query?: string} -> {open, at, query} (the node palette; a node picked is added there)",
        enabled: always,
        shell: true,
    },
    CanvasCommand {
        id: "canvas.reveal",
        label: "Reveal",
        menu: &[],
        shortcut: None,
        params: "{node: node id} -> {node} (the shell centres its view on the node and highlights it for a moment)",
        enabled: always,
        shell: true,
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
pub fn list_entry(s: &Session, c: &CanvasCommand) -> Value {
    let reason = (c.enabled)(s).err();
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

fn palette_param(p: &Value) -> Result<Value, String> {
    check_fields(p, &["open", "at", "query"])?;
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
    Ok(json!({"open": open, "at": at, "query": query}))
}

/// Run canvas command `c` (see the module docs): its core part, then the shell's.
pub fn execute<H: Holder>(h: &mut H, c: &CanvasCommand, params: &Value) -> Result<Value, String> {
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
        "geometry.set" => {
            let reports = crate::geometry::parse_set(params)?;
            Ok(d.apply_geometry(reports))
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
        "canvas.palette" => palette_param(params),
        "canvas.reveal" => {
            check_fields(params, &["node"])?;
            let node = params.get("node").and_then(Value::as_str).ok_or("node must be a node id")?;
            if !d.session.graph().nodes.contains_key(&NodeId::from(node)) {
                return Err(format!("no node {}", shown_id(node)));
            }
            Ok(json!({"node": node}))
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
        assert_eq!(execute(&mut d, "canvas.reveal", &json!({"node": a})).unwrap_err(), "canvas.reveal isn't available from this shell");
    }
}
