//! Audio in the editor (plan 10.4, F3): waveform thumbnails on the canvas, and the Watched
//! panel's audio view (waveform, time axis, spectrogram and playback).
//!
//! - **Peaks.** An engine's `peaks` rendition is a (levels, channels, bins, 2) `<f4` min/max
//!   array, usually inline. [`peaks_of`] decodes it (inline bytes first, else through the
//!   connection) and [`draw_waveform`] draws it, a min/max line per pixel column per channel.
//! - **The full value** is a (frames, channels) `<f4` array with its sample rate
//!   (`Form::Audio`): a [`Clip`]. Once the panel has read it, the waveform is drawn from it.
//! - **The spectrogram** is computed here, from the clip's mono mix: a Hann window,
//!   [`SPEC_FFT`]-point FFT (rustfft), hop [`SPEC_HOP`] (longer when the clip would need more
//!   than [`SPEC_MAX_FRAMES`] columns, so the work is bounded), magnitude in dB through the
//!   theme's ramp. Once per value; off the UI thread when the clip is long.
//! - **Playback** is [`crate::playback`]'s.
//!
//! Every value here is hostile: shapes, sizes and dtypes are checked before a byte is read, and
//! nothing panics on what an engine sends.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use base64::Engine as _;
use egui::{Painter, Pos2, Rect, Stroke, Vec2, pos2};
use plumcraft_proto::data::{DataRef, Form, Locator};
use plumcraft_proto::display::{Display, Rendition};
use plumcraft_store::Blob;
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use serde_json::{Value, json};

use crate::playback::Player;
use crate::run_view::ThumbMiss;
use crate::theme::Tokens;

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
/// A waveform thumbnail's size on a node.
pub const THUMB_SIZE: Vec2 = Vec2::new(120.0, 44.0);
/// Its size on hover.
pub const THUMB_HOVER_SIZE: Vec2 = Vec2::new(280.0, 100.0);

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

/// A peaks rendition's data: decoded inline, else read through the connection.
pub fn peaks_of(r: &DataRef, resolve: &dyn Fn(&DataRef) -> Option<Blob>) -> Result<Peaks, ThumbMiss> {
    let blob = decode_inline(r).or_else(|| resolve(r)).ok_or(ThumbMiss::Missing)?;
    Peaks::from_blob(&blob).map_err(ThumbMiss::Shape)
}

/// The cache key of a data reference: its handle, else its locators.
pub fn key_of(r: &DataRef) -> String {
    r.handle.as_ref().map(ToString::to_string).unwrap_or_else(|| format!("{:?}", r.at))
}

/// Draw `peaks` in `rect`: a lane per channel (at most 8), a min/max line per pixel column,
/// ±1 at the lane's edges (louder is clipped to them). `faded` greys it (stale, gone).
pub fn draw_waveform(p: &Painter, rect: Rect, peaks: &Peaks, tokens: &Tokens, faded: bool) {
    p.rect_filled(rect, f32::from(tokens.radius), tokens.waveform_bg);
    let lanes = peaks.channels.clamp(1, 8);
    let lane_h = rect.height() / lanes as f32;
    let color = if faded { tokens.text_faint } else { tokens.waveform };
    let cols = rect.width().max(1.0) as usize;
    for ch in 0..lanes {
        let top = rect.top() + ch as f32 * lane_h;
        let mid = top + lane_h * 0.5;
        let half = (lane_h * 0.5 - 1.0).max(0.5);
        p.line_segment([pos2(rect.left(), mid), pos2(rect.right(), mid)], Stroke::new(1.0, tokens.separator));
        for col in 0..cols {
            let Some([mn, mx]) = peaks.column(ch, col, cols) else { continue };
            let x = rect.left() + col as f32 + 0.5;
            let (y0, y1) = (mid - mx.clamp(-1.0, 1.0) * half, mid - mn.clamp(-1.0, 1.0) * half);
            p.line_segment([pos2(x, y0 - 0.5), pos2(x, y1 + 0.5)], Stroke::new(1.0, color));
        }
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

    /// The picture: a column per frame, the highest bin on top, the loudest cell at the ramp's
    /// top and [`SPEC_RANGE_DB`] below it at its bottom.
    pub fn image(&self, tokens: &Tokens) -> egui::ColorImage {
        let (w, h) = (self.frames.max(1), self.bins.max(1));
        let top = self.db.iter().copied().filter(|v| v.is_finite()).fold(f32::NEG_INFINITY, f32::max);
        let top = if top.is_finite() { top } else { 0.0 };
        let mut pixels = vec![tokens.ramp(0.0); w * h];
        for (f, row) in self.db.chunks(self.bins.max(1)).enumerate().take(w) {
            for (b, v) in row.iter().enumerate() {
                let t = (v - (top - SPEC_RANGE_DB)) / SPEC_RANGE_DB;
                if let Some(px) = pixels.get_mut((h - 1 - b.min(h - 1)) * w + f) {
                    *px = tokens.ramp(t);
                }
            }
        }
        egui::ColorImage::new([w, h], pixels)
    }
}

/// The spectrogram of `mono` at `rate`: Hann window, `fft`-point FFT, hop `hop` (longer when
/// more than `max_frames` frames would be needed). Shorter than one window: zero-padded.
pub fn spectrogram(mono: &[f32], rate: u32, fft: usize, hop: usize, max_frames: usize) -> Spectro {
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

// ---------- the Watched panel's audio view ----------

/// Where the spectrogram stands.
#[derive(Default)]
enum SpectroState {
    #[default]
    None,
    Pending(Receiver<Spectro>),
    Ready {
        spec: Arc<Spectro>,
        tex: egui::TextureHandle,
    },
    Failed(String),
}

/// The audio view's state: the watched value decoded, its overview, its spectrogram and the
/// player.
#[derive(Default)]
pub struct AudioView {
    /// The value shown (its handle).
    key: Option<String>,
    clip: Option<Arc<Clip>>,
    clip_error: Option<String>,
    /// The clip's overview at the last width drawn.
    overview: Option<(usize, Peaks)>,
    spectro: SpectroState,
    pub player: Player,
}

impl AudioView {
    /// Take the watched display and its full value: decode a new value, and start its
    /// spectrogram. A new value stops playback.
    pub fn sync(&mut self, d: Option<&Display>, full: Option<&Blob>) {
        let full_ref = d.filter(|d| d.media == plumcraft_proto::catalogue::Media::Audio).and_then(|d| d.full.as_ref());
        let key = full_ref.map(key_of);
        if key != self.key {
            self.player.stop();
            *self = AudioView { player: std::mem::take(&mut self.player), key, ..Default::default() };
        }
        if self.clip.is_some() || self.clip_error.is_some() {
            return;
        }
        let (Some(r), Some(blob)) = (full_ref, full) else { return };
        let rate = match &r.meta.form {
            Form::Audio { sample_rate, .. } => *sample_rate,
            _ => {
                self.clip_error = Some("not audio: the value has no sample rate".into());
                return;
            }
        };
        match Clip::from_blob(blob, rate) {
            Ok(clip) => {
                let clip = Arc::new(clip);
                self.clip = Some(Arc::clone(&clip));
                let (tx, rx) = channel();
                let work = move || {
                    let _ = tx.send(spectrogram(&clip.mono(), clip.rate, SPEC_FFT, SPEC_HOP, SPEC_MAX_FRAMES));
                };
                if self.clip.as_ref().is_some_and(|c| c.samples.len() > SPEC_SYNC_SAMPLES) {
                    let _ = std::thread::Builder::new().name("plumcraft-spectrogram".into()).spawn(work);
                } else {
                    work();
                }
                self.spectro = SpectroState::Pending(rx);
            }
            Err(e) => self.clip_error = Some(e),
        }
    }

    /// The decoded value, once read.
    pub fn clip(&self) -> Option<&Arc<Clip>> {
        self.clip.as_ref()
    }

    /// Turn a finished spectrogram into a texture. True while one is still being computed.
    pub fn poll(&mut self, ctx: &egui::Context, tokens: &Tokens) -> bool {
        let SpectroState::Pending(rx) = &self.spectro else { return false };
        match rx.try_recv() {
            Ok(spec) => {
                let tex =
                    ctx.load_texture(format!("plumcraft-spectrogram-{}", self.key.as_deref().unwrap_or("")), spec.image(tokens), egui::TextureOptions::LINEAR);
                self.spectro = SpectroState::Ready { spec: Arc::new(spec), tex };
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                self.spectro = SpectroState::Failed("the spectrogram could not be computed".into());
                false
            }
        }
    }

    /// The spectrogram, once computed and made a texture.
    pub fn spectrogram(&self) -> Option<(&Spectro, &egui::TextureHandle)> {
        match &self.spectro {
            SpectroState::Ready { spec, tex } => Some((spec, tex)),
            _ => None,
        }
    }

    /// Forget the spectrogram's texture (a theme change re-colours it).
    pub fn recolor(&mut self, ctx: &egui::Context, tokens: &Tokens) {
        if let SpectroState::Ready { spec, tex } = &mut self.spectro {
            *tex = ctx.load_texture(tex.name(), spec.image(tokens), egui::TextureOptions::LINEAR);
        }
    }

    /// What the view holds, for the control channel.
    pub fn inspect(&self) -> Value {
        let spectro = match &self.spectro {
            SpectroState::None => json!(null),
            SpectroState::Pending(_) => json!({"state": "pending"}),
            SpectroState::Ready { spec, .. } => json!({"state": "ready", "frames": spec.frames, "bins": spec.bins, "fft": spec.fft, "hop": spec.hop,
                "peakHz": spec.peak_bin().map(|b| spec.bin_hz(b))}),
            SpectroState::Failed(e) => json!({"state": "failed", "error": e}),
        };
        json!({
            "clip": self.clip.as_ref().map(|c| json!({"rate": c.rate, "channels": c.channels, "frames": c.frames, "seconds": c.seconds()})),
            "error": self.clip_error,
            "spectrogram": spectro,
            "playback": self.player.inspect(),
        })
    }

    /// Play or stop the watched value (the panel's button, the control channel's `ui.audio`).
    pub fn set_playing(&mut self, on: bool) -> Result<(), String> {
        if !on {
            self.player.stop();
            return Ok(());
        }
        let (Some(key), Some(clip)) = (self.key.clone(), self.clip.clone()) else { return Err("no audio to play: watch an audio output and run".into()) };
        self.player.play(&key, &clip)
    }

    /// The view: transport, waveform (from `d`'s peaks until the full value is read), time
    /// axis and spectrogram. `missing`: the value's bytes are gone.
    pub fn ui(&mut self, ui: &mut egui::Ui, d: &Display, missing: bool, tokens: &Tokens) {
        let ctx = ui.ctx().clone();
        if self.poll(&ctx, tokens) {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        self.player.tick();
        self.transport(ui, tokens);
        if let Some(e) = &self.clip_error {
            ui.colored_label(tokens.error, e);
        }
        // the waveform: the clip's own overview, else the engine's peaks
        let lanes = self.clip.as_ref().map_or_else(
            || d.renditions.iter().find_map(|r| if let Rendition::Peaks { channels, .. } = r { Some(usize::from(*channels)) } else { None }).unwrap_or(1),
            |c| c.channels,
        );
        let height = (56.0 * lanes.clamp(1, 4) as f32).clamp(80.0, 200.0);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(100.0), height), egui::Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Watched waveform"));
        let cols = rect.width().max(1.0) as usize;
        if let Some(clip) = &self.clip
            && self.overview.as_ref().is_none_or(|(w, _)| *w != cols)
        {
            self.overview = Some((cols, Peaks::from_clip(clip, cols)));
        }
        let painter = ui.painter_at(rect);
        let engine_peaks = || {
            d.renditions
                .iter()
                .find_map(|r| if let Rendition::Peaks { data, .. } = r { decode_inline(data).and_then(|b| Peaks::from_blob(&b).ok()) } else { None })
        };
        match self.overview.as_ref().map(|(_, p)| p.clone()).or_else(engine_peaks) {
            Some(p) => draw_waveform(&painter, rect, &p, tokens, missing),
            None => {
                painter.rect_filled(rect, f32::from(tokens.radius), tokens.waveform_bg);
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, "no waveform", egui::FontId::proportional(11.0), tokens.text_dim);
            }
        }
        let seconds = self.clip.as_ref().map(|c| c.seconds()).or_else(|| d.full.as_ref().and_then(|f| form_seconds(&f.meta.form))).unwrap_or(0.0);
        // a click seeks while playing
        if resp.clicked()
            && let Some(at) = resp.interact_pointer_pos()
            && seconds > 0.0
        {
            self.player.seek(f64::from((at.x - rect.left()) / rect.width().max(1.0)) * seconds);
        }
        if self.player.is_playing() && seconds > 0.0 {
            let x = rect.left() + (self.player.position_s() / seconds).clamp(0.0, 1.0) as f32 * rect.width();
            painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.5, tokens.playhead));
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        time_axis(ui, rect.left(), rect.width(), seconds, tokens);
        ui.add_space(6.0);
        self.spectro_ui(ui, tokens);
    }

    fn transport(&mut self, ui: &mut egui::Ui, tokens: &Tokens) {
        let device = self.player.device();
        let playing = self.player.is_playing();
        let mut toggle = None;
        let mut looping = self.player.looping();
        ui.horizontal(|ui| {
            let can = device.is_ok() && self.clip.is_some();
            let (label, tip) = if playing { ("Stop", "Stop playback") } else { ("Play", "Play the watched audio") };
            let r = ui.add_enabled(can, egui::Button::new(label));
            let r = match (&device, self.clip.is_some()) {
                (Err(e), _) => r.on_disabled_hover_text(e),
                (_, false) => r.on_disabled_hover_text("waiting for the audio's samples"),
                _ => r.on_hover_text(tip),
            };
            if r.clicked() {
                toggle = Some(!playing);
            }
            if ui.selectable_label(looping, "Loop").on_hover_text("Play from the start again at the end").clicked() {
                looping = !looping;
            }
            match &device {
                Err(e) => {
                    ui.colored_label(tokens.text_dim, e);
                }
                Ok(dev) => {
                    let total = self.clip.as_ref().map_or(0.0, |c| c.seconds());
                    let at = if playing { self.player.position_s() } else { 0.0 };
                    ui.label(egui::RichText::new(format!("{at:.2} / {total:.2} s")).monospace().color(tokens.text_dim))
                        .on_hover_text(format!("{} at {} Hz", dev.name, dev.rate));
                }
            }
        });
        self.player.set_looping(looping);
        if let Some(on) = toggle
            && let Err(e) = self.set_playing(on)
        {
            self.player.error = Some(e);
        }
        if let Some(e) = &self.player.error {
            ui.colored_label(tokens.error, e);
        }
    }

    fn spectro_ui(&mut self, ui: &mut egui::Ui, tokens: &Tokens) {
        match &self.spectro {
            SpectroState::None => {
                if self.clip.is_none() && self.clip_error.is_none() {
                    ui.label(egui::RichText::new("The spectrogram needs the audio's samples.").small().color(tokens.text_dim));
                }
            }
            SpectroState::Pending(_) => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0).color(tokens.state_running));
                    ui.label(egui::RichText::new("computing the spectrogram…").small().color(tokens.text_dim));
                });
            }
            SpectroState::Failed(e) => {
                ui.colored_label(tokens.error, e);
            }
            SpectroState::Ready { spec, tex } => {
                let nyquist = f64::from(spec.rate) / 2.0;
                ui.label(
                    egui::RichText::new(format!("Spectrogram · {}-point FFT, hop {}, 0–{}", spec.fft, spec.hop, hz(nyquist))).small().color(tokens.text_dim),
                );
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(100.0), 160.0), egui::Sense::hover());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Spectrogram"));
                egui::Image::new((tex.id(), rect.size())).corner_radius(f32::from(tokens.radius)).paint_at(ui, rect);
                let p = ui.painter_at(rect);
                for (frac, text) in [(0.0, hz(nyquist)), (0.5, hz(nyquist / 2.0))] {
                    let y = rect.top() + rect.height() * frac;
                    p.text(pos2(rect.left() + 4.0, y + 2.0), egui::Align2::LEFT_TOP, text, egui::FontId::proportional(10.0), tokens.text);
                }
                if let Some(b) = spec.peak_bin() {
                    resp.on_hover_text(format!("loudest around {}", hz(spec.bin_hz(b))));
                }
            }
        }
    }
}

/// A value's length in seconds from its form.
fn form_seconds(f: &Form) -> Option<f64> {
    match f {
        Form::Audio { sample_rate, frames, .. } if *sample_rate > 0 => Some(*frames as f64 / f64::from(*sample_rate)),
        _ => None,
    }
}

/// `440 Hz`, `12.0 kHz`.
fn hz(f: f64) -> String {
    if f >= 1000.0 { format!("{:.1} kHz", f / 1000.0) } else { format!("{f:.0} Hz") }
}

/// The tick step for `seconds` over `width` points: ticks at least 64 points apart.
pub fn tick_step(seconds: f64, width: f32) -> f64 {
    const STEPS: &[f64] = &[0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];
    let most = (f64::from(width) / 64.0).max(1.0);
    STEPS.iter().copied().find(|s| seconds / s <= most).unwrap_or(600.0)
}

/// A time axis in seconds under a waveform drawn from `left` over `width` points.
fn time_axis(ui: &mut egui::Ui, left: f32, width: f32, seconds: f64, tokens: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 16.0), egui::Sense::hover());
    if !(seconds.is_finite() && seconds > 0.0) {
        return;
    }
    let p = ui.painter_at(rect.expand2(egui::vec2(24.0, 0.0)));
    let step = tick_step(seconds, width);
    let decimals = if step >= 1.0 { 0 } else { (-step.log10()).ceil() as usize };
    let mut t = 0.0;
    let mut n = 0;
    while t <= seconds + step * 1e-6 && n <= 200 {
        let x = left + (t / seconds) as f32 * width;
        p.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.top() + 4.0)], Stroke::new(1.0, tokens.text_faint));
        let align = if n == 0 { egui::Align2::LEFT_TOP } else { egui::Align2::CENTER_TOP };
        p.text(Pos2::new(x, rect.top() + 4.0), align, format!("{t:.decimals$} s"), egui::FontId::proportional(10.0), tokens.text_dim);
        n += 1;
        t = step * n as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::data::BlobMeta;

    fn sine(hz: f32, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames).map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin()).collect()
    }

    fn f4(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn peaks_decode_and_reject_bad_shapes() {
        // 1 level, 2 channels, 3 bins
        let v = [-1.0, 1.0, -0.5, 0.5, 0.0, 0.25, -0.1, 0.1, -0.2, 0.2, -0.3, 0.3];
        let p = Peaks::from_blob(&Blob { dtype: "<f4".into(), shape: vec![1, 2, 3, 2], bytes: (f4(&v)).into() }).unwrap();
        assert_eq!((p.channels, p.bins), (2, 3));
        assert_eq!(p.data[0], [-1.0, 1.0]);
        assert_eq!(p.data[5], [-0.3, 0.3]);
        // two columns over three bins: the first spans bin 0, the second bins 1 and 2
        assert_eq!(p.column(0, 0, 2), Some([-1.0, 1.0]));
        assert_eq!(p.column(0, 1, 2), Some([-0.5, 0.5]));
        // more columns than bins: the nearest bin
        assert_eq!(p.column(1, 9, 10), Some([-0.3, 0.3]));
        assert_eq!(p.column(2, 0, 2), None, "no third channel");
        assert_eq!(p.column(0, 0, 0), None);
        for bad in [
            Blob { dtype: "<f8".into(), shape: vec![1, 2, 3, 2], bytes: (vec![0; 96]).into() },
            Blob { dtype: "<f4".into(), shape: vec![1, 2, 3], bytes: (vec![0; 24]).into() },
            Blob { dtype: "<f4".into(), shape: vec![1, 2, 3, 2], bytes: (vec![0; 47]).into() },
            Blob { dtype: "<f4".into(), shape: vec![1, 0, 3, 2], bytes: (vec![]).into() },
            Blob { dtype: "<f4".into(), shape: vec![u64::MAX, u64::MAX, 3, 2], bytes: (vec![]).into() },
            Blob { dtype: "<f4".into(), shape: vec![1, 1, MAX_PEAK_BINS + 1, 2], bytes: (vec![]).into() },
        ] {
            assert!(Peaks::from_blob(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn peaks_come_from_inline_bytes_without_a_store() {
        let v = [-0.5f32, 0.5, -0.25, 0.25];
        let bytes = f4(&v);
        let meta = BlobMeta {
            media: "application/x-npy".into(),
            size: bytes.len() as u64,
            form: Form::Array { dtype: "<f4".into(), shape: vec![1, 1, 2, 2], space: None, names: Vec::new(), table: false },
            aliases: Vec::new(),
        };
        let r = DataRef { handle: None, meta, at: vec![Locator::Inline { b64: base64::engine::general_purpose::STANDARD.encode(&bytes) }] };
        let p = peaks_of(&r, &|_| None).unwrap();
        assert_eq!(p.data, vec![[-0.5, 0.5], [-0.25, 0.25]]);
        // no inline bytes and nothing resolves: gone
        let gone = DataRef { at: vec![], ..r.clone() };
        assert_eq!(peaks_of(&gone, &|_| None), Err(ThumbMiss::Missing));
        // bad base64 is a miss, not a panic
        let bad = DataRef { at: vec![Locator::Inline { b64: "!!!".into() }], ..r };
        assert_eq!(peaks_of(&bad, &|_| None), Err(ThumbMiss::Missing));
    }

    #[test]
    fn clips_decode_and_mix() {
        let b = Blob { dtype: "<f4".into(), shape: vec![2, 2], bytes: (f4(&[1.0, 0.0, 0.5, -0.5])).into() };
        let c = Clip::from_blob(&b, 48_000).unwrap();
        assert_eq!((c.frames, c.channels), (2, 2));
        assert_eq!(c.mono(), vec![0.5, 0.0]);
        assert_eq!(c.stereo(), vec![1.0, 0.0, 0.5, -0.5]);
        let mono = Clip::from_blob(&Blob { dtype: "<f4".into(), shape: vec![2, 1], bytes: (f4(&[0.25, f32::NAN])).into() }, 8000).unwrap();
        assert_eq!(mono.stereo(), vec![0.25, 0.25, 0.0, 0.0], "mono doubled, NaN silenced");
        let three = Clip::from_blob(&Blob { dtype: "<f4".into(), shape: vec![1, 3], bytes: (f4(&[0.1, 0.2, 0.3])).into() }, 8000).unwrap();
        assert_eq!(three.stereo(), vec![0.1, 0.2], "the first two channels");
        assert!(Clip::from_blob(&b, 0).is_err());
        assert!(Clip::from_blob(&Blob { dtype: "<f4".into(), shape: vec![3, 2], bytes: (f4(&[0.0; 4])).into() }, 8000).is_err());
        assert!(Clip::from_blob(&Blob { dtype: "<f4".into(), shape: vec![u64::MAX, 2], bytes: (vec![]).into() }, 8000).is_err());
        let p = Peaks::from_clip(&c, 1);
        assert_eq!(p.data, vec![[0.5, 1.0], [-0.5, 0.0]]);
    }

    #[test]
    fn a_1khz_sine_peaks_in_its_bin() {
        let rate = 48_000;
        let s = spectrogram(&sine(1000.0, rate, rate as usize), rate, 1024, 256, SPEC_MAX_FRAMES);
        assert_eq!(s.bins, 513);
        assert_eq!(s.hop, 256);
        assert_eq!(s.frames, (48_000 - 1024) / 256 + 1);
        let expect: f64 = 1000.0 / (48_000.0 / 1024.0); // 21.33
        let peak = s.peak_bin().unwrap();
        assert!((peak as f64 - expect).abs() <= 1.0, "peak bin {peak}, expected ≈ {expect}");
        // a full-scale sine reads near 0 dB at its peak
        let top = s.db.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!((-3.0..=0.5).contains(&top), "{top} dB");
        let img = s.image(&crate::theme::DEFAULT);
        assert_eq!(img.size, [s.frames, 513]);
    }

    #[test]
    fn spectrogram_work_is_bounded() {
        // ten minutes at 48 kHz would be 112 000 frames at hop 256: the hop grows instead
        let long = vec![0.0f32; 48_000 * 600];
        let s = spectrogram(&long, 48_000, 1024, 256, SPEC_MAX_FRAMES);
        assert!(s.frames <= SPEC_MAX_FRAMES && s.frames > SPEC_MAX_FRAMES / 2, "{}", s.frames);
        assert!(s.hop > 256);
        // shorter than a window, empty, or not finite: still a picture, no panic
        for v in [vec![], vec![0.5; 10], vec![f32::NAN; 3000], vec![f32::INFINITY; 2000]] {
            let s = spectrogram(&v, 8000, 1024, 256, SPEC_MAX_FRAMES);
            assert_eq!(s.frames, v.len().saturating_sub(1024) / 256 + 1);
            assert!(s.db.iter().all(|d| d.is_finite()));
            let _ = s.image(&crate::theme::DARK);
        }
    }

    #[test]
    fn the_resampler_is_linear() {
        let st = vec![0.0, 0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0];
        assert_eq!(resample_linear(&st, 48_000, 48_000, usize::MAX), st, "same rate: a copy");
        let up = resample_linear(&st, 1, 2, usize::MAX);
        assert_eq!(up.len(), 16);
        assert_eq!(&up[..6], &[0.0, 0.0, 0.5, -0.5, 1.0, -1.0]);
        // the last frame holds (no reading past the end)
        assert_eq!(&up[14..], &[3.0, -3.0]);
        let down = resample_linear(&st, 2, 1, usize::MAX);
        assert_eq!(down, vec![0.0, 0.0, 2.0, -2.0]);
        // 44.1 kHz to 48 kHz: the length scales
        let one_s = vec![0.0; 44_100 * 2];
        assert_eq!(resample_linear(&one_s, 44_100, 48_000, usize::MAX).len(), 48_000 * 2);
        assert_eq!(resample_linear(&one_s, 44_100, 48_000, 100).len(), 200, "capped");
        assert!(resample_linear(&[], 1, 2, 10).is_empty());
        assert!(resample_linear(&st, 0, 2, 10).is_empty());
        // a sine resampled keeps its frequency
        let s: Vec<f32> = sine(1000.0, 44_100, 44_100).iter().flat_map(|v| [*v, *v]).collect();
        let r = resample_linear(&s, 44_100, 48_000, usize::MAX);
        let mono: Vec<f32> = r.chunks(2).map(|f| f[0]).collect();
        let spec = spectrogram(&mono, 48_000, 1024, 256, SPEC_MAX_FRAMES);
        assert!((spec.peak_bin().unwrap() as f64 - 21.33).abs() <= 1.0);
    }

    #[test]
    fn ticks_stay_apart() {
        assert_eq!(tick_step(2.0, 300.0), 0.5);
        assert_eq!(tick_step(0.01, 300.0), 0.005);
        assert_eq!(tick_step(1e9, 300.0), 600.0);
        assert!(tick_step(600.0, 300.0) >= 120.0);
    }

    #[test]
    fn hostile_views_draw() {
        let ctx = egui::Context::default();
        let tokens = crate::theme::DEFAULT;
        let mut view = AudioView::default();
        let mut d = Display { media: plumcraft_proto::catalogue::Media::Audio, ..Default::default() };
        d.renditions.push(Rendition::Peaks {
            data: DataRef { handle: None, meta: BlobMeta::default(), at: vec![Locator::Inline { b64: "AAAA".into() }] },
            bins: 9,
            channels: 9,
        });
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            view.ui(ui, &d, false, &tokens);
            let p = Peaks { channels: 3, bins: 2, data: vec![[f32::NAN, f32::INFINITY]; 2] };
            draw_waveform(ui.painter(), Rect::from_min_size(Pos2::ZERO, egui::vec2(0.0, 0.0)), &p, &tokens, false);
            time_axis(ui, 0.0, 10.0, f64::NAN, &tokens);
            time_axis(ui, 0.0, 0.0, 1e12, &tokens);
        });
        out.textures_delta.clear();
    }
}
