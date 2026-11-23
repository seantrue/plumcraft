//! PlumCraft's test engine for the port and graph features (`docs/research/24-native-shells.md`,
//! the engine half; the maintainer, 2026-11-21: "a test engine with nodes of its own for the port
//! and graph rows"). Each node carries one feature of the interface a shell must show and drive,
//! with a result that says it was honoured, so a check on a launched app can assert both what the
//! face shows and what the engine computed:
//!
//! - `ports.join`: a variadic input (`c1`, `c2`, … up to 4, floor F14) and a choice whose `line`
//!   option brings a sub-port (`how.width`, F15); it joins its inputs by lines (each cut to the
//!   width) or by spaces;
//! - `ports.mode`: a choice one option of which this machine can't take (`gpu`, disabled with "no
//!   GPU here", F18) and an input active only for another (`radius`, for `good`);
//! - `ports.read`: a path input that opens a text file (a path hint with a filter, F16);
//! - `ports.edge`: an input and an output marked `external` (F17), which a collapsed group shows
//!   as its own;
//! - `ports.result`: a result node (the `result` flag, F19), whose output is the graph's result;
//! - `ports.items` and `ports.count`: a list output and a list input (F20), and what a wire's
//!   slice takes from a list (F21: `[:-1]` keeps a list, `-1` one item);
//! - `ports.tune` and `ports.ink`: a bundle of settings and a node whose `ink.width` is bound to
//!   one of them (`param_ref`, the `bundle_port` flag, F13); wired, the bundle's value wins;
//! - `ports.strip`: a result node whose ARRAY isn't shaped like a picture (4 by 1000), so Save
//!   Result as PNG asks first (the save's question);
//! - `ports.curve`: a value with an editor of its own (`NodeDef.editor`, FR1), which a group can
//!   promote and open from its face;
//! - `ports.traced`: a node that tracks lineage, its text coming with a recipe (`Display.recipe`,
//!   F56), so a viewer offers Recipe.
//!
//! It is never compiled into an app: it is installed from a scratch engines folder as a separate
//! process (its program, `plumcraft-ports-engine`, built with the workspace).

use plumcraft_engine::{Array, Cx, Data, Engine, Inputs, List, Node, Outputs};
use plumcraft_proto::catalogue::{Editor, MemoSpec, NodeDef, NodeFlags, PathFilter, PathHint, Port, Socket, Variadic, WireType, core_types};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::session::EngineInfo;
use serde_json::{Value, json};

const TEXT: &str = core_types::TEXT;
const TUNE: &str = "x.ports.tune";

fn port(name: &str, ty: &str) -> Port {
    Port { name: name.into(), ty: ty.into(), ..Default::default() }
}

fn socket(name: &str, ty: &str) -> Port {
    Port { socket: Some(Socket::Socket), ..port(name, ty) }
}

fn widget(name: &str, ty: &str, default: Value) -> Port {
    Port { default, ..port(name, ty) }
}

fn def(id: &str, display: &str, inputs: Vec<Port>, outputs: Vec<Port>) -> NodeDef {
    NodeDef {
        id: id.into(),
        display: display.into(),
        category: vec!["Ports".into()],
        inputs,
        outputs,
        family: "ports".into(),
        memo: MemoSpec::default(),
        ..Default::default()
    }
}

fn text(i: &Inputs, k: &str) -> String {
    match i.get(k).and_then(Data::json) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(v) => v.to_string(),
    }
}

fn number(i: &Inputs, k: &str) -> Option<f64> {
    i.get(k).and_then(Data::json).and_then(Value::as_f64)
}

fn out(pairs: &[(&str, Value)]) -> Outputs {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), Data::Json(v.clone()))).collect()
}

struct Join;
impl Node for Join {
    fn def(&self) -> NodeDef {
        let mut c = socket("c", TEXT);
        c.variadic = Some(Variadic { template: "c{n}".into(), names: Vec::new(), min: 2, max: 4 });
        let mut how = widget("how", "ENUM", json!("line"));
        how.choices = Some(vec![json!("line"), json!("space")]);
        how.options = Some([("line".to_owned(), vec![widget("width", "INT", json!(80))])].into());
        def("ports.join", "Join", vec![c, how], vec![port("text", TEXT)])
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let parts: Vec<String> = (1..=4).map(|n| text(i, &format!("c{n}"))).filter(|s| !s.is_empty()).collect();
        let joined = if text(i, "how") == "space" {
            parts.join(" ")
        } else {
            let width = number(i, "how.width").unwrap_or(80.0).clamp(1.0, 10_000.0) as usize;
            parts.iter().map(|p| p.chars().take(width).collect::<String>()).collect::<Vec<_>>().join("\n")
        };
        Ok(out(&[("text", json!(joined))]))
    }
}

struct Mode;
impl Node for Mode {
    fn def(&self) -> NodeDef {
        let mut mode = widget("mode", "ENUM", json!("fast"));
        mode.choices = Some(vec![json!("fast"), json!("good"), json!("gpu")]);
        mode.disabled_choices = Some([("gpu".to_owned(), "no GPU here".to_owned())].into());
        let mut radius = widget("radius", "FLOAT", json!(2.0));
        radius.active_when = Some([("mode".to_owned(), vec![json!("good")])].into());
        def("ports.mode", "Mode", vec![mode, radius], vec![port("text", TEXT)])
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let mode = text(i, "mode");
        if mode == "gpu" {
            return Err(NodeError::new(NodeError::CONFIG, "no GPU here"));
        }
        let said = if mode == "good" { format!("good, radius {}", number(i, "radius").unwrap_or(2.0)) } else { mode };
        Ok(out(&[("text", json!(said))]))
    }
}

struct Read;
impl Node for Read {
    fn def(&self) -> NodeDef {
        let mut file = widget("file", "STRING", json!(""));
        file.path = Some(PathHint { mode: "open".into(), filters: vec![PathFilter { name: "Text".into(), extensions: vec!["txt".into()] }] });
        def("ports.read", "Read Text File", vec![file], vec![port("text", TEXT)])
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let f = text(i, "file");
        if f.is_empty() {
            return Err(NodeError::new(NodeError::CONFIG, "no file chosen"));
        }
        let meta = std::fs::metadata(&f).map_err(|e| NodeError::new(NodeError::CONFIG, format!("{f}: {e}")))?;
        if meta.len() > 1 << 20 {
            return Err(NodeError::new(NodeError::CONFIG, format!("{f}: larger than 1 MiB")));
        }
        let s = std::fs::read_to_string(&f).map_err(|e| NodeError::new(NodeError::CONFIG, format!("{f}: {e}")))?;
        Ok(out(&[("text", json!(s))]))
    }
}

struct Edge;
impl Node for Edge {
    fn def(&self) -> NodeDef {
        let input = Port { external: true, ..widget("seed", "STRING", json!("edge")) };
        let output = Port { external: true, ..port("text", TEXT) };
        def("ports.edge", "Edge", vec![input], vec![output, port("inner", TEXT)])
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let s = text(i, "seed");
        Ok(out(&[("text", json!(format!("[{s}]"))), ("inner", json!(s))]))
    }
}

struct ResultNode;
impl Node for ResultNode {
    fn def(&self) -> NodeDef {
        let mut d = def("ports.result", "Result", vec![socket("text", TEXT)], vec![port("text", TEXT)]);
        d.flags = NodeFlags { result: true, ..Default::default() };
        // Save Result (F19): its own list, the engine's format first, then the SDK's text
        d.save_formats = Some(vec![QUOTED.into(), "txt".into()]);
        d
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        Ok(out(&[("text", json!(text(i, "text")))]))
    }
}

struct Items;
impl Node for Items {
    fn def(&self) -> NodeDef {
        let items = Port { list: true, ..port("items", TEXT) };
        def("ports.items", "Items", vec![widget("words", "STRING", json!("a b c d"))], vec![items])
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let items: Vec<Data> = text(i, "words").split_whitespace().take(1000).map(|w| Data::Json(json!(w))).collect();
        Ok([("items".to_owned(), Data::List(List::new(items)))].into())
    }
}

struct Count;
impl Node for Count {
    fn def(&self) -> NodeDef {
        let items = Port { list: true, socket: Some(Socket::Socket), ..port("items", TEXT) };
        def("ports.count", "Count", vec![items], vec![port("count", "INT"), port("joined", TEXT)])
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let (n, joined) = match i.get("items") {
            Some(Data::List(l)) => {
                (l.items.len(), l.items.iter().filter_map(Data::json).map(|v| v.as_str().unwrap_or_default().to_owned()).collect::<Vec<_>>().join(","))
            }
            Some(d) => (1, d.json().and_then(Value::as_str).unwrap_or_default().to_owned()),
            None => (0, String::new()),
        };
        Ok(out(&[("count", json!(n)), ("joined", json!(joined))]))
    }
}

struct Tune;
impl Node for Tune {
    fn def(&self) -> NodeDef {
        def("ports.tune", "Tuning", vec![widget("width", "FLOAT", json!(3.0))], vec![port("tuning", TUNE)])
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        Ok(out(&[("tuning", json!({"ink": {"width": number(i, "width").unwrap_or(3.0)}}))]))
    }
}

struct Ink;
impl Node for Ink {
    fn def(&self) -> NodeDef {
        let mut width = widget("ink.width", "FLOAT", json!(1.0));
        width.param_ref = Some(plumcraft_proto::catalogue::ParamRef { group: "ink".into(), name: "width".into() });
        let tuning = Port { optional: true, ..socket("tuning", TUNE) };
        let mut d = def("ports.ink", "Ink", vec![tuning, width], vec![port("tuning", TUNE), port("level", "FLOAT")]);
        d.flags = NodeFlags { bundle_port: Some("tuning".into()), ..Default::default() };
        d
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        // the bound setting: the bundle's value when one is wired in, else the face's
        let tuning = i.get("tuning").and_then(Data::json).cloned().unwrap_or(json!({}));
        let level = tuning.pointer("/ink/width").and_then(Value::as_f64).or_else(|| number(i, "ink.width")).unwrap_or(1.0);
        Ok(out(&[("tuning", tuning), ("level", json!(level))]))
    }
}

/// `ports.strip`: a result whose value is an ARRAY not shaped like a picture, 4 by 1000 (a side
/// under 64, 250:1), so Save Result as PNG asks first (decisions 2A and 3A, 2026-11-22).
struct Strip;
impl Node for Strip {
    fn def(&self) -> NodeDef {
        let mut d = def("ports.strip", "Strip", vec![widget("level", "FLOAT", json!(0.5))], vec![port("strip", core_types::ARRAY)]);
        d.flags = NodeFlags { result: true, ..Default::default() };
        d
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let level = number(i, "level").unwrap_or(0.5) as f32;
        let values: Vec<f32> = (0..4000u32).map(|k| level * (k % 1000) as f32 / 999.0).collect();
        Ok([("strip".to_owned(), Data::Array(Array::from_f32(vec![4, 1000], &values)))].into_iter().collect())
    }
}

/// `ports.traced`: a node that tracks lineage (decision 7A, 2026-11-22): its text comes with what
/// made it (`Display.recipe`, `{style, params}`), so a viewer offers its Recipe.
struct Traced;
impl Node for Traced {
    fn def(&self) -> NodeDef {
        def("ports.traced", "Traced", vec![widget("seed", "STRING", json!("plum"))], vec![port("text", TEXT)])
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let seed = text(i, "seed");
        let recipe = json!({"style": "traced", "params": {"seed": seed}});
        Ok([("text".to_owned(), Data::Json(json!(format!("traced {seed}")))), (plumcraft_engine::exec::RECIPE_KEY.to_owned(), Data::Json(recipe))]
            .into_iter()
            .collect())
    }
}

/// `ports.curve`: a value with an editor of its own (`NodeDef.editor`, FR1): `curve`, a JSON
/// list of `[x, y]` points no widget edits, which an application's editor of kind `ports.curve`
/// would draw. Nothing draws that kind (the test engine registers no editor), so a shell shows
/// the row's short form and opens what it has for any value. Its text says how many points.
struct Curve;
impl Node for Curve {
    fn def(&self) -> NodeDef {
        // a value the node keeps (no widget edits it) or a wire brings
        let curve = Port { socket: Some(Socket::Both), ..widget("curve", core_types::JSON, json!([[0.0, 0.0], [0.5, 0.6], [1.0, 1.0]])) };
        let mut d = def("ports.curve", "Curve", vec![curve], vec![port("text", TEXT)]);
        d.editor = Some(Editor { kind: "ports.curve".into(), port: "curve".into(), interpolant: None });
        d
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let points = i.get("curve").and_then(Data::json).and_then(Value::as_array).map_or(0, Vec::len);
        Ok(out(&[("text", json!(format!("{points} points")))]))
    }
}

/// The save format of its own (F19, Save Result): text between a pair of `mark`s, an option
/// the editor asks for (`"` or `'`), as a `.quoted` file.
const QUOTED: &str = "quoted";

fn quoted() -> plumcraft_engine::save::Format {
    use plumcraft_engine::save::{Format, option};
    let mark = Port { choices: Some(vec![json!("\""), json!("'")]), ..option("mark", "Quote mark", "ENUM", json!("\"")) };
    Format::new(QUOTED, "Quoted Text", &[QUOTED], &[plumcraft_proto::catalogue::Media::Text], "text/plain; charset=utf-8", |v, cx| {
        let s = v.as_str().ok_or_else(|| NodeError::new(NodeError::CONFIG, "Quoted Text saves text"))?;
        let m = cx.options.get("mark").and_then(Value::as_str).unwrap_or("\"");
        Ok(format!("{m}{s}{m}").into_bytes())
    })
    .with_option(mark)
}

/// The engine, ready to host.
pub fn engine() -> Engine {
    let version = env!("CARGO_PKG_VERSION");
    let info = EngineInfo {
        id: "ports".into(),
        name: "Ports".into(),
        version: version.into(),
        salt: plumcraft_engine::salt(&format!("rs:plumcraft-ports+{version}"), "ports-1"),
        ..Default::default()
    };
    let scalar = |id: &str| WireType { id: id.into(), media: plumcraft_proto::catalogue::Media::Scalar, ..Default::default() };
    let mut e = Engine::new(info)
        .with_wire_type(core_types::text())
        .with_wire_type(core_types::array())
        .with_wire_type(core_types::json())
        .with_wire_type(scalar("STRING"))
        .with_wire_type(scalar("INT"))
        .with_wire_type(scalar("FLOAT"))
        .with_wire_type(scalar("ENUM"))
        .with_wire_type(WireType { id: TUNE.into(), color: Some("#7c5cff".into()), ..Default::default() });
    let nodes: Vec<Box<dyn Node>> = vec![
        Box::new(Join),
        Box::new(Mode),
        Box::new(Read),
        Box::new(Edge),
        Box::new(ResultNode),
        Box::new(Items),
        Box::new(Count),
        Box::new(Tune),
        Box::new(Curve),
        Box::new(Ink),
        Box::new(Strip),
        Box::new(Traced),
    ];
    for n in nodes {
        e = e.with_node(n);
    }
    e.with_save_format(quoted())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_node_says_what_it_carries() {
        let e = engine();
        let s = e.schema();
        let node = |id: &str| s.nodes.iter().find(|n| n.id == id).cloned().unwrap();
        assert!(node("ports.join").inputs[0].variadic.is_some());
        assert!(node("ports.join").inputs[1].options.as_ref().is_some_and(|o| o.contains_key("line")));
        assert_eq!(node("ports.mode").inputs[0].disabled_choices.as_ref().and_then(|d| d.get("gpu")).map(String::as_str), Some("no GPU here"));
        assert_eq!(node("ports.read").inputs[0].path.as_ref().map(|p| p.mode.as_str()), Some("open"));
        assert!(node("ports.edge").inputs[0].external && node("ports.edge").outputs[0].external);
        assert!(node("ports.result").flags.result);
        assert!(node("ports.items").outputs[0].list && node("ports.count").inputs[0].list);
        assert!(node("ports.ink").inputs[1].param_ref.is_some());
        let offered: Vec<&str> = s.save_formats_for(&node("ports.result"), "text").iter().map(|f| f.id.as_str()).collect();
        assert_eq!(offered, ["quoted", "txt"], "the result node saves in its own format first, then as text");
        assert!(e.caps().has("output.save"));
    }

    /// Save Result in both of the result node's formats, through the host: the bytes each writes.
    #[test]
    fn the_result_node_saves_quoted_and_as_text() {
        let h = plumcraft_engine::host::Host::start(Box::new(|| Ok(engine().with_store(None))), None);
        h.wait_ready(std::time::Duration::from_secs(5));
        let g = json!({"kind": "inline", "graph": {"nodes": {"e": {"kind": "ports.edge", "values": {"seed": "plum"}}, "r": {"kind": "ports.result"}},
                        "wires": [{"from": {"node": "e", "port": "text"}, "to": {"node": "r", "port": "text"}}]}});
        let save = |extra: Value| -> plumcraft_proto::run::SaveReply {
            let mut p = json!({"graph": g, "node": "r", "port": "text"});
            for (k, v) in extra.as_object().cloned().unwrap_or_default() {
                p[k] = v;
            }
            serde_json::from_value(h.dispatch("output.save", p).unwrap()).unwrap()
        };
        let bytes = |r: &plumcraft_proto::run::SaveReply| plumcraft_store::read_inline(r.files[0].data.as_ref().unwrap()).unwrap().bytes.to_vec();
        let q = save(json!({"format": "quoted", "options": {"mark": "'"}}));
        assert!(q.ok, "{q:?}");
        let edge = String::from_utf8(bytes(&save(json!({"format": "txt"})))).unwrap();
        assert_eq!(String::from_utf8(bytes(&q)).unwrap(), format!("'{edge}'"));
        assert_eq!(q.files[0].name, "result.quoted");
        assert!(!save(json!({"format": "quoted", "options": {"mark": "`"}})).ok, "a mark it doesn't offer");
    }
}
