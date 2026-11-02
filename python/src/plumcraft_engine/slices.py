"""A wire's slice (spec 7.6 rule 3): Python's `start:stop:step` (any part left out), or a single
index. A range keeps a list a list; an index takes one item. The same grammar and refusals as
`plumcraft_proto::slice`, held to the shared vectors in `crates/proto/fixtures/slices.json`."""

from __future__ import annotations

import re

_PART = re.compile(r"^-?[0-9]+$")
_MAX = 1 << 63
# How many lengths `parse` tries before calling a slice empty whatever the list.
_EMPTY_PROBE = 64


def _number(spec: str, text: str) -> int:
    if not _PART.match(text):
        raise ValueError(f"slice {spec!r}: use start:stop:step or an index, e.g. 1:-1:2 or -1")
    v = int(text)
    if not -_MAX <= v < _MAX:
        raise ValueError(f"slice {spec!r}: {text} is too large")
    return v


def parse(spec: str) -> slice | int:
    """A slice or an index; ValueError for anything else, a zero step, or a slice that is empty
    whatever the list's length."""
    if not spec.strip():
        raise ValueError(f"slice {spec!r} is empty")
    parts = [p.strip() for p in spec.split(":")]
    if len(parts) > 3:
        raise ValueError(f"slice {spec!r}: use start:stop:step or an index, e.g. 1:-1:2 or -1")
    nums = [_number(spec, p) if p else None for p in parts]
    if len(nums) == 1:
        if nums[0] is None:
            raise ValueError(f"slice {spec!r} is empty")
        return nums[0]
    if len(nums) == 3 and nums[2] == 0:
        raise ValueError(f"slice {spec!r}: the step can't be 0")
    s = slice(*nums)
    if all(not range(n)[s] for n in range(1, _EMPTY_PROBE + 1)):
        raise ValueError(f"slice {spec!r} is always empty: it picks nothing from a list of any length")
    return s


def canonical(s: slice | int) -> str:
    """Its canonical text: `-1`, `1:-1:2`, `:-1`, `::2`."""
    if isinstance(s, int):
        return str(s)
    part = lambda v: "" if v is None else str(v)  # noqa: E731
    text = f"{part(s.start)}:{part(s.stop)}"
    return text if s.step is None else f"{text}:{s.step}"


def picks(s: slice | int, n: int) -> list[int]:
    """The items picked from a list of `n`, in order. ValueError for an index out of range or an
    empty result."""
    if isinstance(s, int):
        if not -n <= s < n:
            raise ValueError(f"index {s} is out of range for {n} items")
        return [s + n if s < 0 else s]
    out = list(range(n)[s])
    if not out:
        raise ValueError(f"slice {canonical(s)} of {n} items is empty")
    return out
