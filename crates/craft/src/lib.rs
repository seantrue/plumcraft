//! The craft MCP adapter (plan 10.6, milestone M; note 14, `docs/research/14-craft-mcp-adapter.md`):
//! an artcraft app's MCP server as a PlumCraft engine (spec 16.5). It owns the graph walk and
//! emits every state; the craft is invisible to the UI, which sees NodeDefs, records and DataRefs
//! as with any engine.
//!
//! - [`mcp`]: the hand-rolled MCP client over a child's stdio (D40), cancel = notify then kill (D41).
//! - [`pool`]: hermetic headless processes under automation roots, warm reuse, timeouts,
//!   crash accounting (D46, D49).
//! - [`cas`]: the craft CAS, the craft's only read root; documents are `md5:raw:` of their
//!   uncompressed native save (D43).
//! - [`manifest`]: everything craft-specific; [`grammar`]: the `params` prose grammar and port
//!   inference (note 14, 2.4).
//! - [`nodes`]: the node runner: selection (D44), commands, saves, exports, thumbnails, errors.
//!
//! [`build`] starts one process, reads the command catalogue and the manifest's typed
//! catalogues, and turns the manifest into nodes: curated ones, generated ones (VectorCraft's
//! effects) and the escape hatch (D45). The memo salt is the craft's id, `serverInfo.version`
//! and the MD5 of its executable (D47).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod cas;
pub mod coverage;
mod families;
pub mod grammar;
pub mod manifest;
pub mod mcp;
pub mod nodes;
pub mod pool;
pub mod thumb;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use plumcraft_engine::Engine;
use plumcraft_proto::catalogue::{Media, MemoScope, MemoSpec, NodeDef, PathHint, Port, Socket, WireType};
use plumcraft_proto::session::EngineInfo;
use serde_json::{Map, Value, json};

use crate::cas::Cas;
use crate::manifest::{Kind, Manifest, NodeSpec, WidgetSpec};
use crate::mcp::NoWatch;
use crate::nodes::{CraftNode, FilePlan, Plan, Shared, WidgetPlan};
use crate::pool::{Launch, Pool};

pub use manifest::builtin;

/// The core IMAGE type (`core_types::IMAGE`).
pub const IMAGE: &str = plumcraft_proto::catalogue::core_types::IMAGE;
/// The core SVG type (`core_types::SVG`).
pub const SVG: &str = plumcraft_proto::catalogue::core_types::SVG;
pub const PDF: &str = "PDF";
/// The core JSON type (`core_types::JSON`).
pub const JSON: &str = plumcraft_proto::catalogue::core_types::JSON;

/// How long the catalogue may take at engine start.
const CATALOGUE_TIMEOUT: Duration = Duration::from_secs(60);
/// The most commands or catalogue entries read.
const MAX_ENTRIES: usize = 20_000;

/// Where the adapter keeps its files, and which store it publishes to.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The CAS root; default [`pool::default_root`].
    pub root: Option<PathBuf>,
    /// `Some(None)`: publish nothing; `Some(Some(root))`: that store; `None`: commonkit's default.
    pub store: Option<Option<PathBuf>>,
    /// Build even the nodes the manifest excludes (the smoke test re-checks them).
    pub include_excluded: bool,
}

/// What [`build`] made of the manifest.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub curated: Vec<String>,
    pub generated: Vec<String>,
    pub hatch: Vec<String>,
    /// Nodes not listed, with why: excluded by the manifest, or failing the catalogue check.
    pub dropped: Vec<(String, String)>,
    pub server_version: String,
    /// The pack's version (B17), and the app versions it was made for.
    pub pack_version: String,
    pub salt: String,
    /// The craft's command catalogue: id → params description.
    pub commands: BTreeMap<String, String>,
    /// id → params as a JSON Schema, for crafts whose list gives schemas (PdfCraft).
    pub schemas: BTreeMap<String, Value>,
    /// Nodes built with a doubt, with why: a key the catalogue check couldn't confirm because
    /// the command's description is prose the grammar reads only in part.
    pub warnings: Vec<(String, String)>,
}

/// The command catalogue and the typed catalogues, read once.
#[derive(Debug, Default)]
pub struct Catalogue {
    /// id → params description (prose; empty for a command whose params are a schema).
    pub commands: BTreeMap<String, String>,
    /// id → params as a JSON Schema object, for crafts whose list gives schemas (PdfCraft).
    pub schemas: BTreeMap<String, Value>,
    pub lists: BTreeMap<String, Vec<Value>>,
    /// id → its label and menu path, where the list gives them (generated nodes' names).
    pub info: BTreeMap<String, (String, Vec<String>)>,
}

/// The engine for a manifest, in the default places.
pub fn engine(manifest: &Manifest) -> Result<Engine, String> {
    build(manifest, &Options::default()).map(|b| b.0)
}

/// Build the engine: start a process, read the catalogues, make the nodes.
pub fn build(manifest: &Manifest, opts: &Options) -> Result<(Engine, Report, Arc<Shared>), String> {
    let root = opts.root.clone().unwrap_or_else(|| pool::default_root(&manifest.id));
    let cas = Cas::new(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let exe = manifest.executable().ok_or_else(|| format!("{} isn't installed ({}); {}", manifest.name, manifest.program().join(" "), manifest.hint))?;
    let exe_digest = exe_digest(&exe, &root)?;
    let pool = Pool::new(Launch::new(manifest, cas.read_root(), cas.write_root()));
    let (catalogue, server_version) = read_catalogue(manifest, &pool)?;
    // The pack for this version of the app (B17, #56): another built-in one may fit it better.
    let (chosen, misfit) = manifest::choose(manifest, &server_version);
    if chosen.pack_version != manifest.pack_version || chosen.app_versions != manifest.app_versions {
        drop(pool);
        let chosen = Manifest { builtin: false, ..chosen };
        return build(&chosen, opts);
    }
    if let Some(w) = &misfit {
        eprintln!("plumcraft-craft: {w}");
    }
    let manifest_digest = plumcraft_proto::Handle::md5_raw(format!("{manifest:?}").as_bytes());
    let salt = plumcraft_engine::salt(
        &format!("craft:{}:{}:{}:{}", manifest.id, server_version, exe_digest.digest(), manifest_digest.digest()),
        env!("PLUMCRAFT_CODE_DIGEST"),
    );
    let shared = Arc::new(Shared { manifest: manifest.clone(), pool, cas: cas.clone(), timeout: Duration::from_secs(manifest.timeout_s.max(1)) });
    let mut report = Report {
        server_version: server_version.clone(),
        salt: salt.clone(),
        commands: catalogue.commands.clone(),
        schemas: catalogue.schemas.clone(),
        pack_version: manifest.pack_version.clone(),
        ..Default::default()
    };
    if let Some(w) = misfit {
        report.warnings.push(("pack".into(), w));
    }
    let mut nodes: Vec<CraftNode> = Vec::new();
    for spec in &manifest.nodes {
        if let Some(why) = manifest.exclude.get(&spec.id).filter(|_| !opts.include_excluded) {
            report.dropped.push((spec.id.clone(), why.clone()));
            continue;
        }
        match curated(manifest, spec, &catalogue, &shared, &mut report.warnings) {
            Ok(n) => {
                report.curated.push(spec.id.clone());
                nodes.push(n);
            }
            Err(why) => report.dropped.push((spec.id.clone(), why)),
        }
    }
    for g in &manifest.generated {
        for entry in catalogue.lists.get(&g.catalogue).map(Vec::as_slice).unwrap_or_default() {
            let Some(eid) = entry.get("id").and_then(Value::as_str) else { continue };
            let id = format!("{}{eid}", g.prefix);
            if eid.starts_with("plugin.") {
                continue; // installed plug-ins aren't part of a hermetic catalogue
            }
            if let Some(why) = manifest.exclude.get(&id).filter(|_| !opts.include_excluded) {
                report.dropped.push((id, why.clone()));
                continue;
            }
            match generated(manifest, g, entry, &id, &catalogue, &shared) {
                Ok(n) => {
                    report.generated.push(id);
                    nodes.push(n);
                }
                Err(why) => report.dropped.push((id, why)),
            }
        }
    }
    for g in &manifest.generated_commands {
        for (command, id, why) in families::family(manifest, g, &catalogue) {
            if let Err(why) = why {
                report.dropped.push((id, why));
                continue;
            }
            if let Some(why) = manifest.exclude.get(&id).filter(|_| !opts.include_excluded) {
                report.dropped.push((id, why.clone()));
                continue;
            }
            match families::node(manifest, g, &command, &id, &catalogue) {
                Ok((def, plan)) => {
                    report.generated.push(id);
                    nodes.push(CraftNode { def, plan, shared: Arc::clone(&shared) });
                }
                Err(why) => report.dropped.push((id, why)),
            }
        }
    }
    if let Some(r) = &manifest.run_node {
        nodes.push(hatch(manifest, r, &catalogue, &shared));
        report.hatch.push(r.id.clone());
    }
    // the pack, beside the app's version (B17): what made these nodes
    let mut components = BTreeMap::from([("pack".to_owned(), manifest.pack_version.clone())]);
    if let Some(r) = &manifest.app_versions {
        components.insert("pack.app_versions".into(), r.clone());
    }
    let info = EngineInfo { id: manifest.id.clone(), name: manifest.name.clone(), version: server_version, salt, components, ..Default::default() };
    let mut e = Engine::new(info).with_work_root(cas.root().to_path_buf());
    if let Some(store) = &opts.store {
        e = e.with_store(store.clone());
    }
    for w in wire_types(manifest) {
        e = e.with_wire_type(w);
    }
    // the pack's own labels on its own ports (B18); the editor's tables go over them
    let labels = manifest.label_table();
    for why in &labels.skipped {
        report.warnings.push(("labels".into(), why.clone()));
    }
    for mut n in nodes {
        let kind = n.def.id.clone();
        labels.apply_ports(&kind, &mut n.def.inputs);
        labels.apply_ports(&kind, &mut n.def.outputs);
        e = e.with_node(Box::new(n));
    }
    Ok((e.with_building_blocks(), report, shared))
}

/// The MD5 of the craft's executable (D47), hashed once per (path, size, mtime) and remembered
/// in `<root>/exe-digest.json` (100 MB takes about 0.13 s optimised, seconds in a debug build).
pub fn exe_digest(exe: &Path, root: &Path) -> Result<plumcraft_proto::Handle, String> {
    let meta = std::fs::metadata(exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    let key = json!({"path": exe.display().to_string(), "size": meta.len(), "mtime_ns": mtime.to_string()});
    let file = root.join("exe-digest.json");
    let cached = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    if let Some(c) = cached.filter(|c| c.get("key") == Some(&key))
        && let Some(h) = c.get("digest").and_then(Value::as_str).and_then(|d| d.parse().ok())
    {
        return Ok(h);
    }
    let (h, _) = cas::hash_file(exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    let tmp = root.join(format!(".exe-digest.{}.tmp", std::process::id()));
    let body = json!({"key": key, "digest": h.to_string()}).to_string();
    let _ = std::fs::write(&tmp, body).and_then(|()| std::fs::rename(&tmp, &file));
    Ok(h)
}

fn wire_types(m: &Manifest) -> Vec<WireType> {
    let scalar = |id: &str| WireType { id: id.into(), media: Media::Scalar, ..Default::default() };
    let mut types = vec![
        WireType { id: m.doc.wire.clone(), color: m.doc.color.clone().or(Some("#c2410c".into())), media: Media::Doc, thumb: true, ..Default::default() },
        plumcraft_proto::catalogue::core_types::image(),
        // the core SVG type: an SVG crosses to any engine unchanged
        plumcraft_proto::catalogue::core_types::svg(),
        WireType { id: PDF.into(), color: Some("#9333ea".into()), media: Media::Bytes, ..Default::default() },
        plumcraft_proto::catalogue::core_types::json(),
        scalar("FLOAT"),
        scalar("INT"),
        scalar("BOOL"),
        scalar("STRING"),
        scalar("ENUM"),
    ];
    // What the pack's exports make (SoundCraft's WAV): bytes, declared so another engine that
    // declares the same type can take them.
    for ty in m.nodes.iter().flat_map(|n| n.outputs.iter().map(|o| &o.ty).chain(n.files.iter().map(|f| &f.ty))) {
        if !types.iter().any(|t| &t.id == ty) {
            types.push(WireType { id: ty.clone(), media: Media::Bytes, ..Default::default() });
        }
    }
    types
}

/// Read `command_list` and the manifest's typed catalogues from one process.
fn read_catalogue(m: &Manifest, pool: &Pool) -> Result<(Catalogue, String), String> {
    let mut lease = pool.lease(None, Instant::now() + CATALOGUE_TIMEOUT).map_err(|e| e.message)?;
    let proc = lease.proc().ok_or("no process")?;
    let version = proc.client.server_version.clone();
    let (source, value, text) = match &m.tools.list_resource {
        Some(uri) => {
            let r = proc.client.call("resources/read", json!({"uri": uri}), CATALOGUE_TIMEOUT, &mut NoWatch).map_err(|e| format!("{uri}: {e}"))?;
            let text = r.get("contents").and_then(|c| c.get(0)).and_then(|c| c.get("text")).and_then(Value::as_str).unwrap_or_default().to_owned();
            (uri.clone(), serde_json::from_str::<Value>(&text).ok(), text)
        }
        None => {
            let list = proc.client.tool(&m.tools.list, json!({}), CATALOGUE_TIMEOUT, &mut NoWatch).map_err(|e| format!("{}: {e}", m.tools.list))?;
            (m.tools.list.clone(), list.value, list.text)
        }
    };
    let items = match value {
        Some(Value::Array(a)) => a,
        Some(Value::Object(o)) => o.get("commands").and_then(Value::as_array).cloned().unwrap_or_default(),
        _ => return Err(format!("{source}: not a command list: {}", text.chars().take(200).collect::<String>())),
    };
    let mut cat = Catalogue::default();
    for c in items.iter().take(MAX_ENTRIES) {
        let Some(id) = c.get("id").and_then(Value::as_str) else { continue };
        let label = c.get("label").and_then(Value::as_str).unwrap_or_default().chars().take(200).collect::<String>();
        let menu: Vec<String> =
            c.get("menu").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).take(8).map(str::to_owned).collect()).unwrap_or_default();
        cat.info.insert(id.to_owned(), (label, menu));
        match c.get("params") {
            Some(Value::String(params)) => {
                cat.commands.insert(id.to_owned(), params.clone());
            }
            // A JSON Schema of the params, or null for a command that takes none.
            Some(schema @ Value::Object(_)) => {
                cat.commands.insert(id.to_owned(), String::new());
                cat.schemas.insert(id.to_owned(), schema.clone());
            }
            Some(Value::Null) => {
                cat.commands.insert(id.to_owned(), String::new());
                cat.schemas.insert(id.to_owned(), json!({"type": "object", "properties": {}}));
            }
            _ => {}
        }
    }
    let run = &m.tools.run;
    for spec in &m.catalogues {
        let mut args = Map::new();
        args.insert(run.id.clone(), Value::from(spec.command.as_str()));
        args.insert(run.params.clone(), Value::Object(spec.params.clone()));
        let r = proc.client.tool(&run.tool, Value::Object(args), CATALOGUE_TIMEOUT, &mut NoWatch).map_err(|e| format!("{}: {e}", spec.command))?;
        if r.is_error {
            return Err(format!("{}: {}", spec.command, r.text.chars().take(200).collect::<String>()));
        }
        let entries = r.value.as_ref().map(|v| nodes::dig(v, &spec.path)).and_then(|v| v.as_array().cloned()).unwrap_or_default();
        cat.lists.insert(spec.name.clone(), entries.into_iter().take(MAX_ENTRIES).collect());
    }
    Ok((cat, version))
}

/// The prose text of `key` in a command's description (a schema's `description`).
fn key_text(cat: &Catalogue, command: &str, key: &str) -> Option<String> {
    if let Some(schema) = cat.schemas.get(command) {
        return schema.get("properties")?.get(key)?.get("description")?.as_str().map(str::to_owned);
    }
    let doc = cat.commands.get(command)?;
    // Its own keys first; then those it imports (`…as other.command`).
    if let Some(text) = grammar::doc_keys(doc).ok().and_then(|k| k.keys.iter().find(|k| k.0 == key).map(|k| k.2.to_owned())) {
        return Some(text);
    }
    grammar::key_texts(doc, &|id| cat.commands.get(id).cloned(), 0)?.into_iter().find(|k| k.0 == key).map(|k| k.1)
}

/// A key's port from a command's JSON Schema: type, range, choices and default.
fn schema_infer(cat: &Catalogue, command: &str, key: &str) -> Option<grammar::Inferred> {
    let prop = cat.schemas.get(command)?.get("properties")?.get(key)?;
    let ty = match prop.get("type") {
        Some(Value::String(t)) => t.as_str(),
        // `["integer", "null"]`: the first that isn't null
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).find(|t| *t != "null").unwrap_or_default(),
        _ => "",
    };
    let choices: Vec<String> =
        prop.get("enum").and_then(Value::as_array).map(|e| e.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
    let ty = match ty {
        _ if !choices.is_empty() => "ENUM",
        "integer" => "INT",
        "number" => "FLOAT",
        "boolean" => "BOOL",
        "string" => "STRING",
        "array" | "object" => JSON,
        _ => "",
    };
    Some(grammar::Inferred {
        ty: ty.to_owned(),
        min: prop.get("minimum").and_then(Value::as_f64),
        max: prop.get("maximum").and_then(Value::as_f64),
        choices,
        default: prop.get("default").cloned().unwrap_or(Value::Null),
    })
}

/// The keys a command takes: from its schema's `properties` (any key, when it allows more), or
/// read from its prose.
fn accepted(cat: &Catalogue, command: &str) -> Option<Vec<String>> {
    if let Some(schema) = cat.schemas.get(command) {
        if schema.get("additionalProperties").is_some_and(|a| a != &Value::Bool(false)) {
            return None;
        }
        return Some(schema.get("properties").and_then(Value::as_object).map(|p| p.keys().cloned().collect()).unwrap_or_default());
    }
    let doc = cat.commands.get(command)?;
    grammar::accepted(doc, &|id| cat.commands.get(id).cloned(), 0)
}

/// A widget's port, from the manifest and, where it says nothing, the prose.
fn widget_port(w: &WidgetSpec, command: Option<&str>, cat: &Catalogue) -> Result<Port, String> {
    let key = w.key.clone().unwrap_or_else(|| w.name.clone());
    let text = command.filter(|_| w.into.is_none()).and_then(|c| key_text(cat, c, &key)).unwrap_or_default();
    let inferred = command.filter(|_| w.into.is_none()).and_then(|c| schema_infer(cat, c, &key)).unwrap_or_else(|| grammar::infer(&text));
    let choices = match &w.choices_from {
        Some(spec) => {
            let (list, field) = spec.split_once(':').ok_or_else(|| format!("{}: choices_from is <catalogue>:<field>", w.name))?;
            let entries = cat.lists.get(list).ok_or_else(|| format!("{}: no catalogue {list}", w.name))?;
            Some(entries.iter().filter_map(|e| e.get(field).cloned()).collect::<Vec<_>>())
        }
        None => w.choices.clone().or_else(|| (!inferred.choices.is_empty()).then(|| inferred.choices.iter().map(|c| Value::from(c.as_str())).collect())),
    };
    let ty = w.ty.clone().unwrap_or_else(|| if inferred.ty.is_empty() { "STRING".into() } else { inferred.ty.clone() });
    let default = if w.default.is_null() && !w.optional { inferred.default.clone() } else { w.default.clone() };
    let socket = match w.socket.as_deref() {
        Some("socket") => Some(Socket::Socket),
        Some("both") => Some(Socket::Both),
        Some("widget") => Some(Socket::Widget),
        Some(other) => return Err(format!("{}: socket {other:?}", w.name)),
        None if ty == JSON => Some(Socket::Both),
        None => None,
    };
    let about = text.trim_start_matches(['?', ':', ' ']).trim_end();
    let tooltip = w.tooltip.clone().or_else(|| (!about.is_empty()).then(|| about.chars().take(300).collect()));
    Ok(Port {
        name: w.name.clone(),
        ty: if ty == "ENUM" && choices.is_none() { "STRING".into() } else { ty },
        label: w.label.clone(),
        tooltip,
        default,
        min: w.min.or(inferred.min),
        max: w.max.or(inferred.max),
        choices,
        optional: w.optional,
        advanced: w.advanced,
        socket,
        ..Default::default()
    })
}

fn socket(name: &str, ty: &str) -> Port {
    Port { name: name.into(), ty: ty.into(), socket: Some(Socket::Socket), ..Default::default() }
}

fn select_ports(default: &str) -> [Port; 2] {
    [
        Port {
            name: "select".into(),
            ty: "STRING".into(),
            label: Some("select".into()),
            tooltip: Some("What the command acts on: all, none, ids:1,2, color:#rrggbb, kind:Path, name:… or json:{find filters}".into()),
            default: Value::from(default),
            advanced: true,
            ..Default::default()
        },
        Port {
            name: "ids".into(),
            ty: JSON.into(),
            tooltip: Some("Object ids to act on (a list, such as an upstream result's ids); overrides select".into()),
            optional: true,
            socket: Some(Socket::Socket),
            ..Default::default()
        },
    ]
}

fn memo(pure: bool, content: bool) -> MemoSpec {
    MemoSpec { scope: if pure { MemoScope::Machine } else { MemoScope::None }, content: content && pure, ..Default::default() }
}

/// A curated node: its NodeDef and how it runs, checked against the catalogue.
fn curated(m: &Manifest, spec: &NodeSpec, cat: &Catalogue, shared: &Arc<Shared>, warnings: &mut Vec<(String, String)>) -> Result<CraftNode, String> {
    // Every command the node can run exists.
    let keys = coverage::node_keys(m, spec);
    for c in &keys.runs {
        if !cat.commands.contains_key(c) {
            return Err(format!("{} has no command {c}", m.name));
        }
    }
    // Every key it sends is one the command accepts (the craft rejects any other, note 14, 1.4).
    let target = keys.target;
    if let Some(cmd) = &target
        && let Some(accepts) = accepted(cat, cmd)
    {
        for k in &keys.checked {
            if k != "*" && !accepts.contains(k) {
                // Prose read only in part may not list a key it takes: warn, don't drop.
                if cat.commands.get(cmd).is_some_and(|d| grammar::incomplete(d)) {
                    warnings.push((spec.id.clone(), format!("{cmd} may not take `{k}` (its description is prose, read in part)")));
                    continue;
                }
                return Err(format!("{cmd} doesn't take `{k}`"));
            }
        }
    }
    let mut inputs: Vec<Port> = Vec::new();
    match spec.kind {
        Kind::Open => {
            inputs.push(Port {
                name: "file".into(),
                ty: "STRING".into(),
                tooltip: Some(format!("A file {} opens: an image, an SVG, a PDF or a native document", m.name)),
                optional: true,
                path: Some(PathHint { mode: "open".into(), filters: Vec::new() }),
                ..Default::default()
            });
            inputs.push(Port { optional: true, ..socket("image", IMAGE) });
            if m.doc.opens_svg {
                inputs.push(Port { optional: true, ..socket("svg", SVG) });
            }
            if m.doc.opens_pdf {
                inputs.push(Port { optional: true, ..socket("pdf", PDF) });
            }
        }
        Kind::New => {}
        Kind::Transform | Kind::Query | Kind::Export => inputs.push(socket("doc", &m.doc.wire)),
    }
    if let Some(sel) = &spec.select {
        inputs.extend(select_ports(sel));
    }
    for f in &spec.files {
        if inputs.iter().any(|p| p.name == f.name) || spec.widgets.iter().any(|w| w.name == f.name) {
            return Err(format!("{}: two inputs are called {}", spec.id, f.name));
        }
        inputs.push(Port { optional: f.optional, tooltip: f.tooltip.clone(), ..socket(&f.name, &f.ty) });
    }
    let files: Vec<FilePlan> =
        spec.files.iter().map(|f| FilePlan { name: f.name.clone(), key: f.key.clone().unwrap_or_else(|| "path".into()), optional: f.optional }).collect();
    let mut widgets = Vec::new();
    let mut brought: Vec<(Port, WidgetPlan)> = Vec::new();
    for w in &spec.widgets {
        if let Some(from) = &w.brings {
            brought.extend(brings(w, from, cat)?);
        }
        let mut p = widget_port(w, target.as_deref().filter(|_| !w.template), cat)?;
        if w.template {
            p.ty = "ENUM".into();
            p.socket = Some(Socket::Widget);
        }
        inputs.push(p);
        widgets.push(WidgetPlan {
            name: w.name.clone(),
            key: w.key.clone().unwrap_or_else(|| w.name.clone()),
            into: w.into.clone(),
            optional: w.optional,
            template: w.template,
            allowed: w.template.then(|| w.choices.iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()),
            ..Default::default()
        });
    }
    // after the node's own widgets, so they land inside the object a JSON widget sets
    for (p, w) in brought {
        if inputs.iter().any(|q| q.name == p.name) {
            return Err(format!("{}: two inputs are called {}", spec.id, p.name));
        }
        inputs.push(p);
        widgets.push(w);
    }
    if spec.kind == Kind::Export
        && let Some(from) = &spec.options_from
    {
        for p in export_options(from, &spec.options, cat)? {
            widgets.push(WidgetPlan {
                name: p.name.clone(),
                key: p.name.clone(),
                into: None,
                optional: p.optional,
                template: false,
                allowed: None,
                ..Default::default()
            });
            inputs.push(p);
        }
    }
    if let Some(cmd) = target.as_deref().filter(|_| auto_on(m, spec)) {
        for (p, w) in auto_widgets(m, spec, cmd, cat, &inputs) {
            inputs.push(p);
            widgets.push(w);
        }
    }
    let mut outputs: Vec<Port> = Vec::new();
    match spec.kind {
        Kind::Open | Kind::New | Kind::Transform => {
            outputs.push(Port { name: "doc".into(), ty: m.doc.wire.clone(), ..Default::default() });
            outputs.push(Port { name: "result".into(), ty: JSON.into(), ..Default::default() });
        }
        Kind::Query => {}
        Kind::Export => {}
    }
    for o in &spec.outputs {
        outputs.push(Port { name: o.name.clone(), ty: o.ty.clone(), ..Default::default() });
    }
    if spec.kind == Kind::Export {
        outputs.push(Port { name: "result".into(), ty: JSON.into(), ..Default::default() });
    }
    if spec.kind == Kind::Query && spec.outputs.is_empty() {
        outputs.push(Port { name: "result".into(), ty: JSON.into(), ..Default::default() });
    }
    // An export node's own action: `action`, or `command` with the export action's path key.
    let export_action = match (spec.kind, &spec.action, &spec.command) {
        (Kind::Export, Some(a), _) => Some(a.clone()),
        (Kind::Export, None, Some(c)) => {
            Some(crate::manifest::Action { command: Some(c.clone()), path: m.actions.export.as_ref().and_then(|a| a.path.clone()), ..Default::default() })
        }
        _ => None,
    };
    let export = (spec.kind == Kind::Export).then(|| {
        let ext = spec.ext.clone().unwrap_or_default();
        (spec.format.clone().unwrap_or_default(), ext.clone(), spec.media.clone().unwrap_or_else(|| media_for(&ext).into()))
    });
    let def = NodeDef {
        id: spec.id.clone(),
        display: spec.display.clone(),
        category: [m.name.clone(), spec.family.clone()].into_iter().filter(|s| !s.is_empty()).collect(),
        description: if spec.description.is_empty() { spec.command.clone().unwrap_or_default() } else { spec.description.clone() },
        inputs,
        outputs,
        family: m.id.clone(),
        version: spec.version,
        memo: memo(spec.pure, spec.content),
        x: BTreeMap::from([(format!("{}.command", m.id), Value::from(spec.command.clone().unwrap_or_default()))]),
        // An export turns the document into its file (docs/research/20-convert.md): a step on
        // the way from a document to a value, never lossless (the document's edits stay behind).
        converts: converts(m, spec),
        ..Default::default()
    };
    let plan = Plan {
        kind: spec.kind,
        command: spec.command.clone(),
        select: spec.select.is_some(),
        widgets,
        files,
        fixed: spec.fixed.clone(),
        outputs: spec.outputs.clone(),
        export,
        export_action,
        timeout: spec.timeout_s.map(Duration::from_secs),
    };
    Ok(CraftNode { def, plan, shared: Arc::clone(shared) })
}

/// Keys no auto widget ever exposes: the adapter's plumbing (the document, waiting) and files,
/// which reach a craft only through file sockets (a raw path names nothing in the CAS).
const AUTO_HIDDEN: &[&str] = &["doc", "document", "wait", "path", "paths", "file", "files", "folder", "dir", "directory", "destination", "url"];

/// The command an export node runs: its own, or the manifest's export action's.
fn export_command(m: &Manifest, spec: &NodeSpec) -> Option<String> {
    match (&spec.action, &spec.command) {
        (Some(a), _) => a.command.clone(),
        (None, Some(c)) => Some(c.clone()),
        (None, None) => m.actions.export.as_ref().and_then(|a| a.command.clone()),
    }
}

/// Whether a curated node gets auto widgets ([`manifest::AutoWidgets`]): its `auto`, else on
/// when the manifest's are, except for open nodes, templated commands, and an export whose
/// command the export action or another export node also runs (its options are per format).
fn auto_on(m: &Manifest, spec: &NodeSpec) -> bool {
    if let Some(on) = spec.auto {
        return on;
    }
    if !m.auto_widgets.enabled || spec.kind == Kind::Open || spec.widgets.iter().any(|w| w.template) {
        return false;
    }
    if spec.kind == Kind::Export {
        let Some(cmd) = export_command(m, spec) else { return false };
        let shared_action = spec.action.is_none() && spec.command.is_none();
        let shared = shared_action
            || m.actions.export.as_ref().and_then(|a| a.command.as_ref()) == Some(&cmd)
            || m.nodes.iter().any(|n| n.id != spec.id && n.kind == Kind::Export && export_command(m, n).as_ref() == Some(&cmd));
        return !shared;
    }
    true
}

/// The keys a curated node sends of itself ([`coverage::node_keys`], the one place that works
/// them out), and `format` on every export: an export with a command of its own is never offered
/// one, since the node is one format.
fn sent_keys(m: &Manifest, spec: &NodeSpec) -> Vec<String> {
    let mut sent: Vec<String> = coverage::node_keys(m, spec).sent.into_iter().collect();
    if spec.kind == Kind::Export && !sent.iter().any(|k| k == "format") {
        sent.push("format".into());
    }
    sent
}

/// The rest of a curated node's command (spec: "generate a node's remaining parameters"): an
/// `advanced`, `optional` widget for each key the command accepts and the node doesn't send,
/// typed from the craft's description of it. Unset widgets aren't sent, so a workflow without
/// them runs and keys as before. Left out: keys the node or the manifest `hide`, plumbing
/// ([`AUTO_HIDDEN`], the document id, the selection's keys on a node with `select`), and a key
/// whose name another input already has. Arrays and objects are JSON widgets (a socket too), so
/// a list can be typed or wired in.
fn auto_widgets(m: &Manifest, spec: &NodeSpec, command: &str, cat: &Catalogue, inputs: &[Port]) -> Vec<(Port, WidgetPlan)> {
    let Some(keys) = accepted(cat, command) else { return Vec::new() };
    let sent = sent_keys(m, spec);
    let mut hidden: Vec<&str> = AUTO_HIDDEN.to_vec();
    hidden.extend(spec.hide.iter().map(String::as_str));
    hidden.extend(m.auto_widgets.hide.iter().map(String::as_str));
    hidden.extend(m.doc.id_param.as_deref());
    if spec.select.is_some() {
        hidden.extend(["ids", "id"]);
        hidden.extend(m.selection.as_ref().and_then(|s| s.set_key.as_deref()));
    }
    let mut out: Vec<(Port, WidgetPlan)> = Vec::new();
    for key in keys {
        let taken = inputs.iter().any(|p| p.name == key) || out.iter().any(|(p, _)| p.name == key);
        if taken || sent.contains(&key) || hidden.contains(&key.as_str()) || !grammar_key(&key) {
            continue;
        }
        let w = WidgetSpec { name: key.clone(), optional: true, advanced: true, ..Default::default() };
        let Ok(port) = widget_port(&w, Some(command), cat) else { continue };
        out.push((port, WidgetPlan { name: key.clone(), key, into: None, optional: true, template: false, allowed: None, ..Default::default() }));
    }
    out
}

/// The fields a choice widget's choices bring (`brings = "<catalogue>:<field>:<list>"`): for
/// each choice, its catalogue entry's parameters, shown only while it is chosen (`active_when`)
/// and sent only then, inside `brings_into` (default `params`). A parameter with `choices` is a
/// choice the craft takes by index; unit `Toggle` is a checkbox it takes as 0 or 1.
fn brings(w: &WidgetSpec, from: &str, cat: &Catalogue) -> Result<Vec<(Port, WidgetPlan)>, String> {
    let mut parts = from.splitn(3, ':');
    let (Some(list), Some(field), Some(params)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!("{}: brings is <catalogue>:<field>:<list>", w.name));
    };
    let entries = cat.lists.get(list).ok_or_else(|| format!("{}: no catalogue {list}", w.name))?;
    let into = w.brings_into.clone().unwrap_or_else(|| "params".into());
    let choices: Vec<&str> = w.choices.iter().flatten().filter_map(Value::as_str).collect();
    let mut out = Vec::new();
    for e in entries {
        let Some(id) = e.get(field).and_then(Value::as_str) else { continue };
        if !choices.is_empty() && !choices.contains(&id) {
            continue;
        }
        for q in e.get(params).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
            let Some(key) = q.get("id").and_then(Value::as_str) else { continue };
            let name = format!("{id}_{key}");
            if !grammar_key(&name) {
                continue;
            }
            let num = |k: &str| q.get(k).and_then(Value::as_f64);
            let unit = q.get("unit").and_then(Value::as_str).unwrap_or_default();
            let labels: Vec<String> =
                q.get("choices").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
            let shown = q.get("name").and_then(Value::as_str).unwrap_or(key);
            let suffix = match unit {
                "Db" => " (dB)",
                "Ms" => " (ms)",
                "Hz" => " (Hz)",
                "Percent" => " (%)",
                "Semitones" => " (semitones)",
                "Seconds" => " (s)",
                _ => "",
            };
            let mut port = Port {
                name: name.clone(),
                label: Some(format!("{shown}{suffix}")),
                tooltip: Some(format!("{shown}: a parameter of {}, sent while it is chosen", e.get("name").and_then(Value::as_str).unwrap_or(id))),
                active_when: Some(std::iter::once((w.name.clone(), vec![Value::from(id)])).collect()),
                ..Default::default()
            };
            let encode = if !labels.is_empty() {
                port.ty = "ENUM".into();
                port.default = num("default").and_then(|d| labels.get(d.max(0.0) as usize)).map_or(Value::Null, |l| Value::from(l.as_str()));
                port.choices = Some(labels.iter().map(|l| Value::from(l.as_str())).collect());
                nodes::Encode::Index(labels)
            } else if unit == "Toggle" {
                port.ty = "BOOL".into();
                port.default = Value::Bool(num("default").is_some_and(|d| d >= 0.5));
                nodes::Encode::BoolAsNumber
            } else {
                port.ty = "FLOAT".into();
                port.default = num("default").map_or(Value::Null, Value::from);
                port.min = num("min");
                port.max = num("max");
                nodes::Encode::AsIs
            };
            let plan = WidgetPlan {
                name,
                key: key.to_owned(),
                into: Some(into.clone()),
                optional: false,
                template: false,
                allowed: None,
                when: Some((w.name.clone(), Value::from(id))),
                encode,
            };
            out.push((port, plan));
        }
    }
    Ok(out)
}

/// A key fit to be a port name: letters, digits and `_`, starting with a letter or `_`.
fn grammar_key(k: &str) -> bool {
    let mut c = k.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_') && k.len() <= 64
}

/// What a curated node converts (docs/research/20-convert.md). An export turns the document into
/// its file: a step from a document to a value, never lossless (the document's edits stay
/// behind). An Open turns a picture, an SVG or a PDF into a document, one mark for each socket
/// it has.
fn converts(m: &Manifest, spec: &NodeSpec) -> Vec<plumcraft_proto::convert::Converts> {
    use plumcraft_proto::convert::Converts;
    match (spec.kind, spec.outputs.as_slice()) {
        (Kind::Export, [one]) if spec.files.iter().all(|f| f.optional) => {
            vec![Converts { output: Some(one.name.clone()), ..Converts::new(&m.doc.wire, &one.ty, false).input("doc") }]
        }
        (Kind::Open, _) => {
            let mut from = vec![("image", IMAGE)];
            if m.doc.opens_svg {
                from.push(("svg", SVG));
            }
            if m.doc.opens_pdf {
                from.push(("pdf", PDF));
            }
            from.into_iter().map(|(input, ty)| Converts { output: Some("doc".into()), ..Converts::new(ty, &m.doc.wire, false).input(input) }).collect()
        }
        _ => Vec::new(),
    }
}

fn media_for(ext: &str) -> &'static str {
    match ext {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// Export option widgets from `formats:<id>` (`document.formats`' typed options).
fn export_options(from: &str, names: &[String], cat: &Catalogue) -> Result<Vec<Port>, String> {
    let (list, id) = from.split_once(':').ok_or("options_from is <catalogue>:<format id>")?;
    let fmt = cat.lists.get(list).and_then(|l| l.iter().find(|f| f.get("id").and_then(Value::as_str) == Some(id))).ok_or_else(|| format!("no format {id}"))?;
    let opts = fmt.get("options").and_then(Value::as_object).ok_or_else(|| format!("format {id} has no options"))?;
    let mut out = Vec::new();
    for name in names {
        let o = opts.get(name).ok_or_else(|| format!("format {id} has no option {name}"))?;
        let default = o.get("default").cloned().unwrap_or(Value::Null);
        let ty = match o.get("type").and_then(Value::as_str) {
            Some("integer") => "INT",
            Some("number") => "FLOAT",
            Some("boolean") => "BOOL",
            Some("string") => "STRING",
            _ => JSON,
        };
        // Unset options are not sent: an explicit default isn't neutral (an explicit `artboard: 0`
        // exports that artboard, titled by it, instead of the document).
        let about = o.get("description").and_then(Value::as_str).unwrap_or_default();
        out.push(Port {
            name: name.clone(),
            ty: ty.into(),
            tooltip: Some(format!("{} (unset: {default})", about.chars().take(300).collect::<String>())),
            optional: true,
            advanced: true,
            ..Default::default()
        });
    }
    Ok(out)
}

/// A node generated from a catalogue entry (`{id, label, menu, params, defaults}`).
fn generated(m: &Manifest, g: &manifest::GenerateSpec, entry: &Value, id: &str, cat: &Catalogue, shared: &Arc<Shared>) -> Result<CraftNode, String> {
    if !cat.commands.contains_key(&g.command) {
        return Err(format!("{} has no command {}", m.name, g.command));
    }
    let eid = entry.get("id").and_then(Value::as_str).unwrap_or_default();
    let label = entry.get("label").and_then(Value::as_str).unwrap_or(eid).trim_end_matches('…').to_owned();
    let prose = entry.get("params").and_then(Value::as_str).unwrap_or_default();
    let texts: BTreeMap<String, String> =
        grammar::doc_keys(prose).map(|k| k.keys.iter().map(|(n, _, t)| ((*n).to_owned(), (*t).to_owned())).collect()).unwrap_or_default();
    let defaults = entry.get("defaults").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut inputs = vec![socket("doc", &m.doc.wire)];
    inputs.extend(select_ports(&g.select));
    let mut widgets = Vec::new();
    for (key, default) in &defaults {
        let text = texts.get(key).map(String::as_str).unwrap_or_default();
        let inf = grammar::infer(text);
        let (ty, optional) = match default {
            Value::Bool(_) => ("BOOL", false),
            Value::Number(n) if n.is_i64() || n.is_u64() => ("INT", false),
            Value::Number(_) => ("FLOAT", false),
            Value::String(s) if inf.ty == "ENUM" && inf.choices.iter().any(|c| c == s) => ("ENUM", false),
            Value::String(_) => ("STRING", false),
            _ => (JSON, true),
        };
        let numeric = ty == "INT" || ty == "FLOAT";
        inputs.push(Port {
            name: key.clone(),
            ty: ty.into(),
            tooltip: (!text.is_empty()).then(|| text.trim_start_matches(['?', ':', ' ']).chars().take(300).collect()),
            default: if optional { Value::Null } else { default.clone() },
            min: inf.min.filter(|_| numeric),
            max: inf.max.filter(|_| numeric),
            choices: (ty == "ENUM").then(|| inf.choices.iter().map(|c| Value::from(c.as_str())).collect()),
            optional,
            socket: (ty == JSON).then_some(Socket::Both),
            ..Default::default()
        });
        widgets.push(WidgetPlan {
            name: key.clone(),
            key: key.clone(),
            into: Some(g.params_key.clone()),
            optional,
            template: false,
            allowed: None,
            ..Default::default()
        });
    }
    let menu: Vec<String> =
        entry.get("menu").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
    let mut category = vec![m.name.clone(), g.family.clone()];
    category.extend(menu.into_iter().skip(1));
    let mut fixed = Map::new();
    fixed.insert(g.key.clone(), Value::from(eid));
    fixed.insert(g.params_key.clone(), json!({}));
    let def = NodeDef {
        id: id.to_owned(),
        display: label,
        category: category.into_iter().filter(|s| !s.is_empty()).collect(),
        description: format!("{} {{effect: {eid}}}: {}", g.command, prose.chars().take(400).collect::<String>()),
        inputs,
        outputs: vec![
            Port { name: "doc".into(), ty: m.doc.wire.clone(), ..Default::default() },
            Port { name: "result".into(), ty: JSON.into(), ..Default::default() },
        ],
        family: m.id.clone(),
        memo: memo(true, false),
        x: BTreeMap::from([(format!("{}.command", m.id), Value::from(g.command.as_str())), (format!("{}.effect", m.id), Value::from(eid))]),
        ..Default::default()
    };
    let plan = Plan {
        kind: Kind::Transform,
        command: Some(g.command.clone()),
        select: true,
        widgets,
        files: Vec::new(),
        fixed,
        outputs: Vec::new(),
        export: None,
        export_action: None,
        timeout: None,
    };
    Ok(CraftNode { def, plan, shared: Arc::clone(shared) })
}

/// The escape hatch: any allowlisted command, with JSON params and `select`.
fn hatch(m: &Manifest, r: &manifest::RunNodeSpec, cat: &Catalogue, shared: &Arc<Shared>) -> CraftNode {
    let matches = |id: &str, pats: &[String]| pats.iter().any(|p| id == p || (p.ends_with('.') && id.starts_with(p.as_str())));
    let allowed: Vec<String> = cat.commands.keys().filter(|id| matches(id, &r.allow) && !matches(id, &r.deny)).cloned().collect();
    let mut inputs = vec![socket("doc", &m.doc.wire)];
    inputs.extend(select_ports("all"));
    inputs.push(Port {
        name: "command".into(),
        ty: "ENUM".into(),
        default: allowed.first().map_or(Value::Null, |c| Value::from(c.as_str())),
        choices: Some(allowed.iter().map(|c| Value::from(c.as_str())).collect()),
        socket: Some(Socket::Widget),
        tooltip: Some(format!("A {} command (the manifest's allowlist: {} commands)", m.name, allowed.len())),
        ..Default::default()
    });
    inputs.push(Port {
        name: "params".into(),
        ty: JSON.into(),
        default: json!({}),
        socket: Some(Socket::Both),
        tooltip: Some("The command's parameters, as its description in the command list gives them".into()),
        ..Default::default()
    });
    let def = NodeDef {
        id: r.id.clone(),
        display: r.display.clone(),
        category: vec![m.name.clone(), "Advanced".into()],
        description: format!("Run any allowlisted {} command on the document; its memo scope is none", m.name),
        inputs,
        outputs: vec![
            Port { name: "doc".into(), ty: m.doc.wire.clone(), ..Default::default() },
            Port { name: "result".into(), ty: JSON.into(), ..Default::default() },
        ],
        family: m.id.clone(),
        memo: memo(false, false),
        ..Default::default()
    };
    let widgets = vec![
        WidgetPlan { name: "command".into(), key: "command".into(), into: None, optional: false, template: true, allowed: Some(allowed), ..Default::default() },
        WidgetPlan { name: "params".into(), key: "*".into(), into: None, optional: true, template: false, allowed: None, ..Default::default() },
    ];
    let plan = Plan {
        kind: Kind::Transform,
        command: Some("{command}".into()),
        select: true,
        widgets,
        files: Vec::new(),
        fixed: Map::new(),
        outputs: Vec::new(),
        export: None,
        export_action: None,
        timeout: None,
    };
    CraftNode { def, plan, shared: Arc::clone(shared) }
}

/// One node's smoke outcome: its id, and the milliseconds it took or its error.
pub type SmokeRow = (String, Result<f64, String>);

/// The smoke test (note 14, 2.4 step 5): run every node with its defaults on `fixture` (a raster
/// image), on the traced fixture for nodes that take a document. Returns each node's outcome:
/// `Ok(ms)` or the error. The pack's `exclude` list is what failed here.
pub fn smoke(manifest: &Manifest, opts: &Options, fixture: &Path, only: Option<&str>) -> Result<Vec<SmokeRow>, String> {
    use plumcraft_engine::exec::{self, Memo};
    use plumcraft_proto::event::StatusRecord;
    use plumcraft_proto::run::RunRequest;
    struct Sink;
    impl plumcraft_engine::Emit for Sink {
        fn status(&mut self, _: StatusRecord) {}
    }
    let opts = Options { include_excluded: true, ..opts.clone() };
    let (engine, report, _) = build(manifest, &opts)?;
    let plane = plumcraft_engine::data::Plane { store: engine.store(), shm: true, urls: false, max_inline: engine.limits().max_inline as usize };
    let mut memo = Memo::default();
    let schema = engine.schema();
    let open = manifest.nodes.iter().find(|n| n.kind == Kind::Open).map(|n| n.id.clone()).ok_or("the manifest has no open node")?;
    let base = manifest.smoke_base.clone();
    // The pack's own nodes: a building block without its inputs is a config error by design.
    let prefix = format!("{}.", open.split('.').next().unwrap_or_default());
    let mut out = Vec::new();
    for def in &schema.nodes {
        if !def.id.starts_with(&prefix) || only.is_some_and(|o| !def.id.contains(o)) {
            continue;
        }
        let takes_doc = def.inputs.iter().any(|p| p.name == "doc");
        let how = manifest.smoke.get(&def.id).cloned().unwrap_or_default();
        let mut nodes = Map::new();
        let mut wires = Vec::new();
        nodes.insert("src".into(), json!({"kind": open, "values": {"file": fixture.display().to_string()}}));
        let mut last = "src".to_owned();
        let mut chain: Vec<(String, Value)> = Vec::new();
        if takes_doc
            && !how.raster
            && let Some(b) = &base
        {
            chain.push((b.node.clone(), Value::Object(b.values.clone())));
        }
        chain.extend(how.before.iter().map(|b| (b.clone(), json!({}))));
        for (i, (kind, values)) in chain.into_iter().enumerate() {
            let id = format!("b{i}");
            nodes.insert(id.clone(), json!({"kind": kind, "values": values}));
            wires.push(json!({"from": {"node": last, "port": "doc"}, "to": {"node": id, "port": "doc"}}));
            last = id;
        }
        if def.id != open {
            nodes.insert("n".into(), json!({"kind": def.id, "values": how.values}));
            if takes_doc {
                wires.push(json!({"from": {"node": last, "port": "doc"}, "to": {"node": "n", "port": "doc"}}));
            }
        }
        let target = if def.id == open { "src" } else { "n" };
        let req: RunRequest = serde_json::from_value(json!({"v": 2, "op": "run", "run_id": format!("smoke-{}", def.id), "generation": 1,
            "graph": {"kind": "inline", "graph": {"version": 1, "nodes": nodes, "wires": wires}}, "targets": [target], "memo": "use"}))
        .map_err(|e| e.to_string())?;
        let t0 = Instant::now();
        let reply = exec::run(&engine, &req, &mut Sink, &plumcraft_engine::CancelToken::default(), "smoke", &mut memo, &plane);
        let r = reply.nodes.get(target);
        let outcome = match r.map(|n| (n.state, n.error.clone())) {
            Some((plumcraft_proto::event::NodeState::Done | plumcraft_proto::event::NodeState::Cached, _)) => Ok(t0.elapsed().as_secs_f64() * 1000.0),
            Some((_, Some(e))) => Err(format!("{}: {}", e.kind, e.message)),
            _ => Err(format!("no outcome: {:?}", reply.error)),
        };
        out.push((def.id.clone(), outcome));
    }
    drop(report);
    Ok(out)
}
