//! 3D meshes from STL files (#38): the triangles shaded, turned by dragging, zoomed by
//! scrolling, and fitted again by a double-click or Fit. The file is read by
//! [`plumcraft_proto::mesh`] (binary or ASCII, bounded), on a worker thread, as is each picture.
//!
//! Drawn on the CPU, like SolidCraft's headless pictures (`crates/render/src/raster.rs` in
//! <https://github.com/tangency-labs/solidcraft>, MIT OR Apache-2.0, adapted here to bare
//! triangles): orthographic, Z up, flat-shaded with a z-buffer, rendered at twice the size and
//! averaged down. So it draws the same on every machine, with or without a GPU, and in tests.
//! While the mesh is being turned it renders at the screen's size only, and sharp once it stops.

use std::sync::Arc;

use plumcraft_proto::catalogue::Media;
use plumcraft_proto::display::Display;
use plumcraft_proto::mesh::{Mesh, is_stl_media, read_stl};
use serde_json::{Value, json};

use super::{ViewInput, Viewer};
use crate::canvas::Action;
use crate::media::{Job, Rgba, key_of};
use crate::theme::Tokens;

/// The largest picture rendered, a side, before supersampling.
pub const MAX_SIDE: u32 = 1600;

/// Whether `d` is an STL file this viewer shows.
pub fn accepts(d: &Display) -> bool {
    let typed = d.full.as_ref().is_some_and(|f| is_stl_media(&f.meta.media));
    typed && matches!(d.media, Media::Mesh | Media::Bytes)
}

/// Where the mesh is seen from: turned `yaw` about Z and raised `pitch` above the XY plane
/// (radians), at `zoom` times the size that fits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
}

impl Default for Camera {
    /// From the front right, a little above: SolidCraft's isometric view.
    fn default() -> Self {
        Camera { yaw: std::f32::consts::FRAC_PI_4, pitch: 0.615, zoom: 1.0 }
    }
}

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: V3) -> Option<V3> {
    let l = dot(a, a).sqrt();
    (l > 1e-30 && l.is_finite()).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

impl Camera {
    /// Right, up and back (toward the eye), orthonormal; Z is up on screen.
    fn basis(&self) -> (V3, V3, V3) {
        let (yaw, pitch) = (f64::from(self.yaw), f64::from(self.pitch));
        let back = [-yaw.sin() * pitch.cos(), -yaw.cos() * pitch.cos(), pitch.sin()];
        let right = [yaw.cos(), -yaw.sin(), 0.0];
        let up = unit(cross(back, right)).unwrap_or([0.0, 0.0, 1.0]);
        (right, up, back)
    }

    /// Turned by a drag of `dx`, `dy` points.
    pub fn turned(self, dx: f32, dy: f32) -> Camera {
        let limit = std::f32::consts::FRAC_PI_2 - 0.01;
        Camera { yaw: (self.yaw + dx * 0.01) % std::f32::consts::TAU, pitch: (self.pitch + dy * 0.01).clamp(-limit, limit), ..self }
    }

    /// Zoomed by a scroll of `dy` points.
    pub fn zoomed(self, dy: f32) -> Camera {
        Camera { zoom: (self.zoom * (dy * 0.003).exp()).clamp(0.05, 200.0), ..self }
    }
}

/// The colours a picture is drawn in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colours {
    pub background: [u8; 3],
    pub body: [u8; 3],
}

impl Colours {
    pub fn of(tokens: &Tokens) -> Colours {
        let rgb = |c: egui::Color32| [c.r(), c.g(), c.b()];
        // the plum of groups: a desaturated fruit, light enough for its shading to read
        Colours { background: rgb(tokens.canvas), body: rgb(tokens.group.header) }
    }
}

/// `mesh` seen through `cam`, `w` by `h` pixels, drawn `ss` times larger and averaged down.
pub fn render(mesh: &Mesh, cam: Camera, w: u32, h: u32, ss: u32, colours: Colours) -> Rgba {
    let (w, h, ss) = (w.clamp(1, MAX_SIDE) as usize, h.clamp(1, MAX_SIDE) as usize, ss.clamp(1, 2) as usize);
    let (sw, sh) = (w * ss, h * ss);
    let bg = colours.background.map(f64::from);
    let mut rgb: Vec<[f64; 3]> = vec![bg; sw * sh];
    let mut depth = vec![f64::INFINITY; sw * sh];
    let (lo, hi) = mesh.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
    let centre: V3 = std::array::from_fn(|k| (f64::from(lo[k]) + f64::from(hi[k])) * 0.5);
    let radius = (dot(sub(hi.map(f64::from), lo.map(f64::from)), sub(hi.map(f64::from), lo.map(f64::from))).sqrt() * 0.5).max(1e-9);
    // the whole mesh fits the shorter side, with a margin
    let scale = sw.min(sh) as f64 * 0.5 / (radius * 1.08) * f64::from(cam.zoom);
    let (right, up, back) = cam.basis();
    // a light over the viewer's right shoulder, and the view itself
    let light =
        unit([right[0] * 0.35 + up[0] * 0.55 + back[0], right[1] * 0.35 + up[1] * 0.55 + back[1], right[2] * 0.35 + up[2] * 0.55 + back[2]]).unwrap_or(back);
    let body = colours.body.map(f64::from);
    let project = |p: &[f32; 3]| -> [f64; 3] {
        let d = sub(p.map(f64::from), centre);
        [sw as f64 * 0.5 + dot(d, right) * scale, sh as f64 * 0.5 - dot(d, up) * scale, -dot(d, back)]
    };
    for t in &mesh.triangles {
        let world: [V3; 3] = t.map(|p| p.map(f64::from));
        let Some(n) = unit(cross(sub(world[1], world[0]), sub(world[2], world[0]))) else { continue };
        // two-sided: STL windings are often wrong, so a face lit from behind is lit the same
        let lit = 0.32 + 0.58 * dot(n, light).abs() + 0.18 * dot(n, back).abs();
        let colour = body.map(|c| (c * lit).min(255.0));
        let [a, b, c] = t.map(|p| project(&p));
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-12 || !area.is_finite() {
            continue;
        }
        let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.0);
        let x1 = a[0].max(b[0]).max(c[0]).ceil().min(sw as f64 - 1.0);
        let y0 = a[1].min(b[1]).min(c[1]).floor().max(0.0);
        let y1 = a[1].max(b[1]).max(c[1]).ceil().min(sh as f64 - 1.0);
        if !(x0 <= x1 && y0 <= y1) {
            continue;
        }
        for y in y0 as usize..=y1 as usize {
            let py = y as f64 + 0.5;
            for x in x0 as usize..=x1 as usize {
                let px = x as f64 + 0.5;
                let w0 = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
                let w1 = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                    continue;
                }
                let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
                let i = y * sw + x;
                if depth.get(i).is_some_and(|d| z < *d) {
                    if let Some(d) = depth.get_mut(i) {
                        *d = z;
                    }
                    if let Some(p) = rgb.get_mut(i) {
                        *p = colour;
                    }
                }
            }
        }
    }
    let mut pixels = Vec::with_capacity(w * h * 4);
    let n = (ss * ss) as f64;
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0.0; 3];
            for dy in 0..ss {
                for dx in 0..ss {
                    if let Some(p) = rgb.get((y * ss + dy) * sw + x * ss + dx) {
                        for k in 0..3 {
                            sum[k] += p[k];
                        }
                    }
                }
            }
            pixels.extend(sum.map(|s| (s / n).round().clamp(0.0, 255.0) as u8));
            pixels.push(255);
        }
    }
    Rgba { w: w as u32, h: h as u32, pixels }
}

/// One picture asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shot {
    cam: Camera,
    w: u32,
    h: u32,
    ss: u32,
    colours: Colours,
}

/// The mesh viewer (see the module docs).
#[derive(Default)]
pub struct MeshView {
    key: String,
    ctx: Option<egui::Context>,
    reading: Option<Job<Mesh>>,
    mesh: Option<Arc<Mesh>>,
    error: Option<String>,
    cam: Camera,
    /// The picture the view wants, and the one it has.
    want: Option<Shot>,
    shown: Option<(Shot, egui::TextureHandle)>,
    rendering: Option<(Shot, Job<Rgba>)>,
}

impl MeshView {
    fn poll(&mut self) {
        if let Some(done) = self.reading.as_ref().and_then(Job::poll) {
            self.reading = None;
            match done {
                Ok(m) => self.mesh = Some(Arc::new(m)),
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(done) = self.rendering.as_ref().and_then(|(_, j)| j.poll())
            && let (Some((shot, _)), Some(ctx)) = (self.rendering.take(), &self.ctx)
        {
            match done {
                Ok(img) => {
                    let t = ctx.load_texture(format!("mesh-{}", self.key), img.image(), egui::TextureOptions::LINEAR);
                    self.shown = Some((shot, t));
                }
                Err(e) => self.error = Some(e),
            }
        }
        // one picture at a time: a drag asks for many, and gets the latest when one is done
        let (Some(want), Some(mesh), Some(ctx)) = (self.want, &self.mesh, &self.ctx) else { return };
        if self.rendering.is_some() || self.shown.as_ref().is_some_and(|(s, _)| *s == want) {
            return;
        }
        let mesh = Arc::clone(mesh);
        self.rendering = Some((want, Job::spawn(ctx, move || Ok(render(&mesh, want.cam, want.w, want.h, want.ss, want.colours)))));
    }

    fn ask(&mut self, ui: &egui::Ui, size: egui::Vec2, sharp: bool, tokens: &Tokens) {
        let px = size * ui.ctx().pixels_per_point();
        let (w, h) = ((px.x.round() as u32).clamp(1, MAX_SIDE), (px.y.round() as u32).clamp(1, MAX_SIDE));
        self.want = Some(Shot { cam: self.cam, w, h, ss: if sharp { 2 } else { 1 }, colours: Colours::of(tokens) });
        self.poll();
    }

    fn status(&self) -> &'static str {
        match (&self.error, &self.mesh) {
            (Some(_), _) => "error",
            (None, Some(_)) => "ok",
            (None, None) if self.reading.is_some() => "reading",
            _ => "none",
        }
    }
}

impl Viewer for MeshView {
    fn sync(&mut self, ctx: &egui::Context, v: &ViewInput<'_>) {
        if self.ctx.as_ref().is_some_and(|c| c != ctx) {
            self.shown = None;
            self.rendering = None;
        }
        self.ctx = Some(ctx.clone());
        let key = key_of(v.display);
        if key != self.key {
            *self = MeshView { key, ctx: Some(ctx.clone()), cam: self.cam, ..MeshView::default() };
        }
        if self.mesh.is_none()
            && self.reading.is_none()
            && self.error.is_none()
            && let Some(blob) = v.full
        {
            let blob = Arc::clone(blob);
            self.reading = Some(Job::spawn(ctx, move || read_stl(&blob.bytes)));
        }
        self.poll();
    }

    fn preview(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, max: egui::Vec2, tokens: &Tokens) {
        if self.want.is_none() {
            self.ask(ui, egui::vec2(max.x.min(240.0), max.y.min(180.0)), true, tokens);
        }
        self.poll();
        match &self.shown {
            Some((_, t)) => super::fitted_image(ui, t, max),
            None => super::preview_default(ui, v, max, tokens),
        }
    }

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        self.poll();
        if let Some(e) = &self.error {
            ui.colored_label(tokens.error, format!("not an STL file it can read: {e}"));
            return Vec::new();
        }
        let Some(mesh) = self.mesh.clone() else {
            let note = if v.full.is_none() {
                if v.missing { "the mesh is gone (reaped); rerun to publish it" } else { "the mesh's bytes aren't here" }
            } else {
                "reading the mesh…"
            };
            ui.label(egui::RichText::new(note).color(tokens.text_dim));
            return Vec::new();
        };
        ui.horizontal(|ui| {
            if ui.button("Fit").on_hover_text("Back to the first view (or double-click)").clicked() {
                self.cam = Camera::default();
            }
            let (lo, hi) = mesh.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
            let size: Vec<String> = (0..3).map(|k| format!("{:.4}", hi[k] - lo[k]).trim_end_matches('0').trim_end_matches('.').to_owned()).collect();
            let mut about = format!("{} triangles · {} · {}", mesh.triangles.len(), if mesh.ascii { "ASCII" } else { "binary" }, size.join(" × "));
            if mesh.dropped > 0 {
                about.push_str(&format!(" · {} dropped (bad vertices)", mesh.dropped));
            }
            ui.label(egui::RichText::new(about).small().color(tokens.text_faint));
        });
        let size = ui.available_size().max(egui::vec2(64.0, 64.0));
        let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        if r.dragged() {
            let d = r.drag_delta();
            self.cam = self.cam.turned(d.x, d.y);
        }
        if r.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 {
                self.cam = self.cam.zoomed(dy);
            }
        }
        if r.double_clicked() {
            self.cam = Camera::default();
        }
        let moving = r.dragged() || ui.input(|i| i.smooth_scroll_delta.y != 0.0);
        self.ask(ui, size, !moving, tokens);
        ui.painter().rect_filled(rect, 0.0, tokens.canvas);
        if let Some((_, t)) = &self.shown {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(t.id(), rect, uv, if v.missing { egui::Color32::from_gray(140) } else { egui::Color32::WHITE });
        }
        r.on_hover_text("Drag to turn, scroll to zoom, double-click to fit");
        Vec::new()
    }

    fn inspect(&self) -> Value {
        let mut out = json!({
            "state": self.status(),
            "camera": {"yaw": self.cam.yaw, "pitch": self.cam.pitch, "zoom": self.cam.zoom},
            "rendered": self.shown.as_ref().map(|(s, _)| [s.w, s.h]),
        });
        if let Some(m) = &self.mesh {
            out["triangles"] = json!(m.triangles.len());
            out["ascii"] = json!(m.ascii);
            out["name"] = json!(m.name);
            out["dropped"] = json!(m.dropped);
            out["bounds"] = json!(m.bounds());
        }
        if let Some(e) = &self.error {
            out["error"] = json!(e);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::data::{BlobMeta, DataRef};
    use plumcraft_proto::mesh::{tetrahedron, write_binary_stl};

    const COLOURS: Colours = Colours { background: [250, 250, 250], body: [200, 60, 90] };

    fn mesh() -> Mesh {
        Mesh { triangles: tetrahedron(), ..Mesh::default() }
    }

    fn at(img: &Rgba, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * img.w + x) * 4) as usize;
        [img.pixels[i], img.pixels[i + 1], img.pixels[i + 2]]
    }

    fn body_pixels(img: &Rgba) -> usize {
        img.pixels.chunks(4).filter(|p| p[..3] != COLOURS.background).count()
    }

    #[test]
    fn a_mesh_is_drawn_in_the_middle_and_the_corners_are_background() {
        let img = render(&mesh(), Camera::default(), 120, 90, 2, COLOURS);
        assert_eq!((img.w, img.h, img.pixels.len()), (120, 90, 120 * 90 * 4));
        assert_eq!(at(&img, 0, 0), COLOURS.background);
        assert_eq!(at(&img, 119, 89), COLOURS.background);
        let mid = at(&img, 60, 45);
        assert_ne!(mid, COLOURS.background, "the tetrahedron covers the middle");
        assert!(mid[0] > mid[1] && mid[0] > mid[2], "shaded body colour: {mid:?}");
        // faces turned differently are shaded differently
        let mut shades: Vec<[u8; 3]> = img.pixels.chunks(4).map(|p| [p[0], p[1], p[2]]).filter(|p| *p != COLOURS.background).collect();
        shades.sort();
        shades.dedup();
        assert!(shades.len() >= 2);
    }

    #[test]
    fn turning_and_zooming_change_the_picture() {
        let first = render(&mesh(), Camera::default(), 80, 80, 1, COLOURS);
        let turned = render(&mesh(), Camera::default().turned(120.0, 40.0), 80, 80, 1, COLOURS);
        assert_ne!(first.pixels, turned.pixels);
        let near = render(&mesh(), Camera::default().zoomed(400.0), 80, 80, 1, COLOURS);
        assert!(body_pixels(&near) > body_pixels(&first), "zoomed in, it fills more");
        let far = render(&mesh(), Camera::default().zoomed(-400.0), 80, 80, 1, COLOURS);
        assert!(body_pixels(&far) < body_pixels(&first));
        // the pitch stops short of the poles, and any yaw is fine
        let c = Camera::default().turned(1e6, 1e6);
        assert!(c.pitch < std::f32::consts::FRAC_PI_2 && c.yaw.is_finite());
    }

    #[test]
    fn the_nearer_face_hides_the_farther() {
        // two squares facing the viewer, one behind the other, in different colours: z wins
        let cam = Camera { yaw: 0.0, pitch: 0.0, zoom: 1.0 }; // looking along +Y
        let square = |y: f32, s: f32| -> Vec<[[f32; 3]; 3]> { vec![[[-s, y, -s], [s, y, -s], [s, y, s]], [[-s, y, -s], [s, y, s], [-s, y, s]]] };
        let mut tris = square(5.0, 1.0); // far, large
        tris.extend(square(-5.0, 0.4)); // near, small
        let img = render(&Mesh { triangles: tris.clone(), ..Mesh::default() }, cam, 100, 100, 1, COLOURS);
        let reversed: Vec<_> = tris.into_iter().rev().collect();
        let img2 = render(&Mesh { triangles: reversed, ..Mesh::default() }, cam, 100, 100, 1, COLOURS);
        assert_eq!(img.pixels, img2.pixels, "the order the triangles come in doesn't matter");
    }

    #[test]
    fn degenerate_and_huge_inputs_never_panic() {
        let flat = Mesh { triangles: vec![[[0.0; 3]; 3], [[1.0, 1.0, 1.0]; 3]], ..Mesh::default() };
        let img = render(&flat, Camera::default(), 10, 10, 2, COLOURS);
        assert_eq!(body_pixels(&img), 0);
        let _ = render(&Mesh::default(), Camera::default(), 0, 0, 0, COLOURS);
        let big = Mesh {
            triangles: vec![[[-1e8, -1e8, 0.0], [1e8, -1e8, 0.0], [0.0, 1e8, 1e8]], [[0.0; 3], [1e-30, 0.0, 0.0], [0.0, 1e-30, 0.0]]],
            ..Mesh::default()
        };
        let img = render(&big, Camera::default().zoomed(1e9), 64, 64, 2, COLOURS);
        assert_eq!(img.pixels.len(), 64 * 64 * 4);
        let img = render(&mesh(), Camera::default(), 99_999, 99_999, 9, COLOURS);
        assert_eq!((img.w, img.h), (MAX_SIDE, MAX_SIDE));
    }

    #[test]
    fn stl_values_are_accepted_by_media_type() {
        let display = |media: &str, kind: Media| Display {
            media: kind,
            full: Some(DataRef { meta: BlobMeta { media: media.into(), ..Default::default() }, ..Default::default() }),
            ..Default::default()
        };
        assert!(accepts(&display("model/stl", Media::Bytes)));
        assert!(accepts(&display("model/stl", Media::Mesh)));
        assert!(accepts(&display("application/sla", Media::Bytes)));
        assert!(!accepts(&display("model/obj", Media::Mesh)));
        assert!(!accepts(&display("text/plain", Media::Text)));
        assert!(!accepts(&Display { media: Media::Mesh, ..Default::default() }), "no bytes");
        let ids: Vec<&str> = super::super::kinds_for(&display("model/stl", Media::Bytes)).iter().map(|k| k.id).collect();
        assert_eq!(ids.first(), Some(&"mesh"), "{ids:?}");
        // the file reads, binary
        assert_eq!(read_stl(&write_binary_stl("", &tetrahedron())).map(|m| m.triangles.len()), Ok(4));
    }
}
