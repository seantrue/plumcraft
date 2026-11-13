//! Values the editor has no viewer for (a PDF, a video, a mesh, any opaque bytes): `value.open`
//! opens a copy in the system's default app for it (Open), or in an app the user chooses
//! (Open with…).
//!
//! - **Which values.** An output whose watched display, or whose wire type, has a media kind
//!   with no viewer in the Watched panel ([`has_viewer`]): `bytes`, `mesh`, `composite`,
//!   `sequence`. Documents keep "Open in …" (`doc.open`).
//! - **Which value.** As `doc.open`: the watched display's `full` (the item shown, for a list),
//!   else the latest reply's output, when the node isn't stale; otherwise the command watches the
//!   output and runs (the memo makes it cheap), and [`poll_value_open`] opens it on arrival.
//! - **A copy, verified**, as `doc.open` makes one: `<cache>/plumcraft/open/<node>/<title>-
//!   <digest8>.<ext>`, the extension from the value's media type ([`ext_for`]). A file on this
//!   machine is copied as it is hashed; a value elsewhere (inline, or a remote engine's) is read
//!   through the host's engine link ([`crate::doc_cmds::Resolver`]) and checked again.
//! - **Which app.** macOS: `open <copy>`, or `open -a <app> <copy>`; elsewhere `xdg-open <copy>`,
//!   or `<app> <copy>`. Apps chosen with Open with… are remembered for the session (most recent
//!   first, eight at most) for the menus.

use std::path::{Path, PathBuf};

use plumcraft_graph::NodeId;
use plumcraft_proto::catalogue::Media;
use plumcraft_proto::data::{DataRef, Locator, PortValue};
use plumcraft_proto::display::WatchRequest;
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, opt_str_param};
use crate::doc_cmds::{CopyFrom, Opened, copy_value};
use crate::{Result, Session, SessionError};

/// Apps remembered for Open with….
pub const MAX_RECENT_APPS: usize = 8;

/// An open waiting for its value.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingOpen {
    pub node: String,
    pub port: String,
    /// The app chosen with Open with…; `None` for the default app.
    pub app: Option<PathBuf>,
}

/// Whether the Watched panel shows values of this media kind itself.
pub fn has_viewer(media: Media) -> bool {
    !matches!(media, Media::Bytes | Media::Mesh | Media::Composite | Media::Sequence)
}

pub use plumcraft_proto::media::ext_for;

/// An app's name as a menu shows it: `Preview` for `/System/Applications/Preview.app`.
pub fn app_name(app: &Path) -> String {
    app.file_stem().map_or_else(|| app.display().to_string(), |s| s.to_string_lossy().into_owned())
}

/// What `value.open` acts on: the node and port, and the value when it is at hand.
#[derive(Debug, Clone)]
pub struct ValueTarget {
    pub node: String,
    pub port: String,
    pub value: Option<DataRef>,
}

/// The media kind of `node.port`: its watched display's, else its wire type's.
fn media_of(s: &Session, node: &str, port: &str, ty: &str) -> Media {
    let watched = s.run.watch.iter().zip(&s.run.watched).find(|(w, _)| w.node == node && w.port == port).and_then(|(_, d)| d.as_ref()).map(|d| d.media);
    watched.or_else(|| s.catalog().wire_type(ty).map(|w| w.media)).unwrap_or_default()
}

/// The output of `node` `value.open` would open: `port`, else its first output with no viewer.
pub fn value_target(s: &Session, node: &str, port: Option<&str>) -> std::result::Result<ValueTarget, String> {
    let id = NodeId(node.to_owned());
    let n = s.graph().node(&id).ok_or_else(|| format!("no node {node:?}"))?;
    let def = s.catalog().node(&n.kind).ok_or_else(|| format!("{node}: the engine has no node type {:?}", n.kind))?;
    let out = match port {
        Some(p) => def.output(p).ok_or_else(|| format!("{node} has no output {p:?}"))?,
        None => {
            def.outputs.iter().find(|o| !has_viewer(media_of(s, node, &o.name, &o.ty))).ok_or_else(|| format!("{node} has no output the editor can't show"))?
        }
    };
    let media = media_of(s, node, &out.name, &out.ty);
    if has_viewer(media) {
        return Err(format!("{node}.{} is shown in the Watched panel ({media:?}); Open is for values the editor can't show", out.name));
    }
    let fresh = s.run.node(node).is_some_and(|r| !r.is_stale() && !r.is_active());
    let value = fresh.then(|| value_of(s, node, &out.name)).flatten();
    Ok(ValueTarget { node: node.to_owned(), port: out.name.clone(), value })
}

/// The value of `node.port` the session has: a watched display's `full`, else the latest
/// reply's output (a list's first item).
fn value_of(s: &Session, node: &str, port: &str) -> Option<DataRef> {
    let watched =
        s.run.watch.iter().zip(&s.run.watched).find(|(w, _)| w.node == node && w.port == port).and_then(|(_, d)| d.as_ref()).and_then(|d| d.full.clone());
    fn first(v: &PortValue) -> Option<DataRef> {
        match v {
            PortValue::Blob { data } => Some(data.clone()),
            PortValue::List { items } => items.first().and_then(first),
            _ => None,
        }
    }
    watched.or_else(|| first(s.run.last_reply.as_ref()?.nodes.get(node)?.outputs.get(port)?))
}

/// Why `value.open` can't open `node.port` now (the Watched panel's buttons and the node menu).
pub fn value_open_blocker(s: &Session, node: &str, port: Option<&str>) -> std::result::Result<ValueTarget, String> {
    let t = value_target(s, node, port)?;
    if t.value.is_none() && !s.connected {
        return Err("not run yet, and the engine is not connected".into());
    }
    Ok(t)
}

/// The program and arguments that open `file` (in `app` when given).
fn opener(app: Option<&Path>) -> (PathBuf, Vec<String>) {
    if cfg!(target_os = "macos") {
        let args = app.map(|a| vec!["-a".to_owned(), a.display().to_string()]).unwrap_or_default();
        (PathBuf::from("/usr/bin/open"), args)
    } else {
        match app {
            Some(a) => (a.to_path_buf(), Vec::new()),
            None => (PathBuf::from("xdg-open"), Vec::new()),
        }
    }
}

/// Copy the value (checked) and open the copy.
fn open_now(s: &mut Session, node: &str, port: &str, d: &DataRef, app: Option<&Path>) -> std::result::Result<Opened, String> {
    let title = crate::node_title(s.graph(), s.catalog(), &NodeId(node.to_owned()));
    let ext = ext_for(&d.meta.media);
    crate::doc_cmds::sweep_stale_copies(&s.docs.root);
    let file = d.at.iter().find_map(|l| match l {
        Locator::Path { path, offset: 0 } => Some(PathBuf::from(path)),
        _ => None,
    });
    let (path, reused) = match file.filter(|f| f.is_file()) {
        Some(f) => copy_value(d, CopyFrom::File(&f), ext, &s.docs.root, node, &title)?,
        None => {
            let read = s.docs.resolver.clone().ok_or("the value isn't a file on this machine, and there is no engine link to read it through")?;
            let bytes = read(d).ok_or("the value's bytes are gone or didn't match their handle: run again")?;
            copy_value(d, CopyFrom::Bytes(&bytes), ext, &s.docs.root, node, &title)?
        }
    };
    let (program, args) = opener(app);
    (s.docs.launch)(&program, &args, &path).map_err(|e| format!("opening {}: {e}", path.display()))?;
    if let Some(a) = app {
        s.docs.recent_apps.retain(|r| r != a);
        s.docs.recent_apps.insert(0, a.to_path_buf());
        s.docs.recent_apps.truncate(MAX_RECENT_APPS);
    }
    let handle = d.handle.as_ref().map(ToString::to_string).unwrap_or_default();
    let label = app.map_or_else(|| "the default app".to_owned(), app_name);
    Ok(Opened { node: node.to_owned(), port: port.to_owned(), app: label, path, handle, reused })
}

/// Finish an open waiting for its value. `None` while there is nothing to report; the host
/// calls this every frame.
pub fn poll_value_open(s: &mut Session) -> Option<std::result::Result<Opened, String>> {
    let p = s.docs.pending_value.clone()?;
    let display = s.run.watch.iter().zip(&s.run.watched).find(|(w, _)| w.node == p.node && w.port == p.port).and_then(|(_, d)| d.clone());
    let outcome = match display {
        Some(d) if d.error.is_some() => Err(format!("{}.{}: {}", p.node, p.port, d.error.map(|e| e.message).unwrap_or_default())),
        Some(d) if d.full.is_some() && !has_viewer(d.media) => match d.full {
            Some(full) => open_now(s, &p.node, &p.port, &full, p.app.as_deref()),
            None => Err(format!("{}.{}: no value", p.node, p.port)),
        },
        Some(d) if d.full.is_some() => Err(format!("{}.{} is shown in the Watched panel", p.node, p.port)),
        _ if s.run.running => return None,
        _ => Err(s.run.run_error.clone().map_or_else(|| format!("the run gave no value for {}.{}", p.node, p.port), |e| format!("the run failed: {e}"))),
    };
    s.docs.pending_value = None;
    s.docs.last_value = Some(outcome.clone());
    Some(outcome)
}

fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let node = match opt_str_param(p, "node")? {
        Some(n) => n.to_owned(),
        None => s.run.watch.first().map(|w| w.node.clone()).ok_or_else(|| SessionError::Params("\"node\" is required (nothing is watched)".into()))?,
    };
    let port = opt_str_param(p, "port")?.map(str::to_owned);
    let app = opt_str_param(p, "app")?.filter(|a| !a.is_empty()).map(PathBuf::from);
    if let Some(a) = &app
        && !a.exists()
    {
        return Err(SessionError::File(format!("{}: no such app", a.display())));
    }
    let t = value_target(s, &node, port.as_deref()).map_err(SessionError::Params)?;
    if let Some(d) = t.value.clone() {
        let opened = open_now(s, &t.node, &t.port, &d, app.as_deref());
        s.docs.pending_value = None;
        s.docs.last_value = Some(opened.clone());
        return opened.map(|o| json!(o)).map_err(SessionError::File);
    }
    if !s.connected {
        return Err(SessionError::File(format!("{node}.{} has no value yet, and the engine is not connected", t.port)));
    }
    let watch = vec![WatchRequest { node: node.clone(), port: t.port.clone(), ..Default::default() }];
    let base = crate::paths::base_folder(s.path());
    let graph = crate::convert::runnable(s, base.as_deref());
    let req = s.run.request(&graph, Vec::new(), watch, std::time::Instant::now());
    s.docs.pending_value = Some(PendingOpen { node: node.clone(), port: t.port.clone(), app });
    s.docs.last_value = None;
    Ok(json!({"pending": true, "node": node, "port": t.port, "run_id": req.run_id}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "value.open",
        label: "Open Value",
        menu: &["Node"],
        shortcut: None,
        params: "{node?, port?, app?} (default: the watched node; its first output the editor has no viewer for, such as a PDF, a video or a mesh) -> the copy opened {node, port, app, path, handle, reused}, or {pending: true, run_id} when the value must be run first (see ui.inspect docs.value.last). Opens a verified copy in the system's default app for its type, or in `app` (a path to an application: Open with…)",
        enabled: always,
        run: open,
        journal: false,
    }]
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use plumcraft_proto::data::{BlobMeta, Form};
    use plumcraft_proto::display::Display;
    use plumcraft_proto::event::NodeState;
    use plumcraft_proto::handle::Handle;
    use plumcraft_proto::run::{NodeReport, RunReply};

    use super::*;
    use crate::doc_cmds::DocApps;
    use crate::run::Outgoing;

    type Calls = Arc<Mutex<Vec<(PathBuf, Vec<String>, PathBuf)>>>;

    fn session(dir: &Path) -> (Session, Calls) {
        let catalog = serde_json::from_value(json!({
            "wire_types": [{"id": "PDF", "media": "bytes"}, {"id": "SVG", "media": "vector"}],
            "nodes": [{"id": "x.export", "display": "Export", "outputs": [{"name": "svg", "type": "SVG"}, {"name": "pdf", "type": "PDF"}]}]
        }))
        .unwrap();
        let wf = json!({"version": 1, "nodes": {"e": {"kind": "x.export", "pos": [0, 0]}}, "wires": []});
        let mut s = Session::with_graph(catalog, crate::file_cmds::parse_workflow(&wf.to_string()).unwrap());
        s.connected = true;
        let calls: Calls = Arc::default();
        let c = Arc::clone(&calls);
        s.docs = DocApps::new(Vec::new()).with_root(dir.join("open")).with_launcher(Arc::new(move |p: &Path, a: &[String], f: &Path| {
            c.lock().unwrap().push((p.to_path_buf(), a.to_vec(), f.to_path_buf()));
            Ok(())
        }));
        (s, calls)
    }

    fn tmp(name: &str) -> PathBuf {
        let d = plumcraft_scratch::root().join(format!("open-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn pdf(at: Vec<Locator>, bytes: &[u8]) -> DataRef {
        DataRef {
            handle: Some(Handle::raw(bytes)),
            meta: BlobMeta { media: "application/pdf".into(), size: bytes.len() as u64, form: Form::default(), aliases: Vec::new() },
            at,
        }
    }

    fn reply_with(s: &mut Session, d: &DataRef) {
        let req = s.run.take_outbox().into_iter().find_map(|o| if let Outgoing::Run(r) = o { Some(r) } else { None }).unwrap();
        let mut r = NodeReport { state: NodeState::Done, ..Default::default() };
        r.outputs.insert("pdf".into(), PortValue::Blob { data: d.clone() });
        let nodes = std::collections::BTreeMap::from([("e".to_owned(), r)]);
        let watch = req
            .watch
            .iter()
            .map(|w| Display { node: w.node.clone(), port: w.port.clone(), media: Media::Bytes, full: Some(d.clone()), ..Default::default() })
            .collect();
        let reply = RunReply { run_id: req.run_id.clone(), generation: req.generation, ok: true, nodes, watch, ..Default::default() };
        assert!(s.run.apply_reply(&reply, Instant::now()));
    }

    #[test]
    fn extensions_come_from_media_types() {
        assert_eq!(ext_for("application/pdf"), "pdf");
        assert_eq!(ext_for("Video/MP4; codecs=avc1"), "mp4");
        assert_eq!(ext_for("model/stl"), "stl");
        assert_eq!(ext_for("application/x-weird"), "bin");
        assert!(!has_viewer(Media::Bytes) && !has_viewer(Media::Mesh) && has_viewer(Media::Image) && has_viewer(Media::Doc));
    }

    #[test]
    fn a_value_with_no_viewer_opens_as_a_verified_copy() {
        let dir = tmp("value");
        let (mut s, calls) = session(&dir);
        let bytes = b"%PDF-1.7 hello";
        let src = dir.join("cas-file");
        std::fs::write(&src, bytes).unwrap();
        let d = pdf(vec![Locator::Path { path: src.display().to_string(), offset: 0 }], bytes);
        // the SVG has a viewer; the first output without one is the PDF
        assert!(s.execute("value.open", &json!({"node": "e", "port": "svg"})).unwrap_err().to_string().contains("Watched panel"));
        let r = s.execute("value.open", &json!({"node": "e"})).unwrap();
        assert_eq!((r["pending"].as_bool(), r["port"].as_str()), (Some(true), Some("pdf")), "{r}");
        assert!(poll_value_open(&mut s).is_none(), "still running");
        reply_with(&mut s, &d);
        let opened = poll_value_open(&mut s).unwrap().unwrap();
        assert!(opened.path.extension().is_some_and(|e| e == "pdf"), "{}", opened.path.display());
        assert_eq!(std::fs::read(&opened.path).unwrap(), bytes);
        assert_ne!(opened.path, src, "a copy, never the engine's file");
        let (program, args, file) = calls.lock().unwrap()[0].clone();
        assert_eq!(file, opened.path);
        if cfg!(target_os = "macos") {
            assert_eq!((program, args), (PathBuf::from("/usr/bin/open"), Vec::<String>::new()));
        }
        // Open with…: the value is at hand now; the app is remembered
        let app = dir.join("Preview.app");
        std::fs::create_dir_all(&app).unwrap();
        let r = s.execute("value.open", &json!({"node": "e", "app": app})).unwrap();
        assert_eq!(r["reused"], true, "{r}");
        assert_eq!(r["app"], "Preview");
        assert_eq!(s.docs.recent_apps, vec![app.clone()]);
        if cfg!(target_os = "macos") {
            assert_eq!(calls.lock().unwrap()[1].1, vec!["-a".to_owned(), app.display().to_string()]);
        }
        assert!(s.execute("value.open", &json!({"node": "e", "app": dir.join("Nope.app")})).unwrap_err().to_string().contains("no such app"));
    }

    #[test]
    fn a_value_not_on_this_machine_is_read_through_the_link_and_checked() {
        let dir = tmp("remote");
        let (mut s, calls) = session(&dir);
        let bytes = b"%PDF remote";
        let d = pdf(vec![Locator::Url { url: "/v1/blobs/x".into(), expires: None }], bytes);
        let r = s.execute("value.open", &json!({"node": "e"})).unwrap();
        assert_eq!(r["pending"], true);
        reply_with(&mut s, &d);
        assert!(poll_value_open(&mut s).unwrap().unwrap_err().contains("no engine link"));
        // a link that lies about the bytes is refused
        s.docs.resolver = Some(Arc::new(|_: &DataRef| Some(b"tampered".to_vec())));
        assert!(s.execute("value.open", &json!({"node": "e"})).unwrap_err().to_string().contains("no longer holds"));
        s.docs.resolver = Some(Arc::new(move |_: &DataRef| Some(bytes.to_vec())));
        let r = s.execute("value.open", &json!({"node": "e"})).unwrap();
        assert_eq!(std::fs::read(r["path"].as_str().unwrap()).unwrap(), bytes);
        assert_eq!(calls.lock().unwrap().len(), 1);
    }
}
