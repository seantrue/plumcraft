//! The Acknowledgements window (`app.acknowledgements`, Help › Acknowledgements, or About's
//! button; FR4): the licences of what the editor is built from and of each connected engine, as
//! the core's model gives them (`acknowledgements.get`), searched by the core. Each licence is a
//! section, closed until opened, with its packages and its text, all selectable.

use plumcraft_editor_core::document::Document;
use serde_json::{Value, json};

/// What the window keeps between frames: the search, and the model it last read.
#[derive(Clone, Default)]
struct State {
    query: String,
    /// The query the model was read for, and the model.
    shown: Option<(String, Value)>,
}

/// Draw it, while it is open.
pub fn show(ctx: &egui::Context, doc: &mut Document) {
    let id = egui::Id::new("plumcraft-acknowledgements");
    if !doc.session.views.acknowledgements {
        ctx.data_mut(|d| d.remove::<State>(id));
        return;
    }
    let mut state = ctx.data_mut(|d| d.get_temp::<State>(id)).unwrap_or_default();
    if state.shown.as_ref().is_none_or(|(q, _)| *q != state.query) {
        let model = plumcraft_editor_core::acknowledgements::document_json(doc, &json!({"shell": "egui", "query": state.query}));
        state.shown = Some((state.query.clone(), model));
    }
    let mut open = true;
    let mut closed = false;
    egui::Window::new("Acknowledgements").id(id).collapsible(false).resizable(true).default_size([640.0, 560.0]).open(&mut open).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Search");
            ui.add(egui::TextEdit::singleline(&mut state.query).hint_text("a licence or a package").desired_width(260.0));
        });
        ui.separator();
        if let Some((_, model)) = &state.shown {
            egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 36.0).show(ui, |ui| {
                let name = model["editor"]["name"].as_str().unwrap_or("PlumCraft");
                list(ui, name, &model["editor"]["list"], "editor");
                for e in model["engines"].as_array().into_iter().flatten() {
                    ui.add_space(12.0);
                    let name = format!("Engine: {}", e["name"].as_str().unwrap_or(""));
                    if e["given"].as_bool() == Some(true) {
                        list(ui, &name, &e["list"], e["id"].as_str().unwrap_or(""));
                    } else {
                        ui.heading(name);
                        let why = e["error"].as_str().map_or_else(|| "No list given.".to_owned(), |w| format!("No list given: {w}"));
                        ui.weak(why);
                    }
                }
            });
        }
        ui.separator();
        closed = ui.button("Close").clicked();
    });
    if !open || closed {
        let _ = doc.session.execute("app.acknowledgements", &json!({"open": false}));
    }
    ctx.data_mut(|d| d.insert_temp(id, state));
}

/// One program's list: its name, how many packages, a note, then a section per licence.
fn list(ui: &mut egui::Ui, name: &str, list: &Value, salt: &str) {
    ui.heading(name);
    let all = list["packages"].as_u64().unwrap_or(0);
    let shown = list["shownPackages"].as_u64().unwrap_or(0);
    ui.weak(if shown == all { format!("{all} packages") } else { format!("{shown} of {all} packages") });
    if let Some(note) = list["note"].as_str().filter(|n| !n.is_empty()) {
        ui.add(egui::Label::new(note).selectable(true));
    }
    // a row per licence (the core coalesces its texts): its packages with their own notices, then
    // its text once, and any variant under its packages
    for r in list["rows"].as_array().into_iter().flatten() {
        let licence = r["licence"].as_str().unwrap_or("");
        let packages = r["packages"].as_array().map_or(&[][..], Vec::as_slice);
        // each name once (a crate in two versions is two packages)
        let mut names: Vec<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
        names.dedup();
        let more = if names.len() > 4 { format!(" and {} more", names.len() - 4) } else { String::new() };
        let title = format!("{licence}: {}{more}", names.iter().take(4).copied().collect::<Vec<_>>().join(", "));
        egui::CollapsingHeader::new(title).id_salt((salt, licence)).show(ui, |ui| {
            if let Some(n) = r["name"].as_str().filter(|n| !n.is_empty()) {
                ui.strong(n);
            }
            for p in packages {
                let line = [p["name"].as_str(), p["version"].as_str(), p["url"].as_str()]
                    .into_iter()
                    .flatten()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("  ");
                ui.add(egui::Label::new(line).selectable(true));
                for n in p["notices"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.add(egui::Label::new(egui::RichText::new(n).small().weak()).selectable(true));
                    });
                }
            }
            for t in r["texts"].as_array().into_iter().flatten() {
                ui.add_space(6.0);
                if t["standard"].as_bool() != Some(true) {
                    let who: Vec<&str> = t["packages"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                    ui.strong(format!("As {} have it:", who.join(", ")));
                }
                ui.add(egui::Label::new(egui::RichText::new(t["text"].as_str().unwrap_or("")).monospace().small()).selectable(true).wrap());
            }
        });
    }
}
