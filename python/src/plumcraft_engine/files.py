"""Files: Select Files, Find Files, Load Files and Write Files, on the engine's machine, as
`crates/engine/src/files.rs`.

- **Select Files** is the files chosen in a dialog (its `files` widget holds one path per line),
  as a list.
- **Find Files** lists the files in a folder whose names match a pattern (`*.png, *.jpg`; `*` and
  `?`, case ignored), optionally in its subfolders too, sorted; hidden files and folders are
  skipped.
- **Load Files** reads a file into a value (an image, an SVG, a PDF, any bytes, its media type
  from its extension); given a list of paths, it reads each.
- **Write Files** writes values into a folder, each named from `basename` and what made it
  (`trace_corgi_colors-8.svg`: the labels Vary and the file nodes give each item), with the
  extension its media type has. It never overwrites a different file unless `overwrite` is on.

The lists Select and Find make label each file by its name without the extension, so the results
made from them carry it. Their memo keys cover the files' sizes and times (and Find's the
folder's listing), so a changed file is read again.
"""

from __future__ import annotations

import json
import os
import pathlib
from typing import Any

import numpy as np

from . import ANY, Blob, Cx, List, Node, NodeError, data
from .audio import AudioValue
from .media import ext_for, media_for_ext

# The most files Find Files lists.
MAX_FILES = 10_000
# The largest file Load Files reads.
MAX_LOAD = 1 << 30

MISSING_SOURCE = "missing_source"

# Rust's `str::trim` (Unicode White_Space).
_WS = "\t\n\x0b\x0c\r \x85\xa0                　"


def _category() -> list[str]:
    return ["Building blocks", "Files"]


def _config(message: str) -> NodeError:
    return NodeError(NodeError.CONFIG, message)


def _op(message: str) -> NodeError:
    return NodeError(NodeError.OP, message)


def _why(e: OSError) -> str:
    return e.strerror or str(e)


def _text(i: dict[str, Any], key: str) -> str:
    v = i.get(key)
    return v.strip(_WS) if isinstance(v, str) else ""


def _flag(i: dict[str, Any], key: str) -> bool:
    v = i.get(key)
    return v if isinstance(v, bool) else False


def _path_port(name: str, mode: str, default: str, tip: str) -> dict:
    return {"name": name, "type": "STRING", "default": default, "path": {"mode": mode, "filters": []}, "tooltip": tip}


def _files_out() -> dict:
    return {"name": "files", "type": "STRING", "list": True, "socket": "socket"}


def _expand(p: str) -> str:
    """`~` expanded, as the Rust SDK's (`plumcraft_proto::programs::expand_home`): `HOME`, else
    `USERPROFILE` (Windows sets no `HOME` for a program started from the desktop); `~\\` too on
    Windows."""
    home = os.environ.get("HOME") or os.environ.get("USERPROFILE")
    seps = ("/", "\\") if os.name == "nt" else ("/",)
    if p.startswith("~") and home and (p == "~" or p[1:].startswith(seps)):
        return os.path.join(home, p[1:].lstrip("".join(seps)))
    return p


def _stem(p: str) -> str:
    """A file's name without its extension: the label it gives the values made from it."""
    return os.path.splitext(os.path.basename(p))[0]


def _stamp(p: str) -> list:
    """Size and modification time, for memo keys."""
    try:
        st = os.stat(p)
    except (OSError, ValueError):
        return [p]
    return [p, st.st_size, max(st.st_mtime_ns, 0)]


def _is_file(p: str) -> bool:
    try:
        return os.path.isfile(p)
    except ValueError:
        return False


def _is_dir(p: str) -> bool:
    try:
        return os.path.isdir(p)
    except ValueError:
        return False


def _path_list(paths: list[tuple[str, str]]) -> List:
    """A list of paths, each labelled by its stem."""
    return List([p for p, _ in paths], [], [lab for _, lab in paths])


class Select(Node):
    """Select Files: the files chosen in a dialog."""

    definition = {
        "id": "file.select",
        "display": "Select Files",
        "category": _category(),
        "description": "The files chosen with … (several at once), as a list: wire it into a node's file input to run that node once per file, "
        "each result labelled with the file's name.",
        "inputs": [_path_port("files", "files", "", "The files, one path per line: choose them with …")],
        "outputs": [_files_out()],
    }

    @staticmethod
    def chosen(inputs: dict[str, Any]) -> list[str]:
        return [_expand(line.strip(_WS)) for line in _text(inputs, "files").splitlines() if line.strip(_WS)]

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        chosen = self.chosen(inputs)
        if not chosen:
            raise _config("no files chosen: choose some with …")
        missing = [p for p in chosen if not _is_file(p)]
        if missing:
            raise NodeError(MISSING_SOURCE, f"not a file: {', '.join(missing)}")
        return {"files": _path_list([(p, _stem(p)) for p in chosen])}

    def volatile(self, inputs: dict[str, Any]) -> Any:
        return [_stamp(p) for p in self.chosen(inputs)]


def glob_match(pat: str, name: str) -> bool:
    """Does `name` match glob `pat` (`*`, `?`; case ignored)?"""
    p, n = pat.lower(), name.lower()
    # the classic two-pointer match with one backtrack point
    i = j = mark = 0
    star: int | None = None
    while j < len(n):
        c = p[i] if i < len(p) else None
        if c == "*":
            star, mark = i, j
            i += 1
        elif c is not None and (c == "?" or c == n[j]):
            i += 1
            j += 1
        elif star is not None:
            i = star + 1
            mark += 1
            j = mark
        else:
            return False
    return all(c == "*" for c in p[i:])


class Find(Node):
    """Find Files: a folder's files matching a pattern."""

    definition = {
        "id": "file.find",
        "display": "Find Files",
        "category": _category(),
        "description": "The files in a folder whose names match the pattern (*.png, *.jpg: * and ?, case ignored; several separated by commas), sorted, "
        "optionally in its subfolders too. Wire it into a node's file input to run that node once per file, each result labelled with the file's name.",
        "inputs": [
            _path_port("folder", "dir", "", "The folder to search"),
            {"name": "pattern", "type": "STRING", "default": "*", "tooltip": "File names to match: * and ?, several separated by commas (*.png, *.jpg)"},
            {"name": "recursive", "type": "BOOL", "default": False, "tooltip": "Search the subfolders too"},
        ],
        "outputs": [_files_out()],
    }

    @staticmethod
    def scan(inputs: dict[str, Any]) -> list[tuple[str, str]]:
        """The matching files, sorted, each with its label (its stem; with the folders below the
        searched one when recursive)."""
        folder = _text(inputs, "folder")
        if not folder:
            raise _config("folder: choose a folder to search")
        root = _expand(folder)
        if not _is_dir(root):
            raise NodeError(MISSING_SOURCE, f"folder: {root} isn't a folder")
        pattern = _text(inputs, "pattern")
        pats = [p.strip(_WS) for p in pattern.replace(";", ",").split(",") if p.strip(_WS)] or ["*"]
        recursive = _flag(inputs, "recursive")
        found: list[tuple[str, str]] = []
        dirs = [root]
        while dirs:
            d = dirs.pop()
            try:
                entries = list(os.scandir(d))
            except OSError as e:
                raise _op(f"{d}: {_why(e)}") from None
            for e in entries:
                if e.name.startswith("."):
                    continue
                try:
                    is_dir = e.is_dir(follow_symlinks=False)
                    is_file = e.is_file(follow_symlinks=False) or e.is_file()
                except OSError:
                    continue
                # a link is followed for files only, so a loop of links can't trap the search
                if is_dir:
                    if recursive:
                        dirs.append(e.path)
                elif is_file and any(glob_match(p, e.name) for p in pats):
                    rel = os.path.relpath(e.path, root) if e.path.startswith(root) else e.path
                    found.append((e.path, os.path.splitext(rel)[0]))
                    if len(found) > MAX_FILES:
                        raise _config(f"more than {MAX_FILES} files match: narrow the pattern")
        # as Rust sorts paths: component by component
        found.sort(key=lambda f: (pathlib.PurePath(f[0]).parts, f[1]))
        return found

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        found = self.scan(inputs)
        if not found:
            raise _config(f"no files match {json.dumps(_text(inputs, 'pattern'), ensure_ascii=False)}")
        return {"files": _path_list(found)}

    def volatile(self, inputs: dict[str, Any]) -> Any:
        try:
            return [_stamp(p) for p, _ in self.scan(inputs)]
        except NodeError:
            return None


def _form_for(media: str) -> dict:
    """The form a file's media type suggests (sizes left for the reader)."""
    if media == "image/svg+xml":
        return {"kind": "vector", "width": 0.0, "height": 0.0}
    if media.startswith("image/"):
        return {"kind": "image", "width": 0, "height": 0}
    return {"kind": "bytes"}


class Load(Node):
    """Load Files: a file read into a value."""

    definition = {
        "id": "file.load",
        "display": "Load Files",
        "category": _category(),
        "description": "Read a file into a value: an image, an SVG, a PDF or any bytes, its media type from its extension. "
        "Given a list of files (Find Files, Select Files), it reads each, so wiring its value into, say, an app's Open image input opens every file.",
        "inputs": [
            {
                "name": "file",
                "type": "STRING",
                "default": "",
                "socket": "both",
                "path": {"mode": "open", "filters": [{"name": "Any file", "extensions": []}]},
                "tooltip": "The file to read; wire Find Files or Select Files to read each",
            }
        ],
        "outputs": [{"name": "value", "type": ANY, "socket": "socket"}],
    }

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        file = _text(inputs, "file")
        if not file:
            raise _config("file: choose a file, or wire Find Files or Select Files")
        path = _expand(file)
        try:
            st = os.stat(path)
        except (OSError, ValueError) as e:
            raise NodeError(MISSING_SOURCE, f"{path}: {_why(e) if isinstance(e, OSError) else e}") from None
        if not _is_file(path) or st.st_size > MAX_LOAD:
            raise _config(f"{path}: not a file of at most {MAX_LOAD} bytes")
        try:
            with open(path, "rb") as f:
                raw = f.read()
        except OSError as e:
            raise _op(f"{path}: {_why(e)}") from None
        media = media_for_ext(os.path.splitext(os.path.basename(path))[1][1:])
        meta = {"media": media, "size": len(raw), "form": _form_for(media)}
        return {"value": Blob(data.raw(raw), meta, raw)}

    def volatile(self, inputs: dict[str, Any]) -> Any:
        file = _text(inputs, "file")
        return _stamp(_expand(file)) if file else None


def _safe(s: str) -> str:
    """A file-name piece: letters, digits, `.`, `-` and `_`; anything else becomes `_`."""
    return "".join(c if c.isalnum() or c in ".-_" else "_" for c in s).strip("_.")


def file_stem(basename: str, label: str) -> str:
    """A file name from a basename and an item's label: `trace` and `corgi · colors 8` make
    `trace_corgi_colors-8`."""
    parts = [_safe(p.strip(_WS).replace(" ", "-")) for p in label.split(" · ")]
    first = [b for b in (_safe(basename),) if b]
    return "_".join(first + [p for p in parts if p])


def _plain(v: Any) -> bool:
    return not isinstance(v, List | Blob | np.ndarray | AudioValue)


def _pretty(v: Any) -> bytes:
    """JSON as serde_json's pretty printer writes it (two-space indent, keys sorted)."""
    try:
        return json.dumps(v, indent=2, ensure_ascii=False, sort_keys=True, allow_nan=False).encode("utf-8")
    except (TypeError, ValueError) as e:
        raise _op(str(e)) from None


def _file_bytes(v: Any) -> tuple[bytes, str]:
    """A value's bytes and extension, as a file holds it."""
    if isinstance(v, Blob):
        return v.data, ext_for(str(v.meta.get("media", "")))
    if isinstance(v, str):
        return v.encode("utf-8"), "txt"
    if isinstance(v, List):
        if not all(_plain(i) for i in v.items):
            raise _config("a tuple of values that aren't plain can't be written as one file: Unzip it first")
        return _pretty(list(v.items)), "json"
    if isinstance(v, np.ndarray | AudioValue):
        raise _config("arrays and audio can't be written as files yet")
    return _pretty(v), "json"


def _read(p: str) -> bytes | None:
    try:
        with open(p, "rb") as f:
            return f.read()
    except (OSError, ValueError):
        return None


class Write(Node):
    """Write Files: values into a folder."""

    definition = {
        "id": "file.write",
        "display": "Write Files",
        "category": _category(),
        "description": "Write values into a folder: each file is named from basename and what made the value "
        "(trace_corgi_colors-8.svg: the file it came from and the settings varied), with its type's extension. "
        "A different file already there is an error unless overwrite is on.",
        "inputs": [
            {"name": "values", "type": ANY, "socket": "socket", "tooltip": "The values to write: one, or a list"},
            _path_port("folder", "dir", "", "The folder to write into (made when missing)"),
            {"name": "basename", "type": "STRING", "default": "out", "tooltip": "The start of every file name; what varied follows it"},
            {"name": "overwrite", "type": "BOOL", "default": False, "tooltip": "Replace files that are already there"},
        ],
        "outputs": [_files_out()],
        "flags": {"input_list": True, "output_node": True},
        "memo": {"scope": "none"},
    }

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        folder = _text(inputs, "folder")
        if not folder:
            raise _config("folder: choose a folder to write into")
        if isinstance(inputs.get("folder"), List):
            raise _config("folder: one folder, not a list")
        d = _expand(folder)
        basename = _text(inputs, "basename")
        overwrite = _flag(inputs, "overwrite")
        if "values" not in inputs:
            raise _config("values: wire what to write")
        v = inputs["values"]
        if isinstance(v, List):
            items, labels, axes = list(v.items), [v.label(i) for i in range(len(v.items))], list(v.axes)
        else:
            items, labels, axes = [v], [""], []
        # plan every name first: nothing is written when any would fail
        stems = [file_stem(basename, lab) for lab in labels]
        numbered = len(items) > 1 and not (all(stems) and len(set(stems)) == len(stems))
        planned: list[tuple[str, bytes]] = []
        for i, (item, stem) in enumerate(zip(items, stems, strict=True)):
            try:
                body, ext = _file_bytes(item)
            except NodeError as e:
                if len(items) > 1:
                    raise NodeError(e.kind, f"item {i + 1} of {len(items)}: {e.message}") from None
                raise
            base = stem or "out"
            name = f"{base}-{i + 1:03}.{ext}" if numbered else f"{base}.{ext}"
            planned.append((os.path.join(d, name), body))
        if not overwrite:
            clash = [p for p, b in planned if os.path.exists(p) and _read(p) != b]
            if clash:
                raise _config(f"already there (turn on overwrite to replace): {', '.join(clash)}")
        try:
            os.makedirs(d, exist_ok=True)
        except (OSError, ValueError) as e:
            raise _op(f"{d}: {_why(e) if isinstance(e, OSError) else e}") from None
        n = len(planned)
        for i, (path, body) in enumerate(planned):
            if cx.cancelled():
                raise NodeError.cancelled()
            cx.progress(i / n, "writing")
            if _read(path) == body:
                continue
            tmp = f"{os.path.splitext(path)[0]}.plumcraft-{os.getpid()}.tmp"
            try:
                with open(tmp, "wb") as f:
                    f.write(body)
                os.replace(tmp, path)
            except OSError as e:
                try:
                    os.remove(tmp)
                except OSError:
                    pass
                raise _op(f"{path}: {_why(e)}") from None
        return {"files": List([p for p, _ in planned], axes, labels)}


def nodes() -> list[Node]:
    """Select, Find, Load and Write Files."""
    return [Select(), Find(), Load(), Write()]
