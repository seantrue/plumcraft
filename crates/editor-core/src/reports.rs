//! The windows a native shell draws from a model the core makes (N4 slice 6): the Cache Report
//! (`run.cacheReport`), the graph's Parameters window (`view.params`) and a value editor
//! (`node.editor`), read with `report.get {window, node?}` while the scene's `view.windows` says
//! they are open. The model is sections of rows, each row a label, a value and what can be done
//! there: a command button (`actions`) or a field whose text a command takes (`edit`). The
//! shell draws it in its own controls and runs what a person chooses as commands, so a script
//! reads and does the same. The egui editor draws the same numbers in its own windows
//! (`plumcraft-ui`'s `cache_view`, `graph_params_view`, `value_editors`), with the words here.

use plumcraft_graph::{NodeId, PortRef};
use plumcraft_proto::run::{RunStats, TreeUsage};
use plumcraft_session::Session;
use serde_json::{Value, json};

use crate::run_text::format_ms;

/// `bytes` for a person.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS.get(unit).copied().unwrap_or("TB")) }
}

/// The store in words: its size when the reaper has walked it, and the disk either way.
pub fn tree_text(t: &TreeUsage) -> String {
    let size = match t.bytes {
        Some(b) => format!("{} of {}, {} files", format_bytes(b), format_bytes(t.max_bytes), t.files.unwrap_or(0)),
        None => "size not known yet".to_owned(),
    };
    match t.free_bytes {
        Some(free) => format!("{size}; {} free on disk (it stops writing under {})", format_bytes(free), format_bytes(t.min_free_bytes)),
        None => size,
    }
}

/// Why the store refused writes, in words.
pub fn refusals(s: &RunStats) -> String {
    let lines: Vec<String> = s
        .store
        .iter()
        .filter_map(|(k, v)| {
            let why = match k.strip_prefix("shm_declined_")? {
                "space" => "the disk is nearly full",
                "budget" => "the store is over its budget",
                "size" => "too large",
                "small" => "too small to share",
                "dtype" => "not a stored type",
                "mode" => "the store is read-only",
                other => other,
            };
            Some(format!("{v}: {why}"))
        })
        .collect();
    format!("{}\nThose values stay in memory, and are lost to other engines and later runs.", lines.join("\n"))
}

fn row(label: impl Into<String>, value: impl Into<String>) -> Value {
    json!({"label": label.into(), "value": value.into()})
}

fn totals(s: &RunStats) -> Vec<Value> {
    vec![
        row("Computed", format!("{} nodes, {}", s.computed, format_ms(s.computed_ms))),
        row("From memory", s.from_memory.to_string()),
        row("From the store", s.from_store.to_string()),
        row("By content", s.from_content.to_string()),
        row("Time saved", format_ms(s.saved_ms)),
        row("Records kept", s.kept.to_string()),
        row("No record", s.store_missed.to_string()),
        row("Dropped from memory", s.evicted.to_string()),
    ]
}

/// The Cache Report: the latest run, the session, each engine, the store and each node, as
/// egui's window has them.
pub fn cache_report(s: &Session) -> Value {
    let c = &s.run.cache;
    let close = json!([{"label": "Close", "command": "run.cacheReport", "params": {"open": false}}]);
    let Some(last) = &c.last else {
        return json!({"window": "cache", "title": "Cache Report", "empty": "No run has reported yet.", "sections": [], "actions": close});
    };
    let mut sections =
        vec![json!({"title": "Latest run", "rows": totals(last)}), json!({"title": format!("This session, {} runs", c.runs), "rows": totals(&c.session)})];
    for (engine, st) in &c.by_engine {
        sections.push(json!({"title": format!("Engine {engine}"), "rows": totals(st)}));
    }
    let mut store = Vec::new();
    match &c.session.tree {
        Some(t) => {
            let mut r = row("Store", tree_text(t));
            if t.low_disk() {
                r["warn"] = json!(true);
            }
            store.push(r);
        }
        None => store.push(row("Store", "This engine doesn't report its store.")),
    }
    if c.session.declined() > 0 {
        let mut r = row("Refused", refusals(&c.session));
        r["warn"] = json!(true);
        store.push(r);
    }
    for (k, v) in &c.session.store {
        store.push(row(k.strip_prefix("shm_").unwrap_or(k), v.to_string()));
    }
    sections.push(json!({"title": "Store", "rows": store}));
    let nodes: Vec<Value> = s
        .run
        .nodes
        .iter()
        .filter(|(_, n)| n.state.is_some())
        .map(|(id, n)| {
            let title = if s.graph().node(&NodeId::from(id.as_str())).is_some() {
                plumcraft_session::node_title(s.graph(), s.catalog(), &NodeId::from(id.as_str()))
            } else {
                id.clone()
            };
            let this = n.ms.map_or_else(|| "—".to_owned(), format_ms);
            let compute = n.computed_ms.map_or_else(|| "—".to_owned(), format_ms);
            json!({"label": title, "value": n.source.clone().unwrap_or_else(|| "computed".into()),
                   "detail": format!("this run {this} · to compute {compute}"), "node": id})
        })
        .collect();
    sections.push(json!({"title": "Nodes, latest run", "rows": nodes}));
    json!({"window": "cache", "title": "Cache Report", "sections": sections, "actions": close})
}

fn short(v: &Value) -> String {
    let t = match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    if t.chars().count() > 40 { format!("{}…", t.chars().take(40).collect::<String>()) } else { t }
}

/// The graph's Parameters window: its named ports in order, each with its port, type, value and
/// doc line (a field: `graph.port.doc`), its name (a field: `graph.port.rename`), Go To (the
/// node's parameters), Remove, and Up and Down (`graph.port.reorder`).
pub fn graph_params(s: &Session) -> Value {
    let rows = plumcraft_session::named_cmds::params(s.graph(), s.catalog());
    let names: Vec<String> = rows.iter().filter_map(|r| r["name"].as_str().map(str::to_owned)).collect();
    let close = json!([{"label": "Close", "command": "view.params", "params": {"open": false}}]);
    if rows.is_empty() {
        return json!({"window": "params", "title": "Parameters",
                      "empty": "This graph names no ports. Long-press an input and choose Name This Port… to make it one: a runner then sets it with --set NAME=VALUE.",
                      "sections": [], "actions": close});
    }
    let moved = |i: usize, to: usize| {
        let mut order = names.clone();
        let n = order.remove(i);
        order.insert(to, n);
        json!({"names": order})
    };
    let out: Vec<Value> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let name = r["name"].as_str().unwrap_or_default();
            let port = r["port"].as_str().unwrap_or_default();
            let node = port.split('.').next().unwrap_or_default();
            let mut actions = vec![
                json!({"label": "Go To", "command": "node.parameters", "params": {"node": node}, "help": "Open its node's parameters"}),
                json!({"label": "Remove", "command": "graph.port.unname", "params": {"name": name}, "help": "The port stops being a parameter; its value stays", "destructive": true}),
            ];
            if i > 0 {
                actions.push(json!({"label": "Move Up", "command": "graph.port.reorder", "params": moved(i, i - 1), "icon": "arrow.up"}));
            }
            if i + 1 < names.len() {
                actions.push(json!({"label": "Move Down", "command": "graph.port.reorder", "params": moved(i, i + 1), "icon": "arrow.down"}));
            }
            json!({
                "id": name,
                "label": name,
                "value": format!("= {}", short(&r["value"])),
                "detail": format!("{port} · {}", r["type"].as_str().unwrap_or("?")),
                "edits": [
                    {"label": "Name", "command": "graph.port.rename", "params": {"name": name}, "key": "to", "value": name, "hint": "a name"},
                    {"label": "Description", "command": "graph.port.doc", "params": {"name": name}, "key": "doc", "value": r["doc"].as_str().unwrap_or_default(), "hint": "what it is"},
                ],
                "actions": actions,
            })
        })
        .collect();
    json!({"window": "params", "title": "Parameters", "intro": "Set from outside with run --set NAME=VALUE",
           "sections": [{"title": "", "rows": out}], "actions": close})
}

/// A value editor (`node.editor`): what the node's editor edits, its port's value as JSON, which
/// Apply sets through `node.set` (one command, one undo step). An application's own editor for
/// the kind draws more; these apps draw every kind this way, the value itself.
pub fn value_editor(s: &Session, node: &str) -> Result<Value, String> {
    let id = NodeId::from(node);
    if !s.views.editors.contains(&id) {
        return Err(format!("no value editor is open on {node:?}"));
    }
    let graph = s.graph();
    let n = graph.node(&id).ok_or_else(|| format!("no node {node:?}"))?;
    let def = s.catalog().node(&n.kind);
    let editor = def.and_then(|d| d.editor.as_ref()).ok_or_else(|| format!("{} names no editor", n.kind))?;
    let port = editor.port.clone();
    let declared = def.and_then(|d| d.inputs.iter().find(|p| p.name == port));
    let value = n.values.get(&port).cloned().or_else(|| declared.map(|p| p.default.clone())).unwrap_or(Value::Null);
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
    let wired = graph.wire_into(&PortRef::new(id.clone(), port.clone())).is_some();
    let title = plumcraft_session::node_title(graph, s.catalog(), &id);
    Ok(json!({
        "window": "editor", "node": node, "title": format!("{title} · {port}"), "kind": editor.kind, "port": port,
        "type": declared.map(|p| p.ty.clone()),
        "intro": format!("{}: the value as JSON. Apply sets it, as one undo step (an edit field marked json sends its text read as JSON).", editor.kind),
        "sections": [{"title": "", "rows": [{
            "id": port, "label": port, "value": "",
            "edits": [{"label": "Value", "command": "node.set", "params": {"node": node, "port": port}, "key": "value", "value": text, "hint": "JSON", "multiline": true, "json": true}],
        }]}],
        "wired": wired,
        "actions": [{"label": "Close", "command": "node.editor", "params": {"node": node, "open": false}}],
    }))
}

/// `report.get {window: "cache" | "params" | "editor", node?}`.
pub fn get(s: &Session, params: &Value) -> Result<Value, String> {
    crate::control::check_fields(params, &["window", "node"])?;
    match params.get("window").and_then(Value::as_str) {
        Some("cache") => Ok(cache_report(s)),
        Some("params") => Ok(graph_params(s)),
        Some("editor") => {
            let node = params.get("node").and_then(Value::as_str).filter(|n| !n.is_empty() && n.len() <= 512).ok_or("editor needs node: a node id")?;
            value_editor(s, node)
        }
        _ => Err("window must be cache, params or editor".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_and_the_store_read_as_a_person_would() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        const GB: u64 = 1 << 30;
        let t = TreeUsage { bytes: Some(GB), files: Some(42), max_bytes: 16 * GB, free_bytes: Some(9 * GB), min_free_bytes: 8 * GB };
        assert_eq!(tree_text(&t), "1.0 GB of 16.0 GB, 42 files; 9.0 GB free on disk (it stops writing under 8.0 GB)");
    }

    #[test]
    fn the_windows_models_name_commands_and_refuse_what_they_dont_know() {
        let mut s = Session::new(plumcraft_text::engine().schema());
        let r = get(&s, &json!({"window": "cache"})).unwrap();
        assert_eq!(r["empty"], "No run has reported yet.");
        assert_eq!(r["actions"][0]["command"], "run.cacheReport");
        let a = s.execute("node.add", &json!({"kind": "text.literal"})).unwrap()["id"].as_str().unwrap().to_owned();
        s.execute("graph.port.name", &json!({"port": {"node": a, "port": "text"}, "name": "words"})).unwrap();
        s.execute("graph.port.name", &json!({"port": {"node": a, "port": "text"}, "name": "words"})).unwrap_err();
        let p = get(&s, &json!({"window": "params"})).unwrap();
        let row = &p["sections"][0]["rows"][0];
        assert_eq!(row["label"], "words");
        assert_eq!(row["edits"][0]["command"], "graph.port.rename");
        assert_eq!(row["actions"][0]["params"]["node"], json!(a));
        assert!(get(&s, &json!({"window": "editor", "node": a})).is_err(), "no editor is open");
        assert!(get(&s, &json!({"window": "nope"})).is_err());
        assert!(get(&s, &json!({"window": "cache", "extra": 1})).is_err());
    }
}
