//! Files, channels and stacks (the research software's Arrays and Utilities groups): Load Image, Encode
//! Image, Split, Pick Channel, Stack, Constant, Frame, Crop to Multiple.
//!
//! Picture files are read and written with the `image` crate. Encode Image makes an encoded
//! picture (an `IMAGE` blob) that Write Files saves, so file naming and overwrite rules are the
//! building blocks', as the research software's Save Image did them itself.

use std::sync::Arc;

use plumcraft_engine::{Blob, BlobSource, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::Handle;
use plumcraft_proto::data::{BlobMeta, Form};
use plumcraft_proto::error::NodeError;

use crate::img::Img;

mod contours;
mod svg;
use crate::op::{
    Op, array_in, array_opt, array_out, array_slots, b, bool_in, config, enum_in, f, float_in, img, img_opt, int_in, many, n, one, op_error, out, path_in, s,
    slots,
};

/// The encoded-picture wire type Encode Image makes: the core IMAGE type.
pub const IMAGE: &str = plumcraft_proto::catalogue::core_types::IMAGE;

/// Colour spaces and their channel names (impressionist's `colorspaces.CHANNELS`).
pub const CHANNELS: &[(&str, [&str; 3])] = &[
    ("RGB", ["red", "green", "blue"]),
    ("HSI", ["hue", "saturation", "intensity"]),
    ("IHSL", ["hue", "saturation", "luminance"]),
    ("HSV", ["hue", "saturation", "value"]),
    ("HLS", ["hue", "lightness", "saturation"]),
    ("Lab", ["L", "a", "b"]),
    ("Luv", ["L", "u", "v"]),
    ("YCrCb", ["Y", "Cr", "Cb"]),
    ("XYZ", ["X", "Y", "Z"]),
    ("xyY", ["x", "y", "Y"]),
];

/// The channel names of `space`.
pub fn channel_names(space: &str) -> Option<Vec<String>> {
    CHANNELS.iter().find(|(s, _)| *s == space).map(|(_, n)| n.iter().map(|x| (*x).to_owned()).collect())
}

/// The space whose channel names these are.
pub fn guess_space(names: &[String]) -> Option<String> {
    CHANNELS.iter().find(|(_, n)| n.iter().zip(names).all(|(a, b)| a == b) && names.len() == 3).map(|(s, _)| (*s).to_owned())
}

const BACKGROUNDS: &[(&str, f32)] = &[("black", 0.0), ("white", 1.0), ("gray", 0.5)];
/// The largest picture file read.
const MAX_FILE: u64 = 1 << 30;

pub fn nodes() -> Vec<Box<dyn Node>> {
    let mut v: Vec<Box<dyn Node>> = vec![
        Box::new(Op {
            id: "array.load_image",
            display: "Load Image",
            group: "Arrays",
            description: "A picture file as an RGB array (H × W × 3, 0-1), the start of an array flow. A picture with transparency is flattened over the background colour, and its alpha comes out separately; rgba gives the colours under the alpha with the alpha as a fourth channel (all 1 when the picture has none). With a picture wired into image, that picture is used instead of the file.",
            inputs: vec![
                array_opt("image", "A picture to use instead of the file: RGB, RGBA (alpha last, as rgba gives it), or one channel as grey"),
                path_in("path", "The picture file (PNG, JPEG, TIFF); not read when image is wired"),
                bool_in("align", true, "Crop to multiples of 4 (from the top left), as the research software's commands crop their images"),
                enum_in(
                    "background",
                    &["black", "white", "gray", "as is"],
                    "black",
                    "What transparent pixels become in rgb: composited over black, white or gray, or as is (the colours under the alpha). No effect on pictures without transparency, or on rgba",
                ),
            ],
            outputs: vec![
                array_out("rgb", "Red, green and blue, 0-1"),
                array_out("alpha", "Opacity, 1 = opaque (all 1 for a picture without transparency)"),
                array_out("rgba", "Red, green, blue and alpha, 0-1: the colours under the alpha, not composited"),
            ],
            run: load_image,
        }),
        Box::new(Op {
            id: "array.decode_image",
            display: "Decode Image",
            group: "Arrays",
            description: "An encoded picture (an IMAGE value: PNG, JPEG or TIFF bytes, from Encode Image or another engine's export) as arrays, as Load Image reads a file: rgb (H × W × 3, 0-1) flattened over the background, alpha, and rgba. How a picture made by another engine comes back into an array flow.",
            inputs: vec![
                plumcraft_proto::catalogue::Port {
                    name: "image".into(),
                    ty: IMAGE.into(),
                    socket: Some(plumcraft_proto::catalogue::Socket::Socket),
                    tooltip: Some("The encoded picture".into()),
                    ..Default::default()
                },
                bool_in("align", false, "Crop to multiples of 4 (from the top left), as the research software's commands crop their images"),
                enum_in(
                    "background",
                    &["black", "white", "gray", "as is"],
                    "black",
                    "What transparent pixels become in rgb: composited over black, white or gray, or as is (the colours under the alpha)",
                ),
            ],
            outputs: vec![
                array_out("rgb", "Red, green and blue, 0-1"),
                array_out("alpha", "Opacity, 1 = opaque (all 1 for a picture without transparency)"),
                array_out("rgba", "Red, green, blue and alpha, 0-1: the colours under the alpha, not composited"),
            ],
            run: decode_image,
        }),
        Box::new(Op {
            id: "array.encode_image",
            display: "Encode Image",
            group: "Arrays",
            description: "A picture (RGB, RGBA or one channel, 0-1) as a PNG, JPEG or TIFF file's bytes, for Write Files to save. Values are clipped to 0-1 and turned to 8 bits (truncated as the research software's site does, or rounded).",
            inputs: vec![
                array_in("image", "The picture: RGB, RGBA, or one channel (grey)"),
                enum_in("format", &["png", "jpeg", "tiff"], "png", "The file format"),
                enum_in(
                    "conversion",
                    &["as the site", "round"],
                    "as the site",
                    "0-1 to 8 bits: as the site (times 255, truncated) or rounded to the nearest level",
                ),
                int_in("quality", 95, 1.0, 100.0, "JPEG quality"),
            ],
            outputs: vec![out("image", IMAGE, "The encoded picture")],
            run: encode_image,
        }),
        Box::new(Op {
            id: "array.split",
            display: "Split",
            group: "Arrays",
            description: "A stack's channels as separate one-channel arrays, each keeping its name and colour space, so Stack can put them back together in that space.",
            inputs: vec![array_in("array", "A stack of up to 4 channels")],
            outputs: (1..=4).map(|i| array_out(&format!("c{i}"), &format!("Channel {i} (an error downstream when there are fewer)"))).collect(),
            run: split,
        }),
        Box::new(Op {
            id: "array.pick_channel",
            display: "Pick Channel",
            group: "Arrays",
            description: "One channel of a stack, by number (1 = first), keeping its name and colour space.",
            inputs: vec![array_in("array", "A stack"), int_in("channel", 1, 1.0, 64.0, "1 = the first channel")],
            outputs: vec![array_out("channel", "The channel")],
            run: pick_channel,
        }),
        Box::new(Op {
            id: "array.stack",
            display: "Stack",
            group: "Arrays",
            description: "Channels (and stacks) as one stack, in order. space auto recognises channels split from one colour space (hue, saturation, intensity -> HSI); naming a space instead labels the result as that space without converting it.",
            inputs: vec![
                array_slots("c", "A channel (H × W) or a stack (H × W × C); stacks are concatenated", 1, 16),
                enum_in(
                    "space",
                    &["auto", "none", "RGB", "HSI", "IHSL", "HSV", "HLS", "Lab", "Luv", "YCrCb", "XYZ", "xyY"],
                    "auto",
                    "The stack's colour space: auto from its channels, none, or a named space (needs 3 channels)",
                ),
            ],
            outputs: vec![array_out("stack", "The stack")],
            run: stack,
        }),
        Box::new(Op {
            id: "array.constant",
            display: "Constant",
            group: "Utilities",
            description: "An array filled with one value, the size of like or of width × height.",
            inputs: vec![
                float_in("value", 0.0, -1e9, 1e9, "The value"),
                int_in("channels", 1, 1.0, 8.0, "How many channels the array has (1 grey, 3 colour); ignored when like is connected"),
                int_in("width", 512, 1.0, 16384.0, "Used when like isn't connected"),
                int_in("height", 512, 1.0, 16384.0, "Used when like isn't connected"),
                array_opt("like", "Take the size from this array"),
            ],
            outputs: vec![array_out("constant", "The array")],
            run: constant,
        }),
        Box::new(Op {
            id: "array.frame",
            display: "Frame",
            group: "Utilities",
            description: "Sets a margin around the array to one value and its outermost pixels to another (yapocis set_frame), as the commands seal edge maps before labelling regions. Each channel of a stack is framed.",
            inputs: vec![
                array_in("array", "An array"),
                int_in("margin", 0, 0.0, 4096.0, "Width in pixels of the band set to margin_value (0 = none)"),
                float_in("margin_value", 0.0, -1e9, 1e9, "The margin's value"),
                float_in("frame_value", 1.0, -1e9, 1e9, "Value of the outermost pixels"),
            ],
            outputs: vec![array_out("framed", "The framed array")],
            run: frame,
        }),
        Box::new(Op {
            id: "array.crop_to_multiple",
            display: "Crop to Multiple",
            group: "Utilities",
            description: "Crops height and width down to multiples of a number (yapocis align_image), as Load Image does with 4.",
            inputs: vec![array_in("array", "An array"), int_in("multiple", 4, 1.0, 1024.0, "The multiple")],
            outputs: vec![array_out("cropped", "The cropped array")],
            run: crop_to_multiple,
        }),
    ];
    // SVG beside the encoded pictures: after Load, Decode and Encode Image
    let at = v.iter().position(|n| n.def().id == "array.encode_image").map_or(v.len(), |i| i + 1);
    v.splice(at..at, svg::nodes().into_iter().chain(contours::nodes()));
    v
}

/// The top-left `h - h % m` × `w - w % m` of an array.
pub fn aligned(a: &Img, m: usize) -> Result<Img, String> {
    let m = m.max(1);
    let (h, w) = (a.h - a.h % m, a.w - a.w % m);
    Img::check_size(h, w, a.c).map_err(|e| format!("cropping to multiples of {m}: {e}"))?;
    let mut data = Vec::with_capacity(h * w * a.c);
    for y in 0..h {
        let row = y * a.w * a.c;
        data.extend_from_slice(a.data.get(row..row + w * a.c).ok_or("the array is short")?);
    }
    Ok(Img { h, w, data, ..a.clone() })
}

fn load_image(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let pixels = match img_opt(i, "image")? {
        Some(p) => wired_pixels(p)?,
        None => read_picture(&s(i, "path", ""))?,
    };
    picture_outputs(pixels, b(i, "align", true), &s(i, "background", "black"))
}

fn decode_image(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let blob = i.get("image").and_then(Data::blob).ok_or_else(|| config("image: wire an encoded picture (Encode Image, or another engine's PNG)"))?;
    let bytes = match &blob.source {
        BlobSource::Bytes(b) => b.to_vec(),
        BlobSource::File(p) => {
            let len = std::fs::metadata(p).map_err(|e| NodeError::new(NodeError::MISSING_SOURCE, format!("image: {}: {e}", p.display())))?.len();
            if len > MAX_FILE {
                return Err(config("image: larger than 1 GiB"));
            }
            std::fs::read(p).map_err(|e| NodeError::new(NodeError::MISSING_SOURCE, format!("image: {}: {e}", p.display())))?
        }
    };
    let pixels = decode(&bytes).map_err(|e| config(format!("image: {} isn't a picture this engine reads ({e})", blob.meta.media)))?;
    picture_outputs(pixels, b(i, "align", false), &s(i, "background", "black"))
}

/// Load Image's and Decode Image's outputs from decoded pixels.
fn picture_outputs(pixels: Img, align: bool, background: &str) -> Result<Outputs, NodeError> {
    let pixels = if align { aligned(&pixels, 4).map_err(config)? } else { pixels };
    let (h, w) = (pixels.h, pixels.w);
    let planes = pixels.planes();
    let alpha = if pixels.c == 4 { planes.get(3).cloned().unwrap_or_default() } else { vec![1.0; h * w] };
    let straight: Vec<Vec<f32>> = planes.iter().take(3).cloned().collect();
    let background = BACKGROUNDS.iter().find(|(name, _)| *name == background).map(|(_, v)| *v);
    let rgb: Vec<Vec<f32>> = match background {
        Some(bg) if pixels.c == 4 => straight.iter().map(|p| p.iter().zip(&alpha).map(|(v, a)| v * a + bg * (1.0 - a)).collect()).collect(),
        _ => straight.clone(),
    };
    let names = |n: &[&str]| n.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
    let rgb = Img { space: Some("RGB".into()), names: names(&["red", "green", "blue"]), ..Img::from_planes(h, w, &rgb) };
    let a = Img { names: names(&["alpha"]), ..Img::plane_of(h, w, alpha.clone()) };
    let mut four = straight;
    four.push(alpha);
    let rgba = Img { names: names(&["red", "green", "blue", "alpha"]), ..Img::from_planes(h, w, &four) };
    Ok(many(vec![("rgb", Data::Array(rgb.to_array())), ("alpha", Data::Array(a.to_array())), ("rgba", Data::Array(rgba.to_array()))]))
}

/// A picture wired into Load Image as (H, W, 3) or (H, W, 4): the values unchanged.
fn wired_pixels(p: Img) -> Result<Img, NodeError> {
    if p.table || p.labels {
        return Err(config("image takes a picture, not a table or labels"));
    }
    match (p.c, p.space.as_deref()) {
        (1, _) => {
            let g = p.plane(0);
            Ok(Img::from_planes(p.h, p.w, &[g.clone(), g.clone(), g]))
        }
        (3, Some("RGB") | None) => Ok(p),
        (4, None) if p.names.get(3).is_some_and(|n| n == "alpha") => Ok(p),
        (c, space) => Err(config(format!(
            "image takes an RGB or RGBA picture or one channel, not {c} channels{} (convert it to RGB first)",
            space.map(|s| format!(" of {s}")).unwrap_or_default()
        ))),
    }
}

/// A picture file as (H, W, 3) or, with transparency, (H, W, 4) pixels, 0-1.
fn read_picture(path: &str) -> Result<Img, NodeError> {
    let path = path.trim();
    if path.is_empty() {
        return Err(config("path: choose a picture file"));
    }
    let p = std::path::Path::new(path);
    let meta = std::fs::metadata(p).map_err(|e| NodeError::new(NodeError::MISSING_SOURCE, format!("{path}: {e}")))?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err(NodeError::new(NodeError::MISSING_SOURCE, format!("{path}: not a picture file of at most 1 GiB")));
    }
    let bytes = std::fs::read(p).map_err(|e| op_error(format!("{path}: {e}")))?;
    decode(&bytes).map_err(|e| op_error(format!("{path}: {e}")))
}

/// Decode a picture's bytes (PNG, JPEG, TIFF), bounded.
pub fn decode(bytes: &[u8]) -> Result<Img, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(crate::img::MAX_SIDE as u32);
    limits.max_image_height = Some(crate::img::MAX_SIDE as u32);
    limits.max_alloc = Some(2 << 30);
    reader.limits(limits);
    let picture = reader.decode().map_err(|e| e.to_string())?;
    let (w, h) = (picture.width() as usize, picture.height() as usize);
    Img::check_size(h, w, 4)?;
    let alpha = picture.color().has_alpha();
    let (c, data): (usize, Vec<f32>) = if alpha {
        (4, picture.to_rgba8().into_raw().into_iter().map(|v| f32::from(v) / 255.0).collect())
    } else {
        (3, picture.to_rgb8().into_raw().into_iter().map(|v| f32::from(v) / 255.0).collect())
    };
    Ok(Img { h, w, c, data, ..Default::default() })
}

fn encode_image(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "image")?;
    if a.table {
        return Err(config("image: a table is not a picture"));
    }
    if a.c == 3 && a.space.as_deref().is_some_and(|s| s != "RGB") {
        return Err(config(format!("image: {}: encode RGB (Convert Color Space to RGB first)", a.describe())));
    }
    let round = s(i, "conversion", "as the site") == "round";
    let to8 = |v: f32| {
        if a.labels {
            return v.clamp(0.0, 255.0) as u8;
        }
        let x = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) * 255.0 };
        (if round { x + 0.5 } else { x }) as u8
    };
    let (color, bytes8): (image::ExtendedColorType, Vec<u8>) = match a.c {
        1 => (image::ExtendedColorType::L8, a.data.iter().map(|v| to8(*v)).collect()),
        3 => (image::ExtendedColorType::Rgb8, a.data.iter().map(|v| to8(*v)).collect()),
        4 => (image::ExtendedColorType::Rgba8, a.data.iter().map(|v| to8(*v)).collect()),
        c => return Err(config(format!("image: encode 1, 3 or 4 channels, not {c}"))),
    };
    let format = s(i, "format", "png");
    let (w, h) = (u32::try_from(a.w).map_err(|_| config("too wide"))?, u32::try_from(a.h).map_err(|_| config("too tall"))?);
    let mut out = Vec::new();
    let encoded = match format.as_str() {
        "jpeg" => {
            let quality = u8::try_from(n(i, "quality", 95).clamp(1, 100)).unwrap_or(95);
            let rgb: Vec<u8> = if a.c == 4 { bytes8.chunks(4).flat_map(|p| p.iter().take(3).copied().collect::<Vec<_>>()).collect() } else { bytes8 };
            let color = if a.c == 4 { image::ExtendedColorType::Rgb8 } else { color };
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode(&rgb, w, h, color)
        }
        "tiff" => image::ImageEncoder::write_image(image::codecs::tiff::TiffEncoder::new(std::io::Cursor::new(&mut out)), &bytes8, w, h, color),
        _ => image::ImageEncoder::write_image(image::codecs::png::PngEncoder::new(&mut out), &bytes8, w, h, color),
    };
    encoded.map_err(|e| op_error(format!("encoding {format}: {e}")))?;
    let media = match format.as_str() {
        "jpeg" => "image/jpeg",
        "tiff" => "image/tiff",
        _ => "image/png",
    };
    let thumb = plumcraft_engine::arrays::thumb(&a.to_array(), 160);
    let meta = BlobMeta { media: media.into(), size: out.len() as u64, form: Form::Image { width: w, height: h }, aliases: Vec::new() };
    let blob = Blob { handle: Handle::md5_raw(&out), meta, source: BlobSource::Bytes(Arc::from(out)), publish: true, thumb };
    Ok(Outputs::from([("image".to_owned(), Data::Blob(Box::new(blob)))]))
}

fn split(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    if a.table {
        return Err(config("a table has no channels to split"));
    }
    if a.c > 4 {
        return Err(config(format!("{} channels; Split takes at most 4 (use Pick Channel)", a.c)));
    }
    let outs = (0..a.c)
        .map(|k| {
            let names = a.names.get(k).map(|n| vec![n.clone()]).unwrap_or_default();
            let p = Img { space: a.space.clone(), names, labels: a.labels, ..Img::plane_of(a.h, a.w, a.plane(k)) };
            (format!("c{}", k + 1), Data::Array(p.to_array()))
        })
        .collect();
    Ok(outs)
}

fn pick_channel(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    let k = n(i, "channel", 1);
    if k < 1 || k as usize > a.c {
        return Err(config(format!("{} has {} channel(s)", a.describe(), a.c)));
    }
    let k = (k - 1) as usize;
    let names = a.names.get(k).map(|n| vec![n.clone()]).unwrap_or_default();
    Ok(one("channel", Img { space: a.space.clone(), names, labels: a.labels, ..Img::plane_of(a.h, a.w, a.plane(k)) }))
}

fn stack(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let parts = slots(i, "c")?;
    let Some(first) = parts.first() else { return Err(config("connect at least one channel")) };
    let (h, w) = (first.h, first.w);
    let mut planes = Vec::new();
    let mut names = Vec::new();
    let mut spaces = Vec::new();
    for p in &parts {
        if p.table || (p.h, p.w) != (h, w) {
            return Err(config(format!("every channel must be {h}×{w}: got {}", p.describe())));
        }
        for k in 0..p.c {
            planes.push(p.plane(k));
            names.push(p.names.get(k).cloned().unwrap_or_default());
        }
        spaces.push(p.space.clone());
    }
    Img::check_size(h, w, planes.len()).map_err(config)?;
    let space = match s(i, "space", "auto").as_str() {
        "auto" => auto_space(planes.len(), &names, &spaces),
        "none" => None,
        named => Some(named.to_owned()),
    };
    let out = match space {
        Some(space) => {
            if planes.len() != 3 {
                return Err(config(format!("{space} needs 3 channels, got {}", planes.len())));
            }
            let names = channel_names(&space).unwrap_or_default();
            Img { space: Some(space), names, ..Img::from_planes(h, w, &planes) }
        }
        None => {
            let names = if names.iter().any(|n| !n.is_empty()) {
                names.iter().enumerate().map(|(k, n)| if n.is_empty() { format!("channel {}", k + 1) } else { n.clone() }).collect()
            } else {
                Vec::new()
            };
            Img { names, ..Img::from_planes(h, w, &planes) }
        }
    };
    Ok(one("stack", out))
}

/// The colour space of channels being stacked (the research software's `auto_space`): the one space they
/// were split from, when the named ones sit in that space's order, or the space whose channel
/// names they have.
fn auto_space(count: usize, names: &[String], spaces: &[Option<String>]) -> Option<String> {
    let mut tagged: Vec<&String> = spaces.iter().flatten().collect();
    tagged.dedup();
    if let [space] = tagged.as_slice()
        && count == 3
        && let Some(expected) = channel_names(space)
        && names.iter().zip(&expected).all(|(n, c)| n.is_empty() || n == c)
    {
        return Some((*space).clone());
    }
    guess_space(names)
}

fn constant(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let value = f(i, "value", 0.0) as f32;
    let like = img_opt(i, "like")?;
    let (h, w) = match &like {
        Some(l) => (l.h, l.w),
        None => (n(i, "height", 512).clamp(1, 16384) as usize, n(i, "width", 512).clamp(1, 16384) as usize),
    };
    let c = n(i, "channels", 1).clamp(1, 8) as usize;
    Img::check_size(h, w, c).map_err(config)?;
    Ok(one("constant", Img::filled(h, w, c, value)))
}

/// Set a band `width` wide round each side of a plane to `value` (yapocis `set_margin`).
pub fn set_margin(p: &mut [f32], h: usize, w: usize, width: usize, value: f32) {
    for y in 0..h {
        for x in 0..w {
            if (y < width || x < width || y + width >= h || x + width >= w)
                && let Some(v) = p.get_mut(y * w + x)
            {
                *v = value;
            }
        }
    }
}

fn frame(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    let margin = n(i, "margin", 0).clamp(0, 4096) as usize;
    let (mv, fv) = (f(i, "margin_value", 0.0) as f32, f(i, "frame_value", 1.0) as f32);
    let out = a
        .map_planes(|p, h, w| {
            let mut p = p.to_vec();
            if margin > 0 {
                set_margin(&mut p, h, w, margin, mv);
            }
            set_margin(&mut p, h, w, 1, fv);
            Ok(p)
        })
        .map_err(op_error)?;
    Ok(one("framed", out))
}

fn crop_to_multiple(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "array")?;
    let m = n(i, "multiple", 4).clamp(1, 1024) as usize;
    Ok(one("cropped", aligned(&a, m).map_err(config)?))
}
