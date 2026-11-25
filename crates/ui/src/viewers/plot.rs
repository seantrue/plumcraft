//! Tables plotted by the core's chart layout (note 26 slice 5; the maintainer's decision 4A: the
//! egui editor paints the same display list as the native shells, its own egui_plot plot retired).
//! What plots is the core's rule ([`plumcraft_editor_core::chart::plotted`]): an (N, k) table array
//! whose columns name points (`x`, `y`), segments (`x0 y0 x1 y1`), a histogram (`centre`,
//! `count`: columns), or any (N, 2) table. The core lays the chart out at the view's size
//! ([`plumcraft_editor_core::chart::layout`]); here it is painted, nothing computed.
//!
//! Laid out again only when the value or the size changes.

use plumcraft_editor_core::chart::{self, Kind};
use plumcraft_proto::data::Form;
use plumcraft_proto::display::Display;
use serde_json::{Value, json};

use super::{ViewInput, Viewer};
use crate::canvas::Action;
use crate::media::key_of;
use crate::theme::Tokens;

/// What a display plots as, and its columns' names.
fn plotted(d: &Display) -> Option<(Kind, Vec<String>)> {
    let f = &d.full.as_ref()?.meta.form;
    // any array that plots, flagged a table or not (FR6, note 30)
    let kind = chart::plottable(f)?;
    let names = match f {
        Form::Array { names, .. } => names.clone(),
        _ => Vec::new(),
    };
    Some((kind, names))
}

/// Whether this viewer shows `d`: what the core offers (FR6: its registry decides).
pub fn accepts(d: &Display) -> bool {
    plumcraft_editor_core::viewers::offered(d).iter().any(|(id, _)| *id == "array.plot")
}

/// The plot viewer: the core's display list for the value, at the size last drawn.
#[derive(Default)]
pub struct TablePlot {
    key: Option<String>,
    plot: Option<(Kind, Vec<String>)>,
    /// The laid-out list, the size it was laid out for, and the range and selection drawn.
    list: Option<(Value, (u32, u32))>,
    seen: String,
    error: Option<String>,
}

/// The core's view of a chart from a viewer's input (its range and its value's selection).
fn seen_of(v: &ViewInput<'_>) -> String {
    format!("{:?}{:?}", v.chart_range, v.selection)
}

impl TablePlot {
    /// The display list at `size`, laid out again when the size changed.
    fn list_at(&mut self, v: &ViewInput<'_>, size: egui::Vec2) -> Option<&Value> {
        let want = (size.x.round().max(1.0) as u32, size.y.round().max(1.0) as u32);
        let seen = seen_of(v);
        if self.list.as_ref().is_none_or(|(_, s)| *s != want) || self.seen != seen {
            self.seen = seen;
            let (kind, names) = self.plot.as_ref()?;
            let b = v.full?;
            use plumcraft_session::chart_view::Selection;
            let selected = v.selection.map(|s| match s {
                Selection::Rows { rows } => {
                    let n = b.shape.first().copied().unwrap_or(0) as usize;
                    let mut m = vec![false; n.min(50_000_000)];
                    for r in rows {
                        if let Some(x) = m.get_mut(*r as usize) {
                            *x = true;
                        }
                    }
                    m
                }
                Selection::Brush { column, span } => chart::column_values(kind, names, &b.dtype, &b.shape, &b.bytes, column.as_deref())
                    .map(|vals| vals.iter().map(|x| span.contains(*x)).collect())
                    .unwrap_or_default(),
            });
            let view = chart::View { range: v.chart_range, selected };
            match chart::layout_in(kind, names, &b.dtype, &b.shape, &b.bytes, (f64::from(want.0), f64::from(want.1)), &view) {
                Ok(l) => {
                    self.list = Some((l, want));
                    self.error = None;
                }
                Err(e) => {
                    self.error = Some(e);
                    self.list = None;
                }
            }
        }
        self.list.as_ref().map(|(l, _)| l)
    }
}

impl Viewer for TablePlot {
    fn sync(&mut self, _: &egui::Context, v: &ViewInput<'_>) {
        let key = key_of(v.display);
        if self.key.as_ref() == Some(&key) && self.plot.is_some() {
            return;
        }
        self.key = Some(key);
        self.plot = plotted(v.display);
        self.list = None;
    }

    fn preview(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, max: egui::Vec2, tokens: &Tokens) {
        if v.full.is_none() || self.plot.is_none() {
            return super::preview_default(ui, v, max, tokens);
        }
        let size = egui::vec2(max.x.max(chart::MIN_SIDE as f32), max.y.max(chart::MIN_SIDE as f32));
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        if let Some(list) = self.list_at(v, size).cloned() {
            paint(ui, rect, &list, tokens);
        }
    }

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        if v.full.is_none() || self.plot.is_none() {
            let text = if v.missing { "the values are gone (reaped); rerun to publish them" } else { "no numbers to plot" };
            ui.label(egui::RichText::new(text).color(tokens.text_dim));
            return Vec::new();
        }
        let w = ui.available_width().max(chart::MIN_SIDE as f32);
        let h = (w * 0.75).clamp(chart::MIN_SIDE as f32, ui.available_height().max(chart::MIN_SIDE as f32));
        match self.list_at(v, egui::vec2(w, h)).cloned() {
            Some(list) => {
                let what = match list["kind"].as_str() {
                    Some("columns") => "columns",
                    Some("segments") => "segments",
                    _ => "points",
                };
                ui.label(egui::RichText::new(format!("{} {what} of {} rows", list["drawn"], list["rows"])).small().color(tokens.text_dim));
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
                paint(ui, rect, &list, tokens);
            }
            None => {
                ui.label(egui::RichText::new(self.error.clone().unwrap_or_else(|| "no numbers to plot".into())).color(tokens.text_dim));
            }
        }
        Vec::new()
    }

    fn inspect(&self) -> Value {
        let l = self.list.as_ref().map(|(l, _)| l);
        json!({"kind": "plot", "drawn": l.map(|l| l["drawn"].clone()), "rows": l.map(|l| l["rows"].clone()), "marks": l.map(|l| l["kind"].clone()),
               "sameScale": l.map(|l| l["sameScale"].clone())})
    }
}

/// Whether `d` is a table-flagged array.
pub fn table_array(d: &Display) -> bool {
    d.full.as_ref().is_some_and(|f| matches!(f.meta.form, Form::Array { table: true, .. }))
}

/// Whether the table chart shows `d`: what the core offers (a TABLE, a CSV, rows, or any numeric
/// array that reads as a table: FR6).
pub fn accepts_table(d: &Display) -> bool {
    plumcraft_editor_core::viewers::offered(d).iter().any(|(id, _)| *id == "table.chart")
}

/// A table's chart (note 26 slices 5 to 7): the spec the core keeps for the port (`viewer.chart`),
/// or the one it picks from the columns, laid out by the core ([`plumcraft_editor_core::chart_kinds`]).
#[derive(Default)]
pub struct TableChart {
    key: Option<String>,
    table: Option<plumcraft_media::table::Table>,
    list: Option<(Value, (u32, u32), Value)>,
    seen: String,
    error: Option<String>,
}

impl TableChart {
    fn list_at(&mut self, v: &ViewInput<'_>, size: egui::Vec2) -> Option<&Value> {
        let want = (size.x.round().max(1.0) as u32, size.y.round().max(1.0) as u32);
        let t = self.table.as_ref()?;
        let spec = v.chart.cloned().unwrap_or_else(|| plumcraft_editor_core::chart_kinds::auto_spec(t));
        let seen = seen_of(v);
        if self.list.as_ref().is_none_or(|(_, s, sp)| *s != want || *sp != spec) || self.seen != seen {
            self.seen = seen;
            let x = spec.get("x").and_then(Value::as_str);
            let selected = v.selection.map(|s| plumcraft_editor_core::viewers::mask_of(s, t, x));
            let view = chart::View { range: v.chart_range, selected };
            match plumcraft_editor_core::chart_kinds::layout_in(&spec, t, (f64::from(want.0), f64::from(want.1)), &view) {
                Ok(l) => {
                    self.list = Some((l, want, spec));
                    self.error = None;
                }
                Err(e) => {
                    self.error = Some(e);
                    self.list = None;
                }
            }
        }
        self.list.as_ref().map(|(l, ..)| l)
    }
}

impl Viewer for TableChart {
    fn sync(&mut self, _: &egui::Context, v: &ViewInput<'_>) {
        let key = key_of(v.display);
        if self.key.as_ref() == Some(&key) && self.table.is_some() {
            return;
        }
        use plumcraft_media::table::Table;
        let t = match (&v.display.value, v.full, v.display.full.as_ref()) {
            (Some(val), _, _) => Table::from_value(val),
            // the core's one rule: a numeric array of any rank, a CSV, or a TABLE's JSON
            (None, Some(b), Some(f)) => Table::of(&f.meta.form, &f.meta.media, &b.bytes),
            (None, Some(b), None) => Table::from_bytes(&b.bytes),
            (None, None, _) => return,
        };
        self.key = Some(key);
        self.list = None;
        match t {
            Ok(t) => {
                self.table = Some(t);
                self.error = None;
            }
            Err(e) => {
                self.table = None;
                self.error = Some(e);
            }
        }
    }

    fn preview(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, max: egui::Vec2, tokens: &Tokens) {
        if self.table.is_none() {
            return super::preview_default(ui, v, max, tokens);
        }
        let size = egui::vec2(max.x.max(chart::MIN_SIDE as f32), max.y.max(chart::MIN_SIDE as f32));
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        if let Some(list) = self.list_at(v, size).cloned() {
            paint(ui, rect, &list, tokens);
        }
    }

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        let w = ui.available_width().max(chart::MIN_SIDE as f32);
        let h = (w * 0.66).clamp(chart::MIN_SIDE as f32, ui.available_height().max(chart::MIN_SIDE as f32));
        match self.list_at(v, egui::vec2(w, h)).cloned() {
            Some(list) => {
                ui.label(egui::RichText::new(list["a11y"]["summary"].as_str().unwrap_or_default()).small().color(tokens.text_dim));
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
                paint(ui, rect, &list, tokens);
            }
            None => {
                ui.label(egui::RichText::new(self.error.clone().unwrap_or_else(|| "no table to chart".into())).color(tokens.text_dim));
            }
        }
        Vec::new()
    }

    fn inspect(&self) -> Value {
        let l = self.list.as_ref();
        json!({"kind": "chart", "chart": l.map(|l| l.0["kind"].clone()), "spec": l.map(|l| l.2.clone()), "marks": l.map(|l| l.0["marks"].as_array().map_or(0, Vec::len))})
    }
}

/// A mark's colour: the list's (`#rrggbb`), else the theme's accent.
fn colour_of(m: &Value, tokens: &Tokens) -> egui::Color32 {
    m["color"]
        .as_str()
        .and_then(|c| c.strip_prefix('#'))
        .filter(|h| h.len() == 6)
        .and_then(|h| u32::from_str_radix(h, 16).ok())
        .map_or(tokens.accent, |n| egui::Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

fn nums(v: &Value) -> Vec<f32> {
    v.as_array().map(|a| a.iter().filter_map(Value::as_f64).map(|x| x as f32).collect()).unwrap_or_default()
}

/// The core's display list painted in `rect` (its points are the list's own, from `rect.min`).
pub fn paint(ui: &egui::Ui, rect: egui::Rect, list: &Value, tokens: &Tokens) {
    let p = ui.painter_at(rect);
    let o = rect.min.to_vec2();
    let at = |x: f32, y: f32| egui::pos2(x, y) + o;
    let [px, py, pw, ph] = nums(&list["plot"])[..] else { return };
    let font = egui::FontId::proportional(list["font"].as_f64().unwrap_or(11.0) as f32);
    let grid = egui::Stroke::new(0.5, tokens.separator);
    for x in nums(&list["grid"]["x"]) {
        p.line_segment([at(x, py), at(x, py + ph)], grid);
    }
    for y in nums(&list["grid"]["y"]) {
        p.line_segment([at(px, y), at(px + pw, y)], grid);
    }
    // the marks inside the plot when the list says so (a zoomed chart's run past it), over the
    // value's brushed span (note 26 slice 8)
    let outer = p.clone();
    let p = if list["clip"].as_bool() == Some(true) { p.with_clip_rect(egui::Rect::from_min_size(at(px, py), egui::vec2(pw, ph))) } else { p };
    if let [a, b] = nums(&list["selection"]["x"])[..] {
        p.rect_filled(egui::Rect::from_min_max(at(a, py), at(b, py + ph)), 0.0, egui::Color32::from_rgba_unmultiplied(255, 127, 14, 30));
    }
    for m in list["marks"].as_array().into_iter().flatten() {
        let colour = colour_of(m, tokens);
        match m["kind"].as_str() {
            Some("points") => {
                let r = m["r"].as_f64().unwrap_or(2.0) as f32;
                for c in nums(&m["xy"]).as_chunks::<2>().0.iter() {
                    p.circle_filled(at(c[0], c[1]), r, colour);
                }
            }
            Some("segments") => {
                let s = egui::Stroke::new(m["width"].as_f64().unwrap_or(1.0) as f32, colour);
                for c in nums(&m["xy"]).as_chunks::<4>().0.iter() {
                    p.line_segment([at(c[0], c[1]), at(c[2], c[3])], s);
                }
            }
            Some("rects") => {
                for c in nums(&m["rects"]).as_chunks::<4>().0.iter() {
                    p.rect_filled(egui::Rect::from_min_size(at(c[0], c[1]), egui::vec2(c[2], c[3])), 0.0, colour.gamma_multiply(0.85));
                }
            }
            Some("line") => {
                let pts: Vec<egui::Pos2> = nums(&m["xy"]).as_chunks::<2>().0.iter().map(|c| at(c[0], c[1])).collect();
                p.add(egui::Shape::line(pts, egui::Stroke::new(m["width"].as_f64().unwrap_or(1.5) as f32, colour)));
            }
            // a stacked band: its top left to right, then its base back (not convex: drawn as
            // a quad between each pair of x, which egui fills without a tessellator of its own)
            Some("area") => {
                let pts: Vec<egui::Pos2> = nums(&m["xy"]).as_chunks::<2>().0.iter().map(|c| at(c[0], c[1])).collect();
                let n = pts.len() / 2;
                let fill = colour.gamma_multiply(0.8);
                for i in 0..n.saturating_sub(1) {
                    let quad = vec![pts[i], pts[i + 1], pts[2 * n - 2 - i], pts[2 * n - 1 - i]];
                    p.add(egui::Shape::convex_polygon(quad, fill, egui::Stroke::NONE));
                }
                let top: Vec<egui::Pos2> = pts.iter().take(n).copied().collect();
                p.add(egui::Shape::line(top, egui::Stroke::new(1.0, colour)));
            }
            Some("labels") => {
                for l in m["labels"].as_array().into_iter().flatten() {
                    let (Some(x), Some(y)) = (l["x"].as_f64(), l["y"].as_f64()) else { continue };
                    let pos = at(x as f32, y as f32);
                    p.text(pos, egui::Align2::LEFT_TOP, l["text"].as_str().unwrap_or_default(), font.clone(), egui::Color32::WHITE);
                    if let Some(d) = l["detail"].as_str() {
                        p.text(pos + egui::vec2(0.0, font.size + 2.0), egui::Align2::LEFT_TOP, d, font.clone(), egui::Color32::from_white_alpha(200));
                    }
                }
            }
            _ => {}
        }
    }
    let p = outer;
    for l in list["legend"].as_array().into_iter().flatten() {
        let (Some(x), Some(y)) = (l["x"].as_f64(), l["y"].as_f64()) else { continue };
        let pos = at(x as f32, y as f32);
        p.rect_filled(egui::Rect::from_min_size(pos, egui::vec2(10.0, 10.0)), 2.0, colour_of(l, tokens));
        p.text(pos + egui::vec2(15.0, 5.0), egui::Align2::LEFT_CENTER, l["text"].as_str().unwrap_or_default(), font.clone(), tokens.text);
    }
    if list["axes"].as_array().is_none_or(Vec::is_empty) {
        return;
    }
    let axis = egui::Stroke::new(1.0, tokens.text_faint);
    p.line_segment([at(px, py), at(px, py + ph)], axis);
    p.line_segment([at(px, py + ph), at(px + pw, py + ph)], axis);
    for a in list["axes"].as_array().into_iter().flatten() {
        let bottom = a["side"].as_str() == Some("bottom");
        for t in a["ticks"].as_array().into_iter().flatten() {
            let (Some(v), Some(text)) = (t["at"].as_f64(), t["text"].as_str()) else { continue };
            let v = v as f32;
            if bottom {
                p.text(at(v, py + ph + 4.0), egui::Align2::CENTER_TOP, text, font.clone(), tokens.text_dim);
            } else {
                p.text(at(px - 5.0, v), egui::Align2::RIGHT_CENTER, text, font.clone(), tokens.text_dim);
            }
        }
        let title = a["title"].as_str().unwrap_or_default();
        if bottom {
            p.text(at(px + pw / 2.0, py + ph + font.size + 10.0), egui::Align2::CENTER_TOP, title, font.clone(), tokens.text);
        } else {
            // the y axis's title beside its top (egui's painter draws no turned text)
            p.text(at(px + 4.0, py + 2.0), egui::Align2::LEFT_TOP, title, font.clone(), tokens.text_dim);
        }
    }
}
