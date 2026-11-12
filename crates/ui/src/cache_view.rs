//! What the caches did (spec 14.6): a summary in the status line, and the Cache Report window
//! (View › Cache Report, `run.cacheReport`) with the latest run, the session, each engine, each
//! node and the store. All of it comes from `run.stats`' data, so a script sees the same numbers.
//!
//! A node's result can come from three places, kept apart because they mean different things:
//! **memory** (this process has it), **store** (a persisted record: another run or process made
//! it) and **content** (the same node on the same bytes, however they were made).

use plumcraft_proto::run::{RunStats, TreeUsage};
use plumcraft_session::run::{CacheStats, RunState};
use serde_json::json;

use crate::canvas::Action;
use crate::run_view::format_ms;
use crate::theme::Tokens;

/// `bytes` for a person.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS.get(unit).copied().unwrap_or("TB")) }
}

/// The store in words: its size when the reaper has walked it, and the disk either way.
pub fn tree_text(t: &TreeUsage) -> String {
    let size = match t.bytes {
        Some(b) => format!("{} of {}, {} files", format_bytes(b), format_bytes(t.max_bytes), t.files.unwrap_or(0)),
        None => "size not known yet".to_owned(),
    };
    match t.free_bytes {
        Some(free) => format!("{size}; {} free on disk (it stops writing under {})", format_bytes(free), format_bytes(t.min_free_bytes)),
        None => size,
    }
}

/// The status line's words: how much of the run the caches answered, and what that saved.
pub fn summary(s: &RunStats) -> String {
    let (hits, total) = (s.hits(), s.hits() + s.computed);
    if s.saved_ms > 0.0 { format!("cached {hits} of {total} · saved {}", format_ms(s.saved_ms)) } else { format!("cached {hits} of {total}") }
}

/// The hover text: where the hits came from, and the tree.
fn detail(c: &CacheStats, s: &RunStats) -> String {
    let mut out = format!("{} from memory, {} from the store, {} by content; {} computed", s.from_memory, s.from_store, s.from_content, s.computed);
    if let Some(t) = &s.tree {
        out.push_str(&format!("\nStore: {}", tree_text(t)));
    }
    out.push_str(&format!("\nThis session: {} runs, saved {}\nClick for the Cache Report", c.runs, format_ms(c.session.saved_ms)));
    out
}

/// The status line's part: a link to the report, and a warning when the store refused anything.
/// `after`: something is before it on its line, so a dot goes between.
pub fn status_line(ui: &mut egui::Ui, c: &CacheStats, tokens: &Tokens, after: bool) -> Vec<Action> {
    let mut out = Vec::new();
    let Some(s) = &c.last else { return out };
    if after {
        ui.colored_label(tokens.text_dim, "·");
    }
    if ui.add(egui::Button::new(egui::RichText::new(summary(s)).color(tokens.text_dim)).frame(false)).on_hover_text(detail(c, s)).clicked() {
        out.push(("run.cacheReport", json!({"open": true})));
    }
    let declined = s.declined();
    if declined > 0 {
        let word = if declined == 1 { "write" } else { "writes" };
        let text = format!("store refused {declined} {word}");
        if ui.add(egui::Button::new(egui::RichText::new(text).color(tokens.warning)).frame(false)).on_hover_text(refusals(s)).clicked() {
            out.push(("run.cacheReport", json!({"open": true})));
        }
    } else if let Some(t) = s.tree.as_ref().filter(|t| t.low_disk()) {
        let text = format!("disk low: {} free", format_bytes(t.free_bytes.unwrap_or(0)));
        let hover = format!("{}\nBelow that the store refuses writes, and values stay in memory only.", tree_text(t));
        if ui.add(egui::Button::new(egui::RichText::new(text).color(tokens.warning)).frame(false)).on_hover_text(hover).clicked() {
            out.push(("run.cacheReport", json!({"open": true})));
        }
    }
    out
}

/// Why the store refused writes, in words.
fn refusals(s: &RunStats) -> String {
    let lines: Vec<String> = s
        .store
        .iter()
        .filter_map(|(k, v)| {
            let why = match k.strip_prefix("shm_declined_")? {
                "space" => "the disk is nearly full",
                "budget" => "the store is over its budget",
                "size" => "too large",
                "small" => "too small to share",
                "dtype" => "not a stored type",
                "mode" => "the store is read-only",
                other => other,
            };
            Some(format!("{v}: {why}"))
        })
        .collect();
    format!("{}\nThose values stay in memory, and are lost to other engines and later runs.", lines.join("\n"))
}

/// One summary table.
fn totals(ui: &mut egui::Ui, id: &str, s: &RunStats) {
    egui::Grid::new(id).num_columns(2).striped(true).show(ui, |ui| {
        let mut row = |name: &str, value: String| {
            ui.label(name);
            ui.label(value);
            ui.end_row();
        };
        row("Computed", format!("{} nodes, {}", s.computed, format_ms(s.computed_ms)));
        row("From memory", s.from_memory.to_string());
        row("From the store", s.from_store.to_string());
        row("By content", s.from_content.to_string());
        row("Time saved", format_ms(s.saved_ms));
        row("Records kept", s.kept.to_string());
        row("No record", s.store_missed.to_string());
        row("Dropped from memory", s.evicted.to_string());
    });
}

/// The Cache Report window. Returns the commands it asks for.
pub fn report(ctx: &egui::Context, run: &RunState, tokens: &Tokens) -> Vec<Action> {
    let c = &run.cache;
    let mut open = true;
    egui::Window::new("Cache Report").id(egui::Id::new("plumcraft-cache-report")).open(&mut open).default_size([520.0, 560.0]).show(ctx, |ui| {
        let Some(last) = &c.last else {
            ui.label("No run has reported yet.");
            return;
        };
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Latest run");
            totals(ui, "plumcraft-cache-last", last);
            ui.add_space(8.0);
            ui.heading(format!("This session, {} runs", c.runs));
            totals(ui, "plumcraft-cache-session", &c.session);
            for (engine, s) in &c.by_engine {
                ui.add_space(8.0);
                ui.heading(format!("Engine {engine}"));
                totals(ui, &format!("plumcraft-cache-{engine}"), s);
            }
            ui.add_space(8.0);
            ui.heading("Store");
            match &c.session.tree {
                Some(t) if t.low_disk() => ui.colored_label(tokens.warning, tree_text(t)),
                Some(t) => ui.label(tree_text(t)),
                None => ui.colored_label(tokens.text_dim, "This engine doesn't report its store."),
            };
            if c.session.declined() > 0 {
                ui.colored_label(tokens.warning, refusals(&c.session));
            }
            if !c.session.store.is_empty() {
                egui::Grid::new("plumcraft-cache-counters").num_columns(2).striped(true).show(ui, |ui| {
                    for (k, v) in &c.session.store {
                        ui.label(k.strip_prefix("shm_").unwrap_or(k));
                        ui.label(v.to_string());
                        ui.end_row();
                    }
                });
            }
            ui.add_space(8.0);
            ui.heading("Nodes, latest run");
            egui::Grid::new("plumcraft-cache-nodes").num_columns(4).striped(true).show(ui, |ui| {
                for head in ["Node", "Source", "This run", "To compute"] {
                    ui.strong(head);
                }
                ui.end_row();
                for (id, n) in run.nodes.iter().filter(|(_, n)| n.state.is_some()) {
                    ui.label(id);
                    ui.label(n.source.as_deref().unwrap_or("computed"));
                    ui.label(n.ms.map_or_else(|| "—".to_owned(), format_ms));
                    ui.label(n.computed_ms.map_or_else(|| "—".to_owned(), format_ms));
                    ui.end_row();
                }
            });
        });
    });
    if open { Vec::new() } else { vec![("run.cacheReport", json!({"open": false}))] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_says_how_much_was_answered() {
        let s = RunStats { computed: 3, from_memory: 8, from_store: 3, from_content: 1, saved_ms: 3200.0, ..Default::default() };
        assert_eq!(summary(&s), "cached 12 of 15 · saved 3.20 s");
        assert_eq!(summary(&RunStats { computed: 2, ..Default::default() }), "cached 0 of 2");
    }

    #[test]
    fn bytes_read_as_a_person_would() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(16 << 30), "16.0 GB");
    }

    #[test]
    fn the_store_reads_with_or_without_its_size() {
        const GB: u64 = 1 << 30;
        let t = TreeUsage { bytes: None, files: None, max_bytes: 16 * GB, free_bytes: Some(13 * GB), min_free_bytes: 8 * GB };
        assert_eq!(tree_text(&t), "size not known yet; 13.0 GB free on disk (it stops writing under 8.0 GB)");
        assert!(!t.low_disk());
        let known = TreeUsage { bytes: Some(GB), files: Some(42), free_bytes: Some(9 * GB), ..t };
        assert_eq!(tree_text(&known), "1.0 GB of 16.0 GB, 42 files; 9.0 GB free on disk (it stops writing under 8.0 GB)");
        assert!(known.low_disk(), "a warning before the first refusal");
        assert!(!TreeUsage { free_bytes: None, ..known }.low_disk(), "unknown isn't low");
    }

    #[test]
    fn refusals_say_why() {
        let mut s = RunStats::default();
        s.store.insert("shm_declined_space".into(), 2);
        s.store.insert("shm_hit".into(), 9);
        assert_eq!(s.declined(), 2);
        assert!(refusals(&s).contains("2: the disk is nearly full"));
        assert!(!refusals(&s).contains("9"));
    }
}
