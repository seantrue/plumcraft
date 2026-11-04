//! The parameter editor: one node's inputs in a window of their own, opened from the node's menu
//! (`node.parameters`).
//!
//! Every input is listed, grouped (what is wired in, the parameters, each settings section, and
//! the fields a filling choice doesn't use), with a filter. The editable ones get the widgets a
//! node has no room for ([`crate::param_widgets`]): sliders, switches, segmented choices, a
//! searchable list, chips for several picks, colour pickers. Each label's "?" holds its help.
//! The edits are the same `node.set` commands the node's own widgets send, so the two agree.
//! A port an application draws an editor for ([`crate::value_editors`]) gets its button.
//!
//! Lifted from the research software's `crates/ui-egui/src/params_view.rs`.

use egui::RichText;
use plumcraft_graph::{Graph, NodeId, PortRef, ports};
use plumcraft_proto::catalogue::{NodeDef, Port, SchemaReply, Socket, scalars, texts, widgets as hints};
use plumcraft_session::run::RunState;
use serde_json::{Value, json};

use crate::bundle_view::Source;
use crate::canvas::{Action, InputRow, input_rows};
use crate::param_widgets::{self, SEGMENTED_CHOICES};
use crate::theme::Tokens;
use crate::value_editors;
use crate::widgets::{self, EditBuffers};

/// The window's id: one editor, moved to whichever node it is opened on.
const WINDOW: &str = "plumcraft-params";

/// Rows beyond this get a filter.
const FILTER_ROWS: usize = 8;

/// The width of the reset column.
const RESET_W: f32 = 26.0;

/// The room the header keeps for its Rename… button.
const RENAME_W: f32 = 84.0;

/// What the editor reads.
pub struct Context<'a> {
    pub graph: &'a Graph,
    pub catalog: &'a SchemaReply,
    pub run: &'a RunState,
    pub tokens: &'a Tokens,
    /// The node on screen, when it is: the window opens beside it.
    pub anchor: Option<egui::Rect>,
}

#[derive(Default)]
pub struct ParamsPanel {
    /// Values being edited here (apart from the canvas's, so a field in each can't collide).
    pub edits: EditBuffers,
    /// The node the filter was typed for.
    shown: Option<NodeId>,
    /// The last frame's widget rects (screen) by input key, for tests and the control channel.
    pub rects: Vec<(String, egui::Rect)>,
    /// The filter typed above the rows.
    pub filter: String,
}

/// A group of rows under one heading.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    /// Sockets: what is (or could be) wired in.
    Inputs,
    /// The node's own parameters.
    Parameters,
    /// A settings section (`strokes`, `anneal`).
    Settings(String),
    /// The fields the node's filling choice doesn't use ([`ports::tucked`]).
    More,
}

impl Section {
    fn title(&self) -> String {
        match self {
            Section::Inputs => "Inputs".into(),
            Section::Parameters => "Parameters".into(),
            Section::Settings(s) => {
                let mut c = s.chars();
                c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
            }
            Section::More => "More fields".into(),
        }
    }
}

impl ParamsPanel {
    /// Draw the editor on `node`. Returns the commands to run.
    pub fn show(&mut self, ctx: &egui::Context, cx: &Context<'_>, node: &NodeId) -> Vec<Action> {
        let mut actions = Vec::new();
        self.rects.clear();
        let fresh = self.shown.as_ref() != Some(node);
        if fresh {
            self.filter.clear();
            self.shown = Some(node.clone());
        }
        let Some(def) = cx.graph.node(node).and_then(|n| cx.catalog.node(&n.kind)) else {
            // the node went (deleted, or a new graph): so does the editor
            actions.push(("node.parameters", json!({})));
            return actions;
        };
        let title = plumcraft_session::node_title(cx.graph, cx.catalog, node);
        let mut open = true;
        let content = ctx.content_rect();
        let mut window = egui::Window::new(RichText::new(format!("{title} · parameters")).strong());
        // Opened on a node (or moved to another): beside it, once; after that it stays where it's put.
        if fresh && let Some(at) = cx.anchor.map(|node| beside(node, content)) {
            window = window.current_pos(at);
        }
        window
            .id(egui::Id::new(WINDOW))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([580.0, 660.0])
            .min_width(380.0)
            .max_height(content.height() - 40.0)
            .default_pos(content.right_top() + egui::vec2(-600.0, 60.0))
            .frame(egui::Frame::window(&ctx.global_style()).fill(cx.tokens.panel))
            .show(ctx, |ui| self.body(ui, cx, node, def, &title, &mut actions));
        if !open {
            actions.push(("node.parameters", json!({})));
        }
        actions
    }
}

/// Where the window's top left goes for a node at `node` (screen): right of it, or left when
/// there isn't room on the right, its top level with the node's, kept on screen.
fn beside(node: egui::Rect, content: egui::Rect) -> egui::Pos2 {
    const GAP: f32 = 12.0;
    let size = egui::vec2(580.0, 660.0_f32.min(content.height() - 40.0));
    let right = node.right() + GAP;
    let x = if right + size.x <= content.right() { right } else { (node.left() - GAP - size.x).max(content.left()) };
    let y = node.top().clamp(content.top(), (content.bottom() - size.y).max(content.top()));
    egui::pos2(x, y)
}

impl ParamsPanel {
    fn body(&mut self, ui: &mut egui::Ui, cx: &Context<'_>, node: &NodeId, def: &NodeDef, title: &str, actions: &mut Vec<Action>) {
        header(ui, cx, node, def, title);
        ui.add_space(6.0);
        if let Some(a) = crate::bundle_view::detach_button(ui, cx.graph, cx.catalog, node) {
            actions.push(a);
        }
        let rows = input_rows(def, node, cx.graph);
        if rows.is_empty() {
            ui.weak("This node has no inputs.");
            return;
        }
        if rows.len() > FILTER_ROWS {
            ui.horizontal(|ui| {
                let w = ui.available_width() - 40.0;
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(format!("Filter {} fields…", rows.len())).desired_width(w));
                if !self.filter.is_empty() && ui.small_button("×").on_hover_text("Clear the filter").clicked() {
                    self.filter.clear();
                }
            });
            ui.add_space(4.0);
        }
        let tucked = cx.graph.node(node).map(|n| ports::tucked(def, n, |k| cx.graph.wire_into(&PortRef::new(node.clone(), k)).is_some())).unwrap_or_default();
        let needle = self.filter.to_lowercase();
        let matches = |row: &InputRow| {
            needle.is_empty()
                || row.label.to_lowercase().contains(&needle)
                || row.key.to_lowercase().contains(&needle)
                || row.port.tooltip.as_deref().is_some_and(|t| t.to_lowercase().contains(&needle))
        };
        let mut sections: Vec<(Section, Vec<&InputRow>)> = Vec::new();
        for row in rows.iter().filter(|r| matches(r)) {
            let section = if tucked.contains(&row.key) {
                Section::More
            } else if let Some(s) = &row.port.param_ref {
                Section::Settings(s.group.clone())
            } else if row.port.socket_mode() == Socket::Socket {
                Section::Inputs
            } else {
                Section::Parameters
            };
            match sections.iter_mut().find(|(s, _)| *s == section) {
                Some((_, list)) => list.push(row),
                None => sections.push((section, vec![row])),
            }
        }
        // inputs first, the preset's spare fields last; settings in the node's order
        sections.sort_by_key(|(s, _)| match s {
            Section::Inputs => 0,
            Section::Parameters | Section::Settings(_) => 1,
            Section::More => 2,
        });
        if sections.is_empty() {
            ui.weak(format!("No field matches \"{}\".", self.filter));
            return;
        }
        let single = sections.len() == 1 && sections[0].0 != Section::More;
        egui::ScrollArea::vertical().id_salt("plumcraft-params-scroll").auto_shrink([false, true]).show(ui, |ui| {
            for (section, list) in &sections {
                if single {
                    for row in list {
                        self.row(ui, cx, node, row, actions);
                    }
                    continue;
                }
                let filtering = !needle.is_empty();
                let heading = RichText::new(format!("{}  ({})", section.title(), list.len())).strong().color(cx.tokens.text);
                egui::CollapsingHeader::new(heading)
                    .id_salt(("plumcraft-params-section", node, section.title()))
                    .default_open(*section != Section::More)
                    .open(filtering.then_some(true))
                    .show(ui, |ui| {
                        if *section == Section::More {
                            ui.label(RichText::new("Fields this node's preset doesn't use, at the preset's values.").small().color(cx.tokens.text_faint));
                        }
                        for row in list {
                            self.row(ui, cx, node, row, actions);
                        }
                    });
                ui.add_space(4.0);
            }
        });
    }

    /// One input: its label and "?", what feeds it or its widget, and its reset.
    fn row(&mut self, ui: &mut egui::Ui, cx: &Context<'_>, node: &NodeId, row: &InputRow, actions: &mut Vec<Action>) {
        let port = &row.port;
        let at = PortRef::new(node.clone(), row.key.clone());
        let wire = cx.graph.wire_into(&at);
        let gn = cx.graph.node(node);
        let def = gn.and_then(|n| cx.catalog.node(&n.kind));
        let current = gn.zip(def).map(|(n, d)| ports::current_value(d, n, &row.key, port)).unwrap_or_else(|| port.default.clone());
        // what nobody setting it gives: the chosen preset's value (a filling choice), else the
        // port's own default
        let default = gn.zip(def).and_then(|(n, d)| d.filled_default(&row.key, |k| n.values.get(k).filter(|_| k != row.key))).unwrap_or(&port.default).clone();
        let source = crate::bundle_view::source(cx.graph, cx.catalog, &at, port, crate::bundle_view::provenance(cx.run, node, &row.key));
        let inherited = matches!(source, Source::Inherited(_));
        let own_editor = def.and_then(|d| value_editors::for_port(d, &row.key));
        let editable = wire.is_none() && !inherited && (widgets::has_editor(port) || own_editor.is_some());
        let inactive = widgets::inactive_note(port, |setting| {
            let d = def?;
            let p = d.inputs.iter().find(|p| p.name == setting)?;
            Some(ports::current_value(d, gn?, setting, p))
        });
        let label = if port.list { format!("{} [ ]", row.label) } else { row.label.clone() };
        let help = help_text(row, inactive.as_deref(), cx.catalog);
        let label_w = (ui.available_width() * 0.34).clamp(120.0, 210.0);
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            // the label column: the name (faint when not read now) and its "?"
            ui.allocate_ui_with_layout(egui::vec2(label_w, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_min_width(label_w);
                let color = if inactive.is_some() { cx.tokens.text_faint } else { cx.tokens.text };
                let mut text = RichText::new(&label).color(color);
                if inactive.is_some() {
                    text = text.italics();
                }
                ui.add(egui::Label::new(text).truncate()).on_hover_text(&help);
                param_widgets::help_icon(ui, cx.tokens, &help);
            });
            // the widget column: as wide as the window allows, less the reset and the gaps (a
            // row wider than the window would widen it a little every frame)
            let width = (ui.available_width() - RESET_W - 2.0 * ui.spacing().item_spacing.x).max(120.0);
            let r = ui.allocate_ui_with_layout(egui::vec2(width, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_width(width);
                self.value_cell(ui, cx, node, row, &at, &current, wire, &source, editable, actions)
            });
            if editable {
                self.rects.push((row.key.clone(), r.response.rect));
            }
            if let Some(value) = r.inner {
                actions.push(("node.set", json!({"node": node, "port": row.key, "value": value})));
            }
            if editable && current != default {
                if ui.small_button("↺").on_hover_text(format!("Reset to the default: {}", widgets::value_text(&default))).clicked() {
                    actions.push(("node.set", json!({"node": node, "port": row.key, "value": default})));
                }
            } else {
                ui.add_space(RESET_W);
            }
        });
        ui.add_space(3.0);
        hairline(ui, cx.tokens);
    }

    /// What feeds an input (a wire, a bundle) or its editor. Returns a value to commit; a badge's
    /// reset goes straight to `actions`.
    #[allow(clippy::too_many_arguments)]
    fn value_cell(
        &mut self,
        ui: &mut egui::Ui,
        cx: &Context<'_>,
        node: &NodeId,
        row: &InputRow,
        at: &PortRef,
        current: &Value,
        wire: Option<&plumcraft_graph::Wire>,
        source: &Source,
        editable: bool,
        actions: &mut Vec<Action>,
    ) -> Option<Value> {
        let port = &row.port;
        match (wire, port.socket_mode()) {
            (Some(w), mode) => {
                let from = plumcraft_session::node_title(cx.graph, cx.catalog, &w.from.node);
                let sliced = w.slice.as_deref().map_or_else(String::new, |s| format!(" {}", crate::slice_view::label_text(s)));
                let text = match mode {
                    Socket::Socket => format!("← {from} · {}{sliced}", w.from.port),
                    _ => format!("{}  ← {from} · {}{sliced}", widgets::value_text(current), w.from.port),
                };
                chip(ui, cx.tokens, &text);
                if *source == Source::Overridden {
                    let word = crate::bundle_view::bundle_word(cx.graph, cx.catalog, node);
                    actions.extend(crate::bundle_view::override_badge(ui, cx.tokens, &word, at));
                }
                None
            }
            (None, Socket::Socket) => {
                ui.label(RichText::new(if port.optional { "not wired (optional)" } else { "not wired" }).italics().color(cx.tokens.text_dim));
                None
            }
            (None, _) if let Source::Inherited(value) = source => {
                let word = crate::bundle_view::bundle_word(cx.graph, cx.catalog, node);
                let shown = value.as_ref().map_or_else(|| format!("(from {word})"), widgets::value_text);
                chip(ui, cx.tokens, &format!("{shown}  ← {word}"));
                let help = format!(
                    "From the {word} wired into this node. The widget's own value ({}) is not used: Detach, at the top of this window or in the node's menu, uses its own values.",
                    widgets::value_text(current)
                );
                param_widgets::help_icon(ui, cx.tokens, &help);
                None
            }
            (None, _) if editable => {
                // the node's own editor: beside the widget of a type this editor knows, alone
                // for one it doesn't
                if let Some(kind) = cx.catalog.node(cx.graph.node(node)?.kind.as_str()).and_then(|d| value_editors::for_port(d, &row.key)) {
                    if ui.button(kind.label).on_hover_text(kind.help).clicked() {
                        actions.push(("node.editor", json!({"node": node})));
                    }
                    if !widgets::has_editor(port) {
                        ui.add(egui::Label::new(RichText::new((kind.summary)(current)).color(cx.tokens.text_dim)).truncate());
                        return None;
                    }
                }
                let key = (node.clone(), row.key.clone());
                self.editor(ui, cx, node, port, &key, current)
            }
            (None, _) => {
                ui.label(widgets::value_text(current));
                None
            }
        }
    }

    /// The editor for an editable port, chosen by what the schema says about it.
    fn editor(&mut self, ui: &mut egui::Ui, cx: &Context<'_>, node: &NodeId, port: &Port, key: &widgets::EditKey, current: &Value) -> Option<Value> {
        let id = egui::Id::new(("plumcraft-params-widget", &key.0, &key.1));
        let blank = cx.catalog.text(texts::BLANK_VALUE);
        if port.widget.as_deref() == Some(hints::MULTICHOICE) {
            let choices = port.choices.clone().filter(|c| !c.is_empty()).unwrap_or_else(|| fed_choices(cx.graph, cx.catalog, node));
            if !choices.is_empty() {
                return param_widgets::multichoice(ui, id, cx.tokens, port, &choices, current, "None picked: every choice");
            }
            // nothing to pick from (not wired yet): the text, and why
            let typed = ui.horizontal(|ui| widgets::port_widget_wide(ui, port, key, current, &mut self.edits, blank)).inner;
            return typed;
        }
        let choices = port.choices.clone().unwrap_or_default();
        if !choices.is_empty() {
            if choices.len() <= SEGMENTED_CHOICES {
                return param_widgets::segmented(ui, cx.tokens, port, &choices, current);
            }
            return param_widgets::select(ui, id, port, &choices, current, ui.available_width().min(320.0));
        }
        if (port.ty == scalars::FLOAT || port.ty == scalars::INT) && widgets::has_range(port) {
            return widgets::port_slider(ui, port, key, current, &mut self.edits, blank);
        }
        if port.ty == scalars::BOOL {
            let mut b = current.as_bool().or_else(|| port.default.as_bool()).unwrap_or(false);
            let r = param_widgets::toggle(ui, cx.tokens, &mut b);
            ui.label(RichText::new(if b { "on" } else { "off" }).color(cx.tokens.text_dim));
            return r.changed().then_some(Value::Bool(b));
        }
        widgets::port_widget_wide(ui, port, key, current, &mut self.edits, blank)
    }
}

/// The window's header, in the node's family colour: its title, type and id, what it does.
fn header(ui: &mut egui::Ui, cx: &Context<'_>, node: &NodeId, def: &NodeDef, title: &str) {
    let tint = cx.tokens.family_tint(cx.catalog, &def.family);
    egui::Frame::new().fill(tint.header).corner_radius(8).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            // the title in what the button leaves (a long one is cut, not a wider window)
            let width = (ui.available_width() - RENAME_W).max(80.0);
            ui.scope(|ui| {
                ui.set_max_width(width);
                ui.add(egui::Label::new(RichText::new(title).size(19.0).strong().color(cx.tokens.text)).truncate());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Rename…").on_hover_text("Title this node").clicked() {
                    let current = cx.graph.node(node).and_then(|n| n.title.clone()).unwrap_or_default();
                    crate::rename_view::request(ui.ctx(), crate::rename_view::RenameTarget::Node(node.clone()), &current, &def.display);
                }
            });
        });
        ui.label(RichText::new(format!("{} · {node}", def.display)).color(cx.tokens.text_dim));
        if !def.description.is_empty() {
            ui.add_space(4.0);
            // the first paragraph; the rest on hover
            let first = def.description.split("\n\n").next().unwrap_or_default();
            let r = ui.add(egui::Label::new(RichText::new(first).color(cx.tokens.text_dim)).wrap());
            if first.len() < def.description.len() {
                r.on_hover_text(&def.description);
            }
        }
    });
}

/// A value shown in a rounded field-coloured chip (what a wire or the settings give).
fn chip(ui: &mut egui::Ui, tokens: &Tokens, text: &str) {
    egui::Frame::new().fill(tokens.field).corner_radius(9).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.add(egui::Label::new(RichText::new(text).color(tokens.text)).truncate());
    });
}

/// A thin line between rows.
fn hairline(ui: &mut egui::Ui, tokens: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, tokens.field_border));
}

/// A row's help, for its "?": the port's doc, then what the catalogue says (default, range, step,
/// choices), when it is not read now, and its key when the label differs.
pub fn help_text(row: &InputRow, inactive: Option<&str>, catalog: &SchemaReply) -> String {
    let port = &row.port;
    let mut parts: Vec<String> = Vec::new();
    if let Some(t) = port.tooltip.as_deref().filter(|t| !t.trim().is_empty()) {
        parts.push(t.trim().to_owned());
    }
    let mut kind = port.ty.clone();
    if port.optional {
        kind.push_str(", optional");
    }
    match facts_line(port, catalog.text(texts::BLANK_VALUE)) {
        Some(f) => parts.push(format!("{kind} · {f}")),
        None => parts.push(kind),
    }
    if let Some(note) = inactive {
        parts.push(note.to_owned());
    }
    if row.key != row.label {
        parts.push(format!("Key: {}", row.key));
    }
    parts.join("\n\n")
}

/// The choices of the input `node` feeds, for a multichoice port without its own (Vary Choices'
/// `values`): the first wired target that has some.
pub fn fed_choices(graph: &Graph, catalog: &SchemaReply, node: &NodeId) -> Vec<Value> {
    for w in graph.wires.iter().filter(|w| w.from.node == *node) {
        let Some(target) = graph.node(&w.to.node) else { continue };
        let Some(def) = catalog.node(&target.kind) else { continue };
        let found = ports::input_ports(def, target).into_iter().find(|(k, _)| *k == w.to.port).and_then(|(_, p)| p.choices.clone());
        if let Some(c) = found.filter(|c| !c.is_empty()) {
            return c;
        }
    }
    Vec::new()
}

/// What the catalogue says about a widget port besides its doc: the default, the range, the step,
/// the choices. None for a socket or a port with nothing to say.
pub fn facts_line(port: &Port, blank: Option<&str>) -> Option<String> {
    if port.socket_mode() == Socket::Socket || !widgets::has_editor(port) {
        return None;
    }
    let mut parts = Vec::new();
    if !port.default.is_null() {
        parts.push(format!("default {}", widgets::value_text(&port.default)));
    } else if port.ty == scalars::FLOAT || port.ty == scalars::INT {
        parts.push(format!("default blank ({})", blank.unwrap_or("the engine's own")));
    }
    let num = widgets::number_text;
    match (port.min, port.max) {
        (Some(lo), Some(hi)) if lo.is_finite() && hi.is_finite() => parts.push(format!("{} to {}", num(lo), num(hi))),
        (Some(lo), _) if lo.is_finite() => parts.push(format!("at least {}", num(lo))),
        (_, Some(hi)) if hi.is_finite() => parts.push(format!("at most {}", num(hi))),
        _ => {}
    }
    if (port.soft_min.is_some() || port.soft_max.is_some())
        && let Some((lo, hi)) = widgets::slider_span(port)
    {
        parts.push(format!("slider {} to {}", num(lo), num(hi)));
    }
    if let Some(step) = port.step.filter(|s| s.is_finite() && *s > 0.0 && port.ty == scalars::FLOAT) {
        parts.push(format!("step {}", num(step)));
    }
    if let Some(c) = &port.choices
        && c.len() > SEGMENTED_CHOICES
    {
        parts.push(format!("{} choices", c.len()));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_facts_line_says_default_range_and_step() {
        let p = Port { ty: "FLOAT".into(), default: json!(0.5), min: Some(0.0), max: Some(10.0), soft_max: Some(2.0), step: Some(0.25), ..Default::default() };
        assert_eq!(facts_line(&p, None).unwrap(), "default 0.5 · 0 to 10 · slider 0 to 2 · step 0.25");
        let open = Port { ty: "INT".into(), min: Some(1.0), ..Default::default() };
        assert_eq!(facts_line(&open, Some("the engine's fallback")).unwrap(), "default blank (the engine's fallback) · at least 1");
        let socket = Port { ty: "ARRAY".into(), ..Default::default() };
        assert_eq!(facts_line(&socket, None), None);
    }

    #[test]
    fn help_holds_the_doc_the_facts_and_when_it_is_read() {
        let port = Port { ty: "FLOAT".into(), tooltip: Some("How wide".into()), default: json!(1.0), ..Default::default() };
        let row = InputRow { key: "strokes.width".into(), label: "width".into(), port, kind: crate::canvas::RowKind::Plain };
        let help = help_text(&row, Some("Not used now."), &SchemaReply::default());
        assert_eq!(help, "How wide\n\nFLOAT · default 1.0\n\nNot used now.\n\nKey: strokes.width");
    }
}
