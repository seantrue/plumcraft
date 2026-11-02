//! Thumbnails: a PNG the craft exported, decoded with bounds and shrunk to fit a square, as the
//! (h, w, 4) `|u1` array the canvas draws (spec 10.3 `rgba8`).

use plumcraft_engine::Array;

/// The largest PNG side decoded (spec 18: 16,384 px a side, 256 Mpx).
const MAX_SIDE: u32 = 16_384;
const MAX_PIXELS: u64 = 256 << 20;

/// A PNG's width and height from its header.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let h = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    Some((w, h))
}

/// Decode a PNG to RGBA8 and shrink it (box filter) to fit `px` × `px`.
pub fn rgba8(bytes: &[u8], px: u32) -> Option<Array> {
    let (w0, h0) = png_size(bytes)?;
    if w0 == 0 || h0 == 0 || w0 > MAX_SIDE || h0 > MAX_SIDE || u64::from(w0) * u64::from(h0) > MAX_PIXELS || px == 0 {
        return None;
    }
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let src = buf.get(..info.buffer_size())?;
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None,
    };
    let pixel = |x: usize, y: usize| -> [u8; 4] {
        let i = (y * w + x) * channels;
        let g = |k: usize| src.get(i + k).copied().unwrap_or(0);
        match channels {
            4 => [g(0), g(1), g(2), g(3)],
            3 => [g(0), g(1), g(2), 255],
            2 => [g(0), g(0), g(0), g(1)],
            _ => [g(0), g(0), g(0), 255],
        }
    };
    let scale = (f64::from(px) / w.max(h) as f64).min(1.0);
    let (tw, th) = (((w as f64 * scale).round() as usize).max(1), ((h as f64 * scale).round() as usize).max(1));
    let mut out = Vec::with_capacity(tw * th * 4);
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).max(ty * h / th + 1).min(h));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).max(tx * w / tw + 1).min(w));
            let mut acc = [0u64; 4];
            let mut n = 0u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = pixel(x, y);
                    for (a, v) in acc.iter_mut().zip(p) {
                        *a += u64::from(v);
                    }
                    n += 1;
                }
            }
            out.extend(acc.map(|a| u8::try_from(a / n.max(1)).unwrap_or(255)));
        }
    }
    Some(Array::new("|u1", vec![th as u64, tw as u64, 4], out))
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

    #[test]
    fn decodes_and_shrinks() {
        let data: Vec<u8> = (0..400 * 200).flat_map(|i| [(i % 256) as u8, 0, 0]).collect();
        let p = png_of(400, 200, png::ColorType::Rgb, &data);
        assert_eq!(png_size(&p), Some((400, 200)));
        let t = rgba8(&p, 100).unwrap();
        assert_eq!(t.shape, [50, 100, 4]);
        assert_eq!(t.bytes.len(), 50 * 100 * 4);
        assert!(t.bytes.chunks(4).all(|c| c[3] == 255));
        let small = rgba8(&png_of(2, 1, png::ColorType::Grayscale, &[0, 255]), 100).unwrap();
        assert_eq!(small.shape, [1, 2, 4], "never enlarged");
    }

    #[test]
    fn hostile_pngs_are_none() {
        assert!(rgba8(b"", 10).is_none());
        assert!(rgba8(b"\x89PNG\r\n\x1a\n", 10).is_none());
        let mut huge = png_of(1, 1, png::ColorType::Rgb, &[1, 2, 3]);
        huge[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        assert!(rgba8(&huge, 10).is_none());
        let mut torn = png_of(8, 8, png::ColorType::Rgb, &[7; 192]);
        torn.truncate(torn.len() - 20);
        assert!(rgba8(&torn, 4).is_none());
    }
}
