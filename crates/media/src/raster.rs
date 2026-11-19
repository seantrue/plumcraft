//! A picture as plain bytes: what the media crate decodes and the core computes (a difference,
//! a probe's view), and a shell turns into its own texture (the egui shell:
//! `plumcraft_ui::media::RgbaImage`). Moved from `plumcraft-ui`'s `media` to the core (stage N1),
//! then here (slice 6); the core re-exports it at `plumcraft_editor_core::raster`.

/// A picture: RGBA8, unpremultiplied (straight alpha), sRGB, rows top to bottom, `w × h`.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    /// Its size in bytes (what a cache counts).
    pub fn bytes(&self) -> usize {
        self.pixels.len()
    }
}

/// Shrink to at most `max_side` a side (box filter); smaller pictures are returned as they are.
pub fn shrink(img: Rgba, max_side: u32) -> Rgba {
    let (w, h) = (img.w as usize, img.h as usize);
    // a picture whose size overflows, or whose pixels don't fill it, is left as it is
    let whole = w.checked_mul(h).and_then(|n| n.checked_mul(4));
    if img.w.max(img.h) <= max_side || max_side == 0 || whole.is_none_or(|n| img.pixels.len() < n) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinking_keeps_the_aspect_and_refuses_nothing() {
        let img = Rgba { w: 300, h: 100, pixels: vec![200; 300 * 100 * 4] };
        let s = shrink(img.clone(), 150);
        assert_eq!((s.w, s.h, s.pixels.len()), (150, 50, 150 * 50 * 4));
        assert_eq!(shrink(img.clone(), 0), img, "0 means as it is");
        let torn = Rgba { w: 300, h: 100, pixels: vec![1; 7] };
        assert_eq!(shrink(torn.clone(), 10), torn, "a torn picture is left alone, not read past its end");
    }
}
