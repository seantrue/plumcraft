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

from plumcraft_engine import Engine, ProtoError, arrays, views
from plumcraft_engine.data import nd1
from plumcraft_engine.host import Host

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


class _Plain(Engine):
    id = "plain"


def _inline_ref(a: np.ndarray) -> dict:
    a = np.ascontiguousarray(a)
    meta = {"media": "application/x-npy", "size": int(a.nbytes), "form": {"kind": "array", "dtype": a.dtype.str, "shape": list(a.shape)}}
    return {"handle": nd1(a), "meta": meta, "at": [{"kind": "inline", "b64": base64.b64encode(a.tobytes()).decode()}]}


def test_the_sdk_answers_view_derive_under_its_capability():
    """FR6: a 3-D array's histogram over all its values (the maintainer's instance), from the
    host, the same as the Rust SDK's; junk is refused with a reason."""
    h = Host(_Plain)
    try:
        assert h.wait_ready(5)["state"] == "connected"
        assert "view.derive" in h.dispatch("hello", {"v": 2, "op": "hello", "caps": []})["caps"]
        r = _inline_ref(np.arange(24, dtype="<f4").reshape(2, 3, 4))
        got = h.dispatch("view.derive", {"ref": r, "view": {"kind": "histogram", "params": {"bins": 4}}})
        assert got["table"]["columns"][1]["values"] == [6, 6, 6, 6]
        assert got["meta"]["nan"] == 0
        for junk in (
            None,
            {"ref": r, "view": {"kind": "nope"}},
            {"ref": 7, "view": {"kind": "stats"}},
            {"ref": r, "view": {"kind": "stats"}, "extra": 1},
            {"ref": {"meta": {"media": "x"}}, "view": {"kind": "stats"}},
        ):
            with pytest.raises(ProtoError):
                h.dispatch("view.derive", junk)
        with pytest.raises(ProtoError):
            h.dispatch("view.derive", {"ref": r, "view": {"kind": "histogram"}, "max_bytes": 10})
    finally:
        h.shutdown()


@pytest.mark.parametrize("case", DOC["cases"], ids=[c["name"] for c in DOC["cases"]])
def test_every_case_through_the_hosts_op(case):
    """FR6 slice 3: the engine conformance of `view.derive`: each shared vector asked of the SDK's
    op, its value an inline reference, answers as the reduction does (a refusal is a ProtoError)."""
    i = case["input"]
    data = base64.b64decode(i["b64"])
    want = case["expect"]
    a = np.frombuffer(data, dtype=np.dtype(i["dtype"])) if i["dtype"] else None
    try:
        arr = a.reshape(i["shape"]) if a is not None else None
    except (ValueError, TypeError):
        arr = None
    if arr is None or not data:
        pytest.skip("not an array a reference can carry")
    ref = _inline_ref(arr)
    req = {"ref": ref, "view": case["view"]}
    if want.get("refused"):
        with pytest.raises(ProtoError):
            views.derive_op(None, req)
        return
    got = views.derive_op(None, req)
    if "array" in want:
        assert got["array"]["dtype"] == want["array"]["dtype"]
        assert got["array"]["shape"] == want["array"]["shape"]
        assert got["b64"] == want["array"]["b64"]
        return
    cols = got["table"]["columns"]
    assert [c["name"] for c in cols] == want["order"]
    for c in cols:
        for k, (x, y) in enumerate(zip(c["values"], want["columns"][c["name"]], strict=True)):
            _close(x, y, f"{c['name']}[{k}]")
