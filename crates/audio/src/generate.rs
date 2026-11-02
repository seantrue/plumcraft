//! Generators: `audio.tone` (SoundCraft's signal generator) and `audio.noise` (seeded white noise).

use plumcraft_engine::{Cx, Inputs, Node, Outputs};
use plumcraft_proto::error::NodeError;
use soundcraft_dsp::Waveform;

use crate::util::{self, config};
use crate::{AUDIO, MAX_CHANNELS, MAX_RATE, MAX_SECONDS, MIN_RATE, choice, def, float, int, one, op, output};

const WAVEFORMS: &[&str] = &["sine", "square", "saw", "triangle"];

fn common() -> Vec<plumcraft_proto::catalogue::Port> {
    vec![
        float("level_db", "Level", -6.0, -144.0, 0.0, "dBFS peak"),
        float("seconds", "Seconds", 1.0, 0.001, MAX_SECONDS, "s"),
        int("sample_rate", "Sample rate", 48_000, i64::from(MIN_RATE), i64::from(MAX_RATE)),
        int("channels", "Channels", 1, 1, MAX_CHANNELS as i64),
    ]
}

pub(crate) fn nodes() -> Vec<Box<dyn Node>> {
    let mut tone_in = vec![choice("waveform", "Waveform", WAVEFORMS, "sine"), float("frequency", "Frequency", 440.0, 0.0, 192_000.0, "Hz")];
    tone_in.extend(common());
    let mut noise_in = vec![int("seed", "Seed", 1, 0, 1 << 53)];
    noise_in.extend(common());
    vec![
        op(
            def(
                "audio.tone",
                "Tone",
                &["Sources"],
                "A band-limited test tone (sine, square, saw, triangle) peaking at the level, the same on every channel.",
                tone_in,
                vec![output("audio", AUDIO)],
            ),
            tone,
        ),
        op(
            def(
                "audio.noise",
                "White noise",
                &["Sources"],
                "Uniform white noise peaking at or below the level; each channel its own stream. The same seed gives the same noise.",
                noise_in,
                vec![output("audio", AUDIO)],
            ),
            noise,
        ),
    ]
}

/// Level, seconds, rate and channels, checked: (gain dB, rate, frames, channels).
fn shape(i: &Inputs) -> Result<(f32, u32, usize, usize), NodeError> {
    let level = util::ranged(i, "level_db", -144.0, 0.0)? as f32;
    let seconds = util::ranged(i, "seconds", 0.0, MAX_SECONDS)?;
    let rate = util::int(i, "sample_rate", i64::from(MIN_RATE), i64::from(MAX_RATE))? as u32;
    let channels = util::int(i, "channels", 1, MAX_CHANNELS as i64)? as usize;
    let frames = util::frames_for(seconds, rate, channels)?;
    Ok((level, rate, frames, channels))
}

fn tone(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let kind = Waveform::from_index(util::pick(i, "waveform", WAVEFORMS)?);
    let freq = util::ranged(i, "frequency", 0.0, 192_000.0)?;
    let (level, rate, frames, channels) = shape(i)?;
    if freq >= f64::from(rate) * 0.49 {
        return Err(config(format!("frequency: {freq} Hz is at or above 0.49 × the sample rate ({rate} Hz)")));
    }
    let ch = util::off_thread(cx, "generating", move || soundcraft_dsp::offline::signal_generator(kind, freq as f32, level, rate as f32, frames, channels))?;
    Ok(one("audio", util::emit(rate, &ch)?))
}

/// SplitMix64: a small, fast, well-mixed generator; deterministic across platforms.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [-1, 1), from the top 24 bits so every value is exact in `f32`.
    fn sample(&mut self) -> f32 {
        ((self.next() >> 40) as f32 / (1u64 << 23) as f32) - 1.0
    }
}

fn noise(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let seed = util::int(i, "seed", 0, 1 << 53)? as u64;
    let (level, rate, frames, channels) = shape(i)?;
    let g = soundcraft_dsp::db_to_gain(level);
    let mut ch: Vec<Vec<f32>> = (0..channels).map(|_| Vec::with_capacity(frames)).collect();
    let mut rngs: Vec<SplitMix> = (0..channels as u64).map(|c| SplitMix(seed ^ c.wrapping_mul(0xD1B5_4A32_D192_ED03))).collect();
    util::blocks(cx, frames, "generating", |r| {
        for (c, rng) in ch.iter_mut().zip(rngs.iter_mut()) {
            c.extend((0..r.len()).map(|_| rng.sample() * g));
        }
    })?;
    Ok(one("audio", util::emit(rate, &ch)?))
}
