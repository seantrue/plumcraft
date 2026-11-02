//! Documents in their app (plan 10.6, M3, "Open in VectorCraft"): `doc.open` opens a copy of a
//! node's document value in the craft's desktop app, as `<app> <file>`.
//!
//! - **Which apps.** The host gives the session its [`DocApp`]s from the craft manifests it
//!   trusts (the editor reads them; an engine's messages never name a program). A document value
//!   names its app (`Form::Doc{app}`), and a node's output its wire type; either picks the app.
//! - **Which value.** The watched display's `full`, else the latest reply's output, when the node
//!   isn't stale. Otherwise the command watches that output and runs the graph (the memo makes
//!   it cheap), and the value opens when it arrives ([`poll_doc_open`], called by the host
//!   every frame): the command's reply then says `pending`.
//! - **A copy, verified.** The value must be a file on this machine (a `path` locator: a remote
//!   engine's documents can't be opened, and the command says so). Its bytes are checked against
//!   the handle while they are copied to `<cache>/plumcraft/open/<node>/<title>-<digest8>.<ext>`,
//!   so edits made in the app never touch the engine's CAS, whose file names the handle. A copy
//!   the app has changed is kept, and the next open gets a new name.
//! - **A missing app** is an error naming the path and the fix, never a panic. The app is
//!   started detached, and a thread reaps it when it exits.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use md5::{Digest, Md5};
use plumcraft_graph::NodeId;
use plumcraft_proto::catalogue::Media;
use plumcraft_proto::data::{DataRef, Form, Locator, PortValue};
use plumcraft_proto::display::WatchRequest;
use plumcraft_proto::handle::{Algo, Recipe};
use serde::Serialize;
use serde_json::{Value, json};

use crate::commands::{CommandSpec, opt_str_param};
use crate::{Result, Session, SessionError};

/// The largest document copied (4 GiB).
pub const MAX_DOC_BYTES: u64 = 4 << 30;
/// The longest file-name stem a copy gets from a node's title.
const MAX_STEM: usize = 60;

/// Starts `program args… file`: the default spawns it; tests record the call instead.
pub type Launch = Arc<dyn Fn(&Path, &[String], &Path) -> std::result::Result<(), String> + Send + Sync>;

/// A desktop app that opens documents of one craft.
#[derive(Debug, Clone)]
pub struct DocApp {
    /// The `Form::Doc{app}` its documents carry (`vectorcraft`).
    pub app: String,
    /// The wire type of its documents (`vectorcraft.doc`).
    pub wire: String,
    /// "VectorCraft", as in "Open in VectorCraft".
    pub label: String,
    /// The extension a copy gets (`vectorcraft`).
    pub ext: String,
    /// The program, or why there is none (with the fix).
    pub program: std::result::Result<PathBuf, String>,
    /// Arguments before the file.
    pub args: Vec<String>,
}

/// A document opened in its app.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct Opened {
    pub node: String,
    pub port: String,
    pub app: String,
    /// The copy the app opened.
    pub path: PathBuf,
    pub handle: String,
    /// The copy was already there, unchanged.
    pub reused: bool,
}

/// The session's document apps, where copies go, and an open waiting for its value.
#[derive(Clone)]
pub struct DocApps {
    pub apps: Vec<DocApp>,
    /// Where copies go (default [`default_root`]).
    pub root: PathBuf,
    pub(crate) launch: Launch,
    pending: Option<(String, String)>,
    /// The latest open, or why it failed.
    pub last: Option<std::result::Result<Opened, String>>,
    /// `value.open` (Open, Open with…): an open waiting for its value, the latest outcome, the
    /// apps chosen with Open with… (most recent first), and how to read a value that isn't a
    /// file here (the host's engine link sets it).
    pub(crate) pending_value: Option<crate::open_cmds::PendingOpen>,
    pub last_value: Option<std::result::Result<Opened, String>>,
    pub recent_apps: Vec<PathBuf>,
    pub resolver: Option<Resolver>,
}

/// Reads a value's bytes, checked against its handle (the host's engine link).
pub type Resolver = Arc<dyn Fn(&DataRef) -> Option<Vec<u8>> + Send + Sync>;

impl std::fmt::Debug for DocApps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocApps").field("apps", &self.apps).field("root", &self.root).field("pending", &self.pending).finish_non_exhaustive()
    }
}

impl Default for DocApps {
    fn default() -> Self {
        DocApps {
            apps: Vec::new(),
            root: default_root(),
            launch: Arc::new(spawn_app),
            pending: None,
            last: None,
            pending_value: None,
            last_value: None,
            recent_apps: Vec::new(),
            resolver: None,
        }
    }
}

impl DocApps {
    pub fn new(apps: Vec<DocApp>) -> Self {
        DocApps { apps, ..Default::default() }
    }

    /// Start apps with `launch` instead of spawning them (tests).
    pub fn with_launcher(mut self, launch: Launch) -> Self {
        self.launch = launch;
        self
    }

    pub fn with_root(mut self, root: PathBuf) -> Self {
        self.root = root;
        self
    }

    /// The app for documents of wire type `wire`.
    pub fn for_wire(&self, wire: &str) -> Option<&DocApp> {
        self.apps.iter().find(|a| a.wire == wire)
    }

    /// The (node, port) whose value an open is waiting for.
    pub fn pending(&self) -> Option<(&str, &str)> {
        self.pending.as_ref().map(|(n, p)| (n.as_str(), p.as_str()))
    }

    /// As `ui.inspect`'s `docs`.
    pub fn inspect(&self) -> Value {
        let apps: Vec<Value> = self
            .apps
            .iter()
            .map(|a| json!({"app": a.app, "wire": a.wire, "label": a.label, "program": a.program.as_ref().ok(), "missing": a.program.as_ref().err()}))
            .collect();
        let last = match &self.last {
            Some(Ok(o)) => json!({"ok": true, "opened": o}),
            Some(Err(e)) => json!({"ok": false, "error": e}),
            None => Value::Null,
        };
        let last_value = match &self.last_value {
            Some(Ok(o)) => json!({"ok": true, "opened": o}),
            Some(Err(e)) => json!({"ok": false, "error": e}),
            None => Value::Null,
        };
        json!({"apps": apps, "root": self.root, "pending": self.pending().map(|(n, p)| json!({"node": n, "port": p})), "last": last,
               "value": {"pending": self.pending_value.as_ref().map(|p| json!({"node": p.node, "port": p.port, "app": p.app})), "last": last_value, "recent_apps": self.recent_apps}})
    }
}

/// `<cache>/plumcraft/open`: `~/Library/Caches` on macOS, `%LOCALAPPDATA%` on Windows, else
/// `$XDG_CACHE_HOME` or `~/.cache`.
pub fn default_root() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let home = env("HOME").or_else(|| env("USERPROFILE")).unwrap_or_else(std::env::temp_dir);
    let cache = if cfg!(target_os = "macos") {
        home.join("Library/Caches")
    } else if cfg!(windows) {
        env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"))
    } else {
        env("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"))
    };
    cache.join("plumcraft").join("open")
}

/// Start `program args… file` detached, its output to `<file>.log`; a thread reaps it.
pub fn spawn_app(program: &Path, args: &[String], file: &Path) -> std::result::Result<(), String> {
    let mut log = file.as_os_str().to_owned();
    log.push(".log");
    let log = std::fs::OpenOptions::new().create(true).append(true).open(PathBuf::from(log)).ok();
    let mut cmd = std::process::Command::new(program);
    cmd.args(args).arg(file).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null());
    match log {
        Some(l) => cmd.stderr(l),
        None => cmd.stderr(std::process::Stdio::null()),
    };
    // Its own process group: a Ctrl-C to the editor's terminal doesn't close the app.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", program.display()))?;
    std::thread::Builder::new().name("plumcraft-doc-app".into()).spawn(move || child.wait()).map_err(|e| e.to_string())?;
    Ok(())
}

/// What `doc.open` acts on: the app, the node and port, and the value when it is at hand (the
/// node isn't stale and a display or reply carried it).
#[derive(Debug, Clone)]
pub struct DocTarget {
    pub node: String,
    pub port: String,
    pub app: DocApp,
    pub value: Option<DataRef>,
}

/// The output of `node` `doc.open` would open: `port`, else its first output an app opens.
pub fn doc_target(s: &Session, node: &str, port: Option<&str>) -> std::result::Result<DocTarget, String> {
    let id = NodeId(node.to_owned());
    let n = s.graph().node(&id).ok_or_else(|| format!("no node {node:?}"))?;
    let def = s.catalog().node(&n.kind).ok_or_else(|| format!("{node}: the engine has no node type {:?}", n.kind))?;
    let docs = |ty: &str| s.catalog().wire_type(ty).is_some_and(|w| w.media == Media::Doc);
    let out = match port {
        Some(p) => def.output(p).ok_or_else(|| format!("{node} has no output {p:?}"))?,
        None => def.outputs.iter().find(|o| s.docs.for_wire(&o.ty).is_some()).ok_or_else(|| format!("{node} has no document output"))?,
    };
    let app = s.docs.for_wire(&out.ty).cloned().ok_or_else(|| {
        if docs(&out.ty) { format!("no app opens {} documents", out.ty) } else { format!("{node}.{} is {}, not a document", out.name, out.ty) }
    })?;
    let fresh = s.run.node(node).is_some_and(|r| !r.is_stale() && !r.is_active());
    let value = fresh.then(|| value_of(s, node, &out.name)).flatten();
    Ok(DocTarget { node: node.to_owned(), port: out.name.clone(), app, value })
}

/// The value of `node.port` the session has: a watched display's `full`, else the latest
/// reply's output.
fn value_of(s: &Session, node: &str, port: &str) -> Option<DataRef> {
    let watched =
        s.run.watch.iter().zip(&s.run.watched).find(|(w, _)| w.node == node && w.port == port).and_then(|(_, d)| d.as_ref()).and_then(|d| d.full.clone());
    watched.or_else(|| match s.run.last_reply.as_ref()?.nodes.get(node)?.outputs.get(port)? {
        PortValue::Blob { data } => Some(data.clone()),
        _ => None,
    })
}

/// Why `d` can't be opened here, or the file it is.
pub fn local_file(d: &DataRef, app: &DocApp) -> std::result::Result<PathBuf, String> {
    if let Form::Doc { app: a } = &d.meta.form
        && a != &app.app
    {
        return Err(format!("this is a {a} document, not {}'s", app.label));
    }
    d.at.iter()
        .find_map(|l| match l {
            Locator::Path { path, offset: 0 } => Some(PathBuf::from(path)),
            _ => None,
        })
        .ok_or_else(|| remote(app))
}

fn remote(app: &DocApp) -> String {
    format!("the document is on a remote engine (no file on this machine): Open in {} needs the engine here", app.label)
}

/// Whether `d` is on this machine: a whole file, or published to the commonkit store (a
/// `commonkit` locator, or a `path` into a store segment). A `url` or inline value is not.
fn on_this_machine(d: &DataRef) -> bool {
    d.at.iter().any(|l| matches!(l, Locator::Path { .. } | Locator::Commonkit { .. }))
}

/// Why `d` can't be opened here: it isn't `app`'s, or it is on a remote engine.
fn openable(d: &DataRef, app: &DocApp) -> std::result::Result<(), String> {
    if let Form::Doc { app: a } = &d.meta.form
        && a != &app.app
    {
        return Err(format!("this is a {a} document, not {}'s", app.label));
    }
    if on_this_machine(d) { Ok(()) } else { Err(remote(app)) }
}

/// Why `doc.open` can't open `node.port` now (the Watched panel's button and the node menu).
pub fn open_blocker(s: &Session, node: &str, port: Option<&str>) -> std::result::Result<DocTarget, String> {
    let t = doc_target(s, node, port)?;
    t.app.program.as_ref().map_err(|e| format!("{} isn't installed: {e}", t.app.label))?;
    match &t.value {
        Some(d) => openable(d, &t.app).map(|()| t),
        None if s.connected => Ok(t),
        None => Err("not run yet, and the engine is not connected".into()),
    }
}

/// A file-name piece from text: letters, digits, `-`, `_` and `.`, nothing else, never empty.
pub(crate) fn safe(text: &str, max: usize) -> String {
    let s: String = text.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(max).collect();
    let s = s.trim_matches('_').to_owned();
    if s.is_empty() { "document".into() } else { s }
}

/// The MD5 of a file, read at most `MAX_DOC_BYTES`, written to `to` as it is read when given.
pub(crate) fn hash_copy(from: &Path, mut to: Option<&mut std::fs::File>) -> std::result::Result<String, String> {
    let mut f = std::fs::File::open(from).map_err(|e| format!("{}: {e}", from.display()))?;
    let mut h = Md5::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("{}: {e}", from.display()))?;
        let Some(chunk) = buf.get(..n).filter(|c| !c.is_empty()) else { break };
        h.update(chunk);
        if let Some(t) = to.as_deref_mut() {
            t.write_all(chunk).map_err(|e| format!("copying: {e}"))?;
        }
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Copy `d` (a file on this machine, checked against its handle) to
/// `<root>/<node>/<title>-<digest8>.<ext>`. An unchanged copy there is reused; one the app has
/// changed is kept, and this copy gets the next free name (`-2`, `-3`, …).
///
/// A document published to the commonkit store is read through `resolver` (the host's engine
/// link) and checked the same way.
pub fn copy_for_app(
    d: &DataRef,
    app: &DocApp,
    root: &Path,
    node: &str,
    title: &str,
    resolver: Option<&Resolver>,
) -> std::result::Result<(PathBuf, bool), String> {
    openable(d, app)?;
    if let Ok(src) = local_file(d, app) {
        return copy_value(d, CopyFrom::File(&src), &app.ext, root, node, title);
    }
    let read = resolver.ok_or("the document is in the store, and there is no engine link to read it through")?;
    let bytes = read(d).ok_or_else(|| "the document is no longer in the store; run it again".to_owned())?;
    copy_value(d, CopyFrom::Bytes(&bytes), &app.ext, root, node, title)
}

/// Where a copy's bytes come from: a file on this machine, or bytes already read (and checked
/// by whoever read them; they are checked again here).
pub enum CopyFrom<'a> {
    File(&'a Path),
    Bytes(&'a [u8]),
}

/// Copy a value's bytes to `<root>/<node>/<title>-<digest8>.<ext>`, checked against its
/// `md5:raw:` handle as they are written. An unchanged copy there is reused; a changed one is
/// kept, and this copy gets the next free name (`-2`, `-3`, …).
pub fn copy_value(d: &DataRef, from: CopyFrom<'_>, ext: &str, root: &Path, node: &str, title: &str) -> std::result::Result<(PathBuf, bool), String> {
    let handle = d.handle.as_ref().ok_or("the value has no handle to check it by")?;
    if handle.algo() != Algo::Md5 || handle.recipe() != Recipe::Raw {
        return Err(format!("{handle}: only md5:raw: values can be checked"));
    }
    if let CopyFrom::File(src) = from {
        let meta = std::fs::metadata(src).map_err(|e| format!("{}: {e}", src.display()))?;
        if !meta.is_file() || meta.len() > MAX_DOC_BYTES {
            return Err(format!("{}: not a file of at most {MAX_DOC_BYTES} bytes", src.display()));
        }
    }
    let digest = handle.digest();
    let dir = root.join(safe(node, MAX_STEM));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stem = format!("{}-{}", safe(title, MAX_STEM), digest.get(..8).unwrap_or(digest));
    let ext = safe(ext, 16);
    for n in 1..=100u32 {
        let name = if n == 1 { format!("{stem}.{ext}") } else { format!("{stem}-{n}.{ext}") };
        let dest = dir.join(name);
        if dest.exists() {
            if hash_copy(&dest, None).is_ok_and(|h| h == digest) {
                let _ = std::fs::File::options().append(true).open(&dest).and_then(|f| f.set_modified(std::time::SystemTime::now()));
                return Ok((dest, true));
            }
            continue; // changed in the app: keep it
        }
        let tmp = dir.join(format!(".{stem}.{}.tmp", std::process::id()));
        let mut out = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let got = match from {
            CopyFrom::File(src) => hash_copy(src, Some(&mut out)),
            CopyFrom::Bytes(b) => out.write_all(b).map_err(|e| format!("copying: {e}")).map(|()| Md5::digest(b).iter().map(|x| format!("{x:02x}")).collect()),
        };
        let got = got.and_then(|h| out.sync_all().map(|()| h).map_err(|e| e.to_string()));
        drop(out);
        let what = match from {
            CopyFrom::File(src) => src.display().to_string(),
            CopyFrom::Bytes(_) => "the value read".to_owned(),
        };
        let checked = got.and_then(|h| if h == digest { Ok(()) } else { Err(format!("{what} no longer holds {handle}")) });
        if let Err(e) = checked.and_then(|()| std::fs::rename(&tmp, &dest).map_err(|e| format!("{}: {e}", dest.display()))) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        return Ok((dest, false));
    }
    Err(format!("{}: 100 changed copies already; remove some", dir.display()))
}

/// Remove copies under `root` that nobody changed and nobody opened for `ttl` (the commonkit
/// tree's TTL): the store or a rerun gives them back. A copy the app changed (its bytes no
/// longer match the digest in its name) is the user's work and is never removed. Returns how
/// many went.
pub fn sweep_copies(root: &Path, ttl: std::time::Duration, now: std::time::SystemTime) -> usize {
    let Some(cutoff) = now.checked_sub(ttl) else { return 0 };
    let mut gone = 0;
    for dir in std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        for file in std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()) {
            let old = std::fs::symlink_metadata(&file).is_ok_and(|m| m.is_file() && m.modified().is_ok_and(|t| t < cutoff));
            let Some(digest8) = file.file_stem().and_then(|s| s.to_str()).and_then(copy_digest) else { continue };
            if old && hash_copy(&file, None).is_ok_and(|h| h.starts_with(&digest8)) && std::fs::remove_file(&file).is_ok() {
                let mut log = file.clone().into_os_string();
                log.push(".log");
                let _ = std::fs::remove_file(log);
                gone += 1;
            }
        }
        let _ = std::fs::remove_dir(&dir); // only when empty
    }
    gone
}

/// [`sweep_copies`] with the commonkit tree's TTL, before a new copy is made.
pub(crate) fn sweep_stale_copies(root: &Path) {
    let policy = plumcraft_store::tree_policy(&plumcraft_store::default_root());
    if policy.ttl_s.is_finite() && policy.ttl_s > 0.0 {
        sweep_copies(root, std::time::Duration::from_secs_f64(policy.ttl_s.min(1e9)), std::time::SystemTime::now());
    }
}

/// The digest piece of a copy's stem: `<title>-<digest8>` or `<title>-<digest8>-<n>`.
fn copy_digest(stem: &str) -> Option<String> {
    let hex = |p: &str| p.len() == 8 && p.bytes().all(|b| b.is_ascii_hexdigit());
    let mut parts = stem.rsplit('-');
    let last = parts.next()?;
    if hex(last) {
        return Some(last.to_owned());
    }
    let prev = parts.next()?;
    (last.parse::<u32>().is_ok() && hex(prev)).then(|| prev.to_owned())
}

/// Copy the value and start the app on the copy.
fn open_now(s: &mut Session, t: &DocTarget, d: &DataRef) -> std::result::Result<Opened, String> {
    let program = t.app.program.clone().map_err(|e| format!("{} isn't installed: {e}", t.app.label))?;
    let title = crate::node_title(s.graph(), s.catalog(), &NodeId(t.node.clone()));
    sweep_stale_copies(&s.docs.root);
    let (path, reused) = copy_for_app(d, &t.app, &s.docs.root, &t.node, &title, s.docs.resolver.as_ref())?;
    (s.docs.launch)(&program, &t.app.args, &path).map_err(|e| format!("starting {}: {e}", t.app.label))?;
    let handle = d.handle.as_ref().map(ToString::to_string).unwrap_or_default();
    Ok(Opened { node: t.node.clone(), port: t.port.clone(), app: t.app.label.clone(), path, handle, reused })
}

/// Finish an open waiting for its value: once the watched display for it arrives, open it (or
/// say why not). `None` while there is nothing to report. The host calls this every frame.
pub fn poll_doc_open(s: &mut Session) -> Option<std::result::Result<Opened, String>> {
    let (node, port) = s.docs.pending.clone()?;
    let display = s.run.watch.iter().zip(&s.run.watched).find(|(w, _)| w.node == node && w.port == port).and_then(|(_, d)| d.clone());
    let outcome = match display {
        Some(d) if d.error.is_some() => Err(format!("{node}.{port}: {}", d.error.map(|e| e.message).unwrap_or_default())),
        Some(d) if d.full.is_some() => match (doc_target(s, &node, Some(&port)), d.full) {
            (Ok(t), Some(full)) => open_now(s, &t, &full),
            (Ok(_), None) => Err(format!("{node}.{port}: no value")),
            (Err(e), _) => Err(e),
        },
        _ if s.run.running => return None,
        _ => Err(s.run.run_error.clone().map_or_else(|| format!("the run gave no value for {node}.{port}"), |e| format!("the run failed: {e}"))),
    };
    s.docs.pending = None;
    s.docs.last = Some(outcome.clone());
    Some(outcome)
}

fn has_doc_apps(s: &Session) -> std::result::Result<(), String> {
    if s.docs.apps.is_empty() {
        return Err("no document app is configured".into());
    }
    if !s.catalog().wire_types.iter().any(|w| s.docs.for_wire(&w.id).is_some()) {
        return Err("this engine makes no documents an app opens".into());
    }
    Ok(())
}

fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let node = match opt_str_param(p, "node")? {
        Some(n) => n.to_owned(),
        None => s.run.watch.first().map(|w| w.node.clone()).ok_or_else(|| SessionError::Params("\"node\" is required (nothing is watched)".into()))?,
    };
    let port = opt_str_param(p, "port")?.map(str::to_owned);
    let t = doc_target(s, &node, port.as_deref()).map_err(SessionError::Params)?;
    if let Err(e) = &t.app.program {
        return Err(SessionError::File(format!("{} isn't installed: {e}", t.app.label)));
    }
    if let Some(d) = t.value.clone() {
        let opened = open_now(s, &t, &d);
        s.docs.pending = None;
        s.docs.last = Some(opened.clone());
        return opened.map(|o| json!(o)).map_err(SessionError::File);
    }
    if !s.connected {
        return Err(SessionError::File(format!("{node}.{} has no value yet, and the engine is not connected", t.port)));
    }
    // Watch it and run: the memo answers the rest at once; poll_doc_open opens it on arrival.
    let watch = vec![WatchRequest { node: node.clone(), port: t.port.clone(), ..Default::default() }];
    let base = crate::paths::base_folder(s.path());
    let graph = crate::paths::with_absolute_paths(s.graph(), s.catalog(), base.as_deref());
    let req = s.run.request(&graph, Vec::new(), watch, std::time::Instant::now());
    s.docs.pending = Some((node.clone(), t.port.clone()));
    s.docs.last = None;
    Ok(json!({"pending": true, "node": node, "port": t.port, "app": t.app.label, "run_id": req.run_id}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "doc.open",
        label: "Open Document in Its App",
        menu: &["Node"],
        shortcut: None,
        params: "{node?, port?} (default: the watched node; its first document output) -> the copy opened {node, port, app, path, handle, reused}, or {pending: true, run_id} when the value must be run first (it opens when it arrives; see ui.inspect docs.last). Opens a verified copy of the document in its craft's desktop app (VectorCraft: `vectorcraft <file>`), never the engine's own file; a remote engine's documents can't be opened",
        enabled: has_doc_apps,
        run: open,
        journal: false,
    }]
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Instant;

    use plumcraft_proto::data::BlobMeta;
    use plumcraft_proto::display::Display;
    use plumcraft_proto::event::NodeState;
    use plumcraft_proto::handle::Handle;
    use plumcraft_proto::run::{NodeReport, RunReply};

    use super::*;
    use crate::run::Outgoing;

    fn catalog() -> plumcraft_proto::catalogue::SchemaReply {
        serde_json::from_value(json!({
            "wire_types": [{"id": "vectorcraft.doc", "media": "doc"}, {"id": "SVG", "media": "vector"}],
            "nodes": [
                {"id": "vc.open", "display": "Open", "outputs": [{"name": "doc", "type": "vectorcraft.doc"}, {"name": "result", "type": "JSON"}]},
                {"id": "vc.svg", "display": "Export SVG", "inputs": [{"name": "doc", "type": "vectorcraft.doc"}], "outputs": [{"name": "svg", "type": "SVG"}]}
            ]
        }))
        .unwrap()
    }

    type Calls = Arc<Mutex<Vec<(PathBuf, PathBuf)>>>;

    fn session(dir: &Path, program: std::result::Result<PathBuf, String>) -> (Session, Calls) {
        let wf = json!({"version": 1, "nodes": {"o": {"kind": "vc.open", "pos": [0, 0]}, "s": {"kind": "vc.svg", "pos": [300, 0]}},
                        "wires": [{"from": {"node": "o", "port": "doc"}, "to": {"node": "s", "port": "doc"}}]});
        let mut s = Session::with_graph(catalog(), crate::file_cmds::parse_workflow(&wf.to_string()).unwrap());
        s.connected = true;
        let calls: Calls = Arc::default();
        let c = Arc::clone(&calls);
        let app = DocApp {
            app: "vectorcraft".into(),
            wire: "vectorcraft.doc".into(),
            label: "VectorCraft".into(),
            ext: "vectorcraft".into(),
            program,
            args: Vec::new(),
        };
        s.docs = DocApps::new(vec![app]).with_root(dir.join("open")).with_launcher(Arc::new(move |p: &Path, _: &[String], f: &Path| {
            c.lock().unwrap().push((p.to_path_buf(), f.to_path_buf()));
            Ok(())
        }));
        (s, calls)
    }

    fn tmp(name: &str) -> PathBuf {
        let d = plumcraft_scratch::root().join(format!("doc-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A document in a pretend CAS, and its DataRef with a `path` locator.
    fn doc(dir: &Path, bytes: &[u8]) -> DataRef {
        let h = Handle::md5_raw(bytes);
        let path = dir.join(format!("{}.vectorcraft", h.digest()));
        std::fs::write(&path, bytes).unwrap();
        DataRef {
            handle: Some(h),
            meta: BlobMeta {
                media: "application/vnd.vectorcraft+json".into(),
                size: bytes.len() as u64,
                form: Form::Doc { app: "vectorcraft".into() },
                aliases: Vec::new(),
            },
            at: vec![Locator::Path { path: path.display().to_string(), offset: 0 }],
        }
    }

    /// Run the queued request to a reply that carries `o.doc` (as a target's output) and, when
    /// watched, its display.
    fn reply_with(s: &mut Session, d: &DataRef) {
        let req = s.run.take_outbox().into_iter().find_map(|o| if let Outgoing::Run(r) = o { Some(r) } else { None }).unwrap();
        let mut nodes = std::collections::BTreeMap::new();
        for id in ["o", "s"] {
            let mut r = NodeReport { state: NodeState::Done, ..Default::default() };
            if id == "o" {
                r.outputs.insert("doc".into(), PortValue::Blob { data: d.clone() });
            }
            nodes.insert(id.to_owned(), r);
        }
        let watch = req
            .watch
            .iter()
            .map(|w| Display { node: w.node.clone(), port: w.port.clone(), media: Media::Doc, full: Some(d.clone()), ..Default::default() })
            .collect();
        let reply = RunReply { run_id: req.run_id.clone(), generation: req.generation, ok: true, nodes, watch, ..Default::default() };
        assert!(s.run.apply_reply(&reply, Instant::now()));
    }

    #[test]
    fn a_doc_value_opens_as_a_verified_copy_and_never_the_cas_file() {
        let dir = tmp("open");
        let (mut s, calls) = session(&dir, Ok(PathBuf::from("/fake/vectorcraft")));
        let d = doc(&dir, b"{\"doc\": 1}");
        assert!(s.execute("doc.open", &json!({"node": "s"})).unwrap_err().to_string().contains("no document output"));
        assert!(s.execute("doc.open", &json!({"node": "o", "port": "result"})).unwrap_err().to_string().contains("not a document"));
        // not run yet: it watches and runs, then opens on arrival
        let r = s.execute("doc.open", &json!({"node": "o"})).unwrap();
        assert_eq!(r["pending"], true, "{r}");
        assert_eq!(s.docs.pending(), Some(("o", "doc")));
        assert_eq!(s.run.watch[0].node, "o");
        assert!(poll_doc_open(&mut s).is_none(), "still running");
        reply_with(&mut s, &d);
        let opened = poll_doc_open(&mut s).unwrap().unwrap();
        assert!(poll_doc_open(&mut s).is_none(), "reported once");
        assert_eq!((opened.node.as_str(), opened.port.as_str(), opened.reused), ("o", "doc", false));
        assert!(opened.path.starts_with(dir.join("open").join("o")), "{}", opened.path.display());
        let name = opened.path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("Open_1-") || name.starts_with("Open-"), "named for the node: {name}");
        assert!(name.ends_with(".vectorcraft"), "{name}");
        assert_eq!(std::fs::read(&opened.path).unwrap(), b"{\"doc\": 1}");
        assert_eq!(calls.lock().unwrap().as_slice(), [(PathBuf::from("/fake/vectorcraft"), opened.path.clone())]);
        // now the value is at hand: it opens at once, reusing the unchanged copy
        let again: Opened = serde_json::from_value(s.execute("doc.open", &json!({"node": "o"})).unwrap()).unwrap();
        assert!(again.reused && again.path == opened.path);
        // the app changes its copy: that copy is kept, and the CAS file never changed
        std::fs::write(&opened.path, b"edited in the app").unwrap();
        let third: Opened = serde_json::from_value(s.execute("doc.open", &json!({})).unwrap()).unwrap();
        assert_ne!(third.path, opened.path);
        assert_eq!(std::fs::read(&opened.path).unwrap(), b"edited in the app");
        assert_eq!(std::fs::read(&third.path).unwrap(), b"{\"doc\": 1}");
        let Some(Locator::Path { path, .. }) = d.at.first() else { panic!() };
        assert_eq!(std::fs::read(path).unwrap(), b"{\"doc\": 1}");
        assert_eq!(calls.lock().unwrap().len(), 3);
        assert_eq!(s.docs.inspect()["last"]["ok"], true);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_changed_cas_file_a_remote_value_and_a_missing_app_are_errors() {
        let dir = tmp("refuse");
        let (mut s, calls) = session(&dir, Ok(PathBuf::from("/fake/vectorcraft")));
        let d = doc(&dir, b"original");
        s.execute("doc.open", &json!({"node": "o"})).unwrap();
        reply_with(&mut s, &d);
        // the file no longer holds the handle
        let Some(Locator::Path { path, .. }) = d.at.first() else { panic!() };
        std::fs::write(path, b"tampered").unwrap();
        let e = poll_doc_open(&mut s).unwrap().unwrap_err();
        assert!(e.contains("no longer holds"), "{e}");
        assert!(std::fs::read_dir(dir.join("open").join("o")).unwrap().next().is_none(), "no copy and no temp file left");
        // a remote value: no path locator
        let mut remote = d.clone();
        remote.at = vec![Locator::Url { url: "http://engine.example:17311/v1/blobs/x".into(), expires: None }];
        let t = doc_target(&s, "o", None).unwrap();
        let e = local_file(&remote, &t.app).unwrap_err();
        assert!(e.contains("remote engine"), "{e}");
        remote.at.clear();
        s.run.last_reply.as_mut().unwrap().nodes.get_mut("o").unwrap().outputs.insert("doc".into(), PortValue::Blob { data: remote.clone() });
        s.run.watched[0].as_mut().unwrap().full = Some(remote);
        let e = s.execute("doc.open", &json!({"node": "o"})).unwrap_err().to_string();
        assert!(e.contains("remote engine"), "{e}");
        assert!(open_blocker(&s, "o", None).unwrap_err().contains("remote engine"));
        // another craft's document
        let mut other = d.clone();
        other.meta.form = Form::Doc { app: "soundcraft".into() };
        assert!(local_file(&other, &t.app).unwrap_err().contains("soundcraft"));
        assert!(calls.lock().unwrap().is_empty(), "nothing was started");
        // a missing app: said, with the fix
        let (mut s2, _) = session(&dir, Err("no app at /no/vectorcraft (or set PLUMCRAFT_VECTORCRAFT_APP)".into()));
        let e = s2.execute("doc.open", &json!({"node": "o"})).unwrap_err().to_string();
        assert!(e.contains("VectorCraft isn't installed") && e.contains("PLUMCRAFT_VECTORCRAFT_APP"), "{e}");
        assert!(open_blocker(&s2, "o", None).unwrap_err().contains("isn't installed"));
        // no apps at all: disabled
        let mut s3 = Session::new(catalog());
        s3.docs = DocApps::default();
        assert!(s3.execute("doc.open", &json!({"node": "o"})).unwrap_err().to_string().contains("no document app"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_doc_in_the_store_opens_through_the_engine_link() {
        let dir = tmp("store");
        let (mut s, calls) = session(&dir, Ok(PathBuf::from("/fake/vectorcraft")));
        let mut d = doc(&dir, b"{\"doc\": 2}");
        // As the engine publishes it: a commonkit key and a path into the store's segment
        d.at = vec![Locator::Commonkit { key: "plum:v1:plumraw:x:c".into() }, Locator::Path { path: "/store/x.c.npy".into(), offset: 128 }];
        let t = doc_target(&s, "o", None).unwrap();
        let e = copy_for_app(&d, &t.app, &dir, "o", "t", None).unwrap_err();
        assert!(e.contains("no engine link"), "{e}");
        s.execute("doc.open", &json!({"node": "o"})).unwrap();
        s.docs.resolver = Some(Arc::new(|_: &DataRef| Some(b"{\"doc\": 2}".to_vec())));
        reply_with(&mut s, &d);
        assert!(open_blocker(&s, "o", None).is_ok(), "in the store on this machine is not remote");
        let opened = poll_doc_open(&mut s).unwrap().unwrap();
        assert_eq!(std::fs::read(&opened.path).unwrap(), b"{\"doc\": 2}");
        assert_eq!(calls.lock().unwrap().len(), 1);
        // reaped from the store: said, and nothing opened
        s.docs.resolver = Some(Arc::new(|_: &DataRef| None));
        std::fs::remove_file(&opened.path).unwrap();
        let e = s.execute("doc.open", &json!({"node": "o"})).unwrap_err().to_string();
        assert!(e.contains("no longer in the store"), "{e}");
        // bytes that don't match the handle are refused
        s.docs.resolver = Some(Arc::new(|_: &DataRef| Some(b"tampered".to_vec())));
        assert!(s.execute("doc.open", &json!({"node": "o"})).unwrap_err().to_string().contains("no longer holds"));
        assert_eq!(calls.lock().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unchanged_copies_unused_for_the_ttl_are_swept_and_changed_ones_kept() {
        let dir = tmp("sweep");
        let root = dir.join("open");
        let d = doc(&dir, b"same bytes");
        let digest = d.handle.as_ref().unwrap().digest().to_owned();
        let (old, _) = copy_value(&d, CopyFrom::Bytes(b"same bytes"), "vectorcraft", &root, "n1", "Trace").unwrap();
        std::fs::write(format!("{}.log", old.display()), b"app log").unwrap();
        let (edited, _) = copy_value(&d, CopyFrom::Bytes(b"same bytes"), "vectorcraft", &root, "n2", "Trace").unwrap();
        std::fs::write(&edited, b"edited in the app").unwrap();
        let (fresh, _) = copy_value(&d, CopyFrom::Bytes(b"same bytes"), "vectorcraft", &root, "n3", "Trace").unwrap();
        let other = root.join("n3").join("notes.txt");
        std::fs::write(&other, b"not a copy").unwrap();
        let week = std::time::Duration::from_secs(7 * 86_400);
        let then = std::time::SystemTime::now() - week - week;
        for p in [&old, &edited, &other] {
            std::fs::File::options().append(true).open(p).unwrap().set_modified(then).unwrap();
        }
        assert_eq!(sweep_copies(&root, week, std::time::SystemTime::now()), 1);
        assert!(!old.exists() && !root.join("n1").exists(), "the copy, its log and its folder");
        assert!(edited.exists() && fresh.exists() && other.exists());
        assert_eq!(copy_digest(&format!("Trace-{}", &digest[..8])), Some(digest[..8].to_owned()));
        assert_eq!(copy_digest(&format!("Trace-{}-3", &digest[..8])), Some(digest[..8].to_owned()));
        assert_eq!(copy_digest("notes"), None);
        assert_eq!(sweep_copies(&dir.join("missing"), week, std::time::SystemTime::now()), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn hostile_params_and_values_are_errors() {
        let dir = tmp("hostile");
        let (mut s, _) = session(&dir, Ok(PathBuf::from("/fake/vectorcraft")));
        for p in [json!({"node": 7}), json!({"node": "nope"}), json!({"node": "o", "port": ["doc"]}), json!({"node": "o", "port": "nope"})] {
            assert!(s.execute("doc.open", &p).is_err(), "{p}");
        }
        s.run.watch.clear();
        assert!(s.execute("doc.open", &json!({})).unwrap_err().to_string().contains("node"));
        let t = doc_target(&s, "o", None).unwrap();
        let mut d = doc(&dir, b"x");
        d.at = vec![Locator::Path { path: "/dev".into(), offset: 0 }];
        assert!(copy_for_app(&d, &t.app, &dir, "o", "t", None).is_err(), "a folder isn't a document");
        d.at = vec![Locator::Path { path: dir.join("missing").display().to_string(), offset: 0 }];
        assert!(copy_for_app(&d, &t.app, &dir, "o", "t", None).is_err());
        d.handle = Some(Handle::md5_nd1("|u1", &[1], b"x").unwrap());
        assert!(copy_for_app(&d, &t.app, &dir, "o", "t", None).unwrap_err().contains("md5:raw"));
        assert_eq!(safe("../../etc/passwd", 60), "etc_passwd");
        assert_eq!(safe("", 60), "document");
        assert_eq!(safe("Image Trace 2", 60), "Image_Trace_2");
        // disconnected with no value: said
        s.connected = false;
        assert!(s.execute("doc.open", &json!({"node": "o"})).unwrap_err().to_string().contains("not connected"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The default launcher really starts the program with the file (a fake app that writes
    /// its arguments down), detached, and a missing program is an error.
    #[cfg(unix)]
    #[test]
    fn the_default_launcher_starts_the_app_with_the_copy() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp("spawn");
        let fake = dir.join("fake-vectorcraft");
        std::fs::write(&fake, "#!/bin/sh\necho \"$@\" > \"$0.args\"\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let file = dir.join("doc copy.vectorcraft");
        std::fs::write(&file, b"{}").unwrap();
        spawn_app(&fake, &["--flag".into()], &file).unwrap();
        let args = dir.join("fake-vectorcraft.args");
        let start = Instant::now();
        while !args.exists() {
            assert!(start.elapsed() < std::time::Duration::from_secs(10), "the fake app never ran");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(std::fs::read_to_string(&args).unwrap().trim(), format!("--flag {}", file.display()));
        assert!(spawn_app(&dir.join("no-such-app"), &[], &file).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
