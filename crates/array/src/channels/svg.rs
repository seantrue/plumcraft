//! SVG, the core type (`core_types::SVG`), into and out of arrays: Rasterize SVG draws one
//! (with `plumcraft-svg`, as the editor does) as rgb, alpha and rgba, as Decode Image does a PNG;
//! Array to SVG, its inverse, wraps a picture in an SVG as an embedded PNG (8 bits a channel),
//! so Rasterize SVG at its own size gives the same pixels back.

use plumcraft_engine::{Blob, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::catalogue::{Port, Socket, core_types};
use plumcraft_proto::error::NodeError;

use super::{MAX_FILE, picture_outputs};
use crate::img::Img;
use crate::op::{Op, array_in, array_out, config, enum_in, img, int_in, n, op_error, out, s};

pub fn nodes() -> Vec<Box<dyn Node>> {
    vec![
        Box::new(Op {
            id: "array.rasterize_svg",
            display: "Rasterize SVG",
            group: "Arrays",
            description: "An SVG (from VectorCraft, Array to SVG, Load Files or any engine) drawn as arrays, as Decode Image reads a PNG: rgb (H × W × 3, 0-1) flattened over the background, alpha, and rgba. Drawn at the SVG's own size unless width or height says otherwise; text is drawn with the system's fonts. The inverse of Array to SVG.",
            inputs: vec![
                Port { name: "svg".into(), ty: core_types::SVG.into(), socket: Some(Socket::Socket), tooltip: Some("The SVG".into()), ..Default::default() },
                int_in("width", 0, 0.0, 16384.0, "Pixels across; 0 keeps the SVG's own width, or follows height keeping the shape"),
                int_in("height", 0, 0.0, 16384.0, "Pixels down; 0 keeps the SVG's own height, or follows width keeping the shape"),
                enum_in(
                    "background",
                    &["black", "white", "gray", "as is"],
                    "black",
                    "What transparent pixels become in rgb: composited over black, white or gray, or as is (the colours under the alpha)",
                ),
            ],
            outputs: vec![
                array_out("rgb", "Red, green and blue, 0-1"),
                array_out("alpha", "Opacity, 1 = opaque"),
                array_out("rgba", "Red, green, blue and alpha, 0-1: the colours under the alpha, not composited"),
            ],
            run: rasterize,
        }),
        Box::new(Op {
            id: "array.array_to_svg",
            display: "Array to SVG",
            group: "Arrays",
            description: "A picture (RGB, RGBA or one channel, 0-1) as an SVG holding it as an embedded PNG, one SVG pixel a picture pixel, drawn without smoothing: for engines and apps that take SVG. Values are clipped to 0-1 and rounded to 8 bits. The inverse of Rasterize SVG.",
            inputs: vec![array_in("image", "The picture: RGB, RGBA, or one channel (grey)")],
            outputs: vec![out("svg", core_types::SVG, "The SVG")],
            run: to_svg,
        }),
    ]
}

/// The pixel size to draw an SVG of `size` at, from the width and height asked (0: not asked).
fn size_for(size: (f32, f32), width: i64, height: i64) -> (u32, u32) {
    let (w0, h0) = (size.0.max(1.0), size.1.max(1.0));
    let px = |v: f32| v.round().clamp(1.0, 16384.0) as u32;
    match (width.clamp(0, 16384) as f32, height.clamp(0, 16384) as f32) {
        (w, h) if w > 0.0 && h > 0.0 => (px(w), px(h)),
        (w, _) if w > 0.0 => (px(w), px(h0 * w / w0)),
        (_, h) if h > 0.0 => (px(w0 * h / h0), px(h)),
        _ => (px(w0), px(h0)),
    }
}

fn rasterize(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let blob = i.get("svg").and_then(Data::blob).ok_or_else(|| config("svg: wire an SVG (VectorCraft's Export SVG, Array to SVG, Load Files)"))?;
    let bytes = blob.read(MAX_FILE)?;
    let tree = plumcraft_svg::parse(&bytes).map_err(|e| config(format!("svg: not an SVG this engine can draw ({e})")))?;
    let (w, h) = size_for((tree.size().width(), tree.size().height()), n(i, "width", 0), n(i, "height", 0));
    let r = plumcraft_svg::render(&tree, w, h).map_err(config)?;
    let pixels = Img { h: r.h as usize, w: r.w as usize, c: 4, data: r.pixels.iter().map(|v| f32::from(*v) / 255.0).collect(), ..Default::default() };
    picture_outputs(pixels, false, &s(i, "background", "black"))
}

fn to_svg(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "image")?;
    if a.table || a.labels {
        return Err(config("image: a table or labels is not a picture"));
    }
    if a.c == 3 && a.space.as_deref().is_some_and(|s| s != "RGB") {
        return Err(config(format!("image: {}: convert to RGB first (Convert Color Space)", a.describe())));
    }
    let color = match a.c {
        1 => image::ExtendedColorType::L8,
        3 => image::ExtendedColorType::Rgb8,
        4 => image::ExtendedColorType::Rgba8,
        c => return Err(config(format!("image: 1, 3 or 4 channels, not {c}"))),
    };
    let bytes8: Vec<u8> = a.data.iter().map(|v| if v.is_nan() { 0 } else { (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8 }).collect();
    let (w, h) = (u32::try_from(a.w).map_err(|_| config("too wide"))?, u32::try_from(a.h).map_err(|_| config("too tall"))?);
    let mut png = Vec::new();
    image::ImageEncoder::write_image(image::codecs::png::PngEncoder::new(&mut png), &bytes8, w, h, color)
        .map_err(|e| op_error(format!("encoding the PNG: {e}")))?;
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.encode(&png);
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><image width="{w}" height="{h}" image-rendering="optimizeSpeed" style="image-rendering:pixelated" xlink:href="data:image/png;base64,{data}"/></svg>"#
    );
    let blob = Blob::svg(svg).map_err(|e| op_error(format!("the SVG made: {}", e.message)))?;
    Ok(Outputs::from([("svg".to_owned(), Data::Blob(Box::new(blob)))]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_follow_what_is_asked_keeping_the_shape() {
        assert_eq!(size_for((200.0, 100.0), 0, 0), (200, 100));
        assert_eq!(size_for((200.0, 100.0), 50, 0), (50, 25));
        assert_eq!(size_for((200.0, 100.0), 0, 50), (100, 50));
        assert_eq!(size_for((200.0, 100.0), 30, 30), (30, 30));
        assert_eq!(size_for((0.0, 0.0), 0, 0), (1, 1));
    }
}
