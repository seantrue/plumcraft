//! Viewer commands (menu "View"; docs/research/16-viewers.md): pop-out windows, the viewer each
//! uses, and a wire's peek. None is journaled: they don't change the graph.
//!
//! A pop-out follows a port: `view.popout` adds a watch entry for it (role
//! [`WatchRole::Popout`]), so every run sends its display, and `view.close` drops it. The panel's
//! entry and the others are kept. A peek ([`WatchRole::Peek`], one at a time) asks for a port's
//! value for a wire's hover without changing what the panel shows. Each asks for a run when an
//! engine is connected (memo hits make it quick), as `run.view` does.

use std::time::Instant;

use plumcraft_graph::{GraphError, NodeId};
use plumcraft_proto::display::WatchRequest;
use serde::Serialize;
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, opt_str_param, str_param};
use crate::run::WatchRole;
use crate::run_cmds::{default_output, role_param};
use crate::viewer_cmds::{Compare, Tools};
use crate::{Result, Session, SessionError};

/// The most pop-outs open at once.
pub const MAX_POPOUTS: usize = 16;
/// The longest viewer id taken.
const MAX_VIEWER_ID: usize = 64;

/// A pop-out window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Popout {
    pub id: u64,
    /// The viewer asked for (`array.grid`); `None`: the value's default.
    pub viewer: Option<String>,
    /// Its probe and zoom.
    #[serde(skip)]
    pub tools: Tools,
}

/// The pop-outs open, and the viewer the panel uses.
#[derive(Debug, Clone, Default)]
pub struct Views {
    pub popouts: Vec<Popout>,
    /// The viewer the Watched panel uses; `None`: the value's default.
    pub panel_viewer: Option<String>,
    /// The Cache Report window is open.
    pub cache_report: bool,
    /// The About window is open.
    pub about: bool,
    /// The node the parameter editor is open on.
    pub parameters: Option<NodeId>,
    /// The nodes whose own editors are open (`node.editor`), oldest first.
    pub editors: Vec<NodeId>,
    /// The panel's probe and zoom.
    pub tools: Tools,
    /// The compare view (the panel's).
    pub compare: Compare,
    /// A theme asked for, until the window applies it.
    pub theme_asked: Option<String>,
    next: u64,
}

impl Views {
    pub fn popout(&self, id: u64) -> Option<&Popout> {
        self.popouts.iter().find(|p| p.id == id)
    }
}

fn target(s: &Session, p: &Value) -> Result<WatchRequest> {
    let node = NodeId(str_param(p, "node")?.to_owned());
    if s.graph().node(&node).is_none() {
        return Err(GraphError::UnknownNode(node).into());
    }
    let port = match opt_str_param(p, "port")? {
        Some(port) => port.to_owned(),
        None => default_output(s, &node).ok_or_else(|| SessionError::Params(format!("{node} has no outputs")))?,
    };
    let index = match p.get("index") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| SessionError::Params("\"index\" must be a whole number".into()))?,
    };
    Ok(WatchRequest { node: node.0, port, index, ..Default::default() })
}

fn viewer_param(p: &Value) -> Result<Option<String>> {
    match opt_str_param(p, "viewer")? {
        None | Some("") => Ok(None),
        Some(v) if v.chars().count() <= MAX_VIEWER_ID && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-') => {
            Ok(Some(v.to_owned()))
        }
        Some(_) => Err(SessionError::Params("\"viewer\" is a viewer id (array.grid, table.grid, …)".into())),
    }
}

/// Ask for a run that brings every watched display (cached), when an engine is connected.
pub(crate) fn refresh(s: &mut Session) -> Option<String> {
    if !s.connected {
        return None;
    }
    let base = crate::paths::base_folder(s.path());
    let graph = crate::convert::runnable(s, base.as_deref());
    let watch = s.run.watch.clone();
    Some(s.run.request(&graph, Vec::new(), watch, Instant::now()).run_id)
}

/// The app a document at `w` opens in, when PlumCraft has no view of its own for it.
fn opens_in_app(s: &Session, w: &WatchRequest) -> Option<String> {
    let n = s.graph().node(&NodeId(w.node.clone()))?;
    let out = s.catalog().node(&n.kind)?.output(&w.port)?;
    s.docs.for_wire(&out.ty).filter(|a| !a.viewer).map(|a| a.label.clone())
}

fn popout(s: &mut Session, p: &Value) -> Result<Value> {
    let w = target(s, p)?;
    // A craft document PlumCraft can't show opens in its app, whose saves flow downstream
    // (doc_edits); `window: true` asks for PlumCraft's window anyway.
    if p.get("window").and_then(Value::as_bool) != Some(true)
        && let Some(app) = opens_in_app(s, &w)
    {
        let mut out = crate::doc_cmds::open(s, &json!({"node": w.node, "port": w.port}))?;
        out["in_app"] = json!(app);
        return Ok(out);
    }
    let viewer = viewer_param(p)?;
    // The same port and item already open: bring that one forward.
    if let Some(open) =
        s.views.popouts.iter().find(|o| s.run.entry(WatchRole::Popout(o.id)).is_some_and(|(x, _)| x.node == w.node && x.port == w.port && x.index == w.index))
    {
        return Ok(json!({"id": open.id, "node": w.node, "port": w.port, "existing": true}));
    }
    if s.views.popouts.len() >= MAX_POPOUTS {
        return Err(SessionError::Params(format!("at most {MAX_POPOUTS} pop-outs; close one first")));
    }
    s.views.next = s.views.next.saturating_add(1);
    let id = s.views.next;
    s.views.popouts.push(Popout { id, viewer, tools: Tools::default() });
    let mut out = json!({"id": id, "node": w.node, "port": w.port, "index": w.index});
    s.run.set_watch(WatchRole::Popout(id), Some(w));
    out["run_id"] = json!(refresh(s));
    Ok(out)
}

fn close(s: &mut Session, p: &Value) -> Result<Value> {
    let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| SessionError::Params("\"id\" (a pop-out's id) is required".into()))?;
    if s.views.popout(id).is_none() {
        return Err(SessionError::Params(format!("no pop-out {id}")));
    }
    s.views.popouts.retain(|o| o.id != id);
    s.run.set_watch(WatchRole::Popout(id), None);
    Ok(json!({"closed": id}))
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let popouts: Vec<Value> = s
        .views
        .popouts
        .iter()
        .map(|o| {
            let (w, d) = s.run.entry(WatchRole::Popout(o.id)).map_or((None, None), |(w, d)| (Some(w), d));
            json!({"id": o.id, "node": w.map(|w| &w.node), "port": w.map(|w| &w.port), "index": w.map(|w| w.index), "viewer": o.viewer, "summary": d.map(|d| &d.summary)})
        })
        .collect();
    Ok(json!({"popouts": popouts, "panelViewer": s.views.panel_viewer}))
}

/// The viewer the panel (or a pop-out) uses.
pub(crate) fn set_viewer_of(s: &mut Session, role: WatchRole, viewer: Option<String>) -> Result<()> {
    match role {
        WatchRole::Popout(id) => {
            let o = s.views.popouts.iter_mut().find(|o| o.id == id).ok_or_else(|| SessionError::Params(format!("no pop-out {id}")))?;
            o.viewer = viewer;
        }
        _ => s.views.panel_viewer = viewer,
    }
    Ok(())
}

fn set_viewer(s: &mut Session, p: &Value) -> Result<Value> {
    let viewer = viewer_param(p)?;
    set_viewer_of(s, role_param(p)?, viewer.clone())?;
    Ok(json!({"viewer": viewer}))
}

fn peek(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("node").is_none_or(Value::is_null) {
        s.run.set_watch(WatchRole::Peek, None);
        return Ok(json!({"peek": null}));
    }
    let w = target(s, p)?;
    let already = s.run.entry(WatchRole::Peek).is_some_and(|(x, d)| x.node == w.node && x.port == w.port && d.is_some());
    let mut out = json!({"node": w.node, "port": w.port});
    if already {
        return Ok(out);
    }
    s.run.set_watch(WatchRole::Peek, Some(w));
    out["run_id"] = json!(refresh(s));
    Ok(out)
}

fn has_popouts(s: &Session) -> std::result::Result<(), String> {
    if s.views.popouts.is_empty() { Err("no pop-out is open".into()) } else { Ok(()) }
}

/// A pop-out's node went, and its watch with it.
pub(crate) fn prune(s: &mut Session) {
    let run = &s.run;
    s.views.popouts.retain(|p| run.entry(WatchRole::Popout(p.id)).is_some());
    let graph = &s.graph;
    s.views.editors.retain(|n| graph.node(n).is_some());
}

/// The window applies it: themes are the UI's.
fn theme(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name")?.chars().take(MAX_VIEWER_ID).collect::<String>();
    s.views.theme_asked = Some(name.clone());
    Ok(json!({"theme": name}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "view.popout",
            label: "Pop Out",
            menu: &[],
            shortcut: None,
            params: "{node, port?, index?, viewer?, window?} -> {id, node, port, index, run_id?} (or {id, existing: true}): open a window that follows the port's value, updated by every run; it doesn't block the editor. viewer picks one of the value's viewers (array.grid, table.grid, …). A craft document PlumCraft has no view of its own for opens in its app instead, as doc.open does ({in_app, path, …}, or {pending, in_app, run_id}), and a save there feeds the nodes downstream; window: true asks for PlumCraft's window anyway",
            enabled: always,
            run: popout,
            journal: false,
        },
        CommandSpec {
            id: "view.theme",
            label: "Theme",
            menu: &[],
            shortcut: None,
            params: "{name: a theme's name} -> {theme}: the theme of the window, kept between runs",
            enabled: always,
            run: theme,
            journal: false,
        },
        CommandSpec {
            id: "view.close",
            label: "Close Pop-Out",
            menu: &[],
            shortcut: None,
            params: "{id} -> {closed}: close a pop-out and stop watching its port",
            enabled: has_popouts,
            run: close,
            journal: false,
        },
        CommandSpec {
            id: "view.list",
            label: "Pop-Outs",
            menu: &[],
            shortcut: None,
            params: "{} -> {popouts: [{id, node, port, index, viewer, summary}], panelViewer}",
            enabled: always,
            run: list,
            journal: false,
        },
        CommandSpec {
            id: "view.viewer",
            label: "Choose Viewer",
            menu: &[],
            shortcut: None,
            params: "{viewer: id | null, popout?} -> {viewer}: the viewer the panel (or pop-out popout) uses; null: the value's default",
            enabled: always,
            run: set_viewer,
            journal: false,
        },
        CommandSpec {
            id: "view.peek",
            label: "Peek",
            menu: &[],
            shortcut: None,
            params: "{node, port?} | {} -> {node, port, run_id?}: ask for a port's value for a wire's hover, without changing the Watched panel; {} drops the peek",
            enabled: always,
            run: peek,
            journal: false,
        },
    ]
}
