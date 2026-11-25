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

/// One channel of a picture as grey, or its colour without alpha: what a picture viewer shows
/// for `viewer.picture {channel}` (`"rgba"`, `"rgb"`, `"r"`, `"g"`, `"b"`, `"a"`). `rgba`, a
/// channel it doesn't know, or pixels that don't fill the picture give it back as it is.
pub fn isolate(img: &Rgba, channel: &str) -> Rgba {
    let k = match channel {
        "rgb" => 4,
        "r" => 0,
        "g" => 1,
        "b" => 2,
        "a" => 3,
        _ => return img.clone(),
    };
    let n = (img.w as usize).checked_mul(img.h as usize).and_then(|n| n.checked_mul(4));
    if n.is_none_or(|n| img.pixels.len() < n) {
        return img.clone();
    }
    let mut pixels = img.pixels.clone();
    for px in pixels.as_chunks_mut::<4>().0 {
        if k == 4 {
            px[3] = 255;
        } else {
            let v = px[k];
            *px = [v, v, v, 255];
        }
    }
    Rgba { w: img.w, h: img.h, pixels }
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
    fn a_channel_shows_as_grey_and_rgb_drops_alpha() {
        let img = Rgba { w: 2, h: 1, pixels: vec![10, 20, 30, 40, 50, 60, 70, 80] };
        assert_eq!(isolate(&img, "rgba"), img);
        assert_eq!(isolate(&img, "rgb").pixels, [10, 20, 30, 255, 50, 60, 70, 255]);
        assert_eq!(isolate(&img, "g").pixels, [20, 20, 20, 255, 60, 60, 60, 255]);
        assert_eq!(isolate(&img, "a").pixels, [40, 40, 40, 255, 80, 80, 80, 255]);
        // short pixels, an unknown channel: as it is
        let short = Rgba { w: 4, h: 4, pixels: vec![1, 2, 3, 4] };
        assert_eq!(isolate(&short, "r"), short);
        assert_eq!(isolate(&img, "z"), img);
    }

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
