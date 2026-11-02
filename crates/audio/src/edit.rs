//! Edits: gain, normalize, fade, reverse, invert, remove DC, trim, mix, resample, time stretch,
//! pitch shift, biquad filter.

use plumcraft_engine::{Cx, Inputs, Node, Outputs};
use plumcraft_proto::error::NodeError;
use soundcraft_dsp::FadeShape;
use soundcraft_dsp::biquad::{Biquad, FilterType};
use soundcraft_dsp::offline;

use crate::util::{self, config};
use crate::{AUDIO, MAX_RATE, MAX_SECONDS, MIN_RATE, audio_in, boolean, choice, def, float, int, one, op, output};

const SHAPES: &[&str] = &["linear", "equal_power", "s_curve", "exponential", "logarithmic"];
const FILTERS: &[&str] = &["low_pass", "high_pass", "band_pass", "notch", "peak", "low_shelf", "high_shelf", "all_pass", "low_pass_6db", "high_pass_6db"];
const FILTER_TYPES: [FilterType; 10] = [
    FilterType::LowPass,
    FilterType::HighPass,
    FilterType::BandPass,
    FilterType::Notch,
    FilterType::Peak,
    FilterType::LowShelf,
    FilterType::HighShelf,
    FilterType::AllPass,
    FilterType::LowPass1,
    FilterType::HighPass1,
];

fn edit_def(id: &str, display: &str, description: &str, params: Vec<plumcraft_proto::catalogue::Port>) -> plumcraft_proto::catalogue::NodeDef {
    let mut inputs = vec![audio_in("audio")];
    inputs.extend(params);
    def(id, display, &["Edit"], description, inputs, vec![output("audio", AUDIO)])
}

pub(crate) fn nodes() -> Vec<Box<dyn Node>> {
    vec![
        op(edit_def("audio.gain", "Gain", "Changes the level by a number of decibels.", vec![float("db", "Gain", 0.0, -144.0, 96.0, "dB")]), gain),
        op(
            edit_def(
                "audio.normalize",
                "Normalize",
                "Scales the audio so its peak is the target (all channels together, or each on its own). Silence is left alone.",
                vec![float("target_db", "Target peak", -1.0, -144.0, 24.0, "dBFS"), boolean("per_channel", "Per channel", false)],
            ),
            normalize,
        ),
        op(
            edit_def(
                "audio.fade",
                "Fade",
                "Fades in over the first seconds and out over the last.",
                vec![
                    float("fade_in", "Fade in", 0.01, 0.0, MAX_SECONDS, "s"),
                    float("fade_out", "Fade out", 0.01, 0.0, MAX_SECONDS, "s"),
                    choice("shape", "Shape", SHAPES, "equal_power"),
                ],
            ),
            fade,
        ),
        op(edit_def("audio.reverse", "Reverse", "Plays the audio backwards.", vec![]), |i, cx| whole(i, cx, offline::reverse)),
        op(edit_def("audio.invert", "Invert polarity", "Flips the polarity of every sample.", vec![]), |i, cx| whole(i, cx, offline::invert)),
        op(edit_def("audio.remove_dc", "Remove DC", "Subtracts each channel's mean.", vec![]), |i, cx| whole(i, cx, offline::remove_dc)),
        op(
            edit_def(
                "audio.trim",
                "Trim",
                "Keeps the audio from `start` to `end` seconds; an end of 0 keeps it to the end.",
                vec![float("start", "Start", 0.0, 0.0, MAX_SECONDS, "s"), float("end", "End (0 = to the end)", 0.0, 0.0, MAX_SECONDS, "s")],
            ),
            trim,
        ),
        op(
            def(
                "audio.mix",
                "Mix",
                &["Edit"],
                "Adds two signals, each with its gain. Both must have the same sample rate (resample first); a mono input is spread to every channel of the other; the shorter is padded with silence.",
                vec![audio_in("a"), audio_in("b"), float("gain_a", "Gain A", 0.0, -144.0, 24.0, "dB"), float("gain_b", "Gain B", 0.0, -144.0, 24.0, "dB")],
                vec![output("audio", AUDIO)],
            ),
            mix,
        ),
        op(
            edit_def(
                "audio.resample",
                "Resample",
                "Windowed-sinc sample-rate conversion.",
                vec![int("sample_rate", "Sample rate", 48_000, i64::from(MIN_RATE), i64::from(MAX_RATE))],
            ),
            resample,
        ),
        op(
            edit_def(
                "audio.time_stretch",
                "Time stretch",
                "Changes the length without changing the pitch (WSOLA). Ratio is output length over input length.",
                vec![float("ratio", "Ratio", 1.5, 0.1, 10.0, "× length")],
            ),
            stretch,
        ),
        op(
            edit_def(
                "audio.pitch_shift",
                "Pitch shift",
                "Changes the pitch without changing the length.",
                vec![float("semitones", "Semitones", 0.0, -48.0, 48.0, "st")],
            ),
            pitch,
        ),
        op(
            edit_def(
                "audio.biquad",
                "Filter",
                "A second-order (RBJ cookbook) or first-order (6 dB) filter.",
                vec![
                    choice("type", "Type", FILTERS, "low_pass"),
                    float("frequency", "Frequency", 1000.0, 1.0, 188_160.0, "Hz"),
                    float("q", "Q", std::f64::consts::FRAC_1_SQRT_2, 0.025, 100.0, ""),
                    float("gain_db", "Gain (peak, shelves)", 0.0, -48.0, 48.0, "dB"),
                ],
            ),
            biquad,
        ),
    ]
}

/// An in-place whole-buffer edit (linear time; cancel is checked before and after).
fn whole(i: &Inputs, cx: &mut Cx, f: fn(&mut [Vec<f32>])) -> Result<Outputs, NodeError> {
    let a = util::audio(i, "audio")?;
    util::tick(cx, 0.0, "processing")?;
    let mut ch = a.to_channels();
    f(&mut ch);
    util::tick(cx, 1.0, "processing")?;
    Ok(one("audio", util::emit(a.sample_rate, &ch)?))
}

fn gain(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let db = util::ranged(i, "db", -144.0, 96.0)? as f32;
    let a = util::audio(i, "audio")?;
    let g = soundcraft_dsp::db_to_gain(db);
    let mut ch = a.to_channels();
    let frames = ch.first().map_or(0, Vec::len);
    // offline::gain, block by block (the same multiply).
    util::blocks(cx, frames, "gain", |r| {
        for c in ch.iter_mut() {
            if let Some(s) = c.get_mut(r.clone()) {
                s.iter_mut().for_each(|v| *v *= g);
            }
        }
    })?;
    Ok(one("audio", util::emit(a.sample_rate, &ch)?))
}

fn normalize(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let target = util::ranged(i, "target_db", -144.0, 24.0)? as f32;
    let per_channel = util::flag(i, "per_channel")?;
    let a = util::audio(i, "audio")?;
    util::tick(cx, 0.0, "normalizing")?;
    let mut ch = a.to_channels();
    offline::normalize(&mut ch, target, per_channel);
    util::tick(cx, 1.0, "normalizing")?;
    Ok(one("audio", util::emit(a.sample_rate, &ch)?))
}

fn fade(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let fin = util::ranged(i, "fade_in", 0.0, MAX_SECONDS)?;
    let fout = util::ranged(i, "fade_out", 0.0, MAX_SECONDS)?;
    let shape = FadeShape::ALL.get(util::pick(i, "shape", SHAPES)?).copied().unwrap_or_default();
    let a = util::audio(i, "audio")?;
    let rate = f64::from(a.sample_rate);
    util::tick(cx, 0.0, "fading")?;
    let mut ch = a.to_channels();
    offline::fade(&mut ch, (fin * rate).round() as usize, (fout * rate).round() as usize, shape);
    util::tick(cx, 1.0, "fading")?;
    Ok(one("audio", util::emit(a.sample_rate, &ch)?))
}

fn trim(i: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
    let start = util::ranged(i, "start", 0.0, MAX_SECONDS)?;
    let end = util::ranged(i, "end", 0.0, MAX_SECONDS)?;
    let a = util::audio(i, "audio")?;
    let len = a.frames() as usize;
    let rate = f64::from(a.sample_rate);
    let s = (start * rate).round() as usize;
    let e = if end == 0.0 { len } else { ((end * rate).round() as usize).min(len) };
    if s >= e {
        return Err(config(format!(
            "trim from {start} s to {} s leaves no audio (the input is {:.3} s)",
            if end == 0.0 { a.duration_secs() } else { end },
            a.duration_secs()
        )));
    }
    let ch: Vec<Vec<f32>> = a.to_channels().into_iter().map(|c| c.get(s..e).map(<[f32]>::to_vec).unwrap_or_default()).collect();
    Ok(one("audio", util::emit(a.sample_rate, &ch)?))
}

fn mix(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let ga = soundcraft_dsp::db_to_gain(util::ranged(i, "gain_a", -144.0, 24.0)? as f32);
    let gb = soundcraft_dsp::db_to_gain(util::ranged(i, "gain_b", -144.0, 24.0)? as f32);
    let (a, b) = (util::audio(i, "a")?, util::audio(i, "b")?);
    if a.sample_rate != b.sample_rate {
        return Err(config(format!("a is {} Hz and b is {} Hz: resample one first", a.sample_rate, b.sample_rate)));
    }
    let (na, nb) = (usize::from(a.channels), usize::from(b.channels));
    if na != nb && na != 1 && nb != 1 {
        return Err(config(format!("a has {na} channels and b has {nb}: they must match, or one be mono")));
    }
    let channels = na.max(nb);
    let frames = a.frames().max(b.frames()) as usize;
    util::bounds(frames, channels).map_err(config)?;
    let (ca, cb) = (a.to_channels(), b.to_channels());
    let at = |c: &[Vec<f32>], k: usize, f: usize| c.get(if c.len() == 1 { 0 } else { k }).and_then(|v| v.get(f)).copied().unwrap_or(0.0);
    let mut out: Vec<Vec<f32>> = (0..channels).map(|_| Vec::with_capacity(frames)).collect();
    util::blocks(cx, frames, "mixing", |r| {
        for (k, o) in out.iter_mut().enumerate() {
            o.extend(r.clone().map(|f| at(&ca, k, f) * ga + at(&cb, k, f) * gb));
        }
    })?;
    Ok(one("audio", util::emit(a.sample_rate, &out)?))
}

fn resample(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let to = util::int(i, "sample_rate", i64::from(MIN_RATE), i64::from(MAX_RATE))? as u32;
    let a = util::audio(i, "audio")?;
    let from = a.sample_rate;
    util::scaled_frames(a.frames() as usize, f64::from(to) / f64::from(from), usize::from(a.channels))?;
    let ch = a.to_channels();
    let out = util::off_thread(cx, "resampling", move || offline::resample(&ch, from, to))?;
    Ok(one("audio", util::emit(to, &out)?))
}

fn stretch(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let ratio = util::ranged(i, "ratio", 0.1, 10.0)?;
    let a = util::audio(i, "audio")?;
    util::scaled_frames(a.frames() as usize, ratio, usize::from(a.channels))?;
    let (ch, rate) = (a.to_channels(), a.sample_rate);
    let out = util::off_thread(cx, "stretching", move || offline::time_stretch(&ch, ratio, rate as f32))?;
    Ok(one("audio", util::emit(rate, &out)?))
}

fn pitch(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let st = util::ranged(i, "semitones", -48.0, 48.0)? as f32;
    let a = util::audio(i, "audio")?;
    // The intermediate stretch is up to 16× the input (twice by 4× at ±48 semitones).
    util::scaled_frames(a.frames() as usize, 2f64.powf(f64::from(st.abs()) / 12.0), usize::from(a.channels))?;
    let (ch, rate) = (a.to_channels(), a.sample_rate);
    let out = util::off_thread(cx, "shifting", move || offline::pitch_shift(&ch, st, rate as f32))?;
    Ok(one("audio", util::emit(rate, &out)?))
}

fn biquad(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let kind = FILTER_TYPES.get(util::pick(i, "type", FILTERS)?).copied().unwrap_or(FilterType::LowPass);
    let freq = util::ranged(i, "frequency", 1.0, 188_160.0)?;
    let q = util::ranged(i, "q", 0.025, 100.0)? as f32;
    let gain_db = util::ranged(i, "gain_db", -48.0, 48.0)? as f32;
    let a = util::audio(i, "audio")?;
    let rate = a.sample_rate as f32;
    if freq > f64::from(a.sample_rate) * 0.49 {
        return Err(config(format!("frequency: {freq} Hz is above 0.49 × the sample rate ({} Hz)", a.sample_rate)));
    }
    let mut ch = a.to_channels();
    let mut filters: Vec<Biquad> = ch.iter().map(|_| Biquad::design(kind, freq as f32, q, gain_db, rate)).collect();
    let frames = ch.first().map_or(0, Vec::len);
    util::blocks(cx, frames, "filtering", |r| {
        for (c, f) in ch.iter_mut().zip(filters.iter_mut()) {
            if let Some(s) = c.get_mut(r.clone()) {
                f.process_block(s);
            }
        }
    })?;
    Ok(one("audio", util::emit(a.sample_rate, &ch)?))
}
