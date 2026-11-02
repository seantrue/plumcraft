"""AUDIO, a core type (`plumcraft_proto::catalogue::core_types::AUDIO`; maintainer, 2026-10-30:
"Core audio, please"), as the Rust SDK's `Audio`: PCM samples, float32, shaped (frames, channels)
and interleaved, at a sample rate. It crosses as an `md5:nd1:` blob of those samples with
`Form::Audio` (`sample_rate`, `channels`, `frames`, `dtype` `<f4`); a watched or node value
shows as `peaks`, a waveform overview, made the same way as the Rust SDK's `data::peaks`.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

import numpy as np

# The most bytes a peaks rendition carries inline (spec 10.3; `data::PEAKS_BYTES`).
PEAKS_BYTES = 4096


@dataclass(frozen=True, eq=False)
class AudioValue:
    """Audio: `samples` (frames, channels) float32 at `sample_rate` Hz. Make one from a 1-D array
    (one channel) or (frames, channels); the samples are copied to float32."""

    samples: np.ndarray
    sample_rate: int

    def __init__(self, samples: Any, sample_rate: int):
        a = np.asarray(samples, dtype="<f4")
        if a.ndim == 1:
            a = a.reshape(-1, 1)
        if a.ndim != 2 or a.shape[1] < 1:
            raise ValueError(f"audio is (frames, channels), not {a.shape}")
        if not isinstance(sample_rate, int) or isinstance(sample_rate, bool) or sample_rate <= 0:
            raise ValueError(f"a sample rate is a positive int, not {sample_rate!r}")
        object.__setattr__(self, "samples", np.ascontiguousarray(a))
        object.__setattr__(self, "sample_rate", sample_rate)

    @property
    def channels(self) -> int:
        return int(self.samples.shape[1])

    @property
    def frames(self) -> int:
        return int(self.samples.shape[0])

    @property
    def seconds(self) -> float:
        return self.frames / self.sample_rate

    def form(self) -> dict:
        return {"kind": "audio", "sample_rate": self.sample_rate, "channels": self.channels, "frames": self.frames, "dtype": "<f4"}

    def summary(self) -> str:
        return f"audio {self.sample_rate} Hz, {self.channels} ch, {self.seconds:.2f} s"


def peaks_array(a: AudioValue) -> np.ndarray | None:
    """The min and max of each channel over bins of frames, (1, channels, bins, 2) float32, as many
    bins as fit `PEAKS_BYTES` (no more than there are frames)."""
    ch, frames = a.channels, a.frames
    if ch == 0 or frames == 0:
        return None
    bins = max(1, min(PEAKS_BYTES // (ch * 8), frames))
    out = np.empty((1, ch, bins, 2), dtype="<f4")
    for b in range(bins):
        lo = b * frames // bins
        hi = max((b + 1) * frames // bins, lo + 1)
        part = a.samples[lo:hi]
        # NaN ignored, as Rust's f32::min and max do
        out[0, :, b, 0] = np.fmin.reduce(part, axis=0)
        out[0, :, b, 1] = np.fmax.reduce(part, axis=0)
    return out
