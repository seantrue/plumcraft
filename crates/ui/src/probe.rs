//! The pixel probe: where a point on a shown picture falls in the full array, and the exact
//! values stored there. Lifted from the research software's `crates/ui-egui/src/viewer_probe.rs`.
//!
//! A picture is the array scaled down to at most the size asked, so the viewer draws every
//! picture over a rect with the ARRAY's aspect, and a point maps to an array pixel by its fraction
//! of that rect. [`Pixels`] holds a value's own bytes behind an `Arc`: reading a pixel touches
//! only that pixel's elements, and nothing is copied, which is what makes a 12-megapixel array
//! cheap to hover. Nothing here indexes unchecked.
//!
//! A value whose numbers can't be read here (gone from the store, on a remote engine with no
//! `blob.range`, a dtype this viewer doesn't know) has a reason instead of a [`Pixels`], and the
//! viewer says it: the 8-bit picture is never read back as if it were the value.

use std::sync::Arc;

use egui::{Pos2, Rect, pos2};
use plumcraft_proto::data::Form;
use plumcraft_proto::display::Display;
use plumcraft_store::Blob;
use serde_json::{Value, json};

use crate::viewers::grid::Dtype;

/// The picture geometry of an array: rows × cols pixels of `channels` values each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub rows: usize,
    pub cols: usize,
    pub channels: usize,
}

impl Grid {
    /// The geometry of an array of `shape`: (H, W) or (H, W, C). Anything else (a table, a 1-D or
    /// 4-D array, an empty axis) isn't a picture.
    pub fn of_shape(shape: &[u64]) -> Option<Grid> {
        let n = |d: &u64| usize::try_from(*d).ok().filter(|d| *d > 0);
        match shape {
            [r, c] => Some(Grid { rows: n(r)?, cols: n(c)?, channels: 1 }),
            [r, c, k] => Some(Grid { rows: n(r)?, cols: n(c)?, channels: n(k)? }),
            _ => None,
        }
    }

    /// How many elements it holds.
    pub fn elements(&self) -> Option<usize> {
        self.rows.checked_mul(self.cols)?.checked_mul(self.channels)
    }
}

/// One stored element, as the array holds it: no rounding through a float.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Element {
    Bool(bool),
    Int(i64),
    UInt(u64),
    F32(f32),
    F64(f64),
}

impl Element {
    pub fn as_f64(self) -> f64 {
        match self {
            Element::Bool(b) => f64::from(u8::from(b)),
            Element::Int(i) => i as f64,
            Element::UInt(u) => u as f64,
            Element::F32(f) => f64::from(f),
            Element::F64(f) => f,
        }
    }

    fn is_integral(self) -> bool {
        matches!(self, Element::Int(_) | Element::UInt(_))
    }
}

/// Plain digits, or an exponent where they would run long: exact either way.
fn float_text(v: impl std::fmt::Display + std::fmt::LowerExp, magnitude: f64) -> String {
    if magnitude.is_nan() {
        "nan".into()
    } else if magnitude != 0.0 && magnitude.is_finite() && !(1e-4..1e15).contains(&magnitude) {
        format!("{v:e}")
    } else {
        v.to_string()
    }
}

impl std::fmt::Display for Element {
    /// The shortest text that reads back as exactly this value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Element::Bool(b) => write!(f, "{b}"),
            Element::Int(i) => write!(f, "{i}"),
            Element::UInt(u) => write!(f, "{u}"),
            Element::F32(v) => f.write_str(&float_text(*v, f64::from(v.abs()))),
            Element::F64(v) => f.write_str(&float_text(*v, v.abs())),
        }
    }
}

/// The element at `i` of `bytes` (little-endian, as [`Dtype`] reads), or `None` past the end.
#[inline]
fn element(dtype: Dtype, bytes: &[u8], i: usize) -> Option<Element> {
    #[inline]
    fn get<const N: usize>(bytes: &[u8], i: usize) -> Option<[u8; N]> {
        bytes.get(i.checked_mul(N)?..)?.first_chunk::<N>().copied()
    }
    Some(match dtype {
        Dtype::Bool => Element::Bool(get::<1>(bytes, i)? != [0]),
        Dtype::U8 => Element::UInt(u64::from(u8::from_le_bytes(get(bytes, i)?))),
        Dtype::U16 => Element::UInt(u64::from(u16::from_le_bytes(get(bytes, i)?))),
        Dtype::U32 => Element::UInt(u64::from(u32::from_le_bytes(get(bytes, i)?))),
        Dtype::U64 => Element::UInt(u64::from_le_bytes(get(bytes, i)?)),
        Dtype::I8 => Element::Int(i64::from(i8::from_le_bytes(get(bytes, i)?))),
        Dtype::I16 => Element::Int(i64::from(i16::from_le_bytes(get(bytes, i)?))),
        Dtype::I32 => Element::Int(i64::from(i32::from_le_bytes(get(bytes, i)?))),
        Dtype::I64 => Element::Int(i64::from_le_bytes(get(bytes, i)?)),
        Dtype::F32 => Element::F32(f32::from_le_bytes(get(bytes, i)?)),
        Dtype::F64 => Element::F64(f64::from_le_bytes(get(bytes, i)?)),
    })
}

/// A value's numbers as a picture: its own bytes, shared, with the shape checked against them.
#[derive(Clone)]
pub struct Pixels {
    pub grid: Grid,
    dtype: Dtype,
    /// The dtype as the engine named it.
    pub dtype_name: String,
    blob: Arc<Blob>,
    /// A name per channel.
    pub names: Vec<String>,
}

impl std::fmt::Debug for Pixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pixels").field("grid", &self.grid).field("dtype", &self.dtype_name).finish_non_exhaustive()
    }
}

impl Pixels {
    /// The numbers of `blob`, or why they can't be read as a picture. `names` are the array's
    /// own channel names, used when there is one per channel. Nothing is copied.
    pub fn of(blob: &Arc<Blob>, names: &[String]) -> Result<Pixels, String> {
        let Some(grid) = Grid::of_shape(&blob.shape) else {
            return Err(format!("{} is not a picture (H×W or H×W×C)", shape_text(&blob.shape)));
        };
        let Some(dtype) = Dtype::parse(&blob.dtype) else {
            return Err(format!("this viewer can't read dtype {}", blob.dtype));
        };
        let wanted = grid.elements().and_then(|n| n.checked_mul(dtype.size()));
        if wanted.is_none_or(|n| n > blob.bytes.len()) {
            return Err(format!("{} bytes is too few for {} {}", blob.bytes.len(), shape_text(&blob.shape), blob.dtype));
        }
        Ok(Pixels { grid, dtype, dtype_name: blob.dtype.clone(), blob: Arc::clone(blob), names: channel_names(names, grid.channels) })
    }

    /// The value at (row, col, channel), or `None` outside the array.
    pub fn at(&self, row: usize, col: usize, channel: usize) -> Option<Element> {
        let g = self.grid;
        if row >= g.rows || col >= g.cols || channel >= g.channels {
            return None;
        }
        element(self.dtype, &self.blob.bytes, ((row * g.cols + col) * g.channels) + channel)
    }

    /// Every element in order, for a pass over the whole array (the difference): the bytes are
    /// looked up once, not per element.
    pub fn elements(&self) -> impl Iterator<Item = Option<Element>> + '_ {
        let (dtype, bytes) = (self.dtype, &*self.blob.bytes);
        (0..self.grid.elements().unwrap_or(0)).map(move |i| element(dtype, bytes, i))
    }

    /// The values at (row, col), one per channel, at most [`MAX_CHANNELS_READ`].
    pub fn read(&self, row: usize, col: usize) -> Vec<Element> {
        (0..self.grid.channels.min(MAX_CHANNELS_READ)).filter_map(|ch| self.at(row, col, ch)).collect()
    }
}

/// The most channels a readout lists (a hostile array may claim thousands).
pub const MAX_CHANNELS_READ: usize = 16;

/// What a viewer can read of a value.
#[derive(Debug, Clone, Default)]
pub enum Source {
    /// Nothing is shown.
    #[default]
    None,
    /// Being read (an image's decoder).
    Reading,
    Ready(Pixels),
    /// Why there are no exact numbers here.
    Unreadable(String),
}

/// Why a display's full value is missing here, in words.
pub fn no_numbers(d: &Display, gone: bool) -> String {
    match (&d.full, gone) {
        (None, _) => "the engine sent only a picture: no exact numbers to read".into(),
        (Some(_), true) => "no exact values here: the bytes can't be read (reaped, or on a remote engine without blob.range)".into(),
        (Some(_), false) => "no exact values here: the bytes haven't been read".into(),
    }
}

/// What can be read of an array's display, given its bytes.
pub fn source_of(d: &Display, full: Option<&Arc<Blob>>, gone: bool) -> Source {
    match full {
        Some(b) => match Pixels::of(b, names_of(d)) {
            Ok(p) => Source::Ready(p),
            Err(why) => Source::Unreadable(why),
        },
        None => Source::Unreadable(no_numbers(d, gone)),
    }
}

fn names_of(d: &Display) -> &[String] {
    match d.full.as_ref().map(|f| &f.meta.form) {
        Some(Form::Array { names, .. }) => names,
        _ => &[],
    }
}

/// The array's shape, as sent with its display (before any byte is read).
pub fn shape_of(d: &Display) -> Option<Grid> {
    match d.full.as_ref().map(|f| &f.meta.form) {
        Some(Form::Array { shape, table: false, .. }) => Grid::of_shape(shape),
        Some(Form::Image { width, height }) => Grid::of_shape(&[u64::from(*height), u64::from(*width), 3]),
        _ => None,
    }
}

/// The channel names to print: the array's own when there is one per channel, else "value" for
/// one channel and c0, c1, … for several.
pub fn channel_names(names: &[String], channels: usize) -> Vec<String> {
    let shown = channels.min(MAX_CHANNELS_READ);
    if names.len() == channels && names.iter().all(|n| !n.is_empty()) {
        names.iter().take(shown).cloned().collect()
    } else if channels == 1 {
        vec!["value".to_owned()]
    } else {
        (0..shown).map(|i| format!("c{i}")).collect()
    }
}

/// The array pixel (row, col) under `pos`, for a picture of `rows` × `cols` drawn over `rect`.
/// `None` outside the rect. The right and bottom edges belong to the last pixel.
pub fn display_to_array(pos: Pos2, rect: Rect, rows: usize, cols: usize) -> Option<(usize, usize)> {
    if rows == 0 || cols == 0 || !(rect.width() > 0.0 && rect.height() > 0.0) || !rect.contains(pos) {
        return None;
    }
    let u = f64::from((pos.x - rect.min.x) / rect.width());
    let v = f64::from((pos.y - rect.min.y) / rect.height());
    let pick = |f: f64, n: usize| -> usize {
        let i = (f * n as f64).floor();
        if i.is_finite() && i > 0.0 { (i as usize).min(n - 1) } else { 0 }
    };
    Some((pick(v, rows), pick(u, cols)))
}

/// The screen rect of array pixel (row, col) in a picture drawn over `rect`.
pub fn array_to_display(row: usize, col: usize, rect: Rect, rows: usize, cols: usize) -> Rect {
    let sx = rect.width() / cols.max(1) as f32;
    let sy = rect.height() / rows.max(1) as f32;
    let min = pos2(rect.min.x + col as f32 * sx, rect.min.y + row as f32 * sy);
    Rect::from_min_size(min, egui::vec2(sx, sy))
}

/// "hue 0.12345 · saturation 0.6 · intensity 0.31"; an integer array with one channel reads
/// "label 7".
pub fn format_values(names: &[String], values: &[Element]) -> String {
    if let [v] = values
        && v.is_integral()
        && names.first().is_none_or(|n| n == "value")
    {
        return format!("label {v}");
    }
    values.iter().enumerate().map(|(i, v)| format!("{} {v}", names.get(i).map_or("?", String::as_str))).collect::<Vec<_>>().join(" · ")
}

/// A difference as text (shortest round trip; "nan" for NaN).
pub fn format_f64(v: f64) -> String {
    Element::F64(v).to_string()
}

/// A statistic (a mean, not an exact value) to `digits` significant digits.
pub fn format_sig(v: f64, digits: usize) -> String {
    if !v.is_finite() || v == 0.0 {
        return format_f64(v);
    }
    let rounded: f64 = format!("{v:.*e}", digits.saturating_sub(1)).parse().unwrap_or(v);
    format_f64(rounded)
}

/// What a probe read at a pixel, for `ui.inspect`: the text is exact; a number beside it can't
/// be (JSON has no NaN, and a float32 prints shorter than its float64 value).
pub fn reading(p: &Source, at: (usize, usize)) -> Value {
    let (row, col) = at;
    match p {
        Source::Ready(px) if row >= px.grid.rows || col >= px.grid.cols => {
            json!({"row": row, "col": col, "outside": [px.grid.rows, px.grid.cols]})
        }
        Source::Ready(px) => {
            let values = px.read(row, col);
            let text = format_values(&px.names, &values);
            json!({"row": row, "col": col, "text": text, "dtype": px.dtype_name,
                   "values": values.iter().map(ToString::to_string).collect::<Vec<_>>(), "names": px.names})
        }
        Source::Reading => json!({"row": row, "col": col, "reading": true}),
        Source::Unreadable(why) => json!({"row": row, "col": col, "unavailable": why}),
        Source::None => json!({"row": row, "col": col, "unavailable": "nothing is shown"}),
    }
}

/// An array's shape as `3×4`.
pub fn shape_text<D: std::fmt::Display>(shape: &[D]) -> String {
    if shape.is_empty() {
        return "a scalar".into();
    }
    shape.iter().map(ToString::to_string).collect::<Vec<_>>().join("×")
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn blob(dtype: &str, shape: &[u64], bytes: Vec<u8>) -> Arc<Blob> {
        Arc::new(Blob { dtype: dtype.into(), shape: shape.to_vec(), bytes: bytes.into() })
    }

    fn f32s(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn shapes_to_grids() {
        assert_eq!(Grid::of_shape(&[64, 80]), Some(Grid { rows: 64, cols: 80, channels: 1 }));
        assert_eq!(Grid::of_shape(&[64, 80, 3]).map(|g| g.channels), Some(3));
        for bad in [&[][..], &[5], &[0, 4], &[4, 0, 3], &[4, 4, 0], &[1, 2, 3, 4]] {
            assert_eq!(Grid::of_shape(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_pixel_reads_what_the_bytes_hold_by_row_column_and_channel() {
        // 2 × 3 × 2: each element distinct, so a wrong stride reads a wrong number.
        let v: Vec<f32> = (0..12).map(|i| i as f32 + 0.5).collect();
        let p = Pixels::of(&blob("<f4", &[2, 3, 2], f32s(&v)), &[]).unwrap();
        assert_eq!(p.read(0, 0), [Element::F32(0.5), Element::F32(1.5)]);
        assert_eq!(p.read(1, 2), [Element::F32(10.5), Element::F32(11.5)]);
        assert_eq!(p.at(0, 2, 1), Some(Element::F32(5.5)));
        assert_eq!(p.at(2, 0, 0), None, "row past the end");
        assert_eq!(p.at(0, 3, 0), None, "column past the end");
        assert_eq!(p.at(0, 0, 2), None, "channel past the end");
        assert_eq!(p.names, ["c0", "c1"]);
    }

    #[test]
    fn values_too_large_for_a_float_read_exactly() {
        let big = u64::MAX - 1;
        let p = Pixels::of(&blob("<u8", &[1, 1], big.to_le_bytes().to_vec()), &[]).unwrap();
        assert_eq!(p.at(0, 0, 0).unwrap().to_string(), big.to_string());
        let p = Pixels::of(&blob("<i8", &[1, 1], (-3i64).to_le_bytes().to_vec()), &[]).unwrap();
        assert_eq!(p.at(0, 0, 0), Some(Element::Int(-3)));
        for (dt, bytes, want) in [
            ("|i1", vec![0xfe], Element::Int(-2)),
            ("<i2", (-300i16).to_le_bytes().to_vec(), Element::Int(-300)),
            ("<i4", (-70000i32).to_le_bytes().to_vec(), Element::Int(-70000)),
            ("|b1", vec![1], Element::Bool(true)),
            ("<u2", 40000u16.to_le_bytes().to_vec(), Element::UInt(40000)),
            ("<f8", 0.1f64.to_le_bytes().to_vec(), Element::F64(0.1)),
        ] {
            assert_eq!(Pixels::of(&blob(dt, &[1, 1], bytes), &[]).unwrap().at(0, 0, 0), Some(want), "{dt}");
        }
    }

    #[test]
    fn hostile_arrays_say_why_and_never_read_past_their_bytes() {
        let e = |b: Arc<Blob>| Pixels::of(&b, &[]).unwrap_err();
        assert!(e(blob("<f4", &[2, 2], vec![0; 12])).contains("too few"), "shorter than its shape");
        assert!(e(blob("<f4", &[u64::MAX, u64::MAX], vec![0; 12])).contains("too few"), "a shape that overflows");
        assert!(e(blob("<c8", &[1, 1], vec![0; 8])).contains("dtype"), "complex");
        assert!(e(blob(">f4", &[1, 1], vec![0; 4])).contains("dtype"), "big-endian");
        assert!(e(blob("<f4", &[4], vec![0; 16])).contains("not a picture"));
        // a claim of many channels reads only so many
        let p = Pixels::of(&blob("|u1", &[1, 1, 100_000], vec![0; 100_000]), &[]).unwrap();
        assert_eq!(p.read(0, 0).len(), MAX_CHANNELS_READ);
    }

    #[test]
    fn a_downscaled_picture_maps_to_full_resolution_pixels() {
        let (rows, cols) = (3000, 4000);
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(400.0, 300.0));
        let at = |x: f32, y: f32| display_to_array(pos2(x, y), rect, rows, cols);
        assert_eq!(at(10.0, 20.0), Some((0, 0)), "top-left corner");
        assert_eq!(at(410.0, 320.0), Some((2999, 3999)), "the bottom-right edge is the last pixel");
        assert_eq!(at(209.99, 169.99), Some((1499, 1999)));
        assert_eq!(at(210.0, 170.0), Some((1500, 2000)), "the centre");
        assert_eq!(at(9.9, 20.0), None);
        assert_eq!(at(410.1, 20.0), None);
        assert_eq!(at(10.0, 320.5), None);
    }

    #[test]
    fn mapping_round_trips_through_pixel_rects() {
        let rect = Rect::from_min_size(pos2(-3.0, 7.0), vec2(80.0 * 2.5, 64.0 * 2.5));
        for (r, c) in [(0, 0), (63, 79), (31, 40), (0, 79), (63, 0)] {
            let px = array_to_display(r, c, rect, 64, 80);
            assert_eq!(display_to_array(px.center(), rect, 64, 80), Some((r, c)));
            assert!((px.width() - 2.5).abs() < 1e-4);
        }
    }

    #[test]
    fn degenerate_rects_and_grids_map_nowhere() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0));
        assert_eq!(display_to_array(pos2(5.0, 5.0), rect, 0, 4), None);
        assert_eq!(display_to_array(pos2(0.0, 0.0), Rect::from_min_size(pos2(0.0, 0.0), vec2(0.0, 10.0)), 4, 4), None);
        assert_eq!(display_to_array(pos2(f32::NAN, 5.0), rect, 4, 4), None);
        assert_eq!(display_to_array(pos2(9.99, 0.01), rect, 1, 1), Some((0, 0)));
    }

    #[test]
    fn values_print_exactly_with_names() {
        let names: Vec<String> = ["hue", "saturation", "intensity"].map(String::from).to_vec();
        let vals = [Element::F32(0.12345), Element::F32(0.6), Element::F32(f32::NAN)];
        assert_eq!(format_values(&names, &vals), "hue 0.12345 · saturation 0.6 · intensity nan");
        assert_eq!(format_values(&["value".into()], &[Element::Int(7)]), "label 7");
        assert_eq!(format_values(&["mask".into()], &[Element::Bool(true)]), "mask true");
        assert_eq!(format_f64(f64::INFINITY), "inf");
        assert_eq!(format_sig(2.7127122735445e-8, 4), "2.713e-8");
        assert_eq!(format_sig(0.123456789, 3), "0.123");
        assert_eq!(format_sig(f64::NAN, 3), "nan");
    }

    #[test]
    fn names_fall_back_by_channel_count() {
        let meta = vec!["L".to_owned(), "a".to_owned(), "b".to_owned()];
        assert_eq!(channel_names(&meta, 3), ["L", "a", "b"]);
        assert_eq!(channel_names(&meta, 2), ["c0", "c1"]);
        assert_eq!(channel_names(&[], 1), ["value"]);
    }

    #[test]
    fn a_value_that_cant_be_read_says_so_and_gives_no_number() {
        let d = Display { full: Some(Default::default()), ..Default::default() };
        let Source::Unreadable(why) = source_of(&d, None, true) else { panic!("a source without bytes") };
        assert!(why.contains("blob.range"), "{why}");
        let r = reading(&source_of(&d, None, true), (3, 4));
        assert!(r.get("values").is_none() && r["unavailable"].as_str().is_some(), "{r}");
        let none = Display::default();
        assert!(reading(&source_of(&none, None, false), (0, 0))["unavailable"].as_str().unwrap().contains("only a picture"));
    }
}
