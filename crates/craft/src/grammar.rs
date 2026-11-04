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
    /// Prose was skipped that may name more keys (SolidCraft's dialect): the list may be short.
    pub open: bool,
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

/// The keys a description names, or what doesn't read as one. Three dialects:
///
/// - VectorCraft's (and most crafts'): `{x, width?: pt, name?|title?: …}`;
/// - quoted keys (FilmCraft, PhotoCraft, WordCraft): `{"name":str, "fps":f64=23.976,
///   "mix":"Stereo|Mono"?, "template"?: "blank|letter", "item":id|"items":[id]}`, where `?`
///   after the key or after its type marks it optional and `=` after the type gives the default;
/// - prose without the braces (SolidCraft): `length, width, height: expr; corner?: [x,y,z] |
///   center?: [x,y,z] (…); operation?, name?`, entries split at `,` and `;`, the reply after
///   `→` or `Returns` left out, and an entry that doesn't start with a key read as the previous
///   one's prose.
///
/// What follows the params object (`{…} → {reply}`), or the reply in prose, is never a key.
pub fn doc_keys(doc: &str) -> Result<ParamKeys<'_>, String> {
    let doc = doc.trim_start();
    if !doc.starts_with('{') {
        return prose_keys(doc);
    }
    let mut out = ParamKeys::default();
    let (mut body, mut rest) = object_body(doc)?;
    let mut entries = split_top(body, ',');
    while let Some(next) = rest.trim_start().strip_prefix("or ").or_else(|| rest.trim_start().strip_prefix("| ")).filter(|r| r.trim_start().starts_with('{')) {
        (body, rest) = object_body(next.trim_start())?;
        entries.extend(split_top(body, ','));
    }
    for entry in entries.into_iter().map(str::trim).filter(|e| !e.is_empty()) {
        if let Some(more) = entry.strip_prefix('…') {
            out.more.push(more.trim());
            continue;
        }
        if entry.starts_with('"') {
            quoted_entry(entry, &mut out)?;
        } else {
            entry_keys(entry, &mut out)?;
        }
    }
    Ok(out)
}

/// One `a?|b?: text` entry (VectorCraft's dialect, and SolidCraft's prose entries).
fn entry_keys<'a>(entry: &'a str, out: &mut ParamKeys<'a>) -> Result<(), String> {
    let mut noted = false;
    // The text the whole entry says, shared by `a?|b?: text` alternatives.
    let alternatives = split_top(entry, '|');
    let shared = alternatives.last().map_or("", |a| {
        let head = a.split([':', '=', '(']).next().unwrap_or_default();
        a.get(head.len()..).unwrap_or_default()
    });
    for alternative in &alternatives {
        let head = alternative.split([':', '=', '(']).next().unwrap_or_default();
        // `position? [x,y,z]`, `focalLength? mm`: the key ends at its `?`, its unit or shape follows
        let head = match head.find('?') {
            Some(i) if head.get(i + 1..).is_some_and(|r| r.starts_with([' ', '['])) => head.get(..=i).unwrap_or(head),
            _ => head,
        };
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
    Ok(())
}

/// A quoted-key entry: `"key"?: text`, `"key":type?=default`, `"a":x|"b":y` or
/// `"a":x? or "b":y?`. A later key is a quoted name outside brackets, after `|` or `or`, and
/// followed by `:`; any other quoted text is a value (`"mode":"a"|"b"`).
fn quoted_entry<'a>(entry: &'a str, out: &mut ParamKeys<'a>) -> Result<(), String> {
    // (where the key's opening quote is, where its text starts, its name)
    let mut found: Vec<(usize, usize, &'a str)> = Vec::new();
    let bytes = entry.as_bytes();
    let (mut i, mut depth) = (0usize, 0usize);
    while let Some(&b) = bytes.get(i) {
        match b {
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth = depth.saturating_sub(1),
            b'"' | b'`' => {
                let Some(len) = entry.get(i + 1..).and_then(|r| r.find(b as char)) else { break };
                let close = i + 1 + len;
                if depth == 0 && b == b'"' {
                    let name = entry.get(i + 1..close).unwrap_or_default();
                    let before = entry.get(..i).unwrap_or_default().trim_end();
                    let after = entry.get(close + 1..).unwrap_or_default().trim_start_matches('?').trim_start();
                    let separated = before.ends_with('|') || before == "or" || before.ends_with(" or");
                    if before.is_empty() || (separated && after.starts_with(':') && is_key(name)) {
                        found.push((i, close + 1, name));
                    }
                }
                i = close + 1;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    if !found.first().is_some_and(|f| is_key(f.2)) {
        let start: String = entry.chars().take(60).collect();
        return Err(format!("`{start}` doesn't start with a parameter name"));
    }
    for (n, &(_, start, name)) in found.iter().enumerate() {
        let end = found.get(n + 1).map_or(entry.len(), |f| f.0);
        let text = entry.get(start..end).unwrap_or_default().trim_end();
        let text = text.strip_suffix('|').or_else(|| text.strip_suffix(" or")).unwrap_or(text).trim_end();
        let optional = text.starts_with('?') || split_top(text, '?').len() > 1;
        out.keys.push((name, optional, text));
    }
    Ok(())
}

/// Where the reply starts in a piece of prose: a `→`, `->` or `Returns` outside brackets.
fn reply_start(s: &str) -> Option<usize> {
    let (mut depth, mut quote) = (0usize, None);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '`') => quote = Some(c),
            (None, '{' | '[' | '(') => depth += 1,
            (None, '}' | ']' | ')') => depth = depth.saturating_sub(1),
            (None, _) if depth == 0 => {
                let rest = s.get(i..).unwrap_or_default();
                let word_start = i == 0 || s.get(..i).is_some_and(|b| b.ends_with([' ', '.', ';', ',']));
                if rest.starts_with('→') || rest.starts_with("->") || (word_start && (rest.starts_with("Returns") || rest.starts_with("returns "))) {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Where a sentence ends outside brackets (`. ` then more prose), so what follows is prose.
fn sentence_end(s: &str) -> Option<usize> {
    let (mut depth, mut quote) = (0usize, None);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '`') => quote = Some(c),
            (None, '{' | '[' | '(') => depth += 1,
            (None, '}' | ']' | ')') => depth = depth.saturating_sub(1),
            (None, '.') if depth == 0 && s.get(i + 1..).is_some_and(|r| r.starts_with(' ')) => return Some(i),
            _ => {}
        }
    }
    None
}

/// SolidCraft's prose dialect (see [`doc_keys`]).
fn prose_keys(doc: &str) -> Result<ParamKeys<'_>, String> {
    let mut out = ParamKeys::default();
    if doc.is_empty() {
        return Err("the description is empty".into());
    }
    let mut first = true;
    let mut in_reply = false;
    for segment in split_top(doc, ';') {
        let mut s = segment.trim();
        // `…; or delete: name`: another way to call it, after a reply.
        if let Some(r) = s.strip_prefix("or ") {
            s = r.trim_start();
            in_reply = false;
        }
        if in_reply {
            continue;
        }
        if let Some(i) = reply_start(s) {
            s = s.get(..i).unwrap_or_default();
            in_reply = true;
        }
        // After a sentence ends, the rest of the segment is prose.
        if let Some(i) = sentence_end(s) {
            out.open |= s.get(i + 1..).is_some_and(|r| !r.trim().is_empty());
            s = s.get(..i).unwrap_or_default();
        }
        for piece in split_top(s, ',') {
            let p = piece.trim();
            let p = p.strip_prefix("or ").map_or(p, str::trim_start);
            if p.is_empty() {
                continue;
            }
            let len = p.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(p.len());
            let (name, rest) = (p.get(..len).unwrap_or_default(), p.get(len..).unwrap_or_default());
            let keyed = is_key(name)
                && (rest.is_empty()
                    || rest.starts_with(['?', ':', '=', '(', '|', '['])
                    || rest.starts_with(" (")
                    || rest.starts_with(" |")
                    || rest.starts_with(" :"));
            if !keyed {
                if first {
                    let lower = p.to_ascii_lowercase();
                    if lower.starts_with("no parameters") || lower.starts_with("none") {
                        return Ok(out);
                    }
                    // `as other.command`, `same as other.command`: that command's keys
                    let more = p.strip_prefix("same ").unwrap_or(p);
                    if more.starts_with("as ") {
                        out.more.push(more);
                        first = false;
                        continue;
                    }
                    let start: String = p.chars().take(60).collect();
                    return Err(format!("`{start}` doesn't start with a parameter name"));
                }
                out.open = true;
                continue; // the previous key's prose, run on after a comma
            }
            first = false;
            entry_keys(p, &mut out)?;
            // An optional key in a note: `bodies: [names] (combine?: join with the original)`.
            for (i, _) in p.match_indices('(') {
                let inner = p.get(i + 1..).unwrap_or_default();
                let len = inner.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(inner.len());
                let name = inner.get(..len).unwrap_or_default();
                let after = inner.get(len..).unwrap_or_default();
                if is_key(name) && after.starts_with("?:") && !out.keys.iter().any(|k| k.0 == name) {
                    // the note's own text, to its closing parenthesis
                    let note = after.find(')').and_then(|e| after.get(..e)).unwrap_or(after);
                    out.keys.push((name, true, note));
                }
            }
        }
    }
    Ok(out)
}

/// Whether a description's key list may be short: prose the reader skipped could name more
/// (see [`ParamKeys::open`]). A key it doesn't list is then a warning, not a rejection.
pub fn incomplete(doc: &str) -> bool {
    doc_keys(doc).is_ok_and(|k| k.open)
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
    key_texts(doc, lookup, depth).map(|k| k.into_iter().map(|(name, _)| name).collect())
}

/// Every key a command accepts with its text, as [`accepted`] finds them: its own first, then
/// its `…` imports'. A key named twice is listed once, with its first text.
pub fn key_texts(doc: &str, lookup: &dyn Fn(&str) -> Option<String>, depth: usize) -> Option<Vec<(String, String)>> {
    let parsed = doc_keys(doc).ok()?;
    let mut keys: Vec<(String, String)> = Vec::new();
    let add = |name: String, text: String, keys: &mut Vec<(String, String)>| {
        if !keys.iter().any(|k| k.0 == name) {
            keys.push((name, text));
        }
    };
    for (name, _, text) in &parsed.keys {
        add((*name).to_owned(), (*text).to_owned(), &mut keys);
    }
    let exists = |id: &str| lookup(id).is_some();
    for more in parsed.more {
        let named = named_commands(more, &exists);
        if named.is_empty() || depth > 4 {
            return None;
        }
        for id in named {
            for (name, text) in key_texts(&lookup(id)?, lookup, depth + 1)? {
                add(name, text, &mut keys);
            }
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

/// The one ASCII letter or digit `s` is, if it is one.
fn single(s: &str) -> Option<char> {
    let mut c = s.trim().chars();
    match (c.next(), c.next()) {
        (Some(x), None) if x.is_ascii_alphanumeric() => Some(x),
        _ => None,
    }
}

/// Both digits, both lower case or both upper case: a range between them is a short run.
fn same_class(a: char, b: char) -> bool {
    (a.is_ascii_digit() && b.is_ascii_digit()) || (a.is_ascii_lowercase() && b.is_ascii_lowercase()) || (a.is_ascii_uppercase() && b.is_ascii_uppercase())
}

/// Read a key's text (`: pt (1)`, `=10`, `: "a"|"b" (a)`, `: bool`, `: [x, y]`) as a port.
pub fn infer(text: &str) -> Inferred {
    let t = text.trim_start_matches(['?', ' ']);
    let mut out = Inferred::default();
    if let Some(v) = t.strip_prefix('=') {
        let v = v.split([' ', '|', ',']).next().unwrap_or_default();
        out.default = scalar(v);
    }
    let body = t.trim_start_matches([':', '=', ' ']);
    // The quoted dialect's default after the type: `u32=1920`, `"a|b"="a"`, `[w,h]=[400,200]`.
    if out.default.is_null() {
        out.default = typed_default(body);
    }
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
    // `"V1"|id|"A1"`: quoted values beside a type name (the quoted dialect) are an open set.
    let quoted = pieces.iter().take(alts.len()).filter(|p| p.trim().starts_with('"')).count();
    let mixed = pieces.first().is_some_and(|p| p.trim().starts_with('"')) && quoted < alts.len();
    // `0..9 | a..j` (SoundCraft's insert slot) is ranges, not two choices called `0..9` and
    // `a..j`: a range between two single characters is its values (`0`…`9`, `a`…`j`); any other
    // range (`1..300000`) makes the whole thing no ENUM, so it is read as a number below.
    let mut ranged = false;
    let alts: Vec<String> = alts
        .into_iter()
        .flat_map(|a| match a.split_once("..") {
            None => vec![a],
            Some((lo, hi)) => match (single(lo), single(hi)) {
                (Some(l), Some(h)) if l <= h && same_class(l, h) => (l..=h).map(String::from).collect(),
                _ => {
                    ranged = true;
                    vec![a]
                }
            },
        })
        .collect();
    if alts.len() >= 2 && !mixed && !ranged && alts.iter().all(|a| plain(a)) && !alts.iter().all(|a| a.parse::<f64>().is_ok()) {
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
    // One quoted string of alternatives: `"Stereo|Mono|5.1"?`, `"rgb|gray"="rgb"`.
    if let Some(inner) = body.strip_prefix('"').and_then(|r| r.get(..r.find('"')?)) {
        let alts: Vec<&str> = inner.split('|').map(str::trim).collect();
        if alts.len() >= 2 && alts.iter().all(|a| !a.is_empty() && plain(a)) {
            out.ty = "ENUM".into();
            out.choices = alts.iter().map(|a| (*a).to_owned()).collect();
            if !out.default.as_str().is_some_and(|d| alts.contains(&d)) {
                out.default = Value::Null;
            }
            return out;
        }
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
    if lower.trim_start_matches('"').starts_with('#') || first_word == "colour" || first_word == "color" {
        out.ty = "STRING".into();
        return out;
    }
    // A type name right after the key (the quoted dialect's `str`, `u32`, `f64`, `json`;
    // SolidCraft's `expr`): only when it is the whole first token, so prose isn't read as one.
    let word = lower.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or_default();
    let typed = lower.get(word.len()..).is_none_or(|r| r.is_empty() || r.starts_with(['?', '=', ' ', '(', ',', ';']));
    if typed && matches!(word, "json" | "any" | "object") {
        out.ty = "JSON".into();
        return out;
    }
    if typed && matches!(word, "str" | "string" | "text" | "expr") {
        out.ty = "STRING".into();
        if !out.default.is_string() {
            out.default = Value::Null;
        }
        return out;
    }
    let int_word = typed && matches!(word, "u8" | "u16" | "u32" | "u64" | "i32" | "i64" | "usize" | "int" | "ticks" | "frames");
    let float_word = typed && matches!(word, "f32" | "f64" | "float" | "percent");
    let range = number_range(body);
    // A unit is a word of its own (`in`, not the start of `inert`).
    let unit = UNITS.iter().any(|u| first_word == *u || body.strip_prefix(u).is_some_and(|r| !r.starts_with(|c: char| c.is_ascii_alphabetic())));
    if range.is_some() || unit || int_word || float_word || first_word == "n" || first_word == "integer" || first_word == "number" {
        // `0..1` is a fraction, not a choice of two whole numbers
        let int = int_word
            || first_word == "integer"
            || first_word == "n"
            || (!float_word && range.is_some_and(|(a, b, int)| int && a.fract() == 0.0 && b.fract() == 0.0 && b - a > 1.0 && !unit));
        out.ty = if int { "INT" } else { "FLOAT" }.into();
        if let Some((a, b, _)) = range {
            out.min = Some(a);
            out.max = Some(b);
        }
        if !out.default.is_number() {
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

/// The default after a type with no space before it (`u32=1920`, `"a|b"="a"`, `bool=false`,
/// `[w,h]=[400,200]`, `name?=first`); null when there is none.
fn typed_default(body: &str) -> Value {
    let parts = split_top(body, '=');
    let (Some(ty), Some(_)) = (parts.first(), parts.get(1)) else { return Value::Null };
    if ty.is_empty() || ty.contains(char::is_whitespace) {
        return Value::Null;
    }
    let v = body.get(ty.len() + 1..).unwrap_or_default().trim_start();
    if v.starts_with(['[', '{']) {
        let end = split_top(v, ' ').first().map_or(0, |f| f.len());
        return serde_json::from_str(v.get(..end).unwrap_or_default()).unwrap_or(Value::Null);
    }
    if let Some(q) = v.strip_prefix('"') {
        return q.find('"').and_then(|e| q.get(..e)).map_or(Value::Null, Value::from);
    }
    scalar(v.split([' ', '|', ',', '(', ';']).next().unwrap_or_default())
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
    use serde_json::json;

    #[test]
    fn keys_as_vectorcraft_reads_them() {
        let k = doc_keys("{x, width?: pt, name?|title?: document name, sides=6, …settings as file.new} → {id}").unwrap();
        assert_eq!(k.names(), ["x", "width", "name", "title", "sides"]);
        assert_eq!(k.more, ["settings as file.new"]);
        let k = doc_keys("{path} or {name, dataBase64}").unwrap();
        assert_eq!(k.names(), ["path", "name", "dataBase64"]);
        let k = doc_keys("{mode?: \"a\"|\"b\" (a), level?: 0..100}").unwrap();
        assert_eq!(k.names(), ["mode", "level"]);
        // EffectCraft's `key? unit` and `key? [shape]`
        let k = doc_keys("{name?, focalLength? mm, position? [x,y,z], dof?}").unwrap();
        assert_eq!(k.names(), ["name", "focalLength", "position", "dof"]);
        assert_eq!(k.keys.get(2).map(|e| e.2.trim()), Some("[x,y,z]"));
        assert!(doc_keys("{style: a|b (or display name), or blendingOptions (a group)}").is_err(), "`or` is not a key");
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
    fn quoted_keys_as_filmcraft_photocraft_and_wordcraft_write_them() {
        let k = doc_keys(r#"{"name":str,"width":u32=1920,"mix":"Stereo|Mono|5.1"?,"fromItems":[itemId]?}"#).unwrap();
        assert_eq!(k.names(), ["name", "width", "mix", "fromItems"]);
        assert_eq!(k.keys.iter().map(|x| x.1).collect::<Vec<_>>(), [false, false, true, true]);
        // alternatives: `|` and ` or ` before a quoted key followed by `:`; quoted values aren't keys
        let k = doc_keys(r#"{"item":id|"items":[id] (back to back),"track":"V1"|id|"A1" (sound only)?,"layer":id? or "layers":[id,…]?}"#).unwrap();
        assert_eq!(k.names(), ["item", "items", "track", "layer", "layers"]);
        // WordCraft: `"key"?:`, bare `"key"?`, imports, and alternative objects; the reply isn't keys
        let k = doc_keys(r#"{"template"?: "blank|letter", "title"?, …options of edit.find} → {found: {comments}, residual}"#).unwrap();
        assert_eq!(k.names(), ["template", "title"]);
        assert_eq!(k.more, ["options of edit.find"]);
        let k = doc_keys(r#"{"value": number (multiple) } | {"atLeast": pt} | {"exactly": pt}"#).unwrap();
        assert_eq!(k.names(), ["value", "atLeast", "exactly"]);
        assert!(doc_keys(r#"{"…": x}"#).is_err());
        assert!(!k.open, "a braced description is read whole");
    }

    #[test]
    fn prose_as_solidcraft_writes_it() {
        let k =
            doc_keys("length, width, height: expr; corner?: [x,y,z] | center?: [x,y,z] (default: the origin); operation?, targets?: [body], name?").unwrap();
        assert_eq!(k.names(), ["length", "width", "height", "corner", "center", "operation", "targets", "name"]);
        assert!(!k.open);
        // the reply after `→` or `Returns`, until `; or` starts another way to call it
        let k = doc_keys(
            "name, expression (number or text), unit?: mm|cm; or delete: name; or changes: {name: expression} → changed?: {name: value}, recomputed: features",
        )
        .unwrap();
        assert_eq!(k.names(), ["name", "expression", "unit", "delete", "changes"]);
        let k = doc_keys("center, major: [x,y] | minor_radius?: n. Returns created: {points}").unwrap();
        assert_eq!(k.names(), ["center", "major", "minor_radius"]);
        // a note's optional key; prose run on after a comma or a sentence: open
        let k = doc_keys("features: [names] | bodies: [names] (combine?: join with the original); plane: XY|XZ").unwrap();
        assert_eq!(k.names(), ["features", "bodies", "combine", "plane"]);
        assert_eq!(k.keys.iter().find(|x| x.0 == "combine").unwrap().2, "?: join with the original");
        let k = doc_keys("position: finite number, clamped to 0.05..0.95. Transient comparison").unwrap();
        assert_eq!(k.names(), ["position"]);
        assert!(k.open && incomplete("position: finite number, clamped to 0.05..0.95. Transient comparison"));
        assert!(doc_keys("no parameters → {items}").unwrap().keys.is_empty());
        assert_eq!(doc_keys("as graphics.template.export").unwrap().more, ["as graphics.template.export"]);
        assert!(doc_keys("ordered typed rules, rows and active state").is_err());
        assert!(doc_keys("").is_err(), "an empty description says nothing, not 'no keys'");
        let lookup = |id: &str| (id == "properties.set").then(|| "{color?: name, layer?: name}".to_owned());
        assert_eq!(accepted("as properties.set, leaving out locked layers", &lookup, 0).unwrap(), ["color", "layer"]);
    }

    #[test]
    fn types_from_the_quoted_and_prose_dialects() {
        let t = |s: &str| infer(s);
        let w = t(":1..300000=1920");
        assert_eq!((w.ty.as_str(), w.min, w.max, w.default.clone()), ("INT", Some(1.0), Some(300000.0), Value::from(1920)));
        let r = t(":0.1..1000=1");
        assert_eq!((r.ty.as_str(), r.default.clone()), ("FLOAT", Value::from(1)));
        let m = t(r#":"rgb|gray|cmyk"="rgb""#);
        assert_eq!((m.ty.as_str(), m.choices.clone(), m.default.clone()), ("ENUM", vec!["rgb".to_owned(), "gray".into(), "cmyk".into()], Value::from("rgb")));
        assert_eq!(t(r#":"Stereo|Mono"?"#).default, Value::Null);
        assert_eq!(t(r#":"Letter|Legal|…""#).ty, "", "an open list isn't an ENUM");
        let track = t(r#":"V1"|id|"A1" (sound only)|"new"?"#);
        assert!(track.ty != "ENUM" && track.choices.is_empty(), "{track:?}");
        // SoundCraft's insert slot: ranges, which the craft takes as 3, "5" or "c", never "0..9"
        let slot = t("?: 0..9 | a..j");
        let want: Vec<String> = ('0'..='9').chain('a'..='j').map(String::from).collect();
        assert_eq!((slot.ty.as_str(), &slot.choices), ("ENUM", &want), "each slot, by number or by letter");
        assert_eq!(t(":1..300000=1920").ty, "INT", "a range of numbers is still a number");
        assert_eq!(t(":u32?").ty, "INT");
        assert_eq!(t(":f64=23.976").default, Value::from(23.976));
        assert_eq!(t(":ticks?").ty, "INT");
        assert_eq!(t(":percent=100").ty, "FLOAT");
        assert_eq!(t(":str").ty, "STRING");
        assert_eq!(t(":json").ty, "JSON");
        assert_eq!(t(":bool=false").default, Value::Bool(false));
        assert_eq!(t(":[w,h]=[400,200]").default, json!([400, 200]));
        assert_eq!(t(r##":"#rrggbb"?"##).ty, "STRING");
        assert_eq!(t(": expr").ty, "STRING");
        assert_eq!(t(": constant|curvature (alias g2)|chord").choices, ["constant", "curvature", "chord"]);
        // a type with prose after a space isn't a typed default
        assert_eq!(t(": pt (1)").default, Value::from(1));
        assert_eq!(t(": name of a = b").default, Value::Null);
        // a unit is a word of its own; `0–1` is a fraction; `""` among bare values is one more
        assert_eq!(t(": inert copied look label").ty, "");
        assert_eq!(t(": in (inches)").ty, "FLOAT");
        assert_eq!(t("? (0–1, default 1)").ty, "FLOAT");
        assert_eq!(t(": mm|cm|in|\"\"|text (the unit)").choices, ["mm", "cm", "in", "text"]);
    }

    /// Random descriptions from the dialects' punctuation, multi-byte text included.
    #[test]
    fn random_descriptions_never_panic() {
        let alphabet = [
            "{", "}", "[", "]", "(", ")", "\"", "`", "?", ":", "=", "|", ",", ";", ".", ". ", " ", "or ", "…", "→", "->", "Returns ", "a", "b1", "_", "é", "0",
            "1..2", "-", "as ", "x.y",
        ];
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..20_000 {
            let mut s = String::new();
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            for _ in 0..(seed >> 60) + 1 {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                s.push_str(alphabet[(seed >> 33) as usize % alphabet.len()]);
            }
            let _ = doc_keys(&s);
            let _ = key_texts(&s, &|id| Some(format!("{{{id}?, …as {id}}}")), 0);
            let _ = incomplete(&s);
            let _ = infer(&s);
        }
    }

    #[test]
    fn hostile_descriptions_never_panic() {
        for s in [
            "\"",
            "{\"",
            "{\"a",
            "{\"a\"",
            "{\"a\"?",
            "{\"a\":\"",
            "{\"a\":x|\"",
            "{\"a\":x or \"b\"",
            "{\"é\": 1}",
            "a,",
            "a;",
            ";;",
            ", ,",
            "a: (",
            "a → ",
            "→",
            "->",
            "Returns",
            "a (b?:",
            "a (b?: c",
            "a. ",
            ".",
            "or ",
            "; or",
            "same as",
            "as",
            "a | | b",
            "é: 1",
            "a: `x",
        ] {
            let _ = doc_keys(s);
            let _ = accepted(s, &|_| None, 0);
            let _ = key_texts(s, &|id| Some(id.to_owned()), 0);
            let _ = incomplete(s);
            let _ = infer(s);
        }
        for s in ["=[", ":[=", ":x=[1,", ":\"a|b\"=\"", ":\"", ":u32=", ":\"|\"", "=\"", ":a=\"b"] {
            let _ = infer(s);
        }
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
