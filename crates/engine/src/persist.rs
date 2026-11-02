//! Persisted memo records (spec 14.5): a node slow enough to be worth keeping leaves a record in
//! the commonkit store under its plan key, so a later run (after a restart, or in another engine
//! process on this machine with the same salt) reports it `cached` instead of computing it.
//!
//! A record is JSON, stored compressed (`plum:v1:plummemo:<plan key digest>:c`):
//!
//! ```json
//! {"v": 1, "plan_key": "memo:md5:…", "ms": 2140.5,
//!  "outputs": {"doc":  {"blob":  {"handle": "md5:raw:…", "meta": {BlobMeta}, "publish": true,
//!                                 "thumb": {"handle": "md5:nd1:…", "dtype": "|u1", "shape": [120, 96, 4]}}},
//!              "paths": {"json": 1097},
//!              "codes": {"array": {"handle": "md5:nd1:…", "dtype": "<u4", "shape": [812]}},
//!              "tone":  {"audio": {"handle": "md5:nd1:…", "sample_rate": 48000, "channels": 2}},
//!              "items": {"list":  {"items": [ … ], "axes": [["v.values", 3]], "labels": ["1", "2", "3"]}}}}
//! ```
//!
//! Every value a record names is published first and the record written last, so a record never
//! names a value that isn't stored. Reading is defensive: the record is untrusted input (any
//! process on the machine may write the store), so a record that doesn't parse, names another
//! plan key, nests too deep or names a value that is gone or doesn't match its handle is a miss,
//! never an error (spec 4.6). The Python SDK reads and writes the same records (`persist.py`).

use std::sync::Arc;

use plumcraft_proto::Handle;
use plumcraft_proto::data::BlobMeta;
use plumcraft_store::Store;
use serde_json::{Map, Value, json};

use crate::{Array, Audio, Blob, BlobSource, Data, List, Outputs};

/// The record format.
pub const RECORD_V1: u64 = 1;
/// How deep lists may nest in a record.
const MAX_DEPTH: usize = 16;
/// The most values one record may name.
const MAX_VALUES: usize = 100_000;

/// Persist `outputs` for `plan_key`: publish every value, then write the record. `false` (and no
/// record) when anything can't be stored: a value over the store's read cap, a file that no
/// longer holds its handle, or the store declining.
pub fn save(store: &Store, plan_key: &str, outputs: &Outputs, ms: f64) -> bool {
    let mut n = 0;
    let mut out = Map::new();
    for (port, d) in outputs {
        let Some(v) = to_record(store, d, 0, &mut n) else { return false };
        out.insert(port.clone(), v);
    }
    let record = json!({"v": RECORD_V1, "plan_key": plan_key, "ms": ms, "outputs": out});
    serde_json::to_vec(&record).is_ok_and(|bytes| store.put_record(plan_key, &bytes))
}

/// The outputs a record for `plan_key` holds and what computing them took, every value read back
/// and checked; `None` when there is no record or anything about it is wrong.
pub fn load(store: &Store, plan_key: &str) -> Option<(Outputs, f64)> {
    let bytes = store.read_record(plan_key)?;
    let record: Value = serde_json::from_slice(&bytes).ok()?;
    if record.get("v").and_then(Value::as_u64) != Some(RECORD_V1) || record.get("plan_key").and_then(Value::as_str) != Some(plan_key) {
        return None;
    }
    let mut n = 0;
    let ms = record.get("ms").and_then(Value::as_f64).filter(|m| m.is_finite() && *m >= 0.0).unwrap_or(0.0);
    let outputs: Option<Outputs> =
        record.get("outputs")?.as_object()?.iter().map(|(port, v)| Some((port.clone(), from_record(store, v, 0, &mut n)?))).collect();
    Some((outputs?, ms))
}

/// The bytes `outputs` would write: what persisting costs, against what computing them cost.
pub fn size_of(outputs: &Outputs) -> u64 {
    fn one(d: &Data) -> u64 {
        match d {
            Data::Json(v) => serde_json::to_vec(v).map_or(0, |b| b.len() as u64),
            Data::Array(a) => a.bytes.len() as u64,
            Data::Audio(a) => a.samples.len() as u64 * 4,
            Data::Blob(b) => b.meta.size,
            Data::List(l) => l.items.iter().map(one).sum(),
        }
    }
    outputs.values().map(one).sum()
}

fn to_record(store: &Store, d: &Data, depth: usize, n: &mut usize) -> Option<Value> {
    *n += 1;
    if depth > MAX_DEPTH || *n > MAX_VALUES {
        return None;
    }
    Some(match d {
        Data::Json(v) => json!({"json": v}),
        Data::Array(a) => json!({"array": array_ref(store, a)?}),
        Data::Audio(a) => {
            let bytes: Vec<u8> = a.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
            let frames = a.samples.len() as u64 / u64::from(a.channels.max(1));
            let (h, _) = store.publish_array("<f4", &[frames, u64::from(a.channels)], &bytes)?;
            json!({"audio": {"handle": h.to_string(), "sample_rate": a.sample_rate, "channels": a.channels}})
        }
        Data::Blob(b) => {
            let bytes = match &b.source {
                BlobSource::Bytes(bytes) => bytes.to_vec(),
                BlobSource::File(p) if std::fs::metadata(p).is_ok_and(|m| m.len() <= plumcraft_store::MAX_READ) => std::fs::read(p).ok()?,
                BlobSource::File(_) => return None,
            };
            let (h, _) = store.publish_raw(&bytes).filter(|(h, _)| h == &b.handle)?;
            let thumb = match &b.thumb {
                Some(t) => array_ref(store, t)?,
                None => Value::Null,
            };
            json!({"blob": {"handle": h.to_string(), "meta": serde_json::to_value(&b.meta).ok()?, "publish": b.publish, "thumb": thumb}})
        }
        Data::List(l) => {
            let items = l.items.iter().map(|i| to_record(store, i, depth + 1, n)).collect::<Option<Vec<_>>>()?;
            json!({"list": {"items": items, "axes": l.axes, "labels": l.labels}})
        }
    })
}

fn array_ref(store: &Store, a: &Array) -> Option<Value> {
    let (h, _) = store.publish_array(&a.dtype, &a.shape, &a.bytes)?;
    Some(json!({"handle": h.to_string(), "dtype": a.dtype, "shape": a.shape, "space": a.space, "names": a.names, "table": a.table}))
}

fn from_record(store: &Store, v: &Value, depth: usize, n: &mut usize) -> Option<Data> {
    *n += 1;
    if depth > MAX_DEPTH || *n > MAX_VALUES {
        return None;
    }
    let (kind, body) = v.as_object().filter(|o| o.len() == 1)?.iter().next()?;
    Some(match kind.as_str() {
        "json" => Data::Json(body.clone()),
        "array" => Data::Array(read_array(store, body)?),
        "audio" => {
            let h: Handle = body.get("handle")?.as_str()?.parse().ok()?;
            let channels = u16::try_from(body.get("channels")?.as_u64()?).ok().filter(|c| *c > 0)?;
            let sample_rate = u32::try_from(body.get("sample_rate")?.as_u64()?).ok()?;
            let b = store.read_handle(&h).filter(|b| b.dtype == "<f4" && b.shape.get(1) == Some(&u64::from(channels)))?;
            let samples: Vec<f32> = b.bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
            Data::Audio(Audio { sample_rate, channels, samples: Arc::from(samples) })
        }
        "blob" => {
            let handle: Handle = body.get("handle")?.as_str()?.parse().ok()?;
            let meta: BlobMeta = serde_json::from_value(body.get("meta")?.clone()).ok()?;
            let bytes = store.read_handle(&handle)?.bytes.into_vec();
            let thumb = match body.get("thumb") {
                None | Some(Value::Null) => None,
                Some(t) => Some(read_array(store, t)?),
            };
            let publish = body.get("publish").and_then(Value::as_bool).unwrap_or(true);
            Data::Blob(Box::new(Blob { handle, meta, source: BlobSource::Bytes(Arc::from(bytes)), publish, thumb }))
        }
        "list" => {
            let items = body.get("items")?.as_array()?.iter().map(|i| from_record(store, i, depth + 1, n)).collect::<Option<Vec<_>>>()?;
            let axes: Vec<(String, usize)> = serde_json::from_value(body.get("axes").cloned().unwrap_or(Value::Null)).unwrap_or_default();
            let labels: Vec<String> = serde_json::from_value(body.get("labels").cloned().unwrap_or(Value::Null)).unwrap_or_default();
            Data::List(List { items, axes, labels })
        }
        _ => return None,
    })
}

fn read_array(store: &Store, v: &Value) -> Option<Array> {
    let h: Handle = v.get("handle")?.as_str()?.parse().ok()?;
    let b = store.read_handle(&h)?;
    let dtype = v.get("dtype")?.as_str()?;
    let shape: Vec<u64> = serde_json::from_value(v.get("shape")?.clone()).ok()?;
    let space = v.get("space").and_then(Value::as_str).map(str::to_owned);
    let names: Vec<String> = serde_json::from_value(v.get("names").cloned().unwrap_or(Value::Null)).unwrap_or_default();
    let table = v.get("table").and_then(Value::as_bool).unwrap_or(false);
    (b.dtype == dtype && b.shape == shape).then(|| Array { dtype: b.dtype, shape: b.shape, bytes: Arc::from(b.bytes.into_vec()), space, names, table })
}
