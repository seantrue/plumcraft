//! The pixel probe: where a point on a shown picture falls in the full array, and the exact
//! values stored there. The logic moved to the core ([`plumcraft_editor_core::probe`], stage N1);
//! this re-exports it and takes egui's points and rects where the core takes its own.

use egui::{Pos2, Rect};

use crate::plain::{ToCore, ToEgui};

pub use plumcraft_editor_core::probe::*;

/// The array pixel (row, col) under `pos`, for a picture of `rows` × `cols` drawn over `rect`.
/// `None` outside the rect. The right and bottom edges belong to the last pixel.
pub fn display_to_array(pos: Pos2, rect: Rect, rows: usize, cols: usize) -> Option<(usize, usize)> {
    plumcraft_editor_core::probe::display_to_array(pos.to_core(), rect.to_core(), rows, cols)
}

/// The screen rect of array pixel (row, col) in a picture drawn over `rect`.
pub fn array_to_display(row: usize, col: usize, rect: Rect, rows: usize, cols: usize) -> Rect {
    plumcraft_editor_core::probe::array_to_display(row, col, rect.to_core(), rows, cols).to_egui()
}
