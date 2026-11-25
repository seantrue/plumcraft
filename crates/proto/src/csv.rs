//! CSV read as a table (moved from the text engine, note 26 slice 4, so the engine's Text to
//! Table and the editor's viewers read CSV the same way): the separator worked out from the first
//! line outside quotes (a comma, else a tab, else a semicolon), quoted fields with doubled
//! quotes, CRLF or LF line ends, blank lines skipped; the first row names the columns (an empty
//! name is `columnN`; two the same are refused), and each column is typed INT, FLOAT, BOOL or
//! STRING by its present cells, empty cells null. Hostile input: at most [`MAX_ROWS`] rows and
//! [`MAX_COLUMNS`] fields a row; an error names what is wrong; nothing panics. The caller limits
//! the text's size.

use serde_json::{Value, json};

/// The most rows a CSV makes.
pub const MAX_ROWS: usize = 1_000_000;
/// The most fields in one row.
pub const MAX_COLUMNS: usize = 4096;

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
pub fn csv_rows(text: &str) -> Result<Vec<Vec<String>>, String> {
    let sep = separator(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let (mut row, mut field) = (Vec::new(), String::new());
    let (mut quoting, mut quoted) = (false, false);
    let mut chars = text.chars().peekable();
    let end_row = |row: &mut Vec<String>, field: &mut String, quoted: &mut bool, rows: &mut Vec<Vec<String>>| -> Result<(), String> {
        row.push(std::mem::take(field));
        let line = std::mem::take(row);
        if line.len() > MAX_COLUMNS {
            return Err(format!("csv: a row of more than {MAX_COLUMNS} fields"));
        }
        // a blank line
        if !(line.len() == 1 && line[0].is_empty() && !*quoted) {
            if rows.len() >= MAX_ROWS {
                return Err(format!("csv: more than {MAX_ROWS} rows"));
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
                if row.len() >= MAX_COLUMNS {
                    return Err(format!("csv: a row of more than {MAX_COLUMNS} fields"));
                }
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
        return Err("csv: a quoted field doesn't end".into());
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

/// Whether `s` is a decimal number as a CSV cell writes one (`-1.5`, `2e10`, `.5`).
pub fn is_decimal(s: &str) -> bool {
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
pub fn read_csv(text: &str) -> Result<Value, String> {
    let rows = csv_rows(text)?;
    let Some((header, body)) = rows.split_first() else { return Err("csv: no header row".into()) };
    let names: Vec<String> = header.iter().enumerate().map(|(i, n)| if n.is_empty() { format!("column{}", i + 1) } else { n.clone() }).collect();
    for (i, n) in names.iter().enumerate() {
        if names[..i].contains(n) {
            return Err(format!("csv: two columns are called {n:?}"));
        }
    }
    for (r, row) in body.iter().enumerate() {
        if row.len() > names.len() {
            return Err(format!("csv: row {} has {} fields; the header has {}", r + 2, row.len(), names.len()));
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
