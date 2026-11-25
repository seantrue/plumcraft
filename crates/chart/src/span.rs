//! Spans of numbers and the part of a chart shown (note 26 slice 8: `viewer.range`), kept as
//! their bits so a view's state stays `Eq`.

use serde::{Serialize, Serializer};

/// A span of numbers, `lo < hi`, both finite; kept as their bits so a view's state stays `Eq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    lo: u64,
    hi: u64,
}

impl Span {
    pub fn new(lo: f64, hi: f64) -> Option<Span> {
        (lo.is_finite() && hi.is_finite() && lo < hi).then(|| Span { lo: lo.to_bits(), hi: hi.to_bits() })
    }
    pub fn lo(self) -> f64 {
        f64::from_bits(self.lo)
    }
    pub fn hi(self) -> f64 {
        f64::from_bits(self.hi)
    }
    pub fn contains(self, v: f64) -> bool {
        v >= self.lo() && v <= self.hi()
    }
}

impl Serialize for Span {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        [self.lo(), self.hi()].serialize(s)
    }
}

/// What part of a chart a pop-out shows; `None`: all of it, as the core fits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct ChartRange {
    pub x: Option<Span>,
    pub y: Option<Span>,
}
