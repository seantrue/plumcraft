//! PDF pages (`application/pdf` values: GridCraft's, PdfCraft's, VectorCraft's exports): each page
//! rendered by hayro (pure Rust, on a worker thread: its types stay there), with a page stepper,
//! zoom and pan. A file it can't read (encrypted, malformed) says why; nothing panics on a
//! hostile PDF, and a page is never rendered past [`MAX_PAGE_PIXELS`].

use serde_json::{Value, json};

use super::{ViewInput, Viewer};
use crate::canvas::Action;
use crate::media::{Job, Rgba, ZoomPan, draw_picture, key_of};
use crate::theme::Tokens;

/// Points to pixels: 2 renders at 144 dpi.
pub const SCALE: f32 = 2.0;
/// The most pixels a page is rendered to (it is scaled down to fit).
pub const MAX_PAGE_PIXELS: f32 = 24_000_000.0;
/// The largest PDF read.
pub const MAX_PDF_BYTES: usize = 256 << 20;

/// Render page `page` of `bytes`: the page count and the page's pixels.
pub fn render_page(bytes: Vec<u8>, page: usize) -> Result<(usize, Rgba), String> {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
    if bytes.len() > MAX_PDF_BYTES {
        return Err(format!("larger than {} MB", MAX_PDF_BYTES >> 20));
    }
    let pdf = hayro::hayro_syntax::Pdf::new(bytes).map_err(|e| format!("not a PDF it can read: {e:?}"))?;
    let pages = pdf.pages();
    let count = pages.len();
    let p = pages.get(page).ok_or_else(|| format!("no page {} (it has {count})", page + 1))?;
    let (w, h) = p.render_dimensions();
    let scale = SCALE.min((MAX_PAGE_PIXELS / (w * h).max(1.0)).sqrt()).max(0.05);
    let cache = RenderCache::new();
    let pixmap =
        render(p, &cache, &InterpreterSettings::default(), &RenderSettings::default(), &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE });
    let (pw, ph) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    Ok((count, Rgba { w: pw, h: ph, pixels: pixmap.data_as_u8_slice().to_vec() }))
}

/// A PDF's pages (see the module docs).
#[derive(Default)]
pub struct Pages {
    key: String,
    bytes: Option<std::sync::Arc<Vec<u8>>>,
    count: Option<usize>,
    page: usize,
    shown: Option<(usize, egui::TextureHandle)>,
    job: Option<(usize, Job<(usize, Rgba)>)>,
    error: Option<String>,
    zoom: ZoomPan,
    ctx: Option<egui::Context>,
}

impl Pages {
    fn start(&mut self) {
        let (Some(ctx), Some(bytes)) = (&self.ctx, &self.bytes) else { return };
        if self.job.as_ref().is_some_and(|(p, _)| *p == self.page) || self.shown.as_ref().is_some_and(|(p, _)| *p == self.page) {
            return;
        }
        let (bytes, page) = (std::sync::Arc::clone(bytes), self.page);
        self.job = Some((page, Job::spawn(ctx, move || render_page(bytes.to_vec(), page))));
    }

    fn poll(&mut self) {
        let Some((page, job)) = &self.job else { return };
        let page = *page;
        let Some(done) = job.poll() else { return };
        self.job = None;
        match done {
            Ok((count, img)) => {
                self.count = Some(count);
                self.error = None;
                if let Some(ctx) = &self.ctx {
                    let t = ctx.load_texture(format!("pdf-page-{}-{page}", self.key), img.image(), egui::TextureOptions::LINEAR);
                    self.shown = Some((page, t));
                }
            }
            Err(e) => self.error = Some(e),
        }
    }
}

impl Viewer for Pages {
    fn sync(&mut self, ctx: &egui::Context, v: &ViewInput<'_>) {
        // Another context (an offscreen picture of the editor): its textures are its own.
        if self.ctx.as_ref().is_some_and(|c| c != ctx) {
            self.shown = None;
            self.job = None;
        }
        self.ctx = Some(ctx.clone());
        let key = key_of(v.display);
        if key != self.key {
            *self = Pages { key, ctx: Some(ctx.clone()), ..Pages::default() };
            self.bytes = v.full.map(|b| std::sync::Arc::new(b.bytes.to_vec()));
        }
        self.poll();
        self.start();
    }

    fn preview(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, max: egui::Vec2, tokens: &Tokens) {
        self.poll();
        match &self.shown {
            Some((_, t)) => super::fitted_image(ui, t, max),
            None => super::preview_default(ui, v, max, tokens),
        }
    }

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        self.poll();
        if self.bytes.is_none() {
            ui.label(
                egui::RichText::new(if v.missing { "the PDF is gone (reaped); rerun to publish it" } else { "the PDF's bytes aren't here" })
                    .color(tokens.text_dim),
            );
            return Vec::new();
        }
        let count = self.count.unwrap_or(0);
        ui.horizontal(|ui| {
            if ui.add_enabled(self.page > 0, egui::Button::new("◀")).on_hover_text("The previous page").clicked() {
                self.page -= 1;
            }
            let of = if count > 0 { count.to_string() } else { "…".into() };
            ui.label(egui::RichText::new(format!("page {} of {of}", self.page + 1)).color(tokens.text));
            if ui.add_enabled(self.page + 1 < count, egui::Button::new("▶")).on_hover_text("The next page").clicked() {
                self.page += 1;
            }
            if ui.button("Fit").clicked() {
                self.zoom.fit();
            }
        });
        self.start();
        if let Some(e) = &self.error {
            ui.colored_label(tokens.error, e);
        }
        match &self.shown {
            Some((page, t)) => {
                let [w, h] = t.size();
                draw_picture(ui, &mut self.zoom, t, egui::vec2(w as f32, h as f32), &format!("page {}", page + 1), tokens);
            }
            None if self.error.is_none() => {
                ui.add(egui::Spinner::new());
            }
            None => {}
        }
        Vec::new()
    }

    fn inspect(&self) -> Value {
        json!({"page": self.page, "pages": self.count, "rendered": self.shown.as_ref().map(|(p, t)| json!({"page": p, "size": t.size()})), "error": self.error})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page PDF (200 × 100 pt) with a red rectangle in its left half, xref offsets
    /// computed so it is well-formed.
    pub fn tiny_pdf() -> Vec<u8> {
        let content = "1 0 0 rg 0 0 100 100 re f";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_owned(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
        out
    }

    #[test]
    fn a_page_renders_at_twice_its_size() {
        let (count, img) = render_page(tiny_pdf(), 0).unwrap();
        assert_eq!((count, img.w, img.h), (1, 400, 200));
        let px = |x: u32, y: u32| &img.pixels[((y * img.w + x) * 4) as usize..((y * img.w + x) * 4 + 3) as usize];
        assert_eq!(px(50, 100), &[255, 0, 0], "the red half");
        assert_eq!(px(350, 100), &[255, 255, 255], "the white half");
        assert!(render_page(tiny_pdf(), 1).unwrap_err().contains("no page 2"));
    }

    #[test]
    fn junk_and_missing_pages_are_errors_not_panics() {
        assert!(render_page(b"not a pdf".to_vec(), 0).is_err());
        assert!(render_page(Vec::new(), 0).is_err());
    }
}
