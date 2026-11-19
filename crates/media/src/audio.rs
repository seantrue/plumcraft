//! Audio as plain data (plan 10.4, F3): waveform overviews, decoded clips and spectrograms.
//! Moved from `plumcraft-ui`'s `audio` (stage N1, slice 6); the egui shell draws them.
//!
//! - **Peaks.** An engine's `peaks` rendition is a (levels, channels, bins, 2) `<f4` min/max
//!   array, usually inline: [`Peaks::from_blob`], [`decode_inline`].
//! - **The full value** is a (frames, channels) `<f4` array with its sample rate
//!   (`Form::Audio`): a [`Clip`].
//! - **The spectrogram** ([`spectrogram`], feature `spectrogram`): from the clip's mono mix, a
//!   Hann window, [`SPEC_FFT`]-point FFT (rustfft), hop [`SPEC_HOP`] (longer when the clip would
//!   need more than [`SPEC_MAX_FRAMES`] columns, so the work is bounded), magnitude in dB.
//!   [`Spectro::ramp_positions`] says where each pixel falls on a colour ramp, so every shell
//!   colours it the same way with its own theme.
//!
//! Every value here is hostile: shapes, sizes and dtypes are checked before a byte is read, and
//! nothing panics on what an engine sends.

use base64::Engine as _;
use plumcraft_proto::data::{DataRef, Form, Locator};
use plumcraft_store::Blob;

use crate::raster::Rgba;

/// The most channels drawn or decoded.
pub const MAX_CHANNELS: usize = 64;
/// The most bins a peaks rendition may have.
pub const MAX_PEAK_BINS: u64 = 1 << 16;
/// The largest inline payload decoded (base64 characters).
pub const MAX_INLINE_B64: usize = 1 << 22;
/// The spectrogram's FFT size and hop.
pub const SPEC_FFT: usize = 1024;
pub const SPEC_HOP: usize = 256;
/// The most spectrogram columns (FFT frames) computed: a longer clip gets a longer hop.
pub const SPEC_MAX_FRAMES: usize = 2048;
/// The dB range the ramp spans, below the loudest cell.
pub const SPEC_RANGE_DB: f32 = 90.0;
/// Clips with more samples than this have their spectrogram computed off the UI thread
/// (about 50 ms of work and more).
pub const SPEC_SYNC_SAMPLES: usize = 1 << 20;

// ---------- peaks ----------

/// A waveform overview: per channel, a (min, max) per bin.
#[derive(Debug, Clone, PartialEq)]
pub struct Peaks {
    pub channels: usize,
    pub bins: usize,
    /// Channel-major: `data[ch * bins + bin]`.
    pub data: Vec<[f32; 2]>,
}

impl Peaks {
    /// A (levels, channels, bins, 2) `<f4` blob's first level.
    pub fn from_blob(b: &Blob) -> Result<Peaks, String> {
        if b.dtype != "<f4" {
            return Err(format!("dtype {}, not <f4", b.dtype));
        }
        let [levels, ch, bins, 2] = b.shape.as_slice() else { return Err(format!("shape {:?}, not (levels, channels, bins, 2)", b.shape)) };
        if *levels == 0 || *ch == 0 || *ch > MAX_CHANNELS as u64 || *bins == 0 || *bins > MAX_PEAK_BINS {
            return Err(format!("{ch} channels × {bins} bins"));
        }
        let need = levels.checked_mul(*ch).and_then(|n| n.checked_mul(*bins)).and_then(|n| n.checked_mul(8));
        if need != Some(b.bytes.len() as u64) {
            return Err(format!("{} bytes for shape {:?}", b.bytes.len(), b.shape));
        }
        let (channels, bins) = (*ch as usize, *bins as usize);
        let floats = b.bytes.as_chunks::<4>().0.iter().take(channels * bins * 2).map(|c| f32::from_le_bytes(*c));
        let flat: Vec<f32> = floats.collect();
        let data = flat.as_chunks::<2>().0.to_vec();
        Ok(Peaks { channels, bins, data })
    }

    /// A clip's overview at `bins` bins.
    pub fn from_clip(clip: &Clip, bins: usize) -> Peaks {
        let (ch, frames) = (clip.channels, clip.frames);
        let bins = bins.clamp(1, frames.max(1));
        let mut data = Vec::with_capacity(ch * bins);
        for c in 0..ch {
            for b in 0..bins {
                let (lo, hi) = (b * frames / bins, ((b + 1) * frames / bins).max(b * frames / bins + 1));
                let (mut mn, mut mx) = (f32::INFINITY, f32::NEG_INFINITY);
                for f in lo..hi.min(frames) {
                    let s = clip.samples.get(f * ch + c).copied().unwrap_or(0.0);
                    if s.is_finite() {
                        mn = mn.min(s);
                        mx = mx.max(s);
                    }
                }
                data.push([mn, mx]);
            }
        }
        Peaks { channels: ch, bins, data }
    }

    /// Channel `ch`'s (min, max) over pixel column `col` of `cols`: the bins the column covers,
    /// or the nearest when there are fewer bins than columns. `None` when nothing finite.
    pub fn column(&self, ch: usize, col: usize, cols: usize) -> Option<[f32; 2]> {
        if cols == 0 || ch >= self.channels || self.bins == 0 {
            return None;
        }
        let lo = col * self.bins / cols;
        let hi = ((col + 1) * self.bins / cols).max(lo + 1).min(self.bins);
        let lane = self.data.get(ch * self.bins..(ch + 1) * self.bins)?;
        let (mn, mx) = lane.get(lo..hi)?.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), [m, n]| (a.min(*m), b.max(*n)));
        (mn.is_finite() && mx.is_finite()).then_some([mn, mx])
    }

    /// Its size in bytes (what a cache counts).
    pub fn bytes(&self) -> usize {
        self.data.len() * 8 + 16
    }
}

/// An inline locator's bytes, as the blob its `Form` describes (no store needed).
pub fn decode_inline(r: &DataRef) -> Option<Blob> {
    let b64 = r.at.iter().find_map(|l| match l {
        Locator::Inline { b64 } => Some(b64),
        _ => None,
    })?;
    if b64.len() > MAX_INLINE_B64 {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    match &r.meta.form {
        Form::Array { dtype, shape, .. } => Some(Blob { dtype: dtype.clone(), shape: shape.clone(), bytes: bytes.into() }),
        Form::Audio { dtype, channels, frames, .. } => Some(Blob { dtype: dtype.clone(), shape: vec![*frames, u64::from(*channels)], bytes: bytes.into() }),
        _ => None,
    }
}

// ---------- the full value ----------

/// Decoded audio: interleaved `f32` samples and their rate.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub rate: u32,
    pub channels: usize,
    pub frames: usize,
    pub samples: Vec<f32>,
}

impl Clip {
    /// A (frames, channels) `<f4` blob at `rate`.
    pub fn from_blob(b: &Blob, rate: u32) -> Result<Clip, String> {
        if b.dtype != "<f4" {
            return Err(format!("dtype {}, not <f4", b.dtype));
        }
        let [frames, ch] = b.shape.as_slice() else { return Err(format!("shape {:?}, not (frames, channels)", b.shape)) };
        if *ch == 0 || *ch > MAX_CHANNELS as u64 {
            return Err(format!("{ch} channels"));
        }
        if !(1..=1_000_000).contains(&rate) {
            return Err(format!("sample rate {rate}"));
        }
        if frames.checked_mul(*ch).and_then(|n| n.checked_mul(4)) != Some(b.bytes.len() as u64) {
            return Err(format!("{} bytes for shape {:?}", b.bytes.len(), b.shape));
        }
        let samples: Vec<f32> = b.bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
        Ok(Clip { rate, channels: *ch as usize, frames: *frames as usize, samples })
    }

    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.rate.max(1))
    }

    /// The channels' mean, a sample per frame.
    pub fn mono(&self) -> Vec<f32> {
        let ch = self.channels.max(1);
        self.samples.chunks(ch).map(|f| f.iter().map(|s| if s.is_finite() { *s } else { 0.0 }).sum::<f32>() / ch as f32).collect()
    }

    /// Interleaved stereo: mono doubled, more than two channels cut to the first two.
    pub fn stereo(&self) -> Vec<f32> {
        let ch = self.channels.max(1);
        let mut out = Vec::with_capacity(self.frames.saturating_mul(2));
        for f in self.samples.chunks(ch) {
            let l = f.first().copied().unwrap_or(0.0);
            let r = if ch == 1 { l } else { f.get(1).copied().unwrap_or(0.0) };
            out.extend([l, r].map(|s| if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 }));
        }
        out
    }
}

/// Linear resampling of interleaved stereo from `from` Hz to `to` Hz (at most `max_frames`
/// frames out). The same rate is a copy.
pub fn resample_linear(stereo: &[f32], from: u32, to: u32, max_frames: usize) -> Vec<f32> {
    let frames = stereo.len() / 2;
    if from == 0 || to == 0 || frames == 0 {
        return Vec::new();
    }
    if from == to {
        return stereo.get(..frames.min(max_frames) * 2).unwrap_or_default().to_vec();
    }
    let out_frames = ((frames as u128 * u128::from(to)) / u128::from(from)).min(max_frames as u128) as usize;
    let step = f64::from(from) / f64::from(to);
    let mut out = Vec::with_capacity(out_frames * 2);
    for i in 0..out_frames {
        let t = i as f64 * step;
        let i0 = (t.floor() as usize).min(frames - 1);
        let i1 = (i0 + 1).min(frames - 1);
        let frac = (t - i0 as f64) as f32;
        for c in 0..2 {
            let a = stereo.get(i0 * 2 + c).copied().unwrap_or(0.0);
            let b = stereo.get(i1 * 2 + c).copied().unwrap_or(0.0);
            out.push(a + (b - a) * frac);
        }
    }
    out
}

// ---------- the spectrogram ----------

/// Magnitudes in dB: `db[frame * bins + bin]`, bin 0 at DC.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectro {
    pub frames: usize,
    pub bins: usize,
    pub fft: usize,
    pub hop: usize,
    pub rate: u32,
    pub db: Vec<f32>,
}

impl Spectro {
    /// The bin with the most energy over the whole clip.
    pub fn peak_bin(&self) -> Option<usize> {
        let mut sums = vec![0.0f64; self.bins];
        for row in self.db.chunks(self.bins.max(1)) {
            for (s, v) in sums.iter_mut().zip(row) {
                *s += 10f64.powf(f64::from(*v) / 10.0);
            }
        }
        sums.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(i, _)| i)
    }

    /// A bin's centre frequency in Hz.
    pub fn bin_hz(&self, bin: usize) -> f64 {
        bin as f64 * f64::from(self.rate) / self.fft.max(1) as f64
    }

    /// The picture's size, `(w, h)`: a column per frame, a row per bin.
    pub fn size(&self) -> (usize, usize) {
        (self.frames.max(1), self.bins.max(1))
    }

    /// Where each pixel of the picture falls on the colour ramp, row-major: a column per frame,
    /// the highest bin on top, the loudest cell at 1 and [`SPEC_RANGE_DB`] below it at 0. A pixel
    /// with no cell is `None` (the ramp's foot, as 0.0 is). `ramp_positions().len() == w * h`.
    pub fn ramp_positions(&self) -> Vec<Option<f32>> {
        let (w, h) = self.size();
        let top = self.db.iter().copied().filter(|v| v.is_finite()).fold(f32::NEG_INFINITY, f32::max);
        let top = if top.is_finite() { top } else { 0.0 };
        let mut out = vec![None; w * h];
        for (f, row) in self.db.chunks(self.bins.max(1)).enumerate().take(w) {
            for (b, v) in row.iter().enumerate() {
                let t = (v - (top - SPEC_RANGE_DB)) / SPEC_RANGE_DB;
                if let Some(px) = out.get_mut((h - 1 - b.min(h - 1)) * w + f) {
                    *px = Some(t);
                }
            }
        }
        out
    }

    /// The picture, coloured by `ramp` (a position 0..1, possibly outside it, to straight-alpha
    /// RGBA).
    pub fn rgba(&self, ramp: &dyn Fn(f32) -> [u8; 4]) -> Rgba {
        let (w, h) = self.size();
        let foot = ramp(0.0);
        let pixels = self.ramp_positions().into_iter().flat_map(|t| t.map_or(foot, ramp)).collect();
        Rgba { w: w as u32, h: h as u32, pixels }
    }
}

/// The spectrogram of `mono` at `rate`: Hann window, `fft`-point FFT, hop `hop` (longer when
/// more than `max_frames` frames would be needed). Shorter than one window: zero-padded.
#[cfg(feature = "spectrogram")]
pub fn spectrogram(mono: &[f32], rate: u32, fft: usize, hop: usize, max_frames: usize) -> Spectro {
    use rustfft::FftPlanner;
    use rustfft::num_complex::Complex;
    let fft = fft.clamp(16, 1 << 16);
    let max_frames = max_frames.max(2);
    let span = mono.len().saturating_sub(fft);
    let mut hop = hop.max(1);
    if span / hop + 1 > max_frames {
        hop = span.div_ceil(max_frames - 1).max(1);
    }
    let frames = (span / hop + 1).min(max_frames);
    let bins = fft / 2 + 1;
    let window: Vec<f32> = (0..fft).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / fft as f32).cos()).collect();
    // a full-scale sine reads 0 dB
    let gain = 2.0 / window.iter().sum::<f32>().max(f32::EPSILON);
    let plan = FftPlanner::<f32>::new().plan_fft_forward(fft);
    let mut buf = vec![Complex::new(0.0f32, 0.0); fft];
    let mut scratch = vec![Complex::new(0.0f32, 0.0); plan.get_inplace_scratch_len()];
    let mut db = Vec::with_capacity(frames * bins);
    for f in 0..frames {
        let start = f * hop;
        for (i, (c, w)) in buf.iter_mut().zip(&window).enumerate() {
            let s = mono.get(start + i).copied().filter(|s| s.is_finite()).unwrap_or(0.0);
            *c = Complex::new(s * w, 0.0);
        }
        plan.process_with_scratch(&mut buf, &mut scratch);
        db.extend(buf.iter().take(bins).map(|c| 20.0 * (c.norm() * gain).max(1e-10).log10()));
    }
    Spectro { frames, bins, fft, hop, rate, db }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "spectrogram")]
    #[test]
    fn a_spectrogram_picture_has_a_pixel_per_cell_and_survives_junk() {
        for v in [vec![], vec![0.5; 10], vec![f32::NAN; 3000], (0..9000).map(|i| (i as f32 * 0.3).sin()).collect()] {
            let s = spectrogram(&v, 8000, 1024, 256, SPEC_MAX_FRAMES);
            let (w, h) = s.size();
            let img = s.rgba(&|t| [(t.clamp(0.0, 1.0) * 255.0) as u8, 0, 0, 255]);
            assert_eq!((img.w as usize, img.h as usize, img.pixels.len()), (w, h, w * h * 4));
        }
    }

    #[test]
    fn hostile_inline_bytes_are_a_miss_not_a_panic() {
        use plumcraft_proto::data::BlobMeta;
        let meta = BlobMeta {
            form: Form::Array { dtype: "<f4".into(), shape: vec![1, 1, 2, 2], space: None, names: Vec::new(), table: false },
            ..BlobMeta::default()
        };
        for b64 in ["!!!", "", "AAAA", &"A".repeat(MAX_INLINE_B64 + 4)] {
            let r = DataRef { handle: None, meta: meta.clone(), at: vec![Locator::Inline { b64: b64.into() }] };
            if let Some(b) = decode_inline(&r) {
                assert!(Peaks::from_blob(&b).is_err(), "{b64:.8}");
            }
        }
        let nothing = DataRef { handle: None, meta, at: vec![] };
        assert!(decode_inline(&nothing).is_none());
    }
}
