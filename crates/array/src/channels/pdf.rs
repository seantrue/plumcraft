//! PDF into and out of arrays (`docs/research/20-convert.md`, 6.2): Rasterize PDF draws a page
//! (with hayro, pure Rust, as the editor's PDF viewer does) as rgb, alpha and rgba, as Decode
//! Image does a PNG; Array to PDF, its inverse, makes a one-page PDF of a picture (8 bits a
//! channel, the page as large as the picture at `dpi`), so Rasterize PDF at that `dpi` gives
//! the same pixels back. A PDF it can't read (encrypted, malformed) is a `config` error, and no
//! page is drawn past [`MAX_PIXELS`].

use std::io::Write as _;
use std::sync::Arc;

use plumcraft_engine::{Blob, BlobSource, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::Handle;
use plumcraft_proto::catalogue::{Port, Socket};
use plumcraft_proto::data::{BlobMeta, Form};
use plumcraft_proto::error::NodeError;

use super::{MAX_FILE, picture_outputs};
use crate::img::Img;
use crate::op::{Op, array_in, array_out, config, img, int_in, n, op_error, out};

/// The PDF type, as the craft packs declare it (bytes).
pub const PDF: &str = "PDF";
/// The most pixels a page is drawn at; a larger `dpi` is lowered to fit.
pub const MAX_PIXELS: f32 = 64.0 * 1024.0 * 1024.0;

pub fn nodes() -> Vec<Box<dyn Node>> {
    vec![
        Box::new(Op {
            id: "array.rasterize_pdf",
            display: "Rasterize PDF",
            group: "Arrays",
            description: "A page of a PDF (from WordCraft, GridCraft, PdfCraft or any engine) drawn as arrays, on white, as Decode Image reads a PNG: rgb (H × W × 3, 0-1), alpha, and rgba. Drawn at dpi dots per inch, lowered to fit 64 M pixels. The inverse of Array to PDF.",
            inputs: vec![
                Port { name: "pdf".into(), ty: PDF.into(), socket: Some(Socket::Socket), tooltip: Some("The PDF".into()), ..Default::default() },
                int_in("page", 1, 1.0, 100_000.0, "The page, from 1"),
                int_in("dpi", 144, 18.0, 1200.0, "Dots per inch: 72 draws a point as a pixel"),
            ],
            outputs: vec![
                array_out("rgb", "Red, green and blue, 0-1"),
                array_out("alpha", "Opacity: 1 everywhere, as the page is drawn on white"),
                array_out("rgba", "Red, green, blue and alpha, 0-1"),
            ],
            run: rasterize,
        }),
        Box::new(Op {
            id: "array.array_to_pdf",
            display: "Array to PDF",
            group: "Arrays",
            description: "A picture (RGB or one channel, 0-1) as a one-page PDF, 8 bits a channel, the page as large as the picture at dpi. The inverse of Rasterize PDF.",
            inputs: vec![
                array_in("image", "The picture: RGB, or one channel (grey)"),
                int_in("dpi", 72, 18.0, 1200.0, "Dots per inch: the page's size is the picture's at this"),
            ],
            outputs: vec![out("pdf", PDF, "The PDF")],
            run: to_pdf,
        }),
    ]
}

fn rasterize(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
    let blob = i.get("pdf").and_then(Data::blob).ok_or_else(|| config("pdf: wire a PDF (an export's, or Array to PDF's)"))?;
    let bytes = blob.read(MAX_FILE)?;
    let pdf = hayro::hayro_syntax::Pdf::new(bytes).map_err(|e| config(format!("pdf: not a PDF this engine can read ({e:?})")))?;
    let pages = pdf.pages();
    let page = usize::try_from(n(i, "page", 1)).unwrap_or(1).max(1);
    let p = pages.get(page - 1).ok_or_else(|| config(format!("page {page}: the PDF has {} page(s)", pages.len())))?;
    let (w, h) = p.render_dimensions();
    let wanted = n(i, "dpi", 144).clamp(18, 1200) as f32 / 72.0;
    let scale = wanted.min((MAX_PIXELS / (w * h).max(1.0)).sqrt()).max(0.01);
    let pixmap = render(
        p,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE },
    );
    let (pw, ph) = (usize::from(pixmap.width()), usize::from(pixmap.height()));
    let data = pixmap.data_as_u8_slice().iter().map(|v| f32::from(*v) / 255.0).collect();
    picture_outputs(Img { h: ph, w: pw, c: 4, data, ..Default::default() }, false, "white")
}

/// A one-page PDF of 8-bit samples, `channels` 1 (grey) or 3 (RGB), `w` × `h` pixels on a page
/// of `pw` × `ph` points.
pub fn one_page_pdf(samples: &[u8], w: usize, h: usize, channels: usize, pw: f32, ph: f32) -> Result<Vec<u8>, String> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(samples).map_err(|e| e.to_string())?;
    let image = z.finish().map_err(|e| e.to_string())?;
    let space = if channels == 1 { "/DeviceGray" } else { "/DeviceRGB" };
    let content = format!("q {pw:.4} 0 0 {ph:.4} 0 0 cm /Im0 Do Q\n");
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, head: String, stream: Option<&[u8]>| {
        offsets.push(out.len());
        out.extend_from_slice(head.as_bytes());
        if let Some(s) = stream {
            out.extend_from_slice(b"stream\n");
            out.extend_from_slice(s);
            out.extend_from_slice(b"\nendstream\n");
        }
        out.extend_from_slice(b"endobj\n");
    };
    object(&mut out, "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\n".into(), None);
    object(&mut out, "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\n".into(), None);
    object(
        &mut out,
        format!("3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.4} {ph:.4}] /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\n"),
        None,
    );
    object(&mut out, format!("4 0 obj\n<< /Length {} >>\n", content.len()), Some(content.as_bytes()));
    object(
        &mut out,
        format!(
            "5 0 obj\n<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace {space} /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\n",
            image.len()
        ),
        Some(&image),
    );
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
    Ok(out)
}

fn to_pdf(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = img(i, "image")?;
    if a.table || a.labels {
        return Err(config("image: a table or labels is not a picture"));
    }
    if a.c == 3 && a.space.as_deref().is_some_and(|s| s != "RGB") {
        return Err(config(format!("image: {}: convert to RGB first (Convert Color Space)", a.describe())));
    }
    let to8 = |v: &f32| if v.is_nan() { 0 } else { (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8 };
    let (samples, channels): (Vec<u8>, usize) = match a.c {
        1 | 3 => (a.data.iter().map(to8).collect(), a.c),
        // the page is opaque: the colours under the alpha
        4 => (a.data.chunks(4).flat_map(|p| p.iter().take(3).map(to8).collect::<Vec<_>>()).collect(), 3),
        c => return Err(config(format!("image: 1, 3 or 4 channels, not {c}"))),
    };
    let per_point = 72.0 / n(i, "dpi", 72).clamp(18, 1200) as f32;
    let bytes =
        one_page_pdf(&samples, a.w, a.h, channels, a.w as f32 * per_point, a.h as f32 * per_point).map_err(|e| op_error(format!("writing the PDF: {e}")))?;
    let blob = Blob {
        handle: Handle::md5_raw(&bytes),
        meta: BlobMeta { media: "application/pdf".into(), size: bytes.len() as u64, form: Form::default(), aliases: Vec::new() },
        source: BlobSource::Bytes(Arc::from(bytes)),
        publish: true,
        thumb: None,
    };
    Ok(Outputs::from([("pdf".to_owned(), Data::Blob(Box::new(blob)))]))
}
