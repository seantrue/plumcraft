"""The engine side of the HTTP binding (mode 3, spec 16.4), as `crates/engine/src/http.rs`.

    GET  /v1/health                     {ok, state}
    POST /v1/rpc                        every operation as one JSON-RPC 2.0 request
    POST /v1/blobs/has                  {"handles": [...]} -> {"have", "missing"}
    PUT  /v1/blobs/{md5,b3}/raw/<hex>        an upload, verified against its handle (422 otherwise)
    GET  /v1/blobs/{md5,b3}/{raw,nd1}/<hex>  a blob's bytes; Range: bytes=a-b honoured

Every request carries `Authorization: Bearer <token>`, and the server won't start without one, on
any address: a page in a browser can reach loopback. `ThreadingHTTPServer` serves each request on its own thread, so a
long-poll or a running graph never blocks a cancel.
"""

from __future__ import annotations

import hmac
import json
import os
import secrets
import stat
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from . import INTERNAL, INVALID_REQUEST, PARSE, ProtoError, data
from .host import Factory, Host
from .serve import ENV_TOKEN, MAX_FRAME, _response

MAX_UPLOAD = 512 << 20

UNAUTHORIZED, TOO_LARGE, INTEGRITY, NOT_FOUND, BAD_PARAMS, UNAVAILABLE, UNKNOWN_OP = -32005, -32004, -32007, -32003, -32602, -32006, -32601


def file_token(path: str) -> str:
    """The token in `path`, made there (readable by this user alone) when there is none yet. A file
    others can reach is refused: they would hold the engine's key. On Windows, whose files have no
    owner id or mode bits here (that is in ACLs, which the standard library can't read), only a
    plain file is required: keep it in a folder of your own, such as under `%LOCALAPPDATA%`."""
    p = Path(path)
    try:
        fd = os.open(p, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError:
        info = p.stat(follow_symlinks=False)
        private = os.name == "nt" or (info.st_uid == os.getuid() and not info.st_mode & 0o077)
        if not stat.S_ISREG(info.st_mode) or not private:
            raise SystemExit(f"{path}: a token file must be a plain file of yours that nobody else can reach") from None
        return p.read_text("utf-8").strip()
    token = secrets.token_hex(32)
    with os.fdopen(fd, "w") as f:
        f.write(token + "\n")
    return token


def listen_args(argv: list[str]) -> tuple[str, str | None] | None:
    """`--listen host:port [--port-file f]`, or None."""

    def get(flag: str) -> str | None:
        return argv[argv.index(flag) + 1] if flag in argv and argv.index(flag) + 1 < len(argv) else None

    listen = get("--listen")
    return (listen, get("--port-file")) if listen else None


def parse_range(v: str, total: int) -> tuple[int, int] | None:
    """`bytes=a-b`, `bytes=a-` or `bytes=-n` within `total`; anything else is None (the whole blob)."""
    if not v.startswith("bytes=") or "-" not in v or total == 0:
        return None
    a, b = v[len("bytes=") :].split("-", 1)
    try:
        if not a.strip():
            start, end = max(total - int(b), 0), total - 1
        elif not b.strip():
            start, end = int(a), total - 1
        else:
            start, end = int(a), min(int(b), total - 1)
    except ValueError:
        return None
    return (start, end) if start <= end else None


def serve_http(factory: Factory, listen: str, token: str, port_file: str | None = None) -> None:
    host_s, _, port_s = listen.rpartition(":")
    host_s = host_s.strip("[]")
    if not token:
        raise SystemExit(f"an HTTP engine needs a token: set {ENV_TOKEN}, or pass --token-file")
    engine_host = Host(factory, in_process=False)
    engine_host.serve_blob_urls()

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, fmt: str, *args: Any) -> None:  # stdout is never the data channel
            pass

        def _send(self, status: int, body: bytes, ctype: str = "application/json", headers: dict | None = None) -> None:
            self.send_response(status)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            for k, v in (headers or {}).items():
                self.send_header(k, v)
            self.end_headers()
            self.wfile.write(body)

        def _json(self, status: int, obj: Any) -> None:
            # "replace": a lone surrogate in a hostile value can't be UTF-8, and the reply must still go
            self._send(status, json.dumps(obj, separators=(",", ":"), ensure_ascii=False).encode("utf-8", "replace"))

        def _error(self, status: int, code: int, message: str) -> None:
            self._json(status, {"code": code, "message": message})

        def _authorised(self) -> bool:
            got = self.headers.get("Authorization", "")
            return got.startswith("Bearer ") and hmac.compare_digest(got[len("Bearer ") :].encode(), token.encode())

        def _body(self, limit: int) -> bytes | None:
            try:
                n = int(self.headers.get("Content-Length", "0"))
            except ValueError:
                return None
            if n < 0 or n > limit:
                return None
            return self.rfile.read(n)

        def _path(self) -> str:
            return self.path.split("?", 1)[0]

        def _handle_from(self, p: str) -> str | None:
            parts = p[len("/v1/blobs/") :].split("/")
            return data.parse_handle(":".join(parts)) if len(parts) == 3 else None

        def do_GET(self) -> None:  # noqa: N802 (http.server's naming)
            p = self._path()
            if not self._authorised():
                return self._error(401, UNAUTHORIZED, "a bearer token is required")
            if p == "/v1/health":
                return self._json(200, {"ok": True, "state": engine_host.wait_ready(0)})
            if not p.startswith("/v1/blobs/"):
                return self._error(404, UNKNOWN_OP, f"no route {p}")
            handle = self._handle_from(p)
            engine = engine_host.engine
            blob = data.read_blob(engine.store_root, handle) if handle and engine and engine.store_root else None
            if blob is None:
                return self._error(404, NOT_FOUND, "no such blob")
            payload = blob.tobytes()
            rng = parse_range(self.headers.get("Range", ""), len(payload))
            headers = {"ETag": f'"{handle}"', "Cache-Control": "immutable"}
            if rng:
                a, b = rng
                headers["Content-Range"] = f"bytes {a}-{b}/{len(payload)}"
                return self._send(206, payload[a : b + 1], "application/octet-stream", headers)
            self._send(200, payload, "application/octet-stream", headers)

        def do_POST(self) -> None:  # noqa: N802
            if not self._authorised():
                return self._error(401, UNAUTHORIZED, "a bearer token is required")
            p = self._path()
            body = self._body(MAX_FRAME)
            if body is None:
                return self._error(413, TOO_LARGE, "request too large")
            if p == "/v1/rpc":
                try:
                    req = json.loads(body)
                except ValueError as e:
                    return self._json(200, _response(None, error=ProtoError(PARSE, str(e))))
                if not isinstance(req, dict) or not isinstance(req.get("method"), str):
                    return self._json(200, _response(None, error=ProtoError(INVALID_REQUEST, "expected a JSON-RPC request")))
                try:
                    out = _response(req.get("id"), engine_host.dispatch(req["method"], req.get("params")))
                except ProtoError as e:
                    out = _response(req.get("id"), error=e)
                except Exception as e:  # never let one request take the engine down
                    out = _response(req.get("id"), error=ProtoError(INTERNAL, f"{type(e).__name__}: {e}"))
                return self._json(200, out)
            if p == "/v1/blobs/has":
                try:
                    handles = json.loads(body).get("handles") or []
                except (ValueError, AttributeError):
                    handles = []
                engine = engine_host.engine
                root = engine.store_root if engine else None
                have, missing = [], []
                for h in [h for h in handles if isinstance(h, str)][:10_000]:
                    ok = root is not None and data.parse_handle(h) is not None and data.has(root, h)
                    (have if ok else missing).append(h)
                return self._json(200, {"have": have, "missing": missing})
            self._error(404, UNKNOWN_OP, f"no route {p}")

        def do_PUT(self) -> None:  # noqa: N802
            if not self._authorised():
                return self._error(401, UNAUTHORIZED, "a bearer token is required")
            p = self._path()
            handle = self._handle_from(p) if p.startswith("/v1/blobs/") else None
            if handle is None or not data.is_raw(handle):
                return self._error(400, BAD_PARAMS, "uploads are raw handles (md5:raw: or b3:raw:)")
            engine = engine_host.engine
            if engine is None or not engine.store_root:
                return self._error(503, UNAVAILABLE, "this engine has no store")
            if data.has(engine.store_root, handle):
                self._body(MAX_UPLOAD)  # drain, to keep the connection usable
                return self._json(200, {"stored": False, "had": True})
            body = self._body(MAX_UPLOAD)
            if body is None:
                return self._error(413, TOO_LARGE, f"uploads are limited to {MAX_UPLOAD >> 20} MiB")
            if data.put_raw(engine.store_root, handle, body):
                return self._json(201, {"stored": True})
            self._error(422, INTEGRITY, "the bytes don't match the handle")

    server = ThreadingHTTPServer((host_s, int(port_s)), Handler)
    server.daemon_threads = True
    bound = f"{server.server_address[0]}:{server.server_address[1]}"
    if port_file:
        tmp = port_file + ".tmp"
        with open(tmp, "w") as f:
            f.write(bound)
        os.replace(tmp, port_file)
    print(f"plumcraft engine: listening on http://{bound}", file=sys.stderr)
    server.serve_forever()
