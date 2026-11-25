//! The scene: what the canvas draws, as data, in graph units (`docs/shell-interface.md` section 8;
//! `docs/research/24-native-shells.md` 5.2). Built in N2 slice 2 from the document (graph,
//! catalogue, run state, the Watched panel's node, the selection, the group entered, the
//! application's hooks) and a theme, so a shell draws a node's face without knowing any engine.
//!
//! - [`build`] makes a [`Scene`] from [`Inputs`]: faces ([`Face`]: header, rows, widgets,
//!   preview, footer, run state), wires ([`Wire`]: ends as faces and pin indices, colour, kind,
//!   bend points, slice, net), nets, frames, groups, the application's wire marks, the
//!   selection, the group entered and the run animation's switches. Nothing in it needs measured
//!   geometry: wire paths, open groups' frames and marks' places are slice 3's.
//! - [`Published`] keeps the last scene published with a revision per item, so `scene.get
//!   {since}` sends only what changed (section 8.12). The generation rises only when something
//!   in the scene changed; a scene noted many times between two polls is one change.
//!
//! Colours are the core's [`Paint`]: the theme token a colour came from, where it has one, and
//! the colour itself (premultiplied RGBA, as [`Color`] holds it). The egui shell draws its
//! faces' words, colours and rows from [`build`]'s faces (`plumcraft-ui`'s `canvas.rs`).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Instant;

use plumcraft_graph::{Graph, GroupId, NetKey, NodeId, NodeView, PortRef};
use plumcraft_proto::catalogue::{NodeDef, Port, SchemaReply, Socket, scalars, texts, widgets as hints};
use plumcraft_proto::display::Rendition;
use plumcraft_proto::event::NodeState;
use plumcraft_session::run::{NodeRun, RunState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bundle::{self, Source};
use crate::plain::Color;
use crate::rows::{self, InputRow, port_label};
use crate::run_anim::{RunAnim, Shown};
use crate::run_text;
use crate::selection::{Selection, Visibility};
use crate::theme::{FamilyTint, Tokens, parse_hex};
use crate::widget_text::{self, typed_label, typed_tip, value_text};

/// The shell interface's version, in every scene.
pub const SHELL: u32 = 1;
/// The long side of a node's picture preview (graph units).
pub const THUMB_PX: f32 = 80.0;
/// The long side of a picture preview on hover.
pub const THUMB_HOVER_PX: f32 = 160.0;
/// A waveform preview's size (graph units).
pub const PEAKS_SIZE: [f32; 2] = [120.0, 44.0];
/// Characters of the engine's text shown on a node; the rest is in `full`.
pub const NODE_TEXT_CHARS: usize = 120;
/// Characters of the application's line shown on a node.
pub const APP_TEXT_CHARS: usize = 160;
/// How many generations of removals the published scene remembers for `scene.get {since}`.
pub const KEEP_GENERATIONS: u64 = 256;
/// The icon of a group (a collapsed group's node, an open group's header).
pub const GROUP_ICON: &str = "boxes";
/// The icon a node type without one gets.
pub const FALLBACK_ICON: &str = "box";
/// The icon of the button that enters a group.
pub const ENTER_ICON: &str = "pencil";

/// A colour: a theme token by name, or `[r, g, b, a]` where there is none (computed: a family's
/// tint, an engine's wire-type colour, a frame's own). Exactly one of the two is sent
/// (`docs/shell-interface.md` question 13, decided 13A; built in N2 slice 4): a shell resolves a
/// token with `theme.get`, so a theme change changes only the faces whose computed colours change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Paint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// `[r, g, b, a]`, premultiplied; none when `token` names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 4]>,
}

impl Paint {
    fn token(name: &str, _now: Color) -> Paint {
        Paint { token: Some(name.to_owned()), color: None }
    }
    fn of(c: Color) -> Paint {
        Paint { token: None, color: Some(c.to_array()) }
    }
    /// The colour in `tokens`' theme (a token the theme doesn't have: transparent).
    pub fn resolve(&self, tokens: &Tokens) -> Color {
        match (&self.color, &self.token) {
            (Some([r, g, b, a]), _) => Color::from_rgba_premultiplied(*r, *g, *b, *a),
            (None, Some(t)) => tokens.resolve(t).unwrap_or(Color::TRANSPARENT),
            (None, None) => Color::TRANSPARENT,
        }
    }
}

/// The whole scene, or (from [`Published::get`] with `since`) what changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Scene {
    pub shell: u32,
    pub doc: u64,
    #[serde(rename = "gen")]
    pub generation: u64,
    pub full: bool,
    pub nodes: Vec<Face>,
    pub wires: Vec<Wire>,
    pub nets: Vec<Net>,
    pub frames: Vec<Frame>,
    pub groups: Vec<Group>,
    pub marks: Vec<Mark>,
    pub selection: Picked,
    pub focus: Option<Focus>,
    pub anim: Anim,
    /// What the core asks the shell's view to show (N2 slice 3).
    #[serde(default)]
    pub view: View,
}

/// What the core asks the shell's view to show (`docs/shell-interface.md` 8.9's view domain, as
/// far as N2 slice 3 builds it): the latest fit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct View {
    /// The latest fit asked for (`canvas.fit`), or none since the document opened.
    pub fit: Option<Fit>,
    /// The node palette, while it is open (`canvas.palette`; core state since N3 slice 1, open
    /// question 4). Left out while it is closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<Palette>,
    /// The row a field editor is open on (`row.edit`, N4 slice 2): the shell puts its native
    /// control over it. Left out while none is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit: Option<crate::inline::RowEdit>,
    /// Select mode is on (`canvas.selectMode`, N4 slice 3): a one-finger drag on empty canvas
    /// draws a marquee rather than panning, on a shell recognising touch. Left out while off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub select: bool,
    /// The document's windows (contract 12.5's `windows` domain, carried in the scene's view
    /// since N4 slice 3): the parameters inspector and the pop-outs. Left out while none is open.
    #[serde(default, skip_serializing_if = "crate::windows::Windows::is_empty")]
    pub windows: crate::windows::Windows,
    /// The latest node asked to be revealed (`canvas.reveal`, N4 slice 3: core view state, as
    /// `fit` is): the shell centres its view on `rect` once per `seq`. Left out until one is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reveal: Option<Reveal>,
    /// The latest zoom asked for (`canvas.zoom`, N5 slice 3: Zoom In, Zoom Out and Actual Size
    /// are commands, the view itself the shell's): the shell zooms its view by `by`, or to `to`,
    /// about the view's centre, once per `seq`. Left out until one is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<Zoom>,
    /// A name being edited in place (`name.edit`, N4 slice 3): the shell puts a text field over
    /// `rect`. Left out while none is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<crate::naming::NameEdit>,
}

/// The node palette as the core keeps it (N3 slice 1): where a node picked goes, what was typed,
/// and the wire it was opened for, when a new wire was dropped on empty canvas (question 8).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Palette {
    /// Where a node picked is added (graph units), or none: the view's centre, the shell's.
    pub at: Option<[f64; 2]>,
    /// The search it opened with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// The wire dropped: only kinds that take it are offered (`palette.items`), and the one
    /// picked is added with it connected (`palette.choose`, one undo step).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire: Option<PaletteWire>,
}

/// The end of a dropped wire that stays: the output it leaves (`from`; a new node's input takes
/// it), or the input it enters (`to`; a new node's output feeds it). Exactly one is given.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PaletteWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<PortAt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<PortAt>,
}

/// A port: `{node, port}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PortAt {
    pub node: String,
    pub port: String,
}

impl PaletteWire {
    /// The port and whether it is an output (`from`).
    pub fn end(&self) -> Option<(plumcraft_graph::PortRef, bool)> {
        match (&self.from, &self.to) {
            (Some(f), None) => Some((plumcraft_graph::PortRef::new(f.node.as_str(), f.port.as_str()), true)),
            (None, Some(t)) => Some((plumcraft_graph::PortRef::new(t.node.as_str(), t.port.as_str()), false)),
            _ => None,
        }
    }
}

/// A fit the core asks for: the shell fits `rect` (graph units) into its view, zoom at most 1
/// (and at least 0.2), and animates there. `seq` rises with every request, so a shell fits once
/// a request however often the scene comes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Fit {
    pub seq: u64,
    /// `[x0, y0, x1, y1]`: every face shown, and the frames outside an entered group, with a
    /// margin of 32.
    pub rect: [f64; 4],
}

/// Reveal `node`, whose face is at `rect` (graph units), once per `seq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Reveal {
    pub seq: u64,
    pub node: String,
    pub rect: [f64; 4],
}

/// Zoom the view by `by` (Zoom In 1.25, Zoom Out 0.8) or to `to` (Actual Size 1), about its
/// centre, once per `seq` (`canvas.zoom`, N5 slice 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Zoom {
    pub seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<f64>,
}

/// The selection, as the scene carries it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Picked {
    pub nodes: Vec<String>,
    pub groups: Vec<String>,
    #[cfg_attr(feature = "schema", schemars(with = "Option<crate::shell_schema::NetKeySchema>"))]
    pub net: Option<NetKey>,
}

/// The group entered, and the breadcrumb to it (outermost first).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Focus {
    pub group: String,
    pub path: Vec<Crumb>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Crumb {
    pub id: String,
    pub name: String,
}

/// The run animation's switches. The fill a face takes from `shown` (and its pulse and fade)
/// is the shell's to compute per frame (section 8.6); the core never publishes per frame.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Anim {
    /// Faces are washed with their run state (View › Colour Nodes While Running, or a replay).
    pub on: bool,
    pub replay: bool,
    /// The run shown.
    pub generation: u64,
    pub hold_ms: u64,
    pub fade_ms: u64,
    pub pulse_ms: u64,
}

/// A node's face: a node, or a collapsed group's node (`proxy`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct Face {
    /// The node's id; a collapsed group's is `group:<id>`.
    pub id: String,
    /// Stable for the document's life, never reused (hot calls use it).
    pub nid: u64,
    /// The scene generation at which this face last changed.
    pub rev: u64,
    /// The node type ("" for a proxy).
    pub kind: String,
    pub proxy: Option<ProxyOf>,
    pub pos: [f32; 2],
    /// `full`, `compact` or `collapsed`.
    pub view: String,
    /// The Watched panel shows it (the border is 1.5 wider).
    pub watched: bool,
    pub tint: Tint,
    /// None: the theme's.
    pub border: Option<Border>,
    pub header: Header,
    pub inputs: Vec<InRow>,
    pub outputs: Vec<OutRow>,
    pub preview: Option<Preview>,
    pub footer: Vec<Footer>,
    pub run: RunShown,
    /// The run animation's state for the face (`waiting`, `running`, `done`, `cached`,
    /// `failed`, `skipped`); a proxy shows its members' busiest.
    pub shown: Option<String>,
    /// The context menu to fetch with `menu.get` (`node` or `proxy`).
    pub menu: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ProxyOf {
    pub group: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Tint {
    pub header: Paint,
    pub body: Paint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Border {
    pub paint: Paint,
    pub width: f32,
}

/// The node's real run state (section 8.6's first layer).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct RunShown {
    /// `queued`, `running` (started), `progress`, `preview`, `done`, `cached`, `error`,
    /// `skipped`, `cancelled`; none before any run.
    pub state: Option<String>,
    pub fraction: Option<f64>,
    pub stale: bool,
    pub not_run: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct Header {
    /// The node is open (not `collapsed`); a proxy's chevron expands its group.
    pub open: bool,
    /// A Lucide icon name.
    pub icon: String,
    /// What the icon's hover says of the node type.
    pub about: About,
    pub state: Spot,
    pub title: Title,
    pub badges: Vec<Badge>,
    pub engine: Option<EngineMark>,
    pub status: Vec<Status>,
    /// A proxy's enter button (`canvas.focus`).
    pub enter: bool,
}

/// The node type, for the icon's hover and the hover card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct About {
    pub name: String,
    pub description: String,
    /// `Family: what it means`.
    pub family: Option<String>,
    /// Where it sits in the palette (`A › B`).
    pub path: Option<String>,
    pub id: String,
    /// The engine knows the type.
    pub known: bool,
}

/// The header's state mark: `spinner` while running, `dot` in the state's colour, `none`
/// before any run (the slot is kept).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Spot {
    pub kind: String,
    pub paint: Option<Paint>,
    pub hover: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Title {
    pub text: String,
    /// A tap watches the node's first output (it has outputs), a double tap opens its parameters.
    pub watch: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Badge {
    pub text: String,
    pub paint: Paint,
    pub hover: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EngineMark {
    pub text: String,
    pub pinned: bool,
    pub hover: String,
}

/// A piece of the header's status slot (at least 88 wide).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Status {
    pub text: String,
    pub paint: Paint,
    pub strong: bool,
    pub hover: Option<String>,
}

/// A pin: round for scalars, square otherwise; a port that can't be wired has a hidden pin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Pin {
    /// `circle` or `square`.
    pub shape: String,
    pub fill: Paint,
    /// The colour of a wire drawn from it.
    pub wire: Paint,
    pub wireable: bool,
}

/// An input row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct InRow {
    /// The value and wire key (`sigma`, `mode.turn`, `c2`).
    pub key: String,
    /// The node whose port it is: the face's own, or on a proxy the member it stands for.
    pub node: String,
    /// The label as drawn (`[ ]` after it for a list).
    pub label: String,
    pub list: bool,
    pub ty: String,
    pub pin: Pin,
    /// `wired`, `socket`, `inherited`, `own` or `readonly` (section 8.4).
    pub source: String,
    /// The row's words where it has no widget: `label = value (wired from Title)`, the typed
    /// label, the bundle's value, `label: value`.
    pub text: Option<String>,
    pub hover: Option<String>,
    /// The caption is dim: the value is the default.
    pub dim: bool,
    /// `inherited`: `from <bundle>`.
    pub from: Option<String>,
    /// A named port's tag before the caption.
    pub named: Option<Named>,
    /// `wired` over a bundled value: the badge whose button resets it (`bundle.reset`).
    pub badge: Option<Badge>,
    pub widget: Option<Widget>,
    /// Not read with the node's current values (`Port::active_when`).
    pub inactive: Option<String>,
    /// The value its node names an editor for (`NodeDef.editor`), not on a wire: a tap on the
    /// row opens it, `node.editor {node}` (the row's `node`: on a collapsed group's face, the
    /// member's; FR1), as the node's own footer does. Absent for every other row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<RowEditor>,
}

/// What a row whose value has an editor says of it (section 8.4): the application's editor's
/// label and help where it registered one for the kind, else the command's own words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RowEditor {
    /// The editor's kind (`NodeDef.editor.kind`).
    pub kind: String,
    pub label: String,
    pub hover: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Named {
    pub name: String,
    pub paint: Paint,
    pub hover: String,
}

/// An output row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct OutRow {
    pub key: String,
    /// The node whose output it is (on a proxy, the member's).
    pub node: String,
    /// The label as drawn (typed, `[ ]` before it for a list).
    pub label: String,
    pub list: bool,
    pub ty: String,
    pub pin: Pin,
    pub hover: String,
}

/// A widget: what it shows and what bounds it. Every widget commits one `node.set` (8.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct Widget {
    /// `number`, `toggle`, `choice`, `multichoice`, `text`, `multiline`, `path`, `color`.
    pub kind: String,
    pub value: Value,
    pub default: Value,
    /// The value is JSON null (shown as `blank`).
    pub blank: bool,
    /// The default is null and the value not: a `×` sets null.
    pub clearable: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub soft_min: Option<f64>,
    pub soft_max: Option<f64>,
    pub step: Option<f64>,
    /// FLOAT: the decimals shown ([`widget_text::float_decimals`]).
    pub decimals: Option<usize>,
    /// A number as every shell shows it (N4 slice 4: an INT whole, a FLOAT at `decimals`;
    /// [`widget_text::number_text`]): on the face, in the inspector and while scrubbing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub int: bool,
    pub choices: Vec<Choice>,
    /// `path`: `open`, `save`, `dir`, `files`.
    pub path_mode: Option<String>,
    /// `color`: `rgb` (`r,g,b` in 0..1) or `hex`.
    pub color_format: Option<String>,
    pub hover: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Choice {
    pub value: Value,
    pub label: String,
    /// Why it can't be picked here.
    pub disabled: Option<String>,
}

/// A node's preview (only in view `full`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Preview {
    /// A picture, by its value reference (`media.get`). `items` above 1: the value is a list (of
    /// named pictures, when the engine names them) and this is one of them, `caption` its name.
    #[serde(rename_all = "camelCase")]
    /// Fields may be added: outside this crate a pattern ends with `..` (the core builds it).
    #[non_exhaustive]
    Rgba8 {
        handle: Value,
        w: u32,
        h: u32,
        size: [f32; 2],
        hover_size: [f32; 2],
        items: u64,
        /// A nested list's counts a level at a time, outermost first (`[3, 2]`: 3 runs of 2; the
        /// face says "3 × 2"); empty when the list isn't nested (spec 7.6 rule 2).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        levels: Vec<u64>,
        summary: String,
        /// The picture's own name (the rendition's `caption`), when the engine gives one.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        caption: String,
        note: Option<String>,
        faded: bool,
    },
    /// A waveform (`media.peaks`).
    #[serde(rename_all = "camelCase")]
    Peaks { handle: Value, channels: u16, size: [f32; 2], note: Option<String>, faded: bool },
}

impl Preview {
    pub fn size(&self) -> [f32; 2] {
        match self {
            Preview::Rgba8 { size, .. } | Preview::Peaks { size, .. } => *size,
        }
    }
}

/// A footer item, in the order drawn (section 8.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Footer {
    /// "3 more fields" / "Fewer fields": `node.fields {node, all}`.
    Fields {
        text: String,
        hover: String,
        all: bool,
    },
    /// A Convert node's path, a line each.
    Convert {
        lines: Vec<Line>,
    },
    /// The application's editor: `node.editor {node}`.
    Editor {
        label: String,
        hover: String,
    },
    Progress {
        fraction: f64,
        stage: Option<String>,
    },
    Error {
        text: String,
        full: String,
        paint: Paint,
    },
    /// Save Result… (and Contact Sheet… with `sheet`): `run.save {node, port, sheet, ask: true}`.
    Save {
        port: String,
        sheet: bool,
    },
    /// The text the node made, a few lines: a tap watches it.
    Excerpt {
        text: String,
        faded: bool,
        hover: String,
    },
    #[serde(rename = "engineText")]
    EngineText {
        text: String,
        full: String,
    },
    #[serde(rename = "appText")]
    AppText {
        text: String,
        full: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Line {
    pub text: String,
    pub paint: Paint,
    pub hover: Option<String>,
}

/// A wire, keyed by the input it feeds (one wire an input).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct Wire {
    /// `node.port` of the input.
    pub id: String,
    pub nid: u64,
    pub rev: u64,
    #[cfg_attr(feature = "schema", schemars(with = "crate::shell_schema::PortRefSchema"))]
    pub from: PortRef,
    #[cfg_attr(feature = "schema", schemars(with = "crate::shell_schema::PortRefSchema"))]
    pub to: PortRef,
    /// The faces and pins it is drawn between (a collapsed group's node stands for its members).
    pub from_face: String,
    pub from_pin: usize,
    pub to_face: String,
    pub to_pin: usize,
    pub paint: Paint,
    /// `plain`, `routed` (through `via`) or `net` (drawn through its net's tags).
    pub kind: String,
    pub via: Vec<[i32; 2]>,
    /// A slice's label (`[:-1]`).
    pub slice: Option<String>,
    #[cfg_attr(feature = "schema", schemars(with = "Option<crate::shell_schema::NetKeySchema>"))]
    pub net: Option<NetKey>,
    /// Its path as a cubic chain, graph units: the start, then two control points and the end a
    /// segment ([`crate::wire_path`]); every shell draws this curve and the core hit-tests it.
    /// Empty for a wire drawn through a net (its tags are drawn). N2 slice 3.
    pub path: Vec<[f64; 2]>,
}

/// A net drawn through labels: its driving tag at the output, a receiving tag at each input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Net {
    /// `name`, or `name@group` for a net local to a group.
    pub id: String,
    pub nid: u64,
    pub rev: u64,
    #[cfg_attr(feature = "schema", schemars(with = "crate::shell_schema::NetKeySchema"))]
    pub key: NetKey,
    pub paint: Paint,
    pub selected: bool,
    pub driver: Driver,
    pub uses: Vec<Use>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Driver {
    #[cfg_attr(feature = "schema", schemars(with = "crate::shell_schema::PortRefSchema"))]
    pub from: PortRef,
    pub face: String,
    pub pin: usize,
    /// The tag's offset from its pin.
    pub at: [i32; 2],
    /// What the driver's tag says: the net's name and how many inputs it feeds ("picture ×3",
    /// as the egui editor's tag; N4 slice 3). The hit test's estimate of the tag is this text's.
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Use {
    #[cfg_attr(feature = "schema", schemars(with = "crate::shell_schema::PortRefSchema"))]
    pub to: PortRef,
    pub face: String,
    pub pin: usize,
    pub at: [i32; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Frame {
    pub id: String,
    pub nid: u64,
    pub rev: u64,
    pub title: String,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub paint: Paint,
}

/// A group. A collapsed one is drawn as its proxy face; an open one as a frame around its
/// members, whose rect needs measured geometry (slice 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Group {
    pub id: String,
    pub nid: u64,
    pub rev: u64,
    pub name: String,
    pub collapsed: bool,
    pub parent: Option<String>,
    pub depth: usize,
    /// Its proxy face is drawn (the outermost collapsed group, visible).
    pub proxy: bool,
    /// An open group's frame, `[x0, y0, x1, y1]`, the header band its top
    /// [`crate::geometry::GROUP_HEADER`] ([`crate::geometry::open_group_rects`]); none when
    /// collapsed, entered, or none of its faces is shown. N2 slice 3.
    pub rect: Option<[f64; 4]>,
}

/// An application's mark on a wire (`Hooks::wire_marks`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Mark {
    /// The wire it marks (`node.port` of its input).
    pub id: String,
    pub nid: u64,
    pub rev: u64,
    pub label: String,
    /// `info`, `warn` or `error`.
    pub tone: String,
    pub paint: Paint,
    pub tooltip: Option<String>,
    /// Where it is drawn: its wire's middle (none when its wire isn't drawn). N2 slice 3.
    pub at: Option<[f64; 2]>,
}

/// An application's value editor, as the scene needs it (`plumcraft-ui`'s `value_editors`).
#[derive(Debug, Clone)]
pub struct Editor {
    pub kind: String,
    pub label: String,
    pub help: String,
    /// A short word on a value, where there is no widget for it.
    pub summary: Option<fn(&Value) -> String>,
}

/// What a scene is built from.
pub struct Inputs<'a> {
    pub doc: u64,
    pub graph: &'a Graph,
    pub catalog: &'a SchemaReply,
    pub run: &'a RunState,
    pub selection: Selection,
    pub focus: Option<&'a GroupId>,
    /// The node the Watched panel shows.
    pub watched: Option<NodeId>,
    pub tokens: &'a Tokens,
    /// The run animation, when faces are washed with it.
    pub anim: Option<&'a RunAnim>,
    pub now: Instant,
    /// The application's line on each node, by node id.
    pub app_text: &'a HashMap<String, String>,
    pub editors: &'a [Editor],
    /// The view requests the document holds.
    pub view: View,
    /// What the shell measured (N2 slice 3); none: every face its default size.
    pub geometry: Option<&'a crate::geometry::Measured>,
}

/// The application's line on each node (`Hooks::node_overlays`), by node id.
pub fn app_texts() -> HashMap<String, String> {
    plumcraft_session::commands::node_overlays().into_iter().filter_map(|(NodeId(id), o)| o.text.filter(|t| !t.is_empty()).map(|t| (id, t))).collect()
}

/// The token a run state's colour is.
pub fn state_token(s: NodeState) -> Option<&'static str> {
    Some(match s {
        NodeState::Queued => "state_queued",
        NodeState::Started | NodeState::Progress | NodeState::Preview => "state_running",
        NodeState::Done => "state_done",
        NodeState::Cached => "state_cached",
        NodeState::Error => "state_error",
        NodeState::Skipped => "state_skipped",
        NodeState::Cancelled => "state_cancelled",
        NodeState::Unknown => return None,
    })
}

/// A run state's wire name.
pub fn state_word(s: NodeState) -> Option<&'static str> {
    Some(match s {
        NodeState::Queued => "queued",
        NodeState::Started => "started",
        NodeState::Progress => "progress",
        NodeState::Preview => "preview",
        NodeState::Done => "done",
        NodeState::Cached => "cached",
        NodeState::Error => "error",
        NodeState::Skipped => "skipped",
        NodeState::Cancelled => "cancelled",
        NodeState::Unknown => return None,
    })
}

/// The size to show a `w`×`h` picture at: the long side `max`, aspect kept.
pub fn thumb_size(w: f32, h: f32, max: f32) -> [f32; 2] {
    if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
        return [max, max * 0.5];
    }
    let scale = max / w.max(h);
    [(w * scale).max(1.0), (h * scale).max(1.0)]
}

/// Is a node's preview faded: stale, failed, or from an earlier run than the one now computing
/// it (B22)?
pub fn thumb_faded(run: &NodeRun) -> bool {
    run.is_stale() || run.state == Some(NodeState::Error) || run.thumb_is_older()
}

/// What a preview's hover adds about how current it is.
pub fn thumb_note(run: &NodeRun) -> Option<String> {
    if run.is_stale() {
        Some(run_text::stale_tooltip(run))
    } else if run.thumb_is_older() {
        Some(run_text::OLDER_THUMB.to_owned())
    } else {
        None
    }
}

fn view_word(v: NodeView) -> &'static str {
    match v {
        NodeView::Full => "full",
        NodeView::Compact => "compact",
        NodeView::Collapsed => "collapsed",
    }
}

/// The scene's words and colours for one build: the inputs, the visibility, and family colours
/// worked out once a family (a family's colour looks at the whole catalogue).
struct Ctx<'a> {
    i: &'a Inputs<'a>,
    vis: Visibility,
    tints: std::cell::RefCell<HashMap<String, FamilyTint>>,
    /// The wire into each input (the first, as [`Graph::wire_into`] finds it), indexed once a
    /// build: looking each row's up in the graph's list made a build O(rows × wires).
    into: HashMap<&'a PortRef, &'a plumcraft_graph::Wire>,
    /// Every node's title, worked out once a build (N2 slice 3): a title each, by
    /// [`plumcraft_session::node_title`], looks at every other node, which made a build
    /// quadratic and was most of its cost at 2,000 nodes.
    titles: std::collections::BTreeMap<NodeId, String>,
    /// The catalogue's node types by id, indexed once a build (`SchemaReply::node` searches the
    /// list, and a build asks for each node and each wired row).
    defs: HashMap<&'a str, &'a NodeDef>,
}

impl<'a> Ctx<'a> {
    fn new(i: &'a Inputs<'a>) -> Ctx<'a> {
        let mut into = HashMap::with_capacity(i.graph.wires.len());
        for w in &i.graph.wires {
            into.entry(&w.to).or_insert(w);
        }
        let mut defs = HashMap::with_capacity(i.catalog.nodes.len());
        for d in &i.catalog.nodes {
            // the first of an id, as `SchemaReply::node` finds it
            defs.entry(d.id.as_str()).or_insert(d);
        }
        Ctx {
            i,
            vis: Visibility::new(i.graph, i.focus),
            tints: std::cell::RefCell::new(HashMap::new()),
            into,
            titles: plumcraft_session::node_titles(i.graph, i.catalog),
            defs,
        }
    }

    fn wire_into(&self, at: &PortRef) -> Option<&'a plumcraft_graph::Wire> {
        self.into.get(at).copied()
    }

    /// What the shell measured of face `id`.
    fn measured(&self, id: &str) -> Option<&'a crate::geometry::FaceGeometry> {
        self.i.geometry.and_then(|g| g.get(id))
    }

    /// A node type, as `SchemaReply::node` finds it (Convert included).
    fn def(&self, kind: &str) -> Option<&'a NodeDef> {
        self.defs.get(kind).copied().or_else(|| self.i.catalog.node(kind))
    }
}

impl Ctx<'_> {
    fn tok(&self, name: &str) -> Paint {
        Paint::token(name, self.i.tokens.token(name).unwrap_or(Color::TRANSPARENT))
    }

    fn type_color(&self, ty: &str) -> Option<Color> {
        self.i.catalog.wire_type(ty).and_then(|w| w.color.as_deref()).and_then(parse_hex)
    }

    /// A pin in its wire type's colour: round for scalars, square for everything else.
    fn pin(&self, ty: &str, wireable: bool) -> Pin {
        let t = self.i.tokens;
        if !wireable {
            return Pin { shape: "circle".into(), fill: Paint::token("pin_hidden", t.pin_hidden), wire: self.tok("wire"), wireable: false };
        }
        let typed = self.type_color(ty);
        Pin {
            shape: if scalars::is_scalar(ty) { "circle" } else { "square" }.into(),
            fill: typed.map_or_else(|| self.tok("pin_default"), Paint::of),
            wire: typed.map_or_else(|| self.tok("wire"), Paint::of),
            wireable: true,
        }
    }

    fn family_tint(&self, family: &str) -> FamilyTint {
        if let Some(t) = self.tints.borrow().get(family) {
            return *t;
        }
        let t = self.i.tokens.family_tint(self.i.catalog, family);
        self.tints.borrow_mut().insert(family.to_owned(), t);
        t
    }

    fn title(&self, id: &NodeId) -> String {
        self.titles.get(id).cloned().unwrap_or_else(|| id.to_string())
    }

    /// The face as the run animation shows it now; a proxy shows its members' busiest
    /// (running, else waiting, failed, skipped, done, cached).
    fn shown(&self, id: &NodeId, proxy: Option<&GroupId>) -> Option<Shown> {
        let anim = self.i.anim?;
        let Some(g) = proxy else { return anim.shown(&id.0, self.i.now) };
        let states: Vec<Shown> = self.i.graph.group_members(g).iter().filter_map(|m| anim.shown(&m.0, self.i.now)).collect();
        [Shown::Running, Shown::Waiting, Shown::Failed, Shown::Skipped, Shown::Done, Shown::Cached].into_iter().find(|s| states.contains(s))
    }

    /// The preview is faded: out of date, or ahead of the replay.
    fn faded_now(&self, id: &NodeId, n: &NodeRun) -> bool {
        thumb_faded(n) || (n.is_finished() && matches!(self.shown(id, None), Some(Shown::Waiting | Shown::Running)))
    }

    fn editor(&self, def: &NodeDef) -> Option<&Editor> {
        let kind = &def.editor.as_ref()?.kind;
        self.i.editors.iter().find(|e| &e.kind == kind)
    }
}

/// The faces alone, in the order the canvas draws them (nodes, then collapsed groups' nodes):
/// what a shell that draws every frame needs, without the wires, nets and hooks.
pub fn faces(i: &Inputs<'_>) -> Vec<Face> {
    let cx = Ctx::new(i);
    faces_in(&cx)
}

/// Node `id`'s own face, shown or not (a member of a collapsed group, outside the group
/// entered): what `row.set` reads a row's widget from.
pub fn face_of_node(i: &Inputs<'_>, id: &NodeId) -> Option<Face> {
    let cx = Ctx::new(i);
    i.graph.node(id).map(|node| node_face(&cx, id, node))
}

/// Every input row of node `id`, shown on its face or not, each with the catalogue's row it
/// came from (N4 slice 3: the parameters inspector, and `row.set`, which takes any input).
pub fn param_rows(i: &Inputs<'_>, id: &NodeId) -> Vec<(InRow, rows::InputRow)> {
    let cx = Ctx::new(i);
    let Some(node) = i.graph.node(id) else { return Vec::new() };
    let Some(def) = cx.def(&node.kind) else { return Vec::new() };
    rows::input_rows_with(def, id, i.graph, &|p| cx.wire_into(p).is_some()).into_iter().map(|r| (in_row(&cx, id, &r), r)).collect()
}

fn faces_in(cx: &Ctx<'_>) -> Vec<Face> {
    let mut nodes = Vec::new();
    for (id, node) in &cx.i.graph.nodes {
        if cx.vis.shows(id) {
            nodes.push(node_face(cx, id, node));
        }
    }
    for gid in &cx.vis.proxies {
        if let Some(f) = proxy_face(cx, gid) {
            nodes.push(f);
        }
    }
    make_room(cx, &mut nodes);
    nodes
}

/// The gap a face pushed out of another's way keeps from it (graph units).
pub const ROOM_GAP: f32 = 16.0;
/// The most passes [`make_room`] makes (each settles the overlaps the one before made).
pub const ROOM_PASSES: usize = 8;

/// Faces never overlap (N4 slice 2, the maintainer's N3 exit decision 13: "Nodes that grow
/// after a run overlap their neighbours: Fix in N4"). Where two measured faces overlap, the
/// lower one (the later by top, then left, then id) is pushed out of the way: right when it
/// stands to the right of the other and that is the shorter way, else down, keeping
/// [`ROOM_GAP`]; a face pushed may push the next. The push is the view's, not the graph's: a
/// face's `pos` in the scene is where it is shown, its node's position is untouched, so a run
/// that grows a face changes nothing of the document (no undo step, nothing to save), and when
/// the face shrinks again the push goes. A drag or a nudge moves a face from where it is shown
/// (the gesture's `edit.move` names the shown place plus the offset). Faces not measured yet are
/// left where they are (their size is a guess).
fn make_room(cx: &Ctx<'_>, nodes: &mut [Face]) {
    use crate::plain::{Point, Rect};
    let mut rects: Vec<Option<Rect>> = nodes.iter().map(|f| cx.measured(&f.id).map(|g| crate::geometry::face_rect(f.pos, Some(g)))).collect();
    let mut shift = vec![[0.0f32; 2]; nodes.len()];
    let mut any = false;
    for _ in 0..ROOM_PASSES {
        let mut order: Vec<usize> = (0..nodes.len()).filter(|k| rects.get(*k).is_some_and(Option::is_some)).collect();
        let key = |k: usize| rects.get(k).copied().flatten().map_or((0.0, 0.0), |r| (r.min.y, r.min.x));
        order.sort_by(|a, b| key(*a).partial_cmp(&key(*b)).unwrap_or(std::cmp::Ordering::Equal).then_with(|| nodes[*a].id.cmp(&nodes[*b].id)));
        let mut changed = false;
        for (oi, &b) in order.iter().enumerate() {
            for &a in &order[..oi] {
                let (Some(ra), Some(rb)) = (rects[a], rects[b]) else { continue };
                let overlaps = ra.min.x < rb.max.x && rb.min.x < ra.max.x && ra.min.y < rb.max.y && rb.min.y < ra.max.y;
                if !overlaps || ra.width() <= 0.0 || ra.height() <= 0.0 || rb.width() <= 0.0 || rb.height() <= 0.0 {
                    continue;
                }
                let dx = ra.max.x - rb.min.x + ROOM_GAP;
                let dy = ra.max.y - rb.min.y + ROOM_GAP;
                let by = if rb.min.x > ra.min.x && dx < dy { Point::new(dx, 0.0) } else { Point::new(0.0, dy) };
                rects[b] = Some(Rect::from_min_max(rb.min + by, rb.max + by));
                shift[b][0] += by.x;
                shift[b][1] += by.y;
                changed = true;
            }
        }
        any |= changed;
        if !changed {
            break;
        }
    }
    if any {
        for (f, s) in nodes.iter_mut().zip(shift) {
            f.pos = [crate::wire_path::coord(f.pos[0] + s[0]), crate::wire_path::coord(f.pos[1] + s[1])];
        }
    }
}

/// Build the scene (generation 0, full).
pub fn build(i: &Inputs<'_>) -> Scene {
    let cx = Ctx::new(i);
    let graph = i.graph;
    let nodes = faces_in(&cx);
    let rects: HashMap<&str, crate::plain::Rect> = nodes.iter().map(|f| (f.id.as_str(), crate::geometry::face_rect(f.pos, cx.measured(&f.id)))).collect();
    let (wires, nets) = wires_and_nets(&cx, &nodes, &rects);
    let open = crate::geometry::open_group_rects(graph, i.focus, &rects);
    let middles: HashMap<&str, [f64; 2]> = wires
        .iter()
        .filter_map(|w| {
            let chain: Vec<crate::plain::Point> = w.path.iter().map(|p| crate::plain::Point::new(p[0] as f32, p[1] as f32)).collect();
            crate::wire_path::chain_middle(&chain).map(|m| (w.id.as_str(), [crate::geometry::round(m.x), crate::geometry::round(m.y)]))
        })
        .collect();
    let frames = graph
        .frames
        .iter()
        .map(|(id, f)| Frame {
            id: id.to_string(),
            nid: 0,
            rev: 0,
            title: f.title.clone(),
            pos: f.pos,
            size: grown_frame(graph, id, f, &nodes, &rects),
            paint: f.color.as_deref().and_then(parse_hex).map_or_else(|| cx.tok("accent"), Paint::of),
        })
        .collect();
    let groups = graph
        .groups
        .iter()
        .map(|(id, g)| Group {
            id: id.to_string(),
            nid: 0,
            rev: 0,
            name: g.name.clone(),
            collapsed: g.collapsed,
            parent: g.parent.as_ref().map(ToString::to_string),
            depth: depth_of(graph, id),
            proxy: cx.vis.proxies.contains(id),
            rect: open.get(id).map(|r| crate::geometry::rect_json(*r)),
        })
        .collect();
    let marks = plumcraft_session::commands::wire_marks(graph)
        .into_iter()
        .map(|(to, m)| {
            let (tone, token) = match m.tone {
                plumcraft_session::commands::Tone::Info => ("info", "accent"),
                plumcraft_session::commands::Tone::Warn => ("warn", "warning"),
                plumcraft_session::commands::Tone::Error => ("error", "error"),
            };
            let id = format!("{}.{}", to.node, to.port);
            let at = middles.get(id.as_str()).copied();
            Mark { id, nid: 0, rev: 0, label: m.label, tone: tone.into(), paint: cx.tok(token), tooltip: m.tooltip, at }
        })
        .collect();
    let s = &i.selection;
    let selection =
        Picked { nodes: s.nodes.iter().map(|n| n.0.clone()).collect(), groups: s.groups.iter().map(ToString::to_string).collect(), net: s.net.clone() };
    let focus = i.focus.filter(|g| graph.group(g).is_some()).map(|g| {
        let mut path = Vec::new();
        let mut at = Some(g.clone());
        while let Some(gid) = at {
            if path.len() > graph.groups.len() {
                break;
            }
            let grp = graph.group(&gid);
            path.push(Crumb { id: gid.to_string(), name: grp.map(|g| g.name.clone()).unwrap_or_default() });
            at = grp.and_then(|g| g.parent.clone());
        }
        path.reverse();
        Focus { group: g.to_string(), path }
    });
    let anim = Anim {
        on: i.anim.is_some(),
        replay: i.anim.is_some_and(RunAnim::is_replay),
        generation: i.anim.map_or(0, RunAnim::generation),
        hold_ms: crate::run_anim::HOLD.as_millis() as u64,
        fade_ms: crate::run_anim::FADE.as_millis() as u64,
        pulse_ms: 1200,
    };
    Scene { shell: SHELL, doc: i.doc, generation: 0, full: true, nodes, wires, nets, frames, groups, marks, selection, focus, anim, view: i.view.clone() }
}

/// The room a frame keeps round a member it grew for (graph units).
pub const FRAME_GROW_PAD: f32 = 16.0;

/// A frame's size as shown (N4 slice 3): its own, grown right and down to hold every member the
/// push moved ([`make_room`]), so a face pushed out of another's way stays inside the frame that
/// holds it. Like the push, this is the view's: the frame's size in the graph is untouched, and
/// the growth goes when the push does. What the frame holds is still what its own rect holds
/// (`Graph::frame_held`, by the members' positions), so moving it moves the same things.
fn grown_frame(
    graph: &Graph,
    id: &plumcraft_graph::FrameId,
    f: &plumcraft_graph::Frame,
    faces: &[Face],
    rects: &HashMap<&str, crate::plain::Rect>,
) -> [f32; 2] {
    let held = graph.frame_held(id);
    let own_max = [f.pos[0] + f.size[0], f.pos[1] + f.size[1]];
    let mut max = own_max;
    let pushed = |face: &Face, own: [f32; 2]| face.pos != own;
    for face in faces {
        let own = match face.id.strip_prefix("group:") {
            Some(g) => {
                let gid = GroupId::from(g);
                if !held.groups.contains(&gid) {
                    continue;
                }
                match graph.group(&gid) {
                    Some(grp) => grp.pos,
                    None => continue,
                }
            }
            None => {
                let nid = NodeId::from(face.id.as_str());
                if !held.nodes.contains(&nid) {
                    continue;
                }
                match graph.node(&nid) {
                    Some(n) => n.pos,
                    None => continue,
                }
            }
        };
        if !pushed(face, own) {
            continue;
        }
        if let Some(r) = rects.get(face.id.as_str()) {
            max[0] = max[0].max(r.max.x + FRAME_GROW_PAD);
            max[1] = max[1].max(r.max.y + FRAME_GROW_PAD);
        }
    }
    if max == own_max {
        return f.size;
    }
    [crate::wire_path::coord(max[0] - f.pos[0]), crate::wire_path::coord(max[1] - f.pos[1])]
}

fn depth_of(graph: &Graph, gid: &GroupId) -> usize {
    let mut depth = 0usize;
    let mut at = graph.group(gid).and_then(|g| g.parent.clone());
    while let Some(p) = at {
        depth = depth.saturating_add(1);
        if depth > graph.groups.len() {
            break;
        }
        at = graph.group(&p).and_then(|g| g.parent.clone());
    }
    depth
}

fn node_face(cx: &Ctx<'_>, id: &NodeId, node: &plumcraft_graph::Node) -> Face {
    let i = cx.i;
    let def = cx.def(&node.kind);
    let (rows, tucked) = def.map(|d| rows::shown_rows_with(d, id, i.graph, &|p| cx.wire_into(p).is_some())).unwrap_or_default();
    let run = i.run.node(&id.0);
    let tint = cx.family_tint(def.map_or("", |d| d.family.as_str()));
    let inputs = rows.iter().map(|r| in_row(cx, id, r)).collect();
    let outputs = def.map(|d| d.outputs.iter().map(|p| out_row(cx, id, p, port_label(p))).collect()).unwrap_or_default();
    let watched = i.watched.as_ref() == Some(id);
    let mut footer = Vec::new();
    if tucked > 0 {
        let (text, hover) = if node.all_fields {
            ("Fewer fields".to_owned(), "Tuck away the fields this choice doesn't use")
        } else {
            (format!("{tucked} more field{}", if tucked == 1 { "" } else { "s" }), "Show the fields this choice doesn't use")
        };
        footer.push(Footer::Fields { text, hover: hover.into(), all: !node.all_fields });
    }
    if i.graph.is_convert(id) {
        footer.push(Footer::Convert { lines: convert_lines(cx, id) });
    }
    if let Some(d) = def
        && let Some(e) = cx.editor(d)
        && let Some(port) = d.editor.as_ref().map(|e| e.port.clone())
        && cx.wire_into(&PortRef::new(id.clone(), port)).is_none()
    {
        footer.push(Footer::Editor { label: e.label.clone(), hover: e.help.clone() });
    }
    let full = node.view == NodeView::Full;
    if let Some(n) = run {
        run_footer(cx, id, def, n, full, &mut footer);
    }
    if let Some(text) = i.app_text.get(&id.0) {
        let (short, _) = run_text::truncate_chars(text, APP_TEXT_CHARS);
        footer.push(Footer::AppText { text: short, full: text.clone() });
    }
    let preview = run.filter(|_| full).and_then(|n| preview(cx, id, n));
    Face {
        id: id.0.clone(),
        nid: 0,
        rev: 0,
        kind: node.kind.clone(),
        proxy: None,
        pos: node.pos,
        view: view_word(node.view).into(),
        watched,
        tint: Tint { header: Paint::of(tint.header), body: Paint::of(tint.body) },
        border: border(cx, run, watched),
        header: header(cx, id, def, run, node.view != NodeView::Collapsed, None),
        inputs,
        outputs,
        preview,
        footer,
        run: run_shown(run),
        shown: cx.shown(id, None).map(|s| s.name().to_owned()),
        menu: "node".into(),
    }
}

fn proxy_face(cx: &Ctx<'_>, gid: &GroupId) -> Option<Face> {
    let i = cx.i;
    let grp = i.graph.group(gid)?;
    let id = rows::proxy_node_id(gid);
    let (rows, members) = rows::proxy_inputs(i.graph, i.catalog, gid);
    let inputs = rows.iter().zip(&members).map(|(r, m)| in_row(cx, m, r)).collect();
    let outputs = rows::proxy_outputs(i.graph, i.catalog, gid).into_iter().map(|o| out_row(cx, &o.port.node, &o.def, o.label)).collect();
    let tint = i.tokens.group;
    Some(Face {
        id: id.0.clone(),
        nid: 0,
        rev: 0,
        kind: String::new(),
        proxy: Some(ProxyOf { group: gid.to_string(), name: grp.name.clone() }),
        pos: grp.pos,
        view: "full".into(),
        watched: false,
        tint: Tint { header: Paint::token("group", tint.header), body: Paint::token("group_body", tint.body) },
        border: Some(Border { paint: Paint::token("group", tint.header), width: 2.0 }),
        header: header(cx, &id, None, i.run.node(&id.0), true, Some(grp.name.as_str())),
        inputs,
        outputs,
        preview: None,
        footer: Vec::new(),
        run: RunShown::default(),
        shown: cx.shown(&id, Some(gid)).map(|s| s.name().to_owned()),
        menu: "proxy".into(),
    })
}

fn run_shown(run: Option<&NodeRun>) -> RunShown {
    match run {
        None => RunShown { not_run: true, ..RunShown::default() },
        Some(n) => RunShown {
            state: n.state.and_then(state_word).map(str::to_owned),
            fraction: n.fraction.filter(|f| f.is_finite()),
            stale: run_text::shows_stale(n),
            not_run: n.is_not_run(),
        },
    }
}

/// A node's border: stale, or its run state, thicker when it is the watched node (a revealed
/// node's highlight is the shell's, over this).
fn border(cx: &Ctx<'_>, run: Option<&NodeRun>, watched: bool) -> Option<Border> {
    let width = |base: f32| if watched { base + 1.5 } else { base };
    // stale is for a result an edit made out of date (a node that never completed has none),
    // and gives way to queued and running while a run computes the node again
    if run.is_some_and(run_text::shows_stale) {
        return Some(Border { paint: cx.tok("stale"), width: width(2.0) });
    }
    match run.and_then(|r| r.state).and_then(|s| state_token(s).map(|t| (s, t))) {
        Some((s, t)) => Some(Border { paint: cx.tok(t), width: width(if s == NodeState::Error || run_text::is_running(s) { 2.5 } else { 2.0 }) }),
        None if watched => Some(Border { paint: cx.tok("accent"), width: 2.5 }),
        None => None,
    }
}

fn header(cx: &Ctx<'_>, id: &NodeId, def: Option<&NodeDef>, run: Option<&NodeRun>, open: bool, proxy: Option<&str>) -> Header {
    let i = cx.i;
    let about = match (proxy, def) {
        (Some(name), _) => About {
            name: format!("Collapsed group {name}"),
            description: "Expand it (>) to edit its nodes in place, or enter it (pencil) to see only them".into(),
            family: None,
            path: None,
            id: id.0.clone(),
            known: true,
        },
        (None, Some(d)) => About {
            name: d.display.clone(),
            description: d.description.clone(),
            family: i.catalog.families.iter().find(|f| f.id == d.family).map(|f| format!("{}: {}", f.display, f.description)),
            path: (!d.category.is_empty()).then(|| d.category.join(" › ")),
            id: d.id.clone(),
            known: true,
        },
        (None, None) => About {
            name: i.graph.node(id).map(|n| n.kind.clone()).unwrap_or_default(),
            description: "The engine doesn't know this node type".into(),
            family: None,
            path: None,
            id: i.graph.node(id).map(|n| n.kind.clone()).unwrap_or_default(),
            known: false,
        },
    };
    let icon = match (proxy, def) {
        (Some(_), _) => GROUP_ICON.to_owned(),
        (None, Some(d)) if !d.icon.is_empty() => d.icon.clone(),
        _ => FALLBACK_ICON.to_owned(),
    };
    let state = match run.and_then(|r| r.state) {
        Some(s) if run_text::is_running(s) => Spot { kind: "spinner".into(), paint: Some(cx.tok("state_running")), hover: Some("running".into()) },
        Some(s) if let Some(t) = state_token(s) => Spot { kind: "dot".into(), paint: Some(cx.tok(t)), hover: Some(run_text::state_name(s).into()) },
        _ => Spot { kind: "none".into(), paint: None, hover: None },
    };
    let title = match proxy {
        Some(name) => Title { text: format!("Group: {name}"), watch: false },
        None => Title { text: cx.title(id), watch: def.is_some_and(|d| !d.outputs.is_empty()) },
    };
    let badges = if proxy.is_some() { Vec::new() } else { badges(cx, id) };
    let engine = if proxy.is_some() { None } else { def.and_then(|d| engine_mark(cx, id, d)) };
    let ht = || cx.tok("header_text");
    let mut status = Vec::new();
    if proxy.is_none() && run.is_none_or(NodeRun::is_not_run) {
        status.push(Status { text: run_text::NOT_RUN.into(), paint: ht(), strong: false, hover: Some(run_text::NOT_RUN_HOVER.into()) });
    } else if let Some(run) = run {
        if run_text::shows_stale(run) {
            status.push(Status { text: "stale".into(), paint: cx.tok("stale"), strong: false, hover: Some(run_text::stale_tooltip(run)) });
        }
        match (run.state, run.ms) {
            (Some(NodeState::Cached), fetched) => {
                let (short, hover) = run_text::cached_text(run.computed_ms, fetched);
                status.push(Status { text: short, paint: ht(), strong: false, hover: Some(hover) });
            }
            (Some(NodeState::Done | NodeState::Error), Some(ms)) => {
                status.push(Status { text: run_text::format_ms(ms), paint: ht(), strong: false, hover: None })
            }
            (Some(s), _) if run_text::is_running(s) => {
                status.push(Status { text: run_text::running_text(run.fraction), paint: cx.tok("state_running"), strong: true, hover: None });
            }
            (Some(s @ (NodeState::Queued | NodeState::Skipped | NodeState::Cancelled)), _) => {
                status.push(Status { text: run_text::state_name(s).into(), paint: ht(), strong: false, hover: None });
            }
            _ => {}
        }
    }
    Header { open, icon, about, state, title, badges, engine, status, enter: proxy.is_some() }
}

/// Small header badges on a member node: one of its groups' outputs, and its export mark, in the
/// catalogue's words (moved from `plumcraft-ui`'s `groups_view::header_badges`).
pub fn badges_of(graph: &Graph, catalog: &SchemaReply, node: &NodeId) -> Vec<(String, String)> {
    let mut outputs = Vec::new();
    let mut marked = None;
    for g in graph.enclosing_groups(node) {
        if let Some(grp) = graph.group(&g) {
            outputs.extend(grp.outputs.iter().filter(|o| &o.from.node == node).map(|o| format!("{} › {}", grp.name, o.name)));
            if marked.is_none() && grp.checkpoints.contains(node) {
                marked = Some(grp.name.clone());
            }
        }
    }
    let mut out = Vec::new();
    if let Some(group) = marked {
        let word = catalog.text(texts::EXPORT_MARK).filter(|w| !w.is_empty()).unwrap_or("marked");
        let hover = catalog.text(texts::EXPORT_MARK_HOVER).filter(|h| !h.is_empty()).unwrap_or("marked in {group}").replace("{group}", &group);
        out.push((word.to_owned(), hover));
    }
    if !outputs.is_empty() {
        out.push(("output".to_owned(), format!("group output: {}", outputs.join(", "))));
    }
    out
}

fn badges(cx: &Ctx<'_>, id: &NodeId) -> Vec<Badge> {
    badges_of(cx.i.graph, cx.i.catalog, id).into_iter().map(|(text, hover)| Badge { text, paint: cx.tok("accent"), hover }).collect()
}

/// With several engines (a merged catalogue): the engine that runs the node, as the coordinator
/// decides (spec 8.2): the pin, else the first of the workflow's engines that offers it, else the
/// first that does.
fn engine_mark(cx: &Ctx<'_>, id: &NodeId, def: &NodeDef) -> Option<EngineMark> {
    let graph = cx.i.graph;
    let offered = Some(def.engines.clone()).filter(|e| !e.is_empty())?;
    let pin = graph.node(id).and_then(|n| n.engine.clone()).filter(|p| offered.contains(p));
    let engine = pin.clone().or_else(|| graph.engines.iter().find(|e| offered.contains(e)).cloned()).or_else(|| offered.first().cloned())?;
    let pinned = pin.is_some();
    let hover = match (pinned, offered.len()) {
        (true, _) => format!("Runs on {engine} (pinned). These offer it: {}. Change it in the node's menu (Run on)", offered.join(", ")),
        (false, 1) => format!("Runs on {engine}, the only engine that offers it"),
        (false, _) => format!("Runs on {engine}, the first of {} that offers it. Pin another in the node's menu (Run on)", offered.join(", ")),
    };
    Some(EngineMark { text: if pinned { format!("{engine} •") } else { engine }, pinned, hover })
}

fn in_row(cx: &Ctx<'_>, node_id: &NodeId, row: &InputRow) -> InRow {
    let i = cx.i;
    let port = &row.port;
    let at = PortRef::new(node_id.clone(), row.key.clone());
    let tip = port.tooltip.as_deref().filter(|t| !t.is_empty());
    let label = if port.list { format!("{} [ ]", row.label) } else { row.label.clone() };
    let wire = cx.wire_into(&at);
    let graph_node = i.graph.node(node_id);
    let def = graph_node.and_then(|n| cx.def(&n.kind));
    let current = graph_node.zip(def).map_or_else(|| port.default.clone(), |(n, d)| plumcraft_graph::ports::current_value(d, n, &row.key, port));
    let prov = bundle::provenance(i.run, node_id, &row.key);
    let source = bundle::source(i.graph, i.catalog, &at, port, prov);
    let mut r = InRow {
        key: row.key.clone(),
        node: node_id.0.clone(),
        label: label.clone(),
        list: port.list,
        ty: port.ty.clone(),
        pin: cx.pin(&port.ty, port.wireable()),
        source: String::new(),
        text: None,
        hover: tip.map(str::to_owned),
        dim: false,
        from: None,
        named: None,
        badge: None,
        widget: None,
        inactive: None,
        editor: None,
    };
    // the value the node names an editor for, while no wire feeds it (FR1: on a collapsed
    // group's face too, the member's)
    if wire.is_none()
        && let Some(d) = def
        && let Some(named) = d.editor.as_ref().filter(|e| e.port == row.key)
    {
        r.editor = Some(match cx.editor(d) {
            Some(e) => RowEditor { kind: named.kind.clone(), label: e.label.clone(), hover: e.help.clone() },
            None => RowEditor {
                kind: named.kind.clone(),
                label: "Edit Value…".into(),
                hover: format!("Open the value (this app has no {} editor of its own)", named.kind),
            },
        });
    }
    let named = || {
        let name = i.graph.name_of_port(&at)?;
        let paint = cx.type_color(&port.ty).map_or_else(|| cx.tok("wire"), Paint::of);
        let doc = i.graph.named_port(name).map(|n| n.doc.as_str()).filter(|d| !d.is_empty());
        let hover = format!("Named port {name:?}: set from outside with run --set {name}=VALUE{}", doc.map(|d| format!("\n\n{d}")).unwrap_or_default());
        Some(Named { name: name.to_owned(), paint, hover })
    };
    match (wire, port.socket_mode()) {
        (Some(w), Socket::Both | Socket::Widget) => {
            r.source = "wired".into();
            r.text = Some(format!("{label} = {} (wired from {})", value_text(&current), cx.title(&w.from.node)));
            if source == Source::Overridden {
                let word = bundle::bundle_word(i.graph, i.catalog, node_id);
                r.badge = Some(Badge {
                    text: format!("overrides {word}"),
                    paint: cx.tok("accent"),
                    hover: format!("This wire overrides the value in the {word} wired into this node"),
                });
            }
        }
        (_, Socket::Socket) => {
            r.source = "socket".into();
            r.text = Some(typed_label(&label, &port.ty));
            r.hover = Some(typed_tip(&port.ty, tip));
        }
        (None, _) if let Source::Inherited(value) = &source => {
            let word = bundle::bundle_word(i.graph, i.catalog, node_id);
            r.source = "inherited".into();
            r.text = Some(value.as_ref().map_or_else(|| format!("(from {word})"), value_text));
            r.from = Some(format!("from {word}"));
            r.hover = Some(format!(
                "{label} comes from the {word} wired into this node; the widget's own value ({}) is not used. The node's menu › Detach uses its own values.",
                value_text(&current)
            ));
        }
        (None, _) if widget_text::has_editor(port) => {
            // A value nobody changed stays quiet (by the graph, or what the run used), and one
            // somebody did stands out.
            r.source = "own".into();
            r.dim = current == port.default || prov == Some("default");
            if r.dim {
                r.hover = Some(format!("{}{}(the default value)", tip.unwrap_or(""), if tip.is_some() { "\n\n" } else { "" }));
            }
            r.named = named();
            r.widget = Some(widget(port, &current));
            let values = graph_node.map(|n| &n.values);
            r.inactive = widget_text::inactive_note(port, |k| values.and_then(|v| v.get(k).cloned()));
        }
        (None, _) => {
            // A widget the editor doesn't know: its value read-only, or what the application's
            // editor for it says of it.
            let summary = def.and_then(|d| cx.editor(d).filter(|_| d.editor.as_ref().is_some_and(|e| e.port == row.key))).and_then(|e| e.summary);
            let shown = match summary {
                Some(f) => f(&current),
                None => widget_text::value_short(&current),
            };
            r.source = "readonly".into();
            if summary.is_none() && shown != value_text(&current) {
                // summed up on the face: the whole value on hover
                let whole: String = value_text(&current).chars().take(4096).collect();
                r.hover = Some(match tip {
                    Some(t) if !t.is_empty() => format!("{t}\n\n{whole}"),
                    _ => whole,
                });
            }
            r.text = Some(format!("{label}: {shown}"));
            r.named = named();
        }
    }
    // B164: what this input gets from a list joined from several runs, after its own hover
    if wire.is_some()
        && let Some(n) = crate::joined::note(i.graph, i.catalog, i.run, &at)
    {
        r.hover = Some(match r.hover.take() {
            Some(h) if !h.is_empty() => format!("{h}\n\n{n}"),
            _ => n,
        });
    }
    r
}

fn out_row(cx: &Ctx<'_>, node: &NodeId, port: &Port, label: String) -> OutRow {
    let label = typed_label(&label, &port.ty);
    let label = if port.list { format!("[ ] {label}") } else { label };
    OutRow {
        key: port.name.clone(),
        node: node.0.clone(),
        label,
        list: port.list,
        ty: port.ty.clone(),
        pin: cx.pin(&port.ty, true),
        hover: typed_tip(&port.ty, port.tooltip.as_deref()),
    }
}

/// A port's widget as data.
pub fn widget(port: &Port, current: &Value) -> Widget {
    let mut w = Widget {
        kind: String::new(),
        value: current.clone(),
        default: port.default.clone(),
        blank: current.is_null(),
        clearable: port.default.is_null() && !current.is_null(),
        min: port.min,
        max: port.max,
        soft_min: port.soft_min,
        soft_max: port.soft_max,
        step: port.step,
        decimals: None,
        text: None,
        int: false,
        choices: Vec::new(),
        path_mode: None,
        color_format: None,
        hover: port.tooltip.clone().filter(|t| !t.is_empty()),
    };
    let choices = |w: &mut Widget| {
        w.choices = port
            .choices
            .iter()
            .flatten()
            .map(|c| Choice { value: c.clone(), label: widget_text::choice_text(c), disabled: widget_text::disabled_reason(port, c).map(str::to_owned) })
            .collect();
    };
    if port.widget.as_deref() == Some(hints::MULTICHOICE) {
        w.kind = "multichoice".into();
        choices(&mut w);
    } else if port.choices.is_some() || port.ty == scalars::ENUM {
        w.kind = "choice".into();
        choices(&mut w);
    } else if matches!(port.widget.as_deref(), Some(hints::COLOR | hints::HEX_COLOR)) {
        w.kind = "color".into();
        w.color_format = Some(if port.widget.as_deref() == Some(hints::HEX_COLOR) { "hex" } else { "rgb" }.into());
    } else {
        match port.ty.as_str() {
            scalars::FLOAT => {
                w.kind = "number".into();
                let d = widget_text::float_decimals(port, current);
                w.decimals = Some(d);
                w.text = Some(widget_text::number_text(current, false, d));
            }
            scalars::INT => {
                w.kind = "number".into();
                w.int = true;
                w.text = Some(widget_text::number_text(current, true, 0));
            }
            scalars::BOOL => w.kind = "toggle".into(),
            _ if port.widget.as_deref() == Some(hints::MULTILINE) => w.kind = "multiline".into(),
            _ if port.path.is_some() => {
                w.kind = "path".into();
                w.path_mode = port.path.as_ref().map(|p| if p.mode.is_empty() { "open".to_owned() } else { p.mode.clone() });
            }
            _ => w.kind = "text".into(),
        }
    }
    w
}

/// A Convert node's path (docs/research/20-convert.md): its types and steps, or what it still
/// needs.
fn convert_lines(cx: &Ctx<'_>, id: &NodeId) -> Vec<Line> {
    use plumcraft_proto::convert;
    let dim = || cx.tok("text_dim");
    match cx.i.graph.convert_path(cx.i.catalog, id) {
        Some(p) if p.is_empty() => vec![Line { text: "the types meet: it passes the value on".into(), paint: dim(), hover: None }],
        Some(p) => {
            let mut lines = vec![
                Line { text: convert::types(&p), paint: dim(), hover: Some(format!("{}\nnode.convert picks again", convert::label(&p))) },
                Line { text: convert::label(&p), paint: dim(), hover: None },
            ];
            if !convert::lossless(&p) {
                lines.push(Line { text: "a step isn't lossless".into(), paint: cx.tok("warning"), hover: None });
            }
            lines
        }
        None => vec![Line { text: "wire both ends".into(), paint: dim(), hover: None }],
    }
}

fn run_footer(cx: &Ctx<'_>, id: &NodeId, def: Option<&NodeDef>, n: &NodeRun, full: bool, out: &mut Vec<Footer>) {
    if n.state == Some(NodeState::Progress) {
        let f = n.fraction.filter(|f| f.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0);
        out.push(Footer::Progress { fraction: f, stage: n.stage.clone() });
    }
    if let Some(e) = &n.error {
        // A skipped node isn't at fault: say why quietly; real errors in red.
        let (full, token) = match e.kind.as_str() {
            "skipped" => (format!("skipped: {}", e.message), "text_dim"),
            "" => (e.message.clone(), "error"),
            kind => (format!("{kind} error: {}", e.message), "error"),
        };
        let (short, _) = run_text::truncate_chars(&full, run_text::ERROR_CHARS);
        out.push(Footer::Error { text: short, full, paint: cx.tok(token) });
    }
    // A result that has run: save it; a list of them also as one contact sheet.
    if matches!(n.state, Some(NodeState::Done | NodeState::Cached))
        && let Some(d) = def.filter(|d| d.flags.result)
        && let Some(port) = d.outputs.first().map(|o| o.name.clone())
        // something its engine saves takes this value (spec 7.3 save_formats)
        && !cx.i.catalog.save_formats_for(d, &port).is_empty()
    {
        let items = match &n.thumb {
            Some(Rendition::Rgba8 { items, .. }) => *items,
            _ => 0,
        };
        out.push(Footer::Save { port, sheet: items > 1 });
    }
    if let Some(Rendition::Excerpt { text, .. }) = n.thumb.as_ref().filter(|_| full)
        && !text.is_empty()
    {
        let hover = match thumb_note(n) {
            Some(note) => format!("{note}\nClick to watch"),
            None => "Click to watch".to_owned(),
        };
        out.push(Footer::Excerpt { text: run_text::excerpt_lines(text), faded: cx.faded_now(id, n), hover });
    }
    if let Some(text) = n.text.as_deref().filter(|t| !t.is_empty()) {
        let (short, _) = run_text::truncate_chars(text, NODE_TEXT_CHARS);
        out.push(Footer::EngineText { text: short, full: text.to_owned() });
    }
}

fn preview(cx: &Ctx<'_>, id: &NodeId, n: &NodeRun) -> Option<Preview> {
    let handle = |d: &plumcraft_proto::data::DataRef| serde_json::to_value(d).unwrap_or(Value::Null);
    match n.thumb.as_ref()? {
        Rendition::Rgba8 { data, w, h, summary, caption, items, levels, .. } => Some(Preview::Rgba8 {
            handle: handle(data),
            w: *w,
            h: *h,
            size: thumb_size(*w as f32, *h as f32, THUMB_PX),
            hover_size: thumb_size(*w as f32, *h as f32, THUMB_HOVER_PX),
            items: *items,
            levels: levels.clone(),
            summary: if summary.is_empty() { caption.clone() } else { summary.clone() },
            caption: caption.clone(),
            note: thumb_note(n),
            faded: cx.faded_now(id, n),
        }),
        Rendition::Peaks { data, channels, .. } => {
            Some(Preview::Peaks { handle: handle(data), channels: *channels, size: PEAKS_SIZE, note: thumb_note(n), faded: cx.faded_now(id, n) })
        }
        _ => None,
    }
}

/// The wires as drawn between faces (each with its path between its pins, `rects` the faces'),
/// and the nets drawn through labels.
fn wires_and_nets(cx: &Ctx<'_>, faces: &[Face], rects: &HashMap<&str, crate::plain::Rect>) -> (Vec<Wire>, Vec<Net>) {
    let i = cx.i;
    let graph = i.graph;
    let by_id: HashMap<&str, &Face> = faces.iter().map(|f| (f.id.as_str(), f)).collect();
    let mut wires = Vec::new();
    let mut nets: Vec<Net> = Vec::new();
    for w in &graph.wires {
        if cx.vis.is_outside(&w.from.node) || cx.vis.is_outside(&w.to.node) {
            continue;
        }
        let from_face = cx.vis.proxy_of(&w.from.node).map_or_else(|| w.from.node.0.clone(), |g| rows::proxy_node_id(g).0);
        let to_face = cx.vis.proxy_of(&w.to.node).map_or_else(|| w.to.node.0.clone(), |g| rows::proxy_node_id(g).0);
        if from_face == to_face {
            continue;
        }
        let (Some(ff), Some(tf)) = (by_id.get(from_face.as_str()), by_id.get(to_face.as_str())) else { continue };
        let Some(from_pin) = ff.outputs.iter().position(|o| o.node == w.from.node.0 && o.key == w.from.port) else { continue };
        let Some(to_pin) = tf.inputs.iter().position(|r| r.node == w.to.node.0 && r.key == w.to.port) else { continue };
        let paint = graph
            .node(&w.from.node)
            .and_then(|n| cx.def(&n.kind))
            .and_then(|d| d.output(&w.from.port))
            .and_then(|p| cx.type_color(&p.ty))
            .map_or_else(|| cx.tok("wire"), Paint::of);
        // a label means nothing outside its group: an end in a collapsed group is a wire
        let shown = cx.vis.proxy_of(&w.from.node).is_none() && cx.vis.proxy_of(&w.to.node).is_none();
        let net = graph.net_of_wire(w).filter(|_| shown);
        let kind = if net.is_some() {
            "net"
        } else if w.via.is_empty() {
            "plain"
        } else {
            "routed"
        };
        if let Some(key) = &net {
            let at = w.label_at.unwrap_or(DEFAULT_USE_AT);
            let use_ = Use { to: w.to.clone(), face: to_face.clone(), pin: to_pin, at };
            match nets.iter_mut().find(|n| &n.key == key) {
                Some(n) => n.uses.push(use_),
                None => nets.push(Net {
                    id: net_id(key),
                    nid: 0,
                    rev: 0,
                    key: key.clone(),
                    paint: paint.clone(),
                    selected: i.selection.net.as_ref() == Some(key),
                    driver: Driver {
                        from: w.from.clone(),
                        face: from_face.clone(),
                        pin: from_pin,
                        at: graph.net(key).and_then(|n| n.at).unwrap_or([NET_STUB, 0]),
                        text: String::new(),
                    },
                    uses: vec![use_],
                }),
            }
        }
        let path = match (rects.get(from_face.as_str()), rects.get(to_face.as_str())) {
            (Some(a), Some(b)) => {
                let from = crate::geometry::pin_point(*a, cx.measured(&from_face), true, from_pin);
                let to = crate::geometry::pin_point(*b, cx.measured(&to_face), false, to_pin);
                crate::geometry::chain_json(&crate::geometry::wire_chain(kind, from, to, &w.via, Some((*a, *b))))
            }
            _ => Vec::new(),
        };
        wires.push(Wire {
            id: format!("{}.{}", w.to.node, w.to.port),
            nid: 0,
            rev: 0,
            from: w.from.clone(),
            to: w.to.clone(),
            from_face,
            from_pin,
            to_face,
            to_pin,
            paint,
            kind: kind.into(),
            via: w.via.clone(),
            slice: w.slice.as_ref().map(|s| format!("[{s}]")),
            net,
            path,
        });
    }
    for n in &mut nets {
        n.driver.text = format!("{} ×{}", n.key.name, n.uses.len());
    }
    (wires, nets)
}

/// A net tag's stub, graph units (its default offset from its pin): the session's one rule, the
/// same on both sides (B146).
pub const NET_STUB: i32 = plumcraft_session::layout_cmds::NET_TAG_STUB;
/// A receiving tag's offset when it hasn't been moved.
pub const DEFAULT_USE_AT: [i32; 2] = [-NET_STUB, 0];

/// A net's id in the scene: `name`, or `name@group`.
pub fn net_id(key: &NetKey) -> String {
    match &key.group {
        Some(g) => format!("{}@{g}", key.name),
        None => key.name.clone(),
    }
}

// ---------- revisions ----------

/// What the scene's items have in common: an id, and the interned id and revision the published
/// scene gives them.
trait Item: Serialize {
    fn key(&self) -> &str;
    fn stamp(&mut self, nid: u64, rev: u64);
}

macro_rules! item {
    ($($t:ty),*) => {$(
        impl Item for $t {
            fn key(&self) -> &str {
                &self.id
            }
            fn stamp(&mut self, nid: u64, rev: u64) {
                self.nid = nid;
                self.rev = rev;
            }
        }
    )*};
}
item!(Face, Wire, Net, Frame, Group, Mark);

const KINDS: [&str; 6] = ["nodes", "wires", "nets", "frames", "groups", "marks"];

/// One published item: as built (stamped with its nid and revision 0, to compare the next build
/// with), its revision, and its JSON (what `scene.get` sends).
struct Entry<T> {
    item: T,
    rev: u64,
    /// The publish that last saw it ([`Published::round`]: to find what is gone, and repeats).
    seen: u64,
    value: Value,
}

/// The parts of the scene that come with every reply.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
struct Top {
    selection: Picked,
    focus: Option<Focus>,
    anim: Anim,
    view: View,
}

/// The last scene published, with a revision per item (section 8.12).
#[derive(Default)]
pub struct Published {
    generation: u64,
    /// Interned ids, a table a kind.
    nids: [HashMap<String, u64>; 6],
    next_nid: u64,
    /// Publishes so far, changed or not.
    round: u64,
    nodes: BTreeMap<String, Entry<Face>>,
    wires: BTreeMap<String, Entry<Wire>>,
    nets: BTreeMap<String, Entry<Net>>,
    frames: BTreeMap<String, Entry<Frame>>,
    groups: BTreeMap<String, Entry<Group>>,
    marks: BTreeMap<String, Entry<Mark>>,
    /// Ids removed, with the generation that removed them (the last [`KEEP_GENERATIONS`]).
    removed: VecDeque<(u64, usize, String)>,
    /// The oldest generation a delta can start from.
    floor: u64,
    top: Option<Top>,
    top_value: Value,
    doc: u64,
    /// A shell asked for the scene, so the feed says when it changes.
    pub watched: bool,
    /// Published since [`Published::take_fresh`] last looked.
    fresh: bool,
}

impl std::fmt::Debug for Published {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Published(gen {}, {} faces)", self.generation, self.nodes.len())
    }
}

/// Note one kind's items (`kind` its index in [`KINDS`]); `next` is the generation they change
/// at, `round` the publish's number. Any change? An item is compared with what was published as built (N2 slice 3: comparing
/// the typed items replaced hashing each one's JSON, which was most of a publish's cost).
fn note<T: Item + PartialEq>(
    nids: &mut HashMap<String, u64>,
    next_nid: &mut u64,
    map: &mut BTreeMap<String, Entry<T>>,
    removed: &mut VecDeque<(u64, usize, String)>,
    kind: usize,
    items: Vec<T>,
    (next, round): (u64, u64),
) -> bool {
    let mut changed = false;
    for mut item in items {
        let nid = match nids.get(item.key()) {
            Some(n) => *n,
            None => {
                *next_nid = next_nid.saturating_add(1);
                nids.insert(item.key().to_owned(), *next_nid);
                *next_nid
            }
        };
        item.stamp(nid, 0);
        match map.get_mut(item.key()) {
            // a repeated id: the first wins
            Some(e) if e.seen == round => continue,
            Some(e) if e.item == item => {
                e.seen = round;
                continue;
            }
            _ => {}
        }
        item.stamp(nid, next);
        let value = serde_json::to_value(&item).unwrap_or(Value::Null);
        item.stamp(nid, 0);
        map.insert(item.key().to_owned(), Entry { item, rev: next, seen: round, value });
        changed = true;
    }
    let gone: Vec<String> = map.iter().filter(|(_, e)| e.seen != round).map(|(k, _)| k.clone()).collect();
    for id in gone {
        map.remove(&id);
        removed.push_back((next, kind, id));
        changed = true;
    }
    changed
}

/// One kind's items as JSON: every one, or those changed after `since`.
fn values<T>(map: &BTreeMap<String, Entry<T>>, since: Option<u64>) -> Value {
    Value::Array(map.values().filter(|e| since.is_none_or(|s| e.rev > s)).map(|e| e.value.clone()).collect())
}

impl Published {
    /// The generation last published (0: none yet).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Was the scene published since this was last asked (and forget that it was)? The C ABI's
    /// core rebuilds no scene a request has just published (`scene.get`) in the same turn.
    pub fn take_fresh(&mut self) -> bool {
        std::mem::take(&mut self.fresh)
    }

    /// Note `scene` as the current one: items whose content changed get the new generation as
    /// their revision; the generation rises only when something changed. Returns it.
    pub fn publish(&mut self, scene: Scene) -> u64 {
        let Scene { doc, nodes, wires, nets, frames, groups, marks, selection, focus, anim, view, .. } = scene;
        self.doc = doc;
        self.fresh = true;
        let next = self.generation.saturating_add(1);
        self.round = self.round.wrapping_add(1);
        let at = (next, self.round);
        let [n0, n1, n2, n3, n4, n5] = &mut self.nids;
        let (nn, rm) = (&mut self.next_nid, &mut self.removed);
        let mut changed = note(n0, nn, &mut self.nodes, rm, 0, nodes, at);
        changed |= note(n1, nn, &mut self.wires, rm, 1, wires, at);
        changed |= note(n2, nn, &mut self.nets, rm, 2, nets, at);
        changed |= note(n3, nn, &mut self.frames, rm, 3, frames, at);
        changed |= note(n4, nn, &mut self.groups, rm, 4, groups, at);
        changed |= note(n5, nn, &mut self.marks, rm, 5, marks, at);
        let top = Top { selection, focus, anim, view };
        if self.top.as_ref() != Some(&top) {
            self.top_value = serde_json::to_value(&top).unwrap_or(Value::Null);
            self.top = Some(top);
            changed = true;
        }
        if changed {
            self.generation = next;
            self.floor = self.floor.max(self.generation.saturating_sub(KEEP_GENERATIONS));
            let floor = self.floor;
            self.removed.retain(|(g, ..)| *g > floor);
        }
        self.generation
    }

    fn kind_values(&self, k: usize, since: Option<u64>) -> Value {
        match k {
            0 => values(&self.nodes, since),
            1 => values(&self.wires, since),
            2 => values(&self.nets, since),
            3 => values(&self.frames, since),
            4 => values(&self.groups, since),
            _ => values(&self.marks, since),
        }
    }

    fn has(&self, k: usize, id: &str) -> bool {
        match k {
            0 => self.nodes.contains_key(id),
            1 => self.wires.contains_key(id),
            2 => self.nets.contains_key(id),
            3 => self.frames.contains_key(id),
            4 => self.groups.contains_key(id),
            _ => self.marks.contains_key(id),
        }
    }

    /// `scene.get {since}`: everything (`full: true`) with no `since`, a `since` older than the
    /// removals kept or newer than the scene; else what changed after `since` (`upsert`, `remove`).
    /// The selection, the group entered and the animation's switches come every time.
    pub fn get(&self, since: Option<u64>) -> Value {
        let full = match since {
            None => true,
            Some(s) => s < self.floor || s > self.generation || self.generation == 0,
        };
        let mut out = json!({"shell": SHELL, "doc": self.doc, "gen": self.generation, "full": full});
        if let (Value::Object(o), Value::Object(top)) = (&mut out, &self.top_value) {
            o.extend(top.clone());
        }
        if full {
            for (k, name) in KINDS.iter().enumerate() {
                out[*name] = self.kind_values(k, None);
            }
            return out;
        }
        let since = since.unwrap_or(0);
        out["since"] = json!(since);
        let mut upsert = serde_json::Map::new();
        let mut remove = serde_json::Map::new();
        for (k, name) in KINDS.iter().enumerate() {
            upsert.insert((*name).to_owned(), self.kind_values(k, Some(since)));
            let mut gone: Vec<&str> =
                self.removed.iter().filter(|(g, kind, id)| *g > since && *kind == k && !self.has(k, id)).map(|(_, _, id)| id.as_str()).collect();
            gone.sort_unstable();
            gone.dedup();
            remove.insert((*name).to_owned(), json!(gone));
        }
        out["upsert"] = Value::Object(upsert);
        out["remove"] = Value::Object(remove);
        out
    }

    /// The faces published, by id (what a shell redraws from).
    pub fn face(&self, id: &str) -> Option<&Value> {
        self.nodes.get(id).map(|e| &e.value)
    }

    /// How many faces are published.
    pub fn face_count(&self) -> usize {
        self.nodes.len()
    }

    /// The revision face `id` was published at.
    pub fn face_rev(&self, id: &str) -> Option<u64> {
        self.nodes.get(id).map(|e| e.rev)
    }

    /// The published faces' ids by nid (a typed `geometry.set` names faces by nid).
    pub fn face_ids(&self) -> HashMap<u64, &str> {
        self.nodes.values().map(|e| (e.item.nid, e.item.id.as_str())).collect()
    }

    /// The hot calls' snapshot of what is published, with the geometry the core has (N2 slice 3).
    pub fn snapshot(
        &self,
        key: (u64, u64, u64, Option<String>),
        graph: &Graph,
        catalog: &SchemaReply,
        measured: &crate::geometry::Measured,
    ) -> crate::geometry::Snapshot {
        crate::geometry::Snapshot::build(crate::geometry::SnapshotInputs {
            doc: self.doc,
            generation: self.generation,
            key,
            faces: self.nodes.values().map(|e| &e.item).collect(),
            wires: self.wires.values().map(|e| &e.item).collect(),
            nets: self.nets.values().map(|e| &e.item).collect(),
            frames: self.frames.values().map(|e| &e.item).collect(),
            groups: self.groups.values().map(|e| &e.item).collect(),
            measured,
            graph,
            catalog,
        })
    }
}

/// What `scene.get {since}` returns when it isn't everything (section 8.12): the items whose
/// revision is past `since`, the ids gone since, and the parts that come every time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Delta {
    pub shell: u32,
    pub doc: u64,
    #[serde(rename = "gen")]
    pub generation: u64,
    pub full: bool,
    pub since: u64,
    pub upsert: Upsert,
    pub remove: Removed,
    pub selection: Picked,
    pub focus: Option<Focus>,
    pub anim: Anim,
    #[serde(default)]
    pub view: View,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Upsert {
    pub nodes: Vec<Face>,
    pub wires: Vec<Wire>,
    pub nets: Vec<Net>,
    pub frames: Vec<Frame>,
    pub groups: Vec<Group>,
    pub marks: Vec<Mark>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Removed {
    pub nodes: Vec<String>,
    pub wires: Vec<String>,
    pub nets: Vec<String>,
    pub frames: Vec<String>,
    pub groups: Vec<String>,
    pub marks: Vec<String>,
}

/// `scene.get`'s params: `{since?: gen}`.
pub fn parse_since(params: &Value) -> Result<Option<u64>, String> {
    crate::control::check_fields(params, &["since"])?;
    match params.get("since") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| "since must be a scene generation (a whole number, scene.get's gen)".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use plumcraft_session::Session;

    use super::*;

    static TOKENS: std::sync::LazyLock<Tokens> = std::sync::LazyLock::new(|| crate::theme::DEFAULT);

    fn dial(v: &Value) -> String {
        format!("{} points", v["pts"].as_array().map_or(0, Vec::len))
    }

    static EDITORS: std::sync::LazyLock<Vec<Editor>> = std::sync::LazyLock::new(|| {
        vec![Editor { kind: "test.dial".into(), label: "Edit Dial…".into(), help: "Edit the dial in its window".into(), summary: Some(dial) }]
    });

    /// A catalogue with every row and widget kind: each scalar, choices (one disabled here),
    /// multichoice, multiline, a path, both colours, a socket-only input, lists, a variadic port,
    /// a choice with sub-ports, an engine-coloured wire type, a family, a result to save.
    pub(super) fn catalog() -> SchemaReply {
        serde_json::from_value(json!({
            "wire_types": [{"id": "x.t.pic", "color": "#cc4466"}, {"id": "TEXT", "color": "#d4a020"}],
            "families": [{"id": "fx", "display": "Effects", "description": "Change a picture"}],
            "save_formats": [{"id": "png", "label": "PNG", "extensions": ["png"]}],
            "nodes": [
                {"id": "t.all", "display": "Every widget", "family": "fx", "icon": "sliders", "category": ["Test", "Widgets"],
                 "editor": {"kind": "test.dial", "port": "curve"},
                 "description": "One input of each widget kind",
                 "inputs": [
                    {"name": "gain", "type": "FLOAT", "default": 0.375, "min": 0, "max": 4, "soft_min": 0, "soft_max": 2, "tooltip": "How loud"},
                    {"name": "count", "type": "INT", "default": 3, "min": 1, "step": 1},
                    {"name": "on", "type": "BOOL", "default": true},
                    {"name": "name", "type": "STRING", "default": ""},
                    {"name": "mode", "type": "ENUM", "default": "fast", "choices": ["fast", "good", "gpu"], "disabled_choices": {"gpu": "no GPU here"}},
                    {"name": "tags", "type": "STRING", "default": "", "widget": "multichoice", "choices": ["a", "b"]},
                    {"name": "notes", "type": "STRING", "default": "", "widget": "multiline"},
                    {"name": "file", "type": "STRING", "default": "", "path": {"mode": "open", "filters": []}},
                    {"name": "tint", "type": "STRING", "default": "1,0,0", "widget": "color"},
                    {"name": "hex", "type": "STRING", "default": "#ff0000", "widget": "hexcolor"},
                    {"name": "blank", "type": "FLOAT", "default": null},
                    {"name": "pic", "type": "x.t.pic", "socket": "socket"},
                    {"name": "pics", "type": "x.t.pic", "socket": "socket", "list": true},
                    {"name": "curve", "type": "x.t.curve", "socket": "widget", "default": {"pts": [0, 1]}},
                    {"name": "radius", "type": "FLOAT", "default": 2.0, "active_when": {"mode": ["good"]}}
                 ],
                 "outputs": [{"name": "out", "type": "x.t.pic"}, {"name": "outs", "type": "x.t.pic", "list": true}]},
                {"id": "t.src", "display": "Source", "flags": {"result": true}, "inputs": [],
                 "outputs": [{"name": "pic", "type": "x.t.pic"}, {"name": "text", "type": "TEXT"}, {"name": "pics", "type": "x.t.pic", "list": true}]},
                {"id": "t.join", "display": "Join", "inputs": [
                    {"name": "c", "type": "TEXT", "socket": "socket", "variadic": {"min": 2, "max": 4, "template": "c{n}"}},
                    {"name": "how", "type": "ENUM", "default": "line", "choices": ["line", "space"],
                     "options": {"line": [{"name": "width", "type": "INT", "default": 80}]}}],
                 "outputs": [{"name": "text", "type": "TEXT"}]},
                {"id": "t.ink", "display": "Ink", "flags": {"bundle_port": "tuning"},
                 "inputs": [{"name": "tuning", "type": "x.t.tune"},
                            {"name": "ink.width", "type": "FLOAT", "default": 1.0, "param_ref": {"group": "ink", "name": "width"}}],
                 "outputs": [{"name": "tuning", "type": "x.t.tune"}, {"name": "level", "type": "FLOAT"}]},
                {"id": "t.sink", "display": "Sink", "inputs": [{"name": "text", "type": "TEXT", "socket": "both", "default": "x"}], "outputs": []}
            ]
        }))
        .unwrap()
    }

    fn add(s: &mut Session, kind: &str, pos: [f32; 2]) -> NodeId {
        NodeId(s.execute("node.add", &json!({"kind": kind, "pos": pos})).unwrap()["id"].as_str().unwrap().to_owned())
    }

    fn inputs<'a>(s: &'a Session, sel: Selection, focus: Option<&'a GroupId>, texts: &'a HashMap<String, String>) -> Inputs<'a> {
        Inputs {
            doc: 1,
            graph: s.graph(),
            catalog: s.catalog(),
            run: &s.run,
            selection: sel,
            focus,
            watched: None,
            tokens: &TOKENS,
            anim: None,
            now: Instant::now(),
            app_text: texts,
            editors: &EDITORS,
            view: View::default(),
            geometry: None,
        }
    }

    fn run(state: Option<NodeState>) -> NodeRun {
        NodeRun { state, generation: 1, ..NodeRun::default() }
    }

    /// A graph with every row and widget kind, wires plain, routed, sliced and through a net, a
    /// frame, a collapsed group, and a node in each run state.
    pub(super) fn rich_session() -> Session {
        let mut s = Session::new(catalog());
        let all = add(&mut s, "t.all", [0.0, 0.0]);
        let src = add(&mut s, "t.src", [-300.0, 0.0]);
        let join = add(&mut s, "t.join", [300.0, 0.0]);
        let sink = add(&mut s, "t.sink", [600.0, 0.0]);
        let sink2 = add(&mut s, "t.sink", [600.0, 200.0]);
        s.execute("wire.connect", &json!({"from": {"node": src, "port": "pic"}, "to": {"node": all, "port": "pic"}})).unwrap();
        s.execute("wire.connect", &json!({"from": {"node": src, "port": "text"}, "to": {"node": join, "port": "c1"}})).unwrap();
        s.execute("wire.connect", &json!({"from": {"node": join, "port": "text"}, "to": {"node": sink, "port": "text"}})).unwrap();
        s.execute("wire.connect", &json!({"from": {"node": src, "port": "text"}, "to": {"node": sink2, "port": "text"}})).unwrap();
        s.execute("wire.route", &json!({"to": {"node": sink, "port": "text"}, "via": [[450, 60]]})).unwrap();
        s.execute("wire.label", &json!({"to": {"node": sink2, "port": "text"}, "name": "caption"})).unwrap();
        s.execute("wire.connect", &json!({"from": {"node": src, "port": "pics"}, "to": {"node": all, "port": "pics"}})).unwrap();
        s.execute("wire.slice", &json!({"to": {"node": all, "port": "pics"}, "slice": ":-1"})).unwrap();
        s.execute("node.set", &json!({"node": all, "port": "gain", "value": 1.25})).unwrap();
        s.execute("frame.add", &json!({"title": "Inputs", "pos": [-320, -40], "size": [260, 200]})).unwrap();
        // one of each run state, on nodes of their own
        let states: [(&str, Option<NodeState>); 9] = [
            ("queued", Some(NodeState::Queued)),
            ("started", Some(NodeState::Started)),
            ("progress", Some(NodeState::Progress)),
            ("preview", Some(NodeState::Preview)),
            ("done", Some(NodeState::Done)),
            ("cached", Some(NodeState::Cached)),
            ("error", Some(NodeState::Error)),
            ("skipped", Some(NodeState::Skipped)),
            ("cancelled", Some(NodeState::Cancelled)),
        ];
        for (k, (name, state)) in states.into_iter().enumerate() {
            let n = add(&mut s, "t.src", [0.0, 300.0 + 120.0 * k as f32]);
            s.execute("node.title", &json!({"node": n, "title": name})).unwrap();
            let mut r = run(state);
            match state {
                Some(NodeState::Progress) => {
                    r.fraction = Some(0.4);
                    r.stage = Some("blurring".into());
                }
                Some(NodeState::Done) => {
                    r.ms = Some(12.5);
                    r.completed = true;
                    r.thumb = Some(serde_json::from_value(json!({"kind": "rgba8", "ref": {"handle": "b3:aa", "meta": {"media": "image/x-rgba8"}, "at": []}, "w": 640, "h": 480, "summary": "picture 640×480", "caption": "first", "items": 3, "levels": [3, 2]})).unwrap());
                    r.text = Some("3 faces found".into());
                }
                Some(NodeState::Cached) => {
                    r.ms = Some(0.2);
                    r.computed_ms = Some(40.0);
                    r.completed = true;
                    r.thumb = Some(serde_json::from_value(json!({"kind": "excerpt", "text": "one\ntwo\nthree\nfour", "lines": 4})).unwrap());
                }
                Some(NodeState::Error) => {
                    r.ms = Some(3.0);
                    r.error = Some(serde_json::from_value(json!({"kind": "value", "message": "the picture is empty"})).unwrap());
                }
                Some(NodeState::Skipped) => r.error = Some(serde_json::from_value(json!({"kind": "skipped", "message": "an input failed"})).unwrap()),
                _ => {}
            }
            s.run.nodes.insert(n.0.clone(), r);
        }
        let stale = add(&mut s, "t.src", [300.0, 300.0]);
        s.execute("node.title", &json!({"node": stale, "title": "stale"})).unwrap();
        s.run.nodes.insert(
            stale.0,
            NodeRun { stale_since: Some(1), stale_why: vec!["gain changed".into()], completed: true, ms: Some(5.0), ..run(Some(NodeState::Done)) },
        );
        // a bundle: a value inherited, and one a wire overrides
        let up = add(&mut s, "t.ink", [0.0, 1600.0]);
        let down = add(&mut s, "t.ink", [300.0, 1600.0]);
        let over = add(&mut s, "t.ink", [300.0, 1800.0]);
        s.execute("node.set", &json!({"node": up, "port": "ink.width", "value": 5.0})).unwrap();
        for to in [&down, &over] {
            s.execute("wire.connect", &json!({"from": {"node": up, "port": "tuning"}, "to": {"node": to, "port": "tuning"}})).unwrap();
        }
        s.execute("wire.connect", &json!({"from": {"node": up, "port": "level"}, "to": {"node": over, "port": "ink.width"}})).unwrap();
        // a Convert node, not yet wired
        s.execute("node.add", &json!({"kind": "core.convert", "pos": [0.0, 2000.0]})).unwrap();
        // a collapsed group of two
        let a = add(&mut s, "t.src", [900.0, 0.0]);
        let b = add(&mut s, "t.join", [1100.0, 0.0]);
        s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "c1"}})).unwrap();
        s.execute("wire.connect", &json!({"from": {"node": src, "port": "text"}, "to": {"node": b, "port": "c2"}})).unwrap();
        s.execute("group.create", &json!({"nodes": [a, b], "name": "pair", "collapsed": true})).unwrap();
        s
    }

    fn face<'s>(sc: &'s Scene, title: &str) -> &'s Face {
        sc.nodes.iter().find(|f| f.header.title.text == title).unwrap_or_else(|| panic!("no face {title}"))
    }

    #[test]
    fn every_row_and_widget_kind_is_in_the_scene() {
        let s = rich_session();
        let t = HashMap::new();
        let sc = build(&inputs(&s, Selection::default(), None, &t));
        let all = face(&sc, "Every widget");
        let kinds: Vec<&str> = all.inputs.iter().filter_map(|r| r.widget.as_ref()).map(|w| w.kind.as_str()).collect();
        for k in ["number", "toggle", "choice", "multichoice", "text", "multiline", "path", "color"] {
            assert!(kinds.contains(&k), "{k} in {kinds:?}");
        }
        let row = |k: &str| all.inputs.iter().find(|r| r.key == k).unwrap();
        assert_eq!(row("pic").source, "socket", "a socket-only input, wired, stays a socket row");
        assert_eq!(row("pics").label, "pics [ ]");
        assert_eq!(row("pics").text.as_deref(), Some("pics [ ] · pic"));
        assert_eq!(row("pics").source, "socket");
        assert_eq!(row("pic").pin.shape, "square");
        assert_eq!(row("pic").pin.fill.color, Some([0xcc, 0x44, 0x66, 255]), "the engine's colour");
        assert_eq!((row("gain").pin.fill.token.as_deref(), row("gain").pin.fill.color), (Some("pin_default"), None), "a token is sent by name alone");
        assert_eq!(row("gain").pin.shape, "circle");
        assert!(!row("gain").dim, "changed from the default");
        assert!(row("count").dim);
        let gain = row("gain").widget.as_ref().unwrap();
        assert_eq!((gain.value.clone(), gain.decimals, gain.soft_max), (json!(1.25), Some(3), Some(2.0)));
        let mode = row("mode").widget.as_ref().unwrap();
        assert_eq!(mode.choices.iter().map(|c| c.disabled.is_some()).collect::<Vec<_>>(), [false, false, true]);
        assert!(row("blank").widget.as_ref().unwrap().blank);
        assert_eq!(all.outputs.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["out · pic", "[ ] outs · pic"]);
        assert_eq!(all.header.about.family.as_deref(), Some("Effects: Change a picture"));
        assert_eq!(all.header.about.path.as_deref(), Some("Test › Widgets"));
        assert_eq!(all.header.icon, "sliders");
        // a variadic port's slots, a choice's sub-port, a wired widget, a net, a slice, a route
        let join = sc.nodes.iter().find(|f| f.kind == "t.join").unwrap();
        assert_eq!(join.inputs.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["c1", "c2", "how", "how.width"]);
        let sink = sc.nodes.iter().find(|f| f.kind == "t.sink" && f.inputs[0].text.as_deref().is_some_and(|t| t.contains("wired from Join"))).unwrap();
        assert_eq!(sink.inputs[0].source, "wired");
        let kinds: Vec<&str> = sc.wires.iter().map(|w| w.kind.as_str()).collect();
        for k in ["plain", "routed", "net"] {
            assert!(kinds.contains(&k), "{k} in {kinds:?}");
        }
        assert!(sc.wires.iter().any(|w| w.slice.as_deref() == Some("[:-1]")));
        assert_eq!(sc.nets.len(), 1);
        assert_eq!(sc.nets[0].key.name, "caption");
        assert_eq!(sc.frames.len(), 1);
        // the collapsed group is one face, its wires drawn to it
        let proxy = sc.nodes.iter().find(|f| f.proxy.is_some()).unwrap();
        assert_eq!(proxy.header.title.text, "Group: pair");
        assert!(proxy.header.enter);
        assert!(sc.wires.iter().all(|w| sc.nodes.iter().any(|f| f.id == w.from_face) && sc.nodes.iter().any(|f| f.id == w.to_face)));
    }

    #[test]
    fn every_run_state_shows_as_the_canvas_draws_it() {
        let s = rich_session();
        let t = HashMap::new();
        let sc = build(&inputs(&s, Selection::default(), None, &t));
        let status = |f: &Face| f.header.status.iter().map(|s| s.text.clone()).collect::<Vec<_>>();
        assert_eq!(face(&sc, "queued").header.state.kind, "dot");
        assert_eq!(status(face(&sc, "queued")), ["queued"]);
        assert_eq!(face(&sc, "started").header.state.kind, "spinner");
        assert_eq!(face(&sc, "progress").border.as_ref().unwrap().width, 2.5);
        assert!(face(&sc, "progress").footer.iter().any(|f| matches!(f, Footer::Progress { fraction, .. } if (*fraction - 0.4).abs() < 1e-9)));
        let done = face(&sc, "done");
        assert_eq!(status(done), [run_text::format_ms(12.5)]);
        assert!(matches!(&done.preview, Some(Preview::Rgba8 { size, items: 3, caption, .. }) if *size == [80.0, 60.0] && caption == "first"));
        // a list of lists: each level's count, for the face's "3 × 2" (spec 7.6 rule 2)
        assert!(matches!(&done.preview, Some(Preview::Rgba8 { levels, .. }) if levels == &[3, 2]));
        assert!(done.footer.iter().any(|f| matches!(f, Footer::Save { sheet: true, .. })), "a result with items saves as a sheet too");
        assert!(done.footer.iter().any(|f| matches!(f, Footer::EngineText { .. })));
        assert!(face(&sc, "cached").footer.iter().any(|f| matches!(f, Footer::Excerpt { .. })));
        assert!(face(&sc, "error").footer.iter().any(|f| matches!(f, Footer::Error { paint, .. } if paint.token.as_deref() == Some("error"))));
        assert!(face(&sc, "skipped").footer.iter().any(|f| matches!(f, Footer::Error { paint, .. } if paint.token.as_deref() == Some("text_dim"))));
        assert_eq!(status(face(&sc, "cancelled")), ["cancelled"]);
        let stale = face(&sc, "stale");
        assert!(stale.run.stale);
        assert_eq!(stale.border.as_ref().unwrap().paint.token.as_deref(), Some("stale"));
        assert_eq!(status(stale)[0], "stale");
        let fresh = face(&sc, "Every widget");
        assert!(fresh.run.not_run);
        assert_eq!(status(fresh), [run_text::NOT_RUN]);
        assert!(fresh.border.is_none());
    }

    #[test]
    fn revisions_move_only_for_what_changed() {
        let mut s = rich_session();
        let t = HashMap::new();
        let mut p = Published::default();
        let g1 = p.publish(build(&inputs(&s, Selection::default(), None, &t)));
        assert_eq!(g1, 1);
        assert_eq!(p.publish(build(&inputs(&s, Selection::default(), None, &t))), g1, "nothing changed");
        let full = p.get(None);
        assert_eq!(full["full"], true);
        let faces = full["nodes"].as_array().unwrap().len();
        assert!(faces > 10);
        let nids: Vec<u64> = full["nodes"].as_array().unwrap().iter().map(|f| f["nid"].as_u64().unwrap()).collect();
        assert!(nids.iter().all(|n| *n > 0));
        // one node's run state changes: one face in the delta
        let done = s.run.nodes.iter().find(|(_, r)| r.state == Some(NodeState::Queued)).map(|(k, _)| k.clone()).unwrap();
        s.run.nodes.get_mut(&done).unwrap().state = Some(NodeState::Started);
        let g2 = p.publish(build(&inputs(&s, Selection::default(), None, &t)));
        assert_eq!(g2, g1 + 1);
        let d = p.get(Some(g1));
        assert_eq!(d["full"], false);
        assert_eq!(d["upsert"]["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(d["upsert"]["nodes"][0]["id"], json!(done));
        assert_eq!(d["upsert"]["nodes"][0]["rev"], json!(g2));
        assert!(d["upsert"]["wires"].as_array().unwrap().is_empty());
        // its id keeps its nid
        let before = nids[full["nodes"].as_array().unwrap().iter().position(|f| f["id"] == json!(done)).unwrap()];
        assert_eq!(d["upsert"]["nodes"][0]["nid"], json!(before));
        assert!(p.get(Some(g2))["upsert"]["nodes"].as_array().unwrap().is_empty(), "nothing since the latest");
        // the selection changes the generation, and comes with every reply
        let sel = Selection { nodes: vec![NodeId(done.clone())], ..Selection::default() };
        let g3 = p.publish(build(&inputs(&s, sel.clone(), None, &t)));
        assert_eq!(g3, g2 + 1);
        assert_eq!(p.get(Some(g2))["selection"]["nodes"], json!([done]));
        // a node removed: its face and its wires go
        let src = s.graph().wires.iter().find(|w| w.to.port == "pic").unwrap().from.node.clone();
        s.execute("node.remove", &json!({"nodes": [src.0.clone()]})).unwrap();
        let g4 = p.publish(build(&inputs(&s, sel, None, &t)));
        let d = p.get(Some(g3));
        assert_eq!(d["gen"], json!(g4));
        assert!(d["remove"]["nodes"].as_array().unwrap().contains(&json!(src.0)));
        assert!(!d["remove"]["wires"].as_array().unwrap().is_empty());
        // too old, or from the future: everything
        assert_eq!(p.get(Some(g4 + 5))["full"], true);
    }

    #[test]
    fn removals_older_than_kept_send_everything() {
        let mut s = Session::new(catalog());
        let t = HashMap::new();
        let mut p = Published::default();
        let first = add(&mut s, "t.src", [0.0, 0.0]);
        let g0 = p.publish(build(&inputs(&s, Selection::default(), None, &t)));
        for k in 0..(KEEP_GENERATIONS + 4) {
            s.execute("node.move", &json!({"id": first.0, "pos": [k as f32, 0.0]})).unwrap();
            p.publish(build(&inputs(&s, Selection::default(), None, &t)));
        }
        assert_eq!(p.get(Some(g0))["full"], true, "a delta from before the removals kept");
        assert_eq!(p.get(Some(p.generation() - 1))["full"], false);
    }

    /// FR1: a value a node names an editor for is promoted as any input is, and on the collapsed
    /// group's face its row opens the editor on the member (the application's where it has one,
    /// else the command's own words), with the inner row's short form; a wire takes it away;
    /// undo, save and reopen, and a group inside another keep it.
    #[test]
    fn a_promoted_editor_value_is_a_row_that_opens_it_on_the_groups_face() {
        let mut s = Session::new(catalog());
        let n = add(&mut s, "t.all", [0.0, 0.0]);
        let g = GroupId(s.execute("group.create", &json!({"nodes": [n], "name": "dials"})).unwrap()["id"].as_str().unwrap().to_owned());
        s.execute("group.promote", &json!({"group": g, "port": {"node": n, "port": "curve"}, "label": "shape"})).unwrap();
        let t = HashMap::new();
        let proxy_row = |s: &Session, editors: &[Editor], g: &GroupId| -> Option<InRow> {
            let mut i = inputs(s, Selection::default(), None, &t);
            i.editors = editors;
            let sc = build(&i);
            let id = rows::proxy_node_id(g).0;
            sc.nodes.iter().find(|f| f.id == id)?.inputs.iter().find(|r| r.key == "curve").cloned()
        };
        let inner_row = |s: &Session, editors: &[Editor]| -> InRow {
            let mut i = inputs(s, Selection::default(), None, &t);
            i.editors = editors;
            let sc = build(&i);
            let ids: Vec<String> = sc.nodes.iter().map(|f| f.id.clone()).collect();
            sc.nodes
                .iter()
                .find(|f| f.id == n.0)
                .unwrap_or_else(|| panic!("no face {n:?} in {ids:?}"))
                .inputs
                .iter()
                .find(|r| r.key == "curve")
                .unwrap()
                .clone()
        };
        // open, the node's own row says so too
        let own = inner_row(&s, &EDITORS);
        assert_eq!(own.editor.as_ref().map(|e| (e.kind.as_str(), e.label.as_str())), Some(("test.dial", "Edit Dial…")));
        let own_bare = inner_row(&s, &[]);
        s.execute("group.collapse", &json!({"group": g, "collapsed": true})).unwrap();
        // with the application's editor: its label and summary, on the member
        let row = proxy_row(&s, &EDITORS, &g).expect("the promoted row on the group's face");
        assert_eq!((row.node.as_str(), row.label.as_str(), row.source.as_str()), (n.0.as_str(), "shape", "readonly"));
        assert_eq!(row.text.as_deref(), Some("shape: 2 points"));
        assert_eq!(row.editor.as_ref().map(|e| (e.label.as_str(), e.hover.as_str())), Some(("Edit Dial…", "Edit the dial in its window")));
        // with none (the native shells): the inner row's short form, and the command's words
        let bare = proxy_row(&s, &[], &g).unwrap();
        assert_eq!(bare.text.as_deref(), Some(r#"shape: {"pts":[0,1]}"#));
        assert_eq!(bare.text.as_deref().map(|t| t.trim_start_matches("shape")), own_bare.text.as_deref().map(|t| t.trim_start_matches("curve")));
        assert_eq!(bare.editor, own_bare.editor, "the same as the member's own row");
        let e = bare.editor.clone().expect("the row still opens what this app has");
        assert_eq!((e.kind.as_str(), e.label.as_str()), ("test.dial", "Edit Value…"));
        assert!(e.hover.contains("no test.dial editor"), "{}", e.hover);
        // what opens it is the member's own command
        let r = s.execute("node.editor", &json!({"node": n})).unwrap();
        assert_eq!((r["kind"].as_str(), r["port"].as_str()), (Some("test.dial"), Some("curve")));
        // undo takes the promotion away (collapse, then promote), redo brings it back
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.graph().group(&g).unwrap().promoted.is_empty());
        s.execute("edit.redo", &json!({})).unwrap();
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(proxy_row(&s, &EDITORS, &g).and_then(|r| r.editor).is_some());
        // saved and opened again: the same row
        let dir = plumcraft_scratch::dir("scene-fr1");
        let path = dir.join("dials.json");
        s.execute("graph.saveAs", &json!({"path": path})).unwrap();
        let mut again = Session::new(catalog());
        again.execute("graph.open", &json!({"path": path, "discard": true})).unwrap();
        assert_eq!(proxy_row(&again, &EDITORS, &g), proxy_row(&s, &EDITORS, &g));
        // a wire into another of its inputs leaves the editor (one into the value itself takes it
        // away: no value there to edit, as `node.editor` refuses)
        let src = add(&mut s, "t.all", [0.0, 600.0]);
        s.execute("group.collapse", &json!({"group": g, "collapsed": false})).unwrap();
        s.execute("wire.connect", &json!({"from": {"node": src, "port": "out"}, "to": {"node": n, "port": "pic"}})).unwrap();
        assert!(inner_row(&s, &EDITORS).editor.is_some(), "another input's wire changes nothing");
        // nested: the group inside another, the value promoted to the outer one too
        let outer = GroupId(s.execute("group.create", &json!({"nodes": [], "groups": [g], "name": "outer"})).unwrap()["id"].as_str().unwrap().to_owned());
        s.execute("group.promote", &json!({"group": outer, "port": {"node": n, "port": "curve"}, "label": "outer shape"})).unwrap();
        s.execute("group.collapse", &json!({"group": outer, "collapsed": true})).unwrap();
        let nested = proxy_row(&s, &EDITORS, &outer).expect("the outer group's row");
        assert_eq!((nested.label.as_str(), nested.node.as_str()), ("outer shape", n.0.as_str()));
        assert!(nested.editor.is_some());
        // demoted: gone from the face
        s.execute("group.unpromote", &json!({"group": outer, "label": "outer shape"})).unwrap();
        assert!(proxy_row(&s, &EDITORS, &outer).is_none());
    }

    #[test]
    fn focus_shows_only_the_group_and_its_breadcrumb() {
        let mut s = Session::new(catalog());
        let a = add(&mut s, "t.src", [0.0, 0.0]);
        let b = add(&mut s, "t.sink", [300.0, 0.0]);
        let g = GroupId(s.execute("group.create", &json!({"nodes": [a], "name": "outer"})).unwrap()["id"].as_str().unwrap().to_owned());
        let t = HashMap::new();
        let sc = build(&inputs(&s, Selection::default(), Some(&g), &t));
        assert_eq!(sc.nodes.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), [a.0.as_str()]);
        assert!(!sc.nodes.iter().any(|f| f.id == b.0));
        let focus = sc.focus.unwrap();
        assert_eq!(focus.path.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["outer"]);
    }

    #[test]
    fn the_application_line_and_the_watched_node_show() {
        let mut s = Session::new(catalog());
        let a = add(&mut s, "t.src", [0.0, 0.0]);
        let t: HashMap<String, String> = [(a.0.clone(), "x".repeat(200))].into();
        let mut i = inputs(&s, Selection::default(), None, &t);
        i.watched = Some(a.clone());
        let sc = build(&i);
        let f = &sc.nodes[0];
        assert!(f.watched);
        assert_eq!(f.border.as_ref().map(|b| (b.paint.token.clone(), b.width)), Some((Some("accent".into()), 2.5)));
        assert!(f.footer.iter().any(|x| matches!(x, Footer::AppText { text, full } if text.chars().count() < full.chars().count())));
    }

    #[test]
    fn hostile_since_is_refused() {
        for bad in [json!({"since": -1}), json!({"since": "x"}), json!({"since": 1.5}), json!({"x": 1}), json!([1]), json!("since")] {
            assert!(parse_since(&bad).is_err(), "{bad}");
        }
        assert_eq!(parse_since(&json!({})), Ok(None));
        assert_eq!(parse_since(&Value::Null), Ok(None));
        assert_eq!(parse_since(&json!({"since": u64::MAX})), Ok(Some(u64::MAX)));
        let p = Published::default();
        assert_eq!(p.get(Some(u64::MAX))["full"], true);
        assert_eq!(p.get(Some(0))["full"], true, "nothing published yet");
    }

    /// The shared fixtures (`crates/editor-core/fixtures/scene/`, `docs/shell-interface.md`
    /// 16.2): a full scene with every row, widget, footer and run state, and the delta after one
    /// node's run state changed. Swift and Kotlin decode the same files. Written afresh with
    /// `PLUMCRAFT_WRITE_FIXTURES=1`; otherwise the scene built now must be what they hold.
    #[test]
    fn the_shared_fixtures_are_the_scene_and_decode() {
        let mut s = rich_session();
        let first = s.graph().nodes.keys().next().unwrap().0.clone();
        let t: HashMap<String, String> = [(first.clone(), "the application's line".to_owned())].into();
        // an open group of two (its frame), the first node and the source measured (wires from
        // their pins), and a fit asked for (N2 slice 3)
        let (o1, o2) = (add(&mut s, "t.src", [900.0, 400.0]), add(&mut s, "t.sink", [1200.0, 400.0]));
        s.execute("wire.connect", &json!({"from": {"node": o1, "port": "text"}, "to": {"node": o2, "port": "text"}})).unwrap();
        s.execute("group.create", &json!({"nodes": [o1, o2], "name": "open pair"})).unwrap();
        let mut m = crate::geometry::Measured::default();
        m.set(
            &first,
            crate::geometry::FaceGeometry::sized(
                [240.0, 420.0],
                &[40.0, 62.0, 84.0, 106.0, 128.0, 150.0, 172.0, 194.0, 216.0, 238.0, 260.0, 282.0, 304.0, 326.0, 348.0],
                &[40.0, 62.0],
            ),
        );
        m.set(&o1.0, crate::geometry::FaceGeometry::sized([180.0, 90.0], &[], &[36.0, 58.0, 80.0]));
        let view = View { fit: Some(Fit { seq: 1, rect: [-352.0, -72.0, 1452.0, 2192.0] }), ..View::default() };
        let with = |i: Inputs<'_>| -> Scene {
            let mut i = i;
            i.geometry = Some(&m);
            i.view = view.clone();
            build(&i)
        };
        let mut p = Published::default();
        let g1 = p.publish(with(inputs(&s, Selection::default(), None, &t)));
        let full = p.get(None);
        let queued = s.run.nodes.iter().find(|(_, r)| r.state == Some(NodeState::Queued)).map(|(k, _)| k.clone()).unwrap();
        s.run.nodes.insert(queued, NodeRun { state: Some(NodeState::Progress), fraction: Some(0.5), generation: 1, ..NodeRun::default() });
        p.publish(with(inputs(&s, Selection::default(), None, &t)));
        let delta = p.get(Some(g1));
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join("scene");
        let files = [("full.json", &full), ("delta.json", &delta)];
        if std::env::var_os("PLUMCRAFT_WRITE_FIXTURES").is_some() {
            std::fs::create_dir_all(&dir).unwrap();
            for (name, v) in files {
                std::fs::write(dir.join(name), serde_json::to_string_pretty(v).unwrap() + "\n").unwrap();
            }
        }
        for (name, v) in files {
            let text = std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e} (PLUMCRAFT_WRITE_FIXTURES=1 writes it)"));
            let stored: Value = serde_json::from_str(&text).unwrap();
            fn first_difference(a: &Value, b: &Value, at: &str) -> Option<String> {
                match (a, b) {
                    (Value::Object(x), Value::Object(y)) => x
                        .keys()
                        .chain(y.keys())
                        .find_map(|k| first_difference(x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null), &format!("{at}.{k}"))),
                    (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
                        x.iter().zip(y).enumerate().find_map(|(i, (p, q))| first_difference(p, q, &format!("{at}[{i}]")))
                    }
                    _ if a == b => None,
                    _ => Some(format!("{at}: {a} against {b}")),
                }
            }
            assert!(
                first_difference(&stored, v, "").is_none(),
                "{name} is not the scene built now (PLUMCRAFT_WRITE_FIXTURES=1 rewrites it): {}",
                first_difference(&stored, v, "").unwrap_or_default()
            );
        }
        // the full scene decodes, and covers every kind a shell must draw
        let sc: Scene = serde_json::from_str(&std::fs::read_to_string(dir.join("full.json")).unwrap()).unwrap();
        let widgets: std::collections::BTreeSet<&str> =
            sc.nodes.iter().flat_map(|f| &f.inputs).filter_map(|r| r.widget.as_ref()).map(|w| w.kind.as_str()).collect();
        assert_eq!(widgets.into_iter().collect::<Vec<_>>(), ["choice", "color", "multichoice", "multiline", "number", "path", "text", "toggle"]);
        let sources: std::collections::BTreeSet<&str> = sc.nodes.iter().flat_map(|f| &f.inputs).map(|r| r.source.as_str()).collect();
        for k in ["own", "socket", "wired", "inherited", "readonly"] {
            assert!(sources.contains(k), "{k} in {sources:?}");
        }
        let states: std::collections::BTreeSet<&str> = sc.nodes.iter().filter_map(|f| f.run.state.as_deref()).collect();
        assert_eq!(states.into_iter().collect::<Vec<_>>(), ["cached", "cancelled", "done", "error", "preview", "progress", "queued", "skipped", "started"]);
        assert!(sc.nodes.iter().any(|f| f.run.stale) && sc.nodes.iter().any(|f| f.run.not_run));
        let footers: std::collections::BTreeSet<String> =
            sc.nodes.iter().flat_map(|f| &f.footer).map(|x| serde_json::to_value(x).unwrap()["kind"].as_str().unwrap().to_owned()).collect();
        assert_eq!(footers.into_iter().collect::<Vec<_>>(), ["appText", "convert", "editor", "engineText", "error", "excerpt", "fields", "progress", "save"]);
        assert!(sc.nodes.iter().flat_map(|f| &f.inputs).any(|r| r.badge.is_some()), "a wire over a bundled value");
        assert!(sc.nodes.iter().any(|f| matches!(f.preview, Some(Preview::Rgba8 { .. }))));
        // N2 slice 3's geometry: every drawn wire has its path, the open group its frame, a fit
        let drawn = sc.wires.iter().filter(|w| w.kind != "net");
        assert!(drawn.clone().count() > 0 && drawn.clone().all(|w| w.path.len() >= 4 && (w.path.len() - 1) % 3 == 0));
        assert!(sc.wires.iter().filter(|w| w.kind == "net").all(|w| w.path.is_empty()));
        assert!(sc.groups.iter().any(|g| !g.collapsed && g.rect.is_some()) && sc.groups.iter().any(|g| g.collapsed && g.rect.is_none()));
        assert_eq!(sc.view.fit.as_ref().map(|f| f.seq), Some(1));
        let d: Delta = serde_json::from_str(&std::fs::read_to_string(dir.join("delta.json")).unwrap()).unwrap();
        assert_eq!((d.full, d.since, d.upsert.nodes.len()), (false, g1, 1));
        assert_eq!(d.upsert.nodes[0].run.state.as_deref(), Some("progress"));
    }

    /// The scene's costs at section 10's size (500 nodes, 800 wires, every node run): build,
    /// first publish, the whole scene as bytes, and after one node's run state changes the
    /// rebuild, publish and delta as bytes. Ignored; run in release:
    /// `cargo test --release -p plumcraft-editor-core measure_scene -- --ignored --nocapture`.
    #[test]
    #[ignore = "measurement"]
    fn measure_scene_500() {
        let mut s = Session::new(catalog());
        s.edit("Scene", |g, cat| {
            *g = plumcraft_graph::Graph::new();
            let src = cat.node("t.src").unwrap().clone();
            let join = cat.node("t.join").unwrap().clone();
            let sink = cat.node("t.sink").unwrap().clone();
            let mut srcs = Vec::new();
            for i in 0..500usize {
                let pos = [(i % 25) as f32 * 260.0, (i / 25) as f32 * 160.0];
                let kind = if i % 10 == 9 {
                    &sink
                } else if i % 2 == 0 || srcs.is_empty() {
                    &src
                } else {
                    &join
                };
                let id = g.add_node(kind, pos);
                if kind.id == "t.join" {
                    for k in 0..4usize {
                        let from: &NodeId = &srcs[(i * 7 + k * 13) % srcs.len()];
                        g.connect(cat, PortRef::new(from.clone(), "text"), PortRef::new(id.clone(), format!("c{}", k + 1)))?;
                    }
                }
                if kind.id == "t.src" {
                    srcs.push(id);
                }
            }
            Ok(())
        })
        .unwrap();
        let ids: Vec<String> = s.graph().nodes.keys().map(|k| k.0.clone()).collect();
        for id in &ids {
            s.run.nodes.insert(id.clone(), NodeRun { state: Some(NodeState::Done), ms: Some(1.5), completed: true, generation: 1, ..NodeRun::default() });
        }
        let t = HashMap::new();
        let ms = |t0: Instant| t0.elapsed().as_secs_f64() * 1e3;
        let median = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        let (mut build_ms, mut faces_ms, mut publish_ms, mut full_ms, mut inc_ms, mut delta_ms) = (vec![], vec![], vec![], vec![], vec![], vec![]);
        let (mut full_bytes, mut delta_bytes) = (0, 0);
        for round in 0..20 {
            let mut p = Published::default();
            let t0 = Instant::now();
            let sc = build(&inputs(&s, Selection::default(), None, &t));
            build_ms.push(ms(t0));
            assert_eq!((sc.nodes.len(), sc.wires.len()), (500, 800));
            let t0 = Instant::now();
            let _ = faces(&inputs(&s, Selection::default(), None, &t));
            faces_ms.push(ms(t0));
            let t0 = Instant::now();
            let g1 = p.publish(sc);
            publish_ms.push(ms(t0));
            let t0 = Instant::now();
            let bytes = serde_json::to_vec(&p.get(None)).unwrap();
            full_ms.push(ms(t0));
            full_bytes = bytes.len();
            // one node's run state changes: rebuild, publish, the delta
            let id = &ids[round * 7 % ids.len()];
            s.run.nodes.insert(id.clone(), NodeRun { state: Some(NodeState::Progress), fraction: Some(0.5), generation: 2, ..NodeRun::default() });
            let t0 = Instant::now();
            let g2 = p.publish(build(&inputs(&s, Selection::default(), None, &t)));
            inc_ms.push(ms(t0));
            let t0 = Instant::now();
            let delta = serde_json::to_vec(&p.get(Some(g1))).unwrap();
            delta_ms.push(ms(t0));
            delta_bytes = delta.len();
            assert_eq!(g2, g1 + 1);
            s.run.nodes.insert(id.clone(), NodeRun { state: Some(NodeState::Done), ms: Some(1.5), completed: true, generation: 1, ..NodeRun::default() });
        }
        println!(
            "MEASURE {}",
            json!({"name": "scene.core.500", "build_ms": median(build_ms), "faces_only_ms": median(faces_ms), "first_publish_ms": median(publish_ms),
                   "full_get_to_bytes_ms": median(full_ms), "full_bytes": full_bytes,
                   "one_state_rebuild_and_publish_ms": median(inc_ms), "delta_get_to_bytes_ms": median(delta_ms), "delta_bytes": delta_bytes})
        );
    }

    /// The graph the C ABI's `spike.scene` makes (text nodes; Join takes two wires, Upper case
    /// one, each wired from an earlier node), so the core's costs can be split at the sizes the
    /// C ABI measures.
    pub(super) fn spike_session(n: usize, want: usize) -> Session {
        let mut s = Session::new(plumcraft_text::engine().schema());
        let cat = s.catalog().clone();
        let (lit, join, upper) = (cat.node("text.literal").unwrap().clone(), cat.node("text.join").unwrap().clone(), cat.node("text.upper").unwrap().clone());
        let joins = want.min(2 * (n - 1)).saturating_sub(n - 1).min(n - 1);
        s.edit("Scene", move |g, cat| {
            *g = Graph::new();
            let mut ids: Vec<NodeId> = Vec::with_capacity(n);
            let mut wires = 0;
            for i in 0..n {
                let pos = [(i % 25) as f32 * 260.0, (i / 25) as f32 * 160.0];
                let kind = if i == 0 {
                    &lit
                } else if i <= joins {
                    &join
                } else {
                    &upper
                };
                let id = g.add_node(kind, pos);
                if i > 0 {
                    let ports: &[&str] = if i <= joins { &["a", "b"] } else { &["text"] };
                    for (k, port) in ports.iter().enumerate() {
                        if wires >= want {
                            break;
                        }
                        let from = ids[(i * 7 + k * 13) % i].clone();
                        g.connect(cat, PortRef::new(from, "text"), PortRef::new(id.clone(), *port))?;
                        wires += 1;
                    }
                }
                ids.push(id);
            }
            Ok(())
        })
        .unwrap();
        s
    }

    /// N2 slice 3's split of the scene's cost at 500 and 2,000 nodes (the C ABI's graphs, every
    /// node done): the build, its parts, the first publish, and after one node's run state
    /// changes the rebuild and publish, and the delta to bytes. Ignored; run in release:
    /// `cargo test --release -p plumcraft-editor-core measure_scene_sizes -- --ignored --nocapture`.
    #[test]
    #[ignore = "measurement"]
    fn measure_scene_sizes() {
        // for a profiler: rebuild and publish the 2,000-node scene for this many seconds first
        if let Some(secs) = std::env::var("PLUMCRAFT_MEASURE_LOOP_SECS").ok().and_then(|s| s.parse::<u64>().ok()) {
            let s = spike_session(2000, 3200);
            let t = HashMap::new();
            let mut p = Published::default();
            let t0 = Instant::now();
            let mut k = 0u64;
            while t0.elapsed().as_secs() < secs {
                let mut sel = Selection::default();
                if k.is_multiple_of(2) {
                    sel.nodes.push(s.graph().nodes.keys().next().unwrap().clone());
                }
                p.publish(build(&inputs(&s, sel, None, &t)));
                k += 1;
            }
            println!("LOOPED {k}");
        }
        for (n, w) in [(500usize, 800usize), (2000, 3200)] {
            let mut s = spike_session(n, w);
            let ids: Vec<String> = s.graph().nodes.keys().map(|k| k.0.clone()).collect();
            let done = || NodeRun { state: Some(NodeState::Done), ms: Some(1.5), completed: true, generation: 1, ..NodeRun::default() };
            for id in &ids {
                s.run.nodes.insert(id.clone(), done());
            }
            let t = HashMap::new();
            let ms = |t0: Instant| t0.elapsed().as_secs_f64() * 1e3;
            let median = |mut v: Vec<f64>| {
                v.sort_by(f64::total_cmp);
                v[v.len() / 2]
            };
            let (mut build_ms, mut faces_ms, mut titles_ms, mut publish_ms, mut inc_ms, mut delta_ms, mut full_ms) =
                (vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
            let (mut delta_bytes, mut full_bytes) = (0, 0);
            for round in 0..15 {
                let mut p = Published::default();
                let t0 = Instant::now();
                let sc = build(&inputs(&s, Selection::default(), None, &t));
                build_ms.push(ms(t0));
                assert_eq!((sc.nodes.len(), sc.wires.len()), (n, w));
                let t0 = Instant::now();
                let _ = faces(&inputs(&s, Selection::default(), None, &t));
                faces_ms.push(ms(t0));
                let t0 = Instant::now();
                for id in s.graph().nodes.keys() {
                    std::hint::black_box(plumcraft_session::node_title(s.graph(), s.catalog(), id));
                }
                titles_ms.push(ms(t0));
                let t0 = Instant::now();
                let g1 = p.publish(sc);
                publish_ms.push(ms(t0));
                let t0 = Instant::now();
                full_bytes = serde_json::to_vec(&p.get(None)).unwrap().len();
                full_ms.push(ms(t0));
                let id = &ids[round * 7 % ids.len()];
                s.run.nodes.insert(id.clone(), NodeRun { state: Some(NodeState::Progress), fraction: Some(0.5), generation: 2, ..NodeRun::default() });
                let t0 = Instant::now();
                let g2 = p.publish(build(&inputs(&s, Selection::default(), None, &t)));
                inc_ms.push(ms(t0));
                let t0 = Instant::now();
                delta_bytes = serde_json::to_vec(&p.get(Some(g1))).unwrap().len();
                delta_ms.push(ms(t0));
                assert_eq!(g2, g1 + 1);
                s.run.nodes.insert(id.clone(), done());
            }
            // the hot calls' snapshot, and the calls themselves (every face measured)
            let mut m = crate::geometry::Measured::default();
            for f in faces(&inputs(&s, Selection::default(), None, &t)) {
                let ins: Vec<f32> = (0..f.inputs.len()).map(|k| 40.0 + 22.0 * k as f32).collect();
                let outs: Vec<f32> = (0..f.outputs.len()).map(|k| 40.0 + 22.0 * k as f32).collect();
                m.set(&f.id, crate::geometry::FaceGeometry::sized([200.0, 120.0], &ins, &outs));
            }
            let mut p = Published::default();
            let mut i = inputs(&s, Selection::default(), None, &t);
            i.geometry = Some(&m);
            let t0 = Instant::now();
            p.publish(build(&i));
            let measured_publish_ms = ms(t0);
            let (mut snap_ms, mut hit_us, mut target_us, mut paths_us) = (vec![], vec![], vec![], vec![]);
            let mut snap = crate::geometry::Snapshot::default();
            // for a profiler (N3 slice 1): make the 2,000-node snapshot for this many seconds
            if n == 2000
                && let Some(secs) = std::env::var("PLUMCRAFT_MEASURE_SNAPSHOT_SECS").ok().and_then(|s| s.parse::<u64>().ok())
            {
                let t0 = Instant::now();
                let mut k = 0u64;
                while t0.elapsed().as_secs() < secs {
                    std::hint::black_box(p.snapshot((0, 0, k, None), s.graph(), s.catalog(), &m));
                    k += 1;
                }
                println!("SNAPSHOTS {k}");
            }
            for _ in 0..5 {
                let t0 = Instant::now();
                snap = p.snapshot((0, 0, 0, None), s.graph(), s.catalog(), &m);
                snap_ms.push(ms(t0));
            }
            let nids: Vec<u64> = s.graph().nodes.keys().filter_map(|k| snap.face_nid(&k.0)).collect();
            let mut out = Vec::new();
            for k in 0..200usize {
                let (x, y) = ((k % 25) as f32 * 260.0 + 100.0, (k / 25) as f32 * 160.0 + 61.0);
                let t0 = Instant::now();
                std::hint::black_box(snap.hit(x, y, 12.0));
                hit_us.push(t0.elapsed().as_secs_f64() * 1e6);
                let t0 = Instant::now();
                std::hint::black_box(snap.wire_target(nids[k % nids.len()], true, 0, x, y, 24.0));
                target_us.push(t0.elapsed().as_secs_f64() * 1e6);
                out.clear();
                // a selection of 20 faces dragged
                let picked: Vec<u64> = (0..20).map(|j| nids[(k * 20 + j) % nids.len()]).collect();
                let t0 = Instant::now();
                snap.wire_paths(&picked, 3.0, 4.0, &mut out);
                paths_us.push(t0.elapsed().as_secs_f64() * 1e6);
            }
            println!(
                "MEASURE {}",
                json!({"name": format!("scene.core.hot.{n}"), "measured_first_publish_ms": measured_publish_ms, "snapshot_build_ms": median(snap_ms),
                       "hit_us": median(hit_us), "wire_target_us": median(target_us), "wire_paths_20_faces_us": median(paths_us), "wire_paths_floats": out.len()})
            );
            println!(
                "MEASURE {}",
                json!({"name": format!("scene.core.spike.{n}"), "build_ms": median(build_ms), "faces_only_ms": median(faces_ms),
                       "titles_of_every_node_ms": median(titles_ms), "first_publish_ms": median(publish_ms),
                       "full_get_to_bytes_ms": median(full_ms), "full_bytes": full_bytes,
                       "one_state_rebuild_and_publish_ms": median(inc_ms), "delta_get_to_bytes_ms": median(delta_ms), "delta_bytes": delta_bytes})
            );
        }
    }
}
