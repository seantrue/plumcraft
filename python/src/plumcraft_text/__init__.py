"""The text engine in Python: the same nodes as the Rust `plumcraft-text` crate, with the same
catalogue and the same results, so the UI cannot tell them apart except by `engine.salt` and the
`publisher` on status records. The conformance suite runs every case against both.
"""

from __future__ import annotations

import json
import os
import re
import time
from typing import Annotated, Any

import numpy as np

from plumcraft_engine import (
    Array,
    ArrayValue,
    Blob,
    ConfigError,
    Cx,
    Engine,
    File,
    Float,
    Int,
    Json,
    NodeError,
    OpError,
    Range,
    String,
    Table,
    Text,
    WireType,
    node,
)
from plumcraft_engine.data import raw

__version__ = "0.6.1"

# The largest file `text.load` reads, and the longest text `text.repeat` makes.
MAX_LOAD = 64 << 20
MAX_REPEAT = 64 << 20

# The files the craft apps export, as the craft packs declare them (bytes), so their exports
# cross into this engine (as the Rust `files` module; docs/research/20-convert.md, 6.2).
Txt = Annotated[object, WireType("TXT")]
Md = Annotated[object, WireType("MD")]
Html = Annotated[object, WireType("HTML")]
Csv = Annotated[object, WireType("CSV")]
_MEDIA = {"TXT": "text/plain", "MD": "text/markdown", "HTML": "text/html", "CSV": "text/csv"}
MAX_FILE = 64 << 20
MAX_ROWS = 1_000_000
_INT = re.compile(r"-?[0-9]+")
_DECIMAL = re.compile(r"-?([0-9]+(\.[0-9]*)?|\.[0-9]+)([eE][-+]?[0-9]+)?")

# Unicode White_Space, as Rust's `split_whitespace` uses it (Python's `str.split` also splits on
# U+001C..U+001F, which would count words differently).
_WHITESPACE = re.compile("[\t\n\x0b\x0c\r \x85\xa0  -     　]+")


class TextEngine(Engine):
    id = "text"
    name = "Text"
    version = __version__
    salt = f"py:plumcraft-text+{__version__}"
    category = "Text"
    family = "text"
    wires = (Text, String, Int, Float, Table, Array, Json, Txt, Md, Html, Csv)
    # Every building block (Vary, sequences, files), as the Rust engine's `.with_building_blocks()`.
    building_blocks = True

    @node("Text")
    def literal(self, text: str = "") -> Text:
        return text

    @node("Upper case")
    def upper(self, text: Text) -> Text:
        return text.upper()

    @node("Lower case")
    def lower(self, text: Text) -> Text:
        return text.lower()

    @node("Reverse")
    def reverse(self, text: Text) -> Text:
        return text[::-1]

    @node("Replace")
    def replace(self, text: Text, find: str = "", with_: str = "") -> Text:
        if not find:
            raise ConfigError("find is empty")
        return text.replace(find, with_)

    @node("Join")
    def join(self, a: Text, b: Text, sep: str = " ") -> Text:
        return f"{a}{sep}{b}"

    @node("Word count", output="count")
    def words(self, text: Text) -> int:
        return len([w for w in _WHITESPACE.split(text) if w])

    @node("Fail")
    def fail(self, text: Text) -> Text:
        raise OpError("this node always fails")

    # Passes text through after `seconds`, reporting progress and stopping early on cancel.
    @node("Slow", category=("Text", "Testing"))
    def slow(self, text: Text, seconds: Annotated[float, Range(0, 600, invalid="clamp")] = 1.0, *, cx: Cx) -> Text:
        start = time.monotonic()
        while time.monotonic() - start < seconds:
            if cx.cancelled():
                raise NodeError.cancelled()
            done = (time.monotonic() - start) / max(seconds, 1e-12)
            cx.progress(done, f"{done * 100:.0f}%")
            time.sleep(0.01)
        return text

    # As the Rust engine: anything but a non-negative integer repeats twice, a million at most.
    @node("Repeat")
    def repeat(self, text: Text, times: Annotated[int, Range(0, 1e6, invalid="pass")] = 2) -> Text:
        times = min(times, 1_000_000) if isinstance(times, int) and not isinstance(times, bool) and times >= 0 else 2
        if times * len(text.encode("utf-8")) > MAX_REPEAT:
            raise ConfigError(f"the result would exceed {MAX_REPEAT >> 20} MiB")
        return text * times

    # The Latin letters a-z, case-folded, in alphabetical order (as the Rust engine: Rust's
    # `is_alphabetic` and Python's `isalpha` disagree on parts of Unicode).
    @node("Letter counts (a–z)")
    def letters(self, text: Text) -> Table:
        counts: dict[str, int] = {}
        for c in text:
            if c.isascii() and c.isalpha():
                counts[c.lower()] = counts.get(c.lower(), 0) + 1
        keys = sorted(counts)
        return {"columns": [{"name": "letter", "type": "STRING", "values": keys}, {"name": "count", "type": "INT", "values": [counts[k] for k in keys]}]}

    @node("Code points", output="codes")
    def codes(self, text: Text) -> Array:
        return np.array([ord(c) for c in text], dtype="<u4")

    @node("Load text")
    def load(self, path: Annotated[str, File("open")] = "") -> Text:
        if not path:
            raise ConfigError("path is empty")
        try:
            size = os.path.getsize(path)
        except OSError as e:
            raise ConfigError(f"{path}: {e.strerror}") from None
        if size > MAX_LOAD:
            raise ConfigError(f"{path}: larger than {MAX_LOAD >> 20} MiB")
        try:
            with open(path, "rb") as f:
                raw = f.read()
        except OSError as e:
            raise OpError(f"{path}: {e.strerror}") from None
        try:
            return raw.decode("utf-8")
        except UnicodeDecodeError:
            raise OpError(f"{path}: not UTF-8 text") from None

    # Files as text and tables, and back: as the Rust engine's `files` module, rule for rule.
    @node(
        "TXT to Text",
        category=("Text", "Files"),
        description="A TXT file (as WordCraft or PdfCraft exports one) as text: UTF-8, a byte-order mark dropped.",
        converts=True,
        lossless=True,
    )
    def from_txt(self, txt: Txt) -> Text:
        return _utf8(txt, "txt")

    @node("Text to TXT", category=("Text", "Files"), description="Text as a TXT file, UTF-8. The inverse of TXT to Text.", converts=True, lossless=True)
    def to_txt(self, text: Text) -> Txt:
        return _file("TXT", text.encode("utf-8"))

    @node(
        "Markdown to Text",
        category=("Text", "Files"),
        description="A Markdown file as its text, the markup kept: UTF-8, a byte-order mark dropped.",
        converts=True,
        lossless=True,
    )
    def from_md(self, md: Md) -> Text:
        return _utf8(md, "md")

    @node(
        "Text to Markdown",
        category=("Text", "Files"),
        description="Text as a Markdown file, UTF-8. The inverse of Markdown to Text.",
        converts=True,
        lossless=True,
    )
    def to_md(self, text: Text) -> Md:
        return _file("MD", text.encode("utf-8"))

    @node(
        "HTML to Text",
        category=("Text", "Files"),
        description="An HTML file as its source text, the markup kept: UTF-8, a byte-order mark dropped.",
        converts=True,
        lossless=True,
    )
    def from_html(self, html: Html) -> Text:
        return _utf8(html, "html")

    @node(
        "Text to HTML",
        category=("Text", "Files"),
        description="Text (HTML source) as an HTML file, UTF-8. The inverse of HTML to Text.",
        converts=True,
        lossless=True,
    )
    def to_html(self, text: Text) -> Html:
        return _file("HTML", text.encode("utf-8"))

    @node(
        "CSV to Table",
        category=("Text", "Files"),
        description="A CSV file (as GridCraft exports one) as a table: the first row names the columns, and each column's type comes from its values.",
        converts=True,
    )
    def from_csv(self, csv: Csv) -> Table:
        return read_csv(_utf8(csv, "csv"))

    @node("Table to CSV", category=("Text", "Files"), description="A table as a CSV file, a header line first. The inverse of CSV to Table.", converts=True)
    def to_csv(self, table: Table) -> Csv:
        return _file("CSV", write_csv(table).encode("utf-8"))

    # Converters between core types (docs/research/20-convert.md, 6.3), as the Rust `pairs`
    # module.

    @node(
        "Text to Table",
        category=("Text", "Convert"),
        description=(
            "CSV text as a table, as CSV to Table reads a file: the first row names the columns, "
            "and each column's type comes from its values. The inverse of Table to Text."
        ),
        converts=True,
    )
    def text_to_table(self, text: Text) -> Table:
        return read_csv(text)

    @node(
        "Table to Text",
        category=("Text", "Convert"),
        description="A table as CSV text, a header line first, as Table to CSV writes a file. The inverse of Text to Table.",
        converts=True,
    )
    def table_to_text(self, table: Table) -> Text:
        return write_csv(table)

    @node(
        "JSON to Text",
        category=("Text", "Convert"),
        description="A JSON value as text, indented by two spaces, keys sorted. The inverse of Text to JSON.",
        converts=True,
        lossless=True,
    )
    def json_to_text(self, json: Json) -> Text:
        return json_text(json)

    @node(
        "Text to JSON",
        category=("Text", "Convert"),
        description="Text that is JSON as its value. Text that isn't is an error. The inverse of JSON to Text.",
        converts=True,
    )
    def text_to_json(self, text: Text) -> Json:
        return parse_json(text)

    @node(
        "JSON to Table",
        category=("Text", "Convert"),
        description=(
            "Tabular JSON as a table: rows (a list of lists, the first naming the columns), records (a list of objects, "
            "a column for each key), or columns (an object of lists). Each column's type comes from its values. "
            "Other JSON is an error. The inverse of Table to JSON."
        ),
        converts=True,
    )
    def json_to_table(self, json: Json) -> Table:
        return json_table(_json_value(json))

    @node(
        "Table to JSON",
        category=("Text", "Convert"),
        description="A table as JSON records: a list with an object for each row, keyed by the column names. The inverse of JSON to Table.",
        converts=True,
    )
    def table_to_json(self, table: Table) -> Json:
        return table_records(table)

    @node(
        "Table to Array",
        category=("Text", "Convert"),
        description=(
            "A table's INT and FLOAT columns as a rows × columns array (float64), named by the columns; "
            "a null is NaN, and other columns are left out. The inverse of Array to Table."
        ),
        converts=True,
        shape="series",
    )
    def table_to_array(self, table: Table) -> Array:
        return table_array(table)

    @node(
        "Array to Table",
        category=("Text", "Convert"),
        description=(
            "A 1-D or rows × columns array as a table, a column for each array column, named by the array's names or c1, c2, …: "
            "INT, BOOL or FLOAT as the array's type, NaN as null. The inverse of Table to Array."
        ),
        converts=True,
        shape="series",
    )
    def array_to_table(self, array: Array) -> Table:
        return array_table(array)


def _utf8(value: Any, port: str) -> str:
    data = value.data if isinstance(value, Blob) else None
    if data is None:
        raise ConfigError(f"{port}: wire a {port.upper()} file")
    if len(data) > MAX_FILE:
        raise ConfigError(f"{port}: larger than {MAX_FILE >> 20} MiB")
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError:
        raise ConfigError(f"{port}: not UTF-8 text") from None
    return text[1:] if text.startswith("\ufeff") else text


def _file(ty: str, data: bytes) -> Blob:
    return Blob(handle=raw(data), meta={"media": _MEDIA[ty], "size": len(data), "form": {"kind": "bytes"}}, data=data)


def separator(text: str) -> str:
    """The separator a CSV's first line uses, outside quotes, as the Rust engine picks it."""
    quoting = tab = semi = False
    for c in text:
        if c == '"':
            quoting = not quoting
        elif not quoting:
            if c == ",":
                return ","
            if c == "\t":
                tab = True
            elif c == ";":
                semi = True
            elif c in "\r\n":
                break
    return "\t" if tab else ";" if semi else ","


def csv_rows(text: str) -> list[list[str]]:
    """The rows of a CSV, as the Rust engine reads them."""
    sep = separator(text)
    rows: list[list[str]] = []
    row: list[str] = []
    field: list[str] = []
    quoting = quoted = False

    def end_row() -> None:
        nonlocal row, quoted
        row.append("".join(field))
        field.clear()
        line, row = row, []
        if not (len(line) == 1 and line[0] == "" and not quoted):
            if len(rows) >= MAX_ROWS:
                raise ConfigError(f"csv: more than {MAX_ROWS} rows")
            rows.append(line)
        quoted = False

    i, n = 0, len(text)
    while i < n:
        c = text[i]
        i += 1
        if quoting:
            if c == '"':
                if i < n and text[i] == '"':
                    i += 1
                    field.append('"')
                else:
                    quoting = False
            else:
                field.append(c)
            continue
        if c == '"' and not field and not quoted:
            quoting = quoted = True
        elif c == sep:
            row.append("".join(field))
            field.clear()
            quoted = False
        elif c in "\r\n":
            if c == "\r" and i < n and text[i] == "\n":
                i += 1
            end_row()
        else:
            field.append(c)
    if quoting:
        raise ConfigError("csv: a quoted field doesn't end")
    if field or row or quoted:
        end_row()
    return rows


def _column(cells: list[str | None]) -> tuple[str, list[Any]]:
    present = [c for c in cells if c is not None]

    def every(f: Any) -> bool:
        return bool(present) and all(f(c) for c in present)

    def fits(s: str) -> bool:
        return _INT.fullmatch(s) is not None and -(2**63) <= int(s) < 2**63

    def finite(s: str) -> bool:
        return _DECIMAL.fullmatch(s) is not None and abs(float(s)) != float("inf")

    if every(fits):
        return "INT", [None if c is None else int(c) for c in cells]
    if every(finite):
        return "FLOAT", [None if c is None else float(c) for c in cells]
    # any case: a spreadsheet writes TRUE and FALSE
    if every(lambda s: s.lower() in ("true", "false")):
        return "BOOL", [None if c is None else c.lower() == "true" for c in cells]
    return "STRING", list(cells)


def read_csv(text: str) -> dict:
    """A CSV as a table, as the Rust engine reads it."""
    rows = csv_rows(text)
    if not rows:
        raise ConfigError("csv: no header row")
    header, body = rows[0], rows[1:]
    names = [n or f"column{i + 1}" for i, n in enumerate(header)]
    for i, name in enumerate(names):
        if name in names[:i]:
            raise ConfigError(f"csv: two columns are called {json.dumps(name)}")
    for r, line in enumerate(body):
        if len(line) > len(names):
            raise ConfigError(f"csv: row {r + 2} has {len(line)} fields; the header has {len(names)}")
    columns = []
    for c, name in enumerate(names):
        cells = [line[c] if c < len(line) and line[c] != "" else None for line in body]
        ty, values = _column(cells)
        columns.append({"name": name, "type": ty, "values": values})
    return {"columns": columns}


def _cell(v: Any) -> str:
    if v is None:
        return ""
    if isinstance(v, str):
        return v
    if isinstance(v, bool):
        return "true" if v else "false"
    return json.dumps(v, separators=(",", ":"), ensure_ascii=False)


def _quote(s: str) -> str:
    if any(ch in s for ch in ',"\n\r') or s.startswith(" ") or s.endswith(" "):
        return '"' + s.replace('"', '""') + '"'
    return s


def write_csv(table: Any) -> str:
    """A table as CSV, as the Rust engine writes it."""
    columns = table.get("columns") if isinstance(table, dict) else None
    if not isinstance(columns, list):
        raise ConfigError("table: not a table (no columns)")
    cols = []
    for c in columns:
        values = c.get("values") if isinstance(c, dict) else None
        if not isinstance(values, list):
            raise ConfigError("table: a column without values")
        name = c.get("name") if isinstance(c.get("name"), str) else ""
        cols.append((name, values))
    n = len(cols[0][1]) if cols else 0
    if any(len(v) != n for _, v in cols):
        raise ConfigError("table: its columns differ in length")
    lines = [",".join(_quote(name) for name, _ in cols)]
    lines += [",".join(_quote(_cell(v[r])) for _, v in cols) for r in range(n)]
    return "\n".join(lines) + "\n"


MAX_CELLS = 16 << 20
MAX_TEXT = 64 << 20


def _json_value(value: Any) -> Any:
    """A JSON value, inline or a blob of JSON when large."""
    if isinstance(value, Blob):
        if value.data is None or len(value.data) > MAX_TEXT:
            raise ConfigError("json: wire a JSON value")
        try:
            return json.loads(value.data)
        except ValueError as e:
            raise ConfigError(f"json: the blob isn't JSON ({e})") from None
    return value


def json_text(value: Any) -> str:
    """A JSON value (inline, or a blob of JSON when large) as text, as the Rust engine writes it."""
    value = _json_value(value)
    return json.dumps(value, indent=2, ensure_ascii=False, sort_keys=True, allow_nan=False)


def _is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool) and -(2**63) <= v < 2**63


def _is_number(v: Any) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def _json_column(cells: list[Any]) -> tuple[str, list[Any]]:
    """A column's type and values from JSON values, as the Rust engine types them."""
    present = [v for v in cells if v is not None]

    def every(f: Any) -> bool:
        return bool(present) and all(f(v) for v in present)

    if every(_is_int):
        return "INT", list(cells)
    if every(_is_number):
        return "FLOAT", [None if v is None else float(v) for v in cells]
    if every(lambda v: isinstance(v, bool)):
        return "BOOL", list(cells)

    def text(v: Any) -> Any:
        if v is None or isinstance(v, str):
            return v
        return json.dumps(v, separators=(",", ":"), ensure_ascii=False, sort_keys=True)

    return "STRING", [text(v) for v in cells]


def _json_name(v: Any, i: int) -> str:
    if isinstance(v, str) and v:
        return v
    if v is None or isinstance(v, str):
        return f"column{i + 1}"
    return json.dumps(v, separators=(",", ":"), ensure_ascii=False, sort_keys=True)


def _table_of(names: list[str], rows: list[list[Any]]) -> dict:
    for i, name in enumerate(names):
        if name in names[:i]:
            raise ConfigError(f"json: two columns are called {json.dumps(name)}")
    if len(rows) * max(len(names), 1) > MAX_CELLS:
        raise ConfigError(f"json: more than {MAX_CELLS >> 20} M cells")
    columns = []
    for c, name in enumerate(names):
        ty, values = _json_column([r[c] if c < len(r) else None for r in rows])
        columns.append({"name": name, "type": ty, "values": values})
    return {"columns": columns}


def json_table(v: Any) -> dict:
    """Tabular JSON as a table, as the Rust engine reads it: rows, records, columns or a table."""
    not_tabular = "json: not tabular (rows, records, columns, or a table)"
    if isinstance(v, dict) and isinstance(v.get("columns"), list):
        names, lists = [], []
        for i, c in enumerate(v["columns"]):
            c = c if isinstance(c, dict) else {}
            names.append(_json_name(c.get("name"), i))
            if not isinstance(c.get("values"), list):
                raise ConfigError("json: a column without values")
            lists.append(c["values"])
        n = len(lists[0]) if lists else 0
        if any(len(x) != n for x in lists):
            raise ConfigError("json: its columns differ in length")
        return _table_of(names, [[x[r] for x in lists] for r in range(n)])
    if isinstance(v, dict) and v and all(isinstance(x, list) for x in v.values()):
        keys = sorted(v)
        lists = [v[k] for k in keys]
        n = len(lists[0])
        if any(len(x) != n for x in lists):
            raise ConfigError("json: its columns differ in length")
        return _table_of(keys, [[x[r] for x in lists] for r in range(n)])
    if isinstance(v, list) and v and all(isinstance(r, dict) for r in v):
        names: list[str] = []
        for r in v:
            for k in sorted(r):
                if k not in names:
                    names.append(k)
        return _table_of(names, [[r.get(k) for k in names] for r in v])
    if isinstance(v, list) and v and all(isinstance(r, list) for r in v):
        header, body = v[0], v[1:]
        names = [_json_name(h, i) for i, h in enumerate(header)]
        for r, row in enumerate(body):
            if len(row) > len(names):
                raise ConfigError(f"json: row {r + 2} has {len(row)} values; the header has {len(names)}")
        return _table_of(names, [list(r) for r in body])
    raise ConfigError(not_tabular)


def table_records(table: Any) -> list[dict]:
    """A table as JSON records, as the Rust engine writes them."""
    columns = table.get("columns") if isinstance(table, dict) else None
    if not isinstance(columns, list):
        raise ConfigError("table: not a table (no columns)")
    cols = []
    for c in columns:
        values = c.get("values") if isinstance(c, dict) else None
        if not isinstance(values, list):
            raise ConfigError("table: a column without values")
        name = c.get("name") if isinstance(c.get("name"), str) else ""
        cols.append((name, values))
    n = len(cols[0][1]) if cols else 0
    if any(len(v) != n for _, v in cols):
        raise ConfigError("table: its columns differ in length")
    if n * len(cols) > MAX_CELLS:
        raise ConfigError(f"table: more than {MAX_CELLS >> 20} M cells")
    return [{name: v[r] for name, v in cols} for r in range(n)]


def _no_constant(name: str) -> Any:
    raise ValueError(f"{name} isn't JSON")


def parse_json(text: str) -> Any:
    """Text that is JSON, as its value; NaN and Infinity are refused, as the Rust engine does."""
    if len(text.encode("utf-8")) > MAX_TEXT:
        raise ConfigError(f"text: longer than {MAX_TEXT >> 20} MiB")
    try:
        return json.loads(text, parse_constant=_no_constant)
    except ValueError as e:
        raise ConfigError(f"text: not JSON ({e})") from None


def table_array(table: Any) -> ArrayValue:
    """A table's INT and FLOAT columns as a float64 (rows, columns) table array."""
    columns = table.get("columns") if isinstance(table, dict) else None
    if not isinstance(columns, list):
        raise ConfigError("table: not a table (no columns)")
    numeric = [
        (c.get("name") if isinstance(c.get("name"), str) else "", c["values"])
        for c in columns
        if isinstance(c, dict) and c.get("type") in ("INT", "FLOAT") and isinstance(c.get("values"), list)
    ]
    if not numeric:
        raise ConfigError("table: it has no INT or FLOAT column")
    rows = len(numeric[0][1])
    if any(len(v) != rows for _, v in numeric):
        raise ConfigError("table: its columns differ in length")
    if rows * len(numeric) > MAX_CELLS:
        raise ConfigError(f"table: more than {MAX_CELLS >> 20} M cells")

    def num(v: Any) -> float:
        return float(v) if isinstance(v, int | float) and not isinstance(v, bool) else float("nan")

    data = np.array([[num(v[r]) for _, v in numeric] for r in range(rows)], dtype="<f8").reshape(rows, len(numeric))
    return ArrayValue(data, names=[n for n, _ in numeric], table=True)


def array_table(array: Any) -> dict:
    """A 1-D or 2-D array as a table, as the Rust engine makes it."""
    a = np.asarray(array)
    if a.ndim == 1:
        a = a.reshape(-1, 1)
    if a.ndim != 2:
        raise ConfigError(f"array: a 1-D or rows × columns array, not {'×'.join(str(n) for n in np.shape(array))}")
    rows, cols = a.shape
    if rows * cols > MAX_CELLS:
        raise ConfigError(f"array: more than {MAX_CELLS >> 20} M cells")
    kind = a.dtype.kind
    if not ((kind == "f" and a.dtype.itemsize in (4, 8)) or (kind in "iu" and a.dtype.itemsize in (1, 2, 4, 8)) or kind == "b"):
        raise ConfigError(f"array: its type {a.dtype.str} isn't one a table holds")
    ty = "INT" if kind in "iu" else "BOOL" if kind == "b" else "FLOAT"
    names = list(getattr(array, "names", ()) or ())
    if len(names) != cols:
        names = [f"c{i + 1}" for i in range(cols)]

    def value(v: Any) -> Any:
        return None if isinstance(v, float) and not np.isfinite(v) else v

    return {"columns": [{"name": n, "type": ty, "values": [value(v) for v in a[:, i].tolist()]} for i, n in enumerate(names)]}


def engine() -> TextEngine:
    """The engine, as a host's factory (kept for callers of the low-level API)."""
    return TextEngine()
