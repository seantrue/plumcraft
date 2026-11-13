//! Files: Select Files, Find Files, Load Files and Write Files, on the engine's machine.
//!
//! - **Select Files** is the files chosen in a dialog (its `files` widget holds one path per
//!   line), as a list.
//! - **Find Files** lists the files in a folder whose names match a pattern (`*.png, *.jpg`;
//!   `*` and `?`, case ignored), optionally in its subfolders too, sorted; hidden files and
//!   folders are skipped.
//! - **Load Files** reads a file into a value (an image, an SVG, a PDF, any bytes, its media
//!   type from its extension); given a list of paths, it reads each.
//! - **Write Files** writes values into a folder, each named from `basename` and what made it
//!   (`trace_corgi_colors-8.svg`: the labels Vary and the file nodes give each item), with the
//!   extension its media type has. It never overwrites a different file unless `overwrite` is on.
//!
//! The lists Select and Find make label each file by its name without the extension, so the
//! results made from them carry it. Their memo keys cover the files' sizes and times (and Find's
//! the folder's listing), so a changed file is read again.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use plumcraft_proto::catalogue::{MemoScope, MemoSpec, NodeDef, NodeFlags, PathFilter, PathHint, Port, Socket};
use plumcraft_proto::data::{BlobMeta, Form};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::media::{ext_for, media_for_ext};
use serde_json::{Value, json};

use crate::{ANY, Blob, BlobSource, Cx, Data, Inputs, List, Node, Outputs};

/// The most files Find Files lists.
pub const MAX_FILES: usize = 10_000;
/// The largest file Load Files reads.
pub const MAX_LOAD: u64 = 1 << 30;

fn category() -> Vec<String> {
    vec!["Building blocks".into(), "Files".into()]
}

fn config(message: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::CONFIG, message)
}

fn op(message: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::OP, message)
}

fn text(i: &Inputs, key: &str) -> String {
    i.get(key).and_then(Data::as_str).unwrap_or_default().trim().to_owned()
}

fn flag(i: &Inputs, key: &str) -> bool {
    i.get(key).and_then(Data::json).and_then(Value::as_bool).unwrap_or(false)
}

fn path_port(name: &str, mode: &str, default: &str, tip: &str) -> Port {
    Port {
        name: name.into(),
        ty: "STRING".into(),
        default: json!(default),
        path: Some(PathHint { mode: mode.into(), filters: Vec::new() }),
        tooltip: Some(tip.into()),
        ..Default::default()
    }
}

fn files_out() -> Port {
    Port { name: "files".into(), ty: "STRING".into(), list: true, socket: Some(Socket::Socket), ..Default::default() }
}

/// `~` expanded (`USERPROFILE` when there is no `HOME`; `~\` too on Windows), as the Python SDK's
/// `files._expand`.
fn expand(p: &str) -> PathBuf {
    plumcraft_proto::programs::expand_home(p)
}

/// A file's name without its extension: the label it gives the values made from it.
fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Size and modification time, for memo keys.
fn stamp(p: &Path) -> Value {
    match std::fs::metadata(p) {
        Ok(m) => {
            let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
            json!([p.display().to_string(), m.len(), t])
        }
        Err(_) => json!([p.display().to_string()]),
    }
}

/// A list of paths, each labelled by its stem.
fn path_list(paths: &[(PathBuf, String)]) -> Data {
    Data::List(List {
        items: paths.iter().map(|(p, _)| Data::Json(Value::from(p.display().to_string()))).collect(),
        axes: Vec::new(),
        labels: paths.iter().map(|(_, l)| l.clone()).collect(),
    })
}

/// Select Files: the files chosen in a dialog.
struct Select;

impl Select {
    fn chosen(inputs: &Inputs) -> Vec<PathBuf> {
        text(inputs, "files").lines().map(str::trim).filter(|l| !l.is_empty()).map(expand).collect()
    }
}

impl Node for Select {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "file.select".into(),
            display: "Select Files".into(),
            category: category(),
            description: "The files chosen with \u{2026} (several at once), as a list: wire it into a node's file input to run that node once per file, each result labelled with the file's name.".into(),
            inputs: vec![path_port("files", "files", "", "The files, one path per line: choose them with \u{2026}")],
            outputs: vec![files_out()],
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
        let chosen = Self::chosen(inputs);
        if chosen.is_empty() {
            return Err(config("no files chosen: choose some with \u{2026}"));
        }
        let missing: Vec<String> = chosen.iter().filter(|p| !p.is_file()).map(|p| p.display().to_string()).collect();
        if !missing.is_empty() {
            return Err(NodeError::new(NodeError::MISSING_SOURCE, format!("not a file: {}", missing.join(", "))));
        }
        let paths: Vec<(PathBuf, String)> = chosen.into_iter().map(|p| (p.clone(), stem(&p))).collect();
        Ok(Outputs::from([("files".to_owned(), path_list(&paths))]))
    }

    fn volatile(&self, inputs: &Inputs) -> Option<Value> {
        Some(Value::Array(Self::chosen(inputs).iter().map(|p| stamp(p)).collect()))
    }
}

/// Does `name` match glob `pat` (`*`, `?`; case ignored)?
pub fn glob_match(pat: &str, name: &str) -> bool {
    let p: Vec<char> = pat.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    // the classic two-pointer match with one backtrack point
    let (mut i, mut j, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while j < n.len() {
        match p.get(i) {
            Some('*') => {
                star = Some(i);
                mark = j;
                i += 1;
            }
            Some(c) if *c == '?' || Some(c) == n.get(j) => {
                i += 1;
                j += 1;
            }
            _ => match star {
                Some(s) => {
                    i = s + 1;
                    mark += 1;
                    j = mark;
                }
                None => return false,
            },
        }
    }
    p.get(i..).is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

/// Find Files: a folder's files matching a pattern.
struct Find;

impl Find {
    /// The matching files, sorted, each with its label (its stem; with the folders below the
    /// searched one when recursive).
    fn scan(inputs: &Inputs) -> Result<Vec<(PathBuf, String)>, NodeError> {
        let folder = text(inputs, "folder");
        if folder.is_empty() {
            return Err(config("folder: choose a folder to search"));
        }
        let root = expand(&folder);
        if !root.is_dir() {
            return Err(NodeError::new(NodeError::MISSING_SOURCE, format!("folder: {} isn't a folder", root.display())));
        }
        let pattern = text(inputs, "pattern");
        let pats: Vec<&str> = pattern.split([',', ';']).map(str::trim).filter(|p| !p.is_empty()).collect();
        let pats = if pats.is_empty() { vec!["*"] } else { pats };
        let recursive = flag(inputs, "recursive");
        let mut found = Vec::new();
        let mut dirs = vec![root.clone()];
        while let Some(dir) = dirs.pop() {
            let entries = std::fs::read_dir(&dir).map_err(|e| op(format!("{}: {e}", dir.display())))?;
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let path = e.path();
                let Ok(kind) = e.file_type() else { continue };
                // a link is followed for files only, so a loop of links can't trap the search
                if kind.is_dir() {
                    if recursive {
                        dirs.push(path);
                    }
                } else if (kind.is_file() || path.is_file()) && pats.iter().any(|p| glob_match(p, &name)) {
                    let rel = path.strip_prefix(&root).unwrap_or(&path).with_extension("");
                    found.push((path.clone(), rel.to_string_lossy().into_owned()));
                    if found.len() > MAX_FILES {
                        return Err(config(format!("more than {MAX_FILES} files match: narrow the pattern")));
                    }
                }
            }
        }
        found.sort();
        Ok(found)
    }
}

impl Node for Find {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "file.find".into(),
            display: "Find Files".into(),
            category: category(),
            description: "The files in a folder whose names match the pattern (*.png, *.jpg: * and ?, case ignored; several separated by commas), sorted, optionally in its subfolders too. Wire it into a node's file input to run that node once per file, each result labelled with the file's name.".into(),
            inputs: vec![
                path_port("folder", "dir", "", "The folder to search"),
                Port { name: "pattern".into(), ty: "STRING".into(), default: json!("*"), tooltip: Some("File names to match: * and ?, several separated by commas (*.png, *.jpg)".into()), ..Default::default() },
                Port { name: "recursive".into(), ty: "BOOL".into(), default: json!(false), tooltip: Some("Search the subfolders too".into()), ..Default::default() },
            ],
            outputs: vec![files_out()],
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
        let found = Self::scan(inputs)?;
        if found.is_empty() {
            return Err(config(format!("no files match {:?}", text(inputs, "pattern"))));
        }
        Ok(Outputs::from([("files".to_owned(), path_list(&found))]))
    }

    fn volatile(&self, inputs: &Inputs) -> Option<Value> {
        Some(Self::scan(inputs).map_or(Value::Null, |f| Value::Array(f.iter().map(|(p, _)| stamp(p)).collect())))
    }
}

/// A node thumbnail for a picture file, or `None` for one it can't read: the value is still good.
/// Bounded, as a file is hostile input.
fn picture_thumb(bytes: &[u8]) -> Option<crate::Array> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(1 << 30);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    reader.limits(limits);
    let small = reader.decode().ok()?.thumbnail(crate::exec::THUMB_PX, crate::exec::THUMB_PX).into_rgba8();
    let (w, h) = small.dimensions();
    Some(crate::Array::new("|u1", vec![u64::from(h), u64::from(w), 4], small.into_raw()))
}

/// Load Files: a file read into a value.
struct Load;

/// The form a file's media type suggests (sizes left for the reader).
fn form_for(media: &str) -> Form {
    if media == "image/svg+xml" {
        Form::Vector { width: 0.0, height: 0.0 }
    } else if media.starts_with("image/") {
        Form::Image { width: 0, height: 0 }
    } else {
        Form::Bytes
    }
}

impl Node for Load {
    fn def(&self) -> NodeDef {
        let mut file = path_port("file", "open", "", "The file to read; wire Find Files or Select Files to read each");
        file.socket = Some(Socket::Both);
        file.path = Some(PathHint { mode: "open".into(), filters: vec![PathFilter { name: "Any file".into(), extensions: Vec::new() }] });
        NodeDef {
            id: "file.load".into(),
            display: "Load Files".into(),
            category: category(),
            description: "Read a file into a value: an image, an SVG, a PDF or any bytes, its media type from its extension. Given a list of files (Find Files, Select Files), it reads each, so wiring its value into, say, an app's Open image input opens every file.".into(),
            inputs: vec![file],
            outputs: vec![Port { name: "value".into(), ty: ANY.into(), socket: Some(Socket::Socket), ..Default::default() }],
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
        let file = text(inputs, "file");
        if file.is_empty() {
            return Err(config("file: choose a file, or wire Find Files or Select Files"));
        }
        let path = expand(&file);
        let meta = std::fs::metadata(&path).map_err(|e| NodeError::new(NodeError::MISSING_SOURCE, format!("{}: {e}", path.display())))?;
        if !meta.is_file() || meta.len() > MAX_LOAD {
            return Err(config(format!("{}: not a file of at most {MAX_LOAD} bytes", path.display())));
        }
        let bytes = std::fs::read(&path).map_err(|e| op(format!("{}: {e}", path.display())))?;
        let media = media_for_ext(&path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default());
        // an SVG it can draw is the core SVG type, with its thumbnail; one it can't stays a file
        if media == plumcraft_proto::catalogue::core_types::SVG_MEDIA
            && let Ok(svg) = Blob::svg(bytes.clone())
        {
            return Ok(Outputs::from([("value".to_owned(), Data::Blob(Box::new(svg)))]));
        }
        let thumb = if media.starts_with("image/") { picture_thumb(&bytes) } else { None };
        let blob = Blob {
            handle: plumcraft_proto::Handle::raw(&bytes),
            meta: BlobMeta { media: media.into(), size: bytes.len() as u64, form: form_for(media), aliases: Vec::new() },
            source: BlobSource::Bytes(Arc::from(bytes)),
            publish: true,
            thumb,
        };
        Ok(Outputs::from([("value".to_owned(), Data::Blob(Box::new(blob)))]))
    }

    fn volatile(&self, inputs: &Inputs) -> Option<Value> {
        let file = text(inputs, "file");
        (!file.is_empty()).then(|| stamp(&expand(&file)))
    }
}

/// Write Files: values into a folder.
struct Write;

/// A file-name piece: letters, digits, `.`, `-` and `_`; anything else becomes `_`.
fn safe(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
    t.trim_matches(['_', '.']).to_owned()
}

/// A file name from a basename and an item's label: `trace` and `corgi · colors 8` make
/// `trace_corgi_colors-8`.
pub fn file_stem(basename: &str, label: &str) -> String {
    let parts = label.split(" · ").map(|p| safe(&p.trim().replace(' ', "-"))).filter(|p| !p.is_empty());
    let all: Vec<String> = std::iter::once(safe(basename)).filter(|b| !b.is_empty()).chain(parts).collect();
    all.join("_")
}

/// A value's bytes and extension, as a file holds it.
fn file_bytes(v: &Data) -> Result<(Vec<u8>, &'static str), NodeError> {
    match v {
        Data::Blob(b) => {
            let bytes = match &b.source {
                BlobSource::Bytes(x) => x.to_vec(),
                BlobSource::File(p) => std::fs::read(p).map_err(|e| op(format!("{}: {e}", p.display())))?,
            };
            Ok((bytes, ext_for(&b.meta.media)))
        }
        Data::Json(Value::String(s)) => Ok((s.as_bytes().to_vec(), "txt")),
        Data::Json(j) => Ok((serde_json::to_vec_pretty(j).map_err(|e| op(e.to_string()))?, "json")),
        Data::List(l) => {
            let j: Option<Vec<Value>> = l.items.iter().map(|i| i.json().cloned()).collect();
            let j = j.ok_or_else(|| config("a tuple of values that aren't plain can't be written as one file: Unzip it first"))?;
            Ok((serde_json::to_vec_pretty(&j).map_err(|e| op(e.to_string()))?, "json"))
        }
        Data::Array(_) | Data::Audio(_) => Err(config("arrays and audio can't be written as files yet")),
    }
}

impl Node for Write {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "file.write".into(),
            display: "Write Files".into(),
            category: category(),
            description: "Write values into a folder: each file is named from basename and what made the value (trace_corgi_colors-8.svg: the file it came from and the settings varied), with its type's extension. A different file already there is an error unless overwrite is on.".into(),
            inputs: vec![
                Port { name: "values".into(), ty: ANY.into(), socket: Some(Socket::Socket), tooltip: Some("The values to write: one, or a list".into()), ..Default::default() },
                path_port("folder", "dir", "", "The folder to write into (made when missing)"),
                Port { name: "basename".into(), ty: "STRING".into(), default: json!("out"), tooltip: Some("The start of every file name; what varied follows it".into()), ..Default::default() },
                Port { name: "overwrite".into(), ty: "BOOL".into(), default: json!(false), tooltip: Some("Replace files that are already there".into()), ..Default::default() },
            ],
            outputs: vec![files_out()],
            flags: NodeFlags { input_list: true, output_node: true, ..Default::default() },
            memo: MemoSpec { scope: MemoScope::None, ..Default::default() },
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
        let folder = text(inputs, "folder");
        if folder.is_empty() {
            return Err(config("folder: choose a folder to write into"));
        }
        if inputs.get("folder").is_some_and(|f| f.list().is_some()) {
            return Err(config("folder: one folder, not a list"));
        }
        let dir = expand(&folder);
        let basename = text(inputs, "basename");
        let overwrite = flag(inputs, "overwrite");
        let v = inputs.get("values").ok_or_else(|| config("values: wire what to write"))?;
        let (items, labels, axes): (Vec<&Data>, Vec<String>, Vec<(String, usize)>) = match v {
            Data::List(l) => (l.items.iter().collect(), (0..l.items.len()).map(|i| l.label(i).to_owned()).collect(), l.axes.clone()),
            one => (vec![one], vec![String::new()], Vec::new()),
        };
        // plan every name first: nothing is written when any would fail
        let mut planned: Vec<(PathBuf, Vec<u8>)> = Vec::with_capacity(items.len());
        let stems: Vec<String> = labels.iter().map(|l| file_stem(&basename, l)).collect();
        let numbered = items.len() > 1 && {
            let mut seen = std::collections::BTreeSet::new();
            !stems.iter().all(|s| !s.is_empty() && seen.insert(s.clone()))
        };
        for (i, (item, stem)) in items.iter().zip(&stems).enumerate() {
            let (bytes, ext) = file_bytes(item)
                .map_err(|e| if items.len() > 1 { NodeError { message: format!("item {} of {}: {}", i + 1, items.len(), e.message), ..e } } else { e })?;
            let base = if stem.is_empty() { "out".to_owned() } else { stem.clone() };
            let name = if numbered { format!("{base}-{:03}.{ext}", i + 1) } else { format!("{base}.{ext}") };
            planned.push((dir.join(name), bytes));
        }
        if !overwrite {
            let clash: Vec<String> =
                planned.iter().filter(|(p, b)| p.exists() && std::fs::read(p).ok().as_ref() != Some(b)).map(|(p, _)| p.display().to_string()).collect();
            if !clash.is_empty() {
                return Err(config(format!("already there (turn on overwrite to replace): {}", clash.join(", "))));
            }
        }
        std::fs::create_dir_all(&dir).map_err(|e| op(format!("{}: {e}", dir.display())))?;
        let n = planned.len();
        for (i, (path, bytes)) in planned.iter().enumerate() {
            if cx.cancelled() {
                return Err(NodeError::cancelled());
            }
            cx.progress(i as f64 / n as f64, Some("writing"));
            if std::fs::read(path).ok().as_ref() == Some(bytes) {
                continue;
            }
            let tmp = path.with_extension(format!("plumcraft-{}.tmp", std::process::id()));
            std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                op(format!("{}: {e}", path.display()))
            })?;
        }
        let out = Data::List(List { items: planned.iter().map(|(p, _)| Data::Json(Value::from(p.display().to_string()))).collect(), axes, labels });
        Ok(Outputs::from([("files".to_owned(), out)]))
    }
}

/// Select, Find, Load and Write Files.
pub fn nodes() -> Vec<Box<dyn Node>> {
    vec![Box::new(Select), Box::new(Find), Box::new(Load), Box::new(Write)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_and_names() {
        assert!(glob_match("*.PNG", "corgi.png"));
        assert!(glob_match("c?rgi*", "corgi-2.jpg"));
        assert!(!glob_match("*.png", "corgi.png.txt"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("a*b*c", "aXbY"));
        assert_eq!(file_stem("trace", "corgi · colors 8"), "trace_corgi_colors-8");
        assert_eq!(file_stem("", "sub/dir/a b · x 0.5"), "sub_dir_a-b_x-0.5");
        assert_eq!(file_stem("out", ""), "out");
    }
}

#[cfg(test)]
mod thumb_tests {
    use super::picture_thumb;

    #[test]
    fn a_picture_file_gets_a_thumbnail_and_junk_gets_none() {
        let red = image::RgbImage::from_pixel(320, 200, image::Rgb([230, 20, 10]));
        for format in [image::ImageFormat::Png, image::ImageFormat::Jpeg, image::ImageFormat::Tiff] {
            let mut bytes = std::io::Cursor::new(Vec::new());
            red.write_to(&mut bytes, format).unwrap();
            let t = picture_thumb(&bytes.into_inner()).unwrap();
            assert_eq!(t.shape, [100, 160, 4], "{format:?}: the shape kept");
            assert!(t.bytes[0] > 200 && t.bytes[1] < 60, "{format:?}: red");
        }
        for junk in [&b""[..], b"not a picture", b"\x89PNG\r\n\x1a\njunk"] {
            assert!(picture_thumb(junk).is_none());
        }
    }
}
