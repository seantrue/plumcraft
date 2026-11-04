//! Colour (the research software's Convert Color Space and Color Matrix): conversions between the ten spaces
//! impressionist knows, through RGB ([`plumcraft_engine::color`]), and yapocis colormath's 49
//! named 3 × 3 matrices ([`matrices`]). XYZ and xyY take any of colormath's RGB working
//! spaces, not only sRGB D65.

pub mod matrices;

use plumcraft_engine::{Cx, Inputs, Node, Outputs};
use plumcraft_proto::error::NodeError;

use crate::channels::channel_names;
use crate::img::Img;
use crate::op::{Op, array_in, array_out, b, bool_in, config, enum_in, img, one, s};

const SPACES: &[&str] = &["RGB", "HSI", "IHSL", "HSV", "HLS", "Lab", "Luv", "YCrCb", "XYZ", "xyY"];

pub fn nodes() -> Vec<Box<dyn Node>> {
    let names: Vec<&str> = matrices::MATRICES.iter().map(|(n, _)| *n).collect();
    let working: Vec<&str> = names.iter().filter_map(|n| n.strip_suffix(".rgb2xyz")).collect();
    let sources: Vec<&str> = std::iter::once("auto").chain(SPACES.iter().copied()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    let mut working_sorted = working.clone();
    working_sorted.sort_unstable();
    vec![
        Box::new(Op {
            id: "array.convert_color_space",
            display: "Convert Color Space",
            group: "Color",
            description: "A three-channel stack from one colour space to another (through RGB). RGB: red, green, blue; HSI: hue, saturation, intensity; IHSL: hue, saturation, luminance; HSV: hue, saturation, value; HLS: hue, lightness, saturation; Lab: L, a, b; Luv: L, u, v; YCrCb: Y, Cr, Cb; XYZ: X, Y, Z; xyY: x, y, Y. Ranges: RGB, HSI, IHSL and YCrCb 0-1 (HSI and IHSL hue a fraction of a turn); HSV and HLS hue 0-360, other channels 0-1; Lab and Luv L 0-100, chroma about -128..127; XYZ and xyY Y 0-1 for white.",
            inputs: vec![
                array_in("array", "Three channels"),
                enum_in("target", SPACES, "HSI", "The space to convert to"),
                enum_in("source", &sources, "auto", "The space the array is in: auto reads it from the array (an array without a space is taken as RGB)"),
                crate::op::advanced(enum_in(
                    "working_space",
                    &working_sorted,
                    "srgb_d65",
                    "RGB working space (primaries and white point) for XYZ and xyY; ignored by the other spaces",
                )),
                crate::op::advanced(bool_in(
                    "linearize",
                    true,
                    "For XYZ and xyY: decode sRGB before the matrix, as for ordinary images (off: use the values as they are)",
                )),
            ],
            outputs: vec![array_out("converted", "The stack in the target space")],
            run: convert,
        }),
        Box::new(Op {
            id: "array.color_matrix",
            display: "Color Matrix",
            group: "Color",
            description: "A 3 × 3 colour matrix applied to every pixel, as is (yapocis colormath transform; no gamma). Convert Color Space does XYZ and xyY properly, decoding sRGB first; this is the raw matrix, for linear data.",
            inputs: vec![array_in("array", "Three channels"), enum_in("transform", &sorted, "srgb_d65.rgb2xyz", "The named matrix")],
            outputs: vec![array_out("transformed", "The transformed stack")],
            run: color_matrix,
        }),
    ]
}

fn three(a: &Img) -> Result<(), NodeError> {
    if a.c != 3 || a.table {
        return Err(config(format!("needs 3 channels, got {}", a.describe())));
    }
    Ok(())
}

/// A named matrix, stored column by column.
fn named(name: &str) -> Option<&'static [f32; 9]> {
    matrices::MATRICES.iter().find(|(n, _)| *n == name).map(|(_, m)| m)
}

/// colormath's `transform` kernel on one pixel: the matrix is stored column by column.
fn apply(m: &[f32; 9], [x, y, z]: [f32; 3]) -> [f32; 3] {
    [m[0] * x + m[3] * y + m[6] * z, m[1] * x + m[4] * y + m[7] * z, m[2] * x + m[5] * y + m[8] * z]
}

fn color_matrix(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    three(&a)?;
    let name = s(i, "transform", "srgb_d65.rgb2xyz");
    let m = named(&name).ok_or_else(|| config(format!("transform: no matrix {name:?}")))?;
    let data = a.data.as_chunks::<3>().0.iter().flat_map(|p| apply(m, *p)).collect();
    Ok(one("transformed", Img { data, space: None, names: Vec::new(), labels: false, ..a }))
}

/// numpy `clip(v, 0, None)` (npexact): NaN stays NaN, and -0 becomes +0.
fn clip0(v: f32) -> f32 {
    npexact::elementwise::clip_min_f32(v, 0.0)
}

fn srgb_decode(v: f32) -> f32 {
    let v = clip0(v);
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn srgb_encode(v: f32) -> f32 {
    let v = clip0(v);
    if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf((1.0f64 / 2.4) as f32) - 0.055 }
}

/// RGB planes -> XYZ or xyY in `working`, as impressionist's `colorspaces.from_rgb`.
fn rgb_to(space: &str, working: &str, linearize: bool, planes: &[Vec<f32>]) -> Option<Vec<Vec<f32>>> {
    let m = named(&format!("{working}.rgb2xyz"))?;
    let n = planes.first()?.len();
    let mut out = vec![Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n)];
    for i in 0..n {
        let rgb = [planes.first()?.get(i).copied()?, planes.get(1)?.get(i).copied()?, planes.get(2)?.get(i).copied()?];
        let rgb = if linearize { rgb.map(srgb_decode) } else { rgb };
        let [x, y, z] = apply(m, rgb);
        let v = if space == "xyY" {
            let sum = x + y + z;
            [x / sum, y / sum, y]
        } else {
            [x, y, z]
        };
        for (o, c) in out.iter_mut().zip(v) {
            o.push(c);
        }
    }
    Some(out)
}

/// XYZ or xyY planes in `working` -> RGB, as impressionist's `colorspaces.to_rgb`.
fn to_rgb_from(space: &str, working: &str, linearize: bool, planes: &[Vec<f32>]) -> Option<Vec<Vec<f32>>> {
    let m = named(&format!("{working}.xyz2rgb"))?;
    let n = planes.first()?.len();
    let mut out = vec![Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n)];
    for i in 0..n {
        let v = [planes.first()?.get(i).copied()?, planes.get(1)?.get(i).copied()?, planes.get(2)?.get(i).copied()?];
        let xyz = if space == "xyY" {
            let [x, y, big_y] = v;
            if y == 0.0 { [0.0, 0.0, 0.0] } else { [x * big_y / y, big_y, ((1.0 - x - y) * big_y) / y] }
        } else {
            v
        };
        let rgb = apply(m, xyz);
        let rgb = if linearize { rgb.map(srgb_encode) } else { rgb };
        for (o, c) in out.iter_mut().zip(rgb) {
            o.push(c);
        }
    }
    Some(out)
}

/// Planes in `source` -> planes in `target`, through RGB.
pub fn convert_planes(source: &str, target: &str, working: &str, linearize: bool, planes: &[Vec<f32>]) -> Result<Vec<Vec<f32>>, String> {
    let unknown = |s: &str| format!("unknown colour space {s:?}; use one of {}", SPACES.join(", "));
    let colorimetric = |s: &str| s == "XYZ" || s == "xyY";
    let rgb = if colorimetric(source) {
        to_rgb_from(source, working, linearize, planes).ok_or_else(|| format!("no working space {working:?}"))?
    } else {
        plumcraft_engine::color::to_rgb(source, planes).ok_or_else(|| unknown(source))?
    };
    if colorimetric(target) {
        rgb_to(target, working, linearize, &rgb).ok_or_else(|| format!("no working space {working:?}"))
    } else {
        plumcraft_engine::color::from_rgb(target, &rgb).ok_or_else(|| unknown(target))
    }
}

fn convert(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    if a.c != 3 || a.table {
        return Err(config(format!("colour conversion needs 3 channels, got {}", a.describe())));
    }
    let source = match s(i, "source", "auto").as_str() {
        "auto" => a.space.clone().unwrap_or_else(|| "RGB".into()),
        named => named.to_owned(),
    };
    let target = s(i, "target", "HSI");
    let planes = convert_planes(&source, &target, &s(i, "working_space", "srgb_d65"), b(i, "linearize", true), &a.planes()).map_err(config)?;
    let names = channel_names(&target).unwrap_or_default();
    Ok(one("converted", Img { space: Some(target), names, labels: false, ..Img::from_planes(a.h, a.w, &planes) }))
}
