//! The catalogue (spec section 7): wire types, node definitions, families, expansions and
//! templates. Protocol 1's `SchemaReply` with the generic fields promoted and pack-shaped fields
//! moved under `x`.
//!
//! The UI hard-codes no node id, port name or wire type beyond the scalars `FLOAT INT BOOL
//! STRING ENUM` (spec 7.4): what core used to hard-code comes from `flags.result`,
//! `flags.bundle_port`, `editor.port` and [`SchemaReply::texts`].
//!
//! Structure is checked (a port with an unknown `socket` fails the reply, as in protocol 1);
//! unknown fields are ignored and `null` reads as the default.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::de::{lenient, lenient_vec, null_as_default};
use crate::session::EngineInfo;

/// Scalar wire types: they carry a widget value and default to `socket: both`.
pub mod scalars {
    pub const FLOAT: &str = "FLOAT";
    pub const INT: &str = "INT";
    pub const BOOL: &str = "BOOL";
    pub const STRING: &str = "STRING";
    pub const ENUM: &str = "ENUM";
    pub const ALL: &[&str] = &[FLOAT, INT, BOOL, STRING, ENUM];

    pub fn is_scalar(ty: &str) -> bool {
        ALL.contains(&ty)
    }
}

/// Core wire types beyond the scalars (maintainer, 2026-10-29: "Make arrays first class"): one
/// meaning in every engine, so a value crosses from one engine to another unchanged.
pub mod core_types {
    use super::{Media, Port, WireType};

    /// A numpy-style array: `Form::Array` carries its dtype, shape, colour space and channel
    /// names; it crosses as an `md5:nd1:` blob (spec 4.4).
    pub const ARRAY: &str = "ARRAY";
    /// The views an array viewer offers (spec 7.2), in this order.
    pub const ARRAY_VIEWS: &[&str] = &["auto", "color", "stretch", "clip", "heat", "labels", "angle"];
    /// An SVG document (maintainer, 2026-10-30: "Make SVG first class"): UTF-8, an `md5:raw:`
    /// blob of media [`SVG_MEDIA`]. The editor draws it itself (`plumcraft-svg`), so an engine
    /// needn't send a picture of it; the Rust SDK makes its node thumbnail.
    pub const SVG: &str = "SVG";
    /// An SVG value's media type.
    pub const SVG_MEDIA: &str = "image/svg+xml";
    /// Text (maintainer, 2026-10-30, decision G3: "yes core data types"): UTF-8, a JSON string
    /// inline, or an `md5:raw:` blob of media [`TEXT_MEDIA`] with `Form::Text` when long.
    pub const TEXT: &str = "TEXT";
    pub const TEXT_MEDIA: &str = "text/plain; charset=utf-8";
    /// A table (G3): columnar JSON, `{"columns": [{"name", "type", "values": [...]}]}`, every
    /// column as long as the first; inline, or an `md5:raw:` blob of media [`TABLE_MEDIA`] with
    /// `Form::Table` when large.
    pub const TABLE: &str = "TABLE";
    pub const TABLE_MEDIA: &str = "application/json";
    /// An encoded picture (G3): an `md5:raw:` blob whose `media` names its format
    /// (`image/png`, `image/jpeg`, `image/tiff`, …) with `Form::Image`. Every engine that reads
    /// IMAGE reads PNG, and makes PNG unless asked for another format.
    pub const IMAGE: &str = "IMAGE";
    /// Audio (maintainer, 2026-10-30: "Core audio, please"): PCM as an `md5:nd1:` blob of `<f4`
    /// samples shaped (frames, channels), interleaved, with `Form::Audio` (its sample rate and
    /// channel count).
    pub const AUDIO: &str = "AUDIO";
    /// Any JSON value (maintainer, 2026-10-30: "JSON yes, (format as Yaml for display)"): inline,
    /// or an `md5:raw:` blob of compact JSON of media [`TABLE_MEDIA`] with `Form::Json` when large.
    pub const JSON: &str = "JSON";
    /// Every core type that isn't a scalar.
    pub const ALL: &[&str] = &[ARRAY, SVG, TEXT, TABLE, IMAGE, AUDIO, JSON];

    /// The TEXT wire type every engine with text declares.
    pub fn text() -> WireType {
        WireType { id: TEXT.into(), color: Some("#c9a227".into()), media: Media::Text, thumb: true, ..Default::default() }
    }

    /// The TABLE wire type every engine with tables declares.
    pub fn table() -> WireType {
        WireType { id: TABLE.into(), color: Some("#4f86c6".into()), media: Media::Table, ..Default::default() }
    }

    /// The IMAGE wire type every engine with encoded pictures declares.
    pub fn image() -> WireType {
        WireType { id: IMAGE.into(), color: Some("#2563eb".into()), media: Media::Image, thumb: true, ..Default::default() }
    }

    /// The AUDIO wire type every engine with audio declares.
    pub fn audio() -> WireType {
        WireType { id: AUDIO.into(), color: Some("#2fa39a".into()), media: Media::Audio, thumb: true, ..Default::default() }
    }

    /// The JSON wire type every engine with JSON values declares.
    pub fn json() -> WireType {
        WireType { id: JSON.into(), color: Some("#64748b".into()), media: Media::Tree, ..Default::default() }
    }

    /// The core wire type `id` declares, if it is one.
    pub fn wire_type(id: &str) -> Option<WireType> {
        match id {
            ARRAY => Some(array()),
            SVG => Some(svg()),
            TEXT => Some(text()),
            TABLE => Some(table()),
            IMAGE => Some(image()),
            AUDIO => Some(audio()),
            JSON => Some(json()),
            _ => None,
        }
    }

    /// The ARRAY wire type every engine declares (`Engine::with_arrays`, the Python SDK's `Array`).
    pub fn array() -> WireType {
        let turn = Port {
            name: "turn".into(),
            ty: super::scalars::FLOAT.into(),
            default: serde_json::json!(1.0),
            tooltip: Some("One full turn in the array's units (1 for turns, 360 for degrees, 6.283 for radians)".into()),
            ..Default::default()
        };
        WireType {
            id: ARRAY.into(),
            color: Some("#8a5cc2".into()),
            media: Media::Ndarray,
            views: ARRAY_VIEWS.iter().map(|v| (*v).to_owned()).collect(),
            view_params: [("angle".to_owned(), vec![turn])].into_iter().collect(),
            default_view: Some("auto".into()),
            thumb: true,
            ..Default::default()
        }
    }

    /// The SVG wire type every engine with SVG values declares (`Engine::with_svg`, the Python
    /// SDK's `Svg`).
    pub fn svg() -> WireType {
        WireType { id: SVG.into(), color: Some("#16a34a".into()), media: Media::Vector, thumb: true, ..Default::default() }
    }

    pub fn is_core(ty: &str) -> bool {
        ALL.contains(&ty)
    }
}

/// Media kinds (spec 10.1): how a value is shown. A closed set; unknown reads as `bytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Media {
    Image,
    Ndarray,
    Table,
    Scalar,
    Text,
    Audio,
    Vector,
    Mesh,
    Tree,
    Composite,
    Sequence,
    Doc,
    /// Opaque: the hex viewer plus any rgba8 rendition. Also what an unknown kind reads as.
    #[default]
    #[serde(other)]
    Bytes,
}

/// A wire type: its colour and how its values are shown.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct WireType {
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    /// `#rrggbb`, when the engine suggests one.
    pub color: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub media: Media,
    /// The views the viewer offers, exactly (replaces a hard-coded list).
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<String>,
    /// Parameters per view: `{"angle": [Port…]}`.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "BTreeMap::is_empty")]
    pub view_params: BTreeMap<String, Vec<Port>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_view: Option<String>,
    /// Whether node thumbnails are drawn for it.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "std::ops::Not::not")]
    pub thumb: bool,
    /// For `composite`: field → wire type.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    /// The parameter-bundle type (today `SETTINGS`).
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "std::ops::Not::not")]
    pub bundle: bool,
}

/// An extra conversion a port accepts.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Compat {
    #[serde(deserialize_with = "null_as_default")]
    pub from: String,
    #[serde(deserialize_with = "null_as_default")]
    pub to: String,
}

impl Compat {
    /// The base rules when an engine sends none (`crates/graph/src/ports.rs` today).
    pub fn defaults() -> Vec<Compat> {
        [("INT", "FLOAT"), ("STRING", "ENUM"), ("ENUM", "STRING")].into_iter().map(|(f, t)| Compat { from: f.into(), to: t.into() }).collect()
    }
}

/// A node family: nodes are coloured by family (theme tokens, by id).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Family {
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub display: String,
    #[serde(deserialize_with = "null_as_default")]
    pub description: String,
}

/// How a port appears on a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Socket {
    /// A plain widget; cannot be wired.
    Widget,
    /// Wire only; no widget.
    Socket,
    /// A widget that becomes a socket when wired.
    Both,
}

/// The bundle parameter a port is tied to (protocol 1's impressionist `setting{section, name}`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct ParamRef {
    #[serde(deserialize_with = "null_as_default")]
    pub group: String,
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
}

/// A growable input (`c1`, `c2`, …).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Variadic {
    #[serde(deserialize_with = "null_as_default")]
    pub template: String,
    #[serde(deserialize_with = "null_as_default")]
    pub names: Vec<String>,
    #[serde(deserialize_with = "null_as_default")]
    pub min: u32,
    #[serde(deserialize_with = "null_as_default")]
    pub max: u32,
}

/// What a path input names, for its file dialog: `open`, `save` or `folder`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct PathHint {
    #[serde(deserialize_with = "null_as_default")]
    pub mode: String,
    #[serde(deserialize_with = "null_as_default")]
    pub filters: Vec<PathFilter>,
}

impl PathHint {
    /// A file the node reads (`open`, or unspecified): keyed by its content and, for a remote
    /// engine, uploaded. A `save` path names a file the node writes, a `dir` path a folder, and
    /// a `files` value several files, one path per line (chosen together in one dialog): none
    /// of them is read, keyed by content, or uploaded as one file.
    pub fn is_source(&self) -> bool {
        !matches!(self.mode.as_str(), "save" | "dir" | "files")
    }

    /// Several files, one path per line.
    pub fn is_many(&self) -> bool {
        self.mode == "files"
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct PathFilter {
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub extensions: Vec<String>,
}

/// One input or output of a node. Every protocol 1 field keeps its meaning.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Port {
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(rename = "type", deserialize_with = "null_as_default")]
    pub ty: String,
    pub label: Option<String>,
    pub tooltip: Option<String>,
    pub default: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub decimals: Option<u32>,
    pub choices: Option<Vec<Value>>,
    #[serde(deserialize_with = "null_as_default")]
    pub optional: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub advanced: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub list: bool,
    /// `None`: the type's default ([`Socket::Both`] for scalars, [`Socket::Socket`] otherwise).
    pub socket: Option<Socket>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param_ref: Option<ParamRef>,
    pub variadic: Option<Variadic>,
    /// Sub-ports a choice brings: `{choice: [Port…]}`.
    pub options: Option<BTreeMap<String, Vec<Port>>>,
    pub path: Option<PathHint>,
    /// A group shows this port on its surface, wired or not.
    #[serde(deserialize_with = "null_as_default")]
    pub external: bool,
    /// Choices that can't run on this machine → why.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_choices: Option<BTreeMap<String, String>>,
    /// Read only under these parameters' values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_when: Option<BTreeMap<String, Vec<Value>>>,
    /// A choice that fills other ports: `{choice: {port: value}}`. The filled ports stay editable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fills: Option<BTreeMap<String, serde_json::Map<String, Value>>>,
    /// With `fills`: the ports each choice uses, which the node shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shows: Option<BTreeMap<String, Vec<String>>>,
    /// How to show the value when its type alone doesn't say. Unknown hints are ignored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub widget: Option<String>,
    /// The span a slider covers when narrower than `min..max`, or when a bound is open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soft_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soft_max: Option<f64>,
}

impl Port {
    pub fn socket_mode(&self) -> Socket {
        self.socket.unwrap_or(if scalars::is_scalar(&self.ty) { Socket::Both } else { Socket::Socket })
    }

    pub fn wireable(&self) -> bool {
        self.socket_mode() != Socket::Widget
    }

    pub fn has_widget(&self) -> bool {
        self.socket_mode() != Socket::Socket
    }

    /// The span a slider for this number covers: the soft span where given, else `min..max`;
    /// `None` when an end is open on both counts.
    pub fn slider_span(&self) -> Option<(f64, f64)> {
        let lo = self.soft_min.or(self.min)?;
        let hi = self.soft_max.or(self.max)?;
        (lo < hi).then_some((lo, hi))
    }

    pub fn display_label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.name)
    }
}

/// Behaviour flags of a node.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct NodeFlags {
    #[serde(deserialize_with = "null_as_default")]
    pub output_node: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub input_list: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub per_channel: bool,
    /// `true`, `false` or `"unit"`: whether results keep colour-space tags.
    pub keep: Value,
    /// Its `values` text is seeded with the choices of the ENUM input it is wired into.
    #[serde(deserialize_with = "null_as_default")]
    pub vary_choices: bool,
    /// The input whose recipes this node gets as its `recipe` value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe: Option<String>,
    /// The input that takes the parameter bundle (protocol 1: `settings`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_port: Option<String>,
    /// A result node: its output is the graph's result (protocol 1: `vi.op.result`).
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "std::ops::Not::not")]
    pub result: bool,
}

/// A custom editor: `curves`, `mask` or a pack kind, on a port.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Editor {
    #[serde(deserialize_with = "null_as_default")]
    pub kind: String,
    #[serde(deserialize_with = "null_as_default")]
    pub port: String,
    /// For `curves`: the curve maths (VI: `monotone_cubic`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpolant: Option<String>,
}

/// Whether a node can be marked for export (a Cartooner checkpoint), and what core calls the
/// mark. The pack's export data is opaque.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Export {
    #[serde(deserialize_with = "null_as_default")]
    pub markable: bool,
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "String::is_empty")]
    pub mark: String,
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "BTreeMap::is_empty")]
    pub x: BTreeMap<String, Value>,
}

/// Where a node's results may be reused (spec 14.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MemoScope {
    None,
    Session,
    Global,
    /// The default; also what an unknown scope reads as.
    #[default]
    #[serde(other)]
    Machine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct MemoSpec {
    #[serde(deserialize_with = "lenient")]
    pub scope: MemoScope,
    #[serde(deserialize_with = "null_as_default")]
    pub device_sensitive: bool,
    /// Ask for a content key as well as a plan key.
    #[serde(deserialize_with = "null_as_default")]
    pub content: bool,
    /// Eligible for persistence across restarts.
    #[serde(deserialize_with = "null_as_default")]
    pub persist: bool,
}

/// One node type. Rust never hard-codes one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct NodeDef {
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub display: String,
    #[serde(deserialize_with = "null_as_default")]
    pub category: Vec<String>,
    #[serde(deserialize_with = "null_as_default")]
    pub description: String,
    /// `cpu`, `gpu`, `mlx`, …
    #[serde(deserialize_with = "null_as_default")]
    pub resource: String,
    #[serde(deserialize_with = "null_as_default")]
    pub inputs: Vec<Port>,
    #[serde(deserialize_with = "null_as_default")]
    pub outputs: Vec<Port>,
    #[serde(deserialize_with = "null_as_default")]
    pub flags: NodeFlags,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<Editor>,
    #[serde(deserialize_with = "null_as_default")]
    pub export: Export,
    #[serde(deserialize_with = "null_as_default")]
    pub family: String,
    /// A Lucide icon name.
    #[serde(deserialize_with = "null_as_default")]
    pub icon: String,
    /// Part of the memo key; absent → 1.
    pub version: u32,
    #[serde(deserialize_with = "null_as_default")]
    pub memo: MemoSpec,
    /// Pack fields (`x.vi.code`, …), passed through.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "BTreeMap::is_empty")]
    pub x: BTreeMap<String, Value>,
    /// In a catalogue merged from several (a coordinator's, spec 7.3): the engines that offer
    /// this kind, the one that runs it unless the workflow says otherwise first. Absent from a
    /// single engine's catalogue.
    #[serde(default, deserialize_with = "lenient_vec", skip_serializing_if = "Vec::is_empty")]
    pub engines: Vec<String>,
    /// This node turns one type into another (`docs/research/20-convert.md`): a mark the
    /// editor finds conversions by. A mark that doesn't read is no mark.
    #[serde(default, deserialize_with = "crate::de::lenient_opt", skip_serializing_if = "Option::is_none")]
    pub converts: Option<crate::convert::Converts>,
}

impl Default for NodeDef {
    fn default() -> Self {
        NodeDef {
            id: String::new(),
            display: String::new(),
            category: Vec::new(),
            description: String::new(),
            resource: String::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            flags: NodeFlags::default(),
            editor: None,
            export: Export::default(),
            family: String::new(),
            icon: String::new(),
            version: 1,
            memo: MemoSpec::default(),
            x: BTreeMap::new(),
            engines: Vec::new(),
            converts: None,
        }
    }
}

/// The values of [`Port::widget`] an editor knows; it ignores others.
pub mod widgets {
    /// A STRING of `r,g,b`, each 0..1: a colour picker.
    pub const COLOR: &str = "color";
    /// A STRING of `#rrggbb`: a colour picker.
    pub const HEX_COLOR: &str = "hexcolor";
    /// A STRING of comma-separated choices, from `choices` or, without them, from the choices of
    /// the input its node feeds: several can be picked.
    pub const MULTICHOICE: &str = "multichoice";
    /// A STRING of several lines (a query, a script, a paragraph): a text box, monospace, that a
    /// click elsewhere commits, or ⌘↩ (Run), which runs with it; Enter is a new line.
    pub const MULTILINE: &str = "multiline";
}

impl NodeDef {
    /// What a filling choice ([`Port::fills`]) gives input `name`, when `current` says which is
    /// chosen; `None` if no choice names it.
    pub fn filled_default<'a>(&'a self, name: &str, current: impl Fn(&str) -> Option<&'a Value>) -> Option<&'a Value> {
        self.inputs.iter().find_map(|p| {
            let table = p.fills.as_ref()?;
            let choice = current(&p.name).unwrap_or(&p.default);
            let key = choice.as_str().map_or_else(|| choice.to_string(), str::to_owned);
            table.get(&key)?.get(name)
        })
    }

    pub fn input(&self, name: &str) -> Option<&Port> {
        self.inputs.iter().find(|p| p.name == name)
    }

    pub fn output(&self, name: &str) -> Option<&Port> {
        self.outputs.iter().find(|p| p.name == name)
    }
}

/// A save format for `output.save`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct SaveFormat {
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub label: String,
    #[serde(deserialize_with = "null_as_default")]
    pub extensions: Vec<String>,
    #[serde(deserialize_with = "null_as_default")]
    pub options: Vec<Port>,
    /// The kinds of value it writes. Empty: any.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<Media>,
}

/// Engine-supplied UI strings core used to hard-code (spec 7.1).
pub mod texts {
    /// The meaning of a blank value ("use impressionist's fallback").
    pub const BLANK_VALUE: &str = "blank_value";
    /// Before a result's name ("Result:").
    pub const RESULT_PREFIX: &str = "result_prefix";
    /// What an export mark is called ("checkpoint"), and its hover text.
    pub const EXPORT_MARK: &str = "export_mark";
    pub const EXPORT_MARK_HOVER: &str = "export_mark_hover";
    /// Something the person should know while this engine is connected ("no GPU device: …"). The
    /// editor shows it in its banner.
    pub const ENGINE_NOTICE: &str = "engine_notice";
}

/// The reply to `schema`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct SchemaReply {
    pub v: u32,
    #[serde(deserialize_with = "lenient")]
    pub engine: EngineInfo,
    #[serde(deserialize_with = "null_as_default")]
    pub wire_types: Vec<WireType>,
    /// Extra conversions; `None` means [`Compat::defaults`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compat: Option<Vec<Compat>>,
    #[serde(deserialize_with = "null_as_default")]
    pub families: Vec<Family>,
    #[serde(deserialize_with = "null_as_default")]
    pub nodes: Vec<NodeDef>,
    /// Command node id → the graph it unrolls into. Opaque.
    #[serde(deserialize_with = "null_as_default")]
    pub expansions: BTreeMap<String, Value>,
    /// Name → a graph the UI opens (protocol 1 `style_graphs`).
    #[serde(deserialize_with = "null_as_default")]
    pub templates: BTreeMap<String, Value>,
    #[serde(deserialize_with = "lenient_vec", skip_serializing_if = "Vec::is_empty")]
    pub save_formats: Vec<SaveFormat>,
    #[serde(deserialize_with = "crate::de::lenient_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub texts: BTreeMap<String, String>,
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "BTreeMap::is_empty")]
    pub x: BTreeMap<String, Value>,
}

impl SchemaReply {
    /// The node type `id`. Core's own Convert node ([`crate::convert::KIND`]) is in every
    /// catalogue, listed or not.
    pub fn node(&self, id: &str) -> Option<&NodeDef> {
        self.nodes.iter().find(|n| n.id == id).or_else(|| (id == crate::convert::KIND).then(crate::convert::def))
    }

    pub fn wire_type(&self, id: &str) -> Option<&WireType> {
        self.wire_types.iter().find(|w| w.id == id)
    }

    /// The conversions in force: the engine's, or the base defaults.
    pub fn compat_rules(&self) -> Vec<Compat> {
        self.compat.clone().unwrap_or_else(Compat::defaults)
    }

    /// Does an input of type `to` accept a wire of type `from`?
    pub fn accepts(&self, from: &str, to: &str) -> bool {
        from == to || self.compat_rules().iter().any(|c| c.from == from && c.to == to)
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        self.texts.get(key).map(String::as_str)
    }

    /// The parameter-bundle wire type, if the engine has one.
    pub fn bundle_type(&self) -> Option<&str> {
        self.wire_types.iter().find(|w| w.bundle).map(|w| w.id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_read_paths_are_sources() {
        let hint = |mode: &str| PathHint { mode: mode.into(), ..Default::default() };
        assert!(hint("").is_source() && hint("open").is_source());
        assert!(!hint("save").is_source() && !hint("dir").is_source());
    }

    const FIXTURE: &str = r##"{
      "v": 2,
      "engine": {"id": "text", "name": "Text", "version": "0.1.0", "salt": "rs:text+1", "schema_digest": "d1"},
      "wire_types": [
        {"id": "ARRAY", "color": "#5b8def", "media": "ndarray", "views": ["auto", "heat", "angle"],
         "view_params": {"angle": [{"name": "turn", "type": "FLOAT", "default": 1.0}]}, "default_view": "auto", "thumb": true},
        {"id": "TEXT", "media": "text"},
        {"id": "SETTINGS", "media": "tree", "bundle": true},
        {"id": "HOLOGRAM", "media": "hologram"}
      ],
      "nodes": [
        {"id": "text.split", "display": "Split", "category": ["Text"], "version": 3,
         "inputs": [{"name": "text", "type": "TEXT"},
                    {"name": "sep", "type": "STRING", "default": "\n", "param_ref": {"group": "split", "name": "sep"}}],
         "outputs": [{"name": "parts", "type": "TEXT", "list": true}],
         "flags": {"bundle_port": "settings", "result": false},
         "editor": {"kind": "curves", "port": "curve", "interpolant": "monotone_cubic"},
         "export": {"markable": true, "mark": "checkpoint", "x": {"vi": {"expr": "x"}}},
         "memo": {"scope": "session", "content": true},
         "x": {"vi": {"code": "…"}},
         "unknown": 1}
      ],
      "texts": {"result_prefix": "Result:", "bad": 7},
      "templates": {"pencil": {"nodes": {}}}
    }"##;

    #[test]
    fn fixture_parses() {
        let s: SchemaReply = serde_json::from_str(FIXTURE).unwrap();
        let n = s.node("text.split").unwrap();
        assert_eq!(n.version, 3);
        assert_eq!(n.memo.scope, MemoScope::Session);
        assert!(n.export.markable);
        assert_eq!(n.editor.as_ref().unwrap().port, "curve");
        assert_eq!(n.input("sep").unwrap().param_ref.as_ref().unwrap().group, "split");
        assert_eq!(n.input("sep").unwrap().socket_mode(), Socket::Both);
        assert_eq!(s.wire_type("HOLOGRAM").unwrap().media, Media::Bytes);
        assert_eq!(s.bundle_type(), Some("SETTINGS"));
        assert_eq!(s.text("result_prefix"), Some("Result:"));
        assert_eq!(s.texts.len(), 1, "a non-string text is dropped");
        assert!(s.accepts("INT", "FLOAT") && !s.accepts("FLOAT", "INT"));
        let back: SchemaReply = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn defaults_and_structure() {
        let s: SchemaReply = serde_json::from_str(r#"{"v": 2, "nodes": [{"id": "x"}], "compat": []}"#).unwrap();
        assert_eq!(s.nodes[0].version, 1);
        assert_eq!(s.nodes[0].memo.scope, MemoScope::Machine);
        assert!(!s.accepts("INT", "FLOAT"), "an engine's empty compat list means no conversions");
        assert!(serde_json::from_str::<SchemaReply>(r#"{"v":2,"nodes":[{"inputs":[{"socket":"sideways"}]}]}"#).is_err());
    }
}
