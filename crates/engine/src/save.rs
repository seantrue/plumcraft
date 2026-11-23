//! Save Result (`output.save`, spec 15; the maintainer, 2026-11-22: "Make the save an basic
//! feature"): a node's value written to a file in a format the engine declares.
//!
//! **Formats.** A [`Format`] is a catalogue `save_formats` entry (`id`, `label`, `extensions`,
//! `options`, `media`) and the code that turns a value into its bytes. An engine gets the
//! [`defaults`] for every core type its nodes output with no code at all: TEXT as `.txt`, TABLE
//! as `.csv` (or `.json`), JSON as `.json`, IMAGE as `.png` or `.jpg`, ARRAY as `.png` (drawn as
//! the viewer's `auto` view, or its pixels when it is 8-bit) or `.npy` (its bytes, the interface's
//! array bytes), SVG as `.svg`, AUDIO as `.wav` or `.npy`. It adds or replaces formats with
//! [`crate::Engine::with_save_format`], takes one away with
//! [`crate::Engine::without_save_format`], and gives a node its own list, in order, with
//! `NodeDef::save_formats` (spec 7.3).
//!
//! **A save** ([`save`]) runs the graph for the node as a run does (a memo hit after a run),
//! picks the format by `format` (else the path's extension, else the node's first), checks the
//! options against the format's (`config` otherwise) and fills in their defaults, encodes the
//! value, and writes it: to `path` when the request names one (the engine can write where the
//! person chose, `data.path`), else returns each file as a blob (`files`), which the editor
//! writes (spec 13.2 step 8). A list saves as numbered files (`<stem>-00.<ext>`, …), or one item
//! with `index`; `sheet` puts every item of a list on one picture ([`contact_sheet`]) for a format
//! that makes sheets (PNG and JPEG), and is refused with `config` by any other. A failure is data:
//! `ok: false` and the error, the node's own when computing it failed.
//!
//! **No recipe** (the maintainer, 2026-11-22: "No embedding of recipes. It was ill conceived.";
//! recipes stay with engines that track lineage). A file the defaults write carries, where its
//! format has room, only the workflow's name and its graph's hash (`SaveRequest.graph_info`):
//! PNG as two `iTXt` chunks, WAV as a `LIST` `INFO` `ICMT`, SVG as a `<metadata>` element; JPEG,
//! text, CSV, JSON and `.npy` carry nothing, so their data stays as it is. Nothing reads it back.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use plumcraft_proto::catalogue::{Media, NodeDef, Port, SaveFormat, core_types, scalars};
use plumcraft_proto::data::{BlobMeta, Form};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::StatusRecord;
use plumcraft_proto::run::{GraphInfo, GraphRef, RunRequest, SaveReply, SaveRequest, SavedFile};
use serde_json::{Map, Value, json};

use crate::data::Plane;
use crate::exec::Memo;
use crate::{Array, Audio, Blob, BlobSource, CancelToken, Data, Emit, Engine};

/// The options a save was given, each filled in from its declaration's default.
pub type Options = Map<String, Value>;

/// Turns a value into a file's bytes. A value it can't write is a `config` error that says what
/// would.
pub type Encode = Arc<dyn Fn(&Data, &SaveCx<'_>) -> Result<Vec<u8>, NodeError> + Send + Sync>;

/// What an encoder is told besides the value.
pub struct SaveCx<'a> {
    /// The format's options, checked and with their defaults.
    pub options: &'a Options,
    /// The list item's label (what varied), or `""`.
    pub label: &'a str,
    /// The workflow's name and graph hash (`SaveRequest.graph_info`), for a format with room for
    /// them ([`Format::stamping`]): informational, never read back.
    pub info: Option<&'a GraphInfo>,
}

/// A save format: what the catalogue lists, and how it writes.
#[derive(Clone)]
pub struct Format {
    pub def: SaveFormat,
    /// The media type of the files it writes (`image/png`), for a file returned as a blob.
    pub media_type: String,
    /// It makes contact sheets: one picture of every item of a list (`sheet`, [`contact_sheet`]),
    /// its encoder given the sheet as an (H, W, 3) `|u1` array.
    pub sheets: bool,
    /// It writes [`SaveCx::info`] into its files (`SaveReply.stamped`).
    pub stamps: bool,
    /// It writes pictures: a person's save (`ask`) of an ARRAY not shaped like one is asked
    /// about instead ([`picture_shape`], `SaveReply.question`).
    pub questions: bool,
    pub encode: Encode,
}

impl std::fmt::Debug for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Format")
            .field("def", &self.def)
            .field("media_type", &self.media_type)
            .field("sheets", &self.sheets)
            .field("stamps", &self.stamps)
            .field("questions", &self.questions)
            .finish_non_exhaustive()
    }
}

impl Format {
    /// A format `id` named `label`, writing files with `extensions` (without dots; the first is
    /// what a bare name gets) of media type `media_type`, for values shown as `media` (empty: any).
    pub fn new(
        id: &str,
        label: &str,
        extensions: &[&str],
        media: &[Media],
        media_type: &str,
        encode: impl Fn(&Data, &SaveCx<'_>) -> Result<Vec<u8>, NodeError> + Send + Sync + 'static,
    ) -> Format {
        Format {
            def: SaveFormat {
                id: id.into(),
                label: label.into(),
                extensions: extensions.iter().map(|e| e.trim_start_matches('.').to_owned()).collect(),
                options: Vec::new(),
                media: media.to_vec(),
            },
            media_type: media_type.into(),
            sheets: false,
            stamps: false,
            questions: false,
            encode: Arc::new(encode),
        }
    }

    /// With option `port` (a scalar port: its `type`, `default`, `choices`, `min` and `max` are
    /// what the editor asks with and checks).
    pub fn with_option(mut self, port: Port) -> Format {
        self.def.options.retain(|p| p.name != port.name);
        self.def.options.push(port);
        self
    }

    /// It makes contact sheets ([`Format::sheets`]): its encoder takes an (H, W, 3) `|u1` array.
    pub fn making_sheets(mut self) -> Format {
        self.sheets = true;
        self
    }

    /// Its encoder writes [`SaveCx::info`] into its files ([`Format::stamps`]).
    pub fn stamping(mut self) -> Format {
        self.stamps = true;
        self
    }

    /// It writes pictures, so a person is asked before an array not shaped like one is saved
    /// with it ([`Format::questions`]).
    pub fn questioning(mut self) -> Format {
        self.questions = true;
        self
    }
}

/// A `config` error.
fn config(message: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::CONFIG, message)
}

/// An option port: `name` of scalar type `ty`, its `label` and `default`.
pub fn option(name: &str, label: &str, ty: &str, default: Value) -> Port {
    Port { name: name.into(), ty: ty.into(), label: Some(label.into()), default, ..Default::default() }
}

/// The SDK's own formats, in the order a node's list takes them when it gives none.
pub mod defaults {
    use super::*;

    /// TEXT as UTF-8 (`.txt`).
    pub fn txt() -> Format {
        Format::new("txt", "Text", &["txt"], &[Media::Text], "text/plain; charset=utf-8", |v, _| match v {
            Data::Json(Value::String(s)) => Ok(s.as_bytes().to_vec()),
            Data::Json(Value::Number(n)) => Ok(n.to_string().into_bytes()),
            Data::Json(Value::Bool(b)) => Ok(b.to_string().into_bytes()),
            _ => Err(config("Text saves text")),
        })
    }

    /// TABLE (columnar JSON) as comma-separated values (`.csv`): a header of the column names,
    /// then a line per row; a field with a comma, a quote or a line break is quoted, a quote
    /// doubled; numbers as JSON writes them, `null` empty, anything nested as compact JSON.
    pub fn csv() -> Format {
        Format::new("csv", "CSV", &["csv"], &[Media::Table], "text/csv; charset=utf-8", |v, _| match v {
            Data::Json(t) => csv_bytes(t),
            _ => Err(config("CSV saves a table")),
        })
    }

    /// A JSON value, or a table's columnar JSON, as JSON indented two spaces, with a final
    /// line break (`.json`).
    pub fn json() -> Format {
        Format::new("json", "JSON", &["json"], &[Media::Tree, Media::Table], "application/json", |v, _| {
            match v {
                Data::Json(j) => serde_json::to_vec_pretty(j).map(|mut b| {
                    b.push(b'\n');
                    b
                }),
                _ => return Err(config("JSON saves a JSON value")),
            }
            .map_err(|e| NodeError::new(NodeError::OP, e.to_string()))
        })
    }

    /// IMAGE, or an ARRAY that is a picture, as PNG (`.png`): a PNG value's own bytes, any other
    /// picture re-encoded; an array's pixels when it is 8-bit with 1 to 4 channels, else the
    /// picture the viewer's `auto` view draws.
    pub fn png() -> Format {
        Format::new("png", "PNG", &["png"], &[Media::Image, Media::Ndarray], "image/png", |v, cx| {
            let bytes = match v {
                Data::Blob(b) if b.meta.media == "image/png" => b.read(plumcraft_store::MAX_READ)?,
                other => encode_picture(other, image::ImageFormat::Png, 0)?,
            };
            Ok(match cx.info {
                Some(i) => png_with_info(&bytes, i),
                None => bytes,
            })
        })
        .making_sheets()
        .stamping()
        .questioning()
    }

    /// IMAGE, or an ARRAY that is a picture, as JPEG (`.jpg`), at `quality` 1 to 100 (90).
    pub fn jpeg() -> Format {
        let quality = Port { min: Some(1.0), max: Some(100.0), ..option("quality", "Quality", scalars::INT, json!(90)) };
        Format::new("jpeg", "JPEG", &["jpg", "jpeg"], &[Media::Image, Media::Ndarray], "image/jpeg", |v, cx| {
            let q = cx.options.get("quality").and_then(Value::as_u64).unwrap_or(90).clamp(1, 100);
            encode_picture(v, image::ImageFormat::Jpeg, u8::try_from(q).unwrap_or(90))
        })
        .with_option(quality)
        .making_sheets()
        .questioning()
    }

    /// AUDIO as a RIFF WAVE file (`.wav`): 32-bit float samples, or 16-bit PCM (`bits`).
    pub fn wav() -> Format {
        let bits = Port { choices: Some(vec![json!("float32"), json!("int16")]), ..option("bits", "Sample format", scalars::ENUM, json!("float32")) };
        Format::new("wav", "WAV", &["wav"], &[Media::Audio], "audio/wav", |v, cx| match v {
            Data::Audio(a) => {
                let bytes = wav_bytes(a, cx.options.get("bits").and_then(Value::as_str) == Some("int16"));
                Ok(match cx.info {
                    Some(i) => wav_with_info(&bytes, i),
                    None => bytes,
                })
            }
            _ => Err(config("WAV saves audio")),
        })
        .with_option(bits)
        .stamping()
    }

    /// SVG as itself (`.svg`).
    pub fn svg() -> Format {
        Format::new("svg", "SVG", &["svg"], &[Media::Vector], core_types::SVG_MEDIA, |v, cx| match v {
            Data::Blob(b) if b.is_svg() => {
                let bytes = b.read(plumcraft_store::MAX_READ)?;
                Ok(match cx.info {
                    Some(i) => svg_with_info(&bytes, i),
                    None => bytes,
                })
            }
            _ => Err(config("SVG saves an SVG")),
        })
        .stamping()
    }

    /// An ARRAY (or AUDIO's samples, (frames, channels) `<f4`) as a NumPy `.npy` file: version
    /// 1.0, C order, the array's own dtype and bytes.
    pub fn npy() -> Format {
        Format::new("npy", "NumPy array", &["npy"], &[Media::Ndarray, Media::Audio], crate::data::NPY_MEDIA, |v, _| match v {
            Data::Array(a) => npy_bytes(&a.dtype, &a.shape, &a.bytes),
            Data::Audio(a) => npy_bytes("<f4", &[a.frames(), u64::from(a.channels)], &a.bytes()),
            _ => Err(config("NumPy saves an array")),
        })
    }

    /// The defaults for values of core type `ty`, the first the default.
    pub fn for_type(ty: &str) -> Vec<Format> {
        match ty {
            core_types::TEXT => vec![txt()],
            core_types::TABLE => vec![csv(), json()],
            core_types::JSON => vec![json()],
            core_types::IMAGE => vec![png(), jpeg()],
            core_types::ARRAY => vec![png(), jpeg(), npy()],
            core_types::SVG => vec![svg()],
            core_types::AUDIO => vec![wav(), npy()],
            _ => Vec::new(),
        }
    }

    /// Every default, in catalogue order (a node with no list of its own offers those whose
    /// media take its output's, in this order).
    pub const ORDER: &[&str] = &["txt", "csv", "json", "png", "jpeg", "wav", "svg", "npy"];
}

/// A table's CSV ([`defaults::csv`]).
pub fn csv_bytes(t: &Value) -> Result<Vec<u8>, NodeError> {
    let columns = t.get("columns").and_then(Value::as_array).ok_or_else(|| config("CSV saves a table: {\"columns\": [{name, values}]}"))?;
    let field = |v: &Value| -> String {
        let s = match v {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
    };
    let names: Vec<String> = columns.iter().map(|c| field(c.get("name").unwrap_or(&Value::Null))).collect();
    let values: Vec<&[Value]> = columns.iter().map(|c| c.get("values").and_then(Value::as_array).map_or(&[][..], Vec::as_slice)).collect();
    let rows = values.iter().map(|v| v.len()).max().unwrap_or(0);
    let mut out = names.join(",");
    out.push('\n');
    for r in 0..rows {
        let line: Vec<String> = values.iter().map(|v| v.get(r).map_or_else(String::new, field)).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    Ok(out.into_bytes())
}

/// A WAV file of `a` ([`defaults::wav`]): 32-bit float (format 3) or 16-bit PCM, each sample
/// clamped to -1..1 and scaled by 32767, rounded half away from zero.
pub fn wav_bytes(a: &Audio, int16: bool) -> Vec<u8> {
    let (format, bits): (u16, u16) = if int16 { (1, 16) } else { (3, 32) };
    let channels = a.channels.max(1);
    let block = channels * (bits / 8);
    let data: Vec<u8> = if int16 {
        a.samples
            .iter()
            .flat_map(|s| {
                let x = if s.is_nan() { 0.0 } else { f64::from(s.clamp(-1.0, 1.0)) * 32767.0 };
                // (a float to int cast saturates; the value is already within i16's range)
                ((x.signum() * (x.abs() + 0.5).floor()).clamp(-32767.0, 32767.0) as i16).to_le_bytes()
            })
            .collect()
    } else {
        a.bytes()
    };
    let size = u32::try_from(data.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&size.saturating_add(36).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&format.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&a.sample_rate.to_le_bytes());
    out.extend_from_slice(&a.sample_rate.saturating_mul(u32::from(block)).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// A `.npy` file (version 1.0) of an array of `dtype` and `shape` ([`defaults::npy`]).
pub fn npy_bytes(dtype: &str, shape: &[u64], bytes: &[u8]) -> Result<Vec<u8>, NodeError> {
    let dims: Vec<String> = shape.iter().map(u64::to_string).collect();
    let tuple = match dims.len() {
        1 => format!("({},)", dims.join("")),
        _ => format!("({})", dims.join(", ")),
    };
    let mut header = format!("{{'descr': '{dtype}', 'fortran_order': False, 'shape': {tuple}, }}");
    // magic (6) + version (2) + length (2) + header, padded with spaces to a multiple of 64, a line break last
    let pad = (64 - (10 + header.len() + 1) % 64) % 64;
    header.push_str(&" ".repeat(pad));
    header.push('\n');
    let len = u16::try_from(header.len()).map_err(|_| config("the array has too many dimensions for .npy"))?;
    let mut out = Vec::with_capacity(10 + header.len() + bytes.len());
    out.extend_from_slice(b"\x93NUMPY\x01\x00");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(bytes);
    Ok(out)
}

/// The longest side a picture may have against its shorter (decision 3A: 16:1).
pub const PICTURE_ASPECT: u64 = 16;
/// The shorter side a picture has at least.
pub const PICTURE_MIN_SIDE: u64 = 64;
/// The most channels a picture has.
pub const PICTURE_MAX_CHANNELS: u64 = 4;

/// Why an array isn't shaped like a picture, in a sentence, or `None` when it is (decisions 2A
/// and 3A): a table; not (H, W) or (H, W, C); more than 4 channels; a side under 64; or its
/// longer side more than 16 times its shorter. What a person is asked about before it is saved
/// as PNG or JPEG.
pub fn picture_shape(a: &Array) -> Option<String> {
    let dims = a.shape.iter().map(u64::to_string).collect::<Vec<_>>().join(" by ");
    if a.table {
        return Some(format!("this {dims} array is a table, not a picture"));
    }
    let (h, w, c) = match a.shape.as_slice() {
        [h, w] => (*h, *w, 1),
        [h, w, c] => (*h, *w, *c),
        _ => return Some(format!("this {dims} array has {} dimensions; a picture has 2 or 3", a.shape.len())),
    };
    let (lo, hi) = (h.min(w), h.max(w));
    if c > PICTURE_MAX_CHANNELS {
        Some(format!("this {dims} array has {c} channels; a picture has at most {PICTURE_MAX_CHANNELS}"))
    } else if lo < PICTURE_MIN_SIDE {
        Some(format!("this {dims} array is {lo} pixels on its shorter side; a picture has at least {PICTURE_MIN_SIDE}"))
    } else if hi > lo.saturating_mul(PICTURE_ASPECT) {
        Some(format!("this {dims} array is more than {PICTURE_ASPECT} times as long as it is wide"))
    } else {
        None
    }
}

/// The question a person's save of `v` raises: the first array of it not shaped like a picture.
fn shape_question(v: &Data) -> Option<plumcraft_proto::run::SaveQuestion> {
    match v {
        Data::Array(a) => picture_shape(a).map(|text| plumcraft_proto::run::SaveQuestion { text, shape: a.shape.clone(), offer: "npy".into() }),
        Data::List(l) => l.items.iter().find_map(shape_question),
        _ => None,
    }
}

/// CRC-32 (ISO 3309, as PNG's chunks carry it) of `bytes`.
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for b in bytes {
        c ^= u32::from(*b);
        for _ in 0..8 {
            c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// A PNG `iTXt` chunk: `keyword`, uncompressed UTF-8 `text`, no language.
fn itxt(keyword: &str, text: &str) -> Vec<u8> {
    let mut body = b"iTXt".to_vec();
    body.extend_from_slice(keyword.as_bytes());
    body.extend_from_slice(&[0, 0, 0, 0, 0]);
    body.extend_from_slice(text.as_bytes());
    let mut out = u32::try_from(body.len() - 4).unwrap_or(0).to_be_bytes().to_vec();
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
    out
}

/// The text a file says of its workflow: its name and its graph's hash, each cut to
/// [`GraphInfo::MAX_TEXT`] characters with control characters as spaces.
fn info_parts(i: &GraphInfo) -> (String, String) {
    let clean = |s: &str| s.chars().take(GraphInfo::MAX_TEXT).map(|c| if c.is_control() { ' ' } else { c }).collect::<String>();
    (clean(&i.name), clean(&i.hash))
}

/// A PNG with the workflow's name and graph hash as two `iTXt` chunks (`PlumCraft graph`,
/// `PlumCraft graph hash`) just before `IEND`. Bytes that aren't a PNG ending in `IEND` are
/// returned as they came.
pub fn png_with_info(png: &[u8], i: &GraphInfo) -> Vec<u8> {
    const IEND: usize = 12;
    if png.len() < 8 + IEND || !png.starts_with(b"\x89PNG\r\n\x1a\n") || png.get(png.len() - 8..png.len() - 4) != Some(b"IEND") {
        return png.to_vec();
    }
    let (name, hash) = info_parts(i);
    let at = png.len() - IEND;
    let mut out = png[..at].to_vec();
    out.extend(itxt("PlumCraft graph", &name));
    out.extend(itxt("PlumCraft graph hash", &hash));
    out.extend_from_slice(&png[at..]);
    out
}

/// A WAV with the workflow's name and graph hash as a `LIST` `INFO` chunk at its end (`ICMT`,
/// [`GraphInfo::line`], UTF-8, NUL-terminated and padded to an even length), the RIFF size
/// made to count it. Bytes that aren't a RIFF WAVE are returned as they came.
pub fn wav_with_info(wav: &[u8], i: &GraphInfo) -> Vec<u8> {
    if wav.len() < 12 || !wav.starts_with(b"RIFF") || wav.get(8..12) != Some(b"WAVE") {
        return wav.to_vec();
    }
    let (name, hash) = info_parts(i);
    let mut text = GraphInfo { name, hash }.line().into_bytes();
    text.push(0);
    if text.len() % 2 == 1 {
        text.push(0);
    }
    let mut list = b"LIST".to_vec();
    list.extend_from_slice(&u32::try_from(4 + 8 + text.len()).unwrap_or(0).to_le_bytes());
    list.extend_from_slice(b"INFOICMT");
    list.extend_from_slice(&u32::try_from(text.len()).unwrap_or(0).to_le_bytes());
    list.extend_from_slice(&text);
    let mut out = wav.to_vec();
    if out.len() % 2 == 1 {
        out.push(0);
    }
    out.extend_from_slice(&list);
    let size = u32::try_from(out.len() - 8).unwrap_or(u32::MAX);
    out[4..8].copy_from_slice(&size.to_le_bytes());
    out
}

/// An SVG with the workflow's name and graph hash as a `<metadata>` element just inside its root
/// (`<metadata>PlumCraft graph "name", sha256:…</metadata>`, XML-escaped). Bytes without a root
/// `<svg …>` start tag it can find are returned as they came.
pub fn svg_with_info(svg: &[u8], i: &GraphInfo) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(svg) else { return svg.to_vec() };
    let Some(start) = text.match_indices("<svg").map(|(k, _)| k).find(|k| text[k + 4..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/'))
    else {
        return svg.to_vec();
    };
    let mut quote: Option<char> = None;
    let mut end = None;
    for (k, c) in text[start..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => {
                end = Some(start + k);
                break;
            }
            _ => {}
        }
    }
    let Some(end) = end else { return svg.to_vec() };
    let (name, hash) = info_parts(i);
    let escaped = GraphInfo { name, hash }.line().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let meta = format!("<metadata>{escaped}</metadata>");
    let out = if text[..end].ends_with('/') {
        format!("{}>{meta}</svg>{}", &text[..end - 1], &text[end + 1..])
    } else {
        format!("{}{meta}{}", &text[..=end], &text[end + 1..])
    };
    out.into_bytes()
}

/// The largest side of a contact sheet's cell, in pixels.
pub const SHEET_CELL: usize = 256;
/// The gap around and between a sheet's cells, in pixels.
pub const SHEET_GAP: usize = 8;
/// A sheet's background, and a cell's where its picture doesn't fill it.
pub const SHEET_GROUND: [u8; 3] = [224, 224, 224];

/// An item as RGB, 8 bits a channel, `(w, h, pixels)`: an IMAGE decoded, or a picture ARRAY as
/// [`defaults::png`] writes it; alpha over white, `(c·a + 255·(255 − a) + 127) / 255`.
fn picture_rgb(v: &Data) -> Result<(usize, usize, Vec<u8>), NodeError> {
    let img = match v {
        Data::Blob(b) if b.meta.media.starts_with("image/") && !b.is_svg() => {
            let bytes = b.read(plumcraft_store::MAX_READ)?;
            image::load_from_memory(&bytes).map_err(|e| config(format!("not a picture this engine reads: {e}")))?
        }
        Data::Array(a) => array_image(a)?,
        _ => return Err(config("a contact sheet holds pictures: IMAGE values, or ARRAY values of (H, W) or (H, W, C)")),
    };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let px = rgba
        .as_raw()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| {
            let a = u32::from(a);
            let over = |c: u8| ((u32::from(c) * a + 255 * (255 - a) + 127) / 255) as u8;
            [over(r), over(g), over(b)]
        })
        .collect();
    Ok((w, h, px))
}

/// `px` (`w` × `h` RGB) scaled to `tw` × `th` by an exact box average: each pixel the mean of the
/// source pixels under it, `(sum + n / 2) / n` in integers, so every SDK gets the same pixels.
fn box_scale(w: usize, h: usize, px: &[u8], tw: usize, th: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(tw * th * 3);
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).max(ty * h / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).max(tx * w / tw + 1));
            let n = ((y1 - y0) * (x1 - x0)) as u64;
            for c in 0..3 {
                let mut sum = 0u64;
                for y in y0..y1 {
                    for x in x0..x1 {
                        sum += u64::from(px.get((y * w + x) * 3 + c).copied().unwrap_or(0));
                    }
                }
                out.push(((sum + n / 2) / n) as u8);
            }
        }
    }
    out
}

/// The smallest side of a contact sheet's cell, so a label has room (about 10 characters).
pub const SHEET_MIN_CELL: usize = 64;
/// The band under each cell a label is drawn in, when any item has one: 1 pixel, then the font's
/// line (Aileron at 11 pixels: ascent 11, descent 3, 14 rows).
pub const SHEET_LABEL_BAND: usize = 15;
/// The labels' colour.
pub const SHEET_INK: [u8; 3] = [32, 32, 32];

/// One glyph of the label font: its advance, where its coverage starts from the pen, and its
/// 8-bit coverage, `width` columns by the line's height, row by row.
struct Glyph {
    advance: usize,
    x: i64,
    width: usize,
    coverage: Vec<u8>,
}

/// The label font (decision 4B; B125): Aileron Regular (Sora Sagano, CC0 1.0) at 11 pixels, as
/// Pillow embeds it, rasterised once into `sheet_font.json` (the same file in the Python SDK), so
/// labels are the same pixels in both; no system font.
struct SheetFont {
    height: usize,
    glyphs: std::collections::HashMap<u32, Glyph>,
}

fn sheet_font() -> &'static SheetFont {
    static FONT: std::sync::OnceLock<SheetFont> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        let v: Value = serde_json::from_str(include_str!("sheet_font.json")).unwrap_or(Value::Null);
        let height = v.get("height").and_then(Value::as_u64).unwrap_or(0) as usize;
        let hex = |s: &str| -> Vec<u8> { (0..s.len() / 2).filter_map(|i| s.get(2 * i..2 * i + 2).and_then(|b| u8::from_str_radix(b, 16).ok())).collect() };
        let glyphs = v
            .get("glyphs")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(code, g)| {
                let n = |k: &str| g.get(k).and_then(Value::as_i64).unwrap_or(0);
                let glyph = Glyph {
                    advance: usize::try_from(n("advance")).unwrap_or(0),
                    x: n("x"),
                    width: usize::try_from(n("width")).unwrap_or(0),
                    coverage: hex(g.get("coverage").and_then(Value::as_str).unwrap_or_default()),
                };
                Some((code.parse::<u32>().ok()?, glyph))
            })
            .collect();
        SheetFont { height, glyphs }
    })
}

/// A character's glyph, or `?`'s for one the font hasn't.
fn glyph_of(f: &SheetFont, c: char) -> Option<&Glyph> {
    f.glyphs.get(&u32::from(c)).or_else(|| f.glyphs.get(&u32::from('?')))
}

/// How wide `text` is drawn: its glyphs' advances.
fn label_width(text: &str) -> usize {
    let f = sheet_font();
    text.chars().filter_map(|c| glyph_of(f, c)).map(|g| g.advance).sum()
}

/// Draw `text` into `sheet` (`sw` pixels wide, RGB) with its line's top at `y`, the pen starting
/// at `x`: each glyph's coverage `a` (0 to 255) blends [`SHEET_INK`] over what is there,
/// `(ink · a + old · (255 − a) + 127) / 255` per channel in integers, glyph after glyph, then the
/// pen moves on by the glyph's advance. No kerning.
fn draw_label(sheet: &mut [u8], sw: usize, x: usize, y: usize, text: &str) {
    let f = sheet_font();
    let mut pen = x as i64;
    for c in text.chars() {
        let Some(g) = glyph_of(f, c) else { continue };
        for row in 0..f.height {
            for col in 0..g.width {
                let a = u32::from(g.coverage.get(row * g.width + col).copied().unwrap_or(0));
                let px = pen + g.x + col as i64;
                if a == 0 || px < 0 || px as usize >= sw {
                    continue;
                }
                let at = ((y + row) * sw + px as usize) * 3;
                if let Some(dst) = sheet.get_mut(at..at + 3) {
                    for (d, ink) in dst.iter_mut().zip(SHEET_INK) {
                        *d = ((u32::from(ink) * a + u32::from(*d) * (255 - a) + 127) / 255) as u8;
                    }
                }
            }
        }
        pen += g.advance as i64;
    }
}

/// Every item of a list on one picture (Contact Sheet…; the maintainer, 2026-11-22, decisions 2A
/// and 4B), an (H, W, 3) `|u1` array: `cols` the least whole number whose square holds the items,
/// `rows` enough for them; each cell a square of the largest side of any item, held to 64 to 256
/// pixels, with an 8-pixel gap around and between, on light grey; each item scaled down (never
/// up) to fit its cell by [`box_scale`], keeping its shape (a side `(side · cell + m / 2) / m`, `m`
/// its larger side, at least 1), and centred. When any item has a label, each cell has a
/// 15-pixel band under it with its label in the shared label font (Aileron, [`draw_label`]),
/// centred, cut to the characters the cell's width holds. At most [`crate::exec::MAX_SHEET`] items.
pub fn contact_sheet(items: &[Data], labels: &[String]) -> Result<Array, NodeError> {
    if items.is_empty() {
        return Err(config("a contact sheet needs at least one item"));
    }
    if items.len() > crate::exec::MAX_SHEET {
        return Err(config(format!("a contact sheet holds at most {} items, not {}", crate::exec::MAX_SHEET, items.len())));
    }
    let pictures = items
        .iter()
        .enumerate()
        .map(|(k, v)| picture_rgb(v).map_err(|e| NodeError::new(&e.kind, format!("item {k}: {}", e.message))))
        .collect::<Result<Vec<_>, _>>()?;
    let cell = pictures.iter().map(|(w, h, _)| (*w).max(*h)).max().unwrap_or(1).clamp(SHEET_MIN_CELL, SHEET_CELL);
    let band = if labels.iter().any(|l| !l.is_empty()) { SHEET_LABEL_BAND } else { 0 };
    let n = pictures.len();
    let cols = (1..=n).find(|c| c * c >= n).unwrap_or(n);
    let rows = n.div_ceil(cols);
    let (sw, sh) = (cols * cell + (cols + 1) * SHEET_GAP, rows * (cell + band) + (rows + 1) * SHEET_GAP);
    let mut sheet: Vec<u8> = SHEET_GROUND.iter().copied().cycle().take(sw * sh * 3).collect();
    for (k, (w, h, px)) in pictures.iter().enumerate() {
        let m = (*w).max(*h).max(1);
        let (tw, th) = if m <= cell { (*w, *h) } else { (((w * cell + m / 2) / m).max(1), ((h * cell + m / 2) / m).max(1)) };
        let scaled = if (tw, th) == (*w, *h) { px.clone() } else { box_scale(*w, *h, px, tw, th) };
        let (col, row) = (k % cols, k / cols);
        let (cx, cy) = (SHEET_GAP + col * (cell + SHEET_GAP), SHEET_GAP + row * (cell + band + SHEET_GAP));
        let (x0, y0) = (cx + (cell - tw) / 2, cy + (cell - th) / 2);
        for y in 0..th {
            let from = y * tw * 3;
            let to = ((y0 + y) * sw + x0) * 3;
            if let (Some(src), Some(dst)) = (scaled.get(from..from + tw * 3), sheet.get_mut(to..to + tw * 3)) {
                dst.copy_from_slice(src);
            }
        }
        if let Some(label) = labels.get(k).filter(|l| band > 0 && !l.is_empty()) {
            // as many characters as the cell's width holds
            let mut text = String::new();
            for ch in label.chars() {
                text.push(ch);
                if label_width(&text) > cell {
                    text.pop();
                    break;
                }
            }
            let width = label_width(&text);
            draw_label(&mut sheet, sw, cx + (cell - width) / 2, cy + cell + 1, &text);
        }
    }
    Ok(Array::new("|u1", vec![sh as u64, sw as u64, 3], sheet))
}

/// An IMAGE or a picture ARRAY encoded as `format` (`quality` for JPEG).
fn encode_picture(v: &Data, format: image::ImageFormat, quality: u8) -> Result<Vec<u8>, NodeError> {
    let img = match v {
        Data::Blob(b) if b.meta.media.starts_with("image/") && !b.is_svg() => {
            let bytes = b.read(plumcraft_store::MAX_READ)?;
            image::load_from_memory(&bytes).map_err(|e| config(format!("not a picture this engine reads: {e}")))?
        }
        Data::Array(a) => array_image(a)?,
        _ => return Err(config("saves a picture: an IMAGE, or an ARRAY of (H, W) or (H, W, C)")),
    };
    let img = if format == image::ImageFormat::Jpeg { image::DynamicImage::ImageRgb8(img.to_rgb8()) } else { img };
    let mut out = std::io::Cursor::new(Vec::new());
    let written = if format == image::ImageFormat::Jpeg {
        img.write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality))
    } else {
        img.write_to(&mut out, format)
    };
    written.map_err(|e| NodeError::new(NodeError::OP, format!("encoding: {e}")))?;
    Ok(out.into_inner())
}

/// An array as a picture: its pixels when 8-bit with 1 to 4 channels, else the `auto` view's.
fn array_image(a: &Array) -> Result<image::DynamicImage, NodeError> {
    let not = || config("not a picture: an (H, W) or (H, W, C) array; save it as .npy");
    let l = crate::arrays::layout(a).ok_or_else(not)?;
    let (w, h) = (u32::try_from(l.w).map_err(|_| not())?, u32::try_from(l.h).map_err(|_| not())?);
    if a.dtype == "|u1" {
        let px = a.bytes.to_vec();
        let img = match l.c {
            1 => image::GrayImage::from_raw(w, h, px).map(image::DynamicImage::ImageLuma8),
            2 => image::GrayAlphaImage::from_raw(w, h, px).map(image::DynamicImage::ImageLumaA8),
            3 => image::RgbImage::from_raw(w, h, px).map(image::DynamicImage::ImageRgb8),
            4 => image::RgbaImage::from_raw(w, h, px).map(image::DynamicImage::ImageRgba8),
            _ => None,
        };
        if let Some(img) = img {
            return Ok(img);
        }
    }
    let p = crate::arrays::views(a, "auto", 1.0).into_iter().next().ok_or_else(not)?;
    let px: Vec<u8> = p.rgb.iter().map(|x| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect();
    image::RgbImage::from_raw(w, h, px).map(image::DynamicImage::ImageRgb8).ok_or_else(not)
}

/// `value` against option `port`: its choices, type and range.
fn check_option(port: &Port, value: &Value) -> Result<(), String> {
    let name = &port.name;
    if let Some(choices) = port.choices.as_ref().filter(|c| !c.is_empty())
        && !choices.contains(value)
    {
        return Err(format!("option {name:?} must be one of {}, not {value}", Value::from(choices.clone())));
    }
    match port.ty.as_str() {
        scalars::INT | scalars::FLOAT => {
            let n = value
                .as_f64()
                .filter(|n| port.ty == scalars::FLOAT || n.fract() == 0.0)
                .ok_or_else(|| format!("option {name:?} must be a {} number", port.ty))?;
            if port.min.is_some_and(|m| n < m) || port.max.is_some_and(|m| n > m) {
                return Err(format!("option {name:?} must be from {} to {}, not {n}", port.min.unwrap_or(f64::MIN), port.max.unwrap_or(f64::MAX)));
            }
        }
        scalars::BOOL if !value.is_boolean() => return Err(format!("option {name:?} must be true or false")),
        scalars::STRING | scalars::ENUM if !value.is_string() => return Err(format!("option {name:?} must be text")),
        _ => {}
    }
    Ok(())
}

/// `given` checked against `format`'s options, with each one's default where it isn't given.
pub fn options(format: &SaveFormat, given: &Map<String, Value>) -> Result<Options, NodeError> {
    let mut out = Options::new();
    for (k, v) in given {
        let port = format
            .options
            .iter()
            .find(|p| p.name == *k)
            .ok_or_else(|| config(format!("{} has no option {:?}", format.label, k.chars().take(64).collect::<String>())))?;
        check_option(port, v).map_err(config)?;
        out.insert(k.clone(), v.clone());
    }
    for p in &format.options {
        out.entry(p.name.clone()).or_insert_with(|| p.default.clone());
    }
    Ok(out)
}

/// Saves so far: each runs under a run id of its own.
static SAVES: AtomicU64 = AtomicU64::new(0);

/// Status records of a save's run go nowhere: a save is not a run the editor follows.
struct Quiet;

impl Emit for Quiet {
    fn status(&mut self, _rec: StatusRecord) {}
}

/// The format a request names: by id, else by the path's extension, else the node's first.
fn chosen<'a>(formats: &[&'a Format], req: &SaveRequest) -> Result<&'a Format, NodeError> {
    let ids = || formats.iter().map(|f| f.def.id.as_str()).collect::<Vec<_>>().join(", ");
    if !req.format.is_empty() {
        return formats
            .iter()
            .find(|f| f.def.id == req.format)
            .copied()
            .ok_or_else(|| config(format!("{}.{} doesn't save as {:?}: {}", req.node, req.port, req.format.chars().take(64).collect::<String>(), ids())));
    }
    let ext = req.path.as_deref().and_then(|p| Path::new(p).extension()).and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    match ext {
        Some(e) => formats
            .iter()
            .find(|f| f.def.extensions.iter().any(|x| x.eq_ignore_ascii_case(&e)))
            .copied()
            .ok_or_else(|| config(format!("{}.{} doesn't save .{e} files: {}", req.node, req.port, ids()))),
        None => formats.first().copied().ok_or_else(|| config(format!("{}.{} has no save format", req.node, req.port))),
    }
}

/// The file names for `n` items of a save to `base` (`stem.ext`): the name itself for one, and
/// `stem-00.ext`, `stem-01.ext`, … (two digits or more) for a list.
pub fn numbered(base: &str, n: usize, list: bool) -> Vec<String> {
    if !list {
        return vec![base.to_owned()];
    }
    let (stem, ext) = match base.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (base, String::new()),
    };
    let width = n.saturating_sub(1).to_string().len().max(2);
    (0..n).map(|i| format!("{stem}-{i:0width$}{ext}")).collect()
}

/// Write `bytes` to `p` whole: a file beside it, then a rename, so a reader never sees half.
fn write_whole(p: &Path, bytes: &[u8]) -> Result<(), NodeError> {
    let fail = |e: std::io::Error| NodeError::new(NodeError::OP, format!("{}: {e}", p.display()));
    let dir = p.parent().filter(|d| !d.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let name = p.file_name().ok_or_else(|| config(format!("{}: no file name", p.display())))?.to_string_lossy().into_owned();
    if p.is_dir() {
        return Err(config(format!("{} is a folder", p.display())));
    }
    std::fs::create_dir_all(&dir).map_err(fail)?;
    let tmp = dir.join(format!(".{name}.{}.{}.tmp", std::process::id(), SAVES.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, p)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        fail(e)
    })
}

/// `output.save` for an engine on this crate's executor (spec 15): see the module docs.
pub fn save(engine: &Engine, mut req: SaveRequest, memo: &mut Memo, plane: &Plane<'_>, cancel: &CancelToken, publisher: &str) -> SaveReply {
    match save_inner(engine, &mut req, memo, plane, cancel, publisher) {
        Ok(r) => r,
        Err(e) => SaveReply::failed(e),
    }
}

fn save_inner(
    engine: &Engine,
    req: &mut SaveRequest,
    memo: &mut Memo,
    plane: &Plane<'_>,
    cancel: &CancelToken,
    publisher: &str,
) -> Result<SaveReply, NodeError> {
    let GraphRef::Inline { graph } = &req.graph else { return Err(NodeError::new(NodeError::GRAPH, "graphs by handle aren't supported by this engine")) };
    let kind = graph.get("nodes").and_then(|n| n.get(&req.node)).and_then(|n| n.get("kind")).and_then(Value::as_str);
    let def: NodeDef = kind
        .and_then(|k| engine.node(k))
        .map(crate::Node::def)
        .ok_or_else(|| config(format!("{:?} isn't a node of this engine's in the graph", req.node.chars().take(64).collect::<String>())))?;
    if req.port.is_empty() {
        req.port = def.outputs.first().map(|o| o.name.clone()).unwrap_or_default();
    }
    let catalogue = engine.schema();
    let all = engine.save_formats();
    let formats: Vec<&Format> = catalogue.save_formats_for(&def, &req.port).iter().filter_map(|f| all.iter().find(|x| x.def.id == f.id)).collect();
    if def.output(&req.port).is_none() {
        return Err(config(format!("{} has no output {:?}", req.node, req.port.chars().take(64).collect::<String>())));
    }
    if formats.is_empty() {
        return Err(config(format!("{}.{} has no save format", req.node, req.port)));
    }
    let format = chosen(&formats, req)?;
    let opts = options(&format.def, &req.options)?;
    // the value, as a run makes it (a memo hit after one)
    let n = SAVES.fetch_add(1, Ordering::Relaxed);
    let run = RunRequest {
        run_id: format!("save-{}-{n}", std::process::id()),
        graph: req.graph.clone(),
        targets: vec![req.node.clone()],
        inputs: std::mem::take(&mut req.inputs),
        trusted_roots: req.trusted_roots.clone(),
        priority: 5,
        ..Default::default()
    };
    let (reply, mut values) = crate::exec::run_values(engine, &run, &mut Quiet, cancel, publisher, memo, plane);
    if cancel.is_cancelled() {
        return Err(NodeError::cancelled());
    }
    if let Some(e) = reply.nodes.get(&req.node).and_then(|r| r.error.clone()).or(reply.error) {
        return Err(e);
    }
    let value = values
        .get_mut(&req.node)
        .and_then(|o| o.remove(&req.port))
        .ok_or_else(|| NodeError::new(NodeError::OP, format!("{}.{} made no value", req.node, req.port)))?;
    // a person saving an array not shaped like a picture as one is asked first (decision 2A)
    if req.ask
        && format.questions
        && let Some(q) = shape_question(&value)
    {
        let error = Some(config(format!("{}: save it as NumPy (.npy)", q.text)));
        return Ok(SaveReply { question: Some(q), error, ..Default::default() });
    }
    // the items: one, or every item of a list
    let mut sheet_items: Option<(usize, Vec<String>)> = None;
    let (items, labels, list): (Vec<Data>, Vec<String>, bool) = match value {
        Data::List(l) if req.sheet && !format.sheets => {
            return Err(config(format!("{} can't put a list on one sheet: save its {} items instead", format.def.label, l.items.len())));
        }
        // a contact sheet: every item on one picture, the labels in the reply
        Data::List(l) if req.sheet => {
            let labels = (0..l.items.len()).map(|i| l.label(i).to_owned()).collect::<Vec<_>>();
            let picture = contact_sheet(&l.items, &labels)?;
            sheet_items = Some((l.items.len(), labels));
            (vec![Data::Array(picture)], vec![String::new()], false)
        }
        Data::List(l) => match req.index {
            Some(i) => {
                let i = usize::try_from(i).map_err(|_| config("\"index\" must be a whole number"))?;
                let label = l.label(i).to_owned();
                let item = l.items.into_iter().nth(i).ok_or_else(|| config(format!("index {i} is past the list's end")))?;
                (vec![item], vec![label], false)
            }
            None => {
                let labels = (0..l.items.len()).map(|i| l.label(i).to_owned()).collect();
                (l.items, labels, true)
            }
        },
        one => match req.index {
            Some(i) if i != 0 => return Err(config(format!("{}.{} is one value, not a list", req.node, req.port))),
            _ => (vec![one], vec![String::new()], false),
        },
    };
    let mut encoded = Vec::with_capacity(items.len());
    for (item, label) in items.iter().zip(&labels) {
        if cancel.is_cancelled() {
            return Err(NodeError::cancelled());
        }
        encoded.push((format.encode)(item, &SaveCx { options: &opts, label, info: req.graph_info.as_ref() })?);
    }
    let ext = format.def.extensions.first().map_or("", String::as_str);
    let base = match req.path.as_deref() {
        Some(p) => Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        None => String::new(),
    };
    let base = if base.is_empty() { if ext.is_empty() { "result".to_owned() } else { format!("result.{ext}") } } else { base };
    let names = numbered(&base, encoded.len(), list);
    let stamped = format.stamps && req.graph_info.is_some();
    let mut out = match sheet_items {
        Some((n, labels)) => SaveReply { ok: true, stamped, items: n as u64, labels, ..Default::default() },
        None => SaveReply { ok: true, stamped, items: encoded.len() as u64, labels: if list { labels } else { Vec::new() }, ..Default::default() },
    };
    match req.path.as_deref() {
        Some(p) => {
            let dir = Path::new(p).parent().map(Path::to_path_buf).unwrap_or_default();
            for (name, bytes) in names.iter().zip(&encoded) {
                let target = if list { dir.join(name) } else { PathBuf::from(p) };
                write_whole(&target, bytes)?;
                out.paths.push(target.display().to_string());
            }
        }
        None => {
            for (name, bytes) in names.into_iter().zip(encoded) {
                let blob = Blob {
                    handle: plumcraft_proto::Handle::raw(&bytes),
                    meta: BlobMeta { media: format.media_type.clone(), size: bytes.len() as u64, form: Form::default(), aliases: Vec::new() },
                    source: BlobSource::Bytes(Arc::from(bytes)),
                    publish: true,
                    thumb: None,
                };
                out.files.push(SavedFile { name, data: Some(plane.blob_ref(&blob)?) });
            }
        }
    }
    Ok(out)
}

/// The formats an engine offers: the defaults for each core type its nodes output (unless it
/// turned them off), then its own (an own format replaces a default of the same id), less those
/// it took away; in [`defaults::ORDER`] for the defaults, its own after.
pub fn formats_of(types: &[String], own: &[Format], removed: &[String], with_defaults: bool) -> Vec<Format> {
    let mut out: Vec<Format> = Vec::new();
    if with_defaults {
        let mut d: BTreeMap<String, Format> = BTreeMap::new();
        for t in types {
            for f in defaults::for_type(t) {
                d.entry(f.def.id.clone()).or_insert(f);
            }
        }
        for id in defaults::ORDER {
            if let Some(f) = d.remove(*id) {
                out.push(f);
            }
        }
    }
    for f in own {
        match out.iter_mut().find(|x| x.def.id == f.def.id) {
            Some(slot) => *slot = f.clone(),
            None => out.push(f.clone()),
        }
    }
    out.retain(|f| !removed.contains(&f.def.id));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npy_headers_are_numpys() {
        let b = npy_bytes("<f4", &[2, 3], &[0u8; 24]).unwrap();
        assert_eq!(&b[..8], b"\x93NUMPY\x01\x00");
        let len = usize::from(u16::from_le_bytes([b[8], b[9]]));
        assert_eq!((10 + len) % 64, 0, "the data starts on a 64-byte boundary");
        let header = std::str::from_utf8(&b[10..10 + len]).unwrap();
        assert!(header.starts_with("{'descr': '<f4', 'fortran_order': False, 'shape': (2, 3), }") && header.ends_with('\n'), "{header:?}");
        assert_eq!(b.len(), 10 + len + 24);
        let one = npy_bytes("|u1", &[5], &[0u8; 5]).unwrap();
        assert!(String::from_utf8_lossy(&one[10..]).contains("'shape': (5,), }"));
        let scalar = npy_bytes("<f8", &[], &[0u8; 8]).unwrap();
        assert!(String::from_utf8_lossy(&scalar[10..]).contains("'shape': (), }"));
    }

    #[test]
    fn csv_quotes_what_needs_it() {
        let t = json!({"columns": [{"name": "a", "values": [1, 2.5, null]}, {"name": "b,c", "values": ["x", "say \"hi\"", true]}]});
        assert_eq!(String::from_utf8(csv_bytes(&t).unwrap()).unwrap(), "a,\"b,c\"\n1,x\n2.5,\"say \"\"hi\"\"\"\n,true\n");
        assert_eq!(csv_bytes(&json!({"columns": []})).unwrap(), b"\n");
        assert!(csv_bytes(&json!("no")).is_err());
    }

    #[test]
    fn wav_is_riff_with_the_samples() {
        let a = Audio { sample_rate: 8000, channels: 2, samples: Arc::from(vec![0.5f32, -1.0, 1.5, f32::NAN]) };
        let f = wav_bytes(&a, false);
        assert_eq!((&f[..4], &f[8..16]), (&b"RIFF"[..], &b"WAVEfmt "[..]));
        assert_eq!(u16::from_le_bytes([f[20], f[21]]), 3, "IEEE float");
        assert_eq!(f.len(), 44 + 16);
        let p = wav_bytes(&a, true);
        assert_eq!(u16::from_le_bytes([p[20], p[21]]), 1, "PCM");
        let s: Vec<i16> = p[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(s, [16384, -32767, 32767, 0], "0.5 rounds half away from zero; out of range clamps; NaN is silence");
    }

    #[test]
    fn options_are_checked_and_filled_in() {
        let f = defaults::jpeg().def;
        assert_eq!(options(&f, &Map::new()).unwrap()["quality"], 90);
        for bad in [json!({"quality": 0}), json!({"quality": 50.5}), json!({"quality": "best"}), json!({"nope": 1})] {
            assert_eq!(options(&f, bad.as_object().unwrap()).unwrap_err().kind, NodeError::CONFIG, "{bad}");
        }
        let w = defaults::wav().def;
        assert!(options(&w, json!({"bits": "int8"}).as_object().unwrap()).is_err());
        assert_eq!(options(&w, json!({"bits": "int16"}).as_object().unwrap()).unwrap()["bits"], "int16");
    }

    #[test]
    fn lists_save_as_numbered_files() {
        assert_eq!(numbered("a.png", 1, false), ["a.png"]);
        assert_eq!(numbered("a.png", 3, true), ["a-00.png", "a-01.png", "a-02.png"]);
        assert_eq!(numbered("noext", 2, true), ["noext-00", "noext-01"]);
        assert_eq!(numbered("x.tar.gz", 1, true), ["x.tar-00.gz"]);
        assert_eq!(numbered("a.png", 101, true)[100], "a-100.png");
    }

    #[test]
    fn an_engine_gets_defaults_by_type_adds_replaces_and_removes() {
        let ids = |v: &[Format]| v.iter().map(|f| f.def.id.clone()).collect::<Vec<_>>();
        let types = vec![core_types::TABLE.to_owned(), core_types::TEXT.to_owned()];
        assert_eq!(ids(&formats_of(&types, &[], &[], true)), ["txt", "csv", "json"]);
        let mine = Format::new("csv", "Mine", &["csv"], &[Media::Table], "text/csv", |_, _| Ok(Vec::new()));
        let extra = Format::new("md", "Markdown", &["md"], &[Media::Text], "text/markdown", |_, _| Ok(Vec::new()));
        let f = formats_of(&types, &[mine, extra], &["json".to_owned()], true);
        assert_eq!(ids(&f), ["txt", "csv", "md"]);
        assert_eq!(f[1].def.label, "Mine", "replaced in place");
        assert!(formats_of(&types, &[], &[], false).is_empty());
    }

    #[test]
    fn pictures_encode_from_arrays_and_images() {
        let grey = Data::Array(Array::new("|u1", vec![2, 3], vec![0u8, 50, 100, 150, 200, 250]));
        let png = (defaults::png().encode)(&grey, &SaveCx { options: &Map::new(), label: "", info: None }).unwrap();
        let back = image::load_from_memory(&png).unwrap().to_luma8();
        assert_eq!((back.width(), back.height(), back.as_raw().clone()), (3, 2, vec![0u8, 50, 100, 150, 200, 250]));
        let float = Data::Array(Array::from_f32(vec![1, 2, 3], &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0]));
        let jpg =
            (defaults::jpeg().encode)(&float, &SaveCx { options: &json!({"quality": 95}).as_object().cloned().unwrap_or_default(), label: "", info: None })
                .unwrap();
        assert_eq!(&jpg[..3], &[0xff, 0xd8, 0xff]);
        let table = Data::Array(Array { table: true, ..Array::from_f32(vec![1, 2], &[1.0, 2.0]) });
        assert!((defaults::png().encode)(&table, &SaveCx { options: &Map::new(), label: "", info: None }).unwrap_err().message.contains(".npy"));
    }

    /// The items of the shared contact-sheet fixture (`tests/fixtures/sheet.json`), made by
    /// formula so the Python SDK's test makes the same.
    fn sheet_items() -> Vec<Data> {
        let a = Array::new("|u1", vec![2, 3], vec![0u8, 50, 100, 150, 200, 250]);
        let b: Vec<u8> = (0..300u32).flat_map(|y| (0..100u32).flat_map(move |x| (0..3u32).map(move |c| ((y + 2 * x + 50 * c) % 256) as u8))).collect();
        let c: Vec<u8> = (0..10u32).flat_map(|y| (0..10u32).flat_map(move |x| [(x * 25) as u8, (y * 25) as u8, 128, ((x + y) * 12) as u8])).collect();
        vec![Data::Array(a), Data::Array(Array::new("|u1", vec![300, 100, 3], b)), Data::Array(Array::new("|u1", vec![10, 10, 4], c))]
    }

    /// The shared fixture's labels: one with a character the font hasn't (drawn `?`).
    fn sheet_labels() -> Vec<String> {
        vec!["dim 1".into(), "x=2.5".into(), "Sweep: 3 \u{e9}".into()]
    }

    #[test]
    fn a_contact_sheet_lays_out_every_item_and_matches_the_shared_fixture() {
        let sheet = contact_sheet(&sheet_items(), &sheet_labels()).unwrap();
        // three items: two columns, two rows of 256-pixel cells (the tallest item is 300), each
        // with its 15-pixel label band
        assert_eq!(sheet.shape, [2 * (256 + 15) + 3 * 8, 536, 3]);
        let px = |x: usize, y: usize| -> [u8; 3] {
            let k = (y * 536 + x) * 3;
            [sheet.bytes[k], sheet.bytes[k + 1], sheet.bytes[k + 2]]
        };
        assert_eq!(px(0, 0), SHEET_GROUND, "the gap");
        // the 2 by 3 grey, unscaled, centred in the first cell: x 8 + 126, y 8 + 127
        assert_eq!((px(134, 135), px(136, 136)), ([0, 0, 0], [250, 250, 250]));
        // the 300 by 100 strip scaled to 85 wide and 256 high, centred in the second cell
        assert_eq!(px(8 + 256 + 8 + 85, 8), [0, 50, 100], "its first pixel (a box of one row, a few columns)");
        assert_eq!(px(8 + 256 + 8 + 84, 8), SHEET_GROUND, "and none left of it");
        // the RGBA square over white, in the third cell (second row, first column)
        let (x0, y0) = (8 + 123, 8 + 256 + 15 + 8 + 123);
        assert_eq!(px(x0, y0), [255, 255, 255], "alpha 0 is white");
        let over = |c: u32, a: u32| ((c * a + 255 * (255 - a) + 127) / 255) as u8;
        assert_eq!(px(x0 + 9, y0 + 9), [over(225, 216), over(225, 216), over(128, 216)]);
        // each label in its band, centred under its cell; the empty fourth cell has none
        let ink = |x0: usize, y0: usize| (y0..y0 + 15).flat_map(|y| (x0..x0 + 256).map(move |x| (x, y))).filter(|&(x, y)| px(x, y)[0] < 128).count();
        assert!(ink(8, 8 + 256) > 0 && ink(8 + 256 + 8, 8 + 256) > 0 && ink(8, 8 + 256 + 15 + 8 + 256) > 0);
        assert_eq!(ink(8 + 256 + 8, 8 + 256 + 15 + 8 + 256), 0);
        let fixture: Value = serde_json::from_str(include_str!("../tests/fixtures/sheet.json")).unwrap();
        assert_eq!(fixture["labels"], json!(sheet_labels()));
        assert_eq!(fixture["shape"], json!(sheet.shape));
        let digest = commonkit::digest::fixed::raw(commonkit::digest::fixed::Fixed::Sha256, &sheet.bytes);
        if std::env::var_os("PLUMCRAFT_PRINT_VECTORS").is_some() {
            eprintln!("sheet: {:?} {digest}", sheet.shape);
        }
        assert_eq!(fixture["sha256"], digest, "the Python SDK's test checks the same pixels");
        // no labels: no band; a tiny picture still gets a 64-pixel cell
        let plain = contact_sheet(&sheet_items(), &[]).unwrap();
        assert_eq!(plain.shape, [536, 536, 3]);
        let tiny = contact_sheet(&[Data::Array(Array::new("|u1", vec![1, 1], vec![9u8]))], &["a".into()]).unwrap();
        assert_eq!(tiny.shape, [8 + 64 + 15 + 8, 8 + 64 + 8, 3]);
        // what it refuses
        assert!(contact_sheet(&[], &[]).is_err());
        assert!(contact_sheet(&[Data::Json(json!("text"))], &[]).unwrap_err().message.starts_with("item 0:"));
        assert!(contact_sheet(&vec![Data::Array(Array::new("|u1", vec![1, 1], vec![0u8])); crate::exec::MAX_SHEET + 1], &[]).is_err());
    }

    #[test]
    fn the_sheet_font_is_aileron_and_the_pythons_copy() {
        let f = sheet_font();
        assert_eq!(f.height, 14);
        assert!((32..127).all(|c| f.glyphs.contains_key(&c)), "printable ASCII");
        assert!(f.glyphs.values().all(|g| g.coverage.len() == g.width * f.height));
        assert!(f.glyphs[&32].coverage.iter().all(|a| *a == 0) && f.glyphs[&32].advance > 0, "space is blank and moves the pen");
        let l = &f.glyphs[&u32::from('l')].coverage;
        assert!(l.iter().any(|a| *a > 200) && l.iter().any(|a| *a > 0 && *a < 128), "solid strokes, antialiased edges");
        assert!(!f.glyphs.contains_key(&0xe9), "é isn't in Pillow's Aileron: it draws as ?");
        let text = include_str!("sheet_font.json");
        assert!(text.contains("Aileron Regular") && text.contains("CC0 1.0"));
        assert_eq!(text, include_str!("../../../python/src/plumcraft_engine/sheet_font.json"), "one font, both SDKs");
    }

    #[test]
    fn the_workflows_name_and_hash_go_where_a_format_has_room() {
        let info = GraphInfo { name: "Flow \u{e9}<1>".into(), hash: "sha256:ab".into() };
        let grey = Data::Array(Array::new("|u1", vec![2, 3], vec![0u8, 50, 100, 150, 200, 250]));
        let cx = SaveCx { options: &Map::new(), label: "", info: Some(&info) };
        let png = (defaults::png().encode)(&grey, &cx).unwrap();
        assert!(image::load_from_memory(&png).is_ok(), "still a PNG every reader takes");
        let at = png.windows(4).position(|w| w == b"iTXt").unwrap();
        let len = u32::from_be_bytes([png[at - 4], png[at - 3], png[at - 2], png[at - 1]]) as usize;
        assert_eq!(&png[at + 4..at + 4 + len], "PlumCraft graph\0\0\0\0\0Flow \u{e9}<1>".as_bytes());
        assert_eq!(crc32(&png[at..at + 4 + len]).to_be_bytes(), png[at + 4 + len..at + 8 + len]);
        assert!(png.windows(27).any(|w| w == b"PlumCraft graph hash\0\0\0\0\0sh"));
        assert!(png.ends_with(b"IEND\xae\x42\x60\x82"));
        assert_eq!(png_with_info(b"not a png", &info), b"not a png");
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926, "the check value");
        // WAV: a LIST INFO ICMT at the end, the RIFF size counting it
        let a = Audio { sample_rate: 8000, channels: 1, samples: Arc::from(vec![0.0f32; 3]) };
        let wav = (defaults::wav().encode)(&Data::Audio(a), &cx).unwrap();
        assert_eq!(u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]) as usize, wav.len() - 8);
        let list = wav.windows(4).position(|w| w == b"LIST").unwrap();
        assert_eq!(&wav[list + 8..list + 16], b"INFOICMT");
        assert!(String::from_utf8_lossy(&wav[list + 20..]).starts_with("PlumCraft graph \"Flow \u{e9}<1>\", sha256:ab\0"));
        assert_eq!(wav.len() % 2, 0);
        // SVG: a metadata element inside the root, the text escaped; still an SVG
        let svg = br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="4" height="4" data-x="a>b"><rect width="4" height="4"/></svg>"#;
        let out = String::from_utf8(svg_with_info(svg, &info)).unwrap();
        assert!(out.contains("data-x=\"a>b\"><metadata>PlumCraft graph \"Flow \u{e9}&lt;1&gt;\", sha256:ab</metadata><rect"), "{out}");
        assert!(plumcraft_svg::parse(out.as_bytes()).is_ok());
        let closed = String::from_utf8(svg_with_info(br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#, &info)).unwrap();
        assert!(closed.ends_with("</metadata></svg>") && plumcraft_svg::parse(closed.as_bytes()).is_ok(), "{closed}");
        assert_eq!(svg_with_info(b"<html/>", &info), b"<html/>");
        // no room: text carries nothing
        assert_eq!((defaults::txt().encode)(&Data::Json(json!("plum")), &cx).unwrap(), b"plum");
    }
}
