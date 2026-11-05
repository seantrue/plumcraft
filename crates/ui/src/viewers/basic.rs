//! The simple viewers: a scalar, text, a table, a JSON tree, a list's items, and raw bytes.
//! Each draws only what is on screen (rows by `ScrollArea::show_rows`), so a big value scrolls
//! without slowing the editor, and each reads its value as hostile input.

use plumcraft_proto::display::Rendition;
use serde_json::{Value, json};

use super::{ViewInput, Viewer, fitted_image};
use crate::canvas::Action;
use crate::theme::Tokens;

/// The most JSON nodes the tree draws.
pub const MAX_TREE_NODES: usize = 5000;
/// The most bytes the hex view reads.
pub const MAX_HEX_BYTES: usize = 64 << 20;

pub(crate) fn dim(ui: &mut egui::Ui, text: impl Into<String>, tokens: &Tokens) {
    ui.label(egui::RichText::new(text.into()).color(tokens.text_dim));
}

/// The value's text: the full bytes when read, else the engine's excerpt.
pub(crate) fn text_of(v: &ViewInput<'_>) -> Option<String> {
    v.full.map(|b| String::from_utf8_lossy(&b.bytes).into_owned()).or_else(|| {
        v.display.renditions.iter().find_map(|r| match r {
            Rendition::Excerpt { text, .. } => Some(text.clone()),
            _ => None,
        })
    })
}

/// The value as JSON: its inline value, else its full bytes parsed.
fn json_of(v: &ViewInput<'_>) -> Option<Value> {
    v.display.value.clone().or_else(|| v.full.and_then(|b| serde_json::from_slice(&b.bytes).ok()))
}

/// Whether a value is rows: a non-empty list of lists (Table to Rows', a spreadsheet's Read
/// range).
pub fn is_rows(v: &Value) -> bool {
    v.as_array().is_some_and(|r| !r.is_empty() && r.iter().all(Value::is_array))
}

/// A value as a table (`{"columns": [{name, values}]}`): a table as it is; rows with the first
/// row as the column names when it is all text (and there is more than one row), else `c1`,
/// `c2`, …. Anything else: `None`.
pub fn as_table(v: Value) -> Option<Value> {
    if v.get("columns").is_some_and(Value::is_array) {
        return Some(v);
    }
    if !is_rows(&v) {
        return None;
    }
    let rows: Vec<Vec<Value>> = v.as_array()?.iter().map(|r| r.as_array().cloned().unwrap_or_default()).collect();
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let header = rows.len() > 1 && rows.first().is_some_and(|r| !r.is_empty() && r.iter().all(Value::is_string));
    let names: Vec<String> = (0..width)
        .map(|j| {
            if header {
                rows.first().and_then(|r| r.get(j)).and_then(Value::as_str).map_or_else(|| format!("c{}", j + 1), str::to_owned)
            } else {
                format!("c{}", j + 1)
            }
        })
        .collect();
    let body = if header { rows.get(1..).unwrap_or_default() } else { &rows[..] };
    let columns: Vec<Value> = names
        .into_iter()
        .enumerate()
        .map(|(j, name)| json!({"name": name, "values": body.iter().map(|r| r.get(j).cloned().unwrap_or(Value::Null)).collect::<Vec<_>>()}))
        .collect();
    Some(json!({"columns": columns}))
}

// ---------- scalar ----------

/// A number, a string or a flag, large.
pub struct Scalar;

impl Viewer for Scalar {
    fn sync(&mut self, _: &egui::Context, _: &ViewInput<'_>) {}

    fn preview(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, _: egui::Vec2, tokens: &Tokens) {
        let shown = v.display.value.as_ref().map_or_else(|| v.display.summary.clone(), |x| x.as_str().map_or_else(|| x.to_string(), str::to_owned));
        ui.label(egui::RichText::new(shown).monospace().size(18.0).color(tokens.text));
    }

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        match &v.display.value {
            Some(x) => {
                let shown = x.as_str().map_or_else(|| x.to_string(), str::to_owned);
                ui.label(egui::RichText::new(shown).monospace().size(28.0).color(tokens.text));
                ui.label(egui::RichText::new(&v.display.ty).small().color(tokens.text_faint));
            }
            None => dim(ui, "no value", tokens),
        }
        Vec::new()
    }
}

// ---------- text ----------

/// Text with line numbers, wrapping and find.
#[derive(Default)]
pub struct Text {
    wrap: bool,
    find: String,
}

impl Viewer for Text {
    fn sync(&mut self, _: &egui::Context, _: &ViewInput<'_>) {}

    fn preview(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, max: egui::Vec2, tokens: &Tokens) {
        let t = text_of(v).unwrap_or_default();
        let lines: Vec<&str> = t.lines().take(8).collect();
        ui.set_max_width(max.x);
        ui.label(egui::RichText::new(lines.join("\n")).monospace().small().color(tokens.text));
    }

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        let Some(t) = text_of(v) else {
            dim(ui, "no text", tokens);
            return Vec::new();
        };
        let lines: Vec<&str> = t.lines().collect();
        let hits = if self.find.is_empty() { 0 } else { t.matches(self.find.as_str()).count() };
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.wrap, "Wrap");
            ui.add(egui::TextEdit::singleline(&mut self.find).hint_text("Find").desired_width(140.0));
            if !self.find.is_empty() {
                ui.label(egui::RichText::new(format!("{hits} found")).small().color(tokens.text_dim));
            }
            ui.label(egui::RichText::new(format!("{} lines", lines.len())).small().color(tokens.text_faint));
        });
        let colour = if v.missing { tokens.text_faint } else { tokens.text };
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
        let width = lines.len().to_string().len();
        let scroll = if self.wrap { egui::ScrollArea::vertical() } else { egui::ScrollArea::both() };
        scroll.id_salt("viewer-text").auto_shrink([false, false]).show_rows(ui, row_h, lines.len(), |ui, range| {
            for i in range {
                let line = lines.get(i).copied().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{:>width$}", i + 1)).monospace().color(tokens.text_faint));
                    let mut rt = egui::RichText::new(line).monospace().color(colour);
                    if !self.find.is_empty() && line.contains(self.find.as_str()) {
                        rt = rt.background_color(tokens.highlight.gamma_multiply(0.35));
                    }
                    ui.add(egui::Label::new(rt).wrap_mode(if self.wrap { egui::TextWrapMode::Wrap } else { egui::TextWrapMode::Extend }));
                });
            }
        });
        Vec::new()
    }

    fn inspect(&self) -> Value {
        json!({"wrap": self.wrap, "find": self.find})
    }
}

// ---------- table ----------

/// A table (`{"columns": [{name, type, values}]}`): sortable columns, colour by value.
#[derive(Default)]
pub struct Table {
    sort: Option<(usize, bool)>,
    colored: bool,
    /// The port shown (`node.port`): another port's value starts unsorted.
    port: String,
}

/// A cell's text.
fn cell(v: &Value) -> String {
    v.as_str().map_or_else(|| if v.is_null() { String::new() } else { v.to_string() }, str::to_owned)
}

/// A colour between the panel's and the chart's for `t` in 0..1.
pub fn heat(tokens: &Tokens, t: f64) -> egui::Color32 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) as f32 } else { 0.0 };
    let (a, b) = (tokens.panel, tokens.chart);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    egui::Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

impl Viewer for Table {
    fn sync(&mut self, _: &egui::Context, v: &ViewInput<'_>) {
        let port = format!("{}.{}", v.display.node, v.display.port);
        if port != self.port {
            self.sort = None;
            self.port = port;
        }
    }

    /// egui_extras' `TableBuilder`: the header stays put while the rows scroll, each column starts
    /// as wide as its content needs and can be dragged (an auto-sized one leaves right-aligned
    /// numbers no room), and only the rows on screen are laid out.
    /// Wide tables scroll sideways with the header.
    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        use egui_extras::{Column, TableBuilder};
        let Some(t) = json_of(v).and_then(as_table) else {
            dim(ui, "no table", tokens);
            return Vec::new();
        };
        let cols = t.get("columns").and_then(Value::as_array).cloned().unwrap_or_default();
        let values: Vec<Vec<Value>> = cols.iter().map(|c| c.get("values").and_then(Value::as_array).cloned().unwrap_or_default()).collect();
        let rows = values.iter().map(Vec::len).max().unwrap_or(0);
        let mut order: Vec<usize> = (0..rows).collect();
        if let Some((c, asc)) = self.sort
            && let Some(col) = values.get(c)
        {
            order.sort_by(|a, b| {
                let (x, y) = (col.get(*a).unwrap_or(&Value::Null), col.get(*b).unwrap_or(&Value::Null));
                let o = match (x.as_f64(), y.as_f64()) {
                    (Some(p), Some(q)) => p.total_cmp(&q),
                    _ => cell(x).cmp(&cell(y)),
                };
                if asc { o } else { o.reverse() }
            });
        }
        let range: Vec<(f64, f64)> = values
            .iter()
            .map(|c| c.iter().filter_map(Value::as_f64).filter(|f| f.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x))))
            .collect();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.colored, "Colour by value");
            ui.label(egui::RichText::new(format!("{rows} rows × {} columns", cols.len())).small().color(tokens.text_faint));
        });
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        // Each column starts as wide as its name and its first values need (7.5 points a
        // character), then can be dragged.
        let widths: Vec<f32> = cols
            .iter()
            .zip(&values)
            .map(|(c, vs)| {
                let name = c.get("name").and_then(Value::as_str).map_or(1, |n| n.chars().count() + 2);
                let longest = vs.iter().take(200).map(|v| cell(v).chars().count()).max().unwrap_or(0).max(name);
                (longest as f32 * 7.5 + 16.0).clamp(48.0, 240.0)
            })
            .collect();
        let (colored, sort) = (self.colored, self.sort);
        let mut clicked: Option<usize> = None;
        egui::ScrollArea::horizontal().id_salt("viewer-table-h").auto_shrink([false, true]).show(ui, |ui| {
            let mut table = TableBuilder::new(ui)
                .id_salt("viewer-table")
                .striped(true)
                .resizable(true)
                .auto_shrink([false, false])
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::auto().at_least(36.0));
            for w in &widths {
                table = table.column(Column::initial(*w).at_least(40.0).clip(true));
            }
            table
                .header(row_h, |mut header| {
                    header.col(|ui| {
                        ui.label(egui::RichText::new("#").small().color(tokens.text_faint));
                    });
                    for (j, c) in cols.iter().enumerate() {
                        header.col(|ui| {
                            let name = c.get("name").and_then(Value::as_str).unwrap_or("?");
                            // (the editor's fonts have no arrows)
                            let arrow = match sort {
                                Some((s, true)) if s == j => " ^",
                                Some((s, false)) if s == j => " v",
                                _ => "",
                            };
                            if ui
                                .add(egui::Button::new(egui::RichText::new(format!("{name}{arrow}")).strong()).frame(false))
                                .on_hover_text("Click to sort")
                                .clicked()
                            {
                                clicked = Some(j);
                            }
                        });
                    }
                })
                .body(|body| {
                    body.rows(row_h, rows, |mut row| {
                        let i = order.get(row.index()).copied().unwrap_or(row.index());
                        row.col(|ui| {
                            ui.label(egui::RichText::new((i + 1).to_string()).small().color(tokens.text_faint));
                        });
                        for (j, col) in values.iter().enumerate() {
                            let x = col.get(i).cloned().unwrap_or(Value::Null);
                            let (lo, hi) = range.get(j).copied().unwrap_or((0.0, 0.0));
                            row.col(|ui| {
                                if colored
                                    && let Some(f) = x.as_f64()
                                    && hi > lo
                                {
                                    ui.painter().rect_filled(ui.max_rect().shrink(1.0), 2.0, heat(tokens, (f - lo) / (hi - lo)));
                                }
                                let text = egui::RichText::new(cell(&x)).color(tokens.text);
                                if x.is_number() {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(text));
                                } else {
                                    ui.label(text);
                                }
                            });
                        }
                    });
                });
        });
        if let Some(j) = clicked {
            self.sort = match self.sort {
                Some((s, true)) if s == j => Some((j, false)),
                Some((s, false)) if s == j => None,
                _ => Some((j, true)),
            };
        }
        Vec::new()
    }

    fn inspect(&self) -> Value {
        json!({"sort": self.sort.map(|(c, asc)| json!({"column": c, "ascending": asc})), "colored": self.colored})
    }
}

// ---------- YAML ----------

/// JSON shown as YAML ([`super::yaml`]), every line in view at a time so a large value scrolls.
#[derive(Default)]
pub struct Yaml {
    key: String,
    value: Option<Value>,
    lines: Vec<String>,
}

impl Viewer for Yaml {
    fn sync(&mut self, _: &egui::Context, v: &ViewInput<'_>) {
        let key = crate::media::key_of(v.display);
        // the full value may arrive after the display
        if key != self.key || v.display.value != self.value || (self.lines.is_empty() && v.full.is_some()) {
            self.key = key;
            self.value = v.display.value.clone();
            self.lines = json_of(v).map(|j| super::yaml::to_yaml(&j).lines().map(str::to_owned).collect()).unwrap_or_default();
        }
    }

    fn preview(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, max: egui::Vec2, tokens: &Tokens) {
        if self.lines.is_empty() {
            return super::preview_default(ui, v, max, tokens);
        }
        ui.set_max_width(max.x);
        let shown = ((max.y / 14.0) as usize).clamp(1, 12);
        for line in self.lines.iter().take(shown) {
            ui.add(egui::Label::new(egui::RichText::new(line).monospace().small().color(tokens.text)).truncate());
        }
        if self.lines.len() > shown {
            dim(ui, format!("… {} more lines", self.lines.len() - shown), tokens);
        }
    }

    fn full(&mut self, ui: &mut egui::Ui, _: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        if self.lines.is_empty() {
            dim(ui, "not JSON", tokens);
            return Vec::new();
        }
        if ui.small_button("Copy").on_hover_text("Copy this YAML").clicked() {
            ui.ctx().copy_text(self.lines.join("\n"));
        }
        let row = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::both().id_salt("viewer-yaml").auto_shrink([false, false]).show_rows(ui, row, self.lines.len(), |ui, rows| {
            for line in self.lines.get(rows).unwrap_or_default() {
                ui.add(egui::Label::new(egui::RichText::new(line).monospace().color(tokens.text)).extend());
            }
        });
        Vec::new()
    }

    fn inspect(&self) -> Value {
        json!({"lines": self.lines.len(), "first": self.lines.first()})
    }
}

// ---------- tree ----------

/// JSON as a collapsible tree, with each object's and list's size.
#[derive(Default)]
pub struct Tree {
    raw: bool,
}

fn tree_node(ui: &mut egui::Ui, key: &str, v: &Value, path: &str, budget: &mut usize, tokens: &Tokens) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;
    match v {
        Value::Object(m) => {
            egui::CollapsingHeader::new(format!("{key}  {{{}}}", m.len())).id_salt(path).default_open(path.matches('/').count() < 2).show(ui, |ui| {
                for (k, x) in m {
                    tree_node(ui, k, x, &format!("{path}/{k}"), budget, tokens);
                }
            });
        }
        Value::Array(a) => {
            egui::CollapsingHeader::new(format!("{key}  [{}]", a.len())).id_salt(path).default_open(path.matches('/').count() < 2 && a.len() <= 32).show(
                ui,
                |ui| {
                    for (i, x) in a.iter().enumerate() {
                        tree_node(ui, &i.to_string(), x, &format!("{path}/{i}"), budget, tokens);
                    }
                },
            );
        }
        leaf => {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(key).color(tokens.text_dim));
                ui.label(egui::RichText::new(leaf.to_string()).monospace().color(tokens.text));
            });
        }
    }
}

impl Viewer for Tree {
    fn sync(&mut self, _: &egui::Context, _: &ViewInput<'_>) {}

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        let Some(j) = json_of(v) else {
            dim(ui, "not JSON", tokens);
            return Vec::new();
        };
        ui.checkbox(&mut self.raw, "Raw JSON");
        egui::ScrollArea::both().id_salt("viewer-tree").auto_shrink([false, false]).show(ui, |ui| {
            if self.raw {
                let text = serde_json::to_string_pretty(&j).unwrap_or_default();
                ui.label(egui::RichText::new(text).monospace());
            } else {
                let mut budget = MAX_TREE_NODES;
                tree_node(ui, "value", &j, "", &mut budget, tokens);
                if budget == 0 {
                    dim(ui, format!("(the first {MAX_TREE_NODES} entries)"), tokens);
                }
            }
        });
        Vec::new()
    }

    fn inspect(&self) -> Value {
        json!({"raw": self.raw})
    }
}

// ---------- a list's items ----------

/// A list's items as a grid of their thumbnails and labels; a click shows that item.
pub struct Items;

/// A cell's content width, and all it takes with its frame and the gap.
const ITEM_INNER: f32 = 132.0;
const ITEM_W: f32 = 154.0;

impl Viewer for Items {
    fn sync(&mut self, _: &egui::Context, _: &ViewInput<'_>) {}

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        let mut asks = Vec::new();
        let d = v.display;
        ui.label(egui::RichText::new(format!("{} items", d.items)).small().color(tokens.text_faint));
        egui::ScrollArea::vertical().id_salt("viewer-items").auto_shrink([false, false]).show(ui, |ui| {
            // Rows of whole cells that fit: a wrapped row would widen the panel, not wrap, in here.
            let columns = ((ui.available_width() / ITEM_W).floor() as usize).max(1);
            for (row, chunk) in d.sheet.chunks(columns).enumerate() {
                ui.horizontal(|ui| {
                    for (k, c) in chunk.iter().enumerate() {
                        let j = row * columns + k;
                        let on = j as i64 == d.index;
                        let frame = egui::Frame::group(ui.style()).fill(if on { tokens.highlight.gamma_multiply(0.25) } else { tokens.panel });
                        let r = frame
                            .show(ui, |ui| {
                                ui.set_width(ITEM_INNER);
                                ui.vertical(|ui| {
                                    match v.sheet.get(j).and_then(Option::as_ref) {
                                        Some(t) => fitted_image(ui, t, egui::vec2(ITEM_INNER - 8.0, 90.0)),
                                        None => {
                                            ui.label(egui::RichText::new(&c.summary).small().color(tokens.text_dim));
                                        }
                                    }
                                    let label = if c.label.is_empty() { format!("{}", j + 1) } else { c.label.clone() };
                                    ui.label(egui::RichText::new(label).small().color(tokens.text));
                                });
                            })
                            .response
                            .interact(egui::Sense::click());
                        if r.on_hover_text(&c.summary).clicked() {
                            // from the sheet, a click shows that item alone
                            if v.sheet.iter().any(Option::is_some) {
                                asks.push(("viewer.sheet", json!({"on": false, "index": j})));
                            } else if !on {
                                asks.push(("run.item", json!({"index": j})));
                            }
                        }
                    }
                });
            }
            if (d.sheet.len() as u64) < d.items {
                dim(ui, format!("… and {} more", d.items - d.sheet.len() as u64), tokens);
            }
        });
        asks
    }
}

// ---------- bytes ----------

/// Raw bytes: offset, 16 bytes in hex, and their ASCII.
pub struct Hex;

/// One hex line: offset, bytes, ASCII.
pub fn hex_line(offset: usize, bytes: &[u8]) -> String {
    let hex: Vec<String> = (0..16).map(|i| bytes.get(i).map_or_else(|| "  ".to_owned(), |b| format!("{b:02x}"))).collect();
    let ascii: String = bytes.iter().map(|b| if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '.' }).collect();
    format!("{offset:08x}  {}  {}  {ascii}", hex[..8].join(" "), hex[8..].join(" "))
}

impl Viewer for Hex {
    fn sync(&mut self, _: &egui::Context, _: &ViewInput<'_>) {}

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        let Some(b) = v.full else {
            dim(ui, if v.missing { "the bytes are gone (reaped); rerun to publish them" } else { "the bytes aren't here" }, tokens);
            return Vec::new();
        };
        let bytes = b.bytes.get(..b.bytes.len().min(MAX_HEX_BYTES)).unwrap_or_default();
        ui.label(egui::RichText::new(format!("{} bytes · {} {:?}", b.bytes.len(), b.dtype, b.shape)).small().color(tokens.text_faint));
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
        let lines = bytes.len().div_ceil(16);
        egui::ScrollArea::both().id_salt("viewer-hex").auto_shrink([false, false]).show_rows(ui, row_h, lines, |ui, range| {
            for i in range {
                let chunk = bytes.get(i * 16..((i + 1) * 16).min(bytes.len())).unwrap_or_default();
                ui.label(egui::RichText::new(hex_line(i * 16, chunk)).monospace().color(tokens.text));
            }
        });
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows show as a table: an all-text first row names the columns; otherwise c1, c2, ….
    #[test]
    fn json_shows_as_yaml_first_with_the_tree_beside_it() {
        use plumcraft_proto::catalogue::Media;
        let d = plumcraft_proto::display::Display { media: Media::Tree, value: Some(json!({"name": "corgi", "toys": ["ball"]})), ..Default::default() };
        let kinds: Vec<&str> = super::super::kinds_for(&d).iter().map(|k| k.id).collect();
        assert_eq!(kinds[..2], ["yaml", "tree"], "{kinds:?}");
        let rows = plumcraft_proto::display::Display { media: Media::Tree, value: Some(json!([["a", "b"], [1, 2]])), ..Default::default() };
        assert_eq!(super::super::kinds_for(&rows)[0].id, "table.grid", "rows are a table");
        fn input(display: &plumcraft_proto::display::Display) -> ViewInput<'_> {
            ViewInput { display, full: None, missing: false, rendition: None, pictures: &[], sheet: &[], offered_views: &[], tools: Default::default() }
        }
        let mut view = Yaml::default();
        view.sync(&egui::Context::default(), &input(&d));
        assert_eq!(view.lines, ["name: corgi", "toys:", "  - ball"]);
        let changed = plumcraft_proto::display::Display { value: Some(json!({"name": "heron"})), ..d.clone() };
        view.sync(&egui::Context::default(), &input(&changed));
        assert_eq!(view.lines, ["name: heron"], "a new value with the same summary still redraws");
    }

    #[test]
    fn rows_read_as_tables() {
        let t = as_table(json!([["letter", "count"], ["a", 3], ["b", 1]])).unwrap();
        assert_eq!(t, json!({"columns": [{"name": "letter", "values": ["a", "b"]}, {"name": "count", "values": [3, 1]}]}));
        let t = as_table(json!([[0.5, 2], [1.5]])).unwrap();
        assert_eq!(t["columns"][1], json!({"name": "c2", "values": [2, null]}));
        assert_eq!(as_table(json!([["just", "names"]])).unwrap()["columns"][0]["name"], "c1", "one row: data, not a header");
        assert!(as_table(json!([1, 2])).is_none() && as_table(json!([])).is_none() && as_table(json!({"a": 1})).is_none());
        let table = json!({"columns": []});
        assert_eq!(as_table(table.clone()), Some(table));
    }

    #[test]
    fn hex_lines_pad_and_hide_control_bytes() {
        let short = hex_line(16, b"AB\n");
        assert!(short.starts_with("00000010  41 42 0a   ") && short.ends_with("  AB."), "{short:?}");
        // a short line pads its hex to the full width; only the ASCII column is shorter
        let full = hex_line(0, &[b'x'; 16]);
        assert_eq!(full.len() - short.len(), 16 - 3);
        assert_eq!(hex_line(0, &[0u8; 16]).len(), hex_line(0, &[]).len() + 16);
    }

    #[test]
    fn heat_runs_from_the_panel_to_the_chart_colour() {
        let t = crate::theme::DEFAULT;
        assert_eq!(heat(&t, 0.0), egui::Color32::from_rgb(t.panel.r(), t.panel.g(), t.panel.b()));
        assert_eq!(heat(&t, 1.0), egui::Color32::from_rgb(t.chart.r(), t.chart.g(), t.chart.b()));
        assert_eq!(heat(&t, f64::NAN), heat(&t, 0.0));
    }
}
