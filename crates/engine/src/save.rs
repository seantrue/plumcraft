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
//! with `index`; `sheet` (one picture of every item) is refused with `config` unless a format
//! makes sheets, and none of the defaults does. A failure is data: `ok: false` and the error, the
//! node's own when computing it failed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use plumcraft_proto::catalogue::{Media, NodeDef, Port, SaveFormat, core_types, scalars};
use plumcraft_proto::data::{BlobMeta, Form};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::StatusRecord;
use plumcraft_proto::run::{GraphRef, RunRequest, SaveReply, SaveRequest, SavedFile};
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
    /// What made the value: the whole workflow (`recipe_graph`, or the graph sent). An encoder
    /// whose files carry it says so with [`Format::carrying_recipe`].
    pub recipe: &'a Value,
    /// The list item's label (what varied), or `""`.
    pub label: &'a str,
}

/// A save format: what the catalogue lists, and how it writes.
#[derive(Clone)]
pub struct Format {
    pub def: SaveFormat,
    /// The media type of the files it writes (`image/png`), for a file returned as a blob.
    pub media_type: String,
    /// Its files carry the recipe (`SaveReply.recipe`).
    pub recipe: bool,
    pub encode: Encode,
}

impl std::fmt::Debug for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Format").field("def", &self.def).field("media_type", &self.media_type).field("recipe", &self.recipe).finish_non_exhaustive()
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
            recipe: false,
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

    /// Its encoder writes [`SaveCx::recipe`] into every file.
    pub fn carrying_recipe(mut self) -> Format {
        self.recipe = true;
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
        Format::new("png", "PNG", &["png"], &[Media::Image, Media::Ndarray], "image/png", |v, _| match v {
            Data::Blob(b) if b.meta.media == "image/png" => b.read(plumcraft_store::MAX_READ),
            other => encode_picture(other, image::ImageFormat::Png, 0),
        })
    }

    /// IMAGE, or an ARRAY that is a picture, as JPEG (`.jpg`), at `quality` 1 to 100 (90).
    pub fn jpeg() -> Format {
        let quality = Port { min: Some(1.0), max: Some(100.0), ..option("quality", "Quality", scalars::INT, json!(90)) };
        Format::new("jpeg", "JPEG", &["jpg", "jpeg"], &[Media::Image, Media::Ndarray], "image/jpeg", |v, cx| {
            let q = cx.options.get("quality").and_then(Value::as_u64).unwrap_or(90).clamp(1, 100);
            encode_picture(v, image::ImageFormat::Jpeg, u8::try_from(q).unwrap_or(90))
        })
        .with_option(quality)
    }

    /// AUDIO as a RIFF WAVE file (`.wav`): 32-bit float samples, or 16-bit PCM (`bits`).
    pub fn wav() -> Format {
        let bits = Port { choices: Some(vec![json!("float32"), json!("int16")]), ..option("bits", "Sample format", scalars::ENUM, json!("float32")) };
        Format::new("wav", "WAV", &["wav"], &[Media::Audio], "audio/wav", |v, cx| match v {
            Data::Audio(a) => Ok(wav_bytes(a, cx.options.get("bits").and_then(Value::as_str) == Some("int16"))),
            _ => Err(config("WAV saves audio")),
        })
        .with_option(bits)
    }

    /// SVG as itself (`.svg`).
    pub fn svg() -> Format {
        Format::new("svg", "SVG", &["svg"], &[Media::Vector], core_types::SVG_MEDIA, |v, _| match v {
            Data::Blob(b) if b.is_svg() => b.read(plumcraft_store::MAX_READ),
            _ => Err(config("SVG saves an SVG")),
        })
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
    // the items: one, or every item of a list
    let (items, labels, list): (Vec<Data>, Vec<String>, bool) = match value {
        Data::List(l) if req.sheet => {
            return Err(config(format!("{} can't put a list on one sheet: save its {} items instead", format.def.label, l.items.len())));
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
    let recipe = req.recipe_graph.clone().unwrap_or_else(|| graph.clone());
    let mut encoded = Vec::with_capacity(items.len());
    for (item, label) in items.iter().zip(&labels) {
        if cancel.is_cancelled() {
            return Err(NodeError::cancelled());
        }
        encoded.push((format.encode)(item, &SaveCx { options: &opts, recipe: &recipe, label })?);
    }
    let ext = format.def.extensions.first().map_or("", String::as_str);
    let base = match req.path.as_deref() {
        Some(p) => Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        None => String::new(),
    };
    let base = if base.is_empty() { if ext.is_empty() { "result".to_owned() } else { format!("result.{ext}") } } else { base };
    let names = numbered(&base, encoded.len(), list);
    let mut out =
        SaveReply { ok: true, recipe: format.recipe, items: encoded.len() as u64, labels: if list { labels } else { Vec::new() }, ..Default::default() };
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
        let png = (defaults::png().encode)(&grey, &SaveCx { options: &Map::new(), recipe: &Value::Null, label: "" }).unwrap();
        let back = image::load_from_memory(&png).unwrap().to_luma8();
        assert_eq!((back.width(), back.height(), back.as_raw().clone()), (3, 2, vec![0u8, 50, 100, 150, 200, 250]));
        let float = Data::Array(Array::from_f32(vec![1, 2, 3], &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0]));
        let jpg = (defaults::jpeg().encode)(
            &float,
            &SaveCx { options: &json!({"quality": 95}).as_object().cloned().unwrap_or_default(), recipe: &Value::Null, label: "" },
        )
        .unwrap();
        assert_eq!(&jpg[..3], &[0xff, 0xd8, 0xff]);
        let table = Data::Array(Array { table: true, ..Array::from_f32(vec![1, 2], &[1.0, 2.0]) });
        assert!((defaults::png().encode)(&table, &SaveCx { options: &Map::new(), recipe: &Value::Null, label: "" }).unwrap_err().message.contains(".npy"));
    }
}
