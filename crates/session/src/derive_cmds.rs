//! A value's derived views (FR6, research note 30, decision 6A): what a pop-out's Histogram (and
//! later Stats and Slice) shows of the value it watches, without a node added. The choice lives in
//! the graph's `meta.views["node.port"]` as `{kind, params, log?}`, as a chart's does
//! (`meta.charts`): one undo step a change, saved with the file and outside `graph_info.hash`.
//! The core makes the view (`viewer.data {part: "chart"}`): the engine that made the value when it
//! answers `view.derive`, else the editor itself.

use serde_json::{Map, Value, json};

use crate::chart_cmds::port_key;
use crate::commands::CommandSpec;
use crate::run::WatchRole;
use crate::{Result, Session, SessionError};

/// Where the views' specs live in the graph's `meta`.
pub const META_KEY: &str = "views";

fn bad(msg: impl Into<String>) -> SessionError {
    SessionError::Params(msg.into())
}

/// The view kept for a node's port: `{kind, params, log?}`.
pub fn view_of(graph: &plumcraft_graph::Graph, node: &str, port: &str) -> Option<Value> {
    graph.meta.get(META_KEY)?.get(port_key(node, port)).filter(|v| v.is_object()).cloned()
}

/// The histogram a port's Histogram viewer shows: its kept spec when it is one, else every value
/// in the default bins. `(spec for plumcraft-views, log)`.
pub fn histogram_of(graph: &plumcraft_graph::Graph, node: &str, port: &str) -> (Value, bool) {
    let kept = view_of(graph, node, port).filter(|v| v.get("kind").and_then(Value::as_str) == Some("histogram"));
    let log = kept.as_ref().and_then(|v| v.get("log")).and_then(Value::as_bool).unwrap_or(false);
    let params = kept.as_ref().and_then(|v| v.get("params")).filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({}));
    (json!({"kind": "histogram", "params": params}), log)
}

/// The node and port a pop-out watches.
fn watched(s: &Session, p: &Value) -> Result<(String, String)> {
    let id = p.get("popout").and_then(Value::as_u64).ok_or_else(|| bad("\"popout\" (a pop-out's id) is required"))?;
    let (w, _) = s.run.entry(WatchRole::Popout(id)).ok_or_else(|| bad(format!("no pop-out {id}")))?;
    Ok((w.node.clone(), w.port.clone()))
}

fn keep(s: &mut Session, label: &str, key: String, view: Option<Value>) -> Result<()> {
    s.edit(label, move |g, _| {
        let views = g.meta.entry(META_KEY.to_owned()).or_insert_with(|| json!({}));
        if !views.is_object() {
            *views = json!({});
        }
        if let Some(m) = views.as_object_mut() {
            match view {
                Some(v) => {
                    m.insert(key, v);
                }
                None => {
                    m.remove(&key);
                }
            }
        }
        Ok(())
    })
}

fn check_fields(p: &Value, fields: &[&str], cmd: &str) -> Result<()> {
    if let Some(o) = p.as_object()
        && let Some(k) = o.keys().find(|k| !fields.contains(&k.as_str()))
    {
        return Err(bad(format!("{cmd} takes {}, not {:?}", fields.join(", "), k.chars().take(64).collect::<String>())));
    }
    Ok(())
}

/// `viewer.derive {popout, view: {kind, params?} | null}`: the view kept for the value a pop-out
/// shows, checked as an engine checks it; null forgets it.
fn derive(s: &mut Session, p: &Value) -> Result<Value> {
    check_fields(p, &["popout", "view"], "viewer.derive")?;
    let (node, port) = watched(s, p)?;
    let key = port_key(&node, &port);
    match p.get("view") {
        None => Err(bad("view is {kind, params?}, or null")),
        Some(Value::Null) => {
            keep(s, "Clear View", key, None)?;
            Ok(json!({"popout": p["popout"], "view": null}))
        }
        Some(v) => {
            plumcraft_views::parse(v).map_err(bad)?;
            let view = json!({"kind": v["kind"], "params": v.get("params").filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({}))});
            keep(s, "View", key, Some(view.clone()))?;
            Ok(json!({"popout": p["popout"], "view": view}))
        }
    }
}

/// `viewer.histogram {popout, bins?, lo?, hi?, per?, log?, clear?}`: the histogram a pop-out's
/// Histogram viewer shows; a field left out keeps what it was, null forgets it.
fn histogram(s: &mut Session, p: &Value) -> Result<Value> {
    check_fields(p, &["popout", "bins", "lo", "hi", "per", "log", "clear"], "viewer.histogram")?;
    let (node, port) = watched(s, p)?;
    let key = port_key(&node, &port);
    if p.get("clear").and_then(Value::as_bool) == Some(true) {
        keep(s, "Clear Histogram", key, None)?;
        return Ok(json!({"popout": p["popout"], "view": null}));
    }
    let (spec, mut log) = histogram_of(s.graph(), &node, &port);
    let mut params: Map<String, Value> = spec.get("params").and_then(Value::as_object).cloned().unwrap_or_default();
    for k in ["bins", "lo", "hi", "per"] {
        match p.get(k) {
            None => {}
            Some(Value::Null) => {
                params.remove(k);
            }
            Some(v) => {
                params.insert(k.into(), v.clone());
            }
        }
    }
    if let Some(l) = p.get("log") {
        log = l.as_bool().ok_or_else(|| bad("log is true or false"))?;
    }
    let spec = json!({"kind": "histogram", "params": params});
    plumcraft_views::parse(&spec).map_err(bad)?;
    let mut view = spec;
    if log {
        view["log"] = json!(true);
    }
    keep(s, "Histogram", key, Some(view.clone()))?;
    Ok(json!({"popout": p["popout"], "view": view}))
}

fn has_popouts(s: &Session) -> std::result::Result<(), String> {
    if s.views.popouts.is_empty() { Err("no pop-out is open".into()) } else { Ok(()) }
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "viewer.derive",
            label: "Derived View",
            menu: &[],
            shortcut: None,
            params: "{popout, view: {kind: histogram | stats | slice, params?} | null} -> {popout, view}: the derived view kept for the value a pop-out shows (FR6), checked as an engine checks it; kept in the graph's meta.views[\"node.port\"], one undo step, saved with the file; null forgets it",
            enabled: has_popouts,
            run: derive,
            journal: true,
        },
        CommandSpec {
            id: "viewer.histogram",
            label: "Histogram",
            menu: &[],
            shortcut: None,
            params: "{popout, bins?: 1..65536, lo?, hi?, per?: {axis}, log?: bool, clear?} -> {popout, view}: the histogram a pop-out's Histogram viewer shows of every value (or each plane along per's axis), counts on a log scale when log; a field left out keeps what it was, null forgets it; kept in meta.views like viewer.derive",
            enabled: has_popouts,
            run: histogram,
            journal: true,
        },
    ]
}
