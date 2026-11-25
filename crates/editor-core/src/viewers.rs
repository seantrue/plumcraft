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
    })?;
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

/// The viewers a value is offered in, its default first: egui's registry
/// (`plumcraft_ui::viewers::kinds_for`, which a test there holds this to), and SVG's source as text
/// beside its picture. `(id, label)`.
pub fn offered(d: &Display) -> Vec<(&'static str, &'static str)> {
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
    // a table-flagged array (a histogram's centre and count, a node's points) is a table first: it
    // has no picture (note 26 slice 2; B152 read it as a table for a TABLE value only)
    let table_array = d.full.as_ref().is_some_and(|f| matches!(f.meta.form, Form::Array { table: true, .. }));
    // a table of positions, or a histogram, plots first (the egui editor's Plot; note 26 slice 5,
    // laid out by the core as a chart's display list); its numbers stay a viewer away
    add(d.full.as_ref().is_some_and(|f| crate::chart::plotted(&f.meta.form).is_some()), "array.plot", "Plot");
    add(m == Media::Ndarray && table_array, "table.grid", "Table");
    add(m == Media::Ndarray, "array.pictures", "Pictures");
    add(m == Media::Ndarray, "array.grid", "Values");
    add(m == Media::Audio, "audio", "Audio");
    // a CSV is a table first, its text a viewer away
    add((m == Media::Table || csv || d.value.as_ref().is_some_and(is_rows)) && !(m == Media::Ndarray && table_array), "table.grid", "Table");
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

/// The viewers the Apple shells draw (N4 slice 5); any other offered falls back to the engine's
/// rendition with the summary (contract 13.3).
pub const NATIVE: &[&str] =
    &["picture", "svg", "array.plot", "array.pictures", "array.grid", "audio", "table.grid", "text.plain", "scalar", "yaml", "tree", "items", "hex"];

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
    let d = crate::widget_text::needed_decimals(&json!(x));
    crate::widget_text::number_text(&json!(x), false, d)
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
    crate::control::check_fields(params, &["popout", "part", "row", "col", "width", "height"])?;
    let id = params.get("popout").and_then(Value::as_u64).ok_or("popout must be a pop-out's id")?;
    let o = session.views.popout(id).ok_or_else(|| format!("no pop-out {id}"))?;
    let (_, d) = session.run.entry(WatchRole::Popout(id)).ok_or_else(|| format!("no pop-out {id}"))?;
    let d = d.ok_or("no value yet: run the graph")?;
    let part = params.get("part").and_then(Value::as_str).ok_or("part is table, tree, grid, hex, probe or yaml")?;
    let mut out = match part {
        "table" => table(d, media, &o.state),
        "tree" => tree(d, media, &o.state),
        "yaml" => yaml(d, media),
        "chart" => {
            let side = |k: &str, dflt: f64| match params.get(k) {
                None | Some(Value::Null) => Ok(dflt),
                Some(v) => v.as_f64().filter(|f| f.is_finite() && *f > 0.0).ok_or(format!("{k} must be a positive number of points")),
            };
            chart(d, media, (side("width", crate::chart::DEFAULT_SIZE.0)?, side("height", crate::chart::DEFAULT_SIZE.1)?))
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

fn table(d: &Display, media: &MediaService, st: &ViewerState) -> Value {
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
    let rows = t.rows as u64;
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
    let mut out = json!({"columns": columns, "rows": rows, "page": page, "pages": pages, "pageRows": TABLE_PAGE, "start": start,
           "sort": st.sort, "desc": st.desc, "index": index, "cells": cells, "colored": st.table_colored});
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
fn chart(d: &Display, media: &MediaService, size: (f64, f64)) -> Value {
    let Some(full) = d.full.as_ref() else { return json!({"status": "failed", "error": "this value has no numbers to plot"}) };
    let Some(kind) = crate::chart::plotted(&full.meta.form) else { return json!({"status": "failed", "error": "this value isn't a table a chart plots"}) };
    let names = match &full.meta.form {
        Form::Array { names, .. } => names.clone(),
        _ => Vec::new(),
    };
    let b = match media.get(full, Want::Bytes).bytes() {
        Fetch::Ready(b) => b,
        f => return not_ready(&f, full, Want::Bytes),
    };
    match crate::chart::layout(&kind, &names, &b.dtype, &b.shape, &b.bytes, size) {
        Ok(v) => v,
        Err(e) => json!({"status": "failed", "error": e}),
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
        Display { media, value, ..Default::default() }
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
        let first = table(&d, &m, &st);
        assert_eq!(first["status"], "pending", "never read on the asker's thread");
        assert!(first["wait"].as_str().unwrap().starts_with("derive:table:"));
        let page = until(|| table(&d, &m, &st));
        assert_eq!((page["rows"].as_u64(), page["pages"].as_u64(), page["cells"].as_array().map(Vec::len)), (Some(n as u64), Some(40), Some(500)));
        st.sort = Some(0);
        st.desc = true;
        st.page = 999;
        let sorted = until(|| table(&d, &m, &st));
        assert_eq!(sorted["page"], 39, "a page past the end is the last");
        assert_eq!(sorted["cells"][499][0], "0");
        st.page = 0;
        assert_eq!(until(|| table(&d, &m, &st))["cells"][0][0], json!((n - 1).to_string()));
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
        assert_eq!(ids, ["array.plot", "table.grid", "hex"], "a plot first (note 26 slice 5), no outline of an array");
        let mut st = ViewerState::default();
        let page = until(|| table(&d, &m, &st));
        assert_eq!(page["status"], Value::Null, "{page}");
        assert_eq!(
            (page["rows"].as_u64(), page["columns"][0]["name"].as_str(), page["columns"][1]["name"].as_str()),
            (Some(256), Some("centre"), Some("count"))
        );
        assert_eq!(page["columns"][1]["numeric"], true);
        assert_eq!(page["cells"][0][1], "255");
        st.sort = Some(1);
        assert_eq!(until(|| table(&d, &m, &st))["cells"][0][1], "0", "sorted by count");
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
        assert!(table(&t, &m, &st).get("tints").is_none());
        st.table_colored = true;
        assert_eq!(table(&t, &m, &st)["tints"], json!([[0.0, null], [0.5, null], [1.0, null]]));
        assert_eq!(tint(5.0, 5.0, 5.0), json!(0.5), "a single value");
        assert_eq!(tint(f64::INFINITY, 0.0, 1.0), Value::Null);
        assert_eq!(format_value(1234.5678, "%.4g"), "1235");
    }

    /// Note 26 slice 4: a value typed `text/csv` is a table first and its text second; a TEXT
    /// that is an SVG document is a picture first and its source second; text that only mentions
    /// svg, or merely looks like CSV, stays text.
    #[test]
    fn interchange_formats_are_offered_as_what_they_are() {
        let mut r = DataRef::default();
        r.meta.media = "text/csv; charset=utf-8".into();
        let mut d = display(Media::Bytes, None);
        d.full = Some(r);
        let ids: Vec<&str> = offered(&d).iter().map(|x| x.0).collect();
        assert_eq!(ids, ["table.grid", "text.plain", "hex"]);
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
        assert_eq!(ids, ["array.plot", "table.grid", "array.pictures", "array.grid", "hex"], "a histogram plots first, its table next");
        let mut p = DataRef::default();
        p.meta.form = Form::Array { dtype: "<f4".into(), shape: vec![4, 4], space: None, names: vec![], table: false };
        d.full = Some(p);
        assert_eq!(offered(&d).first().map(|x| x.0), Some("array.pictures"), "a picture stays pictures first");
    }

    /// A `text/csv` value's page: read as CSV on the media workers, typed columns, sorted by the core.
    #[test]
    fn a_csv_value_pages_as_a_table() {
        let (m, mut r) = media_with(b"name,n\nb,2\na,10\nc,1\n".to_vec(), "|u1", vec![]);
        r.meta.media = "text/csv".into();
        let mut d = display(Media::Bytes, None);
        d.full = Some(r);
        let mut st = ViewerState { sort: Some(1), ..Default::default() };
        let page = until(|| table(&d, &m, &st));
        assert_eq!((page["rows"].as_u64(), page["columns"][1]["numeric"].as_bool()), (Some(3), Some(true)), "{page}");
        assert_eq!(page["cells"][0], json!(["c", "1"]), "sorted by n");
        st.sort = None;
        let (m2, mut bad) = media_with(b"a,b\n\"open\n".to_vec(), "|u1", vec![]);
        bad.meta.media = "text/csv".into();
        d.full = Some(bad);
        assert_eq!(until(|| table(&d, &m2, &st))["status"], "failed");
    }

    #[test]
    fn malformed_values_are_refused_with_a_reason() {
        let (m, r) = media_with(b"{\"columns\": 7".to_vec(), "|u1", vec![]);
        let mut d = display(Media::Table, None);
        d.full = Some(r.clone());
        let v = until(|| table(&d, &m, &ViewerState::default()));
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
