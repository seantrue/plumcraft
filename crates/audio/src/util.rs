//! Reading parameters and audio as hostile input, bounding outputs, and running long work so a
//! cancel is honoured.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use plumcraft_engine::{Audio, Cx, Data, Inputs};
use plumcraft_proto::error::NodeError;
use serde_json::Value;

use crate::{MAX_CHANNELS, MAX_FRAMES, MAX_RATE, MAX_SAMPLES, MIN_RATE};

/// Frames per block in the loops this crate owns: between blocks a node checks cancel and
/// reports progress.
pub const BLOCK: usize = 1 << 16;

pub fn config(message: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::CONFIG, message)
}

pub fn op_error(message: impl Into<String>) -> NodeError {
    NodeError::new(NodeError::OP, message)
}

fn json<'a>(i: &'a Inputs, name: &str) -> Result<&'a Value, NodeError> {
    match i.get(name) {
        Some(Data::Json(v)) => Ok(v),
        Some(_) => Err(config(format!("{name}: expected a value, got audio or an array"))),
        None => Err(config(format!("{name}: no value"))),
    }
}

/// A finite number.
pub fn num(i: &Inputs, name: &str) -> Result<f64, NodeError> {
    let v = json(i, name)?;
    let f = v.as_f64().ok_or_else(|| config(format!("{name}: expected a number, got {}", short(v))))?;
    if f.is_finite() { Ok(f) } else { Err(config(format!("{name}: not a finite number"))) }
}

/// A finite number in `lo..=hi`.
pub fn ranged(i: &Inputs, name: &str, lo: f64, hi: f64) -> Result<f64, NodeError> {
    let f = num(i, name)?;
    if (lo..=hi).contains(&f) { Ok(f) } else { Err(config(format!("{name}: {f} is outside {lo}..={hi}"))) }
}

/// A whole number in `lo..=hi` (an integral float is accepted).
pub fn int(i: &Inputs, name: &str, lo: i64, hi: i64) -> Result<i64, NodeError> {
    let v = json(i, name)?;
    let n = match v.as_i64() {
        Some(n) => n,
        None => match v.as_f64() {
            Some(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.0e15 => f as i64,
            _ => return Err(config(format!("{name}: expected a whole number, got {}", short(v)))),
        },
    };
    if (lo..=hi).contains(&n) { Ok(n) } else { Err(config(format!("{name}: {n} is outside {lo}..={hi}"))) }
}

/// A boolean (`true`/`false`, or 0/1).
pub fn flag(i: &Inputs, name: &str) -> Result<bool, NodeError> {
    match json(i, name)? {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) if n.as_f64() == Some(0.0) => Ok(false),
        Value::Number(n) if n.as_f64() == Some(1.0) => Ok(true),
        v => Err(config(format!("{name}: expected true or false, got {}", short(v)))),
    }
}

/// The index of a choice, by its label or by index.
pub fn pick(i: &Inputs, name: &str, choices: &[&str]) -> Result<usize, NodeError> {
    let v = json(i, name)?;
    let found = match v {
        Value::String(s) => choices.iter().position(|c| c == s),
        Value::Number(n) => n.as_u64().and_then(|k| usize::try_from(k).ok()).filter(|k| *k < choices.len()),
        _ => None,
    };
    found.ok_or_else(|| config(format!("{name}: {} is not one of {}", short(v), choices.join(", "))))
}

/// A path string, non-empty and without NUL.
pub fn path(i: &Inputs, name: &str) -> Result<String, NodeError> {
    match json(i, name)? {
        Value::String(s) if !s.is_empty() && !s.contains('\0') => Ok(s.clone()),
        Value::String(s) if s.is_empty() => Err(config(format!("{name} is empty"))),
        v => Err(config(format!("{name}: expected a path, got {}", short(v)))),
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 40 { format!("{}…", s.chars().take(40).collect::<String>()) } else { s }
}

/// Audio on an input, checked: channels, rate, length and bounds.
pub fn audio<'a>(i: &'a Inputs, name: &str) -> Result<&'a Audio, NodeError> {
    let a = match i.get(name) {
        Some(Data::Audio(a)) => a,
        Some(_) => return Err(config(format!("{name}: expected audio"))),
        None => return Err(config(format!("{name}: no audio"))),
    };
    check(a).map_err(|e| config(format!("{name}: {e}")))?;
    Ok(a)
}

/// Why audio is unusable, if it is.
pub fn check(a: &Audio) -> Result<(), String> {
    let ch = usize::from(a.channels);
    if ch == 0 || ch > MAX_CHANNELS {
        return Err(format!("{ch} channels (1..={MAX_CHANNELS})"));
    }
    if !(MIN_RATE..=MAX_RATE).contains(&a.sample_rate) {
        return Err(format!("sample rate {} Hz is outside {MIN_RATE}..={MAX_RATE}", a.sample_rate));
    }
    if !a.samples.len().is_multiple_of(ch) {
        return Err(format!("{} samples don't divide into {ch} channels", a.samples.len()));
    }
    bounds(a.samples.len() / ch, ch)
}

/// Whether `frames` × `channels` is within the engine's limits (at least one frame).
pub fn bounds(frames: usize, channels: usize) -> Result<(), String> {
    if frames == 0 {
        return Err("empty audio (no frames)".into());
    }
    if frames > MAX_FRAMES {
        return Err(format!("{frames} frames is longer than the limit ({MAX_FRAMES}, 10 minutes at 192 kHz)"));
    }
    if frames.checked_mul(channels).is_none_or(|n| n > MAX_SAMPLES) {
        return Err(format!("{frames} frames × {channels} channels is more than the limit ({MAX_SAMPLES} samples)"));
    }
    Ok(())
}

/// Frames for a duration at a rate, bounded.
pub fn frames_for(seconds: f64, rate: u32, channels: usize) -> Result<usize, NodeError> {
    let f = (seconds * f64::from(rate)).round();
    if !f.is_finite() || f < 1.0 || f > MAX_FRAMES as f64 {
        return Err(config(format!("{seconds} s at {rate} Hz is {f} frames (1..={MAX_FRAMES})")));
    }
    let frames = f as usize;
    bounds(frames, channels).map_err(config)?;
    Ok(frames)
}

/// Audio from per-channel buffers, checked (an op that would exceed a bound fails as `op`).
pub fn emit(rate: u32, ch: &[Vec<f32>]) -> Result<Data, NodeError> {
    let frames = ch.iter().map(Vec::len).min().unwrap_or(0);
    if ch.is_empty() || ch.len() > MAX_CHANNELS {
        return Err(op_error(format!("the result has {} channels", ch.len())));
    }
    bounds(frames, ch.len()).map_err(|e| op_error(format!("the result: {e}")))?;
    Ok(Data::Audio(Audio::from_channels(rate, ch)))
}

/// The output length for an input of `frames` scaled by `ratio`, checked against the limits.
pub fn scaled_frames(frames: usize, ratio: f64, channels: usize) -> Result<usize, NodeError> {
    let out = (frames as f64 * ratio).round();
    if !out.is_finite() || out < 1.0 || out > MAX_FRAMES as f64 {
        return Err(config(format!("the result would be {out} frames (1..={MAX_FRAMES})")));
    }
    bounds(out as usize, channels).map_err(config)?;
    Ok(out as usize)
}

/// Check cancel; report progress.
pub fn tick(cx: &mut Cx, fraction: f64, stage: &str) -> Result<(), NodeError> {
    if cx.cancelled() {
        return Err(NodeError::cancelled());
    }
    cx.progress(fraction, Some(stage));
    Ok(())
}

/// Run `work` over `0..total` in [`BLOCK`]-sized ranges, checking cancel and reporting progress
/// between them.
pub fn blocks(cx: &mut Cx, total: usize, stage: &str, mut work: impl FnMut(std::ops::Range<usize>)) -> Result<(), NodeError> {
    let mut at = 0;
    while at < total {
        tick(cx, at as f64 / total as f64, stage)?;
        let end = (at + BLOCK).min(total);
        work(at..end);
        at = end;
    }
    tick(cx, 1.0, stage)
}

/// How often a node waiting on a worker checks cancel.
const POLL: Duration = Duration::from_millis(20);

/// Run a whole-buffer function that has no progress or cancel hook (SoundCraft's offline time
/// stretch, pitch shift, resample, transient detection, decode) on a worker thread, polling
/// cancel every 20 ms. On cancel the node returns `cancelled` at once; the worker runs to the end
/// of its bounded input and its result is dropped. Progress is an estimate that rises towards 1
/// (`t / (t + 1 s)`), labelled with `stage`. A panic in the worker is an `op` error, not a crash.
pub fn off_thread<T: Send + 'static>(cx: &mut Cx, stage: &str, work: impl FnOnce() -> T + Send + 'static) -> Result<T, NodeError> {
    if cx.cancelled() {
        return Err(NodeError::cancelled());
    }
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name(format!("plumcraft-audio {stage}"))
        .spawn(move || {
            let _ = tx.send(work());
        })
        .map_err(|e| op_error(format!("{stage}: no worker thread: {e}")))?;
    let start = Instant::now();
    loop {
        match rx.recv_timeout(POLL) {
            Ok(v) => return Ok(v),
            Err(RecvTimeoutError::Timeout) => {
                let t = start.elapsed().as_secs_f64();
                tick(cx, t / (t + 1.0), stage)?;
            }
            Err(RecvTimeoutError::Disconnected) => return Err(op_error(format!("{stage} failed inside SoundCraft (the worker stopped without a result)"))),
        }
    }
}

/// A table value: named typed columns.
pub fn table(columns: Vec<(&str, &str, Vec<Value>)>) -> Data {
    let cols: Vec<Value> = columns.into_iter().map(|(name, ty, values)| serde_json::json!({"name": name, "type": ty, "values": values})).collect();
    Data::Json(serde_json::json!({ "columns": cols }))
}

/// A float for JSON (non-finite becomes the silence floor, -144).
pub fn finite(f: f64) -> Value {
    serde_json::Number::from_f64(if f.is_finite() { f } else { f64::from(soundcraft_dsp::MIN_DB) }).map_or(Value::Null, Value::Number)
}
