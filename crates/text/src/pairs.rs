//! Converters between core types (`docs/research/20-convert.md`, 6.3), each with its inverse:
//! Text and Table (the text as CSV, read and written as [`crate::files`] reads and writes a CSV
//! file), JSON and Text, and Table and Array. The Python twin has the same nodes and rules.
//!
//! - **JSON to Text** writes the value indented by two spaces, keys sorted (as both engines'
//!   JSON libraries agree on); **Text to JSON** parses it, and text that isn't JSON (or holds
//!   `NaN`) is a `config` error.
//! - **Table to Array** takes the INT and FLOAT columns as an `<f8` (rows, columns) table array
//!   with the columns' names; a null is NaN, and the other columns are left out. **Array to
//!   Table** takes a 1-D (N) or 2-D (N, k) array as columns named by the array's names (when
//!   there is one a column) or `c1`…`ck`: INT for an integer array, BOOL for a boolean one,
//!   else FLOAT, with NaN and ±Inf as null. Both ends are a series, never a picture.

use plumcraft_engine::{Array, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::catalogue::{NodeDef, Port, Socket};
use plumcraft_proto::convert::{Converts, SERIES};
use plumcraft_proto::error::NodeError;
use serde_json::{Number, Value, json};

use crate::files::{read_csv, write_csv};
use crate::{ARRAY, TABLE, TEXT};

pub const JSON: &str = plumcraft_proto::catalogue::core_types::JSON;

/// The most cells Array to Table and Table to Array make.
const MAX_CELLS: usize = 16 << 20;
/// The longest JSON Text to JSON reads.
const MAX_TEXT: usize = 64 << 20;

struct Pair {
    id: &'static str,
    display: &'static str,
    description: &'static str,
    from: &'static str,
    to: &'static str,
    lossless: bool,
}

pub fn nodes() -> Vec<Box<dyn Node>> {
    let all = [
        Pair {
            id: "text.text_to_table",
            display: "Text to Table",
            description: "CSV text as a table, as CSV to Table reads a file: the first row names the columns, and each column's type comes from its values. The inverse of Table to Text.",
            from: TEXT,
            to: TABLE,
            lossless: false,
        },
        Pair {
            id: "text.table_to_text",
            display: "Table to Text",
            description: "A table as CSV text, a header line first, as Table to CSV writes a file. The inverse of Text to Table.",
            from: TABLE,
            to: TEXT,
            lossless: false,
        },
        Pair {
            id: "text.json_to_text",
            display: "JSON to Text",
            description: "A JSON value as text, indented by two spaces, keys sorted. The inverse of Text to JSON.",
            from: JSON,
            to: TEXT,
            lossless: true,
        },
        Pair {
            id: "text.text_to_json",
            display: "Text to JSON",
            description: "Text that is JSON as its value. Text that isn't is an error. The inverse of JSON to Text.",
            from: TEXT,
            to: JSON,
            lossless: false,
        },
        Pair {
            id: "text.table_to_array",
            display: "Table to Array",
            description: "A table's INT and FLOAT columns as a rows × columns array (float64), named by the columns; a null is NaN, and other columns are left out. The inverse of Array to Table.",
            from: TABLE,
            to: ARRAY,
            lossless: false,
        },
        Pair {
            id: "text.array_to_table",
            display: "Array to Table",
            description: "A 1-D or rows × columns array as a table, a column for each array column, named by the array's names or c1, c2, …: INT, BOOL or FLOAT as the array's type, NaN as null. The inverse of Table to Array.",
            from: ARRAY,
            to: TABLE,
            lossless: false,
        },
    ];
    all.into_iter().map(|p| Box::new(p) as Box<dyn Node>).collect()
}

fn port(ty: &str) -> String {
    ty.to_ascii_lowercase()
}

impl Node for Pair {
    fn def(&self) -> NodeDef {
        let (input, output) = (port(self.from), port(self.to));
        let mut mark = Converts { input: Some(input.clone()), output: Some(output.clone()), ..Converts::new(self.from, self.to, self.lossless) };
        if self.from == ARRAY {
            mark = mark.from_shape(SERIES);
        }
        if self.to == ARRAY {
            mark = mark.to_shape(SERIES);
        }
        NodeDef {
            id: self.id.into(),
            display: self.display.into(),
            category: vec!["Text".into(), "Convert".into()],
            description: self.description.into(),
            inputs: vec![Port { name: input, ty: self.from.into(), socket: Some(Socket::Socket), ..Default::default() }],
            outputs: vec![Port { name: output, ty: self.to.into(), ..Default::default() }],
            family: "text".into(),
            converts: vec![mark],
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
        let input = port(self.from);
        let got = inputs.get(&input);
        let text = || match got {
            Some(Data::Json(Value::String(s))) => Ok(s.as_str()),
            _ => Err(config(format!("{input}: wire text"))),
        };
        let table = || match got {
            Some(Data::Json(v)) => Ok(v),
            _ => Err(config(format!("{input}: wire a table"))),
        };
        let out = match (self.from, self.to) {
            (TEXT, TABLE) => Data::Json(read_csv(text()?)?),
            (TABLE, TEXT) => Data::Json(json!(write_csv(table()?)?)),
            (JSON, _) => Data::Json(json!(json_text(got)?)),
            (TEXT, _) => Data::Json(parse_json(text()?)?),
            (TABLE, _) => Data::Array(table_array(table()?)?),
            _ => match got {
                Some(Data::Array(a)) => Data::Json(array_table(a)?),
                _ => return Err(config(format!("{input}: wire an array"))),
            },
        };
        Ok(Outputs::from([(port(self.to), out)]))
    }
}

fn config(msg: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::CONFIG, msg)
}

/// A JSON value (inline, or a blob of JSON when large) as indented text.
fn json_text(got: Option<&Data>) -> Result<String, NodeError> {
    let value = match got {
        Some(Data::Json(v)) => v.clone(),
        Some(Data::Blob(b)) => serde_json::from_slice(&b.read(MAX_TEXT as u64)?).map_err(|e| config(format!("json: the blob isn't JSON ({e})")))?,
        None => Value::Null,
        _ => return Err(config("json: wire a JSON value")),
    };
    serde_json::to_string_pretty(&value).map_err(|e| NodeError::new(NodeError::OP, format!("writing JSON: {e}")))
}

/// Text that is JSON, as its value.
pub fn parse_json(text: &str) -> Result<Value, NodeError> {
    if text.len() > MAX_TEXT {
        return Err(config(format!("text: longer than {} MiB", MAX_TEXT >> 20)));
    }
    serde_json::from_str(text).map_err(|e| config(format!("text: not JSON ({e})")))
}

/// A table's numeric columns as an `<f8` (rows, columns) table array.
pub fn table_array(table: &Value) -> Result<Array, NodeError> {
    let columns = table.get("columns").and_then(Value::as_array).ok_or_else(|| config("table: not a table (no columns)"))?;
    let numeric: Vec<(&str, &Vec<Value>)> = columns
        .iter()
        .filter(|c| matches!(c.get("type").and_then(Value::as_str), Some("INT" | "FLOAT")))
        .filter_map(|c| Some((c.get("name").and_then(Value::as_str).unwrap_or(""), c.get("values").and_then(Value::as_array)?)))
        .collect();
    if numeric.is_empty() {
        return Err(config("table: it has no INT or FLOAT column"));
    }
    let rows = numeric.first().map_or(0, |c| c.1.len());
    if numeric.iter().any(|c| c.1.len() != rows) {
        return Err(config("table: its columns differ in length"));
    }
    if rows.saturating_mul(numeric.len()) > MAX_CELLS {
        return Err(config(format!("table: more than {} M cells", MAX_CELLS >> 20)));
    }
    let mut bytes = Vec::with_capacity(rows * numeric.len() * 8);
    for r in 0..rows {
        for c in &numeric {
            let v = c.1.get(r).and_then(Value::as_f64).unwrap_or(f64::NAN);
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    let names = numeric.iter().map(|c| c.0.to_owned()).collect();
    Ok(Array { table: true, names, ..Array::new("<f8", vec![rows as u64, numeric.len() as u64], bytes) })
}

/// The kind of value an array's dtype holds, and its size in bytes.
fn dtype(d: &str) -> Option<(char, usize)> {
    let (order, rest) = d.split_at_checked(1)?;
    let (kind, size) = rest.split_at_checked(1)?;
    let size: usize = size.parse().ok()?;
    let kind = kind.chars().next()?;
    let ok = match kind {
        'f' => matches!(size, 4 | 8),
        'i' | 'u' => matches!(size, 1 | 2 | 4 | 8),
        'b' => size == 1,
        _ => false,
    };
    // little-endian, or one byte, which has no order
    (ok && (order == "<" || (order == "|" && size == 1))).then_some((kind, size))
}

fn cell(kind: char, b: &[u8]) -> Value {
    let mut w = [0u8; 8];
    let n = b.len().min(8);
    w[..n].copy_from_slice(&b[..n]);
    let sign = |bits: u32| {
        let shift = 64 - bits;
        (i64::from_le_bytes(w) << shift) >> shift
    };
    match (kind, b.len()) {
        ('b', _) => json!(w[0] != 0),
        ('u', _) => json!(u64::from_le_bytes(w)),
        ('i', n) => json!(sign(n as u32 * 8)),
        ('f', 4) => Number::from_f64(f64::from(f32::from_le_bytes([w[0], w[1], w[2], w[3]]))).map_or(Value::Null, Value::Number),
        _ => Number::from_f64(f64::from_le_bytes(w)).map_or(Value::Null, Value::Number),
    }
}

/// A 1-D or 2-D array as a table.
pub fn array_table(a: &Array) -> Result<Value, NodeError> {
    let (rows, cols) = match a.shape.as_slice() {
        [n] => (*n, 1),
        [n, k] => (*n, *k),
        s => return Err(config(format!("array: a 1-D or rows × columns array, not {}", s.iter().map(u64::to_string).collect::<Vec<_>>().join("×")))),
    };
    let (rows, cols) = (usize::try_from(rows).unwrap_or(usize::MAX), usize::try_from(cols).unwrap_or(usize::MAX));
    if rows.saturating_mul(cols) > MAX_CELLS {
        return Err(config(format!("array: more than {} M cells", MAX_CELLS >> 20)));
    }
    let (kind, size) = dtype(&a.dtype).ok_or_else(|| config(format!("array: its type {} isn't one a table holds", a.dtype)))?;
    if a.bytes.len() != rows * cols * size {
        return Err(config("array: its bytes don't fill its shape"));
    }
    let ty = match kind {
        'i' | 'u' => "INT",
        'b' => "BOOL",
        _ => "FLOAT",
    };
    let names: Vec<String> = if a.names.len() == cols { a.names.clone() } else { (1..=cols).map(|i| format!("c{i}")).collect() };
    let columns: Vec<Value> = names
        .iter()
        .enumerate()
        .map(|(c, name)| {
            let values: Vec<Value> =
                (0..rows).map(|r| a.bytes.get((r * cols + c) * size..(r * cols + c + 1) * size).map_or(Value::Null, |b| cell(kind, b))).collect();
            json!({"name": name, "type": ty, "values": values})
        })
        .collect();
    Ok(json!({"columns": columns}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_and_arrays_go_both_ways() {
        let t = json!({"columns": [
            {"name": "x", "type": "INT", "values": [1, 2, null]},
            {"name": "label", "type": "STRING", "values": ["a", "b", "c"]},
            {"name": "y", "type": "FLOAT", "values": [0.5, -1.0, 2.0]},
        ]});
        let a = table_array(&t).unwrap();
        assert_eq!((a.dtype.as_str(), a.shape.clone(), a.names.clone(), a.table), ("<f8", vec![3, 2], vec!["x".into(), "y".into()], true));
        let back = array_table(&a).unwrap();
        assert_eq!(back["columns"][0], json!({"name": "x", "type": "FLOAT", "values": [1.0, 2.0, null]}));
        assert_eq!(back["columns"][1]["values"], json!([0.5, -1.0, 2.0]));
        // integer and boolean arrays keep their kind; a 1-D array is one column
        let ints = Array::new("<i2", vec![3], [-1i16, 0, 7].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
        assert_eq!(array_table(&ints).unwrap(), json!({"columns": [{"name": "c1", "type": "INT", "values": [-1, 0, 7]}]}));
        let bools = Array::new("|b1", vec![2, 1], vec![1u8, 0]);
        assert_eq!(array_table(&bools).unwrap()["columns"][0]["values"], json!([true, false]));
        let picture = Array::from_f32(vec![2, 2, 3], &[0.0; 12]);
        assert!(array_table(&picture).is_err());
        assert!(table_array(&json!({"columns": [{"name": "s", "type": "STRING", "values": ["a"]}]})).is_err());
    }

    #[test]
    fn json_text_round_trips() {
        let v = json!({"b": [1, 2.5, "é"], "a": {"z": null, "y": true}});
        let text = json_text(Some(&Data::Json(v.clone()))).unwrap();
        assert_eq!(text, "{\n  \"a\": {\n    \"y\": true,\n    \"z\": null\n  },\n  \"b\": [\n    1,\n    2.5,\n    \"é\"\n  ]\n}");
        assert_eq!(parse_json(&text).unwrap(), v);
        assert!(parse_json("NaN").is_err());
        assert!(parse_json("{oops").is_err());
    }
}
