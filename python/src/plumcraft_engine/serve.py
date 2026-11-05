"""The engine side of the local socket binding (mode 2, spec 16.3), as `crates/engine/src/serve.rs`.

The engine process connects back to the UI's socket (a Unix socket, or loopback TCP on Windows,
whose Python has no `AF_UNIX`), presents its token in `hello`, and serves
every operation through a `Host`. Each request runs on its own thread, so nothing waits behind a
run; every event is pushed as a `{"method": "event"}` notification. The process exits when the
socket closes, so a UI that crashes leaves no orphan.

Framing: one JSON-RPC 2.0 object per line, at most `MAX_FRAME` bytes.
"""

from __future__ import annotations

import hmac
import ipaddress
import json
import os
import socket
import sys
import threading
from typing import Any

from . import INVALID_REQUEST, PARSE, UNAUTHORIZED, ProtoError
from .host import Factory, Host

ENV_SOCKET = "PLUMCRAFT_ENGINE_SOCKET"
ENV_TOKEN = "PLUMCRAFT_ENGINE_TOKEN"
MAX_FRAME = 16 << 20


def endpoint_from_env(argv: list[str]) -> tuple[str, str]:
    """Where to connect and the token to present: `--connect <locator>` or the environment. The
    locator is `unix:<path>`, `tcp:127.0.0.1:<port>` or a bare socket path (`parse_endpoint`)."""
    loc = None
    if "--connect" in argv:
        i = argv.index("--connect")
        if i + 1 < len(argv) and argv[i + 1].startswith(("unix:", "tcp:")):
            loc = argv[i + 1]
    loc = loc or os.environ.get(ENV_SOCKET)
    if not loc:
        raise SystemExit(f"no socket: pass --connect unix:<path> (or tcp:127.0.0.1:<port>) or set {ENV_SOCKET}")
    return loc, os.environ.get(ENV_TOKEN, "")


def parse_endpoint(loc: str) -> tuple[str, str | tuple[str, int]]:
    """A locator, as `crates/engine/src/local.rs`: `("unix", path)` for `unix:<path>` or a bare path,
    `("tcp", (host, port))` for `tcp:<loopback>:<port>` (`[::1]:<port>` for IPv6). Anything else, or
    an address off loopback, raises `ValueError`."""
    if loc.startswith("tcp:"):
        rest = loc[len("tcp:") :]
        host, sep, port = rest.rpartition(":")
        host = host[1:-1] if host.startswith("[") and host.endswith("]") else host
        if not sep or not port.isdigit():
            raise ValueError(f"{loc!r} is not an address")
        try:
            loopback = ipaddress.ip_address(host).is_loopback
        except ValueError:
            raise ValueError(f"{loc!r} is not an address") from None
        if not loopback:
            raise ValueError(f"{loc!r}: the local binding is loopback only")
        return "tcp", (host, int(port))
    path = loc[len("unix:") :] if loc.startswith("unix:") else loc
    if not path:
        raise ValueError("an empty socket path")
    return "unix", path


def _connect(loc: str) -> socket.socket:
    kind, addr = parse_endpoint(loc)
    if kind == "tcp":
        host, port = addr  # type: ignore[misc]
        sock = socket.create_connection((host, port))
        sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        return sock
    if not hasattr(socket, "AF_UNIX"):
        raise OSError(f"{addr}: Unix sockets aren't available here; the host must offer tcp:")
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(addr)
    return sock


class _Writer:
    """One line at a time, from any thread."""

    def __init__(self, sock: socket.socket):
        self._sock = sock
        self._lock = threading.Lock()

    def send(self, obj: dict) -> bool:
        """Write one frame; False once the socket is gone. A value JSON can't carry (NaN, an
        object) turns a reply into an `internal` error and drops an event, with a note on stderr."""
        try:
            line = _encode(obj)
        except (TypeError, ValueError) as e:
            print(f"plumcraft engine: unencodable {obj.get('method') or 'reply'}: {e}", file=sys.stderr)
            if "id" not in obj:
                return True
            line = _encode(_response(obj["id"], error=ProtoError(-32603, f"the reply can't be encoded as JSON: {e}")))
        with self._lock:
            try:
                self._sock.sendall(line)
                return True
            except OSError:
                return False


def _encode(obj: dict) -> bytes:
    return json.dumps(obj, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode() + b"\n"


def _response(rid: Any, outcome: Any = None, error: ProtoError | None = None) -> dict:
    if error is not None:
        return {"jsonrpc": "2.0", "id": rid, "error": error.to_json()}
    return {"jsonrpc": "2.0", "id": rid, "result": outcome}


def _read_lines(sock: socket.socket):
    """Yield complete lines; stop at end of stream or on a line over `MAX_FRAME` (spec 18, "Frames")."""
    buf = b""
    while True:
        try:
            chunk = sock.recv(65536)
        except OSError:
            return
        if not chunk:
            return
        buf += chunk
        while True:
            i = buf.find(b"\n")
            if i < 0:
                break
            line, buf = buf[:i], buf[i + 1 :]
            if len(line) > MAX_FRAME:
                return
            yield line
        if len(buf) > MAX_FRAME:
            return


def serve_unix(factory: Factory, path: str, token: str) -> None:
    """Serve until the UI closes the connection. `path` is a locator (`parse_endpoint`); the name is
    from when the binding was Unix sockets only."""
    sock = _connect(path)
    writer = _Writer(sock)
    host = Host(factory, in_process=False)

    def push() -> None:
        since = 0
        while True:
            p = host.poll(since, 1000)
            since = p["last_seq"]
            for env in p["events"]:
                if not writer.send({"jsonrpc": "2.0", "method": "event", "params": env}):
                    return

    threading.Thread(target=push, name="plumcraft-push", daemon=True).start()

    def handle(req: dict) -> None:
        rid = req.get("id")
        try:
            out, err = host.dispatch(req["method"], req.get("params")), None
        except ProtoError as e:
            out, err = None, e
        except Exception as e:  # never let one request take the engine down
            out, err = None, ProtoError(-32603, f"{type(e).__name__}: {e}")
        if rid is not None:
            writer.send(_response(rid, out, err))

    authorised = not token
    for line in _read_lines(sock):
        try:
            req = json.loads(line)
        except ValueError as e:
            writer.send(_response(None, error=ProtoError(PARSE, str(e))))
            continue
        if not isinstance(req, dict) or not isinstance(req.get("method"), str):
            if isinstance(req, dict) and ("result" in req or "error" in req):
                continue  # a response: this engine makes no calls
            writer.send(_response(None, error=ProtoError(INVALID_REQUEST, "expected a JSON-RPC request")))
            continue
        method = req["method"]
        if method == "hello":
            params = req.get("params") if isinstance(req.get("params"), dict) else {}
            presented = params.get("token") if isinstance(params.get("token"), str) else ""
            authorised = not token or hmac.compare_digest(presented.encode(), token.encode())
        if not authorised:
            if req.get("id") is not None:
                writer.send(_response(req["id"], error=ProtoError(UNAUTHORIZED, "hello with the engine's token first")))
            continue
        if method == "shutdown":
            break
        threading.Thread(target=handle, args=(req,), name="plumcraft-req", daemon=True).start()
    try:
        sock.close()
    except OSError:
        pass
    host.shutdown(wait=False)


def _lifeline() -> None:
    """Exit when stdin closes: a UI that spawns an HTTP engine passes a pipe, so the engine goes
    when the UI does, however the UI ends."""

    def watch() -> None:
        while sys.stdin.buffer.read(256):
            pass
        print("plumcraft engine: the UI that started this engine has gone; exiting", file=sys.stderr)
        os._exit(0)

    threading.Thread(target=watch, name="plumcraft-lifeline", daemon=True).start()


def main(factory: Factory, argv: list[str] | None = None) -> int:
    """The entry point of an engine process: `--listen host:port [--port-file f] [--token-file f]` serves HTTP
    (mode 3); otherwise it connects back to the UI's socket (mode 2). `register` and `unregister`
    offer an installed engine to PlumCraft and withdraw it (`plumcraft_engine.register`)."""
    args = sys.argv[1:] if argv is None else argv
    # `register` and `unregister`: offer the installed engine to PlumCraft (register.py)
    if isinstance(factory, type):
        from .register import main as register_main

        done = register_main(factory, args)
        if done is not None:
            return done
    from .http import file_token, listen_args, serve_http

    listen = listen_args(args)
    if listen is not None:
        if "--lifeline" in args:
            _lifeline()
        token = os.environ.get(ENV_TOKEN, "")
        if not token and "--token-file" in args and args.index("--token-file") + 1 < len(args):
            token = file_token(args[args.index("--token-file") + 1])
        serve_http(factory, listen[0], token, listen[1])
        return 0
    path, token = endpoint_from_env(args)
    serve_unix(factory, path, token)
    return 0
