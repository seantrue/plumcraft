"""The declarative layer: an engine is a class, each node a decorated, typed method.

    class TextEngine(Engine):
        id, name, version = "text", "Text", "0.1.0"
        wires = (Text, String)

        @node("Replace")
        def replace(self, text: Text, find: str = "", with_: str = "") -> Text:
            return text.replace(find, with_)

    if __name__ == "__main__":
        sys.exit(TextEngine.serve())

The class is compiled when it is defined (`Engine.__init_subclass__` calls `compile_engine`):
every `@node` method becomes a `NodeDef` built from its signature, and every mistake (an
annotation with no port meaning, a default outside its range, two nodes with one id, a missing
return annotation) raises `DeclarationError` then, not when a run reaches the node. Calling the
class, `TextEngine()`, builds an ordinary `Engine` whose nodes are `Node` objects bound to the
instance, so the executor, host and bindings are the low-level ones, unchanged.

Ports come from the signature:

- a parameter's annotation gives the port's type: `str`, `int`, `float`, `bool` are the scalars
  `STRING INT FLOAT BOOL`; `Literal["a", "b"]` is `ENUM` with those choices; a wire type is an
  `Annotated[T, WireType(...)]` alias the engine declares once;
- its default gives the port's `default`; `X | None = None` makes it `optional`;
- `Annotated[...]` extras refine it: `Range(min, max, step)`, `File("open")`, `Port(label=...)`;
- a parameter annotated with a dataclass contributes one port per field, and the method receives
  the dataclass (as Cartooner's plugins take their `Parameters`);
- a parameter named `cx` (or annotated `Cx`) is not a port: it receives the node's `Cx`, for
  progress and cancel;
- a trailing underscore is dropped (`with_` is the port `with`).

Outputs come from the return annotation: one type is one output (named after the wire type's id
in lower case, or `@node(output=...)`), a `NamedTuple` is one output per field, `None` is none.

Alternatives considered (see `python/README.md`): a params dataclass per node as the only form
(Cartooner's `parameters = AnnealParameters`), which reads well for ten settings and badly for
one; and a `Port(...)`-as-default style (`find: str = Port("", label=...)`, as pydantic/typer do),
which hides the real default from type checkers. Signatures are primary, `Annotated` carries
the rest, and a dataclass parameter is there for nodes with many settings.

The low-level layer (`Node` subclasses with a `definition` dict, `simple`, `Engine(info)`) stays:
the decorators compile to it, and it is the escape hatch for whatever a signature can't say
(variadic ports, `options`, a port whose type depends on another).
"""

from __future__ import annotations

import dataclasses
import inspect
import json
import math
import types
import typing
from collections.abc import Callable, Iterable
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Annotated, Any, Literal, TypeVar, Union, get_args, get_origin

import numpy as np

from . import Blob, Cx, Node, NodeError
from .arrays import VIEWS
from .audio import AudioValue

if TYPE_CHECKING:
    from . import Engine

F = TypeVar("F", bound=Callable[..., Any])

# The scalar types the UI knows without a catalogue (spec 7.4); every other type is a socket.
SCALARS = ("FLOAT", "INT", "BOOL", "STRING", "ENUM")


class DeclarationError(TypeError):
    """An engine class that can't be compiled into a catalogue; raised when the class is defined."""


class ConfigError(NodeError):
    """A node's settings are wrong (kind `config`): the user can fix it in the node's widgets."""

    def __init__(self, message: str):
        super().__init__(NodeError.CONFIG, message)


class OpError(NodeError):
    """The computation failed on this input (kind `op`)."""

    def __init__(self, message: str):
        super().__init__(NodeError.OP, message)


@dataclass(frozen=True)
class WireType:
    """A wire type (spec 7.2), used as the metadata of an `Annotated` alias:

    Text = Annotated[str, WireType("TEXT", media="text", color="#c9a227", thumb=True)]
    """

    id: str
    media: str = "bytes"
    color: str | None = None
    thumb: bool = False
    views: tuple[str, ...] = ()
    default_view: str | None = None
    # Parameters per view: `{"angle": [Port…]}` (port dicts as a NodeDef's).
    view_params: dict | None = field(default=None, hash=False, compare=False)

    def to_json(self) -> dict:
        out: dict[str, Any] = {"id": self.id}
        if self.color is not None:
            out["color"] = self.color
        out["media"] = self.media
        if self.views:
            out["views"] = list(self.views)
        if self.view_params:
            out["view_params"] = self.view_params
        if self.default_view is not None:
            out["default_view"] = self.default_view
        if self.thumb:
            out["thumb"] = True
        return out

    @property
    def scalar(self) -> bool:
        return self.id in SCALARS


# The scalar wire types, for an engine's `wires` and for explicit annotations.
String = Annotated[str, WireType("STRING", media="scalar")]
Int = Annotated[int, WireType("INT", media="scalar")]
Float = Annotated[float, WireType("FLOAT", media="scalar")]
Bool = Annotated[bool, WireType("BOOL", media="scalar")]
_ENUM = WireType("ENUM", media="scalar")
# The core ARRAY type (`plumcraft_proto::catalogue::core_types::array`; maintainer, 2026-10-29: "Make
# arrays first class"): the same in every engine, so an array crosses engine borders unchanged. A
# node returns an ndarray, or an `ArrayValue` to say its colour space and channel names.
_TURN = {
    "name": "turn",
    "type": "FLOAT",
    "label": None,
    "tooltip": "One full turn in the array's units (1 for turns, 360 for degrees, 6.283 for radians)",
    "default": 1.0,
}
Array = Annotated[
    np.ndarray,
    WireType("ARRAY", media="ndarray", color="#8a5cc2", thumb=True, views=VIEWS, default_view="auto", view_params={"angle": [_TURN]}),
]
# The core TEXT, TABLE, IMAGE and AUDIO types (`plumcraft_proto::catalogue::core_types`; maintainer,
# 2026-10-30, decision G3: "yes core data types", and "Core audio, please"): the same in every
# engine, so a value crosses engine borders unchanged. A node returns a str for TEXT, a columnar
# table dict (`{"columns": [{"name", "type", "values"}]}`) for TABLE, an encoded picture `Blob`
# for IMAGE (PNG unless asked otherwise), and an `AudioValue` for AUDIO.
Text = Annotated[str, WireType("TEXT", media="text", color="#c9a227", thumb=True)]
Table = Annotated[dict, WireType("TABLE", media="table", color="#4f86c6")]
Image = Annotated[Blob, WireType("IMAGE", media="image", color="#2563eb", thumb=True)]
Audio = Annotated[AudioValue, WireType("AUDIO", media="audio", color="#2fa39a", thumb=True)]
# The core JSON type (maintainer, 2026-10-30: "JSON yes"): any JSON value; the editor shows it as YAML.
Json = Annotated[Any, WireType("JSON", media="tree", color="#64748b")]
# The core SVG type (`plumcraft_proto::catalogue::core_types::svg`; maintainer, 2026-10-30: "Make
# SVG first class"): an SVG document, the same in every engine. A node returns `svg(text)`.
Svg = Annotated[Blob, WireType("SVG", media="vector", color="#16a34a", thumb=True)]
_BY_PY = {str: String, int: Int, float: Float, bool: Bool}


@dataclass(frozen=True)
class Range:
    """A numeric port's bounds and step (`min`, `max`, `step` in the NodeDef).

    `invalid` says what a value outside the range, or of the wrong type, does:
    `"error"` (the default) fails the node with a `config` error; `"clamp"` clamps it to the
    range (and replaces a wrong type with the default); `"pass"` hands it to the node as it is,
    the range being only a hint to the UI.
    """

    min: float | None = None
    max: float | None = None
    step: float | None = None
    invalid: Literal["error", "clamp", "pass"] = "error"


@dataclass(frozen=True)
class File:
    """A path port (`path` in the NodeDef): `open` (a file the node reads, keyed by its content and
    uploaded to a remote engine), `save` or `dir`. `filters` are `(name, [extensions])` pairs."""

    mode: Literal["open", "save", "dir"] = "open"
    filters: tuple[tuple[str, tuple[str, ...]], ...] = ()

    def to_json(self) -> dict:
        out: dict[str, Any] = {"mode": self.mode}
        if self.filters:
            out["filters"] = [{"name": n, "extensions": list(e)} for n, e in self.filters]
        return out


@dataclass(frozen=True)
class Port:
    """Presentation of a port: its label, tooltip, whether it is advanced, its socket mode
    (`widget`, `socket` or `both`), a name other than the parameter's, and a widget hint
    (`widget="multiline"` for a query or a script)."""

    label: str | None = None
    tooltip: str | None = None
    advanced: bool = False
    socket: Literal["widget", "socket", "both"] | None = None
    name: str | None = None
    decimals: int | None = None
    #: How to edit a value its type doesn't say: `multiline` (a text box), `color`, `hexcolor`, `multichoice`.
    widget: str | None = None


_MISSING: Any = object()


@dataclass
class _In:
    """One input port and how a resolved value becomes the argument the method receives."""

    port: dict
    arg: str
    py: Any  # str, int, float, bool, "enum" or None (a wire type, passed through)
    choices: tuple | None
    rng: Range | None
    default: Any

    def value(self, inputs: dict[str, Any]) -> Any:
        name = self.port["name"]
        v = inputs.get(name)
        if v is None:
            return None
        policy = self.rng.invalid if self.rng else "error"
        if policy == "pass":
            return as_text(v) if self.py is str else v
        if self.py is str:
            return as_text(v)
        ok = _check_type(self.py, v, self.choices)
        if ok is _MISSING:
            if policy == "clamp" and self.default is not None:
                return self.default
            raise ConfigError(f"{name}: expected {_describe(self.py, self.choices)}, got {_compact(v)}")
        v = ok
        r = self.rng
        if r is not None and isinstance(v, int | float):
            lo, hi = r.min, r.max
            if (lo is not None and v < lo) or (hi is not None and v > hi):
                if policy == "clamp":
                    v = min(max(v, lo if lo is not None else v), hi if hi is not None else v)
                    v = self.py(v)
                else:
                    raise ConfigError(f"{name}: {_compact(v)} is outside {_bounds(r)}")
        return v


@dataclass
class _Group:
    """A dataclass parameter: its fields' ports, and the dataclass the method receives."""

    cls: type
    arg: str
    fields: list[tuple[str, _In]]

    def value(self, inputs: dict[str, Any]) -> Any:
        return self.cls(**{f: p.value(inputs) for f, p in self.fields})


@dataclass
class NodeSpec:
    """A compiled `@node` method: its NodeDef, and how to call it and read its result."""

    method: str
    fn: Callable[..., Any]
    definition: dict
    params: list[_In | _Group]
    cx_arg: str | None
    outputs: list[str]
    tuple_out: bool

    def call(self, owner: Any, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        kwargs = {p.arg: p.value(inputs) for p in self.params}
        if self.cx_arg is not None:
            kwargs[self.cx_arg] = cx
        out = self.fn(owner, **kwargs)
        if not self.outputs:
            return {}
        if not self.tuple_out:
            return {self.outputs[0]: out}
        if not isinstance(out, tuple) or len(out) != len(self.outputs):
            raise NodeError(NodeError.OP, f"the node returned {type(out).__name__}, not a tuple of {len(self.outputs)} outputs")
        return dict(zip(self.outputs, out, strict=True))


class _Bound(Node):
    """A `NodeSpec` bound to an engine instance: the low-level `Node` the executor runs."""

    def __init__(self, spec: NodeSpec, owner: Any):
        self.definition = json.loads(json.dumps(spec.definition))
        self._spec = spec
        self._owner = owner

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        return self._spec.call(self._owner, inputs, cx)


@dataclass(frozen=True)
class _Mark:
    display: str | None
    id: str | None
    category: tuple[str, ...] | None
    description: str | None
    output: str | tuple[str, ...] | None
    family: str | None
    icon: str | None
    version: int | None
    memo: str | dict | None
    flags: dict | None
    x: dict | None
    converts: bool | tuple[str, str] | None = None
    lossless: bool = False


def node(
    display: str | F | None = None,
    *,
    id: str | None = None,
    category: str | Iterable[str] | None = None,
    description: str | None = None,
    output: str | Iterable[str] | None = None,
    family: str | None = None,
    icon: str | None = None,
    version: int | None = None,
    memo: str | dict | None = None,
    flags: dict | None = None,
    x: dict | None = None,
    converts: bool | tuple[str, str] | None = None,
    lossless: bool = False,
) -> Any:
    """Mark a method as a node. Bare (`@node`) or with options (`@node("Upper case")`).

    - `display`: the palette name (default: the method name, capitalised, `_` as spaces).
    - `id`: the node id (default `<engine id>.<method name>`).
    - `category`: a string or a path (default: the engine's `category`).
    - `description`: default the method's docstring.
    - `output`: the output port's name, or names for a `NamedTuple` (default: the wire type id in
      lower case, or the tuple's field names).
    - `family`, `icon`: default the engine's `family`, `icon`.
    - `version`: part of the memo key; bump it when results change.
    - `memo`: a scope (`"none"`, `"session"`, `"machine"`, `"global"`) or a full `memo` object.
    - `flags`, `x`: passed through to the NodeDef.
    - `converts`: the node turns one type into another (`docs/research/20-convert.md`), so the
      editor can offer it on a wire whose types don't meet, and a Convert node can use it.
      `True` takes the types from the node (its first wire-only input, its one output); or
      `("WAV", "AUDIO")`. `lossless=True` says the inverse gives the value back.

    The decorator only tags the function (as impressionist's `@checkpoint` does); the engine
    class compiles it.
    """
    if callable(display):
        return node()(display)
    cat = (category,) if isinstance(category, str) else (tuple(category) if category is not None else None)
    out = output if output is None or isinstance(output, str) else tuple(output)
    pair = isinstance(converts, tuple) and len(converts) == 2 and all(isinstance(t, str) for t in converts)
    if converts is not None and not (isinstance(converts, bool) or pair):
        raise DeclarationError("converts must be True or a (from, to) pair of type ids")
    mark = _Mark(display, id, cat, description, out, family, icon, version, memo, flags, x, converts or None, bool(lossless))

    def tag(fn: F) -> F:
        if not inspect.isfunction(fn):
            raise DeclarationError(f"@node marks a plain method, not {type(fn).__name__}")
        fn.__plumcraft_node__ = mark  # type: ignore[attr-defined]
        return fn

    return tag


def as_text(v: Any) -> str:
    """A value as text: strings as they are, anything else as compact JSON (as the Rust SDK)."""
    return v if isinstance(v, str) else _compact(v)


def _compact(v: Any) -> str:
    return json.dumps(v, separators=(",", ":"), ensure_ascii=False)


def _check_type(py: Any, v: Any, choices: tuple | None) -> Any:
    """`v` as the port's Python type, or `_MISSING` when it isn't one."""
    if py is None:
        return v
    if py == "enum":
        return v if any(v == c and type(v) is type(c) for c in choices or ()) else _MISSING
    if py is bool:
        return v if isinstance(v, bool) else _MISSING
    if isinstance(v, bool):
        return _MISSING
    if py is int:
        return v if isinstance(v, int) else _MISSING
    if py is float:
        return float(v) if isinstance(v, int | float) and math.isfinite(v) else _MISSING
    return v


def _describe(py: Any, choices: tuple | None) -> str:
    if py == "enum":
        return "one of " + ", ".join(_compact(c) for c in choices or ())
    return {int: "an integer", float: "a number", bool: "true or false"}.get(py, "a value")


def _bounds(r: Range) -> str:
    def f(v: float) -> str:
        return _compact(int(v)) if float(v).is_integer() else _compact(v)

    if r.min is not None and r.max is not None:
        return f"{f(r.min)}..{f(r.max)}"
    return f">= {f(r.min)}" if r.min is not None else f"<= {f(r.max)}"  # type: ignore[arg-type]


def _wire_of(t: Any) -> WireType | None:
    """The `WireType` an `Annotated` alias (or a bare `WireType`) declares, if any."""
    if isinstance(t, WireType):
        return t
    if get_origin(t) is Annotated:
        return next((m for m in t.__metadata__ if isinstance(m, WireType)), None)
    return None


def _split(t: Any, where: str) -> tuple[Any, list[Any], bool, WireType | None]:
    """An annotation as (base type, Annotated extras, optional, wire type), unwrapping `X | None`
    and `Annotated` (whose `WireType`, if any, names the port's type)."""
    extras: list[Any] = []
    optional = False
    while True:
        origin = get_origin(t)
        if origin is Annotated:
            extras = list(t.__metadata__) + extras
            t = t.__origin__
        elif origin in (Union, types.UnionType):
            args = [a for a in get_args(t) if a is not type(None)]
            if len(args) != 1 or len(args) == len(get_args(t)):
                raise DeclarationError(f"{where}: a union port must be `X | None`, not {t!r}")
            optional = True
            t = args[0]
        else:
            wires = [e for e in extras if isinstance(e, WireType)]
            if len(wires) > 1:
                raise DeclarationError(f"{where}: one port, two wire types: {', '.join(w.id for w in wires)}")
            return t, [e for e in extras if not isinstance(e, WireType)], optional, (wires[0] if wires else None)


class _Ctx:
    """What compiling one engine class needs: its name for messages, and the wire types seen."""

    def __init__(self, cls: type, declared: list[WireType] | None):
        self.cls = cls
        self.declared = declared
        self.seen: dict[str, WireType] = {}

    def use(self, w: WireType, where: str) -> None:
        if self.declared is not None and not any(d.id == w.id for d in self.declared):
            raise DeclarationError(f"{where}: wire type {w.id} is not in {self.cls.__name__}.wires")
        prev = self.seen.get(w.id) or next((d for d in self.declared or () if d.id == w.id), None)
        if prev is not None and prev != w:
            raise DeclarationError(f"{where}: wire type {w.id} is declared twice, differently")
        self.seen.setdefault(w.id, w)


def _port_type(ctx: _Ctx, t: Any, w: WireType | None, where: str) -> tuple[WireType, Any, tuple | None]:
    """A base annotation (and its wire type, if `Annotated` named one) as (wire type, Python
    kind, choices). The kind is how values are checked: a scalar type, `"enum"`, `str` for a
    wire type over `str` (values arrive as text), or None (values pass through)."""
    if w is not None:
        py = {"STRING": str, "INT": int, "FLOAT": float, "BOOL": bool}.get(w.id) if w.scalar else (str if t is str else None)
        ctx.use(w, where)
        return w, py, None
    if get_origin(t) is Literal:
        choices = get_args(t)
        if not choices or len({type(c) for c in choices}) != 1 or not isinstance(choices[0], str | int | float):
            raise DeclarationError(f"{where}: Literal choices must be non-empty and all str, int or float")
        ctx.use(_ENUM, where)
        return _ENUM, "enum", choices
    if t in _BY_PY:
        sw = _wire_of(_BY_PY[t])
        if sw is not None:
            ctx.use(sw, where)
            return sw, t, None
    raise DeclarationError(f"{where}: {t!r} has no port meaning; use a scalar, a Literal, or an Annotated[..., WireType(...)] alias")


def _in_port(ctx: _Ctx, arg: str, ann: Any, default: Any, where: str) -> _In:
    base, extras, optional, wire = _split(ann, where)
    w, py, choices = _port_type(ctx, base, wire, where)
    rng = next((e for e in extras if isinstance(e, Range)), None)
    path = next((e for e in extras if isinstance(e, File)), None)
    meta = next((e for e in extras if isinstance(e, Port)), Port())
    for e in extras:
        if not isinstance(e, Range | File | Port):
            raise DeclarationError(f"{where}: unknown annotation {e!r}")
    if rng is not None and py not in (int, float):
        raise DeclarationError(f"{where}: Range applies to int and float ports, not {w.id}")
    if rng is not None and rng.min is not None and rng.max is not None and rng.min > rng.max:
        raise DeclarationError(f"{where}: Range min {rng.min} is above max {rng.max}")
    if path is not None and py is not str:
        raise DeclarationError(f"{where}: File applies to str ports")
    if default is _MISSING:
        default = None
    elif default is None:
        if not optional:
            raise DeclarationError(f"{where}: a default of None needs the annotation `X | None`")
    else:
        checked = _check_type(py, default, choices) if py is not str else (default if isinstance(default, str) else _MISSING)
        if checked is _MISSING:
            raise DeclarationError(f"{where}: default {default!r} is not {_describe(py, choices) if py is not str else 'a str'}")
        default = checked
        if rng is not None and ((rng.min is not None and default < rng.min) or (rng.max is not None and default > rng.max)):
            raise DeclarationError(f"{where}: default {default!r} is outside {_bounds(rng)}")
    name = meta.name or (arg[:-1] if arg.endswith("_") and not arg.endswith("__") else arg)
    port: dict[str, Any] = {"name": name, "type": w.id}
    if meta.label is not None:
        port["label"] = meta.label
    if meta.tooltip is not None:
        port["tooltip"] = meta.tooltip
    if default is not None:
        port["default"] = default
    if rng is not None:
        for k in ("min", "max", "step"):
            if getattr(rng, k) is not None:
                port[k] = float(getattr(rng, k))
    if meta.decimals is not None:
        port["decimals"] = meta.decimals
    if meta.widget is not None:
        port["widget"] = meta.widget
    if choices is not None:
        port["choices"] = list(choices)
    if optional:
        port["optional"] = True
    if meta.advanced:
        port["advanced"] = True
    socket = meta.socket or (None if w.scalar else "socket")
    if socket is not None:
        port["socket"] = socket
    if path is not None:
        port["path"] = path.to_json()
    return _In(port, arg, py, choices, rng, default)


def _is_cx(name: str, ann: Any) -> bool:
    return name == "cx" or ann is Cx


def _hints(obj: Any, where: str) -> dict[str, Any]:
    try:
        return typing.get_type_hints(obj, include_extras=True)
    except NameError as e:
        raise DeclarationError(f"{where}: can't resolve an annotation ({e}); define the types it names at module level") from None


def _out_type(ctx: _Ctx, ann: Any, where: str) -> WireType:
    base, extras, optional, wire = _split(ann, where)
    if extras or optional:
        raise DeclarationError(f"{where}: an output is a plain type (no Range, File, Port or None)")
    return _port_type(ctx, base, wire, where)[0]


def _outputs(ctx: _Ctx, ret: Any, mark: _Mark, where: str) -> tuple[list[dict], list[str], bool]:
    if ret is None or ret is type(None):
        if mark.output is not None:
            raise DeclarationError(f"{where}: output names given but the method returns None")
        return [], [], False
    if isinstance(ret, type) and issubclass(ret, tuple) and hasattr(ret, "_fields"):
        hints = _hints(ret, f"{where} -> {ret.__name__}")
        fields = list(ret._fields)  # type: ignore[attr-defined]
        names = list(mark.output) if isinstance(mark.output, tuple) else fields
        if isinstance(mark.output, str) or len(names) != len(fields):
            raise DeclarationError(f"{where}: output= must name all {len(fields)} fields of {ret.__name__}")
        ports = []
        for f, n in zip(fields, names, strict=True):
            if f not in hints:
                raise DeclarationError(f"{where}: field {ret.__name__}.{f} has no annotation")
            w = _out_type(ctx, hints[f], f"{where} -> {ret.__name__}.{f}")
            ports.append({"name": n, "type": w.id})
        return ports, names, True
    w = _out_type(ctx, ret, f"{where} -> return")
    if isinstance(mark.output, tuple):
        raise DeclarationError(f"{where}: several output names need a NamedTuple return annotation")
    name = mark.output or w.id.lower()
    return [{"name": name, "type": w.id}], [name], False


def _compile_node(ctx: _Ctx, cls: type, method: str, fn: Callable[..., Any], mark: _Mark) -> NodeSpec:
    engine_id = getattr(cls, "id", None)
    node_id = mark.id or (f"{engine_id}.{method}" if engine_id else method)
    where = f"{cls.__name__}.{method} ({node_id})"
    sig = inspect.signature(fn)
    hints = _hints(fn, where)
    if "return" not in hints:
        raise DeclarationError(f"{where}: needs a return annotation (`-> None` for a node with no outputs)")
    params: list[_In | _Group] = []
    cx_arg = None
    for i, (arg, p) in enumerate(sig.parameters.items()):
        if i == 0:
            continue  # self
        if p.kind in (p.VAR_POSITIONAL, p.VAR_KEYWORD, p.POSITIONAL_ONLY):
            raise DeclarationError(f"{where}: *args, **kwargs and positional-only parameters have no port meaning; use the low-level Node for variadics")
        ann = hints.get(arg, _MISSING)
        if _is_cx(arg, ann):
            cx_arg = arg
            continue
        if ann is _MISSING:
            raise DeclarationError(f"{where}: parameter {arg} needs an annotation")
        default = _MISSING if p.default is p.empty else p.default
        if dataclasses.is_dataclass(ann) and isinstance(ann, type):
            if default is not _MISSING:
                raise DeclarationError(f"{where}: dataclass parameter {arg} takes its defaults from its fields")
            fh = _hints(ann, f"{where}: {ann.__name__}")
            fields = []
            for f in dataclasses.fields(ann):
                fd = f.default if f.default is not dataclasses.MISSING else (f.default_factory() if f.default_factory is not dataclasses.MISSING else _MISSING)
                fields.append((f.name, _in_port(ctx, f.name, fh[f.name], fd, f"{where}: {ann.__name__}.{f.name}")))
            params.append(_Group(ann, arg, fields))
        else:
            params.append(_in_port(ctx, arg, ann, default, f"{where}: {arg}"))
    inputs = [p.port for g in params for p in ([f for _, f in g.fields] if isinstance(g, _Group) else [g])]
    seen: set[str] = set()
    for port in inputs:
        if port["name"] in seen:
            raise DeclarationError(f"{where}: two inputs named {port['name']}")
        seen.add(port["name"])
    outs, names, tuple_out = _outputs(ctx, hints["return"], mark, where)
    if len(set(names)) != len(names):
        raise DeclarationError(f"{where}: two outputs with one name")
    doc = mark.description if mark.description is not None else (inspect.cleandoc(fn.__doc__) if fn.__doc__ else None)
    category = list(mark.category) if mark.category is not None else _category(cls)
    d: dict[str, Any] = {"id": node_id, "display": mark.display or method.replace("_", " ").strip().capitalize()}
    if category:
        d["category"] = category
    if doc:
        d["description"] = doc
    d["inputs"] = inputs
    d["outputs"] = outs
    if mark.flags:
        d["flags"] = dict(mark.flags)
    family = mark.family or getattr(cls, "family", None)
    if family:
        d["family"] = family
    icon = mark.icon or getattr(cls, "icon", None)
    if icon:
        d["icon"] = icon
    if mark.version is not None:
        if not isinstance(mark.version, int) or isinstance(mark.version, bool) or mark.version < 1:
            raise DeclarationError(f"{where}: version must be a positive integer")
        d["version"] = mark.version
    if mark.memo is not None:
        memo = {"scope": mark.memo} if isinstance(mark.memo, str) else dict(mark.memo)
        if memo.get("scope", "machine") not in ("none", "session", "machine", "global"):
            raise DeclarationError(f"{where}: memo scope must be none, session, machine or global")
        d["memo"] = memo
    if mark.x:
        d["x"] = dict(mark.x)
    if mark.converts:
        d["converts"] = _converts(where, mark.converts, mark.lossless, inputs, outs)
    return NodeSpec(method, fn, d, params, cx_arg, names, tuple_out)


def _converts(where: str, converts: bool | tuple[str, str], lossless: bool, inputs: list[dict], outs: list[dict]) -> dict:
    """A node's `converts` mark, checked against its ports."""
    wired = [p for p in inputs if p.get("type") not in SCALARS and p.get("socket", "socket") != "widget"]
    if converts is True:
        if not wired or len(outs) != 1:
            raise DeclarationError(f"{where}: converts=True needs a wire-only input and exactly one output; name the types: converts=(from, to)")
        frm, to = wired[0]["type"], outs[0]["type"]
    else:
        frm, to = converts  # type: ignore[misc]
    inp = next((p for p in wired if p.get("type") == frm), None)
    out = next((p for p in outs if p.get("type") == to), None)
    if inp is None or out is None or frm == to:
        raise DeclarationError(f"{where}: converts {frm} to {to}, but it has no wire-only {frm} input and {to} output")
    mark = {"from": frm, "to": to, "input": inp["name"], "output": out["name"]}
    if lossless:
        mark["lossless"] = True
    return mark


def _category(cls: type) -> list[str]:
    c = getattr(cls, "category", None)
    if c is None:
        return []
    return [c] if isinstance(c, str) else list(c)


def compile_engine(cls: type[Engine], reserved: set[str]) -> None:
    """Compile an engine class's `@node` methods into `cls.__plumcraft_nodes__` and its wire
    types into `cls.__plumcraft_wire_types__`; raise `DeclarationError` on any mistake."""
    raw = cls.__dict__.get("wires")
    declared: list[WireType] | None = None
    if raw is not None:
        if isinstance(raw, str | bytes) or not isinstance(raw, Iterable):
            raise DeclarationError(f"{cls.__name__}.wires must be a tuple of WireType aliases")
        declared = []
        for t in raw:
            w = _wire_of(t)
            if w is None:
                raise DeclarationError(f"{cls.__name__}.wires: {t!r} is not a WireType or an Annotated[..., WireType(...)] alias")
            if any(d.id == w.id for d in declared):
                raise DeclarationError(f"{cls.__name__}.wires: {w.id} is listed twice")
            declared.append(w)
    else:
        inherited = getattr(cls, "__plumcraft_wire_types__", None)
        declared = list(inherited) if inherited and getattr(cls, "__plumcraft_explicit__", False) else None
    ctx = _Ctx(cls, declared)
    for w in getattr(cls, "__plumcraft_wire_types__", ()):
        if declared is None:
            ctx.seen.setdefault(w.id, w)
    specs: dict[str, NodeSpec] = dict(getattr(cls, "__plumcraft_nodes__", {}))
    by_method = {s.method: k for k, s in specs.items()}
    for method, fn in cls.__dict__.items():
        mark = getattr(fn, "__plumcraft_node__", None)
        if mark is None:
            if method in by_method:
                specs.pop(by_method[method])  # overridden by a plain method: no longer a node
            continue
        if method in reserved:
            raise DeclarationError(f"{cls.__name__}.{method}: the name is the Engine's own; name the method otherwise and pass id=")
        spec = _compile_node(ctx, cls, method, fn, mark)
        if method in by_method:
            specs.pop(by_method[method])
        nid = spec.definition["id"]
        if nid in specs:
            raise DeclarationError(f"{cls.__name__}.{method}: node id {nid} is also {cls.__name__}.{specs[nid].method}")
        specs[nid] = spec
        by_method[method] = nid
    cls.__plumcraft_nodes__ = specs  # type: ignore[attr-defined]
    cls.__plumcraft_wire_types__ = tuple(declared) if declared is not None else tuple(ctx.seen.values())  # type: ignore[attr-defined]
    cls.__plumcraft_explicit__ = declared is not None  # type: ignore[attr-defined]


def bind(engine: Engine) -> None:
    """Add a compiled class's wire types and nodes to a fresh instance."""
    cls = type(engine)
    for w in getattr(cls, "__plumcraft_wire_types__", ()):
        engine.with_wire_type(w.to_json())
    for spec in getattr(cls, "__plumcraft_nodes__", {}).values():
        engine.with_node(_Bound(spec, engine))
