//! A node as data: its id, title, palette group, description, ports and a function. Every
//! array node is an [`Op`]; the port helpers keep their declarations short and alike.

use plumcraft_engine::{Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::catalogue::{NodeDef, PathHint, Port, Socket, Variadic};
use plumcraft_proto::error::NodeError;
use serde_json::{Value, json};

use crate::img::Img;

pub type Run = fn(&Inputs, &mut Cx) -> Result<Outputs, NodeError>;

/// One array node.
pub struct Op {
    /// `array.<slug>`.
    pub id: &'static str,
    /// The research software's title.
    pub display: &'static str,
    /// The palette group under Array (`Filters`, `Math`, …).
    pub group: &'static str,
    /// The research software's description (what it does, in the user's terms).
    pub description: &'static str,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub run: Run,
}

impl Node for Op {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: self.id.into(),
            display: self.display.into(),
            category: vec!["Array".into(), self.group.into()],
            description: self.description.into(),
            family: "array".into(),
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
        (self.run)(inputs, cx)
    }
}

pub fn config(message: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::CONFIG, message)
}

pub fn op_error(message: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::OP, message)
}

// ---------- ports ----------

const ARRAY: &str = plumcraft_proto::catalogue::core_types::ARRAY;

/// An array input (a socket).
pub fn array_in(name: &str, tooltip: &str) -> Port {
    Port { name: name.into(), ty: ARRAY.into(), tooltip: Some(tooltip.into()), socket: Some(Socket::Socket), ..Default::default() }
}

/// An array input that may be left unwired.
pub fn array_opt(name: &str, tooltip: &str) -> Port {
    Port { optional: true, ..array_in(name, tooltip) }
}

/// A variadic array input: slots `<name>1`, `<name>2`, … (at least `min`, at most `max`).
pub fn array_slots(name: &str, tooltip: &str, min: u32, max: u32) -> Port {
    Port { variadic: Some(Variadic { template: format!("{name}{{n}}"), min, max, ..Default::default() }), ..array_in(name, tooltip) }
}

pub fn array_out(name: &str, tooltip: &str) -> Port {
    Port { name: name.into(), ty: ARRAY.into(), tooltip: Some(tooltip.into()), ..Default::default() }
}

pub fn int_in(name: &str, default: i64, min: f64, max: f64, tooltip: &str) -> Port {
    Port { name: name.into(), ty: "INT".into(), default: json!(default), min: Some(min), max: Some(max), tooltip: Some(tooltip.into()), ..Default::default() }
}

pub fn float_in(name: &str, default: f64, min: f64, max: f64, tooltip: &str) -> Port {
    Port { name: name.into(), ty: "FLOAT".into(), default: json!(default), min: Some(min), max: Some(max), tooltip: Some(tooltip.into()), ..Default::default() }
}

pub fn bool_in(name: &str, default: bool, tooltip: &str) -> Port {
    Port { name: name.into(), ty: "BOOL".into(), default: json!(default), tooltip: Some(tooltip.into()), ..Default::default() }
}

pub fn enum_in(name: &str, choices: &[&str], default: &str, tooltip: &str) -> Port {
    Port {
        name: name.into(),
        ty: "ENUM".into(),
        default: json!(default),
        choices: Some(choices.iter().map(|c| json!(c)).collect()),
        tooltip: Some(tooltip.into()),
        ..Default::default()
    }
}

pub fn string_in(name: &str, default: &str, tooltip: &str) -> Port {
    Port { name: name.into(), ty: "STRING".into(), default: json!(default), tooltip: Some(tooltip.into()), ..Default::default() }
}

/// A file to open (the "…" picks one).
pub fn path_in(name: &str, tooltip: &str) -> Port {
    Port { path: Some(PathHint { mode: "open".into(), ..Default::default() }), ..string_in(name, "", tooltip) }
}

/// The same port, shown under the node's advanced settings.
pub fn advanced(p: Port) -> Port {
    Port { advanced: true, ..p }
}

pub fn out(name: &str, ty: &str, tooltip: &str) -> Port {
    Port { name: name.into(), ty: ty.into(), tooltip: Some(tooltip.into()), ..Default::default() }
}

// ---------- reading inputs ----------

/// A wired array input, as an [`Img`].
pub fn img(i: &Inputs, name: &str) -> Result<Img, NodeError> {
    match i.get(name) {
        Some(Data::Array(a)) => Img::from_array(a).map_err(|e| config(format!("{name}: {e}"))),
        Some(_) => Err(config(format!("{name}: connect an array"))),
        None => Err(config(format!("{name}: connect an array"))),
    }
}

/// An optional array input: `None` when unwired.
pub fn img_opt(i: &Inputs, name: &str) -> Result<Option<Img>, NodeError> {
    match i.get(name) {
        Some(Data::Array(_)) => img(i, name).map(Some),
        _ => Ok(None),
    }
}

/// The wired arrays of a variadic input, in slot order.
pub fn slots(i: &Inputs, name: &str) -> Result<Vec<Img>, NodeError> {
    let mut found: Vec<(u32, Img)> = Vec::new();
    for (k, v) in i {
        let Some(n) = k.strip_prefix(name).and_then(|n| n.parse::<u32>().ok()) else { continue };
        if let Data::Array(a) = v {
            found.push((n, Img::from_array(a).map_err(|e| config(format!("{k}: {e}")))?));
        }
    }
    found.sort_by_key(|(n, _)| *n);
    Ok(found.into_iter().map(|(_, a)| a).collect())
}

pub fn f(i: &Inputs, name: &str, default: f64) -> f64 {
    i.get(name).and_then(Data::json).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(default)
}

pub fn n(i: &Inputs, name: &str, default: i64) -> i64 {
    i.get(name).and_then(Data::json).and_then(|v| v.as_i64().or_else(|| v.as_f64().filter(|f| f.is_finite()).map(|f| f as i64))).unwrap_or(default)
}

pub fn b(i: &Inputs, name: &str, default: bool) -> bool {
    i.get(name).and_then(Data::json).and_then(Value::as_bool).unwrap_or(default)
}

pub fn s(i: &Inputs, name: &str, default: &str) -> String {
    i.get(name).and_then(Data::as_str).unwrap_or(default).to_owned()
}

/// One output.
pub fn one(name: &str, a: Img) -> Outputs {
    Outputs::from([(name.to_owned(), Data::Array(a.to_array()))])
}

/// Several outputs.
pub fn many(outs: Vec<(&str, Data)>) -> Outputs {
    outs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}
