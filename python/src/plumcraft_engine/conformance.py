"""The Python half of conformance (`docs/research/17-engine-generator.md`, section 5.3): checks any
engine passes, reading only its catalogue and, when given, its demo workflow and slow node, never
node names. Each case runs in process and again over HTTP (the engine served on loopback in this
process, reached as an attached engine is).

    report = check(MyEngine, demo=workflow_dict, slow="myengine.slow")
    for r in report: print(r)                # ok or FAIL, the case, why
    assert_conformant(MyEngine, demo=..., slow=...)   # in a test: fails naming every failed case

`plumcraft-engine check` runs them from the command line (`[tool.plumcraft] demo` and
`[tool.plumcraft.conformance] slow`). The Rust half (`plumcraft conform`) adds what only a real
binding shows: `kill -9` recovery, the same-host proof, tokens and resume.
"""

from __future__ import annotations

import math
import secrets
import threading
import time
import typing
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from . import PROTOCOL
from .scratch import mkdtemp

# Building blocks every engine may offer under their own prefixes (`with_building_blocks`).
BUILDING_BLOCKS = ("file.", "seq.", "vary.", "color.")
SCALARS = {"FLOAT", "INT", "BOOL", "STRING", "ENUM", "ANY"}
TERMINAL = {"done", "cached", "error", "skipped", "cancelled"}
# How long a run may take in the checks, and the cancel and off-path limits (spec 19).
RUN_WAIT_S, CANCEL_S, OFF_PATH_S = 30.0, 0.5, 0.2
# Hostile widget values: each must fail the node as config or op, or be clamped, never crash.
HOSTILE = (None, "", "x" * 100_000, float("nan"), float("inf"), -1e300, 1e300, [], {}, True, "\x00\udcff")


@dataclass
class Result:
    case: str
    ok: bool
    why: str = ""

    def __str__(self) -> str:
        return f"{'ok  ' if self.ok else 'FAIL'}  {self.case}{'' if self.ok else ': ' + self.why}"


class Failed(Exception):
    """A case's check didn't hold."""


def core_declarations() -> dict[str, dict]:
    """Every core type's exact declaration, from the SDK's annotations."""
    from . import declare

    out = {}
    for name in ("Text", "Table", "Image", "Audio", "Array", "Svg", "Json"):
        for meta in typing.get_args(getattr(declare, name))[1:]:
            if isinstance(meta, declare.WireType):
                out[meta.id] = meta.to_json()
    return out


# ---------- a run, observed ----------


class Run:
    """One run through a connection: its events in order and its reply."""

    def __init__(self, conn: Any, graph: dict, run_id: str, inputs: dict | None = None, cancel_after: float | None = None):
        req = {"v": PROTOCOL, "op": "run", "run_id": run_id, "generation": 1, "graph": {"kind": "inline", "graph": graph}}
        if inputs:
            req["inputs"] = inputs
        self.events: list[dict] = []
        self.reply: dict | None = None
        self.cancelled_at: float | None = None
        since = conn.call("events.poll", {"since": 0, "wait_ms": 0}).get("last_seq", 0)
        t0 = time.monotonic()
        conn.call("run", req)
        while self.reply is None:
            if cancel_after is not None and self.cancelled_at is None and time.monotonic() - t0 >= cancel_after:
                conn.call("run.cancel", {"run_id": run_id})
                self.cancelled_at = time.monotonic()
            p = conn.call("events.poll", {"since": since, "wait_ms": 10})
            since = p.get("last_seq", since)
            for e in p.get("events", []):
                if e.get("run_id") not in (None, run_id) and (e.get("body") or {}).get("run_id") != run_id:
                    continue
                self.events.append(e)
                if e.get("kind") == "run.reply" and (e.get("body") or {}).get("run_id") == run_id:
                    self.reply = e["body"]
            if time.monotonic() - t0 > RUN_WAIT_S:
                raise Failed(f"no reply to {run_id} after {RUN_WAIT_S:.0f} s")
        self.replied_at = time.monotonic()

    def statuses(self) -> list[dict]:
        return [e["body"] for e in self.events if e.get("kind") == "node.status"]

    def states(self) -> dict[str, str]:
        return {n: (v or {}).get("state", "?") for n, v in ((self.reply or {}).get("nodes") or {}).items()}


# ---------- the engine's shape ----------


class Shape:
    """What the checks read from the catalogue."""

    def __init__(self, hello: dict, schema: dict):
        self.id = hello["engine"]["id"]
        self.schema = schema
        self.defs = {n["id"]: n for n in schema.get("nodes", []) if isinstance(n, dict)}
        self.own = {k: v for k, v in self.defs.items() if not k.startswith(BUILDING_BLOCKS)}
        self.types = {w["id"]: w for w in schema.get("wire_types", []) if isinstance(w, dict)}

    @staticmethod
    def required_sockets(d: dict) -> list[dict]:
        return [p for p in d.get("inputs", []) if p.get("socket") == "socket" and not p.get("optional") and "default" not in p]

    def source_of(self, ty: str) -> tuple[str, str] | None:
        """An own node that makes `ty` from its defaults alone, and its output port."""
        for nid, d in self.own.items():
            # a path input needs a file, which a default doesn't give
            if self.required_sockets(d) or (d.get("memo") or {}).get("scope") == "none" or any(p.get("path") for p in d.get("inputs", [])):
                continue
            for p in d.get("outputs", []):
                if p.get("type") == ty:
                    return nid, p["name"]
        return None

    def graph_for(self, kind: str, values: dict | None = None) -> dict | None:
        """A graph that runs `kind`: its required sockets wired from sources, or None."""
        nodes = {"n": {"kind": kind, "values": values or {}}}
        wires = []
        for i, p in enumerate(self.required_sockets(self.defs[kind])):
            src = self.source_of(p.get("type", ""))
            if src is None:
                return None
            nodes[f"s{i}"] = {"kind": src[0], "values": {}}
            wires.append({"from": {"node": f"s{i}", "port": src[1]}, "to": {"node": "n", "port": p["name"]}})
        return {"nodes": nodes, "wires": wires}


# ---------- the cases ----------


def _session(conn: Any, shape: Shape) -> None:
    try:
        conn.call("hello", {"v": PROTOCOL, "op": "hello", "protocol": {"min": 3, "max": 3}, "client": {"name": "conformance", "version": "0"}, "caps": []})
    except Exception:  # noqa: BLE001 (any refusal will do, as the transport carries it)
        pass
    else:
        raise Failed("hello for protocol 3 only was accepted")
    # `schema_digest` is an optional cache hint: held to it only when it is sent
    digest = shape.schema.get("engine", {}).get("schema_digest")
    if digest and conn.call("schema", {"digest": digest}) != {"unchanged": True}:
        raise Failed("schema with its own digest wasn't {unchanged: true}")
    bad = [nid for nid in shape.own if not nid.startswith(shape.id + ".")]
    if bad:
        raise Failed(f"node ids without the prefix {shape.id + '.'!r}: {', '.join(sorted(bad))}")
    used = {p.get("type") for d in shape.defs.values() for p in d.get("inputs", []) + d.get("outputs", [])}
    missing = sorted(t for t in used if t and t not in shape.types)
    if missing:
        raise Failed(f"wire types used but not declared: {', '.join(missing)}")


def _core_types(conn: Any, shape: Shape) -> None:
    core = core_declarations()
    wrong = [f"{t} is {shape.types[t]}, not {want}" for t, want in core.items() if t in shape.types and shape.types[t] != want]
    if wrong:
        raise Failed("; ".join(wrong))


def _defaults_run(conn: Any, shape: Shape) -> None:
    bad = []
    for i, kind in enumerate(sorted(shape.own)):
        g = shape.graph_for(kind)
        if g is None:
            continue
        r = Run(conn, g, f"conf-defaults-{i}")
        s = r.states().get("n")
        err = (((r.reply or {}).get("nodes") or {}).get("n") or {}).get("error") or {}
        if s not in TERMINAL or err.get("kind") == "engine":
            bad.append(f"{kind}: {s} {err.get('message', '')}".rstrip())
    if bad:
        raise Failed("; ".join(bad))


def _demo(conn: Any, shape: Shape, demo: dict) -> None:
    r = Run(conn, demo, "conf-demo")
    seqs = [e.get("seq", 0) for e in r.events]
    if seqs != sorted(seqs) or len(set(seqs)) != len(seqs):
        raise Failed("event seq doesn't rise")
    states = r.states()
    err = (r.reply or {}).get("error")
    if isinstance(err, dict) and not states:
        raise Failed(f"the run was refused: {err.get('kind')}: {err.get('message')}")
    if set(states) != set(demo.get("nodes", {})):
        raise Failed(f"the reply has nodes {sorted(states)}, the demo {sorted(demo.get('nodes', {}))}")
    not_done = {n: s for n, s in states.items() if s not in ("done", "cached")}
    if not_done:
        raise Failed(f"not done: {not_done}")
    order = r.statuses()
    first = lambda node, wanted: next((i for i, b in enumerate(order) if b.get("node_id") == node and b.get("state") in wanted), None)  # noqa: E731
    for w in demo.get("wires", []):
        a, b = w["from"]["node"], w["to"]["node"]
        pa, sb = first(a, {"preview", "cached"}), first(b, {"started"})
        if sb is not None and (pa is None or pa > sb):
            raise Failed(f"{b} started before {a}'s preview")
    if any(b.get("state") == "done" and not isinstance(b.get("ms"), int | float) for b in order):
        raise Failed("a done record without ms")
    again = Run(conn, demo, "conf-demo-again").states()
    memo_none = {n for n, v in demo["nodes"].items() if (shape.defs.get(v.get("kind"), {}).get("memo") or {}).get("scope") == "none"}
    fresh = {n: s for n, s in again.items() if s != "cached" and n not in memo_none}
    if fresh:
        raise Failed(f"a second run computed again: {fresh}")


def _cancel(conn: Any, shape: Shape, slow: str) -> None:
    seconds = {"seconds": 5.0} if any(p["name"] == "seconds" for p in shape.defs[slow].get("inputs", [])) else {}
    g = shape.graph_for(slow, seconds)
    if g is None:
        raise Failed(f"no node makes the inputs {slow} needs from its defaults")
    r = Run(conn, g, "conf-cancel", cancel_after=0.3)
    took = r.replied_at - (r.cancelled_at or r.replied_at)
    if r.states().get("n") != "cancelled":
        raise Failed(f"{slow} ended {r.states().get('n')}, not cancelled, {took:.2f} s after the cancel")
    if took > CANCEL_S + 0.1:
        raise Failed(f"{slow} took {took:.2f} s to stop after the cancel (at most {CANCEL_S} s)")


def _off_path(conn: Any, shape: Shape, slow: str) -> None:
    seconds = {"seconds": 3.0} if any(p["name"] == "seconds" for p in shape.defs[slow].get("inputs", [])) else {}
    g = shape.graph_for(slow, seconds)
    if g is None:
        raise Failed(f"no node makes the inputs {slow} needs from its defaults")
    slowest: list[tuple[str, float]] = []
    done = threading.Event()

    def watch() -> None:
        time.sleep(0.2)
        for op, params in (("ping", {}), ("events.poll", {"since": 0, "wait_ms": 0}), ("run.get", {"run_id": "conf-off"})):
            t = time.monotonic()
            conn.call(op, params)
            slowest.append((op, time.monotonic() - t))
        done.set()

    th = threading.Thread(target=watch, daemon=True)
    th.start()
    Run(conn, g, "conf-off", cancel_after=0.6)
    done.wait(5)
    late = [f"{op} {s * 1000:.0f} ms" for op, s in slowest if s > OFF_PATH_S]
    if late or len(slowest) < 3:
        raise Failed(f"answers off the run path were late: {', '.join(late) or 'none came'}")


def _hostile(conn: Any, shape: Shape) -> None:
    bad = []
    n = 0
    for kind in sorted(shape.own):
        widgets = [p for p in shape.defs[kind].get("inputs", []) if p.get("type") in SCALARS and p.get("socket") != "socket"]
        for p in widgets:
            for v in HOSTILE:
                g = shape.graph_for(kind, {p["name"]: v})
                if g is None:
                    break
                n += 1
                # a clamped `seconds` could run for minutes: cancelled is a fine end here
                r = Run(conn, g, f"conf-hostile-{n}", cancel_after=0.1)
                node = ((r.reply or {}).get("nodes") or {}).get("n") or {}
                kind_err = (node.get("error") or {}).get("kind")
                if node.get("state") not in TERMINAL or kind_err == "engine":
                    bad.append(f"{kind}.{p['name']}={_show(v)}: {node.get('state')} {kind_err or ''}".rstrip())
    conn.call("ping", {})
    if bad:
        raise Failed("; ".join(bad[:8]) + (f" (and {len(bad) - 8} more)" if len(bad) > 8 else ""))


def _show(v: Any) -> str:
    s = repr(v) if not (isinstance(v, float) and math.isnan(v)) else "nan"
    return s if len(s) <= 24 else s[:23] + "…"


# ---------- running them ----------


TOKEN = secrets.token_hex(32)


def _http(factory: Callable, token: str) -> str:
    """Serve `factory` over HTTP on loopback in this process, with `token`; its URL."""
    from .http import serve_http

    port_file = Path(mkdtemp("conform-")) / "port"
    threading.Thread(target=serve_http, args=(factory, "127.0.0.1:0", token, str(port_file)), daemon=True).start()
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if port_file.exists() and port_file.read_text().strip():
            return f"http://{port_file.read_text().strip()}"
        time.sleep(0.02)
    raise Failed("the HTTP front didn't start")


def _cases(conn: Any, shape: Shape, demo: dict | None, slow: str | None) -> list[tuple[str, Callable[[], None] | None, str]]:
    """Each case: its name, what runs it (None when what it needs wasn't given), and why not."""
    has_slow = slow in shape.defs
    return [
        ("session", lambda: _session(conn, shape), ""),
        ("core types", lambda: _core_types(conn, shape), ""),
        ("every node runs on its defaults", lambda: _defaults_run(conn, shape), ""),
        ("the demo", (lambda: _demo(conn, shape, demo)) if demo else None, "no demo given"),
        ("cancel mid-run", (lambda: _cancel(conn, shape, slow)) if has_slow else None, "no slow node"),
        ("answers off the run path", (lambda: _off_path(conn, shape, slow)) if has_slow else None, "no slow node"),
        ("hostile widget values", lambda: _hostile(conn, shape), ""),
    ]


def check(factory: Callable, demo: dict | None = None, slow: str | None = None, modes: tuple[str, ...] = ("process", "http")) -> list[Result]:
    """Every case against `factory` (an `Engine` subclass or a function returning one), in each mode.
    `slow` defaults to `<id>.slow` when the engine has it; cases that need what isn't given are
    reported as skipped (ok, saying why)."""
    from .cli import Local, Remote, hello

    results: list[Result] = []
    for mode in modes:
        conn = None
        try:
            conn = Local(factory) if mode == "process" else Remote(_http(factory, TOKEN), TOKEN)
            shape = Shape(hello(conn), conn.call("schema", {}))
        except Exception as e:  # noqa: BLE001 (an engine that won't start fails every case)
            results.append(Result(f"{mode}: the engine starts", False, f"{type(e).__name__}: {e}"))
            if conn is not None:
                conn.close()
            continue
        slow_kind = slow or (f"{shape.id}.slow" if f"{shape.id}.slow" in shape.defs else None)
        for name, run, skip in _cases(conn, shape, demo, slow_kind):
            if run is None:
                results.append(Result(f"{mode}: {name}", True, f"skipped: {skip}"))
                continue
            try:
                run()
                results.append(Result(f"{mode}: {name}", True))
            except Failed as e:
                results.append(Result(f"{mode}: {name}", False, str(e)))
            except Exception as e:  # noqa: BLE001 (a case that crashes is a failure, named)
                results.append(Result(f"{mode}: {name}", False, f"{type(e).__name__}: {e}"))
        conn.close()
    return results


def assert_conformant(factory: Callable, demo: dict | None = None, slow: str | None = None, modes: tuple[str, ...] = ("process", "http")) -> None:
    """`check`, failing with every failed case named."""
    failed = [r for r in check(factory, demo, slow, modes) if not r.ok]
    if failed:
        raise AssertionError("not conformant:\n" + "\n".join(str(r) for r in failed))
