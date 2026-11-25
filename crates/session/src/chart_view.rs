//! A chart's view and the value's selection (note 26 slice 8): what part of the chart a pop-out
//! shows (`viewer.range {popout, x?, y?, reset?}`, its zoom and pan, the pop-out's own state like
//! a picture's zoom) and which rows of the value are selected (`viewer.select {popout, brush? |
//! rows? | clear?}`). The selection belongs to the value, its node and port (the maintainer's
//! decision 7A), not to a window: a table and a chart popped out of one port show the same rows.
//! Neither is saved with the graph nor undoable (a view, like a picture's zoom), and neither is in
//! the result hash.
//!
//! The core resolves a brush to rows when it reads the value (`viewer.data`): here a brush is kept
//! as asked, a column and a span of its numbers (times as seconds, `plumcraft_proto::time`).

use std::collections::BTreeMap;

use serde::{Serialize, Serializer};
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::run::WatchRole;
use crate::{Result, Session, SessionError};

/// A span of numbers, `lo < hi`, both finite; kept as their bits so a view's state stays `Eq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    lo: u64,
    hi: u64,
}

impl Span {
    pub fn new(lo: f64, hi: f64) -> Option<Span> {
        (lo.is_finite() && hi.is_finite() && lo < hi).then(|| Span { lo: lo.to_bits(), hi: hi.to_bits() })
    }
    pub fn lo(self) -> f64 {
        f64::from_bits(self.lo)
    }
    pub fn hi(self) -> f64 {
        f64::from_bits(self.hi)
    }
    pub fn contains(self, v: f64) -> bool {
        v >= self.lo() && v <= self.hi()
    }
}

impl Serialize for Span {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        [self.lo(), self.hi()].serialize(s)
    }
}

/// What part of a chart a pop-out shows; `None`: all of it, as the core fits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct ChartRange {
    pub x: Option<Span>,
    pub y: Option<Span>,
}

/// The most rows a selection names one by one.
pub const MAX_ROWS: usize = 100_000;

/// The rows of a value selected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Selection {
    /// The rows whose `column` (none: the chart's x, or a series' index) is in `span`.
    Brush { column: Option<String>, span: Span },
    /// These rows, by their index in the value, sorted, no repeats.
    Rows { rows: Vec<u64> },
}

/// The selections, by `node.port`.
pub type Selections = BTreeMap<String, Selection>;

fn bad(m: impl Into<String>) -> SessionError {
    SessionError::Params(m.into())
}

/// `[lo, hi]` in `p[k]`: `Some(None)` for null (all of it), `None` when absent.
fn span(p: &Value, k: &str) -> Result<Option<Option<Span>>> {
    match p.get(k) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v) => match v.as_array().map(Vec::as_slice) {
            Some([a, b]) => match (a.as_f64(), b.as_f64()) {
                (Some(a), Some(b)) => {
                    Span::new(a.min(b), a.max(b)).map(|s| Some(Some(s))).ok_or_else(|| bad(format!("{k}: [lo, hi], two different finite numbers")))
                }
                _ => Err(bad(format!("{k}: [lo, hi], numbers"))),
            },
            _ => Err(bad(format!("{k} is [lo, hi] or null"))),
        },
    }
}

fn only(p: &Value, cmd: &str, allowed: &[&str]) -> Result<()> {
    if let Some(o) = p.as_object()
        && let Some(k) = o.keys().find(|k| *k != "popout" && !allowed.contains(&k.as_str()))
    {
        return Err(bad(format!("{cmd} takes popout and {}, not {:?}", allowed.join(", "), k.chars().take(64).collect::<String>())));
    }
    Ok(())
}

fn popout_id(p: &Value) -> Result<u64> {
    p.get("popout").and_then(Value::as_u64).ok_or_else(|| bad("\"popout\" (a pop-out's id) is required"))
}

/// `viewer.range {popout, x?: [lo, hi] | null, y?: [lo, hi] | null, reset?}`.
fn range(s: &mut Session, p: &Value) -> Result<Value> {
    only(p, "viewer.range", &["x", "y", "reset"])?;
    let (x, y) = (span(p, "x")?, span(p, "y")?);
    let reset = p.get("reset").and_then(Value::as_bool) == Some(true);
    let id = popout_id(p)?;
    let o = s.views.popouts.iter_mut().find(|o| o.id == id).ok_or_else(|| bad(format!("no pop-out {id}")))?;
    let r = &mut o.state.chart_range;
    if reset {
        *r = ChartRange::default();
    }
    if let Some(x) = x {
        r.x = x;
    }
    if let Some(y) = y {
        r.y = y;
    }
    Ok(json!({"popout": id, "range": *r}))
}

/// The `node.port` a pop-out shows.
pub fn port_of(s: &Session, id: u64) -> Option<String> {
    s.run.entry(WatchRole::Popout(id)).map(|(w, _)| format!("{}.{}", w.node, w.port))
}

/// `viewer.select {popout, brush?: {column?, x: [lo, hi]}, rows?: [index], clear?}`.
fn select(s: &mut Session, p: &Value) -> Result<Value> {
    only(p, "viewer.select", &["brush", "rows", "clear"])?;
    let id = popout_id(p)?;
    if s.views.popout(id).is_none() {
        return Err(bad(format!("no pop-out {id}")));
    }
    let key = port_of(s, id).ok_or_else(|| bad("no value yet: run the graph"))?;
    let given = ["brush", "rows", "clear"].iter().filter(|k| p.get(**k).is_some_and(|v| !v.is_null())).count();
    if given != 1 {
        return Err(bad("viewer.select takes one of brush, rows or clear"));
    }
    s.views.selection_seq = s.views.selection_seq.saturating_add(1);
    if p.get("clear").and_then(Value::as_bool) == Some(true) {
        s.views.selections.remove(&key);
        return Ok(json!({"popout": id, "port": key, "selection": null}));
    }
    let sel = if let Some(b) = p.get("brush").filter(|v| !v.is_null()) {
        let column = match b.get("column") {
            None | Some(Value::Null) => None,
            Some(Value::String(c)) if c.len() <= 256 => Some(c.clone()),
            Some(_) => return Err(bad("brush.column is a column's name")),
        };
        let span = span(b, "x")?.flatten().ok_or_else(|| bad("brush is {column?, x: [lo, hi]}"))?;
        // the chart's x when the brush was drawn, kept: the selection stays those rows when the
        // chart later shows another x (none: the x the core picks, or the row's index)
        let column = column.or_else(|| {
            let (node, port) = key.split_once('.')?;
            crate::chart_cmds::spec_of(s.graph(), node, port)?.get("x")?.as_str().map(str::to_owned)
        });
        Selection::Brush { column, span }
    } else if let Some(r) = p.get("rows").filter(|v| !v.is_null()) {
        let list = r.as_array().ok_or_else(|| bad("rows is a list of row indices"))?;
        if list.len() > MAX_ROWS {
            return Err(bad(format!("rows: at most {MAX_ROWS}")));
        }
        let mut rows = list.iter().map(|v| v.as_u64().ok_or_else(|| bad("rows are whole numbers"))).collect::<Result<Vec<u64>>>()?;
        rows.sort_unstable();
        rows.dedup();
        Selection::Rows { rows }
    } else {
        return Err(bad("clear is true"));
    };
    let out = json!({"popout": id, "port": key, "selection": sel});
    s.views.selections.insert(key, sel);
    Ok(out)
}

fn has_popouts(s: &Session) -> std::result::Result<(), String> {
    if s.views.popouts.is_empty() { Err("no pop-out is open".into()) } else { Ok(()) }
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "viewer.range",
            label: "Zoom Chart",
            menu: &[],
            shortcut: None,
            params: "{popout, x?: [lo, hi] | null, y?: [lo, hi] | null, reset?} -> {popout, range}: the part of a pop-out's chart shown (zoom and pan; null or reset: all of it); the data drawn again at the new resolution",
            enabled: has_popouts,
            run: range,
            journal: false,
        },
        CommandSpec {
            id: "viewer.select",
            label: "Select Rows",
            menu: &[],
            shortcut: None,
            params: "{popout, brush?: {column?, x: [lo, hi]}, rows?: [index], clear?} -> {popout, port, selection}: the rows of the value a pop-out shows selected, by a brushed span of a column (none: the chart's x) or by index; the selection is the value's (node and port), so every pop-out of it shows the same rows",
            enabled: has_popouts,
            run: select,
            journal: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_are_ordered_finite_and_serialised_as_numbers() {
        assert!(Span::new(1.0, 1.0).is_none() && Span::new(f64::NAN, 1.0).is_none() && Span::new(2.0, 1.0).is_none());
        let s = Span::new(-1.5, 2.0).unwrap();
        assert_eq!(serde_json::to_value(s).unwrap(), json!([-1.5, 2.0]));
        assert!(s.contains(0.0) && !s.contains(3.0));
        assert_eq!(span(&json!({"x": [3, 1]}), "x").unwrap(), Some(Some(Span::new(1.0, 3.0).unwrap())), "either order");
        assert!(span(&json!({"x": [1]}), "x").is_err() && span(&json!({"x": "a"}), "x").is_err());
        let sel = Selection::Brush { column: Some("t".into()), span: s };
        assert_eq!(serde_json::to_value(&sel).unwrap(), json!({"kind": "brush", "column": "t", "span": [-1.5, 2.0]}));
    }
}
