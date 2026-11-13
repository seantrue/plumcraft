//! Edits made in a document's app flow downstream (maintainer, 2026-11-03: "saving an edited
//! version of the doc should trigger the downstream nodes").
//!
//! - **Tracked copies.** Every copy `doc.open` starts an app on (a document's pop-out included)
//!   is tracked. The host calls [`poll_doc_edits`] every frame; at most every [`POLL`] it reads
//!   each copy's size and modification time.
//! - **A save.** A copy whose size or time changed, and then held still for one poll (an app may
//!   write in pieces), is hashed. New bytes are published to the store and bound in place of the
//!   node's output on every input wired from it (`RunRequest.inputs`, spec 7.6 rule 10;
//!   [`crate::RunState::doc_edits`]). Those nodes and everything downstream go stale, saying
//!   why, and the graph runs. The node keeps its own value: the edit is the app's, not the
//!   node's. Saving the original bytes back drops the edit.
//! - **Reopening** a document with an edit opens the edited copy again, not a fresh one.
//! - **`doc.revert`** drops an edit (the file stays) and runs again.
//! - Edits last while the graph is open: they aren't saved with the workflow.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use plumcraft_graph::NodeId;
use plumcraft_proto::data::{BlobMeta, DataRef};
use plumcraft_store::Store;
use serde::Serialize;
use serde_json::{Value, json};

use crate::commands::{CommandSpec, opt_str_param, str_param};
use crate::{Result, Session, SessionError};

/// How often copies are looked at.
pub const POLL: Duration = Duration::from_millis(500);
/// The largest edited document taken back (it is read whole to publish it).
pub const MAX_EDIT_BYTES: u64 = 1 << 30;
/// The most copies tracked; the oldest goes first.
const MAX_TRACKED: usize = 64;

/// A copy opened in its app, watched for saves.
#[derive(Debug, Clone, Serialize)]
pub struct Tracked {
    pub node: String,
    pub port: String,
    /// The app's label ("SoundCraft").
    pub app: String,
    pub path: PathBuf,
    /// The MD5 of the value the node gave.
    pub original: String,
    /// The MD5 of the copy when last read.
    pub digest: String,
    /// Its bytes are bound downstream in place of the node's value.
    pub edited: bool,
    #[serde(skip)]
    meta: BlobMeta,
    /// Size and modification time when last looked at.
    #[serde(skip)]
    seen: Option<(SystemTime, u64)>,
    /// Changed at the last look: read it once it holds still.
    #[serde(skip)]
    settling: bool,
}

/// The tracked copies and the store edits are published to.
#[derive(Debug, Clone, Default)]
pub struct Edits {
    pub tracked: Vec<Tracked>,
    /// The store edits go to; `None`: commonkit's default root.
    pub store_root: Option<PathBuf>,
    store: Option<Store>,
    polled: Option<Instant>,
    /// The latest save taken back, or why it couldn't be.
    pub last: Option<std::result::Result<Value, String>>,
}

impl Edits {
    fn store(&mut self) -> &Store {
        let root = self.store_root.clone();
        self.store.get_or_insert_with(|| Store::new(root.unwrap_or_else(plumcraft_store::default_root)))
    }

    /// As `ui.inspect`'s `docs.edits`.
    pub fn inspect(&self) -> Value {
        let last = match &self.last {
            Some(Ok(v)) => json!({"ok": true, "applied": v}),
            Some(Err(e)) => json!({"ok": false, "error": e}),
            None => Value::Null,
        };
        json!({"tracked": self.tracked, "last": last})
    }
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok().filter(std::fs::Metadata::is_file)?;
    Some((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
}

/// The digest of bytes in the process's algorithm (a copy's identity, as its handle's).
fn digest_hex(bytes: &[u8]) -> String {
    commonkit::digest::raw(bytes)
}

/// The edited copy of `node.port`, when it has one still on disk: reopening opens it again.
pub(crate) fn edited_copy(s: &Session, node: &str, port: &str) -> Option<PathBuf> {
    s.docs.edits.tracked.iter().find(|t| t.node == node && t.port == port && t.edited && t.path.is_file()).map(|t| t.path.clone())
}

/// Watch `path`, the copy of `d` just opened in `app`, for saves.
pub(crate) fn track(s: &mut Session, node: &str, port: &str, app: &str, path: &Path, d: &DataRef) {
    let original = d.handle.as_ref().map(|h| h.digest().to_owned()).unwrap_or_default();
    let edits = &mut s.docs.edits;
    if let Some(t) = edits.tracked.iter_mut().find(|t| t.path == path) {
        t.seen = stamp(path);
        t.settling = false;
        return;
    }
    // one copy per output: a fresh open replaces the last (an edited one is kept on disk)
    edits.tracked.retain(|t| !(t.node == node && t.port == port && !t.edited));
    if edits.tracked.len() >= MAX_TRACKED {
        edits.tracked.remove(0);
    }
    edits.tracked.push(Tracked {
        node: node.to_owned(),
        port: port.to_owned(),
        app: app.to_owned(),
        path: path.to_path_buf(),
        digest: original.clone(),
        original,
        edited: false,
        meta: d.meta.clone(),
        seen: stamp(path),
        settling: false,
    });
}

/// Look at the tracked copies (at most every [`POLL`]) and take back any saved since: bind it
/// downstream, mark what it reaches stale, and run. `None` while there is nothing to report;
/// the host calls this every frame and asks for a repaint while [`watching`].
pub fn poll_doc_edits(s: &mut Session, now: Instant) -> Option<std::result::Result<Value, String>> {
    if s.docs.edits.tracked.is_empty() || s.docs.edits.polled.is_some_and(|t| now.saturating_duration_since(t) < POLL) {
        return None;
    }
    s.docs.edits.polled = Some(now);
    let mut saved = Vec::new();
    for (i, t) in s.docs.edits.tracked.iter_mut().enumerate() {
        let Some(st) = stamp(&t.path) else { continue }; // gone: nothing to take back
        if t.seen != Some(st) {
            t.seen = Some(st);
            t.settling = true;
        } else if t.settling {
            t.settling = false;
            saved.push(i);
        }
    }
    republish(s);
    let mut outcome = None;
    for i in saved {
        match take_back(s, i) {
            Ok(None) => {}
            Ok(Some(v)) => outcome = Some(Ok(v)),
            Err(e) => outcome = Some(Err(e)),
        }
    }
    if let Some(o) = &outcome {
        s.docs.edits.last = Some(o.clone());
    }
    outcome
}

/// Whether copies are being watched (the host keeps polling).
pub fn watching(s: &Session) -> bool {
    !s.docs.edits.tracked.is_empty()
}

/// The store may have reaped an edit: publish it again from its copy, when the copy still
/// holds it.
fn republish(s: &mut Session) {
    let edits: Vec<(String, String, DataRef)> = s.run.doc_edits.iter().map(|((n, p), d)| (n.clone(), p.clone(), d.clone())).collect();
    for (node, port, d) in edits {
        let Some(h) = d.handle.clone() else { continue };
        if s.docs.edits.store().locate(&h).is_some() {
            continue;
        }
        let Some(path) = s.docs.edits.tracked.iter().find(|t| t.node == node && t.port == port && t.edited).map(|t| t.path.clone()) else { continue };
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if digest_hex(&bytes) != h.digest() {
            continue; // changed again: the next poll takes it back
        }
        if let Some((_, at)) = s.docs.edits.store().publish_raw(&bytes) {
            s.run.doc_edits.insert((node, port), DataRef { at, ..d });
        }
    }
}

/// Copy `i` was saved: bind its bytes downstream (or drop the edit when they are the node's
/// own again), and run. `Ok(None)`: touched, not changed.
fn take_back(s: &mut Session, i: usize) -> std::result::Result<Option<Value>, String> {
    let Some(t) = s.docs.edits.tracked.get(i).cloned() else { return Ok(None) };
    let len = std::fs::metadata(&t.path).map_err(|e| format!("{}: {e}", t.path.display()))?.len();
    if len > MAX_EDIT_BYTES {
        return Err(format!("{}: over {MAX_EDIT_BYTES} bytes, too large to take back", t.path.display()));
    }
    let bytes = std::fs::read(&t.path).map_err(|e| format!("{}: {e}", t.path.display()))?;
    let digest = digest_hex(&bytes);
    if digest == t.digest {
        return Ok(None);
    }
    let key = (t.node.clone(), t.port.clone());
    let back = digest == t.original;
    let handle = if back {
        s.run.doc_edits.remove(&key);
        None
    } else {
        let (handle, at) = s.docs.edits.store().publish_raw(&bytes).ok_or_else(|| format!("the store declined the edited {}.{}", t.node, t.port))?;
        let meta = BlobMeta { size: bytes.len() as u64, ..t.meta.clone() };
        s.run.doc_edits.insert(key, DataRef { handle: Some(handle.clone()), meta, at });
        Some(handle.to_string())
    };
    if let Some(tr) = s.docs.edits.tracked.get_mut(i) {
        tr.digest = digest;
        tr.edited = !back;
    }
    let why = if back { format!("{}.{} is its own value again", t.node, t.port) } else { format!("{}.{} was edited in {}", t.node, t.port, t.app) };
    s.run.mark_edited(&s.graph, &t.node, &t.port, &why);
    let run_id = run(s);
    Ok(Some(json!({"node": t.node, "port": t.port, "path": t.path, "handle": handle, "edited": !back, "run_id": run_id})))
}

/// Run the graph, when it can run.
fn run(s: &mut Session) -> Option<String> {
    if !s.connected {
        return None;
    }
    s.execute("run.start", &json!({})).ok().and_then(|v| v.get("run_id").and_then(Value::as_str).map(str::to_owned))
}

/// A new graph: nothing of the last one's is tracked or bound.
pub(crate) fn clear(s: &mut Session) {
    s.docs.edits.tracked.clear();
    s.docs.edits.last = None;
    s.run.doc_edits.clear();
}

/// A node that went takes its edits with it.
pub(crate) fn prune(s: &mut Session) {
    let graph = &s.graph;
    s.docs.edits.tracked.retain(|t| graph.node(&NodeId(t.node.clone())).is_some());
    s.run.doc_edits.retain(|(n, _), _| graph.node(&NodeId(n.clone())).is_some());
}

fn revert(s: &mut Session, p: &Value) -> Result<Value> {
    let node = str_param(p, "node")?.to_owned();
    let port = match opt_str_param(p, "port")? {
        Some(port) => port.to_owned(),
        None => s.run.doc_edits.keys().find(|(n, _)| *n == node).map(|(_, p)| p.clone()).ok_or_else(|| SessionError::Params(format!("{node} has no edit")))?,
    };
    if s.run.doc_edits.remove(&(node.clone(), port.clone())).is_none() {
        return Err(SessionError::Params(format!("{node}.{port} has no edit")));
    }
    let mut path = None;
    for t in s.docs.edits.tracked.iter_mut().filter(|t| t.node == node && t.port == port) {
        t.edited = false;
        path = Some(t.path.clone());
    }
    s.run.mark_edited(&s.graph, &node, &port, &format!("{node}.{port}'s edit was dropped"));
    let run_id = run(s);
    Ok(json!({"node": node, "port": port, "kept": path, "run_id": run_id}))
}

fn has_edits(s: &Session) -> std::result::Result<(), String> {
    if s.run.doc_edits.is_empty() { Err("no document has been edited in its app".into()) } else { Ok(()) }
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "doc.revert",
        label: "Drop Edit Made in App",
        menu: &[],
        shortcut: None,
        params: "{node, port?} -> {node, port, kept, run_id?}: stop feeding the copy saved in the document's app downstream (the file is kept) and run again, so downstream nodes get the node's own value",
        enabled: has_edits,
        run: revert,
        journal: false,
    }]
}
