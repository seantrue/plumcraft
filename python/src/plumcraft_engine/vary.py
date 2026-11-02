"""Vary: values to sweep a setting with, as `crates/engine/src/vary.rs` (lifted from the research software's
`research_engine/nodes/vary.py`, and `numpy_ops.py`'s `linspace`, renamed Vary Range). Wired into
an input that takes one value, a Vary node's list runs that node once per value (the executor's
list mapping, `plumcraft_engine.exec`), each result labelled with the value that made it.

- **Vary Float, Int, String, Bool:** a typed-in list, comma-separated; numbers also take ranges,
  `start:stop:step` (stop included when the steps land on it; step 1 when left out) or
  `start:stop/count` (count values evenly spaced, both ends included).
- **Vary Choices** feeds an ENUM input: blank means every choice that input offers (the executor
  passes them in as `_choices`), or a comma-separated subset.
- **Vary Range** is numpy's `linspace`: `num` numbers evenly spaced from `start` to `stop` (stop
  included unless `endpoint` is off).
- **Vary Range 2D and 3D** are `linspace` between two points: `num` points evenly spaced along
  the segment, their coordinates on outputs `x`, `y` (and `z`) that share one axis, so they zip
  downstream (two settings varied together). With `grid` on, every combination of `num` values
  per coordinate instead (a meshgrid, `x` varying fastest).

Every engine built on the SDK can offer them with `Engine.with_vary()` (or `vary = True` on a
declared engine). The arithmetic is the Rust engine's, step for step, so both make the same
floats.
"""

from __future__ import annotations

import json
import math
import re
from typing import Any

from . import ANY, Cx, List, Node, NodeError

# The longest list a Vary node makes (a sweep, not a data array).
MAX_VALUES = 1000

# The input key under which a Vary Choices node gets the choices of the inputs it feeds (as
# `exec.CHOICES_KEY`, which re-exports it).
CHOICES_KEY = "_choices"

FLOAT, INT, STRING, BOOL = "float", "int", "string", "bool"

# Unicode White_Space, as Rust's `str::trim` uses it.
_WS = "\t\n\x0b\x0c\r \x85\xa0                　"
# What Rust's `f64::from_str` accepts.
_F64 = re.compile(r"[+-]?(?:inf|infinity|nan|(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)", re.IGNORECASE | re.ASCII)
_DIGITS = re.compile(r"[0-9]+", re.ASCII)
_U64_MAX = (1 << 64) - 1


def _category() -> list[str]:
    return ["Building blocks", "Vary"]


def wire_types() -> list[dict]:
    """The scalar wire types the Vary nodes make."""
    return [{"id": t, "media": "scalar"} for t in ("FLOAT", "INT", "STRING", "BOOL", "ENUM")]


def _config(message: str) -> NodeError:
    return NodeError(NodeError.CONFIG, message)


def _q(s: str) -> str:
    """A value in a message, quoted as the Rust engine's `{:?}` quotes it."""
    return json.dumps(s, ensure_ascii=False)


def _list_out(name: str, ty: str) -> dict:
    return {"name": name, "type": ty, "list": True, "socket": "socket"}


def _number_in(name: str, label: str, ty: str, default: Any, tooltip: str) -> dict:
    step = 1.0 if ty == "INT" else 0.1
    return {"name": name, "type": ty, "label": label, "default": default, "step": step, "tooltip": tooltip}


def _f64(s: str) -> float | None:
    """`s` as Rust's `str::parse::<f64>` reads it (no whitespace, no underscores), or None."""
    return float(s) if _F64.fullmatch(s) else None


def parse(text: str, kind: str) -> list[Any]:
    """The values a Vary node makes from its text (the research software's `_vary`): comma-separated items, each a
    value of `kind` or, for numbers, a range `start:stop:step` or `start:stop/count`. Blank items
    are skipped. Raises a `config` NodeError."""
    out: list[Any] = []
    for raw in text.split(","):
        item = raw.strip(_WS)
        if not item:
            continue
        rng = _range_parts(item) if kind in (FLOAT, INT) else None
        if rng is None:
            out.append(_one(item, kind))
        else:
            vals = _expand(item, *rng)
            if kind == INT:
                if any(not v.is_integer() for v in vals):
                    raise _config(f"{_q(item)}: the range must hit whole numbers")
                out.extend(int(v) for v in vals)
            else:
                out.extend(vals)
        if len(out) > MAX_VALUES:
            raise _config(f"more than {MAX_VALUES} values")
    if not out:
        raise _config("no values: type some, comma-separated")
    return out


def _range_parts(item: str) -> tuple[float, float, tuple[str, float]] | None:
    """`start:stop[:step]` or `start:stop/count`, as `(start, stop, ("step", s) | ("count", c))`."""
    start, sep, rest = item.partition(":")
    if not sep:
        return None
    how: tuple[str, float]
    if "/" in rest:
        stop, _, count = rest.partition("/")
        if not _DIGITS.fullmatch(count) or ":" in stop:
            return None
        c = int(count)
        if c > _U64_MAX:
            return None
        how = ("count", c)
    elif ":" in rest:
        stop, _, step = rest.partition(":")
        if not step or ":" in step or "/" in step:
            return None
        s = _f64(step.strip(_WS))
        if s is None:
            return None
        how = ("step", s)
    else:
        stop, how = rest, ("step", 1.0)
    if not start or not stop or "/" in start or "/" in stop:
        return None
    a, b = _f64(start.strip(_WS)), _f64(stop.strip(_WS))
    if a is None or b is None:
        return None
    return a, b, how


def _expand(item: str, start: float, stop: float, how: tuple[str, float]) -> list[float]:
    if not math.isfinite(start) or not math.isfinite(stop):
        raise _config(f"{_q(item)}: start and stop must be numbers")
    kind, v = how
    if kind == "count":
        c = int(v)
        if c == 0:
            raise _config(f"{_q(item)}: count must be at least 1")
        if c > MAX_VALUES:
            raise _config(f"more than {MAX_VALUES} values")
        if c == 1:
            return [start]
        return [start + (stop - start) * float(i) / float(c - 1) for i in range(c)]
    step = v
    if step == 0.0 or not math.isfinite(step) or (stop - start) * step < 0.0:
        raise _config(f"{_q(item)}: the step must move from start toward stop")
    n = math.floor((stop - start) / step + 1e-9) + 1.0
    if n > MAX_VALUES:
        raise _config(f"more than {MAX_VALUES} values")
    return [start + step * float(i) for i in range(int(n))]


def _one(item: str, kind: str) -> Any:
    if kind == STRING:
        return item
    if kind == BOOL:
        low = item.lower()
        if low in ("true", "yes", "on", "1"):
            return True
        if low in ("false", "no", "off", "0"):
            return False
        raise _config(f"{_q(item)} is not true or false")
    v = _f64(item)
    if v is None or not math.isfinite(v):
        raise _config(f"{_q(item)} is not a number")
    if kind == INT:
        if not v.is_integer() or abs(v) > 9.0e15:
            raise _config(f"{_q(item)} is not a whole number")
        return int(v)
    return v


def _text(i: dict[str, Any], key: str) -> str:
    v = i.get(key)
    return v if isinstance(v, str) else ""


def _real(v: Any) -> float | None:
    """A JSON number as `Value::as_f64` reads it (never a bool)."""
    if isinstance(v, bool) or not isinstance(v, int | float):
        return None
    try:
        return float(v)
    except OverflowError:
        return None


def _number(i: dict[str, Any], key: str) -> float:
    v = _real(i.get(key))
    if v is None or not math.isfinite(v):
        raise _config(f"{key} must be a number")
    return v


def _flag(i: dict[str, Any], key: str, default: bool) -> bool:
    v = i.get(key)
    return v if isinstance(v, bool) else default


def _values(items: list[Any]) -> List:
    return List(list(items))


class Typed(Node):
    """A typed-in list: Vary Float, Int, String or Bool."""

    def __init__(self, ident: str, display: str, ty: str, kind: str, default: str, what: str):
        self.kind = kind
        if kind in (FLOAT, INT):
            tip = (
                "The values, comma-separated; a range as start:stop:step (stop included when the steps land on it; step 1 if left out) "
                "or start:stop/count (count values, evenly spaced, both ends included)"
            )
        elif kind == BOOL:
            tip = "The values, comma-separated; true/false, yes/no, on/off or 1/0"
        else:
            tip = "The values, comma-separated; each item as typed"
        self.definition = {
            "id": f"vary.{ident}",
            "display": display,
            "category": _category(),
            "description": (
                f"A list of {what} to sweep a setting with: wire it into a parameter and that node runs once per value, each result labelled with its value. "
                "Several varied settings zip when they come from the same Vary node, and form every combination when they don't."
            ),
            "inputs": [{"name": "values", "type": "STRING", "default": default, "tooltip": tip}],
            "outputs": [_list_out("values", ty)],
        }

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        return {"values": _values(parse(_text(inputs, "values"), self.kind))}


class Choices(Node):
    """Vary Choices: every choice of the ENUM input it feeds, or some of them."""

    definition = {
        "id": "vary.choices",
        "display": "Vary Choices",
        "category": _category(),
        "description": (
            "Every choice of the ENUM setting this is wired into (blank), or a comma-separated subset of them: that node runs once per choice, "
            "each result labelled with its choice. The choices come from the input it feeds, so nothing needs typing."
        ),
        "inputs": [
            {
                "name": "values",
                "type": "STRING",
                "default": "",
                "tooltip": "Blank: every choice the wired input offers. Or some of them, comma-separated, as the dropdown spells them",
            }
        ],
        "outputs": [_list_out("values", "STRING")],
        "flags": {"vary_choices": True},
    }

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        raw = inputs.get(CHOICES_KEY)
        offered = [c if isinstance(c, str) else json.dumps(c, separators=(",", ":"), ensure_ascii=False) for c in raw] if isinstance(raw, list) else []
        typed = _text(inputs, "values")
        if not typed.strip(_WS):
            if not offered:
                raise _config("Vary Choices is blank and isn't wired into an input with choices: wire it, or type the choices, comma-separated")
            return {"values": _values(offered)}
        picked = parse(typed, STRING)
        if offered:
            unknown = [c for c in picked if c not in offered]
            if unknown:
                raise _config(f"not among the input's choices: {', '.join(unknown)} (offered: {', '.join(offered)})")
        return {"values": _values(picked)}


def linspace(start: float, stop: float, num: int, endpoint: bool) -> list[float]:
    """numpy's `linspace`: `num` values from `start` to `stop`, `stop` included when `endpoint`."""
    div = max(num - 1, 0) if endpoint else num
    if div == 0:
        return [start] * min(num, 1)
    step = (stop - start) / float(div)
    out = [start + step * float(i) for i in range(num)]
    if endpoint and out:
        out[-1] = stop
    return out


def _count(i: dict[str, Any]) -> int:
    """The count input: 1 to `MAX_VALUES`."""
    n = _real(i.get("num"))
    n = 0.0 if n is None else n
    if not (1.0 <= n <= MAX_VALUES) or not n.is_integer():
        raise _config(f"num must be a whole number from 1 to {MAX_VALUES}")
    return int(n)


AXES = ("x", "y", "z")


class Range(Node):
    """Vary Range (1 dimension), and Vary Range 2D and 3D: `linspace` along a segment between two
    points, or over a grid."""

    def __init__(self, dims: int):
        self.dims = dims
        self.names = AXES[:dims]
        ident, display = {1: ("vary.range", "Vary Range"), 2: ("vary.range2d", "Vary Range 2D")}.get(dims, ("vary.range3d", "Vary Range 3D"))
        inputs: list[dict] = []
        if dims == 1:
            inputs.append(_number_in("start", "start", "FLOAT", 1.0, "The first value"))
            inputs.append(_number_in("stop", "stop", "FLOAT", 3.0, "The last value (included unless endpoint is off)"))
            outputs = [_list_out("values", "FLOAT")]
            description = (
                "num numbers evenly spaced from start to stop (stop included unless endpoint is off), as numpy's linspace. "
                "Wire it into a parameter, such as a width, to run that node once per value."
            )
        else:
            for axis in self.names:
                inputs.append(_number_in(f"start_{axis}", f"start {axis}", "FLOAT", 0.0, f"{axis} at the first point"))
                inputs.append(_number_in(f"stop_{axis}", f"stop {axis}", "FLOAT", 1.0, f"{axis} at the last point"))
            outputs = [_list_out(a, "FLOAT") for a in self.names] + [_list_out("".join(self.names), ANY)]
            coords = ", ".join(self.names)
            points = "num × num points" if dims == 2 else "num × num × num points"
            description = (
                "num points evenly spaced from the start point to the stop point (numpy's linspace between two points): "
                f"their {coords} on outputs that zip, so the settings they feed vary together, and the points as tuples on {''.join(self.names)}. "
                f"With grid on, every combination of num values per coordinate instead ({points}), {AXES[0]} varying fastest."
            )
        inputs.append(
            {
                "name": "num",
                "type": "INT",
                "default": 5,
                "min": 1.0,
                "max": float(MAX_VALUES),
                "step": 1.0,
                "tooltip": "How many values" if dims == 1 else "How many points (with grid on: how many values per coordinate)",
            }
        )
        inputs.append({"name": "endpoint", "type": "BOOL", "default": True, "tooltip": "Include the stop value"})
        if dims > 1:
            grid_tip = "Every combination of the coordinates' values, not points along a line"
            inputs.append({"name": "grid", "type": "BOOL", "default": False, "tooltip": grid_tip})
        self.definition = {"id": ident, "display": display, "category": _category(), "description": description, "inputs": inputs, "outputs": outputs}

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        num = _count(inputs)
        endpoint = _flag(inputs, "endpoint", True)
        if self.dims == 1:
            return {"values": _values(linspace(_number(inputs, "start"), _number(inputs, "stop"), num, endpoint))}
        per_axis = [linspace(_number(inputs, f"start_{a}"), _number(inputs, f"stop_{a}"), num, endpoint) for a in self.names]
        if _flag(inputs, "grid", False):
            total = num**self.dims
            if total > MAX_VALUES:
                raise _config(f"a grid of {num} per coordinate is more than {MAX_VALUES} points")
            # row-major with the last coordinate slowest: x varies fastest
            points = []
            for t in range(total):
                rest, p = t, []
                for vals in per_axis:
                    p.append(vals[rest % num])
                    rest //= num
                points.append(p)
        else:
            points = [[vals[i] for vals in per_axis] for i in range(num)]
        # One axis for every output, named by the node: they zip downstream.
        axes = [(cx.node_id, len(points))]
        out: dict[str, Any] = {axis: List([p[k] for p in points], list(axes)) for k, axis in enumerate(self.names)}
        out["".join(self.names)] = List([List(list(p)) for p in points], list(axes))
        return out


def nodes() -> list[Node]:
    """Every Vary node."""
    return [
        Typed("float", "Vary Float", "FLOAT", FLOAT, "0.5, 1, 2", "numbers"),
        Typed("int", "Vary Int", "INT", INT, "1, 2, 3", "whole numbers"),
        Typed("string", "Vary String", "STRING", STRING, "a, b", "texts"),
        Typed("bool", "Vary Bool", "BOOL", BOOL, "false, true", "on/off values"),
        Choices(),
        Range(1),
        Range(2),
        Range(3),
    ]
