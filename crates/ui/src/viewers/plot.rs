//! Tables of positions as plots: an (N, k) array whose columns name points (`x`, `y`) or segments
//! (`x0 y0 x1 y1`), or the two columns of an (N, 2) table, with egui_plot. Any other table has
//! its numbers (the Values tab). Lifted from the research software's `crates/ui-egui/src/viewer_plot.rs`,
//! whose histograms PlumCraft draws in [`crate::array_view`].
//!
//! The rows are read once per value, at most [`MAX_TABLE_ROWS`]; the rest are counted, not drawn.

use egui_plot::{Arrows, Plot, PlotPoint, PlotPoints, Points};
use plumcraft_proto::data::Form;
use plumcraft_proto::display::Display;
use plumcraft_store::Blob;
use serde_json::{Value, json};

use super::grid::Dtype;
use super::{ViewInput, Viewer};
use crate::canvas::Action;
use crate::media::key_of;
use crate::theme::Tokens;

/// The most rows read for a plot.
pub const MAX_TABLE_ROWS: usize = 50_000;

/// When a table's x and y spans are within this factor, it is drawn to one scale, y down, as
/// positions on a picture are; further apart they are two different quantities.
const SAME_SCALE: f64 = 20.0;

/// How a table is drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Marks {
    Points(Vec<PlotPoint>),
    /// From each `from` to the `to` beside it.
    Segments {
        from: Vec<PlotPoint>,
        to: Vec<PlotPoint>,
    },
}

impl Marks {
    fn len(&self) -> usize {
        match self {
            Marks::Points(p) => p.len(),
            Marks::Segments { from, .. } => from.len(),
        }
    }
}

/// A table read for plotting, with how many rows it has in all.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub marks: Marks,
    pub rows: usize,
    /// x and y spans of the same order.
    pub same_scale: bool,
}

/// The column indices a table plots: segment columns (x0 y0 x1 y1, or x1 y1 x2 y2), named x/y
/// columns, or the two columns of an (N, 2) table. `None`: there is no plot of it.
pub fn table_columns(names: &[String], columns: usize) -> Option<Vec<usize>> {
    let find = |n: &str| names.iter().position(|m| m == n);
    if names.len() == columns {
        for set in [["x0", "y0", "x1", "y1"], ["x1", "y1", "x2", "y2"]] {
            if let Some(cols) = set.iter().map(|n| find(n)).collect::<Option<Vec<_>>>() {
                return Some(cols);
            }
        }
        if let (Some(x), Some(y)) = (find("x"), find("y")) {
            return Some(vec![x, y]);
        }
    }
    (columns == 2).then(|| vec![0, 1])
}

/// A display's table: its columns' names and which it plots.
fn plotted(d: &Display) -> Option<Vec<usize>> {
    match d.full.as_ref().map(|f| &f.meta.form) {
        Some(Form::Array { shape, names, table: true, .. }) => match shape.as_slice() {
            [_, k] => table_columns(names, usize::try_from(*k).ok()?),
            _ => None,
        },
        _ => None,
    }
}

/// Whether this viewer shows `d`.
pub fn accepts(d: &Display) -> bool {
    plotted(d).is_some()
}

/// Read a table for plotting (`None` when it isn't a table of numbers). NaN rows are dropped.
pub fn read_table(b: &Blob, cols: &[usize]) -> Option<Table> {
    let dtype = Dtype::parse(&b.dtype)?;
    let [rows, columns] = *b.shape.as_slice() else { return None };
    let (rows, columns) = (usize::try_from(rows).ok()?, usize::try_from(columns).ok()?);
    if rows.checked_mul(columns)?.checked_mul(dtype.size())? > b.bytes.len() {
        return None;
    }
    let get = |r: usize, c: usize| dtype.read(&b.bytes, r.checked_mul(columns)?.checked_add(c)?).filter(|v| v.is_finite());
    let shown = rows.min(MAX_TABLE_ROWS);
    let marks = match *cols {
        [x, y] => Marks::Points((0..shown).filter_map(|r| Some(PlotPoint::new(get(r, x)?, get(r, y)?))).collect()),
        [x0, y0, x1, y1] => {
            let pair = (0..shown).filter_map(|r| Some((PlotPoint::new(get(r, x0)?, get(r, y0)?), PlotPoint::new(get(r, x1)?, get(r, y1)?))));
            let (from, to) = pair.unzip();
            Marks::Segments { from, to }
        }
        _ => return None,
    };
    Some(Table { same_scale: same_scale(&marks), marks, rows })
}

fn same_scale(marks: &Marks) -> bool {
    let chain: Box<dyn Iterator<Item = &PlotPoint>> = match marks {
        Marks::Points(p) => Box::new(p.iter()),
        Marks::Segments { from, to } => Box::new(from.iter().chain(to)),
    };
    let mut b = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
    for p in chain {
        b = [b[0].min(p.x), b[1].max(p.x), b[2].min(p.y), b[3].max(p.y)];
    }
    let (x, y) = (b[1] - b[0], b[3] - b[2]);
    x.is_finite() && y.is_finite() && x <= y * SAME_SCALE && y <= x * SAME_SCALE
}

/// The plot viewer.
#[derive(Default)]
pub struct TablePlot {
    key: Option<String>,
    table: Option<Table>,
}

impl Viewer for TablePlot {
    fn sync(&mut self, _: &egui::Context, v: &ViewInput<'_>) {
        let key = key_of(v.display);
        if self.key.as_ref() == Some(&key) && self.table.is_some() {
            return;
        }
        self.key = Some(key);
        self.table = v.full.and_then(|b| read_table(b, &plotted(v.display)?));
    }

    fn preview(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, max: egui::Vec2, tokens: &Tokens) {
        match &self.table {
            Some(t) => {
                ui.set_max_size(max);
                plot_ui(ui, "plumcraft-table-plot-preview", t, tokens, max.y);
            }
            None => super::preview_default(ui, v, max, tokens),
        }
    }

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        let Some(t) = &self.table else {
            let text = if v.missing { "the values are gone (reaped); rerun to publish them" } else { "no numbers to plot" };
            ui.label(egui::RichText::new(text).color(tokens.text_dim));
            return Vec::new();
        };
        let drawn = t.marks.len();
        let what = if matches!(t.marks, Marks::Points(_)) { "points" } else { "segments" };
        ui.label(egui::RichText::new(format!("{drawn} {what} of {} rows", t.rows)).small().color(tokens.text_dim));
        let h = (ui.available_width() * 0.75).clamp(160.0, ui.available_height().max(160.0));
        plot_ui(ui, "plumcraft-table-plot", t, tokens, h);
        Vec::new()
    }

    fn inspect(&self) -> Value {
        json!({"kind": "plot", "drawn": self.table.as_ref().map(|t| t.marks.len()), "rows": self.table.as_ref().map(|t| t.rows),
               "marks": self.table.as_ref().map(|t| if matches!(t.marks, Marks::Points(_)) { "points" } else { "segments" }),
               "sameScale": self.table.as_ref().map(|t| t.same_scale)})
    }
}

/// Draw a table, in image coordinates (x = column, y = row, y down) when its spans agree.
fn plot_ui(ui: &mut egui::Ui, id: &str, t: &Table, tokens: &Tokens, height: f32) {
    let mut plot = Plot::new(id).height(height).allow_scroll(false).show_background(false);
    if t.same_scale {
        plot = plot.data_aspect(1.0).invert_y(true);
    }
    plot.show(ui, |p| match &t.marks {
        Marks::Points(points) => p.points(Points::new("points", PlotPoints::Borrowed(points)).radius(2.0).color(tokens.accent)),
        Marks::Segments { from, to } => {
            p.arrows(Arrows::new("segments", PlotPoints::Borrowed(from), PlotPoints::Borrowed(to)).tip_length(0.0).color(tokens.accent))
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| (*s).to_owned()).collect()
    }

    fn table(shape: &[u64], data: &[f64]) -> Blob {
        Blob { dtype: "<f8".into(), shape: shape.to_vec(), bytes: data.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>().into() }
    }

    #[test]
    fn table_columns_by_name_or_width() {
        assert_eq!(table_columns(&names(&["x0", "y0", "x1", "y1"]), 4), Some(vec![0, 1, 2, 3]));
        assert_eq!(table_columns(&names(&["id", "x1", "y1", "x2", "y2"]), 5), Some(vec![1, 2, 3, 4]));
        assert_eq!(table_columns(&names(&["r", "y", "x"]), 3), Some(vec![2, 1]));
        assert_eq!(table_columns(&[], 2), Some(vec![0, 1]));
        assert_eq!(table_columns(&[], 3), None, "an unnamed wide table is numbers");
        assert_eq!(table_columns(&names(&["x", "y"]), 3), None, "names that don't fit are ignored");
    }

    #[test]
    fn reads_points_and_segments_from_tables() {
        let t = read_table(&table(&[3, 2], &[0.0, 1.0, 2.0, f64::NAN, 4.0, 5.0]), &[0, 1]).unwrap();
        assert_eq!(t.marks, Marks::Points(vec![PlotPoint::new(0.0, 1.0), PlotPoint::new(4.0, 5.0)]), "NaN rows are dropped");
        assert_eq!(t.rows, 3);
        let s = read_table(&table(&[1, 4], &[1.0, 2.0, 3.0, 4.0]), &[0, 1, 2, 3]).unwrap();
        assert_eq!(s.marks, Marks::Segments { from: vec![PlotPoint::new(1.0, 2.0)], to: vec![PlotPoint::new(3.0, 4.0)] });
        assert_eq!(read_table(&table(&[4], &[0.0; 4]), &[0, 1]), None, "not a table");
        assert_eq!(read_table(&table(&[9, 2], &[0.0; 4]), &[0, 1]), None, "shorter than its shape");
        assert_eq!(read_table(&Blob { dtype: "<c8".into(), shape: vec![1, 2], bytes: (vec![0; 16]).into() }, &[0, 1]), None);
    }

    #[test]
    fn a_table_with_two_kinds_of_axis_isnt_forced_to_one_scale() {
        assert!(read_table(&table(&[2, 2], &[0.0, 0.0, 100.0, 80.0]), &[0, 1]).unwrap().same_scale);
        assert!(!read_table(&table(&[2, 2], &[0.0, 0.0, 10000.0, 1.0]), &[0, 1]).unwrap().same_scale);
    }

    #[test]
    fn only_a_table_with_plottable_columns_offers_a_plot() {
        use plumcraft_proto::data::DataRef;
        let form = |table: bool, shape: Vec<u64>, names: Vec<String>| Form::Array { dtype: "<f4".into(), shape, space: None, names, table };
        let display = |f: Form| {
            let mut r = DataRef::default();
            r.meta.form = f;
            Display { full: Some(r), ..Default::default() }
        };
        assert!(accepts(&display(form(true, vec![9, 2], Vec::new()))));
        assert!(!accepts(&display(form(false, vec![9, 2], Vec::new()))), "a 9 × 2 picture");
        assert!(!accepts(&display(form(true, vec![9, 5], Vec::new()))));
    }
}
