//! A chart's choices for a value (note 26 slices 7 to 9; the maintainer's decision 3A: "saved with
//! the graph, per output port, in the graph's meta (not in the result hash), and undoable"):
//! which kind of chart, and which of the table's columns are x, y, colour, size and the treemap's
//! path. `viewer.chart {popout, …}` sets them for the value the pop-out shows; the spec lives in
//! the graph's `meta.charts["node.port"]`, one undo step each change, saved with the file and
//! outside `graph_info.hash`. The core lays the chart out from it (`viewer.data {part: "chart"}`).
//!
//! The spec: `{mark: "histogram" | "line" | "column" | "treemap", x?, y?: [column], color?,
//! size?, path?: [column], agg?: "sum" | "mean" | "count" | "min" | "max", stack?: bool, bins?}`.
//! Every column is checked against the value's own columns (a TABLE's, or a table array's
//! names); a number-only role (a histogram's x, y, size) takes only a numeric column.

use serde_json::{Map, Value, json};

use crate::commands::CommandSpec;
use crate::run::WatchRole;
use crate::{Result, Session, SessionError};

/// Where the specs live in the graph's `meta`.
pub const META_KEY: &str = "charts";
/// The kinds of chart a spec names (the maintainer's six: a stacked line is `line` with `stack`,
/// a stacked column `column` with `stack`).
pub const MARKS: &[&str] = &["histogram", "line", "column", "treemap"];
pub const AGGS: &[&str] = &["sum", "mean", "count", "min", "max"];
/// The most y columns, and treemap levels.
pub const MAX_Y: usize = 8;
pub const MAX_PATH: usize = 3;
pub const MAX_BINS: u64 = 1000;

/// A port's key in `meta.charts`.
pub fn port_key(node: &str, port: &str) -> String {
    format!("{node}.{port}")
}

/// The chart spec kept for a node's port, if any.
pub fn spec_of(graph: &plumcraft_graph::Graph, node: &str, port: &str) -> Option<Value> {
    graph.meta.get(META_KEY)?.get(port_key(node, port))?.as_object().map(|o| Value::Object(o.clone()))
}

/// A value's columns as a chart sees them: `(name, numeric)`. A TABLE's (inline, or its form), or
/// a table array's names (all numeric). Empty when it has none.
pub fn columns_of(d: &plumcraft_proto::display::Display) -> Vec<(String, bool)> {
    use plumcraft_proto::data::Form;
    let numeric = |t: &str| matches!(t.to_ascii_uppercase().as_str(), "INT" | "FLOAT" | "NUMBER");
    if let Some(cols) = d.value.as_ref().and_then(|v| v.get("columns")).and_then(Value::as_array) {
        return cols.iter().filter_map(|c| Some((c.get("name")?.as_str()?.to_owned(), c.get("type").and_then(Value::as_str).is_some_and(numeric)))).collect();
    }
    match d.full.as_ref().map(|f| &f.meta.form) {
        Some(Form::Table { columns, .. }) => columns.iter().map(|c| (c.name.clone(), numeric(&c.ty))).collect(),
        Some(Form::Array { shape, names, table: true, .. }) => {
            let k = shape.last().copied().unwrap_or(0) as usize;
            (0..k.min(4096)).map(|i| (names.get(i).filter(|_| names.len() == k).cloned().unwrap_or_else(|| format!("c{}", i + 1)), true)).collect()
        }
        _ => Vec::new(),
    }
}

fn bad(msg: impl Into<String>) -> SessionError {
    SessionError::Params(msg.into())
}

/// A column name in `p[key]` that the value has (and is numeric when `numeric`).
fn column(p: &Value, key: &str, cols: &[(String, bool)], numeric: bool) -> Result<Option<Option<String>>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(c)) => match cols.iter().find(|(n, _)| n == c) {
            Some((_, num)) if numeric && !num => Err(bad(format!("{key}: {c:?} isn't a column of numbers"))),
            Some(_) => Ok(Some(Some(c.clone()))),
            None => Err(bad(format!("{key}: no column {:?}; the columns are {}", c.chars().take(64).collect::<String>(), names(cols)))),
        },
        Some(_) => Err(bad(format!("{key} is a column's name, or null"))),
    }
}

fn names(cols: &[(String, bool)]) -> String {
    cols.iter().take(32).map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
}

/// A list of column names in `p[key]`, at most `max`, each the value's (numeric when `numeric`).
fn column_list(p: &Value, key: &str, cols: &[(String, bool)], numeric: bool, max: usize) -> Result<Option<Vec<String>>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(Vec::new())),
        Some(Value::Array(a)) if a.len() <= max => a
            .iter()
            .map(|v| match column(&json!({key: v}), key, cols, numeric)? {
                Some(Some(c)) => Ok(c),
                _ => Err(bad(format!("{key} is a list of columns' names"))),
            })
            .collect::<Result<Vec<String>>>()
            .map(Some),
        Some(Value::Array(_)) => Err(bad(format!("{key} takes at most {max} columns"))),
        Some(_) => Err(bad(format!("{key} is a list of columns' names"))),
    }
}

/// The value a pop-out shows: its node, port and display.
/// A value's columns: `(name, numeric)`.
type Columns = Vec<(String, bool)>;

fn shown(s: &Session, p: &Value) -> Result<(String, String, Columns)> {
    let id = p.get("popout").and_then(Value::as_u64).ok_or_else(|| bad("\"popout\" (a pop-out's id) is required"))?;
    let (w, d) = s.run.entry(WatchRole::Popout(id)).ok_or_else(|| bad(format!("no pop-out {id}")))?;
    let d = d.ok_or_else(|| bad("no value yet: run the graph"))?;
    let cols = columns_of(d);
    if cols.is_empty() {
        return Err(bad("this value has no columns to chart"));
    }
    Ok((w.node.clone(), w.port.clone(), cols))
}

/// `viewer.chart {popout, mark?, x?, y?, color?, size?, path?, agg?, stack?, bins?, clear?}`.
fn chart(s: &mut Session, p: &Value) -> Result<Value> {
    const FIELDS: &[&str] = &["popout", "mark", "x", "y", "color", "size", "path", "agg", "stack", "bins", "clear"];
    if let Some(o) = p.as_object()
        && let Some(k) = o.keys().find(|k| !FIELDS.contains(&k.as_str()))
    {
        return Err(bad(format!("viewer.chart takes {}, not {:?}", FIELDS.join(", "), k.chars().take(64).collect::<String>())));
    }
    let (node, port, cols) = shown(s, p)?;
    let key = port_key(&node, &port);
    if p.get("clear").and_then(Value::as_bool) == Some(true) {
        s.edit("Clear Chart", |g, _| {
            if let Some(m) = g.meta.get_mut(META_KEY).and_then(Value::as_object_mut) {
                m.remove(&key);
            }
            Ok(())
        })?;
        return Ok(json!({"popout": p["popout"], "spec": null}));
    }
    let mut spec: Map<String, Value> = spec_of(s.graph(), &node, &port).and_then(|v| v.as_object().cloned()).unwrap_or_default();
    if let Some(m) = p.get("mark") {
        let m = m.as_str().filter(|m| MARKS.contains(m)).ok_or_else(|| bad(format!("mark is one of {}", MARKS.join(", "))))?;
        spec.insert("mark".into(), json!(m));
    }
    let mark = spec.get("mark").and_then(Value::as_str).unwrap_or("column").to_owned();
    let set = |spec: &mut Map<String, Value>, k: &str, v: Option<Value>| match v {
        Some(Value::Null) | None => {
            spec.remove(k);
        }
        Some(v) => {
            spec.insert(k.into(), v);
        }
    };
    // a kind and its columns sent together (a shell sends the spec drawn with its one change): an x
    // that doesn't fit the new kind is dropped below rather than refused
    let strict_x = mark == "histogram" && p.get("mark").is_none();
    if let Some(x) = column(p, "x", &cols, strict_x)? {
        set(&mut spec, "x", x.map(Value::from));
    }
    if let Some(y) = column_list(p, "y", &cols, true, MAX_Y)? {
        set(&mut spec, "y", (!y.is_empty()).then(|| json!(y)));
    }
    if let Some(c) = column(p, "color", &cols, false)? {
        set(&mut spec, "color", c.map(Value::from));
    }
    if let Some(c) = column(p, "size", &cols, true)? {
        set(&mut spec, "size", c.map(Value::from));
    }
    if let Some(path) = column_list(p, "path", &cols, false, MAX_PATH)? {
        set(&mut spec, "path", (!path.is_empty()).then(|| json!(path)));
    }
    if let Some(a) = p.get("agg") {
        let a = a.as_str().filter(|a| AGGS.contains(a)).ok_or_else(|| bad(format!("agg is one of {}", AGGS.join(", "))))?;
        spec.insert("agg".into(), json!(a));
    }
    if let Some(b) = p.get("stack") {
        spec.insert("stack".into(), json!(b.as_bool().ok_or_else(|| bad("stack is true or false"))?));
    }
    if let Some(b) = p.get("bins") {
        let n = b.as_u64().filter(|n| (1..=MAX_BINS).contains(n)).ok_or_else(|| bad(format!("bins is a whole number from 1 to {MAX_BINS}")))?;
        spec.insert("bins".into(), json!(n));
    }
    // a column a number-only role names must still be numbers after a mark changes
    if mark == "histogram"
        && let Some(x) = spec.get("x").and_then(Value::as_str)
        && !cols.iter().any(|(n, num)| n == x && *num)
    {
        spec.remove("x");
    }
    let value = Value::Object(spec);
    let stored = value.clone();
    s.edit("Chart", move |g, _| {
        let charts = g.meta.entry(META_KEY.to_owned()).or_insert_with(|| json!({}));
        if !charts.is_object() {
            *charts = json!({});
        }
        if let Some(m) = charts.as_object_mut() {
            m.insert(key, stored);
        }
        Ok(())
    })?;
    Ok(json!({"popout": p["popout"], "spec": value}))
}

fn has_popouts(s: &Session) -> std::result::Result<(), String> {
    if s.views.popouts.is_empty() { Err("no pop-out is open".into()) } else { Ok(()) }
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "viewer.chart",
        label: "Chart",
        menu: &[],
        shortcut: None,
        params: "{popout, mark?: histogram | line | column | treemap, x?, y?: [column], color?, size?, path?: [column], agg?: sum | mean | count | min | max, stack?, bins?, clear?} -> {popout, spec}: the chart of the value a pop-out shows, its columns checked against the value's; kept in the graph's meta.charts[\"node.port\"], one undo step, saved with the file",
        enabled: has_popouts,
        run: chart,
        journal: true,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::display::Display;

    #[test]
    fn a_tables_columns_and_whether_they_are_numbers() {
        let d = Display {
            value: Some(json!({"columns": [{"name": "city", "type": "STRING", "values": []}, {"name": "n", "type": "INT", "values": []}]})),
            ..Default::default()
        };
        assert_eq!(columns_of(&d), vec![("city".into(), false), ("n".into(), true)]);
        assert!(columns_of(&Display::default()).is_empty());
    }
}
