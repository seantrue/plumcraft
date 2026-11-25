//! PlumCraft's charts as data (note 26; the maintainer's decision 1A, "the core lays charts out,
//! each shell paints them", and for face previews option A, "Chart: a"): a value's chart laid out
//! into a display list ([`layout`]), the number text it writes ([`number`]), the array reading it
//! needs ([`dtype`]), and with the `raster` feature the list drawn into a picture ([`raster`]).
//!
//! A crate of its own so that the editor core and the Rust engine SDK lay a chart out the same
//! way: the core for its viewers, the SDK for a node's face thumbnail. It holds no GUI toolkit and
//! no engine (golden rule 3), and depends on nothing but the protocol crate, serde and, for
//! `raster`, tiny-skia (pure Rust). Every input is data from the other side: nothing here panics.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod dtype;
pub mod layout;
#[cfg(feature = "raster")]
pub mod raster;
pub mod span;

pub use layout::*;
pub use span::{ChartRange, Span};

/// A number as the viewers show it: an integer whole; a float at the decimals it needs, at most
/// six; past 1e9 or under 1e-4, four significant decimals in scientific notation; NaN and the
/// infinities by name.
pub fn number(x: f64, int: bool) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if int && x.fract() == 0.0 && x.abs() < 9.0e15 {
        return format!("{}", x as i64);
    }
    let a = x.abs();
    if a != 0.0 && !(1e-4..1e9).contains(&a) {
        return format!("{x:.4e}");
    }
    let d = needed_decimals(x);
    format!("{x:.d$}")
}

/// The most decimals a number's text needs.
pub const MOST_NEEDED: usize = 6;

/// The fewest decimals (at most [`MOST_NEEDED`]) that write `x` exactly enough: no float noise,
/// nothing rounded away.
pub fn needed_decimals(x: f64) -> usize {
    if !x.is_finite() {
        return 0;
    }
    let tol = 1e-9 * x.abs().max(1.0);
    (0..=MOST_NEEDED)
        .find(|d| {
            let m = 10f64.powi(*d as i32);
            ((x * m).round() / m - x).abs() <= tol
        })
        .unwrap_or(MOST_NEEDED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_number_text() {
        assert_eq!(number(3.0, true), "3");
        assert_eq!(number(0.375, false), "0.375");
        assert_eq!(number(1.0 / 3.0, false), "0.333333");
        assert_eq!(number(2e10, false), "2.0000e10");
        assert_eq!(number(f64::NAN, false), "NaN");
        assert_eq!(number(f64::NEG_INFINITY, false), "-inf");
    }
}
