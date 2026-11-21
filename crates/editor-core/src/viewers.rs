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
use plumcraft_session::viewer_state::{HEX_PAGE, TABLE_PAGE, ViewerState};
use serde_json::{Map, Value, json};

/// The most columns a grid page carries.
pub const GRID_COLUMNS: usize = 64;
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
    add(m == Media::Vector, "svg", "SVG");
    add(is_pdf(d), "pdf.pages", "Pages");
    add(m == Media::Doc, "doc", "Document");
    add(m == Media::Ndarray, "array.pictures", "Pictures");
    add(m == Media::Ndarray, "array.grid", "Values");
    add(m == Media::Audio, "audio", "Audio");
    add(m == Media::Table || d.value.as_ref().is_some_and(is_rows), "table.grid", "Table");
    add(is_xml(d), "xml", "XML");
    add(m == Media::Text || m == Media::Vector, "text.plain", if m == Media::Vector { "Source" } else { "Text" });
    add(m == Media::Scalar, "scalar", "Value");
    add(m == Media::Tree, "yaml", "YAML");
    add(m == Media::Tree || m == Media::Table || obj, "tree", "Tree");
    add(d.items > 1 && !d.sheet.is_empty(), "items", "Items");
    add(d.full.is_some() && m != Media::Doc, "hex", "Bytes");
    out
}

/// The viewers the Apple shells draw (N4 slice 5); any other offered falls back to the engine's
/// rendition with the summary (contract 13.3).
pub const NATIVE: &[&str] = &["picture", "svg", "array.pictures", "array.grid", "audio", "table.grid", "text.plain", "scalar", "yaml", "tree", "items", "hex"];

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

fn cell_text(c: &Cell) -> String {
    match c {
        Cell::Null => String::new(),
        Cell::Bool(b) => b.to_string(),
        Cell::Int(i) => i.to_string(),
        Cell::Num(f) => number(*f, false),
        Cell::Text(t) | Cell::Json(t) => t.to_string(),
    }
}

/// What a pop-out's viewer needs beyond `popout.get`'s value: the viewers offered, the list it is
/// part of, the value's shape, its statistics, its pictures and waveform, and the viewer's state.
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
            .map(|c| json!({"name": c.name, "min": c.min.map(|x| number(x, false)), "max": c.max.map(|x| number(x, false)), "nan": c.nan}))
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
/// state's plane and page; `cols?` from the first), `hex` (the state's page), `probe` (`row`, `col`:
/// the exact values there), `yaml`.
pub fn data(session: &Session, media: &MediaService, params: &Value) -> Result<Value, String> {
    crate::control::check_fields(params, &["popout", "part", "row", "col", "cols"])?;
    let id = params.get("popout").and_then(Value::as_u64).ok_or("popout must be a pop-out's id")?;
    let o = session.views.popout(id).ok_or_else(|| format!("no pop-out {id}"))?;
    let (_, d) = session.run.entry(WatchRole::Popout(id)).ok_or_else(|| format!("no pop-out {id}"))?;
    let d = d.ok_or("no value yet: run the graph")?;
    let part = params.get("part").and_then(Value::as_str).ok_or("part is table, tree, grid, hex, probe or yaml")?;
    let mut out = match part {
        "table" => table(d, media, &o.state),
        "tree" => tree(d, media, &o.state),
        "yaml" => yaml(d, media),
        "grid" => grid(d, media, &o.state, whole(params, "cols")?.unwrap_or(0)),
        "hex" => hex(d, media, o.state.hex_page),
        "probe" => {
            let row = whole(params, "row")?.ok_or("probe needs row and col")?;
            let col = whole(params, "col")?.ok_or("probe needs row and col")?;
            probe(d, media, row, col)
        }
        other => return Err(format!("part {:?}: table, tree, grid, hex, probe or yaml", other.chars().take(32).collect::<String>())),
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
    for i in start..end {
        let r = order.as_ref().and_then(|o| o.get(i as usize).copied().map(u64::from)).unwrap_or(i);
        index.push(json!(r));
        cells.push(Value::Array(t.columns.iter().map(|c| json!(c.cells.get(r as usize).map(cell_text).unwrap_or_default())).collect()));
    }
    json!({"columns": columns, "rows": rows, "page": page, "pages": pages, "pageRows": TABLE_PAGE, "start": start,
           "sort": st.sort, "desc": st.desc, "index": index, "cells": cells})
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

fn grid(d: &Display, media: &MediaService, st: &ViewerState, first_col: u64) -> Value {
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
    let c0 = first_col.min(cols.saturating_sub(1));
    let c1 = (c0 + GRID_COLUMNS as u64).min(cols);
    let int = dtype.integer();
    let mut cells = Vec::new();
    for r in start..end {
        let row: Vec<Value> = (c0..c1)
            .map(|c| {
                let i = (r * cols + c) * planes + plane;
                json!(usize::try_from(i).ok().and_then(|i| dtype.read(&b.bytes, i)).map(|x| number(x, int)).unwrap_or_default())
            })
            .collect();
        cells.push(Value::Array(row));
    }
    let plane_name = names.get(plane as usize).cloned();
    json!({"shape": shape, "dtype": b.dtype, "rows": rows, "cols": cols, "planes": planes, "plane": plane, "planeName": plane_name,
           "page": page, "pages": pages, "pageRows": TABLE_PAGE, "start": start, "firstCol": c0, "cells": cells})
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
        assert_eq!(until(|| grid(&a, &m, &ViewerState::default(), 0))["status"], "failed");
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
        let g = until(|| grid(&d, &m, &st, 0));
        assert_eq!(g["cells"][1][2], "13");
        st.plane = 9;
        assert_eq!(until(|| grid(&d, &m, &st, 2))["plane"], 1);
    }
}
