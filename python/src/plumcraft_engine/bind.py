"""External bindings (`RunRequest.inputs`, spec 7.6 rule 10), as `crates/engine/src/bind.rs`:
`{"node.port": PortValue}` values that replace a port's wire or widget. A coordinator feeds one
engine's results to another this way; a client can feed a node a value it already has.

A bound value is read like a wired one: a json value as is; a blob by its handle from this engine's
store, else from its inline bytes, always checked against the handle; a list item by item. By the
blob's form, an array arrives as an `ArrayValue` (with its space, names and table flag), audio as
an `AudioValue` (its (frames, channels) float32 samples and sample rate), text as a string, json and
tables parsed, anything else as a `Blob`. Its memo identity is its content (spec 14.2): the handle
for a blob, `{"json": value}` for json.
"""

from __future__ import annotations

import base64
import binascii
import json
from typing import Any

import numpy as np

from . import Blob, List, NodeError, data
from .arrays import ArrayValue
from .audio import AudioValue

MAX_DEPTH = 8
MAX_VALUES = 100_000


def by_node(inputs: Any, has_node) -> dict[str, dict[str, Any]]:
    """The bindings by node, then port. A key without a `.`, or naming a node the graph lacks, and
    a value that isn't a PortValue object, are dropped (spec 18)."""
    out: dict[str, dict[str, Any]] = {}
    if not isinstance(inputs, dict):
        return out
    for key, v in inputs.items():
        if not isinstance(key, str) or not isinstance(v, dict) or v.get("kind") not in ("json", "blob", "list", "opaque"):
            continue
        node, dot, port = key.rpartition(".")
        if not dot or not node or not port or not has_node(node):
            continue
        out.setdefault(node, {})[port] = v
    return out


def identity(v: dict) -> Any:
    """A bound value's memo identity: its content."""
    kind = v.get("kind")
    if kind == "blob":
        ref = v.get("ref")
        return data.parse_handle(ref.get("handle")) if isinstance(ref, dict) else None
    if kind == "json":
        return {"json": v.get("value")}
    if kind == "list":
        items = v.get("items")
        return [identity(i) if isinstance(i, dict) else None for i in items] if isinstance(items, list) else []
    return {"opaque": v.get("type", "")}


def to_value(root: str | None, port: str, v: dict, trusted: tuple[str, ...] = ()) -> Any:
    """A bound value as the node receives it. `trusted`: the run's `trusted_roots`."""
    return _read(root, port, v, 0, [0], trusted)


def _config(port: str, message: str) -> NodeError:
    return NodeError(NodeError.CONFIG, f"input {json.dumps(port)}: {message}")


def _read(root: str | None, port: str, v: Any, depth: int, count: list[int], trusted: tuple[str, ...]) -> Any:
    count[0] += 1
    if depth > MAX_DEPTH or count[0] > MAX_VALUES:
        raise _config(port, "the bound list is too large or too deep")
    kind = v.get("kind") if isinstance(v, dict) else None
    if kind == "json":
        return v.get("value")
    if kind == "list":
        items = v.get("items")
        return List([_read(root, port, i, depth + 1, count, trusted) for i in (items if isinstance(items, list) else [])])
    if kind == "blob":
        return _blob(root, port, v.get("ref"), trusted)
    if kind == "opaque":
        raise _config(port, f"a {v.get('type', '')} value stays in the engine that made it")
    raise _config(port, "not a value")


def _inline(ref: dict, handle: str, form: dict) -> np.ndarray | None:
    for loc in ref.get("at") or []:
        if not (isinstance(loc, dict) and loc.get("kind") == "inline" and isinstance(loc.get("b64"), str)):
            continue
        try:
            b = base64.b64decode(loc["b64"], validate=True)
        except (binascii.Error, ValueError):
            continue
        if handle.startswith("md5:raw:"):
            if data.raw(b) == handle:
                return np.frombuffer(b, dtype="|u1")
            continue
        try:
            if form.get("kind") == "audio":
                shape = [int(form["frames"]), int(form["channels"])]
            else:
                shape = [int(n) for n in form["shape"]]
            a = np.frombuffer(b, dtype=np.dtype(form["dtype"])).reshape(shape)
        except (KeyError, TypeError, ValueError):
            continue
        if data.nd1(a) == handle:
            return a
    return None


def _blob(root: str | None, port: str, ref: Any, trusted: tuple[str, ...]) -> Any:
    if not isinstance(ref, dict):
        raise _config(port, "a bound blob needs a DataRef")
    handle = data.parse_handle(ref.get("handle"))
    if handle is None:
        raise _config(port, "a bound blob needs a handle")
    meta = ref.get("meta") if isinstance(ref.get("meta"), dict) else {}
    form = meta.get("form") if isinstance(meta.get("form"), dict) else {}
    a = data.read_blob(root, handle) if root else None
    if a is None and root:
        a = data.read_located(root, ref, handle, trusted)
    if a is None:
        a = _inline(ref, handle, form)
    if a is None:
        raise NodeError(NodeError.MISSING_SOURCE, f"input {json.dumps(port)}: {handle} is not in this engine's store")
    kind = form.get("kind")
    if kind == "array":
        names = form.get("names") if isinstance(form.get("names"), list) else []
        space = form.get("space") if isinstance(form.get("space"), str) else None
        return ArrayValue(a, space, [str(n) for n in names], form.get("table") is True)
    if kind == "audio":
        rate = form.get("sample_rate")
        if not isinstance(rate, int) or isinstance(rate, bool) or rate <= 0:
            raise _config(port, f"{handle}: audio without a sample rate")
        return AudioValue(np.asarray(a, dtype="<f4"), rate)
    b = a.tobytes()
    if kind == "text":
        try:
            return b.decode("utf-8")
        except UnicodeDecodeError as e:
            raise _config(port, f"{handle} is not UTF-8 text") from e
    if kind in ("json", "table"):
        try:
            return json.loads(b)
        except ValueError as e:
            raise _config(port, f"{handle} is not JSON ({e})") from e
    return Blob(handle=handle, meta=meta, data=b)
