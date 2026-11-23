//! The `mask` value editor's commands (`docs/shell-interface.md` 12.22; the brush and canvas are
//! `plumcraft_proto::masks`): a stroke, a clear, the edit as it stands, and the folder edits go
//! in. A stroke is one command and one undo step: it repaints from its points onto the node's
//! edit as it stands, writes the result as a new file, and sets the node's input to that file, so
//! undo goes back to the file before (the maintainer's "mask per-stroke undo with fixes").
//!
//! **The edit file** is an RGBA PNG at the mask's size, alpha the mask and the colours the picture
//! as painted, named for its pixels (`<node>-<16 hex>.png`, so each change is a new file and the
//! engine's cache sees it), with a sidecar `<file>.json` saying whose it is (`editor:
//! "plumcraft"`), the node and the size. A canvas no different from where it started clears the
//! input instead.
//!
//! **Where files go** (the maintainer, 2026-11-23, decisions 7A and 4A): `<document>.edits/`
//! beside the document when that can be written; otherwise a folder the person chose once for this
//! document (`mask.folder`, kept in the graph's `meta`). A stroke that has neither fails with
//! [`NEEDS_FOLDER`]; invoked (`ui.invoke`, a person's stroke), it asks for the folder
//! (`folder.pick {purpose: "edits"}`) and then strokes.

use std::path::{Path, PathBuf};

use plumcraft_graph::{NodeId, PortRef};
use plumcraft_proto::display::Rendition;
use plumcraft_proto::masks::{self, MaskCanvas};
use plumcraft_session::{Session, paths};
use serde_json::{Value, json};

use crate::control::check_fields;

/// The editor kind these commands edit.
pub const KIND: &str = "mask";
/// The sidecar's `editor`: whose edit file this is (decision 7A of the S4b set).
pub const SIDECAR_EDITOR: &str = "plumcraft";
/// The graph `meta` key holding the folder chosen for this document's edit files.
pub const FOLDER_META: &str = "mask_folder";
/// The start of the error a stroke gives when it has nowhere to write.
pub const NEEDS_FOLDER: &str = "no folder for mask files";

/// The node, its edit input, and the input's value now.
fn target(s: &Session, p: &Value) -> Result<(NodeId, String, String), String> {
    let node = NodeId::from(p.get("node").and_then(Value::as_str).ok_or("\"node\" is required")?);
    let n = s.graph().node(&node).ok_or_else(|| format!("no node {node}"))?;
    let def = s.catalog().node(&n.kind).ok_or_else(|| format!("{} isn't in the catalogue", n.kind))?;
    let port = match p.get("port") {
        None | Some(Value::Null) => def
            .editor
            .as_ref()
            .filter(|e| e.kind == KIND && !e.port.is_empty())
            .map(|e| e.port.clone())
            .ok_or_else(|| format!("{} names no mask editor: say which port", n.kind))?,
        Some(Value::String(port)) => port.clone(),
        Some(_) => return Err("\"port\" must be an input's name".into()),
    };
    let declared = def.inputs.iter().find(|q| q.name == port).ok_or_else(|| format!("{} has no input {port}", n.kind))?;
    if s.graph().wire_into(&PortRef::new(node.clone(), port.clone())).is_some() {
        return Err(format!("{port} comes from a wire: there is no edit here to paint"));
    }
    let value = plumcraft_graph::ports::current_value(def, n, &port, declared);
    Ok((node, port, value.as_str().unwrap_or_default().to_owned()))
}

/// RGBA pixels from a resolved picture's bytes: raw, or an array file's (`.npy`) whose last
/// `w × h × 4` bytes are its pixels.
fn pixels(bytes: &[u8], w: usize, h: usize) -> Option<Vec<[u8; 4]>> {
    let need = w.checked_mul(h)?.checked_mul(4)?;
    let body = if bytes.len() == need {
        bytes
    } else if bytes.starts_with(b"\x93NUMPY") && bytes.len() > need {
        bytes.get(bytes.len() - need..)?
    } else {
        return None;
    };
    Some(body.as_chunks::<4>().0.to_vec())
}

/// A picture the run sent: its width, height and RGBA pixels.
type Picture = (usize, usize, Vec<[u8; 4]>);

/// The picture the last run sent the node's mask editor, and the mask it starts from.
struct Step {
    w: usize,
    h: usize,
    picture: Vec<[u8; 4]>,
    base: Option<Vec<f32>>,
}

impl Step {
    /// The canvas before any painting: the picture, masked as the base says.
    fn start(&self) -> MaskCanvas {
        match &self.base {
            Some(b) => MaskCanvas::on_base(self.w, self.h, &self.picture, b),
            None => MaskCanvas::new(self.w, self.h, &self.picture),
        }
    }
}

fn step(s: &Session, node: &NodeId) -> Result<Step, String> {
    let data = s.run.node(&node.0).and_then(|r| r.editor.get(KIND)).ok_or("run it once first: the mask editor paints on the picture a run sends")?;
    if let Some(e) = data.get("error").and_then(Value::as_str) {
        return Err(e.to_owned());
    }
    let read = |field: &str| -> Result<Option<Picture>, String> {
        let Some(v) = data.get(field) else { return Ok(None) };
        let Ok(Rendition::Rgba8 { data: r, w, h, .. }) = serde_json::from_value::<Rendition>(v.clone()) else {
            return Err(format!("the run's {field} isn't a picture"));
        };
        let resolver = s.docs.resolver.as_ref().ok_or("no engine link to read the run's picture through")?;
        let bytes = resolver(&r).ok_or("the run's picture is gone: run it again")?;
        let (w, h) = (w as usize, h as usize);
        let px = pixels(&bytes, w, h).ok_or_else(|| format!("the run's {field} isn't {w} × {h} RGBA"))?;
        Ok(Some((w, h, px)))
    };
    let (w, h, picture) = read("picture")?.ok_or("this run sent no picture to paint on")?;
    let base = match read("base")? {
        Some((bw, bh, px)) if (bw, bh) == (w, h) => Some(px.iter().map(|p| f32::from(p[0]) / 255.0).collect()),
        Some(_) => return Err("the run's base mask isn't the picture's size".into()),
        None => None,
    };
    Ok(Step { w, h, picture, base })
}

/// The document's own folder, when it has a file (never the working directory: on the iPad that
/// is `/`, and a path made relative to it lost its root).
fn doc_folder(s: &Session) -> Option<PathBuf> {
    s.path().and_then(Path::parent).filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf)
}

/// A value's file: relative values are relative to the document's folder.
fn resolve(value: &str, s: &Session) -> PathBuf {
    match doc_folder(s) {
        Some(base) if !paths::is_anchored(value) => base.join(value),
        _ => paths::expand_user(value),
    }
}

/// The canvas the node's edit holds now: its file, or where painting starts when it has none.
fn current(s: &Session, value: &str, st: &Step) -> Result<MaskCanvas, String> {
    if value.trim().is_empty() {
        return Ok(st.start());
    }
    let file = resolve(value.trim(), s);
    let bytes = std::fs::read(&file).map_err(|e| format!("the edit {value}: {e}"))?;
    let rgba = plumcraft_media::decode::decode_png(&bytes, u32::MAX).map_err(|e| format!("the edit {value}: {e}"))?;
    if (rgba.w as usize, rgba.h as usize) != (st.w, st.h) {
        return Err(format!("the edit is {} × {}, the picture {} × {}: clear the mask and paint again", rgba.w, rgba.h, st.w, st.h));
    }
    MaskCanvas::from_rgba(st.w, st.h, rgba.pixels.as_chunks::<4>().0).ok_or_else(|| format!("the edit {value} is torn"))
}

/// The folder this document's edit files go in, made if it isn't there. Err: nowhere to write.
pub fn edits_folder(s: &Session) -> Result<PathBuf, String> {
    let chosen = s.graph().meta.get(FOLDER_META).and_then(Value::as_str).filter(|f| !f.trim().is_empty()).map(paths::expand_user);
    let beside = s.path().and_then(|p| Some(p.with_file_name(format!("{}.edits", p.file_stem()?.to_str()?))));
    let mut why = String::new();
    for dir in chosen.into_iter().chain(beside) {
        match std::fs::create_dir_all(&dir).and_then(|()| probe(&dir)) {
            Ok(()) => return Ok(dir),
            Err(e) => why = format!(" ({}: {e})", dir.display()),
        }
    }
    let unsaved = if s.path().is_none() { ": the graph has no file yet" } else { "" };
    Err(format!("{NEEDS_FOLDER}{unsaved}{why}; choose one (mask.folder {{folder}}), or stroke through ui.invoke to be asked"))
}

/// Can files be written in `dir`?
fn probe(dir: &Path) -> std::io::Result<()> {
    let p = dir.join(format!(".probe-{}", std::process::id()));
    std::fs::write(&p, b"")?;
    std::fs::remove_file(&p)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

/// Keep `canvas` as the node's edit file in `dir`: its name, from its pixels.
fn store(canvas: MaskCanvas, node: &NodeId, dir: &Path) -> Result<PathBuf, String> {
    let canvas = canvas.rounded();
    let file = dir.join(format!("{}-{:016x}.png", node.0, canvas.pixel_hash()));
    if !file.is_file() {
        let png = plumcraft_media::decode::encode_png_rgba(canvas.w as u32, canvas.h as u32, &canvas.rgba())?;
        let meta = json!({"call": node.0, "w": canvas.w, "h": canvas.h, "node": node.0, "editor": SIDECAR_EDITOR});
        // the sidecar first: a PNG on disk always has one
        write_atomic(&PathBuf::from(format!("{}.json", file.display())), meta.to_string().as_bytes())?;
        write_atomic(&file, &png)?;
    }
    Ok(file)
}

fn set(s: &mut Session, label: &str, node: NodeId, port: String, value: String) -> Result<(), String> {
    let at = PortRef::new(node, port);
    s.edit(label, |g, catalog| Ok(g.set_value_checked(catalog, &at, Value::String(value))?)).map(|_| ()).map_err(|e| e.to_string())
}

/// `mask.stroke`: one stroke on the node's edit, saved as a new file; one undo step.
pub fn stroke(s: &mut Session, p: &Value) -> Result<Value, String> {
    check_fields(p, &["node", "port", "points", "tool", "radius", "hardness", "flow", "density", "value"])?;
    let (node, port, value) = target(s, p)?;
    let spec = masks::parse_stroke(p)?;
    let st = step(s, &node)?;
    let mut canvas = current(s, &value, &st)?;
    canvas.paint(&spec);
    let (w, h) = (canvas.w, canvas.h);
    let new = if canvas.is_unchanged(&st.start()) {
        String::new()
    } else {
        let file = store(canvas, &node, &edits_folder(s)?)?;
        // relative to the document's folder when it lies inside it, else absolute
        match doc_folder(s) {
            Some(dir) => paths::relative_to(&file, &dir),
            None => file.to_string_lossy().into_owned(),
        }
    };
    if new != value {
        set(s, "Mask Stroke", node, port, new.clone())?;
    }
    Ok(json!({"value": new, "w": w, "h": h}))
}

/// `mask.clear`: no edit; one undo step (none when there was none).
pub fn clear(s: &mut Session, p: &Value) -> Result<Value, String> {
    check_fields(p, &["node", "port"])?;
    let (node, port, value) = target(s, p)?;
    if !value.trim().is_empty() {
        set(s, "Clear Mask", node, port, String::new())?;
    }
    Ok(json!({"value": ""}))
}

/// `mask.get`: the edit as it stands: its value, its file (absolute; null with none) and the
/// mask's size from the last run (null before one).
pub fn get(s: &Session, p: &Value) -> Result<Value, String> {
    check_fields(p, &["node", "port"])?;
    let (node, _, value) = target(s, p)?;
    let file = (!value.trim().is_empty()).then(|| resolve(value.trim(), s).to_string_lossy().into_owned());
    let size = s.run.node(&node.0).and_then(|r| r.editor.get(KIND)).map(|d| (d.get("w").cloned(), d.get("h").cloned()));
    let (w, h) = size.unwrap_or((None, None));
    Ok(json!({"value": value, "file": file, "w": w, "h": h}))
}

/// `mask.folder {folder}`: where this document's edit files go when they can't go beside it;
/// one undo step. An empty folder forgets it.
pub fn folder(s: &mut Session, p: &Value) -> Result<Value, String> {
    check_fields(p, &["folder"])?;
    let f = p.get("folder").and_then(Value::as_str).ok_or("\"folder\" is required: a folder's path, or \"\" to forget it")?.trim().to_owned();
    if !f.is_empty() {
        let dir = paths::expand_user(&f);
        if !dir.is_dir() {
            return Err(format!("{f} isn't a folder"));
        }
        probe(&dir).map_err(|e| format!("{f} can't be written: {e}"))?;
    }
    let same = s.graph().meta.get(FOLDER_META).and_then(Value::as_str).unwrap_or_default() == f;
    if !same {
        let v = f.clone();
        s.edit("Mask Folder", move |g, _| {
            if v.is_empty() {
                g.meta.remove(FOLDER_META);
            } else {
                g.meta.insert(FOLDER_META.into(), Value::String(v));
            }
            Ok(())
        })
        .map_err(|e| e.to_string())?;
    }
    Ok(json!({"folder": f}))
}

// ---------- pruning (the maintainer, 2026-11-23: decision 5B, "Ref commonkit -- both") ----------

/// An edit file's name: `<node>-<16 hex>.png`.
pub const EDIT_FILE_PATTERN: &str = r"[A-Za-z0-9_.-]+-[0-9a-f]{16}\.png";
/// How old an unreachable edit file must be before it goes: another window may hold it in an
/// undo history this one can't see.
pub const PRUNE_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

fn names_in(v: &Value, out: &mut std::collections::BTreeSet<String>) {
    match v {
        Value::String(t) => {
            for line in t.lines() {
                if let Some(n) = Path::new(line.trim()).file_name().and_then(|n| n.to_str()).filter(|n| n.ends_with(".png")) {
                    out.insert(n.to_owned());
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| names_in(x, out)),
        Value::Object(o) => o.values().for_each(|x| names_in(x, out)),
        _ => {}
    }
}

/// The edit files something can still reach: named by the graph, by any graph undo or redo can
/// bring back, or by a JSON file beside the document (the saved graph, a Save As sharing the
/// folder), as file names.
pub fn reachable(s: &Session) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for g in std::iter::once(s.graph()).chain(s.history()) {
        for n in g.nodes.values() {
            for v in n.values.values() {
                names_in(v, &mut out);
            }
        }
    }
    if let Some(folder) = doc_folder(s)
        && let Ok(entries) = std::fs::read_dir(&folder)
    {
        for e in entries.flatten().take(10_000) {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json")
                && e.metadata().is_ok_and(|m| m.is_file() && m.len() < 64 << 20)
                && let Ok(text) = std::fs::read_to_string(&p)
            {
                {
                    for w in text.split(|c: char| c == '"' || c == '/' || c == '\\' || c.is_whitespace()) {
                        if w.ends_with(".png") {
                            out.insert(w.to_owned());
                        }
                    }
                }
            }
        }
    }
    out
}

/// The folders this document's edit files may be in, that exist: the one chosen for it
/// (`mask.folder`) and `<document>.edits` beside it. Pruning looks nowhere else.
pub fn edit_folders(s: &Session) -> Vec<PathBuf> {
    let chosen = s.graph().meta.get(FOLDER_META).and_then(Value::as_str).filter(|f| !f.trim().is_empty()).map(paths::expand_user);
    let beside = s.path().and_then(|p| Some(p.with_file_name(format!("{}.edits", p.file_stem()?.to_str()?))));
    let mut out: Vec<PathBuf> = Vec::new();
    for d in chosen.into_iter().chain(beside) {
        if std::fs::symlink_metadata(&d).is_ok() && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// Is `file` this editor's: its sidecar (`<file>.json`) says [`SIDECAR_EDITOR`], for the node
/// its name starts with (`<node>-<hash>.png`)? Another editor's files, and files with no sidecar,
/// are not.
pub fn ours(file: &Path) -> bool {
    let side = PathBuf::from(format!("{}.json", file.display()));
    let Some(name) = file.file_name().and_then(|n| n.to_str()) else { return false };
    let Some(node) = name.rsplit_once('-').map(|(n, _)| n) else { return false };
    let small = std::fs::symlink_metadata(&side).is_ok_and(|m| m.is_file() && m.len() < 64 << 10);
    let Some(meta) = small.then(|| std::fs::read(&side).ok()).flatten().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return false };
    meta.get("editor").and_then(Value::as_str) == Some(SIDECAR_EDITOR) && meta.get("node").and_then(Value::as_str) == Some(node)
}

/// Remove (or with `dry_run` list) the edit files in `folder` nothing reaches: the one place
/// pruning happens, through `commonkit::prune_folder` (commonkit 0.3; the maintainer: "Ref
/// commonkit -- both"). Candidates are named [`EDIT_FILE_PATTERN`] and claimed by [`ours`]; what
/// is in `reachable` stays, as does anything younger than `min_age`; a file's sidecar goes with
/// it. A folder that is a symbolic link, or not a folder, is refused.
pub fn prune_edits(folder: &Path, reachable: &std::collections::BTreeSet<String>, dry_run: bool, min_age: std::time::Duration) -> commonkit::PruneReport {
    let options =
        commonkit::PruneOptions { pattern: EDIT_FILE_PATTERN.into(), keep: reachable.clone(), min_age, companions: vec![".json".into()], dry_run, now: None };
    let owns = |p: &Path| ours(p);
    commonkit::prune_folder(folder, &options, Some(&owns))
}

/// `mask.prune {dry_run?, age_s?}`: Clean Up Mask Files. In this document's edit folders, the
/// mask files nothing reaches and older than `age_s` (a day by default) go, with their sidecars;
/// a dry run (the default) lists them. Removed files are given as a value would name them.
pub fn prune(s: &Session, p: &Value) -> Result<Value, String> {
    check_fields(p, &["dry_run", "age_s"])?;
    let dry_run = match p.get("dry_run") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("dry_run is true or false".into()),
    };
    let min_age = match p.get("age_s") {
        None | Some(Value::Null) => PRUNE_AGE,
        Some(v) => std::time::Duration::from_secs_f64(v.as_f64().filter(|a| a.is_finite() && *a >= 0.0 && *a < 1e12).ok_or("age_s is seconds, 0 or more")?),
    };
    let names = reachable(s);
    let base = doc_folder(s);
    let (mut removed, mut kept, mut young, mut foreign, mut failed, mut refused) = (Vec::new(), 0, 0, 0, 0, Vec::new());
    let folders = edit_folders(s);
    for f in &folders {
        let r = prune_edits(f, &names, dry_run, min_age);
        removed.extend(r.removed.iter().map(|n| {
            let file = f.join(n);
            match &base {
                Some(dir) => paths::relative_to(&file, dir),
                None => file.to_string_lossy().into_owned(),
            }
        }));
        (kept, young, foreign, failed) = (kept + r.kept, young + r.young, foreign + r.foreign, failed + r.failed);
        refused.extend(r.refused);
    }
    Ok(json!({"dry_run": dry_run, "folders": folders, "reachable": names.len(), "removed": removed, "kept": kept, "young": young,
              "foreign": foreign, "failed": failed, "refused": refused}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture_bytes_raw_or_from_an_array_file() {
        assert_eq!(pixels(&[1, 2, 3, 4, 5, 6, 7, 8], 2, 1), Some(vec![[1, 2, 3, 4], [5, 6, 7, 8]]));
        let mut npy = b"\x93NUMPY\x01\x00header....".to_vec();
        npy.extend([9, 9, 9, 9]);
        assert_eq!(pixels(&npy, 1, 1), Some(vec![[9, 9, 9, 9]]));
        assert_eq!(pixels(&[1, 2, 3], 1, 1), None);
        assert_eq!(pixels(&[], usize::MAX, 2), None);
    }
}
