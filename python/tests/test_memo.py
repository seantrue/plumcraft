"""Memo keys: the shared vectors (also checked by `crates/proto/src/memo.rs`), the Python-only
inputs (tuples, numpy-style scalars, values with no encoding), and `memo.scope: none`."""

import json
import pathlib
import struct
import time

import pytest
from commonkit import digest as ck_digest

from plumcraft_engine import simple
from plumcraft_engine.host import Host
from plumcraft_engine.memo import FLOAT, INT, Uncanonical, canon, canon_float, coerce, plan_key
from plumcraft_text import engine

VECTORS = json.loads((pathlib.Path(__file__).parents[2] / "crates/proto/fixtures/memo_canon.json").read_text())


@pytest.mark.parametrize("case", VECTORS["canon"], ids=lambda c: c["note"])
def test_canon_vectors(case):
    if "float_bits" in case:
        f = struct.unpack(">d", bytes.fromhex(case["float_bits"][2:]))[0]
        assert canon_float(f) == case["canon"]
    else:
        assert canon(coerce(case["in"], {"INT": INT, "FLOAT": FLOAT}.get(case["port"]))) == case["canon"]


@pytest.mark.parametrize("case", VECTORS["keys"], ids=lambda c: c["note"])
def test_key_vectors(case):
    # the process's algorithm picks the column (the suite runs once per algorithm)
    want = case["key"] if ck_digest.current() == "md5" else case["key_b3"]
    assert plan_key(case["kind"], case["version"], case["salt"], case["device_class"], case["params"], case["inputs"]) == want


class _Scalar:
    """Stands in for a numpy scalar: anything with `.item()` keys as its Python value."""

    def __init__(self, v):
        self.v = v

    def item(self):
        return self.v


def test_python_only_inputs():
    assert canon((1, (2.0, "x"))) == canon([1, [2.0, "x"]])
    assert canon(_Scalar(3)) == canon(3) and canon(_Scalar(0.5)) == canon(0.5)
    assert canon(float("nan")) == '{"f":"0x7ff8000000000000"}'
    assert canon(True) != canon(1), "a bool is not an int"
    with pytest.raises(Uncanonical):
        canon(object())
    with pytest.raises(Uncanonical):
        canon({1: "x"})


def test_scope_none_is_never_cached_nor_anything_below_it():
    clock = simple(
        {"id": "test.clock", "inputs": [], "outputs": [{"name": "text", "type": "TEXT"}], "memo": {"scope": "none"}},
        lambda _: {"text": repr(time.monotonic_ns())},
    )
    h = Host(lambda: engine().with_node(clock))
    h.wait_ready(5)
    g = {
        "nodes": {"t": {"kind": "test.clock"}, "u": {"kind": "text.upper"}},
        "wires": [{"from": {"node": "t", "port": "text"}, "to": {"node": "u", "port": "text"}}],
    }
    for rid in ("c1", "c2"):
        h.run({"run_id": rid, "generation": 1, "graph": {"kind": "inline", "graph": g}})
        deadline = time.monotonic() + 5
        while (snap := h.get(rid))["reply"] is None and time.monotonic() < deadline:
            time.sleep(0.01)
        assert {k: n["state"] for k, n in snap["reply"]["nodes"].items()} == {"t": "done", "u": "done"}
    h.shutdown()
