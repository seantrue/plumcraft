//! Workflow bundles: one `.zip` with a workflow and every file it refers to, for archiving and
//! moving (`graph.bundle`; `graph.open` and `graph.insert` read one).
//!
//! **Writing** ([`write_bundle`]):
//! - the workflow goes first, with each path input rewritten to point inside the bundle;
//! - a file inside the workflow's folder keeps its relative path, and any other goes under
//!   `files/`;
//! - each file's `.json` sidecar goes with it;
//! - what can't be carried (a missing file, a folder) is reported and left as it was; a `save`
//!   path names what a node will write, so it isn't touched.
//!
//! **Reading** ([`extract_bundle`]): a bundle is hostile input.
//! - everything is checked before anything is written: entry names stay inside the folder, no
//!   links, and the entry count, depth and declared size are bounded;
//! - declared sizes can lie, so what is unpacked is counted as it comes, and stopped at the bound;
//! - it unpacks into a new folder beside it, never over anything, and a bundle that fails leaves
//!   nothing behind;
//! - nothing in it runs or gets execute permission.
//!
//! Lifted from the research software's `crates/engine/src/bundle.rs` (`8480c03`). Beyond it: `files` values
//! (several paths), links, duplicate names, depth, declared sizes, `../` paths, files copied
//! rather than read whole, and a bundle whose folder has more than one `.json` at its top.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use plumcraft_graph::{Graph, NodeId, resolve_input};
use plumcraft_proto::catalogue::{PathHint, SchemaReply};
use serde_json::Value;
use zip::write::SimpleFileOptions;

use crate::paths::{each_line, path_values, with_absolute_paths};
use crate::{Result, SessionError};

/// The most entries a bundle may have.
pub const MAX_ENTRIES: usize = 10_000;
/// The most bytes a bundle may unpack to.
pub const MAX_UNPACKED: u64 = 4 * 1024 * 1024 * 1024;
/// The most folders an entry name may go down.
pub const MAX_DEPTH: usize = 64;

/// What a bundle may unpack to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub entries: usize,
    pub unpacked: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { entries: MAX_ENTRIES, unpacked: MAX_UNPACKED }
    }
}

fn err(what: impl std::fmt::Display) -> SessionError {
    SessionError::File(what.to_string())
}

/// A file path that names a bundle.
pub fn is_zip(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

/// A path that couldn't go in, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    pub node: NodeId,
    pub port: String,
    pub path: String,
    pub why: &'static str,
}

/// What went into a bundle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundleReport {
    /// Entry names, the workflow first.
    pub entries: Vec<String>,
    pub missing: Vec<Missing>,
}

/// An entry name for `p`: forward slashes.
fn entry_name(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// The name `path` keeps inside the bundle: its place under `base`, else `files/<name>`. A path
/// that leaves `base` by `..` isn't inside it, whatever its text starts with.
fn wanted_name(path: &Path, base: Option<&Path>) -> String {
    let plain = |r: &&Path| r.components().all(|c| matches!(c, std::path::Component::Normal(_))) && !r.as_os_str().is_empty();
    match base.and_then(|b| path.strip_prefix(b).ok()).filter(plain) {
        Some(r) => entry_name(r),
        None => format!("files/{}", path.file_name().map_or("file".into(), |n| n.to_string_lossy().into_owned())),
    }
}

/// `wanted`, or `files/stem-2.ext` and so on: the first name not in `taken`.
fn free_name(wanted: String, taken: &[String]) -> String {
    if !taken.contains(&wanted) {
        return wanted;
    }
    let p = Path::new(&wanted);
    let stem = p.file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned());
    let ext = p.extension().map_or(String::new(), |x| format!(".{}", x.to_string_lossy()));
    (2u32..).map(|k| format!("files/{stem}-{k}{ext}")).find(|e| !taken.contains(e)).unwrap_or(wanted)
}

fn hint(graph: &Graph, catalog: &SchemaReply, node: &NodeId, port: &str) -> Option<PathHint> {
    let n = graph.node(node)?;
    resolve_input(catalog.node(&n.kind)?, n, node, port).ok()?.port.path.clone()
}

/// Files to copy, as (source, entry name).
#[derive(Default)]
struct Plan {
    files: Vec<(PathBuf, String)>,
    taken: Vec<String>,
}

impl Plan {
    /// The entry that holds `path`, which goes in once however often it is used.
    fn entry(&mut self, path: &Path, base: Option<&Path>) -> String {
        if let Some((_, e)) = self.files.iter().find(|(p, _)| p == path) {
            return e.clone();
        }
        let name = free_name(wanted_name(path, base), &self.taken);
        self.taken.push(name.clone());
        self.files.push((path.to_path_buf(), name.clone()));
        name
    }
}

/// Copy `src` into the zip as `name`, a file at a time.
fn add_file(zip: &mut zip::ZipWriter<std::fs::File>, src: &Path, name: &str, opts: SimpleFileOptions) -> std::result::Result<(), String> {
    let mut file = std::fs::File::open(src).map_err(|e| format!("{}: {e}", src.display()))?;
    zip.start_file(name, opts).map_err(|e| e.to_string())?;
    std::io::copy(&mut file, zip).map_err(|e| format!("{}: {e}", src.display()))?;
    Ok(())
}

/// Write `graph` (workflow file name `name`, folder `base`) and the files it refers to into the
/// zip at `zip_path`, atomically.
pub fn write_bundle(graph: &Graph, catalog: &SchemaReply, name: &str, base: Option<&Path>, zip_path: &Path) -> Result<BundleReport> {
    let abs = with_absolute_paths(graph, catalog, base);
    let mut out = graph.clone();
    let mut report = BundleReport { entries: vec![name.to_owned()], ..BundleReport::default() };
    let mut plan = Plan { taken: vec![name.to_owned()], ..Plan::default() };
    for (node, port, text) in path_values(&abs, catalog) {
        if hint(&abs, catalog, &node, &port).is_some_and(|h| h.mode == "save") {
            continue;
        }
        let mut carried = false;
        let lines = |line: &str| {
            let path = crate::paths::expand_user(line);
            let why = if path.is_file() {
                carried = true;
                None
            } else if path.is_dir() {
                Some("a folder isn't carried")
            } else {
                Some("no such file")
            };
            match why {
                Some(why) => {
                    report.missing.push(Missing { node: node.clone(), port: port.clone(), path: line.to_owned(), why });
                    line.to_owned()
                }
                None => plan.entry(&path, base),
            }
        };
        let value = each_line_mut(&text, lines);
        if carried && let Some(n) = out.nodes.get_mut(&node) {
            n.values.insert(port, Value::String(value));
        }
    }
    let json = out.to_json()?;
    let mut size = u64::try_from(json.len()).unwrap_or(u64::MAX);
    for (path, _) in &plan.files {
        size = size.saturating_add(std::fs::metadata(path).map_or(0, |m| m.len()));
    }
    if size > MAX_UNPACKED {
        return Err(err(format!("the workflow and its files come to {size} bytes, more than the {MAX_UNPACKED} a bundle may hold")));
    }
    let dir = zip_path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".{}.tmp-{}", zip_path.file_name().map_or("bundle".into(), |n| n.to_string_lossy().into_owned()), std::process::id()));
    let written = (|| -> std::result::Result<(), String> {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&tmp).map_err(|e| e.to_string())?);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        zip.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        for (path, entry) in &plan.files {
            add_file(&mut zip, path, entry, opts)?;
            report.entries.push(entry.clone());
            let sidecar = PathBuf::from(format!("{}.json", path.display()));
            let side_entry = format!("{entry}.json");
            if sidecar.is_file() && !plan.taken.contains(&side_entry) {
                add_file(&mut zip, &sidecar, &side_entry, opts)?;
                report.entries.push(side_entry);
            }
        }
        let file = zip.finish().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, zip_path).map_err(|e| e.to_string())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(err(format!("can't write the bundle {}: {e}", zip_path.display())));
    }
    Ok(report)
}

/// [`each_line`] for a closure that keeps state.
fn each_line_mut(text: &str, mut f: impl FnMut(&str) -> String) -> String {
    let f = std::cell::RefCell::new(&mut f);
    each_line(text, |l| (*f.borrow_mut())(l))
}

/// A new folder for unpacking `zip_path`: beside it, named after it (`name 2`, … when taken),
/// made here, so it can't be someone else's.
fn fresh_folder(zip_path: &Path) -> Result<PathBuf> {
    let parent = zip_path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let stem = zip_path.file_stem().map_or("bundle".into(), |s| s.to_string_lossy().into_owned());
    for k in 1..1000 {
        let dest = if k == 1 { parent.join(&stem) } else { parent.join(format!("{stem} {k}")) };
        match std::fs::create_dir(&dest) {
            Ok(()) => return Ok(dest),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(err(format!("can't make a folder beside {}: {e}", zip_path.display()))),
        }
    }
    Err(err(format!("no free folder name beside {}", zip_path.display())))
}

/// Is `name` a top-level `.json` that isn't another entry's sidecar?
fn is_workflow(name: &Path, names: &[PathBuf]) -> bool {
    let top = name.components().count() == 1;
    let json = name.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"));
    top && json && !names.iter().any(|n| *n == name.with_extension(""))
}

/// Check every entry, before anything is written. Returns the workflow's name in the bundle.
fn survey<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, limits: Limits) -> std::result::Result<PathBuf, String> {
    if zip.len() > limits.entries {
        return Err(format!("it has more than {} entries", limits.entries));
    }
    let mut names = Vec::with_capacity(zip.len());
    let mut declared: u64 = 0;
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).map_err(|e| e.to_string())?;
        let Some(name) = entry.enclosed_name() else {
            return Err(format!("entry {:?} leaves its folder", entry.name().chars().take(200).collect::<String>()));
        };
        if entry.is_symlink() {
            return Err(format!("entry {} is a link", name.display()));
        }
        if name.components().count() > MAX_DEPTH {
            return Err(format!("an entry goes more than {MAX_DEPTH} folders deep"));
        }
        declared = declared.saturating_add(entry.size());
        if declared > limits.unpacked {
            return Err(format!("it unpacks to more than {} bytes", limits.unpacked));
        }
        names.push(name);
    }
    let workflows: Vec<&PathBuf> = names.iter().filter(|n| is_workflow(n, &names)).collect();
    // A bundle written here has its workflow first; beside it may be any data.
    let first = names.first().filter(|n| is_workflow(n, &names)).cloned();
    match (first, workflows.as_slice()) {
        (Some(w), _) => Ok(w),
        (None, [one]) => Ok((*one).clone()),
        (None, some) => Err(format!("expected one workflow (.json) at the top, found {}", some.len())),
    }
}

/// Unpack every entry into `dest`, counting what really comes out.
fn unpack<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, dest: &Path, limits: Limits) -> std::result::Result<(), String> {
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.enclosed_name().ok_or("an entry leaves its folder")?;
        let target = dest.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // never over a file: an earlier entry of the same name is a malformed bundle
        let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| format!("{}: {e}", name.display()))?;
        let left = limits.unpacked.saturating_sub(total);
        let copied = std::io::copy(&mut (&mut entry).take(left.saturating_add(1)), &mut out).map_err(|e| format!("{}: {e}", name.display()))?;
        total = total.saturating_add(copied);
        if total > limits.unpacked {
            return Err(format!("it unpacks to more than {} bytes", limits.unpacked));
        }
    }
    Ok(())
}

/// Unpack the bundle at `zip_path` into a new folder beside it. Returns the workflow's path.
pub fn extract_bundle(zip_path: &Path) -> Result<PathBuf> {
    extract_within(zip_path, Limits::default())
}

/// [`extract_bundle`] with `limits`.
pub fn extract_within(zip_path: &Path, limits: Limits) -> Result<PathBuf> {
    let file = std::fs::File::open(zip_path).map_err(|e| err(format!("can't open {}: {e}", zip_path.display())))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| err(format!("{} is not a bundle: {e}", zip_path.display())))?;
    let workflow = survey(&mut zip, limits).map_err(|e| err(format!("{} can't be opened: {e}", zip_path.display())))?;
    let dest = fresh_folder(zip_path)?;
    if let Err(e) = unpack(&mut zip, &dest, limits) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(err(format!("can't unpack {}: {e}", zip_path.display())));
    }
    Ok(dest.join(workflow))
}

/// Read the workflow a path names: a workflow file, or a bundle, unpacked beside itself. The
/// path it is then at, and the bundle it came from. A bundle whose workflow can't be read leaves
/// no folder.
pub(crate) fn read_workflow_or_bundle(path: PathBuf) -> Result<(Graph, PathBuf, Option<PathBuf>)> {
    if !is_zip(&path) {
        return Ok((crate::file_cmds::read_workflow(&path)?, path, None));
    }
    let workflow = extract_bundle(&path)?;
    match crate::file_cmds::read_workflow(&workflow) {
        Ok(graph) => Ok((graph, workflow, Some(path))),
        Err(e) => {
            if let Some(dest) = workflow.parent() {
                let _ = std::fs::remove_dir_all(dest);
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_graph::PortRef;
    use serde_json::json;

    fn catalog() -> SchemaReply {
        serde_json::from_value(json!({"nodes": [
            {"id": "t.load", "display": "Load", "inputs": [{"name": "path", "type": "STRING", "default": "", "path": {"mode": "open"}}], "outputs": []},
            {"id": "t.many", "display": "Many", "inputs": [{"name": "paths", "type": "STRING", "default": "", "path": {"mode": "files"}}], "outputs": []},
            {"id": "t.dir", "display": "Dir", "inputs": [{"name": "folder", "type": "STRING", "default": "", "path": {"mode": "dir"}}], "outputs": []},
            {"id": "t.save", "display": "Save", "inputs": [{"name": "to", "type": "STRING", "default": "", "path": {"mode": "save"}}], "outputs": []},
        ]}))
        .unwrap()
    }

    fn folder() -> PathBuf {
        let d = plumcraft_scratch::dir("archive");
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A workflow folder with a picture outside it and an edit (and sidecar) inside it.
    fn setup() -> (PathBuf, Graph, PathBuf) {
        let root = folder();
        let flows = root.join("flows");
        let pics = root.join("pictures");
        std::fs::create_dir_all(flows.join("a.edits")).unwrap();
        std::fs::create_dir_all(&pics).unwrap();
        std::fs::write(pics.join("corgi.jpg"), b"jpeg bytes").unwrap();
        std::fs::write(flows.join("a.edits/n2.png"), b"png bytes").unwrap();
        std::fs::write(flows.join("a.edits/n2.png.json"), br#"{"call": "grad(0)"}"#).unwrap();
        let c = catalog();
        let mut g = Graph::new();
        let load = g.add_node(c.node("t.load").unwrap(), [0.0, 0.0]);
        let edit = g.add_node(c.node("t.load").unwrap(), [0.0, 0.0]);
        let gone = g.add_node(c.node("t.load").unwrap(), [0.0, 0.0]);
        g.set_value(&PortRef::new(load, "path"), json!(pics.join("corgi.jpg"))).unwrap();
        g.set_value(&PortRef::new(edit, "path"), json!("a.edits/n2.png")).unwrap();
        g.set_value(&PortRef::new(gone, "path"), json!("/no/such/picture.png")).unwrap();
        (root, g, flows)
    }

    #[test]
    fn a_bundle_carries_the_workflow_and_its_files_and_opens_elsewhere() {
        let (root, g, flows) = setup();
        let c = catalog();
        let zip_path = root.join("a.zip");
        let report = write_bundle(&g, &c, "a.json", Some(&flows), &zip_path).unwrap();
        assert_eq!(report.entries, ["a.json", "files/corgi.jpg", "a.edits/n2.png", "a.edits/n2.png.json"]);
        assert_eq!(report.missing.len(), 1, "the missing picture is reported");
        assert_eq!(report.missing[0].why, "no such file");
        // moved away from the originals, its paths still lead to the bytes
        std::fs::remove_dir_all(root.join("pictures")).unwrap();
        std::fs::remove_dir_all(&flows).unwrap();
        let wf = extract_bundle(&zip_path).unwrap();
        assert_eq!(wf, root.join("a/a.json"));
        let back = crate::file_cmds::read_workflow(&wf).unwrap();
        let abs = with_absolute_paths(&back, &c, wf.parent());
        let mut read: Vec<Vec<u8>> =
            path_values(&abs, &c).iter().filter(|(_, _, t)| !t.starts_with("/no/such")).map(|(_, _, t)| std::fs::read(t).unwrap()).collect();
        read.sort();
        assert_eq!(read, [b"jpeg bytes".to_vec(), b"png bytes".to_vec()]);
        assert!(root.join("a/a.edits/n2.png.json").is_file(), "the sidecar came too");
        assert_eq!(extract_bundle(&zip_path).unwrap(), root.join("a 2/a.json"), "a second unpack takes a new folder");
    }

    #[test]
    fn a_files_value_is_carried_line_by_line_and_folders_and_save_paths_are_left() {
        let root = folder();
        let flows = root.join("flows");
        std::fs::create_dir_all(flows.join("sub")).unwrap();
        for f in ["one.txt", "sub/two.txt"] {
            std::fs::write(flows.join(f), f).unwrap();
        }
        std::fs::write(root.join("two.txt"), b"another two").unwrap();
        let c = catalog();
        let mut g = Graph::new();
        let many = g.add_node(c.node("t.many").unwrap(), [0.0, 0.0]);
        let dir = g.add_node(c.node("t.dir").unwrap(), [0.0, 0.0]);
        let save = g.add_node(c.node("t.save").unwrap(), [0.0, 0.0]);
        g.set_value(&PortRef::new(many.clone(), "paths"), json!("one.txt\nsub/two.txt\n../two.txt\n../gone.txt")).unwrap();
        g.set_value(&PortRef::new(dir.clone(), "folder"), json!("sub")).unwrap();
        g.set_value(&PortRef::new(save.clone(), "to"), json!("out.png")).unwrap();
        let zip_path = root.join("b.zip");
        let report = write_bundle(&g, &c, "b.json", Some(&flows), &zip_path).unwrap();
        assert_eq!(report.entries, ["b.json", "one.txt", "sub/two.txt", "files/two.txt"], "a ../ path isn't inside, and goes under files/");
        assert_eq!(report.missing.iter().map(|m| m.why).collect::<Vec<_>>(), ["no such file", "a folder isn't carried"]);
        let wf = extract_bundle(&zip_path).unwrap();
        let back = crate::file_cmds::read_workflow(&wf).unwrap();
        assert_eq!(back.nodes[&save].values["to"], json!("out.png"));
        let lines: Vec<String> = back.nodes[&many].values["paths"].as_str().unwrap().lines().map(str::to_owned).collect();
        assert_eq!(lines[..3], ["one.txt", "sub/two.txt", "files/two.txt"]);
        assert_eq!(std::fs::read(wf.parent().unwrap().join("files/two.txt")).unwrap(), b"another two");
    }

    #[test]
    fn two_files_of_one_name_both_go_in_and_a_file_used_twice_goes_in_once() {
        let root = folder();
        for d in ["x", "y"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
            std::fs::write(root.join(d).join("p.png"), d).unwrap();
        }
        let c = catalog();
        let mut g = Graph::new();
        for f in ["x/p.png", "y/p.png", "x/p.png"] {
            let n = g.add_node(c.node("t.load").unwrap(), [0.0, 0.0]);
            g.set_value(&PortRef::new(n, "path"), json!(root.join(f))).unwrap();
        }
        let report = write_bundle(&g, &c, "c.json", None, &root.join("c.zip")).unwrap();
        assert_eq!(report.entries, ["c.json", "files/p.png", "files/p-2.png"]);
    }
}
