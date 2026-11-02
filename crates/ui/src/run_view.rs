//! Run results on the canvas: state names and badges, durations, thumbnails, error text, and
//! hit testing of wires (egui-snarl 0.12 only reports right-clicks on wires, so a left click is
//! tested here against the curves it draws).
//!
//! Lifted from the research software's `crates/ui-egui/src/run_view.rs`, without settings bundles. Run states
//! are protocol 2's [`NodeState`]; their colours are theme tokens ([`crate::theme::Tokens::state_color`]).
//! Thumbnails are [`Rendition`]s: an `rgba8` one is read through the connection
//! ([`Thumbs`]); a `peaks` one is decoded (inline) and drawn as a waveform
//! ([`crate::audio::draw_waveform`]); an excerpt is drawn as text.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use egui::{Painter, Pos2, Rect, Style, Vec2, pos2};
use egui_snarl::ui::{PinInfo, PinWireInfo, SnarlPin, SnarlStyle};
use egui_snarl::{InPinId, OutPinId};
use plumcraft_proto::data::DataRef;
use plumcraft_proto::event::NodeState;
use plumcraft_store::Blob;

/// The longest side of a thumbnail in a node body (half the preview size, so nodes stay
/// compact; the research software's maintainer, 2026-10-17).
pub const THUMB_PX: f32 = 80.0;
/// The longest side of the thumbnail shown on hover.
pub const THUMB_HOVER_PX: f32 = 160.0;
/// Characters of an error shown on a node; the rest is on hover.
pub const ERROR_CHARS: usize = 180;
/// Lines and characters of a text excerpt shown on a node.
pub const EXCERPT_LINES: usize = 3;
pub const EXCERPT_CHARS: usize = 200;
/// The largest rgba8 thumbnail read (pixels on its long side).
pub const MAX_THUMB_SIDE: u64 = 4096;

/// A run state as a word (the node header, the status bar, tooltips).
pub fn state_name(s: NodeState) -> &'static str {
    match s {
        NodeState::Queued => "queued",
        NodeState::Started | NodeState::Progress | NodeState::Preview => "running",
        NodeState::Done => "done",
        NodeState::Cached => "cached",
        NodeState::Error => "error",
        NodeState::Skipped => "skipped",
        NodeState::Cancelled => "cancelled",
        NodeState::Unknown => "",
    }
}

/// Is the node computing now?
pub fn is_running(s: NodeState) -> bool {
    matches!(s, NodeState::Started | NodeState::Progress | NodeState::Preview)
}

/// A cached node's header text and its hover: what computing it took, and what fetching did.
pub fn cached_text(computed: Option<f64>, fetched: Option<f64>) -> (String, String) {
    match (computed, fetched) {
        (Some(c), Some(f)) => (format!("cached · {}", format_ms(c)), format!("Computed in {}, fetched in {}", format_ms(c), format_ms(f))),
        (Some(c), None) => (format!("cached · {}", format_ms(c)), format!("Computed in {}", format_ms(c))),
        (None, Some(f)) => ("cached".to_owned(), format!("Fetched in {}", format_ms(f))),
        (None, None) => ("cached".to_owned(), "From the cache".to_owned()),
    }
}

/// A duration for a node header or the status bar: `0.42 ms`, `12 ms`, `1.24 s`.
pub fn format_ms(ms: f64) -> String {
    if !ms.is_finite() || ms < 0.0 {
        "—".into()
    } else if ms < 10.0 {
        format!("{ms:.2} ms")
    } else if ms < 1000.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// The size to show a `w`×`h` picture at: the long side `max`, aspect kept.
pub fn thumb_size(w: f32, h: f32, max: f32) -> Vec2 {
    if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
        return Vec2::new(max, max * 0.5);
    }
    let scale = max / w.max(h);
    Vec2::new((w * scale).max(1.0), (h * scale).max(1.0))
}

/// At most `n` characters of `s`, with an ellipsis when cut. True when cut.
pub fn truncate_chars(s: &str, n: usize) -> (String, bool) {
    match s.char_indices().nth(n) {
        Some((i, _)) => (format!("{}…", s.get(..i).unwrap_or(s).trim_end()), true),
        None => (s.to_owned(), false),
    }
}

/// A text excerpt as a node shows it: its first [`EXCERPT_LINES`] lines and at most
/// [`EXCERPT_CHARS`] characters, so one long line can't stretch the node.
pub fn excerpt_lines(text: &str) -> String {
    let short: String = text.lines().take(EXCERPT_LINES).collect::<Vec<_>>().join("\n");
    let (short, cut) = truncate_chars(&short, EXCERPT_CHARS);
    if !cut && text.lines().nth(EXCERPT_LINES).is_some() { format!("{short}\n…") } else { short }
}

// ---------- rgba8 thumbnails ----------

/// Why a thumbnail can't be shown.
#[derive(Debug, Clone, PartialEq)]
pub enum ThumbMiss {
    /// Its bytes are gone (reaped from the store): the next run publishes them again.
    Missing,
    /// Not an (h, w, 4) `|u1` array, or too large.
    Shape(String),
}

/// Textures of rgba8 thumbnails, by handle. Read once through the connection; a miss is kept
/// until [`Thumbs::forget_misses`] (a run publishes them again).
#[derive(Default)]
pub struct Thumbs {
    cache: HashMap<String, Result<egui::TextureHandle, ThumbMiss>>,
    /// Waveform overviews (`peaks` renditions), decoded once, by handle.
    peaks: HashMap<String, Result<Rc<crate::audio::Peaks>, ThumbMiss>>,
    /// The viewer's pictures, sampled nearest-neighbour when enlarged ([`Thumbs::picture`]).
    pictures: HashMap<String, Result<egui::TextureHandle, ThumbMiss>>,
    /// Their bytes.
    picture_bytes: usize,
}

/// A picture's texture: smooth shrunk, and each of its pixels a block enlarged.
pub const PIXEL_TEXTURE: egui::TextureOptions = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, ..egui::TextureOptions::LINEAR };

/// Make a texture, shrunk to the largest this context's GPU takes: past that egui panics, and an
/// engine says how large a picture is, not this editor.
pub fn load_picture(ctx: &egui::Context, name: &str, img: egui::ColorImage, options: egui::TextureOptions) -> egui::TextureHandle {
    let max = ctx.input(|i| i.max_texture_side).max(1);
    if img.size[0].max(img.size[1]) <= max {
        return ctx.load_texture(name, img, options);
    }
    let pixels = img.pixels.iter().flat_map(|p| p.to_srgba_unmultiplied()).collect();
    let small = crate::media::shrink(crate::media::Rgba { w: img.size[0] as u32, h: img.size[1] as u32, pixels }, max as u32);
    ctx.load_texture(name, small.image(), options)
}

/// Bytes of pictures kept: a 12-megapixel view is 48 MB, and a picture in use stays alive in its
/// viewer whatever is dropped here.
const MAX_PICTURE_BYTES: usize = 512 << 20;

impl Thumbs {
    /// The texture for `data`, read with `resolve` the first time.
    pub fn get(&mut self, ctx: &egui::Context, data: &DataRef, resolve: &dyn Fn(&DataRef) -> Option<Blob>) -> Result<egui::TextureHandle, ThumbMiss> {
        let key = data.handle.as_ref().map(ToString::to_string).unwrap_or_else(|| format!("{:?}", data.at));
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let made = match resolve(data) {
            None => Err(ThumbMiss::Missing),
            Some(blob) => rgba_image(&blob).map(|img| load_picture(ctx, &format!("plumcraft-thumb-{key}"), img, egui::TextureOptions::LINEAR)),
        };
        if self.cache.len() > 512 {
            self.cache.clear();
        }
        self.cache.insert(key, made.clone());
        made
    }

    /// The texture of a viewer's picture for `data`, read with `resolve` the first time.
    pub fn picture(&mut self, ctx: &egui::Context, data: &DataRef, resolve: &dyn Fn(&DataRef) -> Option<Blob>) -> Result<egui::TextureHandle, ThumbMiss> {
        let key = data.handle.as_ref().map(ToString::to_string).unwrap_or_else(|| format!("{:?}", data.at));
        if let Some(hit) = self.pictures.get(&key) {
            return hit.clone();
        }
        let made = match resolve(data) {
            None => Err(ThumbMiss::Missing),
            Some(blob) => rgba_image(&blob).map(|img| load_picture(ctx, &format!("plumcraft-picture-{key}"), img, PIXEL_TEXTURE)),
        };
        let size = made.as_ref().map_or(0, |t| t.size()[0] * t.size()[1] * 4);
        if self.picture_bytes + size > MAX_PICTURE_BYTES {
            self.pictures.clear();
            self.picture_bytes = 0;
        }
        self.picture_bytes += size;
        self.pictures.insert(key, made.clone());
        made
    }

    /// The waveform overview for a `peaks` rendition's `data`, decoded the first time (inline
    /// bytes, else read with `resolve`).
    pub fn peaks(&mut self, data: &DataRef, resolve: &dyn Fn(&DataRef) -> Option<Blob>) -> Result<Rc<crate::audio::Peaks>, ThumbMiss> {
        let key = crate::audio::key_of(data);
        if let Some(hit) = self.peaks.get(&key) {
            return hit.clone();
        }
        let made = crate::audio::peaks_of(data, resolve).map(Rc::new);
        if self.peaks.len() > 512 {
            self.peaks.clear();
        }
        self.peaks.insert(key, made.clone());
        made
    }

    /// Is the overview for `data` decoded and cached?
    pub fn has_peaks(&self, data: &DataRef) -> bool {
        self.peaks.get(&crate::audio::key_of(data)).is_some_and(Result::is_ok)
    }

    /// How many waveform overviews are cached (decoded or not).
    pub fn peaks_cached(&self) -> usize {
        self.peaks.len()
    }

    /// Forget the thumbnails that were missing: a new run has published them again.
    pub fn forget_misses(&mut self) {
        self.pictures.retain(|_, v| v.is_ok());
        self.cache.retain(|_, v| v.is_ok());
        self.peaks.retain(|_, v| v.is_ok());
    }
}

/// An (h, w, 4) `|u1` blob as an image; anything else is a [`ThumbMiss::Shape`].
pub fn rgba_image(b: &Blob) -> Result<egui::ColorImage, ThumbMiss> {
    let [h, w, 4] = b.shape.as_slice() else { return Err(ThumbMiss::Shape(format!("shape {:?}, not (h, w, 4)", b.shape))) };
    if b.dtype != "|u1" {
        return Err(ThumbMiss::Shape(format!("dtype {}, not |u1", b.dtype)));
    }
    if *h == 0 || *w == 0 || *h > MAX_THUMB_SIDE || *w > MAX_THUMB_SIDE {
        return Err(ThumbMiss::Shape(format!("{w}×{h}")));
    }
    let (Ok(hu), Ok(wu)) = (usize::try_from(*h), usize::try_from(*w)) else { return Err(ThumbMiss::Shape(format!("{w}×{h}"))) };
    if b.bytes.len() != hu.saturating_mul(wu).saturating_mul(4) {
        return Err(ThumbMiss::Shape(format!("{} bytes for {w}×{h}", b.bytes.len())));
    }
    Ok(egui::ColorImage::from_rgba_unmultiplied([wu, hu], &b.bytes))
}

// ---------- wire geometry (mirrors egui-snarl 0.12's Bezier5 wires, MIT) ----------

/// Which pin a recorded position belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PinKey {
    In(InPinId),
    Out(OutPinId),
}

/// Pin centres (graph coordinates) recorded while the snarl draws.
pub type PinSink = Rc<RefCell<HashMap<PinKey, Pos2>>>;

/// A pin that records where it was drawn.
pub struct TrackedPin {
    pub info: PinInfo,
    pub key: PinKey,
    pub sink: PinSink,
}

impl SnarlPin for TrackedPin {
    fn draw(self, snarl_style: &SnarlStyle, style: &Style, rect: Rect, painter: &Painter) -> PinWireInfo {
        self.sink.borrow_mut().insert(self.key, rect.center());
        SnarlPin::draw(self.info, snarl_style, style, rect, painter)
    }
}

/// egui-snarl's frame size for a wire from `from` to `to` (`pin size × 3`, at most a sixth of
/// the distance).
pub fn wire_frame(style: &Style, from: Pos2, to: Pos2) -> f32 {
    let frame = style.spacing.interact_size.y * 0.6 * 3.0;
    frame.min((from - to).length() / 6.0)
}

/// The control points of a wire (egui-snarl's `wire_bezier_5`, simplified for hit testing: the
/// backward cases use the straight variant).
pub fn wire_points(frame: f32, from: Pos2, to: Pos2) -> [Pos2; 6] {
    let from_2 = pos2(from.x + frame, from.y);
    let to_2 = pos2(to.x - frame, to.y);
    let between = (from_2 - to_2).length();
    if from_2.x <= to_2.x && between >= frame * 2.0 {
        let m1 = from_2 + (to_2 - from_2).normalized() * frame;
        let m2 = to_2 + (from_2 - to_2).normalized() * frame;
        [from, from_2, m1, m2, to_2, to]
    } else if from_2.x <= to_2.x {
        [from, from_2, from_2, to_2, to_2, to]
    } else {
        let mid_y = (from_2.y + to_2.y) * 0.5;
        [from, from_2, pos2(from_2.x, mid_y), pos2(to_2.x, mid_y), to_2, to]
    }
}

fn sample(points: &[Pos2; 6], t: f32) -> Pos2 {
    let mut p = *points;
    for k in (1..6).rev() {
        for i in 0..k {
            if let (Some(a), Some(b)) = (p.get(i).copied(), p.get(i + 1).copied())
                && let Some(slot) = p.get_mut(i)
            {
                *slot = a.lerp(b, t);
            }
        }
    }
    p[0]
}

/// The point halfway along the wire from `from` to `to`.
pub fn wire_midpoint(frame: f32, from: Pos2, to: Pos2) -> Pos2 {
    sample(&wire_points(frame, from, to), 0.5)
}

/// The distance from `p` to the wire from `from` to `to`.
pub fn wire_distance(frame: f32, from: Pos2, to: Pos2, p: Pos2) -> f32 {
    let pts = wire_points(frame, from, to);
    const N: usize = 48;
    let mut best = f32::INFINITY;
    let mut prev = from;
    for i in 1..=N {
        let next = sample(&pts, i as f32 / N as f32);
        best = best.min(segment_distance(prev, next, p));
        prev = next;
    }
    best
}

fn segment_distance(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(format_ms(0.213), "0.21 ms");
        assert_eq!(format_ms(41.6), "42 ms");
        assert_eq!(format_ms(1234.0), "1.23 s");
        assert_eq!(format_ms(f64::NAN), "—");
        assert_eq!(cached_text(Some(4410.0), Some(0.01)), ("cached · 4.41 s".to_owned(), "Computed in 4.41 s, fetched in 0.01 ms".to_owned()));
        assert_eq!(cached_text(None, None).0, "cached");
    }

    #[test]
    fn thumbs_keep_their_aspect() {
        assert_eq!(thumb_size(32.0, 26.0, 160.0), Vec2::new(160.0, 130.0));
        assert_eq!(thumb_size(80.0, 160.0, 160.0), Vec2::new(80.0, 160.0));
        assert_eq!(thumb_size(0.0, 10.0, 160.0), Vec2::new(160.0, 80.0));
    }

    #[test]
    fn long_errors_and_excerpts_are_cut() {
        assert_eq!(truncate_chars("short", 10), ("short".into(), false));
        let (t, cut) = truncate_chars("ééééééé", 3);
        assert!(cut && t == "ééé…");
        assert_eq!(excerpt_lines("a\nb"), "a\nb");
        assert_eq!(excerpt_lines("a\nb\nc\nd"), "a\nb\nc\n…");
        assert_eq!(excerpt_lines(&"x".repeat(500)).chars().count(), EXCERPT_CHARS + 1);
    }

    #[test]
    fn states_have_words() {
        assert_eq!(state_name(NodeState::Progress), "running");
        assert!(is_running(NodeState::Preview) && !is_running(NodeState::Done));
    }

    #[test]
    fn rgba_blobs_are_checked() {
        let ok = Blob { dtype: "|u1".into(), shape: vec![2, 3, 4], bytes: (vec![255; 24]).into() };
        assert_eq!(rgba_image(&ok).unwrap().size, [3, 2]);
        for bad in [
            Blob { dtype: "<f4".into(), shape: vec![2, 3, 4], bytes: (vec![0; 96]).into() },
            Blob { dtype: "|u1".into(), shape: vec![2, 3, 3], bytes: (vec![0; 18]).into() },
            Blob { dtype: "|u1".into(), shape: vec![2, 3, 4], bytes: (vec![0; 23]).into() },
            Blob { dtype: "|u1".into(), shape: vec![u64::MAX, 3, 4], bytes: (vec![]).into() },
            Blob { dtype: "|u1".into(), shape: vec![], bytes: (vec![]).into() },
        ] {
            assert!(matches!(rgba_image(&bad), Err(ThumbMiss::Shape(_))), "{bad:?}");
        }
    }

    #[test]
    fn wires_are_hit_near_their_curve() {
        let (from, to) = (pos2(0.0, 0.0), pos2(300.0, 100.0));
        let f = 30.0;
        assert!(wire_distance(f, from, to, pos2(150.0, 50.0)) < 4.0, "the middle");
        assert!(wire_distance(f, from, to, from) < 0.5);
        assert!(wire_distance(f, from, to, pos2(150.0, -60.0)) > 40.0);
        // Backwards wires don't panic.
        assert!(wire_distance(f, to, from, pos2(0.0, 0.0)).is_finite());
        assert!(wire_distance(f, from, from, pos2(1.0, 1.0)).is_finite());
        assert!(wire_midpoint(f, from, to).x > 100.0);
    }
}
