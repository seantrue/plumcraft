//! The document's windows as the core keeps them (N4 slice 3; `docs/shell-interface.md` 12.5):
//! the parameters inspector (`node.parameters`) and the pop-outs (`view.popout`, `view.close`),
//! carried in the scene's `view.windows` so a shell opens, closes and brings forward its native
//! windows by following the core, and a script drives them by the same commands. What each
//! shows is read with a method: `parameters.get` (every input of the node, shown on its face or
//! not, as the face's rows are, with sections, defaults and resets) and `popout.get` (the
//! value, as the Watched panel's was, with the viewer's state).
//!
//! The Watched panel's side column is gone from the Apple shells (the maintainer, N3 exit card
//! 14: "Eliminate the side column in favor of just the popout window"); a tap still watches a
//! value (`run.watch`), and a double tap, Pop Out or the parameters' Pop Out opens it in a
//! window of its own.

use std::hash::{Hash, Hasher};

use plumcraft_graph::{NodeId, PortRef, ports};
use plumcraft_proto::catalogue::{Port, Socket, scalars};
use plumcraft_session::Session;
use plumcraft_session::run::WatchRole;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::scene::{InRow, Inputs};

/// The document's windows, in the scene's `view.windows`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Windows {
    /// The node the parameters inspector is open on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<String>,
    /// The pop-outs open, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub popouts: Vec<PopoutWindow>,
    /// The Settings window is open, on this section (`app.settings {tab?, open?}`, N4 slice 4):
    /// a native shell shows its own settings window or sheet while it is, and closes it with
    /// `app.settings {open: false}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<String>,
    /// The Messages window is open (`view.messages`, N4 slice 5): a native shell shows the
    /// status bar's messages in full (`messages.get`) in a window or sheet of its own, and closes it
    /// with `view.messages {open: false}`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub messages: bool,
}

impl Windows {
    pub fn is_empty(&self) -> bool {
        self.parameters.is_none() && self.popouts.is_empty() && self.settings.is_none() && !self.messages
    }
}

/// One pop-out window: what it follows, and enough for a shell to title it, bring it forward and
/// know when to read its value again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PopoutWindow {
    pub id: u64,
    pub node: String,
    pub port: String,
    pub index: i64,
    /// The window's title: the node's title and the port.
    pub title: String,
    /// The viewer it shows: the one asked for, else the value's default (`values::default_viewer`);
    /// none until a run has sent the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer: Option<String>,
    /// Rises each time the pop-out is asked for while open: the shell brings its window forward.
    pub raised: u64,
    /// Changes when what `popout.get` reads changes (the value a run sent, the viewer's state):
    /// the shell reads it again then.
    pub stamp: u64,
}

/// The windows the session has open.
pub fn of(session: &Session) -> Windows {
    let graph = session.graph();
    let catalog = session.catalog();
    let parameters = session.views.parameters.as_ref().filter(|n| graph.node(n).is_some()).map(|n| n.0.clone());
    let popouts = session
        .views
        .popouts
        .iter()
        .filter_map(|o| {
            let (w, d) = session.run.entry(WatchRole::Popout(o.id))?;
            let node = NodeId::from(w.node.as_str());
            let title = if graph.node(&node).is_some() { plumcraft_session::node_title(graph, catalog, &node) } else { w.node.clone() };
            let viewer = o.viewer.clone().or_else(|| d.map(|d| crate::values::default_viewer(d).to_owned()));
            let mut h = std::collections::hash_map::DefaultHasher::new();
            serde_json::to_string(&d).unwrap_or_default().hash(&mut h);
            serde_json::to_string(&o.text).unwrap_or_default().hash(&mut h);
            serde_json::to_string(&o.state).unwrap_or_default().hash(&mut h);
            format!("{:?}{:?}", o.fit, o.viewer).hash(&mut h);
            Some(PopoutWindow {
                id: o.id,
                node: w.node.clone(),
                port: w.port.clone(),
                index: w.index,
                title: format!("{title} · {}", w.port),
                viewer,
                raised: o.raised,
                stamp: h.finish(),
            })
        })
        .collect();
    Windows { parameters, popouts, settings: session.views.settings.clone(), messages: session.views.messages }
}

/// `popout.get {id}`: a pop-out's value, as `ui.inspect`'s `watched` has the panel's
/// ([`crate::values::value_json`]), with its window's state: `{id, node, port, index, title,
/// viewer, summary, media, value?, data?, picture?, excerpt?, error?, text: {wrap, mono, all},
/// fit: "fit" | "1:1" | null}`, and since N4 slice 5 `state` (the viewer's: a table's sort and
/// page, an outline's open members, play, a channel), `viewers` offered, `type`, `items`,
/// `index`, `form`, `bytes`, `mediaType`, `handle`, `stats`, `views`, `peaks` and `sheet`
/// ([`crate::viewers::popout_extra`]).
pub fn popout_json(session: &Session, params: &Value) -> Result<Value, String> {
    crate::control::check_fields(params, &["id"])?;
    let id = params.get("id").and_then(Value::as_u64).ok_or("id must be a pop-out's id")?;
    let o = session.views.popout(id).ok_or_else(|| format!("no pop-out {id}"))?;
    let (w, d) = session.run.entry(WatchRole::Popout(id)).ok_or_else(|| format!("no pop-out {id}"))?;
    let mut v = crate::values::value_json(w, d);
    let win = of(session).popouts.into_iter().find(|p| p.id == id);
    v["id"] = json!(id);
    v["index"] = json!(w.index);
    if let Some(win) = win {
        v["title"] = json!(win.title);
        v["viewer"] = json!(win.viewer);
        v["stamp"] = json!(win.stamp);
    }
    v["text"] = serde_json::to_value(o.text).unwrap_or(Value::Null);
    for (k, x) in crate::viewers::popout_extra(d, &o.state) {
        v[k] = x;
    }
    v["fit"] = match o.fit {
        Some(plumcraft_session::viewer_cmds::Zoom::Fit) => json!("fit"),
        Some(plumcraft_session::viewer_cmds::Zoom::OneToOne) => json!("1:1"),
        _ => Value::Null,
    };
    Ok(v)
}

/// Which section of the inspector a row is in, egui's parameters panel's order.
fn section_of(row: &crate::rows::InputRow, tucked: &std::collections::BTreeSet<String>) -> (u8, String) {
    if tucked.contains(&row.key) {
        (3, "More fields".into())
    } else if let Some(s) = &row.port.param_ref {
        let mut c = s.group.chars();
        (2, c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect()))
    } else if row.port.socket_mode() == Socket::Socket {
        (0, "Inputs".into())
    } else {
        (1, "Parameters".into())
    }
}

/// What the catalogue says about a port, for its help: the doc, its type, the default and range.
fn help(port: &Port, key: &str, label: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(t) = port.tooltip.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        parts.push(t.to_owned());
    }
    let mut facts = vec![if port.optional { format!("{}, optional", port.ty) } else { port.ty.clone() }];
    if port.socket_mode() != Socket::Socket {
        if !port.default.is_null() {
            facts.push(format!("default {}", crate::widget_text::value_text(&port.default)));
        }
        let num = |f: f64| crate::widget_text::value_text(&json!(f));
        match (port.min, port.max) {
            (Some(lo), Some(hi)) if lo.is_finite() && hi.is_finite() => facts.push(format!("{} to {}", num(lo), num(hi))),
            (Some(lo), _) if lo.is_finite() => facts.push(format!("at least {}", num(lo))),
            (_, Some(hi)) if hi.is_finite() => facts.push(format!("at most {}", num(hi))),
            _ => {}
        }
        if let Some(step) = port.step.filter(|s| s.is_finite() && *s > 0.0 && port.ty == scalars::FLOAT) {
            facts.push(format!("step {}", num(step)));
        }
    }
    parts.push(facts.join(" · "));
    if key != label {
        parts.push(format!("Key: {key}"));
    }
    parts.join("\n\n")
}

/// `parameters.get {node?}` (default the node the inspector is open on): every input of a node,
/// shown on its face or not, in egui's parameters panel's sections (Inputs, Parameters, each
/// settings section, More fields), each row as the face's rows are (`InRow`: its source, text,
/// widget) with `onFace` (shown on the face now), `canHide` (not wired: the face may leave it
/// out), `editable`, `default`, `reset` (the value differs from the default) and `help`; and
/// `hidden`, the inputs not on the face. Edits go
/// through `row.set`, which takes any input of the node, and `node.face` shows or hides one.
pub fn parameters_json(session: &Session, inputs: &Inputs<'_>, params: &Value) -> Result<Value, String> {
    crate::control::check_fields(params, &["node"])?;
    let id = match params.get("node") {
        None | Some(Value::Null) => session.views.parameters.clone().ok_or("the parameters inspector is not open: name a node")?,
        Some(Value::String(s)) if !s.is_empty() && s.len() <= 512 => NodeId::from(s.as_str()),
        Some(_) => return Err("node must be a node id".into()),
    };
    let graph = session.graph();
    let catalog = session.catalog();
    let node = graph.node(&id).ok_or_else(|| format!("no node {}", crate::selection::shown_id(&id.0)))?;
    let def = catalog.node(&node.kind).ok_or_else(|| format!("{} is of a kind no engine has", crate::selection::shown_id(&id.0)))?;
    let rows = crate::scene::param_rows(inputs, &id);
    let tucked = ports::tucked(def, node, |k| graph.wire_into(&PortRef::new(id.clone(), k)).is_some());
    let shown: std::collections::BTreeSet<String> = crate::rows::shown_rows(def, &id, graph).0.into_iter().map(|r| r.key).collect();
    let mut sections: Vec<((u8, String), Vec<Value>)> = Vec::new();
    for (r, row) in &rows {
        let wired = graph.wire_into(&PortRef::new(id.clone(), row.key.clone())).is_some();
        let default = def.filled_default(&row.key, |k| node.values.get(k).filter(|_| k != row.key)).unwrap_or(&row.port.default).clone();
        let current = ports::current_value(def, node, &row.key, &row.port);
        let editable = crate::inline::editable(r);
        let entry = json!({
            "row": serde_json::to_value::<&InRow>(r).unwrap_or(Value::Null),
            "onFace": shown.contains(&row.key),
            "canHide": !wired,
            "editable": editable,
            "default": default,
            "reset": editable && current != default,
            "help": help(&row.port, &row.key, &row.label),
        });
        let s = section_of(row, &tucked);
        match sections.iter_mut().find(|(k, _)| *k == s) {
            Some((_, list)) => list.push(entry),
            None => sections.push((s, vec![entry])),
        }
    }
    sections.sort_by_key(|a| a.0.0);
    let hidden: Vec<&str> = rows.iter().map(|(_, r)| r.key.as_str()).filter(|k| !shown.contains(*k)).collect();
    let sections: Vec<Value> = sections.into_iter().map(|((_, title), rows)| json!({"title": title, "rows": rows})).collect();
    let title = plumcraft_session::node_title(graph, catalog, &id);
    let first = def.description.split("\n\n").next().unwrap_or_default();
    Ok(json!({
        "node": id,
        "title": title,
        "kind": node.kind,
        "display": def.display,
        "description": first,
        "about": def.description,
        "family": def.family,
        "open": session.views.parameters.as_ref() == Some(&id),
        "sections": sections,
        "count": rows.len(),
        "hidden": hidden,
    }))
}

/// `messages.get {}` (N4 slice 5): the status bar's messages in full, as the Messages window shows
/// them: `{open, messages: [{tone: "info" | "warn" | "error", text, detail?}]}`, the run's errors
/// first, then the run's failure, the application's line, a save's outcome and the last thing that
/// went wrong (`control::status_messages`).
pub fn messages_json(session: &Session, last_error: Option<&str>, params: &Value) -> Result<Value, String> {
    crate::control::check_fields(params, &[])?;
    let list: Vec<Value> = crate::control::status_messages(session, last_error, None)
        .into_iter()
        .map(|m| {
            let tone = match m.tone {
                plumcraft_session::commands::Tone::Info => "info",
                plumcraft_session::commands::Tone::Warn => "warn",
                plumcraft_session::commands::Tone::Error => "error",
            };
            json!({"tone": tone, "text": m.text, "detail": m.detail})
        })
        .collect();
    Ok(json!({"open": session.views.messages, "messages": list}))
}
