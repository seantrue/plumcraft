//! The Parameters window (`view.params`, View › Parameters…): the graph's named ports in order
//! (docs/research/23-named-ports.md 3), each with its port, value and doc line, a jump to its
//! node, rename, remove and reorder. Every change is a command, so the window, the canvas's
//! input menu, the control channel and scripts share one path.

use plumcraft_session::Session;
use serde_json::{Value, json};

use crate::canvas::Action;

/// What the window asks for this frame: commands to run, and a name editor for a rename
/// (the canvas's, at `pos`).
#[derive(Default)]
pub struct Asks {
    pub actions: Vec<Action>,
    pub rename: Option<(String, egui::Pos2)>,
}

pub fn show(ctx: &egui::Context, s: &mut Session, docs: &mut std::collections::HashMap<String, String>) -> Asks {
    let mut asks = Asks::default();
    if !s.views.graph_params {
        return asks;
    }
    let rows = plumcraft_session::named_cmds::params(s.graph(), s.catalog());
    let names: Vec<String> = rows.iter().filter_map(|r| r["name"].as_str().map(str::to_owned)).collect();
    docs.retain(|k, _| names.contains(k));
    let mut open = true;
    egui::Window::new("Parameters").id(egui::Id::new("plumcraft-graph-params")).collapsible(false).resizable(true).default_width(420.0).open(&mut open).show(
        ctx,
        |ui| {
            if rows.is_empty() {
                ui.label("This graph names no ports. Right-click an input › Name This Port… to make it one: a runner then sets it with --set NAME=VALUE.");
                return;
            }
            ui.label(egui::RichText::new("Set from outside with run --set NAME=VALUE").small());
            ui.separator();
            for (i, r) in rows.iter().enumerate() {
                let name = r["name"].as_str().unwrap_or_default().to_owned();
                let port = r["port"].as_str().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&name).monospace().strong());
                    ui.label(egui::RichText::new(format!("{port} · {}", r["type"].as_str().unwrap_or("?"))).small());
                    ui.label(egui::RichText::new(format!("= {}", short(&r["value"]))).small());
                });
                ui.horizontal(|ui| {
                    // the graph's doc line, except while it is being typed (undo and renames show)
                    let id = egui::Id::new(("plumcraft-param-doc", &name));
                    let doc = docs.entry(name.clone()).or_default();
                    if !ui.memory(|m| m.has_focus(id)) {
                        *doc = r["doc"].as_str().unwrap_or_default().to_owned();
                    }
                    let edit = ui.add(egui::TextEdit::singleline(doc).id(id).hint_text("what it is").desired_width(220.0));
                    if edit.lost_focus() && doc.as_str() != r["doc"].as_str().unwrap_or_default() {
                        asks.actions.push(("graph.port.doc", json!({"name": name, "doc": doc.clone()})));
                    }
                    if ui.small_button("Go to").on_hover_text("Open its node's parameters").clicked() {
                        let node = port.split('.').next().unwrap_or_default();
                        asks.actions.push(("node.parameters", json!({"node": node})));
                    }
                    if ui.small_button("Rename…").clicked() {
                        asks.rename = Some((name.clone(), ui.min_rect().left_bottom()));
                    }
                    if ui.small_button("Remove").on_hover_text("The port stops being a parameter; its value stays").clicked() {
                        asks.actions.push(("graph.port.unname", json!({"name": name})));
                    }
                    let moved = |to: usize| {
                        let mut order = names.clone();
                        let n = order.remove(i);
                        order.insert(to, n);
                        ("graph.port.reorder", json!({"names": order}))
                    };
                    if ui.add_enabled(i > 0, egui::Button::new("↑").small()).clicked() {
                        asks.actions.push(moved(i - 1));
                    }
                    if ui.add_enabled(i + 1 < names.len(), egui::Button::new("↓").small()).clicked() {
                        asks.actions.push(moved(i + 1));
                    }
                });
                ui.add_space(4.0);
            }
        },
    );
    if !open {
        s.views.graph_params = false;
    }
    asks
}

fn short(v: &Value) -> String {
    let t = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if t.chars().count() > 40 { format!("{}…", t.chars().take(40).collect::<String>()) } else { t }
}
