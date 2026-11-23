//! Paint Mask (`array.paint_mask`): a mask painted by hand on a picture, with the shell's mask
//! editor (`docs/shell-interface.md` 12.22, `plumcraft_proto::masks`).
//!
//! The editor's edit is a file: an RGBA PNG at the mask's size (the picture's, scaled down to
//! [`plumcraft_proto::masks::MAX_SIDE`] on its long side), alpha the mask and the colours the
//! picture as painted. Its path is the node's `edit` input, which the editor's commands set; empty,
//! there is no edit.
//!
//! An incoming `mask` (another node's, one channel 0..1) is where the painting starts: the editor
//! begins from it, strokes add to it and take from it, and with no edit it passes through. Once
//! there is an edit, the edit holds the whole mask as painted: a later change upstream doesn't
//! reach it until the edit is cleared (the maintainer, 2026-11-23, decision 1A, read so).
//!
//! The run sends the editor the picture at the mask's size (`picture`) and, when a mask is wired,
//! that mask (`base`, grey), so a stroke paints on exactly what the node reads back.

use plumcraft_engine::arrays::{rgba8, views};
use plumcraft_engine::exec::{EDITOR_KEY, EDITOR_PICTURE_KEY, EDITOR_PICTURE_PX, EDITOR_PICTURES_PREFIX};
use plumcraft_engine::{Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::masks::canvas_size;
use serde_json::json;

use crate::img::Img;
use crate::op::{Op, array_in, array_opt, array_out, config, img, img_opt, op_error, path_in, s};

/// The editor kind Paint Mask names.
pub const MASK_EDITOR: &str = "mask";

pub fn nodes() -> Vec<Box<dyn Node>> {
    vec![Box::new(Op {
        id: "array.paint_mask",
        display: "Paint Mask",
        group: "Masks",
        description: "A mask painted by hand on a picture: Edit Value… opens the mask editor, whose Mask, Erase and Paint brushes paint on the picture (after a run). A mask wired in is where the painting starts, and passes through until something is painted. The mask comes out as one channel, 0-1, at the picture's size; painted is the picture with Paint's greys laid in. The edit is a file beside the graph, at the picture's size up to 2048 on its long side (scaled up beyond that).",
        inputs: vec![
            array_in("picture", "What to paint on: a colour stack (its first three channels) or one channel, 0-1"),
            array_opt("mask", "A mask to start from: one channel, 0-1, the picture's size"),
            path_in("edit", "The mask editor's edit: an RGBA PNG (alpha the mask) the editor writes; empty, nothing is painted"),
        ],
        outputs: vec![array_out("mask", "The mask: one channel, 0-1"), array_out("painted", "The picture with Paint's greys laid in (RGB, 0-1)")],
        run: paint_mask,
    })]
}

/// The picture as the editor shows it: three channels, 0-1 (one channel as grey), tagged RGB.
fn display(a: &Img) -> Img {
    let unit = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let planes: Vec<Vec<f32>> = (0..3).map(|k| a.plane(if a.c >= 3 { k } else { 0 }).into_iter().map(unit).collect()).collect();
    Img { space: Some("RGB".into()), ..Img::from_planes(a.h, a.w, &planes) }
}

/// The editor's picture of `d` at the mask's size, 8 bits: what the SDK sends it, the same bytes.
fn editor_pixels(d: &Img) -> Option<Vec<u8>> {
    let p = views(&d.to_array(), "auto", 1.0).into_iter().next()?;
    Some(rgba8(&p, EDITOR_PICTURE_PX).bytes.to_vec())
}

/// The edit file at `path`, as RGBA8 `(w, h, pixels)`.
fn read_edit(path: &str) -> Result<(usize, usize, Vec<u8>), NodeError> {
    let bytes = std::fs::read(path).map_err(|e| NodeError::new(NodeError::MISSING_SOURCE, format!("edit: {path}: {e}")))?;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| op_error(format!("edit: {e}")))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(crate::img::MAX_SIDE as u32);
    limits.max_image_height = Some(crate::img::MAX_SIDE as u32);
    reader.limits(limits);
    let picture = reader.decode().map_err(|e| op_error(format!("edit: {path} isn't a picture this engine reads ({e})")))?.to_rgba8();
    Ok((picture.width() as usize, picture.height() as usize, picture.into_raw()))
}

/// `small` (`sh × sw`, one value a pixel) at `h × w`, bilinear (pixel centres).
fn upscale(small: &[f32], sh: usize, sw: usize, h: usize, w: usize) -> Vec<f32> {
    if (sh, sw) == (h, w) {
        return small.to_vec();
    }
    let at = |y: usize, x: usize| small.get(y.min(sh - 1) * sw + x.min(sw - 1)).copied().unwrap_or(0.0);
    let mut out = Vec::with_capacity(h * w);
    for y in 0..h {
        let fy = ((y as f32 + 0.5) * sh as f32 / h as f32 - 0.5).max(0.0);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        for x in 0..w {
            let fx = ((x as f32 + 0.5) * sw as f32 / w as f32 - 0.5).max(0.0);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let top = at(y0, x0) * (1.0 - tx) + at(y0, x0 + 1) * tx;
            let bottom = at(y0 + 1, x0) * (1.0 - tx) + at(y0 + 1, x0 + 1) * tx;
            out.push(top * (1.0 - ty) + bottom * ty);
        }
    }
    out
}

fn paint_mask(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "picture")?;
    if a.table {
        return Err(config("picture: a table has no pixels to paint on"));
    }
    let (h, w) = (a.h, a.w);
    let base = match img_opt(i, "mask")? {
        Some(m) if (m.h, m.w) != (h, w) || m.table => return Err(config(format!("mask: {}, the picture {h} × {w}", m.describe()))),
        Some(m) => Some(m.plane(0).into_iter().map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }).collect::<Vec<f32>>()),
        None => None,
    };
    let shown = display(&a);
    let (mh, mw) = canvas_size(h, w);
    let edit = s(i, "edit", "");
    let (mask, painted) = if edit.trim().is_empty() {
        (base.clone().unwrap_or_else(|| vec![0.0; h * w]), shown.clone())
    } else {
        let (ew, eh, px) = read_edit(edit.trim())?;
        if (eh, ew) != (mh, mw) {
            return Err(config(format!("edit: the edit is {ew} × {eh}, the picture's mask {mw} × {mh}: clear the mask (the editor's Clear) and paint again")));
        }
        let small: Vec<f32> = px.as_chunks::<4>().0.iter().map(|p| f32::from(p[3]) / 255.0).collect();
        let mask = upscale(&small, mh, mw, h, w);
        // the painted colours where the edit differs from the picture the editor painted on
        let shown8 = editor_pixels(&shown).ok_or_else(|| op_error("the picture has no view to paint on"))?;
        let mut painted = shown.clone();
        for y in 0..h {
            let ys = y * mh / h;
            for x in 0..w {
                let k = ys * mw + x * mw / w;
                let (Some(e), Some(o)) = (px.get(k * 4..k * 4 + 3), shown8.get(k * 4..k * 4 + 3)) else { continue };
                if e != o {
                    for c in 0..3 {
                        if let (Some(v), Some(src)) = (painted.data.get_mut((y * w + x) * 3 + c), e.get(c)) {
                            *v = f32::from(*src) / 255.0;
                        }
                    }
                }
            }
        }
        (mask, painted)
    };
    let mask_img = Img::from_planes(h, w, &[mask]);
    let mut out = Outputs::new();
    out.insert("mask".into(), Data::Array(mask_img.to_array()));
    out.insert("painted".into(), Data::Array(painted.to_array()));
    // what the mask editor paints on: the picture, the mask it starts from, and the mask's size
    out.insert(EDITOR_KEY.into(), Data::Json(json!({MASK_EDITOR: {"w": mw, "h": mh}})));
    out.insert(EDITOR_PICTURE_KEY.into(), Data::Array(shown.to_array()));
    if let Some(b) = base {
        let grey = Img { space: Some("RGB".into()), ..Img::from_planes(h, w, &[b.clone(), b.clone(), b]) };
        out.insert(format!("{EDITOR_PICTURES_PREFIX}base"), Data::Array(grey.to_array()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editors_picture_is_the_masks_size() {
        for (h, w) in [(32, 48), (2048, 100), (3000, 4000), (5, 9000)] {
            let a = Img::from_planes(h, w, &[vec![0.5; h * w]]);
            let px = editor_pixels(&display(&a)).unwrap();
            let (mh, mw) = canvas_size(h, w);
            assert_eq!(px.len(), mh * mw * 4, "{h} × {w}");
        }
    }

    #[test]
    fn upscaling_keeps_the_edges_and_the_size() {
        let up = upscale(&[0.0, 1.0, 0.0, 1.0], 2, 2, 4, 4);
        assert_eq!(up.len(), 16);
        assert_eq!((up[0], up[3]), (0.0, 1.0));
        assert_eq!(upscale(&[0.25; 6], 2, 3, 2, 3), vec![0.25; 6]);
    }
}
