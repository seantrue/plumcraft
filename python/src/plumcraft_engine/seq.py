"""Sequences: Zip, Outer Join and Unzip, over any values (the `ANY` wire type), as
`crates/engine/src/seq.rs`.

- **Zip** pairs up its inputs item by item: `(a1, b1), (a2, b2), …`. The lists must be the same
  length; a single value goes with every item.
- **Outer Join** pairs every item of one with every item of the others (the first varying
  slowest): every combination, as one list.
- **Unzip** splits a list of tuples back into its parts, lists that share one axis, so they zip
  again downstream (the settings they feed vary together). A single tuple splits into single
  values.

A tuple is a list inside a list (`List`): Vary Range 2D's `xy` makes the same. Each tuple is
labelled with what made its parts, so a sheet or Write Files can name it.
"""

from __future__ import annotations

import json
from typing import Any

import numpy as np

from . import ANY, Cx, List, Node, NodeError

# The parts a tuple has at most (Zip's and Outer Join's inputs, Unzip's outputs).
PARTS = ("a", "b", "c")


def _category() -> list[str]:
    return ["Building blocks", "Sequences"]


def _config(message: str) -> NodeError:
    return NodeError(NodeError.CONFIG, message)


def _any_in(name: str, optional: bool, tip: str) -> dict:
    port: dict[str, Any] = {"name": name, "type": ANY, "socket": "socket", "tooltip": tip}
    if optional:
        port["optional"] = True
    return port


def _any_out(name: str) -> dict:
    return {"name": name, "type": ANY, "list": True, "socket": "socket"}


def _parts(inputs: dict[str, Any]) -> list[tuple[str, Any]]:
    """The inputs a, b (and c, when wired): each a list, or a single value."""
    return [(p, inputs[p]) for p in PARTS if p in inputs]


def _pick(port: str, d: Any, i: int) -> tuple[Any, str]:
    """Item `i` of a part (a single value is every item) and what it says about the tuple."""
    from .exec import item_label

    if isinstance(d, List):
        return (d.items[i] if 0 <= i < len(d.items) else d), item_label(port, d, i)
    return d, ""


def _tuple(parts: list[tuple[Any, str]]) -> tuple[List, str]:
    """A tuple from its parts, labelled by their labels (repeats dropped)."""
    label: list[str] = []
    items = []
    for d, lab in parts:
        items.append(d)
        if lab and lab not in label:
            label.append(lab)
    return List(items), " · ".join(label)


def _tuples_out(tuples: list[tuple[List, str]]) -> dict[str, Any]:
    return {"tuples": List([t for t, _ in tuples], [], [lab for _, lab in tuples])}


def _len(d: Any) -> int:
    return len(d.items) if isinstance(d, List) else 1


class Combine(Node):
    """Zip or Outer Join."""

    def __init__(self, outer: bool):
        self.outer = outer
        if outer:
            ident, display, description = (
                "seq.outer",
                "Outer Join",
                "Every combination of the items of a, b (and c), as one list of tuples, the first varying slowest. "
                "A single value goes with every item. Unzip splits the tuples again.",
            )
        else:
            ident, display, description = (
                "seq.zip",
                "Zip",
                "The items of a, b (and c) paired up in order, as one list of tuples: (a1, b1), (a2, b2), … "
                "The lists must be the same length; a single value goes with every item. Unzip splits the tuples again.",
            )
        self.definition = {
            "id": ident,
            "display": display,
            "category": _category(),
            "description": description,
            "inputs": [
                _any_in("a", False, "A sequence, or a single value"),
                _any_in("b", False, "A sequence, or a single value"),
                _any_in("c", True, "A third sequence (optional)"),
            ],
            "outputs": [_any_out("tuples")],
            "flags": {"input_list": True},
        }

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        from .exec import MAX_RUNS

        parts = _parts(inputs)
        if self.outer:
            total = 1
            for _, d in parts:
                total *= _len(d)
                if total > MAX_RUNS:
                    raise _config(f"more than {MAX_RUNS} combinations")
            out = []
            for t in range(total):
                # row-major: the first part varies slowest
                idx = [0] * len(parts)
                rest = t
                for slot in range(len(parts) - 1, -1, -1):
                    n = max(_len(parts[slot][1]), 1)
                    idx[slot] = rest % n
                    rest //= n
                out.append(_tuple([_pick(p, d, i) for (p, d), i in zip(parts, idx, strict=True)]))
            return _tuples_out(out)
        lengths = [(p, _len(d)) for p, d in parts if isinstance(d, List)]
        n = lengths[0][1] if lengths else 1
        bad = next(((p, m) for p, m in lengths if m != n), None)
        if bad is not None:
            raise _config(f"Zip: the sequences differ in length ({lengths[0][0]} has {n}, {bad[0]} has {bad[1]})")
        return _tuples_out([_tuple([_pick(p, d, i) for p, d in parts]) for i in range(n)])


class Unzip(Node):
    """Unzip: tuples back into their parts."""

    definition = {
        "id": "seq.unzip",
        "display": "Unzip",
        "category": _category(),
        "description": "A list of tuples (from Zip, Outer Join or Vary Range 2D's xy) split into its parts: a, b and c are lists that zip downstream, "
        "so the settings they feed vary together. A single tuple splits into single values.",
        "inputs": [_any_in("tuples", False, "A list of tuples, or one tuple")],
        "outputs": [_any_out(p) for p in PARTS],
        "flags": {"input_list": True},
    }

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        lst = inputs.get("tuples")
        if not isinstance(lst, List):
            raise _config("tuples: wire a list of tuples (Zip, Outer Join, Vary Range 2D's xy)")
        # one tuple (its items aren't tuples): single values
        if not lst.items or not all(isinstance(i, List) for i in lst.items):
            if not lst.items:
                raise _config("tuples: the list is empty")
            return dict(zip(PARTS, lst.items, strict=False))
        width = min(len(t.items) for t in lst.items)
        if width == 0:
            raise _config("tuples: a tuple is empty")
        # one axis, named by the node: the parts zip downstream
        axes = [(cx.node_id, len(lst.items))]
        out: dict[str, Any] = {}
        for k, p in enumerate(PARTS[:width]):
            items = [t.items[k] if k < len(t.items) else None for t in lst.items]
            out[p] = List(items, list(axes), list(lst.labels))
        return out


MAX_ROWS = 100_000


def table_rows(v: Any, header: bool = True) -> list:
    """A table's rows: `{"columns": [{name, values}]}` as `[[names...], [row...], ...]`; rows pass through."""
    if isinstance(v, list) and all(isinstance(r, list) for r in v):
        return list(v)
    cols = v.get("columns") if isinstance(v, dict) else None
    if not isinstance(cols, list):
        raise _config("table: not a table (no columns)")
    values = []
    for c in cols:
        vs = c.get("values") if isinstance(c, dict) else None
        if not isinstance(vs, list):
            raise _config("table: a column without values")
        values.append(vs)
    n = max((len(v) for v in values), default=0)
    if n > MAX_ROWS:
        raise _config(f"table: {n} rows; at most {MAX_ROWS}")
    rows: list = [[c.get("name") for c in cols]] if header else []
    for i in range(n):
        rows.append([v[i] if i < len(v) else None for v in values])
    return rows


def array_rows(a: np.ndarray, header: bool = True) -> list:
    """A 2-D array's rows (a table array: its channel names are the header; else c1, c2, ...)."""
    from .arrays import meta

    if a.ndim != 2:
        raise _config(f"table: a 2-D array, not shape {tuple(a.shape)}")
    n, k = a.shape
    if n > MAX_ROWS or k > 4096:
        raise _config(f"table: {n} × {k}; at most {MAX_ROWS} rows of 4096")
    names = list(meta(a)[1])
    rows: list = [[names[j] if j < len(names) else f"c{j + 1}" for j in range(k)]] if header else []
    vals = np.asarray(a, dtype=np.float32).astype(np.float64)
    rows.extend([None if not np.isfinite(x) else float(x) for x in r] for r in vals)
    return rows


class TableRows(Node):
    """Table to Rows: a table as rows, the column names first."""

    definition = {
        "id": "seq.table_rows",
        "display": "Table to Rows",
        "category": _category(),
        "description": "A table (named columns, as Letter counts or Transients make it, or a 2-D table array, as Histogram makes it) as rows of values, "
        "the column names first: what a spreadsheet's Write range takes. Rows already in rows pass through.",
        "inputs": [
            _any_in("table", False, 'A table: {"columns": [{name, values}]}'),
            {"name": "header", "type": "BOOL", "default": True, "tooltip": "Put the column names first"},
        ],
        "outputs": [{"name": "rows", "type": ANY, "socket": "socket"}],
    }

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        if "table" not in inputs:
            raise _config("table: wire a table, or a 2-D array (a table array: Histogram)")
        t, header = inputs["table"], inputs.get("header") is not False
        if isinstance(t, np.ndarray):
            return {"rows": array_rows(t, header)}
        return {"rows": table_rows(t, header)}


def rows_table(v: Any, header: bool = True) -> dict:
    """Rows as a table (as `crates/engine/src/seq.rs`): the first row the names, column types
    from the values; a table passes through."""
    if isinstance(v, dict) and isinstance(v.get("columns"), list):
        return v
    if not (isinstance(v, list) and all(isinstance(r, list) for r in v)):
        raise _config("rows: not a list of lists")
    if len(v) > MAX_ROWS + 1:
        raise _config(f"rows: {len(v)} rows; at most {MAX_ROWS}")
    width = max((len(r) for r in v), default=0)
    if width > 4096:
        raise _config(f"rows: {width} columns; at most 4096")
    if header and v:
        first, body = v[0], v[1:]

        def name(j: int) -> str:
            x = first[j] if j < len(first) else None
            if isinstance(x, str) and x:
                return x
            return f"c{j + 1}" if x is None else json.dumps(x)

        names = [name(j) for j in range(width)]
    else:
        names, body = [f"c{j + 1}" for j in range(width)], v
    columns = []
    for j, nm in enumerate(names):
        values = [r[j] if j < len(r) else None for r in body]
        present = [x for x in values if x is not None]
        if present and all(isinstance(x, int) and not isinstance(x, bool) for x in present):
            ty = "INT"
        elif present and all(isinstance(x, (int, float)) and not isinstance(x, bool) for x in present):
            ty = "FLOAT"
        elif present and all(isinstance(x, bool) for x in present):
            ty = "BOOL"
        else:
            ty = "STRING"
        columns.append({"name": nm, "type": ty, "values": values})
    return {"columns": columns}


class RowsTable(Node):
    """Rows to Table: rows back into a table of named columns."""

    definition = {
        "id": "seq.rows_table",
        "display": "Rows to Table",
        "category": _category(),
        "description": "Rows of values (a spreadsheet's Read range, or Table to Rows) as a table of named columns, the first row the names "
        "(without a header, c1, c2, …). A column is INT, FLOAT or BOOL when every value is one, else STRING. Table to Rows goes the other way.",
        "inputs": [
            _any_in("rows", False, "Rows: a list of lists"),
            {"name": "header", "type": "BOOL", "default": True, "tooltip": "The first row holds the column names"},
        ],
        "outputs": [{"name": "table", "type": ANY, "socket": "socket"}],
    }

    def run(self, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
        if "rows" not in inputs:
            raise _config("rows: wire rows (a list of lists)")
        return {"table": rows_table(inputs["rows"], inputs.get("header") is not False)}


def nodes() -> list[Node]:
    """Zip, Outer Join, Unzip, Table to Rows and Rows to Table."""
    return [Combine(False), Combine(True), Unzip(), TableRows(), RowsTable()]
