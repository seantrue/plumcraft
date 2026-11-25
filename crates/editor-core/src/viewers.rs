//! The viewers' data, as the core gives it to a shell that draws natively (N4 slice 5;
//! `docs/shell-interface.md` 13): which viewers a value is offered in ([`offered`], egui's registry
//! in the core), what a pop-out's viewer needs beyond `popout.get` ([`popout_extra`]), and the
//! pages of a large value read on the media service's workers ([`data`], `viewer.data`): a table's
//! rows sorted by the core, a JSON outline's open members, an array's numbers as a grid, a value's
//! bytes as hex, an image's exact values at a pixel, a tree as YAML. Never waits: a page not read
//! yet is `{status: "pending", wait: key}`, and the feed's `media` names `key` when it is.
//!
//! Number text is the core's ([`number`]): an integer whole, a float at the decimals it needs (at
//! most six, [`crate::widget_text::needed_decimals`]), a very large or small one in scientific
//! notation; a shell never formats a number itself.

use std::sync::Arc;

use plumcraft_media::table::{Cell, Table};
use plumcraft_media::{Fetch, MediaService, Want};
use plumcraft_proto::catalogue::Media;
use plumcraft_proto::data::{DataRef, Form};
use plumcraft_proto::display::{Display, Rendition};
use plumcraft_session::Session;
use plumcraft_session::chart_view::Selection;
use plumcraft_session::run::WatchRole;
use plumcraft_session::viewer_state::{GRID_COLUMNS, HEX_PAGE, TABLE_PAGE, ViewerState, last_window};
use serde_json::{Map, Value, json};

/// The most children of one outline member listed (the rest are counted).
pub const MAX_CHILDREN: usize = 1000;
/// The most characters of YAML given.
pub const MAX_YAML_CHARS: usize = 4 << 20;

fn is_pdf(d: &Display) -> bool {
    d.full.as_ref().is_some_and(|f| f.meta.media == "application/pdf")
}

fn is_rows(v: &Value) -> bool {
    v.as_array().is_some_and(|r| !r.is_empty() && r.iter().all(Value::is_array))
}

fn is_xml(d: &Display) -> bool {
    d.full.as_ref().is_some_and(|f| {
        let m = f.meta.media.split(';').next().unwrap_or_default().trim();
        (m == "application/xml" || m == "text/xml" || m.ends_with("+xml")) && m != "image/svg+xml"
    })
}

/// Whether a value's text is an SVG document (note 26 slice 4): it starts, after an optional XML
/// declaration, comments and white space, with an `<svg` element. Only the start is looked at
/// (a TEXT's inline value or its excerpt), so this never reads a large value.
pub fn is_svg_text(d: &Display) -> bool {
    let start = d.value.as_ref().and_then(Value::as_str).map(|t| t.chars().take(2048).collect::<String>()).or_else(|| {
        d.renditions.iter().find_map(|r| match r {
            Rendition::Excerpt { text, .. } => Some(text.chars().take(2048).collect()),
            _ => None,
        })
    });
    let Some(mut t) = start.as_deref().map(str::trim_start) else { return false };
    for _ in 0..16 {
        if let Some(rest) = t.strip_prefix("<?xml") {
            t = rest.split_once("?>").map_or("", |x| x.1).trim_start();
        } else if let Some(rest) = t.strip_prefix("<!--") {
            t = rest.split_once("-->").map_or("", |x| x.1).trim_start();
        } else if let Some(rest) = t.strip_prefix("<!DOCTYPE") {
            t = rest.split_once('>').map_or("", |x| x.1).trim_start();
        } else {
            break;
        }
    }
    t.strip_prefix("<svg").is_some_and(|r| r.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/'))
}

/// The most text drawn as an SVG picture from a TEXT value (a larger one is shown as text).
pub const MAX_SVG_TEXT: usize = 8 << 20;
/// The size, on its long side, a shell first asks an SVG picture at (the Apple shells' `svgPx`).
pub const SVG_TEXT_PX: u32 = 2048;

/// A TEXT that is an SVG document, as a reference a shell draws like any SVG value: the text
/// carried inline, typed `image/svg+xml`, under a handle of its own content (note 26 slice 4).
/// `None` for anything else, or a text past [`MAX_SVG_TEXT`].
pub fn svg_text_ref(d: &Display) -> Option<DataRef> {
    use base64::Engine as _;
    if d.media != Media::Text || !is_svg_text(d) {
        return None;
    }
    // the whole text: the inline value, or an excerpt that holds all of it
    let whole = d.value.as_ref().and_then(Value::as_str).map(str::to_owned).or_else(|| {
        d.renditions.iter().find_map(|r| match r {
            Rendition::Excerpt { text, offset: 0, total_chars, .. } if text.chars().count() as u64 == *total_chars => Some(text.clone()),
            _ => None,
        })
    });
    // longer than its excerpt: the value's own bytes, read as SVG (the media service's SVG
    // picture reads the bytes, whatever their type)
    let Some(whole) = whole else {
        let mut r = d.full.clone().filter(|f| f.handle.is_some() && f.meta.size <= MAX_SVG_TEXT as u64)?;
        r.meta.media = plumcraft_proto::catalogue::core_types::SVG_MEDIA.into();
        return Some(r);
    };
    let text = whole.as_str();
    if text.len() > MAX_SVG_TEXT {
        return None;
    }
    let meta =
        plumcraft_proto::data::BlobMeta { media: plumcraft_proto::catalogue::core_types::SVG_MEDIA.into(), size: text.len() as u64, ..Default::default() };
    Some(DataRef {
        handle: Some(plumcraft_proto::Handle::raw(text.as_bytes())),
        meta,
        at: vec![plumcraft_proto::data::Locator::Inline { b64: base64::engine::general_purpose::STANDARD.encode(text.as_bytes()) }],
    })
}

/// Whether a value is typed `text/csv` (bytes a CSV file type or a pack's type carries).
pub fn is_csv(d: &Display) -> bool {
    d.full.as_ref().is_some_and(|f| plumcraft_media::service::is_csv(&f.meta.media))
}

/// What an array's form allows (FR6, note 30): every view that can be computed for its kind,
/// dtype and rank, never withheld because the result may look odd ("In general, it's the user's
/// job to deal with odd, and your job to make seeing odd things easy", the maintainer,
/// 2026-11-25). `(plot, table)`: it plots (a series, or columns that name a plot or two), and it
/// reads as a table within the reader's limits (any numeric array: rows the leading axes, columns
/// the last).
pub fn array_views(d: &Display) -> (bool, bool) {
    let Some(f) = d.full.as_ref() else { return (false, false) };
    let Form::Array { dtype, shape, .. } = &f.meta.form else { return (false, false) };
    // numbers only: a plot and a table read them (a dtype the table reader takes is one)
    let numeric = plumcraft_media::table::Table::fits_array(dtype, &[1]);
    (numeric && crate::chart::plottable(&f.meta.form).is_some(), plumcraft_media::table::Table::fits_array(dtype, shape))
}

/// The viewers a value is offered in, its default first: egui's registry
/// (`plumcraft_ui::viewers::kinds_for`, which a test there holds this to), and SVG's source as text
/// beside its picture. `(id, label)`.
///
/// An array (FR6, note 30) is offered everything computable for it: Pictures and Values always,
/// Plot where it plots, Table and Chart where it reads as a table. A flag only orders them: a
/// table-flagged array, or a 1-D series, plots (or tabulates) first; any other array shows its
/// pictures first. A plotted array has a Chart beside its Plot (no longer "one chart a value").
pub fn offered(d: &Display) -> Vec<(&'static str, &'static str)> {
    if d.media == Media::Ndarray {
        return offered_array(d);
    }
    let mut out = Vec::new();
    let m = d.media;
    let obj = d.value.as_ref().is_some_and(|v| v.is_object() || v.is_array());
    let mut add = |ok: bool, id, label| {
        if ok {
            out.push((id, label));
        }
    };
    add(m == Media::Image, "picture", "Picture");
    // an interchange format (note 26 slice 4): SVG text is a picture first, its text a viewer away
    let svg_text = m == Media::Text && is_svg_text(d);
    let csv = is_csv(d) && m != Media::Table;
    add(m == Media::Vector || svg_text, "svg", "SVG");
    add(is_pdf(d), "pdf.pages", "Pages");
    add(m == Media::Doc, "doc", "Document");
    add(m == Media::Audio, "audio", "Audio");
    // a CSV is a table first, its text a viewer away
    let tabular = m == Media::Table || csv || d.value.as_ref().is_some_and(is_rows);
    // a TABLE carried as an array (B152's histogram) plots first, as any array that plots does
    add(m == Media::Table && array_views(d).0, "array.plot", "Plot");
    add(tabular, "table.grid", "Table");
    // a table's chart, its kind and columns chosen by `viewer.chart` (note 26 slices 5 to 7)
    add(tabular, "table.chart", "Chart");
    add(is_xml(d), "xml", "XML");
    add(m == Media::Text || m == Media::Vector || csv, "text.plain", if m == Media::Vector || svg_text { "Source" } else { "Text" });
    add(m == Media::Scalar, "scalar", "Value");
    add(m == Media::Tree, "yaml", "YAML");
    // a table that is an array (B152) has no JSON to outline
    let array = d.full.as_ref().is_some_and(|f| matches!(f.meta.form, Form::Array { .. }));
    add(m == Media::Tree || (m == Media::Table && !array) || obj, "tree", "Tree");
    add(d.items > 1 && !d.sheet.is_empty(), "items", "Items");
    add(d.full.is_some() && m != Media::Doc, "hex", "Bytes");
    out
}

/// An array's viewers (FR6): see [`offered`].
fn offered_array(d: &Display) -> Vec<(&'static str, &'static str)> {
    let (plot, table) = array_views(d);
    let flagged = d.full.as_ref().is_some_and(|f| matches!(f.meta.form, Form::Array { table: true, .. }));
    let series = d.full.as_ref().is_some_and(|f| matches!(&f.meta.form, Form::Array { shape, .. } if shape.len() == 1));
    let mut out = Vec::new();
    let mut add = |ok: bool, id, label| {
        if ok {
            out.push((id, label));
        }
    };
    if flagged || series {
        // the flag (or a series, which has no picture) chooses the default: its plot, else its table
        add(plot, "array.plot", "Plot");
        add(table, "table.grid", "Table");
        add(table, "table.chart", "Chart");
        add(true, "array.pictures", "Pictures");
        add(true, "array.grid", "Values");
    } else {
        add(true, "array.pictures", "Pictures");
        add(true, "array.grid", "Values");
        add(plot, "array.plot", "Plot");
        add(table, "table.grid", "Table");
        add(table, "table.chart", "Chart");
    }
    // an array carried inline as JSON (small) outlines as any JSON does
    add(d.value.as_ref().is_some_and(|v| v.is_object() || v.is_array()), "tree", "Tree");
    add(d.items > 1 && !d.sheet.is_empty(), "items", "Items");
    add(d.full.is_some(), "hex", "Bytes");
    out
}

/// The viewers the Apple shells draw (N4 slice 5); any other offered falls back to the engine's
/// rendition with the summary (contract 13.3).
pub const NATIVE: &[&str] = &[
    "picture",
    "svg",
    "array.plot",
    "array.pictures",
    "array.grid",
    "audio",
    "table.grid",
    "table.chart",
    "text.plain",
    "scalar",
    "yaml",
    "tree",
    "items",
    "hex",
];

/// A number as the viewers show it: an integer whole; a float at the decimals it needs, at most
/// six; past 1e9 or under 1e-4, four significant decimals in scientific notation; NaN and the
/// infinities by name.
pub fn number(x: f64, int: bool) -> String {
    plumcraft_chart::number(x, int)
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

/// Where `x` lies from `lo` to `hi`, 0 to 1 at three decimals: a cell's tint (colour by value);
/// none for a value that isn't finite, and 0.5 when the range is a single value.
pub fn tint(x: f64, lo: f64, hi: f64) -> Value {
    if !x.is_finite() || !lo.is_finite() || !hi.is_finite() {
        return Value::Null;
    }
    let t = if hi > lo { ((x - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.5 };
    json!((t * 1000.0).round() / 1000.0)
}

/// The finite minimum and maximum of `xs`.
fn range(xs: impl Iterator<Item = f64>) -> (f64, f64) {
    xs.filter(|x| x.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)))
}

fn cell_text(c: &Cell) -> String {
    match c {
        Cell::Null => String::new(),
        Cell::Bool(b) => b.to_string(),
        Cell::Int(i) => i.to_string(),
        Cell::Num(f) => number(*f, false),
        Cell::Text(t) | Cell::Json(t) => t.to_string(),
    }
}

/// The most bins of one histogram a shell is given (the engines send 256; the rest are summed
/// into the last given, so the picture keeps its total).
pub const MAX_HISTOGRAM_BINS: usize = 1024;

/// A channel's histogram as a shell draws it (B154 slice 1): the counts, each bin's height from 0
/// to 1 on the scale the state asks (`viewer.array {log}`: ln(1 + count)), the range as the core's
/// number text, and the largest count. `None` for a histogram with nothing to draw.
pub fn histogram_json(h: &plumcraft_proto::display::Histogram, log: bool) -> Option<Value> {
    if h.counts.is_empty() {
        return None;
    }
    let mut counts: Vec<u64> = h.counts.iter().take(MAX_HISTOGRAM_BINS).copied().collect();
    let rest: u64 = h.counts.iter().skip(MAX_HISTOGRAM_BINS).fold(0u64, |a, n| a.saturating_add(*n));
    if let Some(last) = counts.last_mut() {
        *last = last.saturating_add(rest);
    }
    let scale = |n: u64| if log { (n as f64).ln_1p() } else { n as f64 };
    let top = counts.iter().copied().max().unwrap_or(0);
    let peak = scale(top);
    let heights: Vec<Value> = counts.iter().map(|n| json!(if peak > 0.0 { (scale(*n) / peak * 1e4).round() / 1e4 } else { 0.0 })).collect();
    let text = |x: Option<f64>| x.filter(|x| x.is_finite()).map(|x| number(x, false));
    Some(json!({"bins": counts.len(), "lo": text(h.lo), "hi": text(h.hi), "top": top, "log": log, "counts": counts, "heights": heights}))
}

/// What a pop-out's viewer needs beyond `popout.get`'s value: the viewers offered, the list it is
/// part of, the value's shape, its statistics (each channel's range, NaN count and histogram), its
/// pictures and waveform, and the viewer's state.
pub fn popout_extra(d: Option<&Display>, state: &ViewerState) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("state".into(), state.json());
    let Some(d) = d else { return m };
    let offered: Vec<Value> = offered(d).into_iter().map(|(id, label)| json!({"id": id, "label": label, "native": NATIVE.contains(&id)})).collect();
    m.insert("viewers".into(), Value::Array(offered));
    m.insert("type".into(), json!(d.ty));
    m.insert("items".into(), json!(d.items));
    m.insert("index".into(), json!(d.index));
    // a list of lists (interface 7.6 rule 2): the item shown within run `index`, and each level's
    // count, for the run and item steppers (decision 6A)
    if !d.levels.is_empty() {
        m.insert("path".into(), json!(d.path));
        m.insert("levels".into(), json!(d.levels));
    }
    if let Some(f) = &d.full {
        m.insert("form".into(), serde_json::to_value(&f.meta.form).unwrap_or(Value::Null));
        m.insert("bytes".into(), json!(f.meta.size));
        m.insert("mediaType".into(), json!(f.meta.media));
        if let Some(h) = &f.handle {
            m.insert("handle".into(), json!(h.to_string()));
        }
    }
    if !d.stats.channels.is_empty() {
        let ch: Vec<Value> = d
            .stats
            .channels
            .iter()
            .take(16)
            .map(|c| {
                let mut o = json!({"name": c.name, "min": c.min.map(|x| number(x, false)), "max": c.max.map(|x| number(x, false)), "nan": c.nan});
                if let Some(h) = c.histogram.as_ref().and_then(|h| histogram_json(h, state.log)) {
                    o["histogram"] = h;
                }
                o
            })
            .collect();
        m.insert("stats".into(), Value::Array(ch));
    }
    let views: Vec<Value> = d
        .renditions
        .iter()
        .filter_map(|r| match r {
            Rendition::Rgba8 { data, role, caption, w, h, .. } if role == "view" => Some(json!({"ref": data, "caption": caption, "w": w, "h": h})),
            _ => None,
        })
        .collect();
    if !views.is_empty() {
        m.insert("views".into(), Value::Array(views));
    }
    if let Some(p) = d.renditions.iter().find_map(|r| match r {
        Rendition::Peaks { data, bins, channels } => Some(json!({"ref": data, "bins": bins, "channels": channels})),
        _ => None,
    }) {
        m.insert("peaks".into(), p);
    }
    if !d.sheet.is_empty() {
        let sheet: Vec<Value> = d
            .sheet
            .iter()
            .take(64)
            .enumerate()
            .map(|(i, c)| {
                let thumb = c.thumb.as_ref().and_then(|t| match t {
                    Rendition::Rgba8 { data, w, h, .. } => Some(json!({"ref": data, "w": w, "h": h})),
                    _ => None,
                });
                json!({"index": i, "label": c.label, "summary": c.summary, "thumb": thumb})
            })
            .collect();
        m.insert("sheet".into(), Value::Array(sheet));
    }
    m
}

/// `{status: "pending", wait: key}`, or why not, for a fetch not ready.
fn not_ready<T>(f: &Fetch<T>, full: &DataRef, want: Want) -> Value {
    let handle = full.handle.as_ref().map(ToString::to_string).unwrap_or_default();
    match f {
        Fetch::Pending => json!({"status": "pending", "wait": plumcraft_media::derived_key(&handle, want)}),
        Fetch::Missing => json!({"status": "missing", "error": "the value's bytes are gone: run again to see it"}),
        Fetch::Failed(e) => json!({"status": "failed", "error": e}),
        Fetch::Ready(_) => json!({"status": "ready"}),
    }
}

fn whole(p: &Value, k: &str) -> Result<Option<u64>, String> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("{k} must be a whole number")),
    }
}

/// `viewer.data {popout, part, …}`: a page of what a pop-out's viewer shows (module docs).
/// Parts: `table` (the state's sort and page), `tree` (every open member's children), `grid` (the
/// state's plane, page and window of columns), `hex` (the state's page), `probe` (`row`, `col`:
/// the exact values there), `yaml`.
pub fn data(session: &Session, media: &MediaService, params: &Value) -> Result<Value, String> {
    crate::control::check_fields(params, &["popout", "part", "row", "col", "width", "height", "x", "y"])?;
    let id = params.get("popout").and_then(Value::as_u64).ok_or("popout must be a pop-out's id")?;
    let o = session.views.popout(id).ok_or_else(|| format!("no pop-out {id}"))?;
    let (_, d) = session.run.entry(WatchRole::Popout(id)).ok_or_else(|| format!("no pop-out {id}"))?;
    let d = d.ok_or("no value yet: run the graph")?;
    let part = params.get("part").and_then(Value::as_str).ok_or("part is table, tree, grid, hex, probe or yaml")?;
    let (w, _) = session.run.entry(WatchRole::Popout(id)).ok_or_else(|| format!("no pop-out {id}"))?;
    let spec = plumcraft_session::chart_cmds::spec_of(session.graph(), &w.node, &w.port);
    let sel = session.views.selections.get(&format!("{}.{}", w.node, w.port));
    let mut out = match part {
        "table" => {
            let mut t = table(d, media, &o.state, sel, spec.as_ref());
            if sel.is_some() && t.get("cells").is_some() {
                t["selectionSeq"] = json!(session.views.selection_seq);
            }
            t
        }
        "tree" => tree(d, media, &o.state),
        "yaml" => yaml(d, media),
        "chart" | "hit" => {
            let side = |k: &str, dflt: f64| match params.get(k) {
                None | Some(Value::Null) => Ok(dflt),
                Some(v) => v.as_f64().filter(|f| f.is_finite() && *f > 0.0).ok_or(format!("{k} must be a positive number of points")),
            };
            let size = (side("width", crate::chart::DEFAULT_SIZE.0)?, side("height", crate::chart::DEFAULT_SIZE.1)?);
            // the pop-out's viewer says which (FR6: an array may have both a Plot and a Chart): its
            // own choice, else its default; any other viewer asking, a plotted value plots
            let plotted = d.full.as_ref().is_some_and(|f| crate::chart::plottable(&f.meta.form).is_some());
            let shown = o.viewer.clone().or_else(|| offered(d).first().map(|(id, _)| (*id).to_owned()));
            let as_plot = match shown.as_deref() {
                Some("array.plot") => plotted,
                Some("table.chart") => false,
                _ => plotted && spec.is_none(),
            };
            let range = o.state.chart_range;
            let mut list = if as_plot { chart(d, media, size, range, sel) } else { table_chart(d, media, spec, size, range, sel) };
            if part == "hit" {
                let at = |k: &str| params.get(k).and_then(Value::as_f64).filter(|f| f.is_finite()).ok_or(format!("hit needs {k}, a point in the chart"));
                let (x, y) = (at("x")?, at("y")?);
                if list.get("marks").is_none() { list } else { hit(&list, x, y) }
            } else {
                brush_band(&mut list, sel);
                // the readouts stay in the core: a shell asks for one (`part: "hit"`)
                if let Some(marks) = list.get_mut("marks").and_then(Value::as_array_mut) {
                    for m in marks {
                        if let Some(o) = m.as_object_mut() {
                            o.remove("tips");
                        }
                    }
                }
                list
            }
        }
        "grid" => grid(d, media, &o.state),
        "hex" => hex(d, media, o.state.hex_page),
        "probe" => {
            let row = whole(params, "row")?.ok_or("probe needs row and col")?;
            let col = whole(params, "col")?.ok_or("probe needs row and col")?;
            probe(d, media, row, col)
        }
        other => return Err(format!("part {:?}: table, tree, grid, hex, probe, yaml or chart", other.chars().take(32).collect::<String>())),
    };
    if let Some(m) = out.as_object_mut() {
        m.entry("status").or_insert(json!("ready"));
        m.insert("popout".into(), json!(id));
        m.insert("part".into(), json!(part));
    }
    Ok(out)
}

/// A value's JSON: inline, else its full bytes parsed on a worker.
fn json_of(d: &Display, media: &MediaService) -> Result<Arc<Value>, Value> {
    if let Some(v) = &d.value {
        return Ok(Arc::new(v.clone()));
    }
    let full = d.full.as_ref().ok_or_else(|| json!({"status": "failed", "error": "this value has nothing to read"}))?;
    match media.get(full, Want::Json).json() {
        Fetch::Ready(v) => Ok(v),
        f => Err(not_ready(&f, full, Want::Json)),
    }
}

fn table(d: &Display, media: &MediaService, st: &ViewerState, sel: Option<&Selection>, spec: Option<&Value>) -> Value {
    let (t, order): (Arc<Table>, Option<Arc<Vec<u32>>>) = match (&d.value, &d.full) {
        (Some(v), _) => match Table::from_value(v) {
            Ok(t) => {
                let order = st.sort.map(|c| Arc::new(t.order(c, st.desc)));
                (Arc::new(t), order)
            }
            Err(e) => return json!({"status": "failed", "error": e}),
        },
        (None, Some(full)) => {
            let t = match media.get(full, Want::Table).table() {
                Fetch::Ready(t) => t,
                f => return not_ready(&f, full, Want::Table),
            };
            let order = match st.sort {
                None => None,
                Some(c) => {
                    let want = Want::Sorted(u16::try_from(c).unwrap_or(u16::MAX), st.desc);
                    match media.get(full, want).order() {
                        Fetch::Ready(o) => Some(o),
                        f => return not_ready(&f, full, want),
                    }
                }
            };
            (t, order)
        }
        (None, None) => return json!({"status": "failed", "error": "this table has nothing to read"}),
    };
    // the value's selection (note 26 slice 8): each row shown says whether it is selected, and
    // `selectedOnly` shows only those, in the order sorted
    let mask = sel.map(|s| mask_of(s, &t, x_column(spec, &t).as_deref()));
    let chosen = mask.as_ref().map(|m| m.iter().filter(|b| **b).count());
    let order: Option<Arc<Vec<u32>>> = match (&mask, st.table_selected_only) {
        (Some(m), true) => {
            let all: Box<dyn Iterator<Item = u32>> = match &order {
                Some(o) => Box::new(o.iter().copied()),
                None => Box::new(0..u32::try_from(t.rows).unwrap_or(u32::MAX)),
            };
            Some(Arc::new(all.filter(|r| m.get(*r as usize).copied().unwrap_or(false)).collect()))
        }
        _ => order,
    };
    let rows = order.as_ref().filter(|_| st.table_selected_only && mask.is_some()).map_or(t.rows as u64, |o| o.len() as u64);
    // where the first selected row is in the order shown, and on which page: a shell's table
    // goes there when a selection arrives (from this window or another)
    let first_selected = mask.as_ref().and_then(|m| {
        let at_row = |i: u64| order.as_ref().map_or(Some(i), |o| o.get(i as usize).map(|r| u64::from(*r)));
        (0..rows).find_map(|i| at_row(i).filter(|r| m.get(*r as usize).copied().unwrap_or(false)).map(|r| (i, r)))
    });
    let pages = rows.div_ceil(TABLE_PAGE).max(1);
    let page = st.page.min(pages - 1);
    let start = page * TABLE_PAGE;
    let end = (start + TABLE_PAGE).min(rows);
    let columns: Vec<Value> = t.columns.iter().map(|c| json!({"name": c.name, "type": c.ty, "numeric": c.numeric()})).collect();
    let mut cells = Vec::with_capacity((end - start) as usize);
    let mut index = Vec::with_capacity((end - start) as usize);
    // colour by value (note 26 slice 2): each numeric column's range, over all its rows
    let ranges: Vec<Option<(f64, f64)>> =
        if st.table_colored { t.columns.iter().map(|c| c.numeric().then(|| range(c.cells.iter().filter_map(Cell::num)))).collect() } else { Vec::new() };
    let mut tints = Vec::new();
    for i in start..end {
        let r = order.as_ref().and_then(|o| o.get(i as usize).copied().map(u64::from)).unwrap_or(i);
        index.push(json!(r));
        cells.push(Value::Array(t.columns.iter().map(|c| json!(c.cells.get(r as usize).map(cell_text).unwrap_or_default())).collect()));
        if st.table_colored {
            tints.push(Value::Array(
                t.columns
                    .iter()
                    .zip(&ranges)
                    .map(|(c, rg)| match (rg, c.cells.get(r as usize).and_then(Cell::num)) {
                        (Some((lo, hi)), Some(x)) => tint(x, *lo, *hi),
                        _ => Value::Null,
                    })
                    .collect(),
            ));
        }
    }
    let flags: Option<Vec<Value>> =
        mask.as_ref().map(|m| index.iter().map(|r| json!(r.as_u64().and_then(|r| m.get(r as usize).copied()).unwrap_or(false))).collect());
    let mut out = json!({"columns": columns, "rows": rows, "page": page, "pages": pages, "pageRows": TABLE_PAGE, "start": start,
           "sort": st.sort, "desc": st.desc, "index": index, "cells": cells, "colored": st.table_colored,
           "selectedOnly": st.table_selected_only && mask.is_some(), "selectedRows": chosen, "allRows": t.rows,
           "firstSelected": first_selected.map(|(at, row)| json!({"at": at, "page": at / TABLE_PAGE, "row": row}))});
    if let Some(f) = flags {
        out["selected"] = Value::Array(f);
    }
    if st.table_colored {
        out["tints"] = Value::Array(tints);
    }
    out
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// A member's text in the outline: a scalar's value, a container's size.
fn leaf_text(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.as_i64().map_or_else(|| number(n.as_f64().unwrap_or(f64::NAN), false), |i| i.to_string()),
        Value::String(s) => {
            let mut t: String = s.chars().take(240).collect();
            if s.chars().count() > 240 {
                t.push('…');
            }
            format!("\"{t}\"")
        }
        Value::Array(a) => format!("[{} item{}]", a.len(), if a.len() == 1 { "" } else { "s" }),
        Value::Object(o) => format!("{{{} key{}}}", o.len(), if o.len() == 1 { "" } else { "s" }),
    }
}

fn escape(k: &str) -> String {
    k.replace('~', "~0").replace('/', "~1")
}

fn children(v: &Value, at: &str) -> Value {
    let one = |key: String, c: &Value| {
        let pointer = format!("{at}/{}", escape(&key));
        let count = match c {
            Value::Array(a) => a.len(),
            Value::Object(o) => o.len(),
            _ => 0,
        };
        json!({"key": key, "pointer": pointer, "kind": kind_of(c), "text": leaf_text(c), "count": count})
    };
    let (list, total): (Vec<Value>, usize) = match v {
        Value::Array(a) => (a.iter().take(MAX_CHILDREN).enumerate().map(|(i, c)| one(i.to_string(), c)).collect(), a.len()),
        Value::Object(o) => (o.iter().take(MAX_CHILDREN).map(|(k, c)| one(k.clone(), c)).collect(), o.len()),
        _ => (Vec::new(), 0),
    };
    json!({"kind": kind_of(v), "text": leaf_text(v), "count": total, "children": list, "more": total.saturating_sub(MAX_CHILDREN)})
}

fn tree(d: &Display, media: &MediaService, st: &ViewerState) -> Value {
    let v = match json_of(d, media) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut nodes = Map::new();
    for at in &st.expanded {
        if let Some(n) = v.pointer(at) {
            nodes.insert(at.clone(), children(n, at));
        }
    }
    json!({"root": {"kind": kind_of(&v), "text": leaf_text(&v)}, "nodes": nodes, "expanded": st.expanded})
}

fn yaml(d: &Display, media: &MediaService) -> Value {
    let v = match json_of(d, media) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let text = crate::yaml::to_yaml(&v);
    let total = text.chars().count();
    let shown: String = text.chars().take(MAX_YAML_CHARS).collect();
    json!({"text": shown, "totalChars": total, "cut": total > MAX_YAML_CHARS})
}

/// An array's bytes (`dtype`, `shape`), read on a worker.
fn array_bytes(d: &Display, media: &MediaService) -> Result<(Arc<plumcraft_store::Blob>, Vec<String>), Value> {
    let full = d.full.as_ref().ok_or_else(|| json!({"status": "failed", "error": "this array has nothing to read"}))?;
    let names = match &full.meta.form {
        Form::Array { names, .. } => names.clone(),
        _ => Vec::new(),
    };
    match media.get(full, Want::Bytes).bytes() {
        Fetch::Ready(b) => Ok((b, names)),
        f => Err(not_ready(&f, full, Want::Bytes)),
    }
}

fn grid(d: &Display, media: &MediaService, st: &ViewerState) -> Value {
    let (b, names) = match array_bytes(d, media) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let Some(dtype) = crate::dtype::Dtype::parse(&b.dtype) else {
        return json!({"status": "failed", "error": format!("this viewer can't read dtype {}", b.dtype)});
    };
    let shape: Vec<u64> = if b.shape.is_empty() { vec![1] } else { b.shape.clone() };
    let rows = shape.first().copied().unwrap_or(1);
    let cols = shape.get(1).copied().unwrap_or(1);
    let planes: u64 = shape.iter().skip(2).try_fold(1u64, |a, n| a.checked_mul(*n)).unwrap_or(0);
    let total = rows.checked_mul(cols).and_then(|n| n.checked_mul(planes.max(1)));
    if total.and_then(|n| n.checked_mul(dtype.size() as u64)).is_none_or(|n| n > b.bytes.len() as u64) {
        return json!({"status": "failed", "error": format!("{} bytes is too few for {} {}", b.bytes.len(), crate::probe::shape_text(&shape), b.dtype)});
    }
    let planes = planes.max(1);
    let plane = st.plane.min(planes - 1);
    let pages = rows.div_ceil(TABLE_PAGE).max(1);
    let page = st.page.min(pages - 1);
    let start = page * TABLE_PAGE;
    let end = (start + TABLE_PAGE).min(rows);
    // the core's window of columns (viewer.grid {col, cols}; decision 26), held to the last one
    let c0 = st.col.min(last_window(cols));
    let c1 = (c0 + GRID_COLUMNS).min(cols);
    let int = dtype.integer();
    let read = |r: u64, c: u64| usize::try_from((r * cols + c) * planes + plane).ok().and_then(|i| dtype.read(&b.bytes, i));
    // the number text: the core's, or the state's printf format (an integer array's %g as %d, as egui does)
    let fmt = st.grid_format.as_deref().map(|f| if int && f.ends_with('g') { "%d" } else { f });
    let text = |x: f64| fmt.map_or_else(|| number(x, int), |f| format_value(x, f));
    // colour by value (note 26 slice 2): the plane's range, over all its cells
    let rg = st.grid_colored.then(|| range((0..rows).flat_map(|r| (0..cols).map(move |c| (r, c))).filter_map(|(r, c)| read(r, c))));
    let mut cells = Vec::new();
    let mut tints = Vec::new();
    for r in start..end {
        let row: Vec<Value> = (c0..c1).map(|c| json!(read(r, c).map(text).unwrap_or_default())).collect();
        cells.push(Value::Array(row));
        if let Some((lo, hi)) = rg {
            tints.push(Value::Array((c0..c1).map(|c| read(r, c).map_or(Value::Null, |x| tint(x, lo, hi))).collect()));
        }
    }
    let plane_name = names.get(plane as usize).cloned();
    json!({"shape": shape, "dtype": b.dtype, "rows": rows, "cols": cols, "planes": planes, "plane": plane, "planeName": plane_name,
           "page": page, "pages": pages, "pageRows": TABLE_PAGE, "start": start, "end": end,
           "firstCol": c0, "endCol": c1, "colsShown": GRID_COLUMNS, "prevCols": c0 > 0, "nextCols": c1 < cols, "cells": cells,
           "colored": st.grid_colored, "format": st.grid_format, "tints": if rg.is_some() { Value::Array(tints) } else { Value::Null },
           "range": rg.filter(|(lo, hi)| lo.is_finite() && hi.is_finite()).map(|(lo, hi)| [text(lo), text(hi)])})
}

/// A chart's display list for a table array the core plots ([`crate::chart`]), laid out at `size`.
fn chart(d: &Display, media: &MediaService, size: (f64, f64), range: plumcraft_session::chart_view::ChartRange, sel: Option<&Selection>) -> Value {
    let Some(full) = d.full.as_ref() else { return json!({"status": "failed", "error": "this value has no numbers to plot"}) };
    let Some(kind) = crate::chart::plottable(&full.meta.form) else { return json!({"status": "failed", "error": "this value isn't a table a chart plots"}) };
    let names = match &full.meta.form {
        Form::Array { names, .. } => names.clone(),
        _ => Vec::new(),
    };
    let b = match media.get(full, Want::Bytes).bytes() {
        Fetch::Ready(b) => b,
        f => return not_ready(&f, full, Want::Bytes),
    };
    // the value's selected rows: by index, or a brush of a column (none: the chart's x)
    let selected = sel.map(|s| match s {
        Selection::Rows { rows } => rows_mask(rows, b.shape.first().copied().unwrap_or(0)),
        Selection::Brush { column, span } => crate::chart::column_values(&kind, &names, &b.dtype, &b.shape, &b.bytes, column.as_deref())
            .map(|v| v.iter().map(|x| span.contains(*x)).collect())
            .unwrap_or_default(),
    });
    match crate::chart::layout_in(&kind, &names, &b.dtype, &b.shape, &b.bytes, size, &crate::chart::View { range, selected }) {
        Ok(v) => v,
        Err(e) => json!({"status": "failed", "error": e}),
    }
}

/// A mask of `n` rows from their indices.
fn rows_mask(rows: &[u64], n: u64) -> Vec<bool> {
    let n = usize::try_from(n.min(50_000_000)).unwrap_or(0);
    let mut m = vec![false; n];
    for r in rows {
        if let Some(x) = usize::try_from(*r).ok().and_then(|r| m.get_mut(r)) {
            *x = true;
        }
    }
    m
}

/// The rows of a table a selection names (note 26 slice 8): by index, or a brush of a column's
/// numbers or times (`column` none: `default`, the chart's x; none of either: the row's index).
pub fn mask_of(sel: &Selection, t: &Table, default: Option<&str>) -> Vec<bool> {
    let n = t.rows;
    match sel {
        Selection::Rows { rows } => rows_mask(rows, n as u64),
        Selection::Brush { column, span } => match column.as_deref().or(default) {
            None => (0..n).map(|r| span.contains(r as f64)).collect(),
            Some(name) => match t.columns.iter().find(|c| c.name == name).and_then(|c| crate::chart_kinds::axis_values(c, n)) {
                Some((vals, _)) => vals.iter().map(|v| v.is_some_and(|v| span.contains(v))).collect(),
                None => vec![false; n],
            },
        },
    }
}

/// A table chart's x column: the spec's, or the one the core picks.
fn x_column(spec: Option<&Value>, t: &Table) -> Option<String> {
    let spec = spec.cloned().unwrap_or_else(|| crate::chart_kinds::auto_spec(t));
    spec.get("x").and_then(Value::as_str).map(str::to_owned)
}

/// A brush over the chart's x drawn as a band (`selection: {x: [from, to]}` in points, within
/// the plot), when the brush is of the column the x axis shows.
fn brush_band(list: &mut Value, sel: Option<&Selection>) {
    let Some(Selection::Brush { column, span }) = sel else { return };
    let title = list["axes"][0]["title"].as_str().unwrap_or_default();
    if column.as_deref().is_some_and(|c| c != title) {
        return;
    }
    let sx: Vec<f64> = list["scale"]["x"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let plot: Vec<f64> = list["plot"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    if let ([d0, d1, p0, p1], [px, _, pw, _]) = (sx.as_slice(), plot.as_slice())
        && d1 != d0
    {
        let at = |v: f64| p0 + (v - d0) / (d1 - d0) * (p1 - p0);
        let (a, b) = (at(span.lo()).clamp(*px, px + pw), at(span.hi()).clamp(*px, px + pw));
        list["selection"] = json!({"x": [crate::chart::round2(a.min(b)), crate::chart::round2(a.max(b))]});
    }
}

/// What is under a point of a chart (`viewer.data {part: "hit", x, y}`): the nearest mark's
/// readout, its series, and the data there by the chart's scales. `{hit: null}` when nothing is
/// within reach.
pub fn hit(list: &Value, x: f64, y: f64) -> Value {
    const REACH: f64 = 24.0;
    let nums = |v: &Value| -> Vec<f64> { v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default() };
    let mut best: Option<(f64, String, usize)> = None;
    for (mi, m) in list["marks"].as_array().into_iter().flatten().enumerate() {
        let Some(tips) = m["tips"].as_array() else { continue };
        let tip = |i: usize| tips.get(i).and_then(Value::as_str).unwrap_or_default().to_owned();
        let mut consider = |d: f64, i: usize| {
            if d <= REACH && best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, tip(i), mi));
            }
        };
        match m["kind"].as_str() {
            Some("rects") => {
                for (i, r) in nums(&m["rects"]).as_chunks::<4>().0.iter().enumerate() {
                    let dx = (r[0] - x).max(x - (r[0] + r[2])).max(0.0);
                    let dy = (r[1] - y).max(y - (r[1] + r[3])).max(0.0);
                    // inside a rectangle beats any point near it
                    consider(if dx == 0.0 && dy == 0.0 { -1.0 } else { dx.hypot(dy) }, i);
                }
            }
            Some("points" | "line" | "area") => {
                for (i, p) in nums(&m["xy"]).as_chunks::<2>().0.iter().enumerate().take(tips.len()) {
                    consider((p[0] - x).hypot(p[1] - y), i);
                }
            }
            Some("segments") => {
                for (i, p) in nums(&m["xy"]).as_chunks::<4>().0.iter().enumerate() {
                    consider((p[0] - x).hypot(p[1] - y).min((p[2] - x).hypot(p[3] - y)), i);
                }
            }
            _ => {}
        }
    }
    let data = |axis: &str, v: f64| -> Value {
        let s = nums(&list["scale"][axis]);
        match s.as_slice() {
            [d0, d1, p0, p1] if p1 != p0 => {
                let d = d0 + (v - p0) / (p1 - p0) * (d1 - d0);
                if axis == "x" && list["timeX"].as_bool() == Some(true) { json!(plumcraft_proto::time::text(d)) } else { json!(number(d, false)) }
            }
            _ => Value::Null,
        }
    };
    let at = json!({"x": data("x", x), "y": data("y", y)});
    match best {
        Some((_, tip, mi)) => {
            let m = &list["marks"][mi];
            json!({"hit": {"text": tip, "mark": mi, "color": m["color"], "selected": m["selected"].as_bool().unwrap_or(false)}, "at": at})
        }
        None => json!({"hit": null, "at": at}),
    }
}

/// A table's chart (note 26 slices 5 to 7): its spec, or one chosen from its columns' types, laid
/// out by [`crate::chart_kinds`]; the reply carries the spec drawn and the columns to choose from.
fn table_chart(
    d: &Display,
    media: &MediaService,
    spec: Option<Value>,
    size: (f64, f64),
    range: plumcraft_session::chart_view::ChartRange,
    sel: Option<&Selection>,
) -> Value {
    let t: Arc<Table> = match (&d.value, &d.full) {
        (Some(v), _) => match Table::from_value(v) {
            Ok(t) => Arc::new(t),
            Err(e) => return json!({"status": "failed", "error": e}),
        },
        (None, Some(full)) => match media.get(full, Want::Table).table() {
            Fetch::Ready(t) => t,
            f => return not_ready(&f, full, Want::Table),
        },
        (None, None) => return json!({"status": "failed", "error": "this value has no table to chart"}),
    };
    let chosen = spec.is_some();
    let spec = spec.unwrap_or_else(|| crate::chart_kinds::auto_spec(&t));
    let columns: Vec<Value> = t
        .columns
        .iter()
        .take(4096)
        .map(|c| json!({"name": c.name, "numeric": c.numeric(), "time": !c.numeric() && crate::chart_kinds::axis_values(c, t.rows.min(10_000)).is_some()}))
        .collect();
    let selected = sel.map(|s| mask_of(s, &t, x_column(Some(&spec), &t).as_deref()));
    match crate::chart_kinds::layout_in(&spec, &t, size, &crate::chart::View { range, selected }) {
        Ok(mut v) => {
            v["spec"] = spec;
            v["chosen"] = json!(chosen);
            v["columns"] = json!(columns);
            v
        }
        Err(e) => json!({"status": "failed", "error": e, "spec": spec, "chosen": chosen, "columns": columns}),
    }
}

fn hex(d: &Display, media: &MediaService, page: u64) -> Value {
    let Some(full) = d.full.as_ref() else { return json!({"status": "failed", "error": "this value has no bytes to show"}) };
    let b = match media.get(full, Want::Bytes).bytes() {
        Fetch::Ready(b) => b,
        f => return not_ready(&f, full, Want::Bytes),
    };
    let len = b.bytes.len() as u64;
    let pages = len.div_ceil(HEX_PAGE).max(1);
    let page = page.min(pages - 1);
    let start = page * HEX_PAGE;
    let end = (start + HEX_PAGE).min(len);
    let mut lines = Vec::new();
    let mut at = start;
    while at < end {
        let stop = (at + 16).min(end);
        let chunk = b.bytes.get(at as usize..stop as usize).unwrap_or_default();
        let hexs: Vec<String> = chunk.iter().map(|x| format!("{x:02x}")).collect();
        let ascii: String = chunk.iter().map(|x| if x.is_ascii_graphic() || *x == b' ' { char::from(*x) } else { '.' }).collect();
        lines.push(json!(format!("{at:08x}  {:<47}  {ascii}", hexs.join(" "))));
        at = stop;
    }
    json!({"bytes": len, "page": page, "pages": pages, "pageBytes": HEX_PAGE, "start": start, "lines": lines})
}

fn probe(d: &Display, media: &MediaService, row: u64, col: u64) -> Value {
    let Some(full) = d.full.as_ref() else { return json!({"status": "failed", "error": crate::probe::no_numbers(d, false)}) };
    let want = if d.media == Media::Image { Want::Numbers } else { Want::Bytes };
    let b = match media.get(full, want).bytes() {
        Fetch::Ready(b) => b,
        f => return not_ready(&f, full, want),
    };
    let names = match &full.meta.form {
        Form::Array { names, .. } => names.clone(),
        _ => plumcraft_media::decode::image_channel_names(b.shape.get(2).map_or(1, |c| *c as usize)),
    };
    let src = match crate::probe::Pixels::of(&b, &names) {
        Ok(p) => crate::probe::Source::Ready(p),
        Err(e) => crate::probe::Source::Unreadable(e),
    };
    let mut v = crate::probe::reading(&src, (row as usize, col as usize));
    v["shape"] = json!(b.shape);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::Handle;
    use plumcraft_proto::data::BlobMeta;
    use plumcraft_store::Blob;

    fn display(media: Media, value: Option<Value>) -> Display {
        let mut d = Display::default();
        (d.media, d.value) = (media, value);
        d
    }

    #[test]
    fn numbers_are_the_cores_text() {
        assert_eq!(number(3.0, true), "3");
        assert_eq!(number(0.30000000000000004, false), "0.3");
        assert_eq!(number(0.375, false), "0.375");
        assert_eq!(number(1.0 / 3.0, false), "0.333333");
        assert_eq!(number(2.5e12, false), "2.5000e12");
        assert_eq!(number(f64::NAN, false), "NaN");
        assert_eq!(number(-0.00001, false), "-1.0000e-5");
    }

    #[test]
    fn every_core_type_is_offered_a_viewer_other_than_bytes() {
        for m in [Media::Image, Media::Vector, Media::Ndarray, Media::Audio, Media::Table, Media::Text, Media::Scalar, Media::Tree] {
            let o = offered(&display(m, None));
            assert!(o.first().is_some_and(|(id, _)| NATIVE.contains(id) && *id != "hex"), "{m:?}: {o:?}");
        }
        let rows = offered(&display(Media::Bytes, Some(json!([[1, 2], [3, 4]]))));
        assert_eq!(rows.first().map(|x| x.0), Some("table.grid"));
    }

    fn media_with(bytes: Vec<u8>, dtype: &str, shape: Vec<u64>) -> (MediaService, DataRef) {
        let m = MediaService::default();
        let b = Blob { dtype: dtype.into(), shape, bytes: bytes.into() };
        m.set_resolver(Some(Arc::new(move |_: &DataRef| Some(b.clone()))));
        let r = DataRef { handle: Some(Handle::raw(b"v")), meta: BlobMeta::default(), at: vec![] };
        (m, r)
    }

    fn until(f: impl Fn() -> Value) -> Value {
        for _ in 0..500 {
            let v = f();
            if v["status"] != "pending" {
                return v;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        f()
    }

    #[test]
    fn a_large_table_is_read_off_the_callers_thread_sorted_and_paged() {
        let n = 20_000;
        let vals: Vec<Value> = (0..n).map(|i| json!((i * 7919) % n)).collect();
        let t = json!({"columns": [{"name": "k", "type": "INT", "values": vals}]});
        let (m, r) = media_with(serde_json::to_vec(&t).unwrap(), "|u1", vec![]);
        let mut d = display(Media::Table, None);
        d.full = Some(r);
        let mut st = ViewerState::default();
        let first = table(&d, &m, &st, None, None);
        assert_eq!(first["status"], "pending", "never read on the asker's thread");
        assert!(first["wait"].as_str().unwrap().starts_with("derive:table:"));
        let page = until(|| table(&d, &m, &st, None, None));
        assert_eq!((page["rows"].as_u64(), page["pages"].as_u64(), page["cells"].as_array().map(Vec::len)), (Some(n as u64), Some(40), Some(500)));
        st.sort = Some(0);
        st.desc = true;
        st.page = 999;
        let sorted = until(|| table(&d, &m, &st, None, None));
        assert_eq!(sorted["page"], 39, "a page past the end is the last");
        assert_eq!(sorted["cells"][499][0], "0");
        st.page = 0;
        assert_eq!(until(|| table(&d, &m, &st, None, None))["cells"][0][0], json!((n - 1).to_string()));
    }

    /// B152: a histogram from an engine that says "table" of a (256, 2) float array. The Table
    /// viewer said "not a table: expected value at line 1 column 1".
    #[test]
    fn an_array_marked_as_a_table_shows_in_the_table_viewer() {
        let bytes: Vec<u8> = (0..256).flat_map(|i| [(i as f32 + 0.5) / 256.0, (255 - i) as f32]).flat_map(f32::to_le_bytes).collect();
        let (m, mut r) = media_with(bytes, "<f4", vec![256, 2]);
        r.meta.form = Form::Array { dtype: "<f4".into(), shape: vec![256, 2], space: None, names: vec!["centre".into(), "count".into()], table: true };
        let mut d = display(Media::Table, None);
        d.full = Some(r);
        let ids: Vec<&str> = offered(&d).iter().map(|x| x.0).collect();
        assert_eq!(ids, ["array.plot", "table.grid", "table.chart", "hex"], "a plot first (note 26 slice 5), a chart beside it (FR6), no outline of an array");
        let mut st = ViewerState::default();
        let page = until(|| table(&d, &m, &st, None, None));
        assert_eq!(page["status"], Value::Null, "{page}");
        assert_eq!(
            (page["rows"].as_u64(), page["columns"][0]["name"].as_str(), page["columns"][1]["name"].as_str()),
            (Some(256), Some("centre"), Some("count"))
        );
        assert_eq!(page["columns"][1]["numeric"], true);
        assert_eq!(page["cells"][0][1], "255");
        st.sort = Some(1);
        assert_eq!(until(|| table(&d, &m, &st, None, None))["cells"][0][1], "0", "sorted by count");
    }

    /// B154 slice 1: `popout.get`'s statistics carry each channel's NaN count and its histogram
    /// (counts, heights on the state's scale, the range as the core's text); a hostile histogram
    /// (no counts, too many bins, NaN or infinite range, huge counts) never breaks it.
    #[test]
    fn a_pop_outs_statistics_carry_each_channels_histogram() {
        use plumcraft_proto::display::{ChannelStats, Histogram, Stats};
        let h = |counts: Vec<u64>, lo: Option<f64>, hi: Option<f64>| Some(Histogram { bins: counts.len() as u32, lo, hi, counts });
        let mut d = display(Media::Ndarray, None);
        d.stats = Stats {
            channels: vec![
                ChannelStats { name: "L".into(), min: Some(0.0), max: Some(100.0), nan: 3, histogram: h(vec![0, 9, 99, 0], Some(0.0), Some(100.0)) },
                ChannelStats { name: "a".into(), min: None, max: None, nan: 7, histogram: h(vec![], Some(0.0), Some(1.0)) },
                ChannelStats { name: "b".into(), min: Some(-1.0), max: Some(1.0), nan: 0, histogram: h(vec![1; 5000], Some(f64::NAN), Some(f64::INFINITY)) },
                ChannelStats { name: "c".into(), min: Some(0.0), max: Some(0.0), nan: 0, histogram: h(vec![u64::MAX, u64::MAX, 0], Some(0.0), Some(1.0)) },
            ],
            ..Default::default()
        };
        let mut st = ViewerState::default();
        let x = popout_extra(Some(&d), &st);
        assert!(x.get("path").is_none() && x.get("levels").is_none(), "a list that isn't nested says nothing of levels");
        let nested = {
            let mut x = d.clone();
            x.items = 3;
            x.index = 1;
            x.path = vec![1];
            x.levels = vec![3, 2];
            x
        };
        let y = popout_extra(Some(&nested), &st);
        assert_eq!((&y["index"], &y["path"], &y["levels"]), (&json!(1), &json!([1]), &json!([3, 2])));
        let s = &x["stats"];
        assert_eq!((s[0]["nan"].as_u64(), s[1]["nan"].as_u64()), (Some(3), Some(7)));
        let l = &s[0]["histogram"];
        assert_eq!(l["counts"], json!([0, 9, 99, 0]));
        assert_eq!(
            (l["bins"].as_u64(), l["top"].as_u64(), l["lo"].as_str(), l["hi"].as_str(), l["log"].as_bool()),
            (Some(4), Some(99), Some("0"), Some("100"), Some(false))
        );
        assert_eq!(l["heights"][2], 1.0);
        assert_eq!(l["heights"][1], json!(0.0909), "9 of 99, four decimals");
        assert!(s[1].get("histogram").is_none(), "no counts, no histogram");
        let b = &s[2]["histogram"];
        assert_eq!(b["counts"].as_array().map(Vec::len), Some(MAX_HISTOGRAM_BINS), "bins past the most are summed into the last");
        assert_eq!(b["counts"][MAX_HISTOGRAM_BINS - 1].as_u64(), Some(5000 - MAX_HISTOGRAM_BINS as u64 + 1));
        assert_eq!((b["lo"].clone(), b["hi"].clone()), (Value::Null, Value::Null), "a range that isn't finite isn't given");
        assert_eq!(s[3]["histogram"]["heights"], json!([1.0, 1.0, 0.0]), "the largest counts");
        st.log = true;
        let l = &popout_extra(Some(&d), &st)["stats"][0]["histogram"];
        assert_eq!(l["log"], true);
        let want = ((10f64.ln() / 100f64.ln()) * 1e4).round() / 1e4;
        assert_eq!(l["heights"][1].as_f64(), Some(want), "ln(1 + 9) / ln(1 + 99)");
        assert_eq!(popout_extra(Some(&d), &st)["state"]["array"]["log"], true);
    }

    /// Note 26 slice 2: a grid's numbers in the state's format, and each cell's tint from the
    /// plane's range; a table's numeric columns tinted over each column's range, text columns not.
    #[test]
    fn colour_by_value_and_the_number_format() {
        let vals: Vec<u8> = [0.0f32, 1.0, 2.0, 3.0, f32::NAN, 4.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let (m, r) = media_with(vals, "<f4", vec![2, 3]);
        let mut d = display(Media::Ndarray, None);
        d.full = Some(r);
        let mut st = ViewerState::default();
        let g = until(|| grid(&d, &m, &st));
        assert_eq!((g["tints"].clone(), g["cells"][0][1].clone()), (Value::Null, json!("1")), "off by default; the core's numbers");
        st.grid_colored = true;
        st.grid_format = Some("%.2f".into());
        let g = until(|| grid(&d, &m, &st));
        assert_eq!(g["cells"][0][1], "1.00");
        assert_eq!(g["tints"], json!([[0.0, 0.25, 0.5], [0.75, null, 1.0]]), "NaN untinted; the plane's range 0 to 4");
        assert_eq!(g["range"], json!(["0.00", "4.00"]));
        let t = display(
            Media::Table,
            Some(json!({"columns": [{"name": "n", "type": "INT", "values": [10, 20, 30]}, {"name": "s", "type": "STRING", "values": ["a", "b", "c"]}]})),
        );
        let mut st = ViewerState::default();
        assert!(table(&t, &m, &st, None, None).get("tints").is_none());
        st.table_colored = true;
        assert_eq!(table(&t, &m, &st, None, None)["tints"], json!([[0.0, null], [0.5, null], [1.0, null]]));
        assert_eq!(tint(5.0, 5.0, 5.0), json!(0.5), "a single value");
        assert_eq!(tint(f64::INFINITY, 0.0, 1.0), Value::Null);
        assert_eq!(format_value(1234.5678, "%.4g"), "1235");
    }

    /// Note 26 slice 4: a value typed `text/csv` is a table first and its text second; a TEXT
    /// that is an SVG document is a picture first and its source second; text that only mentions
    /// svg, or merely looks like CSV, stays text.
    #[test]
    fn svg_text_longer_than_its_excerpt_is_drawn_from_its_bytes() {
        let long = format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{}</svg>", "<g/>".repeat(5000));
        let mut d = display(Media::Text, None);
        d.renditions = vec![Rendition::Excerpt { text: long.chars().take(4096).collect(), offset: 0, total_chars: long.chars().count() as u64, lines: 1 }];
        assert!(svg_text_ref(&d).is_none(), "no bytes: no picture");
        let mut r = DataRef { handle: Some(plumcraft_proto::Handle::raw(long.as_bytes())), ..Default::default() };
        r.meta.media = "text/plain".into();
        r.meta.size = long.len() as u64;
        d.full = Some(r.clone());
        let got = svg_text_ref(&d).unwrap();
        assert_eq!((got.handle, got.meta.media.as_str()), (r.handle.clone(), plumcraft_proto::catalogue::core_types::SVG_MEDIA));
        r.meta.size = MAX_SVG_TEXT as u64 + 1;
        d.full = Some(r);
        assert!(svg_text_ref(&d).is_none(), "too large to draw");
    }

    /// Note 26 slice 8: what is under a point of a chart is the core's: inside a bar, or the
    /// nearest point within reach; the data there by the chart's scales.
    #[test]
    fn a_hit_reads_the_mark_under_a_point() {
        let list = json!({"scale": {"x": [0.0, 10.0, 0.0, 100.0], "y": [0.0, 1.0, 100.0, 0.0]}, "marks": [
            {"kind": "rects", "rects": [10, 50, 20, 50], "tips": ["a: 0.5"], "color": "#4e79a7"},
            {"kind": "line", "xy": [0, 100, 50, 0], "tips": ["x 0: 0", "x 5: 1"]},
        ]});
        assert_eq!(hit(&list, 20.0, 80.0)["hit"]["text"], "a: 0.5", "inside the bar");
        assert_eq!(hit(&list, 52.0, 3.0)["hit"]["text"], "x 5: 1", "the nearest point");
        assert_eq!(hit(&list, 90.0, 50.0)["hit"], Value::Null, "nothing within reach");
        assert_eq!(hit(&list, 50.0, 50.0)["at"], json!({"x": "5", "y": "0.5"}));
        assert_eq!(hit(&json!({}), 1.0, 1.0)["hit"], Value::Null, "a list without marks");
    }

    /// Note 26 slice 8: a selection's rows, by index or by a brush of a column (times as
    /// seconds); with no column, the chart's x, else the row's index.
    #[test]
    fn a_selection_names_rows() {
        use plumcraft_session::chart_view::Span;
        let t = Table::from_value(&json!({"columns": [
            {"name": "d", "type": "STRING", "values": ["1970-01-01", "1970-01-03", null]},
            {"name": "n", "type": "INT", "values": [1, 5, 9]},
        ]}))
        .unwrap();
        let brush = |c: Option<&str>, lo, hi| Selection::Brush { column: c.map(str::to_owned), span: Span::new(lo, hi).unwrap() };
        assert_eq!(mask_of(&brush(Some("n"), 4.0, 10.0), &t, None), [false, true, true]);
        assert_eq!(mask_of(&brush(None, 0.0, 86_400.0), &t, Some("d")), [true, false, false], "the default column, times");
        assert_eq!(mask_of(&brush(None, 0.5, 2.0), &t, None), [false, true, true], "by index");
        assert_eq!(mask_of(&brush(Some("nowhere"), 0.0, 1.0), &t, None), [false, false, false]);
        assert_eq!(mask_of(&Selection::Rows { rows: vec![2, 99] }, &t, None), [false, false, true]);
    }

    #[test]
    fn interchange_formats_are_offered_as_what_they_are() {
        let mut r = DataRef::default();
        r.meta.media = "text/csv; charset=utf-8".into();
        let mut d = display(Media::Bytes, None);
        d.full = Some(r);
        let ids: Vec<&str> = offered(&d).iter().map(|x| x.0).collect();
        assert_eq!(ids, ["table.grid", "table.chart", "text.plain", "hex"]);
        for (text, svg) in [
            ("<svg xmlns='http://www.w3.org/2000/svg'/>", true),
            ("<?xml version='1.0'?>\n<!-- made -->\n<!DOCTYPE svg>\n  <svg width='4'>", true),
            ("<svgx>", false),
            ("see <svg> below", false),
            ("a,b\n1,2\n", false),
            ("<?xml version='1.0'", false),
        ] {
            let t = display(Media::Text, Some(json!(text)));
            let first = offered(&t).first().map(|x| x.0);
            assert_eq!(first == Some("svg"), svg, "{text}");
            if svg {
                assert_eq!(offered(&t).iter().map(|x| (x.0, x.1)).collect::<Vec<_>>()[1], ("text.plain", "Source"));
            }
        }
    }

    /// A table-flagged array of numbers (the Histogram node's) is offered as a table first.
    #[test]
    fn a_table_flagged_array_is_a_table_first() {
        let mut r = DataRef::default();
        r.meta.form = Form::Array { dtype: "<f4".into(), shape: vec![256, 2], space: None, names: vec!["centre".into(), "count".into()], table: true };
        let mut d = display(Media::Ndarray, None);
        d.full = Some(r);
        let ids: Vec<&str> = offered(&d).iter().map(|x| x.0).collect();
        assert_eq!(
            ids,
            ["array.plot", "table.grid", "table.chart", "array.pictures", "array.grid", "hex"],
            "a histogram plots first, its table next, a chart beside them (FR6)"
        );
        let mut p = DataRef::default();
        p.meta.form = Form::Array { dtype: "<f4".into(), shape: vec![4, 4], space: None, names: vec![], table: false };
        d.full = Some(p);
        let ids: Vec<&str> = offered(&d).iter().map(|x| x.0).collect();
        assert_eq!(ids, ["array.pictures", "array.grid", "table.grid", "table.chart", "hex"], "a picture stays pictures first, and is a table and a chart too");
    }

    /// FR6 (note 30, "All A"; "In general, it's the user's job to deal with odd, and your job to
    /// make seeing odd things easy"): every array is offered what can be computed for its kind,
    /// dtype and rank, whatever it looks like; a flag only orders them.
    #[test]
    fn every_array_is_offered_what_its_rank_allows() {
        let offer = |dtype: &str, shape: Vec<u64>, names: Vec<&str>, table: bool| -> Vec<&'static str> {
            let mut r = DataRef::default();
            r.meta.form = Form::Array { dtype: dtype.into(), shape, space: None, names: names.into_iter().map(str::to_owned).collect(), table };
            let mut d = display(Media::Ndarray, None);
            d.full = Some(r);
            offered(&d).into_iter().map(|x| x.0).collect()
        };
        // a series: plotted first, a one-column table and its chart
        assert_eq!(offer("<f4", vec![100], vec![], false), ["array.plot", "table.grid", "table.chart", "array.pictures", "array.grid", "hex"]);
        // a picture of three channels: H×W rows of three columns
        assert_eq!(offer("<f4", vec![64, 48, 3], vec!["L", "a", "b"], false), ["array.pictures", "array.grid", "table.grid", "table.chart", "hex"]);
        // a labelled map, an integer plane
        assert_eq!(offer("<i4", vec![64, 48], vec![], false), ["array.pictures", "array.grid", "table.grid", "table.chart", "hex"]);
        // a volume, rank 3 with more planes than a picture takes; rank 4
        assert_eq!(offer("<f4", vec![10, 32, 200], vec![], false), ["array.pictures", "array.grid", "table.grid", "table.chart", "hex"]);
        assert_eq!(offer("<u2", vec![2, 3, 4, 5], vec![], false), ["array.pictures", "array.grid", "table.grid", "table.chart", "hex"]);
        // two columns plot as points, unflagged too; the pictures stay first
        assert_eq!(offer("<f8", vec![500, 2], vec![], false), ["array.pictures", "array.grid", "array.plot", "table.grid", "table.chart", "hex"]);
        // past the table reader's limits (more columns than it reads): no Table, no Chart
        assert_eq!(offer("<f4", vec![2, 5000], vec![], false), ["array.pictures", "array.grid", "hex"]);
        // not numbers: no table
        assert_eq!(offer("<U8", vec![4], vec![], false), ["array.pictures", "array.grid", "hex"]);
    }

    /// A `text/csv` value's page: read as CSV on the media workers, typed columns, sorted by the core.
    #[test]
    fn a_csv_value_pages_as_a_table() {
        let (m, mut r) = media_with(b"name,n\nb,2\na,10\nc,1\n".to_vec(), "|u1", vec![]);
        r.meta.media = "text/csv".into();
        let mut d = display(Media::Bytes, None);
        d.full = Some(r);
        let mut st = ViewerState { sort: Some(1), ..Default::default() };
        let page = until(|| table(&d, &m, &st, None, None));
        assert_eq!((page["rows"].as_u64(), page["columns"][1]["numeric"].as_bool()), (Some(3), Some(true)), "{page}");
        assert_eq!(page["cells"][0], json!(["c", "1"]), "sorted by n");
        st.sort = None;
        let (m2, mut bad) = media_with(b"a,b\n\"open\n".to_vec(), "|u1", vec![]);
        bad.meta.media = "text/csv".into();
        d.full = Some(bad);
        assert_eq!(until(|| table(&d, &m2, &st, None, None))["status"], "failed");
    }

    #[test]
    fn malformed_values_are_refused_with_a_reason() {
        let (m, r) = media_with(b"{\"columns\": 7".to_vec(), "|u1", vec![]);
        let mut d = display(Media::Table, None);
        d.full = Some(r.clone());
        let v = until(|| table(&d, &m, &ViewerState::default(), None, None));
        assert_eq!(v["status"], "failed", "{v}");
        let mut j = display(Media::Tree, None);
        j.full = Some(r);
        let v = until(|| tree(&j, &m, &ViewerState::default()));
        assert_eq!(v["status"], "failed", "{v}");
        let (m, r) = media_with(vec![1, 2, 3], "<f4", vec![2, 2]);
        let mut a = display(Media::Ndarray, None);
        a.full = Some(r);
        assert_eq!(until(|| grid(&a, &m, &ViewerState::default()))["status"], "failed");
        assert!(until(|| hex(&a, &m, 0))["lines"][0].as_str().unwrap().starts_with("00000000  01 02 03"));
    }

    #[test]
    fn an_outline_lists_the_open_members() {
        let d = display(Media::Tree, Some(json!({"a": [1, 2.5, {"b/c": null}], "s": "x"})));
        let m = MediaService::default();
        let mut st = ViewerState::default();
        let v = tree(&d, &m, &st);
        assert_eq!(v["nodes"][""]["children"][0]["text"], "[3 items]");
        st.expanded.insert("/a".into());
        st.expanded.insert("/a/2".into());
        let v = tree(&d, &m, &st);
        assert_eq!(v["nodes"]["/a"]["children"][1]["text"], "2.5");
        assert_eq!(v["nodes"]["/a/2"]["children"][0]["pointer"], "/a/2/b~1c");
        assert!(yaml(&d, &m)["text"].as_str().unwrap().contains("s: x"));
    }

    #[test]
    fn a_grid_reads_a_plane_page_by_page() {
        let vals: Vec<u8> = (0..(3 * 4 * 2) as u16).flat_map(|i| f32::from(i).to_le_bytes()).collect();
        let (m, r) = media_with(vals, "<f4", vec![3, 4, 2]);
        let mut d = display(Media::Ndarray, None);
        d.full = Some(r);
        let mut st = ViewerState { plane: 1, ..Default::default() };
        let g = until(|| grid(&d, &m, &st));
        assert_eq!(g["cells"][1][2], "13");
        st.plane = 9;
        assert_eq!(until(|| grid(&d, &m, &st))["plane"], 1);
    }

    /// Decision 26 (N4 person gate): which columns a grid shows is the core's window, 64 at a
    /// time, never past the last one, with whether there is more either side.
    #[test]
    fn a_wide_grid_shows_the_cores_window_of_columns() {
        let (rows, cols) = (2u64, 150u64);
        let vals: Vec<u8> = (0..rows * cols).flat_map(|i| (i as f32).to_le_bytes()).collect();
        let (m, r) = media_with(vals, "<f4", vec![rows, cols]);
        let mut d = display(Media::Ndarray, None);
        d.full = Some(r);
        let mut st = ViewerState::default();
        let g = until(|| grid(&d, &m, &st));
        assert_eq!((g["firstCol"].as_u64(), g["endCol"].as_u64(), g["cols"].as_u64()), (Some(0), Some(64), Some(150)));
        assert_eq!((g["prevCols"].as_bool(), g["nextCols"].as_bool(), g["colsShown"].as_u64()), (Some(false), Some(true), Some(64)));
        assert_eq!(g["cells"][0].as_array().map(Vec::len), Some(64));
        st.col = 128;
        let g = until(|| grid(&d, &m, &st));
        assert_eq!((g["firstCol"].as_u64(), g["endCol"].as_u64(), g["nextCols"].as_bool()), (Some(128), Some(150), Some(false)));
        assert_eq!(g["cells"][1][0], "278", "row 1, column 128");
        // a window past the end (a value replaced by a narrower one) is held to the last
        st.col = 1 << 20;
        assert_eq!(until(|| grid(&d, &m, &st))["firstCol"], 128);
    }
}
