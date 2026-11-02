//! The difference |A − B| of two full-resolution values, computed here from their own numbers
//! (never from the 8-bit pictures), with its statistics and a picture stretched over its own
//! range. Lifted from the research software's `crates/ui-egui/src/viewer_diff.rs`.
//!
//! The picture is at most `max_px` on its long side. Each picture pixel shows the LARGEST
//! difference in the block of array pixels it covers, so a single differing pixel is never
//! averaged or sampled away. NaN on either side is a NaN difference: counted, left out of max and
//! mean, and painted in the NaN colour.
//!
//! This runs on a worker: 36 million float32s take a while. It holds the two arrays by `Arc`
//! and allocates only the picture, however large they are.

use egui::Color32;
use plumcraft_session::viewer_cmds::DiffMode;

use crate::media::Rgba;
use crate::probe::{Grid, MAX_CHANNELS_READ, Pixels, shape_text};

/// The long side of a difference picture.
pub const PICTURE_PX: usize = 1024;

/// One picture of the difference.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffPicture {
    pub caption: String,
    pub picture: Rgba,
    /// The picture's own largest difference.
    pub top: f64,
}

/// The difference of two arrays.
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    pub grid: Grid,
    /// Largest finite |A − B| over every element.
    pub max: f64,
    /// Mean finite |A − B| over every element.
    pub mean: f64,
    /// Elements where either side is NaN (or the difference is not finite).
    pub nan: usize,
    /// Elements that differ at all (NaN included).
    pub differing: usize,
    pub elements: usize,
    /// One for the mean, or one per channel (at most [`MAX_CHANNELS_READ`]).
    pub pictures: Vec<DiffPicture>,
}

/// The size of a picture of `rows` × `cols` at most `max_px` on its long side.
pub fn picture_size(rows: usize, cols: usize, max_px: usize) -> (usize, usize) {
    let max_px = max_px.max(1);
    let long = rows.max(cols).max(1);
    if long <= max_px {
        return (rows.max(1), cols.max(1));
    }
    let scale = max_px as f64 / long as f64;
    let fit = |n: usize| ((n as f64 * scale).round() as usize).clamp(1, max_px);
    (fit(rows), fit(cols))
}

/// |A − B| of two pictures of one shape. `stop` is polled between rows: a superseded compare
/// gives up rather than finish.
pub fn diff(a: &Pixels, b: &Pixels, mode: DiffMode, max_px: usize, nan_colour: Color32, stop: &dyn Fn() -> bool) -> Result<Diff, String> {
    if a.grid != b.grid {
        let text = |g: Grid| if g.channels == 1 { shape_text(&[g.rows, g.cols]) } else { shape_text(&[g.rows, g.cols, g.channels]) };
        return Err(format!("A is {} and B is {}: the shapes differ", text(a.grid), text(b.grid)));
    }
    let grid = a.grid;
    let elements = grid.elements().ok_or("too large")?;
    let (out_rows, out_cols) = picture_size(grid.rows, grid.cols, max_px);
    let planes = match mode {
        DiffMode::Mean => 1,
        DiffMode::Channels => grid.channels.min(MAX_CHANNELS_READ),
    };
    let cells = out_rows.checked_mul(out_cols).ok_or("too large")?;
    // Per output pixel: the block's largest difference (NaN marks a block with a NaN).
    let mut blocks = vec![vec![0f64; cells]; planes];
    let (mut max, mut sum, mut finite, mut nan, mut differing) = (0f64, 0f64, 0usize, 0usize, 0usize);
    let (mut xs, mut ys) = (a.elements(), b.elements());
    for row in 0..grid.rows {
        if stop() {
            return Err("stopped".into());
        }
        let orow = (row * out_rows / grid.rows).min(out_rows - 1);
        for col in 0..grid.cols {
            let ocol = (col * out_cols / grid.cols).min(out_cols - 1);
            let cell = orow * out_cols + ocol;
            let (mut channels_sum, mut channels_nan) = (0f64, false);
            for ch in 0..grid.channels {
                let d = match (xs.next().flatten(), ys.next().flatten()) {
                    (Some(x), Some(y)) => (x.as_f64() - y.as_f64()).abs(),
                    _ => return Err("an array is shorter than its shape says".into()),
                };
                if d.is_finite() {
                    max = max.max(d);
                    sum += d;
                    finite += 1;
                    if d != 0.0 {
                        differing += 1;
                    }
                    channels_sum += d;
                } else {
                    nan += 1;
                    differing += 1;
                    channels_nan = true;
                }
                if mode == DiffMode::Channels
                    && let Some(v) = blocks.get_mut(ch).and_then(|p| p.get_mut(cell))
                {
                    keep_largest(v, d);
                }
            }
            if mode == DiffMode::Mean
                && let Some(v) = blocks.first_mut().and_then(|p| p.get_mut(cell))
            {
                keep_largest(v, if channels_nan { f64::NAN } else { channels_sum / grid.channels as f64 });
            }
        }
    }
    let pictures = blocks
        .into_iter()
        .enumerate()
        .map(|(i, block)| {
            let caption = match mode {
                DiffMode::Mean if grid.channels > 1 => "|A−B| mean of channels".to_owned(),
                DiffMode::Mean => "|A−B|".to_owned(),
                DiffMode::Channels => format!("|A−B| {}", a.names.get(i).map_or("?", String::as_str)),
            };
            let (picture, top) = stretch(&block, out_rows, out_cols, nan_colour);
            DiffPicture { caption, picture, top }
        })
        .collect();
    let mean = if finite > 0 { sum / finite as f64 } else { f64::NAN };
    Ok(Diff { grid, max, mean, nan, differing, elements, pictures })
}

/// A block keeps its largest difference; a NaN stays.
fn keep_largest(v: &mut f64, d: f64) {
    if !v.is_nan() {
        *v = if d.is_finite() { v.max(d) } else { f64::NAN };
    }
}

/// The difference of a value with itself, known from its handle without reading it: `grid`'s
/// picture is black.
pub fn identical(grid: Grid, names: &[String], mode: DiffMode, max_px: usize) -> Result<Diff, String> {
    let elements = grid.elements().ok_or("too large")?;
    let (rows, cols) = picture_size(grid.rows, grid.cols, max_px);
    let planes = match mode {
        DiffMode::Mean => 1,
        DiffMode::Channels => grid.channels.min(MAX_CHANNELS_READ),
    };
    let black = Rgba { w: cols as u32, h: rows as u32, pixels: [0, 0, 0, 255].repeat(rows.checked_mul(cols).ok_or("too large")?) };
    let caption = |i: usize| match mode {
        DiffMode::Mean if grid.channels > 1 => "|A−B| mean of channels".to_owned(),
        DiffMode::Mean => "|A−B|".to_owned(),
        DiffMode::Channels => format!("|A−B| {}", names.get(i).map_or("?", String::as_str)),
    };
    let pictures = (0..planes).map(|i| DiffPicture { caption: caption(i), picture: black.clone(), top: 0.0 }).collect();
    Ok(Diff { grid, max: 0.0, mean: 0.0, nan: 0, differing: 0, elements, pictures })
}

/// Grey from 0 (no difference) to the picture's own largest difference; NaN in `nan_colour`.
fn stretch(block: &[f64], rows: usize, cols: usize, nan_colour: Color32) -> (Rgba, f64) {
    let top = block.iter().copied().filter(|v| v.is_finite()).fold(0f64, f64::max);
    let mut pixels = Vec::with_capacity(block.len() * 4);
    for v in block {
        let c = if !v.is_finite() {
            nan_colour
        } else if top > 0.0 {
            Color32::from_gray(((v / top) * 255.0).round().clamp(0.0, 255.0) as u8)
        } else {
            Color32::BLACK
        };
        pixels.extend_from_slice(&[c.r(), c.g(), c.b(), 255]);
    }
    (Rgba { w: cols as u32, h: rows as u32, pixels }, top)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use plumcraft_store::Blob;

    use super::*;

    fn f32s(shape: &[u64], data: &[f32]) -> Pixels {
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        Pixels::of(&Arc::new(Blob { dtype: "<f4".into(), shape: shape.to_vec(), bytes: bytes.into() }), &[]).unwrap()
    }

    const NAN: Color32 = Color32::from_rgb(255, 0, 255);

    fn run(a: &Pixels, b: &Pixels, mode: DiffMode, px: usize) -> Result<Diff, String> {
        diff(a, b, mode, px, NAN, &|| false)
    }

    fn px(p: &Rgba, i: usize) -> [u8; 3] {
        [p.pixels[i * 4], p.pixels[i * 4 + 1], p.pixels[i * 4 + 2]]
    }

    #[test]
    fn identical_arrays_differ_nowhere() {
        let data: Vec<f32> = (0..12).map(|i| i as f32 / 7.0).collect();
        let d = run(&f32s(&[2, 2, 3], &data), &f32s(&[2, 2, 3], &data), DiffMode::Mean, 1024).unwrap();
        assert_eq!((d.max, d.mean, d.nan, d.differing, d.elements), (0.0, 0.0, 0, 0, 12));
        assert!(d.pictures[0].picture.pixels.chunks(4).all(|p| p == [0, 0, 0, 255]));
    }

    #[test]
    fn per_channel_and_mean_statistics() {
        // 1 × 2 pixels, 2 channels: differences (0.5, 0) and (0, 0.25).
        let a = f32s(&[1, 2, 2], &[1.0, 0.0, 0.0, 0.5]);
        let b = f32s(&[1, 2, 2], &[0.5, 0.0, 0.0, 0.25]);
        let d = run(&a, &b, DiffMode::Channels, 1024).unwrap();
        assert_eq!((d.max, d.mean, d.differing), (0.5, 0.1875, 2));
        assert_eq!(d.pictures.len(), 2);
        assert_eq!(d.pictures[0].caption, "|A−B| c0");
        assert_eq!((px(&d.pictures[0].picture, 0), px(&d.pictures[0].picture, 1)), ([255; 3], [0; 3]));
        assert_eq!((px(&d.pictures[1].picture, 0), px(&d.pictures[1].picture, 1)), ([0; 3], [255; 3]), "each stretched over its own range");
        let m = run(&a, &b, DiffMode::Mean, 1024).unwrap();
        assert_eq!(m.pictures.len(), 1);
        assert_eq!(m.pictures[0].top, 0.25, "the mean of the channels' differences");
        assert_eq!((px(&m.pictures[0].picture, 0), px(&m.pictures[0].picture, 1)), ([255; 3], [128; 3]));
    }

    #[test]
    fn nan_is_counted_and_painted_not_averaged() {
        let a = f32s(&[1, 3], &[f32::NAN, 1.0, f32::INFINITY]);
        let b = f32s(&[1, 3], &[0.0, 0.0, 0.0]);
        let d = run(&a, &b, DiffMode::Mean, 1024).unwrap();
        assert_eq!((d.max, d.mean, d.nan, d.differing), (1.0, 1.0, 2, 3));
        let p = &d.pictures[0].picture;
        assert_eq!((px(p, 0), px(p, 1), px(p, 2)), ([255, 0, 255], [255; 3], [255, 0, 255]));
        let all = f32s(&[1, 3], &[f32::NAN; 3]);
        let d = run(&all, &b, DiffMode::Mean, 1024).unwrap();
        assert!(d.mean.is_nan() && d.max == 0.0 && d.nan == 3);
    }

    #[test]
    fn shapes_must_match() {
        let a = f32s(&[2, 3], &[0.0; 6]);
        let b = f32s(&[3, 2], &[0.0; 6]);
        assert_eq!(run(&a, &b, DiffMode::Mean, 64).unwrap_err(), "A is 2×3 and B is 3×2: the shapes differ");
    }

    #[test]
    fn a_single_differing_pixel_survives_downscaling() {
        let (rows, cols) = (40usize, 100usize);
        let a = f32s(&[rows as u64, cols as u64], &vec![0.0; rows * cols]);
        let mut data = vec![0.0; rows * cols];
        data[17 * cols + 63] = 0.001;
        let d = run(&a, &f32s(&[rows as u64, cols as u64], &data), DiffMode::Mean, 10).unwrap();
        let p = &d.pictures[0].picture;
        assert_eq!((p.w, p.h), (10, 4));
        let white: Vec<usize> = (0..40).filter(|i| px(p, *i) == [255; 3]).collect();
        let (orow, ocol) = (17 * 4 / 40, 63 * 10 / 100);
        assert_eq!(white, [orow * 10 + ocol]);
        assert_eq!((d.differing, d.max as f32), (1, 0.001));
    }

    #[test]
    fn a_stopped_compare_gives_up_and_many_channels_stay_bounded() {
        let a = f32s(&[2, 2], &[0.0; 4]);
        assert_eq!(diff(&a, &a, DiffMode::Mean, 8, NAN, &|| true).unwrap_err(), "stopped");
        let c = (MAX_CHANNELS_READ + 4) as u64;
        let wide = f32s(&[1, 1, c], &vec![1.0; c as usize]);
        assert_eq!(run(&wide, &wide, DiffMode::Channels, 8).unwrap().pictures.len(), MAX_CHANNELS_READ);
    }

    #[test]
    fn the_same_handle_needs_no_reading() {
        let g = Grid { rows: 3000, cols: 4000, channels: 3 };
        let d = identical(g, &[], DiffMode::Mean, 1024).unwrap();
        assert_eq!((d.max, d.differing, d.elements), (0.0, 0, 36_000_000));
        assert_eq!((d.pictures[0].picture.w, d.pictures[0].picture.h), (1024, 768));
    }

    #[test]
    fn picture_sizes_keep_aspect() {
        assert_eq!(picture_size(3000, 4000, 1024), (768, 1024));
        assert_eq!(picture_size(64, 80, 1024), (64, 80));
        assert_eq!(picture_size(1, 100_000, 1024), (1, 1024));
        assert_eq!(picture_size(0, 0, 0), (1, 1));
    }
}
