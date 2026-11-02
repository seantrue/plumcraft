//! What every picture of a value's numbers shares: the pixel probe, 1:1 and exact zooms, and the
//! bar and readout that go with them. The array and image viewers and the compare view use it.
//!
//! The session holds the pinned probe and any zoom asked for (`viewer.probe`, `viewer.zoom`), so
//! the buttons, a click on the picture, the control channel and MCP all act alike; a viewer takes
//! them each frame ([`PixelTools::take`]). Hovering is the one thing that isn't a command.

use egui::{Rect, Stroke, Vec2, pos2, vec2};
use plumcraft_session::viewer_cmds::{Tools, Zoom, ZoomAsk};
use serde_json::{Value, json};

use crate::canvas::Action;
use crate::media::ZoomPan;
use crate::probe::{self, Grid, Source};
use crate::theme::Tokens;

/// The largest zoom, in screen pixels per array pixel.
pub const MAX_PIXELS_PER_ARRAY_PIXEL: f32 = 64.0;

/// A viewer's probe and what it last drew.
#[derive(Debug, Default)]
pub struct PixelTools {
    /// What can be read of the value shown.
    pub source: Source,
    /// The probe pinned (row, column).
    pub pinned: Option<(usize, usize)>,
    /// The pixel under the pointer.
    pub hover: Option<(usize, usize)>,
    /// A zoom waiting for a view to apply to.
    ask: Option<ZoomAsk>,
    /// The commands its pictures asked for, for the panel to run.
    pub asks: Vec<Action>,
    /// Detail was asked for this value.
    asked_detail: bool,
    /// The last frame's pictures on screen (for tests and the control channel).
    pub rects: Vec<Rect>,
    /// Screen pixels per array pixel, last frame.
    pub scale: f32,
    /// Array pixels a picture's pixel covers, last frame.
    pub block: f32,
}

impl PixelTools {
    /// Take the session's state for this viewer.
    pub fn take(&mut self, t: Tools) {
        self.pinned = t.probe.and_then(|(r, c)| Some((usize::try_from(r).ok()?, usize::try_from(c).ok()?)));
        if t.zoom.is_some() {
            self.ask = t.zoom;
        }
    }

    /// A new value's shape: the next zoom may want detail again.
    pub fn new_value(&mut self) {
        self.asked_detail = false;
    }

    /// Before a frame's pictures.
    pub fn begin(&mut self) {
        self.rects.clear();
        self.hover = None;
    }

    /// Whether anything wants the numbers (an image decodes them only then).
    pub fn wants_numbers(&self) -> bool {
        self.hover.is_some() || self.pinned.is_some()
    }

    /// The zoom asked for, applied to `zp` where a picture of `grid` fits `view`.
    pub fn apply_zoom(&mut self, zp: &mut ZoomPan, view: Rect, grid: Grid, ppp: f32) {
        let size = vec2(grid.cols as f32, grid.rows as f32);
        let fit = (view.width() / size.x).min(view.height() / size.y);
        if !(fit.is_finite() && fit > 0.0) {
            return;
        }
        zp.limit = (MAX_PIXELS_PER_ARRAY_PIXEL / ppp.max(0.1) / fit).max(crate::media::MAX_ZOOM);
        let Some(ask) = self.ask.take() else { return };
        match ask.to {
            Some(Zoom::Fit) => {
                zp.fit();
                self.asked_detail = false;
            }
            Some(Zoom::OneToOne) => set_scale(zp, 1.0 / ppp.max(0.1) / fit),
            Some(Zoom::Pixels(n)) => set_scale(zp, n as f32 / ppp.max(0.1) / fit),
            Some(Zoom::In) => zp.zoom_at(2.0, Vec2::ZERO),
            Some(Zoom::Out) => zp.zoom_at(0.5, Vec2::ZERO),
            None => {}
        }
        if let Some((row, col)) = ask.at {
            let at = vec2(col as f32 + 0.5, row as f32 + 0.5);
            zp.pan = (size / 2.0 - at) * fit * zp.zoom;
        }
    }

    /// After a picture of `grid` was drawn in `view` from a texture of `tex` pixels: the probe's
    /// marks on `view`, what the pointer is over and what a click pins. `r` is the view's
    /// response. Returns the picture's rect.
    #[allow(clippy::too_many_arguments)]
    pub fn after_picture(&mut self, ui: &egui::Ui, r: &egui::Response, view: Rect, zp: &ZoomPan, grid: Grid, tex: [usize; 2], tokens: &Tokens) -> Rect {
        let image = zp.place(view, vec2(grid.cols as f32, grid.rows as f32));
        self.note(ui, image, grid, tex);
        if let Some(p) = r.hover_pos().filter(|p| view.contains(*p))
            && let Some(at) = probe::display_to_array(p, image, grid.rows, grid.cols)
        {
            self.hover = Some(at);
        }
        if r.clicked() {
            self.click();
        }
        self.marks(ui, view, image, grid, tokens);
        image
    }

    /// Note how large `image` is shown; ask for detail when a picture's pixels are enlarged and
    /// the array has more.
    pub fn note(&mut self, ui: &egui::Ui, image: Rect, grid: Grid, tex: [usize; 2]) {
        self.rects.push(image);
        let ppp = ui.ctx().pixels_per_point();
        self.scale = image.width() / grid.cols.max(1) as f32 * ppp;
        self.block = grid.cols as f32 / tex[0].max(1) as f32;
        if self.block > 1.01 && self.scale * self.block > 1.0 && !self.asked_detail {
            self.asked_detail = true;
            self.asks.push(("viewer.zoom", json!({"to": "detail"})));
        }
    }

    /// A click pins the pixel under the pointer, or unpins it.
    pub fn click(&mut self) {
        if let Some((r, c)) = self.hover {
            let p = if self.pinned == Some((r, c)) { json!({}) } else { json!({"row": r, "col": c}) };
            self.asks.push(("viewer.probe", p));
        }
    }

    /// The hovered pixel's outline and the pinned pixel's crosshair, clipped to `view`.
    pub fn marks(&self, ui: &egui::Ui, view: Rect, image: Rect, grid: Grid, tokens: &Tokens) {
        let p = ui.painter_at(view);
        let px = image.width() / grid.cols.max(1) as f32 * ui.ctx().pixels_per_point();
        if let Some((r, c)) = self.hover
            && px >= 6.0
        {
            p.rect_stroke(probe::array_to_display(r, c, image, grid.rows, grid.cols), 0.0, Stroke::new(1.0, tokens.text), egui::StrokeKind::Outside);
        }
        if let Some((r, c)) = self.pinned
            && r < grid.rows
            && c < grid.cols
        {
            let rect = probe::array_to_display(r, c, image, grid.rows, grid.cols);
            let stroke = Stroke::new(1.0, tokens.accent);
            let gap = rect.width().max(6.0) / 2.0 + 2.0;
            let at = rect.center();
            p.line_segment([pos2(view.min.x, at.y), pos2(at.x - gap, at.y)], stroke);
            p.line_segment([pos2(at.x + gap, at.y), pos2(view.max.x, at.y)], stroke);
            p.line_segment([pos2(at.x, view.min.y), pos2(at.x, at.y - gap)], stroke);
            p.line_segment([pos2(at.x, at.y + gap), pos2(at.x, view.max.y)], stroke);
            if px >= 6.0 {
                p.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Outside);
            }
        }
    }

    /// The readout under a picture: the hovered and the pinned pixel's exact values.
    pub fn readout(&self, ui: &mut egui::Ui, tokens: &Tokens) {
        for (what, at, colour) in [("", self.hover, tokens.text), ("pinned ", self.pinned, tokens.accent)] {
            if let Some(at) = at {
                reading_line(ui, what, at, &self.source, colour, tokens);
            }
        }
        if self.hover.is_none() && self.pinned.is_none() {
            ui.label(egui::RichText::new("Hover the picture to read values; click to pin.").small().color(tokens.text_faint));
        }
    }

    /// The zoom in words, under the bar.
    pub fn status(&self, ui: &mut egui::Ui, grid: Grid, tokens: &Tokens) {
        ui.label(egui::RichText::new(scale_text(grid, self.scale, self.block)).small().color(tokens.text_dim));
    }

    /// Its state, for `ui.inspect`.
    pub fn inspect(&self) -> Value {
        json!({
            "pixelsPerArrayPixel": self.scale,
            "arrayPixelsPerBlock": self.block,
            "pictures": self.rects.iter().map(|r| [r.min.x, r.min.y, r.max.x, r.max.y]).collect::<Vec<_>>(),
            "probe": self.pinned.map(|at| probe::reading(&self.source, at)),
            "hover": self.hover.map(|at| probe::reading(&self.source, at)),
        })
    }
}

/// An exact zoom about the view's centre: a small array at 1:1 is under what fitting zooms out to.
fn set_scale(zp: &mut ZoomPan, zoom: f32) {
    if zp.zoom > 0.0 && zoom.is_finite() && zoom > 0.0 {
        zp.pan *= zoom / zp.zoom;
        zp.zoom = zoom;
    }
}

/// One readout line.
pub fn reading_line(ui: &mut egui::Ui, what: &str, (row, col): (usize, usize), source: &Source, colour: egui::Color32, tokens: &Tokens) {
    let at = format!("{what}row {row} · col {col}");
    let (text, colour) = match source {
        Source::Ready(px) if row >= px.grid.rows || col >= px.grid.cols => (format!("{at}: outside ({} × {})", px.grid.cols, px.grid.rows), tokens.text_dim),
        Source::Ready(px) => (format!("{at}: {}", probe::format_values(&px.names, &px.read(row, col))), colour),
        Source::Reading => (format!("{at}: reading…"), tokens.text_dim),
        Source::Unreadable(why) => (format!("{at}: {why}"), tokens.text_dim),
        Source::None => (at, tokens.text_dim),
    };
    ui.label(egui::RichText::new(text).monospace().size(11.5).color(colour));
}

/// "4000 × 3000 px · zoom 800% · 1 block = 3.9 array pixels".
pub fn scale_text(grid: Grid, scale: f32, block: f32) -> String {
    let mut out = format!("{} × {} px · zoom {:.0}%", grid.cols, grid.rows, scale * 100.0);
    if block > 1.01 && scale * block > 1.0 {
        out.push_str(&format!(" · 1 block = {block:.1} array pixels: this picture is smaller than the array"));
    }
    out
}

/// Fit, 1:1, − and + and the zoom buttons as commands.
pub fn zoom_bar(ui: &mut egui::Ui) -> Vec<Action> {
    let mut asks = Vec::new();
    ui.horizontal(|ui| {
        for (label, to, hover) in [
            ("Fit", json!("fit"), "Fit the picture (or double-click it)"),
            ("1:1", json!("1:1"), "One array pixel per screen pixel, each a sharp block when zoomed in further"),
            ("−", json!("out"), "Zoom out (or scroll)"),
            ("+", json!("in"), "Zoom in (or scroll)"),
        ] {
            if ui.small_button(label).on_hover_text(format!("{hover} (viewer.zoom)")).clicked() {
                asks.push(("viewer.zoom", json!({"to": to})));
            }
        }
    });
    asks
}

/// Interact and draw `tex` over `grid` in a new view of `size`: the picture and the probe.
pub fn pixel_picture(
    ui: &mut egui::Ui,
    zp: &mut ZoomPan,
    tools: &mut PixelTools,
    tex: &egui::TextureHandle,
    grid: Grid,
    ask_size: Vec2,
    tokens: &Tokens,
) -> Rect {
    let (view, r) = crate::media::view_rect(ui, ask_size);
    tools.apply_zoom(zp, view, grid, ui.ctx().pixels_per_point());
    zp.interact(ui, &r);
    crate::media::paint_picture(ui, zp, tex, view, vec2(grid.cols as f32, grid.rows as f32), tokens);
    tools.after_picture(ui, &r, view, zp, grid, tex.size(), tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: Rect = Rect { min: pos2(0.0, 0.0), max: pos2(400.0, 300.0) };
    const GRID: Grid = Grid { rows: 3000, cols: 4000, channels: 3 };

    fn ask(tools: &mut PixelTools, to: Option<Zoom>, at: Option<(u64, u64)>) -> ZoomPan {
        let mut zp = ZoomPan::default();
        tools.take(Tools { probe: None, zoom: Some(ZoomAsk { to, at }) });
        tools.apply_zoom(&mut zp, VIEW, GRID, 2.0);
        zp
    }

    fn scale(zp: &ZoomPan) -> f32 {
        zp.place(VIEW, vec2(4000.0, 3000.0)).width() / 4000.0
    }

    #[test]
    fn one_to_one_is_one_array_pixel_per_screen_pixel() {
        let mut t = PixelTools::default();
        let zp = ask(&mut t, Some(Zoom::OneToOne), None);
        assert!((scale(&zp) * 2.0 - 1.0).abs() < 1e-5, "{}", scale(&zp));
    }

    #[test]
    fn a_zoom_in_pixels_reaches_past_what_fit_allows() {
        let mut t = PixelTools::default();
        // 64 times the fit of 0.1 would stop at 6.4; the view may go to 64 pixels per array pixel
        let zp = ask(&mut t, Some(Zoom::Pixels(64)), None);
        assert!((scale(&zp) * 2.0 - 64.0).abs() < 1e-4, "{}", scale(&zp));
        // and no further
        let mut more = zp;
        more.zoom_at(10.0, Vec2::ZERO);
        assert!((scale(&more) * 2.0 - 64.0).abs() < 1e-4);
    }

    #[test]
    fn a_small_array_at_one_to_one_is_small() {
        let mut t = PixelTools::default();
        let mut zp = ZoomPan::default();
        let small = Grid { rows: 12, cols: 16, channels: 1 };
        t.take(Tools { probe: None, zoom: Some(ZoomAsk { to: Some(Zoom::OneToOne), at: None }) });
        t.apply_zoom(&mut zp, VIEW, small, 1.0);
        assert!((zp.place(VIEW, vec2(16.0, 12.0)).width() - 16.0).abs() < 1e-3);
    }

    #[test]
    fn at_centres_a_pixel() {
        let mut t = PixelTools::default();
        let zp = ask(&mut t, Some(Zoom::Pixels(16)), Some((100, 3000)));
        let image = zp.place(VIEW, vec2(4000.0, 3000.0));
        let c = probe::display_to_array(VIEW.center(), image, 3000, 4000);
        assert_eq!(c, Some((100, 3000)));
    }

    #[test]
    fn scale_text_says_when_a_block_isnt_a_pixel() {
        assert_eq!(scale_text(GRID, 8.0, 1.0), "4000 × 3000 px · zoom 800%");
        assert!(scale_text(GRID, 8.0, 3.9).contains("1 block = 3.9 array pixels"));
        assert!(!scale_text(GRID, 0.1, 3.9).contains("block"), "a shrunk picture has no blocks");
    }
}
