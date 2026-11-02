//! The Watched panel's picture viewers (plan 10.6, M3; note 14, section 4): raster images,
//! SVG and craft documents. Each is a [`MediaViewer`], the shape a later registry of specialist
//! viewers (ROADMAP, "Specialist viewer windows per data type") can adopt without rework: it
//! says which media kinds it shows, takes the watched display and its full value
//! ([`MediaViewer::sync`]), draws itself ([`MediaViewer::ui`]) and reports its state
//! ([`MediaViewer::inspect`], `ui.inspect`'s `watched.media`).
//!
//! - [`ImageView`]: a PNG, JPEG, TIFF, WebP or GIF decoded (bounded, off the UI thread) and shrunk
//!   to at most [`MAX_TEXTURE_SIDE`]; any other format, and a value whose bytes are gone, show the
//!   engine's `rgba8` rendition instead.
//! - [`SvgView`]: the SVG parsed with usvg and rasterised with resvg (tiny-skia), pure Rust,
//!   both off the UI thread. It rasterises again at the zoom shown, so zooming stays crisp, up to
//!   [`MAX_TEXTURE_SIDE`] a side and [`MAX_RENDER_PIXELS`]. External files are never read (only
//!   `data:` images); text that isn't outlined is drawn with the system's fonts
//!   ([`plumcraft_svg`], shared with the engine SDK's SVG thumbnails). An SVG
//!   over [`MAX_SVG_BYTES`] or one that won't parse falls back to the rendition, with why.
//! - [`DocView`]: a document the UI never decodes (spec 10.1): the engine's rendition, and
//!   "Open in …" (`doc.open`), which opens a copy in the craft's app.
//!
//! All three zoom and pan the same way ([`ZoomPan`]): scroll to zoom at the pointer, drag to
//! pan, double-click (or Fit) to fit. Every value is hostile input: sizes are checked before
//! a byte is decoded, and nothing panics on what an engine sends.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use crate::canvas::Action;
use crate::picture_tools::{PixelTools, pixel_picture};
use crate::probe::{self, Grid, Source};
use crate::run_view::{PIXEL_TEXTURE, load_picture};

use egui::{Color32, Rect, Vec2, pos2, vec2};
use plumcraft_proto::catalogue::Media;
use plumcraft_proto::display::Display;
use plumcraft_store::Blob;
use serde_json::{Value, json};

use crate::theme::Tokens;

/// The longest texture side made for a picture.
pub const MAX_TEXTURE_SIDE: u32 = 4096;
/// The most pixels an SVG is rasterised to.
pub const MAX_RENDER_PIXELS: u64 = 16 << 20;
/// The largest PNG decoded: 16,384 px a side, 256 Mpx (spec 18).
pub const MAX_DECODE_SIDE: u32 = 16_384;
pub const MAX_DECODE_PIXELS: u64 = 256 << 20;
/// The largest SVG parsed.
pub const MAX_SVG_BYTES: usize = plumcraft_svg::MAX_SVG_BYTES;
/// The zoom range, relative to fitting the view.
pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 64.0;

/// A viewer for some media kinds in the Watched panel (and, later, in a window of its own).
pub trait MediaViewer {
    /// The media kinds it shows.
    fn media(&self) -> &'static [Media];
    /// Take the watched display, its full value when read, and the rendition's texture when
    /// there is one. Cheap when nothing changed; heavy work goes to a worker thread.
    fn sync(&mut self, ctx: &egui::Context, display: Option<&Display>, full: Option<&Arc<Blob>>, rendition: Option<&egui::TextureHandle>);
    /// Draw it.
    fn ui(&mut self, ui: &mut egui::Ui, tokens: &Tokens);
    /// Its state, for the control channel.
    fn inspect(&self) -> Value;
    /// The commands its last frame asked for.
    fn asks(&mut self) -> Vec<Action> {
        Vec::new()
    }
}

// ---------- zoom and pan ----------

/// A view's zoom (1: the picture fits) and pan (points from centred).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomPan {
    pub zoom: f32,
    pub pan: Vec2,
    /// The largest zoom: a picture of many pixels fits small, and pixel work goes far past that
    /// ([`crate::picture_tools`] sets it).
    pub limit: f32,
}

impl Default for ZoomPan {
    fn default() -> Self {
        ZoomPan { zoom: 1.0, pan: Vec2::ZERO, limit: MAX_ZOOM }
    }
}

impl ZoomPan {
    pub fn fit(&mut self) {
        *self = ZoomPan::default();
    }

    /// Zoom by `factor` keeping the picture point under `at` (relative to the view's centre) still.
    pub fn zoom_at(&mut self, factor: f32, at: Vec2) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let z = (self.zoom * factor).clamp(MIN_ZOOM.min(self.zoom), self.limit.max(MAX_ZOOM));
        let f = z / self.zoom;
        self.pan = at - (at - self.pan) * f;
        self.zoom = z;
    }

    /// Where a picture of `size` (in its own pixels) lies in `view` at this zoom and pan.
    pub fn place(&self, view: Rect, size: Vec2) -> Rect {
        if !(size.x > 0.0 && size.y > 0.0) {
            return view;
        }
        let fit = (view.width() / size.x).min(view.height() / size.y);
        Rect::from_center_size(view.center() + self.pan, size * fit * self.zoom)
    }

    /// The view's interactions: scroll zooms at the pointer, drag pans, double-click fits.
    pub fn interact(&mut self, ui: &egui::Ui, r: &egui::Response) {
        self.interact_about(ui, r, r.rect.center());
    }

    /// [`ZoomPan::interact`] for a view centred on `center` within the response.
    pub fn interact_about(&mut self, ui: &egui::Ui, r: &egui::Response, center: egui::Pos2) {
        if r.double_clicked() {
            let limit = self.limit;
            self.fit();
            self.limit = limit;
        } else if r.dragged() {
            self.pan += r.drag_delta();
        }
        if r.hovered() {
            let (scroll, zoom, at) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos()));
            let factor = zoom * (scroll / 200.0).exp();
            if (factor - 1.0).abs() > 1e-4
                && let Some(at) = at
            {
                self.zoom_at(factor, at - center);
                // the scroll zoomed: the panel around the picture doesn't scroll too
                ui.ctx().input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
            }
        }
    }

    /// The Fit, −, + buttons and the zoom shown.
    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui, tokens: &Tokens) {
        ui.horizontal(|ui| {
            if ui.small_button("Fit").on_hover_text("Fit the picture (or double-click it)").clicked() {
                self.fit();
            }
            if ui.small_button("−").on_hover_text("Zoom out (or scroll)").clicked() {
                self.zoom_at(0.5, Vec2::ZERO);
            }
            if ui.small_button("+").on_hover_text("Zoom in (or scroll)").clicked() {
                self.zoom_at(2.0, Vec2::ZERO);
            }
            ui.label(egui::RichText::new(format!("{:.0} % of fit", self.zoom * 100.0)).small().color(tokens.text_dim));
        });
    }
}

/// The view's rectangle: the panel's width, a height for the picture's aspect (bounded).
pub(crate) fn view_rect(ui: &mut egui::Ui, size: Vec2) -> (Rect, egui::Response) {
    let w = ui.available_width().max(120.0);
    let aspect = if size.x > 0.0 && size.y > 0.0 { size.y / size.x } else { 1.0 };
    let h = (w * aspect).clamp(160.0, 640.0);
    ui.allocate_exact_size(vec2(w, h), egui::Sense::click_and_drag())
}

/// A checkerboard behind transparent pixels, in two theme colours.
fn checker(p: &egui::Painter, rect: Rect, tokens: &Tokens) {
    p.rect_filled(rect, 0.0, tokens.field);
    let cell = 10.0;
    let (cols, rows) = ((rect.width() / cell).ceil() as usize, (rect.height() / cell).ceil() as usize);
    if cols.saturating_mul(rows) > 40_000 {
        return;
    }
    for y in 0..rows {
        for x in (y % 2..cols).step_by(2) {
            let min = rect.min + vec2(x as f32 * cell, y as f32 * cell);
            p.rect_filled(Rect::from_min_max(min, (min + vec2(cell, cell)).min(rect.max)), 0.0, tokens.panel);
        }
    }
}

/// Draw `tex` (a picture of `size`) zoomed and panned in a new view; returns the view.
pub(crate) fn draw_picture(ui: &mut egui::Ui, zp: &mut ZoomPan, tex: &egui::TextureHandle, size: Vec2, label: &str, tokens: &Tokens) -> Rect {
    let (view, r) = view_rect(ui, size);
    zp.interact(ui, &r);
    paint_picture(ui, zp, tex, view, size, tokens);
    r.on_hover_text(label);
    view
}

/// `tex` over a picture of `size`, at `zp`, in `view`.
pub(crate) fn paint_picture(ui: &egui::Ui, zp: &ZoomPan, tex: &egui::TextureHandle, view: Rect, size: Vec2, tokens: &Tokens) {
    let p = ui.painter_at(view);
    let at = zp.place(view, size);
    checker(&p, at.intersect(view), tokens);
    p.image(tex.id(), at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    p.rect_stroke(view, 2.0, egui::Stroke::new(1.0, tokens.separator), egui::StrokeKind::Inside);
}

// ---------- decoding ----------

/// A picture: RGBA8, unpremultiplied, `w × h`.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    pub fn image(&self) -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied([self.w as usize, self.h as usize], &self.pixels)
    }
}

/// A PNG's size from its header.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let h = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    Some((w, h))
}

/// A picture file's size from its header, whatever its format.
pub fn picture_size(bytes: &[u8]) -> Option<(u32, u32)> {
    png_size(bytes).or_else(|| image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok())
}

/// Decode a picture file (bounded) to RGBA8, shrunk to at most `max_side`: PNG here, the other
/// formats with the `image` crate, which reads them in RGB order whatever made them.
pub fn decode_picture(bytes: &[u8], max_side: u32) -> Result<Rgba, String> {
    if png_size(bytes).is_some() {
        return decode_png(bytes, max_side);
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_PIXELS * 4);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    if reader.format().is_none() {
        return Err("not a picture this editor reads".into());
    }
    reader.limits(limits);
    let picture = reader.decode().map_err(|e| e.to_string())?.into_rgba8();
    let (w, h) = picture.dimensions();
    Ok(shrink(Rgba { w, h, pixels: picture.into_raw() }, max_side))
}

/// Decode a PNG (bounded) to RGBA8, shrunk (box filter) to at most `max_side`.
pub fn decode_png(bytes: &[u8], max_side: u32) -> Result<Rgba, String> {
    let (w0, h0) = png_size(bytes).ok_or("not a PNG")?;
    if w0 == 0 || h0 == 0 || w0 > MAX_DECODE_SIDE || h0 > MAX_DECODE_SIDE || u64::from(w0) * u64::from(h0) > MAX_DECODE_PIXELS {
        return Err(format!("a {w0}×{h0} PNG is too large to show"));
    }
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; reader.output_buffer_size().ok_or("PNG too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let src = buf.get(..info.buffer_size()).ok_or("a torn PNG")?;
    let ch = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return Err("an indexed PNG wasn't expanded".into()),
    };
    if src.len() < w * h * ch {
        return Err("a torn PNG".into());
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for px in src.chunks_exact(ch).take(w * h) {
        let g = |k: usize| px.get(k).copied().unwrap_or(0);
        rgba.extend_from_slice(&match ch {
            4 => [g(0), g(1), g(2), g(3)],
            3 => [g(0), g(1), g(2), 255],
            2 => [g(0), g(0), g(0), g(1)],
            _ => [g(0), g(0), g(0), 255],
        });
    }
    Ok(shrink(Rgba { w: info.width, h: info.height, pixels: rgba }, max_side))
}

/// Shrink to at most `max_side` a side (box filter); smaller pictures are returned as they are.
pub fn shrink(img: Rgba, max_side: u32) -> Rgba {
    let (w, h) = (img.w as usize, img.h as usize);
    if img.w.max(img.h) <= max_side || max_side == 0 || img.pixels.len() < w * h * 4 {
        return img;
    }
    let scale = f64::from(max_side) / f64::from(img.w.max(img.h));
    let (tw, th) = (((w as f64 * scale).round() as usize).max(1), ((h as f64 * scale).round() as usize).max(1));
    let mut out = Vec::with_capacity(tw * th * 4);
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).clamp(ty * h / th + 1, h));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).clamp(tx * w / tw + 1, w));
            let mut acc = [0u64; 4];
            let mut n = 0u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * w + x) * 4;
                    for (k, a) in acc.iter_mut().enumerate() {
                        *a += u64::from(img.pixels.get(i + k).copied().unwrap_or(0));
                    }
                    n += 1;
                }
            }
            out.extend(acc.map(|a| u8::try_from(a / n.max(1)).unwrap_or(255)));
        }
    }
    Rgba { w: tw as u32, h: th as u32, pixels: out }
}

/// A picture file's own numbers, as an array: its bit depth kept, nothing shrunk or stripped, so
/// the probe and |A−B| read what the file holds. Bounded as [`decode_picture`] is.
pub fn decode_numbers(bytes: &[u8]) -> Result<Blob, String> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_PIXELS * 4);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    reader.limits(limits);
    let picture = reader.decode().map_err(|e| e.to_string())?;
    let (w, h) = (u64::from(picture.width()), u64::from(picture.height()));
    use image::ColorType as C;
    let (dtype, channels) = match picture.color() {
        C::L8 => ("|u1", 1),
        C::La8 => ("|u1", 2),
        C::Rgb8 => ("|u1", 3),
        C::Rgba8 => ("|u1", 4),
        C::L16 => ("<u2", 1),
        C::La16 => ("<u2", 2),
        C::Rgb16 => ("<u2", 3),
        C::Rgba16 => ("<u2", 4),
        C::Rgb32F => ("<f4", 3),
        C::Rgba32F => ("<f4", 4),
        other => return Err(format!("this viewer can't read {other:?} pixels")),
    };
    let shape = if channels == 1 { vec![h, w] } else { vec![h, w, channels] };
    Ok(Blob { dtype: dtype.into(), shape, bytes: (picture.into_bytes()).into() })
}

/// The names of a picture file's channels.
fn image_channel_names(channels: usize) -> Vec<String> {
    let names: &[&str] = match channels {
        2 => &["L", "A"],
        3 => &["R", "G", "B"],
        4 => &["R", "G", "B", "A"],
        _ => &[],
    };
    names.iter().map(|n| (*n).to_owned()).collect()
}

/// Parse an SVG (bounded; [`plumcraft_svg::parse`]).
pub fn parse_svg(bytes: &[u8]) -> Result<resvg::usvg::Tree, String> {
    plumcraft_svg::parse(bytes)
}

/// The pixel size an SVG of `size` is rasterised to for `long` pixels on its long side, within
/// the bounds.
pub fn render_size(size: Vec2, long: f32) -> (u32, u32) {
    if !(size.x > 0.0 && size.y > 0.0 && size.x.is_finite() && size.y.is_finite()) {
        return (1, 1);
    }
    let mut long = long.clamp(16.0, MAX_TEXTURE_SIDE as f32);
    let px = |l: f32| (((size.x / size.x.max(size.y)) * l).round().max(1.0) as u32, ((size.y / size.x.max(size.y)) * l).round().max(1.0) as u32);
    let (mut w, mut h) = px(long);
    while u64::from(w) * u64::from(h) > MAX_RENDER_PIXELS && long > 16.0 {
        long *= 0.9;
        (w, h) = px(long);
    }
    (w, h)
}

/// Rasterise `tree` to `w × h`, on white as a browser shows it ([`plumcraft_svg::render`]): a
/// drawing's dark lines can't be seen on the dark checkerboard.
pub fn render_svg(tree: &resvg::usvg::Tree, w: u32, h: u32) -> Result<Rgba, String> {
    plumcraft_svg::render(tree, w, h).map(plumcraft_svg::Raster::on_paper).map(|r| Rgba { w: r.w, h: r.h, pixels: r.pixels })
}

/// A picture with the texture made from it. Textures belong to one egui context: an offscreen
/// picture of the editor ([`crate::offscreen`]) draws on a second one, where the window's
/// textures draw nothing, so the texture is made again on whichever context asks (an upload,
/// no decoding or rasterising).
#[derive(Default)]
pub(crate) struct Picture {
    img: Option<Arc<Rgba>>,
    tex: Option<(egui::Context, egui::TextureHandle)>,
    /// Each pixel a block when enlarged (a raster picture, not a drawing).
    blocks: bool,
}

impl Picture {
    /// A picture whose pixels show as blocks when enlarged.
    pub(crate) fn blocks() -> Picture {
        Picture { blocks: true, ..Picture::default() }
    }

    fn options(&self) -> egui::TextureOptions {
        if self.blocks { PIXEL_TEXTURE } else { egui::TextureOptions::LINEAR }
    }

    pub(crate) fn set(&mut self, ctx: &egui::Context, name: &str, img: Rgba) {
        let tex = load_picture(ctx, name, img.image(), self.options());
        self.img = Some(Arc::new(img));
        self.tex = Some((ctx.clone(), tex));
    }

    /// The texture on `ctx`, made there if it was made on another.
    pub(crate) fn texture(&mut self, ctx: &egui::Context, name: &str) -> Option<egui::TextureHandle> {
        let img = self.img.as_ref()?;
        if !self.tex.as_ref().is_some_and(|(c, _)| c == ctx) {
            let tex = load_picture(ctx, name, img.image(), self.options());
            self.tex = Some((ctx.clone(), tex));
        }
        self.tex.as_ref().map(|(_, t)| t.clone())
    }

    /// The picture's pixel size.
    pub(crate) fn size(&self) -> Option<(u32, u32)> {
        self.img.as_ref().map(|i| (i.w, i.h))
    }

    pub(crate) fn is_some(&self) -> bool {
        self.img.is_some()
    }
}

/// One piece of work on a worker thread, polled each frame.
pub(crate) struct Job<T> {
    rx: Receiver<Result<T, String>>,
}

impl<T: Send + 'static> Job<T> {
    pub(crate) fn spawn(ctx: &egui::Context, work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Job<T> {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("plumcraft-media".into()).spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
        if let Err(e) = spawned {
            let (tx, rx2) = channel();
            let _ = tx.send(Err(e.to_string()));
            return Job { rx: rx2 };
        }
        Job { rx }
    }

    /// The result once it is there; a worker that died (a panic in a decoder) is an error.
    pub(crate) fn poll(&self) -> Option<Result<T, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the decoder stopped".into())),
        }
    }
}

/// The key of a display's value: its full handle, else where it is, else its summary.
pub(crate) fn key_of(d: &Display) -> String {
    match &d.full {
        Some(f) => f.handle.as_ref().map_or_else(|| format!("{:?}", f.at), ToString::to_string),
        None => format!("{}.{}:{}", d.node, d.port, d.summary),
    }
}

/// The rendition drawn when nothing better is there.
fn rendition_ui(ui: &mut egui::Ui, zp: &mut ZoomPan, rendition: Option<&egui::TextureHandle>, note: &str, tokens: &Tokens) {
    match rendition {
        Some(t) => {
            let [w, h] = t.size();
            draw_picture(ui, zp, t, vec2(w as f32, h as f32), &format!("the engine's {w}×{h} rendition"), tokens);
            ui.label(egui::RichText::new(format!("the engine's {w}×{h} rendition{note}")).small().color(tokens.text_faint));
        }
        None => {
            ui.label(egui::RichText::new(format!("no picture{note}")).color(tokens.text_dim));
        }
    }
}

// ---------- the image view ----------

/// Raster images (`image` media): the full value decoded, else the rendition.
#[derive(Default)]
pub struct ImageView {
    key: Option<String>,
    job: Option<Job<Rgba>>,
    picture: Picture,
    /// The picture's own size (before shrinking).
    source: Option<(u32, u32)>,
    error: Option<String>,
    rendition: Option<egui::TextureHandle>,
    pub zoom: ZoomPan,
    /// The probe, and the file's own numbers, decoded when something wants them.
    pub tools: PixelTools,
    numbers: Option<Job<Blob>>,
    /// Compare needs the numbers whether or not the probe does.
    pub want_numbers: bool,
}

impl ImageView {
    /// The image's size in its own pixels.
    fn grid(&self) -> Option<Grid> {
        self.source.and_then(|(w, h)| Grid::of_shape(&[u64::from(h), u64::from(w)]))
    }

    /// The file's numbers, decoded on a worker once something wants them.
    fn sync_numbers(&mut self, ctx: &egui::Context, d: &Display, full: Option<&Arc<Blob>>) {
        if let Some(r) = self.numbers.as_ref().and_then(Job::poll) {
            self.numbers = None;
            self.tools.source = match r {
                Ok(b) => {
                    let names = b.shape.get(2).map_or(0, |c| usize::try_from(*c).unwrap_or(0));
                    match probe::Pixels::of(&Arc::new(b), &image_channel_names(names)) {
                        Ok(p) => Source::Ready(p),
                        Err(why) => Source::Unreadable(why),
                    }
                }
                Err(why) => Source::Unreadable(format!("the picture can't be read: {why}")),
            };
        }
        if !matches!(self.tools.source, Source::None) || !(self.want_numbers || self.tools.wants_numbers()) {
            return;
        }
        match full {
            Some(b) => {
                self.tools.source = Source::Reading;
                let b = Arc::clone(b);
                self.numbers = Some(Job::spawn(ctx, move || decode_numbers(&b.bytes)));
            }
            None => self.tools.source = Source::Unreadable(probe::no_numbers(d, d.full.is_some())),
        }
    }

    /// What can be read of the image.
    pub fn source(&self) -> &Source {
        &self.tools.source
    }

    /// The decoded picture as a texture on `ctx`.
    pub(crate) fn texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        self.picture.texture(ctx, "plumcraft-watched-image")
    }
}

impl MediaViewer for ImageView {
    fn media(&self) -> &'static [Media] {
        &[Media::Image]
    }

    fn asks(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.tools.asks)
    }

    fn sync(&mut self, ctx: &egui::Context, display: Option<&Display>, full: Option<&Arc<Blob>>, rendition: Option<&egui::TextureHandle>) {
        self.rendition = rendition.cloned();
        let Some(d) = display else {
            *self = ImageView::default();
            return;
        };
        let key = key_of(d);
        if self.key.as_ref() != Some(&key) || (!self.picture.is_some() && self.job.is_none() && self.error.is_none() && full.is_some()) {
            self.key = Some(key);
            self.picture = Picture::blocks();
            self.error = None;
            self.source = None;
            self.numbers = None;
            self.tools.source = Source::None;
            self.tools.new_value();
            self.job = match full {
                Some(b) => {
                    self.source = picture_size(&b.bytes);
                    let b = Arc::clone(b);
                    Some(Job::spawn(ctx, move || decode_picture(&b.bytes, MAX_TEXTURE_SIDE)))
                }
                None => None,
            };
        }
        if let Some(r) = self.job.as_ref().and_then(Job::poll) {
            self.job = None;
            match r {
                Ok(img) => self.picture.set(ctx, "plumcraft-watched-image", img),
                Err(e) => self.error = Some(e),
            }
        }
        self.sync_numbers(ctx, d, full);
    }

    fn ui(&mut self, ui: &mut egui::Ui, tokens: &Tokens) {
        self.tools.asks.extend(crate::picture_tools::zoom_bar(ui));
        self.tools.begin();
        let ctx = ui.ctx().clone();
        match (self.picture.texture(&ctx, "plumcraft-watched-image"), self.picture.size(), self.grid()) {
            (Some(t), Some((w, h)), Some(grid)) => {
                let (sw, sh) = (grid.cols, grid.rows);
                pixel_picture(ui, &mut self.zoom, &mut self.tools, &t, grid, vec2(sw as f32, sh as f32), tokens);
                self.tools.status(ui, grid, tokens);
                let shrunk = if (sw, sh) != (w as usize, h as usize) { format!(", shown at {w}×{h}") } else { String::new() };
                ui.label(egui::RichText::new(format!("{sw}×{sh} px{shrunk}")).small().color(tokens.text_dim));
                self.tools.readout(ui, tokens);
            }
            (Some(t), Some((w, h)), None) => {
                draw_picture(ui, &mut self.zoom, &t, vec2(w as f32, h as f32), &format!("{w}×{h} px"), tokens);
            }
            _ if self.job.is_some() => {
                ui.label(egui::RichText::new("decoding…").color(tokens.text_dim));
                rendition_ui(ui, &mut self.zoom, self.rendition.as_ref(), "", tokens);
            }
            _ => {
                let note = self.error.as_ref().map(|e| format!(" (the image: {e})")).unwrap_or_default();
                rendition_ui(ui, &mut self.zoom, self.rendition.as_ref(), &note, tokens);
            }
        }
    }

    fn inspect(&self) -> Value {
        let state = if self.picture.is_some() {
            "ready"
        } else if self.job.is_some() {
            "decoding"
        } else if self.rendition.is_some() {
            "rendition"
        } else {
            "none"
        };
        json!({"kind": "image", "state": state, "size": self.source, "texture": self.picture.size().map(|(w, h)| [w, h]), "error": self.error, "zoom": self.zoom.zoom,
               "tools": self.tools.inspect()})
    }
}

// ---------- the SVG view ----------

/// The SVG view's texture name.
const SVG_TEXTURE: &str = "plumcraft-watched-svg";

/// SVG (`vector` media): parsed once, rasterised at the zoom shown.
#[derive(Default)]
pub struct SvgView {
    key: Option<String>,
    parse: Option<Job<Arc<resvg::usvg::Tree>>>,
    tree: Option<Arc<resvg::usvg::Tree>>,
    render: Option<(Job<Rgba>, (u32, u32))>,
    picture: Picture,
    /// The long side wanted at the current zoom (pixels).
    want: f32,
    error: Option<String>,
    rendition: Option<egui::TextureHandle>,
    /// Rasterisations so far for this value.
    pub renders: u32,
    pub zoom: ZoomPan,
}

impl SvgView {
    /// The SVG's own size, once parsed.
    pub fn size(&self) -> Option<Vec2> {
        self.tree.as_ref().map(|t| vec2(t.size().width(), t.size().height()))
    }

    /// The size of the raster shown.
    pub fn raster(&self) -> Option<(u32, u32)> {
        self.picture.size()
    }

    /// The latest raster as a texture on `ctx` (a preview's picture), once there is one.
    pub fn texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        self.picture.texture(ctx, SVG_TEXTURE)
    }

    /// Why the SVG can't be drawn, if it can't.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Rasterise again when the zoom wants a raster over a quarter larger (or under a quarter
    /// of the size).
    fn rerender(&mut self, ctx: &egui::Context) {
        let (Some(tree), None) = (&self.tree, &self.render) else { return };
        let size = vec2(tree.size().width(), tree.size().height());
        let (w, h) = render_size(size, self.want);
        if let Some((tw, th)) = self.picture.size() {
            let (have, want) = (tw.max(th) as f32, w.max(h) as f32);
            // sharper when zoomed in; smaller only when far too large (a texture's memory)
            if (want <= have * 1.25 && want >= have * 0.25) || (have >= want && have >= MAX_TEXTURE_SIDE as f32 * 0.9) {
                return;
            }
        }
        let tree = Arc::clone(tree);
        self.render = Some((Job::spawn(ctx, move || render_svg(&tree, w, h)), (w, h)));
    }
}

impl MediaViewer for SvgView {
    fn media(&self) -> &'static [Media] {
        &[Media::Vector]
    }

    fn sync(&mut self, ctx: &egui::Context, display: Option<&Display>, full: Option<&Arc<Blob>>, rendition: Option<&egui::TextureHandle>) {
        self.rendition = rendition.cloned();
        let Some(d) = display else {
            *self = SvgView::default();
            return;
        };
        let key = key_of(d);
        if self.key.as_ref() != Some(&key) || (self.tree.is_none() && self.parse.is_none() && self.error.is_none() && full.is_some()) {
            let zoom = self.zoom;
            *self = SvgView { key: Some(key), zoom, want: 512.0, rendition: rendition.cloned(), ..Default::default() };
            if let Some(b) = full {
                let b = Arc::clone(b);
                self.parse = Some(Job::spawn(ctx, move || parse_svg(&b.bytes).map(Arc::new)));
            }
        }
        if let Some(r) = self.parse.as_ref().and_then(Job::poll) {
            self.parse = None;
            match r {
                Ok(t) => self.tree = Some(t),
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(r) = self.render.as_ref().and_then(|(j, _)| j.poll()) {
            self.render = None;
            match r {
                Ok(img) => {
                    self.renders = self.renders.saturating_add(1);
                    self.picture.set(ctx, SVG_TEXTURE, img);
                }
                Err(e) => self.error = Some(e),
            }
        }
        if self.error.is_none() {
            self.rerender(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tokens: &Tokens) {
        self.zoom.toolbar(ui, tokens);
        let ctx = ui.ctx().clone();
        match (self.size(), self.picture.texture(&ctx, SVG_TEXTURE), self.picture.size()) {
            (Some(size), Some(t), Some((w, h))) => {
                let view = draw_picture(ui, &mut self.zoom, &t, size, &format!("{:.0}×{:.0} SVG", size.x, size.y), tokens);
                // the long side the picture covers on screen, in pixels: the next raster's size
                let shown = self.zoom.place(view, size);
                self.want = shown.width().max(shown.height()) * ui.ctx().pixels_per_point();
                let busy = if self.render.is_some() { ", sharpening…" } else { "" };
                ui.label(egui::RichText::new(format!("SVG {:.0}×{:.0}, drawn at {w}×{h} px (resvg){busy}", size.x, size.y)).small().color(tokens.text_dim));
            }
            _ if self.error.is_none() && (self.parse.is_some() || self.render.is_some()) => {
                ui.label(egui::RichText::new("drawing…").color(tokens.text_dim));
                rendition_ui(ui, &mut self.zoom, self.rendition.as_ref(), "", tokens);
            }
            _ => {
                let note = self.error.as_ref().map(|e| format!(" (the SVG: {e})")).unwrap_or_default();
                rendition_ui(ui, &mut self.zoom, self.rendition.as_ref(), &note, tokens);
            }
        }
    }

    fn inspect(&self) -> Value {
        let state = if self.error.is_some() {
            "fallback"
        } else if self.picture.is_some() {
            "ready"
        } else if self.parse.is_some() || self.render.is_some() {
            "drawing"
        } else {
            "none"
        };
        json!({"kind": "svg", "state": state, "size": self.size().map(|s| [s.x, s.y]), "raster": self.raster(), "renders": self.renders,
               "error": self.error, "zoom": self.zoom.zoom})
    }
}

// ---------- the document view ----------

/// "Open in …" for the watched document: enabled with its label, or disabled with why.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenButton {
    pub label: String,
    pub blocked: Option<String>,
    /// The latest open: where the copy went, or why it failed.
    pub note: Option<String>,
}

/// A craft document (`doc` media): the engine's rendition, and "Open in …".
#[derive(Default)]
pub struct DocView {
    rendition: Option<egui::TextureHandle>,
    summary: String,
    /// Set by the panel before drawing (`doc.open`'s target), and whether it was clicked.
    pub open: Option<OpenButton>,
    pub clicked: bool,
    pub zoom: ZoomPan,
}

impl MediaViewer for DocView {
    fn media(&self) -> &'static [Media] {
        &[Media::Doc]
    }

    fn sync(&mut self, _ctx: &egui::Context, display: Option<&Display>, _full: Option<&Arc<Blob>>, rendition: Option<&egui::TextureHandle>) {
        self.rendition = rendition.cloned();
        self.summary = display.map(|d| d.summary.clone()).unwrap_or_default();
    }

    fn ui(&mut self, ui: &mut egui::Ui, tokens: &Tokens) {
        if let Some(b) = &self.open {
            let r = ui.add_enabled(b.blocked.is_none(), egui::Button::new(format!("Open in {}", b.label)));
            let r = match &b.blocked {
                Some(why) => r.on_disabled_hover_text(why),
                None => r.on_hover_text(format!("Open a copy of this document in {}: edits there never change the engine's file", b.label)),
            };
            if r.clicked() {
                self.clicked = true;
            }
            if let Some(why) = &b.blocked {
                ui.label(egui::RichText::new(why).small().color(tokens.text_faint));
            }
            if let Some(note) = &b.note {
                ui.label(egui::RichText::new(note).small().color(tokens.text_dim));
            }
        }
        self.zoom.toolbar(ui, tokens);
        rendition_ui(ui, &mut self.zoom, self.rendition.as_ref(), " (export SVG or PNG for the full picture)", tokens);
    }

    fn inspect(&self) -> Value {
        json!({"kind": "doc", "rendition": self.rendition.as_ref().map(egui::TextureHandle::size),
               "open": self.open.as_ref().map(|b| json!({"label": b.label, "enabled": b.blocked.is_none(), "reason": b.blocked, "note": b.note})), "zoom": self.zoom.zoom})
    }
}

/// The picture viewers, one per media kind.
#[derive(Default)]
pub struct MediaViews {
    pub image: ImageView,
    pub svg: SvgView,
    pub doc: DocView,
    /// Arrays (ndarray media): synced by the host with every picture's texture.
    pub array: crate::array_view::ArrayView,
}

impl MediaViews {
    /// The viewer for `media`, if one of these shows it.
    pub fn viewer(&mut self, media: Media) -> Option<&mut dyn MediaViewer> {
        let all: [&mut dyn MediaViewer; 3] = [&mut self.image, &mut self.svg, &mut self.doc];
        all.into_iter().find(|v| v.media().contains(&media))
    }

    /// The state of the viewer for `media` (`ui.inspect`'s `watched.view`).
    pub fn inspect(&self, media: Media) -> Option<Value> {
        if media == Media::Ndarray {
            return Some(self.array.inspect());
        }
        let all: [&dyn MediaViewer; 3] = [&self.image, &self.svg, &self.doc];
        all.into_iter().find(|v| v.media().contains(&media)).map(MediaViewer::inspect)
    }

    /// Give the watched display to its viewer (and clear the others).
    pub fn sync(&mut self, ctx: &egui::Context, display: Option<&Display>, full: Option<&Arc<Blob>>, rendition: Option<&egui::TextureHandle>) {
        let media = display.map(|d| d.media);
        for v in [&mut self.image as &mut dyn MediaViewer, &mut self.svg, &mut self.doc] {
            if media.is_some_and(|m| v.media().contains(&m)) {
                v.sync(ctx, display, full, rendition);
            } else {
                v.sync(ctx, None, None, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_of(w: u32, h: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(color);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(data).unwrap();
        out
    }

    /// A texture belongs to one context: an offscreen picture of the editor draws on another,
    /// where the picture is uploaded again (it drew nothing before: the blank SVG in the
    /// README's offscreen pictures), and once per context, not every frame.
    /// JPEG, TIFF, WebP and GIF, not only PNG: a red picture stays red, RGB in every format, and
    /// junk and hostile sizes are errors, never panics.
    #[test]
    fn every_picture_format_decodes_in_rgb_order() {
        let red = image::RgbImage::from_pixel(32, 16, image::Rgb([230, 20, 10]));
        for format in [image::ImageFormat::Jpeg, image::ImageFormat::Tiff, image::ImageFormat::WebP, image::ImageFormat::Gif, image::ImageFormat::Png] {
            let mut bytes = std::io::Cursor::new(Vec::new());
            red.write_to(&mut bytes, format).unwrap();
            let bytes = bytes.into_inner();
            assert_eq!(picture_size(&bytes), Some((32, 16)), "{format:?}");
            let img = decode_picture(&bytes, 4096).unwrap();
            assert_eq!((img.w, img.h), (32, 16), "{format:?}");
            let (r, g, b) = (img.pixels[0], img.pixels[1], img.pixels[2]);
            assert!(r > 200 && g < 60 && b < 60, "{format:?} reads {r} {g} {b}");
            assert_eq!(decode_picture(&bytes, 8).unwrap().w, 8, "{format:?} shrinks");
        }
        for junk in [&b""[..], b"not a picture", b"\xff\xd8\xff\xe0junk", b"II*\x00junk", b"GIF89a", b"RIFF\x00\x00\x00\x00WEBPjunk"] {
            assert!(decode_picture(junk, 64).is_err(), "{junk:?}");
        }
    }

    /// A 16-bit picture's numbers are its own: stripped to 8 bits, 40000 would read 156 × 257.
    #[test]
    fn a_files_numbers_keep_its_bit_depth() {
        let img = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_fn(3, 2, |x, y| image::Luma([40000 + (y * 3 + x) as u16]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        img.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let b = decode_numbers(&bytes.into_inner()).unwrap();
        assert_eq!((b.dtype.as_str(), b.shape.as_slice()), ("<u2", &[2u64, 3][..]));
        let p = probe::Pixels::of(&Arc::new(b), &[]).unwrap();
        assert_eq!(p.at(1, 2, 0), Some(probe::Element::UInt(40005)));
        let rgb = image::RgbImage::from_pixel(2, 1, image::Rgb([9, 8, 7]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        rgb.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let b = decode_numbers(&bytes.into_inner()).unwrap();
        assert_eq!((b.dtype.as_str(), b.shape.as_slice(), &b.bytes[3..6]), ("|u1", &[1u64, 2, 3][..], &[9u8, 8, 7][..]));
        assert_eq!(image_channel_names(3), ["R", "G", "B"]);
        assert!(decode_numbers(b"not a picture").is_err());
    }

    /// egui panics on a texture past the context's limit, and a context that was never told one
    /// says 2048: a 4000-pixel view killed the editor on its way to an offscreen picture.
    #[test]
    fn a_picture_larger_than_the_gpu_takes_is_shrunk_not_a_panic() {
        let ctx = egui::Context::default();
        let mut p = Picture::blocks();
        let mut size = None;
        let mut out = ctx.run_ui(egui::RawInput { max_texture_side: Some(1024), ..Default::default() }, |_| {
            p.set(&ctx, "t", Rgba { w: 2000, h: 1000, pixels: vec![255; 2000 * 1000 * 4] });
            size = p.texture(&ctx, "t").map(|t| t.size());
        });
        out.textures_delta.clear();
        assert_eq!(size, Some([1024, 512]));
    }

    #[test]
    fn a_picture_is_uploaded_again_on_another_context() {
        let (window, offscreen) = (egui::Context::default(), egui::Context::default());
        let mut p = Picture::default();
        assert!(p.texture(&window, "t").is_none() && !p.is_some());
        p.set(&window, "t", Rgba { w: 2, h: 1, pixels: vec![255; 8] });
        let count = |c: &egui::Context| c.tex_manager().read().num_allocated();
        let before = count(&offscreen);
        let there = p.texture(&offscreen, "t").unwrap();
        assert_eq!(count(&offscreen), before + 1, "uploaded on the offscreen context");
        assert_eq!(p.texture(&offscreen, "t").unwrap().id(), there.id(), "once");
        let back = count(&window);
        assert!(p.texture(&window, "t").is_some());
        assert_eq!(count(&window), back + 1, "and again on the way back");
        assert_eq!(p.size(), Some((2, 1)));
    }

    #[test]
    fn pngs_decode_shrink_and_refuse_junk() {
        let data: Vec<u8> = (0..300 * 100).flat_map(|i| [(i % 256) as u8, 7, 9]).collect();
        let img = decode_png(&png_of(300, 100, png::ColorType::Rgb, &data), 4096).unwrap();
        assert_eq!((img.w, img.h, img.pixels.len()), (300, 100, 300 * 100 * 4));
        assert_eq!(&img.pixels[..8], &[0, 7, 9, 255, 1, 7, 9, 255]);
        let small = decode_png(&png_of(300, 100, png::ColorType::Rgb, &data), 150).unwrap();
        assert_eq!((small.w, small.h), (150, 50));
        assert!(decode_png(b"", 10).is_err());
        let mut huge = png_of(1, 1, png::ColorType::Rgb, &[1, 2, 3]);
        huge[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        assert!(decode_png(&huge, 10).unwrap_err().contains("too large"));
        let mut torn = png_of(8, 8, png::ColorType::Rgb, &[7; 192]);
        torn.truncate(torn.len() - 20);
        assert!(decode_png(&torn, 10).is_err());
    }

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
        <rect x="0" y="0" width="100" height="100" fill="#ff0000"/><rect x="100" y="0" width="100" height="100" fill="#0000ff"/></svg>"##;

    #[test]
    fn svgs_render_at_the_size_asked_and_refuse_files_and_junk() {
        let tree = parse_svg(SVG.as_bytes()).unwrap();
        assert_eq!((tree.size().width(), tree.size().height()), (200.0, 100.0));
        let (w, h) = render_size(vec2(200.0, 100.0), 800.0);
        assert_eq!((w, h), (800, 400));
        let img = render_svg(&tree, w, h).unwrap();
        let px = |x: u32, y: u32| img.pixels[((y * img.w + x) * 4) as usize..][..4].to_vec();
        assert_eq!(px(100, 200), [255, 0, 0, 255]);
        assert_eq!(px(700, 200), [0, 0, 255, 255]);
        // bounded: never more than the texture side or the pixel budget
        let (w, h) = render_size(vec2(1.0, 1.0), 1e9);
        assert!(w <= MAX_TEXTURE_SIDE && u64::from(w) * u64::from(h) <= MAX_RENDER_PIXELS, "{w}×{h}");
        assert_eq!(render_size(vec2(0.0, f32::NAN), 100.0), (1, 1));
        // an external file is never read
        let secret = plumcraft_scratch::root().join("svg-secret.png");
        std::fs::write(&secret, png_of(1, 1, png::ColorType::Rgb, &[0, 255, 0])).unwrap();
        let ext = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="10" height="10"><image width="10" height="10" xlink:href="{}"/></svg>"#,
            secret.display()
        );
        let img = render_svg(&parse_svg(ext.as_bytes()).unwrap(), 10, 10).unwrap();
        assert!(img.pixels.chunks(4).all(|p| p == [255, 255, 255, 255]), "bare paper: nothing drawn from the file");
        let _ = std::fs::remove_file(secret);
        for junk in [&b""[..], b"<svg", b"\x89PNG", b"<svg xmlns='http://www.w3.org/2000/svg' width='-5' height='x'/>"] {
            let _ = parse_svg(junk).and_then(|t| render_svg(&t, 8, 8));
        }
        assert!(parse_svg(&vec![b' '; MAX_SVG_BYTES + 1]).unwrap_err().contains("too large"));
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut z = ZoomPan::default();
        let view = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 200.0));
        let size = vec2(200.0, 100.0);
        assert_eq!(z.place(view, size), view);
        let at = vec2(100.0, 50.0);
        let before = z.place(view, size);
        let p = (view.center() + at - before.min) / before.size();
        z.zoom_at(2.0, at);
        let after = z.place(view, size);
        let q = (view.center() + at - after.min) / after.size();
        assert!((p - q).length() < 1e-4, "{p:?} {q:?}");
        z.zoom_at(1e9, Vec2::ZERO);
        assert_eq!(z.zoom, MAX_ZOOM);
        z.zoom_at(f32::NAN, Vec2::ZERO);
        z.zoom_at(-1.0, Vec2::ZERO);
        assert_eq!(z.zoom, MAX_ZOOM);
        z.fit();
        assert_eq!(z, ZoomPan::default());
    }

    #[test]
    fn the_views_follow_the_display_and_fall_back_to_the_rendition() {
        let ctx = egui::Context::default();
        let mut views = MediaViews::default();
        let d = Display { media: Media::Vector, node: "s".into(), port: "svg".into(), summary: "svg".into(), ..Default::default() };
        let blob = Arc::new(Blob { dtype: "|u1".into(), shape: vec![SVG.len() as u64], bytes: (SVG.as_bytes().to_vec()).into() });
        let start = std::time::Instant::now();
        while views.svg.raster().is_none() {
            assert!(start.elapsed() < std::time::Duration::from_secs(10), "{}", views.svg.inspect());
            views.sync(&ctx, Some(&d), Some(&blob), None);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(views.svg.inspect()["state"], "ready");
        assert_eq!(views.svg.raster(), Some((512, 256)));
        assert_eq!(views.image.inspect()["state"], "none", "the others are cleared");
        // zoomed in: drawn again, larger
        views.svg.want = 2048.0;
        while views.svg.raster() != Some((2048, 1024)) {
            assert!(start.elapsed() < std::time::Duration::from_secs(10), "{}", views.svg.inspect());
            views.sync(&ctx, Some(&d), Some(&blob), None);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(views.svg.renders, 2);
        // a broken SVG: the rendition, with why
        let bad = Display { summary: "bad".into(), ..d.clone() };
        let junk = Arc::new(Blob { bytes: b"<svg".to_vec().into(), ..(*blob).clone() });
        while views.svg.inspect()["state"] != "fallback" {
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            views.sync(&ctx, Some(&bad), Some(&junk), None);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(views.viewer(Media::Doc).is_some() && views.viewer(Media::Audio).is_none());
        // everything draws, on hostile and empty states alike
        let tokens = crate::theme::DEFAULT;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            views.svg.ui(ui, &tokens);
            views.image.ui(ui, &tokens);
            views.doc.open = Some(OpenButton { label: "VectorCraft".into(), blocked: Some("remote".into()), note: None });
            views.doc.ui(ui, &tokens);
        });
        out.textures_delta.clear();
    }
}
