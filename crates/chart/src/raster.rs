//! A display list ([`crate::layout`]) drawn into an RGBA8 picture (note 26, face previews option
//! A): the engine SDK's chart thumbnail is the core's own layout, drawn here with tiny-skia (pure
//! Rust). A list is data from the other side: every size is bounded, every number checked, junk
//! is skipped, nothing panics.
//!
//! Text is not drawn (a thumbnail has no room for it, and a shell draws a chart's text with its
//! own): marks, grid, axes and legend swatches only. [`Frame::Plot`] fits the plot rectangle to
//! the whole picture, a face's thumbnail; [`Frame::Whole`] scales the list's own size to it.

use serde_json::Value;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// The largest side of a picture drawn.
pub const MAX_SIDE: u32 = 4096;
/// The most items (points, segments, rectangles, vertices) drawn from one list.
pub const MAX_ITEMS: usize = 400_000;

/// A drawn picture: `w` × `h` pixels, RGBA8, not premultiplied, rows top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// The colours a list leaves to its painter.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub background: [u8; 4],
    /// Grid and axes.
    pub ink: [u8; 4],
    /// A mark with no colour of its own.
    pub accent: [u8; 4],
}

impl Default for Style {
    /// A light panel, grey ink, the first colour of the core's palette.
    fn default() -> Self {
        Style { background: [250, 250, 248, 255], ink: [120, 120, 120, 255], accent: [0x4e, 0x79, 0xa7, 255] }
    }
}

/// What the picture shows of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// The list's whole size, scaled to the picture.
    Whole,
    /// Only the plot, filling the picture but for `inset` pixels: marks, no axes (a thumbnail).
    Plot { inset: u32 },
}

fn nums(v: &Value) -> Vec<f32> {
    v.as_array().map(|a| a.iter().take(MAX_ITEMS * 4).filter_map(Value::as_f64).map(|x| x as f32).collect()).unwrap_or_default()
}

fn colour_of(v: &Value, dflt: [u8; 4]) -> Color {
    let rgba = v
        .as_str()
        .and_then(|s| s.strip_prefix('#'))
        .filter(|h| h.len() == 6 && h.is_ascii())
        .and_then(|h| u32::from_str_radix(h, 16).ok())
        .map_or(dflt, |n| [(n >> 16) as u8, (n >> 8) as u8, n as u8, 255]);
    Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3])
}

fn paint(c: Color, alpha: f32) -> Paint<'static> {
    let mut p = Paint::default();
    let mut c = c;
    c.apply_opacity(alpha.clamp(0.0, 1.0));
    p.set_color(c);
    p.anti_alias = true;
    p
}

/// `list` drawn into a `w` × `h` picture (module docs). An error only when there is nothing to
/// draw on: a size of zero or past [`MAX_SIDE`], or a list with no plot rectangle.
pub fn draw(list: &Value, w: u32, h: u32, style: &Style, frame: Frame) -> Result<Picture, String> {
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("{w}×{h} is not a picture this draws (1 to {MAX_SIDE} a side)"));
    }
    let plot = nums(&list["plot"]);
    let [px, py, pw, ph] = plot[..] else { return Err("the list has no plot rectangle".into()) };
    if !(px.is_finite() && py.is_finite() && pw.is_finite() && ph.is_finite()) || pw <= 0.0 || ph <= 0.0 {
        return Err("the list's plot rectangle is empty".into());
    }
    let mut pix = Pixmap::new(w, h).ok_or("no room for the picture")?;
    let [r, g, b, a] = style.background;
    pix.fill(Color::from_rgba8(r, g, b, a));
    let (wf, hf) = (w as f32, h as f32);
    // the list's points to pixels
    let t = match frame {
        Frame::Whole => {
            let size = nums(&list["size"]);
            let (lw, lh) = match size[..] {
                [a, b] if a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0 => (a, b),
                _ => (px + pw, py + ph),
            };
            Transform::from_scale(wf / lw, hf / lh)
        }
        Frame::Plot { inset } => {
            let i = (inset as f32).min(wf / 4.0).min(hf / 4.0);
            Transform::from_row((wf - 2.0 * i) / pw, 0.0, 0.0, (hf - 2.0 * i) / ph, i - px * (wf - 2.0 * i) / pw, i - py * (hf - 2.0 * i) / ph)
        }
    };
    let scale = (t.sx.abs() * t.sy.abs()).sqrt().max(0.01);
    let ink = Color::from_rgba8(style.ink[0], style.ink[1], style.ink[2], style.ink[3]);
    let line = |pix: &mut Pixmap, pts: &[(f32, f32)], c: Color, width: f32, alpha: f32, mask: Option<&tiny_skia::Mask>| {
        let mut pb = PathBuilder::new();
        let mut started = false;
        for (x, y) in pts {
            if !(x.is_finite() && y.is_finite()) {
                started = false;
                continue;
            }
            if started {
                pb.line_to(*x, *y);
            } else {
                pb.move_to(*x, *y);
                started = true;
            }
        }
        if let Some(path) = pb.finish() {
            let stroke = Stroke { width: (width / scale).max(1.0 / scale), ..Stroke::default() };
            pix.stroke_path(&path, &paint(c, alpha), &stroke, t, mask);
        }
    };
    let whole = frame == Frame::Whole;
    if whole {
        for x in nums(&list["grid"]["x"]).into_iter().take(64) {
            line(&mut pix, &[(x, py), (x, py + ph)], ink, 0.5, 0.25, None);
        }
        for y in nums(&list["grid"]["y"]).into_iter().take(64) {
            line(&mut pix, &[(px, y), (px + pw, y)], ink, 0.5, 0.25, None);
        }
    }
    // the marks inside the plot when the list says so, or always in a thumbnail
    let mask = if list["clip"].as_bool() == Some(true) || !whole {
        let mut m = tiny_skia::Mask::new(w, h).ok_or("no room for the clip")?;
        if let Some(r) = Rect::from_xywh(px, py, pw, ph) {
            m.fill_path(&PathBuilder::from_rect(r), FillRule::Winding, true, t);
        }
        Some(m)
    } else {
        None
    };
    let mask = mask.as_ref();
    let mut budget = MAX_ITEMS;
    for m in list["marks"].as_array().into_iter().flatten().take(256) {
        let c = colour_of(&m["color"], style.accent);
        let width = m["width"].as_f64().filter(|w| w.is_finite()).map_or(1.5, |w| w.clamp(0.1, 20.0)) as f32;
        match m["kind"].as_str() {
            Some("points") => {
                let r = m["r"].as_f64().filter(|r| r.is_finite()).map_or(2.0, |r| r.clamp(0.2, 20.0)) as f32;
                let xy = nums(&m["xy"]);
                let mut pb = PathBuilder::new();
                for p in xy.as_chunks::<2>().0.iter().take(budget) {
                    if p[0].is_finite() && p[1].is_finite() {
                        pb.push_circle(p[0], p[1], r.max(0.7 / scale));
                    }
                }
                budget = budget.saturating_sub(xy.len() / 2);
                if let Some(path) = pb.finish() {
                    pix.fill_path(&path, &paint(c, 1.0), FillRule::Winding, t, mask);
                }
            }
            Some("segments") => {
                let xy = nums(&m["xy"]);
                let mut pb = PathBuilder::new();
                for s in xy.as_chunks::<4>().0.iter().take(budget) {
                    if s.iter().all(|v| v.is_finite()) {
                        pb.move_to(s[0], s[1]);
                        pb.line_to(s[2], s[3]);
                    }
                }
                budget = budget.saturating_sub(xy.len() / 4);
                if let Some(path) = pb.finish() {
                    let stroke = Stroke { width: (width / scale).max(1.0 / scale), ..Stroke::default() };
                    pix.stroke_path(&path, &paint(c, 1.0), &stroke, t, mask);
                }
            }
            Some("rects") => {
                let rs = nums(&m["rects"]);
                let mut pb = PathBuilder::new();
                for q in rs.as_chunks::<4>().0.iter().take(budget) {
                    // a bar at least a pixel wide and tall, so a thin bar still shows
                    let (qw, qh) = (q[2].max(1.0 / t.sx.abs().max(0.01)), q[3].max(1.0 / t.sy.abs().max(0.01)));
                    if let Some(r) = Rect::from_xywh(q[0], q[1], qw, qh) {
                        pb.push_rect(r);
                    }
                }
                budget = budget.saturating_sub(rs.len() / 4);
                if let Some(path) = pb.finish() {
                    pix.fill_path(&path, &paint(c, 0.85), FillRule::Winding, t, mask);
                }
            }
            Some("line") => {
                let xy = nums(&m["xy"]);
                let pts: Vec<(f32, f32)> = xy.as_chunks::<2>().0.iter().take(budget).map(|p| (p[0], p[1])).collect();
                budget = budget.saturating_sub(pts.len());
                line(&mut pix, &pts, c, width, 1.0, mask);
            }
            Some("area") => {
                let xy = nums(&m["xy"]);
                let mut pb = PathBuilder::new();
                for (i, p) in xy.as_chunks::<2>().0.iter().take(budget).filter(|p| p[0].is_finite() && p[1].is_finite()).enumerate() {
                    if i == 0 { pb.move_to(p[0], p[1]) } else { pb.line_to(p[0], p[1]) }
                }
                budget = budget.saturating_sub(xy.len() / 2);
                pb.close();
                if let Some(path) = pb.finish() {
                    pix.fill_path(&path, &paint(c, 0.8), FillRule::Winding, t, mask);
                }
            }
            _ => {}
        }
        if budget == 0 {
            break;
        }
    }
    if whole && list["axes"].as_array().is_some_and(|a| !a.is_empty()) {
        line(&mut pix, &[(px, py), (px, py + ph), (px + pw, py + ph)], ink, 1.0, 0.7, None);
    }
    if whole {
        for l in list["legend"].as_array().into_iter().flatten().take(64) {
            if let (Some(x), Some(y)) = (l["x"].as_f64(), l["y"].as_f64())
                && let Some(r) = Rect::from_xywh(x as f32, y as f32, 10.0, 10.0)
            {
                pix.fill_rect(r, &paint(colour_of(&l["color"], style.accent), 1.0), t, None);
            }
        }
    }
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for p in pix.pixels() {
        let c = p.demultiply();
        rgba.extend([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(Picture { w, h, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn px(p: &Picture, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * p.w + x) * 4) as usize;
        [p.rgba[i], p.rgba[i + 1], p.rgba[i + 2], p.rgba[i + 3]]
    }

    /// A bar fills its rectangle in its colour; the background stays outside it.
    #[test]
    fn a_bar_is_drawn_where_the_list_says() {
        let list =
            json!({"size": [100, 100], "plot": [0, 0, 100, 100], "marks": [{"kind": "rects", "color": "#ff0000", "rects": [20, 50, 20, 50]}], "axes": []});
        let p = draw(&list, 100, 100, &Style::default(), Frame::Whole).unwrap();
        let inside = px(&p, 30, 75);
        assert!(inside[0] > 200 && inside[1] < 80 && inside[2] < 80, "{inside:?}");
        assert_eq!(px(&p, 70, 20), [250, 250, 248, 255], "the background");
    }

    /// The plot fills a thumbnail: a line across the plot crosses the whole picture.
    #[test]
    fn a_thumbnail_is_the_plot_alone() {
        let list = json!({"size": [600, 400], "plot": [100, 50, 400, 300], "marks": [{"kind": "line", "xy": [100, 200, 500, 200], "width": 2}],
                          "axes": [{"side": "bottom", "ticks": []}]});
        let p = draw(&list, 64, 40, &Style::default(), Frame::Plot { inset: 2 }).unwrap();
        let accent = Style::default().accent;
        for x in [4, 32, 59] {
            let c = px(&p, x, 20);
            assert!((c[2] as i32 - accent[2] as i32).abs() < 40 && c[0] < 180, "x {x}: {c:?}");
        }
        assert_eq!(px(&p, 32, 5), [250, 250, 248, 255], "nothing above the line");
    }

    /// Junk is skipped or refused, never a panic.
    #[test]
    fn hostile_lists_never_panic() {
        let s = Style::default();
        for list in [
            json!(null),
            json!({}),
            json!({"plot": [0, 0, 0, 0]}),
            json!({"plot": [0, 0, "a", 1]}),
            json!({"plot": [0, 0, 10, 10], "marks": "x"}),
            json!({"plot": [0, 0, 10, 10], "marks": [{"kind": "rects", "rects": [1, 2, 3]}, {"kind": "points", "xy": [1e30, -1e30, 1], "r": 1e9},
                                                     {"kind": "line", "xy": [0, 0, 1e38, 1e38, null, 5]}, {"kind": "area", "xy": []}, {"kind": "segments", "xy": [1, 2, 3, 4, 5]},
                                                     {"kind": "weird"}, 7], "clip": true, "size": [-1, 0], "legend": [{"x": 1e40, "y": 1}]}),
        ] {
            for frame in [Frame::Whole, Frame::Plot { inset: 400 }] {
                let _ = draw(&list, 32, 24, &s, frame);
            }
        }
        assert!(draw(&json!({"plot": [0, 0, 1, 1]}), 0, 10, &s, Frame::Whole).is_err());
        assert!(draw(&json!({"plot": [0, 0, 1, 1]}), MAX_SIDE + 1, 10, &s, Frame::Whole).is_err());
        let big: Vec<f64> = (0..(MAX_ITEMS * 3)).map(|i| (i % 100) as f64).collect();
        let p = draw(&json!({"plot": [0, 0, 100, 100], "marks": [{"kind": "points", "xy": big}]}), 50, 50, &s, Frame::Whole).unwrap();
        assert_eq!(p.rgba.len(), 50 * 50 * 4);
    }
}
