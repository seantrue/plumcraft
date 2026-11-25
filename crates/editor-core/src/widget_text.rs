//! What a widget shows of a value, and what decides which widget a port gets: the pure part of
//! the egui shell's `widgets.rs`, moved here in N2 slice 2 so the scene ([`crate::scene`]) words
//! rows as the canvas does. `plumcraft-ui`'s `widgets` re-exports it.

use plumcraft_proto::catalogue::{Port, scalars, widgets as hints};
use serde_json::Value;

/// How a blank number (JSON null: the engine's own fallback applies) is shown.
pub const BLANK: &str = "blank";

/// The number of decimals in a JSON number as written (`0.375` → 3, `2.0` → 1, `7` → 0).
pub fn decimals_of(v: &Value) -> usize {
    match v {
        Value::Number(n) if n.is_f64() => {
            let s = n.to_string();
            if s.contains(['e', 'E']) {
                return 6;
            }
            s.split_once('.').map_or(0, |(_, frac)| frac.len())
        }
        _ => 0,
    }
}

/// The most decimals a value is shown with because of its own digits.
pub const MOST_NEEDED: usize = 6;

/// The decimals a number needs, read as a person reads it (N4 slice 4): the fewest, at most
/// [`MOST_NEEDED`], that give it back to within a part in 10^9, so a slider's `0.30000000000000004`
/// needs 1 and `0.375` needs 3. Not a number: 0.
pub fn needed_decimals(v: &Value) -> usize {
    let Some(x) = v.as_f64().filter(|x| x.is_finite()) else { return 0 };
    let tol = 1e-9 * x.abs().max(1.0);
    (0..=MOST_NEEDED)
        .find(|d| {
            let m = 10f64.powi(*d as i32);
            ((x * m).round() / m - x).abs() <= tol
        })
        .unwrap_or(MOST_NEEDED)
}

/// Decimals to show for a FLOAT port (the egui sheet's, the scene's and every shell's, N4 slice
/// 4): the port's `decimals`, its `step`'s (a step of 0.01 shows 2), and never fewer than its
/// default as written, or its current value needs ([`needed_decimals`]), so a value is never
/// rounded away (the research software's mosaic `tilesize` 0.375) and float noise never shows.
/// One at least when nothing says.
pub fn float_decimals(port: &Port, current: &Value) -> usize {
    let declared = port.decimals.map_or(0, |d| d.min(12) as usize);
    let step = port.step.map_or(0, |s| needed_decimals(&Value::from(s)));
    let floor = decimals_of(&port.default).min(12).max(needed_decimals(current));
    let d = declared.max(step).max(floor);
    if port.decimals.is_none() && port.step.is_none() && d == 0 { 1 } else { d.min(12) }
}

/// A number as shown (N4 slice 4): an INT whole, never with decimals; a FLOAT with `decimals`;
/// blank as [`BLANK`].
pub fn number_text(v: &Value, int: bool, decimals: usize) -> String {
    match v.as_f64() {
        None => BLANK.into(),
        Some(x) if int => format!("{}", x.round() as i64),
        Some(x) => format!("{x:.decimals$}"),
    }
}

/// Text for a value as shown read-only.
pub fn value_text(v: &Value) -> String {
    match v {
        Value::Null => "—".into(),
        Value::String(s) if s.is_empty() => "(blank)".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The most characters a read-only value shows on a face before it is summed up.
pub const READONLY_CHARS: usize = 40;

/// A value with no widget and no application summary, as a face's row shows it: short values as
/// they are; a longer object as its fields' names, a longer list as its length, a longer string
/// cut. The whole value is in the parameter editor and the row's hover.
pub fn value_short(v: &Value) -> String {
    let full = value_text(v);
    if full.chars().count() <= READONLY_CHARS {
        return full;
    }
    let cut = |s: &str| {
        let mut out: String = s.chars().take(READONLY_CHARS.saturating_sub(1)).collect();
        out.push('…');
        out
    };
    match v {
        Value::Object(o) => {
            let names: Vec<&str> = o.keys().map(String::as_str).collect();
            let n = names.len();
            let text = format!("{n} field{}: {}", if n == 1 { "" } else { "s" }, names.join(", "));
            if text.chars().count() <= READONLY_CHARS { text } else { cut(&text) }
        }
        Value::Array(a) => format!("{} item{}", a.len(), if a.len() == 1 { "" } else { "s" }),
        // JSON carried as a string (a curve's, say): summed up as what it holds
        Value::String(t) if t.trim_start().starts_with(['{', '[']) => match serde_json::from_str::<Value>(t) {
            Ok(inner @ (Value::Object(_) | Value::Array(_))) => value_short(&inner),
            _ => cut(&full),
        },
        _ => cut(&full),
    }
}

/// A choice as shown in a combo box (`""` is "(blank)").
pub fn choice_text(v: &Value) -> String {
    match v {
        Value::String(s) if s.is_empty() => "(blank)".into(),
        Value::String(s) => s.clone(),
        Value::Null => "(none)".into(),
        other => other.to_string(),
    }
}

/// Does this port show an editable widget kind PlumCraft knows (a scalar or a choice)?
pub fn has_editor(port: &Port) -> bool {
    scalars::is_scalar(&port.ty) || port.choices.is_some() || matches!(port.widget.as_deref(), Some(hints::COLOR | hints::HEX_COLOR))
}

/// Why choice `c` of `port` can't be picked on this machine (`Port::disabled_choices`), or
/// None.
pub fn disabled_reason<'a>(port: &'a Port, c: &Value) -> Option<&'a str> {
    let key = c.as_str().map_or_else(|| c.to_string(), str::to_owned);
    port.disabled_choices.as_ref()?.get(&key).map(String::as_str)
}

/// When `port` (a setting its node reads only on some branches, `Port::active_when`) is not
/// read with the node's current values: the text saying when it is. `value_of` gives a
/// setting's current value by its port key (`section.name`); settings the node has no port
/// for count as matching.
pub fn inactive_note(port: &Port, value_of: impl Fn(&str) -> Option<Value>) -> Option<String> {
    let when = port.active_when.as_ref()?;
    let mut unmet = Vec::new();
    for (setting, values) in when {
        if let Some(v) = value_of(setting)
            && !values.contains(&v)
        {
            let shown: Vec<String> = values.iter().map(choice_text).collect();
            unmet.push(format!("{setting} is {}", shown.join(" or ")));
        }
    }
    (!unmet.is_empty()).then(|| format!("Not used now: read only when {}.", unmet.join(" and ")))
}

/// A socket's label with its wire type after it (`audio · WAV`), unless the label already
/// names the type (`audio` on an AUDIO port) or the type is a scalar a widget shows. Two ports
/// both called `audio` that can't be wired together then say why at a glance.
pub fn typed_label(label: &str, ty: &str) -> String {
    let norm = |s: &str| s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect::<String>();
    let scalar = matches!(ty, "FLOAT" | "INT" | "BOOL" | "STRING" | "ENUM" | "ANY" | "");
    let shown = shown_type(ty);
    if scalar || norm(label) == norm(shown) { label.to_owned() } else { format!("{label} · {shown}") }
}

/// A wire type as a person reads it: a pack's own type (`x.<pack>.<name>`, interface 7.2) without
/// its namespace, which is for engines (two packs' types never meet on one wire unless a Convert
/// joins them); every other type as it is. The full id stays in the tooltip ([`typed_tip`]).
pub fn shown_type(ty: &str) -> &str {
    match ty.strip_prefix("x.").and_then(|rest| rest.split_once('.')) {
        Some((pack, name)) if !pack.is_empty() && !name.is_empty() => name,
        _ => ty,
    }
}

/// A port's tooltip with its type on the first line.
pub fn typed_tip(ty: &str, tip: Option<&str>) -> String {
    match tip.filter(|t| !t.is_empty()) {
        Some(t) => format!("Type: {ty}\n{t}"),
        None => format!("Type: {ty}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pack_types_show_without_their_namespace_and_long_values_are_summed_up() {
        use serde_json::json;
        assert_eq!(super::shown_type("x.pk.signals"), "signals");
        assert_eq!(super::shown_type("x.pk.a.b"), "a.b");
        assert_eq!(super::shown_type("x.signals"), "x.signals");
        assert_eq!(super::shown_type("x..y"), "x..y");
        assert_eq!(super::shown_type("ARRAY"), "ARRAY");
        assert_eq!(super::typed_label("signals", "x.pk.signals"), "signals");
        assert_eq!(super::typed_label("paths", "x.pk.strokes"), "paths · strokes");
        assert_eq!(super::typed_label("doc", "soundcraft.session"), "doc · soundcraft.session");
        assert_eq!(super::typed_tip("x.pk.signals", None), "Type: x.pk.signals");
        assert_eq!(super::value_short(&json!({"a": 1})), r#"{"a":1}"#);
        let curves = json!({"master": {"points": [[0, 0], [0.25, 0.17], [0.55, 0.68], [1, 1]]}, "red": {}, "green": {}, "blue": {}});
        assert_eq!(super::value_short(&curves), "4 fields: blue, green, master, red");
        assert_eq!(super::value_short(&json!((0..50).collect::<Vec<_>>())), "50 items");
        assert_eq!(super::value_short(&json!(curves.to_string())), "4 fields: blue, green, master, red", "JSON in a string");
        let long = "x".repeat(100);
        let s = super::value_short(&json!(long));
        assert_eq!(s.chars().count(), super::READONLY_CHARS);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn numbers_show_what_a_person_reads() {
        use serde_json::json;
        assert_eq!(super::needed_decimals(&json!(0.30000000000000004)), 1);
        assert_eq!(super::needed_decimals(&json!(0.375)), 3);
        assert_eq!(super::needed_decimals(&json!(1.0 / 3.0)), super::MOST_NEEDED);
        assert_eq!(super::needed_decimals(&json!(7)), 0);
        let p = |v: serde_json::Value| serde_json::from_value::<plumcraft_proto::catalogue::Port>(v).unwrap();
        let amount = p(json!({"name": "amount", "type": "FLOAT", "default": 0.5, "step": 0.01}));
        assert_eq!(super::float_decimals(&amount, &json!(0.30000000000000004)), 2, "the step's, not the noise's");
        assert_eq!(super::number_text(&json!(0.30000000000000004), false, 2), "0.30");
        assert_eq!(super::number_text(&json!(6.7), true, 0), "7", "an INT never shows decimals");
        assert_eq!(super::number_text(&json!(null), false, 2), super::BLANK);
    }
}
