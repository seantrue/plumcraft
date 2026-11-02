//! The parameter editor's widgets ([`crate::params_view`]), with the room a node lacks: a
//! switch for on/off, a segmented row for a few choices, a searchable list for many, chips
//! for several picks, and a "?" whose hover holds the help.
//!
//! Each returns a value to commit (one `node.set`, one undo step), as [`crate::widgets`] do.

use egui::{Align2, FontId, Response, Sense, Stroke, StrokeKind, Vec2};
use plumcraft_proto::catalogue::Port;
use serde_json::Value;

use crate::theme::Tokens;
use crate::widgets::{choice_button, choice_text, disabled_reason};

/// Up to this many choices show as a segmented row; more get a searchable list.
pub const SEGMENTED_CHOICES: usize = 5;

/// More picks than this get a filter above them.
const FILTERED_PICKS: usize = 16;

/// A "?" in a ring; hovering it shows `help`. Nothing when `help` is empty.
pub fn help_icon(ui: &mut egui::Ui, tokens: &Tokens, help: &str) -> Option<Response> {
    if help.trim().is_empty() {
        return None;
    }
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
    let color = if r.hovered() { tokens.accent } else { tokens.text_faint };
    ui.painter().circle_stroke(rect.center(), 6.5, Stroke::new(1.2, color));
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, "?", FontId::proportional(10.5), color);
    Some(r.on_hover_ui(|ui| {
        ui.set_max_width(380.0);
        ui.label(help);
    }))
}

/// An on/off switch: a pill whose knob slides across, in the accent colour when on.
pub fn toggle(ui: &mut egui::Ui, tokens: &Tokens, on: &mut bool) -> Response {
    let size = egui::vec2(34.0, 18.0);
    let (rect, mut r) = ui.allocate_exact_size(size, Sense::click());
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(r.id, *on);
    let radius = rect.height() / 2.0;
    let fill = if *on { tokens.accent } else { tokens.field };
    let painter = ui.painter();
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(rect, radius, Stroke::new(1.0, tokens.field_border), StrokeKind::Inside);
    let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    painter.circle_filled(egui::pos2(x, rect.center().y), radius - 3.0, tokens.text);
    r
}

/// A few choices as one segmented row; the current one selected. Disabled choices are greyed
/// out with their reason.
pub fn segmented(ui: &mut egui::Ui, tokens: &Tokens, port: &Port, choices: &[Value], current: &Value) -> Option<Value> {
    let mut picked = None;
    egui::Frame::new().fill(tokens.field).stroke(Stroke::new(1.0, tokens.field_border)).corner_radius(6).inner_margin(2).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for c in choices {
                if choice_button(ui, port, c, c == current).clicked() && c != current {
                    picked = Some(c.clone());
                }
            }
        });
    });
    picked
}

/// Many choices: a dropdown whose list has a filter at the top.
pub fn select(ui: &mut egui::Ui, id: egui::Id, port: &Port, choices: &[Value], current: &Value, width: f32) -> Option<Value> {
    let filter_id = id.with("filter");
    let mut picked = None;
    let r = egui::ComboBox::from_id_salt(id)
        .selected_text(choice_text(current))
        .width(width)
        .height(340.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
            let field = ui.add(egui::TextEdit::singleline(&mut filter).hint_text(format!("Filter {} choices…", choices.len())).desired_width(f32::INFINITY));
            if !field.has_focus() && filter.is_empty() && !ui.ctx().memory(|m| m.focused().is_some()) {
                field.request_focus();
            }
            ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
            ui.separator();
            let needle = filter.to_lowercase();
            let mut any = false;
            for c in choices.iter().filter(|c| needle.is_empty() || choice_text(c).to_lowercase().contains(&needle)) {
                any = true;
                if choice_button(ui, port, c, c == current).clicked() {
                    if c != current {
                        picked = Some(c.clone());
                    }
                    ui.close();
                }
            }
            if !any {
                ui.weak("No choice matches");
            }
        });
    if picked.is_some() || r.inner.is_none() {
        // closed: the next opening starts unfiltered
        ui.data_mut(|d| d.remove::<String>(filter_id));
    }
    picked
}

/// The picks in a comma-separated text, trimmed, blanks left out.
pub fn picks(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

/// Several picks from `choices` (`Port::widget` "multichoice"), kept as comma-separated text
/// in the order they were picked (Vary Choices runs them in that order); a new pick goes last.
/// Each choice is a chip that toggles; a pick that is no longer a choice stays, marked. No
/// picks is blank, which `blank` describes ("every choice").
pub fn multichoice(ui: &mut egui::Ui, id: egui::Id, tokens: &Tokens, port: &Port, choices: &[Value], current: &Value, blank: &str) -> Option<Value> {
    let names: Vec<String> = choices.iter().map(choice_text).collect();
    let picked = picks(current.as_str().unwrap_or_default());
    let mut on: Vec<bool> = names.iter().map(|n| picked.contains(n)).collect();
    let unknown: Vec<&String> = picked.iter().filter(|p| !names.contains(p)).collect();
    let mut changed = false;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let count = on.iter().filter(|b| **b).count();
            let summary = if count == 0 { blank.to_owned() } else { format!("{count} of {} picked", names.len()) };
            ui.label(egui::RichText::new(summary).small().color(tokens.text_dim));
            if ui.small_button("All").on_hover_text("Pick every choice").clicked() {
                on.iter_mut().for_each(|b| *b = true);
                changed = true;
            }
            if count > 0 && ui.small_button("Clear").on_hover_text(format!("Pick none: {blank}")).clicked() {
                on.iter_mut().for_each(|b| *b = false);
                changed = true;
            }
        });
        let filter_id = id.with("filter");
        let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
        if names.len() > FILTERED_PICKS {
            ui.add(egui::TextEdit::singleline(&mut filter).hint_text(format!("Filter {} choices…", names.len())).desired_width(f32::INFINITY));
            ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
        }
        let needle = filter.to_lowercase();
        let chips = |ui: &mut egui::Ui, on: &mut Vec<bool>, changed: &mut bool| {
            ui.horizontal_wrapped(|ui| {
                for (i, (c, name)) in choices.iter().zip(&names).enumerate() {
                    if !needle.is_empty() && !name.to_lowercase().contains(&needle) {
                        continue;
                    }
                    let Some(slot) = on.get_mut(i) else { continue };
                    let chip = egui::Button::selectable(*slot, name.as_str()).corner_radius(9);
                    let r = match disabled_reason(port, c) {
                        None => ui.add(chip),
                        Some(why) => ui.add_enabled(false, chip).on_disabled_hover_text(format!("Not available here: {why}")),
                    };
                    if r.clicked() {
                        *slot = !*slot;
                        *changed = true;
                    }
                }
                for u in &unknown {
                    ui.label(egui::RichText::new(format!("{u}?")).color(tokens.error)).on_hover_text("Picked, but not one of the choices now");
                }
            });
        };
        if names.len() > FILTERED_PICKS {
            egui::ScrollArea::vertical().id_salt(id.with("scroll")).max_height(180.0).show(ui, |ui| chips(ui, &mut on, &mut changed));
        } else {
            chips(ui, &mut on, &mut changed);
        }
    });
    if !changed {
        return None;
    }
    Some(Value::String(repick(&picked, &names, &on).join(", ")))
}

/// The picks after toggling: those still on (or not choices at all) in their old order, then
/// the newly picked in the choices' order.
fn repick(picked: &[String], names: &[String], on: &[bool]) -> Vec<String> {
    let is_on = |n: &String| names.iter().zip(on).any(|(m, b)| m == n && *b);
    let mut out: Vec<String> = picked.iter().filter(|p| !names.contains(p) || is_on(p)).cloned().collect();
    out.extend(names.iter().zip(on).filter(|(n, b)| **b && !picked.contains(n)).map(|(n, _)| n.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_keeps_the_order_picks_were_made_in() {
        let names: Vec<String> = ["BALLPOINT", "FLAT", "PEN", "SUMI"].map(String::from).to_vec();
        let picked: Vec<String> = ["PEN", "BALLPOINT", "OLD"].map(String::from).to_vec();
        // SUMI picked, BALLPOINT dropped: PEN stays first, OLD (no longer a choice) stays
        assert_eq!(repick(&picked, &names, &[false, false, true, true]), ["PEN", "OLD", "SUMI"]);
        assert_eq!(repick(&[], &names, &[true, false, false, true]), ["BALLPOINT", "SUMI"]);
    }

    #[test]
    fn picks_are_trimmed_and_blanks_dropped() {
        assert_eq!(picks(" PEN, BALLPOINT ,, FLAT,"), ["PEN", "BALLPOINT", "FLAT"]);
        assert!(picks("").is_empty());
    }
}
