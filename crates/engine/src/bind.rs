//! External bindings (`RunRequest.inputs`, spec 7.6 rule 10): `{"node.port": PortValue}` values
//! that replace a port's wire or widget. A coordinator feeds one engine's results to another
//! this way; a client can feed a node a value it already has.
//!
//! A bound value is read like a wired one: a json value as is; a blob by its handle from this
//! engine's store, else from its own locators (a `commonkit` key or a path inside the store, or
//! inline bytes), always checked against the handle; a list item by item. Its memo identity is
//! its content (spec 14.2): the handle for a blob, the value for json.

use std::collections::BTreeMap;
use std::sync::Arc;

use plumcraft_proto::PortValue;
use plumcraft_proto::data::{DataRef, Form};
use plumcraft_proto::error::NodeError;
use plumcraft_store::Store;
use serde_json::{Value, json};

use crate::{Array, Audio, Blob, BlobSource, Data, List};

/// The most nested lists a binding may hold.
const MAX_DEPTH: usize = 8;
/// The most values (items included) one binding may hold.
const MAX_VALUES: usize = 100_000;

/// The bindings by node, then port. A key without a `.`, or naming a node the graph lacks, is
/// dropped (spec 18: hostile input is skipped, not fatal).
pub fn by_node(inputs: &BTreeMap<String, PortValue>, has_node: impl Fn(&str) -> bool) -> BTreeMap<String, BTreeMap<String, &PortValue>> {
    let mut out: BTreeMap<String, BTreeMap<String, &PortValue>> = BTreeMap::new();
    for (key, v) in inputs {
        // Port names never hold a dot; node ids might.
        let Some((node, port)) = key.rsplit_once('.') else { continue };
        if node.is_empty() || port.is_empty() || !has_node(node) {
            continue;
        }
        out.entry(node.to_owned()).or_default().insert(port.to_owned(), v);
    }
    out
}

/// A bound value's memo identity: its content.
pub fn identity(v: &PortValue) -> Value {
    match v {
        PortValue::Blob { data } => data.handle.as_ref().map_or(Value::Null, |h| Value::from(h.to_string())),
        PortValue::Json { value } => json!({"json": value}),
        PortValue::List { items } => Value::Array(items.iter().map(identity).collect()),
        PortValue::Opaque { ty, .. } => json!({"opaque": ty}),
    }
}

/// A bound value as the node receives it.
pub fn to_data(store: Option<&Store>, trusted: &[String], port: &str, v: &PortValue) -> Result<Data, NodeError> {
    let mut n = 0;
    read(store, trusted, port, v, 0, &mut n)
}

fn read(store: Option<&Store>, trusted: &[String], port: &str, v: &PortValue, depth: usize, n: &mut usize) -> Result<Data, NodeError> {
    *n += 1;
    if depth > MAX_DEPTH || *n > MAX_VALUES {
        return Err(NodeError::new(NodeError::CONFIG, format!("input {port:?}: the bound list is too large or too deep")));
    }
    match v {
        PortValue::Json { value } => Ok(Data::Json(value.clone())),
        PortValue::List { items } => Ok(Data::List(List::new(items.iter().map(|i| read(store, trusted, port, i, depth + 1, n)).collect::<Result<_, _>>()?))),
        PortValue::Blob { data } => blob(store, trusted, port, data),
        PortValue::Opaque { ty, .. } => Err(NodeError::new(NodeError::CONFIG, format!("input {port:?}: a {ty} value stays in the engine that made it"))),
    }
}

fn blob(store: Option<&Store>, trusted: &[String], port: &str, r: &DataRef) -> Result<Data, NodeError> {
    let Some(handle) = &r.handle else {
        return Err(NodeError::new(NodeError::CONFIG, format!("input {port:?}: a bound blob needs a handle")));
    };
    let bytes = store
        .and_then(|s| s.read_handle(handle).or_else(|| s.resolve_in(r, true, trusted)))
        .or_else(|| plumcraft_store::read_inline(r))
        // a value too large to read is mapped and not hashed, which a node's input can't skip
        .filter(|b| plumcraft_store::verifies(handle, &b.dtype, &b.shape, &b.bytes))
        .ok_or_else(|| NodeError::new(NodeError::MISSING_SOURCE, format!("input {port:?}: {handle} is not in this engine's store")))?;
    let text = |b: Vec<u8>| String::from_utf8(b).map_err(|_| NodeError::new(NodeError::CONFIG, format!("input {port:?}: {handle} is not UTF-8 text")));
    let parsed = |b: &[u8]| serde_json::from_slice(b).map_err(|e| NodeError::new(NodeError::CONFIG, format!("input {port:?}: {handle} is not JSON ({e})")));
    Ok(match &r.meta.form {
        Form::Array { space, names, table, .. } => Data::Array(Array {
            dtype: bytes.dtype,
            shape: bytes.shape,
            bytes: Arc::from(bytes.bytes.into_vec()),
            space: space.clone(),
            names: names.clone(),
            table: *table,
        }),
        Form::Audio { sample_rate, channels, .. } => {
            if bytes.dtype != "<f4" || bytes.shape.get(1) != Some(&u64::from(*channels)) || *channels == 0 {
                return Err(NodeError::new(
                    NodeError::CONFIG,
                    format!("input {port:?}: {handle} is not ({}, {channels}) <f4 audio", bytes.shape.first().unwrap_or(&0)),
                ));
            }
            let samples: Vec<f32> = bytes.bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
            Data::Audio(Audio { sample_rate: *sample_rate, channels: *channels, samples: Arc::from(samples) })
        }
        Form::Text { .. } => Data::Json(Value::from(text(bytes.bytes.into_vec())?)),
        Form::Json | Form::Table { .. } => Data::Json(parsed(&bytes.bytes)?),
        _ => Data::Blob(Box::new(Blob {
            handle: handle.clone(),
            meta: r.meta.clone(),
            source: BlobSource::Bytes(Arc::from(bytes.bytes.into_vec())),
            publish: true,
            thumb: None,
        })),
    })
}
