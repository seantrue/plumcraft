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
    if scalar || norm(label) == norm(ty) { label.to_owned() } else { format!("{label} · {ty}") }
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
