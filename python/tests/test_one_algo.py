"""One algorithm per process (#91, #92; spec 4.3): every handle, segment, `.sources/` name, memo
record and plan key a run leaves is in commonkit's current algorithm (the tree's `HASH`, then
`COMMONKIT_HASH`, then `b3`), and the hello says exactly that algorithm. The suite runs once per
algorithm, each from an empty store (the maintainer: "Any appearance of unintended hashes is a
bug")."""

import base64
import json
import os
import re
from pathlib import Path

import numpy as np
from commonkit import digest as ck_digest
from commonkit import scan
from commonkit.testing import assert_isolated

from plumcraft_engine import Engine, data, memo

VECTORS = json.loads((Path(__file__).parents[2] / "crates/proto/fixtures/handles.json").read_text())["vectors"]
ALGO = ck_digest.current()
OTHER = "b3" if ALGO == "md5" else "md5"
HEX = {"md5": 32, "b3": 64}


def test_hash_is_what_was_asked():
    assert_isolated()
    asked = os.environ.get("COMMONKIT_HASH")
    assert asked is not None, "COMMONKIT_HASH is not set"
    assert ALGO == asked.strip().lower()


def test_the_shared_vectors_in_this_algorithm():
    field = "handle" if ALGO == "md5" else "b3"
    for v in VECTORS:
        b = base64.b64decode(v["bytes_b64"])
        a = None
        if v["recipe"] == "raw":
            got = data.raw(b)
        else:
            a = np.frombuffer(b, dtype=np.dtype(v["dtype"])).reshape(v["shape"]) if b or v["shape"] else np.zeros(v["shape"], v["dtype"])
            got = data.nd1(a)
        assert got == v[field], v["name"]
        assert data.matches(v[field], a=a, b=b if v["recipe"] == "raw" else None)
        # the other algorithm's handle of the same bytes never matches here
        other = v["b3" if field == "handle" else "handle"]
        assert not data.matches(other, a=a, b=b if v["recipe"] == "raw" else None), v["name"]


def test_handles_parse_and_key_in_this_algorithm():
    a = np.arange(12, dtype="<f4").reshape(3, 4)
    h = data.nd1(a)
    assert h.startswith(f"{ALGO}:nd1:") and len(h) == len(f"{ALGO}:nd1:") + HEX[ALGO]
    assert data.parse_handle(h) == h
    for bad in ["b3:nd1:" + "0" * 32, "md5:nd1:" + "0" * 64, "sha1:raw:" + "0" * 40, "b3:raw:" + "G" * 64]:
        assert data.parse_handle(bad) is None, bad
    assert data.matches(h, a) and not data.matches(h, a + 1)
    assert data.key_for(h).split(":")[2] == data.FORM_ARRAY and data.key_for(data.raw(b"x")).split(":")[2] == data.FORM_RAW
    assert memo.plan_key("k", 1, "s", None, {}, {}).startswith(f"memo:{ALGO}:")


def test_a_plane_makes_this_algorithms_handles(tmp_path):
    big = np.arange(1 << 16, dtype="<f4")
    plane = data.Plane(str(tmp_path), True, 1024)
    ref = plane.array_ref(big)
    assert ref["handle"].startswith(f"{ALGO}:nd1:")
    assert data.matches(ref["handle"], data.read_blob(str(tmp_path), ref["handle"]))


def test_the_hello_says_exactly_this_algorithm():
    e = Engine("t")
    r = e.hello({"op": "hello", "caps": ["hash.b3"]}, "s")
    assert "hash.b3" not in r["caps"], "retired: the algorithm isn't negotiated"
    assert r["data"]["algos"] == [ALGO]


def foreign(root: str) -> list[str]:
    """Everything under `root` hashed in the other algorithm: segments (commonkit's scan of the
    `plum*` forms), `.sources/<digest>/` names, and handles and plan keys inside memo records."""
    found = [f"{p}: {why}" for p, why in scan.foreign_digests(root, [data.FORM_ARRAY, data.FORM_RAW, data.FORM_Z, data.FORM_MEMO], ALGO)]
    src = os.path.join(root, ".sources")
    for name in os.listdir(src) if os.path.isdir(src) else []:
        if re.fullmatch(r"[0-9a-f]+", name) and len(name) != HEX[ALGO]:
            found.append(f".sources/{name}")
    memo_dir = os.path.join(root, data.FORM_MEMO)
    for _dirpath, _, files in os.walk(memo_dir):
        for f in files:
            hexd = f.split(".", 1)[0]
            rec = data.read_record(root, f"memo:{ALGO}:{hexd}")
            text = rec.decode("utf-8", "replace") if rec else ""
            found += [f"{f}: {m}" for m in re.findall(rf'"(?:memo:)?{OTHER}:[a-z0-9:]+', text)]
    return found


def test_a_run_leaves_only_this_algorithm(tmp_path):
    """A run through the Python text engine from an empty store: its records, segments and the
    handles it hands out are all in this algorithm."""
    from plumcraft_engine import CancelToken
    from plumcraft_engine.exec import Memo, run
    from plumcraft_text import TextEngine

    root = str(tmp_path / "store")
    os.makedirs(root)
    e = TextEngine()
    e.store_root = root
    long = "the corgi on the floral carpet " * 8_000
    graph = {"version": 1, "nodes": {"a": {"kind": "text.literal", "values": {"text": long}, "cache": "always"},
                                     "b": {"kind": "text.upper", "cache": "always"}},
             "wires": [{"from": {"node": "a", "port": "text"}, "to": {"node": "b", "port": "text"}}]}
    req = {"v": 2, "op": "run", "run_id": "one-algo", "generation": 1, "graph": {"kind": "inline", "graph": graph}, "targets": ["b"],
           "algo": OTHER}  # an old client's field: ignored
    reply = run(e, req, lambda *_: None, CancelToken(), "test", Memo(), data.Plane(root, True, 1024))
    handles = re.findall(r'"handle": "([^"]+)"', json.dumps(reply))
    assert handles and all(h.startswith(f"{ALGO}:") for h in handles), handles
    assert foreign(root) == []
