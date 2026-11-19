//! `plumcraft-media`: values as plain pictures and numbers, made off the shell's thread, with no
//! GUI toolkit (`docs/research/24-native-shells.md` section 4.4, `docs/shell-interface.md`
//! section 10). Every shell sits on it through the editor core: the egui editor uploads what it
//! makes to textures; the C ABI lends it to Swift and Kotlin as bytes.
//!
//! - [`service`]: the media service. A request by value reference answers at once (`Ready`,
//!   `Pending`, `Missing`, `Failed`); the engine's `resolve` and the decoding run on workers, with
//!   a bounded queue, cancellation of what nobody asks for any more, a byte-budgeted cache, and a
//!   wake when something arrives.
//! - [`decode`]: engines' `rgba8` renditions, picture files (bounded), a picture's own numbers,
//!   1-d arrays.
//! - [`svg`]: SVG parsed and rasterised (feature `svg`).
//! - [`audio`]: waveform peaks, decoded clips, spectrograms (feature `spectrogram` for the FFT).
//! - [`raster`]: [`raster::Rgba`], a picture as bytes, and shrinking it.
//!
//! What it does not hold, and why: arrays become pictures in the engine (the engine SDK's
//! `plumcraft_engine::arrays` makes every view, colour map and plane as `rgba8` renditions), so a
//! shell only decodes those; the probe and compare's difference (`probe`, `diff`) stay in the
//! editor core for now, since they read the core's plain geometry and the session's compare
//! modes.
//!
//! Every value an engine sends is hostile input: decoders check sizes before decoding, and
//! nothing here panics on what an engine sends (`tests/hostile.rs`).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod audio;
pub mod decode;
pub mod raster;
pub mod service;
#[cfg(feature = "svg")]
pub mod svg;

pub use raster::Rgba;
pub use service::{Blocking, Config, Fetch, Made, MediaService, Source, Want};
