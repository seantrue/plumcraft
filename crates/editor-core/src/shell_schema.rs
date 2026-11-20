//! JSON Schema for the shell interface (feature `schema`; N2 slice 4; `docs/shell-interface.md`
//! 3.1 rule 4), so a Swift or Kotlin shell can generate its types and check what it reads and
//! sends. One document: every type under `$defs`, the messages listed by name under `messages`.
//!
//! The scene and its deltas are the serde types the core sends ([`crate::scene`]). What the core
//! parses by hand (`geometry.set`, `ui.gesture`, `geometry.locate`) and the replies it builds as
//! JSON (hits, wire targets, a gesture's reply, `theme.get`) are described here by types that
//! exist only for the schema; the conformance suite checks the behaviour behind them.
//!
//! `schema/shell1.json` is the checked-in copy; `cargo run -p plumcraft-editor-core --example
//! export_shell_schema --features schema` rewrites it, and a test fails while it is stale.

#![cfg_attr(not(feature = "schema"), allow(dead_code))]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A graph port: `{node, port}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PortRefSchema {
    pub node: String,
    pub port: String,
}

/// A net's key: its name, and the group it is local to (absent: the top level).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NetKeySchema {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub name: String,
}

/// One face's measurements in `geometry.set` (graph units, at zoom 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FaceReport {
    /// The face's id (a node's; `group:<id>` for a collapsed group's).
    pub node: String,
    /// The face's `rev` measured; absent: not said.
    #[serde(default)]
    pub rev: Option<u64>,
    /// `[w, h]`, 0 to 1e6.
    pub size: [f64; 2],
    /// Each pin's height from the face's top, in row order; null: not measured.
    #[serde(default)]
    pub pins: Option<PinsReport>,
    /// The header's height (default 28).
    #[serde(default)]
    pub header: Option<f64>,
    /// Each input row's widget rect `[x0, y0, x1, y1]` in face coordinates; null: none.
    #[serde(default)]
    pub rows: Option<Vec<Option<[f64; 4]>>>,
    /// The header's parts and the footer's items, face coordinates.
    #[serde(default)]
    pub parts: Option<PartsReport>,
    /// Where the face's top left is drawn, from its position (default `[0, 0]`).
    #[serde(default)]
    pub offset: Option<[f64; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PinsReport {
    #[serde(default, rename = "in")]
    pub inputs: Option<Vec<Option<f64>>>,
    #[serde(default, rename = "out")]
    pub outputs: Option<Vec<Option<f64>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PartsReport {
    pub title: Option<[f64; 4]>,
    pub chevron: Option<[f64; 4]>,
    pub icon: Option<[f64; 4]>,
    pub menu: Option<[f64; 4]>,
    pub enter: Option<[f64; 4]>,
    pub preview: Option<[f64; 4]>,
    pub footer: Option<Vec<Option<[f64; 4]>>>,
}

/// `geometry.set`'s params.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GeometrySet {
    pub faces: Vec<FaceReport>,
}

/// `geometry.set`'s reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GeometrySetReply {
    pub applied: u64,
    /// Faces measured at an older `rev` than the scene's: kept, and to measure again.
    pub stale: Vec<String>,
    /// Faces the graph doesn't have (at most 64), skipped.
    pub unknown: Vec<String>,
}

/// A gesture's kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum GestureKind {
    Tap,
    DoubleTap,
    /// A secondary click, a Control-click, a long press (`longPress` is taken too).
    Context,
    LongPress,
    Drag,
    Marquee,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Begin,
    Move,
    End,
    Cancel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum Modifier {
    Shift,
    /// ⌘ on Apple platforms, Ctrl elsewhere.
    Primary,
    Alt,
    Ctrl,
}

/// `ui.gesture`'s params: a recognised gesture in graph units.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Gesture {
    pub kind: GestureKind,
    /// A drag's phase (only a drag's).
    #[serde(default)]
    pub phase: Option<Phase>,
    /// `[x, y]`, within ±1e7 (a drag's cancel needs none).
    #[serde(default)]
    pub at: Option<[f64; 2]>,
    /// A marquee's other corner.
    #[serde(default)]
    pub to: Option<[f64; 2]>,
    #[serde(default)]
    pub mods: Option<Vec<Modifier>>,
    /// The hit distance, graph units (default 12).
    #[serde(default)]
    pub slop: Option<f64>,
}

/// What a gesture ran: one per command, in order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CommandRun {
    pub command: String,
    pub params: Value,
    pub ok: bool,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
}

/// What is at a point, as the JSON queries give it (`geometry.hit`, `geometry.wireTarget`, a
/// gesture's `hit` and a wire's `target`); `plum_hit` is its typed form.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Hit {
    /// `none`, `node`, `pin`, `wire`, `bend`, `tag`, `frame` or `group`.
    pub kind: String,
    /// A pin's point (where a wire snaps), else the point asked.
    pub at: [f64; 2],
    /// A node's part (`body`, `header`, `title`, `chevron`, `icon`, `menu`, `enter`, `row`,
    /// `preview`, `footer`), a tag's (`driver`, `use`), a frame's (`body`, `tab`, `handle`), a
    /// group's (`band`).
    #[serde(default)]
    pub part: Option<String>,
    /// The face (node, pin, tag).
    #[serde(default)]
    pub node: Option<String>,
    /// The nid of what was hit.
    #[serde(default)]
    pub nid: Option<u64>,
    /// A pin's side (`in`, `out`), index, type, and whether a wire fits it (0, 1, 2 through a
    /// Convert).
    #[serde(default)]
    pub side: Option<String>,
    #[serde(default)]
    pub pin: Option<u32>,
    #[serde(default, rename = "type")]
    pub ty: Option<String>,
    #[serde(default)]
    pub fits: Option<u32>,
    #[serde(default)]
    pub wire: Option<String>,
    /// A row, footer item or bend point.
    #[serde(default)]
    pub index: Option<i64>,
    #[serde(default)]
    pub net: Option<String>,
    #[serde(default)]
    pub frame: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
}

/// `geometry.hit`'s params.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HitQuery {
    pub at: [f64; 2],
    #[serde(default)]
    pub slop: Option<f64>,
}

/// `geometry.wireTarget`'s params: where a wire dragged from `from` may land.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WireTargetQuery {
    pub at: [f64; 2],
    #[serde(default)]
    pub slop: Option<f64>,
    pub from: PinRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PinRef {
    pub node: String,
    /// `in` or `out`.
    pub side: String,
    pub pin: u32,
}

/// Where a drag is (`ui.gesture`'s `update`; `plum_drag` typed).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DragUpdate {
    pub serial: u64,
    pub dx: f64,
    pub dy: f64,
    /// A marquee's rect, and the faces it touches.
    #[serde(default)]
    pub rect: Option<[f64; 4]>,
    #[serde(default)]
    pub touches: Option<Vec<String>>,
    /// A wire's target.
    #[serde(default)]
    pub target: Option<Hit>,
}

/// `ui.gesture`'s reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GestureReply {
    pub commands: Vec<CommandRun>,
    #[serde(default)]
    pub hit: Option<Hit>,
    /// A drag's: `{serial, kind: move | wire | marquee | bend | frame | resize | group, faces?
    /// (a move's, nids), from? (a wire's fixed end), picked? (an input whose wire's end was
    /// picked up)}`.
    #[serde(default)]
    pub drag: Option<Value>,
    #[serde(default)]
    pub update: Option<DragUpdate>,
    /// A context gesture's: `menu.get`'s `context` for what it was on, and the menu.
    #[serde(default)]
    pub context: Option<Value>,
    #[serde(default)]
    pub menu: Option<Value>,
    /// A drag's cancel: was one under way.
    #[serde(default)]
    pub cancelled: Option<bool>,
}

/// `geometry.locate`'s reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LocateReply {
    pub at: [f64; 2],
    #[serde(default)]
    pub rect: Option<[f64; 4]>,
    #[serde(default)]
    pub nid: Option<u64>,
    #[serde(default)]
    pub measured: Option<bool>,
}

/// `theme.get`'s reply: every token a scene may name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ThemeReply {
    pub shell: u32,
    pub name: String,
    /// `[r, g, b, a]`, premultiplied, by token name.
    pub tokens: BTreeMap<String, [u8; 4]>,
}

/// The shell interface 1 schema document.
#[cfg(feature = "schema")]
pub fn shell1() -> Value {
    use schemars::generate::SchemaSettings;
    let mut generator = SchemaSettings::draft2020_12().into_generator();
    let mut messages = serde_json::Map::new();
    let mut add = |name: &str, schema: schemars::Schema| {
        messages.insert(name.to_owned(), schema.to_value());
    };
    add("Scene", generator.subschema_for::<crate::scene::Scene>());
    add("Delta", generator.subschema_for::<crate::scene::Delta>());
    add("GeometrySet", generator.subschema_for::<GeometrySet>());
    add("GeometrySetReply", generator.subschema_for::<GeometrySetReply>());
    add("Gesture", generator.subschema_for::<Gesture>());
    add("GestureReply", generator.subschema_for::<GestureReply>());
    add("HitQuery", generator.subschema_for::<HitQuery>());
    add("WireTargetQuery", generator.subschema_for::<WireTargetQuery>());
    add("Hit", generator.subschema_for::<Hit>());
    add("LocateReply", generator.subschema_for::<LocateReply>());
    add("ThemeReply", generator.subschema_for::<ThemeReply>());
    serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://github.com/seantrue/plumcraft/schema/shell1.json",
        "title": "PlumCraft shell interface 1",
        "description": "The editor core <-> shell interface (docs/shell-interface.md): scene.get's reply (Scene, or Delta with since), geometry.set (GeometrySet, GeometrySetReply), ui.gesture (Gesture, GestureReply), geometry.hit (HitQuery), geometry.wireTarget (WireTargetQuery), their replies (Hit), geometry.locate (LocateReply), theme.get (ThemeReply). Readers skip unknown fields and kinds.",
        "messages": messages,
        "$defs": generator.take_definitions(true),
    })
}

#[cfg(all(test, feature = "schema"))]
mod tests {
    use super::*;

    #[test]
    fn checked_in_schema_is_current() {
        let text = serde_json::to_string_pretty(&shell1()).unwrap() + "\n";
        let on_disk = include_str!("../schema/shell1.json");
        assert!(text == on_disk, "schema/shell1.json is stale: cargo run -p plumcraft-editor-core --example export_shell_schema --features schema");
    }

    /// The shared fixtures and what the suite sends are what the schema says (the scene by
    /// its own types; a gesture and a report through these).
    #[test]
    fn the_fixtures_read_as_the_schema_types() {
        let full: crate::scene::Scene = serde_json::from_str(include_str!("../fixtures/scene/full.json")).unwrap();
        assert!(!full.nodes.is_empty());
        let _: crate::scene::Delta = serde_json::from_str(include_str!("../fixtures/scene/delta.json")).unwrap();
        let g: Gesture = serde_json::from_value(serde_json::json!({"kind": "drag", "phase": "begin", "at": [1, 2], "mods": ["shift"]})).unwrap();
        assert!(matches!(g.kind, GestureKind::Drag));
        let r: GeometrySet =
            serde_json::from_value(serde_json::json!({"faces": [{"node": "n1", "size": [1, 2], "pins": {"in": [3, null]}, "offset": [-7, -7]}]})).unwrap();
        assert_eq!(r.faces.len(), 1);
        assert!(serde_json::from_value::<Gesture>(serde_json::json!({"kind": "tap", "bogus": 1})).is_err());
        let doc = shell1();
        assert_eq!(doc["messages"].as_object().unwrap().len(), 11);
        assert!(doc["$defs"].get("Face").is_some() && doc["$defs"].get("Paint").is_some());
    }
}
