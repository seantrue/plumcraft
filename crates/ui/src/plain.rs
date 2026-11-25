//! Between the core's plain types ([`plumcraft_editor_core::plain`]) and egui's.
//!
//! Traits, not `From`: both sides are other crates' types, so Rust's orphan rule allows neither
//! `From<egui::Color32> for Color` nor the reverse here, and the core may not name egui. Each
//! conversion is exact: a [`Color`] holds the same premultiplied bytes as a `Color32`.

use egui::{Color32, Pos2, Rect as EguiRect, Vec2};
use plumcraft_editor_core::plain::{Color, Point, Rect};

/// A core value as egui's.
pub trait ToEgui {
    type Egui;
    fn to_egui(self) -> Self::Egui;
}

/// An egui value as the core's.
pub trait ToCore {
    type Core;
    fn to_core(self) -> Self::Core;
}

impl ToEgui for Color {
    type Egui = Color32;
    fn to_egui(self) -> Color32 {
        Color32::from_rgba_premultiplied(self.r, self.g, self.b, self.a)
    }
}

impl ToCore for Color32 {
    type Core = Color;
    fn to_core(self) -> Color {
        let [r, g, b, a] = self.to_array();
        Color::from_rgba_premultiplied(r, g, b, a)
    }
}

impl ToEgui for Point {
    type Egui = Pos2;
    fn to_egui(self) -> Pos2 {
        Pos2::new(self.x, self.y)
    }
}

impl ToCore for Pos2 {
    type Core = Point;
    fn to_core(self) -> Point {
        Point::new(self.x, self.y)
    }
}

impl ToCore for Vec2 {
    type Core = Point;
    fn to_core(self) -> Point {
        Point::new(self.x, self.y)
    }
}

impl ToEgui for Rect {
    type Egui = EguiRect;
    fn to_egui(self) -> EguiRect {
        EguiRect::from_min_max(self.min.to_egui(), self.max.to_egui())
    }
}

impl ToCore for EguiRect {
    type Core = Rect;
    fn to_core(self) -> Rect {
        Rect::from_min_max(self.min.to_core(), self.max.to_core())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    /// The core's colour arithmetic is egui's, byte for byte.
    #[test]
    fn colours_convert_exactly_and_premultiply_as_egui_does() {
        for a in 0..=255u8 {
            for v in (0..=255u8).step_by(3) {
                let (r, g, b) = (v, 255 - v, v / 2);
                let ours = Color::from_rgba_unmultiplied(r, g, b, a);
                let theirs = Color32::from_rgba_unmultiplied(r, g, b, a);
                assert_eq!(ours.to_egui(), theirs, "{r} {g} {b} {a}");
                assert_eq!(theirs.to_core(), ours);
            }
        }
        assert_eq!(Color::from_gray(7).to_egui(), Color32::from_gray(7));
        assert_eq!(Color::BLACK.to_egui(), Color32::BLACK);
        assert_eq!(Color::TRANSPARENT.to_egui(), Color32::TRANSPARENT);
    }

    /// The core's geometry is egui's, to the bit.
    #[test]
    fn points_and_rects_measure_as_egui_does() {
        let pts = [pos2(0.0, 0.0), pos2(3.25, -7.5), pos2(1e-3, 1e6), pos2(-0.1, 0.3), pos2(123.456, 789.01)];
        for a in pts {
            for b in pts {
                let (ca, cb) = (a.to_core(), b.to_core());
                assert_eq!(ca.distance(cb).to_bits(), a.distance(b).to_bits());
                assert_eq!((cb - ca).length_sq().to_bits(), (b - a).length_sq().to_bits());
                assert_eq!((cb - ca).dot(ca).to_bits(), (b - a).dot(a.to_vec2()).to_bits());
                assert_eq!(((cb - ca) * 0.3 / 7.0 + ca).to_egui(), a + (b - a) * 0.3 / 7.0);
                let r = EguiRect::from_min_size(a, vec2(b.x.abs(), b.y.abs()));
                let cr = r.to_core();
                assert_eq!(cr.to_egui(), r);
                assert_eq!(cr.center().to_egui(), r.center());
                assert_eq!((cr.width(), cr.height()), (r.width(), r.height()));
                assert_eq!(cr.contains(cb), r.contains(b));
            }
        }
    }
}
