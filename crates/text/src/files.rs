//! Files the craft apps export, as text and tables, and back (`docs/research/20-convert.md`,
//! 6.2): TXT, Markdown and HTML as Text, CSV as a Table. Each pair is a converter and its
//! inverse. The Python twin (`python/src/plumcraft_text/__init__.py`) has the same nodes and
//! the same rules, so the two engines stay the same engine to the UI.
//!
//! - **Text files** are UTF-8, read strictly (anything else is a `config` error), a leading
//!   byte-order mark dropped. The markup of Markdown and HTML is kept: these decode, they don't
//!   render.
//! - **CSV** is RFC 4180: commas, double quotes with `""` inside, lines ending `\n`, `\r\n` or
//!   `\r`. A file whose first line has no comma but a tab, or else a semicolon, is read with that
//!   as its separator (a TSV, a spreadsheet's CSV in a locale with decimal commas) (B16, #54);
//!   written, a table is always comma-separated. The first row names the columns (an empty name is `column<N>`; two the same is an
//!   error); blank lines are skipped; a short row is filled with nulls, and a long one is an
//!   error. A column is INT when every value is a whole number that fits, FLOAT when every value
//!   is a finite decimal number, BOOL when every value is `true` or `false` in any case (a
//!   spreadsheet writes `TRUE`), else STRING; an
//!   empty cell is null. Written back, a cell is quoted when it holds a comma, a quote, a line
//!   break, or space at either end.

use std::sync::Arc;

use plumcraft_engine::{Blob, BlobSource, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::Handle;
use plumcraft_proto::catalogue::{Media, NodeDef, Port, Socket, WireType};
use plumcraft_proto::convert::Converts;
use plumcraft_proto::data::{BlobMeta, Form};
use plumcraft_proto::error::NodeError;
use serde_json::{Value, json};

use crate::{TABLE, TEXT};

/// The file types; each port is named by its type in lower case.
pub const TXT: &str = "TXT";
pub const MD: &str = "MD";
pub const HTML: &str = "HTML";
pub const CSV: &str = "CSV";

/// The largest file read.
const MAX_FILE: u64 = 64 << 20;
/// The most rows a CSV makes.
const MAX_ROWS: usize = 1_000_000;

/// The types, as the craft packs declare them (bytes), so their exports cross into this engine.
pub fn wire_types() -> Vec<WireType> {
    [TXT, MD, HTML, CSV].iter().map(|t| WireType { id: (*t).into(), media: Media::Bytes, ..Default::default() }).collect()
}

fn media(ty: &str) -> &'static str {
    match ty {
        TXT => "text/plain",
        MD => "text/markdown",
        HTML => "text/html",
        _ => "text/csv",
    }
}

/// One direction of one pair.
struct Codec {
    id: &'static str,
    display: &'static str,
    description: &'static str,
    from: &'static str,
    to: &'static str,
    lossless: bool,
}

pub fn nodes() -> Vec<Box<dyn Node>> {
    let pair = |file: &'static str, from: &'static str, to: &'static str| -> [Codec; 2] {
        [
            Codec { id: from, display: "", description: "", from: file, to: TEXT, lossless: true },
            Codec { id: to, display: "", description: "", from: TEXT, to: file, lossless: true },
        ]
    };
    let [from_txt, to_txt] = pair(TXT, "text.from_txt", "text.to_txt");
    let [from_md, to_md] = pair(MD, "text.from_md", "text.to_md");
    let [from_html, to_html] = pair(HTML, "text.from_html", "text.to_html");
    let all = vec![
        Codec {
            display: "TXT to Text",
            description: "A TXT file (as WordCraft or PdfCraft exports one) as text: UTF-8, a byte-order mark dropped.",
            ..from_txt
        },
        Codec { display: "Text to TXT", description: "Text as a TXT file, UTF-8. The inverse of TXT to Text.", ..to_txt },
        Codec { display: "Markdown to Text", description: "A Markdown file as its text, the markup kept: UTF-8, a byte-order mark dropped.", ..from_md },
        Codec { display: "Text to Markdown", description: "Text as a Markdown file, UTF-8. The inverse of Markdown to Text.", ..to_md },
        Codec { display: "HTML to Text", description: "An HTML file as its source text, the markup kept: UTF-8, a byte-order mark dropped.", ..from_html },
        Codec { display: "Text to HTML", description: "Text (HTML source) as an HTML file, UTF-8. The inverse of HTML to Text.", ..to_html },
        Codec {
            id: "text.from_csv",
            display: "CSV to Table",
            description: "A CSV file (as GridCraft exports one) as a table: the first row names the columns, and each column's type comes from its values.",
            from: CSV,
            to: TABLE,
            lossless: false,
        },
        Codec {
            id: "text.to_csv",
            display: "Table to CSV",
            description: "A table as a CSV file, a header line first. The inverse of CSV to Table.",
            from: TABLE,
            to: CSV,
            lossless: false,
        },
    ];
    all.into_iter().map(|c| Box::new(c) as Box<dyn Node>).collect()
}

fn port(ty: &str) -> String {
    ty.to_ascii_lowercase()
}

impl Node for Codec {
    fn def(&self) -> NodeDef {
        let (input, output) = (port(self.from), port(self.to));
        NodeDef {
            id: self.id.into(),
            display: self.display.into(),
            category: vec!["Text".into(), "Files".into()],
            description: self.description.into(),
            inputs: vec![Port { name: input.clone(), ty: self.from.into(), socket: Some(Socket::Socket), ..Default::default() }],
            outputs: vec![Port { name: output.clone(), ty: self.to.into(), ..Default::default() }],
            family: "text".into(),
            converts: vec![Converts { input: Some(input), output: Some(output), ..Converts::new(self.from, self.to, self.lossless) }],
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
        let input = port(self.from);
        let got = inputs.get(&input);
        let out = match (self.from, self.to) {
            (TABLE, _) => {
                let table = match got {
                    Some(Data::Json(v)) => v,
                    _ => return Err(config(format!("{input}: wire a table"))),
                };
                file(CSV, write_csv(table)?.into_bytes())
            }
            (TEXT, file_ty) => {
                let text = match got {
                    Some(Data::Json(Value::String(s))) => s.clone(),
                    _ => return Err(config(format!("{input}: wire text"))),
                };
                file(file_ty, text.into_bytes())
            }
            (_, to) => {
                let blob = match got {
                    Some(Data::Blob(b)) => b,
                    _ => return Err(config(format!("{input}: wire a {} file", self.from))),
                };
                let text = utf8(blob.read(MAX_FILE)?, &input)?;
                if to == TABLE { Data::Json(read_csv(&text)?) } else { Data::Json(json!(text)) }
            }
        };
        Ok(Outputs::from([(port(self.to), out)]))
    }
}

fn config(msg: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::CONFIG, msg)
}

fn utf8(bytes: Vec<u8>, port: &str) -> Result<String, NodeError> {
    let text = String::from_utf8(bytes).map_err(|_| config(format!("{port}: not UTF-8 text")))?;
    Ok(text.strip_prefix('\u{feff}').map(str::to_owned).unwrap_or(text))
}

fn file(ty: &str, bytes: Vec<u8>) -> Data {
    Data::Blob(Box::new(Blob {
        handle: Handle::raw(&bytes),
        meta: BlobMeta { media: media(ty).into(), size: bytes.len() as u64, form: Form::default(), aliases: Vec::new() },
        source: BlobSource::Bytes(Arc::from(bytes)),
        publish: true,
        thumb: None,
    }))
}

/// The separator a CSV's first line uses, outside quotes: a comma when it has one, else a tab,
/// else a semicolon, else a comma (module docs).
pub fn separator(text: &str) -> char {
    let (mut quoting, mut tab, mut semi) = (false, false, false);
    for c in text.chars() {
        match c {
            '"' => quoting = !quoting,
            ',' if !quoting => return ',',
            '\t' if !quoting => tab = true,
            ';' if !quoting => semi = true,
            '\n' | '\r' if !quoting => break,
            _ => {}
        }
    }
    if tab {
        '\t'
    } else if semi {
        ';'
    } else {
        ','
    }
}

/// The rows of a CSV (module docs), its separator worked out from its first line.
pub fn csv_rows(text: &str) -> Result<Vec<Vec<String>>, NodeError> {
    let sep = separator(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let (mut row, mut field) = (Vec::new(), String::new());
    let (mut quoting, mut quoted) = (false, false);
    let mut chars = text.chars().peekable();
    let end_row = |row: &mut Vec<String>, field: &mut String, quoted: &mut bool, rows: &mut Vec<Vec<String>>| -> Result<(), NodeError> {
        row.push(std::mem::take(field));
        let line = std::mem::take(row);
        // a blank line
        if !(line.len() == 1 && line[0].is_empty() && !*quoted) {
            if rows.len() >= MAX_ROWS {
                return Err(config(format!("csv: more than {MAX_ROWS} rows")));
            }
            rows.push(line);
        }
        *quoted = false;
        Ok(())
    };
    while let Some(c) = chars.next() {
        if quoting {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoting = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !quoted => {
                quoting = true;
                quoted = true;
            }
            c if c == sep => {
                row.push(std::mem::take(&mut field));
                quoted = false;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                end_row(&mut row, &mut field, &mut quoted, &mut rows)?;
            }
            _ => field.push(c),
        }
    }
    if quoting {
        return Err(config("csv: a quoted field doesn't end"));
    }
    if !field.is_empty() || !row.is_empty() || quoted {
        end_row(&mut row, &mut field, &mut quoted, &mut rows)?;
    }
    Ok(rows)
}

fn is_int(s: &str) -> bool {
    let d = s.strip_prefix('-').unwrap_or(s);
    !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
}

fn is_decimal(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let (mantissa, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (whole, frac) = match mantissa.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (mantissa, None),
    };
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match frac {
        Some(f) => digits(whole) && digits(f) && !(whole.is_empty() && f.is_empty()),
        None => !whole.is_empty() && digits(whole),
    };
    let exp_ok = exp.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mantissa_ok && exp_ok
}

/// A column's type and values (module docs).
fn column(cells: &[Option<String>]) -> (&'static str, Vec<Value>) {
    let present: Vec<&str> = cells.iter().flatten().map(String::as_str).collect();
    let all = |f: &dyn Fn(&str) -> bool| !present.is_empty() && present.iter().all(|s| f(s));
    let each = |f: &dyn Fn(&str) -> Value| cells.iter().map(|c| c.as_deref().map_or(Value::Null, f)).collect();
    if all(&|s| is_int(s) && s.parse::<i64>().is_ok()) {
        return ("INT", each(&|s| s.parse::<i64>().map_or(Value::Null, Value::from)));
    }
    if all(&|s| is_decimal(s) && s.parse::<f64>().is_ok_and(f64::is_finite)) {
        return ("FLOAT", each(&|s| s.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map_or(Value::Null, Value::Number)));
    }
    // any case: a spreadsheet writes TRUE and FALSE
    if all(&|s| s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("false")) {
        return ("BOOL", each(&|s| Value::Bool(s.eq_ignore_ascii_case("true"))));
    }
    ("STRING", each(&|s| Value::String(s.to_owned())))
}

/// A CSV as a table (module docs).
pub fn read_csv(text: &str) -> Result<Value, NodeError> {
    let rows = csv_rows(text)?;
    let Some((header, body)) = rows.split_first() else { return Err(config("csv: no header row")) };
    let names: Vec<String> = header.iter().enumerate().map(|(i, n)| if n.is_empty() { format!("column{}", i + 1) } else { n.clone() }).collect();
    for (i, n) in names.iter().enumerate() {
        if names[..i].contains(n) {
            return Err(config(format!("csv: two columns are called {n:?}")));
        }
    }
    for (r, row) in body.iter().enumerate() {
        if row.len() > names.len() {
            return Err(config(format!("csv: row {} has {} fields; the header has {}", r + 2, row.len(), names.len())));
        }
    }
    let columns: Vec<Value> = names
        .iter()
        .enumerate()
        .map(|(c, name)| {
            let cells: Vec<Option<String>> = body.iter().map(|r| r.get(c).filter(|s| !s.is_empty()).cloned()).collect();
            let (ty, values) = column(&cells);
            json!({"name": name, "type": ty, "values": values})
        })
        .collect();
    Ok(json!({"columns": columns}))
}

fn cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

fn quote(s: &str) -> String {
    let needs = s.contains([',', '"', '\n', '\r']) || s.starts_with(' ') || s.ends_with(' ');
    if needs { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_owned() }
}

/// A table as CSV (module docs).
pub fn write_csv(table: &Value) -> Result<String, NodeError> {
    let columns = table.get("columns").and_then(Value::as_array).ok_or_else(|| config("table: not a table (no columns)"))?;
    let cols: Vec<(&str, &Vec<Value>)> = columns
        .iter()
        .map(|c| {
            let name = c.get("name").and_then(Value::as_str).unwrap_or("");
            c.get("values").and_then(Value::as_array).map(|v| (name, v)).ok_or_else(|| config("table: a column without values"))
        })
        .collect::<Result<_, _>>()?;
    let n = cols.first().map_or(0, |c| c.1.len());
    if cols.iter().any(|c| c.1.len() != n) {
        return Err(config("table: its columns differ in length"));
    }
    let mut out = String::new();
    let line = |cells: Vec<String>| cells.iter().map(|c| quote(c)).collect::<Vec<_>>().join(",") + "\n";
    out.push_str(&line(cols.iter().map(|c| c.0.to_owned()).collect()));
    for r in 0..n {
        out.push_str(&line(cols.iter().map(|c| c.1.get(r).map_or_else(String::new, cell)).collect()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_reads_quotes_line_ends_and_types() {
        let t = read_csv("name,n,x,ok,note\r\nada,1,1.5,true,\"a, b\"\nbob,-2,2,false,\"say \"\"hi\"\"\"\n\n,3,,true,\"two\nlines\"").unwrap();
        let col = |i: usize| (t["columns"][i]["type"].as_str().unwrap().to_owned(), t["columns"][i]["values"].clone());
        assert_eq!(col(0), ("STRING".into(), json!(["ada", "bob", null])));
        assert_eq!(col(1), ("INT".into(), json!([1, -2, 3])));
        assert_eq!(col(2), ("FLOAT".into(), json!([1.5, 2.0, null])));
        assert_eq!(col(3), ("BOOL".into(), json!([true, false, true])));
        assert_eq!(col(4).1, json!(["a, b", "say \"hi\"", "two\nlines"]));
        // and back, then again: the same table
        let csv = write_csv(&t).unwrap();
        assert_eq!(read_csv(&csv).unwrap(), t, "{csv}");
    }

    #[test]
    fn csv_refuses_what_it_cant_read() {
        assert!(read_csv("").is_err(), "no header");
        assert!(read_csv("a,a\n1,2\n").is_err(), "two the same");
        assert!(read_csv("a,b\n1,2,3\n").is_err(), "a long row");
        assert!(read_csv("a\n\"open\n").is_err(), "an open quote");
        let t = read_csv("a,,b\n1\n").unwrap();
        assert_eq!(t["columns"][1]["name"], "column2");
        assert_eq!(t["columns"][2]["values"], json!([null]), "a short row is filled");
        // numbers that aren't plain stay strings; one too big for INT is a FLOAT
        let t = read_csv("a,b,c\n1_000,inf,99999999999999999999\n").unwrap();
        assert_eq!(
            (t["columns"][0]["type"].as_str(), t["columns"][1]["type"].as_str(), t["columns"][2]["type"].as_str()),
            (Some("STRING"), Some("STRING"), Some("FLOAT"))
        );
        // as GridCraft writes them
        assert_eq!(read_csv("ok\nTRUE\nFalse\n").unwrap()["columns"][0]["values"], json!([true, false]));
    }

    #[test]
    fn decimals_are_plain() {
        for ok in ["1", "-1", "1.", ".5", "1e5", "1.5E-3", "-0.0"] {
            assert!(is_decimal(ok), "{ok}");
        }
        for bad in ["", ".", "e5", "1e", "1.2.3", "+1", "nan", " 1"] {
            assert!(!is_decimal(bad), "{bad}");
        }
    }
}
