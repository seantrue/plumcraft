//! Analysis: peak (dBFS), loudness (integrated LUFS), spectrum (dB per bin), spectrogram (a
//! picture of the sound over time), transients and non-silent regions (tables).

use plumcraft_engine::{Array, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::error::NodeError;
use serde_json::{Value, json};
use soundcraft_dsp::meter::LoudnessMeter;
use soundcraft_dsp::offline::{self, SilenceScan};

use crate::util::{self, finite};
use crate::{ARRAY, FLOAT, INT, TABLE, audio_in, def, float, int, one, op, output};

const FFT_SIZES: &[i64] = &[256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536];
/// The most FFT windows averaged for one spectrum, evenly spread over the audio.
const MAX_WINDOWS: usize = 1024;

pub(crate) fn nodes() -> Vec<Box<dyn Node>> {
    let mut fft = int("fft_size", "FFT size", 2048, 256, 65536);
    fft.choices = Some(FFT_SIZES.iter().map(|n| json!(n)).collect());
    let analysis = |id: &str, display: &str, description: &str, params: Vec<plumcraft_proto::catalogue::Port>, out: (&str, &str)| {
        let mut inputs = vec![audio_in("audio")];
        inputs.extend(params);
        def(id, display, &["Analysis"], description, inputs, vec![output(out.0, out.1)])
    };
    vec![
        op(analysis("audio.peak", "Peak", "The largest sample magnitude over every channel, in dBFS (-144 for silence).", vec![], ("db", FLOAT)), peak),
        op(
            analysis(
                "audio.loudness",
                "Loudness",
                "Integrated loudness (ITU-R BS.1770, EBU R128 gating), in LUFS; -144 when nothing passes the gates.",
                vec![],
                ("lufs", FLOAT),
            ),
            loudness,
        ),
        op(
            analysis(
                "audio.spectrum",
                "Spectrum",
                "Hann-windowed magnitude spectrum of the channels' mean, in dBFS per bin (`fft_size / 2 + 1` bins; bin k is at k × rate / fft_size Hz), averaged over up to 1024 windows spread across the audio. A float32 array.",
                vec![fft],
                ("spectrum", ARRAY),
            ),
            spectrum,
        ),
        op(
            analysis(
                "audio.spectrogram",
                "Spectrogram",
                "The sound as a picture: time across, pitch up on a log scale, level 0 to 1 from `floor` dB to 0 dBFS. A (height, width) <f4 array.",
                vec![
                    {
                        let mut f = int("fft_size", "FFT size", 4096, 256, 65536);
                        f.choices = Some(FFT_SIZES.iter().map(|n| json!(n)).collect());
                        f
                    },
                    int("width", "Width", 1600, 16, 8192),
                    int("height", "Height", 900, 16, 8192),
                    float("floor", "Floor", -80.0, -160.0, -10.0, "dB"),
                    float("low", "Lowest pitch", 40.0, 1.0, 20000.0, "Hz"),
                ],
                ("spectrogram", ARRAY),
            ),
            spectrogram,
        ),
        op(
            analysis(
                "audio.transients",
                "Transients",
                "Onsets (SoundCraft's vibrato-suppressed spectral flux), as a table of times in seconds and frames.",
                vec![float("sensitivity", "Sensitivity", 0.5, 0.0, 1.0, "0..1, higher finds more")],
                ("onsets", TABLE),
            ),
            transients,
        ),
        op(
            analysis(
                "audio.silence",
                "Non-silent regions",
                "The regions louder than the threshold (Strip Silence): start and end in seconds, end exclusive. Gaps shorter than the minimum silence don't split a region; each is padded and overlaps merged.",
                vec![
                    float("threshold_db", "Threshold", -48.0, -144.0, 0.0, "dBFS"),
                    float("min_silence", "Minimum silence", 0.1, 0.0, 60.0, "s"),
                    float("pad", "Padding", 0.0, 0.0, 10.0, "s"),
                ],
                ("regions", TABLE),
            ),
            silence,
        ),
    ]
}

fn peak(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let a = util::audio(i, "audio")?;
    util::tick(cx, 0.0, "measuring")?;
    let db = offline::peak_db(&a.to_channels());
    Ok(one("db", Data::Json(finite(f64::from(db)))))
}

fn loudness(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let a = util::audio(i, "audio")?;
    let ch = a.to_channels();
    let mut meter = LoudnessMeter::new(a.sample_rate as f32, ch.len());
    let frames = a.frames() as usize;
    let mut block: Vec<Vec<f32>> = ch.iter().map(|_| Vec::with_capacity(util::BLOCK)).collect();
    util::blocks(cx, frames, "measuring", |r| {
        for (b, c) in block.iter_mut().zip(&ch) {
            b.clear();
            b.extend_from_slice(c.get(r.clone()).unwrap_or(&[]));
        }
        meter.process(&block, r.len());
    })?;
    Ok(one("lufs", Data::Json(finite(f64::from(meter.integrated_lufs())))))
}

fn spectrum(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let n = util::int(i, "fft_size", 256, 65536)?;
    if !FFT_SIZES.contains(&n) {
        return Err(util::config(format!("fft_size: {n} is not a power of two from 256 to 65536")));
    }
    let n = n as usize;
    let a = util::audio(i, "audio")?;
    let ch = a.to_channels();
    let frames = a.frames() as usize;
    let nch = ch.len() as f32;
    let mono: Vec<f32> = (0..frames).map(|f| ch.iter().map(|c| c.get(f).copied().filter(|v| v.is_finite()).unwrap_or(0.0)).sum::<f32>() / nch).collect();
    // Window starts evenly spread from 0 to the last full window (one zero-padded window when the
    // audio is shorter than the FFT).
    let last = frames.saturating_sub(n);
    let windows = (last / n + 1).min(MAX_WINDOWS);
    let bins = n / 2 + 1;
    let mut sum = vec![0.0f64; bins];
    for w in 0..windows {
        if w.is_multiple_of(16) {
            util::tick(cx, w as f64 / windows as f64, "analysing")?;
        }
        let start = if windows > 1 { last * w / (windows - 1) } else { 0 };
        let db = soundcraft_dsp::spectrum::magnitude_db(mono.get(start..).unwrap_or(&[]), n);
        for (s, d) in sum.iter_mut().zip(db) {
            *s += f64::from(soundcraft_dsp::db_to_gain(d));
        }
    }
    let out: Vec<f32> = sum.iter().map(|s| soundcraft_dsp::gain_to_db((s / windows.max(1) as f64) as f32)).collect();
    let bytes: Vec<u8> = out.iter().flat_map(|v| v.to_le_bytes()).collect();
    Ok(one("spectrum", Data::Array(Array::new("<f4", vec![bins as u64], bytes))))
}

fn spectrogram(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let n = util::int(i, "fft_size", 256, 65536)?;
    if !FFT_SIZES.contains(&n) {
        return Err(util::config(format!("fft_size: {n} is not a power of two from 256 to 65536")));
    }
    let n = n as usize;
    let (width, height) = (util::int(i, "width", 16, 8192)? as usize, util::int(i, "height", 16, 8192)? as usize);
    let floor = util::ranged(i, "floor", -160.0, -10.0)? as f32;
    let low = util::ranged(i, "low", 1.0, 20000.0)?;
    let a = util::audio(i, "audio")?;
    let nyquist = f64::from(a.sample_rate) / 2.0;
    if low >= nyquist {
        return Err(util::config(format!("low: {low} Hz is above the highest pitch here, {nyquist} Hz")));
    }
    let ch = a.to_channels();
    let frames = a.frames() as usize;
    let nch = ch.len() as f32;
    let mono: Vec<f32> = (0..frames).map(|f| ch.iter().map(|c| c.get(f).copied().unwrap_or(0.0)).sum::<f32>() / nch).collect();
    let last = frames.saturating_sub(n);
    let bins = n / 2 + 1;
    // Which FFT bin each row reads: the top row the highest pitch.
    let row_bin: Vec<usize> = (0..height)
        .map(|y| {
            let t = 1.0 - y as f64 / (height - 1).max(1) as f64;
            let hz = low * (nyquist / low).powf(t);
            ((hz / nyquist * (bins - 1) as f64).round() as usize).min(bins - 1)
        })
        .collect();
    let mut out = vec![0.0f32; width * height];
    for x in 0..width {
        if x.is_multiple_of(32) {
            util::tick(cx, x as f64 / width as f64, "analysing")?;
        }
        let start = last * x / (width - 1).max(1);
        let db = soundcraft_dsp::spectrum::magnitude_db(mono.get(start..).unwrap_or_default(), n);
        for (y, bin) in row_bin.iter().enumerate() {
            let level = db.get(*bin).copied().unwrap_or(floor);
            if let Some(v) = out.get_mut(y * width + x) {
                *v = ((level - floor) / -floor).clamp(0.0, 1.0);
            }
        }
    }
    let bytes: Vec<u8> = out.iter().flat_map(|v| v.to_le_bytes()).collect();
    Ok(one("spectrogram", Data::Array(Array::new("<f4", vec![height as u64, width as u64], bytes))))
}

fn transients(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let sens = util::ranged(i, "sensitivity", 0.0, 1.0)? as f32;
    let a = util::audio(i, "audio")?;
    let (ch, rate) = (a.to_channels(), a.sample_rate);
    let at = util::off_thread(cx, "detecting", move || offline::detect_transients(&ch, rate as f32, sens))?;
    let secs: Vec<Value> = at.iter().map(|f| finite(*f as f64 / f64::from(rate))).collect();
    let frames: Vec<Value> = at.iter().map(|f| json!(f)).collect();
    Ok(one("onsets", util::table(vec![("time", FLOAT, secs), ("frame", INT, frames)])))
}

fn silence(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let thr = util::ranged(i, "threshold_db", -144.0, 0.0)? as f32;
    let min = util::ranged(i, "min_silence", 0.0, 60.0)?;
    let pad = util::ranged(i, "pad", 0.0, 10.0)?;
    let a = util::audio(i, "audio")?;
    let rate = f64::from(a.sample_rate);
    let ch = a.to_channels();
    let frames = a.frames() as usize;
    // SoundCraft's non_silent_ranges, block by block.
    let mut scan = SilenceScan::new(thr, (min * rate).round() as u64);
    let mut block: Vec<Vec<f32>> = ch.iter().map(|_| Vec::with_capacity(util::BLOCK)).collect();
    util::blocks(cx, frames, "scanning", |r| {
        for (b, c) in block.iter_mut().zip(&ch) {
            b.clear();
            b.extend_from_slice(c.get(r.clone()).unwrap_or(&[]));
        }
        scan.push(&block);
    })?;
    let padding = (pad * rate).round() as u64;
    let regions = scan.finish(padding, padding);
    let starts = regions.iter().map(|(s, _)| finite(*s as f64 / rate)).collect();
    let ends = regions.iter().map(|(_, e)| finite(*e as f64 / rate)).collect();
    Ok(one("regions", util::table(vec![("start", FLOAT, starts), ("end", FLOAT, ends)])))
}
