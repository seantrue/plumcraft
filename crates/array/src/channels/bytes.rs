//! Any value as bytes, and bytes as an array (docs/BUGS.md B15, #50): To Bytes makes any value
//! a 1-D `|u1` array of its bytes; Slice keeps part of an array along its first axis (for bytes,
//! a range of them: a binary file cut short); Bytes as Array reads bytes as numbers of a dtype,
//! in a byte order, in a shape. Load File → To Bytes → Slice → Bytes as Array reads a raw
//! binary file as the array it holds.
//!
//! What a value's bytes are:
//!
//! - an array: its numbers as stored (C order, little-endian);
//! - a sound: its samples, `<f4`, frames by channels, interleaved;
//! - a file or any encoded value (an IMAGE, a PDF, an SVG, a document): its bytes;
//! - text: its UTF-8; any other JSON value (a number, a table): its JSON text.

use plumcraft_engine::{Array, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::catalogue::{Port, Socket};
use plumcraft_proto::error::NodeError;

use crate::op::{Op, array_in, array_out, config, enum_in, int_in, n, s, string_in};

/// The most bytes To Bytes and Bytes as Array handle.
pub const MAX_BYTES: u64 = 1 << 30;

/// The dtypes Bytes as Array reads, by name, with numpy's little-endian string and item size.
pub const DTYPES: &[(&str, &str, usize)] = &[
    ("uint8", "|u1", 1),
    ("int8", "|i1", 1),
    ("uint16", "<u2", 2),
    ("int16", "<i2", 2),
    ("uint32", "<u4", 4),
    ("int32", "<i4", 4),
    ("uint64", "<u8", 8),
    ("int64", "<i8", 8),
    ("float16", "<f2", 2),
    ("float32", "<f4", 4),
    ("float64", "<f8", 8),
];

pub fn nodes() -> Vec<Box<dyn Node>> {
    let names: Vec<&str> = DTYPES.iter().map(|(n, ..)| *n).collect();
    vec![
        Box::new(Op {
            id: "array.to_bytes",
            display: "To Bytes",
            group: "Bytes",
            description: "Any value as the bytes it is made of: a 1-D array of uint8. An array gives its numbers as stored (little-endian, C order); a sound its float32 samples, interleaved; a file or an encoded value (a picture file, a PDF, an SVG) its bytes; text its UTF-8; any other value its JSON text. Slice and Bytes as Array then read part of it as numbers of any type and shape.",
            inputs: vec![Port {
                name: "value".into(),
                ty: plumcraft_engine::ANY.into(),
                socket: Some(Socket::Socket),
                tooltip: Some("Any value: an array, a sound, a file (Load File), a picture file, text".into()),
                ..Default::default()
            }],
            outputs: vec![array_out("bytes", "Its bytes, uint8, one dimension")],
            run: to_bytes,
        }),
        Box::new(Op {
            id: "array.slice",
            display: "Slice",
            group: "Bytes",
            description: "Part of an array along its first axis, as Python's a[start:stop:step]: for bytes, a range of them (a header skipped, a file cut short); for a picture, a band of rows. Negative start and stop count from the end, and stop 0 means the end. The array keeps its type and its other axes.",
            inputs: vec![
                array_in("array", "The array to take part of"),
                int_in("start", 0, -1e15, 1e15, "The first to keep (negative: from the end)"),
                int_in("stop", 0, -1e15, 1e15, "One past the last to keep (0: the end; negative: from the end)"),
                int_in("step", 1, 1.0, 1e9, "Keep every step-th"),
            ],
            outputs: vec![array_out("array", "The part kept")],
            run: slice,
        }),
        Box::new(Op {
            id: "array.from_bytes",
            display: "Bytes as Array",
            group: "Bytes",
            description: "An array's bytes (To Bytes, Slice) read as numbers of a type, in a byte order, in a shape: how a raw binary file becomes the array it holds. The shape is its sides, separated by commas or spaces (480, 640, 3); one side may be -1, worked out from how many bytes there are. Bytes left over beyond the shape are an error, or dropped.",
            inputs: vec![
                array_in("bytes", "The bytes to read (an array of any type: its bytes as stored)"),
                enum_in("dtype", &names, "uint8", "The type of each number"),
                enum_in("byte_order", &["little", "big"], "little", "How each number's bytes are ordered in the data"),
                string_in(
                    "shape",
                    "-1",
                    "The sides, separated by commas: 480, 640, 3. One may be -1, worked out from the bytes; -1 alone is all of them, in one dimension",
                ),
                enum_in("extra", &["error", "drop"], "error", "Bytes beyond what the shape needs: an error, or dropped (a file cut to the shape)"),
            ],
            outputs: vec![array_out("array", "The numbers, little-endian, in the shape")],
            run: from_bytes,
        }),
    ]
}

/// A value's bytes (the module's rules).
pub fn bytes_of(value: &Data) -> Result<Vec<u8>, NodeError> {
    let out = match value {
        Data::Array(a) => a.bytes.to_vec(),
        Data::Audio(a) => a.bytes(),
        Data::Blob(b) => b.read(MAX_BYTES).map_err(|e| NodeError::new(&e.kind, format!("value: {}", e.message)))?,
        Data::Json(serde_json::Value::String(t)) => t.as_bytes().to_vec(),
        Data::Json(v) => serde_json::to_vec(v).map_err(|e| config(format!("value: {e}")))?,
        Data::List(_) => return Err(config("value: one value at a time (a list runs To Bytes once per item)")),
    };
    if out.len() as u64 > MAX_BYTES {
        return Err(config("value: larger than 1 GiB"));
    }
    Ok(out)
}

fn to_bytes(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let value = i.get("value").ok_or_else(|| config("value: wire a value"))?;
    let bytes = bytes_of(value)?;
    let len = bytes.len() as u64;
    Ok(Outputs::from([("bytes".to_owned(), Data::Array(Array::new("|u1", vec![len], bytes)))]))
}

fn array(i: &Inputs, name: &str) -> Result<Array, NodeError> {
    match i.get(name) {
        Some(Data::Array(a)) => Ok(a.clone()),
        _ => Err(config(format!("{name}: connect an array"))),
    }
}

/// The item size of a little-endian (or byte) numeric dtype.
fn item_size(dtype: &str) -> Option<usize> {
    DTYPES.iter().find(|(_, d, _)| *d == dtype).map(|(.., size)| *size).or(match dtype {
        "|b1" => Some(1),
        _ => None,
    })
}

/// Python's `range(len)[start:stop:step]` for a positive step, with stop 0 as the end.
pub fn slice_indices(len: u64, start: i64, stop: i64, step: i64) -> Vec<u64> {
    let len_i = i128::from(len);
    let at = |v: i64| -> i128 {
        let v = i128::from(v);
        (if v < 0 { len_i + v } else { v }).clamp(0, len_i)
    };
    let (from, to) = (at(start), if stop == 0 { len_i } else { at(stop) });
    let step = i128::from(step.max(1));
    let mut out = Vec::new();
    let mut k = from;
    while k < to {
        out.push(k as u64);
        k += step;
    }
    out
}

fn slice(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = array(i, "array")?;
    let size = item_size(&a.dtype).ok_or_else(|| config(format!("array: its type {} isn't one Slice reads", a.dtype)))?;
    let Some((&first, rest)) = a.shape.split_first() else { return Err(config("array: a single number has no first axis to slice")) };
    let row = rest.iter().try_fold(size, |acc, d| usize::try_from(*d).ok().and_then(|d| acc.checked_mul(d))).ok_or_else(|| config("array: too large"))?;
    let need = usize::try_from(first).ok().and_then(|f| f.checked_mul(row)).ok_or_else(|| config("array: too large"))?;
    if a.bytes.len() != need {
        return Err(config(format!("array: its bytes ({}) don't fill its shape {:?} of {}", a.bytes.len(), a.shape, a.dtype)));
    }
    let keep = slice_indices(first, n(i, "start", 0), n(i, "stop", 0), n(i, "step", 1));
    let mut bytes = Vec::with_capacity(keep.len().saturating_mul(row));
    for k in &keep {
        let at = (*k as usize) * row;
        bytes.extend_from_slice(&a.bytes[at..at + row]);
    }
    let mut shape = a.shape.clone();
    shape[0] = keep.len() as u64;
    Ok(Outputs::from([("array".to_owned(), Data::Array(Array { shape, bytes: bytes.into(), ..a }))]))
}

/// A shape's sides from text (`480, 640, 3`; one may be -1), for `count` items.
pub fn parse_shape(text: &str, count: u64) -> Result<Vec<u64>, String> {
    let parts: Vec<&str> = text.split(|c: char| c == ',' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err("give the sides, separated by commas (480, 640, 3), or -1".into());
    }
    if parts.len() > 32 {
        return Err("more than 32 sides".into());
    }
    let mut sides: Vec<Option<u64>> = Vec::new();
    for p in &parts {
        match p.parse::<i64>() {
            Ok(-1) => sides.push(None),
            Ok(v) if v >= 0 => sides.push(Some(v as u64)),
            _ => return Err(format!("{p:?} isn't a side (a whole number, or -1)")),
        }
    }
    if sides.iter().filter(|s| s.is_none()).count() > 1 {
        return Err("only one side may be -1".into());
    }
    let known = sides.iter().flatten().try_fold(1u64, |acc, d| acc.checked_mul(*d)).ok_or("the shape is too large")?;
    let free = if sides.contains(&None) {
        if known == 0 {
            return Err("a side of 0 leaves -1 undefined".into());
        }
        Some(count / known)
    } else {
        None
    };
    Ok(sides.into_iter().map(|s| s.or(free).unwrap_or(0)).collect())
}

fn from_bytes(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let a = array(i, "bytes")?;
    let name = s(i, "dtype", "uint8");
    let (_, dtype, size) = *DTYPES.iter().find(|(n, ..)| *n == name).ok_or_else(|| config(format!("dtype: {name:?} isn't one of the types")))?;
    let big = match s(i, "byte_order", "little").as_str() {
        "little" => false,
        "big" => true,
        o => return Err(config(format!("byte_order: {o:?} isn't little or big"))),
    };
    let drop = match s(i, "extra", "error").as_str() {
        "error" => false,
        "drop" => true,
        o => return Err(config(format!("extra: {o:?} isn't error or drop"))),
    };
    let count = (a.bytes.len() / size) as u64;
    let shape = parse_shape(&s(i, "shape", "-1"), count).map_err(|e| config(format!("shape: {e}")))?;
    let items = shape.iter().try_fold(1u64, |acc, d| acc.checked_mul(*d)).ok_or_else(|| config("shape: too large"))?;
    let need = items.checked_mul(size as u64).filter(|n| *n <= MAX_BYTES).ok_or_else(|| config("shape: larger than 1 GiB"))? as usize;
    if need > a.bytes.len() {
        return Err(config(format!("shape: {shape:?} of {name} needs {need} bytes; there are {}", a.bytes.len())));
    }
    if need < a.bytes.len() && !drop {
        return Err(config(format!(
            "shape: {shape:?} of {name} uses {need} of the {} bytes; set extra to drop to cut the rest, or Slice them first",
            a.bytes.len()
        )));
    }
    let mut bytes = a.bytes[..need].to_vec();
    if big && size > 1 {
        for item in bytes.chunks_exact_mut(size) {
            item.reverse();
        }
    }
    Ok(Outputs::from([("array".to_owned(), Data::Array(Array::new(dtype, shape, bytes)))]))
}
