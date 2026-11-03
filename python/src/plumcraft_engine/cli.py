"""`plumcraft-engine`: drive a Python engine from the command line, with no Rust build
(`docs/research/17-engine-generator.md`, sections 2 and 5).

    plumcraft-engine nodes [--engine MODULE:CLASS | --attach URL]
    plumcraft-engine run [WORKFLOW.json] [--engine MODULE:CLASS | --attach URL]
                         [--watch NODE.PORT]... [--cancel-after SECONDS] [--raw]
    plumcraft-engine check [--engine MODULE:CLASS] [--mode process|http]
    plumcraft-engine new DEST [--id ID] [--types text,table,array,…] … (see plumcraft_engine.new)

- **The engine:** `--engine module:Class` (an `Engine` subclass, or a function returning an
  engine), else `[tool.plumcraft] engine` in the nearest `pyproject.toml` from the current
  directory up. It runs in this process, in a `Host`, as the editor's in-process mode does.
  `--attach URL` talks to an engine already serving HTTP (`--listen`), with
  `PLUMCRAFT_ENGINE_TOKEN` when set.
- **The workflow:** a research-shaped JSON file (`{"nodes", "wires"}`), else `[tool.plumcraft]
  demo` (relative to its `pyproject.toml`). Path inputs relative to the workflow's folder are made
  absolute, as `plumcraft run` does.
- **`run` prints each event as it arrives**, then the watched values and a summary line; `--raw`
  prints every event as JSON instead. It exits 0 when every node is `done` or `cached`, 1 when one
  isn't, 2 when the engine or workflow can't be had.
- **`check` runs the conformance cases** (`plumcraft_engine.conformance`), in process and over HTTP,
  with `[tool.plumcraft] demo` and `[tool.plumcraft.conformance] slow` when set. It exits 0 when
  every case holds, 1 when one doesn't.
"""

from __future__ import annotations

import importlib
import json
import os
import sys
import time
import tomllib
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

from . import PROTOCOL
from .host import Host
from .serve import ENV_TOKEN

USAGE = """usage: plumcraft-engine nodes [--engine MODULE:CLASS | --attach URL]
       plumcraft-engine run [WORKFLOW.json] [--engine MODULE:CLASS | --attach URL] [--watch NODE.PORT]... [--cancel-after SECONDS] [--raw]
       plumcraft-engine check [--engine MODULE:CLASS] [--mode process|http]
       plumcraft-engine new DEST [--id ID] [--name NAME] [--types text,table,array,…] [--package NAME]
                            [--sdk path|git[@REV]] [--python 3.X] [--no-pycharm] [--git]"""
CAPS = ["events.push", "events.poll", "events.resume", "status.progress", "display.stream", "codec.json"]
# A watched excerpt as printed: at most this many lines, each cut to this width.
EXCERPT_LINES, EXCERPT_WIDTH = 8, 120
# How long `run` waits for a reply.
REPLY_WAIT_S = 600.0


class Usage(Exception):
    """A mistake on the command line, or an engine or workflow that can't be had: exit 2."""


# ---------- finding the engine ----------


def project(start: Path) -> tuple[Path, dict] | None:
    """The nearest `pyproject.toml` with a `[tool.plumcraft]` table, from `start` up, and that table."""
    for d in [start, *start.parents]:
        f = d / "pyproject.toml"
        if f.is_file():
            try:
                table = tomllib.loads(f.read_text("utf-8")).get("tool", {}).get("plumcraft")
            except (OSError, tomllib.TOMLDecodeError, UnicodeDecodeError) as e:
                raise Usage(f"{f}: {e}") from None
            if isinstance(table, dict):
                return d, table
    return None


def load_engine(spec: str):
    """`module:attr` to an engine factory: the class itself, or the function."""
    module, sep, attr = spec.partition(":")
    if not sep or not module or not attr:
        raise Usage(f"--engine {spec!r}: give MODULE:CLASS (as plumcraft_text:TextEngine)")
    try:
        m = importlib.import_module(module)
    except ImportError as e:
        raise Usage(f"--engine {spec!r}: {e}") from None
    f = getattr(m, attr, None)
    if not callable(f):
        raise Usage(f"--engine {spec!r}: {module} has no {attr}")
    return f


# ---------- talking to it ----------


class Local:
    """The engine in this process, in a `Host`: every operation through its JSON face."""

    def __init__(self, factory: Any):
        self.host = Host(factory)
        state = self.host.wait_ready(30)
        if state.get("state") != "connected":
            self.host.shutdown()
            raise Usage(f"the engine didn't start: {state.get('error') or state}")
        self.where = "in process"

    def call(self, method: str, params: dict) -> Any:
        return self.host.dispatch(method, params)

    def close(self) -> None:
        self.host.shutdown()


def _project_token() -> str:
    """The token `scripts/serve` keeps in this project, when there is one."""
    p = Path(".engine-token")
    return p.read_text().strip() if p.is_file() else ""


class Remote:
    """An engine serving HTTP (mode 3): JSON-RPC 2.0 on `/v1/rpc`, the bearer token from the
    environment."""

    def __init__(self, url: str, token: str = ""):
        self.url = url.rstrip("/")
        self.token = token or os.environ.get(ENV_TOKEN, "") or _project_token()
        self.where = f"at {self.url}"
        self.n = 0

    def call(self, method: str, params: dict) -> Any:
        self.n += 1
        body = json.dumps({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params}).encode()
        req = urllib.request.Request(f"{self.url}/v1/rpc", data=body, headers={"Content-Type": "application/json"})
        if self.token:
            req.add_header("Authorization", f"Bearer {self.token}")
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                out = json.loads(r.read())
        except urllib.error.HTTPError as e:
            raise Usage(f"{self.url}: {method}: HTTP {e.code} {e.reason}") from None
        except (urllib.error.URLError, OSError, ValueError) as e:
            raise Usage(f"{self.url}: {method}: {e}") from None
        if not isinstance(out, dict):
            raise Usage(f"{self.url}: {method}: not a JSON-RPC response")
        if "error" in out:
            err = out["error"] if isinstance(out["error"], dict) else {}
            raise Usage(f"{method}: {err.get('message', out['error'])}")
        return out.get("result")

    def close(self) -> None:
        pass


def connect(opts: dict) -> Local | Remote:
    if opts["attach"]:
        return Remote(opts["attach"])
    spec = opts["engine"]
    if spec is None:
        found = project(Path.cwd())
        spec = found[1].get("engine") if found else None
        if not isinstance(spec, str):
            raise Usage("no engine: give --engine MODULE:CLASS, or set [tool.plumcraft] engine in pyproject.toml")
    return Local(load_engine(spec))


def hello(conn: Local | Remote) -> dict:
    client = {"name": "plumcraft-engine", "version": "0.1.0"}
    req = {"v": PROTOCOL, "op": "hello", "protocol": {"min": PROTOCOL, "max": PROTOCOL}, "client": client, "caps": CAPS}
    reply = conn.call("hello", req)
    if not isinstance(reply, dict) or not isinstance(reply.get("engine"), dict):
        raise Usage("hello: not a hello reply")
    return reply


# ---------- the workflow ----------


def anchored(workflow: dict, schema: dict, folder: Path) -> dict:
    """Path inputs relative to `folder` made absolute (`plumcraft_session::paths::anchor_workflow`)."""
    defs = [n for n in schema.get("nodes", []) if isinstance(n, dict)]
    path_ports = {n["id"]: {p["name"] for p in n.get("inputs", []) if isinstance(p, dict) and p.get("path")} for n in defs}
    out = json.loads(json.dumps(workflow))
    for node in (out.get("nodes") or {}).values():
        if not isinstance(node, dict) or not isinstance(node.get("values"), dict):
            continue
        for port in path_ports.get(node.get("kind"), ()):
            v = node["values"].get(port)
            if isinstance(v, str) and v and not v.startswith(("/", "~")):
                node["values"][port] = str((folder / v).resolve())
    return out


def workflow_for(path: str | None) -> tuple[dict, Path]:
    """The workflow JSON and its folder: the file given, else `[tool.plumcraft] demo`."""
    if path is None:
        found = project(Path.cwd())
        demo = found[1].get("demo") if found else None
        if not isinstance(demo, str):
            raise Usage("no workflow: give WORKFLOW.json, or set [tool.plumcraft] demo in pyproject.toml")
        file = found[0] / demo
    else:
        file = Path(path)
    try:
        w = json.loads(file.read_text("utf-8"))
    except (OSError, ValueError, UnicodeDecodeError) as e:
        raise Usage(f"{file}: {e}") from None
    if not isinstance(w, dict) or not isinstance(w.get("nodes"), dict):
        raise Usage(f"{file}: not a workflow (no nodes)")
    return w, file.resolve().parent


# ---------- printing ----------


def short(v: Any, n: int = 48) -> str:
    s = json.dumps(v, ensure_ascii=False)
    return s if len(s) <= n else s[: n - 1] + "…"


def describe(rec: dict) -> str:
    """The detail column of a status line."""
    state = rec.get("state")
    if state == "progress":
        f = rec.get("fraction") or 0.0
        return f"{f * 100:>3.0f}%  {rec.get('stage') or ''}".rstrip()
    if state == "preview":
        thumb = rec.get("thumb") or {}
        parts = [f"thumb {short(thumb.get('text', ''))}" if thumb.get("kind") == "excerpt" else f"thumb {thumb.get('kind')}" if thumb else ""]
        parts += [f"[watched {d.get('node')}.{d.get('port')}: {d.get('summary')}]" for d in rec.get("display") or [] if isinstance(d, dict)]
        return "  ".join(p for p in parts if p)
    if state == "done":
        return f"{rec.get('ms') or 0.0:.1f} ms"
    err = rec.get("error")
    if isinstance(err, dict):
        return f"{err.get('kind')}: {err.get('message')}"
    return ""


def print_display(d: dict) -> None:
    print()
    print(f"watched {d.get('node')}.{d.get('port')} ({d.get('media')}): {d.get('summary')}")
    for r in d.get("renditions") or []:
        if isinstance(r, dict) and r.get("kind") == "excerpt":
            lines = str(r.get("text", "")).splitlines()
            for line in lines[:EXCERPT_LINES]:
                print(f"  | {line[:EXCERPT_WIDTH]}{'…' if len(line) > EXCERPT_WIDTH else ''}")
            if len(lines) > EXCERPT_LINES:
                print(f"  | … {len(lines) - EXCERPT_LINES} more lines")
    if "value" in d:
        print(f"  value: {short(d['value'], 200)}")
    full = d.get("full")
    if isinstance(full, dict):
        print(f"  full: {full.get('handle')} ({(full.get('meta') or {}).get('media')}, {(full.get('meta') or {}).get('size')} bytes)")
    if isinstance(d.get("error"), dict):
        print(f"  error: {d['error'].get('message')}")


# ---------- the commands ----------


def nodes(conn: Local | Remote) -> int:
    hello(conn)
    schema = conn.call("schema", {})
    rows = []
    for n in schema.get("nodes", []):
        ins = ", ".join(f"{p['name']}:{p['type']}" for p in n.get("inputs", []))
        outs = ", ".join(f"{p['name']}:{p['type']}" for p in n.get("outputs", []))
        rows.append((n["id"], n.get("display", ""), f"({ins}) -> ({outs})"))
    w0 = max((len(r[0]) for r in rows), default=0)
    w1 = max((len(r[1]) for r in rows), default=0)
    for a, b, c in rows:
        print(f"{a:<{w0}} {b:<{w1}} {c}")
    return 0


def run(conn: Local | Remote, opts: dict) -> int:
    workflow, folder = workflow_for(opts["workflow"])
    h = hello(conn)
    schema = conn.call("schema", {})
    workflow = anchored(workflow, schema, folder)
    info = h["engine"]
    raw = opts["raw"]
    if not raw:
        print(f"({info.get('name')} {info.get('version')}, {conn.where}; {len(schema.get('nodes', []))} nodes)")
    run_id = f"cli-{os.getpid()}-{int(time.time() * 1000)}"
    watch = [{"node": n, "port": p} for n, p in opts["watch"]]
    conn.call("run", {"v": PROTOCOL, "op": "run", "run_id": run_id, "generation": 1, "graph": {"kind": "inline", "graph": workflow}, "watch": watch})
    t0, since, cancelled, reply = time.monotonic(), 0, False, None
    while reply is None:
        if opts["cancel_after"] is not None and not cancelled and time.monotonic() - t0 >= opts["cancel_after"]:
            cancelled = True
            conn.call("run.cancel", {"run_id": run_id})
            if not raw:
                print("        cancel     sent")
        p = conn.call("events.poll", {"since": since, "wait_ms": 100})
        since = p.get("last_seq", since)
        for e in p.get("events", []):
            if raw:
                print(json.dumps(e))
            body = e.get("body") or {}
            if e.get("kind") == "node.status" and e.get("run_id") in (None, run_id):
                if not raw:
                    print(f"{e.get('seq', 0):>7}  {body.get('node_id', ''):<10} {body.get('state', ''):<9} {describe(body)}".rstrip())
            elif e.get("kind") == "run.reply" and body.get("run_id") == run_id:
                reply = body
        if reply is None and time.monotonic() - t0 > REPLY_WAIT_S:
            raise Usage(f"no reply after {REPLY_WAIT_S:.0f} s")
    states = {nid: (n or {}).get("state", "?") for nid, n in (reply.get("nodes") or {}).items()}
    if not raw:
        for d in reply.get("watch") or []:
            if isinstance(d, dict):
                print_display(d)
        counts: dict[str, int] = {}
        for s in states.values():
            counts[s] = counts.get(s, 0) + 1
        err = reply.get("error")
        tail = f"; {err.get('kind')}: {err.get('message')}" if isinstance(err, dict) else ""
        print(f"run {run_id}: {', '.join(f'{v} {k}' for k, v in sorted(counts.items()))} in {time.monotonic() - t0:.2f} s{tail}")
    return 0 if states and all(s in ("done", "cached") for s in states.values()) else 1


def check(opts: dict) -> int:
    from . import conformance

    found = project(Path.cwd())
    table = found[1] if found else {}
    spec = opts["engine"] or table.get("engine")
    if not isinstance(spec, str):
        raise Usage("no engine: give --engine MODULE:CLASS, or set [tool.plumcraft] engine in pyproject.toml")
    factory = load_engine(spec)
    demo = None
    if isinstance(table.get("demo"), str):
        # its relative paths made absolute, as `run` does: a demo that reads a file works here too
        workflow, folder = workflow_for(None)
        demo = anchored(workflow, factory().schema(), folder)
    conf = table.get("conformance")
    slow = conf.get("slow") if isinstance(conf, dict) else None
    modes = (opts["mode"],) if opts["mode"] else ("process", "http")
    results = conformance.check(factory, demo=demo, slow=slow if isinstance(slow, str) else None, modes=modes)
    for r in results:
        print(f"{r}{'  (' + r.why + ')' if r.ok and r.why else ''}")
    failed = sum(not r.ok for r in results)
    print(f"{len(results) - failed} of {len(results)} cases hold")
    return 1 if failed else 0


def parse(argv: list[str]) -> tuple[str, dict]:
    if not argv or argv[0] not in ("nodes", "run", "check"):
        raise Usage(USAGE)
    cmd, opts = argv[0], {"engine": None, "attach": None, "workflow": None, "watch": [], "cancel_after": None, "raw": False, "mode": None}
    it = iter(argv[1:])
    for a in it:
        if a in ("--engine", "--attach", "--watch", "--cancel-after", "--mode"):
            v = next(it, None)
            if v is None:
                raise Usage(f"{a} needs a value")
            if a == "--watch":
                node, sep, port = v.partition(".")
                if not sep or not node or not port:
                    raise Usage("--watch needs NODE.PORT")
                opts["watch"].append((node, port))
            elif a == "--cancel-after":
                try:
                    opts["cancel_after"] = float(v)
                except ValueError:
                    raise Usage("--cancel-after needs seconds") from None
            elif a == "--mode" and v not in ("process", "http"):
                raise Usage("--mode is process or http")
            else:
                opts[a[2:]] = v
        elif a == "--raw":
            opts["raw"] = True
        elif a.startswith("-"):
            raise Usage(f"unknown option {a}\n{USAGE}")
        elif cmd == "run" and opts["workflow"] is None:
            opts["workflow"] = a
        else:
            raise Usage(f"unexpected {a!r}\n{USAGE}")
    if opts["engine"] and opts["attach"]:
        raise Usage("--engine and --attach: give one")
    return cmd, opts


def main(argv: list[str] | None = None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    if argv[:1] == ["new"]:
        from .new import main as new

        return new(argv[1:])
    try:
        cmd, opts = parse(argv)
        if cmd == "check":
            return check(opts)
        conn = connect(opts)
        try:
            return nodes(conn) if cmd == "nodes" else run(conn, opts)
        finally:
            conn.close()
    except Usage as e:
        print(f"plumcraft-engine: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())

