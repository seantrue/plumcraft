//! Conversions between types (`docs/research/20-convert.md`).
//!
//! An engine marks a node whose job is to turn one value into another ([`Converts`], the
//! `converts` field of a [`NodeDef`]): WAV to Audio, a craft document's export. The marks of
//! every connected engine are the edges of one graph of types, and [`SchemaReply::convert_paths`]
//! finds the ways from one type to another through it. The editor uses a path two ways: placing
//! its nodes on a wire the types refuse (`wire.convert`), and the core Convert node
//! ([`KIND`]), which takes the path's place until a run, when it is expanded into the path's
//! nodes. Nothing here converts anything: what runs is always an engine's node.
//!
//! A mark is hostile input like any other: one that names a port the node lacks, or a port of
//! another type, is ignored ([`SchemaReply::converters`]), never fatal. A node may carry several
//! (a craft's Open takes a picture, an SVG or a PDF): `converts` is one mark or a list.
//!
//! **An array is not always a picture.** An ARRAY can be an image (H, W[, C]) or a series (a
//! 1-D signal, or frames × channels, as Audio to Array makes). A mark may say which its ARRAY end
//! is (`from_shape`, `to_shape`: [`IMAGE`], [`SERIES`] or [`VIDEO`], frames (T, H, W, C)), and a
//! path never hands an array of one shape to a step that wants another: Audio to Array, then
//! Encode Image, is no path. An array whose shape nobody says (another node's output) may go
//! any way.
//!
//! **A last resort.** A mark from [`ANY`] (To Bytes: any value as its bytes, a 1-D uint8 array)
//! takes every type, so it would win every search it may join. It joins none that can be
//! answered without it: only when no path exists does [`SchemaReply::convert_paths_where`] try
//! it, as a path's first step, ordinary converters after it (maintainer, 2026-11-06: "Convert
//! should accept byte array as last resort").

use std::collections::BTreeSet;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::catalogue::{NodeDef, Port, SchemaReply, Socket};
use crate::de::null_as_default;

/// The core Convert node's kind. Core defines it; no engine runs it (it is expanded first).
pub const KIND: &str = "core.convert";
/// Its one input and its one output.
pub const PORT: &str = "value";
/// The type of its ports: no engine declares it, and only a Convert end matches it.
pub const TYPE: &str = "CONVERT";
/// The type a last-resort mark takes: any value.
pub const ANY: &str = "ANY";
/// The longest path searched.
pub const MAX_HOPS: usize = 4;
/// The most paths [`SchemaReply::convert_paths`] returns.
pub const MAX_PATHS: usize = 8;
/// An array that is a picture: (H, W) or (H, W, C).
pub const IMAGE: &str = "image";
/// An array that is a series: 1-D, or (frames, channels).
pub const SERIES: &str = "series";
/// An array that is a video: (T, H, W, C) frames, as Video to Array makes it.
pub const VIDEO: &str = "video";

/// Where a Convert node keeps the path it was given (`"convert": ["audio.from_wav"]`), in the
/// workflow's node.
pub const PIN: &str = "convert";

/// A node's mark: it turns a `from` into a `to`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Converts {
    #[serde(deserialize_with = "null_as_default")]
    pub from: String,
    #[serde(deserialize_with = "null_as_default")]
    pub to: String,
    /// The input that takes the `from`, when the node has more than one of that type; else its
    /// first wireable input of type `from`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    /// The output that gives the `to`; else its first output of type `to`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// The engine says `to` back to `from` gives the value back. Only ranks paths; a path with a
    /// step that isn't is reported as lossy.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "std::ops::Not::not")]
    pub lossless: bool,
    /// What the `from` array is, when it is one: [`IMAGE`] or [`SERIES`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_shape: Option<String>,
    /// What the `to` array is, when it is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_shape: Option<String>,
}

impl Converts {
    pub fn new(from: &str, to: &str, lossless: bool) -> Self {
        Converts { from: from.into(), to: to.into(), lossless, ..Default::default() }
    }

    /// This mark taking input `name`.
    pub fn input(mut self, name: &str) -> Self {
        self.input = Some(name.into());
        self
    }

    /// This mark's `from` array is a `shape`.
    pub fn from_shape(mut self, shape: &str) -> Self {
        self.from_shape = Some(shape.into());
        self
    }

    /// This mark's `to` array is a `shape`.
    pub fn to_shape(mut self, shape: &str) -> Self {
        self.to_shape = Some(shape.into());
        self
    }
}

/// `converts` on a NodeDef: one mark (an object) or several (a list). What doesn't read as a
/// mark is dropped, never fatal; one is written as an object.
pub mod marks {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::Value;

    use super::Converts;

    pub fn serialize<S: Serializer>(v: &[Converts], s: S) -> Result<S::Ok, S::Error> {
        match v {
            [one] => one.serialize(s),
            many => many.serialize(s),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Converts>, D::Error> {
        let v = Value::deserialize(d)?;
        Ok(match v {
            Value::Array(items) => items.into_iter().filter_map(|i| serde_json::from_value(i).ok()).collect(),
            o @ Value::Object(_) => serde_json::from_value(o).ok().into_iter().collect(),
            _ => Vec::new(),
        })
    }
}

/// One usable converter: a node with a mark that holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converter<'a> {
    pub kind: &'a str,
    pub display: &'a str,
    pub from: &'a str,
    pub to: &'a str,
    pub input: &'a str,
    pub output: &'a str,
    pub lossless: bool,
    pub from_shape: Option<&'a str>,
    pub to_shape: Option<&'a str>,
    /// The engine that runs it, in a merged catalogue; `None` in a single engine's.
    pub engine: Option<&'a str>,
}

impl Converter<'_> {
    /// It takes any value ([`ANY`]): tried only when nothing else converts (module docs).
    pub fn last_resort(&self) -> bool {
        self.from == ANY
    }
}

/// Can an array said to be `have` go where `want` is wanted? Yes unless both are said and differ.
fn shapes_meet(have: Option<&str>, want: Option<&str>) -> bool {
    match (have, want) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// A way from one type to another: its converters, in order. Empty when the types already meet.
pub type Path<'a> = Vec<Converter<'a>>;

/// `WAV to Audio → Audio to Spectrum`: how a path is shown.
pub fn label(path: &[Converter<'_>]) -> String {
    if path.is_empty() {
        return "no conversion needed".into();
    }
    path.iter().map(|c| c.display).collect::<Vec<_>>().join(" → ")
}

/// `WAV → AUDIO`: the types a path passes through.
pub fn types(path: &[Converter<'_>]) -> String {
    let mut out: Vec<&str> = path.first().map(|c| vec![c.from]).unwrap_or_default();
    out.extend(path.iter().map(|c| c.to));
    out.join(" → ")
}

/// Is every step of `path` lossless?
pub fn lossless(path: &[Converter<'_>]) -> bool {
    path.iter().all(|c| c.lossless)
}

fn validated<'a>(n: &'a NodeDef, m: &'a Converts) -> Option<Converter<'a>> {
    let plain = |t: &str| !t.is_empty() && t != ANY && t != TYPE;
    // `from` may be ANY (a last resort); `to` never is
    if n.id == KIND || !(plain(&m.from) || m.from == ANY) || !plain(&m.to) || m.from == m.to {
        return None;
    }
    let input = match &m.input {
        Some(name) => n.inputs.iter().find(|p| &p.name == name && p.ty == m.from && p.wireable())?,
        None => n.inputs.iter().find(|p| p.ty == m.from && p.wireable())?,
    };
    let output = match &m.output {
        Some(name) => n.outputs.iter().find(|p| &p.name == name && p.ty == m.to)?,
        None => n.outputs.iter().find(|p| p.ty == m.to)?,
    };
    let display = if n.display.is_empty() { n.id.as_str() } else { n.display.as_str() };
    Some(Converter {
        kind: &n.id,
        display,
        from: &m.from,
        to: &m.to,
        input: &input.name,
        output: &output.name,
        lossless: m.lossless,
        from_shape: m.from_shape.as_deref(),
        to_shape: m.to_shape.as_deref(),
        engine: n.engines.first().map(String::as_str),
    })
}

impl SchemaReply {
    /// The catalogue's converters, in its order: the marks that hold (module docs).
    pub fn converters(&self) -> Vec<Converter<'_>> {
        self.nodes.iter().flat_map(|n| n.converts.iter().filter_map(move |m| validated(n, m))).collect()
    }

    /// The ways from a value of type `from` to one that `done` accepts, best first: fewest
    /// steps, then every step lossless, then fewest changes of engine, then the catalogue's
    /// order. An empty path first when `from` is accepted already. At most [`MAX_PATHS`], of at
    /// most [`MAX_HOPS`] steps, never passing a type twice. When there is none, the last resorts
    /// ([`Converter::last_resort`]) are tried as a first step.
    pub fn convert_paths_where(&self, from: &str, done: impl Fn(&str) -> bool) -> Vec<Path<'_>> {
        let all = self.converters();
        let (resorts, all): (Vec<Converter<'_>>, Vec<Converter<'_>>) = all.into_iter().partition(Converter::last_resort);
        let mut found: Vec<Path<'_>> = Vec::new();
        if done(from) {
            found.push(Vec::new());
        }
        let mut stack: Vec<(String, Path<'_>, BTreeSet<String>)> = vec![(from.to_owned(), Vec::new(), BTreeSet::from([from.to_owned()]))];
        let mut tried_resorts = false;
        loop {
            let Some((at, path, seen)) = stack.pop() else {
                // nothing else converts: start again from each last resort
                if !found.is_empty() || tried_resorts {
                    break;
                }
                tried_resorts = true;
                for c in &resorts {
                    if c.to == from {
                        continue;
                    }
                    let first = vec![c.clone()];
                    if done(c.to) {
                        found.push(first.clone());
                    }
                    stack.push((c.to.to_owned(), first, BTreeSet::from([from.to_owned(), c.to.to_owned()])));
                }
                continue;
            };
            if path.len() >= MAX_HOPS {
                continue;
            }
            // what the last step said its array is
            let shape = path.last().and_then(|c| c.to_shape);
            for c in &all {
                if !self.accepts(&at, c.from) || !shapes_meet(shape, c.from_shape) || seen.contains(c.to) {
                    continue;
                }
                let mut next = path.clone();
                next.push(c.clone());
                if done(c.to) {
                    found.push(next.clone());
                }
                let mut seen = seen.clone();
                seen.insert(c.to.to_owned());
                stack.push((c.to.to_owned(), next, seen));
            }
        }
        let order = |p: &Path<'_>| p.iter().map(|c| all.iter().chain(&resorts).position(|a| a == c).unwrap_or(usize::MAX)).collect::<Vec<_>>();
        let crossings = |p: &Path<'_>| p.windows(2).filter(|w| w[0].engine != w[1].engine).count();
        found.sort_by_key(|p| (p.len(), p.iter().filter(|c| !c.lossless).count(), crossings(p), order(p)));
        found.dedup_by(|a, b| a.iter().map(|c| c.kind).eq(b.iter().map(|c| c.kind)));
        found.truncate(MAX_PATHS);
        found
    }

    /// The ways from a value of type `from` to an input of type `to`.
    pub fn convert_paths(&self, from: &str, to: &str) -> Vec<Path<'_>> {
        self.convert_paths_where(from, |t| self.accepts(t, to))
    }

    /// Does any converter take a value of type `from`? A last resort takes every one.
    pub fn converts_from(&self, from: &str) -> bool {
        self.converters().iter().any(|c| c.last_resort() || self.accepts(from, c.from))
    }

    /// Does any converter give what an input of type `to` accepts?
    pub fn converts_to(&self, to: &str) -> bool {
        self.converters().iter().any(|c| self.accepts(c.to, to))
    }

    /// The path `kinds` names, when each is a converter here and each step takes what the one
    /// before gives, starting from `from`: what a Convert node's pin means now.
    pub fn convert_path_of(&self, from: &str, kinds: &[String]) -> Option<Path<'_>> {
        let all = self.converters();
        let mut at = from.to_owned();
        let mut path: Path<'_> = Vec::new();
        for k in kinds {
            // a node with several marks: the one that takes what there is
            let shape = path.last().and_then(|c| c.to_shape);
            let c = all.iter().find(|c| c.kind == k && (c.last_resort() || self.accepts(&at, c.from)) && shapes_meet(shape, c.from_shape))?;
            at = c.to.to_owned();
            path.push(c.clone());
        }
        Some(path)
    }
}

static DEF: LazyLock<NodeDef> = LazyLock::new(|| {
    let port = |tip: &str| Port {
        name: PORT.into(),
        ty: TYPE.into(),
        socket: Some(Socket::Socket),
        tooltip: Some(tip.into()),
        default: json!(null),
        ..Default::default()
    };
    NodeDef {
        id: KIND.into(),
        display: "Convert".into(),
        category: vec!["Building blocks".into(), "Convert".into()],
        description: "Turns a value into the type the input it feeds needs, by the engines' converters (WAV to Audio, a document's export). Wire both ends; it picks the way and keeps it. When it runs, the converters run in its place.".into(),
        inputs: vec![port("Any value an engine can convert")],
        outputs: vec![port("The value as the inputs it feeds need it")],
        family: "core".into(),
        icon: "repeat".into(),
        ..Default::default()
    }
});

/// The Convert node's definition, which every catalogue the editor shows holds.
pub fn def() -> &'static NodeDef {
    &DEF
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(id: &str, from: &str, to: &str, lossless: bool) -> NodeDef {
        NodeDef {
            id: id.into(),
            display: id.into(),
            inputs: vec![
                Port { name: "x".into(), ty: from.into(), socket: Some(Socket::Socket), ..Default::default() },
                Port { name: "bits".into(), ty: "INT".into(), ..Default::default() },
            ],
            outputs: vec![Port { name: "y".into(), ty: to.into(), ..Default::default() }],
            converts: vec![Converts::new(from, to, lossless)],
            ..Default::default()
        }
    }

    fn catalog() -> SchemaReply {
        SchemaReply {
            nodes: vec![
                conv("from_wav", "WAV", "AUDIO", true),
                conv("to_wav", "AUDIO", "WAV", true),
                conv("spectrum", "AUDIO", "ARRAY", false),
                conv("bounce", "x.sc.doc", "WAV", false),
                conv("lossy_direct", "x.sc.doc", "AUDIO", false),
            ],
            ..Default::default()
        }
    }

    fn kinds(p: &Path<'_>) -> Vec<String> {
        p.iter().map(|c| c.kind.to_owned()).collect()
    }

    #[test]
    fn paths_go_through_the_marks_best_first() {
        let c = catalog();
        let p = c.convert_paths("WAV", "AUDIO");
        assert_eq!(kinds(&p[0]), ["from_wav"]);
        assert_eq!(c.convert_paths("WAV", "ARRAY").iter().map(kinds).collect::<Vec<_>>(), [vec!["from_wav", "spectrum"]]);
        // one lossy step beats two
        let doc = c.convert_paths("x.sc.doc", "AUDIO");
        assert_eq!(kinds(&doc[0]), ["lossy_direct"]);
        assert_eq!(kinds(&doc[1]), ["bounce", "from_wav"]);
        assert_eq!(label(&doc[1]), "bounce → from_wav");
        assert_eq!(types(&doc[1]), "x.sc.doc → WAV → AUDIO");
        assert!(!lossless(&doc[1]) && lossless(&p[0]));
        // the types meet already: an empty path first
        assert!(c.convert_paths("AUDIO", "AUDIO")[0].is_empty());
        // nothing goes back to a document
        assert!(c.convert_paths("AUDIO", "x.sc.doc").is_empty());
        assert!(c.converts_from("WAV") && !c.converts_from("TEXT"));
        assert!(c.converts_to("ARRAY") && !c.converts_to("TEXT"));
    }

    #[test]
    fn a_pin_holds_while_its_steps_still_fit() {
        let c = catalog();
        let pin = vec!["bounce".to_owned(), "from_wav".to_owned()];
        assert_eq!(c.convert_path_of("x.sc.doc", &pin).map(|p| p.len()), Some(2));
        assert!(c.convert_path_of("WAV", &pin).is_none(), "bounce takes a document");
        assert!(c.convert_path_of("x.sc.doc", &["gone".to_owned()]).is_none());
        assert_eq!(c.convert_path_of("WAV", &[]).map(|p| p.len()), Some(0));
    }

    #[test]
    fn a_mark_that_lies_is_ignored() {
        let mut wrong_port = conv("a", "WAV", "AUDIO", true);
        wrong_port.converts = vec![Converts::new("WAV", "AUDIO", true).input("nope")];
        let mut wrong_type = conv("b", "WAV", "AUDIO", true);
        wrong_type.converts = vec![Converts::new("TEXT", "AUDIO", true)];
        let mut widget_in = conv("c", "WAV", "AUDIO", true);
        widget_in.inputs[0].socket = Some(Socket::Widget);
        let same = conv("d", "WAV", "WAV", true);
        // ANY may be taken (a last resort), never given
        let to_any = conv("e", "AUDIO", "ANY", true);
        let mut me = conv(KIND, "WAV", "AUDIO", true);
        me.id = KIND.into();
        let c = SchemaReply { nodes: vec![wrong_port, wrong_type, widget_in, same, to_any, me], ..Default::default() };
        assert!(c.converters().is_empty());
        assert!(c.convert_paths("WAV", "AUDIO").is_empty());
    }

    /// B15 (#50): a converter from ANY (To Bytes) is a last resort: never in a path when
    /// another exists, the first step of one when none does.
    #[test]
    fn a_converter_from_any_is_a_last_resort() {
        let bytes = Converts::new(ANY, "ARRAY", false).to_shape(SERIES);
        let mut to_bytes = conv("to_bytes", ANY, "ARRAY", false);
        to_bytes.converts = vec![bytes];
        let mut c = catalog();
        c.nodes.push(to_bytes);
        c.nodes.push(conv("array_to_table", "ARRAY", "TABLE", false));
        assert!(c.converters().iter().any(Converter::last_resort));
        // WAV reaches AUDIO without it
        assert!(c.convert_paths("WAV", "AUDIO").iter().all(|p| p.iter().all(|s| !s.last_resort())));
        // nothing else takes a PDF to an array: its bytes
        let p = c.convert_paths("PDF", "ARRAY");
        assert_eq!(p.iter().map(kinds).collect::<Vec<_>>(), [vec!["to_bytes".to_owned()]]);
        // and ordinary steps may follow it
        assert_eq!(kinds(&c.convert_paths("PDF", "TABLE")[0]), ["to_bytes", "array_to_table"]);
        // anything converts now, and a pinned path through it reads back
        assert!(c.converts_from("PDF"));
        assert!(c.convert_path_of("PDF", &["to_bytes".to_owned(), "array_to_table".to_owned()]).is_some());
        // a type that already meets needs nothing
        assert_eq!(c.convert_paths("ARRAY", "ARRAY"), [Vec::<Converter<'_>>::new()]);
    }

    #[test]
    fn the_mark_reads_leniently() {
        // a mark that doesn't read is no mark; the node still reads
        let n: NodeDef = serde_json::from_value(json!({"id": "k", "converts": {"from": "WAV", "to": "AUDIO", "lossless": "yes"}})).unwrap();
        assert_eq!((n.id.as_str(), n.converts.len()), ("k", 0));
        let n: NodeDef = serde_json::from_value(json!({"id": "k", "converts": 7})).unwrap();
        assert!(n.converts.is_empty());
        let n: NodeDef = serde_json::from_value(json!({"id": "k", "converts": {"from": "WAV", "to": "AUDIO", "lossless": true}})).unwrap();
        assert_eq!(n.converts, [Converts::new("WAV", "AUDIO", true)]);
        assert_eq!(serde_json::to_value(&n).unwrap()["converts"], json!({"from": "WAV", "to": "AUDIO", "lossless": true}), "one mark is an object");
        // several: a list, and a bad one among them is dropped
        let n: NodeDef =
            serde_json::from_value(json!({"id": "k", "converts": [{"from": "A", "to": "B"}, 3, {"from": "C", "to": "B", "to_shape": "image"}]})).unwrap();
        assert_eq!(n.converts.len(), 2);
        assert_eq!(n.converts[1].to_shape.as_deref(), Some(IMAGE));
        assert!(serde_json::to_value(&n).unwrap()["converts"].is_array());
    }

    /// An array is not always a picture: a sound made an array never becomes an image, nor a
    /// picture a sound; an array nobody describes may go either way.
    #[test]
    fn an_array_of_one_shape_never_goes_where_the_other_is_wanted() {
        let mut to_array = conv("to_array", "AUDIO", "ARRAY", false);
        to_array.converts = vec![Converts::new("AUDIO", "ARRAY", false).to_shape(SERIES)];
        let mut from_array = conv("from_array", "ARRAY", "AUDIO", true);
        from_array.converts = vec![Converts::new("ARRAY", "AUDIO", true).from_shape(SERIES)];
        let mut decode = conv("decode", "IMAGE", "ARRAY", true);
        decode.converts = vec![Converts::new("IMAGE", "ARRAY", true).to_shape(IMAGE)];
        let mut encode = conv("encode", "ARRAY", "IMAGE", false);
        encode.converts = vec![Converts::new("ARRAY", "IMAGE", false).from_shape(IMAGE)];
        let c = SchemaReply { nodes: vec![to_array, from_array, decode, encode], ..Default::default() };
        assert!(c.convert_paths("AUDIO", "IMAGE").is_empty(), "a sound isn't a picture");
        assert!(c.convert_paths("IMAGE", "AUDIO").is_empty(), "nor a picture a sound");
        assert_eq!(c.convert_paths("AUDIO", "ARRAY").first().map(kinds), Some(vec!["to_array".to_owned()]));
        // an array from anywhere else: either
        assert_eq!(c.convert_paths("ARRAY", "IMAGE").first().map(kinds), Some(vec!["encode".to_owned()]));
        assert_eq!(c.convert_paths("ARRAY", "AUDIO").first().map(kinds), Some(vec!["from_array".to_owned()]));
        assert!(c.convert_path_of("AUDIO", &["to_array".to_owned(), "encode".to_owned()]).is_none(), "a pin can't cross shapes either");
    }

    #[test]
    fn a_node_with_several_marks_converts_from_each() {
        let mut open = conv("open", "IMAGE", "DOC", false);
        open.inputs.push(Port { name: "svg".into(), ty: "SVG".into(), socket: Some(Socket::Socket), ..Default::default() });
        open.converts = vec![Converts::new("IMAGE", "DOC", false).input("x"), Converts::new("SVG", "DOC", false).input("svg")];
        let c = SchemaReply { nodes: vec![open], ..Default::default() };
        assert_eq!(c.converters().len(), 2);
        assert_eq!(c.convert_paths("SVG", "DOC")[0][0].input, "svg");
        assert_eq!(c.convert_paths("IMAGE", "DOC")[0][0].input, "x");
        assert_eq!(c.convert_path_of("SVG", &["open".to_owned()]).map(|p| p[0].input), Some("svg"));
    }

    #[test]
    fn the_convert_node_is_core() {
        assert_eq!(def().id, KIND);
        assert_eq!((def().inputs[0].ty.as_str(), def().outputs[0].ty.as_str()), (TYPE, TYPE));
    }
}
