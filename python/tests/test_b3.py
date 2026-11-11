"""BLAKE3 handles (B10, spec 4.3): the shared vectors' `b3` handles, checking in a handle's own
algorithm, a run that asks for `b3`, and the hello's `hash.b3`."""

import base64
import json
from pathlib import Path

import numpy as np

from plumcraft_engine import Engine, data

VECTORS = json.loads((Path(__file__).parents[2] / "crates/proto/fixtures/handles.json").read_text())["vectors"]


def test_the_shared_vectors_b3_handles():
    assert data.HAS_B3
    for v in VECTORS:
        b = base64.b64decode(v["bytes_b64"])
        if v["recipe"] == "raw":
            got = data.raw(b, "b3")
        else:
            a = np.frombuffer(b, dtype=np.dtype(v["dtype"])).reshape(v["shape"]) if b or v["shape"] else np.zeros(v["shape"], v["dtype"])
            got = data.nd1(a, "b3")
        assert got == v["b3"], v["name"]
        assert data.matches(v["b3"], a=None if v["recipe"] == "raw" else a, b=b if v["recipe"] == "raw" else None)
        assert data.matches(v["handle"], a=None if v["recipe"] == "raw" else a, b=b if v["recipe"] == "raw" else None)


def test_handles_parse_and_check_in_their_own_algorithm():
    a = np.arange(12, dtype="<f4").reshape(3, 4)
    m, b = data.nd1(a), data.nd1(a, "b3")
    assert m.startswith("md5:nd1:") and b.startswith("b3:nd1:") and len(b) == len("b3:nd1:") + 64
    assert data.parse_handle(m) == m and data.parse_handle(b) == b
    for bad in ["b3:nd1:" + "0" * 32, "md5:nd1:" + "0" * 64, "sha1:raw:" + "0" * 40, "b3:raw:" + "G" * 64]:
        assert data.parse_handle(bad) is None, bad
    assert data.matches(b, a) and not data.matches(b, a + 1)
    assert data.key_for(b).split(":")[2] == data.FORM_ARRAY and data.key_for(data.raw(b"x", "b3")).split(":")[2] == data.FORM_RAW


def test_a_plane_makes_the_handles_its_run_asks_for(tmp_path):
    big = np.arange(1 << 16, dtype="<f4")
    for algo, prefix in [("b3", "b3:nd1:"), ("md5", "md5:nd1:"), ("sha1", "md5:nd1:")]:
        plane = data.Plane(str(tmp_path), True, 1024, algo=algo)
        ref = plane.array_ref(big)
        assert ref["handle"].startswith(prefix), algo
        assert data.matches(ref["handle"], data.read_blob(str(tmp_path), ref["handle"]))


def test_the_hello_lists_hash_b3_and_prefers_b3():
    e = Engine("t")
    r = e.hello({"op": "hello", "caps": ["hash.b3"]}, "s")
    assert "hash.b3" in r["caps"] and r["data"]["algos"] == ["b3", "md5"]
    assert "hash.b3" not in e.hello({"op": "hello", "caps": []}, "s")["caps"]
