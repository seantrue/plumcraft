//! Filters, edges and geometry (the research software's Filters and Edges groups and two Utilities): Gaussian
//! Blur, Median 3×3, Convolve, Custom Convolve, Perona–Malik; Gradient, Sobel Magnitude, Zero
//! Crossings, Segment Edges, Trace Contours; Rotate and Resize.
//!
//! The arithmetic is yapocis's numba kernels ([`kernels`]), its truetrace CPU path ([`trace`]),
//! virtual_impressionist's `flow` and `core.resize` with Pillow's float resampling ([`resize`]).
//! Sobel Magnitude and Rotate were yapocis_external GPU kernels, written here plainly from them.
//! Functions of one channel run on each channel of a stack; the result keeps the stack's colour
//! space and names when the values keep their meaning (a blurred hue is still a hue; a gradient
//! is not).

pub mod conv_kernels;
pub mod kernels;
pub mod ladder;
pub mod resize;
pub mod trace;

use plumcraft_engine::{Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::error::NodeError;

use crate::img::{Img, MAX_VALUES};
use crate::op::{Op, array_in, array_opt, array_out, b, bool_in, config, enum_in, f, float_in, img, img_opt, int_in, many, n, one, op_error, s, string_in};

const FILTERS: &str = "Filters";
const EDGES: &str = "Edges";
const GEOMETRY: &str = "Geometry";

/// The research software's `ARRAY_TIP`.
const ARRAY_TIP: &str = "An array: a channel (H × W) or a stack (H × W × C), 0-1";

const SCALE_TIP: &str = "Gaussian scale: sigma 1.6^scale (0: 1 px, 1: 1.6, 2: 2.6, 3: 4.1, 4: 6.6, 5: 10.5, 6: 16.8, 7: 26.8 px)";

const DIRECTIONS: &[&str] = &["both", "horizontal", "vertical"];
const DIRECTION_TIP: &str = "Along rows (horizontal), columns (vertical), or one then the other";

/// The research software's `ANGLE_UNITS`: the angle in multiples of π times this.
const ANGLE_UNITS: &[(&str, f32)] = &[("turns", 0.5), ("degrees", 180.0), ("radians", std::f32::consts::PI), ("π units", 1.0)];
const ANGLE_TIP: &str = "Units of the angle output. yapocis gives multiples of π (-1..1, 0 = increasing downward, 0.5 = increasing to the right); degrees, radians and turns are the same angle scaled";

fn angle_in(default: &str) -> plumcraft_proto::catalogue::Port {
    let units: Vec<&str> = ANGLE_UNITS.iter().map(|(u, _)| *u).collect();
    enum_in("angle_unit", &units, default, ANGLE_TIP)
}

pub fn nodes() -> Vec<Box<dyn Node>> {
    vec![
        Box::new(Op {
            id: "array.gaussian_blur",
            display: "Gaussian Blur",
            group: FILTERS,
            description: "Separable Gaussian blur (yapocis convolvesep). Each channel of a stack is blurred. Like yapocis, it leaves a black (0) frame half the kernel wide (4 px at scale 0, 63 px at 7): the edges are not padded.",
            inputs: vec![array_in("array", ARRAY_TIP), int_in("scale", 1, 0.0, 7.0, SCALE_TIP)],
            outputs: vec![array_out("blurred", "The blurred array, 0 within half the kernel of the border")],
            run: gaussian_blur,
        }),
        Box::new(Op {
            id: "array.median_3x3",
            display: "Median 3×3",
            group: FILTERS,
            description: "3×3 median filter: removes speckle and keeps edges. Each channel of a stack is filtered.",
            inputs: vec![array_in("array", ARRAY_TIP), int_in("iterations", 1, 0.0, 100.0, "Passes; each widens the effect")],
            outputs: vec![array_out("filtered", "The filtered array (the outermost pixels are kept)")],
            run: median3x3,
        }),
        Box::new(Op {
            id: "array.convolve",
            display: "Convolve",
            group: FILTERS,
            description: "One of yapocis's named 1-D kernels along rows and/or columns: windows (hamming, hann, blackman, bartlett), smoothing (box, triangle, binomial), derivatives (deriv1 central difference, deriv2 second difference, sobel, prewitt, edge, savgol), sharpening (sharpen, unsharp) and band-pass (dog). The number is the kernel length. The kernel runs over the flattened rows (or columns), so it carries on from one row's end into the next row's start, and reflects at the array's first and last values (yapocis convolve on the CPU).",
            inputs: vec![
                array_in("array", ARRAY_TIP),
                {
                    let names = conv_kernels::names();
                    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                    enum_in("kernel", &refs, "binomial5", "The kernel: its type and length")
                },
                enum_in("direction", DIRECTIONS, "both", DIRECTION_TIP),
            ],
            outputs: vec![array_out("convolved", "The convolved array")],
            run: convolve,
        }),
        Box::new(Op {
            id: "array.custom_convolve",
            display: "Custom Convolve",
            group: FILTERS,
            description: "A 1-D kernel of your own weights along rows and/or columns (yapocis Convolve1D), run as Convolve runs its kernels.",
            inputs: vec![
                array_in("array", ARRAY_TIP),
                string_in("weights", "1, 4, 6, 4, 1", "Kernel weights, comma or space separated (an odd count)"),
                bool_in("normalize", true, "Divide by the sum of the weights (when it isn't 0), so flat areas keep their value"),
                enum_in("direction", DIRECTIONS, "both", DIRECTION_TIP),
            ],
            outputs: vec![array_out("convolved", "The convolved array")],
            run: custom_convolve,
        }),
        Box::new(Op {
            id: "array.perona_malik",
            display: "Perona–Malik",
            group: FILTERS,
            description: "Anisotropic (edge-preserving) diffusion (yapocis anisodiff): smooths flat areas, keeps edges. Each channel is mapped to 0-1 for the filter and back. Connect `edges` (e.g. Segment Edges from a SAM2 segmentation) to keep those boundaries: nothing diffuses across them, everything else diffuses as without them (flow.perona_malik).",
            inputs: vec![
                array_in("array", ARRAY_TIP),
                int_in("iterations", 8, 0.0, 500.0, "Diffusion steps; more = smoother"),
                float_in("kappa", 3.0, 0.01, 256.0, "Edge threshold on a 0-256 scale: differences smaller than this diffuse, larger ones are kept"),
                float_in("step", 0.2, 0.01, 0.25, "Step size per iteration (at most 0.25 for stability)"),
                array_opt("edges", "Boundaries to keep (1 on edges), e.g. Segment Edges or DoG Sketch's sketch"),
                float_in(
                    "boost",
                    0.5,
                    0.05,
                    0.9,
                    "Ridge height given to the edges (content and kappa are scaled to make room). Higher = a smaller halo where the ridge bleeds into its neighbours. Ignored without edges.",
                ),
                enum_in(
                    "input",
                    &["map to 0-1", "as is"],
                    "map to 0-1",
                    "map to 0-1: each channel's own range (kappa then means the same for any array). as is: the values go straight to yapocis anisodiff, as the commands' intensity I() does (kappa on a 0-256 scale only for 0-1 data)",
                ),
            ],
            outputs: vec![array_out("diffused", "The diffused array")],
            run: perona_malik,
        }),
        Box::new(Op {
            id: "array.gradient",
            display: "Gradient",
            group: EDGES,
            description: "Gradient magnitude and angle by central differences `reach` pixels apart (yapocis gradient; not divided by the distance). A border `reach` wide is 0.",
            inputs: vec![
                array_in("array", ARRAY_TIP),
                int_in("reach", 3, 1.0, 64.0, "Pixels between the centre and each sample; larger = smoother, broader edges"),
                angle_in("degrees"),
            ],
            outputs: vec![array_out("magnitude", "Gradient magnitude"), array_out("angle", "Gradient angle")],
            run: gradient,
        }),
        Box::new(Op {
            id: "array.sobel_magnitude",
            display: "Sobel Magnitude",
            group: EDGES,
            description: "Gradient magnitude by the Sobel operator (yapocis_external anisodiff_ctrl), unnormalised: up to about 5.7 × the data's range. The border repeats its outermost pixels.",
            inputs: vec![array_in("array", ARRAY_TIP)],
            outputs: vec![array_out("magnitude", "Sobel gradient magnitude")],
            run: sobel_magnitude,
        }),
        Box::new(Op {
            id: "array.zero_crossings",
            display: "Zero Crossings",
            group: EDGES,
            description: "1 where the array changes sign between opposite neighbours with enough contrast (yapocis zcs), e.g. on a DoG or a second derivative.",
            inputs: vec![
                array_in("array", ARRAY_TIP),
                float_in("threshold", 10.0, 0.0, 1e9, "Minimum squared difference across the pixel, in millionths (10 = yapocis's default 1e-5)"),
            ],
            outputs: vec![array_out("crossings", "1 on zero crossings, 0 elsewhere")],
            run: zero_crossings,
        }),
        Box::new(Op {
            id: "array.segment_edges",
            display: "Segment Edges",
            group: EDGES,
            description: "Exact boundaries of a segmentation: 1 where a pixel's segment differs from the pixel to its right or below (single-pixel lines, none missed), 0 elsewhere: the same form as DoG Sketch's sketch. Takes a label map or a colour-coded segmentation such as SAM2's segmented_image (through Image to Array).",
            inputs: vec![
                array_in("segments", "Labels, or a stack where each segment has its own colour"),
                float_in("tolerance", 0.0, 0.0, 1.0, "Largest per-channel difference still counted as the same segment (0: any change is an edge)"),
                bool_in("frame", true, "Also mark the border, as vsketch does"),
            ],
            outputs: vec![array_out("edges", "1 on boundaries, 0 inside segments")],
            run: segment_edges,
        }),
        Box::new(Op {
            id: "array.trace_contours",
            display: "Trace Contours",
            group: EDGES,
            description: "The outline of a mask as line segments (yapocis truetrace's CPU path: an exact distance transform and marching squares): a table of x0, y0, x1, y1 rows (x = column, y = row), sorted by x0, y0, x1, y1, for Draw Shapes (lines) and the like.",
            inputs: vec![
                array_in("mask", "One channel; nonzero = inside"),
                float_in("offset", 0.0, 0.0, 100.0, "Trace this many pixels outside the mask's edge (0 = the edge)"),
            ],
            outputs: vec![array_out("segments", "Table of x0, y0, x1, y1")],
            run: trace_contours,
        }),
        Box::new(Op {
            id: "array.rotate",
            display: "Rotate",
            group: GEOMETRY,
            description: "Rotates about the centre, nearest-neighbour (yapocis_external rotate). Positive angles turn the picture counter-clockwise.",
            inputs: vec![
                array_in("array", ARRAY_TIP),
                float_in("degrees", 15.0, -360.0, 360.0, "The angle; positive turns counter-clockwise"),
                bool_in("expand", true, "Grow the array to hold the whole rotated picture (off: keep the size and crop)"),
                float_in("background", 0.0, -1e9, 1e9, "Value where nothing rotates in"),
            ],
            outputs: vec![array_out("rotated", "The rotated array")],
            run: rotate,
        }),
        Box::new(Op {
            id: "array.resize",
            display: "Resize",
            group: GEOMETRY,
            description: "Rescales a picture or any array, every channel the same way (core.resize.resize). The size is given by maxdim (the longest side), mindim (the shortest side), size (width and/or height; one left 0 keeps the aspect) or scale (a factor). Float channels are resampled as Pillow does in 32-bit float; labels and other integer arrays always take the nearest pixel, so they are never averaged. The colour tags are kept.",
            inputs: vec![
                array_in("array", "The picture or array to resize: 2-D, or channels last"),
                enum_in(
                    "by",
                    &["maxdim", "mindim", "size", "scale"],
                    "maxdim",
                    "How the size is given: maxdim makes the longest side dim, mindim the shortest; size takes target width and/or height; scale multiplies both sides",
                ),
                int_in("dim", 1600, 1.0, 16384.0, "The longest (maxdim) or shortest (mindim) side, in pixels"),
                int_in("target_width", 0, 0.0, 16384.0, "size: the width; 0 keeps the aspect from the height"),
                int_in("target_height", 0, 0.0, 16384.0, "size: the height; 0 keeps the aspect from the width"),
                float_in("scale", 0.5, 0.01, 16.0, "scale: the factor for both sides"),
                enum_in(
                    "resample",
                    &["lanczos", "bilinear", "nearest"],
                    "lanczos",
                    "The filter for float channels: lanczos (sharp, the default), bilinear (soft), or nearest (no new values). Integer arrays always take the nearest pixel",
                ),
                bool_in("shrink_only", true, "Never enlarge: a target larger than the array on either side leaves it its size"),
                bool_in(
                    "align",
                    true,
                    "Height and width multiples of 4, as Load Image aligns, so the kernels' work groups fit: a resized array is resized to them, one that keeps its size is cropped",
                ),
                bool_in("keep_range", true, "Clip each resampled channel to its own range, so lanczos's ringing can't take a 0-1 picture outside 0-1"),
            ],
            outputs: vec![array_out("resized", "The resized array")],
            run: resize_node,
        }),
    ]
}

// ---------- running a function on each channel ----------

/// One plane a function made: its height, width and values.
pub struct Plane {
    pub h: usize,
    pub w: usize,
    pub data: Vec<f32>,
}

impl Plane {
    fn same(h: usize, w: usize, data: Vec<f32>) -> Plane {
        Plane { h, w, data }
    }
}

/// An array a per-channel node takes: a picture (not a table) of a sane size.
fn picture(a: &Img, what: &str) -> Result<(), NodeError> {
    if a.table {
        return Err(config(format!("{what} is a table, not a picture")));
    }
    Img::check_size(a.h, a.w, a.c).map_err(|e| config(format!("{what}: {e}")))
}

/// The research software's `run_per_channel`: `f` on each channel (as float), the planes of each output
/// stacked; the outputs keep the input's colour space and names when `keep`.
fn per_channel(a: &Img, keep: bool, outputs: usize, mut f: impl FnMut(&[f32], usize, usize) -> Result<Vec<Plane>, String>) -> Result<Vec<Img>, NodeError> {
    picture(a, "array")?;
    let mut per_output: Vec<Vec<Plane>> = (0..outputs).map(|_| Vec::new()).collect();
    for k in 0..a.c {
        let made = f(&a.plane(k), a.h, a.w).map_err(op_error)?;
        if made.len() != outputs {
            return Err(op_error("a channel gave the wrong number of outputs"));
        }
        for (o, p) in per_output.iter_mut().zip(made) {
            o.push(p);
        }
    }
    per_output
        .into_iter()
        .map(|planes| {
            let (h, w) = planes.first().map(|p| (p.h, p.w)).unwrap_or((a.h, a.w));
            if planes.iter().any(|p| p.h != h || p.w != w || p.data.len() != h * w) {
                return Err(op_error("the channels came out different sizes"));
            }
            let data: Vec<Vec<f32>> = planes.into_iter().map(|p| p.data).collect();
            let out = Img::from_planes(h, w, &data);
            Ok(if keep { Img { space: a.space.clone(), names: a.names.clone(), ..out } } else { out })
        })
        .collect()
}

/// [`per_channel`] with one output of the input's size.
fn each(a: &Img, keep: bool, mut f: impl FnMut(&[f32], usize, usize) -> Vec<f32>) -> Result<Img, NodeError> {
    let mut outs = per_channel(a, keep, 1, |p, h, w| Ok(vec![Plane::same(h, w, f(p, h, w))]))?;
    outs.pop().ok_or_else(|| op_error("no output"))
}

// ---------- filters ----------

fn gaussian_blur(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    let scale = n(i, "scale", 1).clamp(0, 7) as usize;
    let weights = ladder::LADDER.get(scale).ok_or_else(|| config(format!("scale {scale}: 0 to 7")))?;
    Ok(one("blurred", each(&a, true, |p, h, w| kernels::gauss_image(p, h, w, weights))?))
}

fn median3x3(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    let iterations = n(i, "iterations", 1).clamp(0, 100) as usize;
    Ok(one("filtered", each(&a, true, |p, h, w| kernels::median(p, h, w, iterations))?))
}

fn direction(i: &Inputs) -> Result<String, NodeError> {
    let d = s(i, "direction", "both");
    if DIRECTIONS.contains(&d.as_str()) { Ok(d) } else { Err(config(format!("direction {d:?}: both, horizontal or vertical"))) }
}

fn convolve(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    let name = s(i, "kernel", "binomial5");
    let k = conv_kernels::kernel(&name).ok_or_else(|| config(format!("kernel {name:?}: not one of yapocis's named kernels")))?;
    let d = direction(i)?;
    Ok(one("convolved", each(&a, true, |p, h, w| kernels::convolve_rows(p, h, w, &k, &d))?))
}

/// Custom Convolve's weights: an odd count of finite numbers (at most 1023), normalised by
/// their sum (Python's, in float64) when asked and it isn't 0, as float32.
pub fn custom_weights(text: &str, normalize: bool) -> Result<Vec<f32>, String> {
    let values: Vec<f64> =
        text.replace(',', " ").split_whitespace().map(|t| t.parse::<f64>().map_err(|_| format!("weights: {t:?} is not a number"))).collect::<Result<_, _>>()?;
    if values.is_empty() || values.len().is_multiple_of(2) {
        return Err(format!("weights: give an odd number of values, got {}", values.len()));
    }
    if values.len() > 1023 {
        return Err(format!("weights: at most 1023 values, got {}", values.len()));
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Err("weights: every weight must be a finite number".into());
    }
    let sum: f64 = values.iter().sum();
    let values: Vec<f64> = if normalize && sum != 0.0 { values.iter().map(|v| v / sum).collect() } else { values };
    Ok(values.iter().map(|v| *v as f32).collect())
}

fn custom_convolve(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    let k = custom_weights(&s(i, "weights", "1, 4, 6, 4, 1"), b(i, "normalize", true)).map_err(config)?;
    let d = direction(i)?;
    Ok(one("convolved", each(&a, true, |p, h, w| kernels::convolve_rows(p, h, w, &k, &d))?))
}

fn perona_malik(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    let iterations = n(i, "iterations", 8).clamp(0, 500) as usize;
    let kappa = f(i, "kappa", 3.0).clamp(0.01, 256.0);
    let step = f(i, "step", 0.2).clamp(0.01, 0.25);
    let boost = f(i, "boost", 0.5).clamp(0.05, 0.9);
    let as_is = match s(i, "input", "map to 0-1").as_str() {
        "map to 0-1" => false,
        "as is" => true,
        other => return Err(config(format!("input {other:?}: map to 0-1 or as is"))),
    };
    let edges = img_opt(i, "edges")?;
    if let Some(e) = &edges {
        if as_is {
            return Err(config("edges need input 'map to 0-1'"));
        }
        if e.table || e.c != 1 {
            return Err(config(format!("edges must be one channel, got {} (use Split or Pick Channel)", e.describe())));
        }
        if (e.h, e.w) != (a.h, a.w) {
            return Err(config(format!("edges ({}×{}) and array ({}×{}) differ in size", e.h, e.w, a.h, a.w)));
        }
    }
    let mask = edges.map(|e| e.data);
    let out = each(&a, true, |p, h, w| {
        if as_is {
            kernels::anisodiff_n(p, h, w, iterations, kappa, step)
        } else {
            kernels::perona_malik(p, h, w, iterations, kappa, step, mask.as_deref().map(|m| (m, boost)))
        }
    })?;
    Ok(one("diffused", out))
}

// ---------- edges ----------

fn angle_factor(i: &Inputs) -> Result<f32, NodeError> {
    let unit = s(i, "angle_unit", "degrees");
    ANGLE_UNITS.iter().find(|(u, _)| *u == unit).map(|(_, f)| *f).ok_or_else(|| config(format!("angle_unit {unit:?}: turns, degrees, radians or π units")))
}

fn gradient(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    let reach = n(i, "reach", 3).clamp(1, 64) as usize;
    let factor = angle_factor(i)?;
    let mut outs = per_channel(&a, false, 2, |p, h, w| {
        let (g, t) = kernels::gradient(p, h, w, reach);
        Ok(vec![Plane::same(h, w, g), Plane::same(h, w, t.iter().map(|v| v * factor).collect())])
    })?
    .into_iter();
    let (Some(mag), Some(angle)) = (outs.next(), outs.next()) else { return Err(op_error("no output")) };
    Ok(many(vec![("magnitude", Data::Array(mag.to_array())), ("angle", Data::Array(angle.to_array()))]))
}

fn sobel_magnitude(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    Ok(one("magnitude", each(&a, false, kernels::sobel)?))
}

fn zero_crossings(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    // threshold / 1e6 is a Python float; numpy compares it with the float32 deltas as float32
    let threshold = (f(i, "threshold", 10.0).clamp(0.0, 1e9) / 1e6) as f32;
    Ok(one("crossings", each(&a, false, |p, h, w| kernels::zcs(p, h, w, threshold))?))
}

fn segment_edges(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "segments")?;
    picture(&a, "segments")?;
    let tolerance = f(i, "tolerance", 0.0).clamp(0.0, 1.0);
    let frame = b(i, "frame", true);
    Ok(one("edges", Img::plane_of(a.h, a.w, kernels::segment_edges(&a.data, a.h, a.w, a.c, tolerance, frame))))
}

fn trace_contours(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "mask")?;
    picture(&a, "mask")?;
    if a.c != 1 {
        return Err(config(format!("mask must be one channel, got {} (use Split or Pick Channel)", a.describe())));
    }
    let offset = f(i, "offset", 0.0).clamp(0.0, 100.0) as f32;
    let max = MAX_VALUES / 4;
    let segs = trace::trace(&a.data, a.h, a.w, offset, max).ok_or_else(|| op_error(format!("more than {max} segments")))?;
    let names = ["x0", "y0", "x1", "y1"].iter().map(|n| (*n).to_owned()).collect();
    let table = Img { h: segs.len(), w: 1, c: 4, data: segs.into_iter().flatten().collect(), names, table: true, ..Default::default() };
    Ok(one("segments", table))
}

// ---------- geometry ----------

fn rotate(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    let degrees = f(i, "degrees", 15.0).clamp(-360.0, 360.0);
    let expand = b(i, "expand", true);
    let background = f(i, "background", 0.0).clamp(-1e9, 1e9) as f32;
    picture(&a, "array")?;
    let (out_h, out_w) = if expand {
        let (fh, fw) = kernels::rotated_size(a.h, a.w, degrees);
        // a 1 × 1 array turned by a multiple of 90° can come out 0 a side: keep 1
        (fh.clamp(1.0, 1e9) as usize, fw.clamp(1.0, 1e9) as usize)
    } else {
        (a.h, a.w)
    };
    Img::check_size(out_h, out_w, a.c).map_err(|e| config(format!("rotated: {e}")))?;
    let mut outs = per_channel(&a, true, 1, |p, h, w| Ok(vec![Plane::same(out_h, out_w, kernels::rotate(p, h, w, out_h, out_w, degrees, background))]))?;
    Ok(one("rotated", outs.pop().ok_or_else(|| op_error("no output"))?))
}

fn resize_node(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    if a.table {
        return Err(config("resize takes a picture or an array, not a table"));
    }
    Img::check_size(a.h, a.w, a.c).map_err(|e| config(format!("array: {e}")))?;
    let target = resize::Target {
        by: s(i, "by", "maxdim"),
        dim: n(i, "dim", 1600).clamp(1, 16384),
        width: n(i, "target_width", 0).clamp(0, 16384),
        height: n(i, "target_height", 0).clamp(0, 16384),
        scale: f(i, "scale", 0.5).clamp(0.01, 16.0),
        shrink_only: b(i, "shrink_only", true),
    };
    let filter = match s(i, "resample", "lanczos").as_str() {
        "lanczos" => Some(resize::Filter::Lanczos),
        "bilinear" => Some(resize::Filter::Bilinear),
        "nearest" => None,
        other => return Err(config(format!("resample {other:?}: lanczos, bilinear or nearest"))),
    };
    let (in_h, in_w) = (a.h, a.w);
    let (mut height, mut width) = resize::target_size(in_h, in_w, &target).map_err(config)?;
    let mut data = a.clone();
    if b(i, "align", true) {
        let m = resize::ALIGNMENT;
        if (height, width) == (in_h, in_w) {
            // an array smaller than the alignment keeps its size: cropping would empty it
            height = if in_h >= m { in_h - in_h % m } else { in_h };
            width = if in_w >= m { in_w - in_w % m } else { in_w };
            let mut rows = Vec::with_capacity(height * width * a.c);
            for y in 0..height {
                let start = y * in_w * a.c;
                rows.extend_from_slice(a.data.get(start..start + width * a.c).ok_or_else(|| op_error("the array is short"))?);
            }
            data = Img { h: height, w: width, data: rows, ..data };
        } else {
            height = m.max(height - height % m);
            width = m.max(width - width % m);
        }
    }
    Img::check_size(height, width, a.c).map_err(|e| config(format!("resized: {e}")))?;
    if (height, width) == (data.h, data.w) {
        return Ok(one("resized", data));
    }
    if a.labels {
        let out = resize::nearest(&data.data, data.h, data.w, data.c, height, width);
        return Ok(one("resized", Img { h: height, w: width, data: out, ..data }));
    }
    let keep_range = b(i, "keep_range", true);
    let mut outs = per_channel(&data, true, 1, |p, h, w| {
        let out = match filter {
            None => resize::nearest(p, h, w, 1, height, width),
            Some(filter) => {
                let mut out = resize::resample(p, h, w, height, width, filter);
                if keep_range {
                    let finite = p.iter().filter(|v| v.is_finite());
                    let (lo, hi) = finite.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
                    if lo <= hi {
                        for v in &mut out {
                            if *v < lo {
                                *v = lo;
                            } else if *v > hi {
                                *v = hi;
                            }
                        }
                    }
                }
                out
            }
        };
        Ok(vec![Plane::same(height, width, out)])
    })?;
    Ok(one("resized", outs.pop().ok_or_else(|| op_error("no output"))?))
}
