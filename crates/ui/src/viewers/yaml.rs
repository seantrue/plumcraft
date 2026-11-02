//! JSON values as YAML (the maintainer: "JSON yes, (format as Yaml for display)"): easier to read
//! than JSON, with no braces or quotes around ordinary words. For display only: the value stays
//! JSON. Written here rather than with a YAML crate because the one everybody used is no longer
//! maintained, and this needs a hundred lines. Strings that YAML would read as something else
//! are quoted; the output parses back to the same value (a test holds it to that).

use serde_json::Value;

/// How deep it writes before showing the rest as flow JSON: indentation past this helps nobody,
/// and a hostile value is not to recurse without end.
const MAX_DEPTH: usize = 64;

/// A plain YAML word for `s`, or `s` quoted.
fn scalar(s: &str) -> String {
    if is_plain(s) { s.to_owned() } else { quoted(s) }
}

/// `s` double-quoted: JSON's escapes are YAML's too, but JSON leaves as they are characters that
/// a YAML reader folds or refuses (U+0085, U+2028, DEL), so those are escaped as well.
fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in Value::from(s).to_string().chars() {
        if is_printable(c) && !c.is_control() {
            out.push(c);
        } else if let Ok(b) = u8::try_from(u32::from(c)) {
            out.push_str(&format!("\\x{b:02X}"));
        } else {
            out.push_str(&format!("\\u{:04X}", u32::from(c)));
        }
    }
    out
}

/// Can `s` be written bare and read back as this same string?
fn is_plain(s: &str) -> bool {
    let (Some(first), Some(last)) = (s.chars().next(), s.chars().last()) else { return false };
    if first.is_whitespace() || last.is_whitespace() || last == ':' {
        return false;
    }
    // indicators that start something else; `-`, `?` and `:` only when a space follows
    if "!&*[]{}|>'\"%@`#,".contains(first) || ("-?:".contains(first) && s.chars().nth(1).is_none_or(char::is_whitespace)) {
        return false;
    }
    if s.contains(": ") || s.contains(" #") || s.chars().any(|c| c.is_control() || !is_printable(c)) {
        return false;
    }
    !reads_as_other(s)
}

/// Unicode YAML treats as line breaks, or that won't show.
fn is_printable(c: char) -> bool {
    !matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{feff}' | '\u{fffe}' | '\u{ffff}')
}

/// Would a YAML reader, of either version, take `s` for a null, a boolean, a number or a date?
fn reads_as_other(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("---") || lower.starts_with("...") {
        return true; // document markers
    }
    if matches!(lower.as_str(), "~" | "null" | "true" | "false" | "yes" | "no" | "on" | "off" | "y" | "n" | ".inf" | "+.inf" | "-.inf" | ".nan" | "<<" | "=") {
        return true;
    }
    if s.len() >= 8 && s.as_bytes().first().is_some_and(u8::is_ascii_digit) && s.as_bytes().get(4) == Some(&b'-') {
        return true; // a date
    }
    // anything number-shaped: 12, 1.5, 1e3, 0x1f, 0o7, +1, 1_000, 12:30:00 (a YAML 1.1 sexagesimal)
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    body.as_bytes().first().is_some_and(|b| b.is_ascii_digit() || *b == b'.')
        && body.chars().all(|c| c.is_ascii_hexdigit() || matches!(c, '.' | '_' | ':' | '+' | '-' | 'x' | 'o' | 'X' | 'O'))
}

/// A string over several lines as a block (`|`), or `None` when only quoting keeps it exact.
fn block(s: &str, indent: usize) -> Option<String> {
    let (body, chomp) = match s.strip_suffix('\n') {
        Some(b) => (b, "|"),
        None => (s, "|-"),
    };
    let clean = body.contains('\n') || chomp == "|";
    let hard = body.is_empty()
        || body.ends_with('\n')
        || body.starts_with([' ', '\t', '\n'])
        || body.chars().any(|c| (c.is_control() && c != '\n') || !is_printable(c));
    if !clean || hard || body.split('\n').any(|l| l.ends_with(' ') || l.starts_with(' ')) {
        return None;
    }
    let pad = " ".repeat(indent);
    let lines: Vec<String> = body.split('\n').map(|l| if l.is_empty() { String::new() } else { format!("{pad}{l}") }).collect();
    Some(format!("{chomp}\n{}", lines.join("\n")))
}

fn leaf(v: &Value, indent: usize) -> String {
    match v {
        Value::String(s) if s.contains('\n') => block(s, indent).unwrap_or_else(|| quoted(s)),
        Value::String(s) => scalar(s),
        other => other.to_string(),
    }
}

fn is_empty(v: &Value) -> bool {
    v.as_object().is_some_and(serde_json::Map::is_empty) || v.as_array().is_some_and(Vec::is_empty)
}

fn nested(v: &Value) -> bool {
    (v.is_object() || v.is_array()) && !is_empty(v)
}

/// `v` at `indent`, its first line after whatever `first` the caller wrote (`- `).
fn write(out: &mut String, v: &Value, indent: usize, depth: usize, first: bool) {
    let pad = " ".repeat(indent);
    let start = |out: &mut String, i: usize| {
        if !(first && i == 0) {
            out.push_str(&pad);
        }
    };
    if depth >= MAX_DEPTH {
        start(out, 0);
        out.push_str(&v.to_string());
        out.push('\n');
        return;
    }
    match v {
        Value::Object(m) if !m.is_empty() => {
            for (i, (k, x)) in m.iter().enumerate() {
                start(out, i);
                out.push_str(&scalar(k));
                out.push(':');
                if nested(x) {
                    out.push('\n');
                    // a list stays level with its key's own indent plus two, as people write it
                    write(out, x, indent + 2, depth + 1, false);
                } else {
                    out.push(' ');
                    out.push_str(&leaf(x, indent + 2));
                    out.push('\n');
                }
            }
        }
        Value::Array(a) if !a.is_empty() => {
            for (i, x) in a.iter().enumerate() {
                start(out, i);
                out.push_str("- ");
                if nested(x) {
                    write(out, x, indent + 2, depth + 1, true);
                } else {
                    out.push_str(&leaf(x, indent + 2));
                    out.push('\n');
                }
            }
        }
        other => {
            start(out, 0);
            out.push_str(&leaf(other, indent + 2));
            out.push('\n');
        }
    }
}

/// `v` as YAML.
pub fn to_yaml(v: &Value) -> String {
    let mut out = String::new();
    write(&mut out, v, 0, 0, false);
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn it_reads_as_people_write_it() {
        let v = json!({"name": "corgi", "age": 3, "good": true, "toys": ["ball", {"kind": "rope", "length": 1.5}], "owner": null, "empty": [], "none": {}});
        let want = "age: 3\nempty: []\ngood: true\nname: corgi\nnone: {}\nowner: null\ntoys:\n  - ball\n  - kind: rope\n    length: 1.5\n";
        assert_eq!(to_yaml(&v), want);
        assert_eq!(to_yaml(&json!([[1, 2], [3]])), "- - 1\n  - 2\n- - 3\n");
        assert_eq!(to_yaml(&json!("plain words")), "plain words\n");
        assert_eq!(to_yaml(&json!(1.5)), "1.5\n");
    }

    #[test]
    fn strings_that_would_read_as_something_else_are_quoted() {
        for s in [
            "",
            " pad",
            "pad ",
            "true",
            "No",
            "null",
            "~",
            "12",
            "1.5",
            "1e3",
            "0x1f",
            "12:30:00",
            "2026-10-30",
            "a: b",
            "a #b",
            "- x",
            "-",
            "?",
            ":",
            "[x]",
            "{x}",
            "*a",
            "&a",
            "!x",
            "|",
            ">",
            "'q",
            "\"q",
            "%x",
            "@x",
            "`x",
            "#x",
            "x:",
            "tab\there",
        ] {
            let y = to_yaml(&json!(s));
            assert!(y.starts_with('"'), "{s:?} wrote {y:?}");
        }
        for s in ["corgi", "two words", "a-b", "a:b", "x#y", "-x", "1st", "C:/dir", "100%", "naïve"] {
            assert_eq!(to_yaml(&json!(s)), format!("{s}\n"), "{s:?}");
        }
    }

    #[test]
    fn several_lines_are_a_block() {
        assert_eq!(to_yaml(&json!({"poem": "one\ntwo\n"})), "poem: |\n  one\n  two\n");
        assert_eq!(to_yaml(&json!({"poem": "one\ntwo"})), "poem: |-\n  one\n  two\n");
        assert_eq!(to_yaml(&json!({"poem": "one\n\ntwo"})), "poem: |-\n  one\n\n  two\n");
        assert!(to_yaml(&json!({"odd": "  indented\nnext"})).starts_with("odd: \""), "exactness first");
    }

    /// Whatever the value, the YAML reads back to it: the point of all the quoting.
    #[test]
    fn it_parses_back_to_the_same_value() {
        let tricky = [
            "",
            " ",
            "\n",
            "\n\n",
            "a\n",
            "a\n\n",
            "\ta",
            "a\tb",
            "true",
            "yes",
            "1",
            "0o7",
            "1_000",
            "+1",
            ".5",
            "1e+3",
            "~",
            "null",
            "a: b",
            "- a",
            "#",
            "a #",
            "é",
            "🐕",
            "\u{85}",
            "\u{2028}",
            "\u{feff}x",
            "\\",
            "\"",
            "'",
            "%",
            "---",
            "...",
            "=",
            "<<",
            "[",
            "x\u{7f}",
            "\r\n",
            "a\rb",
        ];
        let mut list = Vec::new();
        for s in tricky {
            let back: Value = serde_yaml::from_str(&to_yaml(&json!(s))).unwrap_or_else(|e| panic!("{s:?}: {e}\n{}", to_yaml(&json!(s))));
            assert_eq!(back, json!(s), "{s:?} wrote {:?}", to_yaml(&json!(s)));
            list.push(json!({s: [s, {"deep": s}], "n": [s]}));
        }
        let v = json!({"all": list, "nums": [0, -1, 1.5, 1e300, -1e-300, 18446744073709551615u64], "lists": [[], {}, [[]], [{}], [[1, [2, [3]]]], {"a": {"b": {"c": []}}}], "b": [true, false, null]});
        let y = to_yaml(&v);
        let back: Value = serde_yaml::from_str(&y).unwrap_or_else(|e| panic!("{e}\n{y}"));
        assert_eq!(back, v);
    }

    #[test]
    fn a_hostile_depth_is_bounded_and_still_valid() {
        let deep = |n: usize| (0..n).fold(json!(1), |v, _| json!([v]));
        assert!(to_yaml(&deep(500)).len() < 40_000, "indentation stops growing");
        // past the depth it is flow JSON, which is YAML (the parser's own limit is 128)
        let back: Value = serde_yaml::from_str(&to_yaml(&deep(100))).unwrap();
        assert_eq!(back, deep(100));
    }
}
