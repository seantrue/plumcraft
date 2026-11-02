//! Arrays as pictures (spec 10.2, 10.4; ported from the research software's `nodes/arrays.py` and
//! `display.py`): the views an array viewer offers, node thumbnails, and per-channel stats.
//!
//! An array is (H, W) one channel, (H, W, C) a stack of C, or an (N, k) table; any numeric dtype
//! is read as `f32` ([`values`]). Its colour space is optional: an array with none is plain
//! numbers, and only the `color` view (or `auto` choosing it for an unnamed 3-channel stack)
//! treats three channels as RGB.
//!
//! Views, as the research software's:
//! - `auto`: `labels` for an integer dtype, else `color` for 3 channels with a colour space or
//!   no channel names, else `clip` when every value is within 0..1, else `stretch`.
//! - `color`: one picture by channel count: 1 grey, 2 grey and alpha, 3 RGB (converted from its
//!   colour space when it has one, [`crate::color`]), 4 RGB and alpha; alpha over a checkerboard.
//! - `stretch` (each channel's finite min..max to 0..1), `clip` (0..1 as is), `heat` (turbo),
//!   `labels` (a colour per integer, negatives black), `angle` (hue = value / turn): one picture
//!   per channel, captioned with its name, at most [`MAX_VIEWS`].

use plumcraft_proto::display::{ChannelStats, Histogram, Stats};

use crate::Array;

/// The most pictures a view shows (spec 10.2).
pub const MAX_VIEWS: usize = 4;
/// Histogram bins per channel.
pub const BINS: u32 = 256;
/// The views, in the order a viewer offers them.
pub const VIEWS: &[&str] = plumcraft_proto::catalogue::core_types::ARRAY_VIEWS;

/// An array's layout: height, width, channels (a 1-D array is one row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub h: usize,
    pub w: usize,
    pub c: usize,
}

/// The layout of an array a picture can show: 2-D or 3-D, not a table, at most 16384 a side
/// and 64 channels. `None` for anything else.
pub fn layout(a: &Array) -> Option<Layout> {
    if a.table {
        return None;
    }
    let dim = |i: usize| a.shape.get(i).and_then(|n| usize::try_from(*n).ok());
    let l = match a.shape.len() {
        2 => Layout { h: dim(0)?, w: dim(1)?, c: 1 },
        3 => Layout { h: dim(0)?, w: dim(1)?, c: dim(2)? },
        _ => return None,
    };
    (l.h > 0 && l.w > 0 && l.c > 0 && l.h <= 16384 && l.w <= 16384 && l.c <= 64).then_some(l)
}

/// Whether an array's dtype is an integer (or bool) one: its values are labels.
pub fn is_integer(dtype: &str) -> bool {
    matches!(dtype.get(1..2), Some("u" | "i" | "b"))
}

/// Every element as `f32`, in C order. Little-endian (or byte) numeric dtypes only; `None` for
/// anything else, or when the bytes don't fill the shape.
pub fn values(a: &Array) -> Option<Vec<f32>> {
    let n = a.shape.iter().try_fold(1usize, |acc, d| acc.checked_mul(usize::try_from(*d).ok()?))?;
    let b: &[u8] = &a.bytes;
    let size = match a.dtype.as_str() {
        "|u1" | "|i1" | "|b1" => 1,
        "<u2" | "<i2" | "<f2" => 2,
        "<u4" | "<i4" | "<f4" => 4,
        "<u8" | "<i8" | "<f8" => 8,
        _ => return None,
    };
    if b.len() != n.checked_mul(size)? {
        return None;
    }
    let out: Vec<f32> = match a.dtype.as_str() {
        "|u1" | "|b1" => b.iter().map(|v| f32::from(*v)).collect(),
        "|i1" => b.iter().map(|v| f32::from(i8::from_le_bytes([*v]))).collect(),
        "<u2" => b.as_chunks::<2>().0.iter().map(|c| f32::from(u16::from_le_bytes(*c))).collect(),
        "<i2" => b.as_chunks::<2>().0.iter().map(|c| f32::from(i16::from_le_bytes(*c))).collect(),
        "<f2" => b.as_chunks::<2>().0.iter().map(|c| f16_to_f32(u16::from_le_bytes(*c))).collect(),
        "<u4" => b.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c) as f32).collect(),
        "<i4" => b.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c) as f32).collect(),
        "<f4" => b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect(),
        "<u8" => b.as_chunks::<8>().0.iter().map(|c| u64::from_le_bytes(*c) as f32).collect(),
        "<i8" => b.as_chunks::<8>().0.iter().map(|c| i64::from_le_bytes(*c) as f32).collect(),
        _ => b.as_chunks::<8>().0.iter().map(|c| f64::from_le_bytes(*c) as f32).collect(),
    };
    Some(out)
}

fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f32::from(h & 0x3ff);
    match exp {
        0 => sign * frac * 2f32.powi(-24),
        31 if frac == 0.0 => sign * f32::INFINITY,
        31 => f32::NAN,
        e => sign * (1.0 + frac / 1024.0) * 2f32.powi(e - 15),
    }
}

/// Channel `k` of an (H, W, C) buffer.
pub fn plane(v: &[f32], l: Layout, k: usize) -> Vec<f32> {
    if l.c == 1 {
        return v.to_vec();
    }
    v.iter().skip(k).step_by(l.c).copied().collect()
}

/// The name of channel `k`: its own, or `channel <k+1>`.
pub fn channel_name(a: &Array, k: usize) -> String {
    a.names.get(k).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("channel {}", k + 1))
}

/// A picture: a caption and (H, W, 3) RGB, 0..1.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub caption: String,
    pub h: usize,
    pub w: usize,
    pub rgb: Vec<f32>,
}

/// The view `auto` stands for, for this array.
pub fn resolve_view(a: &Array, v: &[f32], l: Layout, mode: &str) -> String {
    if mode != "auto" {
        return mode.to_owned();
    }
    if is_integer(&a.dtype) {
        return "labels".into();
    }
    if l.c == 3 && (a.space.is_some() || a.names.is_empty()) {
        return "color".into();
    }
    let finite = v.iter().filter(|x| x.is_finite());
    let in_unit = finite.clone().all(|x| (0.0..=1.0).contains(x));
    if in_unit { "clip".into() } else { "stretch".into() }
}

/// The pictures `mode` shows for `a` (`turn` for `angle`); empty when it isn't a picture.
pub fn views(a: &Array, mode: &str, turn: f64) -> Vec<Picture> {
    let (Some(l), Some(v)) = (layout(a), values(a)) else { return Vec::new() };
    let mode = resolve_view(a, &v, l, mode);
    let n = l.h * l.w;
    if mode == "color" && (1..=4).contains(&l.c) {
        return vec![color_picture(a, &v, l)];
    }
    let turn = if turn.is_finite() && turn != 0.0 { turn as f32 } else { 1.0 };
    (0..l.c.min(MAX_VIEWS))
        .map(|k| {
            let p = plane(&v, l, k);
            let rgb = match mode.as_str() {
                "labels" => label_colours(&p),
                "heat" => stretched(&p).into_iter().flat_map(turbo).collect(),
                "angle" => p.iter().flat_map(|x| hue_rgb((x / turn).rem_euclid(1.0))).collect(),
                "clip" => p.iter().flat_map(|x| [clip01(*x); 3]).collect(),
                _ => stretched(&p).into_iter().flat_map(|x| [x; 3]).collect(),
            };
            debug_assert_eq!(rgb.len(), n * 3);
            Picture { caption: channel_name(a, k), h: l.h, w: l.w, rgb }
        })
        .collect()
}

fn clip01(x: f32) -> f32 {
    if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) }
}

/// Each finite value mapped from the plane's finite min..max to 0..1; NaN reads as the min.
pub fn stretched(p: &[f32]) -> Vec<f32> {
    let (lo, hi) = p.iter().filter(|x| x.is_finite()).fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
    if lo > hi {
        return vec![0.0; p.len()];
    }
    let span = if hi > lo { hi - lo } else { 1.0 };
    p.iter().map(|x| if x.is_nan() { 0.0 } else { ((x - lo) / span).clamp(0.0, 1.0) }).collect()
}

/// One picture by channel count (the research software's `image_view`): grey, grey + alpha, RGB, RGBA.
fn color_picture(a: &Array, v: &[f32], l: Layout) -> Picture {
    let planes: Vec<Vec<f32>> = (0..l.c).map(|k| plane(v, l, k)).collect();
    let n = l.h * l.w;
    let grey = |i: usize| planes.first().and_then(|p| p.get(i)).copied().unwrap_or(0.0);
    let mut rgb: Vec<f32> = match l.c {
        3 | 4 => {
            let three: Vec<Vec<f32>> = planes.iter().take(3).cloned().collect();
            let rgb = match a.space.as_deref() {
                Some(space) if l.c == 3 => crate::color::to_rgb(space, &three).unwrap_or(three),
                _ => three,
            };
            (0..n).flat_map(|i| (0..3).map(move |k| (k, i))).map(|(k, i)| clip01(rgb.get(k).and_then(|p| p.get(i)).copied().unwrap_or(0.0))).collect()
        }
        _ => (0..n).flat_map(|i| [clip01(grey(i)); 3]).collect(),
    };
    if l.c == 2 || l.c == 4 {
        let size = (l.h.max(l.w) / 64).max(8);
        if let Some(alpha) = planes.last() {
            for i in 0..n {
                let (y, x) = (i / l.w, i % l.w);
                let check = if (y / size + x / size).is_multiple_of(2) { 0.8 } else { 0.6 };
                let al = clip01(alpha.get(i).copied().unwrap_or(1.0));
                for k in 0..3 {
                    if let Some(c) = rgb.get_mut(i * 3 + k) {
                        *c = *c * al + check * (1.0 - al);
                    }
                }
            }
        }
    }
    let caption = a.space.clone().unwrap_or_else(|| ["grey", "grey + alpha", "RGB", "RGBA"].get(l.c - 1).copied().unwrap_or("").to_owned());
    rgb.truncate(n * 3);
    Picture { caption, h: l.h, w: l.w, rgb }
}

/// A colour per integer label (a fixed pseudo-random palette, the same every time), negative
/// labels black.
fn label_colours(p: &[f32]) -> Vec<f32> {
    p.iter()
        .flat_map(|x| {
            if !x.is_finite() || *x < 0.0 {
                return [0.0; 3];
            }
            // splitmix64 of the label: three bytes of colour
            let mut z = (*x as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^= z >> 31;
            [(z & 0xff) as f32 / 255.0, ((z >> 8) & 0xff) as f32 / 255.0, ((z >> 16) & 0xff) as f32 / 255.0]
        })
        .collect()
}

/// Google's Turbo colour map (Mikhail, 2019), its polynomial approximation; 0..1 in.
fn turbo(x: f32) -> [f32; 3] {
    let x = f64::from(x.clamp(0.0, 1.0));
    let poly = |c: [f64; 6]| (c[0] + x * (c[1] + x * (c[2] + x * (c[3] + x * (c[4] + x * c[5]))))).clamp(0.0, 1.0) as f32;
    [
        poly([0.135_721_38, 4.615_392_6, -42.660_322_6, 132.131_082_9, -152.942_393_9, 59.286_379_77]),
        poly([0.091_402_61, 2.194_188_39, 4.842_966_58, -14.185_033_77, 4.277_298_55, 2.829_566_04]),
        poly([0.106_673_3, 12.641_946_08, -60.582_048_36, 110.362_767_71, -89.903_109_13, 27.348_249_81]),
    ]
}

/// A fully saturated hue (0..1 of a turn) as RGB.
fn hue_rgb(h: f32) -> [f32; 3] {
    let f = |n: f32| {
        let k = (n + h * 6.0).rem_euclid(6.0);
        (1.0 - (k.min(4.0 - k).clamp(0.0, 1.0))).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)]
}

/// A picture as an (h, w, 4) `|u1` array, scaled down (area average) to at most `max_px` a side.
pub fn rgba8(p: &Picture, max_px: u32) -> Array {
    let max_px = (max_px.max(1)) as usize;
    let scale = (p.h.max(p.w) as f64 / max_px as f64).max(1.0);
    let (th, tw) = (((p.h as f64 / scale).round() as usize).max(1), ((p.w as f64 / scale).round() as usize).max(1));
    let mut out = Vec::with_capacity(th * tw * 4);
    for ty in 0..th {
        let (y0, y1) = (ty * p.h / th, ((ty + 1) * p.h / th).max(ty * p.h / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * p.w / tw, ((tx + 1) * p.w / tw).max(tx * p.w / tw + 1));
            let mut acc = [0f32; 3];
            let mut n = 0f32;
            for y in y0..y1.min(p.h) {
                for x in x0..x1.min(p.w) {
                    for (k, a) in acc.iter_mut().enumerate() {
                        *a += p.rgb.get((y * p.w + x) * 3 + k).copied().unwrap_or(0.0);
                    }
                    n += 1.0;
                }
            }
            for a in acc {
                out.push((a / n.max(1.0) * 255.0 + 0.5).clamp(0.0, 255.0) as u8);
            }
            out.push(255);
        }
    }
    Array::new("|u1", vec![th as u64, tw as u64, 4], out)
}

/// A node thumbnail for an array: its first `auto` picture, at most `px` a side.
pub fn thumb(a: &Array, px: u32) -> Option<Array> {
    views(a, "auto", 1.0).first().map(|p| rgba8(p, px))
}

/// Per-channel stats: exact min and max over finite values, the NaN count, and a [`BINS`]-bin
/// histogram over each channel's own finite min..max (a table: per column). `None` when the
/// array isn't numeric.
pub fn stats(a: &Array) -> Option<Stats> {
    let v = values(a)?;
    let (rows, c) = match (a.table, a.shape.as_slice()) {
        (true, [n, k]) | (false, [n, _, k]) if *k > 0 => (usize::try_from(*n).ok()?, usize::try_from(*k).ok()?),
        _ => (v.len(), 1),
    };
    let c = if layout(a).is_some() || a.table { c } else { 1 };
    let _ = rows;
    let channels = (0..c.min(64))
        .map(|k| {
            let p: Vec<f32> = if c == 1 { v.clone() } else { v.iter().skip(k).step_by(c).copied().collect() };
            let nan = p.iter().filter(|x| x.is_nan()).count() as u64;
            let finite: Vec<f64> = p.iter().filter(|x| x.is_finite()).map(|x| f64::from(*x)).collect();
            let (lo, hi) = finite.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
            let histogram = (!finite.is_empty()).then(|| {
                let top = if hi > lo { hi } else { lo + 1.0 };
                let mut counts = vec![0u64; BINS as usize];
                for x in &finite {
                    let i = (((x - lo) / (top - lo)) * f64::from(BINS)) as usize;
                    if let Some(n) = counts.get_mut(i.min(BINS as usize - 1)) {
                        *n += 1;
                    }
                }
                Histogram { bins: BINS, lo: Some(lo), hi: Some(top), counts }
            });
            ChannelStats { name: channel_name(a, k), min: (!finite.is_empty()).then_some(lo), max: (!finite.is_empty()).then_some(hi), nan, histogram }
        })
        .collect();
    Some(Stats { channels, ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32s(shape: Vec<u64>, v: &[f32]) -> Array {
        Array::from_f32(shape, v)
    }

    #[test]
    fn values_read_every_numeric_dtype_and_refuse_the_rest() {
        assert_eq!(values(&Array::new("|u1", vec![3], vec![0u8, 128, 255])), Some(vec![0.0, 128.0, 255.0]));
        assert_eq!(values(&Array::new("<i2", vec![2], [(-2i16).to_le_bytes(), 7i16.to_le_bytes()].concat())), Some(vec![-2.0, 7.0]));
        assert_eq!(values(&Array::new("<f8", vec![1], 0.5f64.to_le_bytes().to_vec())), Some(vec![0.5]));
        assert_eq!(values(&Array::new("<f2", vec![1], 0x3c00u16.to_le_bytes().to_vec())), Some(vec![1.0]));
        assert!(values(&Array::new(">f4", vec![1], vec![0u8; 4])).is_none(), "big-endian");
        assert!(values(&Array::new("<f4", vec![2], vec![0u8; 4])).is_none(), "short");
        assert!(values(&Array::new("<f4", vec![u64::MAX, 2], vec![])).is_none(), "overflow");
    }

    #[test]
    fn auto_picks_as_the_research_software_does() {
        let l = |a: &Array| layout(a).unwrap();
        let grey = f32s(vec![2, 2], &[0.0, 0.5, 1.0, 0.25]);
        assert_eq!(resolve_view(&grey, &values(&grey).unwrap(), l(&grey), "auto"), "clip");
        let big = f32s(vec![1, 2], &[-3.0, 9.0]);
        assert_eq!(resolve_view(&big, &values(&big).unwrap(), l(&big), "auto"), "stretch");
        let labels = Array::new("<i4", vec![1, 2], [1i32.to_le_bytes(), 2i32.to_le_bytes()].concat());
        assert_eq!(resolve_view(&labels, &values(&labels).unwrap(), l(&labels), "auto"), "labels");
        let rgb = f32s(vec![1, 1, 3], &[1.0, 0.0, 0.0]);
        assert_eq!(resolve_view(&rgb, &values(&rgb).unwrap(), l(&rgb), "auto"), "color", "three unnamed channels");
        let named = rgb.clone().with_space(None, vec!["a".into(), "b".into(), "c".into()]);
        assert_eq!(resolve_view(&named, &values(&named).unwrap(), l(&named), "auto"), "clip", "named, no space: numbers");
    }

    #[test]
    fn views_are_one_picture_per_channel_or_one_in_colour() {
        let stack = f32s(vec![2, 3, 5], &(0..30).map(|x| x as f32).collect::<Vec<_>>());
        let v = views(&stack, "stretch", 1.0);
        assert_eq!(v.len(), MAX_VIEWS);
        assert_eq!((v[0].caption.as_str(), v[0].h, v[0].w, v[0].rgb.len()), ("channel 1", 2, 3, 18));
        assert_eq!((v[0].rgb[0], v[0].rgb[15]), (0.0, 1.0), "each channel over its own range");
        let rgb = f32s(vec![1, 2, 3], &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let c = views(&rgb, "color", 1.0);
        assert_eq!((c.len(), c[0].caption.as_str(), &c[0].rgb[..3]), (1, "RGB", &[1.0, 0.0, 0.0][..]));
        for mode in VIEWS {
            for a in [&stack, &rgb] {
                assert!(views(a, mode, 360.0).iter().all(|p| p.rgb.len() == p.h * p.w * 3 && p.rgb.iter().all(|x| (0.0..=1.0).contains(x))), "{mode}");
            }
        }
        assert!(views(&f32s(vec![5], &[1.0; 5]), "auto", 1.0).is_empty(), "1-D: a plot, not a picture");
        let nan = f32s(vec![1, 3], &[f32::NAN, f32::INFINITY, 2.0]);
        assert!(views(&nan, "stretch", 1.0)[0].rgb.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn thumbs_fit_and_stats_count() {
        let a = f32s(vec![400, 200], &vec![0.5; 80_000]);
        let t = thumb(&a, 160).unwrap();
        assert_eq!((t.dtype.as_str(), t.shape.as_slice()), ("|u1", &[160u64, 80, 4][..]));
        assert!(t.bytes.chunks(4).all(|p| p == [128, 128, 128, 255]));
        let s = stats(&f32s(vec![1, 4], &[1.0, f32::NAN, 3.0, 2.0])).unwrap();
        let c = &s.channels[0];
        assert_eq!((c.min, c.max, c.nan), (Some(1.0), Some(3.0), 1));
        let h = c.histogram.as_ref().unwrap();
        assert_eq!((h.counts.iter().sum::<u64>(), h.counts[0], h.counts[255]), (3, 1, 1));
        let s = stats(&f32s(vec![2, 1, 2], &[0.0, 5.0, 1.0, 6.0])).unwrap();
        assert_eq!(s.channels.iter().map(|c| c.max).collect::<Vec<_>>(), vec![Some(1.0), Some(6.0)]);
    }
}
