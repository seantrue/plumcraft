//! Widgets for scalar ports, driven only by the catalogue's `Port` fields.
//!
//! Every edit is one `node.set` (one journal entry): a drag is committed on release, a typed
//! number or text when the field loses focus (or on Enter); a checkbox or a combo choice at
//! once. Values being edited live in [`EditBuffers`] until then, so the graph (and the journal)
//! never sees the intermediate frames.
//!
//! Lifted from the research software's `crates/ui-egui/src/widgets.rs`, without VI's curves type (no wire type
//! is hard-coded beyond the scalars `FLOAT INT BOOL STRING ENUM`), the curve and mask editors or
//! picture saving. Colours and pick lists follow [`Port::widget`].

use std::collections::HashMap;

use egui::Color32;
use plumcraft_graph::NodeId;
use plumcraft_proto::catalogue::{PathHint, Port, scalars, widgets as hints};
use serde_json::Value;

/// Where an edit is kept: a node and the value key (`port` or `port.sub`).
pub type EditKey = (NodeId, String);

/// Values being edited and not yet committed.
#[derive(Debug, Default, Clone)]
pub struct EditBuffers {
    numbers: HashMap<EditKey, f64>,
    ints: HashMap<EditKey, i64>,
    texts: HashMap<EditKey, String>,
    /// Colours being picked, committed when the picker closes.
    rgbs: HashMap<EditKey, [f32; 3]>,
    /// A path widget's "…" was clicked: the app runs the file dialog
    /// ([`crate::PathDialogs::pick_path`]) and sets the value. (Key, hint, current value.)
    browse: Option<(EditKey, PathHint, String)>,
}

impl EditBuffers {
    /// The file dialog a path widget asked for this frame, if any.
    pub fn take_browse(&mut self) -> Option<(EditKey, PathHint, String)> {
        self.browse.take()
    }

    /// Is anything being edited (a drag or a text field in progress)?
    pub fn is_editing(&self) -> bool {
        !(self.numbers.is_empty() && self.ints.is_empty() && self.texts.is_empty() && self.rgbs.is_empty())
    }
}

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

/// Decimals to show for a FLOAT port: the port's `decimals`, but never fewer than its default
/// (or its current value) has, so a value is never rounded away (the research software's mosaic `tilesize`
/// 0.375).
pub fn float_decimals(port: &Port, current: &Value) -> usize {
    let declared = port.decimals.map_or(0, |d| d.min(12) as usize);
    let floor = decimals_of(&port.default).max(decimals_of(current));
    let d = declared.max(floor);
    if port.decimals.is_none() && d == 0 { 1 } else { d.min(12) }
}

pub(crate) fn range_f64(port: &Port) -> std::ops::RangeInclusive<f64> {
    let lo = port.min.filter(|v| v.is_finite()).unwrap_or(f64::NEG_INFINITY);
    let hi = port.max.filter(|v| v.is_finite()).unwrap_or(f64::INFINITY);
    if lo <= hi { lo..=hi } else { hi..=lo }
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

/// How a blank number (JSON null: the engine's own fallback applies) is shown.
pub const BLANK: &str = "blank";

/// For a number whose default is blank: a small button that makes it blank again. Its hover
/// text is the engine's (`texts.blank_value`) when it gives one.
fn clear_button(ui: &mut egui::Ui, port: &Port, current: &Value, blank_hint: Option<&str>) -> Option<Value> {
    if !port.default.is_null() || current.is_null() {
        return None;
    }
    let r = ui.small_button("×").on_hover_text(blank_hint.unwrap_or("Blank: use the engine's fallback"));
    r.clicked().then_some(Value::Null)
}

/// Does this port show an editable widget kind PlumCraft knows (a scalar or a choice)?
pub fn has_editor(port: &Port) -> bool {
    scalars::is_scalar(&port.ty) || port.choices.is_some() || matches!(port.widget.as_deref(), Some(hints::COLOR | hints::HEX_COLOR))
}

/// Draw the widget for `port` with value `current`; returns a value to commit with `node.set`.
pub fn port_widget(ui: &mut egui::Ui, port: &Port, key: &EditKey, current: &Value, buffers: &mut EditBuffers, blank_hint: Option<&str>) -> Option<Value> {
    port_widget_in(ui, port, key, current, buffers, blank_hint, false)
}

/// As [`port_widget`], but as wide as `ui` allows (the parameter editor).
pub fn port_widget_wide(ui: &mut egui::Ui, port: &Port, key: &EditKey, current: &Value, buffers: &mut EditBuffers, blank_hint: Option<&str>) -> Option<Value> {
    port_widget_in(ui, port, key, current, buffers, blank_hint, true)
}

fn port_widget_in(
    ui: &mut egui::Ui,
    port: &Port,
    key: &EditKey,
    current: &Value,
    buffers: &mut EditBuffers,
    blank_hint: Option<&str>,
    wide: bool,
) -> Option<Value> {
    let tip = port.tooltip.as_deref().filter(|t| !t.is_empty());
    let with_tip = |r: egui::Response| match tip {
        Some(t) => r.on_hover_text(t),
        None => r,
    };
    if port.choices.is_some() || port.ty == scalars::ENUM {
        let width = if wide { (ui.available_width() - 8.0).max(120.0) } else { 120.0 };
        return enum_widget(ui, port, key, current, width, with_tip);
    }
    if matches!(port.widget.as_deref(), Some(hints::COLOR | hints::HEX_COLOR)) {
        let picked = colour_widget(ui, port, key, current, buffers, &with_tip);
        return if picked.is_none() && wide { text_widget(ui, port, key, current, buffers, wide, with_tip) } else { picked };
    }
    match port.ty.as_str() {
        scalars::FLOAT => {
            let d = float_decimals(port, current);
            // A blank shows inside the range: DragValue clamps a value outside it and reports
            // the clamp as a change, which set every blank widget whose minimum is above 0 (M3:
            // Image Trace's `colors` became 2 and overrode the preset).
            let range = range_f64(port);
            let fallback = 0f64.clamp(*range.start(), (*range.end()).max(*range.start()));
            let mut v = buffers.numbers.get(key).copied().or_else(|| current.as_f64()).or_else(|| port.default.as_f64()).unwrap_or(fallback);
            let speed = port.step.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(10f64.powi(-(d.min(6) as i32)));
            let blank = current.is_null() && !buffers.numbers.contains_key(key);
            let mut dv = egui::DragValue::new(&mut v).range(range).speed(speed).fixed_decimals(d).update_while_editing(false);
            if blank {
                dv = dv.custom_formatter(|_, _| BLANK.to_owned());
            }
            let r = with_tip(ui.add(dv));
            if let Some(v) = clear_button(ui, port, current, blank_hint) {
                return Some(v);
            }
            if r.changed() {
                buffers.numbers.insert(key.clone(), v);
            }
            if !r.dragged()
                && !r.has_focus()
                && let Some(v) = buffers.numbers.remove(key)
                && current.as_f64() != Some(v)
            {
                return serde_json::Number::from_f64(v).map(Value::Number);
            }
            None
        }
        scalars::INT => {
            let v =
                buffers.ints.get(key).copied().or_else(|| current.as_i64()).or_else(|| current.as_f64().map(|f| f as i64)).or_else(|| port.default.as_i64());
            let lo = port.min.filter(|m| m.is_finite()).map_or(i64::MIN, |m| m.ceil() as i64);
            let hi = port.max.filter(|m| m.is_finite()).map_or(i64::MAX, |m| m.floor() as i64);
            // a blank shows inside the range (see FLOAT above)
            let mut v = v.unwrap_or_else(|| 0i64.clamp(lo.min(hi), hi.max(lo)));
            let speed = port.step.filter(|s| s.is_finite() && *s >= 1.0).unwrap_or(1.0) * 0.1;
            let blank = current.is_null() && !buffers.ints.contains_key(key);
            let mut dv = egui::DragValue::new(&mut v).range(lo.min(hi)..=hi.max(lo)).speed(speed).update_while_editing(false);
            if blank {
                dv = dv.custom_formatter(|_, _| BLANK.to_owned());
            }
            let r = with_tip(ui.add(dv));
            if let Some(v) = clear_button(ui, port, current, blank_hint) {
                return Some(v);
            }
            if r.changed() {
                buffers.ints.insert(key.clone(), v);
            }
            if !r.dragged()
                && !r.has_focus()
                && let Some(v) = buffers.ints.remove(key)
                && current.as_i64() != Some(v)
            {
                return Some(Value::from(v));
            }
            None
        }
        scalars::BOOL => {
            let mut b = current.as_bool().or_else(|| port.default.as_bool()).unwrap_or(false);
            let r = with_tip(ui.checkbox(&mut b, ""));
            r.changed().then_some(Value::Bool(b))
        }
        _ => text_widget(ui, port, key, current, buffers, wide, with_tip),
    }
}

/// A text field for a STRING, committed when it loses focus; a path also gets "…".
fn text_widget(
    ui: &mut egui::Ui,
    port: &Port,
    key: &EditKey,
    current: &Value,
    buffers: &mut EditBuffers,
    wide: bool,
    with_tip: impl Fn(egui::Response) -> egui::Response,
) -> Option<Value> {
    let shown = match current {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    let editing = buffers.texts.contains_key(key);
    let mut text = buffers.texts.get(key).cloned().unwrap_or_else(|| shown.clone());
    // A path (the catalogue says so): a narrower field, then "…" for a file dialog.
    let narrow = if port.path.is_some() { 116.0 } else { 140.0 };
    let width = if wide { (ui.available_width() - if port.path.is_some() { 32.0 } else { 8.0 }).max(narrow) } else { narrow };
    // a minimum width too: a narrow node mustn't squeeze the field to nothing
    // several files (Select Files): one path per line
    let edit = if port.path.as_ref().is_some_and(PathHint::is_many) {
        egui::TextEdit::multiline(&mut text).desired_rows(3)
    } else {
        egui::TextEdit::singleline(&mut text)
    };
    let r = with_tip(ui.add(edit.desired_width(width).min_size(egui::vec2(width, 0.0))));
    if let Some(hint) = &port.path
        && ui.small_button("…").on_hover_text("Choose with a file dialog").clicked()
    {
        buffers.browse = Some((key.clone(), hint.clone(), shown.clone()));
    }
    if r.has_focus() || r.changed() {
        buffers.texts.insert(key.clone(), text.clone());
    }
    if editing && (r.lost_focus() || !r.has_focus()) {
        buffers.texts.remove(key);
        if text != shown {
            return Some(match current {
                Value::String(_) | Value::Null => Value::String(text),
                // A non-string value typed as text: keep JSON if it parses.
                _ => serde_json::from_str(&text).unwrap_or(Value::String(text)),
            });
        }
    }
    None
}

/// A colour picker for a `Port::widget` colour (`r,g,b` or `#rrggbb` text). Commits when the
/// picker closes, so one pick is one undo step.
fn colour_widget(
    ui: &mut egui::Ui,
    port: &Port,
    key: &EditKey,
    current: &Value,
    buffers: &mut EditBuffers,
    with_tip: &impl Fn(egui::Response) -> egui::Response,
) -> Option<Value> {
    let hex = port.widget.as_deref() == Some(hints::HEX_COLOR);
    let parse = |t: &str| if hex { parse_hex(t) } else { parse_rgb(t) };
    let shown = current.as_str().and_then(parse).or_else(|| port.default.as_str().and_then(parse)).unwrap_or([0.0; 3]);
    let mut c = buffers.rgbs.get(key).copied().unwrap_or(shown);
    let popup_id = ui.auto_id_with("popup");
    let r = with_tip(egui::widgets::color_picker::color_edit_button_rgb(ui, &mut c));
    if r.changed() {
        buffers.rgbs.insert(key.clone(), c);
    }
    if !egui::Popup::is_id_open(ui.ctx(), popup_id)
        && let Some(c) = buffers.rgbs.remove(key)
        && c != shown
    {
        return Some(Value::String(if hex { hex_text(c) } else { rgb_text(c) }));
    }
    None
}

/// `r,g,b` (each a number in 0..1) as three numbers; None unless exactly three parse.
pub fn parse_rgb(text: &str) -> Option<[f32; 3]> {
    let parts: Vec<f32> = text.split(',').map(|p| p.trim().parse::<f32>()).collect::<Result<_, _>>().ok()?;
    let [r, g, b] = parts[..] else { return None };
    Some([r, g, b].map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }))
}

/// Three numbers as `r,g,b`: at most three decimals, no trailing zeros (`0.1,0.15,0.45`).
pub fn rgb_text(c: [f32; 3]) -> String {
    let one = |v: f32| {
        let t = format!("{:.3}", v.clamp(0.0, 1.0));
        let t = t.trim_end_matches('0').trim_end_matches('.');
        if t.is_empty() { "0".to_owned() } else { t.to_owned() }
    };
    c.map(one).join(",")
}

/// `#rrggbb` as three numbers in 0..1.
pub fn parse_hex(text: &str) -> Option<[f32; 3]> {
    let t = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if t.len() != 6 || !t.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(t.get(i..i + 2)?, 16).ok();
    let c = Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?);
    Some([c.r(), c.g(), c.b()].map(|v| f32::from(v) / 255.0))
}

/// Three numbers in 0..1 as `#rrggbb`.
pub fn hex_text(c: [f32; 3]) -> String {
    let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The finite span a slider for `port` covers ([`Port::slider_span`]), if it has one.
pub fn slider_span(port: &Port) -> Option<(f64, f64)> {
    port.slider_span().filter(|(lo, hi)| lo.is_finite() && hi.is_finite())
}

/// Does `port` have the finite span a slider needs?
pub fn has_range(port: &Port) -> bool {
    slider_span(port).is_some()
}

/// A number as a range shows it: whole numbers without a point.
pub fn number_text(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e9 { format!("{}", v as i64) } else { format!("{v}") }
}

/// Width of the faint bound labels at each end of a slider's rail.
const BOUND_W: f32 = 40.0;
/// Width of the typed value beside a slider.
const VALUE_W: f32 = 64.0;

/// A slider for a FLOAT or INT port with a span ([`slider_span`]), as wide as `ui` allows:
/// the span's ends at each end of the rail, and the value typed beside it. The rail covers
/// the soft span; a typed value may go past it, up to the port's min and max (an end with
/// room past it is marked "…"). Commits as [`port_widget`] does: a drag on release, a
/// typed value when the field loses focus. Blank (JSON null) shows "blank" until it is moved.
pub fn port_slider(ui: &mut egui::Ui, port: &Port, key: &EditKey, current: &Value, buffers: &mut EditBuffers, blank_hint: Option<&str>) -> Option<Value> {
    let int = port.ty == scalars::INT;
    let tip = port.tooltip.as_deref().filter(|t| !t.is_empty());
    let (lo, hi) = slider_span(port).unwrap_or((0.0, 1.0));
    let hard = range_f64(port);
    let edited = buffers.numbers.get(key).copied();
    let mut v = edited.or_else(|| current.as_f64()).or_else(|| port.default.as_f64()).unwrap_or(lo);
    let blank = current.is_null() && edited.is_none();
    let d = if int { 0 } else { float_decimals(port, current) };
    let step = port.step.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(if int { 1.0 } else { 0.0 });
    let below = *hard.start() < lo;
    let above = *hard.end() > hi;
    let past = |end: f64| if end.is_finite() { format!("up to {}", number_text(end)) } else { "with no limit".to_owned() };
    let lo_tip = if below {
        format!("The slider starts at {}; type a smaller value, {}", number_text(lo), past(*hard.start()).replace("up to", "down to"))
    } else {
        format!("The least it takes: {}", number_text(lo))
    };
    let hi_tip = if above {
        format!("The slider ends at {}; type a larger value, {}", number_text(hi), past(*hard.end()))
    } else {
        format!("The most it takes: {}", number_text(hi))
    };
    let faint = ui.visuals().weak_text_color();
    let bound = |ui: &mut egui::Ui, text: String, tip: String, layout: egui::Layout| {
        // the full width whatever the text, so the rails and values line up row to row
        ui.allocate_ui_with_layout(egui::vec2(BOUND_W, ui.spacing().interact_size.y), layout, |ui| {
            ui.set_min_width(BOUND_W);
            ui.label(egui::RichText::new(text).small().color(faint)).on_hover_text(tip)
        });
    };
    let lo_text = format!("{}{}", if below { "…" } else { "" }, number_text(lo));
    let hi_text = format!("{}{}", number_text(hi), if above { "…" } else { "" });
    bound(ui, lo_text, lo_tip, egui::Layout::right_to_left(egui::Align::Center));
    // the rail takes the row, less its bound labels, the value field and the clear button
    let clear_w = if port.default.is_null() && !current.is_null() { 24.0 } else { 0.0 };
    ui.spacing_mut().slider_width = (ui.available_width() - BOUND_W - VALUE_W - clear_w - 3.0 * ui.spacing().item_spacing.x).max(60.0);
    let slider = egui::Slider::new(&mut v, lo..=hi).show_value(false).clamping(egui::SliderClamping::Never).step_by(step);
    let mut r = ui.add(slider);
    bound(ui, hi_text, hi_tip, egui::Layout::left_to_right(egui::Align::Center));
    // the typed value: past the rail if wanted, held to the port's own limits
    let speed = if step > 0.0 { step } else { (hi - lo) / 200.0 };
    let mut dv = egui::DragValue::new(&mut v).range(hard).speed(speed).fixed_decimals(d).update_while_editing(false).clamp_existing_to_range(false);
    if blank {
        dv = dv.custom_formatter(|_, _| BLANK.to_owned());
    }
    let mut typed = ui.add_sized([VALUE_W, ui.spacing().interact_size.y], dv);
    if let Some(t) = tip {
        r = r.on_hover_text(t);
        typed = typed.on_hover_text(t);
    }
    if let Some(v) = clear_button(ui, port, current, blank_hint) {
        return Some(v);
    }
    if r.changed() || typed.changed() {
        buffers.numbers.insert(key.clone(), v);
    }
    let editing = r.dragged() || r.has_focus() || typed.dragged() || typed.has_focus();
    if !editing && let Some(v) = buffers.numbers.remove(key) {
        if int {
            let i = v.round() as i64;
            return (current.as_i64() != Some(i)).then_some(Value::from(i));
        }
        if current.as_f64() != Some(v) {
            return serde_json::Number::from_f64(v).map(Value::Number);
        }
    }
    None
}

fn enum_widget(
    ui: &mut egui::Ui,
    port: &Port,
    key: &EditKey,
    current: &Value,
    width: f32,
    with_tip: impl Fn(egui::Response) -> egui::Response,
) -> Option<Value> {
    let choices = port.choices.clone().unwrap_or_default();
    let mut picked = None;
    let id = egui::Id::new(("plumcraft-enum", &key.0, &key.1));
    let r = egui::ComboBox::from_id_salt(id).selected_text(choice_text(current)).width(width).show_ui(ui, |ui| {
        for c in &choices {
            if choice_button(ui, port, c, c == current).clicked() && c != current {
                picked = Some(c.clone());
            }
        }
    });
    with_tip(r.response);
    picked
}

/// Why choice `c` of `port` can't be picked on this machine (`Port::disabled_choices`), or
/// None.
pub fn disabled_reason<'a>(port: &'a Port, c: &Value) -> Option<&'a str> {
    let key = c.as_str().map_or_else(|| c.to_string(), str::to_owned);
    port.disabled_choices.as_ref()?.get(&key).map(String::as_str)
}

/// A choice as a selectable label, greyed out (with its reason on hover) when it can't run
/// here. A disabled choice that is the current value still shows as selected.
pub fn choice_button(ui: &mut egui::Ui, port: &Port, c: &Value, selected: bool) -> egui::Response {
    match disabled_reason(port, c) {
        None => ui.selectable_label(selected, choice_text(c)),
        Some(reason) => {
            ui.add_enabled(false, egui::Button::selectable(selected, choice_text(c))).on_disabled_hover_text(format!("Not available here: {reason}"))
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn port(v: Value) -> Port {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn unavailable_choices_are_disabled_with_their_reason() {
        let p =
            port(json!({"name": "backend", "type": "ENUM", "default": "auto", "choices": ["auto", "cpu", "gpu"], "disabled_choices": {"gpu": "no GPU here"}}));
        assert_eq!(disabled_reason(&p, &json!("gpu")), Some("no GPU here"));
        assert_eq!(disabled_reason(&p, &json!("cpu")), None);
        let plain = port(json!({"name": "x", "type": "ENUM", "choices": ["a"]}));
        assert_eq!(disabled_reason(&plain, &json!("a")), None);
    }

    #[test]
    fn decimals_never_drop_below_the_default() {
        assert_eq!(decimals_of(&json!(0.375)), 3);
        assert_eq!(decimals_of(&json!(2.0)), 1);
        assert_eq!(decimals_of(&json!(7)), 0);
        assert_eq!(decimals_of(&json!("x")), 0);
        let tilesize = port(json!({"name": "tilesize", "type": "FLOAT", "default": 0.375, "decimals": 2}));
        assert_eq!(float_decimals(&tilesize, &json!(0.375)), 3, "the research software's mosaic lesson");
        let sigma = port(json!({"name": "sigma", "type": "FLOAT", "default": 2.0, "decimals": 1}));
        assert_eq!(float_decimals(&sigma, &json!(2.0)), 1);
        assert_eq!(float_decimals(&sigma, &json!(2.25)), 2, "a value set elsewhere isn't rounded either");
        let bare = port(json!({"name": "x", "type": "FLOAT", "default": 3}));
        assert_eq!(float_decimals(&bare, &json!(3)), 1);
        let exact = port(json!({"name": "x", "type": "FLOAT", "default": 1.0, "decimals": 4}));
        assert_eq!(float_decimals(&exact, &json!(1.0)), 4);
    }

    /// M3 regression: a blank number whose minimum is above 0 was clamped by DragValue to its
    /// minimum on the first frame and committed (Image Trace's `colors` became 2, overriding
    /// the preset, and the workflow turned dirty without an edit).
    #[test]
    fn a_blank_number_above_zero_stays_blank() {
        let ctx = egui::Context::default();
        let mut buffers = EditBuffers::default();
        for p in [
            port(json!({"name": "colors", "type": "INT", "min": 2.0, "max": 256.0, "optional": true})),
            port(json!({"name": "gain", "type": "FLOAT", "min": 0.5, "max": 4.0, "optional": true})),
            port(json!({"name": "neg", "type": "INT", "min": -9.0, "max": -3.0})),
        ] {
            let key: EditKey = (NodeId::from("n"), p.name.clone());
            for _ in 0..3 {
                let mut set = None;
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| set = port_widget(ui, &p, &key, &Value::Null, &mut buffers, None));
                out.textures_delta.clear();
                assert_eq!(set, None, "{}: nothing committed", p.name);
            }
            assert!(buffers.ints.is_empty() && buffers.numbers.is_empty(), "{}", p.name);
        }
    }

    #[test]
    fn colours_read_and_write_both_ways() {
        assert_eq!(rgb_text(parse_rgb(" 0.1, 0.15,0.45 ").unwrap()), "0.1,0.15,0.45");
        assert_eq!(parse_rgb("1,2"), None);
        assert_eq!(parse_rgb("x,0,0"), None);
        assert_eq!(hex_text(parse_hex("#ff8000").unwrap()), "#ff8000");
        assert_eq!(parse_hex("#ff80"), None);
        assert_eq!(parse_hex("#éééé"), None);
    }

    #[test]
    fn a_slider_needs_a_finite_span() {
        let p = |min, max, soft_max| Port { min, max, soft_max, ..Default::default() };
        assert_eq!(slider_span(&p(Some(0.0), Some(10.0), Some(2.0))), Some((0.0, 2.0)));
        assert!(has_range(&p(Some(0.0), None, Some(2.0))), "a soft end closes an open one");
        assert!(!has_range(&p(Some(0.0), None, None)));
        assert!(!has_range(&p(Some(5.0), Some(5.0), None)));
    }

    #[test]
    fn a_setting_not_read_now_says_when_it_is() {
        let when = std::collections::BTreeMap::from([("mode".to_owned(), vec![serde_json::json!("a"), serde_json::json!("b")])]);
        let p = Port { active_when: Some(when), ..Default::default() };
        assert_eq!(inactive_note(&p, |_| Some(serde_json::json!("a"))), None);
        assert_eq!(inactive_note(&p, |_| None), None, "a setting the node lacks counts as met");
        assert_eq!(inactive_note(&p, |_| Some(serde_json::json!("c"))).unwrap(), "Not used now: read only when mode is a or b.");
    }

    #[test]
    fn texts() {
        assert_eq!(choice_text(&json!("")), "(blank)");
        assert_eq!(choice_text(&json!("Lab")), "Lab");
        assert_eq!(choice_text(&json!(3)), "3");
        assert_eq!(value_text(&json!(7.5)), "7.5");
        assert_eq!(value_text(&Value::Null), "—");
        assert!(range_f64(&port(json!({"name": "x", "type": "FLOAT", "min": 5.0, "max": 1.0}))).contains(&3.0));
        assert!(has_editor(&port(json!({"name": "s", "type": "STRING"}))));
        assert!(has_editor(&port(json!({"name": "c", "type": "x.pack.CHOICE", "choices": ["a"]}))));
        assert!(!has_editor(&port(json!({"name": "a", "type": "ARRAY"}))));
        assert!(!has_editor(&port(json!({"name": "c", "type": "COLOR"}))), "no pack type is known to core");
    }
}
