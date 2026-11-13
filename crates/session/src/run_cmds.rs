//! Run commands (menu "Run"): start, cancel, auto-run, and choosing what the Watched panel
//! shows. None of them is journaled: they don't change the graph. Lifted from the research software's
//! `run_cmds.rs`, with `run.inspect` and `run.viewer` becoming `run.watch`.
//!
//! The session can't reach the engine itself: `run.start` leaves a request in
//! [`crate::run::RunState`]'s outbox, and whatever hosts the session sends it over its
//! `Connection` and feeds the events back.

use std::time::Instant;

use plumcraft_graph::{GraphError, NodeId};
use plumcraft_proto::display::WatchRequest;
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, opt_str_param, str_param};
use crate::run::WatchRole;
use crate::{Result, Session, SessionError};

fn can_run(s: &Session) -> std::result::Result<(), String> {
    if !s.connected {
        return Err("the engine is not connected".into());
    }
    if s.graph().nodes.is_empty() {
        return Err("the graph is empty".into());
    }
    Ok(())
}

fn is_running(s: &Session) -> std::result::Result<(), String> {
    if s.run.running { Ok(()) } else { Err("no run in progress".into()) }
}

fn watch_list(p: &Value) -> Result<Option<Vec<WatchRequest>>> {
    match p.get("watch") {
        None | Some(Value::Null) => Ok(None),
        Some(v @ Value::Array(_)) => serde_json::from_value(v.clone()).map(Some).map_err(|e| SessionError::Params(format!("\"watch\": {e}"))),
        Some(_) => Err(SessionError::Params("\"watch\" must be a list of {node, port}".into())),
    }
}

fn start(s: &mut Session, p: &Value) -> Result<Value> {
    // Empty targets: the engine runs the graph's sinks (spec 8.1), whose ancestors are every
    // other node.
    let targets = crate::commands::id_list_param(p, "targets")?;
    let watch = watch_list(p)?.unwrap_or_else(|| s.run.watch.clone());
    // Relative path inputs are relative to the workflow's folder (the editor's, unsaved): the
    // engine gets them absolute.
    let base = crate::paths::base_folder(s.path());
    let graph = crate::convert::runnable(s, base.as_deref());
    let req = s.run.request(&graph, targets, watch, Instant::now());
    let mut out = json!({"run_id": req.run_id, "generation": req.generation});
    if s.run.awaiting_review() {
        out["held"] = json!("this workflow came from elsewhere: see graph.review, then graph.trust");
    }
    Ok(out)
}

fn cancel(s: &mut Session, _: &Value) -> Result<Value> {
    s.run.cancel();
    Ok(json!({"generation": s.run.generation}))
}

fn auto(s: &mut Session, p: &Value) -> Result<Value> {
    let on = match p.get("on") {
        None | Some(Value::Null) => !s.run.auto,
        Some(v) => v.as_bool().ok_or_else(|| SessionError::Params("\"on\" must be true or false".into()))?,
    };
    s.run.set_auto(on, Instant::now());
    Ok(json!({"auto": on}))
}

/// The output a click on a node watches: its first output.
pub fn default_output(s: &Session, node: &NodeId) -> Option<String> {
    let def = s.catalog().node(&s.graph().node(node)?.kind)?;
    def.outputs.first().map(|o| o.name.clone())
}

fn watch(s: &mut Session, p: &Value) -> Result<Value> {
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
    s.run.set_watch(WatchRole::Panel, Some(WatchRequest { node: node.0.clone(), port: port.clone(), index, ..Default::default() }));
    Ok(json!({"node": node, "port": port, "index": index}))
}

/// Whose entry a command acts on: `{popout: id}`'s window, else the Watched panel.
pub fn role_param(p: &Value) -> Result<WatchRole> {
    match p.get("popout") {
        None | Some(Value::Null) => Ok(WatchRole::Panel),
        Some(v) => v.as_u64().map(WatchRole::Popout).ok_or_else(|| SessionError::Params("\"popout\" must be a pop-out's id".into())),
    }
}

/// The panel's or a pop-out's value is a list.
fn has_list(s: &Session) -> std::result::Result<(), String> {
    if s.run.watched.iter().flatten().any(|d| d.items > 1) { Ok(()) } else { Err("the watched value is not a list".into()) }
}

/// Show another item of the watched list: the watch asks for it, and a run (all cached)
/// brings its display.
fn item(s: &mut Session, p: &Value) -> Result<Value> {
    let index = p
        .get("index")
        .and_then(Value::as_i64)
        .ok_or_else(|| SessionError::Params("\"index\" (a whole number; negative counts from the end) is required".into()))?;
    let role = role_param(p)?;
    let w = s.run.entry_mut(role).ok_or_else(|| SessionError::Params("nothing is watched there".into()))?;
    w.index = index;
    let watch = s.run.watch.clone();
    if !s.connected {
        return Ok(json!({"index": index}));
    }
    let base = crate::paths::base_folder(s.path());
    let graph = crate::convert::runnable(s, base.as_deref());
    let req = s.run.request(&graph, Vec::new(), watch, Instant::now());
    Ok(json!({"index": index, "run_id": req.run_id}))
}

/// The views the watched value's wire type offers (spec 7.2), when it offers any.
pub fn offered_views(s: &Session) -> Vec<String> {
    offered_views_for(s, WatchRole::Panel)
}

/// The views the value `role` watches offers.
pub fn offered_views_for(s: &Session, role: WatchRole) -> Vec<String> {
    let Some(d) = s.run.entry(role).and_then(|(_, d)| d) else { return Vec::new() };
    s.catalog().wire_types.iter().find(|w| w.id == d.ty).map(|w| w.views.clone()).unwrap_or_default()
}

/// The panel's or a pop-out's value offers views.
fn has_views(s: &Session) -> std::result::Result<(), String> {
    let any = (0..s.run.watch.len()).any(|i| !offered_views_for(s, s.run.role(i)).is_empty());
    if any { Ok(()) } else { Err("the watched value has no views to choose from".into()) }
}

/// Show the watched value in another view (an array: `heat`, `labels`, `angle` with its
/// `turn`, …): the watch asks for it, and a run (all cached) brings its pictures.
fn view(s: &mut Session, p: &Value) -> Result<Value> {
    let id = str_param(p, "view")?.to_owned();
    let role = role_param(p)?;
    let offered = offered_views_for(s, role);
    if !offered.contains(&id) {
        return Err(SessionError::Params(format!("view {id:?}: this value offers {}", offered.join(", "))));
    }
    let params = match p.get("params") {
        None | Some(Value::Null) => serde_json::Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => return Err(SessionError::Params("\"params\" must be an object".into())),
    };
    let w = s.run.entry_mut(role).ok_or_else(|| SessionError::Params("nothing is watched there".into()))?;
    w.view = plumcraft_proto::display::View { id: id.clone(), params };
    let watch = s.run.watch.clone();
    if !s.connected {
        return Ok(json!({"view": id}));
    }
    let base = crate::paths::base_folder(s.path());
    let graph = crate::convert::runnable(s, base.as_deref());
    let req = s.run.request(&graph, Vec::new(), watch, Instant::now());
    Ok(json!({"view": id, "run_id": req.run_id}))
}

/// `run.stats`: what the caches did, for the Cache Report, the control channel and scripts.
fn stats(s: &mut Session, _: &Value) -> Result<Value> {
    let c = &s.run.cache;
    let nodes: Vec<Value> = s
        .run
        .nodes
        .iter()
        .filter(|(_, n)| n.state.is_some())
        .map(|(id, n)| json!({"node": id, "state": n.state, "ms": n.ms, "source": n.source, "computedMs": n.computed_ms}))
        .collect();
    Ok(json!({"last": c.last, "runs": c.runs, "session": c.session, "byEngine": c.by_engine, "nodes": nodes}))
}

/// `run.cacheReport {open?}`: show or hide the Cache Report (a view, so nothing else changes).
fn cache_report(s: &mut Session, p: &Value) -> Result<Value> {
    let open = match p.get("open") {
        None | Some(Value::Null) => !s.views.cache_report,
        Some(v) => v.as_bool().ok_or_else(|| SessionError::Params("\"open\" must be true or false".into()))?,
    };
    s.views.cache_report = open;
    Ok(json!({"open": open}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "run.start",
            label: "Run",
            menu: &["Run"],
            shortcut: Some("Cmd+Enter"),
            params: "{targets?: [node ids] (default: the graph's sinks, so every node runs), watch?: [{node, port}] (default: what the Watched panel shows)} -> {run_id, generation}",
            enabled: can_run,
            run: start,
            journal: false,
        },
        CommandSpec {
            id: "run.stats",
            label: "Cache Stats",
            menu: &[],
            shortcut: None,
            params: "{} -> {last, runs, session, byEngine, nodes: [{node, state, ms, source, computedMs}]} (what the caches did: nodes computed and answered from memory, the store or by content, time saved, the store's counters and the tree's usage)",
            enabled: always,
            run: stats,
            journal: false,
        },
        CommandSpec {
            id: "run.cacheReport",
            label: "Cache Report",
            menu: &[],
            shortcut: None,
            params: "{open?: bool} (toggles when left out) -> {open}: the Cache Report window (Settings › Cache, or the status line's cache summary)",
            enabled: always,
            run: cache_report,
            journal: false,
        },
        CommandSpec {
            id: "run.cancel",
            label: "Cancel Run",
            menu: &["Run"],
            shortcut: Some("Cmd+Period"),
            params: "{} -- cancel the run in flight: the engine stops it between nodes or inside a long one, and the rest of its records and its reply are ignored. Starting a new run also cancels one in flight (latest wins).",
            enabled: is_running,
            run: cancel,
            journal: false,
        },
        CommandSpec {
            id: "run.auto",
            label: "Auto-Run",
            menu: &["Run"],
            shortcut: None,
            params: "{on?: bool} (toggles when left out): run 150 ms after the last edit",
            enabled: always,
            run: auto,
            journal: false,
        },
        CommandSpec {
            id: "run.watch",
            label: "Watch Output",
            menu: &[],
            shortcut: None,
            params: "{node, port?, index?} (default: the node's first output; a list's first item): show it in the Watched panel; the next run streams it",
            enabled: always,
            run: watch,
            journal: false,
        },
        CommandSpec {
            id: "run.view",
            label: "Show View",
            menu: &[],
            shortcut: None,
            params: "{view, params?, popout?} -> {view, run_id?}: show the watched value (the panel's, or pop-out popout's) in one of the views its wire type offers (an array: auto, color, stretch, clip, heat, labels, angle; angle takes {turn}); a run (cached) brings it",
            enabled: has_views,
            run: view,
            journal: false,
        },
        CommandSpec {
            id: "run.item",
            label: "Show List Item",
            menu: &[],
            shortcut: None,
            params: "{index, popout?} (negative counts from the end) -> {index, run_id?}: show that item of the watched list (the panel's, or pop-out popout's); a run (cached) brings it",
            enabled: has_list,
            run: item,
            journal: false,
        },
    ]
}
