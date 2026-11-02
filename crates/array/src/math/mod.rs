//! Math (the research software's MATH category, `ops.py`, and `numpy_ops.py`'s set where): Arithmetic,
//! Clamp, Stretch, Binarize, Set Where, Normalize, Equalize, Histogram, Histogram Match, Apply
//! Curve, Matrix Multiply, Wrap Angle.
//!
//! The arithmetic is in [`kernels`] (yapocis's numba kernels and host code) and [`curves`]
//! (impressionist's tone curves). A node that the research software runs "per channel" applies to each
//! channel of a stack and refuses a table; whether the result keeps the array's colour space
//! and channel names follows the research software's `keep` (always, only for the 0-1 spaces RGB, HSI, IHSL
//! and YCrCb, or never).

pub mod curves;
pub mod kernels;

use plumcraft_engine::{Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::catalogue::Port;
use plumcraft_proto::error::NodeError;
use serde_json::json;

use crate::img::Img;
use crate::op::{Op, array_in, array_opt, array_out, config, enum_in, f, float_in, img, img_opt, int_in, n, one, s, string_in};
use kernels::Arith;

const ARRAY_TIP: &str = "An array: a channel (H × W) or a stack (H × W × C), 0-1";

/// The spaces whose values are 0-1, so a 0-1 result is still in them (the research software's UNIT_SPACES).
const UNIT_SPACES: &[&str] = &["RGB", "HSI", "IHSL", "YCrCb"];

/// What a result keeps of its input's tags (the research software's `keep`).
#[derive(Clone, Copy, PartialEq)]
enum Keep {
    All,
    Unit,
    Nothing,
}

/// `out` with `like`'s colour space and names when `keep` says so and the channel count is the
/// same (the research software's `plain`).
fn tagged(out: Img, like: &Img, keep: Keep) -> Img {
    let keep = match keep {
        Keep::All => true,
        Keep::Unit => like.space.as_deref().is_some_and(|s| UNIT_SPACES.contains(&s)),
        Keep::Nothing => false,
    };
    if keep && !like.table && out.c == like.c {
        Img { space: like.space.clone(), names: like.names.clone(), ..out }
    } else {
        Img { space: None, names: Vec::new(), ..out }
    }
}

/// A picture input for a per-channel node: not a table.
fn picture(i: &Inputs, name: &str) -> Result<Img, NodeError> {
    let a = img(i, name)?;
    if a.table {
        return Err(config(format!("{name} is a table, not a picture")));
    }
    Ok(a)
}

/// `f` on each channel of `a`, the result tagged as `keep` says.
fn per_channel(a: &Img, keep: Keep, f: impl FnMut(&[f32], usize, usize) -> Result<Vec<f32>, String>) -> Result<Img, NodeError> {
    let out = a.map_planes(f).map_err(config)?;
    Ok(tagged(Img { space: None, names: Vec::new(), ..out }, a, keep))
}

/// A float input, read only under some value of a choice.
fn float_when(name: &str, default: f64, min: f64, max: f64, tooltip: &str, choice: &str, value: &str) -> Port {
    Port { active_when: Some([(choice.to_owned(), vec![json!(value)])].into_iter().collect()), ..float_in(name, default, min, max, tooltip) }
}

pub fn nodes() -> Vec<Box<dyn Node>> {
    vec![
        Box::new(Op {
            id: "array.arithmetic",
            display: "Arithmetic",
            group: "Math",
            description: "a (operation) b elementwise (yapocis operators). b is an array (one channel applies to every channel of a), or the constant when b isn't connected. The result keeps a's colour tags: intensity × 1.2 is still intensity. Besides add, subtract, multiply and divide it does numpy's steps: subtract from (b - a: 1 - x with the constant 1), minimum and maximum.",
            inputs: vec![
                array_in("a", ARRAY_TIP),
                enum_in(
                    "operation",
                    &Arith::NAMES,
                    "multiply",
                    "a + b, a - b, a × b, a / b (yapocis divides by b + 1e-20), b - a, or the smaller or larger of a and b (numpy minimum and maximum: NaN wins)",
                ),
                float_in("constant", 1.0, -1e9, 1e9, "b when no array is connected"),
                array_opt("b", "Same size as a: one channel, or as many as a"),
            ],
            outputs: vec![array_out("result", "a (operation) b")],
            run: arithmetic,
        }),
        Box::new(Op {
            id: "array.clamp",
            display: "Clamp",
            group: "Math",
            description: "Limits values to low..high (yapocis clamp_pixels).",
            inputs: vec![
                array_in("array", ARRAY_TIP),
                float_in("low", 0.0, -1e9, 1e9, "The lowest value"),
                float_in("high", 1.0, -1e9, 1e9, "The highest value"),
            ],
            outputs: vec![array_out("clamped", "Values limited to low..high")],
            run: clamp,
        }),
        Box::new(Op {
            id: "array.stretch",
            display: "Stretch",
            group: "Math",
            description: "Maps values linearly from an input range to low..high, clipping outside it (yapocis stretch). Each channel of a stack is stretched over its own range in auto mode.",
            inputs: vec![
                array_in("array", ARRAY_TIP),
                enum_in(
                    "input_range",
                    &["auto", "percentile", "manual"],
                    "auto",
                    "auto: each channel's own min..max; percentile: ignoring percent of the values at each end; manual: input_low..input_high",
                ),
                float_when("percent", 1.0, 0.0, 49.0, "Ignore this percentage of values at each end (percentile)", "input_range", "percentile"),
                float_when("input_low", 0.0, -1e9, 1e9, "The bottom of the input range (manual)", "input_range", "manual"),
                float_when("input_high", 1.0, -1e9, 1e9, "The top of the input range (manual)", "input_range", "manual"),
                float_in("low", 0.0, -1e9, 1e9, "Output for the bottom of the input range"),
                float_in("high", 1.0, -1e9, 1e9, "Output for the top of the input range"),
            ],
            outputs: vec![array_out("stretched", "The stretched array")],
            run: stretch,
        }),
        Box::new(Op {
            id: "array.binarize",
            display: "Binarize",
            group: "Math",
            description: "1 where the value is at least the threshold, 0 elsewhere (yapocis binarize).",
            inputs: vec![array_in("array", ARRAY_TIP), float_in("threshold", 0.5, -1e9, 1e9, "Values at or above it become 1")],
            outputs: vec![array_out("binary", "1 or 0")],
            run: binarize,
        }),
        Box::new(Op {
            id: "array.set_where",
            display: "Set Where",
            group: "Math",
            description: "x with value wherever mask is 1 (mosaic's grid lines): numpy's x[np.where(mask == 1.0)] = value. A one-channel mask sets every channel of x.",
            inputs: vec![
                array_in("x", ARRAY_TIP),
                array_in("mask", "Same size as x: one channel, or as many as x; exactly 1 where x is set"),
                float_in("value", 1.0, -1e9, 1e9, "The value set"),
            ],
            outputs: vec![array_out("result", "x, set where mask is 1")],
            run: set_where,
        }),
        Box::new(Op {
            id: "array.normalize",
            display: "Normalize",
            group: "Math",
            description: "Values mapped from their min..max to 0..1 (yapocis normalize): each channel over its own range, or a whole stack over one range, keeping its colour balance (as the commands normalize the picture). A constant array becomes 0.",
            inputs: vec![
                array_in("array", ARRAY_TIP),
                enum_in("over", &["each channel", "whole array"], "each channel", "each channel: its own min..max; whole array: one min..max for the stack"),
            ],
            outputs: vec![array_out("normalized", "Values 0..1")],
            run: normalize,
        }),
        Box::new(Op {
            id: "array.equalize",
            display: "Equalize",
            group: "Math",
            description: "Histogram equalization of each channel (yapocis histeq): values spread evenly over 0-1. Black (bin 0) is left out of the histogram, as yapocis does.",
            inputs: vec![
                array_in("array", "Values in 0-1 (or choose stretch)"),
                enum_in(
                    "input",
                    &["0-1", "stretch", "as is"],
                    "0-1",
                    "0-1: values outside are clipped; stretch: each channel's own min..max first; as is (impressionist method): unchanged",
                ),
                int_in("bins", 65536, 16.0, 1_048_576.0, "Histogram bins"),
                enum_in(
                    "method",
                    &["yapocis", "impressionist"],
                    "yapocis",
                    "yapocis: histeq, black (bin 0) left out. impressionist: core.utils.histeq, the density histogram's cumulative distribution interpolated (sketch's equalized weight; use 256 bins and input as is)",
                ),
            ],
            outputs: vec![array_out("equalized", "Values spread over 0-1")],
            run: equalize,
        }),
        Box::new(Op {
            id: "array.histogram",
            display: "Histogram",
            group: "Math",
            description: "Counts of one channel's values in equal bins over low..high (yapocis histogram), as a table of bin centre and count; View Array plots it. Values outside low..high count in the end bins, as yapocis counts them.",
            inputs: vec![
                array_in("array", "One channel"),
                int_in("bins", 256, 2.0, 65536.0, "How many bins"),
                float_in("low", 0.0, -1e9, 1e9, "The bottom of the first bin"),
                float_in("high", 1.0, -1e9, 1e9, "The top of the last bin"),
            ],
            outputs: vec![array_out("histogram", "Table of bin centre, count")],
            run: histogram,
        }),
        Box::new(Op {
            id: "array.histogram_match",
            display: "Histogram Match",
            group: "Math",
            description: "Remaps each channel of `array` so its histogram matches the same channel of `target` (yapocis histmatch): borrow another picture's tonal distribution.",
            inputs: vec![
                array_in("array", "Values in 0-1"),
                array_in("target", "Values in 0-1; one channel, or as many as array"),
                int_in("bins", 256, 16.0, 65536.0, "Histogram bins"),
            ],
            outputs: vec![array_out("matched", "array with target's tones")],
            run: histogram_match,
        }),
        Box::new(Op {
            id: "array.apply_curve",
            display: "Apply Curve",
            group: "Math",
            description: "Tone curves (impressionist's core.curves, as the research software's curve editor draws them): an all-channels curve and one per channel (R, G, B, or the stack's own first three channels), applied in that order. Until PlumCraft has a curve editor, the curves are typed in as the research software stores them (see the curves tooltip).",
            inputs: vec![
                array_in("array", "Values in 0-1, or any range with input_range own range"),
                string_in(
                    "curves",
                    &curves::default_json(),
                    "The curves, as alexandria's ColorCurvesData JSON: {\"master\": {\"points\": [{\"x\": 0, \"y\": 0}, {\"x\": 1, \"y\": 1}], \"enabled\": true}, \"red\": {…}, \"green\": {…}, \"blue\": {…}}, points in 0..1 (a point may be [x, y]). Each channel goes through master, then its own. A missing curve is the identity; {\"points\": [[x, y], …]} (ComfyUI's curve widget) is the master curve. Smooth cubic segments through the points, flat beyond the first and last.",
                ),
                enum_in(
                    "input_range",
                    &["0-1", "own range"],
                    "0-1",
                    "0-1: values outside are clipped (RGB, HSI, ...). own range: each channel's min..max is mapped onto the curve's 0..1 and back (Lab's L, gradients, ...)",
                ),
            ],
            outputs: vec![array_out("curved", "The curved array")],
            run: apply_curve,
        }),
        Box::new(Op {
            id: "array.matrix_multiply",
            display: "Matrix Multiply",
            group: "Math",
            description: "A × B for tables (linalg matmul): A is M × K, B is K × N. A colour stack times a 3 × 3 matrix applies the matrix to every pixel.",
            inputs: vec![array_in("a", "A table, or a stack (pixels × channels)"), array_in("b", "A table K × N")],
            outputs: vec![array_out("product", "A × B: a table, or a stack of N channels")],
            run: matrix_multiply,
        }),
        Box::new(Op {
            id: "array.wrap_angle",
            display: "Wrap Angle",
            group: "Math",
            description: "Angles wrapped into one turn: signed (-½..½ turn, like yapocis compute_360_to_180) or unsigned (0..1 turn).",
            inputs: vec![
                array_in("array", ARRAY_TIP),
                float_in("turn", 360.0, 1e-6, 1e9, "One full turn: 360 degrees, 1 for HSI hue, 6.2832 radians, 2 for yapocis π units"),
                enum_in("range", &["signed", "unsigned"], "signed", "signed: -½..½ turn; unsigned: 0..1 turn"),
            ],
            outputs: vec![array_out("wrapped", "The wrapped angles")],
            run: wrap_angle,
        }),
    ]
}

/// A finite float parameter within the research software's widget range.
fn param(i: &Inputs, name: &str, default: f64, min: f64, max: f64) -> Result<f64, NodeError> {
    let v = f(i, name, default);
    if !(min..=max).contains(&v) {
        return Err(config(format!("{name}: {v} is outside {min}..{max}")));
    }
    Ok(v)
}

/// An integer parameter within range.
fn count(i: &Inputs, name: &str, default: i64, min: i64, max: i64) -> Result<usize, NodeError> {
    let v = n(i, name, default);
    if !(min..=max).contains(&v) {
        return Err(config(format!("{name}: {v} is outside {min}..{max}")));
    }
    usize::try_from(v).map_err(|_| config(format!("{name}: {v}")))
}

/// A choice parameter that must be one of `choices`.
fn choice(i: &Inputs, name: &str, choices: &[&str], default: &str) -> Result<String, NodeError> {
    let v = s(i, name, default);
    if choices.contains(&v.as_str()) { Ok(v) } else { Err(config(format!("{name}: {v:?} is not one of {}", choices.join(", ")))) }
}

/// The planes of `b` to pair with `a`'s: as many, or one repeated. Same height and width.
fn partner(a: &Img, b: &Img, what: &str) -> Result<Vec<Vec<f32>>, NodeError> {
    if (a.h, a.w) != (b.h, b.w) || a.table != b.table {
        return Err(config(format!("arrays differ in size: {} and {what} {}", a.describe(), b.describe())));
    }
    let planes = b.planes();
    match planes.as_slice() {
        [one] => Ok(vec![one.clone(); a.c]),
        _ if planes.len() == a.c => Ok(planes),
        _ => Err(config(format!("a has {} channels, {what} {}", a.c, b.c))),
    }
}

fn arithmetic(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "a")?;
    let name = choice(i, "operation", &Arith::NAMES, "multiply")?;
    let op = Arith::parse(&name).ok_or_else(|| config(format!("operation: {name:?}")))?;
    let constant = param(i, "constant", 1.0, -1e9, 1e9)? as f32;
    let (a, others) = match img_opt(i, "b")? {
        None => {
            let c = a.c;
            (a.clone(), vec![vec![constant; a.h * a.w]; c])
        }
        // a one-channel a with a stack b: a applies to every channel of b (numpy's a × b)
        Some(b) if a.c == 1 && b.c > 1 => {
            let others = b.planes();
            partner(&b, &a, "a")?;
            let spread =
                Img { data: a.plane(0).iter().flat_map(|v| std::iter::repeat_n(*v, b.c)).collect(), c: b.c, space: None, names: Vec::new(), ..a.clone() };
            (spread, others)
        }
        Some(b) => {
            let others = partner(&a, &b, "b")?;
            (a, others)
        }
    };
    let planes: Vec<Vec<f32>> = a.planes().iter().zip(&others).map(|(p, q)| p.iter().zip(q).map(|(x, y)| op.apply(*x, *y)).collect()).collect();
    let out = Img { table: a.table, ..Img::from_planes(a.h, a.w, &planes) };
    Ok(one("result", tagged(out, &a, Keep::All)))
}

fn clamp(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = picture(i, "array")?;
    let (low, high) = (param(i, "low", 0.0, -1e9, 1e9)?, param(i, "high", 1.0, -1e9, 1e9)?);
    Ok(one("clamped", per_channel(&a, Keep::All, |p, _, _| Ok(kernels::clamp_pixels(p, low, high)))?))
}

fn stretch(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = picture(i, "array")?;
    let mode = choice(i, "input_range", &["auto", "percentile", "manual"], "auto")?;
    let percent = param(i, "percent", 1.0, 0.0, 49.0)?;
    let (input_low, input_high) = (param(i, "input_low", 0.0, -1e9, 1e9)?, param(i, "input_high", 1.0, -1e9, 1e9)?);
    let (low, high) = (param(i, "low", 0.0, -1e9, 1e9)?, param(i, "high", 1.0, -1e9, 1e9)?);
    let out = per_channel(&a, Keep::Unit, |p, _, _| {
        let (lo, hi) = match mode.as_str() {
            "auto" => kernels::nan_min_max(p),
            "percentile" => match kernels::nan_percentiles(p, &[percent, 100.0 - percent]).as_slice() {
                [lo, hi] => (*lo, *hi),
                _ => (f64::NAN, f64::NAN),
            },
            _ => (input_low, input_high),
        };
        if hi <= lo {
            return Ok(vec![low as f32; p.len()]);
        }
        Ok(kernels::stretch(p, lo, hi, low, high))
    })?;
    Ok(one("stretched", out))
}

fn binarize(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = picture(i, "array")?;
    let threshold = param(i, "threshold", 0.5, -1e9, 1e9)?;
    Ok(one("binary", per_channel(&a, Keep::Nothing, |p, _, _| Ok(kernels::binarize(p, threshold)))?))
}

fn set_where(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let x = img(i, "x")?;
    let mask = img(i, "mask")?;
    let value = param(i, "value", 1.0, -1e9, 1e9)? as f32;
    let masks = partner(&x, &mask, "mask")?;
    let planes: Vec<Vec<f32>> =
        x.planes().iter().zip(&masks).map(|(p, m)| p.iter().zip(m).map(|(v, m)| if *m == 1.0 { value } else { *v }).collect()).collect();
    let out = Img { table: x.table, labels: x.labels, ..Img::from_planes(x.h, x.w, &planes) };
    Ok(one("result", tagged(out, &x, Keep::All)))
}

fn normalize(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    let over = choice(i, "over", &["each channel", "whole array"], "each channel")?;
    let out = if over == "whole array" {
        Img { data: kernels::normalize(&a.data), space: None, names: Vec::new(), labels: false, ..a.clone() }
    } else {
        let planes: Vec<Vec<f32>> = a.planes().iter().map(|p| kernels::normalize(p)).collect();
        Img { table: a.table, ..Img::from_planes(a.h, a.w, &planes) }
    };
    Ok(one("normalized", tagged(out, &a, Keep::Unit)))
}

fn equalize(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = picture(i, "array")?;
    let input = choice(i, "input", &["0-1", "stretch", "as is"], "0-1")?;
    let bins = count(i, "bins", 65536, 16, 1 << 20)?;
    let method = choice(i, "method", &["yapocis", "impressionist"], "yapocis")?;
    let out = per_channel(&a, Keep::Unit, |p, _, _| {
        if method == "impressionist" {
            let source = if input == "stretch" { kernels::stretched(p) } else { p.to_vec() };
            return kernels::histeq_impressionist(&source, bins);
        }
        let unit: Vec<f32> =
            if input == "stretch" { kernels::stretched(p) } else { p.iter().map(|&v| if v.is_nan() { v } else { v.clamp(0.0, 1.0) }).collect() };
        Ok(kernels::histeq(&unit, bins))
    })?;
    Ok(one("equalized", out))
}

fn histogram(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    if a.table || a.c != 1 {
        return Err(config(format!("array must be one channel, got {} (use Split or Pick Channel)", a.describe())));
    }
    let bins = count(i, "bins", 256, 2, 65536)?;
    let (low, high) = (param(i, "low", 0.0, -1e9, 1e9)?, param(i, "high", 1.0, -1e9, 1e9)?);
    if high <= low {
        return Err(config(format!("high ({high}) must be above low ({low})")));
    }
    let counts = kernels::histogram(&a.data, bins, low, high);
    let mut data = Vec::with_capacity(bins * 2);
    for (k, c) in counts.iter().enumerate() {
        data.push((low + (k as f64 + 0.5) * (high - low) / bins as f64) as f32);
        data.push(*c as f32);
    }
    let table = Img { h: bins, w: 1, c: 2, data, names: vec!["centre".into(), "count".into()], table: true, ..Default::default() };
    Ok(one("histogram", table))
}

fn histogram_match(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = picture(i, "array")?;
    let target = picture(i, "target")?;
    let bins = count(i, "bins", 256, 16, 65536)?;
    if target.c != 1 && target.c != a.c {
        return Err(config(format!("target has {} channels; give it one, or as many as array ({})", target.c, a.c)));
    }
    let targets = target.planes();
    let hists: Vec<Vec<f64>> = targets
        .iter()
        .map(|t| kernels::target_histogram(&t.iter().map(|&v| if v.is_nan() { v } else { v.clamp(0.0, 1.0) }).collect::<Vec<_>>(), bins))
        .collect();
    let mut k = 0usize;
    let out = per_channel(&a, Keep::Unit, |p, _, _| {
        let hist = hists.get(k.min(hists.len().saturating_sub(1))).ok_or("target has no channels")?;
        k += 1;
        let unit: Vec<f32> = p.iter().map(|&v| if v.is_nan() { v } else { v.clamp(0.0, 1.0) }).collect();
        Ok(kernels::histmatch(&unit, hist, bins))
    })?;
    Ok(one("matched", out))
}

fn apply_curve(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    if a.table {
        return Err(config("a table has no tones to curve"));
    }
    let set = curves::parse(&s(i, "curves", "")).map_err(config)?;
    let own_range = choice(i, "input_range", &["0-1", "own range"], "0-1")? == "own range";
    let planes = curves::curve_planes(&a.planes(), &set, own_range);
    let out = Img::from_planes(a.h, a.w, &planes);
    Ok(one("curved", tagged(out, &a, if own_range { Keep::All } else { Keep::Unit })))
}

/// The largest matrix product computed (multiply-adds).
const MAX_PRODUCT_WORK: usize = 1 << 32;

fn matrix_multiply(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "a")?;
    let b = img(i, "b")?;
    // B: a table (K rows of N columns) or a plain 2-D array (K × N)
    let (kb, nb) = if b.table {
        (b.h, b.c)
    } else if b.c == 1 {
        (b.h, b.w)
    } else {
        return Err(config(format!("b must be a table K × N, got {}", b.describe())));
    };
    let (m, k) = if a.table { (a.h, a.c) } else { (a.h * a.w, a.c) };
    if k != kb {
        return Err(config(format!("A has {k} columns but B has {kb} rows ({} × {})", a.describe(), b.describe())));
    }
    if m.saturating_mul(k).saturating_mul(nb) > MAX_PRODUCT_WORK {
        return Err(config(format!("{m} × {k} × {nb} is too large a product")));
    }
    let product = kernels::matmul(&a.data, &b.data, m, k, nb);
    let out = if a.table {
        Img::check_size(m, 1, nb).map_err(config)?;
        Img { h: m, w: 1, c: nb, data: product, table: true, ..Default::default() }
    } else {
        Img::check_size(a.h, a.w, nb).map_err(config)?;
        Img { h: a.h, w: a.w, c: nb, data: product, ..Default::default() }
    };
    Ok(Outputs::from([("product".to_owned(), Data::Array(out.to_array()))]))
}

fn wrap_angle(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = picture(i, "array")?;
    let turn = param(i, "turn", 360.0, 1e-6, 1e9)?;
    let signed = choice(i, "range", &["signed", "unsigned"], "signed")? == "signed";
    Ok(one("wrapped", per_channel(&a, Keep::All, |p, _, _| Ok(kernels::wrap_angle(p, turn, signed)))?))
}
