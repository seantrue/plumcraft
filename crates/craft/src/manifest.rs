//! A craft pack's manifest (note 14, section 3.10): how to start the craft's MCP server, which
//! of its tools and commands do what, the document type, and the nodes. Everything craft-specific
//! lives here; the client, pool, CAS, selection compiler, memo and status code are generic.
//!
//! Every `packs/<id>.toml` ships built in ([`builtin`], [`BUILTIN`]; `build.rs` makes the table),
//! with `packs/<id>-demo.json` as its demo workflow when present. `vectorcraft` has the 24 curated
//! nodes, the effect nodes generated from `effect.list`, and the `vc.run` escape hatch.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

/// The VectorCraft pack.
pub const VECTORCRAFT: &str = include_str!("../packs/vectorcraft.toml");
/// The SoundCraft genericity pack.
pub const SOUNDCRAFT: &str = include_str!("../packs/soundcraft.toml");
/// The VectorCraft demo workflow: open (choose a file) → Image Trace → Recolor → Export SVG,
/// and Export PNG from the trace.
pub const VECTORCRAFT_DEMO: &str = include_str!("../packs/vectorcraft-demo.json");

include!(concat!(env!("OUT_DIR"), "/packs.rs"));

/// The built-in packs' ids, sorted.
pub const BUILTIN: &[&str] = &{
    let mut ids = [""; PACKS.len()];
    let mut i = 0;
    while i < PACKS.len() {
        ids[i] = PACKS[i].0;
        i += 1;
    }
    ids
};

/// A built-in pack's demo workflow.
pub fn demo(name: &str) -> Option<&'static str> {
    PACKS.iter().find(|p| p.0 == name).and_then(|p| p.2)
}

/// The most bytes a manifest file may have.
const MAX_MANIFEST: u64 = 1 << 20;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The engine id (`vectorcraft`): the memo salt and the CAS folder name.
    pub id: String,
    pub name: String,
    /// The server: program and arguments (`~` is expanded). The roots are appended.
    pub command: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// The flags that confine the craft to the CAS (read) and the work folder (write). A craft
    /// whose server has no such flags omits `[roots]` and is not confined.
    #[serde(default)]
    pub roots: Option<Roots>,
    /// The fix shown when the craft can't be started.
    #[serde(default)]
    pub hint: String,
    #[serde(default)]
    pub pool: PoolSpec,
    /// The default per-call timeout, seconds (note 14, 3.7: 10 minutes).
    #[serde(default = "default_timeout")]
    pub timeout_s: u64,
    pub tools: Tools,
    pub actions: Actions,
    #[serde(default)]
    pub selection: Option<Selection>,
    pub doc: DocSpec,
    #[serde(default)]
    pub thumb: Option<ThumbSpec>,
    #[serde(default)]
    pub errors: Errors,
    #[serde(default, rename = "catalogue")]
    pub catalogues: Vec<CatalogueSpec>,
    #[serde(default, rename = "node")]
    pub nodes: Vec<NodeSpec>,
    #[serde(default, rename = "generate")]
    pub generated: Vec<GenerateSpec>,
    /// Nodes generated from the command list itself, one per matching command (PhotoCraft's
    /// filters, EffectCraft's layer commands).
    #[serde(default, rename = "generate_commands")]
    pub generated_commands: Vec<GenerateCommandsSpec>,
    #[serde(default)]
    pub run_node: Option<RunNodeSpec>,
    /// Nodes left out after the smoke test, with why (note 14, 2.4 step 5).
    #[serde(default)]
    pub exclude: BTreeMap<String, String>,
    /// Reply fields that name process state (the temp path a file went to, a document's index
    /// in its process): dropped from every `result`, so a result depends only on the inputs.
    #[serde(default = "default_volatile")]
    pub volatile: Vec<String>,
    /// How the smoke test runs a node, when its defaults on the traced fixture aren't a fair
    /// test (an outline needs a stroke first): by node id.
    #[serde(default)]
    pub smoke: BTreeMap<String, SmokeSpec>,
    /// The node that turns the fixture image into the document other nodes are smoke-tested on.
    #[serde(default)]
    pub smoke_base: Option<SmokeBase>,
    /// The craft's desktop app, which opens a copy of a document value (M3, "Open in …").
    #[serde(default)]
    pub gui: Option<GuiSpec>,
    /// The parameters of a curated node's command its pack doesn't list, exposed as advanced
    /// optional widgets ([`AutoWidgets`]).
    #[serde(default)]
    pub auto_widgets: AutoWidgets,
}

/// `[auto_widgets]`: every key a curated node's command accepts that the node doesn't send
/// (its widgets, files, fixed values, select and export plumbing) becomes an `advanced`,
/// `optional` widget, typed from the craft's own description of it. Unset, it isn't sent, so
/// the node does what it did without it. Not for open nodes, templated commands, or an export
/// whose command is shared by several formats (`file.export`); a node's `auto` overrides that.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoWidgets {
    /// Off: no node gets them unless its `auto = true`.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Keys never exposed on any node (beside the built-in plumbing: `doc`, `wait`, paths).
    #[serde(default)]
    pub hide: Vec<String>,
}

impl Default for AutoWidgets {
    fn default() -> Self {
        AutoWidgets { enabled: true, hide: Vec::new() }
    }
}

/// The craft's desktop app: `program [files…]` opens documents in a window. The editor reads
/// this from the manifest it trusts (never from an engine's messages) to offer "Open in …" on
/// document values (plan 10.6, M3).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuiSpec {
    /// The app's program and arguments (`~` expanded); the file is appended.
    pub command: Vec<String>,
    /// An environment variable that names the program instead (a different build, a test fake).
    #[serde(default)]
    pub env: Option<String>,
    /// The name in "Open in …" (default: the manifest's `name`).
    #[serde(default)]
    pub label: Option<String>,
    /// How to get the app when it is missing.
    #[serde(default)]
    pub hint: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmokeBase {
    pub node: String,
    #[serde(default)]
    pub values: serde_json::Map<String, Value>,
}

/// One node's smoke test: the nodes run before it (each with its defaults, after the trace),
/// the values it is given, or the raw image instead of the trace.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmokeSpec {
    #[serde(default)]
    pub before: Vec<String>,
    #[serde(default)]
    pub values: serde_json::Map<String, Value>,
    #[serde(default)]
    pub raster: bool,
}

fn default_volatile() -> Vec<String> {
    vec!["path".into(), "index".into()]
}

fn default_timeout() -> u64 {
    600
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roots {
    /// The flag naming a folder the craft may read (repeated per folder).
    pub read: String,
    pub write: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolSpec {
    #[serde(default = "default_pool_max")]
    pub max: usize,
    #[serde(default = "default_idle")]
    pub idle_s: u64,
    /// Reset the undo history every this many nodes on one document.
    #[serde(default = "default_clear_every")]
    pub clear_every: u32,
}

fn default_pool_max() -> usize {
    std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(1, 4))
}

fn default_idle() -> u64 {
    60
}

fn default_clear_every() -> u32 {
    20
}

impl Default for PoolSpec {
    fn default() -> Self {
        PoolSpec { max: default_pool_max(), idle_s: default_idle(), clear_every: default_clear_every() }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tools {
    /// Lists the commands: a bare array, or `{commands: […]}`, of `{id, label, menu, params, …}`.
    pub list: String,
    /// An MCP resource that lists the commands as JSON instead (`resources/read`, the first
    /// content's text), for a craft whose `list` tool answers in prose (FigCraft).
    #[serde(default)]
    pub list_resource: Option<String>,
    pub run: RunTool,
}

/// The tool that runs a command by id.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunTool {
    pub tool: String,
    /// The argument naming the command (`id` for VectorCraft, `command` for SoundCraft).
    pub id: String,
    pub params: String,
}

/// One document operation: a command (run through [`Tools::run`]) or a tool of its own.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub command: Option<String>,
    pub tool: Option<String>,
    /// The parameter that takes the file path.
    pub path: Option<String>,
    /// Parameters always sent.
    #[serde(default)]
    pub params: serde_json::Map<String, Value>,
    /// The reply field holding the id of the document this action opened, for a craft that
    /// names documents by id (`doc.id_param`; PdfCraft's `doc_open` → `doc`).
    #[serde(default)]
    pub returns: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actions {
    pub open: Action,
    pub save: Action,
    /// Closes every document (before a cold open, so a process holds one).
    pub close_all: Option<Action>,
    /// Clears the undo history ([`PoolSpec::clear_every`]).
    pub clear_history: Option<Action>,
    /// Exports (`format` and the format's options are added).
    pub export: Option<Action>,
}

/// The selection commands (note 14, 3.3).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub all: String,
    pub none: String,
    /// Takes `{ids: […]}` (or `{<set_key>: […]}`).
    pub set: String,
    /// The key `set` takes its ids under, when not `ids` (FilmCraft's `clips`, CADCraft's
    /// `handles`). With it, ids may be strings as well as whole numbers.
    #[serde(default)]
    pub set_key: Option<String>,
    /// Every id in `select = "ids:…"` is a string, even all digits (CADCraft's hex handles:
    /// `101` is 0x101, not 101).
    #[serde(default)]
    pub string_ids: bool,
    /// Takes `{fill: "#rrggbb"}`.
    pub color: Option<String>,
    /// Takes filters, returns `{matches: [{id}]}`.
    pub find: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocSpec {
    /// The doc wire type (`vectorcraft.doc`, D48).
    pub wire: String,
    pub ext: String,
    pub media: String,
    /// `Form::Doc{app}`.
    pub app: String,
    /// The file stem the craft saves to (some crafts name the document after its file).
    #[serde(default = "default_stem")]
    pub stem: String,
    #[serde(default)]
    pub color: Option<String>,
    /// Its open node also takes a wired SVG, as `svg`, beside `image`.
    #[serde(default)]
    pub opens_svg: bool,
    /// Its open node also takes a wired PDF, as `pdf`, beside `image` (PdfCraft).
    #[serde(default)]
    pub opens_pdf: bool,
    /// For a craft with no active document, that names each by the id its open action
    /// `returns`: the parameter the id goes in on every other call (tools at the top level,
    /// commands inside their params). PdfCraft: `doc`.
    #[serde(default)]
    pub id_param: Option<String>,
}

fn default_stem() -> String {
    "document".into()
}

/// The node thumbnail: a small PNG export, decoded to `rgba8` (note 14, section 4).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThumbSpec {
    pub px: u32,
    /// A query whose reply's `artboards[0]` has `width` and `height`.
    pub size_command: String,
    #[serde(default)]
    pub size_params: serde_json::Map<String, Value>,
    /// No thumb for documents larger than this many bytes (a huge export is slow).
    #[serde(default = "default_thumb_max")]
    pub max_doc_bytes: u64,
}

fn default_thumb_max() -> u64 {
    64 << 20
}

/// How tool errors map to node error kinds (note 14, 3.8).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Errors {
    /// A tool error containing one of these is `config`; any other is `op`.
    #[serde(default = "default_config_patterns")]
    pub config: Vec<String>,
    /// A tool error containing one of these means the catalogue is stale.
    #[serde(default = "default_unknown")]
    pub unknown_command: Vec<String>,
}

fn default_config_patterns() -> Vec<String> {
    ["invalid parameters", "unknown parameter", "is not available right now", "select an image", "unknown command", "nothing selected"]
        .map(str::to_owned)
        .to_vec()
}

fn default_unknown() -> Vec<String> {
    vec!["unknown command".into()]
}

impl Default for Errors {
    fn default() -> Self {
        Errors { config: default_config_patterns(), unknown_command: default_unknown() }
    }
}

/// A typed catalogue the craft serves (note 14, 1.4): run once at engine start.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogueSpec {
    pub name: String,
    pub command: String,
    /// The reply field holding the list.
    pub path: String,
    /// Open a new document first (`effect.list` describes the selection).
    #[serde(default)]
    pub needs_doc: bool,
    #[serde(default)]
    pub params: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A document from a file or an image: doc out.
    Open,
    /// A new document: doc out.
    New,
    /// Doc in, command, doc out (saved and hashed).
    Transform,
    /// Doc in, command, JSON out; the document is unchanged.
    Query,
    /// Doc in, an exported file out.
    Export,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSpec {
    pub id: String,
    pub display: String,
    pub kind: Kind,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub description: String,
    /// The command; `{widget}` is replaced by that widget's value (a `template` widget).
    pub command: Option<String>,
    /// The `select` parameter's default; absent: the node has no `select` port.
    pub select: Option<String>,
    /// Deterministic given its inputs (memo scope `machine`); otherwise `none` (D47).
    #[serde(default = "yes")]
    pub pure: bool,
    /// Ask for a content key (D43: byte-stable outputs only).
    #[serde(default)]
    pub content: bool,
    /// Parameters always sent.
    #[serde(default)]
    pub fixed: serde_json::Map<String, Value>,
    /// Export: the format id, the file extension, the media type and wire type of the output.
    pub format: Option<String>,
    pub ext: Option<String>,
    pub media: Option<String>,
    /// Export: generate widgets from this catalogue entry's `options` (`formats:svg`).
    pub options_from: Option<String>,
    /// Export: which of those options become widgets.
    #[serde(default)]
    pub options: Vec<String>,
    /// Export: its own action (a tool or a command) instead of `actions.export`, sent the path
    /// and the node's widgets and never `format` (a craft that rejects unknown keys, or a format
    /// with a command or tool of its own). A `command` on an export node is the same as
    /// `action = { command = … }` with the export action's path parameter.
    #[serde(default)]
    pub action: Option<Action>,
    #[serde(default, rename = "widget")]
    pub widgets: Vec<WidgetSpec>,
    /// Sockets for files: a value wired in (another PDF, a picture) is put in the craft's work
    /// folder and its path sent as `key`.
    #[serde(default, rename = "file")]
    pub files: Vec<FileSpec>,
    #[serde(default, rename = "output")]
    pub outputs: Vec<OutSpec>,
    pub timeout_s: Option<u64>,
    #[serde(default = "one")]
    pub version: u32,
    /// Keys of the command never exposed as auto widgets ([`AutoWidgets`]).
    #[serde(default)]
    pub hide: Vec<String>,
    /// Auto widgets for this node: `true` even where they are off by default (an export on a
    /// shared command, `[auto_widgets] enabled = false`), `false` never.
    #[serde(default)]
    pub auto: Option<bool>,
}

fn yes() -> bool {
    true
}

fn one() -> u32 {
    1
}

/// A socket whose value reaches the craft as a file (`[[node.file]]`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSpec {
    pub name: String,
    /// The wire type it takes (`PDF`, `IMAGE`).
    #[serde(rename = "type")]
    pub ty: String,
    /// The parameter the file's path goes in (default `path`).
    pub key: Option<String>,
    #[serde(default)]
    pub optional: bool,
    pub tooltip: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WidgetSpec {
    pub name: String,
    /// The craft's parameter key (default: `name`).
    pub key: Option<String>,
    #[serde(rename = "type")]
    pub ty: Option<String>,
    #[serde(default)]
    pub default: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub choices: Option<Vec<Value>>,
    /// `<catalogue>:<field>`: the choices are that field of each catalogue entry.
    pub choices_from: Option<String>,
    /// Not sent while unset (null); the craft's default applies.
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub advanced: bool,
    /// `widget`, `socket` or `both`.
    pub socket: Option<String>,
    /// Sent inside this parameter object (`params` for Image Trace overrides).
    pub into: Option<String>,
    pub label: Option<String>,
    pub tooltip: Option<String>,
    /// Used only in the command template, never sent.
    #[serde(default)]
    pub template: bool,
    /// `<catalogue>:<field>:<list>`: each choice brings the parameters its catalogue entry
    /// lists (the entry whose `<field>` is the choice; `<list>` of `{id, name, default, min,
    /// max, unit, choices}`), as fields shown only while it is chosen (SoundCraft's plug-ins).
    pub brings: Option<String>,
    /// The parameter object the brought values go in (default `params`).
    pub brings_into: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    /// A dotted path into the command's reply; absent: the whole reply.
    pub from: Option<String>,
}

/// Nodes generated from a typed catalogue (VectorCraft's 116 effects).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerateSpec {
    pub catalogue: String,
    pub prefix: String,
    pub command: String,
    /// The parameter naming the entry (`effect`).
    pub key: String,
    /// The parameter holding the entry's own parameters (`params`).
    pub params_key: String,
    #[serde(default)]
    pub family: String,
    #[serde(default = "default_all")]
    pub select: String,
}

fn default_all() -> String {
    "all".into()
}

/// A family of nodes generated from the command list (`[[generate_commands]]`): one transform
/// node per command that `commands` matches and neither `deny` nor the escape hatch's `deny`
/// does, with its widgets read from the command's own params description. The node id is
/// `id_prefix` + the command id (`pc.filter.blur.boxBlur`), so it stays put as long as the
/// command does. A command a curated node runs is left to that node.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerateCommandsSpec {
    /// Command id prefixes (ending in `.`) or exact ids.
    pub commands: Vec<String>,
    /// Command id prefixes or ids left out, even when `commands` matches.
    #[serde(default)]
    pub deny: Vec<String>,
    /// The node id prefix (`pc.`).
    pub id_prefix: String,
    #[serde(default)]
    pub family: String,
    /// The `select` parameter's default; absent: no `select` port.
    #[serde(default)]
    pub select: Option<String>,
    /// Values for parameters by key, across the family: the widget's default, always sent
    /// (EffectCraft's `layers = ["#1"]`: a reopened project has no selected layers).
    #[serde(default)]
    pub defaults: serde_json::Map<String, Value>,
    /// Parameters never made widgets: file paths (outside the CAS) and process state (a
    /// document index). The craft's default applies.
    #[serde(default)]
    pub drop_keys: Vec<String>,
    /// Deterministic given its inputs (memo scope `machine`).
    #[serde(default = "yes")]
    pub pure: bool,
}

/// The escape hatch (note 14, 2.3, `vc.run`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunNodeSpec {
    pub id: String,
    pub display: String,
    /// Command id prefixes allowed (`object.`), or exact ids.
    pub allow: Vec<String>,
    /// Command id prefixes or ids never allowed, even when `allow` matches.
    #[serde(default)]
    pub deny: Vec<String>,
}

impl Manifest {
    /// Parse a manifest.
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let m: Manifest = toml::from_str(text).map_err(|e| format!("manifest: {e}"))?;
        m.check()?;
        Ok(m)
    }

    /// Read a manifest file (at most 1 MiB).
    pub fn load(path: &Path) -> Result<Manifest, String> {
        let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if meta.len() > MAX_MANIFEST {
            return Err(format!("{}: over {MAX_MANIFEST} bytes", path.display()));
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Manifest::parse(&text)
    }

    /// A built-in pack by name, or a manifest file by path.
    pub fn named(name_or_path: &str) -> Result<Manifest, String> {
        match builtin(name_or_path) {
            Some(m) => m,
            None => Manifest::load(Path::new(name_or_path)),
        }
    }

    fn check(&self) -> Result<(), String> {
        let ident = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if !ident(&self.id) {
            return Err(format!("manifest: id {:?} must be letters, digits, - and _", self.id));
        }
        if self.command.is_empty() {
            return Err("manifest: empty command".into());
        }
        if self.gui.as_ref().is_some_and(|g| g.command.is_empty()) {
            return Err("manifest: empty gui.command".into());
        }
        if !ident(&self.doc.ext) || !ident(&self.doc.stem) {
            return Err("manifest: doc.ext and doc.stem must be plain names".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for n in &self.nodes {
            if !seen.insert(n.id.as_str()) {
                return Err(format!("manifest: node {} twice", n.id));
            }
            if n.kind != Kind::Export && n.kind != Kind::Open && n.command.is_none() {
                return Err(format!("manifest: node {} has no command", n.id));
            }
            let own_action = n.action.is_some() || n.command.is_some();
            if n.kind == Kind::Export && ((n.format.is_none() && !own_action) || n.ext.as_deref().is_none_or(|e| !ident(e))) {
                return Err(format!("manifest: export node {} needs a plain ext, and a format or its own command or action", n.id));
            }
            if n.action.as_ref().is_some_and(|a| a.tool.is_none() && a.command.is_none()) {
                return Err(format!("manifest: node {}'s action has neither a tool nor a command", n.id));
            }
            if n.select.is_some() && self.selection.is_none() {
                return Err(format!("manifest: node {} has select, and the manifest no [selection]", n.id));
            }
        }
        for g in &self.generated_commands {
            if !g.id_prefix.ends_with('.') || !ident(g.id_prefix.trim_end_matches('.')) {
                return Err(format!("manifest: generate_commands id_prefix {:?} must be a plain name and a dot", g.id_prefix));
            }
            if g.commands.is_empty() {
                return Err(format!("manifest: generate_commands {} matches no commands", g.id_prefix));
            }
            if g.select.is_some() && self.selection.is_none() {
                return Err(format!("manifest: generate_commands {} has select, and the manifest no [selection]", g.id_prefix));
            }
        }
        Ok(())
    }

    /// The command with `~` expanded in its program.
    pub fn program(&self) -> Vec<String> {
        self.command.iter().enumerate().map(|(i, a)| if i == 0 { expand_home(a) } else { a.clone() }).collect()
    }

    /// The executable's path: the program as given, or found on `PATH`.
    pub fn executable(&self) -> Option<PathBuf> {
        let prog = PathBuf::from(self.program().first()?);
        if prog.components().count() > 1 {
            return prog.is_file().then_some(prog);
        }
        // PATH, the login shell's and the usual folders: an app from the Finder has none of them
        plumcraft_proto::programs::find_program(&prog)
    }
}

impl GuiSpec {
    /// The app's program and arguments: the `env` override when it is set, else `command`
    /// with `~` expanded.
    pub fn program(&self) -> Vec<String> {
        if let Some(p) = self.env.as_deref().and_then(std::env::var_os).filter(|p| !p.is_empty()) {
            return vec![p.to_string_lossy().into_owned()];
        }
        self.command.iter().enumerate().map(|(i, a)| if i == 0 { expand_home(a) } else { a.clone() }).collect()
    }

    /// The program's path when it is installed: as given, or found on `PATH`; otherwise why
    /// not, with the fix.
    pub fn executable(&self) -> Result<PathBuf, String> {
        let program = self.program();
        let first = program.first().cloned().unwrap_or_default();
        let prog = PathBuf::from(&first);
        let found = if prog.components().count() > 1 { prog.is_file().then_some(prog) } else { plumcraft_proto::programs::find_program(&prog) };
        let via = self.env.as_deref().map(|e| format!(" (or set {e})")).unwrap_or_default();
        found.ok_or_else(|| {
            let hint = if self.hint.is_empty() { String::new() } else { format!("; {}", self.hint) };
            format!("no app at {first}{via}{hint}")
        })
    }
}

/// A built-in pack.
pub fn builtin(name: &str) -> Option<Result<Manifest, String>> {
    PACKS.iter().find(|p| p.0 == name).map(|p| Manifest::parse(p.1))
}

/// `~/x` → `$HOME/x`.
pub fn expand_home(s: &str) -> String {
    match (s.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => s.to_owned(),
    }
}
