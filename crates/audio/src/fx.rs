//! SoundCraft's effect plugins as nodes, generated from its registry: `audio.fx.<plugin id>`.
//! Every [`soundcraft_dsp::ParamInfo`] becomes a widget port (FLOAT with its range and default,
//! BOOL for a toggle, ENUM for a choice). Instruments, the signal generator and plugins that are
//! not offline-capable are left out ([`partition`]).
//!
//! Rendering is [`soundcraft_dsp::offline::apply_plugin_with_tail`]'s, block for block (prepare,
//! the tail capped at 60 s, reset, latency compensation, non-finite samples to 0), written out
//! here so the node checks cancel and reports progress between 1024-frame blocks. A test checks
//! the two agree sample for sample on every effect.

use plumcraft_engine::{Cx, Inputs, Node, Outputs};
use plumcraft_proto::catalogue::{NodeDef, Port};
use plumcraft_proto::error::NodeError;
use serde_json::{Value, json};
use soundcraft_dsp::{Category, ParamInfo, Plugin, PluginInfo, Unit};

use crate::util::{self, config, op_error};
use crate::{AUDIO, BOOL, ENUM, FLOAT, audio_in, def, output};

/// SoundCraft's offline block size.
const BLOCK: usize = 1024;
/// The longest tail rendered, in seconds (as `apply_plugin_with_tail`).
const MAX_TAIL_SECS: f32 = 60.0;
/// Blocks between cancel checks (about 0.1 s of audio at 48 kHz).
const CHECK_EVERY: usize = 4;

/// The plugins offered, and those left out with the reason.
pub(crate) fn partition() -> (Vec<&'static str>, Vec<(&'static str, &'static str)>) {
    let mut keep = Vec::new();
    let mut skip = Vec::new();
    for info in soundcraft_dsp::plugins() {
        if let Some(why) = excluded(info) {
            skip.push((info.id, why));
        } else {
            keep.push(info.id);
        }
    }
    (keep, skip)
}

fn excluded(info: &PluginInfo) -> Option<&'static str> {
    if info.is_instrument {
        Some("an instrument: it plays MIDI notes, which an offline graph has none of")
    } else if soundcraft_dsp::is_generator(info.id) {
        Some("a generator: it ignores its input (audio.tone and audio.noise generate)")
    } else if !info.audiosuite {
        Some("not offline-capable in SoundCraft (audiosuite: false)")
    } else {
        None
    }
}

pub(crate) fn nodes() -> Vec<Box<dyn Node>> {
    partition().0.into_iter().filter_map(soundcraft_dsp::plugin_info).map(|info| Box::new(Fx { info }) as Box<dyn Node>).collect()
}

fn category(c: Category) -> &'static str {
    match c {
        Category::Eq => "EQ",
        Category::Dynamics => "Dynamics",
        Category::PitchShift => "Pitch",
        Category::Reverb => "Reverb",
        Category::Delay => "Delay",
        Category::Modulation => "Modulation",
        Category::Harmonic => "Harmonic",
        Category::Dither => "Dither",
        Category::Instrument => "Instrument",
        Category::Other => "Utility",
    }
}

fn unit(u: Unit) -> &'static str {
    match u {
        Unit::Db => "dB",
        Unit::Hz => "Hz",
        Unit::Ms => "ms",
        Unit::Percent => "%",
        Unit::Ratio => ":1",
        Unit::Semitones => "st",
        Unit::Cents => "cents",
        Unit::Seconds => "s",
        Unit::None | Unit::Toggle | Unit::Choice => "",
    }
}

/// The widget port for a parameter.
fn port(p: &ParamInfo) -> Port {
    let base = Port { name: p.id.into(), label: Some(p.name.into()), ..Default::default() };
    match p.unit {
        Unit::Toggle => Port { ty: BOOL.into(), default: json!(p.default >= 0.5), ..base },
        Unit::Choice => Port {
            ty: ENUM.into(),
            default: p.choices.get(p.default.max(0.0) as usize).map_or(Value::Null, |c| json!(c)),
            choices: Some(p.choices.iter().map(|c| json!(c)).collect()),
            ..base
        },
        u => Port {
            ty: FLOAT.into(),
            default: json!(f64::from(p.default)),
            min: Some(f64::from(p.min)),
            max: Some(f64::from(p.max)),
            tooltip: (!unit(u).is_empty()).then(|| unit(u).to_owned()),
            ..base
        },
    }
}

/// A parameter's value from its port, checked: in range for a number, a label or index for a
/// choice, a boolean for a toggle.
fn value(i: &Inputs, p: &ParamInfo) -> Result<f32, NodeError> {
    match p.unit {
        Unit::Toggle => Ok(if util::flag(i, p.id)? { 1.0 } else { 0.0 }),
        Unit::Choice => Ok(util::pick(i, p.id, p.choices)? as f32),
        _ => Ok(util::ranged(i, p.id, f64::from(p.min), f64::from(p.max))? as f32),
    }
}

/// One SoundCraft effect.
struct Fx {
    info: &'static PluginInfo,
}

impl Node for Fx {
    fn def(&self) -> NodeDef {
        let mut inputs = vec![audio_in("audio")];
        inputs.extend(self.info.params.iter().map(port));
        def(
            &format!("audio.fx.{}", self.info.id),
            self.info.name,
            &["Effects", category(self.info.category)],
            &format!("SoundCraft's {} effect, rendered offline with its tail (up to 60 s).", self.info.name),
            inputs,
            vec![output("audio", AUDIO)],
        )
    }

    fn run(&self, i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
        let values: Vec<(&str, f32)> = self.info.params.iter().map(|p| value(i, p).map(|v| (p.id, v))).collect::<Result<_, _>>()?;
        let a = util::audio(i, "audio")?;
        let mut plugin = soundcraft_dsp::create(self.info.id).ok_or_else(|| op_error(format!("SoundCraft has no plugin {:?}", self.info.id)))?;
        for (id, v) in values {
            if !plugin.set_param(id, v) {
                return Err(op_error(format!("{}: SoundCraft refused parameter {id}", self.info.id)));
            }
        }
        let out = render_with_tail(&a.to_channels(), plugin.as_mut(), a.sample_rate as f32, Some(cx))?;
        Ok(crate::one("audio", util::emit(a.sample_rate, &out)?))
    }
}

/// [`soundcraft_dsp::offline::apply_plugin_with_tail`], checking cancel and reporting progress
/// every few blocks when given a context. The output length is checked against the engine's
/// limits before anything is allocated.
pub fn render_with_tail(ch: &[Vec<f32>], plugin: &mut dyn Plugin, sample_rate: f32, mut cx: Option<&mut Cx>) -> Result<Vec<Vec<f32>>, NodeError> {
    let len = ch.iter().map(Vec::len).max().unwrap_or(0);
    let channels = ch.len();
    plugin.prepare(sample_rate, BLOCK, channels);
    let tail = plugin.tail_samples().min((sample_rate * MAX_TAIL_SECS) as usize);
    let out_len = len.checked_add(tail).ok_or_else(|| config("the effect's tail is too long"))?;
    util::bounds(out_len, channels).map_err(|e| config(format!("with its tail, {e}")))?;
    plugin.prepare(sample_rate, BLOCK, channels);
    plugin.reset();
    let lat = plugin.latency().min(crate::MAX_FRAMES);
    let mut out: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0; out_len]).collect();
    let mut scratch: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0; BLOCK]).collect();
    let total = out_len + lat;
    let mut done = 0usize;
    let mut block = 0usize;
    while done < total {
        if let Some(cx) = cx.as_deref_mut()
            && block.is_multiple_of(CHECK_EVERY)
        {
            util::tick(cx, done as f64 / total as f64, "rendering")?;
        }
        block += 1;
        let n = BLOCK.min(total - done);
        for (s, c) in scratch.iter_mut().zip(ch) {
            for (k, v) in s.iter_mut().enumerate() {
                *v = if k < n { c.get(done + k).copied().unwrap_or(0.0) } else { 0.0 };
            }
        }
        plugin.process(&mut scratch, n);
        for (s, o) in scratch.iter().zip(out.iter_mut()) {
            for (k, v) in s.iter().take(n).enumerate() {
                let idx = done + k;
                if idx >= lat
                    && let Some(d) = o.get_mut(idx - lat)
                {
                    *d = if v.is_finite() { *v } else { 0.0 };
                }
            }
        }
        done += n;
    }
    Ok(out)
}
