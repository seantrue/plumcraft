"""The host, as `crates/engine/src/host.rs`: the engine runs graphs on a worker thread built by a
factory; everything off the run path (spec 5.5) is answered at once from a lock-protected run
table: `hello`, `schema`, `run.cancel`, `run.get`, `events.poll`. A running graph never blocks
them.

The host assigns every envelope's `seq` in one place, keeps a ring for resume and polling, and
keeps tombstones for cancels that arrive before their run. An exception that escapes the
executor (a bug in the SDK, not in a node) fails the run with `kind: engine` and rebuilds the
engine from the factory; five in a minute stop the rebuilds.
"""

from __future__ import annotations

import hmac
import os
import queue
import secrets
import threading
import time
import traceback
from collections import deque
from collections.abc import Callable
from typing import Any

from . import BAD_PARAMS, NOT_FOUND, PROTOCOL, UNAVAILABLE, UNKNOWN_OP, CancelToken, Engine, NodeError, ProtoError, data, exec, now_ts

Factory = Callable[[], Engine]

RING = 4096
KEEP_RUNS = 16
MAX_FAILURES = 5
FAILURE_WINDOW_S = 60.0
MAX_WAIT_MS = 30_000
# How long `hello` waits for the engine to be built (the Rust host's `HELLO_WAIT`).
HELLO_WAIT_S = 8.0
TERMINAL = {"done", "cached", "error", "skipped", "cancelled"}


def _write_nonce(root: str) -> tuple[str, str] | None:
    """Write a nonce file under `{root}/.hello/` (mode 0600); its path and nonce, or None."""
    try:
        d = os.path.join(root, ".hello")
        os.makedirs(d, exist_ok=True)
        nonce = secrets.token_hex(32)
        path = os.path.join(d, f"e-{os.getpid()}-{nonce[:8]}.nonce")
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as f:
            f.write(nonce)
        return path, nonce
    except OSError:
        return None


def _check_nonce(path: Any, nonce: Any) -> bool:
    """Read the other side's nonce file and compare. Any failure is False."""
    if not isinstance(path, str) or not isinstance(nonce, str) or not nonce:
        return False
    try:
        with open(path) as f:
            return hmac.compare_digest(f.read(128).strip(), nonce)
    except (OSError, UnicodeDecodeError):
        return False


def failed_reply(run_id: str, generation: int, err: NodeError) -> dict:
    return {"v": PROTOCOL, "run_id": run_id, "generation": generation, "ok": False, "error": err.to_json(), "nodes": {}, "order": [], "watch": []}


class _RunEntry:
    def __init__(self, generation: int, cancel: CancelToken, last_seq: int):
        self.generation = generation
        self.phase = "accepted"
        self.nodes: dict[str, dict] = {}
        self.reply: dict | None = None
        self.cancel = cancel
        self.last_seq = last_seq

    def apply(self, rec: dict) -> None:
        if self.phase == "accepted":
            self.phase = "running"
        n = self.nodes.setdefault(rec["node_id"], {"state": "queued"})
        if rec["state"] == "preview":
            if "thumb" in rec:
                n["thumb"] = rec["thumb"]
            return
        if n["state"] in TERMINAL:
            return
        n["state"] = rec["state"]
        for k in ("fraction", "stage", "ms", "error"):
            if rec.get(k) is not None:
                n[k] = rec[k]


class Host:
    def __init__(self, factory: Factory, wake: Callable[[], None] | None = None, in_process: bool = True):
        """`in_process`: the UI shares this process, so both same-host verdicts hold without files.
        A socket or HTTP front passes False: blobs go through the store only after `session.confirm`."""
        self._factory = factory
        self._wake = wake
        self._lock = threading.Lock()
        self._cv = threading.Condition(self._lock)
        self._seq = 0
        self._ring: deque[dict] = deque()
        self._runs: dict[str, _RunEntry] = {}
        self._finished: deque[str] = deque()
        self._tombstones: set[str] = set()
        self._engine: Engine | None = None
        self._schema: dict | None = None
        self._state: dict = {"state": "connecting", "url": "inproc"}
        self._in_process = in_process
        self._same_host = {"shm": in_process, "path": in_process}
        self._nonce_path: str | None = None
        self._urls = False
        self._jobs: queue.Queue[tuple[dict, CancelToken] | None] = queue.Queue()
        self.session = f"s-{os.getpid()}"
        self._worker = threading.Thread(target=self._worker_loop, name="plumcraft-engine", daemon=True)
        self._worker.start()

    # The table. Every method named _locked_* expects the lock held.

    def _locked_push(self, run: tuple[str, int] | None, kind: str, body: dict) -> None:
        """Append an envelope (the one place `seq` is assigned), update the run table, and wake."""
        self._seq += 1
        env: dict[str, Any] = {"v": PROTOCOL, "seq": self._seq, "ts": now_ts(), "kind": kind, "body": body}
        if run is not None:
            env["run_id"], env["generation"] = run
            entry = self._runs.get(run[0])
            if entry is not None:
                entry.last_seq = self._seq
                if kind == "node.status":
                    entry.apply(body)
        self._ring.append(env)
        while len(self._ring) > RING:
            self._ring.popleft()
        self._cv.notify_all()
        if self._wake is not None:
            self._wake()

    def _set_state(self, state: dict) -> None:
        with self._lock:
            self._state = state
            self._locked_push(None, "engine.state", state)

    def _finish(self, reply: dict) -> None:
        with self._lock:
            run_id = reply["run_id"]
            entry = self._runs.get(run_id)
            if entry is not None:
                err = reply.get("error")
                entry.phase = "failed" if err else "finished"
                if err and err.get("kind") == NodeError.CANCELLED:
                    entry.phase = "cancelled"
                entry.reply = reply
            self._finished.append(run_id)
            while len(self._finished) > KEEP_RUNS:
                self._runs.pop(self._finished.popleft(), None)
            self._locked_push((run_id, reply["generation"]), "run.reply", reply)

    # Operations (spec 6.1).

    def wait_ready(self, timeout: float) -> dict:
        deadline = time.monotonic() + timeout
        with self._lock:
            while self._state["state"] == "connecting":
                left = deadline - time.monotonic()
                if left <= 0:
                    break
                self._cv.wait(left)
            return dict(self._state)

    def hello(self, req: dict) -> dict:
        # A slow-to-build engine (hashing its code for the memo salt, importing numpy) is waited
        # for, as the Rust host waits (`HELLO_WAIT`), rather than refused.
        self.wait_ready(HELLO_WAIT_S)
        with self._lock:
            engine = self._engine
        if engine is None:
            raise ProtoError(UNAVAILABLE, "the engine isn't ready")
        reply = engine.hello(req, self.session)
        reply["caps"] = sorted(set(reply["caps"]) | {"data.shm", "data.path"})
        if self._in_process:
            return reply
        # The same-host proof (spec 5.4): read the UI's nonce file, write ours for it to read.
        proof = req.get("proof") if isinstance(req.get("proof"), dict) else {}
        client_read = _check_nonce(proof.get("path"), proof.get("nonce"))
        ours = _write_nonce(engine.store_root) if engine.store_root else None
        if ours is not None:
            self._nonce_path = ours[0]
        reply["proof"] = {"path": ours[0], "nonce": ours[1]} if ours else {}
        if client_read:
            reply["proof"]["client_proof_read"] = True
        return reply

    def serve_blob_urls(self) -> None:
        """The HTTP front calls this: without the proof, values go by `url` locator."""
        self._urls = True

    @property
    def engine(self) -> Engine | None:
        with self._lock:
            return self._engine

    def confirm(self, params: dict) -> None:
        """`session.confirm`: the UI's same-host verdicts."""
        with self._lock:
            if not self._in_process:
                self._same_host = {"shm": params.get("shm") is True, "path": params.get("path") is True}
        if self._nonce_path:
            try:
                os.unlink(self._nonce_path)
            except OSError:
                pass
            self._nonce_path = None

    def schema(self, digest: str | None) -> dict:
        with self._lock:
            s = self._schema
        if s is None:
            raise ProtoError(UNAVAILABLE, "the engine isn't ready")
        if digest and digest == s["engine"].get("schema_digest"):
            return {"unchanged": True}
        return s

    def run(self, req: dict) -> dict:
        """Accept a run at once; its result arrives as a `run.reply` event and from `run.get`."""
        run_id = req.get("run_id")
        if not isinstance(run_id, str) or not run_id:
            raise ProtoError(BAD_PARAMS, "run_id is required")
        generation = req.get("generation", 0)
        if not isinstance(generation, int) or isinstance(generation, bool) or generation < 0:
            raise ProtoError(BAD_PARAMS, "generation must be a non-negative integer")
        cancel = CancelToken()
        with self._lock:
            if run_id in self._runs:
                raise ProtoError(BAD_PARAMS, "run_id already used")
            if run_id in self._tombstones:
                self._tombstones.discard(run_id)
                cancel.cancel()
            queued_behind = sum(1 for e in self._runs.values() if e.reply is None)
            self._runs[run_id] = _RunEntry(generation, cancel, self._seq)
        self._jobs.put((req, cancel))
        return {"accepted": True, "run_id": run_id, "queued_behind": queued_behind}

    def cancel(self, run_id: str) -> None:
        """Cancel at once, off the run path. Idempotent; an unknown run leaves a tombstone (spec 11)."""
        with self._lock:
            entry = self._runs.get(run_id)
            if entry is not None:
                entry.cancel.cancel()
            else:
                self._tombstones.add(run_id)

    def get(self, run_id: str) -> dict:
        """`run.get`: the resync path, answered from the table."""
        with self._lock:
            e = self._runs.get(run_id)
            if e is None:
                raise ProtoError(NOT_FOUND, f"no run {run_id}")
            return {
                "v": PROTOCOL,
                "run_id": run_id,
                "generation": e.generation,
                "phase": e.phase,
                "last_seq": e.last_seq,
                "nodes": {k: dict(v) for k, v in e.nodes.items()},
                "reply": e.reply,
            }

    def poll(self, since: int, wait_ms: int, run_id: str | None = None) -> dict:
        """`events.poll`: the events after `since`, waiting up to `wait_ms` for the first."""
        deadline = time.monotonic() + min(max(wait_ms, 0), MAX_WAIT_MS) / 1000.0
        with self._lock:
            while True:
                gap = bool(self._ring) and self._ring[0]["seq"] > since + 1
                events = [e for e in self._ring if e["seq"] > since and (run_id is None or e.get("run_id") == run_id)]
                left = deadline - time.monotonic()
                if events or gap or left <= 0:
                    if gap:
                        resync = {
                            "v": PROTOCOL,
                            "seq": self._seq,
                            "ts": now_ts(),
                            "kind": "resync",
                            "body": {"reason": "events older than the ring were dropped"},
                        }
                        events.insert(0, resync)
                    return {"events": events, "last_seq": self._seq, "gap": gap}
                self._cv.wait(left)

    def dispatch(self, method: str, params: Any) -> Any:
        """The JSON face of every operation, as the socket binding calls it."""
        p = params if isinstance(params, dict) else {}

        def s(k: str) -> str:
            v = p.get(k)
            return v if isinstance(v, str) else ""

        def n(k: str) -> int:
            v = p.get(k)
            return v if isinstance(v, int) and not isinstance(v, bool) and v >= 0 else 0

        if method == "hello":
            return self.hello(p)
        if method == "session.confirm":
            self.confirm(p)
            return {}
        if method == "ping":
            return {"v": PROTOCOL, "pong": now_ts()}
        if method == "schema":
            return self.schema(s("digest") or None)
        if method == "run":
            return self.run(p)
        if method == "run.cancel":
            self.cancel(s("run_id"))
            return {}
        if method == "run.get":
            return self.get(s("run_id"))
        if method == "events.poll":
            return self.poll(n("since"), n("wait_ms"), s("run_id") or None)
        raise ProtoError(UNKNOWN_OP, f"unknown op {exec.quoted(method[:64])}")

    def shutdown(self, wait: bool = True) -> None:
        """Stop the worker after the run in flight, cancelling it."""
        with self._lock:
            for e in self._runs.values():
                e.cancel.cancel()
        self._jobs.put(None)
        if wait:
            self._worker.join(timeout=5)

    # The worker.

    def _build(self) -> bool:
        try:
            engine = self._factory()
        except Exception as e:
            traceback.print_exc()
            self._set_state({"state": "engine_down", "reason": f"{type(e).__name__}: {e}"})
            return False
        with self._lock:
            self._engine = engine
            self._schema = engine.schema()
        self._set_state({"state": "connected", "version": engine.info.get("version", ""), "nodes": len(engine.nodes), "caps": sorted(engine.caps)})
        return True

    def _worker_loop(self) -> None:
        if not self._build():
            return
        publisher = f"py@{os.getpid()}"
        failures: deque[float] = deque()
        store = exec.Memo()
        while True:
            job = self._jobs.get()
            if job is None:
                break
            req, cancel = job
            run_id, generation = req["run_id"], req.get("generation", 0)
            with self._lock:
                engine = self._engine
            if engine is None:
                self._finish(failed_reply(run_id, generation, NodeError(NodeError.ENGINE, "the engine has stopped")))
                continue

            with self._lock:
                shm = self._same_host["shm"]
            plane = data.Plane(engine.store_root, shm, engine.limits["max_inline"], urls=self._urls, algo=req.get("algo") or "md5")

            def emit(rec: dict, run=(run_id, generation)) -> None:
                with self._lock:
                    self._locked_push(run, "node.status", rec)

            try:
                reply = exec.run(engine, req, emit, cancel, publisher, store, plane)
            except Exception as e:
                traceback.print_exc()
                what = f"{type(e).__name__}: {e}"
                with self._lock:
                    self._engine = None  # its state may be inconsistent: rebuild (spec 16.1)
                store = exec.Memo()  # results from an engine that failed aren't trusted
                # Down, then restarting, before the failed reply: a client that sees the reply
                # already knows to wait for `connected`.
                self._set_state({"state": "engine_down", "reason": what})
                self._set_state({"state": "connecting", "url": "inproc (restarting)"})
                self._finish(failed_reply(run_id, generation, NodeError(NodeError.ENGINE, f"the engine crashed ({what}); restarting")))
                now = time.monotonic()
                failures.append(now)
                while failures and now - failures[0] > FAILURE_WINDOW_S:
                    failures.popleft()
                if len(failures) >= MAX_FAILURES or not self._build():
                    self._set_state({"state": "engine_down", "reason": f"{len(failures)} crashes in a minute; stopped"})
                    break
                continue
            self._finish(reply)
        # Anything still queued fails rather than hanging (spec 12.3).
        while True:
            try:
                job = self._jobs.get_nowait()
            except queue.Empty:
                break
            if job is not None:
                req, _ = job
                self._finish(failed_reply(req["run_id"], req.get("generation", 0), NodeError(NodeError.ENGINE, "the engine has stopped")))
