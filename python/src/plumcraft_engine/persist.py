"""Persisted memo records (spec 14.5), as `crates/engine/src/persist.rs`: a node slow enough to be
worth keeping leaves a record in the commonkit store under its plan key, so a later run reports it
`cached` instead of computing it.

A record is JSON, stored compressed under `plum:v1:plummemo:<plan key digest>:c`:
`{"v": 1, "plan_key", "ms", "outputs": {port: value}}`, a value being `{"json": v}`,
`{"array": {"handle", "dtype", "shape"}}`, `{"blob": {"handle", "meta", "publish", "thumb"}}`,
`{"audio": {...}}` (the Rust SDK's; a miss here, where there is no audio value) or
`{"list": {"items", "axes", "labels"}}`. Every value is published first and the record written
last. Reading is defensive: a record is untrusted input, and anything wrong with it is a miss.
"""

from __future__ import annotations

import json
from typing import Any

import numpy as np

from . import Blob, List, data
from .audio import AudioValue

RECORD_V1 = 1
MAX_DEPTH = 16
MAX_VALUES = 100_000


class _Miss(Exception):
    pass


def save(root: str, plan_key: str, outputs: dict[str, Any], ms: float) -> bool:
    """Publish every value, then write the record; False (and no record) when anything can't be stored."""
    store = data.open_store(root)
    count = [0]
    try:
        out = {port: _to_record(store, v, 0, count) for port, v in outputs.items()}
        record = json.dumps({"v": RECORD_V1, "plan_key": plan_key, "ms": ms, "outputs": out}, separators=(",", ":"), allow_nan=False)
    except (_Miss, TypeError, ValueError):
        return False
    return data.put_record(root, plan_key, record.encode("utf-8"))


def load(root: str, plan_key: str) -> tuple[dict[str, Any], float] | None:
    """The outputs a record for `plan_key` holds and what computing them took, every value read back
    and checked; None when there is no record or anything about it is wrong."""
    raw = data.read_record(root, plan_key)
    if raw is None:
        return None
    try:
        record = json.loads(raw)
        if not isinstance(record, dict) or record.get("v") != RECORD_V1 or record.get("plan_key") != plan_key:
            return None
        outputs = record.get("outputs")
        if not isinstance(outputs, dict):
            return None
        count = [0]
        ms = record.get("ms")
        ms = float(ms) if isinstance(ms, int | float) and not isinstance(ms, bool) and ms >= 0 and ms != float("inf") else 0.0
        return {port: _from_record(root, v, 0, count) for port, v in outputs.items()}, ms
    except (_Miss, ValueError, TypeError, KeyError, AttributeError, RecursionError):
        return None


def size_of(outputs: dict[str, Any]) -> int:
    """The bytes `outputs` would write."""

    def one(v: Any) -> int:
        if isinstance(v, List):
            return sum(one(x) for x in v.items)
        if isinstance(v, Blob):
            return len(v.data)
        if isinstance(v, np.ndarray):
            return int(v.nbytes)
        if isinstance(v, AudioValue):
            return int(v.samples.nbytes)
        try:
            return len(json.dumps(v))
        except (TypeError, ValueError):
            return 0

    return sum(one(v) for v in outputs.values())


def _to_record(store: Any, v: Any, depth: int, count: list[int]) -> dict:
    count[0] += 1
    if depth > MAX_DEPTH or count[0] > MAX_VALUES:
        raise _Miss
    if isinstance(v, List):
        return {"list": {"items": [_to_record(store, x, depth + 1, count) for x in v.items], "axes": [list(a) for a in v.axes], "labels": list(v.labels)}}
    if isinstance(v, Blob):
        if data.raw(v.data) != v.handle or not data.store_raw(store, v.handle, v.data):
            raise _Miss
        return {"blob": {"handle": v.handle, "meta": v.meta, "publish": True, "thumb": None}}
    if isinstance(v, AudioValue):
        handle = data.nd1(v.samples)
        if not (data.locate(store, handle) is not None or store.publish(data.key_for(handle), v.samples)):
            raise _Miss
        return {"audio": {"handle": handle, "sample_rate": v.sample_rate, "channels": v.channels}}
    if isinstance(v, np.ndarray):
        a = np.ascontiguousarray(v)
        if a.dtype.byteorder == ">":
            a = a.astype(a.dtype.newbyteorder("<"))
        handle = data.nd1(a)
        key = data.key_for(handle)
        if not (data.locate(store, handle) is not None or store.publish(key, a)):
            raise _Miss
        return {"array": {"handle": handle, "dtype": a.dtype.str, "shape": [int(n) for n in a.shape]}}
    json.dumps(v, allow_nan=False)  # a value that isn't JSON can't be persisted
    return {"json": v}


def _from_record(root: str, v: Any, depth: int, count: list[int]) -> Any:
    count[0] += 1
    if depth > MAX_DEPTH or count[0] > MAX_VALUES or not isinstance(v, dict) or len(v) != 1:
        raise _Miss
    ((kind, body),) = v.items()
    if kind == "json":
        return body
    if kind == "array":
        handle = data.parse_handle(body.get("handle"))
        a = data.read_blob(root, handle) if handle else None
        if a is None or a.dtype.str != body.get("dtype") or [int(n) for n in a.shape] != body.get("shape"):
            raise _Miss
        return a
    if kind == "audio":
        handle = data.parse_handle(body.get("handle"))
        a = data.read_blob(root, handle) if handle else None
        rate, channels = body.get("sample_rate"), body.get("channels")
        if a is None or a.dtype.str != "<f4" or a.ndim != 2 or a.shape[1] != channels or not isinstance(rate, int) or rate <= 0:
            raise _Miss
        return AudioValue(a, rate)
    if kind == "blob":
        handle = data.parse_handle(body.get("handle"))
        meta = body.get("meta")
        a = data.read_blob(root, handle) if handle and data.is_raw(handle) else None
        if a is None or not isinstance(meta, dict):
            raise _Miss
        return Blob(handle=handle, meta=meta, data=a.tobytes())
    if kind == "list":
        items = body.get("items")
        if not isinstance(items, list):
            raise _Miss
        axes = body.get("axes") or []
        labels = body.get("labels") or []
        if not (isinstance(axes, list) and all(isinstance(a, list) and len(a) == 2 and isinstance(a[0], str) and isinstance(a[1], int) for a in axes)):
            raise _Miss
        if not (isinstance(labels, list) and all(isinstance(x, str) for x in labels)):
            raise _Miss
        return List([_from_record(root, x, depth + 1, count) for x in items], [(a[0], a[1]) for a in axes], labels)
    raise _Miss  # "audio" (the Rust SDK's) and anything unknown
