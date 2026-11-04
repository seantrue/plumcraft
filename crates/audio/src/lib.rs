//! The audio engine (plan 10.4, F2): audio nodes on SoundCraft's offline DSP
//! (`soundcraft-dsp`) and file I/O (`soundcraft-audio-io`), served in every mode the SDK serves:
//! compiled in (mode 1), as `plumcraft-audio-engine` on a socket (mode 2) or over HTTP (mode 3).
//!
//! Audio crosses as [`plumcraft_engine::Audio`]: (frames, channels) `<f4` with its sample rate.
//! Every node checks its inputs as hostile: parameters must be finite and in range, audio must
//! have 1..=[`MAX_CHANNELS`] channels, at least one frame, a sample rate in
//! [`MIN_RATE`]..=[`MAX_RATE`], and at most [`MAX_FRAMES`] frames and [`MAX_SAMPLES`] samples.
//! A bad value is a `config` error, never a clamp the user can't see and never a panic.
//!
//! **Cancel and progress.** Loops this crate owns (effects, filters, loudness, spectrum,
//! silence, noise) run in blocks, checking cancel and reporting progress between them.
//! SoundCraft's whole-buffer functions (time stretch, pitch shift, resample, transients,
//! decode, the tone generator) have no callback, so they run on a worker thread while the
//! node polls cancel every 20 ms: a cancel returns at once and the worker's result is dropped
//! when it finishes (see [`util::off_thread`]).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod analysis;
mod edit;
mod fx;
mod generate;
mod io;
pub mod util;

use plumcraft_engine::{Cx, Engine, Inputs, Node, Outputs};
use plumcraft_proto::catalogue::{Media, NodeDef, Port, Socket, WireType};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::session::EngineInfo;
use serde_json::{Value, json};

pub const AUDIO: &str = plumcraft_proto::catalogue::core_types::AUDIO;
pub const FLOAT: &str = "FLOAT";
pub const INT: &str = "INT";
pub const BOOL: &str = "BOOL";
pub const STRING: &str = "STRING";
pub const ENUM: &str = "ENUM";
pub const TABLE: &str = plumcraft_proto::catalogue::core_types::TABLE;
pub const ARRAY: &str = plumcraft_proto::catalogue::core_types::ARRAY;
/// A WAV file as a value, as SoundCraft's pack bounces one.
pub const WAV: &str = "WAV";
/// FLAC and AIFF files as values, as SoundCraft's pack bounces them.
pub const FLAC: &str = "FLAC";
pub const AIFF: &str = "AIFF";

/// The SoundCraft revision this engine is built on; part of the memo salt, so a DSP upgrade
/// never reuses an old result.
pub const SOUNDCRAFT_REV: &str = "40f593a360d67f21022c4206df1781952db37e54";

/// The lowest and highest sample rates accepted anywhere.
pub const MIN_RATE: u32 = 1000;
pub const MAX_RATE: u32 = 384_000;
/// The most channels a value may have.
pub const MAX_CHANNELS: usize = 16;
/// The longest audio: 10 minutes at 192 kHz.
pub const MAX_FRAMES: usize = 600 * 192_000;
/// The most samples (frames × channels) in one value: 10 minutes of stereo at 192 kHz, about
/// 0.9 GB of `f32`.
pub const MAX_SAMPLES: usize = 2 * MAX_FRAMES;
/// The longest generated or edited duration a parameter may ask for, in seconds.
pub const MAX_SECONDS: f64 = 600.0;
/// The largest file `audio.load` reads.
pub const MAX_FILE: u64 = 1 << 30;

/// A demo graph: a tone through a fade, gain and a reverb, with its peak, loudness and spectrum.
pub const DEMO_WORKFLOW: &str = include_str!("../../../apps/plumcraft-editor/demos/audio.json");

/// The engine, ready to host.
pub fn engine() -> Engine {
    let version = env!("CARGO_PKG_VERSION");
    let info = EngineInfo {
        id: "audio".into(),
        name: "Audio".into(),
        version: version.into(),
        salt: plumcraft_engine::salt(&format!("rs:plumcraft-audio+{version}+soundcraft@{SOUNDCRAFT_REV}"), env!("PLUMCRAFT_CODE_DIGEST")),
        ..Default::default()
    };
    let scalar = |id: &str| WireType { id: id.into(), media: Media::Scalar, ..Default::default() };
    let mut e = Engine::new(info)
        .with_wire_type(plumcraft_proto::catalogue::core_types::audio())
        .with_wire_type(scalar(FLOAT))
        .with_wire_type(scalar(INT))
        .with_wire_type(scalar(BOOL))
        .with_wire_type(scalar(STRING))
        .with_wire_type(scalar(ENUM))
        .with_wire_type(plumcraft_proto::catalogue::core_types::table())
        .with_wire_type(plumcraft_proto::catalogue::core_types::array())
        .with_wire_type(WireType { id: WAV.into(), media: Media::Bytes, ..Default::default() })
        .with_wire_type(WireType { id: FLAC.into(), media: Media::Bytes, ..Default::default() })
        .with_wire_type(WireType { id: AIFF.into(), media: Media::Bytes, ..Default::default() });
    for n in nodes() {
        e = e.with_node(n);
    }
    e.with_building_blocks()
}

/// Every node: sources, edits, effects, analysis, output.
pub fn nodes() -> Vec<Box<dyn Node>> {
    let mut v: Vec<Box<dyn Node>> = Vec::new();
    v.extend(io::nodes());
    v.extend(generate::nodes());
    v.extend(edit::nodes());
    v.extend(fx::nodes());
    v.extend(analysis::nodes());
    v
}

/// The SoundCraft plugins offered as `audio.fx.*` nodes, and those left out with the reason.
pub fn effects() -> (Vec<&'static str>, Vec<(&'static str, &'static str)>) {
    fx::partition()
}

/// An effect rendered as its `audio.fx.<id>` node renders it, with default parameters and no
/// cancel: for checking the render against SoundCraft's own `apply_plugin_with_tail`.
pub fn render_effect(id: &str, ch: &[Vec<f32>], sample_rate: f32) -> Option<Vec<Vec<f32>>> {
    let mut p = soundcraft_dsp::create(id)?;
    fx::render_with_tail(ch, p.as_mut(), sample_rate, None).ok()
}

/// A node from its definition and a function.
pub(crate) struct Op {
    pub def: NodeDef,
    pub run: fn(&Inputs, &mut Cx) -> Result<Outputs, NodeError>,
}

impl Node for Op {
    fn def(&self) -> NodeDef {
        self.def.clone()
    }

    fn run(&self, inputs: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
        (self.run)(inputs, cx)
    }
}

/// A node definition in the `audio` family.
pub(crate) fn def(id: &str, display: &str, category: &[&str], description: &str, inputs: Vec<Port>, outputs: Vec<Port>) -> NodeDef {
    NodeDef {
        id: id.into(),
        display: display.into(),
        category: std::iter::once("Audio").chain(category.iter().copied()).map(str::to_owned).collect(),
        description: description.into(),
        inputs,
        outputs,
        family: "audio".into(),
        ..Default::default()
    }
}

pub(crate) fn op(def: NodeDef, run: fn(&Inputs, &mut Cx) -> Result<Outputs, NodeError>) -> Box<dyn Node> {
    Box::new(Op { def, run })
}

/// An audio socket.
pub(crate) fn audio_in(name: &str) -> Port {
    Port { name: name.into(), ty: AUDIO.into(), socket: Some(Socket::Socket), ..Default::default() }
}

pub(crate) fn output(name: &str, ty: &str) -> Port {
    Port { name: name.into(), ty: ty.into(), ..Default::default() }
}

/// A FLOAT widget with its range and a unit for the tooltip.
pub(crate) fn float(name: &str, label: &str, default: f64, min: f64, max: f64, unit: &str) -> Port {
    Port {
        name: name.into(),
        ty: FLOAT.into(),
        label: Some(label.into()),
        tooltip: (!unit.is_empty()).then(|| unit.to_owned()),
        default: json!(default),
        min: Some(min),
        max: Some(max),
        ..Default::default()
    }
}

pub(crate) fn int(name: &str, label: &str, default: i64, min: i64, max: i64) -> Port {
    Port {
        name: name.into(),
        ty: INT.into(),
        label: Some(label.into()),
        default: json!(default),
        min: Some(min as f64),
        max: Some(max as f64),
        ..Default::default()
    }
}

pub(crate) fn boolean(name: &str, label: &str, default: bool) -> Port {
    Port { name: name.into(), ty: BOOL.into(), label: Some(label.into()), default: json!(default), ..Default::default() }
}

pub(crate) fn choice(name: &str, label: &str, choices: &[&str], default: &str) -> Port {
    Port {
        name: name.into(),
        ty: ENUM.into(),
        label: Some(label.into()),
        default: json!(default),
        choices: Some(choices.iter().map(|c| Value::from(*c)).collect()),
        ..Default::default()
    }
}

/// One output.
pub(crate) fn one(name: &str, d: plumcraft_engine::Data) -> Outputs {
    Outputs::from([(name.to_owned(), d)])
}
