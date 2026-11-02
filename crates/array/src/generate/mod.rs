//! Arrays made from nothing, regions, drawing and the frequency domain (the research software's Generate,
//! Regions, Draw and Frequency groups, from its `generate.py` and `external.py`): Coordinates,
//! Simplex Noise, Mandelbrot, Game of Life; Random Points, Mask Points; Connected Components,
//! Region Outlines, Grid Segments, Segment Average; FFT, Inverse FFT; Draw Shapes, Draw Lines,
//! Composite.
//!
//! Simplex Noise, Mandelbrot, FFT and Draw Shapes/Composite are ported from yapocis's numba
//! kernels (`simplex.py`, `mandelbrot.py`, `fft.py`, `graphics4.py`) without `fastmath`; what
//! The research software took from yapocis_external (Coordinates, Game of Life, Connected Components, Region
//! Outlines, Segment Average, Draw Lines) is written here plainly from its description. numpy's
//! own draws (`np.random.default_rng`) come from npexact, so seeded points are numpy's.
//!
//! Draw Text is left out: it needs a font rasteriser and a font, and this crate has neither.

pub mod draw;
pub mod fft;
pub mod noise;
pub mod points;
pub mod regions;
pub mod text;

use plumcraft_engine::{Inputs, Node};
use plumcraft_proto::catalogue::Port;
use plumcraft_proto::error::NodeError;

use crate::img::{Img, MAX_SIDE};
use crate::op::{Op, array_in, array_opt, array_out, bool_in, config, enum_in, float_in, img_opt, int_in, n, string_in};

const GENERATE: &str = "Generate";
const REGIONS: &str = "Regions";
const DRAW: &str = "Draw";
const FREQUENCY: &str = "Frequency";

/// The research software's default array tooltip.
const ARRAY_TIP: &str = "An array: a channel (H × W) or a stack (H × W × C), 0-1";

/// The largest seed (the research software's `2**31 - 1`).
const MAX_SEED: f64 = 2_147_483_647.0;

pub fn nodes() -> Vec<Box<dyn Node>> {
    let mut size = size_inputs();
    vec![
        // ---------- Generate ----------
        Box::new(Op {
            id: "array.coordinates",
            display: "Coordinates",
            group: GENERATE,
            description: "Each pixel's row and column (yapocis_external xy), for position-dependent effects with Arithmetic: gradients, vignettes, ramps.",
            inputs: vec![array_in("like", "Take the size from this array")],
            outputs: vec![array_out("x", "Column index"), array_out("y", "Row index")],
            run: noise::coordinates,
        }),
        Box::new(Op {
            id: "array.simplex_noise",
            display: "Simplex Noise",
            group: GENERATE,
            description: "Fractal simplex noise, 0-1 (yapocis simplex): smooth (fBm), turbulence (|noise|, billowy) or ridged (sharp crests). For textures, paper grain, displacement, masks.",
            inputs: {
                let mut v = vec![
                    enum_in("kind", &["smooth", "turbulence", "ridged"], "smooth", "smooth (fBm), turbulence (|noise|, billowy) or ridged (sharp crests)"),
                    float_in("persistence", 0.5, 0.05, 1.0, "Amplitude kept per octave: higher = rougher (smooth only)"),
                    float_in("lacunarity", 2.0, 1.1, 4.0, "Frequency gain per octave (smooth only)"),
                    bool_in("tileable", false, "Wrap seamlessly at the edges (smooth only)"),
                    float_in("cells", 4.0, 0.1, 512.0, "Noise cells across each side (yapocis scale): more = finer"),
                    int_in("octaves", 4, 1.0, 10.0, "Layers of finer detail"),
                    int_in("seed", 0, 0.0, MAX_SEED, "The noise's seed"),
                ];
                v.append(&mut size.clone());
                v
            },
            outputs: vec![array_out("noise", "Noise, 0-1")],
            run: noise::simplex_noise,
        }),
        Box::new(Op {
            id: "array.mandelbrot",
            display: "Mandelbrot",
            group: GENERATE,
            description: "The Mandelbrot set (yapocis mandelbrot, float32 so deep zooms pixelate): each pixel's escape iteration divided by max_iterations, 0 inside the set.",
            inputs: {
                let mut v = vec![
                    float_in("center_x", -0.5, -4.0, 4.0, "The real part at the centre"),
                    float_in("center_y", 0.0, -4.0, 4.0, "The imaginary part at the centre"),
                    float_in("zoom", 1.0, 0.01, 1e6, "1 shows 3 units across"),
                    int_in("max_iterations", 256, 8.0, 32000.0, "Iterations before a point counts as inside the set"),
                ];
                v.append(&mut size);
                v
            },
            outputs: vec![array_out("escape", "Escape iteration / max_iterations, 0 inside the set")],
            run: noise::mandelbrot,
        }),
        Box::new(Op {
            id: "array.game_of_life",
            display: "Game of Life",
            group: GENERATE,
            description: "Conway's Game of Life on a torus (yapocis_external life): live cells (1) with 2 or 3 live neighbours survive, dead cells with 3 come alive.",
            inputs: vec![
                int_in("steps", 50, 0.0, 100_000.0, "Generations to run"),
                array_opt("start", "Starting cells: nonzero = alive (e.g. Binarize of a picture)"),
                array_opt("like", "Size for a random start"),
                float_in("density", 0.3, 0.0, 1.0, "Live fraction of a random start"),
                int_in("seed", 0, 0.0, MAX_SEED, "Seed of a random start"),
            ],
            outputs: vec![array_out("cells", "1 = alive, 0 = dead")],
            run: noise::game_of_life,
        }),
        // ---------- Regions ----------
        Box::new(Op {
            id: "array.connected_components",
            display: "Connected Components",
            group: REGIONS,
            description: "Labels the 4-connected areas between edges (yapocis_external label kernel, propagated to convergence), numbered 0, 1, 2, ... Edge pixels get their own labels too. Faster than Region Labels for few regions; no single-pixel merging.",
            inputs: vec![
                array_in("edges", "1 on boundaries, 0 inside regions"),
                int_in(
                    "max_iterations",
                    10000,
                    1.0,
                    1_000_000.0,
                    "Upper bound on propagation steps (about the longest path inside a region); the labelling here always runs to convergence, so this is kept only for the research software's flows",
                ),
            ],
            outputs: vec![array_out("labels", "Labels 0, 1, 2, ... (integers), numbered in scan order")],
            run: regions::connected_components,
        }),
        Box::new(Op {
            id: "array.region_outlines",
            display: "Region Outlines",
            group: REGIONS,
            description: "1 on the pixels of each region that touch another region, 0 inside (yapocis_external hollow): outlines of a label map.",
            inputs: vec![array_in("labels", "A label map (one channel; values are rounded to integers)")],
            outputs: vec![array_out("outlines", "1 on region boundaries, 0 inside")],
            run: regions::region_outlines,
        }),
        Box::new(Op {
            id: "array.grid_segments",
            display: "Grid Segments",
            group: REGIONS,
            description: "A segments table of rectangles tiling the array: for Segment Average (mosaic, pixelation).",
            inputs: vec![
                array_in("like", "Take the size from this array"),
                int_in("tile_width", 16, 1.0, 4096.0, "Tile width in pixels"),
                int_in("tile_height", 16, 1.0, 4096.0, "Tile height in pixels"),
            ],
            outputs: vec![array_out("segments", "Table of n, x1, y1, x2, y2")],
            run: regions::grid_segments,
        }),
        Box::new(Op {
            id: "array.segment_average",
            display: "Segment Average",
            group: REGIONS,
            description: "Replaces each rectangle of a segments table by its mean or median (yapocis_external averagesegments / applysegments): with Grid Segments, a mosaic. Pixels outside every rectangle keep their values; values are clamped to 0-1.",
            inputs: vec![
                array_in("array", "Values in 0-1; a stack is done channel by channel"),
                array_in("segments", "Table of n, x1, y1, x2, y2 (x = column, y = row; n = pixels)"),
                enum_in("statistic", &["mean", "median"], "mean", "Each rectangle's mean or median"),
                float_in("period", 0.0, 0.0, 1e6, "For two-pixel segments: the circular period (1 for hue in turns); 0 = not circular"),
            ],
            outputs: vec![array_out("averaged", "The array with each rectangle replaced")],
            run: regions::segment_average,
        }),
        // ---------- Draw ----------
        Box::new(Op {
            id: "array.draw_shapes",
            display: "Draw Shapes",
            group: DRAW,
            description: "Draws a table of shapes onto a canvas, antialiased and blended (yapocis graphics4): lines (x0 y0 x1 y1), rects (corners x0 y0 x1 y1), circles (x y radius), ellipses (x y radius_x radius_y), triangles and mesh triangles (x1 y1 x2 y2 x3 y3; mesh triangles draw shared edges once). x = column, y = row. A colour canvas comes back as RGB, a one-channel one as grey.",
            inputs: vec![
                array_in("canvas", "One channel or a colour stack (Constant for a blank one)"),
                array_in("shapes", "A table with the columns the shape needs (Table, Trace Contours, Random Points)"),
                enum_in("shape", &draw::SHAPES.iter().map(|(s, _)| *s).collect::<Vec<_>>(), "lines", "The kind of shape each row is"),
                string_in("color", "#ffffff", "The colour, #rrggbb"),
                float_in("opacity", 1.0, 0.0, 1.0, "How opaque the shapes are"),
                float_in("width", 1.0, 0.1, 200.0, "Line width in pixels (lines only)"),
                float_in("antialias", 1.0, 0.0, 10.0, "Width of the soft edge in pixels (0 = hard)"),
                array_opt("colors", "Per-shape colours: a table of r, g, b (0-1) rows"),
            ],
            outputs: vec![array_out("drawing", "The canvas with the shapes drawn")],
            run: draw::draw_shapes,
        }),
        Box::new(Op {
            id: "array.draw_lines",
            display: "Draw Lines",
            group: DRAW,
            description: "Draws a table of lines into one channel: hard 1-pixel Bresenham lines of any value (yapocis_external draw), or antialiased 1-pixel lines (graphicsx; 1 - distance, overwriting).",
            inputs: vec![
                array_in("canvas", "One channel to draw on (Constant for a blank one)"),
                array_in("lines", "Table of x0, y0, x1, y1 (x = column, y = row)"),
                enum_in("style", &["hard", "antialiased"], "hard", "hard (Bresenham, value) or antialiased (1 - distance)"),
                float_in("value", 1.0, -1e9, 1e9, "Value drawn (hard lines)"),
            ],
            outputs: vec![array_out("drawing", "The channel with the lines drawn")],
            run: draw::draw_lines,
        }),
        Box::new(Op {
            id: "array.composite",
            display: "Composite",
            group: DRAW,
            description: "Pastes one array onto another at x, y (yapocis graphics4 composite), blended by `alpha` or opacity. Parts outside the canvas are dropped.",
            inputs: vec![
                array_in("canvas", "One channel or a colour stack"),
                array_in("block", "What to paste (one channel or a colour stack)"),
                int_in("x", 0, -16384.0, 16384.0, "Column of its left edge"),
                int_in("y", 0, -16384.0, 16384.0, "Row of its top edge"),
                float_in("opacity", 1.0, 0.0, 1.0, "How opaque the block is"),
                array_opt("alpha", "Per-pixel opacity of the block, 0-1 (e.g. Artistic Lines' alpha)"),
            ],
            outputs: vec![array_out("composite", "The canvas with the block pasted")],
            run: draw::composite,
        }),
        Box::new(Op {
            id: "array.draw_text",
            display: "Draw Text",
            group: DRAW,
            description: "Text onto a canvas, set in a font's outlines and filled antialiased (the research software drew it with yapocis graphics4 and FreeType). With no anchor, x, y is the left end of the text, vertically centred, and a negative position counts back from the right or bottom edge (slice style: x -20 is 20 px from the right). With an anchor (L, C, R across; T, C, B down) the position is an offset from that edge or the centre of the canvas, and the text's own edge or centre is put there. Fonts: Inter, the craft-fonts faces when this build has them, or a font file. One line per line break; no shaping (no ligatures or kerning, Arabic not joined); a character the font lacks is skipped.",
            inputs: vec![
                array_in("canvas", "One channel or a colour stack"),
                string_in("text", "Virtual Impressionist", "The text; a line break starts a new line"),
                int_in("x", 20, -16384.0, 16384.0, "Column; negative counts from the right edge, or with an anchor, the offset from it"),
                int_in("y", 40, -16384.0, 16384.0, "Row; negative counts from the bottom edge, or with an anchor, the offset from it"),
                enum_in(
                    "font",
                    &text::font_names().iter().map(String::as_str).collect::<Vec<_>>(),
                    "Inter",
                    "The face: Inter, or a craft-fonts face (in builds made with CRAFT_FONTS_DIR)",
                ),
                crate::op::advanced(crate::op::path_in("font_file", "A font file (TTF or OTF) to use instead of font")),
                int_in("size", 32, 4.0, 1000.0, "Size in pixels"),
                float_in("rotation", 0.0, -180.0, 180.0, "Degrees, counter-clockwise, about the text's centre"),
                string_in("color", "#ffffff", "The colour, #rrggbb"),
                float_in("opacity", 1.0, 0.0, 1.0, "How opaque the text is"),
                enum_in(
                    "h_align",
                    text::H_ALIGN,
                    "none",
                    "Anchor across: L (left edge), C (centre), R (right edge); x is the offset from it. none: x is the column",
                ),
                enum_in(
                    "v_align",
                    text::V_ALIGN,
                    "none",
                    "Anchor down: T (top edge), C (centre), B (bottom edge); y is the offset from it. none: y is the row",
                ),
            ],
            outputs: vec![array_out("drawing", "The canvas with the text drawn")],
            run: text::draw_text,
        }),
        Box::new(Op {
            id: "array.random_points",
            display: "Random Points",
            group: DRAW,
            description: "Random x, y points over an array's area, optionally with extra constant columns (a radius for circles, say): seeds for Durer Strokes, centres for Fluid and Draw Shapes.",
            inputs: vec![
                array_in("like", "Points fall within this array"),
                int_in("count", 200, 0.0, 10_000_000.0, "How many points"),
                int_in("seed", 0, 0.0, MAX_SEED, "The draws' seed"),
                string_in("extra", "", "Values appended to every row, e.g. 6 for circles of radius 6"),
            ],
            outputs: vec![array_out("points", "Table of x, y (and the extra columns c3, c4, ...)")],
            run: points::random_points,
        }),
        Box::new(Op {
            id: "array.mask_points",
            display: "Mask Points",
            group: DRAW,
            description: "The x, y of pixels where a mask is on (optionally a random fraction of them): seeds that follow edges or a region.",
            inputs: vec![
                array_in("mask", "One channel; above 0.5 = on"),
                float_in("fraction", 1.0, 0.0, 1.0, "The fraction of the pixels kept, at random"),
                int_in("seed", 0, 0.0, MAX_SEED, "The draws' seed"),
            ],
            outputs: vec![array_out("points", "Table of x, y")],
            run: points::mask_points,
        }),
        // ---------- Frequency ----------
        Box::new(Op {
            id: "array.fft",
            display: "FFT",
            group: FREQUENCY,
            description: "The 2-D Fourier transform of each channel (yapocis fft, rows then columns), as magnitude and phase with zero frequency at the centre. Edit the magnitude (Arithmetic with a mask, Gaussian Blur, ...) and Inverse FFT to filter in the frequency domain. View the magnitude in log form. Any size: lengths that aren't powers of 2 use Bluestein's transform.",
            inputs: vec![array_in("array", ARRAY_TIP)],
            outputs: vec![
                array_out("magnitude", "Magnitude, zero frequency at the centre"),
                array_out("phase", "Radians"),
                array_out("log_magnitude", "log(1 + magnitude), for viewing"),
            ],
            run: fft::fft,
        }),
        Box::new(Op {
            id: "array.inverse_fft",
            display: "Inverse FFT",
            group: FREQUENCY,
            description: "Back from magnitude and phase (as FFT gives them, zero frequency at the centre) to an array: the real part of the inverse transform.",
            inputs: vec![array_in("magnitude", ARRAY_TIP), array_in("phase", "Radians")],
            outputs: vec![array_out("array", "The real part of the inverse transform")],
            run: fft::inverse_fft,
        }),
    ]
}

/// The research software's `size_inputs`: width, height, or the size of `like`.
fn size_inputs() -> Vec<Port> {
    vec![
        int_in("width", 512, 8.0, 16384.0, "Used when like isn't connected"),
        int_in("height", 512, 8.0, 16384.0, "Used when like isn't connected"),
        array_opt("like", "Take the size from this array"),
    ]
}

/// The (height, width) of an array as the research software sees it (`data.shape[:2]`): a table's is
/// (rows, columns).
pub fn size_of(a: &Img) -> (usize, usize) {
    if a.table { (a.h, a.c) } else { (a.h, a.w) }
}

/// The research software's `size_of`: the size of `like`, or width × height.
fn size_from(i: &Inputs) -> Result<(usize, usize), NodeError> {
    let (h, w) = match img_opt(i, "like")? {
        Some(l) => size_of(&l),
        None => (n(i, "height", 512).clamp(8, MAX_SIDE as i64) as usize, n(i, "width", 512).clamp(8, MAX_SIDE as i64) as usize),
    };
    Img::check_size(h, w, 1).map_err(config)?;
    Ok((h, w))
}

/// One channel, as the research software's `as_channel` takes it.
fn as_channel(a: &Img, what: &str) -> Result<Vec<f32>, NodeError> {
    if a.table || a.c != 1 {
        return Err(config(format!("{what} must be one channel, got {} (use Split or Pick Channel)", a.describe())));
    }
    if a.data.len() != a.h * a.w {
        return Err(config(format!("{what}: the values don't fill the shape")));
    }
    Ok(a.data.clone())
}

/// A seed from an input, as numpy's `default_rng` takes it (0 ..= 2^31 - 1).
fn seed(i: &Inputs) -> u128 {
    n(i, "seed", 0).clamp(0, MAX_SEED as i64) as u128
}

/// A table of `rows` × `names.len()` values.
fn table(data: Vec<f32>, names: Vec<String>) -> Img {
    let c = names.len().max(1);
    Img { h: data.len() / c, w: 1, c, data, names, table: true, ..Default::default() }
}

/// The rows of a table (or of any array, reshaped to (H, W·C) as numpy's
/// `reshape(len(data), -1)` does), with their column count.
fn rows_of(a: &Img) -> (usize, usize) {
    if a.table { (a.h, a.c) } else { (a.h, a.w * a.c) }
}

/// Values of a table up to [`crate::img::MAX_VALUES`].
fn check_table(rows: usize, cols: usize) -> Result<(), NodeError> {
    if rows.saturating_mul(cols.max(1)) > crate::img::MAX_VALUES {
        return Err(config(format!("a table of {rows} × {cols} is too large (at most {} values)", crate::img::MAX_VALUES)));
    }
    Ok(())
}

/// Python's `max(a, b)` on floats: `a` unless `b` is greater (so NaN in `b` gives `a`).
fn pymax(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// Python's `min(a, b)` on floats: `a` unless `b` is smaller.
fn pymin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

fn cancelled() -> NodeError {
    NodeError::new(NodeError::CANCELLED, "cancelled")
}
