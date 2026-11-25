"""FR6 (research note 30): the Python SDK's derived views against the shared vectors
(`crates/proto/fixtures/views.json`), which the Rust crate `plumcraft-views` runs too
(`crates/views/tests/vectors.rs`), so the two implementations agree; and both SDKs' per-channel
display stats (`stats` in a Display) held to the same file's `sdk_stats`."""

import base64
import json
import math
import pathlib

import numpy as np
import pytest

from plumcraft_engine import arrays, views

FIXTURE = pathlib.Path(__file__).resolve().parents[2] / "crates" / "proto" / "fixtures" / "views.json"
DOC = json.loads(FIXTURE.read_text())
TOL = DOC["tolerance"]


def _close(x, y, at):
    if x is None or y is None:
        assert x is None and y is None, at
    elif isinstance(x, int) and isinstance(y, int) and not isinstance(x, bool):
        assert x == y, at
    else:
        assert x == y or abs(x - y) <= TOL * max(abs(x), abs(y), 1e-300), f"{at}: {x} against {y}"


@pytest.mark.parametrize("case", DOC["cases"], ids=[c["name"] for c in DOC["cases"]])
def test_every_case_agrees_with_the_reference(case):
    i = case["input"]
    data = base64.b64decode(i["b64"])
    want = case["expect"]
    if want.get("refused"):
        with pytest.raises(views.ViewError):
            views.derive(case["view"], i["dtype"], i["shape"], data)
        return
    got = views.derive(case["view"], i["dtype"], i["shape"], data)
    if "array" in want:
        assert got["array"]["dtype"] == want["array"]["dtype"]
        assert got["array"]["shape"] == want["array"]["shape"]
        assert base64.b64encode(got["bytes"]).decode() == want["array"]["b64"]
        return
    cols = got["table"]["columns"]
    assert [c["name"] for c in cols] == want["order"]
    for c in cols:
        w = want["columns"][c["name"]]
        assert len(c["values"]) == len(w), c["name"]
        for k, (x, y) in enumerate(zip(c["values"], w, strict=True)):
            _close(x, y, f"{c['name']}[{k}]")
    for k, w in want["meta"].items():
        _close(got["meta"][k], w, f"meta {k}")


def test_junk_never_raises_anything_but_a_view_error():
    for v in [
        None,
        1,
        "x",
        {"kind": 3},
        {"kind": "histogram", "params": []},
        {"kind": "histogram", "params": {"bins": True}},
        {"kind": "histogram", "params": {"lo": float("nan")}},
        {"kind": "slice", "params": {"axes": [0, "a"]}},
    ]:
        with pytest.raises(views.ViewError):
            views.derive(v, "<f4", [1], b"\0\0\0\0")
    for dtype, shape, data in [("<f4", [2], b"\0" * 7), ("<U4", [1], b"\0" * 16), ("<f4", [], b""), ("<f4", [-1], b"")]:
        with pytest.raises(views.ViewError):
            views.derive({"kind": "stats"}, dtype, shape, data)


@pytest.mark.parametrize("case", DOC["sdk_stats"], ids=[c["name"] for c in DOC["sdk_stats"]])
def test_the_display_stats_agree_with_the_rust_sdk(case):
    i = case["input"]
    a = np.frombuffer(base64.b64decode(i["b64"]), dtype=np.dtype(i["dtype"])).reshape(i["shape"])
    got = arrays.stats(a)["channels"]
    assert len(got) == len(case["channels"])
    for g, w in zip(got, case["channels"], strict=True):
        assert math.isclose(g["min"], w["min"], rel_tol=TOL) and math.isclose(g["max"], w["max"], rel_tol=TOL)
        assert g.get("nan", 0) == w["nan"]
        assert g["histogram"]["counts"] == w["counts"]
        assert math.isclose(g["histogram"]["hi"], w["hi"], rel_tol=TOL)
