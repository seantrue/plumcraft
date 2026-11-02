//! Sequences: Zip, Outer Join and Unzip, over any values (the `ANY` wire type).
//!
//! - **Zip** pairs up its inputs item by item: `(a1, b1), (a2, b2), …`. The lists must be the
//!   same length; a single value goes with every item.
//! - **Outer Join** pairs every item of one with every item of the others (the first varying
//!   slowest): every combination, as one list.
//! - **Unzip** splits a list of tuples back into its parts, lists that share one axis, so they
//!   zip again downstream (the settings they feed vary together). A single tuple splits into
//!   single values.
//!
//! - **Table to Rows** turns a table (named columns, as Letter counts, Transients and Histogram
//!   make it) into rows, a header row first: what a spreadsheet's Write range takes. **Rows to
//!   Table** goes back: rows (a spreadsheet's Read range), the first row the names, into a table.
//!
//! A tuple is a list inside a list ([`Data::List`]): Vary Range 2D's `xy` makes the same. Each
//! tuple is labelled with what made its parts, so a sheet or Write Files can name it.

use plumcraft_proto::catalogue::{NodeDef, NodeFlags, Port, Socket};
use plumcraft_proto::error::NodeError;

use crate::exec::{MAX_RUNS, item_label};
use crate::{ANY, Cx, Data, Inputs, List, Node, Outputs};

/// The parts a tuple has at most (Zip's and Outer Join's inputs, Unzip's outputs).
const PARTS: [&str; 3] = ["a", "b", "c"];

fn category() -> Vec<String> {
    vec!["Building blocks".into(), "Sequences".into()]
}

fn config(message: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::CONFIG, message)
}

fn any_in(name: &str, optional: bool, tip: &str) -> Port {
    Port { name: name.into(), ty: ANY.into(), socket: Some(Socket::Socket), optional, tooltip: Some(tip.into()), ..Default::default() }
}

fn any_out(name: &str) -> Port {
    Port { name: name.into(), ty: ANY.into(), list: true, socket: Some(Socket::Socket), ..Default::default() }
}

/// The inputs a, b (and c, when wired): each a list, or a single value (`None` length).
fn parts(inputs: &Inputs) -> Vec<(&'static str, &Data)> {
    PARTS.iter().filter_map(|p| inputs.get(*p).map(|d| (*p, d))).collect()
}

/// Item `i` of a part (a single value is every item) and what it says about the tuple.
fn pick<'a>(port: &str, d: &'a Data, i: usize) -> (&'a Data, String) {
    match d {
        Data::List(l) => (l.items.get(i).unwrap_or(d), item_label(port, l, i)),
        one => (one, String::new()),
    }
}

/// A tuple from its parts, labelled by their labels (repeats dropped).
fn tuple(parts: Vec<(&Data, String)>) -> (Data, String) {
    let mut label: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(parts.len());
    for (d, l) in parts {
        items.push(d.clone());
        if !l.is_empty() && !label.contains(&l) {
            label.push(l);
        }
    }
    (Data::List(List::new(items)), label.join(" · "))
}

fn tuples_out(tuples: Vec<(Data, String)>) -> Outputs {
    let (items, labels) = tuples.into_iter().unzip();
    Outputs::from([("tuples".to_owned(), Data::List(List { items, axes: Vec::new(), labels }))])
}

/// Zip or Outer Join.
struct Combine {
    outer: bool,
}

impl Node for Combine {
    fn def(&self) -> NodeDef {
        let (id, display, description) = if self.outer {
            (
                "seq.outer",
                "Outer Join",
                "Every combination of the items of a, b (and c), as one list of tuples, the first varying slowest. A single value goes with every item. Unzip splits the tuples again.",
            )
        } else {
            (
                "seq.zip",
                "Zip",
                "The items of a, b (and c) paired up in order, as one list of tuples: (a1, b1), (a2, b2), … The lists must be the same length; a single value goes with every item. Unzip splits the tuples again.",
            )
        };
        NodeDef {
            id: id.into(),
            display: display.into(),
            category: category(),
            description: description.into(),
            inputs: vec![
                any_in("a", false, "A sequence, or a single value"),
                any_in("b", false, "A sequence, or a single value"),
                any_in("c", true, "A third sequence (optional)"),
            ],
            outputs: vec![any_out("tuples")],
            flags: NodeFlags { input_list: true, ..Default::default() },
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
        let parts = parts(inputs);
        let len = |d: &Data| d.list().map_or(1, |l| l.items.len());
        if self.outer {
            let total = parts.iter().try_fold(1usize, |t, (_, d)| t.checked_mul(len(d))).filter(|t| *t <= MAX_RUNS);
            let total = total.ok_or_else(|| config(format!("more than {MAX_RUNS} combinations")))?;
            let mut out = Vec::with_capacity(total);
            for t in 0..total {
                // row-major: the first part varies slowest
                let mut idx = vec![0usize; parts.len()];
                let mut rest = t;
                for (slot, (_, d)) in idx.iter_mut().zip(&parts).rev() {
                    let n = len(d).max(1);
                    *slot = rest % n;
                    rest /= n;
                }
                out.push(tuple(parts.iter().zip(&idx).map(|((p, d), i)| pick(p, d, *i)).collect()));
            }
            return Ok(tuples_out(out));
        }
        let lengths: Vec<(&str, usize)> = parts.iter().filter(|(_, d)| d.list().is_some()).map(|(p, d)| (*p, len(d))).collect();
        let n = lengths.first().map_or(1, |(_, n)| *n);
        if let Some((p, m)) = lengths.iter().find(|(_, m)| *m != n) {
            return Err(config(format!("Zip: the sequences differ in length ({} has {n}, {p} has {m})", lengths[0].0)));
        }
        Ok(tuples_out((0..n).map(|i| tuple(parts.iter().map(|(p, d)| pick(p, d, i)).collect())).collect()))
    }
}

/// Unzip: tuples back into their parts.
struct Unzip;

impl Node for Unzip {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "seq.unzip".into(),
            display: "Unzip".into(),
            category: category(),
            description: "A list of tuples (from Zip, Outer Join or Vary Range 2D's xy) split into its parts: a, b and c are lists that zip downstream, so the settings they feed vary together. A single tuple splits into single values.".into(),
            inputs: vec![any_in("tuples", false, "A list of tuples, or one tuple")],
            outputs: PARTS.iter().map(|p| any_out(p)).collect(),
            flags: NodeFlags { input_list: true, ..Default::default() },
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
        let l = inputs.get("tuples").and_then(Data::list).ok_or_else(|| config("tuples: wire a list of tuples (Zip, Outer Join, Vary Range 2D's xy)"))?;
        // one tuple (its items aren't tuples): single values
        if !l.items.iter().all(|i| i.list().is_some()) || l.items.is_empty() {
            if l.items.is_empty() {
                return Err(config("tuples: the list is empty"));
            }
            return Ok(PARTS.iter().zip(&l.items).map(|(p, d)| ((*p).to_owned(), d.clone())).collect());
        }
        let width = l.items.iter().filter_map(Data::list).map(|t| t.items.len()).min().unwrap_or(0);
        if width == 0 {
            return Err(config("tuples: a tuple is empty"));
        }
        // one axis, named by the node: the parts zip downstream
        let axes = vec![(cx.node_id().to_owned(), l.items.len())];
        let mut out = Outputs::new();
        for (k, p) in PARTS.iter().enumerate().take(width) {
            let items = l.items.iter().filter_map(Data::list).map(|t| t.items.get(k).cloned().unwrap_or(Data::Json(serde_json::Value::Null))).collect();
            out.insert((*p).to_owned(), Data::List(List { items, axes: axes.clone(), labels: l.labels.clone() }));
        }
        Ok(out)
    }
}

/// Zip, Outer Join and Unzip.
/// The most rows Table to Rows makes.
pub const MAX_ROWS: usize = 100_000;

/// Table to Rows: `{"columns": [{name, values}]}` into `[[names…], [row…], …]`.
struct TableRows;

impl Node for TableRows {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "seq.table_rows".into(),
            display: "Table to Rows".into(),
            category: category(),
            description: "A table (named columns, as Letter counts or Transients make it, or a 2-D table array, as Histogram makes it) as rows of values, the column names first: what a spreadsheet's Write range takes. Rows already in rows pass through.".into(),
            inputs: vec![
                any_in("table", false, "A table: {\"columns\": [{name, values}]}"),
                Port { name: "header".into(), ty: "BOOL".into(), default: serde_json::json!(true), tooltip: Some("Put the column names first".into()), ..Default::default() },
            ],
            outputs: vec![Port { name: "rows".into(), ty: ANY.into(), socket: Some(Socket::Socket), ..Default::default() }],
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let header = inputs.get("header").and_then(Data::json).and_then(serde_json::Value::as_bool).unwrap_or(true);
        let rows = match inputs.get("table") {
            Some(Data::Json(v)) => table_rows(v, header)?,
            Some(Data::Array(a)) => array_rows(a, header)?,
            _ => return Err(config("table: wire a table, or a 2-D array (a table array: Histogram)")),
        };
        Ok(Outputs::from([("rows".to_owned(), Data::Json(rows))]))
    }
}

/// A table's rows (see [`TableRows`]).
pub fn table_rows(v: &serde_json::Value, header: bool) -> Result<serde_json::Value, NodeError> {
    use serde_json::Value;
    if let Some(rows) = v.as_array().filter(|r| r.iter().all(Value::is_array)) {
        return Ok(Value::Array(rows.clone()));
    }
    let cols = v.get("columns").and_then(Value::as_array).ok_or_else(|| config("table: not a table (no columns)"))?;
    let values: Vec<&Vec<Value>> =
        cols.iter().map(|c| c.get("values").and_then(Value::as_array).ok_or_else(|| config("table: a column without values"))).collect::<Result<_, _>>()?;
    let n = values.iter().map(|v| v.len()).max().unwrap_or(0);
    if n > MAX_ROWS {
        return Err(config(format!("table: {n} rows; at most {MAX_ROWS}")));
    }
    let mut rows: Vec<Value> = Vec::with_capacity(n + 1);
    if header {
        rows.push(Value::Array(cols.iter().map(|c| c.get("name").cloned().unwrap_or(Value::Null)).collect()));
    }
    for i in 0..n {
        rows.push(Value::Array(values.iter().map(|v| v.get(i).cloned().unwrap_or(Value::Null)).collect()));
    }
    Ok(Value::Array(rows))
}

/// A 2-D array's rows (a table array: its channel names are the header; else c1, c2, …).
pub fn array_rows(a: &crate::Array, header: bool) -> Result<serde_json::Value, NodeError> {
    use serde_json::Value;
    let [n, k] = a.shape.as_slice() else { return Err(config(format!("table: a 2-D array, not shape {:?}", a.shape))) };
    let (n, k) = (usize::try_from(*n).unwrap_or(usize::MAX), usize::try_from(*k).unwrap_or(usize::MAX));
    if n > MAX_ROWS || k > 4096 {
        return Err(config(format!("table: {n} × {k}; at most {MAX_ROWS} rows of 4096")));
    }
    let values = crate::arrays::values(a).ok_or_else(|| config(format!("table: unsupported dtype {:?}", a.dtype)))?;
    let mut rows: Vec<Value> = Vec::with_capacity(n + 1);
    if header {
        rows.push(Value::Array((0..k).map(|j| Value::from(a.names.get(j).cloned().unwrap_or_else(|| format!("c{}", j + 1)))).collect()));
    }
    for r in values.chunks(k.max(1)).take(n) {
        rows.push(Value::Array(r.iter().map(|v| serde_json::Number::from_f64(f64::from(*v)).map_or(Value::Null, Value::Number)).collect()));
    }
    Ok(Value::Array(rows))
}

/// Rows to Table: `[[names…], [row…], …]` into `{"columns": [{name, type, values}]}`.
struct RowsTable;

impl Node for RowsTable {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "seq.rows_table".into(),
            display: "Rows to Table".into(),
            category: category(),
            description: "Rows of values (a spreadsheet's Read range, or Table to Rows) as a table of named columns, the first row the names (without a header, c1, c2, …). A column is INT, FLOAT or BOOL when every value is one, else STRING. Table to Rows goes the other way.".into(),
            inputs: vec![
                any_in("rows", false, "Rows: a list of lists"),
                Port { name: "header".into(), ty: "BOOL".into(), default: serde_json::json!(true), tooltip: Some("The first row holds the column names".into()), ..Default::default() },
            ],
            outputs: vec![Port { name: "table".into(), ty: ANY.into(), socket: Some(Socket::Socket), ..Default::default() }],
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let header = inputs.get("header").and_then(Data::json).and_then(serde_json::Value::as_bool).unwrap_or(true);
        let v = inputs.get("rows").and_then(Data::json).ok_or_else(|| config("rows: wire rows (a list of lists)"))?;
        Ok(Outputs::from([("table".to_owned(), Data::Json(rows_table(v, header)?))]))
    }
}

/// Rows as a table (see [`RowsTable`]); a table passes through.
pub fn rows_table(v: &serde_json::Value, header: bool) -> Result<serde_json::Value, NodeError> {
    use serde_json::{Value, json};
    if v.get("columns").is_some_and(Value::is_array) {
        return Ok(v.clone());
    }
    let rows = v.as_array().filter(|r| r.iter().all(Value::is_array)).ok_or_else(|| config("rows: not a list of lists"))?;
    if rows.len() > MAX_ROWS + 1 {
        return Err(config(format!("rows: {} rows; at most {MAX_ROWS}", rows.len())));
    }
    let width = rows.iter().filter_map(Value::as_array).map(Vec::len).max().unwrap_or(0);
    if width > 4096 {
        return Err(config(format!("rows: {width} columns; at most 4096")));
    }
    let (names, body): (Vec<String>, &[Value]) = match (header, rows.split_first()) {
        (true, Some((first, rest))) => {
            let first = first.as_array().map(Vec::as_slice).unwrap_or_default();
            let name = |j: usize| match first.get(j) {
                Some(Value::String(s)) if !s.is_empty() => s.clone(),
                Some(Value::Null) | None => format!("c{}", j + 1),
                Some(other) => other.to_string(),
            };
            ((0..width).map(name).collect(), rest)
        }
        _ => ((1..=width).map(|j| format!("c{j}")).collect(), rows.as_slice()),
    };
    let columns: Vec<Value> = names
        .into_iter()
        .enumerate()
        .map(|(j, name)| {
            let values: Vec<Value> = body.iter().map(|r| r.get(j).cloned().unwrap_or(Value::Null)).collect();
            let present = || values.iter().filter(|v| !v.is_null());
            let ty = if present().all(|v| v.is_i64() || v.is_u64()) && present().next().is_some() {
                "INT"
            } else if present().all(Value::is_number) && present().next().is_some() {
                "FLOAT"
            } else if present().all(Value::is_boolean) && present().next().is_some() {
                "BOOL"
            } else {
                "STRING"
            };
            json!({"name": name, "type": ty, "values": values})
        })
        .collect();
    Ok(json!({"columns": columns}))
}

pub fn nodes() -> Vec<Box<dyn Node>> {
    vec![Box::new(Combine { outer: false }), Box::new(Combine { outer: true }), Box::new(Unzip), Box::new(TableRows), Box::new(RowsTable)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Table to Rows (as `python/tests/test_vary.py`): named columns as rows, the names first; a
    /// table array likewise; rows pass through; bad input is a config error.
    #[test]
    fn table_to_rows() {
        let t = json!({"columns": [{"name": "letter", "values": ["a", "b"]}, {"name": "count", "values": [3, 1]}]});
        assert_eq!(table_rows(&t, true).unwrap(), json!([["letter", "count"], ["a", 3], ["b", 1]]));
        assert_eq!(table_rows(&t, false).unwrap(), json!([["a", 3], ["b", 1]]));
        assert_eq!(table_rows(&json!([[1, 2], [3, 4]]), true).unwrap(), json!([[1, 2], [3, 4]]));
        let mut a = crate::Array::from_f32(vec![2, 2], &[0.25, 2.0, 0.75, f32::NAN]);
        a.names = vec!["centre".into(), "count".into()];
        a.table = true;
        assert_eq!(array_rows(&a, true).unwrap(), json!([["centre", "count"], [0.25, 2.0], [0.75, null]]));
        assert_eq!(array_rows(&crate::Array::from_f32(vec![1, 2], &[0.0, 0.0]), true).unwrap()[0], json!(["c1", "c2"]));
        for bad in [json!({"rows": []}), json!({"columns": [{"name": "x"}]}), json!(7)] {
            assert!(table_rows(&bad, true).is_err(), "{bad}");
        }
        assert!(array_rows(&crate::Array::from_f32(vec![2, 2, 2], &[0.0; 8]), true).is_err());
    }

    /// Rows to Table, the other way: names from the first row, column types from the values,
    /// and a round trip through Table to Rows gives the table back.
    #[test]
    fn rows_to_table() {
        let rows = json!([["letter", "count", "share", "ok"], ["a", 3, 0.5, true], ["b", 1, 1, null]]);
        let t = rows_table(&rows, true).unwrap();
        assert_eq!(
            t,
            json!({"columns": [
                {"name": "letter", "type": "STRING", "values": ["a", "b"]},
                {"name": "count", "type": "INT", "values": [3, 1]},
                {"name": "share", "type": "FLOAT", "values": [0.5, 1]},
                {"name": "ok", "type": "BOOL", "values": [true, null]},
            ]})
        );
        assert_eq!(table_rows(&t, true).unwrap(), rows, "a round trip");
        assert_eq!(rows_table(&json!([[1, 2]]), false).unwrap()["columns"][1]["name"], "c2");
        assert_eq!(rows_table(&t, true).unwrap(), t, "a table passes through");
        for bad in [json!(7), json!([1, 2]), json!({"rows": []})] {
            assert!(rows_table(&bad, true).is_err(), "{bad}");
        }
    }
}
