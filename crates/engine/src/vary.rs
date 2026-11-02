//! Vary: values to sweep a setting with, lifted from the research software (`research_engine/nodes/vary.py`,
//! and `numpy_ops.py`'s `linspace`, renamed Vary Range). Wired into an input that takes one
//! value, a Vary node's list runs that node once per value (the executor's list mapping,
//! [`crate::exec`]), each result labelled with the value that made it.
//!
//! - **Vary Float, Int, String, Bool:** a typed-in list, comma-separated; numbers also take
//!   ranges, `start:stop:step` (stop included when the steps land on it; step 1 when left out)
//!   or `start:stop/count` (count values evenly spaced, both ends included).
//! - **Vary Choices** feeds an ENUM input: blank means every choice that input offers (the
//!   executor passes them in as `_choices`), or a comma-separated subset.
//! - **Vary Range** is numpy's `linspace`: `num` numbers evenly spaced from `start` to `stop`
//!   (stop included unless `endpoint` is off).
//! - **Vary Range 2D and 3D** are `linspace` between two points: `num` points evenly spaced
//!   along the segment, their coordinates on outputs `x`, `y` (and `z`) that share one axis, so
//!   they zip downstream (two settings varied together), and the points themselves on `xy`
//!   (`xyz`), as tuples (`(x, y)`, which Unzip splits again). With `grid` on, every combination
//!   of `num` values per coordinate instead (a meshgrid, `x` varying fastest).
//!
//! Every engine built on the SDK can offer them with [`crate::Engine::with_vary`].

use plumcraft_proto::catalogue::{Media, NodeDef, NodeFlags, Port, Socket, WireType};
use plumcraft_proto::error::NodeError;
use serde_json::{Value, json};

use crate::exec::CHOICES_KEY;
use crate::{Cx, Data, Inputs, List, Node, Outputs};

/// The longest list a Vary node makes (a sweep, not a data array).
pub const MAX_VALUES: usize = 1000;

/// The category every Vary node is listed under.
fn category() -> Vec<String> {
    vec!["Building blocks".into(), "Vary".into()]
}

/// The scalar wire types the Vary nodes make.
pub fn wire_types() -> Vec<WireType> {
    ["FLOAT", "INT", "STRING", "BOOL", "ENUM"].into_iter().map(|id| WireType { id: id.into(), media: Media::Scalar, ..Default::default() }).collect()
}

fn config(message: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::CONFIG, message)
}

fn list_out(name: &str, ty: &str) -> Port {
    Port { name: name.into(), ty: ty.into(), list: true, socket: Some(Socket::Socket), ..Default::default() }
}

fn number_in(name: &str, label: &str, ty: &str, default: Value, tooltip: &str) -> Port {
    let step = if ty == "INT" { 1.0 } else { 0.1 };
    Port { name: name.into(), ty: ty.into(), label: Some(label.into()), default, step: Some(step), tooltip: Some(tooltip.into()), ..Default::default() }
}

/// The kinds of value a typed-in Vary node makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Float,
    Int,
    String,
    Bool,
}

/// The values a Vary node makes from its text (the research software's `_vary`): comma-separated items,
/// each a value of `kind` or, for numbers, a range `start:stop:step` or `start:stop/count`.
/// Blank items are skipped.
pub fn parse(text: &str, kind: Kind) -> Result<Vec<Value>, NodeError> {
    let mut out: Vec<Value> = Vec::new();
    for item in text.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        let range = matches!(kind, Kind::Float | Kind::Int).then(|| range_parts(item)).flatten();
        match range {
            None => out.push(one(item, kind)?),
            Some((start, stop, how)) => {
                let vals = expand(item, start, stop, how)?;
                if kind == Kind::Int {
                    if vals.iter().any(|v| v.fract() != 0.0) {
                        return Err(config(format!("{item:?}: the range must hit whole numbers")));
                    }
                    out.extend(vals.into_iter().map(|v| json!(v as i64)));
                } else {
                    out.extend(vals.into_iter().map(|v| json!(v)));
                }
            }
        }
        if out.len() > MAX_VALUES {
            return Err(config(format!("more than {MAX_VALUES} values")));
        }
    }
    if out.is_empty() {
        return Err(config("no values: type some, comma-separated"));
    }
    Ok(out)
}

/// How a range is spaced: by a step, or by a count.
#[derive(Debug, Clone, Copy)]
enum Spacing {
    Step(f64),
    Count(u64),
}

/// `start:stop[:step]` or `start:stop/count` (the research software's `([^:/]+):([^:/]+)(?::([^:/]+)|/(\d+))?`).
fn range_parts(item: &str) -> Option<(f64, f64, Spacing)> {
    let (start, rest) = item.split_once(':')?;
    let (stop, how) = if let Some((stop, count)) = rest.split_once('/') {
        if count.is_empty() || !count.bytes().all(|b| b.is_ascii_digit()) || stop.contains(':') {
            return None;
        }
        (stop, Spacing::Count(count.parse().ok()?))
    } else if let Some((stop, step)) = rest.split_once(':') {
        if step.contains([':', '/']) || step.is_empty() {
            return None;
        }
        (stop, Spacing::Step(step.trim().parse().ok()?))
    } else {
        (rest, Spacing::Step(1.0))
    };
    if start.is_empty() || stop.is_empty() || start.contains('/') || stop.contains('/') {
        return None;
    }
    Some((start.trim().parse().ok()?, stop.trim().parse().ok()?, how))
}

fn expand(item: &str, start: f64, stop: f64, how: Spacing) -> Result<Vec<f64>, NodeError> {
    if !start.is_finite() || !stop.is_finite() {
        return Err(config(format!("{item:?}: start and stop must be numbers")));
    }
    match how {
        Spacing::Count(0) => Err(config(format!("{item:?}: count must be at least 1"))),
        Spacing::Count(c) if c as usize > MAX_VALUES => Err(config(format!("more than {MAX_VALUES} values"))),
        Spacing::Count(1) => Ok(vec![start]),
        Spacing::Count(c) => Ok((0..c).map(|i| start + (stop - start) * i as f64 / (c - 1) as f64).collect()),
        Spacing::Step(step) => {
            if step == 0.0 || !step.is_finite() || (stop - start) * step < 0.0 {
                return Err(config(format!("{item:?}: the step must move from start toward stop")));
            }
            let n = ((stop - start) / step + 1e-9).floor() + 1.0;
            if n > MAX_VALUES as f64 {
                return Err(config(format!("more than {MAX_VALUES} values")));
            }
            Ok((0..n as u64).map(|i| start + step * i as f64).collect())
        }
    }
}

fn one(item: &str, kind: Kind) -> Result<Value, NodeError> {
    match kind {
        Kind::String => Ok(json!(item)),
        Kind::Bool => match item.to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Ok(json!(true)),
            "false" | "no" | "off" | "0" => Ok(json!(false)),
            _ => Err(config(format!("{item:?} is not true or false"))),
        },
        Kind::Float | Kind::Int => {
            let v: f64 = item.parse().ok().filter(|v: &f64| v.is_finite()).ok_or_else(|| config(format!("{item:?} is not a number")))?;
            if kind == Kind::Int {
                if v.fract() != 0.0 || v.abs() > 9.0e15 {
                    return Err(config(format!("{item:?} is not a whole number")));
                }
                return Ok(json!(v as i64));
            }
            Ok(json!(v))
        }
    }
}

fn text(i: &Inputs, key: &str) -> String {
    i.get(key).and_then(Data::as_str).unwrap_or_default().to_owned()
}

fn number(i: &Inputs, key: &str) -> Result<f64, NodeError> {
    i.get(key).and_then(Data::json).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| config(format!("{key} must be a number")))
}

fn flag(i: &Inputs, key: &str, default: bool) -> bool {
    i.get(key).and_then(Data::json).and_then(Value::as_bool).unwrap_or(default)
}

fn values(items: Vec<Value>) -> Data {
    Data::List(List::new(items.into_iter().map(Data::Json).collect()))
}

fn one_output(name: &str, d: Data) -> Outputs {
    Outputs::from([(name.to_owned(), d)])
}

/// A typed-in list: Vary Float, Int, String or Bool.
struct Typed {
    ident: &'static str,
    display: &'static str,
    ty: &'static str,
    kind: Kind,
    default: &'static str,
    what: &'static str,
}

impl Node for Typed {
    fn def(&self) -> NodeDef {
        let tip = match self.kind {
            Kind::Float | Kind::Int => {
                "The values, comma-separated; a range as start:stop:step (stop included when the steps land on it; step 1 if left out) or start:stop/count (count values, evenly spaced, both ends included)"
            }
            Kind::Bool => "The values, comma-separated; true/false, yes/no, on/off or 1/0",
            Kind::String => "The values, comma-separated; each item as typed",
        };
        NodeDef {
            id: format!("vary.{}", self.ident),
            display: self.display.into(),
            category: category(),
            description: format!(
                "A list of {} to sweep a setting with: wire it into a parameter and that node runs once per value, each result labelled with its value. Several varied settings zip when they come from the same Vary node, and form every combination when they don't.",
                self.what
            ),
            inputs: vec![Port { name: "values".into(), ty: "STRING".into(), default: json!(self.default), tooltip: Some(tip.into()), ..Default::default() }],
            outputs: vec![list_out("values", self.ty)],
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
        Ok(one_output("values", values(parse(&text(inputs, "values"), self.kind)?)))
    }
}

/// Vary Choices: every choice of the ENUM input it feeds, or some of them.
struct Choices;

impl Node for Choices {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "vary.choices".into(),
            display: "Vary Choices".into(),
            category: category(),
            description: "Every choice of the ENUM setting this is wired into (blank), or a comma-separated subset of them: that node runs once per choice, each result labelled with its choice. The choices come from the input it feeds, so nothing needs typing.".into(),
            inputs: vec![Port {
                name: "values".into(),
                ty: "STRING".into(),
                default: json!(""),
                tooltip: Some("Blank: every choice the wired input offers. Or some of them, comma-separated, as the dropdown spells them".into()),
                ..Default::default()
            }],
            outputs: vec![list_out("values", "STRING")],
            flags: NodeFlags { vary_choices: true, ..Default::default() },
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
        let offered: Vec<String> = inputs
            .get(CHOICES_KEY)
            .and_then(Data::json)
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|c| c.as_str().map_or_else(|| c.to_string(), str::to_owned)).collect())
            .unwrap_or_default();
        let typed = text(inputs, "values");
        if typed.trim().is_empty() {
            if offered.is_empty() {
                return Err(config("Vary Choices is blank and isn't wired into an input with choices: wire it, or type the choices, comma-separated"));
            }
            return Ok(one_output("values", values(offered.into_iter().map(Value::from).collect())));
        }
        let picked = parse(&typed, Kind::String)?;
        if !offered.is_empty() {
            let unknown: Vec<&str> = picked.iter().filter_map(Value::as_str).filter(|c| !offered.iter().any(|o| o == c)).collect();
            if !unknown.is_empty() {
                return Err(config(format!("not among the input's choices: {} (offered: {})", unknown.join(", "), offered.join(", "))));
            }
        }
        Ok(one_output("values", values(picked)))
    }
}

/// numpy's `linspace`: `num` values from `start` to `stop`, `stop` included when `endpoint`.
pub fn linspace(start: f64, stop: f64, num: usize, endpoint: bool) -> Vec<f64> {
    let div = if endpoint { num.saturating_sub(1) } else { num };
    if div == 0 {
        return vec![start; num.min(1)];
    }
    let step = (stop - start) / div as f64;
    let mut out: Vec<f64> = (0..num).map(|i| start + step * i as f64).collect();
    if endpoint && let Some(last) = out.last_mut() {
        *last = stop;
    }
    out
}

/// The count input: 1 to [`MAX_VALUES`].
fn count(i: &Inputs) -> Result<usize, NodeError> {
    let n = i.get("num").and_then(Data::json).and_then(Value::as_f64).unwrap_or(0.0);
    if !(1.0..=MAX_VALUES as f64).contains(&n) || n.fract() != 0.0 {
        return Err(config(format!("num must be a whole number from 1 to {MAX_VALUES}")));
    }
    Ok(n as usize)
}

/// Vary Range (1 dimension), and Vary Range 2D and 3D: `linspace` along a segment between two
/// points, or over a grid.
struct Range {
    dims: usize,
}

const AXES: [&str; 3] = ["x", "y", "z"];

impl Range {
    fn names(&self) -> &'static [&'static str] {
        AXES.get(..self.dims).unwrap_or(&AXES)
    }
}

impl Node for Range {
    fn def(&self) -> NodeDef {
        let mut inputs = Vec::new();
        let (id, display) = match self.dims {
            1 => ("vary.range", "Vary Range"),
            2 => ("vary.range2d", "Vary Range 2D"),
            _ => ("vary.range3d", "Vary Range 3D"),
        };
        let outputs: Vec<Port>;
        let description: String;
        if self.dims == 1 {
            inputs.push(number_in("start", "start", "FLOAT", json!(1.0), "The first value"));
            inputs.push(number_in("stop", "stop", "FLOAT", json!(3.0), "The last value (included unless endpoint is off)"));
            outputs = vec![list_out("values", "FLOAT")];
            description = "num numbers evenly spaced from start to stop (stop included unless endpoint is off), as numpy's linspace. Wire it into a parameter, such as a width, to run that node once per value.".into();
        } else {
            for (axis, (from, to)) in self.names().iter().zip([(0.0, 1.0), (0.0, 1.0), (0.0, 1.0)]) {
                inputs.push(number_in(&format!("start_{axis}"), &format!("start {axis}"), "FLOAT", json!(from), &format!("{axis} at the first point")));
                inputs.push(number_in(&format!("stop_{axis}"), &format!("stop {axis}"), "FLOAT", json!(to), &format!("{axis} at the last point")));
            }
            outputs = self.names().iter().map(|a| list_out(a, "FLOAT")).chain([list_out(&self.names().concat(), crate::ANY)]).collect();
            let coords = self.names().join(", ");
            description = format!(
                "num points evenly spaced from the start point to the stop point (numpy's linspace between two points): their {coords} on outputs that zip, so the settings they feed vary together, and the points as tuples on {}. With grid on, every combination of num values per coordinate instead ({}), {} varying fastest.",
                self.names().concat(),
                if self.dims == 2 { "num × num points" } else { "num × num × num points" },
                AXES[0]
            );
        }
        inputs.push(Port {
            name: "num".into(),
            ty: "INT".into(),
            default: json!(5),
            min: Some(1.0),
            max: Some(MAX_VALUES as f64),
            step: Some(1.0),
            tooltip: Some(if self.dims == 1 { "How many values".into() } else { "How many points (with grid on: how many values per coordinate)".into() }),
            ..Default::default()
        });
        inputs.push(Port {
            name: "endpoint".into(),
            ty: "BOOL".into(),
            default: json!(true),
            tooltip: Some("Include the stop value".into()),
            ..Default::default()
        });
        if self.dims > 1 {
            inputs.push(Port {
                name: "grid".into(),
                ty: "BOOL".into(),
                default: json!(false),
                tooltip: Some("Every combination of the coordinates' values, not points along a line".into()),
                ..Default::default()
            });
        }
        NodeDef { id: id.into(), display: display.into(), category: category(), description, inputs, outputs, ..Default::default() }
    }

    fn run(&self, inputs: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
        let num = count(inputs)?;
        let endpoint = flag(inputs, "endpoint", true);
        if self.dims == 1 {
            let v = linspace(number(inputs, "start")?, number(inputs, "stop")?, num, endpoint);
            return Ok(one_output("values", values(v.into_iter().map(Value::from).collect())));
        }
        let mut per_axis: Vec<Vec<f64>> = Vec::new();
        for axis in self.names() {
            per_axis.push(linspace(number(inputs, &format!("start_{axis}"))?, number(inputs, &format!("stop_{axis}"))?, num, endpoint));
        }
        let points: Vec<Vec<f64>> = if flag(inputs, "grid", false) {
            let total = num
                .checked_pow(self.dims as u32)
                .filter(|t| *t <= MAX_VALUES)
                .ok_or_else(|| config(format!("a grid of {num} per coordinate is more than {MAX_VALUES} points")))?;
            // row-major with the last coordinate slowest: x varies fastest
            (0..total)
                .map(|t| {
                    let mut rest = t;
                    per_axis
                        .iter()
                        .map(|vals| {
                            let v = vals.get(rest % num).copied().unwrap_or(0.0);
                            rest /= num;
                            v
                        })
                        .collect()
                })
                .collect()
        } else {
            (0..num).map(|i| per_axis.iter().map(|vals| vals.get(i).copied().unwrap_or(0.0)).collect()).collect()
        };
        // One axis for every output, named by the node: they zip downstream.
        let axes = vec![(cx.node_id().to_owned(), points.len())];
        let mut out = Outputs::new();
        for (k, axis) in self.names().iter().enumerate() {
            let items = points.iter().map(|p| Data::Json(Value::from(p.get(k).copied().unwrap_or(0.0)))).collect();
            out.insert((*axis).to_owned(), Data::List(List { items, axes: axes.clone(), labels: Vec::new() }));
        }
        let tuples = points.iter().map(|p| Data::List(List::new(p.iter().map(|v| Data::Json(Value::from(*v))).collect()))).collect();
        out.insert(self.names().concat(), Data::List(List { items: tuples, axes, labels: Vec::new() }));
        Ok(out)
    }
}

/// Every Vary node.
pub fn nodes() -> Vec<Box<dyn Node>> {
    vec![
        Box::new(Typed { ident: "float", display: "Vary Float", ty: "FLOAT", kind: Kind::Float, default: "0.5, 1, 2", what: "numbers" }),
        Box::new(Typed { ident: "int", display: "Vary Int", ty: "INT", kind: Kind::Int, default: "1, 2, 3", what: "whole numbers" }),
        Box::new(Typed { ident: "string", display: "Vary String", ty: "STRING", kind: Kind::String, default: "a, b", what: "texts" }),
        Box::new(Typed { ident: "bool", display: "Vary Bool", ty: "BOOL", kind: Kind::Bool, default: "false, true", what: "on/off values" }),
        Box::new(Choices),
        Box::new(Range { dims: 1 }),
        Box::new(Range { dims: 2 }),
        Box::new(Range { dims: 3 }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lists_and_ranges_as_the_research_software_does() {
        assert_eq!(parse("0.5, 1, 2", Kind::Float).unwrap(), vec![json!(0.5), json!(1.0), json!(2.0)]);
        assert_eq!(parse("1:3", Kind::Int).unwrap(), vec![json!(1), json!(2), json!(3)]);
        assert_eq!(parse("0:1:0.25", Kind::Float).unwrap().len(), 5);
        assert_eq!(parse("0:1/3", Kind::Float).unwrap(), vec![json!(0.0), json!(0.5), json!(1.0)]);
        assert_eq!(parse("10:0:-5", Kind::Int).unwrap(), vec![json!(10), json!(5), json!(0)]);
        assert_eq!(parse("a, , b", Kind::String).unwrap(), vec![json!("a"), json!("b")]);
        assert_eq!(parse("yes, OFF, 1", Kind::Bool).unwrap(), vec![json!(true), json!(false), json!(true)]);
        for (bad, kind) in [
            ("", Kind::Float),
            ("1.5", Kind::Int),
            ("0:1:0.3", Kind::Int),
            ("1:0", Kind::Float),
            ("0:1:0", Kind::Float),
            ("0:1/0", Kind::Float),
            ("maybe", Kind::Bool),
            ("x", Kind::Float),
            ("0:1e9", Kind::Float),
            ("nan", Kind::Float),
        ] {
            assert!(parse(bad, kind).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn linspace_matches_numpy() {
        assert_eq!(linspace(1.0, 3.0, 5, true), vec![1.0, 1.5, 2.0, 2.5, 3.0]);
        assert_eq!(linspace(0.0, 1.0, 4, false), vec![0.0, 0.25, 0.5, 0.75]);
        assert_eq!(linspace(2.0, 9.0, 1, true), vec![2.0]);
        assert_eq!(linspace(0.0, 1.0, 3, true).last(), Some(&1.0));
    }
}
