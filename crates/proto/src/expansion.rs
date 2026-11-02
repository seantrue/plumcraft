//! An expansion (spec 7.5, F24): the graph a command node unrolls into, as an engine sends it in
//! `SchemaReply.expansions`. The catalogue keeps it as JSON; [`Expansion::parse`] reads one.
//!
//! The format is the research software's (`python/src/research_engine/expansions.py`): a graph fragment with
//! local node ids, its wires, and how the command maps onto it.
//!
//! Hostile input, like everything from the other side: [`Expansion::parse`] never panics, and
//! bounds what it accepts, since `pos` lands on the canvas and `nodes` in the graph.
//!
//! [`StoredExpansion`] is the same thing as a file: a group exported so that a pack can offer it
//! as a command (F67). Which folder holds such files, and what runs them, is the pack's.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Value, json};

/// The most nodes an expansion may add.
pub const MAX_NODES: usize = 4096;
/// The most wires an expansion may add.
pub const MAX_WIRES: usize = 65536;
/// How far from the command a fragment node may sit (graph units).
pub const MAX_OFFSET: f32 = 1_000_000.0;

/// A node of the fragment.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FragmentNode {
    pub kind: String,
    /// Widget values, over the node type's defaults.
    #[serde(default)]
    pub values: BTreeMap<String, Value>,
    /// Relative to the command.
    #[serde(default)]
    pub pos: [f32; 2],
    /// The title the designer gave it.
    #[serde(default)]
    pub title: Option<String>,
}

/// A port of a fragment node, by its local id.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LocalPort {
    pub node: String,
    pub port: String,
}

/// A wire between fragment nodes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FragmentWire {
    pub from: LocalPort,
    pub to: LocalPort,
    #[serde(default)]
    pub slice: Option<String>,
}

/// A parameter of the group the fragment came from.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FragmentPromoted {
    pub port: LocalPort,
    pub label: String,
    #[serde(default)]
    pub doc: String,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

/// A named result of the group the fragment came from.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FragmentOutput {
    pub from: LocalPort,
    pub name: String,
}

/// How the group the fragment came from was made (F67). Expanded into a group, it comes back as
/// it was. `checkpoints` are local node ids; the editor keeps them on the group for what reads
/// them.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct FragmentGroup {
    #[serde(default)]
    pub promoted: Vec<FragmentPromoted>,
    #[serde(default)]
    pub checkpoints: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<FragmentOutput>,
}

/// What a command node unrolls into.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Expansion {
    /// The command this is for; when given, it must be the one expanded.
    #[serde(default)]
    pub command: String,
    pub nodes: BTreeMap<String, FragmentNode>,
    #[serde(default)]
    pub wires: Vec<FragmentWire>,
    /// Command input -> the fragment inputs it feeds.
    #[serde(default)]
    pub inputs: BTreeMap<String, Vec<(String, String)>>,
    /// Command setting -> the fragment widgets that take it.
    #[serde(default)]
    pub settings: BTreeMap<String, Vec<(String, String)>>,
    /// Command output -> the fragment output that stands for it.
    #[serde(default)]
    pub outputs: BTreeMap<String, (String, String)>,
    /// What it unrolls into, for people.
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub group: Option<FragmentGroup>,
}

impl Expansion {
    /// Read and check an expansion: it has nodes, not too many, all a sane distance from the
    /// command, and every local id it uses names one of them.
    pub fn parse(value: &Value) -> Result<Expansion, String> {
        let e = Expansion::deserialize(value).map_err(|e| e.to_string())?;
        if e.nodes.is_empty() {
            return Err("it has no nodes".into());
        }
        if e.nodes.len() > MAX_NODES {
            return Err(format!("it has more than {MAX_NODES} nodes"));
        }
        if e.wires.len() > MAX_WIRES {
            return Err(format!("it has more than {MAX_WIRES} wires"));
        }
        if let Some((name, _)) = e.nodes.iter().find(|(_, n)| n.pos.iter().any(|c| !c.is_finite() || c.abs() > MAX_OFFSET)) {
            return Err(format!("node {name:?} sits too far away"));
        }
        e.check_locals()?;
        Ok(e)
    }

    /// Every local id named anywhere, to refuse one that names no node before anything changes.
    fn check_locals(&self) -> Result<(), String> {
        let known = |n: &str| if self.nodes.contains_key(n) { Ok(()) } else { Err(format!("it names a missing node {n:?}")) };
        for w in &self.wires {
            known(&w.from.node)?;
            known(&w.to.node)?;
        }
        for (n, _) in self.inputs.values().chain(self.settings.values()).flatten().chain(self.outputs.values()) {
            known(n)?;
        }
        if let Some(g) = &self.group {
            for n in g.promoted.iter().map(|p| &p.port.node).chain(g.outputs.iter().map(|o| &o.from.node)).chain(&g.checkpoints) {
                known(n)?;
            }
        }
        Ok(())
    }
}

/// What a stored expansion's `format` says.
pub const STORED_FORMAT: &str = "plumcraft.expansion";
/// The version of the format this reads.
pub const STORED_VERSION: u64 = 1;
/// A stored expansion's file name ends so.
pub const STORED_SUFFIX: &str = ".expansion.json";
/// The largest stored expansion read.
pub const MAX_STORED_BYTES: usize = 16 * 1024 * 1024;
/// The longest port name or command id.
const MAX_NAME: usize = 128;
/// The longest display name, in characters.
const MAX_DISPLAY: usize = 200;
/// The longest description, in characters.
const MAX_DESCRIPTION: usize = 8192;

/// An expansion as a file: what the command is called, and the graph it stands for. Unlike an
/// expansion an engine sends, it is the whole of the command: its ports come from the fragment.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredExpansion {
    pub display: String,
    pub description: String,
    pub expansion: Expansion,
    /// The expansion as it was read, for a catalogue's `expansions`.
    pub raw: Value,
}

fn is_name(name: &str, limit: usize) -> bool {
    !name.trim().is_empty() && name.len() <= limit && !name.chars().any(char::is_control)
}

impl StoredExpansion {
    /// A file's bytes, read and checked: never panics, and refuses what couldn't run.
    pub fn parse(bytes: &[u8]) -> Result<StoredExpansion, String> {
        if bytes.len() > MAX_STORED_BYTES {
            return Err(format!("it is larger than {MAX_STORED_BYTES} bytes"));
        }
        let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("it isn't JSON: {e}"))?;
        if v.get("format").and_then(Value::as_str) != Some(STORED_FORMAT) {
            return Err(format!("its format isn't {STORED_FORMAT}"));
        }
        match v.get("version").and_then(Value::as_u64) {
            Some(STORED_VERSION) => {}
            Some(other) => return Err(format!("it is version {other}; this reads version {STORED_VERSION}")),
            None => return Err("it has no version".into()),
        }
        let display = v.get("display").and_then(Value::as_str).unwrap_or_default().to_owned();
        if !is_name(&display, MAX_DISPLAY * 4) || display.chars().count() > MAX_DISPLAY {
            return Err("its display name is empty or unreadable".into());
        }
        let description = v.get("description").and_then(Value::as_str).unwrap_or_default().to_owned();
        if description.chars().count() > MAX_DESCRIPTION {
            return Err("its description is too long".into());
        }
        let raw = v.get("expansion").cloned().ok_or("it has no expansion")?;
        let expansion = Expansion::parse(&raw)?;
        expansion.check_whole()?;
        Ok(StoredExpansion { display, description, expansion, raw })
    }

    /// The file, from an expansion given as JSON: what [`StoredExpansion::parse`] reads.
    pub fn file(display: &str, description: &str, expansion: Value) -> Value {
        json!({"format": STORED_FORMAT, "version": STORED_VERSION, "display": display, "description": description, "expansion": expansion})
    }
}

impl Expansion {
    /// What a stored expansion needs beyond [`Expansion::parse`], since nothing but it describes
    /// the command: an id; an output; ports with usable names, none both an input and a setting;
    /// each fragment input fed once, by a wire or from outside; and no cycle.
    fn check_whole(&self) -> Result<(), String> {
        let id_ok = !self.command.is_empty()
            && self.command.len() <= MAX_NAME
            && self.command.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if !id_ok {
            return Err("its command id is empty or unreadable".into());
        }
        if self.outputs.is_empty() {
            return Err("it has no outputs".into());
        }
        let ports = || self.inputs.keys().chain(self.settings.keys()).chain(self.outputs.keys());
        if let Some(bad) = ports().find(|p| !is_name(p, MAX_NAME)) {
            return Err(format!("it has a bad port name {:?}", bad.chars().take(64).collect::<String>()));
        }
        if let Some(both) = self.inputs.keys().find(|p| self.settings.contains_key(*p)) {
            return Err(format!("{both:?} is both an input and a setting"));
        }
        let mut fed = BTreeSet::new();
        let wired = self.wires.iter().map(|w| (w.to.node.as_str(), w.to.port.as_str()));
        let mapped = self.inputs.values().chain(self.settings.values()).flatten().map(|(n, p)| (n.as_str(), p.as_str()));
        for (node, port) in wired.chain(mapped) {
            if !fed.insert((node, port)) {
                return Err(format!("{node}.{port} is fed twice"));
            }
        }
        // Kahn's: anything left over sits on a cycle
        let mut waiting: BTreeMap<&str, usize> = self.nodes.keys().map(|n| (n.as_str(), 0)).collect();
        let mut next: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for w in &self.wires {
            *waiting.entry(w.to.node.as_str()).or_default() += 1;
            next.entry(w.from.node.as_str()).or_default().push(w.to.node.as_str());
        }
        let mut ready: Vec<&str> = waiting.iter().filter(|(_, n)| **n == 0).map(|(k, _)| *k).collect();
        let mut done = 0;
        while let Some(n) = ready.pop() {
            done += 1;
            for t in next.get(n).into_iter().flatten() {
                if let Some(left) = waiting.get_mut(t) {
                    *left -= 1;
                    if *left == 0 {
                        ready.push(t);
                    }
                }
            }
        }
        if done == self.nodes.len() { Ok(()) } else { Err("its graph has a cycle".into()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> Value {
        json!({"command": "t.cmd", "nodes": {"a": {"kind": "t.a", "pos": [10, 20], "values": {"x": 1}}},
               "inputs": {"in": [["a", "in"]]}, "settings": {"s": [["a", "x"]]}, "outputs": {"out": ["a", "out"]}, "note": "one step"})
    }

    #[test]
    fn reads_the_format() {
        let e = Expansion::parse(&small()).unwrap();
        assert_eq!(e.nodes["a"].pos, [10.0, 20.0]);
        assert_eq!(e.inputs["in"], [("a".to_owned(), "in".to_owned())]);
        assert_eq!(e.outputs["out"], ("a".to_owned(), "out".to_owned()));
        assert_eq!(e.note, "one step");
    }

    #[test]
    fn refuses_what_would_harm_the_graph() {
        let with = |key: &str, v: Value| {
            let mut e = small();
            e[key] = v;
            e
        };
        let bad = [
            json!(null),
            json!([]),
            json!({"nodes": {}}),
            with("nodes", json!({"a": {"kind": "t.a", "pos": [1e300, 0]}})),
            with("nodes", json!({"a": {"kind": "t.a", "pos": [2e6, 0]}})),
            with("wires", json!([{"from": {"node": "zz", "port": "x"}, "to": {"node": "a", "port": "x"}}])),
            with("inputs", json!({"in": [["zz", "x"]]})),
            with("settings", json!({"s": [["zz", "x"]]})),
            with("outputs", json!({"out": ["zz", "x"]})),
            with("group", json!({"checkpoints": ["zz"]})),
            with("group", json!({"outputs": [{"from": {"node": "zz", "port": "x"}, "name": "n"}]})),
        ];
        for b in bad {
            assert!(Expansion::parse(&b).is_err(), "{b}");
        }
    }

    fn stored() -> Value {
        StoredExpansion::file("My group", "what it does", small())
    }

    #[test]
    fn a_stored_expansion_round_trips() {
        let bytes = stored().to_string();
        let s = StoredExpansion::parse(bytes.as_bytes()).unwrap();
        assert_eq!((s.display.as_str(), s.description.as_str(), s.expansion.command.as_str()), ("My group", "what it does", "t.cmd"));
        assert_eq!(s.raw, small());
    }

    #[test]
    fn a_stored_expansion_refuses_what_couldnt_run() {
        let two = json!({"a": {"kind": "t.a"}, "b": {"kind": "t.b"}});
        let wire = |from: &str, to: &str| json!({"from": {"node": from, "port": "o"}, "to": {"node": to, "port": "i"}});
        let change = |key: &str, v: Value| {
            let mut f = stored();
            f["expansion"][key] = v;
            f
        };
        let top = |key: &str, v: Value| {
            let mut f = stored();
            f[key] = v;
            f
        };
        let mut cycle = stored();
        cycle["expansion"]["nodes"] = two.clone();
        cycle["expansion"]["wires"] = json!([wire("a", "b"), wire("b", "a")]);
        let mut twice = stored();
        twice["expansion"]["nodes"] = two;
        twice["expansion"]["wires"] = json!([wire("a", "b")]);
        twice["expansion"]["inputs"] = json!({"in": [["b", "i"]]});
        let bad = [
            top("format", json!("other")),
            top("version", json!(2)),
            top("version", json!("1")),
            top("display", json!("")),
            top("display", json!("a\nb")),
            top("description", json!("x".repeat(9000))),
            change("command", json!("")),
            change("command", json!("t cmd")),
            change("outputs", json!({})),
            change("inputs", json!({"s": [["a", "in"]]})),
            change("inputs", json!({"bad\u{0}name": [["a", "in"]]})),
            change("inputs", json!({"in": [["a", "x"]]})),
            cycle,
            twice,
        ];
        for b in bad {
            assert!(StoredExpansion::parse(b.to_string().as_bytes()).is_err(), "{b}");
        }
        assert!(StoredExpansion::parse(&vec![b' '; MAX_STORED_BYTES + 1]).unwrap_err().contains("larger"));
        assert!(StoredExpansion::parse(b"not json").is_err());
    }
}
