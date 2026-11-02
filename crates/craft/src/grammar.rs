//! The `params` prose grammar (note 14, 1.4 and 2.4): a port of VectorCraft's own parser
//! (`crates/engine/src/cmd/params.rs`, `doc_keys` and `accepted`), so the keys found here are the
//! keys the craft accepts, plus a heuristic reading of each key's text into a port type.
//!
//! A description starts with the params object: `{x, width?: pt, name?|title?: …, sides=6, …}`,
//! or with objects taken instead of one another (`{path} or {name, dataBase64}`). `?` marks an
//! optional key, `|` alternatives, and `:`, `=` or `(` starts the prose. An entry starting with
//! `…` stands for another command's keys (`…as imageTrace.make`), or, naming none, any key.

use serde_json::Value;

/// The keys a params description names.
#[derive(Debug, Default, PartialEq)]
pub struct ParamKeys<'a> {
    /// `(key, optional, the entry's text after the key)`.
    pub keys: Vec<(&'a str, bool, &'a str)>,
    /// `…` entries, without the `…`.
    pub more: Vec<&'a str>,
}

impl ParamKeys<'_> {
    pub fn names(&self) -> Vec<&str> {
        self.keys.iter().map(|k| k.0).collect()
    }
}

/// The body of the object `doc` starts with, and what follows it.
fn object_body(doc: &str) -> Result<(&str, &str), String> {
    if !doc.starts_with('{') {
        return Err("the description doesn't start with the params object".into());
    }
    let (mut depth, mut quote) = (0usize, None);
    for (i, c) in doc.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '`') => quote = Some(c),
            (None, '{' | '[' | '(') => depth += 1,
            (None, '}' | ']' | ')') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok((doc.get(1..i).unwrap_or_default(), doc.get(i + 1..).unwrap_or_default()));
                }
            }
            _ => {}
        }
    }
    Err("the params object isn't closed".into())
}

/// `s` split at each `sep` outside brackets, quotes and backticks.
pub fn split_top(s: &str, sep: char) -> Vec<&str> {
    let (mut out, mut depth, mut quote, mut start) = (Vec::new(), 0usize, None, 0);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '`') => quote = Some(c),
            (None, '{' | '[' | '(') => depth += 1,
            (None, '}' | ']' | ')') => depth = depth.saturating_sub(1),
            (None, _) if c == sep && depth == 0 => {
                out.push(s.get(start..i).unwrap_or_default());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(s.get(start..).unwrap_or_default());
    out
}

fn is_key(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The keys a description names, or what doesn't read as one.
pub fn doc_keys(doc: &str) -> Result<ParamKeys<'_>, String> {
    let mut out = ParamKeys::default();
    let (mut body, mut rest) = object_body(doc)?;
    let mut entries = split_top(body, ',');
    while let Some(next) = rest.trim_start().strip_prefix("or ").or_else(|| rest.trim_start().strip_prefix("| ")).filter(|r| r.starts_with('{')) {
        (body, rest) = object_body(next)?;
        entries.extend(split_top(body, ','));
    }
    for entry in entries.into_iter().map(str::trim).filter(|e| !e.is_empty()) {
        if let Some(more) = entry.strip_prefix('…') {
            out.more.push(more.trim());
            continue;
        }
        let mut noted = false;
        // The text the whole entry says, shared by `a?|b?: text` alternatives.
        let alternatives = split_top(entry, '|');
        let shared = alternatives.last().map_or("", |a| {
            let head = a.split([':', '=', '(']).next().unwrap_or_default();
            a.get(head.len()..).unwrap_or_default()
        });
        for alternative in &alternatives {
            let head = alternative.split([':', '=', '(']).next().unwrap_or_default();
            let after = alternative.get(head.len()..).unwrap_or_default();
            let optional = head.trim_end().ends_with('?');
            let name = head.trim().trim_end_matches('?').trim_end();
            if !noted {
                if !is_key(name) {
                    let start: String = entry.chars().take(60).collect();
                    return Err(format!("`{start}` doesn't start with a parameter name"));
                }
                // `a: x|y`: the text is the whole entry after the key, alternatives included.
                let text = entry.get(entry.find(head).map_or(0, |i| i + head.len())..).unwrap_or_default();
                out.keys.push((name, optional, if after.is_empty() { shared } else { text }));
                noted = !after.is_empty();
            } else if is_key(name) && (head.trim_end().ends_with('?') || after.starts_with(':')) {
                out.keys.push((name, optional, after));
            }
        }
    }
    Ok(out)
}

/// The commands a `…` entry names after `as` or `of`.
fn named_commands<'a>(more: &'a str, exists: &dyn Fn(&str) -> bool) -> Vec<&'a str> {
    let words: Vec<&str> = more.split_whitespace().collect();
    (1..words.len())
        .filter_map(|i| {
            let w = words.get(i)?.trim_matches(|c: char| !c.is_ascii_alphanumeric());
            (matches!(words.get(i - 1), Some(&"as" | &"of")) && exists(w)).then_some(w)
        })
        .collect()
}

/// Every key a command accepts, its `…` imports included; `None`: any key (a `…` that names no
/// command, as the export options do, or a description that doesn't parse).
pub fn accepted(doc: &str, lookup: &dyn Fn(&str) -> Option<String>, depth: usize) -> Option<Vec<String>> {
    let parsed = doc_keys(doc).ok()?;
    let mut keys: Vec<String> = parsed.names().into_iter().map(str::to_owned).collect();
    let exists = |id: &str| lookup(id).is_some();
    for more in parsed.more {
        let named = named_commands(more, &exists);
        if named.is_empty() || depth > 4 {
            return None;
        }
        for id in named {
            keys.extend(accepted(&lookup(id)?, lookup, depth + 1)?);
        }
    }
    Some(keys)
}

/// A key's port, read from its text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Inferred {
    /// `BOOL`, `INT`, `FLOAT`, `ENUM`, `STRING` or `JSON`; empty when nothing says.
    pub ty: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub choices: Vec<String>,
    pub default: Value,
}

const UNITS: &[&str] = &["pt", "deg", "degrees", "%", "px", "ppi", "lpi", "mm", "in"];

/// Read a key's text (`: pt (1)`, `=10`, `: "a"|"b" (a)`, `: bool`, `: [x, y]`) as a port.
pub fn infer(text: &str) -> Inferred {
    let t = text.trim_start_matches(['?', ' ']);
    let mut out = Inferred::default();
    if let Some(v) = t.strip_prefix('=') {
        let v = v.split([' ', '|', ',']).next().unwrap_or_default();
        out.default = scalar(v);
    }
    let body = t.trim_start_matches([':', '=', ' ']);
    let lower = body.to_ascii_lowercase();
    let first_word = lower.split(|c: char| !c.is_ascii_alphanumeric()).find(|w| !w.is_empty()).unwrap_or_default();
    // Alternatives: `"a"|"b"` or `a|b|c` at the start; a middle one may carry a note in
    // parentheses (`"exact" (default)|"hueShift"`).
    let pieces = split_top(body, '|');
    let token = |p: &str| p.trim().split([' ', '(', ';', ',']).next().unwrap_or_default().trim_matches('"').to_owned();
    let bare = |p: &str| {
        let mut depth = 0usize;
        p.chars()
            .filter(|c| {
                match c {
                    '(' => depth += 1,
                    ')' => depth = depth.saturating_sub(1),
                    _ => return depth == 0,
                }
                false
            })
            .collect::<String>()
            .trim()
            .trim_matches('"')
            .to_owned()
    };
    let plain = |a: &str| a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
    let n = pieces.len();
    let alts: Vec<String> =
        pieces.iter().enumerate().take_while(|(i, p)| *i + 1 == n || bare(p) == token(p)).map(|(_, p)| token(p)).filter(|a| !a.is_empty()).collect();
    if alts.len() >= 2 && alts.iter().all(|a| plain(a)) && !alts.iter().all(|a| a.parse::<f64>().is_ok()) {
        out.ty = "ENUM".into();
        out.choices = alts.clone();
        if let Some(d) = paren_default(body).filter(|d| out.choices.iter().any(|c| c == d.as_str().unwrap_or_default())) {
            out.default = d;
        } else if let Some(i) = body.find("(default)") {
            // `"exact" (default)|"preserveTints"`: the alternative before the marker.
            let before = body.get(..i).unwrap_or_default();
            if let Some(d) = split_top(before, '|').last().map(|a| a.trim().trim_matches('"')) {
                out.default = Value::from(d);
            }
        }
        return out;
    }
    if first_word == "bool" || first_word == "true" || first_word == "false" {
        out.ty = "BOOL".into();
        if out.default.is_null() {
            out.default = match first_word {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => paren_default(body).filter(Value::is_boolean).unwrap_or(Value::Null),
            };
        }
        return out;
    }
    if body.starts_with('[') || body.starts_with('{') {
        out.ty = "JSON".into();
        return out;
    }
    if lower.starts_with('#') || first_word == "colour" || first_word == "color" {
        out.ty = "STRING".into();
        return out;
    }
    let range = number_range(body);
    let unit = UNITS.iter().any(|u| first_word == *u || body.starts_with(u));
    if range.is_some() || unit || first_word == "n" || first_word == "integer" || first_word == "number" {
        let int = first_word == "integer" || first_word == "n" || range.is_some_and(|(a, b, int)| int && a.fract() == 0.0 && b.fract() == 0.0 && !unit);
        out.ty = if int { "INT" } else { "FLOAT" }.into();
        if let Some((a, b, _)) = range {
            out.min = Some(a);
            out.max = Some(b);
        }
        if out.default.is_null() {
            out.default = paren_default(body).filter(Value::is_number).unwrap_or(Value::Null);
        }
        return out;
    }
    if !out.default.is_null() {
        out.ty = match &out.default {
            Value::Bool(_) => "BOOL",
            Value::Number(n) if n.is_i64() => "INT",
            Value::Number(_) => "FLOAT",
            _ => "STRING",
        }
        .into();
    }
    out
}

/// A scalar written in prose: a number, `true`/`false`, `null`, or a quoted string.
fn scalar(s: &str) -> Value {
    let s = s.trim().trim_end_matches([')', ';', '.']);
    if let Ok(i) = s.parse::<i64>() {
        return Value::from(i);
    }
    if let Ok(f) = s.parse::<f64>()
        && f.is_finite()
    {
        return Value::from(f);
    }
    match s {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "" | "null" => Value::Null,
        _ => Value::from(s.trim_matches(['"', '\''])),
    }
}

/// The default in the first parenthesis: `(1)`, `(0..360, 360)`, `(default 3)`, `(circle)`.
fn paren_default(body: &str) -> Option<Value> {
    let open = body.find('(')?;
    let inner = body.get(open + 1..)?;
    let inner = inner.get(..inner.find(')')?)?;
    let last = inner.rsplit(',').next()?.trim().trim_start_matches("default").trim();
    if last.contains("..") || last.contains(' ') {
        return None;
    }
    let v = scalar(last);
    (!v.is_null()).then_some(v)
}

/// `a..b`, `a-b` or `a–b` near the start: (min, max, both integers).
fn number_range(body: &str) -> Option<(f64, f64, bool)> {
    let window: String = body.chars().take(40).collect();
    for sep in ["..", "–", "-"] {
        for (i, _) in window.match_indices(sep) {
            let left = window.get(..i)?;
            let right = window.get(i + sep.len()..)?;
            let a: String = left.chars().rev().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect::<Vec<_>>().into_iter().rev().collect();
            let b: String = right.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            let (Ok(x), Ok(y)) = (a.trim_start_matches('-').parse::<f64>(), b.parse::<f64>()) else { continue };
            let x = if a.starts_with('-') && sep != "-" { -x } else { x };
            if x <= y && x.is_finite() && y.is_finite() {
                return Some((x, y, !a.contains('.') && !b.contains('.')));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_as_vectorcraft_reads_them() {
        let k = doc_keys("{x, width?: pt, name?|title?: document name, sides=6, …settings as file.new} → {id}").unwrap();
        assert_eq!(k.names(), ["x", "width", "name", "title", "sides"]);
        assert_eq!(k.more, ["settings as file.new"]);
        let k = doc_keys("{path} or {name, dataBase64}").unwrap();
        assert_eq!(k.names(), ["path", "name", "dataBase64"]);
        let k = doc_keys("{mode?: \"a\"|\"b\" (a), level?: 0..100}").unwrap();
        assert_eq!(k.names(), ["mode", "level"]);
        assert!(doc_keys("no object").is_err());
        assert!(doc_keys("{unclosed").is_err());
        assert!(doc_keys("{3x: nope}").is_err());
    }

    #[test]
    fn imports_resolve_through_the_catalogue() {
        let lookup = |id: &str| match id {
            "imageTrace.make" => Some("{id?, preset?: name, params?: {…}}".to_owned()),
            _ => None,
        };
        assert_eq!(accepted("{…as imageTrace.make} then expand", &lookup, 0).unwrap(), ["id", "preset", "params"]);
        assert_eq!(accepted("{format?, …the format's options}", &lookup, 0), None, "a … naming no command takes any key");
    }

    #[test]
    fn types_from_prose() {
        let t = |s: &str| infer(s);
        assert_eq!(t(": bool").ty, "BOOL");
        assert_eq!(t(": true").default, Value::Bool(true));
        let lvl = t(": 0..100 (the Simplify slider)");
        assert_eq!((lvl.ty.as_str(), lvl.min, lvl.max), ("INT", Some(0.0), Some(100.0)));
        let tol = t(": pt (1)");
        assert_eq!((tol.ty.as_str(), tol.default.clone()), ("FLOAT", Value::from(1)));
        let m = t(": \"exact\" (default)|\"preserveTints\"|\"hueShift\" (how)");
        assert_eq!(m.ty, "ENUM");
        assert_eq!(m.choices, ["exact", "preserveTints", "hueShift"]);
        assert_eq!(m.default, Value::from("exact"));
        let shape = t(": \"circle\"|\"square\" (circle)");
        assert_eq!(shape.default, Value::from("circle"));
        assert_eq!(t("=10 (whole numbers)").default, Value::from(10));
        assert_eq!(t(": [x,y]").ty, "JSON");
        assert_eq!(t(": #rrggbb (white)").ty, "STRING");
        let angle = t(": degrees (0..360, 360)");
        assert_eq!((angle.ty.as_str(), angle.min, angle.max, angle.default.clone()), ("FLOAT", Some(0.0), Some(360.0), Value::from(360)));
        assert_eq!(t(": left|right").choices, ["left", "right"]);
        assert_eq!(t("").ty, "");
    }

    #[test]
    fn hostile_descriptions_never_panic() {
        for s in ["{", "}", "{…}", "{a?|", "{a: (", "{a: \"", "{a: `x", "{é: 1}", "{a: 1..}", "{a: -..-}", "{a=}", "{a: ((((}", "{,,,}"] {
            let _ = doc_keys(s);
            let _ = accepted(s, &|_| None, 0);
            let _ = infer(s);
        }
        for s in ["=", ": -", ": 1..", ": ..5", ": 5..1", ": 1e999..2", ": (", ": (default)", ": \"\"|\"\""] {
            let _ = infer(s);
        }
    }
}
