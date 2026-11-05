//! The Settings window (`app.settings`, File › Settings…, ⌘,): Appearance (theme, text size,
//! zoom, colours with their contrast, fruits, pinned families, theme files), Engines (Pick
//! Engines Automatically) and Cache (the Cache Report). Every control dispatches a command, so the
//! control channel and MCP can do all of it: the session checks a command's shape and leaves it
//! in `views.ui_asks`, and [`EditorApp::window_command`] carries it out here, where the themes are.

use std::io::Read;

use egui::{Color32, RichText};
use plumcraft_session::settings_cmds::TABS;
use serde_json::{Value, json};

use crate::EditorApp;
use crate::appearance::{Appearance, MAX_THEME_BYTES};
use crate::canvas::Action;
use crate::theme::{self, FRUITS, OLIVE, PLUM, TOKENS, Tokens};

/// What the window keeps that isn't a setting.
#[derive(Debug, Default)]
pub struct SettingsUi {
    /// A text size or zoom being dragged, sent when it is let go.
    text_size: Option<f32>,
    zoom: Option<f32>,
    /// Import… or Export… was clicked: the dialog opens after the window is drawn.
    import: bool,
    export: bool,
}

fn tab_label(tab: &str) -> &'static str {
    match tab {
        "engines" => "Engines",
        "cache" => "Cache",
        _ => "Appearance",
    }
}

/// `p[key]` as a theme's name: the current one when it's left out.
fn theme_of(p: &Value, current: &'static str) -> Result<&'static str, String> {
    match p.get("theme").and_then(Value::as_str) {
        None => Ok(current),
        Some(n) => theme::by_name(n).map(|t| t.name).ok_or_else(|| format!("no theme called {:?}", n.chars().take(64).collect::<String>())),
    }
}

fn color_param(p: &Value) -> Option<&str> {
    p.get("color").and_then(Value::as_str)
}

impl EditorApp {
    /// After a command ran: carry out what it left for the window (the look), and fill in
    /// `app.settings`'s reply. Anything else passes `v` through.
    pub(crate) fn window_command(&mut self, id: &str, v: Value) -> Result<Value, String> {
        let mut out = v;
        for (ask, p) in std::mem::take(&mut self.session.views.ui_asks) {
            out = self.appearance_command(&ask, &p)?;
        }
        if id == "app.settings"
            && let (Some(o), Value::Object(more)) = (out.as_object_mut(), self.settings_state())
        {
            o.extend(more);
        }
        Ok(out)
    }

    /// The settings as `app.settings` returns them.
    pub fn settings_state(&self) -> Value {
        let a = &self.prefs.prefs().appearance;
        let below: Vec<Value> =
            self.tokens.checks().into_iter().filter(|c| !c.passes()).map(|c| json!({"fg": c.fg, "bg": c.bg, "ratio": c.ratio, "need": c.need})).collect();
        json!({
            "theme": self.tokens.name,
            "themes": theme::ALL.iter().map(|t| json!({"name": t.name, "label": t.label})).collect::<Vec<_>>(),
            "appearance": a.summary(),
            "contrast": {"pairs": self.tokens.checks().len(), "below": below},
            "autoEngines": self.session.engines.auto,
            "cacheReport": self.session.views.cache_report,
        })
    }

    fn change_appearance(&mut self, f: impl FnOnce(&mut Appearance)) {
        let mut a = self.prefs.prefs().appearance.clone();
        f(&mut a);
        self.prefs.set_appearance(a);
        self.retheme();
    }

    /// One appearance command, its parameters already checked for shape by the session.
    fn appearance_command(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        let current = self.tokens.name;
        match id {
            "view.textSize" => {
                let size = p.get("size").and_then(Value::as_f64).map(|s| s as f32);
                self.change_appearance(|a| a.text_size = size);
                Ok(json!({"textSize": self.tokens.text_size}))
            }
            "view.zoom" => {
                let zoom = p.get("zoom").and_then(Value::as_f64).map(|z| z as f32);
                self.change_appearance(|a| a.zoom = zoom);
                Ok(json!({"zoom": zoom.unwrap_or(1.0)}))
            }
            "theme.setToken" => {
                let theme = theme_of(p, current)?;
                let token = p.get("token").and_then(Value::as_str).unwrap_or_default();
                if theme::PAPER.token(token).is_none() {
                    let names: Vec<&str> = TOKENS.iter().flat_map(|(_, t)| t.iter().copied()).collect();
                    return Err(format!("no token called {:?}; there are {}", token.chars().take(64).collect::<String>(), names.join(", ")));
                }
                let color = color_param(p);
                self.change_appearance(|a| a.set_token(theme, token, color));
                let t = self.prefs.prefs().appearance.resolve(theme);
                let checks: Vec<Value> = t
                    .checks_of(token)
                    .into_iter()
                    .map(|c| json!({"fg": c.fg, "bg": c.bg, "ratio": (c.ratio * 100.0).round() / 100.0, "need": c.need}))
                    .collect();
                Ok(json!({"theme": theme, "token": token, "color": t.token(token).map(theme::hex), "checks": checks}))
            }
            "theme.setFruit" => {
                let theme = theme_of(p, current)?;
                let fruit = p.get("fruit").and_then(Value::as_str).unwrap_or_default();
                if theme::PAPER.fruit(fruit).is_none() {
                    return Err(format!(
                        "no fruit called {:?}; there are {}, {OLIVE} and {PLUM}",
                        fruit.chars().take(64).collect::<String>(),
                        FRUITS.join(", ")
                    ));
                }
                let color = color_param(p);
                self.change_appearance(|a| a.set_fruit(theme, fruit, color));
                let t = self.prefs.prefs().appearance.resolve(theme);
                Ok(json!({"theme": theme, "fruit": fruit, "color": t.fruit(fruit).map(theme::hex)}))
            }
            "theme.pinFamily" => {
                let family = p.get("family").and_then(Value::as_str).unwrap_or_default();
                let to = p.get("to").and_then(Value::as_str);
                if let Some(to) = to
                    && self.tokens.fruit(to).is_none()
                    && theme::parse_hex(to).is_none()
                {
                    return Err(format!(
                        "{:?} is neither a fruit ({}, {OLIVE}, {PLUM}) nor a #rrggbb",
                        to.chars().take(64).collect::<String>(),
                        FRUITS.join(", ")
                    ));
                }
                self.change_appearance(|a| a.pin(family, to));
                let header = self.tokens.family_tint(self.session.catalog(), family).header;
                Ok(json!({"family": family, "to": to, "fruit": theme::hex(header)}))
            }
            "theme.reset" => {
                let theme = theme_of(p, current)?;
                self.change_appearance(|a| a.reset(theme));
                Ok(json!({"theme": theme}))
            }
            "theme.import" => {
                let path = p.get("path").and_then(Value::as_str).unwrap_or_default();
                let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
                let mut text = String::new();
                file.take(MAX_THEME_BYTES as u64 + 1).read_to_string(&mut text).map_err(|e| format!("{path}: {e}"))?;
                let mut a = self.prefs.prefs().appearance.clone();
                let (base, skipped) = a.import(&text)?;
                self.prefs.set_appearance(a);
                self.prefs.set_theme(base);
                self.tokens.name = base;
                self.retheme();
                Ok(json!({"theme": base, "skipped": skipped}))
            }
            "theme.export" => {
                let theme = theme_of(p, current)?;
                let path = p.get("path").and_then(Value::as_str).unwrap_or_default();
                let text = serde_json::to_string_pretty(&self.prefs.prefs().appearance.export(theme)).map_err(|e| e.to_string())?;
                plumcraft_session::file_cmds::write_atomic(std::path::Path::new(path), &text).map_err(|e| format!("{path}: {e}"))?;
                Ok(json!({"path": path, "theme": theme}))
            }
            other => Err(format!("{other} is not one of the Settings window's commands")),
        }
    }

    /// Draw the Settings window while it is open; what its controls ask for is returned.
    pub(crate) fn settings_window(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = Vec::new();
        let Some(tab) = self.session.views.settings.clone() else { return out };
        let mut open = true;
        egui::Window::new("Settings")
            .id(egui::Id::new("plumcraft-settings"))
            .collapsible(false)
            .resizable(true)
            .default_size([560.0, 600.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for t in TABS {
                        if ui.selectable_label(tab == *t, tab_label(t)).clicked() && tab != *t {
                            out.push(("app.settings", json!({"tab": t})));
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| match tab.as_str() {
                    "engines" => self.engines_tab(ui, &mut out),
                    "cache" => self.cache_tab(ui, &mut out),
                    _ => self.appearance_tab(ui, &mut out),
                });
            });
        if !open {
            out.push(("app.settings", json!({"open": false})));
        }
        if std::mem::take(&mut self.settings_ui.import)
            && let Some(path) = self.dialogs.open_theme()
        {
            out.push(("theme.import", json!({"path": path.display().to_string()})));
        }
        if std::mem::take(&mut self.settings_ui.export) {
            let name = format!("{}.plumtheme.json", self.tokens.name);
            if let Some(path) = self.dialogs.save_theme(&name) {
                out.push(("theme.export", json!({"path": path.display().to_string()})));
            }
        }
        out
    }

    fn engines_tab(&self, ui: &mut egui::Ui, out: &mut Vec<Action>) {
        let mut auto = self.session.engines.auto;
        let tip = "Start the engines a workflow needs when it is opened or inserted, each hosted the fastest way it can be: in-process (mode 1), then a separate process, then HTTP";
        if ui.checkbox(&mut auto, "Pick Engines Automatically").on_hover_text(tip).changed() {
            out.push(("engine.auto", json!({"on": auto})));
        }
        ui.label(RichText::new(tip).color(self.tokens.text_dim));
        ui.add_space(8.0);
        ui.label(RichText::new("The Engine menu starts, attaches and installs engines one at a time.").color(self.tokens.text_dim));
    }

    fn cache_tab(&self, ui: &mut egui::Ui, out: &mut Vec<Action>) {
        ui.label("What the caches did: the latest run, this session, each engine and each node.");
        ui.add_space(4.0);
        let label = if self.session.views.cache_report { "Close Cache Report" } else { "Open Cache Report" };
        if ui.button(label).clicked() {
            out.push(("run.cacheReport", json!({"open": !self.session.views.cache_report})));
        }
        if self.session.run.cache.last.is_none() {
            ui.label(RichText::new("No run has reported yet.").color(self.tokens.text_dim));
        }
    }

    fn appearance_tab(&mut self, ui: &mut egui::Ui, out: &mut Vec<Action>) {
        let t = self.tokens.clone();
        let a = self.prefs.prefs().appearance.clone();
        ui.heading("Theme");
        ui.horizontal_wrapped(|ui| {
            for th in &theme::ALL {
                let shown = a.resolve(th.name);
                let r = theme_card(ui, &shown, th.name == t.name);
                if r.clicked() && th.name != t.name {
                    out.push(("view.theme", json!({"name": th.name})));
                }
            }
        });
        ui.add_space(8.0);
        ui.heading("Text");
        egui::Grid::new("plumcraft-settings-text").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Text size");
            let mut size = self.settings_ui.text_size.unwrap_or(t.text_size);
            let r = ui.add(egui::Slider::new(&mut size, theme::TEXT_SIZES).step_by(0.5).max_decimals(1).suffix(" pt"));
            if r.dragged() {
                self.settings_ui.text_size = Some(size);
            } else if r.drag_stopped() || r.changed() {
                self.settings_ui.text_size = None;
                out.push(("view.textSize", json!({"size": size})));
            }
            ui.end_row();
            ui.label("Zoom");
            let mut zoom = self.settings_ui.zoom.unwrap_or(a.zoom.unwrap_or(1.0));
            let r = ui.add(egui::Slider::new(&mut zoom, theme::ZOOMS).step_by(0.05).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
            if r.dragged() {
                self.settings_ui.zoom = Some(zoom);
            } else if r.drag_stopped() || r.changed() {
                self.settings_ui.zoom = None;
                out.push(("view.zoom", json!({"zoom": zoom})));
            }
            ui.end_row();
        });
        if (a.text_size.is_some() || a.zoom.is_some()) && ui.button("Default text size and zoom").clicked() {
            out.push(("view.textSize", json!({"size": null})));
            out.push(("view.zoom", json!({"zoom": null})));
        }
        ui.add_space(8.0);
        let overridden = a.overrides.get(t.name).cloned().unwrap_or_default();
        let changed_fruits = a.fruits.get(t.name).cloned().unwrap_or_default();
        egui::CollapsingHeader::new(RichText::new(format!("Colours of {}", t.label)).strong()).id_salt("plumcraft-settings-colours").show(ui, |ui| {
            for (group, names) in TOKENS {
                ui.label(RichText::new(*group).color(t.text_dim));
                egui::Grid::new(("plumcraft-settings-tokens", *group)).num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
                    for name in *names {
                        let Some(mut c) = t.token(name) else { continue };
                        if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                            out.push(("theme.setToken", json!({"token": name, "color": theme::hex(c)})));
                        }
                        ui.label(*name);
                        contrast_badge(ui, &t, name);
                        if overridden.contains_key(*name) && ui.small_button("Reset").clicked() {
                            out.push(("theme.setToken", json!({"token": name, "color": null})));
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
            }
        });
        egui::CollapsingHeader::new(RichText::new("Node families").strong()).id_salt("plumcraft-settings-fruits").show(ui, |ui| {
            ui.label(RichText::new("Each family takes one of twelve fruits; olive is the quiet one, plum is the groups'.").color(t.text_dim));
            egui::Grid::new("plumcraft-settings-fruit-grid").num_columns(4).spacing([10.0, 4.0]).show(ui, |ui| {
                for (i, name) in FRUITS.iter().chain([OLIVE, PLUM].iter()).enumerate() {
                    let Some(mut c) = t.fruit(name) else { continue };
                    ui.horizontal(|ui| {
                        if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                            out.push(("theme.setFruit", json!({"fruit": name, "color": theme::hex(c)})));
                        }
                        let text = if changed_fruits.contains_key(*name) { format!("{name} •") } else { (*name).to_owned() };
                        ui.label(text);
                    });
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
            let families: std::collections::BTreeSet<String> =
                self.session.catalog().nodes.iter().map(|n| n.family.clone()).filter(|f| !f.is_empty()).collect();
            if !families.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new("This catalogue's families").color(t.text_dim));
                egui::Grid::new("plumcraft-settings-families").num_columns(3).spacing([10.0, 4.0]).show(ui, |ui| {
                    for f in &families {
                        let tint = t.family_tint(self.session.catalog(), f);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 16.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 3.0, tint.header);
                        ui.label(f);
                        let pinned = a.families.get(f).cloned();
                        let own = t.family_fruit(self.session.catalog(), f).unwrap_or("default");
                        let shown = pinned.clone().unwrap_or_else(|| format!("its own ({own})"));
                        egui::ComboBox::from_id_salt(("plumcraft-pin", f.as_str())).selected_text(shown).show_ui(ui, |ui| {
                            if ui.selectable_label(pinned.is_none(), format!("its own ({own})")).clicked() {
                                out.push(("theme.pinFamily", json!({"family": f, "to": null})));
                            }
                            for fruit in FRUITS.iter().chain([OLIVE].iter()) {
                                if ui.selectable_label(pinned.as_deref() == Some(*fruit), *fruit).clicked() {
                                    out.push(("theme.pinFamily", json!({"family": f, "to": fruit})));
                                }
                            }
                        });
                        ui.end_row();
                    }
                });
            }
        });
        egui::CollapsingHeader::new(RichText::new("Theme files").strong()).id_salt("plumcraft-settings-files").show(ui, |ui| {
            ui.label(RichText::new("A .plumtheme.json holds a theme's changes from its built-in colours.").color(t.text_dim));
            ui.horizontal(|ui| {
                self.settings_ui.import |= ui.button("Import…").clicked();
                self.settings_ui.export |= ui.button("Export…").clicked();
                if (!overridden.is_empty() || !changed_fruits.is_empty()) && ui.button(format!("Reset {}", t.label)).clicked() {
                    out.push(("theme.reset", json!({})));
                }
            });
        });
    }
}

/// A theme's card: a small picture of a node on its canvas, and its name.
fn theme_card(ui: &mut egui::Ui, t: &Tokens, selected: bool) -> egui::Response {
    let size = egui::vec2(132.0, 84.0);
    let (rect, r) = ui.allocate_exact_size(size + egui::vec2(0.0, 22.0), egui::Sense::click());
    let p = ui.painter();
    let pic = egui::Rect::from_min_size(rect.min, size);
    p.rect_filled(pic, 6.0, t.canvas);
    p.rect_filled(egui::Rect::from_min_size(pic.min, egui::vec2(26.0, size.y)), egui::CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, t.panel);
    for (i, fruit) in ["blueberry", "pear", "fig"].iter().enumerate() {
        let at = pic.min + egui::vec2(34.0 + i as f32 * 8.0, 10.0 + i as f32 * 22.0);
        let node = egui::Rect::from_min_size(at, egui::vec2(80.0, 18.0));
        let header = t.fruit(fruit).unwrap_or(t.family_default.header);
        p.rect_filled(node, 3.0, theme::mix(t.family_default.body, header, t.family_body_mix));
        p.rect_filled(egui::Rect::from_min_size(node.min, egui::vec2(80.0, 8.0)), egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, header);
    }
    let stroke = if selected {
        egui::Stroke::new(2.5, ui.visuals().selection.stroke.color)
    } else {
        egui::Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color)
    };
    p.rect_stroke(pic, 6.0, stroke, egui::StrokeKind::Outside);
    p.text(
        egui::pos2(pic.center().x, pic.max.y + 11.0),
        egui::Align2::CENTER_CENTER,
        t.label,
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    let label = t.label;
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, label));
    r.on_hover_text(format!("Use {}", t.label))
}

/// The worst contrast pair a token is in, as "4.9:1 on panel": dim when it passes with room,
/// amber under 4.5:1, red under what it needs.
fn contrast_badge(ui: &mut egui::Ui, t: &Tokens, token: &str) {
    let checks = t.checks_of(token);
    let Some(worst) = checks.iter().min_by(|a, b| (a.ratio / a.need).total_cmp(&(b.ratio / b.need))) else {
        ui.label("");
        return;
    };
    let other = if worst.fg == token { worst.bg } else { worst.fg };
    let color: Color32 = if !worst.passes() {
        t.error
    } else if worst.ratio < 4.5 {
        t.warning
    } else {
        t.text_dim
    };
    let text = format!("{:.1}:1 with {other}", worst.ratio);
    let tip = format!("needs {}:1 (WCAG 2){}", worst.need, if worst.passes() { "" } else { ": below it" });
    ui.label(RichText::new(text).color(color)).on_hover_text(tip);
}
