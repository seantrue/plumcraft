//! Editing a value on a node's face (N4 slice 2; `docs/shell-interface.md` 12.10): which rows
//! can be edited, what a shell's field editor typed becomes, and the order Tab goes in.
//!
//! **The shell recognises, the core decides** (plan section 3, rule 2). A shell draws a widget
//! at rest into its face and, when one is to be edited, puts one native control over it (the
//! field-editor pattern, plan 5.4). What that control sends is checked here, the same for every
//! shell and for a script: a number is parsed, made whole where the port is an INT and held to
//! its `min`..`max` (with a note saying so), a choice must be one of the port's and not a
//! disabled one, a colour is put in the port's own format, several choices are each checked.
//! A value the core refuses changes nothing and says why; one it takes is one `node.set`, one
//! undo step ("Set Value"), and none at all when it is what the port holds already.
//!
//! Everything here is hostile input: a text is at most [`MAX_TEXT`] bytes, a number must be
//! finite, an unknown widget kind is refused, never a panic.

use plumcraft_graph::NodeId;
use serde_json::{Value, json};

use crate::scene::{Face, InRow, Widget};

/// The widget kinds a face's row can be edited with.
pub const EDITABLE: &[&str] = &["number", "toggle", "choice", "multichoice", "text", "multiline", "path", "color"];

/// The longest text a row takes (bytes).
pub const MAX_TEXT: usize = 1 << 20;

/// The row a field editor is open on (`row.edit`; the scene's `view.edit`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RowEdit {
    /// The node whose input it is (a collapsed group's row: the member's).
    pub node: String,
    /// The input's key (`sigma`, `mode.turn`).
    pub port: String,
    /// The face that shows it, and the row's index in that face's `inputs`.
    pub face: String,
    pub row: usize,
}

/// Can `r` be edited on the face? Its own value, with a widget of a kind the core edits. A
/// wired, socket-only, inherited or read-only row can't (its words say what it is).
pub fn editable(r: &InRow) -> bool {
    r.source == "own" && r.widget.as_ref().is_some_and(|w| EDITABLE.contains(&w.kind.as_str()))
}

/// Why `r` can't be edited on the face.
pub fn why_not(r: &InRow) -> String {
    match r.source.as_str() {
        "wired" => format!("{} is wired: its value comes from the wire (disconnect it to set its own)", r.key),
        "socket" => format!("{} takes a wire only", r.key),
        "inherited" => format!("{} comes from the bundle wired into the node (Detach to use its own)", r.key),
        _ => format!("{} has no widget the editor can edit here", r.key),
    }
}

/// What a shell sends for a row: the text its field holds, or a value (a switch's, a menu's
/// choice, a colour as `[r, g, b]` in 0..1, a scrub's number).
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Text(String),
    Value(Value),
}

impl Input {
    /// `row.set`'s `text` or `value`, exactly one.
    pub fn parse(params: &Value) -> Result<Input, String> {
        match (params.get("text"), params.get("value")) {
            (Some(Value::String(t)), None) => {
                if t.len() > MAX_TEXT {
                    return Err(format!("a value is at most {} MiB of text", MAX_TEXT >> 20));
                }
                Ok(Input::Text(t.clone()))
            }
            (Some(_), None) => Err("text must be a string (what the field holds)".into()),
            (None, Some(v)) => {
                if v.to_string().len() > MAX_TEXT {
                    return Err(format!("a value is at most {} MiB", MAX_TEXT >> 20));
                }
                Ok(Input::Value(v.clone()))
            }
            _ => Err("give the row's text (what its field holds) or its value, one of them".into()),
        }
    }
}

/// A value the core takes for a row, and what it changed of what was sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub value: Value,
    /// Said when the value isn't what was sent: held to the range, made whole, put in the
    /// port's format.
    pub note: Option<String>,
}

fn num_text(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

/// A number from what was sent: text (trimmed; `blank` or nothing is blank where the port may
/// be blank), a JSON number, or null.
fn number_of(w: &Widget, label: &str, input: &Input) -> Result<Option<f64>, String> {
    let blank_ok = w.clearable || w.default.is_null();
    let text = match input {
        Input::Value(Value::Number(n)) => return n.as_f64().filter(|f| f.is_finite()).map(Some).ok_or_else(|| format!("{label} needs a finite number")),
        Input::Value(Value::Null) if blank_ok => return Ok(None),
        Input::Value(Value::Null) => return Err(format!("{label} can't be blank")),
        Input::Value(Value::String(s)) | Input::Text(s) => s.trim().to_owned(),
        Input::Value(other) => return Err(format!("{label} needs a number, not {}", short(&other.to_string()))),
    };
    if text.is_empty() || text.eq_ignore_ascii_case(crate::widget_text::BLANK) {
        return if blank_ok { Ok(None) } else { Err(format!("{label} needs a number")) };
    }
    let t = text.strip_prefix('+').unwrap_or(&text).replace('\u{2212}', "-");
    match t.parse::<f64>() {
        Ok(f) if f.is_finite() => Ok(Some(f)),
        _ => Err(format!("{label} needs a number: {} isn't one", short(&text))),
    }
}

fn short(s: &str) -> String {
    let mut t: String = s.chars().take(40).collect();
    if s.chars().count() > 40 {
        t.push('…');
    }
    format!("{t:?}")
}

/// `v` held to the widget's range (and made whole for an INT), with a note when it moved.
pub fn clamp_number(w: &Widget, label: &str, v: f64) -> (f64, Option<String>) {
    let mut note = None;
    let mut v = v;
    if w.int && v.fract() != 0.0 {
        let r = v.round();
        note = Some(format!("{label} is a whole number: set to {}", num_text(r)));
        v = r;
    }
    let (mut lo, mut hi) = (w.min.filter(|m| m.is_finite()), w.max.filter(|m| m.is_finite()));
    if w.int {
        lo = lo.map(f64::ceil);
        hi = hi.map(f64::floor);
    }
    if let (Some(l), Some(h)) = (lo, hi)
        && l > h
    {
        (lo, hi) = (Some(h), Some(l));
    }
    if let Some(l) = lo
        && v < l
    {
        note = Some(format!("{label} is at least {}: set to {}", num_text(l), num_text(l)));
        v = l;
    }
    if let Some(h) = hi
        && v > h
    {
        note = Some(format!("{label} is at most {}: set to {}", num_text(h), num_text(h)));
        v = h;
    }
    (v, note)
}

/// The JSON a number widget stores: an INT as a whole number, a FLOAT as a float.
pub fn number_value(w: &Widget, v: f64) -> Value {
    if w.int {
        // within i64 (a range past it would be the catalogue's mistake): clamped there
        let i = v.clamp(i64::MIN as f64, i64::MAX as f64) as i64;
        json!(i)
    } else {
        serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
    }
}

fn text_of(input: &Input) -> Option<String> {
    match input {
        Input::Text(t) | Input::Value(Value::String(t)) => Some(t.clone()),
        _ => None,
    }
}

/// The colour `input` names as `[r, g, b]` in 0..1: `#rrggbb` (or `rrggbb`), `r,g,b` in 0..1,
/// or a list of three numbers. Its parts held to 0..1, with a note when one moved.
fn colour_of(label: &str, input: &Input) -> Result<([f64; 3], bool), String> {
    let three = |parts: Vec<f64>| -> Result<([f64; 3], bool), String> {
        let [r, g, b] = parts[..] else { return Err(format!("{label} needs a colour: #rrggbb or r,g,b in 0..1")) };
        if ![r, g, b].iter().all(|v| v.is_finite()) {
            return Err(format!("{label} needs a colour of finite numbers"));
        }
        let c = [r, g, b].map(|v| v.clamp(0.0, 1.0));
        Ok((c, c != [r, g, b]))
    };
    if let Input::Value(Value::Array(a)) = input {
        return three(a.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect());
    }
    let Some(t) = text_of(input) else { return Err(format!("{label} needs a colour: #rrggbb or r,g,b in 0..1")) };
    let t = t.trim();
    let hex = t.strip_prefix('#').unwrap_or(t);
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("00"), 16).map_or(0.0, |b| f64::from(b) / 255.0);
        return Ok(([byte(0), byte(2), byte(4)], false));
    }
    let parts: Result<Vec<f64>, _> = t.split(',').map(|p| p.trim().parse::<f64>()).collect();
    match parts {
        Ok(p) => three(p),
        Err(_) => Err(format!("{label} needs a colour: #rrggbb or r,g,b in 0..1, not {}", short(t))),
    }
}

/// A colour in the port's format: `#rrggbb`, or `r,g,b` with at most three decimals (as egui's
/// picker writes them).
fn colour_text(hex: bool, c: [f64; 3]) -> String {
    if hex {
        let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        return format!("#{r:02x}{g:02x}{b:02x}");
    }
    c.map(|v| {
        let t = format!("{:.3}", v.clamp(0.0, 1.0));
        let t = t.trim_end_matches('0').trim_end_matches('.').to_owned();
        if t.is_empty() { "0".to_owned() } else { t }
    })
    .join(",")
}

fn choice_matches(c: &crate::scene::Choice, v: &Value) -> bool {
    if &c.value == v {
        return true;
    }
    match (v, &c.value) {
        (Value::String(s), other) => c.label == *s || crate::widget_text::choice_text(other) == *s,
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        _ => false,
    }
}

/// What `input` becomes for the widget `w` of the row labelled `label`: the value to set, or why
/// not. Pure: the caller sets it (`node.set`).
pub fn parse(w: &Widget, label: &str, input: &Input) -> Result<Parsed, String> {
    match w.kind.as_str() {
        "number" => {
            let Some(v) = number_of(w, label, input)? else { return Ok(Parsed { value: Value::Null, note: None }) };
            let (v, note) = clamp_number(w, label, v);
            Ok(Parsed { value: number_value(w, v), note })
        }
        "toggle" => {
            let b = match input {
                Input::Value(Value::Bool(b)) => *b,
                _ => match text_of(input).map(|t| t.trim().to_ascii_lowercase()).as_deref() {
                    Some("on" | "true" | "yes" | "1") => true,
                    Some("off" | "false" | "no" | "0") => false,
                    _ => return Err(format!("{label} is on or off")),
                },
            };
            Ok(Parsed { value: Value::Bool(b), note: None })
        }
        "choice" => {
            let v = match input {
                Input::Text(t) => Value::String(t.trim().to_owned()),
                Input::Value(v) => v.clone(),
            };
            if w.choices.is_empty() {
                return match v {
                    Value::String(_) | Value::Number(_) | Value::Bool(_) => Ok(Parsed { value: v, note: None }),
                    Value::Null if w.clearable || w.default.is_null() => Ok(Parsed { value: v, note: None }),
                    _ => Err(format!("{label} needs one of its choices")),
                };
            }
            let Some(c) = w.choices.iter().find(|c| choice_matches(c, &v)) else {
                let all: Vec<&str> = w.choices.iter().map(|c| c.label.as_str()).take(12).collect();
                return Err(format!("{label}: {} isn't one of its choices ({})", short(&crate::widget_text::choice_text(&v)), all.join(", ")));
            };
            if let Some(why) = &c.disabled {
                return Err(format!("{label}: {} can't be picked here: {why}", c.label));
            }
            Ok(Parsed { value: c.value.clone(), note: None })
        }
        "multichoice" => {
            let picks: Vec<String> = match input {
                Input::Value(Value::Array(a)) => a.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).collect(),
                Input::Value(Value::Null) => Vec::new(),
                _ => {
                    let Some(t) = text_of(input) else { return Err(format!("{label} needs its picks as comma-separated text")) };
                    t.split(',').map(|p| p.trim().to_owned()).collect()
                }
            };
            let picks: Vec<String> = picks.into_iter().map(|p| p.trim().to_owned()).filter(|p| !p.is_empty()).collect();
            if !w.choices.is_empty() {
                for p in &picks {
                    match w.choices.iter().find(|c| choice_matches(c, &Value::String(p.clone()))) {
                        None => return Err(format!("{label}: {} isn't one of its choices", short(p))),
                        Some(c) if c.disabled.is_some() => {
                            return Err(format!("{label}: {} can't be picked here: {}", c.label, c.disabled.as_deref().unwrap_or_default()));
                        }
                        Some(_) => {}
                    }
                }
            }
            Ok(Parsed { value: Value::String(picks.join(", ")), note: None })
        }
        "text" | "multiline" | "path" => match input {
            Input::Text(t) | Input::Value(Value::String(t)) => {
                // a field shows a non-string value as JSON: typed back, it is that JSON again
                if !w.value.is_string()
                    && !w.value.is_null()
                    && let Ok(v) = serde_json::from_str::<Value>(t)
                {
                    return Ok(Parsed { value: v, note: None });
                }
                let t = if w.kind == "text" && t.contains(['\n', '\r']) { t.replace("\r\n", " ").replace(['\n', '\r'], " ") } else { t.clone() };
                Ok(Parsed { value: Value::String(t), note: None })
            }
            Input::Value(Value::Null) if w.clearable || w.default.is_null() => Ok(Parsed { value: Value::Null, note: None }),
            Input::Value(_) => Err(format!("{label} needs text")),
        },
        "color" => {
            let hex = w.color_format.as_deref() == Some("hex");
            let (c, clamped) = colour_of(label, input)?;
            let note = clamped.then(|| format!("{label}'s parts are 0 to 1: held there"));
            Ok(Parsed { value: Value::String(colour_text(hex, c)), note })
        }
        other => Err(format!("{label}: a {} widget isn't edited on the face", short(other))),
    }
}

/// A scrub's step a screen point (egui's drag speeds, `crates/ui/src/widgets.rs`): an INT moves
/// `step · 0.1` (0.1 with no step of 1 or more), a FLOAT `step`, else `10^-decimals`.
pub fn scrub_speed(w: &Widget) -> f64 {
    if w.int {
        return w.step.filter(|s| s.is_finite() && *s >= 1.0).unwrap_or(1.0) * 0.1;
    }
    let d = w.decimals.unwrap_or(3).min(6);
    w.step.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(10f64.powi(-i32::try_from(d).unwrap_or(3)))
}

/// Where a scrub starts: the value, else the default, else 0 held to the range (a blank shows
/// inside the range, as egui's does).
pub fn scrub_start(w: &Widget) -> f64 {
    let v = w.value.as_f64().or_else(|| w.default.as_f64()).unwrap_or(0.0);
    clamp_number(w, "", v).0
}

/// A scrub's value `points` screen points from where it began: moved by the speed, held to the
/// range, a whole number for an INT, else rounded to the decimals shown.
pub fn scrub_value(w: &Widget, start: f64, points: f64) -> f64 {
    let raw = start + points * scrub_speed(w);
    let (v, _) = clamp_number(w, "", if w.int { raw.round() } else { raw });
    if w.int {
        return v;
    }
    let d = i32::try_from(w.decimals.unwrap_or(3).min(12)).unwrap_or(3);
    let k = 10f64.powi(d);
    let r = (v * k).round() / k;
    clamp_number(w, "", r).0
}

/// The editable rows of `faces` in Tab's order: faces left to right, then top to bottom (by
/// their positions, a column at a time, as a graph reads), each face's rows top to bottom.
pub fn tab_order(faces: &[Face]) -> Vec<RowEdit> {
    let mut order: Vec<&Face> = faces.iter().collect();
    order.sort_by(|a, b| {
        let key = |f: &Face| ((f.pos[0] / 40.0).round() as i64, (f.pos[1] * 1000.0).round() as i64);
        key(a).cmp(&key(b)).then_with(|| a.id.cmp(&b.id))
    });
    let mut out = Vec::new();
    for f in order {
        for (k, r) in f.inputs.iter().enumerate() {
            if editable(r) {
                out.push(RowEdit { node: r.node.clone(), port: r.key.clone(), face: f.id.clone(), row: k });
            }
        }
    }
    out
}

/// The face and row that show input `port` of `node` among `faces`.
pub fn find(faces: &[Face], node: &str, port: &str) -> Option<(usize, usize)> {
    faces.iter().enumerate().find_map(|(fi, f)| f.inputs.iter().position(|r| r.node == node && r.key == port).map(|k| (fi, k)))
}

/// `node` as a graph node id.
pub fn node_id(node: &str) -> NodeId {
    NodeId::from(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Choice;

    fn number(int: bool, min: Option<f64>, max: Option<f64>) -> Widget {
        Widget {
            kind: "number".into(),
            value: json!(3),
            default: json!(3),
            blank: false,
            clearable: false,
            min,
            max,
            soft_min: None,
            soft_max: None,
            step: if int { Some(1.0) } else { None },
            decimals: if int { None } else { Some(2) },
            text: None,
            int,
            choices: Vec::new(),
            path_mode: None,
            color_format: None,
            hover: None,
        }
    }

    fn text(s: &str) -> Input {
        Input::Text(s.into())
    }

    #[test]
    fn numbers_are_parsed_made_whole_and_held_to_their_range_with_a_note() {
        let w = number(true, Some(0.0), Some(10.0));
        assert_eq!(parse(&w, "count", &text(" 7 ")).unwrap(), Parsed { value: json!(7), note: None });
        let p = parse(&w, "count", &text("12")).unwrap();
        assert_eq!((p.value, p.note.as_deref()), (json!(10), Some("count is at most 10: set to 10")));
        let p = parse(&w, "count", &text("-3")).unwrap();
        assert_eq!((p.value, p.note.as_deref()), (json!(0), Some("count is at least 0: set to 0")));
        let p = parse(&w, "count", &text("2.6")).unwrap();
        assert_eq!((p.value, p.note.as_deref()), (json!(3), Some("count is a whole number: set to 3")));
        assert_eq!(parse(&w, "count", &text("+4")).unwrap().value, json!(4));
        assert_eq!(parse(&w, "count", &text("\u{2212}1")).unwrap().value, json!(0), "a typographic minus is a minus");
        for bad in ["abc", "", "inf", "NaN", "1e999", "4 5"] {
            let e = parse(&w, "count", &text(bad)).unwrap_err();
            assert!(e.starts_with("count needs a number"), "{bad}: {e}");
        }
        assert!(parse(&w, "count", &Input::Value(json!([1]))).is_err());
        let f = number(false, Some(0.0), Some(1.0));
        assert_eq!(parse(&f, "amount", &text("0.25")).unwrap().value, json!(0.25));
        assert_eq!(parse(&f, "amount", &Input::Value(json!(2))).unwrap().value, json!(1.0));
        // a blank-able number takes blank
        let mut b = number(false, None, None);
        b.default = Value::Null;
        assert_eq!(parse(&b, "x", &text("blank")).unwrap().value, Value::Null);
        assert_eq!(parse(&b, "x", &text("")).unwrap().value, Value::Null);
    }

    #[test]
    fn choices_colours_switches_and_picks_are_checked() {
        let mut c = number(false, None, None);
        c.kind = "choice".into();
        c.choices = vec![
            Choice { value: json!("upper"), label: "upper".into(), disabled: None },
            Choice { value: json!("off"), label: "off".into(), disabled: Some("not on this machine".into()) },
        ];
        assert_eq!(parse(&c, "mode", &Input::Value(json!("upper"))).unwrap().value, json!("upper"));
        assert_eq!(parse(&c, "mode", &text("upper")).unwrap().value, json!("upper"));
        assert!(parse(&c, "mode", &text("sideways")).unwrap_err().contains("isn't one of its choices"));
        assert_eq!(parse(&c, "mode", &text("off")).unwrap_err(), "mode: off can't be picked here: not on this machine");
        let mut t = c.clone();
        t.kind = "toggle".into();
        assert_eq!(parse(&t, "flag", &Input::Value(json!(false))).unwrap().value, json!(false));
        assert_eq!(parse(&t, "flag", &text("On")).unwrap().value, json!(true));
        assert!(parse(&t, "flag", &text("maybe")).is_err());
        let mut m = c.clone();
        m.kind = "multichoice".into();
        assert_eq!(parse(&m, "tags", &text("upper , ,")).unwrap().value, json!("upper"));
        assert!(parse(&m, "tags", &text("upper, nope")).is_err());
        assert!(parse(&m, "tags", &text("off")).is_err());
        let mut h = c.clone();
        h.kind = "color".into();
        h.color_format = Some("hex".into());
        assert_eq!(parse(&h, "tint", &text("#3366CC")).unwrap().value, json!("#3366cc"));
        assert_eq!(parse(&h, "tint", &Input::Value(json!([1.0, 0.5, 0.0]))).unwrap().value, json!("#ff8000"));
        let p = parse(&h, "tint", &Input::Value(json!([2.0, 0.0, 0.0]))).unwrap();
        assert_eq!((p.value, p.note.is_some()), (json!("#ff0000"), true));
        assert!(parse(&h, "tint", &text("red")).is_err());
        h.color_format = Some("rgb".into());
        assert_eq!(parse(&h, "tint", &text("#ff8000")).unwrap().value, json!("1,0.502,0"));
        assert_eq!(parse(&h, "tint", &text("0.1, 0.15,0.45")).unwrap().value, json!("0.1,0.15,0.45"));
        let mut s = c;
        s.kind = "text".into();
        s.value = json!("a");
        assert_eq!(parse(&s, "label", &text("two\nlines")).unwrap().value, json!("two lines"), "one line");
        s.kind = "multiline".into();
        assert_eq!(parse(&s, "note", &text("two\nlines")).unwrap().value, json!("two\nlines"));
        s.kind = "editor".into();
        assert!(parse(&s, "x", &text("a")).is_err());
    }

    #[test]
    fn a_scrub_moves_by_egui_speeds_and_stays_in_range() {
        let w = number(true, Some(0.0), Some(10.0));
        assert_eq!(scrub_speed(&w), 0.1);
        assert_eq!(scrub_value(&w, 3.0, 20.0), 5.0);
        assert_eq!(scrub_value(&w, 3.0, 900.0), 10.0);
        assert_eq!(scrub_value(&w, 3.0, -900.0), 0.0);
        let f = number(false, Some(0.0), Some(1.0));
        assert_eq!(scrub_speed(&f), 0.01);
        assert!((scrub_value(&f, 0.5, 13.0) - 0.63).abs() < 1e-9);
        assert_eq!(scrub_start(&f), 1.0, "a value past the range starts at its end");
    }

    #[test]
    fn hostile_inputs_are_refused() {
        assert!(Input::parse(&json!({})).is_err());
        assert!(Input::parse(&json!({"text": 5})).is_err());
        assert!(Input::parse(&json!({"text": "a", "value": 1})).is_err());
        assert!(Input::parse(&json!({"text": "x".repeat(MAX_TEXT + 1)})).is_err());
        assert_eq!(Input::parse(&json!({"value": null})).unwrap(), Input::Value(Value::Null));
    }
}
