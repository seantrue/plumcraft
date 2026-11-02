"""The graph executor, as `crates/engine/src/exec.rs`: workflow JSON in the research software's shape, run in
topological order with every status record the interface requires (spec 7.6, 9.4).

Inputs resolve as a wire, then the node's widget value, then the port's default. A failing node
is `error` and everything downstream is `skipped`. UI-only fields are ignored. A node that raises
anything other than `NodeError` fails with kind `op` and the exception's type and message; the
engine carries on, since a Python exception leaves no state behind the way a Rust panic may.

The memo (spec 14): every node gets a plan key from its kind, version, the engine salt, its
resolved parameters (coerced to INT or FLOAT by port type) and its upstream nodes' plan keys. A
node whose key is in the `Memo` is not computed: it sends `preview` (thumb and watched displays
from the stored outputs) and `cached`, never `started`. The run's `memo` mode chooses: `use`
(the default), `refresh` (recompute the targets), `off` (neither read nor write). Nodes declaring
`memo.scope: none` are never stored, and pass a per-run identity downstream, so nothing below
them is reused either. Errors are never stored.

Persisting (spec 14.5), as the Rust executor: a `machine`-scope node whose own work (less the
setup it reports with `Cx.setup_took`) took the engine's threshold (`Engine.persist_ms`, 1 s by
default) leaves a record in the store (`persist.py`), unless writing and reading its outputs back
would cost about as much; a workflow node's `cache` (`always`, `never`) overrides. A node not in
memory looks for a record before it computes.

Lists (spec 7.6 rule 2, as the Rust executor): a `List` reaching an input that takes one item runs
the node once per item, unless the node takes `input_list` or the port is a `list` port. Each list
carries its axes: a list a node makes is one axis, `<node>.<port>` (or the axes the node named, so
its outputs zip), and a node mapped over lists gives its outputs the union of their axes. Lists
meeting at a node zip along an axis they share and form a product over axes they don't (two Vary
nodes into two settings: every combination, the first varying slowest; inputs taken in port-name
order, as the Rust engine's sorted maps take them). Single values broadcast. A Vary Choices node
(`flags.vary_choices`) gets the choices of the ENUM inputs it feeds as `_choices`, part of its memo
key.
"""

from __future__ import annotations

import json
import os
import time
import traceback
from collections import OrderedDict
from collections.abc import Callable
from typing import Any

import numpy as np

from . import ANY, PROGRESS_EVERY_S, PROTOCOL, Blob, CancelToken, Cx, Engine, List, Node, NodeError, arrays, bind, data, memo, now_ts, persist, slices, status
from .audio import AudioValue
from .vary import CHOICES_KEY

# The longest text an excerpt rendition carries (spec 10.3).
EXCERPT_CHARS = 4096
# Memo entries kept; the oldest goes first.
MEMO_ENTRIES = 1024
# The memo parameter under which a node's `volatile` value (what its inputs don't cover: a file's
# size and time, a folder's listing) joins its key.
VOLATILE_KEY = "_volatile"
# The most runs a node mapped over lists makes.
MAX_RUNS = 100_000
# The most items a list display's sheet describes.
MAX_SHEET = 256
# Bytes the store writes and reads back per ms, roughly (an SSD's ~200 MB/s): a result is persisted
# only when writing it and reading it back would cost well under computing it (spec 14.5).
STORE_BYTES_PER_MS = 200_000.0


class Memo:
    """Results by plan key, in memory for the engine's life (spec 14.4: `session` and `machine`
    scopes alike, until persistence lands). The host drops it when it rebuilds the engine."""

    def __init__(self) -> None:
        # outputs, and what computing them took
        self._entries: OrderedDict[str, tuple[dict, float]] = OrderedDict()
        self.evicted = 0

    def __len__(self) -> int:
        return len(self._entries)

    def get(self, key: str) -> tuple[dict, float] | None:
        return self._entries.get(key)

    def put(self, key: str, outputs: dict, ms: float = 0.0) -> None:
        self._entries.setdefault(key, (outputs, ms))
        while len(self._entries) > MEMO_ENTRIES:
            self._entries.popitem(last=False)
            self.evicted += 1

    def take_evicted(self) -> int:
        """Entries dropped since the last call."""
        n, self.evicted = self.evicted, 0
        return n


def _graph_error(message: str) -> NodeError:
    return NodeError(NodeError.GRAPH, message)


class _Run:
    def __init__(self, emit: Callable[[dict], None], publisher: str):
        self.emit = emit
        self.publisher = publisher
        self.seq = 0

    def record(self, rec: dict) -> None:
        self.seq += 1
        rec["seq"] = self.seq
        rec["publisher"] = self.publisher
        rec["ts"] = now_ts()
        self.emit(rec)

    def state(self, node: str, state: str) -> None:
        self.record(status(node, state))


def run(
    engine: Engine, req: dict, emit: Callable[[dict], None], cancel: CancelToken, publisher: str, store: Memo | None = None, plane: data.Plane | None = None
) -> dict:
    """Run a request to completion and return the authoritative reply. A bad graph is a reply with a
    `graph` error, never an exception."""
    started = time.monotonic()
    reply: dict[str, Any] = {"v": PROTOCOL, "run_id": req.get("run_id", ""), "generation": req.get("generation", 0), "ok": False, "nodes": {}, "order": []}
    targets = [t for t in req.get("targets") or [] if isinstance(t, str)]
    watch = [w for w in req.get("watch") or [] if isinstance(w, dict)]
    try:
        nodes, wires = _parse(engine, req)
        order = _topo_order(nodes, wires)
    except NodeError as e:
        reply["error"] = e.to_json()
        reply["watch"] = []
        reply["elapsed_ms"] = 0.0
        return reply
    # Bindings (spec 7.6 rule 10) replace the wires into the ports they name, so what fed those
    # ports is neither waited for nor computed.
    bound = bind.by_node(req.get("inputs"), lambda n: n in nodes)
    trusted = tuple(t for t in (req.get("trusted_roots") or []) if isinstance(t, str))
    wires = [w for w in wires if w["to"]["port"] not in bound.get(w["to"]["node"], {})]
    wanted = _needed(nodes, wires, targets, watch)
    order = [n for n in order if n in wanted]
    r = _Run(emit, publisher)
    values: dict[str, dict] = {}
    finished_ok: set[str] = set()
    watched: list[dict | None] = [None] * len(watch)
    keys: dict[str, str] = {}
    mode = req.get("memo") if req.get("memo") in ("off", "refresh") else "use"
    store = store if store is not None else Memo()
    stats: dict[str, Any] = dict.fromkeys(("computed", "from_memory", "from_store", "from_content", "kept", "store_missed"), 0)
    stats.update(computed_ms=0.0, saved_ms=0.0)
    plane = plane if plane is not None else data.Plane(None, False, engine.limits["max_inline"])

    for nid in order:
        wn = nodes[nid]
        node = engine.nodes[wn["kind"]]
        if cancel.is_cancelled():
            r.state(nid, "cancelled")
            reply["nodes"][nid] = {"state": "cancelled"}
            continue
        d = node.definition
        upstream = [w for w in wires if w["to"]["node"] == nid]
        bad = next((w for w in upstream if w["from"]["node"] not in finished_ok), None)
        if bad is not None:
            err = NodeError(NodeError.SKIPPED, f"upstream {bad['from']['node']} did not finish").to_json()
            rec = status(nid, "skipped")
            rec["error"] = err
            r.record(rec)
            reply["nodes"][nid] = {"state": "skipped", "error": err}
            continue
        t0 = time.monotonic()
        try:
            binds = bound.get(nid, {})
            given = {port: bind.to_value(engine.store_root, port, v, trusted) for port, v in binds.items()}
            inputs, params = _resolve_inputs(d, wn, upstream, values, engine.store_root, given)
            resolve_error = None
        except NodeError as e:
            inputs, params, resolve_error = None, None, e
        if inputs is not None and params is not None:
            try:
                v = node.volatile(inputs)
                if v is not None:
                    params[VOLATILE_KEY] = v
            except Exception as e:  # a bug in a node is that node's error, as data
                traceback.print_exc()
                inputs, params, resolve_error = None, None, NodeError(NodeError.OP, f"{type(e).__name__}: {e}")
        if (d.get("flags") or {}).get("vary_choices") is True and inputs is not None and params is not None:
            try:
                offered = _offered_choices(engine, nodes, wires, nid)
                params[CHOICES_KEY] = list(offered)
                inputs[CHOICES_KEY] = list(offered)
            except NodeError as e:
                inputs, params, resolve_error = None, None, e
        key = None
        if params is not None:
            identities = {w["to"]["port"]: memo.input_identity(keys.get(w["from"]["node"], ""), w["from"]["port"], _slice_text(w)) for w in upstream}
            identities.update({port: bind.identity(v) for port, v in bound.get(nid, {}).items()})
            try:
                key = memo.plan_key(wn["kind"], d.get("version", 1), engine.info.get("salt", ""), None, params, identities)
            except memo.Uncanonical:
                key = None
        scope = (d.get("memo") or {}).get("scope", "machine")
        stores = mode != "off" and scope != "none"
        reads = stores and (mode == "use" or nid not in targets)
        if key is not None:
            # Downstream of a `none` node, the input is new every run: pass a per-run identity
            # down the chain so nothing below it hits on a stale value.
            keys[nid] = f"memo:none:{req.get('run_id', '')}:{req.get('generation', 0)}:{nid}" if scope == "none" else key
        persists = scope == "machine" and wn["cache"] != "never" and engine.store_root is not None
        declared = [p.get("name") for p in d.get("outputs") or [] if isinstance(p, dict)]

        def lookup(k: str, persists: bool = persists, declared: list = declared) -> tuple[dict, float, bool] | None:
            # Memory, then a persisted record (spec 14.5), for `machine`-scope nodes.
            held = store.get(k)
            if held is not None:
                return held[0], held[1], True
            if not persists:
                return None
            loaded = persist.load(engine.store_root, k)
            if loaded is None or not all(name in loaded[0] for name in declared):
                return None
            store.put(k, loaded[0], loaded[1])
            return loaded[0], loaded[1], False

        found = lookup(key) if key is not None and reads else None
        # The plan key missed: the same node on the same bytes, however they were made (spec 14.3).
        content = None
        if found is None and stores and inputs is not None and params is not None:
            wired = [w["to"]["port"] for w in upstream] + list(bound.get(nid, {}))
            content = _content_key(wn["kind"], d, engine.info.get("salt", ""), params, inputs, wired)
            content = None if content == key else content
        by_content = found is None
        if found is None and content is not None and reads:
            found = lookup(content)
        if found is None and reads and persists:
            stats["store_missed"] += 1
        hit = None
        if found is not None:
            hit, computed_ms, held = found
            if by_content and key is not None:
                store.put(key, hit, computed_ms)
            source = "content" if by_content else "memory" if held else "store"
            stats[f"from_{source}"] += 1
            stats["saved_ms"] += computed_ms
        if hit is not None:
            ms = (time.monotonic() - t0) * 1000.0
            thumb = _preview(r, engine, d, nid, watch, hit, watched, plane)
            rec = status(nid, "cached")
            rec["ms"] = ms
            r.record(rec)
            report = {"state": "cached", "ms": ms, "memo": {"plan_key": key, "source": source, "computed_ms": computed_ms}}
            if content is not None:
                report["memo"]["content_key"] = content
            if thumb is not None:
                report["thumb"] = thumb
            if nid in targets:
                report["outputs"] = _target_outputs(engine, d, hit, plane)
            reply["nodes"][nid] = report
            finished_ok.add(nid)
            values[nid] = hit
            continue
        r.state(nid, "started")
        setup = [0.0]
        work_ms = 0.0
        try:
            if resolve_error is not None:
                raise resolve_error
            t1 = time.monotonic()
            outputs = _name_lists(nid, _call(node, d, nid, inputs, cancel, r.record, setup))
            work_ms = max(0.0, time.monotonic() - t1 - setup[0]) * 1000.0
            error = None
        except NodeError as e:
            outputs, error = None, e
        except Exception as e:  # a bug in a node is that node's error, as data
            traceback.print_exc()
            outputs, error = None, NodeError(NodeError.OP, f"{type(e).__name__}: {e}")
        ms = (time.monotonic() - t0) * 1000.0
        if error is None and cancel.is_cancelled():
            r.state(nid, "cancelled")
            reply["nodes"][nid] = {"state": "cancelled", "ms": ms}
        elif error is None:
            # The preview, the moment the node finishes and before anything downstream starts.
            thumb = _preview(r, engine, d, nid, watch, outputs, watched, plane)
            done = status(nid, "done")
            done["ms"] = ms
            r.record(done)
            report: dict[str, Any] = {"state": "done", "ms": ms}
            if thumb is not None:
                report["thumb"] = thumb
            if nid in targets:
                report["outputs"] = _target_outputs(engine, d, outputs, plane)
            stats["computed"] += 1
            stats["computed_ms"] += work_ms
            if key is not None:
                report["memo"] = {"plan_key": key, "computed_ms": work_ms}
                if content is not None:
                    report["memo"]["content_key"] = content
                if stores:
                    # Slow enough, and cheap to write and read back next to recomputing it.
                    threshold = engine.persist_ms
                    worth = threshold is not None and work_ms >= threshold and persist.size_of(outputs) / STORE_BYTES_PER_MS * 2.0 < work_ms
                    if persists and (wn["cache"] == "always" or (wn["cache"] == "auto" and worth)):
                        stats["kept"] += int(persist.save(engine.store_root, key, outputs, work_ms))
                        if content is not None:
                            persist.save(engine.store_root, content, outputs, work_ms)
                    if content is not None:
                        store.put(content, outputs, work_ms)
                    store.put(key, outputs, work_ms)
            reply["nodes"][nid] = report
            finished_ok.add(nid)
            values[nid] = outputs
        else:
            state = "cancelled" if error.kind == NodeError.CANCELLED else "error"
            rec = status(nid, state)
            rec["ms"] = ms
            rec["error"] = error.to_json()
            r.record(rec)
            reply["nodes"][nid] = {"state": state, "ms": ms, "error": error.to_json()}

    reply["order"] = order
    reply["ok"] = bool(reply["nodes"]) and all(n["state"] in ("done", "cached") for n in reply["nodes"].values())
    if cancel.is_cancelled():
        reply["ok"] = False
        reply["error"] = NodeError.cancelled().to_json()
    reply["watch"] = [
        d if d is not None else {"node": w.get("node", ""), "port": w.get("port", ""), "error": NodeError(NodeError.OP, "no value").to_json()}
        for w, d in zip(watch, watched, strict=True)
    ]
    reply["elapsed_ms"] = (time.monotonic() - started) * 1000.0
    stats["evicted"] = store.take_evicted()
    if engine.store_root is not None:
        stats.update(data.store_stats(engine.store_root))
    reply["stats"] = stats
    return reply


def _offered_choices(engine: Engine, nodes: dict[str, dict], wires: list[dict], nid: str) -> list[Any]:
    """The choices of the ENUM inputs node `nid` feeds: every one it is wired into must offer the same."""
    offered: list[Any] | None = None
    for w in wires:
        if w["from"]["node"] != nid:
            continue
        target = engine.nodes.get(nodes.get(w["to"]["node"], {}).get("kind", ""))
        port = next((p for p in (target.definition.get("inputs") or []) if isinstance(p, dict) and p.get("name") == w["to"]["port"]), None) if target else None
        choices = port.get("choices") if port else None
        if not isinstance(choices, list) or not choices:
            continue
        if offered is None:
            offered = choices
        elif not _same_json(offered, choices):
            to = w["to"]
            raise NodeError(NodeError.CONFIG, f"Vary Choices feeds inputs with different choices ({to['node']} and {to['node']}.{to['port']})")
    return list(offered or [])


def _same_json(a: Any, b: Any) -> bool:
    """Equal as JSON values are (an INT 1 is not a FLOAT 1.0, nor `true`)."""
    return memo.canon(a) == memo.canon(b) if _canonical(a) and _canonical(b) else a == b


def _canonical(v: Any) -> bool:
    try:
        memo.canon(v)
        return True
    except memo.Uncanonical:
        return False


def _mapping_plan(lists: list[tuple[str, List]]) -> tuple[list[tuple[str, int]], list[list[int]]]:
    """How a node maps over its list inputs: the union of their axes (in order of first appearance),
    and for each run which item of each list input it takes."""
    axes: list[tuple[str, int]] = []
    for port, lst in lists:
        for name, n in lst.axes_or_plain():
            have = next((a for a in axes if a[0] == name), None)
            if have is None:
                axes.append((name, n))
            elif have[1] != n:
                what = name or "the lists"
                raise NodeError(NodeError.CONFIG, f"list inputs differ in length: {what} is {have[1]} long and {quoted(port)} brings {n}")
    total = 1
    for _, n in axes:
        total *= n
        if total > MAX_RUNS:
            raise NodeError(NodeError.CONFIG, f"the lists make more than {MAX_RUNS} runs")
    where = {name: i for i, (name, _) in enumerate(axes)}
    plain = [lst.axes_or_plain() for _, lst in lists]
    picks: list[list[int]] = []
    for t in range(total):
        # the index along each axis, row-major (the first axis varies slowest)
        idx = [0] * len(axes)
        rest = t
        for slot in range(len(axes) - 1, -1, -1):
            n = max(axes[slot][1], 1)
            idx[slot] = rest % n
            rest //= n
        pick = []
        for la in plain:
            j = 0
            for name, n in la:
                j = j * n + idx[where[name]]
            pick.append(j)
        picks.append(pick)
    return axes, picks


def _run_one(node: Node, inputs: dict[str, Any], cx: Cx) -> dict[str, Any]:
    """One run of a node; anything it raises but a `NodeError` is an `op` error, as data."""
    try:
        outputs = node.run(inputs, cx)
    except NodeError:
        raise
    except Exception as e:  # a bug in a node is that node's error, as data
        traceback.print_exc()
        raise NodeError(NodeError.OP, f"{type(e).__name__}: {e}") from None
    if not isinstance(outputs, dict):
        raise NodeError(NodeError.OP, f"the node returned {type(outputs).__name__}, not a dict of outputs")
    return outputs


def _call(
    node: Node, d: dict, nid: str, inputs: dict[str, Any], cancel: CancelToken, record: Callable[[dict], None], setup: list[float] | None = None
) -> dict[str, Any]:
    """Run a node: once, or once per item when lists reach inputs that take one item (each run's
    progress scaled into the whole), its outputs then lists over the runs' axes. The time its calls
    spent getting ready (`Cx.setup_took`) is added to `setup[0]`."""
    setup = setup if setup is not None else [0.0]
    flags = d.get("flags") if isinstance(d.get("flags"), dict) else {}
    whole_ports = {p.get("name") for p in d.get("inputs") or [] if isinstance(p, dict) and p.get("list") is True}

    def whole(k: str) -> bool:
        return flags.get("input_list") is True or k in whole_ports

    lists = [(k, inputs[k]) for k in sorted(inputs) if not whole(k) and isinstance(inputs[k], List)]
    if not lists:
        return _run_one(node, inputs, Cx(nid, cancel, record, setup))
    axes, picks = _mapping_plan(lists)
    n = len(picks)
    at = {k: i for i, (k, _) in enumerate(lists)}
    per_item: list[dict[str, Any]] = []
    labels: list[str] = []
    last: float | None = None
    for i, pick in enumerate(picks):
        if cancel.is_cancelled():
            raise NodeError.cancelled()
        item: dict[str, Any] = {}
        for k, v in inputs.items():
            if k in at:
                lst, j = lists[at[k]][1], pick[at[k]]
                if j < len(lst.items):
                    item[k] = lst.items[j]
            else:
                item[k] = v
        label: list[str] = []
        for (k, lst), j in zip(lists, pick, strict=True):
            lab = item_label(k, lst, j)
            if lab and lab not in label:
                label.append(lab)
        labels.append(" · ".join(label))

        def scaled(rec: dict, i: int = i) -> None:
            if rec.get("state") == "progress":
                f = rec.get("fraction")
                rec["fraction"] = (i + (float(f) if isinstance(f, int | float) else 0.0)) / n
                stage = rec.get("stage")
                rec["stage"] = f"item {i + 1} of {n}: {stage}" if stage is not None else f"item {i + 1} of {n}"
            record(rec)

        now = time.monotonic()
        if last is None or now - last >= PROGRESS_EVERY_S:
            last = now
            rec = status(nid, "progress")
            rec["fraction"] = 0.0
            scaled(rec)
        try:
            out = _run_one(node, item, Cx(nid, cancel, scaled, setup))
        except NodeError as e:
            if e.kind == NodeError.CANCELLED:
                raise
            raise NodeError(e.kind, f"item {i + 1} of {n}: {e.message}") from None
        per_item.append(out)
    # An output of one item per run spans the runs' axes; one that is itself a list per run is a
    # new, flat list (named as it leaves the node).
    names = [p["name"] for p in d.get("outputs") or [] if isinstance(p, dict) and isinstance(p.get("name"), str)]
    for o in per_item:
        names.extend(sorted(k for k in o if k not in names))
    result: dict[str, Any] = {}
    for name in names:
        vals = [o[name] for o in per_item if name in o]
        if len(vals) != len(per_item):
            continue
        if not any(isinstance(v, List) for v in vals):
            result[name] = List(vals, list(axes), list(labels))
            continue
        flat = List([])
        for v, run_label in zip(vals, labels, strict=True):
            if isinstance(v, List):
                for j, x in enumerate(v.items):
                    flat.items.append(x)
                    flat.labels.append(" · ".join(s for s in (run_label, v.label(j)) if s))
            else:
                flat.items.append(v)
                flat.labels.append(run_label)
        result[name] = flat
    return result


def item_label(port: str, lst: List, j: int) -> str:
    """What item `j` of list input `port` says about a run: the item's own label (what made it
    upstream) when it has one, else `port value` for a scalar (a swept setting)."""
    if lst.label(j):
        return lst.label(j)
    v = lst.items[j] if 0 <= j < len(lst.items) else None
    if isinstance(v, bool | int | float):
        return f"{port} {short_number(v)}"
    if isinstance(v, str) and len(v) <= 40 and "\n" not in v:
        return f"{port} {v}"
    # a tuple of numbers (Vary Range 2D's xy): `port (x, y)`
    if isinstance(v, List) and v.items and all(isinstance(i, int | float) and not isinstance(i, bool) for i in v.items):
        return f"{port} ({', '.join(short_number(i) for i in v.items)})"
    return ""


def short_number(v: Any) -> str:
    """A number as a label shows it: at most 6 decimals, trailing zeros dropped."""
    if isinstance(v, float) and not isinstance(v, bool) and v == v and v not in (float("inf"), float("-inf")):
        t = f"{v:.6f}".rstrip("0").rstrip(".")
        return "0" if t == "-0" else t
    return json.dumps(v)


def _name_lists(nid: str, outputs: dict[str, Any]) -> dict[str, Any]:
    """Give each list a node made without axes (or with axes that don't fit it) its own axis,
    `<node>.<port>`, so it zips with itself downstream."""
    out = dict(outputs)
    for port, v in outputs.items():
        if isinstance(v, List):
            total = 1
            for _, n in v.axes:
                total *= n
            if not v.axes or total != len(v.items):
                out[port] = List(v.items, [(f"{nid}.{port}", len(v.items))], v.labels)
    return out


def _target_outputs(engine: Engine, d: dict, outputs: dict, plane: data.Plane) -> dict:
    """A target's outputs as the reply carries them, each crossing as `Plane.port_value` decides."""
    ports = {p["name"]: p.get("type", "") for p in d.get("outputs") or []}
    return {k: plane.port_value(_media(engine, ports.get(k, "")), v) for k, v in outputs.items()}


def _preview(r: _Run, engine: Engine, d: dict, nid: str, watch: list[dict], outputs: dict, watched: list[dict | None], plane: data.Plane) -> dict | None:
    """Send a node's `preview` record (thumb and watched displays) and return the thumb."""
    pv = status(nid, "preview")
    first = (d.get("outputs") or [None])[0]
    thumb = _thumb(engine, first["type"], outputs.get(first["name"]), plane) if first else None
    if thumb is not None:
        pv["thumb"] = thumb
    displays = []
    for i, w in enumerate(watch):
        if w.get("node") == nid:
            disp = _display(engine, d, nid, w, outputs, plane)
            displays.append(disp)
            watched[i] = disp
    if displays:
        pv["display"] = displays
    r.record(pv)
    return thumb


def _parse(engine: Engine, req: dict) -> tuple[dict[str, dict], list[dict]]:
    g = req.get("graph")
    if not isinstance(g, dict) or g.get("kind") != "inline":
        raise _graph_error("graphs by handle aren't supported by this engine")
    w = g.get("graph")
    if not isinstance(w, dict):
        raise _graph_error(f"malformed workflow: expected an object, got {type(w).__name__}")
    nodes = w.get("nodes") or {}
    wires = w.get("wires") or []
    if not isinstance(nodes, dict) or not isinstance(wires, list):
        raise _graph_error("malformed workflow: nodes must be an object and wires a list")
    clean_nodes: dict[str, dict] = {}
    for nid, n in nodes.items():
        if not isinstance(n, dict):
            raise _graph_error(f"malformed workflow: node {nid} is not an object")
        kind = n.get("kind", "")
        if kind not in engine.nodes:
            raise _graph_error(f"node {nid}: unknown kind {quoted(kind)}")
        values = n.get("values") if isinstance(n.get("values"), dict) else {}
        # Whether to persist its result (spec 14.5): `always`, `never`, else `auto`.
        cache = n.get("cache") if n.get("cache") in ("always", "never") else "auto"
        clean_nodes[nid] = {"kind": kind, "values": values, "cache": cache}
    clean_wires = []
    for wire in wires:
        ends = [wire.get(k) if isinstance(wire, dict) else None for k in ("from", "to")]
        if not all(isinstance(e, dict) for e in ends):
            raise _graph_error("malformed workflow: a wire needs from and to")
        ends = [{"node": str(e.get("node", "")), "port": str(e.get("port", ""))} for e in ends]
        for e in ends:
            if e["node"] not in clean_nodes:
                raise _graph_error(f"wire names unknown node {quoted(e['node'])}")
        spec = wire.get("slice")
        spec = spec.strip() if isinstance(spec, str) else ""
        clean_wires.append({"from": ends[0], "to": ends[1], **({"slice": spec} if spec else {})})
    return clean_nodes, clean_wires


def _topo_order(nodes: dict[str, dict], wires: list[dict]) -> list[str]:
    """Kahn's algorithm, ties broken by node id so the order is stable (and equal to the Rust engine's)."""
    indegree = {k: 0 for k in nodes}
    for w in wires:
        indegree[w["to"]["node"]] += 1
    ready = sorted(k for k, d in indegree.items() if d == 0)
    order = []
    while ready:
        nxt = ready.pop(0)
        order.append(nxt)
        for w in wires:
            if w["from"]["node"] == nxt:
                to = w["to"]["node"]
                indegree[to] -= 1
                if indegree[to] == 0:
                    ready.append(to)
                    ready.sort()
    if len(order) != len(nodes):
        raise _graph_error("the graph has a cycle")
    return order


def _needed(nodes: dict[str, dict], wires: list[dict], targets: list[str], watch: list[dict]) -> set[str]:
    """The targets (or, when there are none, the graph's sinks), the watched nodes, and all their ancestors."""
    sinks = targets or [n for n in nodes if not any(w["from"]["node"] == n for w in wires)]
    want = [n for n in sinks + [str(w.get("node", "")) for w in watch] if n in nodes]
    out: set[str] = set()
    while want:
        n = want.pop(0)
        if n not in out:
            out.add(n)
            want.extend(w["from"]["node"] for w in wires if w["to"]["node"] == n)
    return out


def _content_identity(v: Any, hash_arrays: bool) -> Any:
    """A value's identity by its bytes (spec 14.3), as a bound value's is (`bind.identity`). Blobs
    carry their handle and JSON is its own; an array is hashed only for a node worth it
    (`memo.content`), and otherwise there is no content key. Raises `_NoIdentity` for those, and
    for a value that is neither."""
    if isinstance(v, List):
        return [_content_identity(i, hash_arrays) for i in v.items]
    if isinstance(v, Blob):
        return v.handle
    if isinstance(v, AudioValue | np.ndarray):
        if not hash_arrays:
            raise _NoIdentity
        return data.nd1(v.samples if isinstance(v, AudioValue) else v)
    if not data.is_json(v):
        raise _NoIdentity
    return {"json": v}


class _NoIdentity(Exception):
    pass


def _content_key(kind: str, d: dict, salt: str, params: dict, inputs: dict, wired: list[str]) -> str | None:
    """A node's content key (spec 14.3): its plan key with each wired input named by its bytes
    rather than by what made them. None for a node with nothing wired, or an input not hashed."""
    if not wired:
        return None
    hash_arrays = (d.get("memo") or {}).get("content") is True
    try:
        identities = {port: _content_identity(inputs[port], hash_arrays) for port in wired}
        return memo.plan_key(kind, d.get("version", 1), salt, None, params, identities)
    except (_NoIdentity, KeyError, memo.Uncanonical):
        return None


def _slice_text(wire: dict) -> str | None:
    """A wire's slice, canonical as the memo key names it; a bad slice as written."""
    spec = wire.get("slice")
    if not spec:
        return None
    try:
        return slices.canonical(slices.parse(spec))
    except ValueError:
        return spec


def _deliver(wire: dict, value: Any) -> Any:
    """The value a wire delivers (spec 7.6 rule 3): a range keeps a list a list (a sweep of its
    own, shorter than the list it came from), an index takes one item."""
    spec = wire.get("slice")
    if not spec:
        return value
    name = quoted(wire["to"]["port"])
    try:
        s = slices.parse(spec)
        if not isinstance(value, List):
            raise ValueError(f"slice {quoted(spec)} needs a list, but the wire carries a single value")
        picks = slices.picks(s, len(value.items))
    except ValueError as e:
        raise NodeError(NodeError.CONFIG, f"input {name}: {e}") from None
    if isinstance(s, int):
        return value.items[picks[0]]
    names = "+".join(n for n, _ in value.axes_or_plain())
    labels = [value.labels[i] for i in picks if i < len(value.labels)]
    return List([value.items[i] for i in picks], [(f"{names}[{slices.canonical(s)}]", len(picks))], labels)


def _resolve_inputs(
    d: dict, wn: dict, upstream: list[dict], values: dict[str, dict], store_root: str | None = None, bound: dict[str, Any] | None = None
) -> tuple[dict[str, Any], dict[str, Any]]:
    """The node's inputs, and its parameters (the resolved non-wired values) for the memo key. A
    parameter on an INT or FLOAT port is coerced to that type first (spec 14.1 rule 1), and the node
    receives the coerced value, so the key and the computation agree."""
    inputs: dict[str, Any] = {}
    params: dict[str, Any] = {}
    for port in d.get("inputs") or []:
        name = port["name"]
        numeric = {"INT": memo.INT, "FLOAT": memo.FLOAT}.get(port.get("type", ""))
        wire = next((w for w in upstream if w["to"]["port"] == name), None)
        if bound and name in bound:
            inputs[name] = bound[name]
        elif wire is not None and wire["from"]["port"] in values.get(wire["from"]["node"], {}):
            inputs[name] = _deliver(wire, values[wire["from"]["node"]][wire["from"]["port"]])
        elif wn["values"].get(name) is not None:
            inputs[name] = params[name] = memo.coerce(wn["values"][name], numeric)
        elif port.get("default") is not None:
            inputs[name] = params[name] = memo.coerce(port["default"], numeric)
        elif not port.get("optional", False):
            raise NodeError(NodeError.CONFIG, f"input {quoted(name)} has no value")
        is_source = isinstance(port.get("path"), dict) and port["path"].get("mode", "") not in ("save", "dir", "files")
        if name in params and is_source and isinstance(params[name], dict) and "source" in params[name]:
            # An uploaded source (spec 8.1, 13.2): the node gets a local file with its bytes, and the
            # key is its handle, as a local file with the same bytes would be.
            handle, path = _materialize_source(store_root, params[name]["source"])
            params[name], inputs[name] = handle, path
        elif name in params and is_source and isinstance(params[name], str):
            # A file source is keyed by its content, never its path (spec 14.3).
            params[name] = _source_identity(params[name]) or params[name]
    return inputs, params


def _materialize_source(root: str | None, src: Any) -> tuple[str, str]:
    """Write an uploaded source's bytes to `{store}/.sources/<hex>/<name>`, once per handle."""
    handle = data.parse_handle(src.get("handle") if isinstance(src, dict) else None)
    if handle is None:
        raise NodeError(NodeError.CONFIG, "source: no valid handle")
    if not root:
        raise NodeError(NodeError.CONFIG, "source: this engine has no store")
    blob = data.read_blob(root, handle)
    if blob is None:
        raise NodeError(NodeError.CONFIG, f"source: {handle} was not uploaded")
    # Only the last path component of the client's name, and never `..`: the name is a label.
    name = os.path.basename(str(src.get("name") or "").replace("\\", "/")) or "source"
    if name in (".", ".."):
        name = "source"
    d = os.path.join(root, ".sources", handle.rsplit(":", 1)[1])
    path = os.path.join(d, name)
    if not os.path.isfile(path):
        try:
            os.makedirs(d, exist_ok=True)
            tmp = os.path.join(d, f".{name}.{os.getpid()}.tmp")
            with open(tmp, "wb") as f:
                f.write(blob.tobytes())
            os.replace(tmp, path)
        except OSError as e:
            raise NodeError(NodeError.OP, f"source: {e.strerror}") from None
    return handle, path


# The largest source file hashed for a memo key.
MAX_SOURCE = 1 << 30


def _source_identity(path: str) -> str | None:
    """A file's content handle (`md5:raw:`), or None when it can't be read."""
    try:
        if not os.path.isfile(path) or os.path.getsize(path) > MAX_SOURCE:
            return None
        with open(path, "rb") as f:
            return data.raw(f.read())
    except OSError:
        return None


def _media(engine: Engine, type_id: str) -> str:
    w = engine.wire_type(type_id)
    return w.get("media", "") if w else ""


def _thumb(engine: Engine, type_id: str, v: Any, plane: data.Plane | None = None) -> dict | None:
    """A thumbnail for a value of a wire type: an excerpt for text, a picture of an array, a list's
    first item's thumb."""
    if isinstance(v, List):
        return _thumb(engine, type_id, v.items[0], plane) if v.items else None
    if isinstance(v, np.ndarray) and plane is not None:
        return _array_thumb(v, plane)
    if isinstance(v, AudioValue) and plane is not None:
        return plane.peaks(v)
    if isinstance(v, Blob) and plane is not None:
        return _blob_thumb(engine, v, plane)
    if _media(engine, type_id) == "text" and isinstance(v, str):
        return excerpt(v)
    return None


# A node thumbnail's long side (spec F47: `node_px`).
THUMB_PX = 160


def _blob_thumb(engine: Engine, v: Blob, plane: data.Plane) -> dict | None:
    """A blob's own thumbnail, else the engine's picture of a picture file. A file it can't read
    just has no thumbnail: the value is still good."""
    t = v.thumb
    if t is None and str(v.meta.get("media", "")).startswith("image/"):
        try:
            t = engine.picture_thumb(v)
        except Exception:
            return None
    return plane.rgba8(t) if t is not None else None


def _array_thumb(v: np.ndarray, plane: data.Plane) -> dict | None:
    t = arrays.thumb(v, THUMB_PX)
    return plane.rgba8(t) if t is not None else None


def excerpt(s: str) -> dict:
    return {"kind": "excerpt", "text": s[:EXCERPT_CHARS], "offset": 0, "total_chars": len(s), "lines": line_count(s)}


def line_count(s: str) -> int:
    """Lines as Rust's `str::lines` counts them: split on `\n`, a final newline ends the last line."""
    if not s:
        return 0
    return s.count("\n") + (0 if s.endswith("\n") else 1)


def _display(engine: Engine, d: dict, nid: str, w: dict, outputs: dict, plane: data.Plane) -> dict:
    """A watched value: text as an excerpt (plus `full` when too long to send inline), a table inline
    or by `full`, an array always by `full`, a scalar as `value`. A list shows the item `index`
    asks for (negative from the end), with `items` and a sheet line per item (its label and
    summary; thumbs when `sheet` is asked for)."""
    port = str(w.get("port", ""))
    out = next((p for p in d.get("outputs") or [] if p["name"] == port), None)
    ty = out["type"] if out else ""
    disp: dict[str, Any] = {"node": nid, "port": port, "type": ty, "items": 1}
    media = _media(engine, ty)
    # a port of any value: the media of the value itself (a list: its first item's)
    if ty == ANY and port in outputs:
        v = outputs[port]
        media = _value_media(v.items[0] if isinstance(v, List) and v.items else v)
    if media:
        disp["media"] = media
    if isinstance(w.get("view"), dict):
        disp["view"] = w["view"]
    px = w.get("px") if isinstance(w.get("px"), int) and not isinstance(w.get("px"), bool) else 1024
    if port not in outputs:
        disp["error"] = NodeError(NodeError.OP, f"no output {quoted(port)}").to_json()
    elif isinstance(outputs[port], List):
        lst = outputs[port]
        n = len(lst.items)
        disp["items"] = n
        index = w.get("index")
        index = index if isinstance(index, int) and not isinstance(index, bool) else 0
        at = n + index if index < 0 else index
        at = min(max(at, 0), max(n - 1, 0))
        disp["index"] = at
        sheet = []
        for j, item in enumerate(lst.items[:MAX_SHEET]):
            cell: dict[str, Any] = {}
            _summarize(item, media, cell)
            c = {"label": lst.label(j), "summary": cell["summary"]}
            t = _thumb(engine, ty, item, plane) if w.get("sheet") is True else None
            if t is not None:
                c["thumb"] = t
            sheet.append(c)
        if sheet:
            disp["sheet"] = sheet
        if n:
            _fill(lst.items[at], media, plane, disp, px)
            label = lst.label(at)
            label = f" ({label})" if label else ""
            disp["summary"] = f"item {at + 1} of {n}{label}: {disp['summary']}"
        else:
            disp["summary"] = "an empty list"
    else:
        _fill(outputs[port], media, plane, disp, px)
    return disp


def _value_media(v: Any) -> str:
    """The media kind a value shows as when its wire type doesn't say (`ANY`)."""
    if isinstance(v, str):
        return "text"
    if isinstance(v, np.ndarray):
        return "ndarray"
    if isinstance(v, AudioValue):
        return "audio"
    if isinstance(v, Blob):
        form = v.meta.get("form") if isinstance(v.meta.get("form"), dict) else {}
        m = str(v.meta.get("media", ""))
        if form.get("kind") == "doc":
            return "doc"
        if m == "image/svg+xml":
            return "vector"
        return "image" if m.startswith("image/") else "bytes"
    return "scalar"


def _plain(v: Any) -> bool:
    """A value that is JSON as it stands (not a list, an array or a blob)."""
    return not isinstance(v, List | Blob | np.ndarray | AudioValue)


def _tuple_json(lst: List) -> list | None:
    """A tuple (a list inside a list) of plain values as a JSON array."""
    return list(lst.items) if all(_plain(i) for i in lst.items) else None


def _summarize(v: Any, media: str, disp: dict) -> None:
    """A one-line summary of a value."""
    if isinstance(v, str):
        disp["summary"] = f"text, {len(v)} characters, {line_count(v)} lines"
    elif isinstance(v, List):
        t = _tuple_json(v)
        disp["summary"] = f"({', '.join(x if isinstance(x, str) else short_number(x) for x in t)})" if t is not None else f"a tuple of {len(v.items)}"
    elif isinstance(v, Blob):
        disp["summary"] = f"{v.meta.get('media', '')}, {v.meta.get('size', 0)} bytes"
    elif isinstance(v, AudioValue):
        disp["summary"] = v.summary()
    elif isinstance(v, np.ndarray):
        space, _, table = arrays.meta(v)
        what = "table" if table else "labels" if arrays.layout(v) is not None and arrays.is_integer(v) else "array"
        tag = f" {space}" if space is not None else ""
        disp["summary"] = f"{what}{tag} {'×'.join(str(int(n)) for n in v.shape)} {v.dtype.str}"
    elif not data.is_json(v):
        disp["summary"] = f"a {type(v).__name__} the engine keeps to itself"
    elif media == "table":
        rows, columns = data.table_shape(v)
        disp["summary"] = f"table, {rows} rows: {', '.join(c['name'] for c in columns)}"
    else:
        disp["summary"] = _compact(v)[:200]


def _fill(v: Any, media: str, plane: data.Plane, disp: dict, px: int = 1024) -> None:
    """Fill a display with one (non-list) value: its summary, renditions, `full` and `value`. An
    array gets the pictures of the view asked for (at most `px` a side) and per-channel stats."""
    _summarize(v, media, disp)
    if isinstance(v, str):
        disp["renditions"] = [excerpt(v)]
        if len(v.encode("utf-8")) > plane.max_inline:
            _full(disp, lambda: plane.text_ref(v))
    elif isinstance(v, List):
        # a tuple (a list inside a list): its values when they are plain, else its first item
        t = _tuple_json(v)
        if t is not None:
            disp["value"] = t
        else:
            summary = disp["summary"]
            if v.items:
                _fill(v.items[0], media, plane, disp, px)
            disp["summary"] = f"{summary}, the first: {disp['summary']}"
    elif isinstance(v, Blob):
        if v.thumb is not None:
            r = plane.rgba8(v.thumb)
            disp["renditions"] = [r] if r is not None else []
        _full(disp, lambda: plane.blob_ref(v))
    elif isinstance(v, AudioValue):
        disp["media"] = "audio"
        pk = plane.peaks(v)
        if pk is not None:
            disp["renditions"] = [pk]
        _full(disp, lambda: plane.audio_ref(v))
    elif isinstance(v, np.ndarray):
        view = disp.get("view") if isinstance(disp.get("view"), dict) else {}
        params = view.get("params") if isinstance(view.get("params"), dict) else {}
        turn = params.get("turn")
        turn = float(turn) if isinstance(turn, int | float) and not isinstance(turn, bool) else 1.0
        mode = view.get("id") if isinstance(view.get("id"), str) else "auto"
        px = min(px, 4096) if px > 0 else 1024
        rends = [plane.rgba8(arrays.rgba8(rgb, px), "view", caption) for caption, rgb in arrays.views(v, mode, turn)]
        if rends:
            disp["renditions"] = [r for r in rends if r is not None]
        st = arrays.stats(v)
        if st is not None:
            disp["stats"] = st
        _full(disp, lambda: plane.array_ref(v))
    elif not data.is_json(v):
        pass  # a live object: its summary says what it is
    elif media == "table":
        if data.json_len(v) > plane.max_inline:
            _full(disp, lambda: plane.table_ref(v))
        else:
            disp["value"] = v
    elif media == "tree" and data.json_len(v) > plane.max_inline:
        _full(disp, lambda: plane.json_ref(v))
    else:
        disp["value"] = v


def _full(disp: dict, make: Callable[[], dict]) -> None:
    try:
        disp["full"] = make()
    except NodeError as e:
        disp["error"] = e.to_json()


def _compact(v: Any) -> str:
    return json.dumps(v, separators=(",", ":"), ensure_ascii=False)


def quoted(s: str) -> str:
    """A name in a message, quoted as the Rust engine quotes it, so both say the same thing."""
    return json.dumps(s, ensure_ascii=False)
