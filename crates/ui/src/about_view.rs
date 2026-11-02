//! The About window (`app.about`, Help › About): what the application is, what it was built from,
//! and what the connected engine says of itself. All of it comes from the application and the
//! catalogue, so PlumImpress's revisions are its own and core names none.

use plumcraft_session::Session;

/// The window's title: `About PlumImpress`.
pub fn title(s: &Session) -> String {
    format!("About {}", s.about.title)
}

fn rows(ui: &mut egui::Ui, id: &str, rows: &[(&str, &str)]) {
    egui::Grid::new(id).num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
        for (name, value) in rows {
            ui.weak(*name);
            ui.label(*value);
            ui.end_row();
        }
    });
}

/// Draw it, while it is open.
pub fn show(ctx: &egui::Context, s: &mut Session) {
    if !s.views.about {
        return;
    }
    let mut open = true;
    let mut closed = false;
    egui::Window::new(title(s))
        .id(egui::Id::new("plumcraft-about"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.heading(&s.about.title);
            ui.label(format!("Version {}", s.about.version));
            if !s.about.parts.is_empty() {
                ui.separator();
                let parts: Vec<(&str, &str)> = s.about.parts.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
                rows(ui, "plumcraft-about-parts", &parts);
            }
            let engine = &s.catalog().engine;
            if !engine.name.is_empty() {
                ui.separator();
                ui.label(format!("Engine: {} {}", engine.name, engine.version));
                // what the application hasn't said already
                let shown = |name: &str| s.about.parts.iter().any(|(n, _)| n == name);
                let parts: Vec<(&str, &str)> = engine.components.iter().filter(|(n, _)| !shown(n)).map(|(n, v)| (n.as_str(), v.as_str())).collect();
                rows(ui, "plumcraft-about-engine", &parts);
            }
            ui.separator();
            closed = ui.button("Close").clicked();
        });
    if !open || closed {
        s.views.about = false;
    }
}
