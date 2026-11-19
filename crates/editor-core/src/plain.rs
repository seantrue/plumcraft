//! The core's own plain types for what a toolkit would otherwise lend it: a colour, a point and a
//! rect (`docs/research/24-native-shells.md` section 3, rule 1). Each is `Copy` and holds only
//! numbers; a shell converts them to its own (the egui shell in `plumcraft_ui::plain`).
//!
//! They compute exactly as egui's `Color32`, `Pos2`/`Vec2` and `Rect` do, operation for
//! operation, so moving logic here changes no pixel and no hit test: the egui shell's tests hold
//! the two to the same results.

use std::ops::{Add, Div, Mul, Sub};

/// An sRGB colour with **premultiplied** alpha, stored as egui's `Color32` stores it, so the
/// conversion either way is the same four bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Nothing drawn.
    pub const TRANSPARENT: Color = Color::from_rgba_premultiplied(0, 0, 0, 0);
    pub const BLACK: Color = Color::from_rgb(0, 0, 0);

    /// Opaque.
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    /// Opaque grey.
    pub const fn from_gray(l: u8) -> Color {
        Color::from_rgb(l, l, l)
    }

    /// From premultiplied channels, as given.
    pub const fn from_rgba_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    /// From unmultiplied channels: each colour channel times `a`, rounded (egui's fixed-point
    /// `mul_frac_round`).
    pub const fn from_rgba_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> Color {
        match a {
            0 => Color::TRANSPARENT,
            255 => Color::from_rgb(r, g, b),
            a => Color::from_rgba_premultiplied(mul_frac_round(r, a), mul_frac_round(g, a), mul_frac_round(b, a), a),
        }
    }

    /// `[r, g, b, a]`, premultiplied.
    pub const fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

/// `val × frac / 255`, rounded, in fixed point (as egui computes it).
const fn mul_frac_round(val: u8, frac: u8) -> u8 {
    let p = (val as u16) * (frac as u16) + 128;
    ((p + (p >> 8)) >> 8) as u8
}

/// A point, or a step between two (egui's `Pos2` and `Vec2` in one).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    /// The length as a step.
    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    pub fn length_sq(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    pub fn dot(self, other: Point) -> f32 {
        self.x * other.x + self.y * other.y
    }

    pub fn distance(self, other: Point) -> f32 {
        (self - other).length()
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point { x: self.x + o.x, y: self.y + o.y }
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point { x: self.x - o.x, y: self.y - o.y }
    }
}

impl Mul<f32> for Point {
    type Output = Point;
    fn mul(self, f: f32) -> Point {
        Point { x: self.x * f, y: self.y * f }
    }
}

impl Div<f32> for Point {
    type Output = Point;
    fn div(self, f: f32) -> Point {
        Point { x: self.x / f, y: self.y / f }
    }
}

/// An axis-aligned rect from `min` (top left) to `max`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}

impl Rect {
    pub const fn from_min_max(min: Point, max: Point) -> Rect {
        Rect { min, max }
    }

    pub fn from_min_size(min: Point, size: Point) -> Rect {
        Rect { min, max: min + size }
    }

    /// Can be negative.
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    /// Can be negative.
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn center(&self) -> Point {
        Point::new((self.min.x + self.max.x) / 2.0, (self.min.y + self.max.y) / 2.0)
    }

    /// Inside or on an edge.
    pub fn contains(&self, p: Point) -> bool {
        self.min.x <= p.x && p.x <= self.max.x && self.min.y <= p.y && p.y <= self.max.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmultiplied_colours_premultiply_with_rounding() {
        assert_eq!(Color::from_rgba_unmultiplied(9, 8, 7, 0), Color::TRANSPARENT);
        assert_eq!(Color::from_rgba_unmultiplied(9, 8, 7, 255), Color::from_rgb(9, 8, 7));
        assert_eq!(Color::from_rgba_unmultiplied(255, 128, 0, 128).to_array(), [128, 64, 0, 128]);
    }

    #[test]
    fn points_and_rects_measure() {
        let (a, b) = (Point::new(1.0, 2.0), Point::new(4.0, 6.0));
        assert_eq!(a.distance(b), 5.0);
        assert_eq!((b - a).length_sq(), 25.0);
        assert_eq!((b - a).dot(Point::new(1.0, 0.0)), 3.0);
        let r = Rect::from_min_size(a, Point::new(3.0, 4.0));
        assert_eq!(r.max, b);
        assert!(r.contains(b) && r.contains(a) && !r.contains(Point::new(0.0, 2.0)));
        assert_eq!((r.width(), r.height()), (3.0, 4.0));
    }
}
