//! The array viewer (ndarray media; ported in spirit from the research software's `viewer.rs`): the pictures
//! of the view the engine sent (one in colour, or one per channel, captioned), sharing one zoom
//! and pan; a picker of the views the value's wire type offers (`session run.view`, a cached
//! rerun brings the new pictures), with `turn` for `angle`; and each channel's exact range, NaN
//! count and histogram. A 1-D array is a plot ([`crate::watched::plot`]), as before.

use std::sync::Arc;

use egui::{pos2, vec2};
use plumcraft_proto::display::{ChannelStats, Display};
use plumcraft_store::Blob;
use serde_json::{Value, json};

use crate::canvas::Action;
use crate::media::ZoomPan;
use crate::picture_tools::{PixelTools, pixel_picture, zoom_bar};
use crate::probe::{self, Grid};
use crate::theme::Tokens;

/// The array viewer's state.
#[derive(Default)]
pub struct ArrayView {
    key: Option<String>,
    /// The pictures: caption and texture.
    pictures: Vec<(String, egui::TextureHandle)>,
    pub zoom: ZoomPan,
    /// Histograms on a log scale.
    pub log: bool,
    /// `angle`'s turn, as the next view asks for it.
    turn: f64,
    /// The views the watched value's wire type offers.
    pub offered: Vec<String>,
    /// The array's size, which the pictures are drawn over: they may be smaller.
    grid: Option<Grid>,
    /// The probe, and the zoom asked for.
    pub tools: PixelTools,
}

impl ArrayView {
    /// Take the watched display, its pictures' textures and its bytes (`gone`: they can't be
    /// read). A new port or shape fits again; a rerun of the same one stays where it is looked at.
    pub fn sync(&mut self, d: Option<&Display>, pictures: Vec<(String, egui::TextureHandle)>, full: Option<&Arc<Blob>>, gone: bool) {
        let grid = d.and_then(probe::shape_of).or_else(|| pictures.first().and_then(|(_, t)| Grid::of_shape(&[t.size()[1] as u64, t.size()[0] as u64])));
        let key = d.map(|d| format!("{}.{}:{grid:?}", d.node, d.port));
        if key != self.key {
            self.key = key;
            self.zoom.fit();
            self.tools.new_value();
        }
        if self.turn == 0.0 {
            self.turn = 1.0;
        }
        self.grid = grid;
        self.tools.source = d.map_or(probe::Source::None, |d| probe::source_of(d, full, gone));
        self.pictures = pictures;
    }

    /// Whether there is anything to draw as pictures (else the caller plots or says so).
    pub fn has_pictures(&self) -> bool {
        !self.pictures.is_empty()
    }

    /// The view picker: the views `offered`, the one shown, and `turn` for `angle`. Returns
    /// `run.view` when the user picks another.
    pub fn view_bar(&mut self, ui: &mut egui::Ui, d: &Display, tokens: &Tokens) -> Option<(&'static str, Value)> {
        let offered = self.offered.clone();
        if offered.is_empty() {
            return None;
        }
        let mut ask = None;
        let shown = if d.view.id.is_empty() { "auto" } else { d.view.id.as_str() };
        if let Some(t) = d.view.params.get("turn").and_then(Value::as_f64) {
            self.turn = t;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("View").color(tokens.text_dim));
            for v in &offered {
                if ui.selectable_label(shown == v, v).on_hover_text(view_tip(v)).clicked() && shown != v {
                    let params = if v == "angle" { json!({"turn": self.turn}) } else { json!({}) };
                    ask = Some(("run.view", json!({"view": v, "params": params})));
                }
            }
            if shown == "angle" {
                ui.label(egui::RichText::new("turn").color(tokens.text_dim));
                let r = ui
                    .add(egui::DragValue::new(&mut self.turn).speed(0.1).range(0.001..=1e6))
                    .on_hover_text("One full turn in the array's units: 1 for turns, 360 for degrees, 6.283 for radians");
                if r.lost_focus() || r.drag_stopped() {
                    ask = Some(("run.view", json!({"view": "angle", "params": {"turn": self.turn}})));
                }
            }
        });
        ask
    }

    /// The pictures (one wide, or two to a row), sharing one zoom and pan and one probe, the
    /// zoom bar above them and the probe's readout below. Returns the commands they ask for.
    pub fn pictures_ui(&mut self, ui: &mut egui::Ui, tokens: &Tokens) -> Vec<Action> {
        self.tools.asks.extend(zoom_bar(ui));
        self.tools.begin();
        let (zoom, tools) = (&mut self.zoom, &mut self.tools);
        let Some(grid) = self.grid else { return std::mem::take(&mut tools.asks) };
        match self.pictures.as_slice() {
            [] => {}
            [(caption, t)] => picture(ui, zoom, tools, caption, t, grid, tokens),
            many => {
                for pair in many.chunks(2) {
                    ui.columns(2, |cols| {
                        for ((caption, t), col) in pair.iter().zip(cols.iter_mut()) {
                            picture(col, zoom, tools, caption, t, grid, tokens);
                        }
                    });
                }
            }
        }
        tools.status(ui, grid, tokens);
        tools.readout(ui, tokens);
        std::mem::take(&mut tools.asks)
    }

    /// Each channel's exact range, NaN count and histogram.
    pub fn stats_ui(&mut self, ui: &mut egui::Ui, d: &Display, tokens: &Tokens) {
        if d.stats.channels.is_empty() {
            return;
        }
        egui::CollapsingHeader::new(format!("Channels ({})", d.stats.channels.len())).id_salt("plumcraft-array-channels").default_open(true).show(ui, |ui| {
            ui.checkbox(&mut self.log, "Log scale").on_hover_text("Histogram counts on a log scale");
            channels_ui(ui, d, self.log, tokens);
        });
    }

    pub fn inspect(&self) -> Value {
        json!({"kind": "array", "pictures": self.pictures.iter().map(|(c, t)| json!({"caption": c, "size": t.size()})).collect::<Vec<_>>(), "zoom": self.zoom.zoom, "log": self.log,
               "shape": self.grid.map(|g| [g.rows, g.cols, g.channels]), "tools": self.tools.inspect()})
    }
}

fn view_tip(v: &str) -> &'static str {
    match v {
        "auto" => "Chosen for the array: labels for integers, colour for three channels in a colour space, else clip or stretch",
        "color" => "One picture: grey, grey + alpha, RGB (from its colour space) or RGBA",
        "stretch" => "Each channel from its own min to max",
        "clip" => "Each channel's values 0 to 1 as they are",
        "heat" => "Each channel stretched, in the Turbo colour map",
        "labels" => "A colour per integer value (negatives black)",
        "angle" => "Each value as a hue: one turn round the colour wheel",
        _ => "",
    }
}

fn picture(ui: &mut egui::Ui, zoom: &mut ZoomPan, tools: &mut PixelTools, caption: &str, t: &egui::TextureHandle, grid: Grid, tokens: &Tokens) {
    pixel_picture(ui, zoom, tools, t, grid, vec2(grid.cols as f32, grid.rows as f32), tokens);
    ui.label(egui::RichText::new(caption).small().color(tokens.text_dim));
}

/// Each channel's range and histogram.
pub fn channels_ui(ui: &mut egui::Ui, d: &Display, log: bool, tokens: &Tokens) {
    for c in &d.stats.channels {
        channel_ui(ui, c, log, tokens);
    }
}

fn number(v: Option<f64>) -> String {
    match v {
        Some(x) if x.fract() == 0.0 && x.abs() < 1e15 => format!("{x:.0}"),
        Some(x) => format!("{x:.6}"),
        None => "–".into(),
    }
}

fn channel_ui(ui: &mut egui::Ui, c: &ChannelStats, log: bool, tokens: &Tokens) {
    let nan = if c.nan > 0 { format!(", {} NaN", c.nan) } else { String::new() };
    ui.label(egui::RichText::new(format!("{}: {} … {}{nan}", c.name, number(c.min), number(c.max))).monospace().small());
    let Some(h) = &c.histogram else { return };
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width().max(120.0), 48.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, tokens.field);
    let scale = |n: u64| if log { (n as f64 + 1.0).ln() } else { n as f64 };
    let top = h.counts.iter().map(|n| scale(*n)).fold(0.0f64, f64::max);
    if top <= 0.0 || h.counts.is_empty() {
        return;
    }
    let bw = rect.width() / h.counts.len() as f32;
    for (i, n) in h.counts.iter().enumerate() {
        let frac = (scale(*n) / top) as f32;
        if frac <= 0.0 {
            continue;
        }
        let x = rect.left() + i as f32 * bw;
        let bar = egui::Rect::from_min_max(pos2(x, rect.bottom() - frac * rect.height()), pos2(x + bw.max(1.0), rect.bottom()));
        p.rect_filled(bar, 0.0, tokens.accent.gamma_multiply(0.8));
    }
    let lo = number(h.lo);
    let hi = number(h.hi);
    r.on_hover_text(format!("{} bins from {lo} to {hi}{}", h.bins, if log { ", log scale" } else { "" }));
}
