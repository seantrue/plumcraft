//! The difference |A − B| of two full-resolution values. The logic moved to the core
//! ([`plumcraft_editor_core::diff`], stage N1); this re-exports it and takes the NaN colour as
//! egui's.

use egui::Color32;

use crate::plain::ToCore;
use crate::probe::Pixels;

pub use plumcraft_editor_core::diff::*;
use plumcraft_session::viewer_cmds::DiffMode;

/// |A − B| of two pictures of one shape. `stop` is polled between rows: a superseded compare
/// gives up rather than finish.
pub fn diff(a: &Pixels, b: &Pixels, mode: DiffMode, max_px: usize, nan_colour: Color32, stop: &dyn Fn() -> bool) -> Result<Diff, String> {
    plumcraft_editor_core::diff::diff(a, b, mode, max_px, nan_colour.to_core(), stop)
}
