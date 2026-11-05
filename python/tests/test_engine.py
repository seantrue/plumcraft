"""The Python SDK on its own. The interface conformance suite (`crates/text/tests/conformance.rs`)
drives this engine through the Rust host over a real socket; these tests cover what is
Python-specific: the host without a socket, a node that raises, the token check, the frame cap."""

import json
import os
import socket
import threading
import time

import pytest
import scratch

from plumcraft_engine import CancelToken, Engine, Node, NodeError, exec, simple
from plumcraft_engine.host import Host
from plumcraft_engine.serve import MAX_FRAME, serve_unix
from plumcraft_text import engine

CHAIN = {
    "nodes": {"a": {"kind": "text.literal", "values": {"text": "the corgi"}}, "b": {"kind": "text.upper"}, "c": {"kind": "text.words"}},
    "wires": [
        {"from": {"node": "a", "port": "text"}, "to": {"node": "b", "port": "text"}},
        {"from": {"node": "b", "port": "text"}, "to": {"node": "c", "port": "text"}},
    ],
}


def run_req(run_id, graph, **extra):
    return {"v": 2, "op": "run", "run_id": run_id, "generation": 1, "graph": {"kind": "inline", "graph": graph}, **extra}


def reply_of(host, run_id, timeout=10.0):
    deadline, since = time.monotonic() + timeout, 0
    while time.monotonic() < deadline:
        p = host.poll(since, 200, run_id)
        since = p["last_seq"]
        for e in p["events"]:
            if e["kind"] == "run.reply":
                return e["body"]
    raise AssertionError(f"no reply for {run_id}")


@pytest.fixture
def host():
    h = Host(engine)
    assert h.wait_ready(5)["state"] == "connected"
    yield h
    h.shutdown()


def test_chain_runs_with_outputs_and_order(host):
    host.run(run_req("r", CHAIN, targets=["c"]))
    reply = reply_of(host, "r")
    assert reply["ok"] and reply["order"] == ["a", "b", "c"]
    assert reply["nodes"]["c"]["outputs"] == {"count": {"kind": "json", "value": 2}}
    assert host.get("r")["phase"] == "finished"


def test_cancel_and_tombstone(host):
    slow = {
        "nodes": {"a": {"kind": "text.literal"}, "s": {"kind": "text.slow", "values": {"seconds": 30}}},
        "wires": [{"from": {"node": "a", "port": "text"}, "to": {"node": "s", "port": "text"}}],
    }
    host.run(run_req("c1", slow))
    time.sleep(0.2)
    assert host.get("c1")["nodes"]["s"]["state"] == "progress"
    t = time.monotonic()
    host.cancel("c1")
    assert reply_of(host, "c1")["error"]["kind"] == "cancelled"
    assert time.monotonic() - t < 0.5
    host.cancel("early")
    host.run(run_req("early", slow))
    reply = reply_of(host, "early")
    assert reply["error"]["kind"] == "cancelled" and all(n["state"] == "cancelled" for n in reply["nodes"].values())


def test_a_raising_node_is_an_op_error_and_the_engine_carries_on():
    def boom(_):
        raise ZeroDivisionError("division by zero")

    def build():
        return engine().with_node(simple({"id": "test.boom", "inputs": [], "outputs": [{"name": "text", "type": "TEXT"}]}, boom))

    h = Host(build)
    h.wait_ready(5)
    h.run(run_req("b", {"nodes": {"x": {"kind": "test.boom"}}}))
    reply = reply_of(h, "b")
    assert reply["nodes"]["x"]["error"] == {"kind": "op", "message": "ZeroDivisionError: division by zero"}
    h.run(run_req("after", CHAIN))
    assert reply_of(h, "after")["ok"]
    h.shutdown()


def test_an_sdk_failure_fails_the_run_as_engine_and_rebuilds(monkeypatch):
    from plumcraft_engine import exec as ex

    builds = []

    def build():
        builds.append(1)
        return engine()

    h = Host(build)
    h.wait_ready(5)
    real = ex.run
    monkeypatch.setattr(ex, "run", lambda *a: (_ for _ in ()).throw(RuntimeError("executor bug")))
    h.run(run_req("x", CHAIN))
    assert reply_of(h, "x")["error"]["kind"] == "engine"
    monkeypatch.setattr(ex, "run", real)
    h.run(run_req("y", CHAIN))
    assert reply_of(h, "y")["ok"] and len(builds) == 2
    h.shutdown()


def test_bad_params_and_unknown_ops_are_protocol_errors(host):
    from plumcraft_engine import BAD_PARAMS, NOT_FOUND, UNKNOWN_OP, ProtoError

    for method, params, code in [
        ("run", {"run_id": ""}, BAD_PARAMS),
        ("run", {"run_id": "g", "generation": -1}, BAD_PARAMS),
        ("run.get", {"run_id": "nope"}, NOT_FOUND),
        ("teleport", {}, UNKNOWN_OP),
    ]:
        with pytest.raises(ProtoError) as e:
            host.dispatch(method, params)
        assert e.value.code == code


def test_node_error_kinds():
    assert NodeError.cancelled().to_json() == {"kind": "cancelled", "message": "cancelled"}
    assert isinstance(engine(), Engine)


class _Ui:
    """The UI end of the socket: listen, let the engine connect, and talk lines."""

    def __init__(self, token, kind=None):
        """`kind` is "unix" or "tcp"; by default Unix where the platform has it (not Windows)."""
        kind = kind or ("unix" if hasattr(socket, "AF_UNIX") else "tcp")
        if kind == "unix":
            self.dir = scratch.mkdtemp("ui-")
            loc = os.path.join(self.dir, "e.sock")
            srv = socket.socket(socket.AF_UNIX)
            srv.bind(loc)
        else:
            srv = socket.socket(socket.AF_INET)
            srv.bind(("127.0.0.1", 0))
            loc = f"tcp:127.0.0.1:{srv.getsockname()[1]}"
        srv.listen(1)
        self.thread = threading.Thread(target=serve_unix, args=(engine, loc, token), daemon=True)
        self.thread.start()
        srv.settimeout(5)
        self.sock, _ = srv.accept()
        srv.close()
        self.f = self.sock.makefile("rb")
        self.next = 0

    def send_raw(self, data: bytes):
        self.sock.sendall(data)

    def call(self, method, params):
        self.next += 1
        self.send_raw((json.dumps({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params}) + "\n").encode())
        while True:
            line = self.f.readline()
            if not line:
                return None
            msg = json.loads(line)
            if msg.get("id") == self.next:
                return msg

    def close(self):
        self.f.close()
        self.sock.close()
        self.thread.join(5)


@pytest.mark.parametrize("kind", ["unix", "tcp"])
def test_wrong_token_is_refused_and_right_token_is_served(kind):
    if kind == "unix" and not hasattr(socket, "AF_UNIX"):
        pytest.skip("no Unix sockets here (Windows): the local binding is TCP")
    ui = _Ui("s3cret", kind)
    assert ui.call("schema", {})["error"]["code"] == -32005, "nothing before hello"
    assert ui.call("hello", {"v": 2, "op": "hello", "token": "wrong"})["error"]["code"] == -32005
    assert ui.call("schema", {})["error"]["code"] == -32005
    assert ui.call("hello", {"v": 2, "op": "hello", "token": "s3cret"})["result"]["protocol"] == 2
    assert "nodes" in ui.call("schema", {})["result"]
    ui.close()
    assert not ui.thread.is_alive(), "the engine stops when the UI closes the socket"


def test_locators_are_unix_paths_or_loopback_tcp():
    from plumcraft_engine.serve import endpoint_from_env, parse_endpoint

    assert parse_endpoint("unix:/tmp/x/e.sock") == ("unix", "/tmp/x/e.sock")
    assert parse_endpoint("/tmp/x/e.sock") == ("unix", "/tmp/x/e.sock")
    assert parse_endpoint("tcp:127.0.0.1:4100") == ("tcp", ("127.0.0.1", 4100))
    assert parse_endpoint("tcp:[::1]:9") == ("tcp", ("::1", 9))
    for bad in ["tcp:192.168.2.5:80", "tcp:nonsense", "tcp:127.0.0.1:x", "unix:"]:
        with pytest.raises(ValueError):
            parse_endpoint(bad)
    assert endpoint_from_env(["--connect", "tcp:127.0.0.1:1"])[0] == "tcp:127.0.0.1:1"


def test_an_over_long_frame_drops_the_connection():
    ui = _Ui("")
    ui.send_raw(b"x" * (MAX_FRAME + 1))
    ui.thread.join(5)
    assert not ui.thread.is_alive()
    ui.f.close()
    ui.sock.close()


def test_http_ranges_and_listen_args():
    from plumcraft_engine.http import listen_args, parse_range

    assert parse_range("bytes=0-9", 100) == (0, 9)
    assert parse_range("bytes=90-", 100) == (90, 99)
    assert parse_range("bytes=-10", 100) == (90, 99)
    assert parse_range("bytes=50-500", 100) == (50, 99)
    assert parse_range("bytes=9-0", 100) is None
    assert parse_range("items=0-1", 100) is None
    assert parse_range("bytes=0-0", 0) is None
    assert listen_args(["--listen", "127.0.0.1:0", "--port-file", "f"]) == ("127.0.0.1:0", "f")
    assert listen_args(["--connect", "unix:/x"]) is None


def test_http_needs_a_token_on_any_address():
    from plumcraft_engine.http import serve_http

    for listen in ("0.0.0.0:0", "127.0.0.1:0"):
        with pytest.raises(SystemExit, match="token"):
            serve_http(engine, listen, "")


def test_a_token_file_is_made_private_and_reused(tmp_path):
    from plumcraft_engine.http import file_token

    path = tmp_path / "token"
    token = file_token(str(path))
    assert len(token) == 64
    assert file_token(str(path)) == token
    if os.name == "nt":
        return  # no owner id or mode bits to check (http.file_token): the folder keeps it private
    assert path.stat().st_mode & 0o777 == 0o600
    path.chmod(0o644)
    with pytest.raises(SystemExit, match="nobody else"):
        file_token(str(path))


def test_uploads_are_verified_and_sources_are_keyed_by_content(tmp_path):
    from plumcraft_engine import data

    root = str(tmp_path / "store")
    b = b"a corgi on a floral carpet"
    h = data.raw(b)
    assert not data.put_raw(root, h, b"something else"), "a mismatch stores nothing"
    assert data.read_blob(root, h) is None
    assert data.put_raw(root, h, b)
    assert data.read_blob(root, h).tobytes() == b
    host = Host(lambda: engine().with_store(root))
    host.wait_ready(5)
    src = {"nodes": {"l": {"kind": "text.load", "values": {"path": {"source": {"handle": h, "name": "../../etc/x.txt"}}}}}}
    host.run(run_req("s", src, targets=["l"]))
    reply = reply_of(host, "s")
    assert reply["nodes"]["l"]["outputs"]["text"]["value"] == b.decode()
    assert (tmp_path / "store" / ".sources" / h.rsplit(":", 1)[1] / "x.txt").is_file(), "only the name's last part is used"
    host.shutdown()


def test_rgba8_averages_each_cell_as_a_loop_would():
    import numpy as np

    from plumcraft_engine import arrays

    def by_hand(rgb, max_px):
        h, w = rgb.shape[:2]
        scale = max(max(h, w) / max(max_px, 1), 1.0)
        th, tw = max(round(h / scale), 1), max(round(w / scale), 1)
        out = np.empty((th, tw, 3), dtype=np.uint8)
        for ty in range(th):
            y0, y1 = ty * h // th, max((ty + 1) * h // th, ty * h // th + 1)
            for tx in range(tw):
                x0, x1 = tx * w // tw, max((tx + 1) * w // tw, tx * w // tw + 1)
                out[ty, tx] = np.clip(rgb[y0:y1, x0:x1].reshape(-1, 3).mean(axis=0, dtype=np.float64) * 255 + 0.5, 0, 255).astype(np.uint8)
        return out

    rng = np.random.default_rng(1)
    for h, w, px in [(192, 256, 160), (7, 300, 160), (300, 7, 160), (1, 1, 160), (16, 16, 160), (33, 17, 5), (5, 5, 1)]:
        rgb = rng.random((h, w, 3), dtype=np.float32)
        got = arrays.rgba8(rgb, px)
        assert np.abs(got[..., :3].astype(int) - by_hand(rgb.astype(np.float64), px).astype(int)).max() <= 1, (h, w, px)
        assert (got[..., 3] == 255).all()
    spoilt = np.ones((8, 8, 3), dtype=np.float32)
    spoilt[0, 0] = np.nan
    assert arrays.rgba8(spoilt, 8)[7, 7, 0] == 255, "a NaN stays in its own cell"


def test_a_live_object_on_a_wire_stays_in_the_engine():
    """A value that isn't JSON or bytes (a model, a handle) is watched and targeted without
    failing: the reply names it, and a later node still gets it."""

    class Live:
        pass

    class Maker(Node):
        definition = {"id": "t.live", "inputs": [], "outputs": [{"name": "model", "type": "x.t.MODEL"}]}

        def run(self, inputs, cx):
            return {"model": Live()}

    class User(Node):
        definition = {"id": "t.use", "inputs": [{"name": "model", "type": "x.t.MODEL", "socket": "socket"}], "outputs": [{"name": "name", "type": "STRING"}]}

        def run(self, inputs, cx):
            return {"name": type(inputs["model"]).__name__}

    live = Engine({"id": "t", "salt": "t"}).with_store(None).with_node(Maker()).with_node(User())
    wires = [{"from": {"node": "m", "port": "model"}, "to": {"node": "u", "port": "model"}}]
    graph = {"kind": "inline", "graph": {"version": 1, "nodes": {"m": {"kind": "t.live"}, "u": {"kind": "t.use"}}, "wires": wires}}
    watch = [{"node": "m", "port": "model", "index": 0}]
    req = {"run_id": "r", "generation": 1, "graph": graph, "targets": ["m", "u"], "watch": watch}
    reply = exec.run(live, req, lambda _: None, CancelToken(), "test", exec.Memo())
    text = json.dumps(reply)
    assert reply["nodes"]["u"]["outputs"]["name"] == {"kind": "json", "value": "Live"}
    assert reply["nodes"]["m"]["outputs"]["model"]["kind"] == "opaque"
    assert "Live" in reply["nodes"]["m"]["outputs"]["model"]["summary"], text
    assert "value" not in reply["watch"][0], text
