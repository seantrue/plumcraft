//! SVG (`vector` media) as pictures: parsed with usvg and rasterised with resvg (tiny-skia),
//! pure Rust, bounded ([`plumcraft_svg`], shared with the engine SDK's SVG thumbnails). External
//! files are never read (only `data:` images); text that isn't outlined is drawn with the
//! system's fonts. Moved from `plumcraft-ui`'s `media` (stage N1, slice 6).

use crate::decode::MAX_TEXTURE_SIDE;
use crate::raster::Rgba;

/// The most pixels an SVG is rasterised to.
pub const MAX_RENDER_PIXELS: u64 = 16 << 20;
/// The largest SVG parsed.
pub const MAX_SVG_BYTES: usize = plumcraft_svg::MAX_SVG_BYTES;

/// A parsed SVG.
pub type Tree = plumcraft_svg::resvg::usvg::Tree;

/// Parse an SVG (bounded; [`plumcraft_svg::parse`]).
pub fn parse_svg(bytes: &[u8]) -> Result<Tree, String> {
    plumcraft_svg::parse(bytes)
}

/// An SVG's own size, `(width, height)`.
pub fn size(tree: &Tree) -> (f32, f32) {
    (tree.size().width(), tree.size().height())
}

/// The pixel size an SVG of `(w, h)` is rasterised to for `long` pixels on its long side, within
/// the bounds.
pub fn render_size((sw, sh): (f32, f32), long: f32) -> (u32, u32) {
    if !(sw > 0.0 && sh > 0.0 && sw.is_finite() && sh.is_finite()) {
        return (1, 1);
    }
    let mut long = long.clamp(16.0, MAX_TEXTURE_SIDE as f32);
    let px = |l: f32| (((sw / sw.max(sh)) * l).round().max(1.0) as u32, ((sh / sw.max(sh)) * l).round().max(1.0) as u32);
    let (mut w, mut h) = px(long);
    while u64::from(w) * u64::from(h) > MAX_RENDER_PIXELS && long > 16.0 {
        long *= 0.9;
        (w, h) = px(long);
    }
    (w, h)
}

/// Rasterise `tree` to `w × h`, on white as a browser shows it ([`plumcraft_svg::render`]): a
/// drawing's dark lines can't be seen on the dark checkerboard.
pub fn render_svg(tree: &Tree, w: u32, h: u32) -> Result<Rgba, String> {
    plumcraft_svg::render(tree, w, h).map(plumcraft_svg::Raster::on_paper).map(|r| Rgba { w: r.w, h: r.h, pixels: r.pixels })
}

/// An SVG's bytes parsed and drawn with `long` pixels on its long side.
pub fn rasterise(bytes: &[u8], long: f32) -> Result<Rgba, String> {
    let tree = parse_svg(bytes)?;
    let (w, h) = render_size(size(&tree), long);
    render_svg(&tree, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
        <rect x="0" y="0" width="100" height="100" fill="#ff0000"/><rect x="100" y="0" width="100" height="100" fill="#0000ff"/></svg>"##;

    #[test]
    fn svgs_render_at_the_size_asked_within_bounds() {
        let img = rasterise(SVG.as_bytes(), 800.0).unwrap();
        assert_eq!((img.w, img.h), (800, 400));
        let px = |x: u32, y: u32| img.pixels[((y * img.w + x) * 4) as usize..][..4].to_vec();
        assert_eq!(px(100, 200), [255, 0, 0, 255]);
        assert_eq!(px(700, 200), [0, 0, 255, 255]);
        let (w, h) = render_size((1.0, 1.0), 1e9);
        assert!(w <= MAX_TEXTURE_SIDE && u64::from(w) * u64::from(h) <= MAX_RENDER_PIXELS, "{w}×{h}");
        assert_eq!(render_size((0.0, f32::NAN), 100.0), (1, 1));
        for junk in [&b""[..], b"<svg", b"\x89PNG", b"<svg xmlns='http://www.w3.org/2000/svg' width='-5' height='x'/>"] {
            let _ = rasterise(junk, 64.0);
        }
        assert!(parse_svg(&vec![b' '; MAX_SVG_BYTES + 1]).unwrap_err().contains("too large"));
    }
}
