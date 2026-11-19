//! A picture as plain bytes: what the core decodes and computes (a difference, a probe's view),
//! and a shell turns into its own texture (the egui shell: `plumcraft_ui::media::RgbaImage`).
//! Moved from `plumcraft-ui`'s `media` (stage N1).

/// A picture: RGBA8, unpremultiplied, `w × h`.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub pixels: Vec<u8>,
}
