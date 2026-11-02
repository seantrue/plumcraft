"""Bindings (`RunRequest.inputs`, spec 7.6 rule 10), as `crates/engine/tests/bind.rs`: one
engine's outputs, bound into a second engine's run over the same store, arrive as the same values
(json, text, array with its space, encoded bytes, lists), inline or in the store; a binding
replaces a wire and what fed it isn't run; the memo key is the content; keys that name nothing are
ignored; missing, handle-less, opaque and over-deep values are node errors, never a crash."""

import base64
import pathlib

import numpy as np
import scratch

from plumcraft_engine import ArrayValue, AudioValue, Blob, CancelToken, Engine, List, Node, NodeError, bind, data, exec

KINDS = ["json", "text", "array", "blob", "list"]


class Make(Node):
    def __init__(self):
        self.definition = {"id": "t.make", "inputs": [{"name": "n", "type": "INT", "default": 1}], "outputs": [{"name": k, "type": "ANY"} for k in KINDS]}
        self.runs = 0

    def run(self, inputs, cx):
        self.runs += 1
        n = inputs["n"]
        svg = (f"<svg n='{n}'/>" * 50).encode()
        meta = {"media": "image/svg+xml", "size": len(svg), "form": {"kind": "vector", "width": 1.0, "height": 1.0}}
        return {
            "json": {"n": n, "xs": [1, 2.5]},
            "text": f"line {n}\n" * 40,
            "array": ArrayValue(np.arange(12, dtype="<f4").reshape(2, 2, 3) + n, "Lab", ["L", "a", "b"]),
            "blob": Blob(handle=data.raw(svg), meta=meta, data=svg),
            "list": List([n, "two"]),
        }


class Echo(Node):
    def __init__(self):
        self.definition = {"id": "t.echo", "inputs": [{"name": "x", "type": "ANY", "default": "widget"}], "outputs": [{"name": "x", "type": "ANY"}]}
        self.runs = 0
        self.got = None

    def run(self, inputs, cx):
        self.runs += 1
        self.got = inputs["x"]
        return {"x": inputs["x"]}


class Rig:
    def __init__(self, root):
        self.root = root
        self.make, self.echo = Make(), Echo()
        self.engine = Engine({"id": "t", "salt": "s"}).with_store(root).with_wire_type({"id": "ANY", "media": "tree"}).with_node(self.make).with_node(self.echo)
        self.memo = exec.Memo()

    def run(self, graph, inputs, targets, max_inline):
        req = {"run_id": "r", "generation": 1, "graph": {"kind": "inline", "graph": graph}, "targets": targets, "inputs": inputs}
        return exec.run(self.engine, req, lambda _: None, CancelToken(), "test", self.memo, data.Plane(self.root, True, max_inline))

    def made(self, n, max_inline):
        r = self.run({"version": 1, "nodes": {"m": {"kind": "t.make", "values": {"n": n}}}, "wires": []}, {}, ["m"], max_inline)
        assert r["ok"], r
        return r["nodes"]["m"]["outputs"]


ECHO = {"version": 1, "nodes": {"e": {"kind": "t.echo"}}, "wires": []}


def root():
    return scratch.mkdtemp("bind-")


def test_every_kind_crosses_unchanged_inline_or_in_the_store():
    r = root()
    for max_inline in (0, 1 << 20):
        outs = Rig(r).made(3, max_inline)
        b = Rig(r)
        for kind in KINDS:
            got = b.run(ECHO, {"e.x": outs[kind]}, ["e"], max_inline)
            assert got["ok"], (kind, got)
            assert got["nodes"]["e"]["outputs"]["x"] == outs[kind], f"{kind} (inline up to {max_inline}) arrives as it left"
    outs = Rig(r).made(3, 0)
    for kind in ("text", "array", "blob"):
        assert outs[kind]["kind"] == "blob", kind


def test_an_array_keeps_its_space_and_names():
    r = root()
    v = Rig(r).made(1, 0)["array"]
    b = Rig(r)
    b.run(ECHO, {"e.x": v}, ["e"], 0)
    got = b.echo.got
    assert isinstance(got, ArrayValue) and got.space == "Lab" and tuple(got.names) == ("L", "a", "b") and got.shape == (2, 2, 3)


def test_a_binding_replaces_the_wire_and_its_source_is_not_run():
    a = Rig(root())
    wire = {"from": {"node": "m", "port": "json"}, "to": {"node": "e", "port": "x"}}
    graph = {"version": 1, "nodes": {"m": {"kind": "t.make"}, "e": {"kind": "t.echo"}}, "wires": [wire]}
    v = {"kind": "json", "value": "bound"}
    got = a.run(graph, {"e.x": v}, ["e"], 1 << 20)
    assert got["ok"], got
    assert got["nodes"]["e"]["outputs"]["x"] == v
    assert got["order"] == ["e"] and a.make.runs == 0


def test_the_memo_key_is_the_content():
    r = root()
    a = Rig(r)
    in_store, inline = a.made(5, 0)["array"], a.made(5, 1 << 20)["array"]
    assert in_store != inline
    b = Rig(r)
    assert b.run(ECHO, {"e.x": in_store}, ["e"], 0)["nodes"]["e"]["state"] == "done"
    assert b.run(ECHO, {"e.x": inline}, ["e"], 0)["nodes"]["e"]["state"] == "cached", "same bytes: a hit"
    assert b.run(ECHO, {"e.x": a.made(6, 0)["array"]}, ["e"], 0)["nodes"]["e"]["state"] == "done"
    j = {"kind": "json", "value": "widget"}
    assert b.run(ECHO, {}, ["e"], 0)["nodes"]["e"]["state"] == "done"
    assert b.run(ECHO, {"e.x": j}, ["e"], 0)["nodes"]["e"]["state"] == "done", "bound, not the widget's key"
    assert b.run(ECHO, {"e.x": j}, ["e"], 0)["nodes"]["e"]["state"] == "cached"
    assert b.echo.runs == 4


def test_rust_audio_arrives_as_its_samples():
    """The Rust SDK's audio DataRef (inline here): an `AudioValue`, its (frames, channels) float32
    samples at its sample rate (the core AUDIO type)."""
    samples = np.array([[0.5, -0.5], [1.0, 0.0]], dtype="<f4")
    form = {"kind": "audio", "sample_rate": 8000, "channels": 2, "frames": 2, "dtype": "<f4"}
    inline = [{"kind": "inline", "b64": base64.b64encode(samples.tobytes()).decode()}]
    ref = {"handle": data.nd1(samples), "meta": {"media": "application/x-npy", "size": 16, "form": form}, "at": inline}
    b = Rig(root())
    assert b.run(ECHO, {"e.x": {"kind": "blob", "ref": ref}}, ["e"], 1 << 20)["ok"]
    assert isinstance(b.echo.got, AudioValue) and b.echo.got.sample_rate == 8000
    assert np.array_equal(b.echo.got.samples, samples)


def test_keys_naming_nothing_are_ignored():
    a = Rig(root())
    v = {"kind": "json", "value": 1}
    got = a.run(ECHO, {"zz.x": v, "e.nope": v, "nodot": v, ".x": v, "e.": v, "e.x ": 5, 7: v}, ["e"], 1 << 20)
    assert got["ok"], got
    assert got["nodes"]["e"]["outputs"]["x"] == {"kind": "json", "value": "widget"}
    assert a.run(ECHO, ["not", "a", "map"], ["e"], 1 << 20)["ok"]


def test_bad_bindings_are_node_errors():
    a = Rig(root())
    form = {"kind": "array", "dtype": "<f4", "shape": [1]}
    gone = {"kind": "blob", "ref": {"handle": data.raw(b"never stored"), "meta": {"form": form}, "at": []}}
    no_handle = {"kind": "blob", "ref": {"meta": {"form": form}, "at": []}}
    lying = {"kind": "blob", "ref": {"handle": data.raw(b"one"), "meta": {}, "at": [{"kind": "inline", "b64": base64.b64encode(b"two").decode()}]}}
    opaque = {"kind": "opaque", "type": "STYLE", "summary": "s"}
    deep = {"kind": "json", "value": 0}
    for _ in range(20):
        deep = {"kind": "list", "items": [deep]}
    for v, kind in ((gone, "missing_source"), (no_handle, "config"), (lying, "missing_source"), (opaque, "config"), (deep, "config")):
        got = a.run(ECHO, {"e.x": v}, ["e"], 1 << 20)["nodes"]["e"]
        assert got["state"] == "error" and got["error"]["kind"] == kind, (v, got)


def located(a, where):
    """A DataRef to `a`, in a file at `where`, as another engine would publish it."""
    where.parent.mkdir(parents=True, exist_ok=True)
    np.save(where, a)
    return {"kind": "blob", "ref": {"handle": data.nd1(a), "meta": {"form": {"kind": "array"}}, "at": [{"kind": "path", "path": str(where), "offset": 0}]}}


def test_a_blob_another_engine_keyed_its_own_way_is_read_by_its_path():
    root = pathlib.Path(scratch.mkdtemp("bind-"))
    a = np.arange(12, dtype="<f4").reshape(2, 2, 3)
    # under a form of its own: its handle finds nothing here, as with PlumImpress's `impcarr`
    ref = located(a, root / "impcarr" / "ab" / "x.npy")
    assert data.read_blob(str(root), ref["ref"]["handle"]) is None
    got = bind.to_value(str(root), "array", ref)
    assert np.array_equal(got, a)


def test_a_path_is_honoured_only_inside_the_store_and_only_with_the_right_bytes():
    root, away = pathlib.Path(scratch.mkdtemp("bind-")), pathlib.Path(scratch.mkdtemp("bind-"))
    a = np.arange(6, dtype="<f4").reshape(2, 3)

    def refused(ref):
        try:
            bind.to_value(str(root), "array", ref)
        except NodeError as e:
            assert e.kind == NodeError.MISSING_SOURCE, e
            return True
        return False

    outside = located(a, away / "outside.npy")
    assert refused(outside), "outside the store"
    assert np.array_equal(bind.to_value(str(root), "array", outside, (str(away),)), a), "unless the run trusts it"
    link = root / "link"
    link.symlink_to(away)
    assert refused({"kind": "blob", "ref": located(a, away / "x.npy")["ref"] | {"at": [{"kind": "path", "path": str(link / "x.npy")}]}}), "a link out of it"
    wrong = located(a, root / "wrong.npy")
    np.save(root / "wrong.npy", a + 1)
    assert refused(wrong), "bytes that aren't the handle's"
    (root / "torn.npy").write_bytes(b"not an npy")
    torn = {"kind": "blob", "ref": wrong["ref"] | {"at": [{"kind": "path", "path": str(root / "torn.npy")}]}}
    assert refused(torn), "a torn file"
    pickled = root / "p.npy"
    np.save(pickled, np.array([{"a": 1}], dtype=object), allow_pickle=True)
    assert refused({"kind": "blob", "ref": wrong["ref"] | {"at": [{"kind": "path", "path": str(pickled)}]}}), "never unpickled"


def test_an_array_past_the_read_limit_publishes_and_reads_back():
    root = scratch.mkdtemp("bind-")
    a = np.zeros(data.MAX_READ // 4 + 1024, dtype="<f4")
    a[7] = 1.5
    plane = data.Plane(root, True, 1024)
    ref = plane.array_ref(ArrayValue(a))
    assert data.read_blob(root, ref["handle"]) is not None
    assert np.array_equal(bind.to_value(root, "array", {"kind": "blob", "ref": ref}), a)
