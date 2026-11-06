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
            converts: marks(self.id),
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
        (self.run)(inputs, cx).map(|out| keep_flat(inputs, out))
    }
}

/// An array is not always a picture: a 1-D array (a series: a spectrum, a mono signal) is read
/// as one row ([`Img::from_array`]), so what a node makes of it comes back (1, n). Put such a
/// result back to `[n]` when the node was given a 1-D array of that length, so a series stays a
/// series through the array nodes. Tables keep their shape.
pub(crate) fn keep_flat(inputs: &Inputs, mut out: Outputs) -> Outputs {
    let lengths: Vec<u64> = inputs
        .values()
        .filter_map(|d| match d {
            Data::Array(a) if a.shape.len() == 1 => a.shape.first().copied(),
            _ => None,
        })
        .collect();
    if lengths.is_empty() {
        return out;
    }
    for d in out.values_mut() {
        if let Data::Array(a) = d
            && !a.table
            && let [1, n] = a.shape.as_slice()
            && lengths.contains(n)
        {
            a.shape = vec![*n];
        }
    }
    out
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

/// The array nodes that convert between types (docs/research/20-convert.md), with their arrays'
/// shape: a picture, so a path never takes a sound's samples to Encode Image.
fn marks(id: &str) -> Vec<plumcraft_proto::convert::Converts> {
    use plumcraft_proto::catalogue::core_types::{ARRAY, IMAGE, SVG};
    use plumcraft_proto::convert::{Converts, IMAGE as PICTURE, VIDEO};
    match id {
        // a picture's colours, 0-1: the bytes come back as the same picture
        "array.decode_image" => vec![Converts { output: Some("rgb".into()), ..Converts::new(IMAGE, ARRAY, true).to_shape(PICTURE) }],
        // 8 bits, clipped to 0-1
        "array.encode_image" => vec![Converts::new(ARRAY, IMAGE, false).from_shape(PICTURE)],
        "array.rasterize_svg" => vec![Converts { output: Some("rgb".into()), ..Converts::new(SVG, ARRAY, false).to_shape(PICTURE) }],
        // a page drawn at a resolution
        "array.rasterize_pdf" => vec![Converts { output: Some("rgb".into()), ..Converts::new("PDF", ARRAY, false).to_shape(PICTURE) }],
        // 8 bits a channel
        "array.array_to_pdf" => vec![Converts::new(ARRAY, "PDF", false).from_shape(PICTURE)],
        // frames (T, H, W, C), and the frame rate beside them
        "array.mp4_to_array" | "array.mov_to_array" | "array.gif_to_array" => {
            let ty = id.trim_start_matches("array.").split('_').next().unwrap_or("").to_ascii_uppercase();
            vec![Converts { output: Some("frames".into()), ..Converts::new(&ty, ARRAY, false).to_shape(VIDEO) }]
        }
        // any value as its bytes: Convert's last resort (B15, #50), a 1-D uint8 series
        "array.to_bytes" => vec![Converts::new(plumcraft_proto::convert::ANY, ARRAY, false).to_shape(plumcraft_proto::convert::SERIES)],
        "array.array_to_mp4" | "array.array_to_mov" | "array.array_to_gif" => {
            let ty = id.rsplit('_').next().unwrap_or("").to_ascii_uppercase();
            vec![Converts::new(ARRAY, &ty, false).from_shape(VIDEO)]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use plumcraft_engine::Array;

    use super::*;

    #[test]
    fn a_series_stays_a_series() {
        let series = Array::from_f32(vec![4], &[1.0, 2.0, 3.0, 4.0]);
        let inputs = Inputs::from([("a".to_owned(), Data::Array(series))]);
        let row = Array::from_f32(vec![1, 4], &[2.0, 4.0, 6.0, 8.0]);
        let other = Array::from_f32(vec![1, 3], &[0.0; 3]);
        let mut table = Array::from_f32(vec![1, 4], &[0.0; 4]);
        table.table = true;
        let out =
            keep_flat(&inputs, Outputs::from([("r".to_owned(), Data::Array(row)), ("o".to_owned(), Data::Array(other)), ("t".to_owned(), Data::Array(table))]));
        let shape = |k: &str| match &out[k] {
            Data::Array(a) => a.shape.clone(),
            _ => vec![],
        };
        assert_eq!(shape("r"), [4], "(1, 4) from a 4-long series is the series");
        assert_eq!(shape("o"), [1, 3], "another length is left");
        assert_eq!(shape("t"), [1, 4], "a table keeps its shape");
        // a picture in: nothing changes
        let pic = Inputs::from([("a".to_owned(), Data::Array(Array::from_f32(vec![1, 4], &[0.0; 4])))]);
        let out = keep_flat(&pic, Outputs::from([("r".to_owned(), Data::Array(Array::from_f32(vec![1, 4], &[0.0; 4])))]));
        assert!(matches!(&out["r"], Data::Array(a) if a.shape == [1, 4]));
    }
}
