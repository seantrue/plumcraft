"""PlumCraft's engine SDK in Python: the same contract as the Rust `plumcraft-engine` crate.

Declare an engine as a class with `@node` methods (`plumcraft_engine.declare`, and
`python/README.md`), and serve it with `MyEngine.serve()`. Underneath is the low-level layer the
declarations compile to: `Node` subclasses with a `definition` dict (or `simple`), collected in an
`Engine(info)` and served with `plumcraft_engine.serve.main` (mode 2, spec 16.3; mode 3 with
`--listen`). The executor emits every status
record the interface requires (spec 9.4); the host answers everything off the run path from a
lock-protected run table (spec 5.5).

Messages are plain dicts in protocol 2's JSON shape. Readers on the UI side default every field
a message leaves out, so this SDK sends what it means and nothing else.

Standard library only. Stdout is never the data channel: diagnostics go to stderr.
"""

from __future__ import annotations

import functools
import hashlib
import os
import sys
import threading
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Any, ClassVar

PROTOCOL = 2

# Protocol error codes (spec 12.2), as JSON-RPC numbers.
PARSE = -32700
INVALID_REQUEST = -32600
UNKNOWN_OP = -32601
BAD_PARAMS = -32602
INTERNAL = -32603
VERSION = -32000
NOT_FOUND = -32003
UNAUTHORIZED = -32005
UNAVAILABLE = -32006

# Capabilities the engine offers whether or not the client listed them (spec 5.3).
_ENGINE_ONLY = {"run.plan", "run.peek", "run.watch", "run.multi", "graph.put", "data.cas"}

# Progress records are sent at most this often per node (spec 9.4).
PROGRESS_EVERY_S = 0.1


def now_ts() -> float:
    """Seconds since the epoch, as status records carry it."""
    return time.time()


class ProtoError(Exception):
    """A protocol error: the reply to a request that could not be served (spec 12.2)."""

    def __init__(self, code: int, message: str):
        super().__init__(message)
        self.code = code
        self.message = message

    def to_json(self) -> dict:
        return {"code": self.code, "message": self.message}


class NodeError(Exception):
    """A node's failure, carried as data in records and replies (spec 12.1)."""

    OP = "op"
    CONFIG = "config"
    GRAPH = "graph"
    SKIPPED = "skipped"
    CANCELLED = "cancelled"
    ENGINE = "engine"
    MISSING_SOURCE = "missing_source"

    def __init__(self, kind: str, message: str):
        super().__init__(message)
        self.kind = kind
        self.message = message

    def to_json(self) -> dict:
        return {"kind": self.kind, "message": self.message}

    @classmethod
    def cancelled(cls) -> NodeError:
        return cls(cls.CANCELLED, "cancelled")


class CancelToken:
    """A run's cancel flag: set at once off the run path, checked by the executor and long nodes."""

    def __init__(self) -> None:
        self._flag = threading.Event()

    def cancel(self) -> None:
        self._flag.set()

    def is_cancelled(self) -> bool:
        return self._flag.is_set()


class Cx:
    """What a running node can do besides compute."""

    def __init__(self, node_id: str, cancel: CancelToken, record: Callable[[dict], None], setup: list[float] | None = None):
        self.node_id = node_id
        self._cancel = cancel
        self._record = record
        self._last_progress: float | None = None
        self._setup = setup if setup is not None else [0.0]

    def cancelled(self) -> bool:
        return self._cancel.is_cancelled()

    def setup_took(self, seconds: float) -> None:
        """Time this call spent getting ready rather than working (waiting for or starting a child
        process). The memo leaves it out when it weighs whether a result is slow enough to persist
        (spec 14.5), so a cold start doesn't make a trivial node look expensive."""
        self._setup[0] += max(0.0, float(seconds))

    def progress(self, fraction: float, stage: str | None = None) -> None:
        """Report progress (0..1) and an optional stage text, rate-limited to one record per 100 ms."""
        now = time.monotonic()
        if self._last_progress is not None and now - self._last_progress < PROGRESS_EVERY_S:
            return
        self._last_progress = now
        rec = status(self.node_id, "progress")
        rec["fraction"] = min(max(float(fraction), 0.0), 1.0)
        if stage is not None:
            rec["stage"] = stage
        self._record(rec)


@dataclass
class List:
    """A list value and the sweeps it spans (spec 7.6 rule 2), as the Rust SDK's `List`: wired into
    an input that takes one item, the node runs once per item.

    `axes` is `[(name, length), …]`, row-major (the first varies slowest), its lengths
    multiplying to `len(items)`. A node may name its own axes (outputs that share one zip
    downstream); one it leaves empty gets `<node>.<port>` as it leaves the node. `labels`, when
    given, says what made each item (a sheet label)."""

    items: list[Any]
    axes: list[tuple[str, int]] = field(default_factory=list)
    labels: list[str] = field(default_factory=list)

    def axes_or_plain(self) -> list[tuple[str, int]]:
        """Its axes, or one anonymous axis when it has none."""
        return list(self.axes) if self.axes else [("", len(self.items))]

    def label(self, i: int) -> str:
        """The label of item `i`, or `""`."""
        return self.labels[i] if 0 <= i < len(self.labels) else ""


@dataclass
class Blob:
    """Encoded bytes named by content (spec 4.4), as the Rust SDK's `Blob` with bytes in memory:
    an image, an SVG, a PDF, any file. `handle` is `md5:raw:` of `data`; `meta` is a BlobMeta dict
    (`media`, `size`, `form`)."""

    handle: str
    meta: dict
    data: bytes
    # A picture of it, (h, w, 4) uint8: the node's thumbnail, and its rendition when watched.
    thumb: Any = None


# The wire type of a port that takes or makes any value (Zip, Unzip, Load and Write Files). An
# engine that declares it gets conversions both ways between it and every other type it declares
# (see `Engine.schema`).
ANY = "ANY"


class Node:
    """One node type. Subclasses set `definition` (a NodeDef dict) and implement `run`."""

    definition: dict = {}

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        raise NotImplementedError

    def volatile(self, inputs: dict[str, Any]) -> Any:
        """What the node's memo key must also cover that its inputs don't: the files a directory
        holds, a file's size and time. Called before the key is made (so it should be cheap);
        None adds nothing."""
        return None


class _Simple(Node):
    def __init__(self, definition: dict, f: Callable[[dict[str, Any]], dict[str, Any]]):
        self.definition = definition
        self._f = f

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        return self._f(inputs)


def simple(definition: dict, f: Callable[[dict[str, Any]], dict[str, Any]]) -> Node:
    """A node from a definition and a function of its inputs."""
    return _Simple(definition, f)


def status(node_id: str, state: str) -> dict:
    """A StatusRecord body (spec 9.3); the executor stamps `seq`, `publisher` and `ts`."""
    return {"node_id": node_id, "state": state}


def _hash_tree(h: Any, root: str, files: list[str]) -> None:
    for f in sorted(files):
        h.update(os.path.relpath(f, root).encode("utf-8") + b"\0")
        try:
            with open(f, "rb") as fh:
                h.update(fh.read())
        except OSError:
            pass
        h.update(b"\0")


def _py_files(path: str) -> list[str]:
    if os.path.isfile(path):
        return [path]
    out = []
    for d, dirs, names in os.walk(path):
        dirs[:] = [x for x in dirs if x != "__pycache__" and not x.startswith(".")]
        out += [os.path.join(d, n) for n in names if n.endswith(".py")]
    return out


def _versions() -> bytes:
    from importlib import metadata

    def version(dist: str) -> str:
        try:
            return metadata.version(dist)
        except metadata.PackageNotFoundError:
            return "-"

    return f"python {sys.version_info[0]}.{sys.version_info[1]}; numpy {version('numpy')}; pyuncommonkit {version('pyuncommonkit')}".encode()


@functools.cache
def sdk_digest() -> str:
    """The MD5 of this SDK's own `.py` files and the versions results depend on (Python's minor
    version, numpy, pyuncommonkit), as the Rust SDK's `SDK_DIGEST`."""
    h = hashlib.md5()
    root = os.path.dirname(os.path.abspath(__file__))
    _hash_tree(h, root, _py_files(root))
    h.update(_versions())
    return h.hexdigest()


def code_digest(cls: type) -> str:
    """The MD5 of the code that defines `cls`: its package's `.py` files when its module is in a
    package, else its module's file alone."""
    mod = sys.modules.get(cls.__module__)
    path = getattr(mod, "__file__", None)
    if not path:
        return "-"
    path = os.path.abspath(path)
    pkg = getattr(mod, "__package__", None)
    root = os.path.dirname(path) if pkg else path
    h = hashlib.md5()
    _hash_tree(h, os.path.dirname(path), _py_files(root))
    return h.hexdigest()


# How slow a node must be (ms of its own work) before its result is persisted (spec 14.5), when
# `PLUMCRAFT_MEMO_PERSIST_MS` (a number of ms, or `off`) doesn't say. As the Rust SDK's.
PERSIST_MS = 1000.0
PERSIST_ENV = "PLUMCRAFT_MEMO_PERSIST_MS"


def persist_ms_from_env() -> float | None:
    v = os.environ.get(PERSIST_ENV)
    if v is None:
        return PERSIST_MS
    if v.strip().lower() == "off":
        return None
    try:
        ms = float(v)
    except ValueError:
        return PERSIST_MS
    return ms if ms >= 0.0 and ms != float("inf") else PERSIST_MS


def dependency_digest(dists: tuple[str, ...]) -> str:
    """The MD5 of these distributions' installed versions (`-` for one that isn't installed)."""
    from importlib import metadata

    def version(dist: str) -> str:
        try:
            return metadata.version(dist)
        except metadata.PackageNotFoundError:
            return "-"

    return hashlib.md5("; ".join(f"{d} {version(d)}" for d in sorted(dists)).encode()).hexdigest()


def salt(prefix: str, code: str, dependencies: tuple[str, ...] = ()) -> str:
    """A memo salt (spec 14.2) that changes whenever code that can change a result does, as the
    Rust SDK's `salt`: persisted records outlive the process. `dependencies` names the packages
    an engine's results depend on, so upgrading one never serves a result the old version made."""
    deps = f"+deps:{dependency_digest(dependencies)}" if dependencies else ""
    return f"{prefix}+code:{code}+sdk:{sdk_digest()}{deps}"


class Engine:
    """An engine: its identity, wire types and nodes. Not tied to a binding.

    Declaratively, subclass it with class attributes and `@node` methods:

        class TextEngine(Engine):
            id = "text"            # node ids default to "<id>.<method>"
            name = "Text"
            version = "0.1.0"
            salt = "py:text+0.1.0"  # default "py:<id>+<version>"; the code digest is added
            category = "Text"      # default palette category for its nodes
            family = "text"        # default family for its nodes
            wires = (Text, String)  # optional: in catalogue order; default as first used

    and call the class with no arguments to build one. At the low level, pass `info` and add
    nodes with `with_node`.
    """

    id: str = ""
    name: str = ""
    version: str = "0.0.0"
    salt: str | None = None
    category: str | tuple[str, ...] | None = None
    family: str | None = None
    icon: str | None = None
    wires: ClassVar[tuple[Any, ...] | None] = None
    # The distributions results depend on (`("opencv-python-headless",)`): their versions join the
    # memo salt, so an upgrade never serves a result the old version made.
    dependencies: ClassVar[tuple[str, ...]] = ()
    # Offer the Vary nodes (`with_vary`) too.
    vary: ClassVar[bool] = False
    # Offer every building block (`with_building_blocks`: Vary, sequences, files) too.
    building_blocks: ClassVar[bool] = False

    def __init_subclass__(cls, **kwargs: Any) -> None:
        super().__init_subclass__(**kwargs)
        from .declare import compile_engine

        compile_engine(cls, _RESERVED)

    @classmethod
    def serve(cls, argv: list[str] | None = None) -> int:
        """Run this engine as a process: mode 2 (connect back to the UI's socket, from
        `--connect unix:<path>` or the environment) or mode 3 (`--listen host:port`)."""
        from .serve import main

        return main(cls, argv)

    def __init__(self, info: dict | None = None):
        if info is None:
            cls = type(self)
            prefix = cls.salt or f"py:{cls.id}+{cls.version}"
            info = {"id": cls.id, "name": cls.name or cls.id, "version": cls.version, "salt": salt(prefix, code_digest(cls), cls.dependencies)}
        self.info = info
        self.wire_types: list[dict] = []
        self.nodes: dict[str, Node] = {}
        self.caps = ["codec.json", "display.stream", "events.poll", "events.push", "events.resume", "run.stats", "status.progress"]
        self.limits = {"max_inline": 64 << 10}
        # Blobs go to the shared commonkit store unless `with_store` says otherwise.
        import os

        self.store_root: str | None = os.environ.get("COMMONKIT_STORE_ROOT") or _default_root()
        self.persist_ms: float | None = persist_ms_from_env()
        from .declare import bind

        bind(self)
        if type(self).building_blocks:
            self.with_building_blocks()
        elif type(self).vary:
            self.with_vary()

    def with_vary(self) -> Engine:
        """Offer the Vary nodes (`plumcraft_engine.vary`: Vary Float, Int, String, Bool, Choices,
        Range, Range 2D and 3D) and the scalar wire types they make, where the engine lacks them."""
        from . import vary

        for w in vary.wire_types():
            if self.wire_type(w["id"]) is None:
                self.wire_types.append(w)
        for n in vary.nodes():
            self.with_node(n)
        return self

    def with_building_blocks(self) -> Engine:
        """Offer every building block: the Vary nodes (`with_vary`), the sequence nodes
        (`plumcraft_engine.seq`: Zip, Outer Join, Unzip) and the file nodes
        (`plumcraft_engine.files`: Select, Find, Load and Write Files), with the `ANY` wire type
        they use."""
        from . import files, seq

        if self.wire_type(ANY) is None:
            self.wire_types.append({"id": ANY, "color": "#9ca3af", "media": "bytes"})
        self.with_vary()
        for n in [*seq.nodes(), *files.nodes()]:
            self.with_node(n)
        return self

    def with_store(self, root: str | None) -> Engine:
        """Publish blobs under `root`; None publishes nothing (every value must then fit inline)."""
        self.store_root = root
        return self

    def with_persist_ms(self, ms: float | None) -> Engine:
        """Persist results whose own work took at least `ms` (spec 14.5); None persists nothing
        unless a node is marked `cache: always`. Persisting needs a store."""
        self.persist_ms = ms
        return self

    def with_wire_type(self, w: dict) -> Engine:
        self.wire_types.append(w)
        return self

    def picture_thumb(self, blob: Blob) -> Any:
        """A thumbnail, (h, w, 4) uint8, for a picture file's value that has none (Load Files'): this
        SDK can't decode pictures, so an engine that can overrides this. None shows no picture."""
        return None

    def with_node(self, n: Node) -> Engine:
        self.nodes[n.definition["id"]] = n
        return self

    def wire_type(self, type_id: str) -> dict | None:
        return next((w for w in self.wire_types if w["id"] == type_id), None)

    def schema(self) -> dict:
        out = {
            "v": PROTOCOL,
            "engine": self.info,
            "wire_types": self.wire_types,
            "nodes": [self.nodes[k].definition for k in sorted(self.nodes)],
        }
        # With ANY declared: the base conversions, plus every type to and from ANY.
        if self.wire_type(ANY) is not None:
            compat = [{"from": f, "to": t} for f, t in (("INT", "FLOAT"), ("STRING", "ENUM"), ("ENUM", "STRING"))]
            for w in self.wire_types:
                if w["id"] != ANY:
                    compat += [{"from": w["id"], "to": ANY}, {"from": ANY, "to": w["id"]}]
            out["compat"] = compat
        return out

    def hello(self, req: dict, session: str) -> dict:
        """Answer `hello`: choose the protocol and the session's capabilities."""
        if req.get("op") != "hello":
            raise ProtoError(INVALID_REQUEST, "expected op hello")
        rng = req.get("protocol") if isinstance(req.get("protocol"), dict) else {}
        lo = max(_int(rng.get("min"), PROTOCOL), PROTOCOL)
        hi = min(_int(rng.get("max"), PROTOCOL), PROTOCOL)
        if hi < lo:
            raise ProtoError(VERSION, f"engine speaks protocol {PROTOCOL}–{PROTOCOL}, this app {rng.get('min')}–{rng.get('max')}")
        client = req.get("caps") if isinstance(req.get("caps"), list) else []
        caps = sorted(c for c in self.caps if c in client or c in _ENGINE_ONLY or _is_pack_op(c))
        reply = {"v": PROTOCOL, "protocol": hi, "session": session, "engine": self.info, "caps": caps}
        if self.store_root:
            reply["data"] = {"stores": [self.store_root]}
        return reply


# Names a node method can't take: they are the Engine's own.
_RESERVED = {k for k in vars(Engine) if not k.startswith("__")} | {"info", "wire_types", "wires", "nodes", "caps", "limits", "store_root", "persist_ms"}


def _default_root() -> str:
    from commonkit.shared import setting

    return setting("ROOT")


def _int(v: Any, default: int) -> int:
    return v if isinstance(v, int) and not isinstance(v, bool) else default


def _is_pack_op(cap: str) -> bool:
    parts = cap.split(".")
    return len(parts) == 3 and parts[0] == "x" and all(parts[1:])


from .arrays import ArrayValue  # noqa: E402
from .audio import AudioValue  # noqa: E402
from .declare import (  # noqa: E402  (the declarative layer builds on the names above)
    Array,
    Audio,
    Bool,
    ConfigError,
    DeclarationError,
    File,
    Float,
    Image,
    Int,
    Json,
    OpError,
    Port,
    Range,
    String,
    Svg,
    Table,
    Text,
    WireType,
    node,
)
from .image import image  # noqa: E402
from .svg import svg  # noqa: E402

__all__ = [
    "ANY",
    "Array",
    "ArrayValue",
    "Audio",
    "AudioValue",
    "Blob",
    "Bool",
    "CancelToken",
    "ConfigError",
    "Cx",
    "DeclarationError",
    "Engine",
    "File",
    "Float",
    "Image",
    "Int",
    "Json",
    "List",
    "Node",
    "NodeError",
    "OpError",
    "Port",
    "ProtoError",
    "Range",
    "String",
    "Svg",
    "Table",
    "Text",
    "WireType",
    "image",
    "node",
    "simple",
    "svg",
]
