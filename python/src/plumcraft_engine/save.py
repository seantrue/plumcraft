"""Save Result (`output.save`, spec 15; the maintainer, 2026-11-22: "Make the save an basic
feature"), as the Rust SDK's `crates/engine/src/save.rs`: a node's value written to a file in a
format the engine declares.

**Formats.** A `Format` is a catalogue `save_formats` entry (`id`, `label`, `extensions`,
`options`, `media`) and the function that turns a value into its bytes. An engine gets the
defaults for every core type its nodes output with no code at all: TEXT as `.txt`, TABLE as
`.csv` (or `.json`), JSON as `.json`, IMAGE as `.png` or `.jpg`, ARRAY as `.png` (its pixels when
8-bit, else the viewer's `auto` view) or `.npy`, SVG as `.svg`, AUDIO as `.wav` or `.npy`. It adds
or replaces formats with `Engine.with_save_format` (or the class attribute `save_formats`), takes
one away with `Engine.without_save_format`, and gives a node its own list, in order, with
`@node(save_formats=[...])` (`NodeDef.save_formats`, spec 7.3).

**A save** (`save`) runs the graph for the node as a run does (a memo hit after a run), picks the
format by `format` (else the path's extension, else the node's first), checks the options against
the format's (`config` otherwise) and fills in their defaults, encodes the value and writes it: to
`path` when the request names one, else returns each file as a blob (`files`), which the editor
writes (spec 13.2 step 8). A list saves as numbered files (`<stem>-00.<ext>`, …), or one item with
`index`; `sheet` is refused with `config` (no default makes sheets). A failure is data: `ok:
false` and the error, the node's own when computing it failed. The same bytes as the Rust SDK for
text, CSV, WAV and `.npy`; pictures decode alike, and JSON matches but for how a float is spelled.
"""

from __future__ import annotations

import io
import itertools
import json
import os
from collections.abc import Callable
from dataclasses import dataclass, field, replace
from typing import Any

import numpy as np

from . import Blob, CancelToken, List, NodeError, arrays, data
from .audio import AudioValue

TEXT, TABLE, JSON, IMAGE, ARRAY, SVG, AUDIO = "TEXT", "TABLE", "JSON", "IMAGE", "ARRAY", "SVG", "AUDIO"
SVG_MEDIA = "image/svg+xml"
# The largest file a format reads back from a value (`plumcraft_store::MAX_READ`'s order).
MAX_READ = 1 << 30


@dataclass(frozen=True)
class SaveCx:
    """What an encoder is told besides the value: the format's options (checked, with their
    defaults), what made the value (`recipe`: the whole workflow), and a list item's label."""

    options: dict
    recipe: Any = None
    label: str = ""


Encode = Callable[[Any, SaveCx], bytes]


@dataclass(frozen=True)
class Format:
    """A save format: what the catalogue lists, and how it writes. `media_type` is the files'
    (`image/png`), for a file returned as a blob; `recipe` says its files carry `SaveCx.recipe`."""

    id: str
    label: str
    extensions: tuple[str, ...]
    media: tuple[str, ...]
    media_type: str
    encode: Encode
    options: tuple[dict, ...] = field(default=())
    recipe: bool = False

    def with_option(self, port: dict) -> Format:
        """With option `port` (a scalar port dict: `name`, `type`, `default`, `choices`, `min`, `max`)."""
        kept = tuple(p for p in self.options if p.get("name") != port.get("name"))
        return replace(self, options=(*kept, dict(port)))

    def carrying_recipe(self) -> Format:
        return replace(self, recipe=True)

    def definition(self) -> dict:
        """The catalogue's `save_formats` entry."""
        extensions = [e.lstrip(".") for e in self.extensions]
        d: dict[str, Any] = {"id": self.id, "label": self.label, "extensions": extensions, "options": [dict(p) for p in self.options]}
        if self.media:
            d["media"] = list(self.media)
        return d


def option(name: str, label: str, type_id: str, default: Any, **more: Any) -> dict:
    """An option port: `name` of scalar type `type_id`, its `label` and `default` (and `choices`,
    `min`, `max`, …)."""
    return {"name": name, "type": type_id, "label": label, "default": default, **more}


def _config(message: str) -> NodeError:
    return NodeError(NodeError.CONFIG, message)


def csv_bytes(t: Any) -> bytes:
    """A table's CSV: a header of the column names, then a line per row; a field with a comma, a
    quote or a line break is quoted, a quote doubled; numbers as JSON writes them, null empty,
    anything nested as compact JSON."""
    cols = t.get("columns") if isinstance(t, dict) else None
    if not isinstance(cols, list):
        raise _config('CSV saves a table: {"columns": [{name, values}]}')

    def text(v: Any) -> str:
        if v is None:
            return ""
        if isinstance(v, str):
            s = v
        else:
            s = json.dumps(v, separators=(",", ":"), ensure_ascii=False)
        return '"' + s.replace('"', '""') + '"' if any(c in s for c in ',"\n\r') else s

    names = [text(c.get("name") if isinstance(c, dict) else None) for c in cols]
    values = [c.get("values") if isinstance(c, dict) and isinstance(c.get("values"), list) else [] for c in cols]
    rows = max((len(v) for v in values), default=0)
    out = ",".join(names) + "\n"
    for r in range(rows):
        out += ",".join(text(v[r]) if r < len(v) else "" for v in values) + "\n"
    return out.encode("utf-8")


def wav_bytes(a: AudioValue, int16: bool) -> bytes:
    """A RIFF WAVE file of `a`: 32-bit float (format 3), or 16-bit PCM, each sample clamped to
    -1..1 and scaled by 32767, rounded half away from zero (as the Rust SDK)."""
    fmt, bits = (1, 16) if int16 else (3, 32)
    ch = max(a.channels, 1)
    block = ch * bits // 8
    if int16:
        x = np.nan_to_num(np.clip(a.samples.astype(np.float64), -1.0, 1.0), nan=0.0) * 32767.0
        payload = np.clip(np.sign(x) * np.floor(np.abs(x) + 0.5), -32767, 32767).astype("<i2").tobytes()
    else:
        payload = np.ascontiguousarray(a.samples, dtype="<f4").tobytes()
    size = min(len(payload), 0xFFFFFFFF)
    head = b"RIFF" + (min(size + 36, 0xFFFFFFFF)).to_bytes(4, "little") + b"WAVEfmt " + (16).to_bytes(4, "little")
    head += fmt.to_bytes(2, "little") + ch.to_bytes(2, "little") + a.sample_rate.to_bytes(4, "little")
    head += min(a.sample_rate * block, 0xFFFFFFFF).to_bytes(4, "little") + block.to_bytes(2, "little") + bits.to_bytes(2, "little")
    return head + b"data" + size.to_bytes(4, "little") + payload


def npy_bytes(dtype: str, shape: tuple[int, ...], payload: bytes) -> bytes:
    """A `.npy` file, version 1.0, C order, as the Rust SDK writes it (numpy's own header)."""
    dims = [str(int(n)) for n in shape]
    tup = f"({dims[0]},)" if len(dims) == 1 else "(" + ", ".join(dims) + ")"
    header = f"{{'descr': '{dtype}', 'fortran_order': False, 'shape': {tup}, }}"
    header += " " * ((64 - (10 + len(header) + 1) % 64) % 64) + "\n"
    if len(header) > 0xFFFF:
        raise _config("the array has too many dimensions for .npy")
    return b"\x93NUMPY\x01\x00" + len(header).to_bytes(2, "little") + header.encode("latin-1") + payload


def _array_pixels(a: np.ndarray) -> np.ndarray:
    """An array as a picture's pixels: its own when 8-bit with 1 to 4 channels, else the `auto`
    view's RGB."""
    lay = arrays.layout(a)
    if lay is None:
        raise _config("not a picture: an (H, W) or (H, W, C) array; save it as .npy")
    if a.dtype == np.uint8 and lay[2] in (1, 2, 3, 4):
        return np.ascontiguousarray(np.asarray(a).reshape(lay[0], lay[1]) if lay[2] == 1 else np.asarray(a))
    v = arrays.views(a, "auto")
    if not v:
        raise _config("not a picture: an (H, W) or (H, W, C) array; save it as .npy")
    return (np.clip(v[0][1], 0.0, 1.0) * 255.0 + 0.5).astype(np.uint8)


def _picture(v: Any, fmt: str, quality: int = 90) -> bytes:
    from PIL import Image

    if isinstance(v, Blob) and str(v.meta.get("media", "")).startswith("image/") and v.meta.get("media") != SVG_MEDIA:
        try:
            img = Image.open(io.BytesIO(v.data))
            img.load()
        except Exception as e:  # PIL raises many kinds for bytes it can't read
            raise _config(f"not a picture this engine reads: {e}") from e
    elif isinstance(v, np.ndarray):
        img = Image.fromarray(_array_pixels(v))
    else:
        raise _config("saves a picture: an IMAGE, or an ARRAY of (H, W) or (H, W, C)")
    out = io.BytesIO()
    if fmt == "JPEG":
        img.convert("RGB").save(out, format="JPEG", quality=int(quality))
    else:
        img.save(out, format="PNG")
    return out.getvalue()


def _txt(v: Any, _: SaveCx) -> bytes:
    if isinstance(v, str):
        return v.encode("utf-8")
    if isinstance(v, bool):
        return b"true" if v else b"false"
    if isinstance(v, int | float):
        return json.dumps(v).encode("utf-8")
    raise _config("Text saves text")


def _json(v: Any, _: SaveCx) -> bytes:
    if isinstance(v, Blob | AudioValue | np.ndarray | List):
        raise _config("JSON saves a JSON value")
    return (json.dumps(v, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def _png(v: Any, _: SaveCx) -> bytes:
    if isinstance(v, Blob) and v.meta.get("media") == "image/png":
        return bytes(v.data)
    return _picture(v, "PNG")


def _jpeg(v: Any, cx: SaveCx) -> bytes:
    q = cx.options.get("quality", 90)
    return _picture(v, "JPEG", max(1, min(100, int(q) if isinstance(q, int | float) else 90)))


def _wav(v: Any, cx: SaveCx) -> bytes:
    if not isinstance(v, AudioValue):
        raise _config("WAV saves audio")
    return wav_bytes(v, cx.options.get("bits") == "int16")


def _svg(v: Any, _: SaveCx) -> bytes:
    if isinstance(v, Blob) and v.meta.get("media") == SVG_MEDIA:
        return bytes(v.data)
    raise _config("SVG saves an SVG")


def _npy(v: Any, _: SaveCx) -> bytes:
    if isinstance(v, AudioValue):
        s = np.ascontiguousarray(v.samples, dtype="<f4")
        return npy_bytes("<f4", s.shape, s.tobytes())
    if isinstance(v, np.ndarray):
        a = np.ascontiguousarray(np.asarray(v))
        if a.dtype.byteorder == ">":
            a = a.astype(a.dtype.newbyteorder("<"))
        return npy_bytes(a.dtype.str, a.shape, a.tobytes())
    raise _config("NumPy saves an array")


def txt() -> Format:
    return Format("txt", "Text", ("txt",), ("text",), "text/plain; charset=utf-8", _txt)


def csv() -> Format:
    return Format("csv", "CSV", ("csv",), ("table",), "text/csv; charset=utf-8", lambda v, _: csv_bytes(v))


def json_format() -> Format:
    return Format("json", "JSON", ("json",), ("tree", "table"), "application/json", _json)


def png() -> Format:
    return Format("png", "PNG", ("png",), ("image", "ndarray"), "image/png", _png)


def jpeg() -> Format:
    return Format("jpeg", "JPEG", ("jpg", "jpeg"), ("image", "ndarray"), "image/jpeg", _jpeg).with_option(
        option("quality", "Quality", "INT", 90, min=1.0, max=100.0)
    )


def wav() -> Format:
    bits = option("bits", "Sample format", "ENUM", "float32", choices=["float32", "int16"])
    return Format("wav", "WAV", ("wav",), ("audio",), "audio/wav", _wav).with_option(bits)


def svg() -> Format:
    return Format("svg", "SVG", ("svg",), ("vector",), SVG_MEDIA, _svg)


def npy() -> Format:
    return Format("npy", "NumPy array", ("npy",), ("ndarray", "audio"), data.NPY_MEDIA, _npy)


def for_type(type_id: str) -> list[Format]:
    """The defaults for values of core type `type_id`, the first the default."""
    return {
        TEXT: [txt()],
        TABLE: [csv(), json_format()],
        JSON: [json_format()],
        IMAGE: [png(), jpeg()],
        ARRAY: [png(), jpeg(), npy()],
        SVG: [svg()],
        AUDIO: [wav(), npy()],
    }.get(type_id, [])


# Every default, in catalogue order (a node with no list of its own offers those whose media take
# its output's, in this order).
ORDER = ("txt", "csv", "json", "png", "jpeg", "wav", "svg", "npy")


def formats_of(types: list[str], own: list[Format], removed: list[str], with_defaults: bool) -> list[Format]:
    """The defaults for `types` (unless turned off) in `ORDER`, then `own` (in place of a default
    of the same id), less `removed`."""
    out: list[Format] = []
    if with_defaults:
        found: dict[str, Format] = {}
        for t in types:
            for f in for_type(t):
                found.setdefault(f.id, f)
        out = [found[i] for i in ORDER if i in found]
    for f in own:
        at = next((i for i, x in enumerate(out) if x.id == f.id), None)
        if at is None:
            out.append(f)
        else:
            out[at] = f
    return [f for f in out if f.id not in removed]


def _media_of(schema: dict, type_id: str) -> str:
    w = next((w for w in schema.get("wire_types", []) if w.get("id") == type_id), None)
    if w is not None:
        return w.get("media", "bytes")
    return {TEXT: "text", TABLE: "table", JSON: "tree", IMAGE: "image", ARRAY: "ndarray", SVG: "vector", AUDIO: "audio"}.get(type_id, "bytes")


def formats_for(schema: dict, definition: dict, port: str) -> list[str]:
    """The format ids output `port` of node `definition` offers, its default first (spec 7.3, as
    `SchemaReply::save_formats_for`): the node's own list in order (or every format), those whose
    media take the port's."""
    out = next((o for o in definition.get("outputs", []) if o.get("name") == port), None)
    if out is None:
        return []
    media = _media_of(schema, out.get("type", ""))
    listed = {f["id"]: f for f in schema.get("save_formats", [])}
    ids = definition.get("save_formats")
    ids = list(listed) if ids is None else [i for i in ids if isinstance(i, str)]
    chosen: list[str] = []
    for i in ids:
        f = listed.get(i)
        if f is not None and (not f.get("media") or media in f["media"]) and i not in chosen:
            chosen.append(i)
    return chosen


def _check_option(port: dict, v: Any) -> None:
    name = port.get("name")
    choices = port.get("choices") or []
    if choices and v not in choices:
        raise _config(f"option {name!r} must be one of {json.dumps(choices)}, not {json.dumps(v)}")
    t = port.get("type")
    if t in ("INT", "FLOAT"):
        ok = isinstance(v, int | float) and not isinstance(v, bool) and (t == "FLOAT" or float(v).is_integer())
        if not ok:
            raise _config(f"option {name!r} must be a {t} number")
        lo, hi = port.get("min"), port.get("max")
        if (lo is not None and v < lo) or (hi is not None and v > hi):
            raise _config(f"option {name!r} must be from {lo} to {hi}, not {v}")
    elif t == "BOOL" and not isinstance(v, bool):
        raise _config(f"option {name!r} must be true or false")
    elif t in ("STRING", "ENUM") and not isinstance(v, str):
        raise _config(f"option {name!r} must be text")


def options(f: Format, given: Any) -> dict:
    """`given` checked against `f`'s options, with each one's default where it isn't given."""
    given = given if isinstance(given, dict) else {}
    out: dict[str, Any] = {}
    for k, v in given.items():
        port = next((p for p in f.options if p.get("name") == k), None)
        if port is None:
            raise _config(f"{f.label} has no option {str(k)[:64]!r}")
        _check_option(port, v)
        out[k] = v
    for p in f.options:
        out.setdefault(p["name"], p.get("default"))
    return out


def numbered(base: str, n: int, is_list: bool) -> list[str]:
    """File names for `n` items saved to `base` (`stem.ext`): the name for one, `stem-00.ext`, …
    (two digits or more) for a list."""
    if not is_list:
        return [base]
    stem, dot, ext = base.rpartition(".")
    if not dot or not stem:
        stem, ext = base, ""
    ext = f".{ext}" if ext else ""
    width = max(2, len(str(max(n - 1, 0))))
    return [f"{stem}-{i:0{width}d}{ext}" for i in range(n)]


def _write_whole(path: str, payload: bytes) -> None:
    folder = os.path.dirname(path) or "."
    if os.path.isdir(path):
        raise _config(f"{path} is a folder")
    os.makedirs(folder, exist_ok=True)
    tmp = os.path.join(folder, f".{os.path.basename(path)}.{os.getpid()}.{next(_SAVES)}.tmp")
    try:
        with open(tmp, "wb") as f:
            f.write(payload)
        os.replace(tmp, path)
    except OSError as e:
        try:
            os.remove(tmp)
        except OSError:
            pass
        raise NodeError(NodeError.OP, f"{path}: {e}") from e


_SAVES = itertools.count()


def failed(e: NodeError) -> dict:
    return {"ok": False, "paths": [], "recipe": False, "items": 0, "error": e.to_json()}


def save(engine: Any, req: dict, memo: Any, plane: data.Plane, cancel: CancelToken, publisher: str) -> dict:
    """`output.save` for an engine on this SDK's executor (spec 15): see the module docs."""
    try:
        return _save(engine, req if isinstance(req, dict) else {}, memo, plane, cancel, publisher)
    except NodeError as e:
        return failed(e)


def _save(engine: Any, req: dict, memo: Any, plane: data.Plane, cancel: CancelToken, publisher: str) -> dict:
    from . import exec

    def s(k: str) -> str:
        v = req.get(k)
        return v if isinstance(v, str) else ""

    graph = req.get("graph") if isinstance(req.get("graph"), dict) else {}
    if graph.get("kind") != "inline":
        raise NodeError(NodeError.GRAPH, "graphs by handle aren't supported by this engine")
    wf = graph.get("graph") if isinstance(graph.get("graph"), dict) else {}
    node_id = s("node")
    wn = (wf.get("nodes") or {}).get(node_id) if isinstance(wf.get("nodes"), dict) else None
    kind = wn.get("kind") if isinstance(wn, dict) else None
    node = engine.nodes.get(kind) if isinstance(kind, str) else None
    if node is None:
        raise _config(f"{node_id[:64]!r} isn't a node of this engine's in the graph")
    d = node.definition
    port = s("port") or next((o.get("name", "") for o in d.get("outputs", [])), "")
    if not any(o.get("name") == port for o in d.get("outputs", [])):
        raise _config(f"{node_id} has no output {port[:64]!r}")
    schema = engine.schema()
    every = {f.id: f for f in engine.all_save_formats()}
    formats = [every[i] for i in formats_for(schema, d, port) if i in every]
    if not formats:
        raise _config(f"{node_id}.{port} has no save format")
    ids = ", ".join(f.id for f in formats)
    path = req.get("path") if isinstance(req.get("path"), str) else None
    if s("format"):
        f = next((f for f in formats if f.id == s("format")), None)
        if f is None:
            raise _config(f"{node_id}.{port} doesn't save as {s('format')[:64]!r}: {ids}")
    else:
        ext = os.path.splitext(path)[1].lstrip(".").lower() if path else ""
        if ext:
            f = next((f for f in formats if any(e.lstrip(".").lower() == ext for e in f.extensions)), None)
            if f is None:
                raise _config(f"{node_id}.{port} doesn't save .{ext} files: {ids}")
        else:
            f = formats[0]
    opts = options(f, req.get("options"))
    run = {
        "run_id": f"save-{os.getpid()}-{next(_SAVES)}",
        "generation": 0,
        "graph": graph,
        "targets": [node_id],
        "inputs": req.get("inputs") if isinstance(req.get("inputs"), dict) else {},
        "trusted_roots": req.get("trusted_roots") if isinstance(req.get("trusted_roots"), list) else [],
    }
    reply, values = exec.run_values(engine, run, lambda _rec: None, cancel, publisher, memo, plane)
    if cancel.is_cancelled():
        raise NodeError.cancelled()
    err = (reply.get("nodes", {}).get(node_id) or {}).get("error") or reply.get("error")
    if err:
        raise NodeError(err.get("kind", NodeError.OP), err.get("message", ""))
    if port not in values.get(node_id, {}):
        raise NodeError(NodeError.OP, f"{node_id}.{port} made no value")
    value = values[node_id][port]
    index = req.get("index")
    if index is not None and (not isinstance(index, int) or isinstance(index, bool) or index < 0):
        raise _config('"index" must be a whole number')
    if isinstance(value, List):
        if req.get("sheet") is True:
            raise _config(f"{f.label} can't put a list on one sheet: save its {len(value.items)} items instead")
        if index is not None:
            if index >= len(value.items):
                raise _config(f"index {index} is past the list's end")
            items, labels, is_list = [value.items[index]], [value.label(index)], False
        else:
            items, labels, is_list = list(value.items), [value.label(i) for i in range(len(value.items))], True
    else:
        if index not in (None, 0):
            raise _config(f"{node_id}.{port} is one value, not a list")
        items, labels, is_list = [value], [""], False
    recipe = req.get("recipe_graph") if req.get("recipe_graph") is not None else wf
    encoded = []
    for item, label in zip(items, labels, strict=True):
        if cancel.is_cancelled():
            raise NodeError.cancelled()
        b = f.encode(item, SaveCx(opts, recipe, label))
        if not isinstance(b, bytes | bytearray):
            raise NodeError(NodeError.OP, f"{f.label} made {type(b).__name__}, not bytes")
        encoded.append(bytes(b))
    ext = f.extensions[0].lstrip(".") if f.extensions else ""
    base = os.path.basename(path) if path else ""
    base = base or (f"result.{ext}" if ext else "result")
    names = numbered(base, len(encoded), is_list)
    out: dict[str, Any] = {"ok": True, "paths": [], "recipe": f.recipe, "items": len(encoded), "error": None}
    if is_list:
        out["labels"] = labels
    if path is not None:
        folder = os.path.dirname(path)
        for name, b in zip(names, encoded, strict=True):
            target = os.path.join(folder, name) if is_list else path
            _write_whole(target, b)
            out["paths"].append(target)
    else:
        files = []
        for name, b in zip(names, encoded, strict=True):
            blob = Blob(handle=data.raw(b), meta={"media": f.media_type, "size": len(b), "form": {"kind": "bytes"}}, data=b)
            files.append({"name": name, "ref": plane.blob_ref(blob)})
        out["files"] = files
    return out
