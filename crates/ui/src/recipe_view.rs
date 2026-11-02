//! What made the value shown (its recipe, spec 7.6 rule 9, `Display.recipe`): the style, the values
//! that differ from their defaults or were swept, and the settings. An engine that tracks lineage
//! sends it, and writes the same recipe into the files it saves.
//!
//! Lifted from the research software's `crates/ui-egui/src/viewer.rs` (`recipe_ui`). The recipe is the engine's,
//! so anything in it may be missing or the wrong shape: what doesn't fit isn't shown. The Watched
//! panel shares its height with the viewer, so this starts closed.

use egui::RichText;
use serde_json::{Value, json};

use crate::theme::Tokens;

/// A value as the recipe shows it: text without its quotes, anything else as JSON.
fn shown(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_owned)
}

fn row(ui: &mut egui::Ui, key: &str, value: &Value, tokens: &Tokens) {
    ui.label(RichText::new(key).monospace().size(11.0).color(tokens.text_dim));
    ui.add(egui::Label::new(RichText::new(shown(value)).monospace().size(11.0)).truncate());
    ui.end_row();
}

/// The recipe's header text.
pub fn title(recipe: &Value) -> String {
    match recipe.get("style").and_then(Value::as_str) {
        Some(style) => format!("Recipe · {style}"),
        None => "Recipe".to_owned(),
    }
}

/// Show `recipe`. Returns the action its Copy asks for.
pub fn ui(ui: &mut egui::Ui, recipe: &Value, tokens: &Tokens) -> Option<(&'static str, Value)> {
    let mut ask = None;
    egui::CollapsingHeader::new(title(recipe)).id_salt("watched-recipe").default_open(false).show(ui, |ui| {
        match recipe.get("params").and_then(Value::as_object).filter(|p| !p.is_empty()) {
            Some(params) => {
                egui::Grid::new("watched-recipe-params").num_columns(2).striped(true).show(ui, |ui| {
                    for (k, v) in params {
                        row(ui, k, v, tokens);
                    }
                });
            }
            None => {
                ui.weak("Every value is its default.");
            }
        }
        if let Some(settings) = recipe.get("settings").and_then(Value::as_object).filter(|s| !s.is_empty()) {
            egui::CollapsingHeader::new("Settings").id_salt("watched-recipe-settings").show(ui, |ui| {
                egui::Grid::new("watched-recipe-settings-grid").num_columns(2).striped(true).show(ui, |ui| {
                    for (section, values) in settings {
                        for (k, v) in values.as_object().into_iter().flatten() {
                            row(ui, &format!("{section}.{k}"), v, tokens);
                        }
                    }
                });
            });
        }
        if ui.small_button("Copy recipe").on_hover_text("The recipe as JSON: its graph, parameters and settings").clicked() {
            ask = Some(("viewer.recipe", json!({"copy": true})));
        }
    });
    ask
}
