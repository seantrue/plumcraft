//! Draw Text (the research software's, which drew with yapocis graphics4 and FreeType): text set in a font's
//! outlines (skrifa) and filled antialiased (tiny-skia), blended onto the canvas in a colour.
//!
//! Fonts: Inter (the UI's own OFL face, always there), every craft-fonts face when the build
//! embedded them (`CRAFT_FONTS_DIR`, see `build.rs`; BIZ UDPGothic and the Mincho faces for
//! Japanese, Noto for Chinese, Arabic, Thai and Georgian), or any font file by path. A character
//! the font lacks is skipped: there is no fallback between faces. Text is laid out left to right,
//! one line per `\n`, without shaping (no ligatures or kerning; Arabic is not joined).

use plumcraft_engine::{Cx, Inputs};
use plumcraft_proto::error::NodeError;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

use super::draw::{back_from_rgba, hex_rgb, rgba_canvas};
use crate::op::{config, f, img, n, one, s};

include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

/// The UI's own text fallback face (Inter, OFL: `assets/fonts/OFL-Inter.txt`, links to
/// `crates/ui`'s so a published crate carries them).
static INTER: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
/// The largest font file read by path.
const MAX_FONT_FILE: u64 = 64 << 20;
/// The most characters drawn.
const MAX_CHARS: usize = 100_000;

pub const H_ALIGN: &[&str] = &["none", "L", "C", "R"];
pub const V_ALIGN: &[&str] = &["none", "T", "C", "B"];

/// The font names Draw Text offers: Inter, then each embedded craft-fonts face ("family style").
pub fn font_names() -> Vec<String> {
    std::iter::once("Inter".to_owned()).chain(CRAFT_FONTS.iter().map(|(family, style, _, _)| format!("{family} {style}"))).collect()
}

fn font_bytes(name: &str) -> Option<&'static [u8]> {
    if name == "Inter" {
        return Some(INTER);
    }
    CRAFT_FONTS.iter().find(|(family, style, _, _)| format!("{family} {style}") == name).map(|(_, _, _, b)| *b)
}

/// Collects a glyph's outline into a tiny-skia path, flipping y (fonts go up, pictures down) and
/// moving it to `(x, y)`.
struct Pen<'a> {
    b: &'a mut tiny_skia::PathBuilder,
    x: f32,
    y: f32,
}

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.b.move_to(self.x + x, self.y - y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.b.line_to(self.x + x, self.y - y);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.b.quad_to(self.x + cx0, self.y - cy0, self.x + x, self.y - y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.b.cubic_to(self.x + cx0, self.y - cy0, self.x + cx1, self.y - cy1, self.x + x, self.y - y);
    }
    fn close(&mut self) {
        self.b.close();
    }
}

/// The text's outlines at `size` px, laid out from (0, 0) at the top left of its block, and the
/// block's width and height.
fn layout(font: &FontRef<'_>, text: &str, size: f32) -> (Option<tiny_skia::Path>, f32, f32) {
    let px = Size::new(size);
    let metrics = font.metrics(px, LocationRef::default());
    let line = (metrics.ascent - metrics.descent + metrics.leading).max(size * 0.5);
    let advances = font.glyph_metrics(px, LocationRef::default());
    let charmap = font.charmap();
    let glyphs = font.outline_glyphs();
    let mut b = tiny_skia::PathBuilder::new();
    let mut width = 0.0f32;
    let lines: Vec<&str> = text.split('\n').collect();
    for (row, text) in lines.iter().enumerate() {
        let mut x = 0.0f32;
        let baseline = metrics.ascent + row as f32 * line;
        for ch in text.chars() {
            let Some(gid) = charmap.map(ch) else { continue };
            if let Some(g) = glyphs.get(gid) {
                let _ = g.draw(DrawSettings::unhinted(px, LocationRef::default()), &mut Pen { b: &mut b, x, y: baseline });
            }
            x += advances.advance_width(gid).unwrap_or(0.0);
        }
        width = width.max(x);
    }
    (b.finish(), width, line * lines.len() as f32)
}

/// Where the block's top left goes (the research software's `text_location`): without an anchor x is the left
/// end and y the vertical centre of the block, negative counting from the right or bottom edge;
/// with one, x, y offset from that canvas edge or its centre, and the block's own edge or centre
/// is put there.
fn place((h, w): (f32, f32), (x, y): (f32, f32), (h_align, v_align): (&str, &str), (bh, bw): (f32, f32)) -> (f32, f32) {
    let left = match h_align {
        "L" => x,
        "C" => w / 2.0 + x - bw / 2.0,
        "R" => w + x - bw,
        _ if x < 0.0 => w + x,
        _ => x,
    };
    let top = match v_align {
        "T" => y,
        "C" => h / 2.0 + y - bh / 2.0,
        "B" => h + y - bh,
        _ if y < 0.0 => h + y - bh / 2.0,
        _ => y - bh / 2.0,
    };
    (left, top)
}

pub fn draw_text(i: &Inputs, cx: &mut Cx) -> Result<plumcraft_engine::Outputs, NodeError> {
    let canvas = img(i, "canvas")?;
    let mut rgba = rgba_canvas(&canvas, "canvas").map_err(config)?;
    let text = s(i, "text", "");
    if text.chars().count() > MAX_CHARS {
        return Err(config(format!("text: at most {MAX_CHARS} characters")));
    }
    let file = s(i, "font_file", "");
    let owned;
    let bytes: &[u8] = if file.trim().is_empty() {
        let name = s(i, "font", "Inter");
        font_bytes(&name).ok_or_else(|| config(format!("font: no font {name:?}; this build has {}", font_names().join(", "))))?
    } else {
        let p = std::path::Path::new(file.trim());
        let len = std::fs::metadata(p).map_err(|e| NodeError::new(NodeError::MISSING_SOURCE, format!("font_file: {}: {e}", p.display())))?.len();
        if len > MAX_FONT_FILE {
            return Err(config("font_file: larger than 64 MiB"));
        }
        owned = std::fs::read(p).map_err(|e| NodeError::new(NodeError::MISSING_SOURCE, format!("font_file: {}: {e}", p.display())))?;
        &owned
    };
    let font = FontRef::new(bytes).map_err(|e| config(format!("font: not a font file it can read ({e})")))?;
    let size = n(i, "size", 32).clamp(4, 1000) as f32;
    let (path, bw, bh) = layout(&font, &text, size);
    let colour = hex_rgb(&s(i, "color", "#ffffff")).map_err(config)?;
    let opacity = f(i, "opacity", 1.0).clamp(0.0, 1.0) as f32;
    let (h, w) = (rgba.h, rgba.w);
    let Some(path) = path else { return Ok(one("drawing", back_from_rgba(&rgba, &canvas))) };
    let h_align = s(i, "h_align", "none");
    let v_align = s(i, "v_align", "none");
    if !H_ALIGN.contains(&h_align.as_str()) || !V_ALIGN.contains(&v_align.as_str()) {
        return Err(config("h_align is none, L, C or R; v_align none, T, C or B"));
    }
    let x = n(i, "x", 20).clamp(-16384, 16384) as f32;
    let y = n(i, "y", 40).clamp(-16384, 16384) as f32;
    let (left, top) = place((h as f32, w as f32), (x, y), (&h_align, &v_align), (bh, bw));
    // Rotation (degrees, counter-clockwise as the research software's) about the block's centre.
    let degrees = f(i, "rotation", 0.0).clamp(-180.0, 180.0) as f32;
    let (ccx, ccy) = (left + bw / 2.0, top + bh / 2.0);
    let transform = tiny_skia::Transform::from_translate(left, top).post_concat(tiny_skia::Transform::from_rotate_at(-degrees, ccx, ccy));
    if cx.cancelled() {
        return Err(NodeError::cancelled());
    }
    let (Ok(mw), Ok(mh)) = (u32::try_from(w), u32::try_from(h)) else { return Err(config("canvas: too large")) };
    let mut mask = tiny_skia::Mask::new(mw, mh).ok_or_else(|| config("canvas: empty"))?;
    mask.fill_path(&path, tiny_skia::FillRule::Winding, true, transform);
    let c = [colour[0] as f32, colour[1] as f32, colour[2] as f32];
    for (k, cov) in mask.data().iter().enumerate() {
        let a = f32::from(*cov) / 255.0 * opacity;
        if a <= 0.0 {
            continue;
        }
        if let Some(px) = rgba.data.get_mut(k * 4..k * 4 + 3) {
            for (d, s) in px.iter_mut().zip(c) {
                *d = s * a + *d * (1.0 - a);
            }
        }
    }
    Ok(one("drawing", back_from_rgba(&rgba, &canvas)))
}
