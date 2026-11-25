"""Derived views of a value (FR6, research note 30): the Python SDK's port of the Rust crate
`plumcraft-views`, held to it by the shared vectors (`crates/proto/fixtures/views.json`).

A view is a small typed value made from an array, without a node:

- ``histogram``: the finite values counted into ``bins`` bins over ``lo..hi`` (by default their own
  finite min..max), over all values or per plane along an axis: a TABLE (``centre``, ``count``, and
  ``plane`` per plane), its NaN, ±inf and out-of-range counts in ``meta``;
- ``stats``: count, NaN, ±inf, min, max, mean, standard deviation and percentiles of the finite
  values, over all or per plane: a TABLE, one row a plane;
- ``slice``: the 2-D plane of an N-d array along two axes at given indices of the others.

The rules (the Rust crate's module docs): a finite ``x`` in ``lo..=hi`` goes to bin
``floor(((x - lo) / (hi - lo)) * bins)`` in float64, the last bin taking ``hi``; with no range
given ``lo..hi`` is the finite min..max, ``hi = lo + 1`` when they are equal; statistics in float64,
the mean by a compensated (Neumaier) sum, the population standard deviation in two passes,
percentiles by linear interpolation (numpy's default). Every input is from the other side: a bad
view raises ``ViewError`` with the reason, never anything else.
"""

from __future__ import annotations

import math
from typing import Any

import numpy as np

MAX_BINS = 65_536
MAX_PLANES = 65_536
MAX_ROWS = 1 << 22
MAX_PERCENTILES = 32
MAX_RANK = 32
MAX_SORTED = 1 << 28
MAX_SLICE = 1 << 26
KINDS = ("histogram", "stats", "slice")
_DTYPES = {"|u1", "|i1", "|b1", "<u2", "<i2", "<u4", "<i4", "<u8", "<i8", "<f2", "<f4", "<f8"}


class ViewError(ValueError):
    """A view that can't be made, and why."""


def _uint(p: dict, k: str) -> int | None:
    v = p.get(k)
    if v is None:
        return None
    if isinstance(v, bool) or not isinstance(v, int) or v < 0:
        raise ViewError(f"{k} must be a whole number")
    return v


def _float(p: dict, k: str) -> float | None:
    v = p.get(k)
    if v is None:
        return None
    if isinstance(v, bool) or not isinstance(v, (int, float)) or not math.isfinite(v):
        raise ViewError(f"{k} must be a finite number")
    return float(v)


def _per(p: dict) -> int | None:
    v = p.get("per")
    if v is None or v == "all":
        return None
    if isinstance(v, dict):
        a = v.get("axis")
        if isinstance(a, int) and not isinstance(a, bool) and 0 <= a < MAX_RANK:
            return a
    raise ViewError('per is "all" or {axis: k}')


def parse(v: Any) -> dict:
    """A view from its JSON (``{kind, params}``), checked and bounded."""
    if not isinstance(v, dict) or not isinstance(v.get("kind"), str):
        raise ViewError(f"a view has a kind: {', '.join(KINDS)}")
    kind = v["kind"]
    p = v.get("params")
    p = {} if p is None else p
    if not isinstance(p, dict):
        raise ViewError("params is an object")
    allowed = {"histogram": ("bins", "lo", "hi", "per"), "stats": ("per", "percentiles"), "slice": ("axes", "at")}.get(kind)
    if allowed is None:
        raise ViewError(f"no view {kind[:32]!r}: the views are {', '.join(KINDS)}")
    extra = [k for k in p if k not in allowed]
    if extra:
        raise ViewError(f"{kind} takes {', '.join(allowed)}, not {extra[0][:32]!r}")
    if kind == "histogram":
        bins = _uint(p, "bins")
        bins = 256 if bins is None else bins
        if bins == 0 or bins > MAX_BINS:
            raise ViewError(f"bins is 1 to {MAX_BINS}")
        lo, hi = _float(p, "lo"), _float(p, "hi")
        if lo is not None and hi is not None and hi <= lo:
            raise ViewError("hi must be above lo")
        return {"kind": kind, "bins": bins, "lo": lo, "hi": hi, "per": _per(p)}
    if kind == "stats":
        qs = p.get("percentiles") or []
        if not isinstance(qs, list) or len(qs) > MAX_PERCENTILES:
            raise ViewError(f"percentiles is a list of at most {MAX_PERCENTILES} numbers")
        for q in qs:
            if isinstance(q, bool) or not isinstance(q, (int, float)) or not 0 <= q <= 100:
                raise ViewError("a percentile is 0 to 100")
        return {"kind": kind, "per": _per(p), "percentiles": [float(q) for q in qs]}
    axes = p.get("axes")
    if not isinstance(axes, list) or len(axes) != 2:
        raise ViewError("axes is two axes")
    for a in axes:
        if isinstance(a, bool) or not isinstance(a, int) or not 0 <= a < MAX_RANK:
            raise ViewError("an axis is a whole number")
    if axes[0] == axes[1]:
        raise ViewError("the two axes differ")
    at = p.get("at") or []
    if not isinstance(at, list) or len(at) > MAX_RANK:
        raise ViewError("at is a list of indices")
    return {"kind": kind, "axes": [int(a) for a in axes], "at": [int(x) if isinstance(x, int) and not isinstance(x, bool) and x >= 0 else 0 for x in at]}


def _array(dtype: str, shape: list[int], data: bytes) -> np.ndarray:
    if not shape or len(shape) > MAX_RANK:
        raise ViewError(f"an array of {len(shape)} axes (1 to {MAX_RANK})")
    if dtype not in _DTYPES:
        raise ViewError(f"an array of {dtype[:16]!r} isn't numbers")
    n = 1
    for d in shape:
        if not isinstance(d, int) or d < 0:
            raise ViewError("its shape is whole numbers")
        n *= d
    dt = np.dtype(dtype)
    if len(data) != n * dt.itemsize:
        raise ViewError(f"{len(data)} bytes, where {shape} of {dtype} is {n * dt.itemsize}")
    return np.frombuffer(data, dtype=dt).reshape(shape)


def _neumaier(xs: np.ndarray) -> float:
    s, c = 0.0, 0.0
    for x in xs.tolist():
        t = s + x
        if abs(s) >= abs(x):
            c += (s - t) + x
        else:
            c += (x - t) + s
        s = t
    return s + c


def _planes(a: np.ndarray, per: int | None) -> list[np.ndarray]:
    if per is None:
        return [a.astype(np.float64).ravel()]
    if per >= a.ndim:
        raise ViewError(f"no axis {per}: the array has {a.ndim}")
    if a.shape[per] > MAX_PLANES:
        raise ViewError(f"{a.shape[per]} planes along axis {per}, more than {MAX_PLANES}")
    return [np.take(a, p, axis=per).astype(np.float64).ravel() for p in range(a.shape[per])]


def _num(x: float) -> float | None:
    return float(x) if math.isfinite(x) else None


def _histogram(a: np.ndarray, v: dict) -> dict:
    bins, lo, hi, per = v["bins"], v["lo"], v["hi"], v["per"]
    np_ = 1 if per is None else (a.shape[per] if per < a.ndim else 0)
    if per is not None and per >= a.ndim:
        raise ViewError(f"no axis {per}: the array has {a.ndim}")
    if np_ * bins > MAX_ROWS:
        raise ViewError(f"{np_} planes of {bins} bins is more than {MAX_ROWS} rows")
    planes = _planes(a, per)
    flat = a.astype(np.float64).ravel()
    fin = flat[np.isfinite(flat)]
    if lo is None or hi is None:
        mn, mx = (float(fin.min()), float(fin.max())) if fin.size else (0.0, 1.0)
        lo_, hi_ = (mn if lo is None else lo), (mx if hi is None else hi)
        lo, hi = lo_, (hi_ if hi_ > lo_ else lo_ + 1.0)
    counts: list[int] = []
    below = above = 0
    for p in planes:
        f = p[np.isfinite(p)]
        below += int((f < lo).sum())
        above += int((f > hi).sum())
        f = f[(f >= lo) & (f <= hi)]
        k = np.floor(((f - lo) / (hi - lo)) * bins)
        k = np.clip(k, 0, bins - 1).astype(np.int64)
        counts.extend(int(n) for n in np.bincount(k, minlength=bins))
    width = (hi - lo) / bins
    cols = []
    if per is not None:
        cols.append({"name": "plane", "type": "INT", "values": [p for p in range(len(planes)) for _ in range(bins)]})
    cols.append({"name": "centre", "type": "FLOAT", "values": [lo + (k + 0.5) * width for _ in planes for k in range(bins)]})
    cols.append({"name": "count", "type": "INT", "values": counts})
    meta = {
        "bins": bins,
        "lo": lo,
        "hi": hi,
        "nan": int(np.isnan(flat).sum()),
        "posinf": int((flat == np.inf).sum()),
        "neginf": int((flat == -np.inf).sum()),
        "below": below,
        "above": above,
    }
    if per is not None:
        meta["axis"] = per
    return {"table": {"columns": cols}, "meta": meta}


def _percentile(srt: np.ndarray, q: float) -> float | None:
    n = srt.size
    if n == 0:
        return None
    pos = q / 100.0 * (n - 1)
    lo = math.floor(pos)
    hi = min(lo + 1, n - 1)
    t = pos - lo
    a, b = float(srt[lo]), float(srt[hi])
    d = b - a
    return _num(b - d * (1.0 - t) if t >= 0.5 else a + d * t)


def _pname(q: float) -> str:
    s = repr(float(q))
    return "p" + (s[:-2] if s.endswith(".0") else s)


def _stats(a: np.ndarray, v: dict) -> dict:
    per, qs = v["per"], v["percentiles"]
    if qs and a.size > MAX_SORTED:
        raise ViewError(f"{a.size} values is more than exact percentiles sort ({MAX_SORTED})")
    planes = _planes(a, per)
    rows = []
    for p in planes:
        f = p[np.isfinite(p)]
        n = int(f.size)
        mean = _neumaier(f) / n if n else float("nan")
        sd = math.sqrt(_neumaier((f - mean) ** 2) / n) if n else float("nan")
        srt = np.sort(f)
        rows.append(
            {
                "count": n,
                "nan": int(np.isnan(p).sum()),
                "posinf": int((p == np.inf).sum()),
                "neginf": int((p == -np.inf).sum()),
                "min": _num(float(f.min())) if n else None,
                "max": _num(float(f.max())) if n else None,
                "mean": _num(mean),
                "sd": _num(sd),
                **{_pname(q): _percentile(srt, q) for q in qs},
            }
        )
    cols = []
    if per is not None:
        cols.append({"name": "plane", "type": "INT", "values": list(range(len(planes)))})
    for name, ty in [
        ("count", "INT"),
        ("nan", "INT"),
        ("posinf", "INT"),
        ("neginf", "INT"),
        ("min", "FLOAT"),
        ("max", "FLOAT"),
        ("mean", "FLOAT"),
        ("sd", "FLOAT"),
    ] + [(_pname(q), "FLOAT") for q in qs]:
        cols.append({"name": name, "type": ty, "values": [r[name] for r in rows]})
    meta: dict[str, Any] = {"values": int(a.size)}
    if per is not None:
        meta["axis"] = per
    return {"table": {"columns": cols}, "meta": meta}


def _slice(a: np.ndarray, v: dict) -> dict:
    axes, at = v["axes"], v["at"]
    if any(x >= a.ndim for x in axes):
        raise ViewError(f"the array has {a.ndim} axes")
    ni, nj = a.shape[axes[0]], a.shape[axes[1]]
    if ni * nj > MAX_SLICE:
        raise ViewError(f"a slice of {ni}×{nj} is more than {MAX_SLICE} elements")
    idx: list[Any] = []
    for k in range(a.ndim):
        if k in axes:
            idx.append(slice(None))
        else:
            idx.append(min(at[k] if k < len(at) else 0, max(a.shape[k] - 1, 0)))
    s = a[tuple(idx)]
    if axes[0] > axes[1]:
        s = s.T
    s = np.ascontiguousarray(s)
    return {"array": {"dtype": a.dtype.str, "shape": list(s.shape)}, "bytes": s.tobytes()}


def derive(view: Any, dtype: str, shape: list[int], data: bytes) -> dict:
    """The view (its JSON) made from an array's bytes: ``{table, meta}`` or ``{array, bytes}``."""
    v = parse(view)
    a = _array(dtype, list(shape), data)
    if v["kind"] == "histogram":
        return _histogram(a, v)
    if v["kind"] == "stats":
        return _stats(a, v)
    return _slice(a, v)
