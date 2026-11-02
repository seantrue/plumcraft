//! SVG, a core type (`plumcraft_proto::catalogue::core_types::SVG`): an SVG document, UTF-8,
//! crossing as an `md5:raw:` blob of media `image/svg+xml`. This crate reads and draws one, the
//! same way for the editor's SVG view and for the engine SDK's thumbnails:
//!
//! - [`parse`]: usvg, bounded by [`MAX_SVG_BYTES`]. External files are never read (only `data:`
//!   images). Text is shaped with the system's fonts, loaded once ([`fonts`]), so labels that
//!   aren't outlined are drawn.
//! - [`render`]: resvg (tiny-skia) to unpremultiplied RGBA at a given size, bounded by
//!   [`MAX_PIXELS`].
//! - [`thumb`]: both, fitted into a square.
//!
//! Every SVG is hostile input: a bad one is an `Err`, never a panic.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::{Arc, OnceLock};

pub use resvg;
use resvg::usvg;

/// The largest SVG read.
pub const MAX_SVG_BYTES: usize = 64 << 20;
/// The most pixels a raster has (16 Mpx).
pub const MAX_PIXELS: u64 = 16 << 20;
/// The SVG wire type's media type.
pub const MEDIA: &str = "image/svg+xml";

/// Unpremultiplied RGBA8, `w × h`.
#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    pub w: u32,
    pub h: u32,
    pub pixels: Vec<u8>,
}

impl Raster {
    /// Flattened onto white paper. A drawing is mostly transparent, and dark lines with nothing
    /// behind them can't be seen on a dark node.
    pub fn on_paper(mut self) -> Raster {
        for p in self.pixels.as_chunks_mut::<4>().0 {
            let a = u16::from(p[3]);
            for c in &mut p[..3] {
                *c = ((u16::from(*c) * a + 255 * (255 - a)) / 255) as u8;
            }
            p[3] = 255;
        }
        self
    }
}

/// The system's fonts, loaded on first use (a few hundred milliseconds; call it off the UI
/// thread). Generic families keep fontdb's defaults (Times New Roman, Arial, Courier New).
pub fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    Arc::clone(FONTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    }))
}

/// The usvg options for hostile SVGs: no external files (only `data:` images), the system's
/// fonts.
fn options() -> usvg::Options<'static> {
    let mut opt = usvg::Options { resources_dir: None, fontdb: fonts(), ..Default::default() };
    opt.image_href_resolver.resolve_string = Box::new(|_, _| None);
    opt
}

/// Parse an SVG (bounded).
pub fn parse(bytes: &[u8]) -> Result<usvg::Tree, String> {
    if bytes.len() > MAX_SVG_BYTES {
        return Err(format!("{} MB is too large to draw", bytes.len() >> 20));
    }
    usvg::Tree::from_data(bytes, &options()).map_err(|e| e.to_string())
}

/// Whether `bytes` look like an SVG document (an `<svg` element near the start, after an XML
/// declaration, comments or a doctype): cheap, before parsing.
pub fn sniff(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    String::from_utf8_lossy(head).contains("<svg")
}

/// Rasterise `tree` to `w × h` (each at least 1), stretched to fill.
pub fn render(tree: &usvg::Tree, w: u32, h: u32) -> Result<Raster, String> {
    let (w, h) = (w.max(1), h.max(1));
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!("{w}×{h} is too large to draw"));
    }
    let size = tree.size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("no pixmap")?;
    let t = resvg::tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(tree, t, &mut pixmap.as_mut());
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Ok(Raster { w: pixmap.width(), h: pixmap.height(), pixels })
}

/// The pixel size of a picture of `size` whose long side is `long`, keeping its shape.
pub fn fit(size: (f32, f32), long: u32) -> (u32, u32) {
    let (w, h) = size;
    if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
        return (1, 1);
    }
    let long = long.max(1) as f32;
    let m = w.max(h);
    (((w / m) * long).round().max(1.0) as u32, ((h / m) * long).round().max(1.0) as u32)
}

/// An SVG drawn to fit `px` × `px`.
pub fn thumb(bytes: &[u8], px: u32) -> Result<Raster, String> {
    let tree = parse(bytes)?;
    let (w, h) = fit((tree.size().width(), tree.size().height()), px);
    render(&tree, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
        <rect x="0" y="0" width="100" height="100" fill="#ff0000"/>
        <rect x="100" y="0" width="100" height="100" fill="#0000ff" fill-opacity="0.5"/>
    </svg>"##;

    fn px(r: &Raster, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * r.w + x) * 4) as usize;
        [r.pixels[i], r.pixels[i + 1], r.pixels[i + 2], r.pixels[i + 3]]
    }

    #[test]
    fn on_paper_is_opaque_and_keeps_what_was_drawn() {
        let r = Raster { w: 3, h: 1, pixels: vec![0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 128] }.on_paper();
        assert_eq!(&r.pixels[..8], [0, 0, 0, 255, 255, 255, 255, 255], "ink stays ink, nothing is paper");
        assert!(r.pixels[8] > 120 && r.pixels[8] < 135 && r.pixels[11] == 255, "half ink is half grey: {:?}", &r.pixels[8..]);
    }

    #[test]
    fn draws_at_the_size_asked_unpremultiplied() {
        let r = render(&parse(SQUARES.as_bytes()).unwrap(), 400, 200).unwrap();
        assert_eq!((r.w, r.h, r.pixels.len()), (400, 200, 400 * 200 * 4));
        assert_eq!(px(&r, 50, 100), [255, 0, 0, 255]);
        let blue = px(&r, 350, 100);
        assert_eq!((blue[2], blue[3]), (255, 128), "half-transparent blue stays full blue: {blue:?}");
    }

    #[test]
    fn a_thumb_keeps_the_shape() {
        let r = thumb(SQUARES.as_bytes(), 160).unwrap();
        assert_eq!((r.w, r.h), (160, 80));
        assert_eq!(fit((0.0, 5.0), 10), (1, 1));
    }

    #[test]
    fn text_is_drawn_with_the_systems_fonts() {
        // a label that isn't outlined: some pixels of the text's box are inked
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="60"><text x="10" y="45" font-family="sans-serif" font-size="40" fill="#000">Plum</text></svg>"##;
        let r = thumb(svg.as_bytes(), 200).unwrap();
        let inked = r.pixels.as_chunks::<4>().0.iter().filter(|p| p[3] > 128).count();
        if fonts().is_empty() {
            return; // a machine with no fonts at all draws no text, and that's all
        }
        assert!(inked > 200, "{inked} inked pixels");
    }

    #[test]
    fn hostile_svgs_are_errors_not_panics() {
        assert!(parse(b"not svg").is_err());
        assert!(parse(b"").is_err());
        assert!(parse(b"<svg xmlns='http://www.w3.org/2000/svg' width='-5' height='nan'/>").is_err());
        let tree = parse(SQUARES.as_bytes()).unwrap();
        assert!(render(&tree, 100_000, 100_000).is_err());
        assert!(sniff(SQUARES.as_bytes()) && !sniff(b"\x89PNG"));
    }
}
