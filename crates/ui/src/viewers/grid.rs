//! An array's values as a grid, like PyCharm's "View as Array" (JetBrains' data viewer): rows
//! and columns with their indices, a format (`%.3f`, `%.4g`, `%e`, `%d`), colour by value from
//! the slice's minimum to its maximum, and for an array of three or more dimensions a picker for
//! every axis after the first two (a picture's channel). A table array's columns take its names.
//!
//! Only the cells on screen are read, straight from the bytes, so a 16-megapixel array scrolls
//! as fast as a small one. Every dtype the engines make is read; anything else says so.

use serde_json::{Value, json};

use super::basic::heat;
use super::{ViewInput, Viewer};
use crate::canvas::Action;
use crate::theme::Tokens;

const CELL_W: f32 = 76.0;
const HEADER_W: f32 = 56.0;
/// The formats offered (any `%.Nf`, `%.Ne`, `%.Ng` or `%d` can be typed).
pub const FORMATS: &[&str] = &["%.4g", "%.3f", "%.6f", "%.3e", "%d"];

/// How to read one element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dtype {
    U8,
    I8,
    Bool,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
}

impl Dtype {
    pub fn parse(s: &str) -> Option<Dtype> {
        Some(match s {
            "|u1" => Dtype::U8,
            "|i1" => Dtype::I8,
            "|b1" => Dtype::Bool,
            "<u2" => Dtype::U16,
            "<i2" => Dtype::I16,
            "<u4" => Dtype::U32,
            "<i4" => Dtype::I32,
            "<u8" => Dtype::U64,
            "<i8" => Dtype::I64,
            "<f4" => Dtype::F32,
            "<f8" => Dtype::F64,
            _ => return None,
        })
    }

    pub fn size(self) -> usize {
        match self {
            Dtype::U8 | Dtype::I8 | Dtype::Bool => 1,
            Dtype::U16 | Dtype::I16 => 2,
            Dtype::U32 | Dtype::I32 | Dtype::F32 => 4,
            Dtype::U64 | Dtype::I64 | Dtype::F64 => 8,
        }
    }

    pub fn integer(self) -> bool {
        !matches!(self, Dtype::F32 | Dtype::F64)
    }

    /// Element `i` of `bytes`, or `None` past the end.
    pub fn read(self, bytes: &[u8], i: usize) -> Option<f64> {
        let at = i.checked_mul(self.size())?;
        let b = bytes.get(at..at + self.size())?;
        let arr = |n: usize| -> [u8; 8] {
            let mut a = [0u8; 8];
            a[..n].copy_from_slice(&b[..n]);
            a
        };
        Some(match self {
            Dtype::U8 => f64::from(b[0]),
            Dtype::I8 => f64::from(b[0] as i8),
            Dtype::Bool => f64::from(u8::from(b[0] != 0)),
            Dtype::U16 => f64::from(u16::from_le_bytes([b[0], b[1]])),
            Dtype::I16 => f64::from(i16::from_le_bytes([b[0], b[1]])),
            Dtype::U32 => f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            Dtype::I32 => f64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            Dtype::U64 => u64::from_le_bytes(arr(8)) as f64,
            Dtype::I64 => i64::from_le_bytes(arr(8)) as f64,
            Dtype::F32 => f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            Dtype::F64 => f64::from_le_bytes(arr(8)),
        })
    }
}

/// `v` in `format` (`%.Nf`, `%.Ne`, `%.Ng`, `%d`; anything else reads as `%.4g`).
pub fn format_value(v: f64, format: &str) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let f = format.trim();
    let prec =
        |default: usize| f.strip_prefix("%.").and_then(|r| r.get(..r.len().saturating_sub(1))).and_then(|n| n.parse::<usize>().ok()).unwrap_or(default).min(17);
    match f.chars().last() {
        Some('d') => format!("{}", v.round() as i64),
        Some('f') => format!("{v:.*}", prec(3)),
        Some('e') => format!("{v:.*e}", prec(3)),
        _ => {
            // %g: fixed for moderate magnitudes, else scientific, trailing zeros dropped.
            let p = prec(4).max(1);
            if v == 0.0 {
                return "0".into();
            }
            let exp = v.abs().log10().floor() as i32;
            if exp < -4 || exp >= p as i32 {
                format!("{v:.*e}", p.saturating_sub(1))
            } else {
                let decimals = (p as i32 - 1 - exp).max(0) as usize;
                let s = format!("{v:.decimals$}");
                if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
            }
        }
    }
}

/// An array's shape as rows and columns, and the strides (in elements) to find a cell.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub rows: usize,
    pub cols: usize,
    /// For each axis after the first two: its length.
    pub extra: Vec<usize>,
    strides: Vec<usize>,
}

impl Layout {
    pub fn of(shape: &[u64]) -> Option<Layout> {
        let dims: Vec<usize> = shape.iter().map(|d| usize::try_from(*d).ok()).collect::<Option<_>>()?;
        let (rows, cols, extra): (usize, usize, Vec<usize>) = match dims.as_slice() {
            [] => (1, 1, Vec::new()),
            [n] => (*n, 1, Vec::new()),
            [r, c, rest @ ..] => (*r, *c, rest.to_vec()),
        };
        let mut strides = vec![1usize; dims.len().max(1)];
        for a in (0..dims.len().saturating_sub(1)).rev() {
            strides[a] = strides[a + 1].checked_mul(*dims.get(a + 1)?)?;
        }
        Some(Layout { rows, cols, extra, strides })
    }

    /// The flat index of row `r`, column `c` in the slice `at` (one index per extra axis).
    pub fn index(&self, r: usize, c: usize, at: &[usize]) -> usize {
        let s = |a: usize| self.strides.get(a).copied().unwrap_or(1);
        let base = if self.strides.len() == 1 && self.cols == 1 { r * s(0) } else { r * s(0) + c * s(1) };
        at.iter().enumerate().fold(base, |acc, (k, i)| acc + i * s(k + 2))
    }
}

/// The grid (see the module docs).
pub struct ArrayGrid {
    format: String,
    colored: bool,
    /// The index on every axis after the first two.
    slice: Vec<usize>,
    /// The slice's range, and what it was computed for.
    range: Option<((f64, f64), String)>,
}

impl Default for ArrayGrid {
    fn default() -> Self {
        ArrayGrid { format: FORMATS[0].into(), colored: true, slice: Vec::new(), range: None }
    }
}

/// The column names a table array (or a picture's channels on the last axis) carries.
fn names(v: &ViewInput<'_>) -> Vec<String> {
    match v.display.full.as_ref().map(|f| &f.meta.form) {
        Some(plumcraft_proto::data::Form::Array { names, .. }) => names.clone(),
        _ => Vec::new(),
    }
}

impl ArrayGrid {
    fn range(&mut self, dtype: Dtype, bytes: &[u8], l: &Layout, key: String) -> (f64, f64) {
        if let Some((r, k)) = &self.range
            && *k == key
        {
            return *r;
        }
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        // At most 4 million cells sampled evenly, so a huge slice stays quick.
        let n = l.rows.saturating_mul(l.cols);
        let step = (n / 4_000_000).max(1);
        for k in (0..n).step_by(step) {
            let (r, c) = (k / l.cols.max(1), k % l.cols.max(1));
            if let Some(x) = dtype.read(bytes, l.index(r, c, &self.slice)).filter(|x| x.is_finite()) {
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        self.range = Some(((lo, hi), key));
        (lo, hi)
    }
}

impl Viewer for ArrayGrid {
    fn sync(&mut self, _: &egui::Context, v: &ViewInput<'_>) {
        let extra = v.full.and_then(|b| Layout::of(&b.shape)).map_or(0, |l| l.extra.len());
        self.slice.resize(extra, 0);
    }

    fn preview(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, _: egui::Vec2, tokens: &Tokens) {
        let Some(b) = v.full else { return super::preview_default(ui, v, egui::vec2(160.0, 120.0), tokens) };
        let (Some(dt), Some(l)) = (Dtype::parse(&b.dtype), Layout::of(&b.shape)) else { return };
        egui::Grid::new("array-grid-preview").show(ui, |ui| {
            for r in 0..l.rows.min(5) {
                for c in 0..l.cols.min(5) {
                    let x = dt.read(&b.bytes, l.index(r, c, &self.slice)).unwrap_or(f64::NAN);
                    ui.label(egui::RichText::new(format_value(x, &self.format)).monospace().small().color(tokens.text));
                }
                ui.end_row();
            }
        });
    }

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        let Some(b) = v.full else {
            ui.label(
                egui::RichText::new(if v.missing { "the values are gone (reaped); rerun to publish them" } else { "the values aren't here" })
                    .color(tokens.text_dim),
            );
            return Vec::new();
        };
        let Some(dt) = Dtype::parse(&b.dtype) else {
            ui.label(egui::RichText::new(format!("can't read dtype {}", b.dtype)).color(tokens.text_dim));
            return Vec::new();
        };
        let Some(l) = Layout::of(&b.shape) else { return Vec::new() };
        let names = names(v);
        // The controls: format, colour, the slice.
        ui.horizontal_wrapped(|ui| {
            ui.label("Format");
            ui.add(egui::TextEdit::singleline(&mut self.format).desired_width(64.0).font(egui::TextStyle::Monospace));
            egui::ComboBox::from_id_salt("array-grid-format").selected_text("Presets").width(72.0).show_ui(ui, |ui| {
                for f in FORMATS {
                    ui.selectable_value(&mut self.format, (*f).to_owned(), *f);
                }
            });
            ui.checkbox(&mut self.colored, "Colour by value");
            for (k, len) in l.extra.iter().enumerate() {
                if let Some(at) = self.slice.get_mut(k) {
                    let axis = if k + 1 == l.extra.len() && !names.is_empty() { "channel".to_owned() } else { format!("axis {}", k + 2) };
                    ui.label(axis);
                    ui.add(egui::DragValue::new(at).range(0..=len.saturating_sub(1)));
                    if k + 1 == l.extra.len()
                        && let Some(n) = names.get(*at)
                    {
                        ui.label(egui::RichText::new(n).color(tokens.text_dim));
                    }
                }
            }
        });
        let slice_text = if self.slice.is_empty() {
            String::new()
        } else {
            format!("[:, :, {}]", self.slice.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))
        };
        let key = format!("{}{:?}{:?}", b.dtype, b.shape, self.slice);
        let (lo, hi) = self.range(dt, &b.bytes, &l, key);
        ui.label(
            egui::RichText::new(format!(
                "{} × {} {}{slice_text} · {} … {}",
                l.rows,
                l.cols,
                b.dtype,
                format_value(lo, &self.format),
                format_value(hi, &self.format)
            ))
            .small()
            .color(tokens.text_faint),
        );
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + 6.0;
        let content = egui::vec2(HEADER_W + l.cols as f32 * CELL_W, row_h * (l.rows as f32 + 1.0));
        let font = egui::FontId::monospace(12.0);
        let (fmt, colored) = (self.format.clone(), self.colored);
        let slice = self.slice.clone();
        egui::ScrollArea::both().id_salt("viewer-array-grid").auto_shrink([false, false]).show_viewport(ui, |ui, view| {
            let (rect, _) = ui.allocate_exact_size(content, egui::Sense::hover());
            let p = ui.painter_at(rect.intersect(ui.clip_rect()));
            let origin = rect.min;
            let c0 = ((view.min.x - HEADER_W) / CELL_W).floor().max(0.0) as usize;
            let c1 = (((view.max.x - HEADER_W) / CELL_W).ceil().max(0.0) as usize).min(l.cols);
            let r0 = ((view.min.y - row_h) / row_h).floor().max(0.0) as usize;
            let r1 = (((view.max.y - row_h) / row_h).ceil().max(0.0) as usize).min(l.rows);
            // Column headers, then row headers, then the cells.
            for c in c0..c1 {
                let x = origin.x + HEADER_W + c as f32 * CELL_W;
                let label = names.get(c).filter(|_| l.extra.is_empty()).cloned().unwrap_or_else(|| c.to_string());
                p.text(
                    egui::pos2(x + CELL_W - 6.0, origin.y + view.min.y.max(0.0) + row_h / 2.0),
                    egui::Align2::RIGHT_CENTER,
                    label,
                    font.clone(),
                    tokens.text_dim,
                );
            }
            for r in r0..r1 {
                let y = origin.y + row_h * (r as f32 + 1.0);
                p.text(
                    egui::pos2(origin.x + view.min.x.max(0.0) + HEADER_W - 8.0, y + row_h / 2.0),
                    egui::Align2::RIGHT_CENTER,
                    r.to_string(),
                    font.clone(),
                    tokens.text_dim,
                );
                for c in c0..c1 {
                    let x = origin.x + HEADER_W + c as f32 * CELL_W;
                    let cell = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(CELL_W, row_h));
                    let value = dt.read(&b.bytes, l.index(r, c, &slice));
                    if colored
                        && let Some(x) = value.filter(|x| x.is_finite())
                        && hi > lo
                    {
                        p.rect_filled(cell.shrink(0.5), 0.0, heat(tokens, (x - lo) / (hi - lo)));
                    }
                    let text = value.map_or_else(|| "?".into(), |x| format_value(x, if dt.integer() && fmt.ends_with('g') { "%d" } else { &fmt }));
                    p.text(cell.right_center() - egui::vec2(6.0, 0.0), egui::Align2::RIGHT_CENTER, text, font.clone(), tokens.text);
                }
            }
        });
        Vec::new()
    }

    fn inspect(&self) -> Value {
        json!({"format": self.format, "colored": self.colored, "slice": self.slice, "range": self.range.as_ref().map(|(r, _)| [r.0, r.1])})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_printf() {
        assert_eq!(format_value(1.0 / 3.0, "%.3f"), "0.333");
        assert_eq!(format_value(1234.5678, "%.4g"), "1235");
        assert_eq!(format_value(0.000_012_3, "%.4g"), "1.230e-5");
        assert_eq!(format_value(0.5, "%.4g"), "0.5");
        assert_eq!(format_value(2.6, "%d"), "3");
        assert_eq!(format_value(12345.0, "%.2e"), "1.23e4");
        assert_eq!(format_value(f64::NAN, "%.3f"), "nan");
        assert_eq!(format_value(1.5, "junk"), "1.5");
    }

    #[test]
    fn layouts_find_cells_in_c_order() {
        let l = Layout::of(&[2, 3, 4]).unwrap();
        assert_eq!((l.rows, l.cols, l.extra.clone()), (2, 3, vec![4]));
        assert_eq!(l.index(1, 2, &[3]), 12 + 2 * 4 + 3, "row 1 (stride 12), column 2 (stride 4), channel 3");
        let one = Layout::of(&[5]).unwrap();
        assert_eq!((one.rows, one.cols, one.index(3, 0, &[])), (5, 1, 3));
        assert_eq!(Layout::of(&[]).map(|l| (l.rows, l.cols)), Some((1, 1)));
        assert!(Layout::of(&[u64::MAX, u64::MAX, 2]).is_none(), "overflow is refused, not a panic");
    }

    #[test]
    fn dtypes_read_little_endian_and_stop_at_the_end() {
        let b = [1.5f32.to_le_bytes(), (-2.0f32).to_le_bytes()].concat();
        assert_eq!(Dtype::F32.read(&b, 1), Some(-2.0));
        assert_eq!(Dtype::F32.read(&b, 2), None);
        assert_eq!(Dtype::I16.read(&(-3i16).to_le_bytes(), 0), Some(-3.0));
        assert_eq!(Dtype::parse(">f4"), None);
    }
}
