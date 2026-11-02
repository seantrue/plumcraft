//! Playback of the watched audio (plan 10.4, F3). The state lives in the UI, not the session:
//! a [`Player`] holds the output, the prepared buffer and the stream, and the panel's Play/Stop
//! and Loop (and the control channel's `ui.audio`) drive it.
//!
//! The clip is played as stereo (mono doubled; more than two channels: the first two) at the
//! device's rate: copied when the rates match, otherwise linearly resampled
//! ([`crate::audio::resample_linear`]). The output is an [`AudioOut`]: the system's through cpal
//! ([`CpalOutput`], feature `audio-out`), none ([`NoOutput`]: "no audio output", Play disabled),
//! or a silent one for tests ([`NullOutput`]). No output device is a state shown, never a panic.

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use serde_json::{Value, json};

use crate::audio::{Clip, resample_linear};

/// What the panel says when there is no output.
pub const NO_OUTPUT: &str = "no audio output";
/// The longest buffer played, in seconds at the device's rate.
pub const MAX_PLAY_SECONDS: usize = 20 * 60;

/// An output device.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub name: String,
    pub rate: u32,
}

/// What the stream and the UI share: the position (frames into the buffer), and whether it is
/// playing and looping. The stream's callback writes the position and clears `playing` at the
/// end of a clip that doesn't loop.
#[derive(Debug, Default)]
pub struct Shared {
    pub pos: AtomicU64,
    pub playing: AtomicBool,
    pub looping: AtomicBool,
}

/// An audio output.
pub trait AudioOut {
    /// The device, or why there is none.
    fn probe(&mut self) -> Result<Device, String>;
    /// Start playing `buf` (interleaved stereo at `rate`) from `shared`'s position. Dropping the
    /// returned stream stops it.
    fn start(&mut self, rate: u32, buf: Arc<Vec<f32>>, shared: Arc<Shared>) -> Result<Box<dyn Any>, String>;
}

/// No output: Play is disabled.
pub struct NoOutput;

impl AudioOut for NoOutput {
    fn probe(&mut self) -> Result<Device, String> {
        Err(NO_OUTPUT.into())
    }

    fn start(&mut self, _rate: u32, _buf: Arc<Vec<f32>>, _shared: Arc<Shared>) -> Result<Box<dyn Any>, String> {
        Err(NO_OUTPUT.into())
    }
}

/// A silent output (tests): it "plays" without a sound and counts its starts; the position
/// only moves when a test calls [`render`].
#[derive(Default)]
pub struct NullOutput {
    pub rate: u32,
    pub starts: Arc<AtomicUsize>,
}

impl AudioOut for NullOutput {
    fn probe(&mut self) -> Result<Device, String> {
        Ok(Device { name: "silent".into(), rate: self.rate })
    }

    fn start(&mut self, _rate: u32, _buf: Arc<Vec<f32>>, _shared: Arc<Shared>) -> Result<Box<dyn Any>, String> {
        self.starts.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(()))
    }
}

/// The system's output: cpal's default host and device (CoreAudio on macOS).
#[cfg(feature = "audio-out")]
pub struct CpalOutput;

#[cfg(feature = "audio-out")]
impl CpalOutput {
    fn device() -> Result<(cpal::Device, cpal::SupportedStreamConfig), String> {
        use cpal::traits::{DeviceTrait, HostTrait};
        let device = cpal::default_host().default_output_device().ok_or(NO_OUTPUT)?;
        let config = device.default_output_config().map_err(|e| format!("{NO_OUTPUT}: {e}"))?;
        Ok((device, config))
    }
}

#[cfg(feature = "audio-out")]
impl AudioOut for CpalOutput {
    fn probe(&mut self) -> Result<Device, String> {
        use cpal::traits::DeviceTrait;
        let (device, config) = Self::device()?;
        let name = device.description().map(|d| d.name().to_owned()).unwrap_or_else(|_| "audio output".into());
        Ok(Device { name, rate: config.sample_rate() })
    }

    fn start(&mut self, rate: u32, buf: Arc<Vec<f32>>, shared: Arc<Shared>) -> Result<Box<dyn Any>, String> {
        use cpal::SampleFormat as F;
        use cpal::traits::StreamTrait;
        let (device, supported) = Self::device()?;
        let mut config = supported.config();
        config.sample_rate = rate;
        let stream = match supported.sample_format() {
            F::F32 => build::<f32>(&device, config, buf, shared),
            F::I16 => build::<i16>(&device, config, buf, shared),
            F::U16 => build::<u16>(&device, config, buf, shared),
            F::I32 => build::<i32>(&device, config, buf, shared),
            F::F64 => build::<f64>(&device, config, buf, shared),
            other => Err(format!("the output's sample format {other} is not supported")),
        }?;
        stream.play().map_err(|e| format!("audio output: {e}"))?;
        Ok(Box::new(stream))
    }
}

#[cfg(feature = "audio-out")]
fn build<T>(device: &cpal::Device, config: cpal::StreamConfig, buf: Arc<Vec<f32>>, shared: Arc<Shared>) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait;
    let channels = usize::from(config.channels);
    let mut scratch: Vec<f32> = Vec::new();
    device
        .build_output_stream::<T, _, _>(
            config,
            move |out: &mut [T], _| {
                if scratch.len() < out.len() {
                    scratch.resize(out.len(), 0.0);
                }
                if let Some(s) = scratch.get_mut(..out.len()) {
                    render(s, channels, &buf, &shared);
                    for (o, v) in out.iter_mut().zip(s.iter()) {
                        *o = T::from_sample(*v);
                    }
                }
            },
            |e| eprintln!("plumcraft: audio output: {e}"),
            None,
        )
        .map_err(|e| format!("audio output: {e}"))
}

/// Fill `out` (`channels` interleaved) from `buf` (interleaved stereo) at `shared`'s position,
/// advancing it: left and right in the first two channels (their mean on a mono device),
/// silence in the rest. At the end it loops or stops (clearing `playing`).
pub fn render(out: &mut [f32], channels: usize, buf: &[f32], shared: &Shared) {
    let frames = buf.len() / 2;
    let mut pos = usize::try_from(shared.pos.load(Ordering::Relaxed)).unwrap_or(usize::MAX);
    let mut playing = shared.playing.load(Ordering::Relaxed);
    let looping = shared.looping.load(Ordering::Relaxed);
    for frame in out.chunks_mut(channels.max(1)) {
        if playing && pos >= frames {
            if looping && frames > 0 {
                pos = 0;
            } else {
                playing = false;
            }
        }
        let (l, r) = if playing {
            let s = (buf.get(pos * 2).copied().unwrap_or(0.0), buf.get(pos * 2 + 1).copied().unwrap_or(0.0));
            pos += 1;
            s
        } else {
            (0.0, 0.0)
        };
        match frame {
            [m] => *m = (l + r) * 0.5,
            [a, b, rest @ ..] => {
                *a = l;
                *b = r;
                rest.fill(0.0);
            }
            [] => {}
        }
    }
    shared.pos.store(pos as u64, Ordering::Relaxed);
    if !playing {
        shared.playing.store(false, Ordering::Relaxed);
    }
}

/// The output the app starts with: the system's (feature `audio-out`), else none.
pub fn default_output() -> Box<dyn AudioOut> {
    #[cfg(feature = "audio-out")]
    {
        Box::new(CpalOutput)
    }
    #[cfg(not(feature = "audio-out"))]
    {
        Box::new(NoOutput)
    }
}

/// The player: an output, the buffer prepared for it, and the stream while playing.
pub struct Player {
    out: Box<dyn AudioOut>,
    /// The device, probed once (on first use).
    device: Option<Result<Device, String>>,
    shared: Arc<Shared>,
    stream: Option<Box<dyn Any>>,
    /// The value's key and its buffer at the device's rate.
    prepared: Option<(String, Arc<Vec<f32>>)>,
    /// The last playback error, shown under the transport.
    pub error: Option<String>,
}

impl Default for Player {
    fn default() -> Self {
        Player::new(default_output())
    }
}

impl Player {
    pub fn new(out: Box<dyn AudioOut>) -> Self {
        Player { out, device: None, shared: Arc::default(), stream: None, prepared: None, error: None }
    }

    /// Use another output (stops playback; the device is probed again).
    pub fn set_output(&mut self, out: Box<dyn AudioOut>) {
        self.stop();
        self.out = out;
        self.device = None;
        self.prepared = None;
    }

    /// The output device (probed the first time), or why there is none.
    pub fn device(&mut self) -> Result<Device, String> {
        let out = &mut self.out;
        self.device.get_or_insert_with(|| out.probe()).clone()
    }

    /// Play `clip` (whose key is `key`) from the start.
    pub fn play(&mut self, key: &str, clip: &Clip) -> Result<(), String> {
        self.stop();
        self.error = None;
        let dev = self.device()?;
        let buf = match &self.prepared {
            Some((k, b)) if k == key => Arc::clone(b),
            _ => {
                let b = Arc::new(resample_linear(&clip.stereo(), clip.rate, dev.rate, MAX_PLAY_SECONDS.saturating_mul(dev.rate as usize)));
                self.prepared = Some((key.to_owned(), Arc::clone(&b)));
                b
            }
        };
        self.shared.pos.store(0, Ordering::Relaxed);
        self.shared.playing.store(true, Ordering::Relaxed);
        match self.out.start(dev.rate, buf, Arc::clone(&self.shared)) {
            Ok(stream) => {
                self.stream = Some(stream);
                Ok(())
            }
            Err(e) => {
                self.shared.playing.store(false, Ordering::Relaxed);
                Err(e)
            }
        }
    }

    /// Stop (dropping the stream silences it at once).
    pub fn stop(&mut self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        self.stream = None;
    }

    /// Every frame: drop the stream of a clip that has played to its end.
    pub fn tick(&mut self) {
        if self.stream.is_some() && !self.shared.playing.load(Ordering::Relaxed) {
            self.stream = None;
        }
    }

    pub fn is_playing(&self) -> bool {
        self.stream.is_some() && self.shared.playing.load(Ordering::Relaxed)
    }

    pub fn looping(&self) -> bool {
        self.shared.looping.load(Ordering::Relaxed)
    }

    pub fn set_looping(&mut self, on: bool) {
        self.shared.looping.store(on, Ordering::Relaxed);
    }

    /// The position in seconds.
    pub fn position_s(&self) -> f64 {
        let rate = match &self.device {
            Some(Ok(d)) if d.rate > 0 => d.rate,
            _ => return 0.0,
        };
        self.shared.pos.load(Ordering::Relaxed) as f64 / f64::from(rate)
    }

    /// Move the position to `seconds`.
    pub fn seek(&mut self, seconds: f64) {
        if let Some(Ok(d)) = &self.device
            && seconds.is_finite()
        {
            self.shared.pos.store((seconds.max(0.0) * f64::from(d.rate)) as u64, Ordering::Relaxed);
        }
    }

    /// The shared state (tests drive [`render`] with it).
    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// The prepared buffer, when there is one.
    pub fn buffer(&self) -> Option<&Arc<Vec<f32>>> {
        self.prepared.as_ref().map(|(_, b)| b)
    }

    /// Playback state, for the control channel.
    pub fn inspect(&self) -> Value {
        let output = match &self.device {
            None => Value::Null,
            Some(Ok(d)) => json!({"name": d.name, "rate": d.rate}),
            Some(Err(e)) => json!({"error": e}),
        };
        json!({"playing": self.is_playing(), "loop": self.looping(), "position": self.position_s(), "output": output, "error": self.error})
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(rate: u32, samples: Vec<f32>, channels: usize) -> Clip {
        Clip { rate, channels, frames: samples.len() / channels, samples }
    }

    #[test]
    fn render_plays_stops_and_loops() {
        let buf = vec![0.1, 0.2, 0.3, 0.4];
        let s = Shared::default();
        s.playing.store(true, Ordering::Relaxed);
        let mut out = vec![9.0; 8];
        render(&mut out, 2, &buf, &s);
        assert_eq!(out, vec![0.1, 0.2, 0.3, 0.4, 0.0, 0.0, 0.0, 0.0], "then silence");
        assert!(!s.playing.load(Ordering::Relaxed), "stopped at the end");
        s.pos.store(0, Ordering::Relaxed);
        s.playing.store(true, Ordering::Relaxed);
        s.looping.store(true, Ordering::Relaxed);
        let mut out = vec![0.0; 3];
        render(&mut out, 1, &buf, &s);
        assert!((out[0] - 0.15).abs() < 1e-6 && (out[1] - 0.35).abs() < 1e-6 && (out[2] - 0.15).abs() < 1e-6, "mono device, looped: {out:?}");
        assert!(s.playing.load(Ordering::Relaxed));
        let mut out = vec![9.0; 4];
        s.pos.store(0, Ordering::Relaxed);
        render(&mut out, 4, &buf, &s);
        assert_eq!(out, vec![0.1, 0.2, 0.0, 0.0], "four channels: the rest silent");
        // nothing to play, or channels 0: no panic
        render(&mut [0.0; 4], 0, &[], &s);
        s.pos.store(u64::MAX, Ordering::Relaxed);
        render(&mut [0.0; 4], 2, &buf, &s);
    }

    #[test]
    fn no_output_disables_play() {
        let mut p = Player::new(Box::new(NoOutput));
        assert_eq!(p.device(), Err(NO_OUTPUT.to_owned()));
        let c = clip(48_000, vec![0.0; 100], 1);
        assert!(p.play("k", &c).is_err());
        assert!(!p.is_playing());
        assert_eq!(p.position_s(), 0.0);
        p.seek(1.0);
        p.stop();
    }

    #[test]
    fn a_silent_output_plays_resampled_stereo() {
        let starts = Arc::new(AtomicUsize::new(0));
        let mut p = Player::new(Box::new(NullOutput { rate: 48_000, starts: Arc::clone(&starts) }));
        let c = clip(24_000, vec![0.5; 2400], 1);
        p.play("a", &c).unwrap();
        assert!(p.is_playing());
        assert_eq!(p.buffer().map(|b| b.len()), Some(4800 * 2), "24 kHz mono to 48 kHz stereo");
        // the callback runs: the position moves, and the end stops it
        let buf = Arc::clone(p.buffer().unwrap());
        let mut out = vec![0.0; 4800 * 2];
        render(&mut out, 2, &buf, p.shared());
        assert!((p.position_s() - 0.1).abs() < 1e-9);
        render(&mut out, 2, &buf, p.shared());
        p.tick();
        assert!(!p.is_playing(), "played to the end");
        // same key: the buffer is reused
        p.play("a", &c).unwrap();
        assert_eq!(starts.load(Ordering::Relaxed), 2);
        p.seek(0.05);
        assert!((p.position_s() - 0.05).abs() < 1e-9);
        p.stop();
        assert!(!p.is_playing());
        assert_eq!(p.inspect()["output"]["rate"], 48_000);
    }
}
