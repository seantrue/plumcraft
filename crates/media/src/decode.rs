//! Decoding values to plain pictures and numbers. Every value is hostile input (spec 18): sizes
//! are checked before a byte is decoded, and nothing panics on what an engine sends. Moved from
//! `plumcraft-ui`'s `media`, `run_view` and `watched` (stage N1, slice 6), unchanged in what they
//! make.
//!
//! - [`rendition`]: an engine's `rgba8` rendition, an (h, w, 4) `|u1` array, as a picture.
//! - [`decode_picture`]: a picture file (PNG here; JPEG, TIFF, WebP and GIF with the `images`
//!   feature), shrunk to a side.
//! - [`decode_numbers`]: a picture file's own numbers, its bit depth kept (`images`).
//! - [`numbers`]: a 1-d array's values.

use plumcraft_store::Blob;

use crate::raster::{Rgba, shrink};

/// The longest texture side made for a picture.
pub const MAX_TEXTURE_SIDE: u32 = 4096;
/// The largest PNG decoded: 16,384 px a side, 256 Mpx (spec 18).
pub const MAX_DECODE_SIDE: u32 = 16_384;
pub const MAX_DECODE_PIXELS: u64 = 256 << 20;
/// The largest rgba8 rendition read (pixels on its long side).
pub const MAX_THUMB_SIDE: u64 = 4096;

/// An (h, w, 4) `|u1` blob (an engine's `rgba8` rendition) as a picture; anything else, or too
/// large, is an error saying what it is.
pub fn rendition(b: &Blob) -> Result<Rgba, String> {
    let (w, h) = rendition_size(b)?;
    Ok(Rgba { w, h, pixels: b.bytes.to_vec() })
}

/// An rgba8 rendition's width and height, after every check [`rendition`] makes.
pub fn rendition_size(b: &Blob) -> Result<(u32, u32), String> {
    let [h, w, 4] = b.shape.as_slice() else { return Err(format!("shape {:?}, not (h, w, 4)", b.shape)) };
    if b.dtype != "|u1" {
        return Err(format!("dtype {}, not |u1", b.dtype));
    }
    if *h == 0 || *w == 0 || *h > MAX_THUMB_SIDE || *w > MAX_THUMB_SIDE {
        return Err(format!("{w}×{h}"));
    }
    let (Ok(hu), Ok(wu)) = (usize::try_from(*h), usize::try_from(*w)) else { return Err(format!("{w}×{h}")) };
    if b.bytes.len() != hu.saturating_mul(wu).saturating_mul(4) {
        return Err(format!("{} bytes for {w}×{h}", b.bytes.len()));
    }
    Ok((*w as u32, *h as u32))
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
    #[cfg(feature = "images")]
    {
        png_size(bytes).or_else(|| image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok())
    }
    #[cfg(not(feature = "images"))]
    {
        png_size(bytes)
    }
}

/// Decode a picture file (bounded) to RGBA8, shrunk to at most `max_side`: PNG here, the other
/// formats with the `image` crate, which reads them in RGB order whatever made them.
pub fn decode_picture(bytes: &[u8], max_side: u32) -> Result<Rgba, String> {
    if png_size(bytes).is_some() {
        return decode_png(bytes, max_side);
    }
    #[cfg(feature = "images")]
    {
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
    #[cfg(not(feature = "images"))]
    {
        Err("not a picture this editor reads (only PNG without the `images` feature)".into())
    }
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

/// An RGBA8 picture (`w × h`, row by row) as a PNG file's bytes: 8 bits a channel, alpha kept.
pub fn encode_png_rgba(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let n = (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4)).ok_or("too large")?;
    if w == 0 || h == 0 || rgba.len() != n || u64::from(w) * u64::from(h) > MAX_DECODE_PIXELS {
        return Err(format!("{} bytes aren't a {w}×{h} RGBA picture", rgba.len()));
    }
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// A picture file's own numbers, as an array: its bit depth kept, nothing shrunk or stripped, so
/// the probe and |A−B| read what the file holds. Bounded as [`decode_picture`] is.
pub fn decode_numbers(bytes: &[u8]) -> Result<Blob, String> {
    #[cfg(feature = "images")]
    {
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
    #[cfg(not(feature = "images"))]
    {
        let _ = bytes;
        Err("a picture's numbers need the `images` feature".into())
    }
}

/// The names of a picture file's channels.
pub fn image_channel_names(channels: usize) -> Vec<String> {
    let names: &[&str] = match channels {
        2 => &["L", "A"],
        3 => &["R", "G", "B"],
        4 => &["R", "G", "B", "A"],
        _ => &[],
    };
    names.iter().map(|n| (*n).to_owned()).collect()
}

/// A 1-d array's values as numbers, for the dtypes the text engine makes and a few more.
pub fn numbers(b: &Blob) -> Option<Vec<f64>> {
    if b.shape.len() != 1 {
        return None;
    }
    let v: Vec<f64> = match b.dtype.as_str() {
        "|u1" => b.bytes.iter().map(|x| f64::from(*x)).collect(),
        "<u4" => b.bytes.as_chunks::<4>().0.iter().map(|c| f64::from(u32::from_le_bytes(*c))).collect(),
        "<i4" => b.bytes.as_chunks::<4>().0.iter().map(|c| f64::from(i32::from_le_bytes(*c))).collect(),
        "<f4" => b.bytes.as_chunks::<4>().0.iter().map(|c| f64::from(f32::from_le_bytes(*c))).collect(),
        "<f8" => b.bytes.as_chunks::<8>().0.iter().map(|c| f64::from_le_bytes(*c)).collect(),
        _ => return None,
    };
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_rgba_picture_round_trips_through_png() {
        let px: Vec<u8> = (0..6 * 4 * 4).map(|i| (i * 37 % 256) as u8).collect();
        let png = encode_png_rgba(6, 4, &px).unwrap();
        let back = decode_png(&png, u32::MAX).unwrap();
        assert_eq!((back.w, back.h, back.pixels), (6, 4, px));
        assert!(encode_png_rgba(6, 4, &[0; 10]).is_err());
        assert!(encode_png_rgba(0, 4, &[]).is_err());
    }

    fn png_of(w: u32, h: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(color);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(data).unwrap();
        out
    }

    #[test]
    fn renditions_are_checked() {
        let ok = Blob { dtype: "|u1".into(), shape: vec![2, 3, 4], bytes: (vec![255; 24]).into() };
        let r = rendition(&ok).unwrap();
        assert_eq!((r.w, r.h, r.pixels.len()), (3, 2, 24));
        for bad in [
            Blob { dtype: "<f4".into(), shape: vec![2, 3, 4], bytes: (vec![0; 96]).into() },
            Blob { dtype: "|u1".into(), shape: vec![2, 3, 3], bytes: (vec![0; 18]).into() },
            Blob { dtype: "|u1".into(), shape: vec![2, 3, 4], bytes: (vec![0; 23]).into() },
            Blob { dtype: "|u1".into(), shape: vec![u64::MAX, 3, 4], bytes: (vec![]).into() },
            Blob { dtype: "|u1".into(), shape: vec![0, 3, 4], bytes: (vec![]).into() },
            Blob { dtype: "|u1".into(), shape: vec![4097, 1, 4], bytes: (vec![0; 4097 * 4]).into() },
            Blob { dtype: "|u1".into(), shape: vec![], bytes: (vec![]).into() },
        ] {
            assert!(rendition(&bad).is_err(), "{bad:?}");
        }
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
        let mut zero = png_of(1, 1, png::ColorType::Rgb, &[1, 2, 3]);
        zero[16..20].copy_from_slice(&0u32.to_be_bytes());
        assert!(decode_png(&zero, 10).is_err(), "a zero side");
        let mut torn = png_of(8, 8, png::ColorType::Rgb, &[7; 192]);
        torn.truncate(torn.len() - 20);
        assert!(decode_png(&torn, 10).is_err());
    }

    #[test]
    fn numbers_read_the_engines_dtypes() {
        let b = Blob { dtype: "<u4".into(), shape: vec![2], bytes: [65u32, 66].iter().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>().into() };
        assert_eq!(numbers(&b), Some(vec![65.0, 66.0]));
        assert_eq!(numbers(&Blob { dtype: "<c8".into(), shape: vec![1], bytes: (vec![0; 8]).into() }), None);
        assert_eq!(numbers(&Blob { dtype: "|u1".into(), shape: vec![1, 1], bytes: (vec![0]).into() }), None, "1-d only");
        let torn = Blob { dtype: "<f8".into(), shape: vec![3], bytes: (vec![0; 13]).into() };
        assert_eq!(numbers(&torn).map(|v| v.len()), Some(1), "whole elements only");
    }
}
