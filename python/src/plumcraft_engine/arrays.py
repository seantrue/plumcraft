"""Arrays as pictures, as `crates/engine/src/arrays.rs` (spec 10.2, 10.4; ported from the research software's
`nodes/arrays.py` and `display.py`): the views an array viewer offers, node thumbnails, and
per-channel stats. The same rules and the same arithmetic as the Rust SDK, so either engine's
arrays look alike; renditions may differ by a rounding step.

An array's colour space is optional (`ArrayValue.space` None): an array with none is plain
numbers, and only the `color` view (or `auto` choosing it for an unnamed 3-channel stack) treats
three channels as RGB.
"""

from __future__ import annotations

from typing import Any

import numpy as np

MAX_VIEWS = 4
BINS = 256
VIEWS = ("auto", "color", "stretch", "clip", "heat", "labels", "angle")
MAX_SIDE = 16384
MAX_CHANNELS = 64


class ArrayValue(np.ndarray):
    """An ndarray with the core ARRAY type's metadata: its colour space (optional), channel or column
    names, and whether it is an (N, k) table. Return one from a node to say what the array is;
    a plain ndarray has none. Arithmetic on it gives plain arrays again (the metadata doesn't
    follow: a channel picked out or a sum is no longer in that space)."""

    space: str | None
    names: tuple[str, ...]
    table: bool

    def __new__(cls, data: Any, space: str | None = None, names: tuple[str, ...] | list[str] = (), table: bool = False) -> ArrayValue:
        obj = np.asarray(data).view(cls)
        obj.space = space
        obj.names = tuple(names)
        obj.table = bool(table)
        return obj

    def __array_finalize__(self, obj: Any) -> None:
        self.space = None
        self.names = ()
        self.table = False


def meta(a: np.ndarray) -> tuple[str | None, tuple[str, ...], bool]:
    """(space, names, table) of an array: an `ArrayValue`'s, else none."""
    if isinstance(a, ArrayValue):
        return a.space, a.names, a.table
    return None, (), False


def layout(a: np.ndarray) -> tuple[int, int, int] | None:
    """(h, w, c) of an array a picture can show: 2-D or 3-D, not a table, at most 16384 a side
    and 64 channels."""
    if meta(a)[2] or a.ndim not in (2, 3):
        return None
    h, w = a.shape[:2]
    c = a.shape[2] if a.ndim == 3 else 1
    ok = 0 < h <= MAX_SIDE and 0 < w <= MAX_SIDE and 0 < c <= MAX_CHANNELS
    return (int(h), int(w), int(c)) if ok else None


def is_integer(a: np.ndarray) -> bool:
    return a.dtype.kind in "uib"


def channel_name(a: np.ndarray, k: int) -> str:
    names = meta(a)[1]
    return names[k] if k < len(names) and names[k] else f"channel {k + 1}"


def _planes(a: np.ndarray, c: int) -> list[np.ndarray]:
    f = np.asarray(a, dtype=np.float32)
    return [f] if c == 1 and f.ndim == 2 else [np.ascontiguousarray(f[:, :, k]) for k in range(c)]


def resolve_view(a: np.ndarray, c: int, mode: str) -> str:
    if mode != "auto":
        return mode
    space, names, _ = meta(a)
    if is_integer(a):
        return "labels"
    if c == 3 and (space is not None or not names):
        return "color"
    f = np.asarray(a, dtype=np.float32)
    finite = f[np.isfinite(f)]
    return "clip" if finite.size == 0 or (finite.min() >= 0 and finite.max() <= 1) else "stretch"


def stretched(p: np.ndarray) -> np.ndarray:
    finite = np.isfinite(p)
    if not finite.any():
        return np.zeros_like(p, dtype=np.float32)
    lo, hi = np.float32(p[finite].min()), np.float32(p[finite].max())
    span = hi - lo if hi > lo else np.float32(1.0)
    out = np.clip((np.where(np.isnan(p), lo, p) - lo) / span, 0, 1)
    return np.where(np.isnan(p), np.float32(0.0), out).astype(np.float32)


def _clip01(p: np.ndarray) -> np.ndarray:
    return np.clip(np.nan_to_num(p, nan=0.0), 0, 1).astype(np.float32)


_TURBO = (
    (0.13572138, 4.6153926, -42.6603226, 132.1310829, -152.9423939, 59.28637977),
    (0.09140261, 2.19418839, 4.84296658, -14.18503377, 4.27729855, 2.82956604),
    (0.1066733, 12.64194608, -60.58204836, 110.36276771, -89.90310913, 27.34824981),
)


def turbo(x: np.ndarray) -> np.ndarray:
    """Google's Turbo colour map, its polynomial approximation; (…, 3) out."""
    x = np.clip(x.astype(np.float64), 0, 1)
    chans = []
    for c in _TURBO:
        v = c[0] + x * (c[1] + x * (c[2] + x * (c[3] + x * (c[4] + x * c[5]))))
        chans.append(np.clip(v, 0, 1).astype(np.float32))
    return np.stack(chans, axis=-1)


def label_colours(p: np.ndarray) -> np.ndarray:
    """A colour per integer label (splitmix64 of the label, as the Rust SDK), negatives black."""
    bad = ~np.isfinite(p) | (p < 0)
    z = np.where(bad, 0, p).astype(np.uint64)
    with np.errstate(over="ignore"):
        z = z + np.uint64(0x9E3779B97F4A7C15)
        z = (z ^ (z >> np.uint64(30))) * np.uint64(0xBF58476D1CE4E5B9)
        z = (z ^ (z >> np.uint64(27))) * np.uint64(0x94D049BB133111EB)
        z = z ^ (z >> np.uint64(31))
    rgb = np.stack([(z >> np.uint64(s)) & np.uint64(0xFF) for s in (0, 8, 16)], axis=-1).astype(np.float32) / 255
    return np.where(bad[..., None], np.float32(0.0), rgb).astype(np.float32)


def hue_rgb(h: np.ndarray) -> np.ndarray:
    """Fully saturated hues (0..1 of a turn) as RGB."""

    def f(n: float) -> np.ndarray:
        k = np.mod(n + h * 6.0, 6.0)
        return np.clip(1.0 - np.clip(np.minimum(k, 4.0 - k), 0, 1), 0, 1)

    return np.stack([f(5.0), f(3.0), f(1.0)], axis=-1).astype(np.float32)


def views(a: np.ndarray, mode: str = "auto", turn: float = 1.0) -> list[tuple[str, np.ndarray]]:
    """(caption, (H, W, 3) 0..1 float32 RGB) pictures `mode` shows for `a`; empty when it isn't a
    picture."""
    lay = layout(a)
    if lay is None or a.dtype.kind not in "uibf":
        return []
    h, w, c = lay
    mode = resolve_view(a, c, mode)
    planes = _planes(a, c)
    if mode == "color" and 1 <= c <= 4:
        return [_color_picture(a, planes, h, w, c)]
    turn = turn if np.isfinite(turn) and turn != 0 else 1.0
    out = []
    for k, p in enumerate(planes[:MAX_VIEWS]):
        if mode == "labels":
            rgb = label_colours(p)
        elif mode == "heat":
            rgb = turbo(stretched(p))
        elif mode == "angle":
            rgb = hue_rgb(np.mod(p / np.float32(turn), 1.0))
        elif mode == "clip":
            rgb = np.repeat(_clip01(p)[..., None], 3, axis=2)
        else:
            rgb = np.repeat(stretched(p)[..., None], 3, axis=2)
        out.append((channel_name(a, k), rgb.astype(np.float32)))
    return out


def _color_picture(a: np.ndarray, planes: list[np.ndarray], h: int, w: int, c: int) -> tuple[str, np.ndarray]:
    space = meta(a)[0]
    if c in (3, 4):
        rgb = np.stack(planes[:3], axis=-1)
        if c == 3 and space is not None and space != "RGB":
            from . import color

            converted = color.to_rgb(space, planes[:3])
            if converted is not None:
                rgb = np.stack(converted, axis=-1)
        rgb = _clip01(rgb)
    else:
        rgb = np.repeat(_clip01(planes[0])[..., None], 3, axis=2)
    if c in (2, 4):
        size = max(8, max(h, w) // 64)
        y, x = np.mgrid[0:h, 0:w]
        check = np.where((y // size + x // size) % 2 == 0, 0.8, 0.6).astype(np.float32)[..., None]
        alpha = _clip01(planes[-1])[..., None]
        rgb = rgb * alpha + check * (1 - alpha)
    caption = space if space is not None else ("grey", "grey + alpha", "RGB", "RGBA")[c - 1]
    return caption, rgb.astype(np.float32)


def rgba8(rgb: np.ndarray, max_px: int) -> np.ndarray:
    """A picture as (h, w, 4) uint8, scaled down (area average) to at most `max_px` a side."""
    h, w = rgb.shape[:2]
    scale = max(max(h, w) / max(max_px, 1), 1.0)
    th, tw = max(round(h / scale), 1), max(round(w / scale), 1)
    # Each cell's mean from a summed-area table: a loop here cost 75 ms a thumbnail. A NaN counts
    # as 0, or it would spread through the sums to every cell after it.
    y0 = np.arange(th) * h // th
    y1 = np.maximum((np.arange(th) + 1) * h // th, y0 + 1)
    x0 = np.arange(tw) * w // tw
    x1 = np.maximum((np.arange(tw) + 1) * w // tw, x0 + 1)
    table = np.zeros((h + 1, w + 1, 3), dtype=np.float64)
    table[1:, 1:] = np.nan_to_num(np.asarray(rgb, dtype=np.float64)).cumsum(axis=0).cumsum(axis=1)
    y = np.ix_(y1, x1)
    total = table[y] - table[np.ix_(y0, x1)] - table[np.ix_(y1, x0)] + table[np.ix_(y0, x0)]
    area = np.outer(y1 - y0, x1 - x0)[..., None]
    out = np.empty((th, tw, 4), dtype=np.uint8)
    out[..., :3] = np.clip(total / area * 255 + 0.5, 0, 255).astype(np.uint8)
    out[..., 3] = 255
    return out


def thumb(a: np.ndarray, px: int) -> np.ndarray | None:
    """A node thumbnail for an array: its first `auto` picture, at most `px` a side."""
    v = views(a, "auto")
    return rgba8(v[0][1], px) if v else None


def stats(a: np.ndarray) -> dict | None:
    """Per-channel stats: finite min and max, the NaN count, and a 256-bin histogram over each
    channel's own finite min..max (a table: per column)."""
    if a.dtype.kind not in "uibf":
        return None
    table = meta(a)[2]
    if (table and a.ndim == 2) or (layout(a) is not None and a.ndim == 3):
        cols = [np.asarray(a[..., k], dtype=np.float32).ravel() for k in range(min(a.shape[-1], MAX_CHANNELS))]
    else:
        cols = [np.asarray(a, dtype=np.float32).ravel()]
    channels = []
    for k, p in enumerate(cols):
        nan = int(np.isnan(p).sum())
        finite = p[np.isfinite(p)].astype(np.float64)
        ch: dict[str, Any] = {"name": channel_name(a, k), "min": None, "max": None}
        if nan:
            ch["nan"] = nan
        if finite.size:
            lo, hi = float(finite.min()), float(finite.max())
            top = hi if hi > lo else lo + 1.0
            idx = np.minimum(((finite - lo) / (top - lo) * BINS).astype(np.int64), BINS - 1)
            counts = np.bincount(idx, minlength=BINS)
            ch.update({"min": lo, "max": hi, "histogram": {"bins": BINS, "lo": lo, "hi": top, "counts": [int(n) for n in counts]}})
        channels.append(ch)
    return {"channels": channels}
