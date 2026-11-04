//! Running a node on a craft (note 14, 3.3–3.8): lease a process, put the input document in it
//! (warm or cold), compile `select`, run the command, save and hash the document, export a
//! thumbnail, publish. Every failure is a node error of the kind section 3.8 gives it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use plumcraft_engine::{Array, Blob, BlobSource, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::Handle;
use plumcraft_proto::catalogue::NodeDef;
use plumcraft_proto::data::{BlobMeta, Form};
use plumcraft_proto::error::NodeError;
use serde_json::{Map, Value, json};

use crate::cas::Cas;
use crate::manifest::{Action, Kind, Manifest, OutSpec};
use crate::mcp::{CallError, Progress, Watch};
use crate::pool::{Lease, Pool, engine_error};

/// The shortest message kept of a craft error (spec 12.1 shows 180; the rest is on hover).
const MAX_MESSAGE: usize = 600;
/// The largest `select` text or `ids` list accepted.
const MAX_SELECT: usize = 64 << 10;
const MAX_IDS: usize = 100_000;
/// The longest string object id (with a `set_key`).
const MAX_ID_LEN: usize = 256;

/// What every node of one engine shares.
pub struct Shared {
    pub manifest: Manifest,
    pub pool: Pool,
    pub cas: Cas,
    pub timeout: Duration,
}

/// How a widget's value goes to the craft, when not as it is.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Encode {
    #[default]
    AsIs,
    /// A checkbox the craft takes as a number: `true` is 1, `false` 0.
    BoolAsNumber,
    /// A choice the craft takes as its index in this list.
    Index(Vec<String>),
}

/// One widget of a node, as the run sends it.
#[derive(Debug, Clone, Default)]
pub struct WidgetPlan {
    pub name: String,
    /// The craft's key; `*` merges a JSON object into the params.
    pub key: String,
    pub into: Option<String>,
    pub optional: bool,
    pub template: bool,
    /// Allowed values of a template widget (the escape hatch's allowlist).
    pub allowed: Option<Vec<String>>,
    /// Sent only while that widget has this value (a parameter a choice brings, `brings`).
    pub when: Option<(String, Value)>,
    pub encode: Encode,
}

/// A socket whose value goes to the craft as a file: staged, and its path sent as `key`.
#[derive(Debug, Clone)]
pub struct FilePlan {
    pub name: String,
    pub key: String,
    pub optional: bool,
}

/// How a node runs.
#[derive(Debug, Clone)]
pub struct Plan {
    pub kind: Kind,
    pub command: Option<String>,
    pub select: bool,
    pub widgets: Vec<WidgetPlan>,
    pub files: Vec<FilePlan>,
    pub fixed: Map<String, Value>,
    pub outputs: Vec<OutSpec>,
    /// Export: format (empty with its own action), extension, media.
    pub export: Option<(String, String, String)>,
    /// Export: the node's own action, sent without `format` (else `actions.export`).
    pub export_action: Option<crate::manifest::Action>,
    pub timeout: Option<Duration>,
}

pub struct CraftNode {
    pub def: NodeDef,
    pub plan: Plan,
    pub shared: Arc<Shared>,
}

impl Node for CraftNode {
    fn def(&self) -> NodeDef {
        self.def.clone()
    }

    fn run(&self, inputs: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
        let mut r = Runner { s: &self.shared, plan: &self.plan, timeout: self.plan.timeout.unwrap_or(self.shared.timeout) };
        r.run(inputs, cx)
    }
}

/// A [`Watch`] over the node's [`Cx`]: cancel from the run, progress mapped into `lo..hi`.
struct CxWatch<'c, 'a> {
    cx: &'c mut Cx<'a>,
    lo: f64,
    hi: f64,
    stage: String,
}

impl Watch for CxWatch<'_, '_> {
    fn cancelled(&self) -> bool {
        self.cx.cancelled()
    }

    fn progress(&mut self, p: &Progress) {
        let frac = p.total.map_or(0.0, |t| (p.progress / t).clamp(0.0, 1.0));
        let stage = p.message.clone().unwrap_or_else(|| self.stage.clone());
        self.cx.progress(self.lo + (self.hi - self.lo) * frac, Some(&stage));
    }
}

struct Runner<'s> {
    s: &'s Shared,
    plan: &'s Plan,
    timeout: Duration,
}

/// The input document of a node: its handle and a file holding it.
struct DocIn {
    handle: Handle,
    path: PathBuf,
}

impl Runner<'_> {
    fn name(&self) -> &str {
        &self.s.manifest.name
    }

    fn run(&mut self, inputs: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
        let (params, command) = self.params(inputs)?;
        let deadline = Instant::now() + self.timeout;
        match self.plan.kind {
            Kind::Open => {
                let src = self.source(inputs)?;
                let mut lease = self.lease(None, deadline, cx)?;
                cx.progress(0.05, Some("opening"));
                self.reset(&mut lease, cx)?;
                let mut p = params;
                let open = self.s.manifest.actions.open.clone();
                p.insert(open.path.clone().unwrap_or_else(|| "path".into()), Value::from(src.display().to_string()));
                let result = self.action(&mut lease, &open, p, cx, 0.1, 0.8, "opening")?;
                self.doc_out(&mut lease, result, cx)
            }
            Kind::New => {
                let mut lease = self.lease(None, deadline, cx)?;
                self.reset(&mut lease, cx)?;
                let cmd = command.ok_or_else(|| config("no command"))?;
                let result = self.command(&mut lease, &cmd, params, cx, 0.1, 0.8)?;
                self.doc_out(&mut lease, result, cx)
            }
            Kind::Transform => {
                let doc = self.doc_in(inputs)?;
                let mut lease = self.lease(Some(&doc.handle), deadline, cx)?;
                self.ensure(&mut lease, &doc, cx)?;
                // From here the process's document may change without a save.
                if let Some(p) = lease.proc() {
                    p.active = None;
                }
                self.select(&mut lease, inputs, cx)?;
                let cmd = command.ok_or_else(|| config("no command"))?;
                let result = self.command(&mut lease, &cmd, params, cx, 0.1, 0.85)?;
                self.doc_out(&mut lease, result, cx)
            }
            Kind::Query => {
                let doc = self.doc_in(inputs)?;
                let mut lease = self.lease(Some(&doc.handle), deadline, cx)?;
                self.ensure(&mut lease, &doc, cx)?;
                self.select(&mut lease, inputs, cx)?;
                let cmd = command.ok_or_else(|| config("no command"))?;
                let result = self.command(&mut lease, &cmd, params, cx, 0.1, 0.95)?;
                let base = if self.plan.outputs.is_empty() { Outputs::from([("result".to_owned(), Data::Json(result.clone()))]) } else { Outputs::new() };
                Ok(self.lifted(&result, base))
            }
            Kind::Export => {
                let doc = self.doc_in(inputs)?;
                let mut lease = self.lease(Some(&doc.handle), deadline, cx)?;
                self.ensure(&mut lease, &doc, cx)?;
                self.export(&mut lease, params, cx)
            }
        }
    }

    /// The command and its parameters from the widgets: optional ones left out while null,
    /// `into` ones nested, template ones substituted into the command.
    fn params(&self, inputs: &Inputs) -> Result<(Map<String, Value>, Option<String>), NodeError> {
        let mut params = self.plan.fixed.clone();
        let mut command = self.plan.command.clone();
        for w in &self.plan.widgets {
            let v = inputs.get(&w.name).and_then(Data::json).cloned().unwrap_or(Value::Null);
            if w.template {
                let s = v.as_str().ok_or_else(|| config(format!("{} must be text", w.name)))?;
                if let Some(allowed) = &w.allowed
                    && !allowed.iter().any(|a| a == s)
                {
                    return Err(config(format!("{s:?} is not one of the commands this node may run")));
                }
                if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_') || s.len() > 200 {
                    return Err(config(format!("{} {s:?} is not a command id", w.name)));
                }
                command = command.map(|c| c.replace(&format!("{{{}}}", w.name), s));
                continue;
            }
            if v.is_null() && w.optional {
                continue;
            }
            // a parameter another choice brings isn't this node's to send
            if let Some((ctrl, want)) = &w.when
                && inputs.get(ctrl).and_then(Data::json) != Some(want)
            {
                continue;
            }
            let v = match &w.encode {
                Encode::AsIs => v,
                Encode::BoolAsNumber => json!(if v.as_bool().unwrap_or(false) { 1.0 } else { 0.0 }),
                Encode::Index(labels) => match labels.iter().position(|l| Some(l.as_str()) == v.as_str()) {
                    Some(i) => json!(i as f64),
                    None => return Err(config(format!("{} must be one of {}", w.name, labels.join(", ")))),
                },
            };
            if w.key == "*" {
                match v {
                    Value::Object(m) => params.extend(m),
                    Value::Null => {}
                    other => return Err(config(format!("{} must be a JSON object, not {}", w.name, short(&other.to_string())))),
                }
                continue;
            }
            match &w.into {
                Some(outer) => {
                    let slot = params.entry(outer.clone()).or_insert_with(|| json!({}));
                    match slot.as_object_mut() {
                        Some(m) => {
                            m.insert(w.key.clone(), v);
                        }
                        None => return Err(config(format!("{outer} is not an object"))),
                    }
                }
                None => {
                    params.insert(w.key.clone(), v);
                }
            }
        }
        for f in &self.plan.files {
            match inputs.get(&f.name).and_then(Data::blob) {
                Some(b) => {
                    let path = self.staged(&f.name, b)?;
                    params.insert(f.key.clone(), Value::from(path.display().to_string()));
                }
                None if f.optional => {}
                None => return Err(config(format!("{}: wire in a file", f.name))),
            }
        }
        Ok((params, command))
    }

    /// A process to run on, its wait and any start counted as setup, not work ([`Cx::setup_took`]).
    fn lease(&self, want: Option<&plumcraft_proto::Handle>, deadline: std::time::Instant, cx: &mut Cx) -> Result<Lease<'_>, NodeError> {
        let t0 = std::time::Instant::now();
        let lease = self.s.pool.lease(want, deadline);
        cx.setup_took(t0.elapsed());
        lease
    }

    /// A blob wired into a file socket, in the craft's work folder: its path.
    fn staged(&self, name: &str, b: &Blob) -> Result<PathBuf, NodeError> {
        let ext = ext_for(&b.meta.media).unwrap_or("bin");
        let staged = match &b.source {
            BlobSource::File(p) if self.s.cas.contains(p) => {
                crate::cas::touch(p);
                return Ok(p.clone());
            }
            BlobSource::File(p) if !p.is_file() => {
                return Err(NodeError::new(NodeError::MISSING_SOURCE, format!("{name}: {} is gone; rerun upstream", b.handle)));
            }
            BlobSource::File(p) => self.s.cas.stage_file(p),
            BlobSource::Bytes(bytes) => self.s.cas.stage_bytes(bytes, &format!("{name}.{ext}")),
        };
        staged.map(|(_, p)| p).map_err(|e| op(format!("staging {name}: {e}")))
    }

    /// The file an open node opens: an upstream image blob, else the `file` path.
    fn source(&self, inputs: &Inputs) -> Result<PathBuf, NodeError> {
        let staged = if let Some(b) = inputs.get("image").or_else(|| inputs.get("svg")).or_else(|| inputs.get("pdf")).and_then(Data::blob) {
            let ext = ext_for(&b.meta.media).unwrap_or("png");
            match &b.source {
                BlobSource::File(p) if self.s.cas.contains(p) => {
                    crate::cas::touch(p);
                    return Ok(p.clone());
                }
                BlobSource::File(p) => self.s.cas.stage_file(p),
                BlobSource::Bytes(bytes) => self.s.cas.stage_bytes(bytes, &format!("image.{ext}")),
            }
        } else {
            let path = inputs.get("file").and_then(Data::as_str).filter(|s| !s.is_empty()).ok_or_else(|| config("file: choose a file to open"))?;
            let p = Path::new(path);
            if !p.is_file() {
                return Err(NodeError::new(NodeError::MISSING_SOURCE, format!("file: {path} isn't a readable file")));
            }
            self.s.cas.stage_file(p)
        };
        staged.map(|(_, p)| p).map_err(|e| op(format!("staging the source: {e}")))
    }

    fn doc_in(&self, inputs: &Inputs) -> Result<DocIn, NodeError> {
        let b = inputs.get("doc").and_then(Data::blob).ok_or_else(|| config(format!("doc: wire a {} document", self.name())))?;
        let ext = &self.s.manifest.doc.ext;
        let path = match &b.source {
            BlobSource::File(p) => p.clone(),
            BlobSource::Bytes(bytes) => self.s.cas.stage_bytes(bytes, &format!("{}.{ext}", self.s.manifest.doc.stem)).map_err(|e| op(e.to_string()))?.1,
        };
        if !path.is_file() {
            return Err(NodeError::new(NodeError::MISSING_SOURCE, format!("doc: {} is no longer in the craft CAS; rerun upstream", b.handle)));
        }
        let path = if self.s.cas.contains(&path) { path } else { self.s.cas.stage_file(&path).map_err(|e| op(e.to_string()))?.1 };
        crate::cas::touch(&path);
        Ok(DocIn { handle: b.handle.clone(), path })
    }

    /// Put `doc` in the leased process as its active document: nothing when it is already
    /// (warm), else close everything and open the file (cold).
    fn ensure(&self, lease: &mut Lease<'_>, doc: &DocIn, cx: &mut Cx) -> Result<(), NodeError> {
        if lease.proc().is_some_and(|p| p.active.as_ref() == Some(&doc.handle)) {
            return Ok(());
        }
        cx.progress(0.03, Some("opening"));
        self.reset(lease, cx)?;
        let open = self.s.manifest.actions.open.clone();
        let mut p = Map::new();
        p.insert(open.path.clone().unwrap_or_else(|| "path".into()), Value::from(doc.path.display().to_string()));
        self.action(lease, &open, p, cx, 0.03, 0.08, "opening")?;
        if let Some(p) = lease.proc() {
            // Open → save is the identity (note 14, 1.6, with SOURCE_DATE_EPOCH pinned): the
            // process now holds exactly `doc`.
            p.active = Some(doc.handle.clone());
            p.since_clear = 0;
        }
        Ok(())
    }

    /// Close every document: with `close_all`, or, when the manifest has none or it fails (a
    /// craft that refuses to close when nothing is open), by starting a fresh process. A process
    /// that has run nothing holds nothing and is left as it is.
    fn reset(&self, lease: &mut Lease<'_>, cx: &mut Cx) -> Result<(), NodeError> {
        let fresh = match lease.proc() {
            Some(p) => {
                p.active = None;
                p.fresh
            }
            None => false,
        };
        if fresh {
            return Ok(());
        }
        let closed = match self.s.manifest.actions.close_all.clone() {
            Some(close) => self.action(lease, &close, Map::new(), cx, 0.0, 0.0, "closing").is_ok(),
            None => false,
        };
        if let Some(p) = lease.proc() {
            p.doc_id = None;
        }
        if !closed {
            cx.progress(0.0, Some("restarting"));
            lease.respawn()?;
        }
        Ok(())
    }

    /// Compile `select` (and the `ids` socket) into selection commands (note 14, 3.3).
    fn select(&self, lease: &mut Lease<'_>, inputs: &Inputs, cx: &mut Cx) -> Result<(), NodeError> {
        if !self.plan.select {
            return Ok(());
        }
        let Some(sel) = self.s.manifest.selection.clone() else { return Ok(()) };
        let wired = inputs.get("ids").and_then(Data::json).filter(|v| !v.is_null());
        // With a `set_key` the craft names objects by string too (a clip id, a hex handle).
        let strings = sel.set_key.is_some() || sel.string_ids;
        let one_id = |x: &Value| -> Option<Value> {
            match x {
                Value::Number(n) if n.as_u64().is_some() => Some(x.clone()),
                Value::String(t) if strings && !t.is_empty() && t.len() <= MAX_ID_LEN && !t.chars().any(char::is_control) => Some(x.clone()),
                _ => None,
            }
        };
        let ids_from = |v: &Value| -> Result<Vec<Value>, NodeError> {
            let list = v.as_array().ok_or_else(|| config("ids: a JSON list of object ids"))?;
            if list.len() > MAX_IDS {
                return Err(config(format!("ids: more than {MAX_IDS}")));
            }
            list.iter().map(|x| one_id(x).ok_or_else(|| config(format!("ids: {} is not an object id", short(&x.to_string()))))).collect()
        };
        let text = inputs.get("select").and_then(Data::as_str).unwrap_or("all").trim().to_owned();
        if text.len() > MAX_SELECT {
            return Err(config("select: too long"));
        }
        let (command, params, counted) = if let Some(v) = wired {
            (sel.set.clone(), json!({"ids": ids_from(v)?}), true)
        } else if text.is_empty() || text == "none" {
            (sel.none.clone(), json!({}), false)
        } else if text == "all" {
            (sel.all.clone(), json!({}), true)
        } else if let Some(list) = text.strip_prefix("ids:") {
            let parsed: Option<Vec<Value>> = list
                .split(',')
                .map(str::trim)
                // With `string_ids`, every id stays a string: `101` may be a hex handle, not 0x65.
                .map(|t| match (sel.string_ids, t.parse::<u64>()) {
                    (false, Ok(n)) => Some(Value::from(n)),
                    _ => one_id(&Value::from(t)),
                })
                .collect();
            let what = if strings { "ids, comma separated" } else { "whole numbers, comma separated" };
            let ids = parsed.ok_or_else(|| config(format!("select: {text:?}: ids are {what}")))?;
            if ids.len() > MAX_IDS {
                return Err(config(format!("select: more than {MAX_IDS} ids")));
            }
            (sel.set.clone(), json!({"ids": ids}), true)
        } else if let Some(c) = text.strip_prefix("color:") {
            let cmd = sel.color.clone().ok_or_else(|| config("select: this craft can't select by colour"))?;
            (cmd, json!({"fill": c.trim()}), true)
        } else {
            let filter = if let Some(j) = text.strip_prefix("json:") {
                serde_json::from_str::<Value>(j).ok().filter(Value::is_object).ok_or_else(|| config("select: json: takes a JSON object of find filters"))?
            } else if let Some(k) = text.strip_prefix("kind:") {
                json!({"kind": k.trim()})
            } else if let Some(n) = text.strip_prefix("name:") {
                json!({"name": n.trim()})
            } else {
                return Err(config(format!("select: {text:?}: use all, none, ids:1,2, color:#rrggbb, kind:Path, name:… or json:{{…}}")));
            };
            let find = sel.find.clone().ok_or_else(|| config("select: this craft can't find objects"))?;
            let mut f = filter.as_object().cloned().unwrap_or_default();
            f.entry("limit").or_insert(json!(MAX_IDS));
            let found = self.command(lease, &find, f, cx, 0.08, 0.1)?;
            let ids: Vec<Value> = found
                .get("matches")
                .and_then(Value::as_array)
                .map(|m| m.iter().filter_map(|x| x.get("id").and_then(one_id)).take(MAX_IDS).collect())
                .unwrap_or_default();
            if ids.is_empty() {
                return Err(config(format!("select: {text:?} matched nothing")));
            }
            (sel.set.clone(), json!({"ids": ids}), true)
        };
        if counted && params.get("ids").and_then(Value::as_array).is_some_and(Vec::is_empty) {
            return Err(config("select: the ids list is empty"));
        }
        let mut params = params.as_object().cloned().unwrap_or_default();
        if let Some(key) = sel.set_key.as_ref()
            && let Some(ids) = params.remove("ids")
        {
            params.insert(key.clone(), ids);
        }
        let reply = self.command(lease, &command, params, cx, 0.08, 0.1)?;
        if counted && reply.get("count").and_then(Value::as_u64) == Some(0) {
            return Err(config(format!("select: {:?} selected nothing", if wired.is_some() { "ids" } else { text.as_str() })));
        }
        Ok(())
    }

    /// Run a command through the run tool.
    fn command(&self, lease: &mut Lease<'_>, id: &str, params: Map<String, Value>, cx: &mut Cx, lo: f64, hi: f64) -> Result<Value, NodeError> {
        let a = Action { command: Some(id.to_owned()), ..Default::default() };
        self.action(lease, &a, params, cx, lo, hi, &format!("running {id}"))
    }

    /// Run an action (a command or a tool) and map every failure (note 14, 3.8).
    #[allow(clippy::too_many_arguments)]
    fn action(&self, lease: &mut Lease<'_>, a: &Action, params: Map<String, Value>, cx: &mut Cx, lo: f64, hi: f64, stage: &str) -> Result<Value, NodeError> {
        let mut params = params;
        for (k, v) in &a.params {
            params.entry(k.clone()).or_insert_with(|| v.clone());
        }
        // A craft that names documents by id gets the open one's id on every call but the open.
        if let (Some(key), None) = (&self.s.manifest.doc.id_param, &a.returns)
            && let Some(id) = lease.proc().and_then(|p| p.doc_id.clone())
        {
            params.entry(key.clone()).or_insert(id);
        }
        let run = &self.s.manifest.tools.run;
        let (tool, args, what) = match (&a.tool, &a.command) {
            (Some(t), _) => (t.clone(), Value::Object(params), t.clone()),
            (None, Some(c)) => {
                let mut args = Map::new();
                args.insert(run.id.clone(), Value::from(c.as_str()));
                args.insert(run.params.clone(), Value::Object(params));
                (run.tool.clone(), Value::Object(args), c.clone())
            }
            (None, None) => return Err(engine_error("the manifest names an action with neither a tool nor a command")),
        };
        cx.progress(lo, Some(stage));
        let timeout = self.timeout;
        let Some(proc) = lease.proc() else { return Err(engine_error("no process")) };
        proc.fresh = false;
        let mut watch = CxWatch { cx, lo, hi, stage: stage.to_owned() };
        let out = proc.client.tool(&tool, args, timeout, &mut watch);
        match out {
            Ok(r) if r.is_error => {
                // The command failed part way: the document may have changed.
                if let Some(p) = lease.proc() {
                    p.active = None;
                }
                let msg = short(&r.text);
                let m = &self.s.manifest.errors;
                if m.config.iter().any(|p| r.text.contains(p.as_str())) { Err(config(msg)) } else { Err(op(msg)) }
            }
            Ok(r) => {
                let mut v = r.value.unwrap_or(Value::Null);
                if let Some(path) = &a.returns {
                    let id = dig(&v, path);
                    if let (false, Some(p)) = (id.is_null(), lease.proc()) {
                        p.doc_id = Some(id);
                    }
                }
                if let Some(o) = v.as_object_mut() {
                    o.retain(|k, _| !self.s.manifest.volatile.contains(k));
                }
                Ok(v)
            }
            Err(e) => {
                let err = self.call_error(&e, &what);
                lease.discard();
                if matches!(e, CallError::Dead { .. } | CallError::Timeout { .. } | CallError::Io(_)) {
                    self.s.pool.crashed();
                }
                Err(err)
            }
        }
    }

    fn call_error(&self, e: &CallError, what: &str) -> NodeError {
        let name = self.name();
        match e {
            CallError::Cancelled => NodeError::cancelled(),
            CallError::Timeout { after, .. } => {
                engine_error(format!("{name} did not answer `{what}` in {:.0} s; the process was restarted", after.as_secs_f64()))
            }
            CallError::Dead { reason, stderr } => {
                let tail = if stderr.is_empty() { String::new() } else { format!("; stderr: {}", short(stderr)) };
                engine_error(format!("{name} stopped during `{what}` ({reason}){tail}; the process is replaced on the next run"))
            }
            CallError::Rpc(p) if p.code.code() == -32603 => op(format!("{name} bug in `{what}`: {}", short(&p.message))),
            CallError::Rpc(p) => engine_error(format!("{name} refused `{what}`: {}", short(&p.message))),
            CallError::Io(m) => engine_error(format!("{name}: {m}")),
        }
    }

    /// Save the active document into the CAS and return the node's outputs: `doc`, `result`
    /// and the outputs lifted from the reply.
    fn doc_out(&self, lease: &mut Lease<'_>, result: Value, cx: &mut Cx) -> Result<Outputs, NodeError> {
        let m = &self.s.manifest;
        let dir = self.s.cas.tmp_dir().map_err(|e| op(format!("work folder: {e}")))?;
        let file = dir.join(format!("{}.{}", m.doc.stem, m.doc.ext));
        let save = m.actions.save.clone();
        let mut p = Map::new();
        p.insert(save.path.clone().unwrap_or_else(|| "path".into()), Value::from(file.display().to_string()));
        let saved = self.action(lease, &save, p, cx, 0.88, 0.95, "saving");
        let adopted = saved.and_then(|_| self.s.cas.adopt(&file, &m.doc.ext).map_err(|e| op(format!("saving the document: {e}"))));
        let _ = std::fs::remove_dir_all(&dir);
        let (handle, path, size) = adopted?;
        let mut clear = false;
        if let Some(p) = lease.proc() {
            p.active = Some(handle.clone());
            p.since_clear += 1;
            clear = p.since_clear >= m.pool.clear_every;
        }
        if clear && let Some(c) = m.actions.clear_history.clone() {
            // Best effort: a document with no history ("no history to clear") is fine. The
            // document itself is unchanged by clearing, so it stays the saved handle.
            let cleared = self.action(lease, &c, Map::new(), cx, 0.95, 0.95, "clearing history");
            if let Some(p) = lease.proc() {
                p.since_clear = 0;
                if cleared.is_err() && !p.client.is_dead() {
                    p.active = Some(handle.clone());
                }
            }
            if let Err(e) = cleared
                && e.kind == NodeError::ENGINE
            {
                return Err(e);
            }
        }
        let thumb = if size <= m.thumb.as_ref().map_or(0, |t| t.max_doc_bytes) { self.thumb(lease, cx) } else { None };
        let meta = BlobMeta { media: m.doc.media.clone(), size, form: Form::Doc { app: m.doc.app.clone() }, aliases: Vec::new() };
        // Published like any value the UI sees, so the commonkit store keeps it under its own
        // budget and TTL; the CAS file is only the craft's working copy (`cas.rs`).
        let doc = Data::Blob(Box::new(Blob { handle, meta, source: BlobSource::File(path), publish: true, thumb }));
        let mut out = Outputs::from([("doc".to_owned(), doc), ("result".to_owned(), Data::Json(result.clone()))]);
        out = self.lifted(&result, out);
        Ok(out)
    }

    /// Outputs taken from the reply by `from` path (the whole reply without one).
    fn lifted(&self, result: &Value, mut out: Outputs) -> Outputs {
        for o in &self.plan.outputs {
            let v = match &o.from {
                Some(path) => dig(result, path),
                None => result.clone(),
            };
            out.insert(o.name.clone(), Data::Json(v));
        }
        out
    }

    /// A small PNG of the active document, decoded to `rgba8`. Best effort: a failure is no thumb.
    fn thumb(&self, lease: &mut Lease<'_>, cx: &mut Cx) -> Option<Array> {
        let m = &self.s.manifest;
        let t = m.thumb.as_ref()?;
        let export = m.actions.export.clone()?;
        let info = self.command(lease, &t.size_command, t.size_params.clone(), cx, 0.95, 0.95).ok()?;
        let ab = info.get("artboards")?.get(0)?;
        let (w, h) = (ab.get("width")?.as_f64()?, ab.get("height")?.as_f64()?);
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
            return None;
        }
        let scale = f64::from(t.px) / w.max(h);
        let dir = self.s.cas.tmp_dir().ok()?;
        let file = dir.join("thumb.png");
        let mut p = Map::new();
        p.insert(export.path.clone().unwrap_or_else(|| "path".into()), Value::from(file.display().to_string()));
        p.insert("format".into(), json!("png"));
        p.insert("scale".into(), json!(scale));
        let ok = self.action(lease, &export, p, cx, 0.96, 0.98, "thumbnail").is_ok();
        let bytes = if ok { std::fs::read(&file).ok() } else { None };
        let _ = std::fs::remove_dir_all(&dir);
        crate::thumb::rgba8(&bytes?, t.px)
    }

    /// Export the active document; the file goes into the CAS and out as a published blob.
    fn export(&self, lease: &mut Lease<'_>, params: Map<String, Value>, cx: &mut Cx) -> Result<Outputs, NodeError> {
        let (format, ext, media) = self.plan.export.clone().ok_or_else(|| engine_error("an export node without a format"))?;
        let own = self.plan.export_action.clone();
        let export = match &own {
            Some(a) => a.clone(),
            None => self.s.manifest.actions.export.clone().ok_or_else(|| engine_error("the manifest has no export action"))?,
        };
        let dir = self.s.cas.tmp_dir().map_err(|e| op(format!("work folder: {e}")))?;
        let file = dir.join(format!("{}.{ext}", self.s.manifest.doc.stem));
        let mut p = params;
        p.insert(export.path.clone().unwrap_or_else(|| "path".into()), Value::from(file.display().to_string()));
        if own.is_none() {
            p.insert("format".into(), Value::from(format.as_str()));
        }
        let what = if format.is_empty() { ext.clone() } else { format.clone() };
        let result = self.action(lease, &export, p, cx, 0.1, 0.9, &format!("exporting {what}"));
        let adopted = result.and_then(|r| self.s.cas.adopt(&file, &ext).map(|a| (a, r)).map_err(|e| op(format!("the export: {e}"))));
        let _ = std::fs::remove_dir_all(&dir);
        let ((handle, path, size), result) = adopted?;
        let (form, thumb) = describe(&path, &ext, self.s.manifest.thumb.as_ref().map_or(120, |t| t.px));
        // An SVG or PDF has no thumb of its own: the document it was exported from gives one
        // (M3: every export node shows what it exported).
        let thumb = thumb.or_else(|| self.thumb(lease, cx));
        let meta = BlobMeta { media, size, form, aliases: Vec::new() };
        let out_name = self.plan.outputs.first().map_or_else(|| ext.clone(), |o| o.name.clone());
        let blob = Data::Blob(Box::new(Blob { handle, meta, source: BlobSource::File(path), publish: true, thumb }));
        Ok(Outputs::from([(out_name, blob), ("result".to_owned(), Data::Json(result))]))
    }
}

/// An exported file's form, and a thumbnail when it is a PNG.
fn describe(path: &Path, ext: &str, px: u32) -> (Form, Option<Array>) {
    match ext {
        "png" => {
            let bytes = std::fs::read(path).ok();
            let size = bytes.as_deref().and_then(crate::thumb::png_size);
            let form = size.map_or(Form::Bytes, |(w, h)| Form::Image { width: w, height: h });
            // Thumbs from files up to 16 MB; bigger PNGs are left to the viewer.
            let thumb = bytes.filter(|b| b.len() <= 16 << 20).and_then(|b| crate::thumb::rgba8(&b, px));
            (form, thumb)
        }
        "jpg" | "jpeg" => (Form::Image { width: 0, height: 0 }, None),
        "svg" => (svg_size(path).map_or(Form::Vector { width: 0.0, height: 0.0 }, |(w, h)| Form::Vector { width: w, height: h }), None),
        _ => (Form::Bytes, None),
    }
}

/// An SVG's `width` and `height` attributes, from its first 4 KB.
fn svg_size(path: &Path) -> Option<(f64, f64)> {
    use std::io::Read;
    let mut head = vec![0u8; 4096];
    let n = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
    let text = String::from_utf8_lossy(head.get(..n)?).into_owned();
    let tag = text.get(text.find("<svg")?..)?;
    let tag = tag.get(..tag.find('>')?)?;
    let attr = |name: &str| -> Option<f64> {
        let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
        let v = tag.get(at..)?;
        let v = v.get(..v.find('"')?)?;
        v.trim_end_matches(|c: char| c.is_ascii_alphabetic()).parse().ok()
    };
    Some((attr("width")?, attr("height")?))
}

/// The extension for a media type.
fn ext_for(media: &str) -> Option<&'static str> {
    match media {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/svg+xml" => Some("svg"),
        "application/pdf" => Some("pdf"),
        _ => None,
    }
}

/// `a.b.0.c` into a JSON value; null when it isn't there.
pub fn dig(v: &Value, path: &str) -> Value {
    let mut cur = v;
    for part in path.split('.').filter(|p| !p.is_empty()) {
        let next = match part.parse::<usize>() {
            Ok(i) => cur.get(i),
            Err(_) => cur.get(part),
        };
        match next {
            Some(n) => cur = n,
            None => return Value::Null,
        }
    }
    cur.clone()
}

fn short(s: &str) -> String {
    let s = s.trim().trim_start_matches("command_run: ");
    if s.chars().count() <= MAX_MESSAGE { s.to_owned() } else { format!("{}…", s.chars().take(MAX_MESSAGE).collect::<String>()) }
}

fn config(m: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::CONFIG, m)
}

fn op(m: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::OP, m)
}

/// Tool errors by node, for the smoke report.
pub type Failures = BTreeMap<String, String>;
