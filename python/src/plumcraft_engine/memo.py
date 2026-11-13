"""Memo keys (spec 14), as `crates/proto/src/memo.rs`: the typed canonical form (14.1) and the
plan key (14.2, 14.3). Both are held to `crates/proto/fixtures/memo_canon.json`.

The canonical form tags every number, so an INT `1` and a FLOAT `1.0` never share a key, and
writes floats as their IEEE-754 bit pattern, so every language agrees on them.
"""

from __future__ import annotations

import json
import struct
from typing import Any

from commonkit import digest as ck_digest

MEMO_V1 = "plumcraft-memo-v1"

INT = "int"
FLOAT = "float"


class Uncanonical(TypeError):
    """A value with no defined encoding (spec 14.1 rule 4): a node `config` error, never a `repr`."""


def coerce(v: Any, kind: str | None) -> Any:
    """Coerce a resolved value to its port's numeric kind: an integral float on an INT port becomes
    an integer, an integer on a FLOAT port becomes a float. Anything else is left as it is."""
    if isinstance(v, bool):
        return v
    if kind == FLOAT and isinstance(v, int):
        return float(v)
    if kind == INT and isinstance(v, float) and v.is_integer() and abs(v) < 2.0**53:
        return int(v)
    return v


def canon_float(f: float) -> str:
    return '{"f":"0x' + struct.pack(">d", f).hex() + '"}'


def canon(v: Any) -> str:
    """The typed canonical form of a value (spec 14.1)."""
    out: list[str] = []
    _write(out, v)
    return "".join(out)


def _string(s: str) -> str:
    return json.dumps(s, ensure_ascii=False)


def _write(out: list[str], v: Any) -> None:
    if v is None:
        out.append("null")
    elif v is True:
        out.append("true")
    elif v is False:
        out.append("false")
    elif isinstance(v, int):
        out.append('{"i":"' + str(v) + '"}')
    elif isinstance(v, float):
        out.append(canon_float(v))
    elif isinstance(v, str):
        out.append(_string(v))
    elif isinstance(v, list | tuple):
        out.append("[")
        for i, x in enumerate(v):
            if i:
                out.append(",")
            _write(out, x)
        out.append("]")
    elif isinstance(v, dict):
        if not all(isinstance(k, str) for k in v):
            raise Uncanonical("object keys must be strings")
        out.append("{")
        for i, k in enumerate(sorted(v, key=lambda k: k.encode("utf-8", "surrogatepass"))):
            if i:
                out.append(",")
            out.append(_string(k))
            out.append(":")
            _write(out, v[k])
        out.append("}")
    elif hasattr(v, "item") and callable(v.item):  # a numpy scalar: its Python value
        _write(out, v.item())
    else:
        raise Uncanonical(f"no canonical form for {type(v).__name__}")


def plan_key(kind: str, version: int, salt: str, device_class: str | None, params: dict, inputs: dict) -> str:
    """A node's plan key: the digest, in the process's algorithm (commonkit's switch, D11), of the
    canonical form of `["plumcraft-memo-v1", kind, version, salt, device_class, params, inputs]`,
    as `memo:<algo>:<hex>`."""
    text = canon([MEMO_V1, kind, version, salt, device_class, params, inputs])
    return f"memo:{ck_digest.current()}:" + ck_digest.raw(text.encode("utf-8", "surrogatepass"))


def input_identity(upstream_key: str, port: str, slice_: str | None = None) -> str:
    """The identity of a wired input: its upstream node's plan key and port, and the canonical
    text of its wire's slice."""
    return f"{upstream_key}#{port}[{slice_}]" if slice_ else f"{upstream_key}#{port}"
