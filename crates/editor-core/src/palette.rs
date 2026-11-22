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
//!
//! **A dropped wire** (N3 slice 1, contract question 8): a palette opened for a new wire dropped
//! on empty canvas offers only the kinds with a port that takes it ([`fitting`], by
//! `Graph::input_for_new` and `Graph::output_for_new`: the types and list-ness `wire.connect`
//! checks), and [`items`] is what `palette.items` lists, for a shell that draws the popup.
//!
//! **Testing kinds** (the maintainer, 2026-11-21, card 31 B: "hide Testing kinds from the
//! palette"): a kind in a `Testing` category (the text engines' Every widget and Slow) is not
//! offered to a person, in the palette or the Node › Add menu of any shell ([`offered`]); it is
//! still added by id (`node.add`), as the conformance cases and tests do.

use plumcraft_graph::{Graph, PortRef};
use plumcraft_proto::catalogue::{NodeDef, SchemaReply};
use serde_json::{Value, json};

/// The most results the palette draws.
pub const MAX_SHOWN: usize = 150;

/// The palette's search index over one catalogue.
#[derive(Debug, Clone, Default)]
pub struct PaletteIndex {
    /// The catalogue it was built from: (digest, node count, first and last ids).
    key: (String, usize, String, String),
    /// Per node: (lower-case haystack, category path, display name), in catalogue order.
    entries: Vec<(String, String, String)>,
    /// The indices of the nodes a person is offered ([`offered`]), sorted by (category path,
    /// display name): a Testing kind is indexed but never listed.
    order: Vec<usize>,
}

/// The display name of a node type (its id when it has none).
pub fn display_name(def: &NodeDef) -> &str {
    if def.display.is_empty() { &def.id } else { &def.display }
}

/// The category whose kinds are never offered to a person to add.
pub const TESTING: &str = "Testing";

/// Whether a person is offered `def` to add (the palette, Node › Add): not a kind in a
/// [`TESTING`] category, at any depth of its path.
pub fn offered(def: &NodeDef) -> bool {
    !def.category.iter().any(|c| c == TESTING)
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
        let mut order: Vec<usize> = (0..self.entries.len()).filter(|&i| catalog.nodes.get(i).is_some_and(offered)).collect();
        order.sort_by(|&a, &b| {
            let (ea, eb) = (self.entries.get(a), self.entries.get(b));
            ea.map(|e| (&e.1, e.2.to_lowercase())).cmp(&eb.map(|e| (&e.1, e.2.to_lowercase())))
        });
        self.order = order;
    }

    /// Node indices matching every word of `query` (case-insensitive): those whose title is the
    /// query itself (case and spaces aside) first, then the rest, each in category order; at
    /// most `cap`. The second value is the total number of matches. (Decision 13 after the person
    /// check: typing "Text" finds the Text node first, not whatever category sorts before it.)
    pub fn filter(&self, query: &str, cap: usize) -> (Vec<usize>, usize) {
        self.filter_in(query, cap, |_| true)
    }

    /// [`PaletteIndex::filter`] among the nodes `keep` takes (a dropped wire's [`fitting`]).
    pub fn filter_in(&self, query: &str, cap: usize, keep: impl Fn(usize) -> bool) -> (Vec<usize>, usize) {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let whole = words.join(" ");
        let (mut exact, mut rest) = (Vec::new(), Vec::new());
        for &i in &self.order {
            let Some((hay, _, display)) = self.entries.get(i) else { continue };
            if keep(i) && words.iter().all(|w| hay.contains(w.as_str())) {
                let title: Vec<String> = display.split_whitespace().map(str::to_lowercase).collect();
                if !whole.is_empty() && title.join(" ") == whole { exact.push(i) } else { rest.push(i) }
            }
        }
        let total = exact.len() + rest.len();
        exact.extend(rest);
        exact.truncate(cap);
        (exact, total)
    }

    /// Node `i`'s category path as indexed (empty past the end).
    pub fn category(&self, i: usize) -> &str {
        self.entries.get(i).map_or("", |e| e.1.as_str())
    }
}

/// The node types (by index in `catalog.nodes`) a palette opened for a dropped wire offers, each
/// with the port a pick connects: with `from_output`, `end` is the output the wire leaves and the
/// port is the new node's input that takes it; else `end` is the input the wire enters and the
/// port the new node's output that feeds it. In catalogue order.
pub fn fitting(graph: &Graph, catalog: &SchemaReply, end: &PortRef, from_output: bool) -> Vec<(usize, String)> {
    catalog
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(i, def)| {
            let port = if from_output { graph.input_for_new(catalog, end, def) } else { graph.output_for_new(catalog, def, end) }?;
            Some((i, port))
        })
        .collect()
}

/// `catalog` with `starts` after its own nodes: the node types an engine not started offers,
/// which choosing starts (N4 slice 1). Borrowed when there are none.
pub fn with_starts<'a>(catalog: &'a SchemaReply, starts: &[&NodeDef]) -> std::borrow::Cow<'a, SchemaReply> {
    if starts.is_empty() {
        return std::borrow::Cow::Borrowed(catalog);
    }
    let mut c = catalog.clone();
    c.nodes.extend(starts.iter().map(|d| (*d).clone()));
    std::borrow::Cow::Owned(c)
}

/// `palette.items {query?, limit?}`'s reply for a palette in `palette` (or none open):
/// `{items: [{kind, label, category, port?, starts?}], total, wire?}`, at most `limit` (default
/// and most [`MAX_SHOWN`]) in category order, matching every word of `query` (default the
/// palette's own); with a wire, only the kinds that take it, each with the port a pick connects.
/// `starts` are node types the catalogue lacks whose engine isn't started
/// (`plumcraft_session::engine_cmds::startable_defs`): they are listed among the rest, each with
/// `starts: <engine>`, since choosing one starts that engine and the add waits for it (N4 slice 1).
pub fn items(graph: &Graph, catalog: &SchemaReply, starts: &[&NodeDef], palette: Option<&crate::scene::Palette>, params: &Value) -> Result<Value, String> {
    crate::control::check_fields(params, &["query", "limit"])?;
    let query = match params.get("query") {
        None | Some(Value::Null) => palette.and_then(|p| p.query.clone()).unwrap_or_default(),
        Some(Value::String(q)) => q.chars().take(256).collect(),
        Some(_) => return Err("query must be a string".into()),
    };
    let limit = match params.get("limit") {
        None | Some(Value::Null) => MAX_SHOWN,
        Some(v) => v.as_u64().filter(|n| *n >= 1).map(|n| n.min(MAX_SHOWN as u64) as usize).ok_or("limit must be a whole number, 1 or more")?,
    };
    let own = catalog.nodes.len();
    let catalog = with_starts(catalog, starts);
    let mut index = PaletteIndex::default();
    index.refresh(&catalog);
    let wire = palette.and_then(|p| p.wire.as_ref());
    let fits: Option<std::collections::BTreeMap<usize, String>> =
        wire.and_then(|w| w.end()).map(|(end, out)| fitting(graph, &catalog, &end, out).into_iter().collect());
    let (shown, total) = index.filter_in(&query, limit, |i| fits.as_ref().is_none_or(|f| f.contains_key(&i)));
    let items: Vec<Value> = shown
        .iter()
        .filter_map(|&i| {
            let def = catalog.nodes.get(i)?;
            let mut v = json!({"kind": def.id, "label": display_name(def), "category": index.category(i)});
            if let Some(port) = fits.as_ref().and_then(|f| f.get(&i)) {
                v["port"] = json!(port);
            }
            if i >= own
                && let Some(e) = def.engines.first()
            {
                v["starts"] = json!(e);
            }
            Some(v)
        })
        .collect();
    let mut out = json!({"items": items, "total": total, "query": query});
    if let Some(w) = wire {
        out["wire"] = serde_json::to_value(w).unwrap_or(Value::Null);
    }
    Ok(out)
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
        assert_eq!(total, 4);
        // Sorted by category path, then name; the Testing kind (text.slow) is never listed
        assert_eq!(names(&c, &all), ["bare", "table.sort", "text.lower", "text.upper"]);
        assert_eq!(names(&c, &index.filter("UPPER", 100).0), ["text.upper"]);
        assert_eq!(names(&c, &index.filter("text.", 100).0), ["text.lower", "text.upper"]);
        assert_eq!(index.filter("nothing like it", 100), (vec![], 0));
        let (capped, total) = index.filter("", 2);
        assert_eq!((capped.len(), total), (2, 4));
    }

    /// Decision 13: a title the query is (case and spaces aside) comes first; the rest keep
    /// their category order; a word that only begins the title, or a cap of one, still works.
    #[test]
    fn an_exact_title_comes_first() {
        let c: SchemaReply = serde_json::from_str(
            r#"{"v": 2, "engine": {"schema_digest": "e"}, "nodes": [
              {"id": "a.text_join", "display": "Join text", "category": ["Arrays"]},
              {"id": "a.to_text", "display": "Array to Text", "category": ["Arrays"]},
              {"id": "text.literal", "display": "Text", "category": ["Text"]},
              {"id": "text.upper", "display": "Upper case", "category": ["Text"]}
            ]}"#,
        )
        .unwrap();
        let mut index = PaletteIndex::default();
        index.refresh(&c);
        assert_eq!(names(&c, &index.filter("", 10).0), ["a.to_text", "a.text_join", "text.literal", "text.upper"], "no query: category order");
        assert_eq!(names(&c, &index.filter("text", 10).0), ["text.literal", "a.to_text", "a.text_join", "text.upper"]);
        assert_eq!(names(&c, &index.filter("  TEXT ", 10).0), ["text.literal", "a.to_text", "a.text_join", "text.upper"]);
        assert_eq!(index.filter("text", 1), (vec![2], 4), "the cap keeps the exact one");
        assert_eq!(names(&c, &index.filter("array  to text", 10).0), ["a.to_text"]);
        assert_eq!(names(&c, &index.filter("tex", 10).0), ["a.to_text", "a.text_join", "text.literal", "text.upper"], "not a whole title: category order");
    }

    #[test]
    fn index_rebuilds_for_a_new_catalog() {
        let mut index = PaletteIndex::default();
        index.refresh(&catalog());
        let mut c2 = catalog();
        c2.nodes.truncate(2);
        index.refresh(&c2);
        assert_eq!(index.filter("", 10).1, 1, "text.slow, the second, is a Testing kind");
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

    /// The maintainer's decision (2026-11-21, card 31 B): a Testing kind is not offered by
    /// `palette.items` for any query or dropped wire, nor by the Node › Add menu, but `node.add`
    /// by id still adds it.
    #[test]
    fn testing_kinds_are_not_offered_but_are_added_by_id() {
        let text = plumcraft_text::engine().schema();
        let testing: Vec<&str> = text.nodes.iter().filter(|d| !offered(d)).map(|d| d.id.as_str()).collect();
        assert!(testing.contains(&"text.widgets") && testing.contains(&"text.slow"), "{testing:?}");
        let g = Graph::new();
        for q in ["", "slow", "every widget", "testing", "text.widgets"] {
            let r = items(&g, &text, &[], None, &json!({"query": q})).unwrap();
            let kinds: Vec<&str> = r["items"].as_array().unwrap().iter().filter_map(|i| i["kind"].as_str()).collect();
            assert!(kinds.iter().all(|k| !testing.contains(k)), "{q:?} offered {kinds:?}");
        }
        assert!(items(&g, &text, &[], None, &json!({"query": "upper"})).unwrap()["total"].as_u64().unwrap() >= 1);
        // the same from an engine not started (`starts`)
        let starts: Vec<&NodeDef> = text.nodes.iter().collect();
        let r = items(&g, &SchemaReply::default(), &starts, None, &json!({"query": "slow"})).unwrap();
        assert_eq!(r["total"], 0, "{r}");
        // added by id all the same
        let mut s = plumcraft_session::Session::new(text.clone());
        let n = s.execute("node.add", &json!({"kind": "text.widgets", "pos": [0, 0]})).unwrap();
        assert!(n["id"].is_string(), "{n}");
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
