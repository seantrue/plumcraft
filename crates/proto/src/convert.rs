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
//! another type, is ignored ([`SchemaReply::converters`]), never fatal.

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
/// The longest path searched.
pub const MAX_HOPS: usize = 4;
/// The most paths [`SchemaReply::convert_paths`] returns.
pub const MAX_PATHS: usize = 8;
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
}

impl Converts {
    pub fn new(from: &str, to: &str, lossless: bool) -> Self {
        Converts { from: from.into(), to: to.into(), input: None, output: None, lossless }
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
    /// The engine that runs it, in a merged catalogue; `None` in a single engine's.
    pub engine: Option<&'a str>,
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

fn validated(n: &NodeDef) -> Option<Converter<'_>> {
    let m = n.converts.as_ref()?;
    let plain = |t: &str| !t.is_empty() && t != "ANY" && t != TYPE;
    if n.id == KIND || !plain(&m.from) || !plain(&m.to) || m.from == m.to {
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
        engine: n.engines.first().map(String::as_str),
    })
}

impl SchemaReply {
    /// The catalogue's converters, in its order: the marks that hold (module docs).
    pub fn converters(&self) -> Vec<Converter<'_>> {
        self.nodes.iter().filter_map(validated).collect()
    }

    /// The ways from a value of type `from` to one that `done` accepts, best first: fewest
    /// steps, then every step lossless, then fewest changes of engine, then the catalogue's
    /// order. An empty path first when `from` is accepted already. At most [`MAX_PATHS`], of at
    /// most [`MAX_HOPS`] steps, never passing a type twice.
    pub fn convert_paths_where(&self, from: &str, done: impl Fn(&str) -> bool) -> Vec<Path<'_>> {
        let all = self.converters();
        let mut found: Vec<Path<'_>> = Vec::new();
        if done(from) {
            found.push(Vec::new());
        }
        let mut stack: Vec<(String, Path<'_>, BTreeSet<String>)> = vec![(from.to_owned(), Vec::new(), BTreeSet::from([from.to_owned()]))];
        while let Some((at, path, seen)) = stack.pop() {
            if path.len() >= MAX_HOPS {
                continue;
            }
            for c in &all {
                if !self.accepts(&at, c.from) || seen.contains(c.to) {
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
        let order = |p: &Path<'_>| p.iter().map(|c| all.iter().position(|a| a.kind == c.kind).unwrap_or(usize::MAX)).collect::<Vec<_>>();
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

    /// Does any converter take a value of type `from`?
    pub fn converts_from(&self, from: &str) -> bool {
        self.converters().iter().any(|c| self.accepts(from, c.from))
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
        let mut path = Vec::new();
        for k in kinds {
            let c = all.iter().find(|c| c.kind == k)?;
            if !self.accepts(&at, c.from) {
                return None;
            }
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
            converts: Some(Converts::new(from, to, lossless)),
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
        wrong_port.converts = Some(Converts { input: Some("nope".into()), ..Converts::new("WAV", "AUDIO", true) });
        let mut wrong_type = conv("b", "WAV", "AUDIO", true);
        wrong_type.converts = Some(Converts::new("TEXT", "AUDIO", true));
        let mut widget_in = conv("c", "WAV", "AUDIO", true);
        widget_in.inputs[0].socket = Some(Socket::Widget);
        let same = conv("d", "WAV", "WAV", true);
        let any = conv("e", "ANY", "AUDIO", true);
        let mut me = conv(KIND, "WAV", "AUDIO", true);
        me.id = KIND.into();
        let c = SchemaReply { nodes: vec![wrong_port, wrong_type, widget_in, same, any, me], ..Default::default() };
        assert!(c.converters().is_empty());
        assert!(c.convert_paths("WAV", "AUDIO").is_empty());
    }

    #[test]
    fn the_mark_reads_leniently() {
        // a mark that doesn't read is no mark; the node still reads
        let n: NodeDef = serde_json::from_value(json!({"id": "k", "converts": {"from": "WAV", "to": "AUDIO", "lossless": "yes"}})).unwrap();
        assert_eq!((n.id.as_str(), n.converts.as_ref()), ("k", None));
        let n: NodeDef = serde_json::from_value(json!({"id": "k", "converts": 7})).unwrap();
        assert!(n.converts.is_none());
        let n: NodeDef = serde_json::from_value(json!({"id": "k", "converts": {"from": "WAV", "to": "AUDIO", "lossless": true}})).unwrap();
        assert_eq!(n.converts, Some(Converts::new("WAV", "AUDIO", true)));
        assert_eq!(serde_json::to_value(&n).unwrap()["converts"], json!({"from": "WAV", "to": "AUDIO", "lossless": true}));
    }

    #[test]
    fn the_convert_node_is_core() {
        assert_eq!(def().id, KIND);
        assert_eq!((def().inputs[0].ty.as_str(), def().outputs[0].ty.as_str()), (TYPE, TYPE));
    }
}
