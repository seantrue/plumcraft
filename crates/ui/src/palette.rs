//! The node palette: a searchable popup at the cursor (right-click on the canvas, or Tab/Space).
//!
//! Filtering is one pass over a precomputed lower-case haystack per node type (display name,
//! id and category path), and the list shows at most [`MAX_SHOWN`] results, so a catalogue of
//! hundreds of types stays responsive while typing. The node types come only from the engine's
//! catalogue.
//!
//! Lifted from the research software's `crates/ui-egui/src/palette.rs`. The search index is the
//! core's ([`plumcraft_editor_core::palette`], stage N1); drawing it is here.

use egui::{Key, Pos2};
use plumcraft_proto::catalogue::SchemaReply;

pub use plumcraft_editor_core::palette::{MAX_SHOWN, PaletteIndex, category_path, display_name};

/// An open palette.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteState {
    /// Where it opened, in screen points.
    pub screen_pos: Pos2,
    /// Where a node is added, in graph coordinates.
    pub graph_pos: Pos2,
    pub query: String,
    /// The highlighted row among the shown results.
    pub selected: usize,
    focus_requested: bool,
    /// Opened for a dropped wire: the node types (catalogue indices) that take it, the only ones
    /// offered (the core's `palette::fitting`, N3 slice 1).
    pub fits: Option<std::collections::BTreeSet<usize>>,
}

impl PaletteState {
    pub fn new(screen_pos: Pos2, graph_pos: Pos2) -> Self {
        PaletteState { screen_pos, graph_pos, query: String::new(), selected: 0, focus_requested: false, fits: None }
    }
}

/// What the palette did this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteOutcome {
    Open,
    Closed,
    /// Add a node of this kind at the palette's graph position.
    Add(String),
}

/// The palette's search box hint (tests and the control channel find it by this label).
pub const SEARCH_HINT: &str = "Search nodes…";

/// Draw the palette. Escape or a click outside closes it. The kinds from `own` on are of engines
/// not started (`plumcraft_editor_core::palette::with_starts`): each says which engine choosing
/// it starts.
pub fn show(ctx: &egui::Context, state: &mut PaletteState, index: &PaletteIndex, catalog: &SchemaReply, own: usize) -> PaletteOutcome {
    let (shown, total) = index.filter_in(&state.query, MAX_SHOWN, |i| state.fits.as_ref().is_none_or(|f| f.contains(&i)));
    let mut outcome = PaletteOutcome::Open;
    let (up, down, enter, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.key_pressed(Key::Enter),
            i.consume_key(egui::Modifiers::NONE, Key::Escape),
        )
    });
    if escape {
        return PaletteOutcome::Closed;
    }
    if down && state.selected + 1 < shown.len() {
        state.selected += 1;
    }
    if up {
        state.selected = state.selected.saturating_sub(1);
    }
    if state.selected >= shown.len() {
        state.selected = shown.len().saturating_sub(1);
    }
    let area = egui::Area::new(egui::Id::new("plumcraft-palette")).order(egui::Order::Foreground).fixed_pos(state.screen_pos).constrain(true).fade_in(false);
    let r = area.show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(300.0);
            let search = ui.add(egui::TextEdit::singleline(&mut state.query).hint_text(SEARCH_HINT).desired_width(f32::INFINITY));
            if !state.focus_requested {
                search.request_focus();
                state.focus_requested = true;
            }
            if search.changed() {
                state.selected = 0;
            }
            if enter && let Some(def) = shown.get(state.selected).and_then(|&i| catalog.nodes.get(i)) {
                outcome = PaletteOutcome::Add(def.id.clone());
            }
            ui.separator();
            if shown.is_empty() {
                ui.weak(if catalog.nodes.is_empty() {
                    "No node types: the engine is not connected."
                } else if state.fits.is_some() && state.query.trim().is_empty() {
                    "No node takes this wire."
                } else {
                    "No matching nodes."
                });
            }
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                let mut last_category: Option<&str> = None;
                for (row, &i) in shown.iter().enumerate() {
                    let Some(def) = catalog.nodes.get(i) else { continue };
                    let category = index.category(i);
                    if last_category != Some(category) {
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new(category).small().strong());
                        last_category = Some(category);
                    }
                    let selected = row == state.selected;
                    let starts = (i >= own).then(|| def.engines.first()).flatten();
                    let r = match starts {
                        Some(e) => ui.add(egui::Button::selectable(selected, format!("{}  · starts the {e} engine", display_name(def)))),
                        None => ui.add(egui::Button::selectable(selected, display_name(def))),
                    };
                    if selected && (up || down) {
                        r.scroll_to_me(None);
                    }
                    let r = if def.description.is_empty() { r.on_hover_text(&def.id) } else { r.on_hover_text(format!("{}\n\n{}", def.description, def.id)) };
                    let r = match starts {
                        Some(e) => r.on_hover_text(format!("Choosing it starts the {e} engine; the node is added once it has connected.")),
                        None => r,
                    };
                    if r.clicked() {
                        outcome = PaletteOutcome::Add(def.id.clone());
                    }
                }
                if total > shown.len() {
                    ui.weak(format!("{} more: type to narrow", total - shown.len()));
                }
            });
        });
    });
    if outcome == PaletteOutcome::Open && r.response.clicked_elsewhere() && state.focus_requested && !ctx.input(|i| i.pointer.secondary_clicked()) {
        return PaletteOutcome::Closed;
    }
    outcome
}
