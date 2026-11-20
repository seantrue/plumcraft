//! The text engine: the thin engine PlumCraft's interface is proven on (plan milestone S4). No
//! domain knowledge in the editor, no the research software: a handful of text nodes, plus two that exist for
//! the conformance suite (`text.slow`, which reports progress and honours cancel, and
//! `text.fail`).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod files;
pub mod pairs;

use std::time::{Duration, Instant};

use plumcraft_engine::{Array, Cx, Data, Engine, Inputs, Node, Outputs};
use plumcraft_proto::catalogue::{Media, NodeDef, PathHint, Port, Socket, WireType, core_types};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::session::EngineInfo;
use serde_json::{Value, json};

pub const TEXT: &str = core_types::TEXT;
pub const STRING: &str = "STRING";
pub const INT: &str = "INT";
pub const FLOAT: &str = "FLOAT";
pub const BOOL: &str = "BOOL";
pub const ENUM: &str = "ENUM";
pub const TABLE: &str = core_types::TABLE;
pub const ARRAY: &str = plumcraft_proto::catalogue::core_types::ARRAY;

/// The largest file `text.load` reads.
const MAX_LOAD: u64 = 64 << 20;
/// The longest text `text.repeat` makes.
const MAX_REPEAT: usize = 64 << 20;

/// A demo graph: a literal through a slow node, replace, upper case and a word count, with a
/// failing branch beside it. The CLI and the viewer open it when given no workflow.
pub const DEMO_WORKFLOW: &str = r#"{
  "version": 1,
  "nodes": {
    "story":  {"kind": "text.literal", "values": {"text": "A corgi naps on a floral carpet.\nThe carpet is red and gold."}},
    "think":  {"kind": "text.slow",    "values": {"seconds": 2.0}},
    "swap":   {"kind": "text.replace", "values": {"find": "corgi", "with": "very good corgi"}},
    "shout":  {"kind": "text.upper"},
    "count":  {"kind": "text.words"},
    "broken": {"kind": "text.fail"},
    "after":  {"kind": "text.lower"}
  },
  "wires": [
    {"from": {"node": "story", "port": "text"}, "to": {"node": "think", "port": "text"}},
    {"from": {"node": "think", "port": "text"}, "to": {"node": "swap",  "port": "text"}},
    {"from": {"node": "swap",  "port": "text"}, "to": {"node": "shout", "port": "text"}},
    {"from": {"node": "swap",  "port": "text"}, "to": {"node": "count", "port": "text"}},
    {"from": {"node": "story", "port": "text"}, "to": {"node": "broken", "port": "text"}},
    {"from": {"node": "broken", "port": "text"}, "to": {"node": "after", "port": "text"}}
  ]
}"#;

/// The same engine written in Python (`python/src/plumcraft_text`), as a mode 2 process: the
/// command and environment that start it from this checkout, under `uv run` (it needs numpy and
/// commonkit; `uv` syncs the project's environment on first use). `$PLUMCRAFT_UV` overrides the
/// `uv` binary.
pub fn python_command() -> (Vec<String>, Vec<(String, String)>) {
    let uv = std::env::var("PLUMCRAFT_UV").unwrap_or_else(|_| "uv".into());
    let project = concat!(env!("CARGO_MANIFEST_DIR"), "/../../python");
    (vec![uv, "run".into(), "--quiet".into(), "--project".into(), project.into(), "python".into(), "-m".into(), "plumcraft_text".into()], Vec::new())
}

/// The engine, ready to host.
pub fn engine() -> Engine {
    let info = EngineInfo {
        id: "text".into(),
        name: "Text".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        salt: plumcraft_engine::salt(&format!("rs:plumcraft-text+{}", env!("CARGO_PKG_VERSION")), &plumcraft_engine::code_digest!()),
        ..Default::default()
    };
    let mut e = Engine::new(info)
        .with_wire_type(core_types::text())
        .with_wire_type(WireType { id: STRING.into(), media: Media::Scalar, ..Default::default() })
        .with_wire_type(WireType { id: INT.into(), media: Media::Scalar, ..Default::default() })
        .with_wire_type(WireType { id: FLOAT.into(), media: Media::Scalar, ..Default::default() })
        .with_wire_type(WireType { id: BOOL.into(), media: Media::Scalar, ..Default::default() })
        .with_wire_type(WireType { id: ENUM.into(), media: Media::Scalar, ..Default::default() })
        .with_wire_type(core_types::table())
        .with_wire_type(core_types::array())
        .with_wire_type(core_types::json());
    for w in files::wire_types() {
        e = e.with_wire_type(w);
    }
    for n in nodes().into_iter().chain(files::nodes()).chain(pairs::nodes()) {
        e = e.with_node(n);
    }
    e.with_building_blocks()
}

fn nodes() -> Vec<Box<dyn Node>> {
    vec![
        Box::new(Simple { id: "text.literal", display: "Text", inputs: vec![widget("text", STRING, json!(""))], output: TEXT, f: |i| ok(json!(s(i, "text"))) }),
        Box::new(Simple { id: "text.upper", display: "Upper case", inputs: vec![socket("text")], output: TEXT, f: |i| ok(json!(s(i, "text").to_uppercase())) }),
        Box::new(Simple { id: "text.lower", display: "Lower case", inputs: vec![socket("text")], output: TEXT, f: |i| ok(json!(s(i, "text").to_lowercase())) }),
        Box::new(Simple {
            id: "text.reverse",
            display: "Reverse",
            inputs: vec![socket("text")],
            output: TEXT,
            f: |i| ok(json!(s(i, "text").chars().rev().collect::<String>())),
        }),
        Box::new(Simple {
            id: "text.replace",
            display: "Replace",
            inputs: vec![socket("text"), widget("find", STRING, json!("")), widget("with", STRING, json!(""))],
            output: TEXT,
            f: |i| {
                let find = s(i, "find");
                if find.is_empty() {
                    return Err(NodeError::new(NodeError::CONFIG, "find is empty"));
                }
                ok(json!(s(i, "text").replace(&find, &s(i, "with"))))
            },
        }),
        Box::new(Simple {
            id: "text.join",
            display: "Join",
            inputs: vec![socket("a"), socket("b"), widget("sep", STRING, json!(" "))],
            output: TEXT,
            f: |i| ok(json!(format!("{}{}{}", s(i, "a"), s(i, "sep"), s(i, "b")))),
        }),
        Box::new(Simple {
            id: "text.words",
            display: "Word count",
            inputs: vec![socket("text")],
            output: INT,
            f: |i| ok(json!(s(i, "text").split_whitespace().count())),
        }),
        Box::new(Simple {
            id: "text.fail",
            display: "Fail",
            inputs: vec![socket("text")],
            output: TEXT,
            f: |_| Err(NodeError::new(NodeError::OP, "this node always fails")),
        }),
        Box::new(Slow),
        Box::new(Widgets),
        Box::new(Simple {
            id: "text.repeat",
            display: "Repeat",
            inputs: vec![
                socket("text"),
                Port { name: "times".into(), ty: INT.into(), default: json!(2), min: Some(0.0), max: Some(1e6), ..Default::default() },
            ],
            output: TEXT,
            f: |i| {
                let text = s(i, "text");
                let times = i.get("times").and_then(Data::json).and_then(Value::as_u64).unwrap_or(2).min(1_000_000);
                let len = usize::try_from(times).ok().and_then(|t| t.checked_mul(text.len())).filter(|n| *n <= MAX_REPEAT);
                if len.is_none() {
                    return Err(NodeError::new(NodeError::CONFIG, format!("the result would exceed {} MiB", MAX_REPEAT >> 20)));
                }
                ok(json!(text.repeat(usize::try_from(times).unwrap_or(0))))
            },
        }),
        Box::new(Simple {
            id: "text.letters", display: "Letter counts (a–z)", inputs: vec![socket("text")], output: TABLE, f: |i| ok(letters(&s(i, "text")))
        }),
        Box::new(Simple {
            id: "text.codes",
            display: "Code points",
            inputs: vec![socket("text")],
            output: ARRAY,
            f: |i| {
                let codes: Vec<u8> = s(i, "text").chars().flat_map(|c| u32::from(c).to_le_bytes()).collect();
                let n = codes.len() as u64 / 4;
                Ok(Data::Array(Array::new("<u4", vec![n], codes)))
            },
        }),
        Box::new(Simple {
            id: "text.load",
            display: "Load text",
            inputs: vec![Port {
                name: "path".into(),
                ty: STRING.into(),
                default: json!(""),
                path: Some(PathHint { mode: "open".into(), ..Default::default() }),
                ..Default::default()
            }],
            output: TEXT,
            f: |i| {
                let path = s(i, "path");
                if path.is_empty() {
                    return Err(NodeError::new(NodeError::CONFIG, "path is empty"));
                }
                let meta = std::fs::metadata(&path).map_err(|e| NodeError::new(NodeError::CONFIG, format!("{path}: {e}")))?;
                if meta.len() > MAX_LOAD {
                    return Err(NodeError::new(NodeError::CONFIG, format!("{path}: larger than {} MiB", MAX_LOAD >> 20)));
                }
                let bytes = std::fs::read(&path).map_err(|e| NodeError::new(NodeError::OP, format!("{path}: {e}")))?;
                let text = String::from_utf8(bytes).map_err(|_| NodeError::new(NodeError::OP, format!("{path}: not UTF-8 text")))?;
                ok(json!(text))
            },
        }),
    ]
}

fn ok(v: Value) -> Result<Data, NodeError> {
    Ok(Data::Json(v))
}

/// The output port's name for an output type.
fn out_port(ty: &str) -> &'static str {
    match ty {
        INT => "count",
        TABLE => "table",
        ARRAY => "codes",
        _ => "text",
    }
}

/// Letter counts as a table: the Latin letters a–z, case-folded, in alphabetical order. (Rust's
/// `is_alphabetic` and Python's `isalpha` disagree on parts of Unicode, so both engines count
/// a–z and stay identical.)
fn letters(text: &str) -> Value {
    let mut counts: std::collections::BTreeMap<char, u64> = std::collections::BTreeMap::new();
    for c in text.chars().filter(char::is_ascii_alphabetic).map(|c| c.to_ascii_lowercase()) {
        *counts.entry(c).or_default() += 1;
    }
    json!({"columns": [
        {"name": "letter", "type": STRING, "values": counts.keys().map(|c| c.to_string()).collect::<Vec<_>>()},
        {"name": "count", "type": INT, "values": counts.values().collect::<Vec<_>>()},
    ]})
}

fn s(i: &Inputs, k: &str) -> String {
    match i.get(k) {
        Some(Data::Json(Value::String(s))) => s.clone(),
        Some(Data::Json(Value::Null)) | None => String::new(),
        Some(Data::Json(v)) => v.to_string(),
        Some(Data::Array(a)) => format!("array {}", a.dtype),
        Some(Data::Audio(a)) => format!("audio {} Hz", a.sample_rate),
        Some(Data::Blob(b)) => format!("blob {}", b.handle),
        Some(Data::List(l)) => format!("a list of {}", l.items.len()),
    }
}

fn socket(name: &str) -> Port {
    Port { name: name.into(), ty: TEXT.into(), socket: Some(Socket::Socket), ..Default::default() }
}

fn widget(name: &str, ty: &str, default: Value) -> Port {
    Port { name: name.into(), ty: ty.into(), default, ..Default::default() }
}

/// A one-output node from a function.
struct Simple {
    id: &'static str,
    display: &'static str,
    inputs: Vec<Port>,
    output: &'static str,
    f: fn(&Inputs) -> Result<Data, NodeError>,
}

impl Node for Simple {
    fn def(&self) -> NodeDef {
        let out = out_port(self.output);
        NodeDef {
            id: self.id.into(),
            display: self.display.into(),
            category: vec!["Text".into()],
            inputs: self.inputs.clone(),
            outputs: vec![Port { name: out.into(), ty: self.output.into(), ..Default::default() }],
            family: "text".into(),
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
        Ok(Outputs::from([(out_port(self.output).to_owned(), (self.f)(inputs)?)]))
    }
}

/// Every kind of widget a face can hold, for the editors' inline-edit tests (N4 slice 2): a text,
/// a whole and a fractional number with their ranges, a switch, a choice, several choices,
/// several lines and a colour. It says what it was given, one line a setting (as the Python
/// engine's `widgets_text`).
struct Widgets;

impl Node for Widgets {
    fn def(&self) -> NodeDef {
        let hint = |name: &str, widget: &str, default: Value| Port { widget: Some(widget.into()), ..self::widget(name, STRING, default) };
        NodeDef {
            id: "text.widgets".into(),
            display: "Every widget".into(),
            category: vec!["Text".into(), "Testing".into()],
            inputs: vec![
                socket("text"),
                self::widget("label", STRING, json!("hello")),
                Port { min: Some(0.0), max: Some(10.0), step: Some(1.0), ..self::widget("count", INT, json!(3)) },
                Port { min: Some(0.0), max: Some(1.0), decimals: Some(2), ..self::widget("amount", FLOAT, json!(0.5)) },
                self::widget("flag", BOOL, json!(true)),
                Port { choices: Some(vec![json!("upper"), json!("lower"), json!("title")]), ..self::widget("mode", ENUM, json!("upper")) },
                hint("tags", "multichoice", json!("")),
                hint("note", "multiline", json!("")),
                hint("tint", "hexcolor", json!("#3366cc")),
            ],
            outputs: vec![Port { name: "text".into(), ty: TEXT.into(), ..Default::default() }],
            family: "text".into(),
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
        let json_of = |k: &str| inputs.get(k).and_then(Data::json).cloned().unwrap_or(Value::Null);
        let text = s(inputs, "text");
        let flag = json_of("flag").as_bool().unwrap_or(true);
        let mode = s(inputs, "mode");
        let shown = if !flag {
            text.clone()
        } else {
            match mode.as_str() {
                "upper" => text.to_uppercase(),
                "lower" => text.to_lowercase(),
                "title" => title_case(&text),
                _ => text.clone(),
            }
        };
        let count = json_of("count");
        let amount = json_of("amount").as_f64().unwrap_or(0.0);
        let out = format!(
            "{}: {shown}\ncount={} amount={amount:.2} flag={} mode={mode}\ntags={} tint={}\n{}",
            s(inputs, "label"),
            count.as_i64().map_or_else(|| count.to_string(), |n| n.to_string()),
            if flag { "on" } else { "off" },
            s(inputs, "tags"),
            s(inputs, "tint"),
            s(inputs, "note"),
        );
        Ok(Outputs::from([("text".to_owned(), Data::Json(json!(out)))]))
    }
}

/// Python's `str.title` for the Latin letters: each run of letters starts upper case, the rest
/// lower.
fn title_case(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut in_word = false;
    for c in t.chars() {
        if c.is_alphabetic() {
            if in_word {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            in_word = true;
        } else {
            out.push(c);
            in_word = false;
        }
    }
    out
}

/// Passes text through after `seconds`, reporting progress in tenths and stopping early on
/// cancel: the conformance suite's long node.
struct Slow;

impl Node for Slow {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "text.slow".into(),
            display: "Slow".into(),
            category: vec!["Text".into(), "Testing".into()],
            inputs: vec![
                socket("text"),
                Port { name: "seconds".into(), ty: FLOAT.into(), default: json!(1.0), min: Some(0.0), max: Some(600.0), ..Default::default() },
            ],
            outputs: vec![Port { name: "text".into(), ty: TEXT.into(), ..Default::default() }],
            family: "text".into(),
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
        let seconds = inputs.get("seconds").and_then(Data::json).and_then(Value::as_f64).unwrap_or(1.0).clamp(0.0, 600.0);
        let total = Duration::from_secs_f64(seconds);
        let start = Instant::now();
        while start.elapsed() < total {
            if cx.cancelled() {
                return Err(NodeError::cancelled());
            }
            let done = start.elapsed().as_secs_f64() / seconds.max(f64::EPSILON);
            cx.progress(done, Some(&format!("{:.0}%", done * 100.0)));
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(Outputs::from([("text".to_owned(), Data::Json(json!(s(inputs, "text"))))]))
    }
}
