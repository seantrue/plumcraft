//! Render the editor without its window: a second egui context draws the app a few frames at
//! the window's size, and egui_kittest's wgpu renderer turns the last frame into pixels. It
//! works while the screen is locked, when `ViewportCommand::Screenshot` never answers, so the
//! control channel's `ui.screenshot` and `--screenshot` fall back to it
//! ([`crate::control`]). The same renderer as `examples/snapshot.rs`.
//!
//! Textures loaded on the window's context (thumbnails) mean nothing to the second context, so
//! the app's thumbnail cache is swapped out while it draws and put back after, and value editors
//! let go of theirs. The view
//! transform is carried over, so the picture shows what the window shows.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use egui_kittest::TestRenderer;
use egui_kittest::wgpu::WgpuTestRenderer;

use crate::EditorApp;

/// Frames drawn before the picture is taken: node sizes are measured on the first frames.
pub const FRAMES: usize = 8;
/// The size used when the window's is unknown.
pub const DEFAULT_SIZE: egui::Vec2 = egui::vec2(1440.0, 860.0);
/// The largest picture side, in pixels.
const MAX_PIXELS: f32 = 8192.0;

/// Panels whose sizes the window's context remembers (a resized Watched panel).
const PANELS: &[&str] = &["plumcraft-watched"];

/// As the window draws, but for dithering. egui_kittest's default filters every texture bilinearly
/// in its shader, whatever the texture asked for, so a picture zoomed to nearest-neighbour blocks
/// would come out blurred here and sharp on screen.
fn render_options() -> eframe::egui_wgpu::RendererOptions {
    eframe::egui_wgpu::RendererOptions { msaa_samples: 1, dithering: false, ..Default::default() }
}

/// Draw `app` offscreen at `size` points and `pixels_per_point`, and return the picture.
/// `like` is the window's context, whose panel sizes the picture keeps.
pub fn render(app: &mut EditorApp, size: egui::Vec2, pixels_per_point: f32, like: Option<&egui::Context>) -> Result<egui::ColorImage, String> {
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 { pixels_per_point.clamp(0.5, 4.0) } else { 1.0 };
    let size = if size.x.is_finite() && size.y.is_finite() && size.x >= 1.0 && size.y >= 1.0 { size } else { DEFAULT_SIZE };
    let size = size.min(egui::Vec2::splat(MAX_PIXELS / ppp));
    // The renderer panics when there is no adapter: that's an error here, not a crash.
    let state = catch_unwind(|| egui_kittest::wgpu::create_render_state(egui_kittest::wgpu::default_wgpu_setup(), render_options()))
        .map_err(|_| "offscreen rendering: no GPU adapter could be set up".to_owned())?;
    // A context takes 2048 as the largest texture until it's told: this GPU's, as a window's is.
    let max_texture_side = usize::try_from(state.device.limits().max_texture_dimension_2d).ok();
    let mut renderer = WgpuTestRenderer::from_render_state(state);
    let ctx = egui::Context::default();
    app.setup_context(&ctx);
    if let Some(like) = like {
        for name in PANELS {
            let id = egui::Id::new(*name);
            if let Some(state) = egui::containers::panel::PanelState::load(like, id) {
                ctx.data_mut(|d| d.insert_persisted(id, state));
            }
        }
    }
    // A few frames milliseconds apart: no animation would finish (a window would still be
    // fading in), so none run.
    ctx.all_styles_mut(|s| s.animation_time = 0.0);
    let thumbs = std::mem::take(&mut app.thumbs);
    app.value_editors.forget_textures();
    let (transform, rect) = (app.canvas.to_global, app.canvas.rect);
    let was_offscreen = std::mem::replace(&mut app.offscreen, true);
    let start = Instant::now();
    let mut last = None;
    for _ in 0..FRAMES {
        // every frame: egui discards the first pass of a fresh canvas, and the snarl starts
        // centred on its nodes
        app.canvas.set_transform = Some(transform);
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(start.elapsed().as_secs_f64()),
            max_texture_side,
            ..Default::default()
        };
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        let mut out = ctx.run_ui(raw, |ui| app.show(ui));
        renderer.handle_delta(&mut out.textures_delta);
        last = Some(out);
    }
    app.offscreen = was_offscreen;
    app.thumbs = thumbs;
    app.value_editors.forget_textures();
    // The window's own view, unchanged.
    app.canvas.set_transform = Some(transform);
    app.canvas.to_global = transform;
    app.canvas.rect = rect;
    let out = last.ok_or("offscreen rendering drew no frame")?;
    let image = catch_unwind(AssertUnwindSafe(|| renderer.render(&ctx, &out))).map_err(|_| "offscreen rendering failed".to_owned())??;
    let px = [image.width() as usize, image.height() as usize];
    let pixels = image
        .pixels()
        .map(|p| {
            let [r, g, b, a] = p.0;
            egui::Color32::from_rgba_unmultiplied(r, g, b, a)
        })
        .collect();
    Ok(egui::ColorImage::new(px, pixels))
}
