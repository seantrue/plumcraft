//! Compare: A and B, two values pinned from the Watched panel (`viewer.pin`), side by side, as a
//! split with a draggable divider, or as their exact difference. Lifted from the research software's
//! `crates/ui-egui/src/viewer.rs`; the difference itself is [`crate::diff`].
//!
//! The session holds what is pinned and how it is shown ([`plumcraft_session::viewer_cmds`]);
//! each side is a [`Side`] here, with its bytes, shared with everything else that shows them. A
//! side not pinned is the panel's value. A difference runs on a worker, once per pair of values,
//! and two equal handles are the same bytes, so that one needs no reading at all. It works on any
//! engine's arrays and images: the numbers come from the value, never from a picture of it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use plumcraft_proto::catalogue::Media;
use plumcraft_proto::data::DataRef;
use plumcraft_proto::display::{Display, Rendition};
use plumcraft_session::Session;
use plumcraft_session::run::WatchRole;
use plumcraft_session::viewer_cmds::{Compare, CompareHow, DiffMode};
use plumcraft_store::Blob;
use serde_json::{Value, json};

use crate::array_view::channels_ui;
use crate::canvas::Action;
use crate::diff::{self, Diff};
use crate::media::{ImageView, Job, MediaViewer, Picture, ZoomPan, key_of};
use crate::picture_tools::{PixelTools, zoom_bar};
use crate::probe::{self, Element, Grid, Source, format_f64, format_sig};
use crate::run_view::Thumbs;
use crate::theme::Tokens;
use crate::watched::Watched;

/// The height of a cell's caption.
const CAPTION_H: f32 = 16.0;

/// One side of the compare.
#[derive(Default)]
pub struct Side {
    /// `node.port`, and the item of a list.
    title: String,
    display: Option<Display>,
    read: Watched,
    /// Each view's picture.
    pictures: Vec<(String, egui::TextureHandle)>,
    /// An image's own decoding.
    image: ImageView,
    source: Source,
    grid: Option<Grid>,
}

impl Side {
    /// Take the newest display (`None` while a run computes: the last stays). True when its
    /// bytes are gone and a rerun should publish them again.
    fn sync(
        &mut self,
        ctx: &egui::Context,
        title: String,
        display: Option<Display>,
        thumbs: &mut Thumbs,
        resolve: &dyn Fn(&DataRef) -> Option<Blob>,
        full: &dyn Fn(&DataRef) -> Option<Arc<Blob>>,
    ) -> bool {
        self.title = title;
        if display.is_some() {
            self.display = display;
        }
        let Some(d) = &self.display else { return false };
        let rerun = self.read.update(Some(d), full);
        let views = d.renditions.iter().filter_map(|r| match r {
            Rendition::Rgba8 { data, role, caption, .. } if role == "view" => Some((caption.clone(), data)),
            _ => None,
        });
        let mut pictures: Vec<(String, egui::TextureHandle)> = views.filter_map(|(c, data)| Some((c, thumbs.picture(ctx, data, resolve).ok()?))).collect();
        if pictures.is_empty()
            && let Some(data) = d.renditions.iter().find_map(|r| match r {
                Rendition::Rgba8 { data, .. } => Some(data),
                _ => None,
            })
        {
            pictures.extend(thumbs.picture(ctx, data, resolve).ok().map(|t| (String::new(), t)));
        }
        if d.media == Media::Image {
            self.image.want_numbers = true;
            self.image.sync(ctx, Some(d), self.read.full.as_ref(), pictures.first().map(|(_, t)| t));
            self.source = self.image.source().clone();
            if let Some(t) = self.image.texture(ctx) {
                pictures = vec![(String::new(), t)];
            }
        } else {
            self.source = probe::source_of(d, self.read.full.as_ref(), self.read.missing);
        }
        self.grid = match &self.source {
            Source::Ready(p) => Some(p.grid),
            _ => probe::shape_of(d).or_else(|| pictures.first().and_then(|(_, t)| Grid::of_shape(&[t.size()[1] as u64, t.size()[0] as u64]))),
        };
        self.pictures = pictures;
        rerun
    }

    /// The picture of view `channel`, or the last one.
    fn picture(&self, channel: usize) -> Option<&(String, egui::TextureHandle)> {
        self.pictures.get(channel.min(self.pictures.len().saturating_sub(1)))
    }

    fn inspect(&self) -> Value {
        json!({"title": self.title, "summary": self.display.as_ref().map(|d| &d.summary), "full": self.read.full.is_some(), "missing": self.read.missing,
               "pictures": self.pictures.iter().map(|(c, t)| json!({"caption": c, "size": t.size()})).collect::<Vec<_>>()})
    }
}

/// The difference, or the worker making it.
struct Running {
    key: (String, String, DiffMode),
    job: Option<Job<Diff>>,
    stop: Arc<AtomicBool>,
    result: Option<Result<Diff, String>>,
    /// Decided from the handles alone.
    by_handle: bool,
    pictures: Vec<Picture>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The compare view.
#[derive(Default)]
pub struct CompareView {
    pub a: Side,
    pub b: Side,
    pub zoom: ZoomPan,
    /// The probe and zooms; its source isn't used: each side has its own.
    pub tools: PixelTools,
    state: Compare,
    on: bool,
    running: Option<Running>,
    /// The split's handle in the last frame (for tests and the control channel).
    pub split_handle: Option<Rect>,
}

/// One cell of the picture area: one layer, or A then B in a split.
struct Cell {
    caption: String,
    grid: Grid,
    layers: Vec<egui::TextureHandle>,
}

/// What the session says each side is: its title and display. A side not pinned is the panel's.
pub fn sides(s: &Session) -> [(String, Option<Display>); 2] {
    let side = |role: WatchRole| {
        let entry = s.run.entry(role).or_else(|| s.run.panel());
        let title = entry
            .map_or_else(String::new, |(w, _)| if w.index != 0 { format!("{}.{}[{}]", w.node, w.port, w.index) } else { format!("{}.{}", w.node, w.port) });
        (title, entry.and_then(|(_, d)| d).cloned())
    };
    [side(WatchRole::PinA), side(WatchRole::PinB)]
}

impl CompareView {
    /// Let go of A and B.
    pub fn clear(&mut self) {
        if self.on {
            let zoom = self.zoom;
            *self = CompareView { zoom, ..Default::default() };
        }
    }

    /// Is it showing?
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Take the session's state and each side. True when a rerun should publish a value again.
    #[allow(clippy::too_many_arguments)]
    pub fn sync(
        &mut self,
        ctx: &egui::Context,
        state: &Compare,
        sides: [(String, Option<Display>); 2],
        thumbs: &mut Thumbs,
        resolve: &dyn Fn(&DataRef) -> Option<Blob>,
        full: &dyn Fn(&DataRef) -> Option<Arc<Blob>>,
        nan: Color32,
    ) -> bool {
        self.on = true;
        self.state = state.clone();
        let [a, b] = sides;
        let rerun = self.a.sync(ctx, a.0, a.1, thumbs, resolve, full);
        let rerun = self.b.sync(ctx, b.0, b.1, thumbs, resolve, full) || rerun;
        if state.how == CompareHow::Difference {
            self.sync_diff(ctx, nan);
        }
        rerun
    }

    /// Start the difference this pair wants, and take it when it's done.
    fn sync_diff(&mut self, ctx: &egui::Context, nan: Color32) {
        let (Some(da), Some(db)) = (&self.a.display, &self.b.display) else { return };
        let key = (key_of(da), key_of(db), self.state.diff);
        if self.running.as_ref().is_none_or(|r| r.key != key) {
            let handle = |d: &Display| d.full.as_ref().and_then(|f| f.handle.clone());
            let same = handle(da).is_some() && handle(da) == handle(db);
            let mode = self.state.diff;
            let stop = Arc::new(AtomicBool::new(false));
            let mut run = Running { key, job: None, stop: Arc::clone(&stop), result: None, by_handle: false, pictures: Vec::new() };
            match (&self.a.source, &self.b.source, self.a.grid) {
                // equal handles are equal bytes: no need to look (plan, "Compare")
                (a, _, Some(grid)) if same => {
                    let names = if let Source::Ready(p) = a { p.names.clone() } else { Vec::new() };
                    run.by_handle = true;
                    run.result = Some(diff::identical(grid, &names, mode, diff::PICTURE_PX));
                }
                (Source::Ready(a), Source::Ready(b), _) => {
                    let (a, b) = (a.clone(), b.clone());
                    run.job = Some(Job::spawn(ctx, move || diff::diff(&a, &b, mode, diff::PICTURE_PX, nan, &|| stop.load(Ordering::Relaxed))));
                }
                (Source::Unreadable(why), _, _) => run.result = Some(Err(format!("A: {why}"))),
                (_, Source::Unreadable(why), _) => run.result = Some(Err(format!("B: {why}"))),
                // being read: try again next frame
                _ => return,
            }
            self.running = Some(run);
        }
        let Some(run) = &mut self.running else { return };
        if let Some(r) = run.job.as_ref().and_then(Job::poll) {
            run.job = None;
            run.result = Some(r);
        }
        if let (Some(Ok(d)), true) = (&run.result, run.pictures.is_empty()) {
            run.pictures = d
                .pictures
                .iter()
                .map(|p| {
                    let mut out = Picture::blocks();
                    out.set(ctx, &p.caption, p.picture.clone());
                    out
                })
                .collect();
        }
    }

    /// The compare's bar: how A and B are shown.
    fn bar(&self, ui: &mut egui::Ui, asks: &mut Vec<Action>) {
        let s = &self.state;
        ui.horizontal_wrapped(|ui| {
            for (label, how, id) in
                [("A | B", CompareHow::Side, "side"), ("Split", CompareHow::Split, "split"), ("|A−B|", CompareHow::Difference, "difference")]
            {
                if ui.selectable_label(s.how == how, label).on_hover_text("viewer.compare").clicked() && s.how != how {
                    asks.push(("viewer.compare", json!({"how": id})));
                }
            }
            if s.how == CompareHow::Difference {
                ui.separator();
                for (label, mode, id) in [("mean", DiffMode::Mean, "mean"), ("per channel", DiffMode::Channels, "channels")] {
                    if ui.selectable_label(s.diff == mode, label).clicked() && s.diff != mode {
                        asks.push(("viewer.compare", json!({"diff": id})));
                    }
                }
            } else {
                let captions: Vec<String> = self.a.pictures.iter().map(|(c, _)| c.clone()).collect();
                if captions.len() > 1 {
                    ui.separator();
                    let on = s.channel.min(captions.len() - 1);
                    for (i, c) in captions.iter().enumerate() {
                        let label = if c.is_empty() { i.to_string() } else { c.clone() };
                        if ui.selectable_label(on == i, label).clicked() && on != i {
                            asks.push(("viewer.compare", json!({"channel": i})));
                        }
                    }
                }
            }
        });
    }

    /// The cells to draw.
    fn cells(&mut self, ctx: &egui::Context) -> Vec<Cell> {
        let channel = self.state.channel;
        let cell = |side: &Side, name: &str| {
            let (caption, t) = side.picture(channel)?;
            Some(Cell { caption: format!("{name} {caption}"), grid: side.grid?, layers: vec![t.clone()] })
        };
        match self.state.how {
            CompareHow::Side => [cell(&self.a, "A"), cell(&self.b, "B")].into_iter().flatten().collect(),
            CompareHow::Split => match (cell(&self.a, "A"), cell(&self.b, "B")) {
                (Some(mut a), Some(b)) => {
                    a.caption = format!("{} | {}", a.caption, b.caption);
                    a.layers.extend(b.layers);
                    vec![a]
                }
                (a, b) => a.into_iter().chain(b).collect(),
            },
            CompareHow::Difference => {
                let Some(run) = &mut self.running else { return Vec::new() };
                let Some(Ok(d)) = &run.result else { return Vec::new() };
                let (grid, captions) = (d.grid, d.pictures.iter().map(|p| p.caption.clone()).collect::<Vec<_>>());
                let pictures = run.pictures.iter_mut().zip(captions).filter_map(|(p, c)| Some((p.texture(ctx, &c)?, c)));
                pictures.map(|(t, caption)| Cell { caption, grid, layers: vec![t] }).collect()
            }
        }
    }

    /// The pictures in a grid of cells sharing one zoom and pan.
    fn pictures_ui(&mut self, ui: &mut egui::Ui, cells: &[Cell], tokens: &Tokens, asks: &mut Vec<Action>) {
        let Some(first) = cells.first() else { return };
        let width = ui.available_width().max(64.0);
        let n = cells.len();
        let columns = if n == 1 { 1 } else { 2 };
        let rows = n.div_ceil(columns);
        let cell_w = width / columns as f32;
        let aspect = first.grid.cols as f32 / first.grid.rows.max(1) as f32;
        let cell_h = (cell_w / aspect.max(0.05)).clamp(120.0, 560.0);
        let (area, response) = ui.allocate_exact_size(vec2(width, rows as f32 * (cell_h + CAPTION_H)), Sense::click_and_drag());
        let rect_of = |i: usize| {
            let top = area.min + vec2((i % columns) as f32 * cell_w, (i / columns) as f32 * (cell_h + CAPTION_H));
            (Rect::from_min_size(top, vec2(cell_w, CAPTION_H)), Rect::from_min_size(top + vec2(0.0, CAPTION_H), vec2(cell_w, cell_h)).shrink(1.0))
        };
        let ppp = ui.ctx().pixels_per_point();
        self.tools.apply_zoom(&mut self.zoom, rect_of(0).1, first.grid, ppp);
        let hovered = response.hover_pos().and_then(|p| (0..n).find(|i| rect_of(*i).1.contains(p)));
        self.zoom.interact_about(ui, &response, hovered.map_or(area.center(), |i| rect_of(i).1.center()));
        self.tools.begin();
        if let (Some(p), Some(i)) = (response.hover_pos(), hovered)
            && let Some(cell) = cells.get(i)
        {
            let size = vec2(cell.grid.cols as f32, cell.grid.rows as f32);
            self.tools.hover = probe::display_to_array(p, self.zoom.place(rect_of(i).1, size), cell.grid.rows, cell.grid.cols);
        }
        if response.clicked() {
            self.tools.click();
        }
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0.0, tokens.canvas);
        for (i, cell) in cells.iter().enumerate() {
            let (cap, pic) = rect_of(i);
            painter.text(
                cap.left_center() + vec2(4.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &cell.caption,
                tokens.font(crate::theme::Text::Caption),
                tokens.text_dim,
            );
            let image = self.zoom.place(pic, vec2(cell.grid.cols as f32, cell.grid.rows as f32));
            let split_x = (image.min.x + image.width() * self.state.split).clamp(pic.min.x, pic.max.x);
            for (k, t) in cell.layers.iter().enumerate() {
                // a split shows A left of the divider and B right of it
                let clip = match (cell.layers.len(), k) {
                    (2, 0) => Rect::from_min_max(pic.min, pos2(split_x, pic.max.y)),
                    (2, _) => Rect::from_min_max(pos2(split_x, pic.min.y), pic.max),
                    _ => pic,
                };
                painter.with_clip_rect(clip).image(t.id(), image, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
            }
            if let Some(t) = cell.layers.first() {
                self.tools.note(ui, image, cell.grid, t.size());
            }
            self.tools.marks(ui, pic, image, cell.grid, tokens);
            if cell.layers.len() == 2 {
                self.split_handle = Some(self.divider(ui, &painter, pic, image, split_x, tokens, asks));
            }
        }
    }

    /// The split's divider and its drag.
    #[allow(clippy::too_many_arguments)]
    fn divider(&self, ui: &mut egui::Ui, painter: &egui::Painter, pic: Rect, image: Rect, x: f32, tokens: &Tokens, asks: &mut Vec<Action>) -> Rect {
        let clipped = painter.with_clip_rect(pic);
        clipped.line_segment([pos2(x, pic.min.y), pos2(x, pic.max.y)], Stroke::new(2.0, tokens.accent));
        let handle = Rect::from_center_size(pos2(x, pic.center().y), vec2(12.0, 36.0));
        let r = ui.interact(handle, ui.id().with("compare-split"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if r.dragged()
            && let Some(p) = r.interact_pointer_pos()
            && image.width() > 0.0
        {
            asks.push(("viewer.compare", json!({"split": (p.x - image.min.x) / image.width()})));
        }
        clipped.rect_filled(handle, 3.0, tokens.accent);
        handle
    }

    /// The difference in words, or why there is none.
    fn diff_status(&self, ui: &mut egui::Ui, tokens: &Tokens) {
        if self.state.how != CompareHow::Difference {
            return;
        }
        match self.running.as_ref().map(|r| (&r.result, r.by_handle)) {
            Some((Some(Ok(d)), by_handle)) => {
                ui.label(egui::RichText::new(diff_text(d, by_handle)).monospace().size(11.5));
                if d.grid.rows.max(d.grid.cols) > diff::PICTURE_PX {
                    let note = "each block shows the largest difference it covers";
                    ui.label(egui::RichText::new(note).small().color(tokens.text_faint));
                }
            }
            Some((Some(Err(e)), _)) => {
                ui.colored_label(tokens.error, e);
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new("Comparing every element…").color(tokens.text_dim));
                });
            }
        }
    }

    /// The probe, at the hovered and the pinned pixel: A, B and A−B.
    fn readout(&self, ui: &mut egui::Ui, tokens: &Tokens) {
        for (what, at, colour) in [("", self.tools.hover, tokens.text), ("pinned ", self.tools.pinned, tokens.accent)] {
            let Some(at) = at else { continue };
            ui.label(egui::RichText::new(format!("{what}row {} · col {}", at.0, at.1)).monospace().size(11.5).color(colour));
            for (name, source) in [("A", &self.a.source), ("B", &self.b.source)] {
                ui.label(egui::RichText::new(format!("{name} {}", line(source, at))).monospace().size(11.5).color(colour));
            }
            if let Some(d) = difference(&self.a.source, &self.b.source, at) {
                ui.label(egui::RichText::new(format!("A−B {d}")).monospace().size(11.5).color(colour));
            }
        }
        if self.tools.hover.is_none() && self.tools.pinned.is_none() {
            ui.label(egui::RichText::new("Hover the picture to read values; click to pin.").small().color(tokens.text_faint));
        }
    }

    /// Draw it. Returns the commands it asks for.
    pub fn ui(&mut self, ui: &mut egui::Ui, log: bool, tokens: &Tokens) -> Vec<Action> {
        let mut asks = Vec::new();
        self.split_handle = None;
        self.bar(ui, &mut asks);
        asks.extend(zoom_bar(ui));
        for (name, side) in [("A", &self.a), ("B", &self.b)] {
            ui.label(
                egui::RichText::new(format!(
                    "{name}: {} · {}",
                    side.title,
                    side.display.as_ref().map_or("not run yet", |d| d.summary.lines().next().unwrap_or_default())
                ))
                .small()
                .color(tokens.text_dim),
            );
            if let Some(e) = side.display.as_ref().and_then(|d| d.error.as_ref()) {
                ui.colored_label(tokens.error, format!("{name}: {}", e.message));
            }
        }
        let ctx = ui.ctx().clone();
        let cells = self.cells(&ctx);
        egui::ScrollArea::vertical().id_salt("plumcraft-compare").auto_shrink([false, false]).show(ui, |ui| {
            self.pictures_ui(ui, &cells, tokens, &mut asks);
            self.diff_status(ui, tokens);
            if let Some(cell) = cells.first() {
                self.tools.status(ui, cell.grid, tokens);
            }
            self.readout(ui, tokens);
            for (name, side) in [("A", &self.a), ("B", &self.b)] {
                if let Some(d) = side.display.as_ref().filter(|d| !d.stats.channels.is_empty()) {
                    ui.label(egui::RichText::new(name).strong());
                    channels_ui(ui, d, log, tokens);
                }
            }
        });
        asks.append(&mut self.tools.asks);
        asks
    }

    /// Its state, for `ui.inspect`.
    pub fn inspect(&self) -> Value {
        if !self.on {
            return Value::Null;
        }
        let result = self.running.as_ref().map(|r| match &r.result {
            Some(Ok(d)) => json!({
                "state": "ready", "byHandle": r.by_handle, "text": diff_text(d, r.by_handle), "max": d.max, "mean": d.mean, "nan": d.nan,
                "differing": d.differing, "elements": d.elements,
                "pictures": d.pictures.iter().map(|p| json!({"caption": p.caption, "size": [p.picture.w, p.picture.h], "top": p.top})).collect::<Vec<_>>(),
            }),
            Some(Err(e)) => json!({"state": "error", "error": e}),
            None => json!({"state": "computing"}),
        });
        let s = &self.state;
        let read = |at: Option<(usize, usize)>| {
            at.map(|at| json!({"a": probe::reading(&self.a.source, at), "b": probe::reading(&self.b.source, at), "difference": difference(&self.a.source, &self.b.source, at)}))
        };
        json!({"how": s.how, "diff": s.diff, "split": s.split, "channel": s.channel, "a": self.a.inspect(), "b": self.b.inspect(), "result": result,
               "probe": read(self.tools.pinned), "hover": read(self.tools.hover), "tools": self.tools.inspect()})
    }
}

/// One side's readout.
fn line(source: &Source, (row, col): (usize, usize)) -> String {
    match source {
        Source::Ready(p) if row >= p.grid.rows || col >= p.grid.cols => format!("outside ({} × {})", p.grid.cols, p.grid.rows),
        Source::Ready(p) => probe::format_values(&p.names, &p.read(row, col)),
        Source::Reading => "reading…".into(),
        Source::Unreadable(why) => why.clone(),
        Source::None => "not shown".into(),
    }
}

/// A−B at a pixel, by channel, when both sides read there.
fn difference(a: &Source, b: &Source, (row, col): (usize, usize)) -> Option<String> {
    let (Source::Ready(a), Source::Ready(b)) = (a, b) else { return None };
    let (x, y): (Vec<Element>, Vec<Element>) = (a.read(row, col), b.read(row, col));
    if x.is_empty() || y.is_empty() {
        return None;
    }
    if x.len() != y.len() {
        return Some("the channel counts differ".into());
    }
    let parts = a.names.iter().zip(x.iter().zip(&y)).map(|(n, (x, y))| format!("{n} {}", format_f64(x.as_f64() - y.as_f64())));
    Some(parts.collect::<Vec<_>>().join(" · "))
}

/// "|A−B| max 0.5 · mean 0.1875 · 2 of 4 elements differ", or that they're identical.
pub fn diff_text(d: &Diff, by_handle: bool) -> String {
    if d.differing == 0 {
        let how = if by_handle { ": one value" } else { "" };
        return format!("A and B are identical ({} elements){how}", d.elements);
    }
    let mut s = format!("|A−B| max {} · mean {} · {} of {} elements differ", format_f64(d.max), format_sig(d.mean, 4), d.differing, d.elements);
    if d.nan > 0 {
        s.push_str(&format!(" · {} NaN (in the error colour)", d.nan));
    }
    s
}

/// Whether a value can be pinned: a picture of numbers.
pub fn comparable(d: &Display) -> bool {
    matches!(d.media, Media::Ndarray | Media::Image) && probe::shape_of(d).is_some()
}

/// The compare's controls, under the panel's header.
pub fn pin_bar(ui: &mut egui::Ui, pinned: bool, on: bool, asks: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        if ui.button("Pin A").on_hover_text("Keep this value as A for comparing (viewer.pin)").clicked() {
            asks.push(("viewer.pin", json!({"slot": "a"})));
        }
        if ui.button("Pin B").on_hover_text("Keep this value as B; else B is the value shown (viewer.pin)").clicked() {
            asks.push(("viewer.pin", json!({"slot": "b"})));
        }
        if pinned {
            if ui.selectable_label(on, "Compare").on_hover_text("Show A and B, or this value alone (viewer.compare)").clicked() {
                asks.push(("viewer.compare", json!({"on": !on})));
            }
            if ui.small_button("Unpin").on_hover_text("Forget A and B (viewer.unpin)").clicked() {
                asks.push(("viewer.unpin", json!({})));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Pixels;

    fn src(data: &[f32]) -> Source {
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        let shape = vec![1, 1, data.len() as u64];
        Source::Ready(Pixels::of(&Arc::new(Blob { dtype: "<f4".into(), shape, bytes: bytes.into() }), &[]).unwrap())
    }

    #[test]
    fn the_probe_reads_each_side_and_the_difference() {
        let (a, b) = (src(&[0.75, 0.25]), src(&[0.5, 1.0]));
        assert_eq!(line(&a, (0, 0)), "c0 0.75 · c1 0.25");
        assert_eq!(difference(&a, &b, (0, 0)).unwrap(), "c0 0.25 · c1 -0.75");
        assert_eq!(line(&a, (5, 0)), "outside (1 × 1)");
        assert_eq!(difference(&a, &b, (5, 0)), None);
        assert_eq!(difference(&a, &src(&[1.0]), (0, 0)).unwrap(), "the channel counts differ");
        let why = Source::Unreadable("no exact values".into());
        assert_eq!(difference(&a, &why, (0, 0)), None, "never a number made up from a side that can't be read");
    }

    #[test]
    fn the_text_says_what_it_found() {
        let d = Diff { grid: Grid { rows: 1, cols: 2, channels: 2 }, max: 0.5, mean: 0.1875, nan: 1, differing: 2, elements: 4, pictures: Vec::new() };
        assert_eq!(diff_text(&d, false), "|A−B| max 0.5 · mean 0.1875 · 2 of 4 elements differ · 1 NaN (in the error colour)");
        let same = Diff { differing: 0, nan: 0, max: 0.0, mean: 0.0, ..d };
        assert_eq!(diff_text(&same, true), "A and B are identical (4 elements): one value");
    }
}
