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
    let kind = chart::plotted(f)?;
    let names = match f {
        Form::Array { names, .. } => names.clone(),
        _ => Vec::new(),
    };
    Some((kind, names))
}

/// Whether this viewer shows `d`.
pub fn accepts(d: &Display) -> bool {
    plotted(d).is_some()
}

/// The plot viewer: the core's display list for the value, at the size last drawn.
#[derive(Default)]
pub struct TablePlot {
    key: Option<String>,
    plot: Option<(Kind, Vec<String>)>,
    /// The laid-out list and the size it was laid out for.
    list: Option<(Value, (u32, u32))>,
    error: Option<String>,
}

impl TablePlot {
    /// The display list at `size`, laid out again when the size changed.
    fn list_at(&mut self, v: &ViewInput<'_>, size: egui::Vec2) -> Option<&Value> {
        let want = (size.x.round().max(1.0) as u32, size.y.round().max(1.0) as u32);
        if self.list.as_ref().is_none_or(|(_, s)| *s != want) {
            let (kind, names) = self.plot.as_ref()?;
            let b = v.full?;
            match chart::layout(kind, names, &b.dtype, &b.shape, &b.bytes, (f64::from(want.0), f64::from(want.1))) {
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
    for m in list["marks"].as_array().into_iter().flatten() {
        match m["kind"].as_str() {
            Some("points") => {
                let r = m["r"].as_f64().unwrap_or(2.0) as f32;
                for c in nums(&m["xy"]).as_chunks::<2>().0.iter() {
                    p.circle_filled(at(c[0], c[1]), r, tokens.accent);
                }
            }
            Some("segments") => {
                let s = egui::Stroke::new(m["width"].as_f64().unwrap_or(1.0) as f32, tokens.accent);
                for c in nums(&m["xy"]).as_chunks::<4>().0.iter() {
                    p.line_segment([at(c[0], c[1]), at(c[2], c[3])], s);
                }
            }
            Some("rects") => {
                for c in nums(&m["rects"]).as_chunks::<4>().0.iter() {
                    p.rect_filled(egui::Rect::from_min_size(at(c[0], c[1]), egui::vec2(c[2], c[3])), 0.0, tokens.accent.gamma_multiply(0.85));
                }
            }
            Some("line") => {
                let pts: Vec<egui::Pos2> = nums(&m["xy"]).as_chunks::<2>().0.iter().map(|c| at(c[0], c[1])).collect();
                p.add(egui::Shape::line(pts, egui::Stroke::new(m["width"].as_f64().unwrap_or(1.5) as f32, tokens.accent)));
            }
            _ => {}
        }
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
