//! TABLE values as columns a viewer pages through, made on a worker (N4 slice 5): the core type's
//! columnar JSON (`{"columns": [{name, type, values}]}`, `docs/interface.md` section 3) read without
//! building a JSON tree of the whole (a million rows stay a few bytes a cell), rows (a list of
//! lists, Table to Rows'), and a sort order (a permutation) made beside it. Hostile input: every
//! size is checked, nothing panics, and a malformed value is an error naming what is wrong.

use std::cmp::Ordering;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

/// The most bytes of a table read.
pub const MAX_TABLE_BYTES: usize = 512 << 20;
/// The most columns.
pub const MAX_COLUMNS: usize = 4096;
/// The most rows.
pub const MAX_ROWS: usize = 50_000_000;
/// The longest text cell kept whole (longer is cut, with an ellipsis).
pub const MAX_CELL_CHARS: usize = 4096;

/// One cell.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Bool(bool),
    Int(i64),
    Num(f64),
    Text(Box<str>),
    /// An object or list, as compact JSON.
    Json(Box<str>),
}

impl Cell {
    fn rank(&self) -> u8 {
        match self {
            Cell::Null => 4,
            Cell::Bool(_) => 0,
            Cell::Int(_) | Cell::Num(_) => 1,
            Cell::Text(_) => 2,
            Cell::Json(_) => 3,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Cell::Int(i) => Some(*i as f64),
            Cell::Num(f) => Some(*f),
            _ => None,
        }
    }

    /// Ordering for a sort: numbers by value (NaN last), text by its characters, nulls last.
    pub fn order(&self, other: &Cell) -> Ordering {
        match (self, other) {
            (Cell::Int(a), Cell::Int(b)) => a.cmp(b),
            (Cell::Bool(a), Cell::Bool(b)) => a.cmp(b),
            (Cell::Text(a), Cell::Text(b)) | (Cell::Json(a), Cell::Json(b)) => a.cmp(b),
            _ => match (self.num(), other.num()) {
                (Some(a), Some(b)) => a.total_cmp(&b),
                _ => self.rank().cmp(&other.rank()),
            },
        }
    }

    /// The cell as a JSON value (numbers are the core's to format).
    pub fn json(&self) -> Value {
        match self {
            Cell::Null => Value::Null,
            Cell::Bool(b) => Value::Bool(*b),
            Cell::Int(i) => Value::from(*i),
            Cell::Num(f) => serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number),
            Cell::Text(t) | Cell::Json(t) => Value::String(t.to_string()),
        }
    }

    fn bytes(&self) -> usize {
        24 + match self {
            Cell::Text(t) | Cell::Json(t) => t.len(),
            _ => 0,
        }
    }

    fn of(v: Value) -> Cell {
        match v {
            Value::Null => Cell::Null,
            Value::Bool(b) => Cell::Bool(b),
            Value::Number(n) => n.as_i64().map_or_else(|| Cell::Num(n.as_f64().unwrap_or(f64::NAN)), Cell::Int),
            Value::String(s) => Cell::Text(cut(s).into()),
            other => Cell::Json(cut(other.to_string()).into()),
        }
    }
}

fn cut(mut s: String) -> String {
    if s.len() > MAX_CELL_CHARS {
        let mut end = MAX_CELL_CHARS;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push('…');
    }
    s
}

/// One column.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub name: String,
    /// Its type as the value names it (`FLOAT`, `INT`, `STRING`, …), or as read (`number`, `text`).
    pub ty: String,
    pub cells: Vec<Cell>,
}

impl Column {
    /// Whether its cells are numbers (and nulls): right-aligned, sorted by value.
    pub fn numeric(&self) -> bool {
        self.cells.iter().any(|c| c.num().is_some()) && self.cells.iter().all(|c| matches!(c, Cell::Int(_) | Cell::Num(_) | Cell::Null))
    }
}

/// A table, column by column; every column is `rows` long (shorter ones padded with nulls).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub columns: Vec<Column>,
    pub rows: usize,
}

impl Table {
    /// Bytes held, as a cache counts them.
    pub fn bytes(&self) -> usize {
        self.columns.iter().map(|c| c.name.len() + c.cells.iter().map(Cell::bytes).sum::<usize>()).sum::<usize>() + 64
    }

    /// The core type's columnar JSON, or rows (a list of lists), from bytes.
    /// A CSV's bytes (UTF-8, at most [`MAX_TABLE_BYTES`]) as a table, read by the reader the
    /// text engine uses (`plumcraft_proto::csv`, note 26 slice 4).
    pub fn from_csv(bytes: &[u8]) -> Result<Table, String> {
        if bytes.len() > MAX_TABLE_BYTES {
            return Err(format!("{} MiB is more than a table viewer reads ({} MiB)", bytes.len() >> 20, MAX_TABLE_BYTES >> 20));
        }
        let text = std::str::from_utf8(bytes).map_err(|e| format!("not a CSV: not UTF-8 text ({e})"))?;
        let v = plumcraft_proto::csv::read_csv(text)?;
        Table::from_value(&v)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Table, String> {
        if bytes.len() > MAX_TABLE_BYTES {
            return Err(format!("{} MiB is more than a table viewer reads ({} MiB)", bytes.len() >> 20, MAX_TABLE_BYTES >> 20));
        }
        let first = bytes.iter().find(|b| !b.is_ascii_whitespace()).copied();
        if first == Some(b'[') {
            let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("not a table: {e}"))?;
            return Table::from_value(&v);
        }
        let mut de = serde_json::Deserializer::from_slice(bytes);
        let t = TableSeed.deserialize(&mut de).map_err(|e| format!("not a table: {e}"))?;
        de.end().map_err(|e| format!("not a table: {e}"))?;
        t.finish()
    }

    /// A numeric array as a table (`Form::Array`, flagged a table or not: FR6, note 30): a row to
    /// a line, its last axis the columns, every axis before it flattened into rows (`(rows,
    /// columns)`; a 1-d array is one column; an (H, W, C) picture is H×W rows of C), named by
    /// `names` when there is one a column. Integers stay integers. Every size is checked before a
    /// byte is read.
    pub fn from_array(dtype: &str, shape: &[u64], names: &[String], bytes: &[u8]) -> Result<Table, String> {
        let (size, float, signed) = match dtype {
            "|u1" | "|b1" => (1, false, false),
            "|i1" => (1, false, true),
            "<u2" => (2, false, false),
            "<i2" => (2, false, true),
            "<u4" => (4, false, false),
            "<i4" => (4, false, true),
            "<u8" => (8, false, false),
            "<i8" => (8, false, true),
            "<f4" => (4, true, true),
            "<f8" => (8, true, true),
            other => return Err(format!("not a table: an array of {other:?} isn't numbers")),
        };
        let (lead, columns) = match shape {
            [] => return Err("not a table: an array with no shape".into()),
            [n] => (std::slice::from_ref(n), 1u64),
            [lead @ .., k] => (lead, *k),
        };
        let rows = lead.iter().try_fold(1u64, |a, n| a.checked_mul(*n)).ok_or("not a table: its shape overflows")?;
        let (rows, columns) =
            (usize::try_from(rows).map_err(|_| "not a table: its shape overflows")?, usize::try_from(columns).map_err(|_| "not a table: its shape overflows")?);
        if columns == 0 || columns > MAX_COLUMNS {
            return Err(format!("{columns} columns"));
        }
        if rows > MAX_ROWS {
            return Err(format!("{rows} rows"));
        }
        let need = rows.checked_mul(columns).and_then(|n| n.checked_mul(size)).ok_or("not a table: its shape overflows")?;
        if need > MAX_TABLE_BYTES {
            return Err(format!("{} MiB is more than a table viewer reads ({} MiB)", need >> 20, MAX_TABLE_BYTES >> 20));
        }
        let data = bytes.get(..need).ok_or_else(|| format!("not a table: {} bytes, where {rows}×{columns} of {dtype} is {need}", bytes.len()))?;
        let cell = |b: &[u8]| -> Cell {
            let mut a = [0u8; 8];
            if let Some(dst) = a.get_mut(..b.len()) {
                dst.copy_from_slice(b);
            }
            match (float, size) {
                (true, 4) => Cell::Num(f64::from(f32::from_le_bytes([a[0], a[1], a[2], a[3]]))),
                (true, _) => Cell::Num(f64::from_le_bytes(a)),
                (false, _) if !signed => i64::try_from(u64::from_le_bytes(a)).map_or_else(|_| Cell::Num(u64::from_le_bytes(a) as f64), Cell::Int),
                (false, 1) => Cell::Int(i64::from(a[0] as i8)),
                (false, 2) => Cell::Int(i64::from(i16::from_le_bytes([a[0], a[1]]))),
                (false, 4) => Cell::Int(i64::from(i32::from_le_bytes([a[0], a[1], a[2], a[3]]))),
                (false, _) => Cell::Int(i64::from_le_bytes(a)),
            }
        };
        let named = names.len() == columns;
        let mut out: Vec<Column> = (0..columns)
            .map(|j| Column {
                name: names.get(j).filter(|_| named).cloned().unwrap_or_else(|| format!("c{}", j + 1)),
                ty: if float { "FLOAT" } else { "INT" }.into(),
                cells: Vec::with_capacity(rows),
            })
            .collect();
        for row in data.chunks_exact(columns * size) {
            for (c, b) in out.iter_mut().zip(row.chunks_exact(size)) {
                c.cells.push(cell(b));
            }
        }
        Table { columns: out, rows: 0 }.finish()
    }

    /// The table a value reads as, by its form and media type (the one rule the core's viewers,
    /// its exports and the egui editor share): a numeric array (any rank, FR6), a value typed
    /// `text/csv`, or a TABLE's JSON (columnar, or rows).
    pub fn of(form: &plumcraft_proto::data::Form, media: &str, bytes: &[u8]) -> Result<Table, String> {
        match form {
            plumcraft_proto::data::Form::Array { dtype, shape, names, .. } => Table::from_array(dtype, shape, names, bytes),
            _ if crate::service::is_csv(media) => Table::from_csv(bytes),
            _ => Table::from_bytes(bytes),
        }
    }

    /// Whether a numeric array of this dtype and shape reads as a table within the reader's limits
    /// (its columns, rows and bytes), without reading it: what makes Table and Chart computable for
    /// it (FR6).
    pub fn fits_array(dtype: &str, shape: &[u64]) -> bool {
        let size: u64 = match dtype {
            "|u1" | "|b1" | "|i1" => 1,
            "<u2" | "<i2" => 2,
            "<u4" | "<i4" | "<f4" => 4,
            "<u8" | "<i8" | "<f8" => 8,
            _ => return false,
        };
        let (lead, columns) = match shape {
            [] => return false,
            [n] => (std::slice::from_ref(n), 1u64),
            [lead @ .., k] => (lead, *k),
        };
        let Some(rows) = lead.iter().try_fold(1u64, |a, n| a.checked_mul(*n)) else { return false };
        columns > 0
            && columns <= MAX_COLUMNS as u64
            && rows <= MAX_ROWS as u64
            && rows.checked_mul(columns).and_then(|n| n.checked_mul(size)).is_some_and(|b| b <= MAX_TABLE_BYTES as u64)
    }

    /// A value already parsed: columnar, or rows (the first row its names when it is all text and
    /// there is more than one).
    pub fn from_value(v: &Value) -> Result<Table, String> {
        if let Some(cols) = v.get("columns").and_then(Value::as_array) {
            if cols.len() > MAX_COLUMNS {
                return Err(format!("{} columns", cols.len()));
            }
            let mut t = Table::default();
            for (i, c) in cols.iter().enumerate() {
                let name = c.get("name").and_then(Value::as_str).map_or_else(|| format!("c{}", i + 1), str::to_owned);
                let ty = c.get("type").and_then(Value::as_str).unwrap_or_default().to_owned();
                let values = c.get("values").and_then(Value::as_array).ok_or_else(|| format!("column {name:?} has no list of values"))?;
                if values.len() > MAX_ROWS {
                    return Err(format!("{} rows", values.len()));
                }
                t.columns.push(Column { name, ty, cells: values.iter().cloned().map(Cell::of).collect() });
            }
            return t.finish();
        }
        let rows = v.as_array().filter(|r| !r.is_empty() && r.iter().all(Value::is_array)).ok_or("not a table: neither columns nor a list of rows")?;
        if rows.len() > MAX_ROWS {
            return Err(format!("{} rows", rows.len()));
        }
        let width = rows.iter().filter_map(Value::as_array).map(Vec::len).max().unwrap_or(0);
        if width > MAX_COLUMNS {
            return Err(format!("{width} columns"));
        }
        let header = rows.len() > 1 && rows.first().and_then(Value::as_array).is_some_and(|r| !r.is_empty() && r.iter().all(Value::is_string));
        let names: Vec<String> = (0..width)
            .map(|j| {
                if header {
                    rows.first().and_then(|r| r.get(j)).and_then(Value::as_str).map_or_else(|| format!("c{}", j + 1), str::to_owned)
                } else {
                    format!("c{}", j + 1)
                }
            })
            .collect();
        let mut columns: Vec<Column> = names.into_iter().map(|name| Column { name, ty: String::new(), cells: Vec::new() }).collect();
        for r in rows.iter().skip(usize::from(header)).filter_map(Value::as_array) {
            for (j, c) in columns.iter_mut().enumerate() {
                c.cells.push(r.get(j).cloned().map_or(Cell::Null, Cell::of));
            }
        }
        Table { columns, rows: 0 }.finish()
    }

    /// Pad short columns, name the types not named.
    fn finish(mut self) -> Result<Table, String> {
        let rows = self.columns.iter().map(|c| c.cells.len()).max().unwrap_or(0);
        if rows > MAX_ROWS {
            return Err(format!("{rows} rows"));
        }
        for c in &mut self.columns {
            c.cells.resize(rows, Cell::Null);
            if c.ty.is_empty() {
                c.ty = if c.numeric() { "number".into() } else { "text".into() };
            }
        }
        self.rows = rows;
        Ok(self)
    }

    /// The rows' order sorted by `column` (stable: equal cells keep their order), largest first
    /// when `desc`. A column it lacks: as it came.
    pub fn order(&self, column: usize, desc: bool) -> Vec<u32> {
        let n = u32::try_from(self.rows).unwrap_or(u32::MAX);
        let mut idx: Vec<u32> = (0..n).collect();
        let Some(c) = self.columns.get(column) else { return idx };
        idx.sort_by(|a, b| {
            let (x, y) = (c.cells.get(*a as usize).unwrap_or(&Cell::Null), c.cells.get(*b as usize).unwrap_or(&Cell::Null));
            // nulls last either way
            match (matches!(x, Cell::Null), matches!(y, Cell::Null)) {
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                _ if desc => y.order(x),
                _ => x.order(y),
            }
        });
        idx
    }
}

// ---------- columnar JSON, read without a tree of the whole ----------

struct TableSeed;

impl<'de> DeserializeSeed<'de> for TableSeed {
    type Value = Table;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Table, D::Error> {
        d.deserialize_map(TableVisitor)
    }
}

struct TableVisitor;

impl<'de> Visitor<'de> for TableVisitor {
    type Value = Table;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("{\"columns\": [...]}")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Table, A::Error> {
        let mut t = None;
        while let Some(k) = m.next_key::<String>()? {
            if k == "columns" {
                t = Some(m.next_value_seed(ColumnsSeed)?);
            } else {
                m.next_value::<de::IgnoredAny>()?;
            }
        }
        t.ok_or_else(|| de::Error::custom("no \"columns\""))
    }
}

struct ColumnsSeed;

impl<'de> DeserializeSeed<'de> for ColumnsSeed {
    type Value = Table;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Table, D::Error> {
        d.deserialize_seq(ColumnsVisitor)
    }
}

struct ColumnsVisitor;

impl<'de> Visitor<'de> for ColumnsVisitor {
    type Value = Table;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a list of columns")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Table, A::Error> {
        let mut t = Table::default();
        while let Some(c) = s.next_element_seed(ColumnSeed(t.columns.len()))? {
            if t.columns.len() >= MAX_COLUMNS {
                return Err(de::Error::custom(format!("more than {MAX_COLUMNS} columns")));
            }
            t.columns.push(c);
        }
        Ok(t)
    }
}

struct ColumnSeed(usize);

impl<'de> DeserializeSeed<'de> for ColumnSeed {
    type Value = Column;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Column, D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for ColumnSeed {
    type Value = Column;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a column {name, type, values}")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Column, A::Error> {
        let mut c = Column { name: format!("c{}", self.0 + 1), ty: String::new(), cells: Vec::new() };
        let mut seen = false;
        while let Some(k) = m.next_key::<String>()? {
            match k.as_str() {
                "name" => c.name = m.next_value::<Option<String>>()?.unwrap_or(c.name),
                "type" => c.ty = m.next_value::<Option<String>>()?.unwrap_or_default(),
                "values" => {
                    c.cells = m.next_value_seed(CellsSeed)?;
                    seen = true;
                }
                _ => {
                    m.next_value::<de::IgnoredAny>()?;
                }
            }
        }
        if !seen {
            return Err(de::Error::custom(format!("column {:?} has no list of values", c.name)));
        }
        Ok(c)
    }
}

struct CellsSeed;

impl<'de> DeserializeSeed<'de> for CellsSeed {
    type Value = Vec<Cell>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Vec<Cell>, D::Error> {
        d.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for CellsSeed {
    type Value = Vec<Cell>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a list of values")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Vec<Cell>, A::Error> {
        let mut out = Vec::with_capacity(s.size_hint().unwrap_or(0).min(1 << 20));
        while let Some(v) = s.next_element::<CellValue>()? {
            if out.len() >= MAX_ROWS {
                return Err(de::Error::custom(format!("more than {MAX_ROWS} rows")));
            }
            out.push(v.0);
        }
        Ok(out)
    }
}

/// A cell read straight from the JSON: scalars kept as they are, an object or list as text.
struct CellValue(Cell);

impl<'de> serde::Deserialize<'de> for CellValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<CellValue, D::Error> {
        d.deserialize_any(CellVisitor)
    }
}

struct CellVisitor;

impl<'de> Visitor<'de> for CellVisitor {
    type Value = CellValue;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a value")
    }
    fn visit_unit<E>(self) -> Result<CellValue, E> {
        Ok(CellValue(Cell::Null))
    }
    fn visit_none<E>(self) -> Result<CellValue, E> {
        Ok(CellValue(Cell::Null))
    }
    fn visit_bool<E>(self, v: bool) -> Result<CellValue, E> {
        Ok(CellValue(Cell::Bool(v)))
    }
    fn visit_i64<E>(self, v: i64) -> Result<CellValue, E> {
        Ok(CellValue(Cell::Int(v)))
    }
    fn visit_u64<E>(self, v: u64) -> Result<CellValue, E> {
        Ok(CellValue(i64::try_from(v).map_or(Cell::Num(v as f64), Cell::Int)))
    }
    fn visit_f64<E>(self, v: f64) -> Result<CellValue, E> {
        Ok(CellValue(Cell::Num(v)))
    }
    fn visit_str<E>(self, v: &str) -> Result<CellValue, E> {
        Ok(CellValue(Cell::Text(cut(v.to_owned()).into())))
    }
    fn visit_string<E>(self, v: String) -> Result<CellValue, E> {
        Ok(CellValue(Cell::Text(cut(v).into())))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, s: A) -> Result<CellValue, A::Error> {
        let v = Value::deserialize(de::value::SeqAccessDeserializer::new(s))?;
        Ok(CellValue(Cell::Json(cut(v.to_string()).into())))
    }
    fn visit_map<A: MapAccess<'de>>(self, m: A) -> Result<CellValue, A::Error> {
        let v = Value::deserialize(de::value::MapAccessDeserializer::new(m))?;
        Ok(CellValue(Cell::Json(cut(v.to_string()).into())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// B152: an engine's histogram is a (256, 2) float array marked as a table, not JSON.
    #[test]
    fn an_array_marked_as_a_table_reads_column_by_column() {
        let f: Vec<u8> = [0.5f32, 3.0, 1.5, 7.0, 2.5, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let names = vec!["centre".to_owned(), "count".to_owned()];
        let t = Table::from_array("<f4", &[3, 2], &names, &f).unwrap();
        assert_eq!((t.rows, t.columns.len()), (3, 2));
        assert_eq!((t.columns[0].name.as_str(), t.columns[1].name.as_str(), t.columns[1].ty.as_str()), ("centre", "count", "FLOAT"));
        assert_eq!(t.columns[1].cells, vec![Cell::Num(3.0), Cell::Num(7.0), Cell::Num(0.0)]);
        assert_eq!(t.order(1, true), vec![1, 0, 2]);
        // (rows, 1, columns), as a picture-shaped engine stores a table; names that don't fit are dropped
        let t = Table::from_array("<f4", &[3, 1, 2], &["x".to_owned()], &f).unwrap();
        assert_eq!((t.rows, t.columns[0].name.as_str(), t.columns[1].name.as_str()), (3, "c1", "c2"));
        let i: Vec<u8> = [-2i16, 5, 9].iter().flat_map(|v| v.to_le_bytes()).collect();
        let t = Table::from_array("<i2", &[3], &[], &i).unwrap();
        assert_eq!((t.columns.len(), t.columns[0].ty.as_str()), (1, "INT"));
        assert_eq!(t.columns[0].cells, vec![Cell::Int(-2), Cell::Int(5), Cell::Int(9)]);
        assert_eq!(Table::from_array("<u8", &[1], &[], &u64::MAX.to_le_bytes()).unwrap().columns[0].cells, vec![Cell::Num(u64::MAX as f64)]);
        assert_eq!(Table::from_array("<f8", &[0, 2], &[], &[]).unwrap().rows, 0);
    }

    #[test]
    fn a_hostile_array_is_refused_before_a_byte_is_read() {
        for (dtype, shape, bytes) in [
            ("<f4", vec![3, 2], vec![0u8; 23]),
            ("<f4", vec![u64::MAX, u64::MAX], vec![0u8; 8]),
            ("<f4", vec![u64::MAX, 2], vec![]),
            ("<f4", vec![2, 0], vec![]),
            ("<f4", vec![], vec![0u8; 4]),
            ("<f4", vec![1, (MAX_COLUMNS + 1) as u64], vec![]),
            ("<c8", vec![1, 1], vec![0u8; 8]),
            ("", vec![1], vec![0u8; 8]),
        ] {
            assert!(Table::from_array(dtype, &shape, &[], &bytes).is_err(), "{dtype} {shape:?}");
        }
    }

    #[test]
    fn columnar_and_rows_read_alike_and_sort() {
        let a = Table::from_bytes(
            br#"{"columns": [{"name": "n", "type": "INT", "values": [3, 1, null, 2]}, {"name": "s", "type": "STRING", "values": ["c", "a", "d", "b"]}]}"#,
        )
        .unwrap();
        assert_eq!((a.rows, a.columns.len()), (4, 2));
        assert!(a.columns[0].numeric());
        assert_eq!(a.order(0, false), vec![1, 3, 0, 2], "nulls last");
        assert_eq!(a.order(0, true), vec![0, 3, 1, 2], "nulls last either way");
        assert_eq!(a.order(1, false), vec![1, 3, 0, 2]);
        let r = Table::from_value(&json!([["n", "s"], [1, "a"], [2.5, {"x": 1}]])).unwrap();
        assert_eq!(r.columns[0].name, "n");
        assert_eq!(r.rows, 2);
        assert_eq!(r.columns[1].cells[1], Cell::Json(r#"{"x":1}"#.into()));
        let ragged = Table::from_bytes(br#"{"columns": [{"name": "a", "values": [1, 2, 3]}, {"values": []}]}"#).unwrap();
        assert_eq!((ragged.rows, ragged.columns[1].cells.len(), ragged.columns[1].name.as_str()), (3, 3, "c2"));
    }

    #[test]
    fn malformed_tables_are_errors() {
        for bad in [
            &b""[..],
            b"{",
            b"[]",
            b"[1, 2]",
            b"{\"columns\": 3}",
            b"{\"columns\": [{\"name\": \"a\"}]}",
            b"{\"columns\": [{\"values\": 7}]}",
            b"\xff\xfe",
            b"{\"columns\": []} trailing",
        ] {
            assert!(Table::from_bytes(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
        }
        assert!(Table::from_bytes(br#"{"columns": []}"#).unwrap().rows == 0);
    }
}
