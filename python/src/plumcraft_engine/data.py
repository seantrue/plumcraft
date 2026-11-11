"""How a value crosses the interface, as `crates/engine/src/data.rs` (spec 4.4, 4.5, 10.2).

- Small JSON (at most `max_inline` bytes) travels as `{"kind": "json"}`.
- Handles are `md5:` by default and `b3:` (BLAKE3) when the run's request asks for them and the
  `blake3` package is here (spec 4.3; B10); a handle is always checked in its own algorithm.
- Large text becomes an `md5:raw:` blob of its UTF-8 bytes; a large table an `md5:raw:` blob of
  its JSON. Both live in commonkit as 1-d `|u1` segments.
- Arrays (numpy) are always blobs (`md5:nd1:`): inline bytes when small, a segment otherwise.
- A blob that needs the store is published only when the session proved the UI reads this
  engine's files (`shm`); otherwise it is `unavailable` (spec 16.3).
- A raw blob of at least `COMPRESS_MIN` bytes that deflate makes at least 10% smaller is stored
  compressed (zlib format, level 1) under form `plumz`, keyed by its uncompressed `raw` digest,
  with a `commonkit` locator only; arrays never are. Memo records live under `plummemo`, always
  compressed. As `crates/store/src/lib.rs`, so either SDK reads what the other wrote.

Handles are the same as the Rust engine's for the same value (`crates/proto/fixtures/handles.json`).
"""

from __future__ import annotations

import base64
import functools
import hashlib
import json
import os
import shutil
import zlib
from typing import Any

import numpy as np
from commonkit import keys as ck_keys
from commonkit.shared import shared_store
from commonkit.store import SharedArrayStore

from . import Blob, List, NodeError
from .audio import AudioValue, peaks_array

TEXT_MEDIA = "text/plain; charset=utf-8"
NPY_MEDIA = "application/x-npy"
JSON_MEDIA = "application/json"

DOMAIN, VERSION, FORM_ARRAY, FORM_RAW = "plum", "v1", "plumarr", "plumraw"
FORM_Z, FORM_MEMO = "plumz", "plummemo"
ROOT_ENV = "COMMONKIT_STORE_ROOT"
# The smallest raw blob worth compressing, and how much is tried first.
COMPRESS_MIN = SAMPLE = 64 << 10
# The largest blob read back (commonkit's `max_array_bytes` default), compressed or not.
MAX_READ = 128 << 20
# The largest array published: a 12 MP RGB float32 picture is 144 MB, past commonkit's default.
MAX_ARRAY = 2 << 30

try:
    SCHEME = ck_keys.register(
        ck_keys.KeyScheme(
            domain=DOMAIN,
            version=VERSION,
            forms=(FORM_ARRAY, FORM_RAW, FORM_Z, FORM_MEMO),
            variants=("c",),
            dtypes={FORM_RAW: "uint8", FORM_Z: "uint8", FORM_MEMO: "uint8"},
        )
    )
except ValueError:  # already registered in this process
    SCHEME = ck_keys.scheme_for(f"{DOMAIN}:{VERSION}:{FORM_RAW}:{'0' * 32}:c")


def default_root() -> str:
    from commonkit.shared import setting

    return setting("ROOT")


def shape_repr(shape: tuple[int, ...]) -> str:
    return repr(tuple(int(n) for n in shape))


try:  # BLAKE3 handles (spec 4.3; B10): listed as `hash.b3` only when the package imports
    import blake3 as _blake3
except ImportError:  # pragma: no cover - blake3 is a dependency; an engine without it stays md5
    _blake3 = None

#: Whether this engine can make and check `b3:` handles.
HAS_B3 = _blake3 is not None


def _hasher(algo: str) -> Any:
    if algo == "b3":
        if _blake3 is None:
            raise NodeError(NodeError.OP, "b3 handles need the blake3 package")
        return _blake3.blake3(max_threads=_blake3.blake3.AUTO)
    return hashlib.md5()


def algo_of(handle: str) -> str:
    """A handle's algorithm, `md5` or `b3`."""
    return "b3" if handle.startswith("b3:") else "md5"


def is_raw(handle: str) -> bool:
    """Whether a handle names bytes as stored (`<algo>:raw:`), not an array."""
    return handle.split(":", 2)[1:2] == ["raw"]


def nd1(a: np.ndarray, algo: str = "md5") -> str:
    """`<algo>:nd1:` of an array: the normalised dtype string, the shape's repr, the C-order bytes."""
    if a.dtype.hasobject or a.dtype.fields is not None:
        raise NodeError(NodeError.OP, f"no handle for dtype {a.dtype}")
    if a.dtype.byteorder == ">":
        a = a.byteswap().view(a.dtype.newbyteorder("<"))
    a = np.ascontiguousarray(a) if a.ndim else a
    h = _hasher(algo)
    h.update(a.dtype.str.encode("ascii"))
    h.update(shape_repr(a.shape).encode("ascii"))
    h.update(a.tobytes(order="C"))
    return f"{algo}:nd1:" + h.hexdigest()


def raw(b: bytes, algo: str = "md5") -> str:
    h = _hasher(algo)
    h.update(b)
    return f"{algo}:raw:" + h.hexdigest()


def matches(handle: str, a: np.ndarray | None = None, b: bytes | None = None) -> bool:
    """Whether an array (`nd1`) or bytes (`raw`) are `handle`, in the handle's own algorithm.
    False, never an error, when they aren't or the algorithm can't be computed here."""
    try:
        if is_raw(handle):
            return raw(b if b is not None else (a.tobytes() if a is not None else b""), algo_of(handle)) == handle
        return a is not None and nd1(a, algo_of(handle)) == handle
    except NodeError:
        return False


def table_shape(v: Any) -> tuple[int, list[dict]]:
    cols = v.get("columns") if isinstance(v, dict) and isinstance(v.get("columns"), list) else []
    first = cols[0].get("values") if cols and isinstance(cols[0], dict) else None
    rows = len(first) if isinstance(first, list) else 0
    return rows, [{"name": c.get("name", ""), "type": c.get("type", "")} for c in cols if isinstance(c, dict)]


def compact(v: Any) -> bytes:
    """JSON as the Rust engine writes it (serde_json's compact form, keys sorted because its maps
    are ordered), so table handles agree. Floats may still differ in spelling: tables of floats
    need the typed canonical form before their handles can be compared across languages."""
    return json.dumps(v, separators=(",", ":"), ensure_ascii=False, sort_keys=True).encode("utf-8")


def line_count(s: str) -> int:
    if not s:
        return 0
    return s.count("\n") + (0 if s.endswith("\n") else 1)


class Plane:
    """What the executor needs to publish values for one run."""

    def __init__(self, root: str | None, shm: bool, max_inline: int, urls: bool = False, algo: str = "md5"):
        """`urls`: an HTTP session without the proof; publish to this engine's store and hand out
        `url` locators the UI fetches by handle (spec 13.2 step 5). `algo`: the handles this run
        makes, `b3` when its request asks for them and this engine can (spec 4.3)."""
        self.algo = "b3" if algo == "b3" and HAS_B3 else "md5"
        self.root = root
        self.shm = shm
        self.urls = urls and not shm
        self.max_inline = max_inline
        self._store = open_store(root) if root else None

    def _publish(self, handle: str, arr: np.ndarray) -> list[dict]:
        if self._store is None or not (self.shm or self.urls):
            raise NodeError("unavailable", f"a {arr.nbytes} byte value needs the shared store, and this session's same-host proof didn't pass")
        key = key_for(handle)
        path = _store_path(self._store, key)
        store = large_store(self.root) if arr.nbytes > MAX_READ else self._store
        if not os.path.isfile(path) and not store.publish(key, arr):
            raise NodeError("unavailable", "the store declined the value")
        if self.urls:
            return [{"kind": "url", "url": blob_url(handle)}]
        return [{"kind": "commonkit", "key": key}, {"kind": "path", "path": path, "offset": _payload_offset(path)}]

    def _publish_raw(self, handle: str, payload: bytes) -> list[dict]:
        """A raw blob: found already (touched), else stored compressed when worth it, else as is."""
        if self._store is None or not (self.shm or self.urls):
            raise NodeError("unavailable", f"a {len(payload)} byte value needs the shared store, and this session's same-host proof didn't pass")
        if not store_raw(self._store, handle, payload):
            raise NodeError("unavailable", "the store declined the value")
        return [{"kind": "url", "url": blob_url(handle)}] if self.urls else (locate(self._store, handle) or [])

    def _blob(self, handle: str, meta: dict, payload: bytes, arr: np.ndarray) -> dict:
        if len(payload) <= self.max_inline:
            return {"handle": handle, "meta": meta, "at": [{"kind": "inline", "b64": base64.b64encode(payload).decode("ascii")}]}
        if is_raw(handle):
            return {"handle": handle, "meta": meta, "at": self._publish_raw(handle, payload)}
        return {"handle": handle, "meta": meta, "at": self._publish(handle, arr)}

    def text_ref(self, s: str) -> dict:
        b = s.encode("utf-8")
        meta = {"media": TEXT_MEDIA, "size": len(b), "form": {"kind": "text", "lines": line_count(s), "encoding": "utf-8"}}
        return self._blob(raw(b, self.algo), meta, b, np.frombuffer(b, dtype="|u1"))

    def array_ref(self, a: np.ndarray) -> dict:
        from .arrays import meta as array_meta

        space, names, table = array_meta(a)
        a = np.ascontiguousarray(np.asarray(a))
        if a.dtype.byteorder == ">":
            a = a.astype(a.dtype.newbyteorder("<"))
        form: dict[str, Any] = {"kind": "array", "dtype": a.dtype.str, "shape": [int(n) for n in a.shape]}
        if space is not None:
            form["space"] = space
        if names:
            form["names"] = list(names)
        if table:
            form["table"] = True
        meta = {"media": NPY_MEDIA, "size": int(a.nbytes), "form": form}
        return self._blob(nd1(a, self.algo), meta, a.tobytes(), a)

    def audio_ref(self, a: Any) -> dict:
        """An `AudioValue` as an `md5:nd1:` blob of its (frames, channels) `<f4` samples, `Form::Audio`."""
        s = a.samples
        meta = {"media": NPY_MEDIA, "size": int(s.nbytes), "form": a.form()}
        return self._blob(nd1(s, self.algo), meta, s.tobytes(), s)

    def peaks(self, a: AudioValue) -> dict | None:
        """A waveform overview of `a` (spec 10.3 `peaks`), always inline, as the Rust SDK's."""
        p = peaks_array(a)
        if p is None:
            return None
        form = {"kind": "array", "dtype": "<f4", "shape": [int(n) for n in p.shape]}
        at = [{"kind": "inline", "b64": base64.b64encode(p.tobytes()).decode("ascii")}]
        ref = {"handle": nd1(p), "meta": {"media": NPY_MEDIA, "size": int(p.nbytes), "form": form}, "at": at}
        return {"kind": "peaks", "ref": ref, "bins": int(p.shape[2]), "channels": a.channels}

    def rgba8(self, t: np.ndarray, role: str = "", caption: str = "") -> dict | None:
        """An (h, w, 4) uint8 array as an `rgba8` rendition."""
        if t.ndim != 3 or t.shape[2] != 4 or t.dtype != np.uint8:
            return None
        try:
            ref = self.array_ref(t)
        except NodeError:
            return None
        r: dict[str, Any] = {"kind": "rgba8", "ref": ref, "w": int(t.shape[1]), "h": int(t.shape[0])}
        if role:
            r["role"] = role
        if caption:
            r["caption"] = caption
        return r

    def table_ref(self, v: Any) -> dict:
        b = compact(v)
        rows, columns = table_shape(v)
        meta = {"media": JSON_MEDIA, "size": len(b), "form": {"kind": "table", "rows": rows, "columns": columns}}
        return self._blob(raw(b, self.algo), meta, b, np.frombuffer(b, dtype="|u1"))

    def json_ref(self, v: Any) -> dict:
        """A large JSON value as a blob."""
        b = compact(v)
        return self._blob(raw(b, self.algo), {"media": JSON_MEDIA, "size": len(b), "form": {"kind": "json"}}, b, np.frombuffer(b, dtype="|u1"))

    def blob_ref(self, b: Blob) -> dict:
        """A `Blob` value: inline bytes when small, else published to the store."""
        return self._blob(b.handle, dict(b.meta), b.data, np.frombuffer(b.data, dtype="|u1"))

    def port_value(self, media: str, v: Any) -> dict:
        """The value a reply carries for a target's output (a list item by item)."""
        if isinstance(v, List):
            return {"kind": "list", "items": [self.port_value(media, x) for x in v.items]}
        try:
            if isinstance(v, Blob):
                return {"kind": "blob", "ref": self.blob_ref(v)}
            if isinstance(v, AudioValue):
                return {"kind": "blob", "ref": self.audio_ref(v)}
            if isinstance(v, np.ndarray):
                return {"kind": "blob", "ref": self.array_ref(v)}
            if isinstance(v, str) and len(v.encode("utf-8")) > self.max_inline:
                return {"kind": "blob", "ref": self.text_ref(v)}
            if media == "table" and len(compact(v)) > self.max_inline:
                return {"kind": "blob", "ref": self.table_ref(v)}
            if media == "tree" and json_len(v) > self.max_inline:
                return {"kind": "blob", "ref": self.json_ref(v)}
        except NodeError as e:
            kinds = ((Blob, "BLOB"), (AudioValue, "AUDIO"), (np.ndarray, "ARRAY"))
            kind = next((k for t, k in kinds if isinstance(v, t)), "TABLE" if media == "table" else "TEXT")
            return {"kind": "opaque", "type": kind, "summary": e.message}
        except (TypeError, ValueError):
            pass  # `compact` met a value that isn't JSON: below
        if not is_json(v):
            # A live object a node made (a model, a handle): it stays in the engine, and the
            # reply says what it is rather than failing to serialise.
            return {"kind": "opaque", "type": type(v).__name__, "summary": f"a {type(v).__name__} the engine keeps to itself"}
        return {"kind": "json", "value": v}


def is_json(v: Any) -> bool:
    """Whether `v` can cross as JSON (a live object a node made can't)."""
    try:
        json.dumps(v)
    except (TypeError, ValueError):
        return False
    return True


@functools.lru_cache(maxsize=16)
def open_store(root: str):
    """One store per root, so its counters add up."""
    return shared_store(mode="readwrite", min_array_bytes=0, root=root)


@functools.cache
def large_store(root: str):
    """The same tree for an array `open_store`'s declines for its size: the same budget and reaper."""
    s = open_store(root)
    return SharedArrayStore(
        root,
        mode="readwrite",
        attach="mmap",
        max_bytes=s.max_bytes,
        min_free_bytes=s.min_free_bytes,
        ttl_s=s.ttl_s,
        min_array_bytes=0,
        max_array_bytes=MAX_ARRAY,
    )


def _counted(store: Any, value: Any) -> Any:
    """Reads here go straight to the file, so they count as commonkit's own reads do."""
    store.stats["shm_hit" if value is not None else "shm_miss"] += 1
    return value


def store_stats(root: str) -> dict:
    """commonkit's counters since the last call, and the tree as its reaper last walked it."""
    store = open_store(root)
    out: dict[str, Any] = {"store": dict(store.drain_stats())}
    used, files = store.usage()
    tree: dict[str, Any] = {"max_bytes": store.max_bytes, "min_free_bytes": store.min_free_bytes}
    if used >= 0:
        tree.update(bytes=used, files=max(files, 0))
    try:
        tree["free_bytes"] = shutil.disk_usage(root).free
    except OSError:
        pass
    out["tree"] = tree
    return out


def key_for(handle: str) -> str:
    return SCHEME.key(handle.rsplit(":", 1)[1], "c", FORM_RAW if is_raw(handle) else FORM_ARRAY)


def z_key_for(handle: str) -> str:
    return SCHEME.key(handle.rsplit(":", 1)[1], "c", FORM_Z)


def memo_key_for(plan_key: Any) -> str | None:
    """The key of a plan key's memo record (`memo:md5:<hex>`); None for anything else."""
    if not isinstance(plan_key, str) or not plan_key.startswith("memo:md5:"):
        return None
    hexd = plan_key[len("memo:md5:") :]
    if len(hexd) != 32 or not all(c in "0123456789abcdef" for c in hexd):
        return None
    return SCHEME.key(hexd, "c", FORM_MEMO)


def compress(b: bytes) -> bytes | None:
    """`b` deflated (zlib format, level 1), or None when it isn't worth it: under `COMPRESS_MIN`,
    over `MAX_READ`, or less than 10% smaller, judged on the first `SAMPLE` bytes first."""
    if len(b) < COMPRESS_MIN or len(b) > MAX_READ:
        return None
    if len(zlib.compress(b[:SAMPLE], 1)) * 10 > SAMPLE * 9:
        return None
    z = zlib.compress(b, 1)
    return z if len(z) * 10 <= len(b) * 9 else None


def inflate(z: bytes) -> bytes | None:
    """A zlib stream inflated, at most `MAX_READ` bytes; anything longer or malformed is None."""
    try:
        d = zlib.decompressobj()
        out = d.decompress(z, MAX_READ + 1)
    except zlib.error:
        return None
    return out if d.eof and len(out) <= MAX_READ else None


def _touch(path: str) -> None:
    try:
        os.utime(path)
    except OSError:
        pass


def _store_path(store: Any, key: str) -> str:
    """Where `store` keeps `key`, in this platform's form. pyuncommonkit 0.1.0 joins the segment's
    relative path with `/` (`keys.relpath`), so on Windows a path locator came out as
    `...\\store\\plumarr/91/...` where Rust's says `...\\plumarr\\91\\...`: the same file, but
    not the same locator. A no-op on Unix."""
    return os.path.normpath(store.path_for(key))


def locate(store: Any, handle: str) -> list[dict] | None:
    """The locators of a value already in `store`, touching it; None when it isn't there."""
    key = key_for(handle)
    path = _store_path(store, key)
    if os.path.isfile(path):
        _touch(path)
        return [{"kind": "commonkit", "key": key}, {"kind": "path", "path": path, "offset": _payload_offset(path)}]
    if is_raw(handle):
        key = z_key_for(handle)
        path = store.path_for(key)
        if os.path.isfile(path):
            _touch(path)
            return [{"kind": "commonkit", "key": key}]
    return None


def has(root: str, handle: str) -> bool:
    """Whether the store at `root` holds `handle`, as it is or compressed."""
    store = open_store(root)
    return os.path.isfile(store.path_for(key_for(handle))) or (is_raw(handle) and os.path.isfile(store.path_for(z_key_for(handle))))


def _segment_bytes(path: str) -> bytes | None:
    try:
        if os.path.getsize(path) > MAX_READ + 4096:
            return None
        return np.load(path, mmap_mode=None, allow_pickle=False).tobytes()
    except (OSError, ValueError):
        return None


def put_record(root: str, plan_key: str, record: bytes) -> bool:
    """Store a memo record (spec 14.5), compressed, replacing one already there."""
    key = memo_key_for(plan_key)
    if key is None:
        return False
    store = open_store(root)
    path = store.path_for(key)
    try:
        os.unlink(path)
    except OSError:
        pass
    return bool(store.publish(key, np.frombuffer(zlib.compress(record, 6), dtype="|u1")))


def read_record(root: str, plan_key: str) -> bytes | None:
    """A memo record, inflated; None when there is none or it is torn. Untrusted input."""
    key = memo_key_for(plan_key)
    if key is None:
        return None
    store = open_store(root)
    z = _counted(store, _segment_bytes(store.path_for(key)))
    return inflate(z) if z is not None else None


def blob_url(handle: str) -> str:
    """Where the HTTP binding serves a blob, relative to the engine's base URL."""
    algo, recipe, hexd = handle.split(":")
    return f"/v1/blobs/{algo}/{recipe}/{hexd}"


def parse_handle(h: Any) -> str | None:
    """A handle string if `h` is a well-formed `md5:{raw,nd1}:<32 hex>` or `b3:{raw,nd1}:<64 hex>`,
    else None."""
    if not isinstance(h, str):
        return None
    parts = h.split(":")
    if len(parts) != 3 or parts[1] not in ("raw", "nd1") or len(parts[2]) != {"md5": 32, "b3": 64}.get(parts[0]):
        return None
    return h if all(c in "0123456789abcdef" for c in parts[2]) else None


def read_blob(root: str, handle: str) -> np.ndarray | None:
    """A blob from the store by handle (a compressed one inflated), checked against the handle;
    None when missing or wrong."""
    store = open_store(root)
    try:
        a = np.load(store.path_for(key_for(handle)), mmap_mode=None, allow_pickle=False)
    except (OSError, ValueError):
        a = None
    if a is None and is_raw(handle):
        z = _segment_bytes(store.path_for(z_key_for(handle)))
        b = inflate(z) if z is not None else None
        a = np.frombuffer(b, dtype="|u1") if b is not None else None
    if a is None:
        return _counted(store, None)
    ok = matches(handle, a)
    return _counted(store, a if ok else None)


def _inside(path: str, roots: list[str]) -> str | None:
    """`path`, resolved, when it is a file under one of `roots`: no symlink out of them."""
    real = os.path.realpath(path)
    for root in roots:
        top = os.path.realpath(root)
        if real.startswith(top + os.sep) and os.path.isfile(real):
            return real
    return None


def read_located(root: str, ref: dict, handle: str, trusted: tuple[str, ...] = ()) -> np.ndarray | None:
    """A blob by its own `path` locators (spec 7.6 rule 10): another engine may publish under keys
    of its own, so its handle finds nothing here. Honoured only under `root` or `trusted`, and
    checked against the handle like any other read."""
    for loc in ref.get("at") or []:
        if not (isinstance(loc, dict) and loc.get("kind") == "path" and isinstance(loc.get("path"), str)):
            continue
        path = _inside(loc["path"], [root, *trusted])
        if path is None:
            continue
        try:
            a = np.load(path, mmap_mode=None, allow_pickle=False)
        except (OSError, ValueError, EOFError):
            continue
        if matches(handle, a):
            return a
    return None


def put_raw(root: str, handle: str, payload: bytes) -> bool:
    """Store an upload that claims to be `handle`, only if its MD5 matches (spec 13.1)."""
    if parse_handle(handle) is None or not is_raw(handle) or not matches(handle, b=payload):
        return False
    return store_raw(open_store(root), handle, payload)


def store_raw(store: Any, handle: str, payload: bytes) -> bool:
    """Put raw bytes (already checked against `handle`) in `store`: found already (touched), else
    compressed when `compress` says it is worth it, else as they are. False when it declines."""
    if locate(store, handle) is not None:
        return True
    z = compress(payload)
    if z is not None and store.publish(z_key_for(handle), np.frombuffer(z, dtype="|u1")):
        return True
    return bool(store.publish(key_for(handle), np.frombuffer(payload, dtype="|u1")))


def _payload_offset(path: str) -> int:
    """Where a `.npy` file's payload starts (version 1.0: 10 bytes plus the header length)."""
    try:
        with open(path, "rb") as f:
            head = f.read(10)
        return 10 + int.from_bytes(head[8:10], "little")
    except OSError:
        return 0


def json_len(v: Any) -> int:
    try:
        return len(compact(v))
    except (TypeError, ValueError):
        return 0
