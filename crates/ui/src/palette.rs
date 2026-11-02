//! The node palette: a searchable popup at the cursor (right-click on the canvas, or Tab/Space).
//!
//! Filtering is one pass over a precomputed lower-case haystack per node type (display name,
//! id and category path), and the list shows at most [`MAX_SHOWN`] results, so a catalogue of
//! hundreds of types stays responsive while typing. The node types come only from the engine's
//! catalogue.
//!
//! Lifted from the research software's `crates/ui-egui/src/palette.rs`.

use std::collections::BTreeMap;

use egui::{Key, Pos2};
use plumcraft_proto::catalogue::{NodeDef, SchemaReply};

/// The most results the palette draws.
pub const MAX_SHOWN: usize = 150;

/// The palette's search index over one catalogue.
#[derive(Debug, Clone, Default)]
pub struct PaletteIndex {
    /// The catalogue it was built from: (digest, node count, first and last ids).
    key: (String, usize, String, String),
    /// Per node: (lower-case haystack, category path, display name), in catalogue order.
    entries: Vec<(String, String, String)>,
    /// Node indices sorted by (category path, display name).
    order: Vec<usize>,
}

/// The display name of a node type (its id when it has none).
pub fn display_name(def: &NodeDef) -> &str {
    if def.display.is_empty() { &def.id } else { &def.display }
}

/// A category path as shown (`Text › Testing`); `Other` when empty.
pub fn category_path(def: &NodeDef) -> String {
    if def.category.is_empty() { "Other".to_owned() } else { def.category.join(" › ") }
}

impl PaletteIndex {
    /// Rebuild if `catalog` isn't the one indexed.
    pub fn refresh(&mut self, catalog: &SchemaReply) {
        let ends = |n: Option<&NodeDef>| n.map(|d| d.id.clone()).unwrap_or_default();
        let key = (catalog.engine.schema_digest.clone(), catalog.nodes.len(), ends(catalog.nodes.first()), ends(catalog.nodes.last()));
        if self.key == key && self.entries.len() == catalog.nodes.len() {
            return;
        }
        self.key = key;
        self.entries = catalog
            .nodes
            .iter()
            .map(|d| {
                let category = category_path(d);
                let display = display_name(d).to_owned();
                (format!("{} {} {}", display, d.id, d.category.join(" ")).to_lowercase(), category, display)
            })
            .collect();
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_by(|&a, &b| {
            let (ea, eb) = (self.entries.get(a), self.entries.get(b));
            ea.map(|e| (&e.1, e.2.to_lowercase())).cmp(&eb.map(|e| (&e.1, e.2.to_lowercase())))
        });
        self.order = order;
    }

    /// Node indices matching every word of `query` (case-insensitive), in category order, at
    /// most `cap`. The second value is the total number of matches.
    pub fn filter(&self, query: &str, cap: usize) -> (Vec<usize>, usize) {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut shown = Vec::new();
        let mut total = 0;
        for &i in &self.order {
            let Some((hay, _, _)) = self.entries.get(i) else { continue };
            if words.iter().all(|w| hay.contains(w.as_str())) {
                total += 1;
                if shown.len() < cap {
                    shown.push(i);
                }
            }
        }
        (shown, total)
    }

    fn category(&self, i: usize) -> &str {
        self.entries.get(i).map_or("", |e| e.1.as_str())
    }
}

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
}

impl PaletteState {
    pub fn new(screen_pos: Pos2, graph_pos: Pos2) -> Self {
        PaletteState { screen_pos, graph_pos, query: String::new(), selected: 0, focus_requested: false }
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

/// Draw the palette. Escape or a click outside closes it.
pub fn show(ctx: &egui::Context, state: &mut PaletteState, index: &PaletteIndex, catalog: &SchemaReply) -> PaletteOutcome {
    let (shown, total) = index.filter(&state.query, MAX_SHOWN);
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
                ui.weak(if catalog.nodes.is_empty() { "No node types: the engine is not connected." } else { "No matching nodes." });
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
                    let r = ui.add(egui::Button::selectable(selected, display_name(def)));
                    if selected && (up || down) {
                        r.scroll_to_me(None);
                    }
                    let r = if def.description.is_empty() { r.on_hover_text(&def.id) } else { r.on_hover_text(format!("{}\n\n{}", def.description, def.id)) };
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

/// Nested menus of node types by category path (Node › Add), under a menu per engine first
/// when several are connected. Returns the kind clicked.
pub fn node_menus(ui: &mut egui::Ui, catalog: &SchemaReply) -> Option<String> {
    #[derive(Default)]
    struct Tree<'a> {
        children: BTreeMap<&'a str, Tree<'a>>,
        leaves: Vec<&'a NodeDef>,
    }
    fn draw(ui: &mut egui::Ui, tree: &Tree<'_>, picked: &mut Option<String>) {
        for (name, sub) in &tree.children {
            ui.menu_button(*name, |ui| draw(ui, sub, picked));
        }
        for def in &tree.leaves {
            let r = ui.button(display_name(def));
            let r = if def.description.is_empty() { r } else { r.on_hover_text(&def.description) };
            if r.clicked() {
                *picked = Some(def.id.clone());
                ui.close();
            }
        }
    }
    fn sort_leaves(t: &mut Tree<'_>) {
        t.leaves.sort_by_key(|d| display_name(d).to_lowercase());
        for c in t.children.values_mut() {
            sort_leaves(c);
        }
    }
    // Several engines (a merged catalogue): a menu per engine first, and the kinds more than one
    // offers (the building blocks) under Shared.
    let several = catalog.nodes.iter().flat_map(|d| d.engines.iter()).collect::<std::collections::BTreeSet<_>>().len() > 1;
    let mut root = Tree::default();
    for def in &catalog.nodes {
        let mut at = &mut root;
        if several {
            let branch = match def.engines.as_slice() {
                [one] => one.as_str(),
                [] => "Other",
                _ => "Shared",
            };
            at = at.children.entry(branch).or_default();
        }
        if def.category.is_empty() {
            at = at.children.entry("Other").or_default();
        }
        for part in &def.category {
            at = at.children.entry(part.as_str()).or_default();
        }
        at.leaves.push(def);
    }
    sort_leaves(&mut root);
    let mut picked = None;
    draw(ui, &root, &mut picked);
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> SchemaReply {
        serde_json::from_str(
            r#"{"v": 2, "engine": {"schema_digest": "d"}, "nodes": [
              {"id": "text.upper", "display": "Upper case", "category": ["Text"]},
              {"id": "text.slow", "display": "Slow", "category": ["Text", "Testing"]},
              {"id": "text.lower", "display": "Lower case", "category": ["Text"]},
              {"id": "table.sort", "display": "Sort", "category": ["Tables"]},
              {"id": "bare"}
            ]}"#,
        )
        .unwrap()
    }

    fn names(c: &SchemaReply, idx: &[usize]) -> Vec<String> {
        idx.iter().map(|&i| c.nodes[i].id.clone()).collect()
    }

    #[test]
    fn filter_matches_display_id_and_category() {
        let c = catalog();
        let mut index = PaletteIndex::default();
        index.refresh(&c);
        let (all, total) = index.filter("", 100);
        assert_eq!(total, 5);
        // Sorted by category path, then name.
        assert_eq!(names(&c, &all), ["bare", "table.sort", "text.lower", "text.upper", "text.slow"]);
        assert_eq!(names(&c, &index.filter("UPPER", 100).0), ["text.upper"]);
        assert_eq!(names(&c, &index.filter("text.", 100).0), ["text.lower", "text.upper", "text.slow"]);
        assert_eq!(names(&c, &index.filter("text testing", 100).0), ["text.slow"]);
        assert_eq!(index.filter("nothing like it", 100), (vec![], 0));
        let (capped, total) = index.filter("", 2);
        assert_eq!((capped.len(), total), (2, 5));
    }

    #[test]
    fn index_rebuilds_for_a_new_catalog() {
        let mut index = PaletteIndex::default();
        index.refresh(&catalog());
        let mut c2 = catalog();
        c2.nodes.truncate(2);
        index.refresh(&c2);
        assert_eq!(index.filter("", 10).1, 2);
        assert_eq!(category_path(&c2.nodes[1]), "Text › Testing");
        assert_eq!(category_path(&NodeDef::default()), "Other");
        // the same size and digest, other nodes (an engine switched): rebuilt
        let mut c3 = catalog();
        c3.nodes[0].id = "text.shout".into();
        c3.nodes[4].id = "other".into();
        index.refresh(&catalog());
        index.refresh(&c3);
        assert_eq!(names(&c3, &index.filter("shout", 10).0), ["text.shout"]);
    }

    /// Thousands of types, filtered on every keystroke: well under a frame.
    #[test]
    fn filtering_a_large_catalog_is_fast() {
        let mut c = SchemaReply::default();
        for i in 0..2000 {
            c.nodes.push(NodeDef {
                id: format!("pack.op.node{i}"),
                display: format!("Node {i}"),
                category: vec!["Utilities".into(), format!("g{}", i % 7)],
                ..Default::default()
            });
        }
        let mut index = PaletteIndex::default();
        let t = std::time::Instant::now();
        index.refresh(&c);
        for q in ["n", "no", "nod", "node 1", "g3 node 19"] {
            let _ = index.filter(q, MAX_SHOWN);
        }
        assert!(t.elapsed() < std::time::Duration::from_millis(200), "{:?}", t.elapsed());
    }
}
