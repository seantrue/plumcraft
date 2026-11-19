//! The node palette's search index: what a searchable popup at the cursor (right-click on the
//! canvas, or Tab/Space) filters.
//!
//! Filtering is one pass over a precomputed lower-case haystack per node type (display name,
//! id and category path), and the list shows at most [`MAX_SHOWN`] results, so a catalogue of
//! hundreds of types stays responsive while typing. The node types come only from the engine's
//! catalogue.
//!
//! Lifted from the research software's `crates/ui-egui/src/palette.rs`. The index moved to the
//! core (stage N1); drawing the popup stays in each shell (the egui shell: `plumcraft_ui::palette`).

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

    /// Node `i`'s category path as indexed (empty past the end).
    pub fn category(&self, i: usize) -> &str {
        self.entries.get(i).map_or("", |e| e.1.as_str())
    }
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
